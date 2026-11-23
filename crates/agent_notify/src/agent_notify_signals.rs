//! What happens once an edge or an exit is known, independent of how it was
//! learned. `agent_notify_watch` derives `answering`/`title` from a live
//! `AgentView` and calls into this; `agent_notify_tests` calls the same
//! methods directly with values it chooses, which is what makes the dedup
//! and quiet-period rules here testable without a real agent tab.

use std::time::Duration;

use agent_ui::{AgentView, TurnEvent};
use gpui::{App, Context, EntityId, Notification, SharedString};
use util::ResultExt as _;

use crate::agent_notify_settings::AgentFinishedNotificationSetting;
use crate::agent_notify_watch::AgentNotifier;
use crate::{AgentAttention, AttentionKind, HEURISTIC_ANSWER_MIN_GAP, HEURISTIC_ANSWER_QUIET};

/// How long a transcript-reported turn end is held before it is believed.
///
/// Not the user's `quiet_period_ms`: that setting exists to outwait the
/// pty heuristic's false edges, which this path does not have. What is left
/// to outwait is an agent that logs a turn end and then immediately starts
/// another (a blocking stop hook), which arrives as `Started` well inside
/// this window and retracts it.
const TURN_END_CONFIRMATION: Duration = Duration::from_secs(3);

impl AgentNotifier {
    /// The `AgentViewEvent::Activity` handler. Fires on both edges -- reading
    /// `answering` is what tells them apart, since the event alone does not.
    pub(crate) fn on_activity(
        &mut self,
        id: EntityId,
        answering: bool,
        transcript_turns: bool,
        title: SharedString,
        cx: &mut Context<Self>,
    ) {
        let attention_wanted = self.attention_wanted;
        let Some(watched) = self.watched.get_mut(&id) else {
            return;
        };
        // For a tab whose turns come from its transcript the pty-rate edges
        // are ignored: they cannot tell a finished answer from a keystroke, a
        // resize or a long-running command. Passed per event because a tab
        // can stop qualifying (a fresh session after its own is gone).
        if transcript_turns {
            return;
        }

        if answering {
            // The rising edge. Dropping `quiet` cancels an armed timer from a
            // pause that turned out to be inside the same answer, and
            // clearing `notified` re-arms the next falling edge.
            watched.quiet = None;
            watched.notified = false;
            watched.attention_quiet = None;
            watched.answer_raised = false;
            return;
        }

        // Armed once per quiet stretch, so the wait counts from its start.
        if attention_wanted && !watched.answer_raised && watched.attention_quiet.is_none() {
            let title = title.clone();
            watched.attention_quiet = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(HEURISTIC_ANSWER_QUIET).await;
                this.update(cx, |this, cx| {
                    if let Some(watched) = this.watched.get_mut(&id) {
                        watched.attention_quiet = None;
                    }
                    this.raise_answer(id, &title, false, cx);
                })
                .log_err();
            }));
        }

        if watched.notified || !cx.can_post_notifications() {
            // Already notified for this answer -- a pause after the pause
            // must not arm a second timer -- or nothing could be posted.
            return;
        }

        let period = AgentFinishedNotificationSetting::quiet_period(cx);
        watched.quiet = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(period).await;
            this.update(cx, |this, cx| this.fire_answered(id, &title, false, cx))
                .log_err();
        }));
    }

    /// The `AgentViewEvent::Turn` handler, for tabs whose turns come from
    /// their transcript. Ignored for any other tab, so a stray event can never
    /// reach the heuristic path's latch.
    pub(crate) fn on_turn(
        &mut self,
        id: EntityId,
        event: TurnEvent,
        transcript_turns: bool,
        title: SharedString,
        cx: &mut Context<Self>,
    ) {
        let Some(watched) = self.watched.get_mut(&id) else {
            return;
        };
        if !transcript_turns {
            return;
        }

        match event {
            TurnEvent::Started | TurnEvent::Interrupted => {
                // `Interrupted` follows `Ended` immediately for a turn the
                // user cut short, so cancelling the armed timer is what
                // suppresses the announcement.
                watched.quiet = None;
                watched.notified = false;
                watched.attention_quiet = None;
                watched.answer_raised = false;
            }
            // The transcript is polled on its own clock, so an event can be
            // delivered after the exit waiter has already fired; announcing
            // it would put "Finished answering" behind "Session ended".
            TurnEvent::Ended | TurnEvent::ApprovalNeeded if watched.exited => {}
            TurnEvent::Ended => {
                if watched.notified || !(cx.can_post_notifications() || self.attention_wanted) {
                    return;
                }
                watched.quiet = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(TURN_END_CONFIRMATION).await;
                    this.update(cx, |this, cx| this.fire_answered(id, &title, true, cx))
                        .log_err();
                }));
            }
            TurnEvent::ApprovalNeeded => {
                if self.attention_wanted && !watched.approval_raised {
                    watched.approval_raised = true;
                    cx.emit(AgentAttention {
                        tab: id,
                        title: title.clone(),
                        kind: AttentionKind::ApprovalNeeded,
                    });
                }
                if watched.approval_notified {
                    return;
                }
                if !posts_to_the_desktop(cx) {
                    return;
                }
                cx.post_notification(Notification {
                    id: format!("{id}-approval").into(),
                    title,
                    body: "Waiting for your approval".into(),
                });
                watched.approval_notified = true;
            }
            TurnEvent::ApprovalCleared => {
                watched.approval_notified = false;
                watched.approval_raised = false;
            }
        }
    }

    /// Fires once the quiet period has elapsed with nothing having cancelled
    /// it. There is no "is the agent still quiet" recheck here beyond that:
    /// the timer that reaches this is cancelled synchronously (by
    /// [`Self::on_activity`]'s rising-edge branch, by a transcript
    /// `Started` or `Interrupted`, or by [`Self::fire_exit`])
    /// the moment any of them happens, so if this runs at all, both were still
    /// true when it started. The one thing that *can* change between arming
    /// and firing without going through this tab's own edges is the setting
    /// itself, which is why that alone is re-read here.
    fn fire_answered(
        &mut self,
        id: EntityId,
        title: &SharedString,
        from_transcript: bool,
        cx: &mut Context<Self>,
    ) {
        // The heuristic path raises its event on its own, longer, clock.
        if from_transcript {
            self.raise_answer(id, title, true, cx);
        }
        let Some(watched) = self.watched.get_mut(&id) else {
            return;
        };
        watched.quiet = None;
        if watched.notified {
            return;
        }
        if !posts_to_the_desktop(cx) {
            return;
        }

        cx.post_notification(Notification {
            id: format!("{id}-answer").into(),
            title: title.clone(),
            body: "Finished answering".into(),
        });
        watched.notified = true;
    }

    /// Raises the answer in progress as an event, once.
    fn raise_answer(
        &mut self,
        id: EntityId,
        title: &SharedString,
        from_transcript: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.attention_wanted {
            return;
        }
        let now = cx.background_executor().now();
        let Some(watched) = self.watched.get_mut(&id) else {
            return;
        };
        if watched.answer_raised {
            return;
        }
        watched.answer_raised = true;
        if !from_transcript {
            let too_soon = watched.heuristic_answer_raised_at.is_some_and(|raised_at| {
                now.saturating_duration_since(raised_at) < HEURISTIC_ANSWER_MIN_GAP
            });
            if too_soon {
                return;
            }
            watched.heuristic_answer_raised_at = Some(now);
        }
        cx.emit(AgentAttention {
            tab: id,
            title: title.clone(),
            kind: AttentionKind::Answered { from_transcript },
        });
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
        // `terminal` stays on the dead terminal's id: clearing it would make
        // the next `reread` (any view notify) see a change, re-arm a waiter
        // on a terminal that has already completed, and post a second
        // "Session ended". A restart puts a new id there, which does re-arm.
        watched.exited = true;
        watched.notified = false;
        watched.approval_notified = false;
        watched.attention_quiet = None;
        watched.answer_raised = false;
        watched.approval_raised = false;
        if self.attention_wanted {
            cx.emit(AgentAttention {
                tab: id,
                title: title.clone(),
                kind: AttentionKind::Ended,
            });
        }

        if !posts_to_the_desktop(cx) {
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

/// Whether a desktop notification is both wanted and possible here. The
/// [`AgentAttention`] events are raised either way.
fn posts_to_the_desktop(cx: &App) -> bool {
    cx.can_post_notifications() && AgentFinishedNotificationSetting::is_enabled(cx)
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
/// `"{entity_id}-answer"`, `"{entity_id}-approval"` or `"{entity_id}-exit"`.
pub(crate) fn parse_notification_id(id: &str) -> Option<EntityId> {
    let raw = id
        .strip_suffix("-answer")
        .or_else(|| id.strip_suffix("-approval"))
        .or_else(|| id.strip_suffix("-exit"))?;
    raw.parse::<u64>().ok().map(EntityId::from)
}
