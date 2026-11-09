use notify::EventKind;
use parking_lot::Mutex;
use std::{
    collections::{BTreeMap, HashMap},
    ops::DerefMut,
    path::Path,
    sync::{Arc, OnceLock},
};
use util::{ResultExt, paths::SanitizedPath};

use crate::{PathEvent, PathEventKind, Watcher};

pub struct FsWatcher {
    tx: smol::channel::Sender<()>,
    pending_path_events: Arc<Mutex<Vec<PathEvent>>>,
    registrations: Mutex<BTreeMap<Arc<std::path::Path>, WatcherRegistrationId>>,
}

impl FsWatcher {
    pub fn new(
        tx: smol::channel::Sender<()>,
        pending_path_events: Arc<Mutex<Vec<PathEvent>>>,
    ) -> Self {
        Self {
            tx,
            pending_path_events,
            registrations: Default::default(),
        }
    }
}

impl Drop for FsWatcher {
    fn drop(&mut self) {
        let mut registrations = BTreeMap::new();
        {
            let old = &mut self.registrations.lock();
            std::mem::swap(old.deref_mut(), &mut registrations);
        }

        let _ = global(|g| {
            for (_, registration) in registrations {
                g.remove(registration);
            }
        });
    }
}

impl Watcher for FsWatcher {
    fn add(&self, path: &std::path::Path) -> anyhow::Result<()> {
        log::trace!("watcher add: {path:?}");
        let tx = self.tx.clone();
        let pending_paths = self.pending_path_events.clone();

        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            // Return early if an ancestor of this path was already being watched.
            // saves a huge amount of memory
            if let Some((watched_path, _)) = self
                .registrations
                .lock()
                .range::<std::path::Path, _>((
                    std::ops::Bound::Unbounded,
                    std::ops::Bound::Included(path),
                ))
                .next_back()
                && path.starts_with(watched_path.as_ref())
            {
                log::trace!(
                    "path to watch is covered by existing registration: {path:?}, {watched_path:?}"
                );
                return Ok(());
            }
        }
        #[cfg(target_os = "linux")]
        {
            if self.registrations.lock().contains_key(path) {
                log::trace!("path to watch is already watched: {path:?}");
                return Ok(());
            }
        }

        let root_path = SanitizedPath::new_arc(path);
        let path: Arc<std::path::Path> = path.into();

        #[cfg(any(target_os = "windows", target_os = "macos"))]
        let mode = notify::RecursiveMode::Recursive;
        #[cfg(target_os = "linux")]
        let mode = notify::RecursiveMode::NonRecursive;

        let registration_path = path.clone();
        let registration_id = global({
            let watch_path = path.clone();
            let callback_path = path;
            |g| {
                g.add(watch_path, mode, move |event: &notify::Event| {
                    log::trace!("watcher received event: {event:?}");
                    let kind = match event.kind {
                        EventKind::Create(_) => Some(PathEventKind::Created),
                        EventKind::Modify(_) => Some(PathEventKind::Changed),
                        EventKind::Remove(_) => Some(PathEventKind::Removed),
                        _ => None,
                    };
                    let mut path_events = event
                        .paths
                        .iter()
                        .filter_map(|event_path| {
                            let event_path = SanitizedPath::new(event_path);
                            event_path.starts_with(&root_path).then(|| PathEvent {
                                path: event_path.as_path().to_path_buf(),
                                kind,
                            })
                        })
                        .collect::<Vec<_>>();

                    let is_rescan_event = event.need_rescan();
                    if is_rescan_event {
                        // `handle_event` warns once per lost sync; every watched
                        // directory sees it, so a warning here filled the log.
                        log::trace!(
                            "filesystem watcher lost sync for {callback_path:?}; scheduling rescan"
                        );
                        // we only keep the first event per path below, this ensures it will be the rescan event
                        // we'll remove any existing pending events for the same reason once we have the lock below
                        path_events.retain(|p| &p.path != callback_path.as_ref());
                        path_events.push(PathEvent {
                            path: callback_path.to_path_buf(),
                            kind: Some(PathEventKind::Rescan),
                        });
                    }

                    if !path_events.is_empty() {
                        path_events.sort();
                        let mut pending_paths = pending_paths.lock();
                        if pending_paths.is_empty() {
                            tx.try_send(()).ok();
                        }
                        coalesce_pending_rescans(&mut pending_paths, &mut path_events);
                        util::extend_sorted(
                            &mut *pending_paths,
                            path_events,
                            usize::MAX,
                            |a, b| a.path.cmp(&b.path),
                        );
                    }
                })
            }
        })??;

        self.registrations
            .lock()
            .insert(registration_path, registration_id);

        Ok(())
    }

    fn remove(&self, path: &std::path::Path) -> anyhow::Result<()> {
        log::trace!("remove watched path: {path:?}");
        let Some(registration) = self.registrations.lock().remove(path) else {
            return Ok(());
        };

        global(|w| w.remove(registration))
    }
}

fn coalesce_pending_rescans(pending_paths: &mut Vec<PathEvent>, path_events: &mut Vec<PathEvent>) {
    if !path_events
        .iter()
        .any(|event| event.kind == Some(PathEventKind::Rescan))
    {
        return;
    }

    let mut new_rescan_paths: Vec<std::path::PathBuf> = path_events
        .iter()
        .filter(|e| e.kind == Some(PathEventKind::Rescan))
        .map(|e| e.path.clone())
        .collect();
    new_rescan_paths.sort_unstable();

    let mut deduped_rescans: Vec<std::path::PathBuf> = Vec::with_capacity(new_rescan_paths.len());
    for path in new_rescan_paths {
        if deduped_rescans
            .iter()
            .any(|ancestor| path != *ancestor && path.starts_with(ancestor))
        {
            continue;
        }
        deduped_rescans.push(path);
    }

    deduped_rescans.retain(|new_path| {
        !pending_paths
            .iter()
            .any(|pending| is_covered_rescan(pending.kind, new_path, &pending.path))
    });

    if !deduped_rescans.is_empty() {
        pending_paths.retain(|pending| {
            !deduped_rescans.iter().any(|rescan_path| {
                pending.path == *rescan_path
                    || is_covered_rescan(pending.kind, &pending.path, rescan_path)
            })
        });
    }

    path_events.retain(|event| {
        event.kind != Some(PathEventKind::Rescan) || deduped_rescans.contains(&event.path)
    });
}

fn is_covered_rescan(kind: Option<PathEventKind>, path: &Path, ancestor: &Path) -> bool {
    kind == Some(PathEventKind::Rescan) && path != ancestor && path.starts_with(ancestor)
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct WatcherRegistrationId(u32);

struct WatcherRegistrationState {
    callback: Arc<dyn Fn(&notify::Event) + Send + Sync>,
    path: Arc<std::path::Path>,
}

#[derive(Default)]
struct WatcherState {
    watchers: HashMap<WatcherRegistrationId, WatcherRegistrationState>,
    path_registrations: HashMap<Arc<std::path::Path>, u32>,
    /// Registrations by their sanitized path, the form a registration's
    /// callback compares event paths against.
    registrations_by_path: HashMap<Arc<std::path::Path>, Vec<WatcherRegistrationId>>,
    last_registration: WatcherRegistrationId,
}

impl WatcherState {
    fn insert(
        &mut self,
        path: Arc<std::path::Path>,
        callback: Arc<dyn Fn(&notify::Event) + Send + Sync>,
    ) -> WatcherRegistrationId {
        let id = self.last_registration;
        self.last_registration = WatcherRegistrationId(id.0 + 1);
        self.registrations_by_path
            .entry(SanitizedPath::new(path.as_ref()).as_path().into())
            .or_default()
            .push(id);
        self.watchers.insert(
            id,
            WatcherRegistrationState {
                callback,
                path: path.clone(),
            },
        );
        *self.path_registrations.entry(path).or_insert(0) += 1;
        id
    }

    /// Forgets a registration. Returns its path when that was the path's last
    /// registration, so the caller stops watching it.
    fn remove_registration(&mut self, id: WatcherRegistrationId) -> Option<Arc<std::path::Path>> {
        let registration_state = self.watchers.remove(&id)?;
        let sanitized: &std::path::Path =
            SanitizedPath::new(registration_state.path.as_ref()).as_path();
        if let Some(ids) = self.registrations_by_path.get_mut(sanitized) {
            ids.retain(|registered| *registered != id);
            if ids.is_empty() {
                self.registrations_by_path.remove(sanitized);
            }
        }

        let count = self.path_registrations.get_mut(&registration_state.path)?;
        *count -= 1;
        if *count > 0 {
            return None;
        }
        self.path_registrations.remove(&registration_state.path);
        Some(registration_state.path)
    }

    /// The callbacks an event concerns: those registered on a path of the
    /// event or on one of its ancestors -- every other callback would discard
    /// it. Handing each event to every callback costs as much as there are
    /// watched directories, and with one per directory on Linux a large tree
    /// slowed the event thread until the kernel queue overflowed.
    fn callbacks_for(
        &self,
        event: &notify::Event,
    ) -> Vec<Arc<dyn Fn(&notify::Event) + Send + Sync>> {
        if event.need_rescan() {
            return self
                .watchers
                .values()
                .map(|registration| registration.callback.clone())
                .collect();
        }

        let mut ids = event
            .paths
            .iter()
            .flat_map(|path| SanitizedPath::new(path).as_path().ancestors())
            .filter_map(|ancestor| self.registrations_by_path.get(ancestor))
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        ids.sort_unstable_by_key(|id| id.0);
        ids.dedup();
        ids.iter()
            .filter_map(|id| self.watchers.get(id))
            .map(|registration| registration.callback.clone())
            .collect()
    }
}

pub struct GlobalWatcher {
    state: Mutex<WatcherState>,

    // DANGER: never keep the state lock while holding the watcher lock
    // two mutexes because calling watcher.add triggers an watcher.event, which needs watchers.
    #[cfg(target_os = "linux")]
    watcher: Mutex<notify::INotifyWatcher>,
    #[cfg(target_os = "freebsd")]
    watcher: Mutex<notify::KqueueWatcher>,
    #[cfg(target_os = "windows")]
    watcher: Mutex<notify::ReadDirectoryChangesWatcher>,
    #[cfg(target_os = "macos")]
    watcher: Mutex<notify::FsEventWatcher>,
}

impl GlobalWatcher {
    #[must_use]
    fn add(
        &self,
        path: Arc<std::path::Path>,
        mode: notify::RecursiveMode,
        cb: impl Fn(&notify::Event) + Send + Sync + 'static,
    ) -> anyhow::Result<WatcherRegistrationId> {
        use notify::Watcher;

        let mut state = self.state.lock();

        // Check if this path is already covered by an existing watched ancestor path.
        // On macOS and Windows, watching is recursive, so we don't need to watch
        // child paths if an ancestor is already being watched.
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        let path_already_covered = state.path_registrations.keys().any(|existing| {
            path.starts_with(existing.as_ref()) && path.as_ref() != existing.as_ref()
        });

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let path_already_covered = false;

        if !path_already_covered && !state.path_registrations.contains_key(&path) {
            drop(state);
            self.watcher.lock().watch(&path, mode)?;
            state = self.state.lock();
        }

        Ok(state.insert(path, Arc::new(cb)))
    }

    pub fn remove(&self, id: WatcherRegistrationId) {
        use notify::Watcher;
        let mut state = self.state.lock();
        let Some(unwatched_path) = state.remove_registration(id) else {
            return;
        };
        drop(state);
        self.watcher.lock().unwatch(&unwatched_path).log_err();
    }
}

static FS_WATCHER_INSTANCE: OnceLock<anyhow::Result<GlobalWatcher, notify::Error>> =
    OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn rescan(path: &str) -> PathEvent {
        PathEvent {
            path: PathBuf::from(path),
            kind: Some(PathEventKind::Rescan),
        }
    }

    fn changed(path: &str) -> PathEvent {
        PathEvent {
            path: PathBuf::from(path),
            kind: Some(PathEventKind::Changed),
        }
    }

    struct TestCase {
        name: &'static str,
        pending_paths: Vec<PathEvent>,
        path_events: Vec<PathEvent>,
        expected_pending_paths: Vec<PathEvent>,
        expected_path_events: Vec<PathEvent>,
    }

    fn recording_state(paths: &[&str]) -> (WatcherState, Arc<Mutex<Vec<String>>>) {
        let called = Arc::new(Mutex::new(Vec::new()));
        let mut state = WatcherState::default();
        for path in paths {
            let called = called.clone();
            let name = path.to_string();
            state.insert(
                Arc::from(Path::new(path)),
                Arc::new(move |_: &notify::Event| called.lock().push(name.clone())),
            );
        }
        (state, called)
    }

    fn dispatched(
        state: &WatcherState,
        called: &Mutex<Vec<String>>,
        event: &notify::Event,
    ) -> Vec<String> {
        called.lock().clear();
        for callback in state.callbacks_for(event) {
            callback(event);
        }
        let mut names = called.lock().clone();
        names.sort();
        names
    }

    #[test]
    fn an_event_reaches_only_the_registrations_above_it() {
        let (state, called) = recording_state(&["/root", "/root/src", "/root/src/deep", "/other"]);

        let edit = notify::Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
            .add_path(PathBuf::from("/root/src/main.rs"));
        assert_eq!(dispatched(&state, &called, &edit), ["/root", "/root/src"]);

        let rename = notify::Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
            .add_path(PathBuf::from("/root/src/deep/a.rs"))
            .add_path(PathBuf::from("/other/b.rs"));
        assert_eq!(
            dispatched(&state, &called, &rename),
            ["/other", "/root", "/root/src", "/root/src/deep"],
            "each path of the event reaches its own ancestors, once each"
        );

        let elsewhere = notify::Event::new(EventKind::Create(notify::event::CreateKind::Any))
            .add_path(PathBuf::from("/unwatched/file"));
        assert!(dispatched(&state, &called, &elsewhere).is_empty());
    }

    #[test]
    fn a_lost_sync_event_reaches_every_registration() {
        let (state, called) = recording_state(&["/root", "/root/src", "/other"]);
        // An overflowed queue names no path: every watcher has to rescan.
        let overflow = notify::Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan);
        assert_eq!(
            dispatched(&state, &called, &overflow),
            ["/other", "/root", "/root/src"]
        );
    }

    #[test]
    fn a_removed_registration_no_longer_receives_events() {
        let (mut state, called) = recording_state(&["/root", "/root/src"]);
        let id = state.registrations_by_path[Path::new("/root/src")][0];
        state.remove_registration(id);

        let edit = notify::Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
            .add_path(PathBuf::from("/root/src/main.rs"));
        assert_eq!(dispatched(&state, &called, &edit), ["/root"]);
        assert!(
            !state
                .registrations_by_path
                .contains_key(Path::new("/root/src"))
        );
        assert!(
            !state
                .path_registrations
                .contains_key(Path::new("/root/src"))
        );
    }

    #[test]
    fn test_coalesce_pending_rescans() {
        let test_cases = [
            TestCase {
                name: "coalesces descendant rescans under pending ancestor",
                pending_paths: vec![rescan("/root")],
                path_events: vec![rescan("/root/child"), rescan("/root/child/grandchild")],
                expected_pending_paths: vec![rescan("/root")],
                expected_path_events: vec![],
            },
            TestCase {
                name: "new ancestor rescan replaces pending descendant rescans",
                pending_paths: vec![
                    changed("/other"),
                    rescan("/root/child"),
                    rescan("/root/child/grandchild"),
                ],
                path_events: vec![rescan("/root")],
                expected_pending_paths: vec![changed("/other")],
                expected_path_events: vec![rescan("/root")],
            },
            TestCase {
                name: "same path rescan replaces pending non-rescan event",
                pending_paths: vec![changed("/root")],
                path_events: vec![rescan("/root")],
                expected_pending_paths: vec![],
                expected_path_events: vec![rescan("/root")],
            },
            TestCase {
                name: "unrelated rescans are preserved",
                pending_paths: vec![rescan("/root-a")],
                path_events: vec![rescan("/root-b")],
                expected_pending_paths: vec![rescan("/root-a")],
                expected_path_events: vec![rescan("/root-b")],
            },
            TestCase {
                name: "batch ancestor rescan replaces descendant rescan",
                pending_paths: vec![],
                path_events: vec![rescan("/root/child"), rescan("/root")],
                expected_pending_paths: vec![],
                expected_path_events: vec![rescan("/root")],
            },
        ];

        for test_case in test_cases {
            let mut pending_paths = test_case.pending_paths;
            let mut path_events = test_case.path_events;

            coalesce_pending_rescans(&mut pending_paths, &mut path_events);

            assert_eq!(
                pending_paths, test_case.expected_pending_paths,
                "pending_paths mismatch for case: {}",
                test_case.name
            );
            assert_eq!(
                path_events, test_case.expected_path_events,
                "path_events mismatch for case: {}",
                test_case.name
            );
        }
    }
}

fn handle_event(event: Result<notify::Event, notify::Error>) {
    log::trace!("global handle event: {event:?}");
    // Filter out access events, which could lead to a weird bug on Linux after upgrading notify
    // https://github.com/zed-industries/zed/actions/runs/14085230504/job/39449448832
    let Some(event) = event
        .log_err()
        .filter(|event| !matches!(event.kind, EventKind::Access(_)))
    else {
        return;
    };
    global::<()>(move |watcher| {
        let callbacks = watcher.state.lock().callbacks_for(&event);
        if event.need_rescan() {
            log::warn!(
                "filesystem watcher lost sync; rescanning {} registrations",
                callbacks.len()
            );
        }
        for callback in callbacks {
            callback(&event);
        }
    })
    .log_err();
}

pub fn global<T>(f: impl FnOnce(&GlobalWatcher) -> T) -> anyhow::Result<T> {
    let result = FS_WATCHER_INSTANCE.get_or_init(|| {
        notify::recommended_watcher(handle_event).map(|file_watcher| GlobalWatcher {
            state: Mutex::new(WatcherState::default()),
            watcher: Mutex::new(file_watcher),
        })
    });
    match result {
        Ok(g) => Ok(f(g)),
        Err(e) => Err(anyhow::anyhow!("{e}")),
    }
}
