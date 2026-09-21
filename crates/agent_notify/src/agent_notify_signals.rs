//! What happens once an edge or an exit is known, independent of how it was
//! learned. `agent_notify_watch` derives `answering`/`title` from a live
//! `AgentView` and calls into this; `agent_notify_tests` calls the same
//! methods directly with values it chooses, which is what makes the dedup
//! and quiet-period rules here testable without a real agent tab.

use agent_ui::AgentView;
use gpui::{Context, EntityId, Notification, SharedString};
use util::ResultExt as _;

use crate::agent_notify_settings::AgentFinishedNotificationSetting;
use crate::agent_notify_watch::AgentNotifier;

impl AgentNotifier {
    /// The `AgentViewEvent::Activity` handler. Fires on both edges -- reading
    /// `answering` is what tells them apart, since the event alone does not.
    pub(crate) fn on_activity(
        &mut self,
        id: EntityId,
        answering: bool,
        title: SharedString,
        cx: &mut Context<Self>,
    ) {
        let Some(watched) = self.watched.get_mut(&id) else {
            return;
        };

        if answering {
            // The rising edge. Dropping `quiet` cancels an armed timer from a
            // pause that turned out to be inside the same answer, and
            // clearing `notified` re-arms the next falling edge.
            watched.quiet = None;
            watched.notified = false;
            return;
        }

        if watched.notified {
            // Already notified for this answer -- a pause after the pause
            // must not arm a second timer.
            return;
        }

        let period = AgentFinishedNotificationSetting::quiet_period(cx);
        watched.quiet = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(period).await;
            this.update(cx, |this, cx| this.fire_answered(id, &title, cx))
                .log_err();
        }));
    }

    /// Fires once the quiet period has elapsed with nothing having cancelled
    /// it. There is no "is the agent still quiet" recheck here beyond that:
    /// the timer that reaches this is cancelled synchronously (by
    /// [`Self::on_activity`]'s rising-edge branch, or by [`Self::fire_exit`])
    /// the moment either happens, so if this runs at all, both were still
    /// true when it started. The one thing that *can* change between arming
    /// and firing without going through this tab's own edges is the setting
    /// itself, which is why that alone is re-read here.
    fn fire_answered(&mut self, id: EntityId, title: &SharedString, cx: &mut Context<Self>) {
        let Some(watched) = self.watched.get_mut(&id) else {
            return;
        };
        watched.quiet = None;
        if watched.notified {
            return;
        }
        if !AgentFinishedNotificationSetting::is_enabled(cx) {
            return;
        }

        cx.post_notification(Notification {
            id: format!("{id}-answer").into(),
            title: title.clone(),
            body: "Finished answering".into(),
        });
        watched.notified = true;
    }

    /// Fires once the tab's CLI has actually exited.
    ///
    /// Cancels any armed "finished answering" timer first -- the de-dup rule:
    /// an agent that answers and then exits a second later produces one
    /// notification, the exit one, not two. An agent that answers, sits at
    /// its prompt, and is closed much later still produces two, because
    /// nothing here was left armed to cancel.
    pub(crate) fn fire_exit(&mut self, id: EntityId, title: &SharedString, cx: &mut Context<Self>) {
        let Some(watched) = self.watched.get_mut(&id) else {
            return;
        };
        watched.exit = None;
        watched.quiet = None;
        watched.terminal = None;
        watched.notified = false;

        if !AgentFinishedNotificationSetting::is_enabled(cx) {
            return;
        }

        cx.post_notification(Notification {
            id: format!("{id}-exit").into(),
            title: title.clone(),
            body: "Session ended".into(),
        });
    }

    /// Routes a click on a posted notification back to the tab it named.
    ///
    /// Both the workspace and the tab itself are found through weak handles:
    /// a failed upgrade means the tab closed between the notification being
    /// posted and being clicked, which is an ordinary outcome, not an error.
    pub(crate) fn handle_click(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(entity_id) = parse_notification_id(id) else {
            return;
        };
        let Some(watched) = self.watched.get(&entity_id) else {
            return;
        };
        let Some(workspace) = watched.workspace.upgrade() else {
            return;
        };
        let agent = watched.agent.clone();
        let window = watched.window;

        cx.activate(true);
        window
            .update(cx, move |_root, window, cx| {
                AgentView::activate_for_agent(workspace, &agent, window, cx);
            })
            .log_err();
    }
}

/// Whether a tab needs to be looked at again, given which terminal (if any)
/// is currently being awaited versus which one the tab actually holds now.
///
/// A free function, mirroring `keep_awake::needs_rereading`, so the restart
/// rule can be asserted without a terminal, a spawned task, or a clock.
pub(crate) fn needs_reread(awaited: Option<EntityId>, current: Option<EntityId>) -> bool {
    awaited != current
}

/// Recovers the watched tab's `EntityId` from a notification id built as
/// `"{entity_id}-answer"` or `"{entity_id}-exit"`.
fn parse_notification_id(id: &str) -> Option<EntityId> {
    let raw = id.strip_suffix("-answer").or_else(|| id.strip_suffix("-exit"))?;
    raw.parse::<u64>().ok().map(EntityId::from)
}
