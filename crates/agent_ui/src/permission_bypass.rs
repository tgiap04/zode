//! Which checkouts the reader has switched the permission prompts off for.
//!
//! Keyed by the checkout's own path, because a checkout is a directory on disk
//! shared by every window that opens it -- the same reasoning, and the same
//! defect avoided, as `git_ui`'s `CheckoutViewState`: a record keyed by
//! workspace makes switching checkout open a different drawer.
//!
//! And by the host that path is on. `/home/me/repo` in a WSL distro and the
//! same string on this machine are different directories, and a toggle the
//! reader confirmed for one must not switch the prompts off in the other.
//!
//! The repository anchor rides along as the *value*, never as the key. It is
//! `original_repo_abs_path` and never `work_directory_abs_path`, which is the
//! checkout you happen to be standing in; `prune` is the only thing that reads
//! it, and it is the only thing that needs to scope itself to one repository.
//!
//! This record is the only input that decides whether a safety control is off,
//! so what can write it matters more than what it holds. One caller writes it:
//! a menu action the reader confirmed. No settings key reaches it, so nothing a
//! repository carries can switch it on.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;
use collections::HashMap;
use db::kvp::KeyValueStore;
use gpui::{App, AppContext as _, Context, Entity, Global, Task};
use project::Project;
use serde::{Deserialize, Serialize};
use util::ResultExt as _;

/// One un-scoped key. No workspace id: see the module doc.
const BYPASS_KEY: &str = "AgentPermissionBypass";

/// Writes are coalesced behind this delay, as `CheckoutViewState` does, so
/// toggling several checkouts in a row is one database round-trip.
const WRITE_THROTTLE: Duration = Duration::from_millis(500);

struct GlobalPermissionBypass(Entity<PermissionBypassStore>);

impl Global for GlobalPermissionBypass {}

/// The machine a checkout path is a directory on.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BypassHost {
    Local,
    /// A remote host, by a stable identity of its connection.
    Remote(String),
}

impl BypassHost {
    /// The host `project`'s checkouts live on, or `None` for a collaboration
    /// guest: those checkouts are on the collaborator's machine, and no record
    /// here speaks for them.
    pub fn for_project(project: &Project, cx: &App) -> Option<Self> {
        let identity = project
            .remote_connection_options(cx)
            .map(|options| workspace::remote_connection_identity(&options));
        Self::for_connection(project.is_local(), identity.as_ref())
    }

    fn for_connection(
        is_local: bool,
        identity: Option<&workspace::RemoteConnectionIdentity>,
    ) -> Option<Self> {
        match identity {
            Some(identity) => Some(Self::Remote(host_key(identity))),
            None if is_local => Some(Self::Local),
            None => None,
        }
    }
}

/// The spelling a remote host is recorded under. Shared with everything else
/// that keys a record by host, so it is spelled in one place.
fn host_key(identity: &workspace::RemoteConnectionIdentity) -> String {
    identity.stable_key()
}

/// The wire shape. A map rather than a list of pairs, so writing the same
/// checkout twice cannot record it twice.
#[derive(Default, Serialize, Deserialize)]
struct SerializedBypass {
    /// checkout path -> repository anchor, for checkouts on this machine.
    /// Kept where it always was, so a record written before remote hosts
    /// existed still reads as this machine's.
    #[serde(default)]
    enabled: HashMap<String, String>,
    /// host identity -> (checkout path -> repository anchor). An older build
    /// ignores this field, so a remote checkout's toggle is simply absent
    /// there.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    remote: HashMap<String, HashMap<String, String>>,
}

pub struct PermissionBypassStore {
    enabled: HashMap<String, String>,
    remote: HashMap<String, HashMap<String, String>>,
    /// Whether the record has been read. A write before the read has landed
    /// would persist an empty map over whatever is on disk.
    loaded: bool,
    /// Held rather than detached, so dropping the global cancels a write that
    /// has not fired yet.
    pending_write: Option<Task<Option<()>>>,
}

impl PermissionBypassStore {
    /// The record, created on first use.
    pub fn global(cx: &mut App) -> Entity<Self> {
        if let Some(global) = cx.try_global::<GlobalPermissionBypass>() {
            return global.0.clone();
        }
        let store = cx.new(|_| Self::new());
        cx.set_global(GlobalPermissionBypass(store.clone()));
        store
    }

    pub fn new() -> Self {
        Self {
            enabled: HashMap::default(),
            remote: HashMap::default(),
            loaded: false,
            pending_write: None,
        }
    }

    /// Reads the record. Idempotent, so every caller may await it.
    pub fn load(&mut self, cx: &mut Context<Self>) -> Task<()> {
        if self.loaded {
            return Task::ready(());
        }
        let kvp = KeyValueStore::global(cx);
        cx.spawn(async move |this, cx| {
            let raw = cx
                .background_spawn(async move { kvp.read_kvp(BYPASS_KEY) })
                .await
                .context("loading the agent permission bypass record")
                .log_err()
                .flatten();

            this.update(cx, |this, cx| {
                if let Some(record) = raw
                    .as_deref()
                    .and_then(|raw| serde_json::from_str::<SerializedBypass>(raw).log_err())
                {
                    // Merged, not assigned. Anything already in memory when the
                    // read lands is a gesture the reader confirmed in the
                    // meantime, and the disk is the older of the two.
                    let mut merged = record.enabled;
                    merged.extend(std::mem::take(&mut this.enabled));
                    this.enabled = merged;
                    let mut merged_remote = record.remote;
                    for (host, checkouts) in std::mem::take(&mut this.remote) {
                        merged_remote.entry(host).or_default().extend(checkouts);
                    }
                    this.remote = merged_remote;
                }
                // Set whether or not anything was found: what gates a write is
                // "we looked", not "we found something".
                this.loaded = true;
                cx.notify();
            })
            .ok();
        })
    }

    fn checkouts(&self, host: &BypassHost) -> Option<&HashMap<String, String>> {
        match host {
            BypassHost::Local => Some(&self.enabled),
            BypassHost::Remote(id) => self.remote.get(id),
        }
    }

    fn checkouts_mut(&mut self, host: &BypassHost) -> &mut HashMap<String, String> {
        match host {
            BypassHost::Local => &mut self.enabled,
            BypassHost::Remote(id) => self.remote.entry(id.clone()).or_default(),
        }
    }

    /// Drops a remote host's map once nothing on it is switched on, so a host
    /// toggled once does not stay in the record forever.
    fn forget_empty_hosts(&mut self) {
        self.remote.retain(|_, checkouts| !checkouts.is_empty());
    }

    /// Whether agents started in `checkout` on `host` should skip their
    /// permission prompts. A checkout nobody has touched answers `false`.
    pub fn is_enabled(&self, host: &BypassHost, checkout: &Path) -> bool {
        self.checkouts(host)
            .is_some_and(|checkouts| checkouts.contains_key(checkout.to_string_lossy().as_ref()))
    }

    /// `anchor` must be the repository's `original_repo_abs_path`. It is stored
    /// for `prune` alone.
    pub fn set(
        &mut self,
        host: &BypassHost,
        checkout: &Path,
        anchor: &Path,
        on: bool,
        cx: &mut Context<Self>,
    ) {
        let checkout = checkout.to_string_lossy().to_string();
        let changed = if on {
            let anchor = anchor.to_string_lossy().to_string();
            // Compared by value, not by presence: re-enabling with a corrected
            // anchor changes the map, and `is_none()` would call that no change
            // and never schedule the write.
            self.checkouts_mut(host).insert(checkout, anchor.clone()) != Some(anchor)
        } else {
            // Removed rather than stored as `false`: a record that keeps its
            // disabled entries grows by one for every checkout anyone ever
            // toggled twice, and never shrinks.
            let removed = self.checkouts_mut(host).remove(&checkout).is_some();
            self.forget_empty_hosts();
            removed
        };
        if changed {
            self.schedule_write(cx);
            cx.notify();
        }
    }

    /// Drops entries for checkouts of `anchor` that no longer exist, and touches
    /// nothing belonging to any other repository -- a panel that cannot see a
    /// project has no standing to delete its state.
    pub fn prune(
        &mut self,
        host: &BypassHost,
        anchor: &Path,
        live_checkouts: &[PathBuf],
        cx: &mut Context<Self>,
    ) {
        // `git worktree list` always names the main checkout, so an empty list
        // means the store is still warming up rather than that the worktrees are
        // gone. Pruning on it erases the record on every launch.
        if live_checkouts.is_empty() {
            return;
        }
        let anchor = anchor.to_string_lossy().to_string();
        let live: collections::HashSet<String> = live_checkouts
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect();

        let Some(before) = self.checkouts(host).map(HashMap::len) else {
            return;
        };
        let checkouts = self.checkouts_mut(host);
        checkouts
            .retain(|checkout, entry_anchor| *entry_anchor != anchor || live.contains(checkout));
        let after = checkouts.len();
        self.forget_empty_hosts();
        if after != before {
            self.schedule_write(cx);
            cx.notify();
        }
    }

    fn schedule_write(&mut self, cx: &mut Context<Self>) {
        // Resolved here rather than after the await: reaching back for the store
        // once the task is running is how a write outlives the app that wanted it.
        let kvp = KeyValueStore::global(cx);
        // Replacing the task drops the previous one, which cancels a write that
        // has not fired yet. That is the throttle.
        self.pending_write = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(WRITE_THROTTLE).await;
            // Read after the delay, never snapshotted before it. A snapshot is
            // the map as it was when the *first* of the coalesced gestures
            // happened, so anything that changed during the throttle -- another
            // toggle, or `load` merging the disk in -- would be written back out
            // of existence by a write that predates it.
            let record = this
                .read_with(cx, |this, _| SerializedBypass {
                    enabled: this.enabled.clone(),
                    remote: this.remote.clone(),
                })
                .ok()?;
            let value = serde_json::to_string(&record).log_err()?;
            cx.background_spawn(async move { kvp.write_kvp(BYPASS_KEY.to_string(), value).await })
                .await
                .context("writing the agent permission bypass record")
                .log_err()
                .map(|_| ())
        }));
    }
}

impl Default for PermissionBypassStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    /// A database of this test's own. `AppDatabase::global` otherwise falls back
    /// to a process-wide static, and since this record lives under one un-scoped
    /// key every test in the binary would then read and write the same row --
    /// which is a flake, not a suite. The same trap `checkout_state`'s tests hit.
    fn init_test(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_global(db::AppDatabase::test_new()));
    }

    fn store(cx: &mut TestAppContext) -> Entity<PermissionBypassStore> {
        init_test(cx);
        cx.new(|_| PermissionBypassStore::new())
    }

    /// The safe default, and the only one that can be wrong in the dangerous
    /// direction.
    #[gpui::test]
    fn a_checkout_nobody_touched_has_no_bypass(cx: &mut TestAppContext) {
        let store = store(cx);
        store.read_with(cx, |store, _| {
            assert!(!store.is_enabled(&BypassHost::Local, Path::new("/repos/zode/wt-a")));
        });
    }

    /// The anchor has to survive the round trip or `prune` loses its scoping
    /// after one restart and starts deleting other repositories' entries.
    #[gpui::test]
    fn the_record_survives_a_round_trip(cx: &mut TestAppContext) {
        let store = store(cx);
        store.update(cx, |store, cx| {
            store.set(
                &BypassHost::Local,
                Path::new("/repos/zode/wt-a"),
                Path::new("/repos/zode"),
                true,
                cx,
            );
        });

        let json = store.read_with(cx, |store, _| {
            serde_json::to_string(&SerializedBypass {
                enabled: store.enabled.clone(),
                remote: store.remote.clone(),
            })
            .expect("the record must serialise")
        });
        let parsed: SerializedBypass =
            serde_json::from_str(&json).expect("and parse back into itself");

        assert_eq!(
            parsed.enabled.get("/repos/zode/wt-a").map(String::as_str),
            Some("/repos/zode"),
            "the checkout is the key and its repository anchor is the value"
        );
    }

    /// The whole reason the anchor is stored at all.
    #[gpui::test]
    fn prune_drops_a_gone_checkout_and_leaves_another_repository_alone(cx: &mut TestAppContext) {
        let store = store(cx);
        store.update(cx, |store, cx| {
            store.set(
                &BypassHost::Local,
                Path::new("/repos/a/gone"),
                Path::new("/repos/a"),
                true,
                cx,
            );
            store.set(
                &BypassHost::Local,
                Path::new("/repos/a/live"),
                Path::new("/repos/a"),
                true,
                cx,
            );
            store.set(
                &BypassHost::Local,
                Path::new("/repos/b/live"),
                Path::new("/repos/b"),
                true,
                cx,
            );

            store.prune(
                &BypassHost::Local,
                Path::new("/repos/a"),
                &[PathBuf::from("/repos/a/live")],
                cx,
            );

            assert!(
                !store.is_enabled(&BypassHost::Local, Path::new("/repos/a/gone")),
                "removed"
            );
            assert!(
                store.is_enabled(&BypassHost::Local, Path::new("/repos/a/live")),
                "still there"
            );
            assert!(
                store.is_enabled(&BypassHost::Local, Path::new("/repos/b/live")),
                "another project's entry is not this panel's to delete"
            );
        });
    }

    /// `git worktree list` always names the main checkout, so an empty list is
    /// the store warming up. Pruning on it erases the record on every launch.
    #[gpui::test]
    fn an_empty_live_list_prunes_nothing(cx: &mut TestAppContext) {
        let store = store(cx);
        store.update(cx, |store, cx| {
            store.set(
                &BypassHost::Local,
                Path::new("/repos/a/wt"),
                Path::new("/repos/a"),
                true,
                cx,
            );
            store.prune(&BypassHost::Local, Path::new("/repos/a"), &[], cx);
            assert!(store.is_enabled(&BypassHost::Local, Path::new("/repos/a/wt")));
        });
    }

    /// The Critical defect review found, pinned.
    ///
    /// The panel reads this record to draw its mark and build its menu, and it
    /// was not loading it -- so after a restart every checkout read as "off",
    /// and the first toggle serialised a one-entry map over the real record.
    /// Two halves failed together: the panel did not call `load` (fixed in
    /// `git_ui`, held by compilation), and `load` *assigned* where it had to
    /// merge, so even a panel that did load could clobber a gesture the reader
    /// confirmed while the read was in flight.
    ///
    /// This half is the merge. Falsifiable by restoring `this.enabled =
    /// record.enabled`: the confirmed entry disappears.
    #[gpui::test]
    async fn a_gesture_made_before_the_read_lands_survives_it(cx: &mut TestAppContext) {
        init_test(cx);
        let on_disk = SerializedBypass {
            enabled: [("/repos/other/wt".to_string(), "/repos/other".to_string())]
                .into_iter()
                .collect(),
            remote: HashMap::default(),
        };
        cx.update(|cx| db::kvp::KeyValueStore::global(cx))
            .write_kvp(
                BYPASS_KEY.to_string(),
                serde_json::to_string(&on_disk).expect("the record must serialise"),
            )
            .await
            .expect("seeding the record must succeed");

        let store = cx.new(|_| PermissionBypassStore::new());
        // Confirmed before the read has landed -- the reader clicked while the
        // app was still starting.
        store.update(cx, |store, cx| {
            store.set(
                &BypassHost::Local,
                Path::new("/repos/zode/wt"),
                Path::new("/repos/zode"),
                true,
                cx,
            );
        });
        store.update(cx, |store, cx| store.load(cx)).await;

        store.read_with(cx, |store, _| {
            assert!(
                store.is_enabled(&BypassHost::Local, Path::new("/repos/zode/wt")),
                "a read that lands after a confirmed gesture must not undo it"
            );
            assert!(
                store.is_enabled(&BypassHost::Local, Path::new("/repos/other/wt")),
                "and it must not lose what was already on disk either"
            );
        });

        // And the same has to be true on disk. This is the only test that reads
        // the record back out; without it the write half of this module is
        // exercised by nothing at all.
        cx.executor().advance_clock(WRITE_THROTTLE * 2);
        cx.run_until_parked();
        let written = cx
            .update(|cx| db::kvp::KeyValueStore::global(cx))
            .read_kvp(BYPASS_KEY)
            .expect("the record must be readable")
            .expect("and must have been written");
        let written: SerializedBypass =
            serde_json::from_str(&written).expect("and must parse back");
        assert!(
            written.enabled.contains_key("/repos/zode/wt")
                && written.enabled.contains_key("/repos/other/wt"),
            "the persisted map must hold both, got {:?}",
            written.enabled
        );
    }

    /// Storing `false` would grow the record by one for every checkout anyone
    /// ever toggled twice, and it would never shrink.
    #[gpui::test]
    fn turning_it_off_removes_the_entry_rather_than_storing_false(cx: &mut TestAppContext) {
        let store = store(cx);
        store.update(cx, |store, cx| {
            let checkout = Path::new("/repos/a/wt");
            store.set(
                &BypassHost::Local,
                checkout,
                Path::new("/repos/a"),
                true,
                cx,
            );
            store.set(
                &BypassHost::Local,
                checkout,
                Path::new("/repos/a"),
                false,
                cx,
            );

            assert!(!store.is_enabled(&BypassHost::Local, checkout));
            assert!(
                store.enabled.is_empty(),
                "off means gone, not recorded as off; got {:?}",
                store.enabled
            );
        });
    }

    fn wsl() -> BypassHost {
        BypassHost::Remote(r#"["wsl","Ubuntu",null]"#.to_string())
    }

    async fn local_project(cx: &mut TestAppContext) -> Entity<Project> {
        cx.update(|cx| {
            let store = settings::SettingsStore::test(cx);
            cx.set_global(store);
        });
        Project::test(fs::FakeFs::new(cx.executor()), [], cx).await
    }

    #[gpui::test]
    async fn a_local_project_records_against_this_machine(cx: &mut TestAppContext) {
        let project = local_project(cx).await;
        let host = project.read_with(cx, |project, cx| BypassHost::for_project(project, cx));
        assert_eq!(host, Some(BypassHost::Local));
    }

    /// A guest's checkouts are on the collaborator's machine. Reading them as
    /// this machine's would let a local toggle on the same path switch the
    /// prompts off there.
    #[gpui::test]
    async fn a_collaboration_guest_has_no_bypass_host(cx: &mut TestAppContext) {
        let project = local_project(cx).await;
        project.update(cx, |project, _| project.mark_as_collab_for_testing());
        let host = project.read_with(cx, |project, cx| BypassHost::for_project(project, cx));
        assert_eq!(host, None);
    }

    /// The spellings are what the record on disk is keyed by: a change to any
    /// of them silently turns every recorded remote toggle off.
    #[test]
    fn remote_hosts_are_spelled_as_recorded() {
        use workspace::RemoteConnectionIdentity as Identity;
        let cases = [
            (
                Identity::Wsl {
                    distro_name: "Ubuntu".into(),
                    user: None,
                },
                r#"["wsl","Ubuntu",null]"#,
            ),
            (
                Identity::Wsl {
                    distro_name: "Ubuntu".into(),
                    user: Some("me".into()),
                },
                r#"["wsl","Ubuntu","me"]"#,
            ),
            (
                Identity::Ssh {
                    host: "box".into(),
                    username: Some("me".into()),
                    port: Some(2222),
                },
                r#"["ssh","box","me",2222]"#,
            ),
            (
                Identity::Ssh {
                    host: "box".into(),
                    username: None,
                    port: None,
                },
                r#"["ssh","box",null,null]"#,
            ),
            (
                Identity::Docker {
                    container_id: "c0ffee".into(),
                    name: "dev".into(),
                    remote_user: "root".into(),
                },
                r#"["docker","c0ffee","dev","root"]"#,
            ),
            (
                Identity::Relay {
                    host_device_id: "device-1".into(),
                },
                r#"["relay","device-1"]"#,
            ),
            (Identity::Mock { id: 7 }, r#"["Mock { id: 7 }"]"#),
        ];
        for (identity, spelling) in cases {
            assert_eq!(
                BypassHost::for_connection(true, Some(&identity)),
                Some(BypassHost::Remote(spelling.to_string())),
                "{identity:?}"
            );
        }
        assert_eq!(
            BypassHost::for_connection(true, None),
            Some(BypassHost::Local)
        );
        assert_eq!(BypassHost::for_connection(false, None), None, "a guest");
    }

    /// The defect host keying exists for: the same path string on two
    /// machines is two directories, and the reader confirmed only one.
    #[gpui::test]
    fn the_same_path_on_two_hosts_does_not_share_a_bypass(cx: &mut TestAppContext) {
        let store = store(cx);
        let checkout = Path::new("/home/me/repo/wt");
        let anchor = Path::new("/home/me/repo");
        let other_remote = BypassHost::Remote("Ssh { host: \"box\" }".to_string());
        store.update(cx, |store, cx| {
            store.set(&BypassHost::Local, checkout, anchor, true, cx);
            assert!(store.is_enabled(&BypassHost::Local, checkout));
            assert!(
                !store.is_enabled(&wsl(), checkout),
                "a local toggle must not switch the prompts off on a remote host"
            );

            store.set(&wsl(), checkout, anchor, true, cx);
            store.set(&BypassHost::Local, checkout, anchor, false, cx);
            assert!(!store.is_enabled(&BypassHost::Local, checkout));
            assert!(
                store.is_enabled(&wsl(), checkout),
                "turning one off leaves the other"
            );
            assert!(
                !store.is_enabled(&other_remote, checkout),
                "remote hosts are apart too"
            );

            store.set(&wsl(), checkout, anchor, false, cx);
            assert!(
                store.remote.is_empty(),
                "a host with nothing on leaves no entry behind, got {:?}",
                store.remote
            );
        });
    }

    #[gpui::test]
    fn prune_on_one_host_leaves_another_hosts_entries_alone(cx: &mut TestAppContext) {
        let store = store(cx);
        let anchor = Path::new("/home/me/repo");
        store.update(cx, |store, cx| {
            store.set(
                &BypassHost::Local,
                Path::new("/home/me/repo/wt"),
                anchor,
                true,
                cx,
            );
            store.set(&wsl(), Path::new("/home/me/repo/gone"), anchor, true, cx);
            store.set(&wsl(), Path::new("/home/me/repo/live"), anchor, true, cx);

            store.prune(&wsl(), anchor, &[PathBuf::from("/home/me/repo/live")], cx);

            assert!(!store.is_enabled(&wsl(), Path::new("/home/me/repo/gone")));
            assert!(store.is_enabled(&wsl(), Path::new("/home/me/repo/live")));
            assert!(
                store.is_enabled(&BypassHost::Local, Path::new("/home/me/repo/wt")),
                "a remote panel's live list says nothing about this machine's checkouts"
            );
        });
    }

    /// A record written before hosts existed is this machine's, and nothing
    /// else's.
    #[gpui::test]
    async fn a_record_from_before_hosts_loads_as_local_only(cx: &mut TestAppContext) {
        init_test(cx);
        cx.update(|cx| db::kvp::KeyValueStore::global(cx))
            .write_kvp(
                BYPASS_KEY.to_string(),
                r#"{"enabled":{"/home/me/repo/wt":"/home/me/repo"}}"#.to_string(),
            )
            .await
            .expect("seeding the record must succeed");

        let store = cx.new(|_| PermissionBypassStore::new());
        store.update(cx, |store, cx| store.load(cx)).await;

        store.read_with(cx, |store, _| {
            let checkout = Path::new("/home/me/repo/wt");
            assert!(store.is_enabled(&BypassHost::Local, checkout));
            assert!(!store.is_enabled(&wsl(), checkout));
        });
    }

    /// A record on disk names its hosts by the spelling of the build that
    /// wrote it; the host it was written for must still find it.
    #[gpui::test]
    async fn a_remote_record_written_earlier_still_matches_its_host(cx: &mut TestAppContext) {
        init_test(cx);
        cx.update(|cx| db::kvp::KeyValueStore::global(cx))
            .write_kvp(
                BYPASS_KEY.to_string(),
                r#"{"enabled":{},"remote":{"[\"wsl\",\"Ubuntu\",null]":{"/home/me/repo/wt":"/home/me/repo"},"[\"ssh\",\"box\",\"me\",2222]":{"/srv/app":"/srv/app"}}}"#
                    .to_string(),
            )
            .await
            .expect("seeding the record must succeed");

        let store = cx.new(|_| PermissionBypassStore::new());
        store.update(cx, |store, cx| store.load(cx)).await;

        let wsl = workspace::RemoteConnectionIdentity::Wsl {
            distro_name: "Ubuntu".into(),
            user: None,
        };
        let ssh = workspace::RemoteConnectionIdentity::Ssh {
            host: "box".into(),
            username: Some("me".into()),
            port: Some(2222),
        };
        let host_of =
            |identity| BypassHost::for_connection(false, Some(identity)).expect("a remote host");
        store.read_with(cx, |store, _| {
            assert!(store.is_enabled(&host_of(&wsl), Path::new("/home/me/repo/wt")));
            assert!(store.is_enabled(&host_of(&ssh), Path::new("/srv/app")));
            assert!(!store.is_enabled(&host_of(&wsl), Path::new("/srv/app")));
        });
    }

    #[gpui::test]
    async fn a_remote_entry_survives_a_restart(cx: &mut TestAppContext) {
        init_test(cx);
        let checkout = Path::new("/home/me/repo/wt");
        let store = cx.new(|_| PermissionBypassStore::new());
        store.update(cx, |store, cx| store.load(cx)).await;
        store.update(cx, |store, cx| {
            store.set(&wsl(), checkout, Path::new("/home/me/repo"), true, cx)
        });
        cx.executor().advance_clock(WRITE_THROTTLE * 2);
        cx.run_until_parked();

        let written = cx
            .update(|cx| db::kvp::KeyValueStore::global(cx))
            .read_kvp(BYPASS_KEY)
            .expect("the record must be readable")
            .expect("and must have been written");
        let parsed: SerializedBypass = serde_json::from_str(&written).expect("and parse back");
        assert!(
            parsed.enabled.is_empty(),
            "nothing local was turned on: {written}"
        );

        let restarted = cx.new(|_| PermissionBypassStore::new());
        restarted.update(cx, |store, cx| store.load(cx)).await;
        restarted.read_with(cx, |store, _| {
            assert!(store.is_enabled(&wsl(), checkout), "record: {written}");
            assert!(!store.is_enabled(&BypassHost::Local, checkout));
        });
    }
}
