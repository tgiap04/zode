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

use std::sync::Arc;

use agent_sessions::{SessionIndex, SessionProvider};
use gpui::{App, AppContext as _, Context, Entity, Global, Task};

/// Handle to the process-wide store.
struct GlobalSessionStore(Entity<SessionStore>);

impl Global for GlobalSessionStore {}

pub struct SessionStore {
    index: Arc<SessionIndex>,
    providers: Vec<Arc<dyn SessionProvider>>,
    /// The sweep in flight. Held in a field rather than detached so closing the
    /// app drops it -- a detached background scan outlives everything that
    /// wanted its answer.
    scanning: Option<Task<()>>,
    /// A refresh that arrived while a sweep was already running. Coalesced to a
    /// single re-run rather than queued: ten requests during one sweep must
    /// cost one more sweep, not ten.
    rescan_requested: bool,
    /// Bumped on every completed sweep. Consumers that cache anything derived
    /// from the index compare this instead of comparing the index itself.
    generation: u64,
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
        let store = cx.new(|_| Self::new(agent_sessions::default_providers()));
        cx.set_global(GlobalSessionStore(store.clone()));
        store
    }

    /// The store if one has been created, without creating one. For read-only
    /// callers that must not bring the scan into existence.
    pub fn try_global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalSessionStore>()
            .map(|global| global.0.clone())
    }

    fn new(providers: Vec<Arc<dyn SessionProvider>>) -> Self {
        Self {
            index: Arc::new(SessionIndex::empty()),
            providers,
            scanning: None,
            rescan_requested: false,
            generation: 0,
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

        let providers = self.providers.clone();
        self.scanning = Some(cx.spawn(async move |this, cx| {
            let sessions = cx
                .background_spawn(async move { agent_sessions::list_all(&providers) })
                .await;

            this.update(cx, |this, cx| {
                this.index = Arc::new(SessionIndex::new(sessions));
                this.generation += 1;
                this.scanning = None;
                cx.notify();

                if std::mem::take(&mut this.rescan_requested) {
                    this.refresh(cx);
                }
            })
            .ok();
        }));
    }

    /// Drops a session that has already been removed from disk.
    ///
    /// Cheaper and more honest than a re-sweep: the caller knows exactly what
    /// it deleted, and a sweep would read every other transcript to learn one
    /// fact it was already told.
    pub fn forget(&mut self, id: &str, cx: &mut Context<Self>) {
        self.index = Arc::new(self.index.without(id));
        self.generation += 1;
        cx.notify();
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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_sessions::{
        AgentKind, Availability, Fork, ResumeCommand, SessionCounts, SessionSummary,
    };
    use gpui::TestAppContext;
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
        fn new_session_command(&self, _id: &str, _cwd: &Path) -> Option<ResumeCommand> {
            None
        }
        fn counts(&self, _session: &SessionSummary) -> anyhow::Result<SessionCounts> {
            Ok(SessionCounts::default())
        }
        fn resume_command(&self, _s: &SessionSummary, _f: Fork) -> Option<ResumeCommand> {
            None
        }
        fn paths_to_trash(&self, _session: &SessionSummary) -> Vec<PathBuf> {
            Vec::new()
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
        let store = cx.new(|_| SessionStore::new(vec![provider]));
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
