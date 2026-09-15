use std::time::Duration;

use chrono::{DateTime, Utc};
use gpui::{App, AppContext as _, Context, Entity, Global, Task};
use project::AgentId;
use workspace::{StatusBarSettings, item::Settings as _};

use crate::{Outcome, SourceState, claude, codex};

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

/// The quota, read once for the whole application.
///
/// A quota belongs to an account, not to a project: every indicator on screen is
/// showing the same token's numbers. This used to live on the indicator, which
/// the status bar builds once per `Workspace` — and this fork keeps a workspace
/// per project inside one window, so a user with eight projects open ran eight
/// poll loops against one endpoint on one token, each with its own retry chain.
/// Every throttle in this crate was written per indicator and so could not see
/// the other seven.
///
/// One store, one loop, however many indicators observe it.
pub(crate) struct AgentUsageStore {
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
        Self {
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
        if !Self::may_read_usage() {
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

        self.fetching = true;
        // Stamped here rather than on the answer: this is the moment a request
        // goes out, and it is requests the throttle is trying not to duplicate.
        self.last_polled_at = Some(Utc::now());
        cx.notify();

        let http_client = cx.http_client();
        let executor = cx.background_executor().clone();
        let claude_executor = executor.clone();
        let codex_executor = executor;
        self._fetch = Some(cx.spawn(async move |this, cx| {
            // Concurrently, and neither waits on the other: one agent being
            // absent must not delay the other's numbers by a process spawn. A
            // disabled agent substitutes a `ready` future rather than dropping
            // out of the `join`, so the concurrency shape is unchanged whichever
            // agents are on.
            let claude_future = if claude_on {
                futures::future::Either::Left(async move {
                    Outcome::from(claude::fetch(http_client, claude_executor).await)
                })
            } else {
                futures::future::Either::Right(std::future::ready(Self::disabled_outcome()))
            };
            let codex_future = if codex_on {
                futures::future::Either::Left(async move {
                    Outcome::from(codex::fetch(codex_executor).await)
                })
            } else {
                futures::future::Either::Right(std::future::ready(Self::disabled_outcome()))
            };

            let (claude_outcome, codex_outcome) =
                futures::future::join(claude_future, codex_future).await;

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


}
