use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, Global, SharedString, Subscription, Task,
    WeakEntity,
};
use project::{AgentId, Project};
use util::command::CommandHost;
use workspace::{RemoteConnectionIdentity, StatusBarSettings, item::Settings as _};

use crate::{Outcome, SourceState, UNAVAILABLE_FOR_CONNECTION, claude, codex};

/// How often the quota is re-read while some window has the user's attention.
///
/// Claude's endpoint offers no push, so polling is the only way its numbers stay
/// current. A minute is short enough that a reset is noticed soon after it
/// happens and long enough that it is not traffic worth thinking about.
const POLL_INTERVAL: Duration = Duration::from_secs(60);

/// How recent an attempt has to be for a window regaining focus to trust it
/// rather than ask again.
///
/// Regaining focus used to fetch unconditionally, so alt-tabbing in and out five
/// times was five requests in a few seconds — against an undocumented endpoint
/// this editor shares with the Claude Code CLI on one token. A manual refresh is
/// never throttled: the whole point of pressing it is to distrust what is on
/// screen.
const ACTIVATION_MIN_INTERVAL: Duration = Duration::from_secs(30);

/// Whether an ask came from a person or from a window regaining focus, which
/// decides whether it may be skipped.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PollReason {
    /// A window regained focus. Skippable when an attempt was made recently.
    Activation,
    /// Someone pressed refresh. Never skipped.
    Manual,
}

struct GlobalAgentUsageStore(Entity<AgentUsageStore>);

impl Global for GlobalAgentUsageStore {}

/// One store per remote host, by the identity of its connection.
///
/// Keyed by host rather than by project: every project on a WSL distro or an
/// SSH host is spent from the account its CLIs are signed into there, so a
/// store per project would poll one account once per window. Bounded by the
/// hosts this app has opened a project on in this session.
#[derive(Default)]
struct RemoteAgentUsageStores(HashMap<RemoteConnectionIdentity, Entity<AgentUsageStore>>);

impl Global for RemoteAgentUsageStores {}

/// The one store for every project whose host this app cannot run anything
/// on. It never reads anything.
struct UnreachableAgentUsageStore(Entity<AgentUsageStore>);

impl Global for UnreachableAgentUsageStore {}

/// Where a store reads its quota.
enum UsageHost {
    /// This machine: its keychain, its home directory, its `codex`.
    ThisMachine,
    /// A WSL distro, an SSH host or a dev container, asked through whichever
    /// of its open projects is still connected.
    Remote {
        /// What the host is shown as in a reason.
        label: SharedString,
        /// Oldest first. A project leaves the moment it is released, so a
        /// read never waits on a connection whose window has closed.
        projects: Vec<RemoteProject>,
    },
    /// A project reached through the relay, whose host is a device of its
    /// own: it cannot be asked from here, and this machine's account would be
    /// the wrong one to show for it.
    Unreachable,
}

struct RemoteProject {
    project: EntityId,
    /// Weak so the store never keeps a closed window's project alive.
    handle: WeakEntity<Project>,
    _released: Subscription,
}

/// What one refresh asks.
enum FetchTarget {
    ThisMachine,
    Host {
        label: SharedString,
        host: Task<anyhow::Result<Arc<dyn CommandHost>>>,
    },
    /// A remote store with no connected project to ask through.
    NoConnection,
}

/// The quota of one machine's account, read once for the whole application.
///
/// A quota belongs to an account, not to a project: every indicator for a
/// project on the same machine is showing the same token's numbers. A project
/// on a remote host is spent from the account its CLIs are signed into there,
/// so each such host has a store of its own (see [`Self::for_project`]).
///
/// This used to live on the indicator, which the status bar builds once per
/// `Workspace` — and this fork keeps a workspace per project inside one
/// window, so a user with eight projects open ran eight poll loops against one
/// endpoint on one token, each with its own retry chain. Every throttle in this
/// crate was written per indicator and so could not see the other seven.
///
/// One store per machine, one loop per store, however many indicators observe
/// it.
pub(crate) struct AgentUsageStore {
    host: UsageHost,
    /// Fixed order, so the numbers do not swap places between reads.
    sources: [SourceState; 2],
    /// A fetch is in flight. Guards a click so pressing twice does not queue a
    /// second request behind the first.
    fetching: bool,
    /// When a read was last *attempted*, whatever came back.
    ///
    /// Deliberately not "when a read last succeeded": the question the throttle
    /// asks is "have I already asked recently", and answering it with success
    /// would invert the whole thing. `Outcome::Keep` never sets a source's
    /// `fetched_at` and `Outcome::Clear` nulls it, so a persistent 429 would
    /// leave every activation unthrottled, and each one now costs a retry chain
    /// rather than one request.
    last_polled_at: Option<DateTime<Utc>>,
    /// The interval loop, held so it stops when no window is active.
    _poll: Option<Task<()>>,
    _fetch: Option<Task<()>>,
}

impl AgentUsageStore {
    /// The store, created on first ask.
    ///
    /// Lazily rather than from an `init`: the indicator is the only thing that
    /// wants it, it is built long after the app starts, and a store nobody asked
    /// for would poll on behalf of nobody.
    pub(crate) fn global(cx: &mut App) -> Entity<Self> {
        if let Some(global) = cx.try_global::<GlobalAgentUsageStore>() {
            return global.0.clone();
        }
        let store = cx.new(|_| Self::new());
        cx.set_global(GlobalAgentUsageStore(store.clone()));
        store
    }

    /// The store for the account `project`'s agents spend: this machine's for
    /// a local project, its host's for a remote one.
    ///
    /// Never this machine's for a project that lives elsewhere. A project
    /// whose host cannot be asked gets a store that says so instead.
    pub(crate) fn for_project(project: &Entity<Project>, cx: &mut App) -> Entity<Self> {
        let remote = {
            let project = project.read(cx);
            // A guest's agents run here, on the guest's own machine, so the
            // account they spend is this one.
            if project.is_local() || project.is_via_collab() {
                return Self::global(cx);
            }
            project
                .remote_connection_options(cx)
                .filter(|_| project.is_via_remote_server())
                .map(|options| {
                    (
                        workspace::remote_connection_identity(&options),
                        options.display_name(),
                    )
                })
                .filter(|(identity, _)| !matches!(identity, RemoteConnectionIdentity::Relay { .. }))
        };
        match remote {
            Some((identity, label)) => Self::for_remote_host(identity, label.into(), project, cx),
            None => Self::unreachable(cx),
        }
    }

    fn for_remote_host(
        identity: RemoteConnectionIdentity,
        label: SharedString,
        project: &Entity<Project>,
        cx: &mut App,
    ) -> Entity<Self> {
        let existing = cx
            .default_global::<RemoteAgentUsageStores>()
            .0
            .get(&identity)
            .cloned();
        let store = match existing {
            Some(store) => store,
            None => {
                let store = cx.new(|_| {
                    Self::with_host(UsageHost::Remote {
                        label,
                        projects: Vec::new(),
                    })
                });
                cx.default_global::<RemoteAgentUsageStores>()
                    .0
                    .insert(identity, store.clone());
                store
            }
        };
        store.update(cx, |store, cx| store.register(project, cx));
        store
    }

    fn unreachable(cx: &mut App) -> Entity<Self> {
        if let Some(global) = cx.try_global::<UnreachableAgentUsageStore>() {
            return global.0.clone();
        }
        let store = cx.new(|_| {
            let mut store = Self::with_host(UsageHost::Unreachable);
            let now = Utc::now();
            for source in &mut store.sources {
                source.apply(Outcome::Clear(UNAVAILABLE_FOR_CONNECTION.into()), now);
            }
            store
        });
        cx.set_global(UnreachableAgentUsageStore(store.clone()));
        store
    }

    fn register(&mut self, project: &Entity<Project>, cx: &mut Context<Self>) {
        let UsageHost::Remote { projects, .. } = &mut self.host else {
            return;
        };
        let project_id = project.entity_id();
        if projects.iter().any(|known| known.project == project_id) {
            return;
        }
        let released = cx.observe_release(project, move |this, _, _| {
            if let UsageHost::Remote { projects, .. } = &mut this.host {
                projects.retain(|known| known.project != project_id);
                // Nobody left to show this host's numbers to.
                if projects.is_empty() {
                    this._poll = None;
                }
            }
        });
        projects.push(RemoteProject {
            project: project_id,
            handle: project.downgrade(),
            _released: released,
        });
    }

    /// What the next refresh asks, and through which connection.
    fn fetch_target(&self, cx: &mut App) -> Option<FetchTarget> {
        match &self.host {
            UsageHost::ThisMachine => Some(FetchTarget::ThisMachine),
            UsageHost::Unreachable => None,
            UsageHost::Remote { label, projects } => {
                let connected = projects.iter().find_map(|known| {
                    known
                        .handle
                        .upgrade()
                        .filter(|project| !project.read(cx).is_disconnected(cx))
                });
                Some(match connected {
                    Some(project) => FetchTarget::Host {
                        label: label.clone(),
                        host: project.update(cx, |project, cx| project.command_host(cx)),
                    },
                    None => FetchTarget::NoConnection,
                })
            }
        }
    }

    /// Whether this store speaks for an account on some other machine.
    pub(crate) fn speaks_for_another_machine(&self) -> bool {
        !matches!(self.host, UsageHost::ThisMachine)
    }

    /// A store for the host named `label`, with no project and no tasks.
    #[cfg(test)]
    pub(crate) fn test_for_host(label: &str) -> Self {
        Self::with_host(UsageHost::Remote {
            label: SharedString::from(label.to_string()),
            projects: Vec::new(),
        })
    }

    /// The host's name for reasons, `None` for this machine.
    #[cfg(test)]
    pub(crate) fn host_label(&self) -> Option<SharedString> {
        match &self.host {
            UsageHost::Remote { label, .. } => Some(label.clone()),
            UsageHost::ThisMachine | UsageHost::Unreachable => None,
        }
    }

    /// A store with fixed state and no tasks.
    ///
    /// Tests about `apply` and about what is drawn must not start a poll loop:
    /// that would put a real HTTP request and a real subprocess behind a unit
    /// test, which is slow, machine-dependent, and not what is being asserted.
    #[cfg(test)]
    pub(crate) fn test_new() -> Self {
        Self::new()
    }

    /// The sources, for a test that asserts on one directly.
    #[cfg(test)]
    pub(crate) fn source_at(&self, index: usize) -> &SourceState {
        &self.sources[index]
    }

    /// Plants a loop handle, so a test can assert what drops it.
    #[cfg(test)]
    pub(crate) fn plant_poll_task(&mut self, task: Task<()>) {
        self._poll = Some(task);
    }

    /// Whether a loop is held.
    #[cfg(test)]
    pub(crate) fn is_polling(&self) -> bool {
        self._poll.is_some()
    }

    /// Marks a fetch in flight, for a test asserting that `apply` clears it.
    #[cfg(test)]
    pub(crate) fn mark_fetching(&mut self) {
        self.fetching = true;
    }

    /// Stamps the last attempt, for a test driving the throttle.
    #[cfg(test)]
    pub(crate) fn set_last_polled_at(&mut self, at: Option<DateTime<Utc>>) {
        self.last_polled_at = at;
    }

    fn new() -> Self {
        Self::with_host(UsageHost::ThisMachine)
    }

    fn with_host(host: UsageHost) -> Self {
        Self {
            host,
            sources: [
                SourceState::new(AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string())),
                SourceState::new(AgentId::new(project::CODEX_AGENT_ID.to_string())),
            ],
            fetching: false,
            last_polled_at: None,
            _poll: None,
            _fetch: None,
        }
    }

    /// Whether this build is allowed to read a quota at all.
    ///
    /// Reading one means shelling out to the OS keychain, reading the user's home
    /// directory, calling an HTTP endpoint and spawning a CLI. That is the right
    /// behaviour in the editor and the wrong behaviour underneath a test of
    /// something else: it is real I/O the deterministic scheduler cannot account
    /// for, and it made every test that opens a workspace fail.
    ///
    /// Gated on the feature rather than on `cfg(test)`, because the tests that
    /// were failing live in another crate — `cfg(test)` is only ever set for the
    /// crate under test, so it would not have reached them.
    const fn may_read_usage() -> bool {
        !cfg!(feature = "test-support")
    }

    /// A window gained focus.
    ///
    /// Every indicator in that window reports the same activation, so this is
    /// called once per indicator for one event. The throttle below is what
    /// collapses those into a single request.
    pub(crate) fn window_activated(&mut self, cx: &mut Context<Self>) {
        self.start_polling(PollReason::Activation, cx);
    }

    /// A window lost focus.
    ///
    /// Asks gpui which window is active rather than keeping a set of them here.
    /// A set would go stale: closing a window fires no deactivation — the
    /// observers run from the platform's active-status change alone — so a
    /// window closed while active would leave its id behind for ever and the
    /// application would keep polling on behalf of a window nobody has.
    ///
    /// Moving between two windows keeps polling, because the other one is active
    /// by the time this runs.
    pub(crate) fn window_deactivated(&mut self, cx: &mut Context<Self>) {
        if !Self::should_keep_polling(cx) {
            self._poll = None;
        }
    }

    /// Whether anyone is still looking.
    ///
    /// The single question both the deactivation path and the loop's own tick
    /// ask, so the two cannot drift apart on what "still wanted" means.
    pub(crate) fn should_keep_polling(cx: &App) -> bool {
        cx.active_window().is_some()
    }

    /// A person pressed refresh.
    pub(crate) fn refresh_now(&mut self, cx: &mut Context<Self>) {
        self.start_polling(PollReason::Manual, cx);
    }

    /// (Re)starts the interval loop, fetching first unless the ask may be skipped.
    ///
    /// Assigning over `_poll` drops any previous loop, so this can be called
    /// freely — on activation, or after a manual refresh — without stacking
    /// timers.
    fn start_polling(&mut self, reason: PollReason, cx: &mut Context<Self>) {
        if !Self::may_read_usage() || matches!(self.host, UsageHost::Unreachable) {
            return;
        }
        if !self.should_fetch(reason, Utc::now()) {
            // Still ensure a loop exists: it was dropped when the last window lost
            // focus, so without this the numbers would never refresh again.
            if self._poll.is_none() {
                self.restart_timer(cx);
            }
            return;
        }
        self.refresh(cx);
        self.restart_timer(cx);
    }

    /// Whether an ask should become a request.
    ///
    /// Pure, and separate from `refresh`, because this is the decision that keeps
    /// eight indicators from becoming eight requests — and the only part of that
    /// claim provable without a network.
    pub(crate) fn should_fetch(&self, reason: PollReason, now: DateTime<Utc>) -> bool {
        if self.fetching {
            return false;
        }
        if reason == PollReason::Manual {
            return true;
        }
        let Ok(cutoff) = chrono::Duration::from_std(ACTIVATION_MIN_INTERVAL) else {
            return true;
        };
        !self
            .last_polled_at
            .is_some_and(|polled_at| now - polled_at < cutoff)
    }

    /// The interval loop on its own, with no immediate fetch.
    fn restart_timer(&mut self, cx: &mut Context<Self>) {
        self._poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                // The backstop for a window closed while active, which fires no
                // deactivation for anyone to notice. An activation starts a
                // fresh loop, so returning here costs nothing but the tick.
                if !cx.update(|cx| Self::should_keep_polling(cx)) {
                    return;
                }
                let carry_on = this.update(cx, |this, cx| this.refresh(cx)).is_ok();
                if !carry_on {
                    return;
                }
            }
        }));
    }

    /// Which agents a refresh should actually ask.
    pub(crate) fn agents_to_fetch(settings: &StatusBarSettings) -> (bool, bool) {
        (settings.claude_usage_button, settings.codex_usage_button)
    }

    /// What a switched-off agent's source becomes instead of a real fetch.
    ///
    /// `Clear`, not `Keep`: it nulls the source's windows along with skipping the
    /// request, releasing the memory the same way an entitlement failure does.
    pub(crate) fn disabled_outcome() -> Outcome {
        Outcome::Clear("switched off in status bar settings".into())
    }

    /// Reads both agents' quota once, concurrently.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.fetching {
            return;
        }

        let (claude_on, codex_on) = Self::agents_to_fetch(StatusBarSettings::get_global(cx));
        if !claude_on && !codex_on {
            // Returning before `fetching` and `last_polled_at` are stamped is
            // load-bearing: stamping them and then never landing an `apply` to
            // clear `fetching` would wedge the store for good.
            return;
        }

        let Some(target) = self.fetch_target(cx) else {
            return;
        };

        self.fetching = true;
        // Stamped here rather than on the answer: this is the moment a request
        // goes out, and it is requests the throttle is trying not to duplicate.
        self.last_polled_at = Some(Utc::now());
        cx.notify();

        let http_client = cx.http_client();
        let executor = cx.background_executor().clone();
        self._fetch = Some(cx.spawn(async move |this, cx| {
            let (claude_outcome, codex_outcome) =
                read_both(target, claude_on, codex_on, http_client, executor).await;

            this.update(cx, |this, cx| {
                this.apply(claude_outcome, codex_outcome, Utc::now());
                cx.notify();
            })
            .ok();
        }));
    }

    /// Folds both outcomes into the displayed state.
    ///
    /// Pure and separate from the fetching on purpose: the interesting decision is
    /// which failures keep the old numbers and which clear them, and that is worth
    /// asserting without a network or a subprocess in the way.
    pub(crate) fn apply(&mut self, claude: Outcome, codex: Outcome, now: DateTime<Utc>) {
        self.fetching = false;
        self.sources[0].apply(claude, now);
        self.sources[1].apply(codex, now);
    }

    /// Whether a read is in flight, for the panel's refresh glyph.
    pub(crate) fn is_fetching(&self) -> bool {
        self.fetching
    }

    /// The state as it stands, for the indicator and the panel to render.
    pub(crate) fn source_states(&self) -> Vec<SourceState> {
        self.sources.to_vec()
    }
}

/// Reads both agents' quota once, concurrently, from wherever `target` says.
///
/// Concurrently, and neither waits on the other: one agent being absent must
/// not delay the other's numbers by a process spawn. A disabled agent
/// substitutes a `ready` future rather than dropping out of the `join`, so the
/// concurrency shape is unchanged whichever agents are on.
async fn read_both(
    target: FetchTarget,
    claude_on: bool,
    codex_on: bool,
    http_client: Arc<dyn http_client::HttpClient>,
    executor: gpui::BackgroundExecutor,
) -> (Outcome, Outcome) {
    use futures::future::Either;

    let (host, label) = match target {
        FetchTarget::ThisMachine => (None, None),
        FetchTarget::NoConnection => {
            return (
                Outcome::Keep(UNAVAILABLE_FOR_CONNECTION.into()),
                Outcome::Keep(UNAVAILABLE_FOR_CONNECTION.into()),
            );
        }
        FetchTarget::Host { label, host } => match host.await {
            Ok(host) => (Some(host), Some(label)),
            Err(error) => {
                log::warn!("agent usage: {label} could not be asked: {error:#}");
                return (
                    Outcome::Keep(UNAVAILABLE_FOR_CONNECTION.into()),
                    Outcome::Keep(UNAVAILABLE_FOR_CONNECTION.into()),
                );
            }
        },
    };

    let claude_future = if claude_on {
        let executor = executor.clone();
        let host = host.clone();
        let label = label.clone();
        Either::Left(async move {
            let result = match host {
                Some(host) => claude::fetch_on_host(http_client, executor, host).await,
                None => claude::fetch(http_client, executor).await,
            };
            Outcome::from_claude(result, label.as_ref().map(SharedString::as_str))
        })
    } else {
        Either::Right(std::future::ready(AgentUsageStore::disabled_outcome()))
    };
    let codex_future = if codex_on {
        Either::Left(async move {
            let result = match host {
                Some(host) => codex::fetch_on_host(executor, host).await,
                None => codex::fetch(executor).await,
            };
            Outcome::from_codex(result, label.as_ref().map(SharedString::as_str))
        })
    } else {
        Either::Right(std::future::ready(AgentUsageStore::disabled_outcome()))
    };

    futures::future::join(claude_future, codex_future).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(last_polled_at: Option<DateTime<Utc>>, fetching: bool) -> AgentUsageStore {
        let mut store = AgentUsageStore::new();
        store.last_polled_at = last_polled_at;
        store.fetching = fetching;
        store
    }

    /// The whole point of the store: a window full of workspaces is one request.
    ///
    /// Eight indicators each report the same window's activation. The first ask
    /// becomes a request and stamps the attempt; the other seven land inside the
    /// throttle and must not.
    #[test]
    fn one_window_of_eight_indicators_asks_once() {
        let now = Utc::now();
        let mut store = store_with(None, false);

        assert!(
            store.should_fetch(PollReason::Activation, now),
            "the first activation of a session has nothing on screen to trust"
        );

        // What `refresh` stamps when it lets a request out.
        store.last_polled_at = Some(now);

        for indicator in 2..=8 {
            assert!(
                !store.should_fetch(PollReason::Activation, now),
                "indicator {indicator} in the same window must not become a second request"
            );
        }
    }

    #[test]
    fn a_fetch_in_flight_blocks_every_reason() {
        let now = Utc::now();
        let store = store_with(None, true);

        assert!(!store.should_fetch(PollReason::Activation, now));
        assert!(
            !store.should_fetch(PollReason::Manual, now),
            "two requests for one intention is the shape of a queue"
        );
    }

    #[test]
    fn a_person_is_never_throttled() {
        let now = Utc::now();
        let store = store_with(Some(now), false);

        assert!(
            !store.should_fetch(PollReason::Activation, now),
            "a fresh attempt makes regaining focus skippable"
        );
        assert!(
            store.should_fetch(PollReason::Manual, now),
            "the whole point of pressing refresh is to distrust what is on screen"
        );
    }

    /// The loop is held while a window is active and dropped when none is.
    ///
    /// Asked of gpui rather than of a set kept here, because closing a window
    /// fires no deactivation: a set would keep the dead id and this application
    /// would poll for ever on behalf of a window nobody has.
    #[gpui::test]
    fn the_loop_is_held_only_while_a_window_is_active(cx: &mut gpui::TestAppContext) {
        let store = cx.new(|_| AgentUsageStore::test_new());
        let (_root, cx) = cx.add_window_view(|_, _| gpui::Empty);
        // The harness opens a window without focusing it; the platform does that
        // for itself.
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();

        store.update(cx, |store, cx| {
            store.plant_poll_task(cx.background_executor().spawn(async {}));
            assert!(
                AgentUsageStore::should_keep_polling(cx),
                "a window is open and active"
            );
            store.window_deactivated(cx);
            assert!(
                store.is_polling(),
                "one indicator reporting its window must not stop a loop the \
                 application still wants"
            );
        });

        cx.deactivate_window();

        store.update(cx, |store, cx| {
            assert!(
                !AgentUsageStore::should_keep_polling(cx),
                "nothing is active now"
            );
            store.window_deactivated(cx);
            assert!(
                !store.is_polling(),
                "with no window active the loop must go, or it polls at a machine \
                 nobody is sitting at"
            );
        });
    }

    #[test]
    fn the_throttle_lapses_after_the_interval() {
        let now = Utc::now();
        let store = store_with(Some(now), false);

        let just_inside = now + chrono::Duration::seconds(29);
        let just_outside = now + chrono::Duration::seconds(31);

        assert!(!store.should_fetch(PollReason::Activation, just_inside));
        assert!(store.should_fetch(PollReason::Activation, just_outside));
    }

    fn a_window(percent: u8) -> crate::UsageWindow {
        crate::UsageWindow {
            percent,
            resets_at: None,
            label: None,
            kind: crate::WindowKind::Session,
        }
    }

    struct RemoteRig {
        project: Entity<Project>,
        host_name: String,
        _server: Entity<()>,
    }

    async fn remote_project(
        cx: &mut gpui::TestAppContext,
        server_cx: &mut gpui::TestAppContext,
    ) -> RemoteRig {
        let fs = fs::FakeFs::new(cx.executor());
        let (project, server) = Project::test_remote(fs, cx, server_cx).await;
        let host_name = project.read_with(cx, |project, cx| {
            project
                .remote_connection_options(cx)
                .expect("a remote project has connection options")
                .display_name()
        });
        RemoteRig {
            project,
            host_name,
            _server: server,
        }
    }

    async fn local_project(cx: &mut gpui::TestAppContext) -> Entity<Project> {
        cx.update(|cx| {
            if !cx.has_global::<settings::SettingsStore>() {
                let settings_store = settings::SettingsStore::test(cx);
                cx.set_global(settings_store);
            }
        });
        let fs = fs::FakeFs::new(cx.executor());
        Project::test(fs, [], cx).await
    }

    /// The account a remote project's agents spend is its host's, so its
    /// numbers live in a store of their own, and neither store's answer
    /// reaches the other.
    #[gpui::test]
    async fn a_remote_project_s_store_is_its_host_s_and_independent_of_this_machine_s(
        cx: &mut gpui::TestAppContext,
        server_cx: &mut gpui::TestAppContext,
    ) {
        let remote = remote_project(cx, server_cx).await;
        let local = local_project(cx).await;

        let remote_store = cx.update(|cx| AgentUsageStore::for_project(&remote.project, cx));
        let local_store = cx.update(|cx| AgentUsageStore::for_project(&local, cx));
        let this_machine = cx.update(|cx| AgentUsageStore::global(cx));

        assert_eq!(local_store.entity_id(), this_machine.entity_id());
        assert_ne!(
            remote_store.entity_id(),
            local_store.entity_id(),
            "a remote project must never read this machine's account"
        );
        assert_eq!(
            cx.update(|cx| AgentUsageStore::for_project(&remote.project, cx))
                .entity_id(),
            remote_store.entity_id(),
            "one store per host, however often it is asked for"
        );
        remote_store.read_with(cx, |store, _| {
            assert_eq!(
                store.host_label().as_ref().map(SharedString::as_str),
                Some(remote.host_name.as_str())
            );
        });

        let now = Utc::now();
        remote_store.update(cx, |store, _| {
            store.apply(
                Outcome::Windows(vec![a_window(71)]),
                Outcome::Keep("unchanged".into()),
                now,
            );
        });
        local_store.update(cx, |store, _| {
            store.apply(
                Outcome::Windows(vec![a_window(4)]),
                Outcome::Keep("unchanged".into()),
                now,
            );
        });

        remote_store.read_with(cx, |store, _| {
            assert_eq!(store.source_at(0).windows, vec![a_window(71)]);
        });
        local_store.read_with(cx, |store, _| {
            assert_eq!(store.source_at(0).windows, vec![a_window(4)]);
        });
    }

    /// A host with no connected project to ask through says so, rather than
    /// reading anything here.
    #[gpui::test]
    async fn a_host_with_no_connection_is_unavailable_for_this_connection(
        cx: &mut gpui::TestAppContext,
        server_cx: &mut gpui::TestAppContext,
    ) {
        let remote = remote_project(cx, server_cx).await;
        let store = cx.update(|cx| AgentUsageStore::for_project(&remote.project, cx));
        let remote_client = remote.project.read_with(cx, |project, _| {
            project
                .remote_client()
                .expect("a remote project has a remote client")
        });
        remote_client.update(cx, |remote_client, cx| {
            remote_client.force_server_not_running(cx)
        });

        let target = store.update(cx, |store, cx| store.fetch_target(cx));
        assert!(
            matches!(target, Some(FetchTarget::NoConnection)),
            "a disconnected host is asked through nothing"
        );

        let http_client = http_client::FakeHttpClient::with_404_response();
        let outcomes = read_both(
            FetchTarget::NoConnection,
            true,
            true,
            http_client,
            cx.executor(),
        )
        .await;
        assert_eq!(
            outcomes,
            (
                Outcome::Keep(UNAVAILABLE_FOR_CONNECTION.into()),
                Outcome::Keep(UNAVAILABLE_FOR_CONNECTION.into()),
            )
        );
    }

    /// A host whose command cannot be built is unavailable, and nothing is
    /// read in its place.
    #[gpui::test]
    async fn a_host_that_cannot_be_built_reads_nothing_here(cx: &mut gpui::TestAppContext) {
        let http_client = http_client::FakeHttpClient::with_404_response();
        let outcomes = read_both(
            FetchTarget::Host {
                label: "Ubuntu-22.04".into(),
                host: Task::ready(Err(anyhow::anyhow!("Ubuntu-22.04 is not connected"))),
            },
            true,
            true,
            http_client,
            cx.executor(),
        )
        .await;
        assert_eq!(
            outcomes,
            (
                Outcome::Keep(UNAVAILABLE_FOR_CONNECTION.into()),
                Outcome::Keep(UNAVAILABLE_FOR_CONNECTION.into()),
            )
        );
    }

    /// The not-signed-in reasons name the host, so a reader can tell which
    /// machine to sign in on.
    #[gpui::test]
    async fn a_host_that_is_signed_out_is_named(cx: &mut gpui::TestAppContext) {
        let http_client = http_client::FakeHttpClient::with_404_response();
        let disconnected: Arc<dyn CommandHost> =
            Arc::new(crate::test_support::RecordingHost::disconnected());
        let outcomes = read_both(
            FetchTarget::Host {
                label: "Ubuntu-22.04".into(),
                host: Task::ready(Ok(disconnected)),
            },
            true,
            true,
            http_client,
            cx.executor(),
        )
        .await;
        assert_eq!(
            outcomes,
            (
                Outcome::Keep(UNAVAILABLE_FOR_CONNECTION.into()),
                Outcome::Keep(UNAVAILABLE_FOR_CONNECTION.into()),
            ),
            "a host that could not run the read is unavailable, not signed out"
        );

        assert_eq!(
            Outcome::from_claude(
                Err(claude::Unavailable::NoCredentials),
                Some("Ubuntu-22.04")
            ),
            Outcome::Clear("not signed in on Ubuntu-22.04".into())
        );
        assert_eq!(
            Outcome::from_codex(Err(codex::Unavailable::NotInstalled), Some("Ubuntu-22.04")),
            Outcome::Clear("codex CLI not installed on Ubuntu-22.04".into())
        );
        assert_eq!(
            Outcome::from_claude(Err(claude::Unavailable::NoCredentials), None),
            Outcome::Clear("no Claude Code sign-in was found on this machine".into()),
            "this machine's wording is unchanged"
        );
    }

    /// A guest's agents run on the guest's own machine, so a collaborator's
    /// project reads this machine's account, as it always has.
    #[gpui::test]
    async fn a_collaborator_s_project_reads_this_machine_s_account(cx: &mut gpui::TestAppContext) {
        let project = local_project(cx).await;
        project.update(cx, |project, _| project.mark_as_collab_for_testing());

        let store = cx.update(|cx| AgentUsageStore::for_project(&project, cx));

        assert_eq!(
            store.entity_id(),
            cx.update(|cx| AgentUsageStore::global(cx)).entity_id()
        );
    }

    /// A project reached through a connection this app cannot run anything
    /// on -- the relay's -- reads nothing and says why.
    #[gpui::test]
    async fn an_unreachable_host_never_polls_and_says_why(cx: &mut gpui::TestAppContext) {
        let store = cx.update(|cx| AgentUsageStore::unreachable(cx));

        store.update(cx, |store, cx| {
            store.refresh_now(cx);
            assert!(!store.is_fetching(), "nothing is read for it");
            assert!(!store.is_polling(), "and no loop runs for it");
            assert!(store.fetch_target(cx).is_none());
            for source in store.source_states() {
                assert_eq!(
                    source.reason.as_ref().map(SharedString::as_str),
                    Some(UNAVAILABLE_FOR_CONNECTION)
                );
                assert!(source.windows.is_empty());
            }
        });
    }
}
