//! One scan of the session stores, shared by everything that needs it.
//!
//! Reading the agents' histories means opening every transcript on disk
//! (`agent_sessions::claude::ClaudeProvider::list`), which for a heavy user is
//! thousands of files. Before this existed the history panel did that sweep for
//! itself; the moment a second surface wants the same answer -- the sidebar,
//! showing which agents ran in which worktree -- a second sweep is one sweep too
//! many, and a sweep per rendered row is a hang.
//!
//! So the sweep happens here, once, and the result is an
//! [`agent_sessions::SessionIndex`] behind an `Arc`: every consumer reads the
//! same allocation, and asking "which sessions ran in this directory" is a hash
//! rather than a filter.
//!
//! The index type itself lives in `agent_sessions`, which is deliberately
//! synchronous and free of `gpui`. This is the half that needs a window: the
//! background task, the global, and the change notification.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use agent_sessions::{AgentKind, SessionIndex};
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, Global, SharedString, Subscription, Task,
    WeakEntity,
};
use project::{
    Project,
    agent_session_host::{AgentSessionHost, SessionListing},
};
use workspace::RemoteConnectionIdentity;

/// Handle to the process-wide store.
struct GlobalSessionStore(Entity<SessionStore>);

impl Global for GlobalSessionStore {}

/// One store per remote host, by the identity of its connection.
///
/// Keyed by host rather than by project: every project on a WSL distro or an
/// SSH host shares that host's home, and so its stores. A store per project
/// would sweep the same thousands of transcripts once per window.
///
/// Bounded by the hosts this app has opened a project on in this session, a
/// handful at most. An entry outlives the last window on its host on purpose:
/// opening another project there finds the list it left, and what it holds
/// once no project is open is one index -- its connections leave with their
/// projects (see [`SweepHosts::Remote`]).
#[derive(Default)]
struct RemoteSessionStores(HashMap<RemoteConnectionIdentity, Entity<SessionStore>>);

impl Global for RemoteSessionStores {}

/// What a sweep asks.
enum SweepHosts {
    /// One host for the whole store: this machine's own.
    Here(AgentSessionHost),
    /// The projects open on one remote host, oldest first, each through its
    /// own connection. A sweep asks through the first whose connection is
    /// still up.
    ///
    /// A project leaves this list the moment it is released. Holding its
    /// connection past that would keep a dead link alive and have the next
    /// sweep wait on it, while other windows on the same host still have
    /// connections that answer.
    Remote(Vec<RemoteRegistrant>),
}

struct RemoteRegistrant {
    project: EntityId,
    /// Weak so this store never keeps a closed window's project alive; read
    /// at sweep time to pass over one whose connection has dropped.
    project_handle: WeakEntity<Project>,
    host: AgentSessionHost,
    _released: Subscription,
}

pub struct SessionStore {
    index: Arc<SessionIndex>,
    hosts: SweepHosts,
    /// Why the last sweep found nothing to show, when it could not ask at all
    /// -- a remote server too old to answer, or one that did not. Cleared by
    /// the next sweep that succeeds.
    error: Option<SharedString>,
    /// The stores the last sweep could not read, and why.
    unavailable: Vec<(AgentKind, String)>,
    /// The sweep in flight. Held in a field rather than detached so closing the
    /// app drops it -- a detached background scan outlives everything that
    /// wanted its answer.
    scanning: Option<Task<()>>,
    /// The remote project the sweep in flight is asking through. If it closes,
    /// the answer can no longer arrive, so the sweep is started over.
    sweeping_through: Option<EntityId>,
    /// A refresh that arrived while a sweep was already running. Coalesced to a
    /// single re-run rather than queued: ten requests during one sweep must
    /// cost one more sweep, not ten.
    rescan_requested: bool,
    /// Bumped on every completed sweep. Consumers that cache anything derived
    /// from the index compare this instead of comparing the index itself.
    generation: u64,
    /// Ids forgotten while a sweep was in flight. That sweep may have read the
    /// disk before the trash, so its listing can still name them; they are
    /// filtered out of it before it is installed. Emptied when the sweep
    /// lands -- a later sweep reads the disk after the delete -- so it holds at
    /// most one sweep's worth of deletes.
    forgotten_during_sweep: HashSet<Arc<str>>,
}

impl SessionStore {
    /// The store, created on first use.
    ///
    /// Not created in `init`: a session that never opens an agent surface
    /// should never pay for one.
    pub fn global(cx: &mut App) -> Entity<Self> {
        if let Some(global) = cx.try_global::<GlobalSessionStore>() {
            return global.0.clone();
        }
        let store = cx.new(|_| {
            Self::new(SweepHosts::Here(AgentSessionHost::Local(
                agent_sessions::default_providers(),
            )))
        });
        cx.set_global(GlobalSessionStore(store.clone()));
        store
    }

    /// The store holding `project`'s sessions: the process-wide one for a
    /// project on this machine, and the one for its host for a remote project.
    ///
    /// A collaboration guest gets the process-wide one, as it always has: there
    /// is no host it could ask instead, and its deletes refuse on their own.
    pub fn for_project(project: &Entity<Project>, cx: &mut App) -> Entity<Self> {
        let remote = {
            let project = project.read(cx);
            project
                .remote_session_host(cx)
                .zip(project.remote_connection_options(cx))
        };
        match remote {
            Some((host, options)) => Self::for_remote_host(
                workspace::remote_connection_identity(&options),
                project,
                AgentSessionHost::Remote(host),
                cx,
            ),
            None => Self::global(cx),
        }
    }

    /// The store for the host `identity` names, created on first use, with
    /// `project` -- reached through `host` -- among those a sweep may ask.
    ///
    /// A project already registered has its host replaced: the store must ask
    /// through the connection the project has now.
    pub(crate) fn for_remote_host(
        identity: RemoteConnectionIdentity,
        project: &Entity<Project>,
        host: AgentSessionHost,
        cx: &mut App,
    ) -> Entity<Self> {
        let existing = cx
            .default_global::<RemoteSessionStores>()
            .0
            .get(&identity)
            .cloned();
        let store = match existing {
            Some(store) => store,
            None => {
                let store = cx.new(|_| Self::new(SweepHosts::Remote(Vec::new())));
                cx.default_global::<RemoteSessionStores>()
                    .0
                    .insert(identity, store.clone());
                store
            }
        };
        store.update(cx, |store, cx| store.register(project, host, cx));
        store
    }

    fn register(
        &mut self,
        project: &Entity<Project>,
        host: AgentSessionHost,
        cx: &mut Context<Self>,
    ) {
        let SweepHosts::Remote(registrants) = &mut self.hosts else {
            return;
        };
        let project_id = project.entity_id();
        if let Some(registrant) = registrants
            .iter_mut()
            .find(|registrant| registrant.project == project_id)
        {
            registrant.host = host;
            return;
        }
        let released = cx.observe_release(project, move |this, _, cx| {
            if let SweepHosts::Remote(registrants) = &mut this.hosts {
                registrants.retain(|registrant| registrant.project != project_id);
            }
            if this.sweeping_through == Some(project_id) {
                this.scanning = None;
                this.sweeping_through = None;
                this.refresh(cx);
            }
        });
        registrants.push(RemoteRegistrant {
            project: project_id,
            project_handle: project.downgrade(),
            host,
            _released: released,
        });
    }

    /// Where the next sweep asks, if anywhere can be asked, and through which
    /// remote project.
    fn sweep_host(&self, cx: &App) -> Option<(Option<EntityId>, AgentSessionHost)> {
        match &self.hosts {
            SweepHosts::Here(host) => Some((None, host.clone())),
            SweepHosts::Remote(registrants) => registrants
                .iter()
                .find(|registrant| {
                    registrant
                        .project_handle
                        .upgrade()
                        .is_some_and(|project| !project.read(cx).is_disconnected(cx))
                })
                .map(|registrant| (Some(registrant.project), registrant.host.clone())),
        }
    }

    /// Installs a store that sweeps `providers` instead of the real ones, which
    /// read the developer's home directory.
    #[cfg(any(test, feature = "test-support"))]
    pub fn install_global_for_test(
        providers: Vec<Arc<dyn agent_sessions::SessionProvider>>,
        cx: &mut App,
    ) -> Entity<Self> {
        let store = cx.new(|_| Self::new(SweepHosts::Here(AgentSessionHost::Local(providers))));
        cx.set_global(GlobalSessionStore(store.clone()));
        store
    }

    /// The store if one has been created, without creating one. For read-only
    /// callers that must not bring the scan into existence.
    pub fn try_global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalSessionStore>()
            .map(|global| global.0.clone())
    }

    fn new(hosts: SweepHosts) -> Self {
        Self {
            index: Arc::new(SessionIndex::empty()),
            hosts,
            error: None,
            unavailable: Vec::new(),
            scanning: None,
            sweeping_through: None,
            rescan_requested: false,
            generation: 0,
            forgotten_during_sweep: HashSet::default(),
        }
    }

    pub fn index(&self) -> &Arc<SessionIndex> {
        &self.index
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn is_scanning(&self) -> bool {
        self.scanning.is_some()
    }

    /// Why the last sweep could not ask the host at all.
    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// The stores the last sweep could not read, and why.
    pub fn unavailable(&self) -> &[(AgentKind, String)] {
        &self.unavailable
    }

    /// Whether `id` was deleted while the sweep now in flight was running, so
    /// anything that remembers it from before the delete must let it go.
    pub fn was_forgotten_during_sweep(&self, id: &str) -> bool {
        self.forgotten_during_sweep.contains(id)
    }

    /// Sweeps the stores on the background executor.
    ///
    /// Calling this while a sweep is running does not start a second one; it
    /// marks the result stale so exactly one more sweep follows. Two concurrent
    /// sweeps would read the same thousands of files twice for one answer.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.scanning.is_some() {
            self.rescan_requested = true;
            return;
        }

        let Some((through, host)) = self.sweep_host(cx) else {
            self.install_listing(
                Err(anyhow::anyhow!("no project on this remote host is open")),
                cx,
            );
            return;
        };
        let listing = host.list(cx);
        self.sweeping_through = through;
        self.scanning = Some(cx.spawn(async move |this, cx| {
            let listing = listing.await;
            this.update(cx, |this, cx| this.install_listing(listing, cx))
                .ok();
        }));
    }

    /// Puts a finished sweep's answer in place.
    ///
    /// A sweep that could not ask the host at all leaves an empty list and the
    /// reason, and asks again only when something asks this store to: a server
    /// too old to answer will not have changed a moment later, and a store that
    /// retried on its own would put a request on the wire for as long as the
    /// window stays open.
    fn install_listing(&mut self, listing: anyhow::Result<SessionListing>, cx: &mut Context<Self>) {
        let forgotten = std::mem::take(&mut self.forgotten_during_sweep);
        let sessions = match listing {
            Ok(listing) => {
                self.error = None;
                self.unavailable = listing.unavailable;
                listing.sessions
            }
            Err(error) => {
                log::warn!("listing agent sessions failed: {error:#}");
                self.error = Some(error.to_string().into());
                self.unavailable = Vec::new();
                Vec::new()
            }
        };
        let sessions = if forgotten.is_empty() {
            sessions
        } else {
            sessions
                .into_iter()
                .filter(|session| !forgotten.contains(&session.id))
                .collect()
        };
        self.index = Arc::new(SessionIndex::new(sessions));
        self.generation += 1;
        self.scanning = None;
        self.sweeping_through = None;
        cx.notify();

        if std::mem::take(&mut self.rescan_requested) {
            self.refresh(cx);
        }
    }

    /// Drops a session that has already been removed from disk.
    ///
    /// Cheaper and more honest than a re-sweep: the caller knows exactly what
    /// it deleted, and a sweep would read every other transcript to learn one
    /// fact it was already told.
    pub fn forget(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.scanning.is_some() {
            self.forgotten_during_sweep.insert(Arc::from(id));
        }
        self.index = Arc::new(self.index.without(id));
        self.generation += 1;
        cx.notify();
        forget_names([Arc::from(id)], cx);
    }

    /// Drops a whole set of sessions that have already been removed from disk.
    ///
    /// One index swap, one generation bump, one notify -- whatever the set
    /// holds. Looping [`Self::forget`] instead would rebuild the index once per
    /// session, which is `O(N * S)` for a bulk delete and re-renders every
    /// reader N times on the way.
    ///
    /// An empty set returns without touching anything: a delete that trashed
    /// nothing is not a change, and bumping the generation for it would redraw
    /// every surface reading the store for no reason.
    pub fn forget_many(&mut self, ids: &HashSet<Arc<str>>, cx: &mut Context<Self>) {
        if ids.is_empty() {
            return;
        }
        if self.scanning.is_some() {
            self.forgotten_during_sweep.extend(ids.iter().cloned());
        }
        self.index = Arc::new(self.index.without_all(ids));
        self.generation += 1;
        cx.notify();
        forget_names(ids.iter().cloned(), cx);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_index_for_test(
        &mut self,
        sessions: Vec<agent_sessions::SessionSummary>,
        cx: &mut Context<Self>,
    ) {
        self.index = Arc::new(SessionIndex::new(sessions));
        self.generation += 1;
        cx.notify();
    }
}

/// A deleted session takes the name it was given with it. Only ever reached
/// after the transcript is gone, which is the one thing `forget` is told.
fn forget_names(ids: impl IntoIterator<Item = Arc<str>>, cx: &mut App) {
    crate::SessionNames::global(cx).update(cx, |names, cx| names.forget_deleted(ids, cx));
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_sessions::{
        AgentCommand, AgentKind, Availability, Deletion, Fork, SessionCounts, SessionProvider,
        SessionSummary,
    };
    use gpui::TestAppContext;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::SystemTime;

    /// Counts sweeps. The whole point of the store is that these stay rare, and
    /// a number is the only way to say so that a later change can falsify.
    struct CountingProvider {
        list_calls: Arc<AtomicUsize>,
        sessions: Vec<SessionSummary>,
    }

    impl SessionProvider for CountingProvider {
        fn agent(&self) -> AgentKind {
            AgentKind::Claude
        }
        fn availability(&self) -> Availability {
            Availability::Ready
        }
        fn list(&self) -> anyhow::Result<Vec<SessionSummary>> {
            self.list_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.sessions.clone())
        }
        fn find(&self, _id: &str) -> anyhow::Result<Option<SessionSummary>> {
            Ok(None)
        }
        fn new_session_command(&self, _id: &str, _cwd: &Path) -> Option<AgentCommand> {
            None
        }
        fn counts(&self, _session: &SessionSummary) -> anyhow::Result<SessionCounts> {
            Ok(SessionCounts::default())
        }
        fn resume_command(&self, _s: &SessionSummary, _f: Fork) -> Option<AgentCommand> {
            None
        }
        fn deletion(&self, _session: &SessionSummary) -> Deletion {
            Deletion::Nothing
        }
    }

    fn session(id: &str, cwd: &str) -> SessionSummary {
        SessionSummary {
            id: Arc::from(id),
            agent: AgentKind::Claude,
            title: id.to_string(),
            preview: String::new(),
            preview_speaker: None,
            cwd: PathBuf::from(cwd),
            branch: None,
            model: None,
            updated_at: SystemTime::UNIX_EPOCH,
            log_path: None,
            log_bytes: 0,
        }
    }

    fn store(cx: &mut TestAppContext) -> (Entity<SessionStore>, Arc<AtomicUsize>) {
        let list_calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(CountingProvider {
            list_calls: list_calls.clone(),
            sessions: vec![session("a", "/repo/main"), session("b", "/repo/feature")],
        });
        let store = cx
            .new(|_| SessionStore::new(SweepHosts::Here(AgentSessionHost::Local(vec![provider]))));
        (store, list_calls)
    }

    /// Ten callers asking while a sweep is running must cost one more sweep,
    /// not ten. Each sweep opens every transcript on disk; queueing them would
    /// turn a busy moment into a stall.
    #[gpui::test]
    async fn refreshes_during_a_sweep_coalesce_into_one(cx: &mut TestAppContext) {
        let (store, list_calls) = store(cx);

        store.update(cx, |store, cx| {
            for _ in 0..10 {
                store.refresh(cx);
            }
        });
        cx.run_until_parked();

        assert_eq!(
            list_calls.load(Ordering::SeqCst),
            2,
            "the first sweep, plus exactly one re-run for everything that asked during it"
        );
    }

    /// Reading is what a render does, and a render must never reach the disk.
    #[gpui::test]
    async fn reading_the_index_never_sweeps(cx: &mut TestAppContext) {
        let (store, list_calls) = store(cx);
        store.update(cx, |store, cx| store.refresh(cx));
        cx.run_until_parked();
        let after_first_sweep = list_calls.load(Ordering::SeqCst);

        store.read_with(cx, |store, _| {
            for _ in 0..100 {
                let found: Vec<_> = store
                    .index()
                    .sessions_for(Path::new("/repo/main"))
                    .collect();
                assert_eq!(found.len(), 1);
            }
        });

        assert_eq!(
            list_calls.load(Ordering::SeqCst),
            after_first_sweep,
            "a hundred lookups must not read the disk once"
        );
    }

    /// A delete already knows what it removed. Re-sweeping to learn it would
    /// read every other transcript for one fact.
    #[gpui::test]
    async fn forgetting_a_session_does_not_sweep(cx: &mut TestAppContext) {
        let (store, list_calls) = store(cx);
        store.update(cx, |store, cx| store.refresh(cx));
        cx.run_until_parked();
        let before = list_calls.load(Ordering::SeqCst);

        store.update(cx, |store, cx| store.forget("a", cx));

        assert_eq!(list_calls.load(Ordering::SeqCst), before);
        store.read_with(cx, |store, _| {
            assert_eq!(store.index().len(), 1);
            assert!(
                store
                    .index()
                    .sessions_for(Path::new("/repo/main"))
                    .next()
                    .is_none()
            );
        });
    }

    /// The bulk drop must cost ONE rebuild, not one per session.
    ///
    /// `generation` is what makes that observable: an implementation that loops
    /// `forget` bumps it once per id and fails this assertion, so the O(n^2)
    /// trap cannot creep back in behind a passing suite.
    #[gpui::test]
    async fn forgetting_many_costs_one_rebuild_and_no_sweep(cx: &mut TestAppContext) {
        let (store, list_calls) = store(cx);
        store.update(cx, |store, cx| store.refresh(cx));
        cx.run_until_parked();
        let sweeps_before = list_calls.load(Ordering::SeqCst);
        let generation_before = store.read_with(cx, |store, _| store.generation());

        let ids: HashSet<Arc<str>> = ["a", "b"].into_iter().map(Arc::from).collect();
        store.update(cx, |store, cx| store.forget_many(&ids, cx));

        assert_eq!(
            list_calls.load(Ordering::SeqCst),
            sweeps_before,
            "forgetting what we just deleted must not re-read the disk"
        );
        store.read_with(cx, |store, _| {
            assert_eq!(store.index().len(), 0);
            assert_eq!(
                store.generation(),
                generation_before + 1,
                "one bulk drop is one rebuild; a loop over forget would bump this twice"
            );
        });
    }

    /// A sweep that read the disk before a delete finishes after it. Its listing
    /// still names the deleted session, and installing it as-is would put the
    /// row back until the next manual refresh.
    #[gpui::test]
    async fn a_sweep_in_flight_during_a_delete_does_not_bring_the_row_back(
        cx: &mut TestAppContext,
    ) {
        let (store, _) = store(cx);
        store.update(cx, |store, cx| {
            store.set_index_for_test(
                vec![session("a", "/repo/main"), session("b", "/repo/feature")],
                cx,
            );
            store.refresh(cx);
            store.forget("a", cx);
        });
        cx.run_until_parked();

        store.read_with(cx, |store, _| {
            let ids: Vec<&str> = store
                .index()
                .sessions()
                .iter()
                .map(|session| session.id.as_ref())
                .collect();
            assert_eq!(
                ids,
                vec!["b"],
                "a sweep that started before the delete must not resurrect the session"
            );
        });
    }

    /// The bulk drop has the same race as the single one.
    #[gpui::test]
    async fn a_sweep_in_flight_during_a_bulk_delete_does_not_bring_the_rows_back(
        cx: &mut TestAppContext,
    ) {
        let (store, _) = store(cx);
        store.update(cx, |store, cx| {
            store.set_index_for_test(
                vec![session("a", "/repo/main"), session("b", "/repo/feature")],
                cx,
            );
            store.refresh(cx);
            let ids: HashSet<Arc<str>> = ["a", "b"].into_iter().map(Arc::from).collect();
            store.forget_many(&ids, cx);
        });
        cx.run_until_parked();

        store.read_with(cx, |store, _| {
            assert_eq!(store.index().len(), 0);
            assert!(
                !store.was_forgotten_during_sweep("a"),
                "the record covers one sweep and must be empty once it lands"
            );
        });
    }

    /// A delete that trashed nothing is not a change. Bumping the generation
    /// for it would redraw every surface reading the store for no reason.
    #[gpui::test]
    async fn forgetting_nothing_changes_nothing(cx: &mut TestAppContext) {
        let (store, _) = store(cx);
        store.update(cx, |store, cx| store.refresh(cx));
        cx.run_until_parked();

        // The entity IS its subscription list, so the observe stays registered
        // for as long as `_observer` is held -- the surrounding tests drop the
        // subscription at the end of the closure, which would silently observe
        // nothing here.
        let notifications = Arc::new(AtomicUsize::new(0));
        let _observer = cx.new(|cx| {
            let notifications = notifications.clone();
            vec![
                cx.observe(&store, move |_: &mut Vec<gpui::Subscription>, _, _| {
                    notifications.fetch_add(1, Ordering::SeqCst);
                }),
            ]
        });
        let generation_before = store.read_with(cx, |store, _| store.generation());
        let len_before = store.read_with(cx, |store, _| store.index().len());

        store.update(cx, |store, cx| store.forget_many(&HashSet::new(), cx));
        cx.run_until_parked();

        store.read_with(cx, |store, _| {
            assert_eq!(store.generation(), generation_before);
            assert_eq!(store.index().len(), len_before);
        });
        assert_eq!(
            notifications.load(Ordering::SeqCst),
            0,
            "an empty forget must not notify"
        );
    }

    /// A test that needs the global must be able to give it providers of its own:
    /// the real ones read the developer's home directory.
    #[gpui::test]
    async fn an_installed_store_sweeps_only_the_providers_it_was_given(cx: &mut TestAppContext) {
        let list_calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(CountingProvider {
            list_calls: list_calls.clone(),
            sessions: vec![session("only", "/repo/main")],
        });
        let installed = cx.update(|cx| SessionStore::install_global_for_test(vec![provider], cx));

        let global = cx.update(|cx| SessionStore::global(cx));
        assert_eq!(global.entity_id(), installed.entity_id());
        assert_eq!(installed.read_with(cx, |store, _| store.generation()), 0);

        installed.update(cx, |store, cx| store.refresh(cx));
        cx.run_until_parked();

        assert_eq!(list_calls.load(Ordering::SeqCst), 1);
        installed.read_with(cx, |store, _| {
            assert_eq!(store.generation(), 1);
            assert_eq!(store.index().len(), 1);
            assert!(store.index().find("only").is_some());
        });
    }

    fn counting_host(sessions: Vec<SessionSummary>) -> (AgentSessionHost, Arc<AtomicUsize>) {
        let list_calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(CountingProvider {
            list_calls: list_calls.clone(),
            sessions,
        });
        (AgentSessionHost::Local(vec![provider]), list_calls)
    }

    async fn test_project(cx: &mut TestAppContext) -> Entity<Project> {
        cx.update(|cx| {
            if !cx.has_global::<settings::SettingsStore>() {
                let settings_store = settings::SettingsStore::test(cx);
                cx.set_global(settings_store);
            }
        });
        let fs = fs::FakeFs::new(cx.executor());
        Project::test(fs, Vec::<&Path>::new(), cx).await
    }

    /// Every project on one remote host reads the same home, so they share one
    /// store and one sweep: a second window on the same distro must not read
    /// its thousands of transcripts again. A second host is a second store.
    #[gpui::test]
    async fn two_projects_on_one_host_share_one_store_and_one_sweep(cx: &mut TestAppContext) {
        let first_project = test_project(cx).await;
        let second_project = test_project(cx).await;
        let (host, list_calls) = counting_host(vec![session("a", "/home/u/p")]);
        let wsl = RemoteConnectionIdentity::Mock { id: 7 };

        let first = cx.update(|cx| {
            SessionStore::for_remote_host(wsl.clone(), &first_project, host.clone(), cx)
        });
        let second = cx.update(|cx| {
            SessionStore::for_remote_host(wsl.clone(), &second_project, host.clone(), cx)
        });
        first.update(cx, |store, cx| store.refresh(cx));
        cx.run_until_parked();

        assert_eq!(first.entity_id(), second.entity_id());
        assert_eq!(list_calls.load(Ordering::SeqCst), 1);
        assert_eq!(second.read_with(cx, |store, _| store.index().len()), 1);

        let (other_host, other_calls) = counting_host(Vec::new());
        let ssh = RemoteConnectionIdentity::Mock { id: 8 };
        let other =
            cx.update(|cx| SessionStore::for_remote_host(ssh, &first_project, other_host, cx));
        assert_ne!(other.entity_id(), first.entity_id());
        let global = cx.update(|cx| SessionStore::global(cx));
        assert_ne!(global.entity_id(), first.entity_id());
        assert_eq!(other_calls.load(Ordering::SeqCst), 0);
    }

    /// Each project on a host has a connection of its own. Closing one must
    /// leave the sweep asking through a project still open -- not through the
    /// connection that went away with the closed one, whether that was the
    /// last project to open the store or the first.
    #[gpui::test]
    async fn a_sweep_asks_through_a_project_still_open_on_the_host(cx: &mut TestAppContext) {
        let first_project = test_project(cx).await;
        let second_project = test_project(cx).await;
        let third_project = test_project(cx).await;
        let (first_host, first_calls) = counting_host(vec![session("a", "/home/u/p")]);
        let (second_host, second_calls) = counting_host(vec![session("a", "/home/u/p")]);
        let (third_host, third_calls) = counting_host(vec![session("a", "/home/u/p")]);
        let wsl = RemoteConnectionIdentity::Mock { id: 9 };
        let store = cx.update(|cx| {
            SessionStore::for_remote_host(wsl.clone(), &first_project, first_host, cx)
        });
        cx.update(|cx| {
            SessionStore::for_remote_host(wsl.clone(), &second_project, second_host, cx)
        });
        cx.update(|cx| SessionStore::for_remote_host(wsl.clone(), &third_project, third_host, cx));
        let calls = || {
            [&first_calls, &second_calls, &third_calls].map(|calls| calls.load(Ordering::SeqCst))
        };
        let sweep = |cx: &mut TestAppContext| {
            store.update(cx, |store, cx| store.refresh(cx));
            cx.run_until_parked();
        };

        // A dropped entity is released when effects next flush.
        drop(third_project);
        cx.update(|_| {});
        sweep(cx);
        assert_eq!(calls(), [1, 0, 0], "the last to register has closed");

        drop(first_project);
        cx.update(|_| {});
        sweep(cx);
        assert_eq!(calls(), [1, 1, 0], "the first to register has closed");
        store.read_with(cx, |store, _| {
            assert_eq!(store.index().len(), 1);
            assert!(store.error().is_none());
        });

        drop(second_project);
        cx.update(|_| {});
        sweep(cx);
        assert_eq!(calls(), [1, 1, 0]);
        store.read_with(cx, |store, _| {
            assert!(
                !store.is_scanning(),
                "with no project left to ask, a refresh must end, not wait"
            );
            assert!(store.error().is_some());
        });
    }

    /// A project still open whose connection has died must not be the one a
    /// sweep asks through while another window on the host still answers.
    #[gpui::test]
    async fn a_sweep_passes_over_a_project_whose_connection_is_gone(cx: &mut TestAppContext) {
        let first_project = test_project(cx).await;
        let second_project = test_project(cx).await;
        let (first_host, first_calls) = counting_host(vec![session("a", "/home/u/p")]);
        let (second_host, second_calls) = counting_host(vec![session("a", "/home/u/p")]);
        let wsl = RemoteConnectionIdentity::Mock { id: 10 };
        let store = cx.update(|cx| {
            SessionStore::for_remote_host(wsl.clone(), &first_project, first_host, cx)
        });
        cx.update(|cx| SessionStore::for_remote_host(wsl, &second_project, second_host, cx));

        first_project.update(cx, |project, cx| {
            project.mark_as_collab_for_testing();
            project.disconnected_from_host(cx);
        });
        store.update(cx, |store, cx| store.refresh(cx));
        cx.run_until_parked();

        assert_eq!(first_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            second_calls.load(Ordering::SeqCst),
            1,
            "the sweep must ask through the connection that still answers"
        );
    }

    /// The project a sweep is asking through closes mid-sweep: its connection
    /// is gone, so the answer it was waiting for never comes. The sweep starts
    /// over through the next project instead of waiting out its timeout.
    #[gpui::test]
    async fn a_sweep_whose_project_closes_starts_over_through_the_next(cx: &mut TestAppContext) {
        let first_project = test_project(cx).await;
        let second_project = test_project(cx).await;
        let (first_host, _first_calls) = counting_host(vec![session("a", "/home/u/p")]);
        let (second_host, second_calls) = counting_host(vec![session("a", "/home/u/p")]);
        let wsl = RemoteConnectionIdentity::Mock { id: 11 };
        let store = cx.update(|cx| {
            SessionStore::for_remote_host(wsl.clone(), &first_project, first_host, cx)
        });
        cx.update(|cx| SessionStore::for_remote_host(wsl, &second_project, second_host, cx));

        store.update(cx, |store, cx| store.refresh(cx));
        drop(first_project);
        cx.update(|_| {});
        cx.run_until_parked();

        assert_eq!(
            second_calls.load(Ordering::SeqCst),
            1,
            "a sweep through a closed project must start over through one still open"
        );
        store.read_with(cx, |store, _| {
            assert!(!store.is_scanning());
            assert_eq!(store.index().len(), 1);
        });
    }

    /// A project on this machine, and a collaboration guest's, read this
    /// machine's own stores: both get the process-wide one.
    #[gpui::test]
    async fn a_project_not_on_a_remote_host_gets_the_process_wide_store(cx: &mut TestAppContext) {
        let local = test_project(cx).await;
        let guest = test_project(cx).await;
        guest.update(cx, |project, _| project.mark_as_collab_for_testing());

        let global = cx.update(|cx| SessionStore::global(cx));
        let for_local = cx.update(|cx| SessionStore::for_project(&local, cx));
        let for_guest = cx.update(|cx| SessionStore::for_project(&guest, cx));

        assert_eq!(for_local.entity_id(), global.entity_id());
        assert_eq!(for_guest.entity_id(), global.entity_id());
        assert!(cx.update(|cx| {
            cx.try_global::<RemoteSessionStores>()
                .is_none_or(|stores| stores.0.is_empty())
        }));
    }

    /// A sweep that could not ask the host shows nothing and says why, and
    /// nothing asks again until someone does.
    #[gpui::test]
    async fn a_failed_sweep_empties_the_list_names_the_reason_and_does_not_retry(
        cx: &mut TestAppContext,
    ) {
        let (store, list_calls) = store(cx);
        store.update(cx, |store, cx| store.refresh(cx));
        cx.run_until_parked();
        let sweeps = list_calls.load(Ordering::SeqCst);

        store.update(cx, |store, cx| {
            store.install_listing(
                Err(anyhow::anyhow!(
                    project::agent_session_host::REMOTE_SESSIONS_OUT_OF_DATE
                )),
                cx,
            )
        });
        cx.run_until_parked();

        store.read_with(cx, |store, _| {
            assert_eq!(store.index().len(), 0);
            assert_eq!(
                store.error().map(|error| error.as_ref()),
                Some(project::agent_session_host::REMOTE_SESSIONS_OUT_OF_DATE)
            );
            assert!(!store.is_scanning());
        });
        assert_eq!(list_calls.load(Ordering::SeqCst), sweeps, "no retry");

        store.update(cx, |store, cx| store.refresh(cx));
        cx.run_until_parked();
        store.read_with(cx, |store, _| {
            assert!(store.error().is_none(), "a sweep that answers clears it");
            assert_eq!(store.index().len(), 2);
        });
    }

    /// The store outlives every panel that reads it. If it ever holds one, the
    /// panel can never be dropped -- this is the test that says so.
    #[gpui::test]
    async fn the_store_holds_nothing_that_would_outlive_a_reader(cx: &mut TestAppContext) {
        let (store, _) = store(cx);

        let reader = cx.new(|cx| {
            let _subscription = cx.observe(&store, |_: &mut usize, _, _| {});
            0usize
        });
        let weak = reader.downgrade();
        drop(reader);
        cx.run_until_parked();

        assert!(
            weak.upgrade().is_none(),
            "observing the store must not keep the observer alive"
        );
    }
}
