//! Wires [`AgentNotifier`] to real `AgentView` tabs: construction, and the
//! two callbacks that watch a tab for as long as it exists. What happens once
//! an edge or an exit is known lives in `agent_notify_signals`, which is also
//! what `agent_notify_tests` drives directly.

use std::collections::HashMap;

use agent_ui::{AgentView, AgentViewEvent};
use gpui::{AnyWindowHandle, Context, Entity, EntityId, Subscription, Task, WeakEntity};
use project::AgentId;
use util::ResultExt as _;
use workspace::Workspace;

use crate::agent_notify_signals::needs_reread;

/// What is kept alive per watched agent tab.
///
/// No handle to the `AgentView` entity itself: every method that touches
/// `Watched` after `watch()` receives whatever it needs (the answering
/// signal, the tab's title, the entity to reread) as a parameter from the
/// callback that is already holding it. That is what keeps
/// `agent_notify_signals` testable with a synthetic id and no real agent tab.
pub(crate) struct Watched {
    /// Where a click on this tab's notification should land.
    pub(crate) workspace: WeakEntity<Workspace>,
    pub(crate) window: AnyWindowHandle,
    pub(crate) agent: AgentId,
    /// Which terminal `exit` is awaiting. Identity, not a flag --
    /// `AgentView::restart` puts a brand new terminal in the same tab, and a
    /// completion waiter keyed on the tab rather than the terminal would
    /// never re-arm for it. See `needs_reread`.
    pub(crate) terminal: Option<EntityId>,
    /// Whether the "finished answering" notification has already fired for
    /// the answer currently in progress. Cleared on the next rising edge, so
    /// one answer produces at most one notification and two answers produce
    /// two.
    pub(crate) notified: bool,
    /// The one-shot timer armed on the falling edge. Dropping it cancels it --
    /// that is the whole cancellation mechanism, not a flag beside it.
    pub(crate) quiet: Option<Task<()>>,
    /// Awaits this tab's CLI exiting.
    pub(crate) exit: Option<Task<()>>,
    _activity: Subscription,
    _identity: Subscription,
    _release: Subscription,
}

/// Watches every `AgentView` in the process and notifies on its two triggers.
pub struct AgentNotifier {
    pub(crate) watched: HashMap<EntityId, Watched>,
    _observe_new: Subscription,
}

impl AgentNotifier {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let this = cx.entity();
        // Not read inside this closure: `agent_view` arrives mid-construction,
        // and `view.read(cx)` on it panics with "cannot read while it is
        // already being updated" -- the exact bug the 2026-08-26 journal
        // records taking ~30 workspace tests down. Everything this needs is
        // read off the `&mut AgentView` borrow the closure is handed instead.
        let observe_new = cx.observe_new::<AgentView>(move |agent_view, window, cx| {
            let Some(window) = window else {
                // No window yet to activate on a click; nothing this tab could
                // be notified about is worth tracking without one.
                return;
            };
            let view = cx.entity();
            let workspace = agent_view.workspace();
            let window = window.window_handle();
            let agent = agent_view.agent_id().clone();
            this.update(cx, |this, cx| {
                this.watch(view, workspace, window, agent, cx);
            });
        });
        Self {
            watched: HashMap::default(),
            _observe_new: observe_new,
        }
    }

    fn watch(
        &mut self,
        view: Entity<AgentView>,
        workspace: WeakEntity<Workspace>,
        window: AnyWindowHandle,
        agent: AgentId,
        cx: &mut Context<Self>,
    ) {
        let id = view.entity_id();
        if self.watched.contains_key(&id) {
            return;
        }

        let activity = cx.subscribe(&view, move |this, view, event, cx| {
            if !matches!(event, AgentViewEvent::Activity) {
                return;
            }
            let agent_view = view.read(cx);
            let answering = agent_view.is_answering();
            let title = agent_view.tab_label();
            this.on_activity(id, answering, title, cx);
        });
        let identity = cx.observe(&view, |this, view, cx| this.reread(view, cx));
        // `observe`/`subscribe` stop delivering once the entity is gone, but
        // neither prunes `watched` for it -- this is the only path that does.
        let release = cx.observe_release(&view, move |this, _view, _cx| {
            this.watched.remove(&id);
        });

        self.watched.insert(
            id,
            Watched {
                workspace,
                window,
                agent,
                terminal: None,
                notified: false,
                quiet: None,
                exit: None,
                _activity: activity,
                _identity: identity,
                _release: release,
            },
        );

        // Deferred rather than called inline: `watch` is reached synchronously
        // from `observe_new`, which fires while `view` is still mid-construction
        // -- `view.read(cx)` inside `reread` would panic with "cannot read
        // while it is already being updated" (see the 2026-08-26 journal).
        // `cx.defer` runs this once that update has finished.
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            this.update(cx, |this, cx| this.reread(view, cx)).log_err();
        });
    }

    /// Reads one tab's terminal and (re)arms the exit waiter if it changed.
    ///
    /// Keyed on the terminal's own `EntityId`, not on whether a completion
    /// task happens to be running: `AgentView::restart` drops the old
    /// terminal and starts a new one in the same tab, and a task still
    /// running for the *dead* terminal would otherwise read as "already
    /// covered" and this tab would never notify on exit again for the rest of
    /// its life. Reasoning copied from `keep_awake::reread`, which was bitten
    /// by exactly this.
    fn reread(&mut self, view: Entity<AgentView>, cx: &mut Context<Self>) {
        let id = view.entity_id();
        if !self.watched.contains_key(&id) {
            return;
        }

        let agent_view = view.read(cx);
        let terminal = agent_view
            .terminal()
            .map(|terminal_view| terminal_view.read(cx).terminal().clone());
        let terminal_id = terminal.as_ref().map(|terminal| terminal.entity_id());
        let title = agent_view.tab_label();

        let awaited = self.watched.get(&id).and_then(|watched| watched.terminal);
        if !needs_reread(awaited, terminal_id) {
            return;
        }

        if let Some(watched) = self.watched.get_mut(&id) {
            // Dropping the previous waiter cancels it -- it was watching the
            // terminal that has just been replaced or removed.
            watched.exit = None;
            watched.terminal = terminal_id;
        }

        let Some(terminal) = terminal else {
            return;
        };

        // Resolves immediately if the terminal has already exited, so this is
        // safe to spawn unconditionally rather than gating on "is it still
        // running" the way `keep_awake` has to for a different question.
        let completion = terminal.read(cx).wait_for_completed_task(cx);
        let task = cx.spawn(async move |this, cx| {
            completion.await;
            this.update(cx, |this, cx| this.fire_exit(id, &title, cx))
                .log_err();
        });
        if let Some(watched) = self.watched.get_mut(&id) {
            watched.exit = Some(task);
        }
    }
}
