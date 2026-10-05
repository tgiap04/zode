//! Tests for the transcript path: `AgentNotifier::on_turn` and the approval
//! notification. Same boundary as `agent_notify_tests` -- the turn events are
//! passed by hand, since producing them for real needs a CLI writing a
//! transcript; what is exercised for real is the arming, cancelling, latching
//! and settings gate that happen once an event is known.

use std::time::Duration;

use agent_ui::TurnEvent;
use gpui::{Entity, EntityId, TestAppContext, UpdateGlobal as _};
use settings::SettingsStore;

use crate::AgentNotifier;
use crate::agent_notify_signals::parse_notification_id;
use crate::agent_notify_tests::{notifier, open_tab};

const CONFIRMATION: Duration = Duration::from_secs(3);
const HEURISTIC_PERIOD: Duration = Duration::from_millis(12_000);
const TITLE: &str = "Claude Code";

fn turn(
    cx: &mut gpui::VisualTestContext,
    notifier: &Entity<AgentNotifier>,
    id: EntityId,
    event: TurnEvent,
) {
    turn_on(cx, notifier, id, event, true);
}

fn turn_on(
    cx: &mut gpui::VisualTestContext,
    notifier: &Entity<AgentNotifier>,
    id: EntityId,
    event: TurnEvent,
    transcript_turns: bool,
) {
    notifier.update(cx, |n, cx| {
        n.on_turn(id, event, transcript_turns, TITLE.into(), cx)
    });
}

fn bodies(cx: &mut gpui::VisualTestContext) -> Vec<String> {
    cx.posted_notifications()
        .iter()
        .map(|posted| posted.body.to_string())
        .collect()
}

#[gpui::test]
async fn a_turn_end_notifies_once_after_the_confirmation(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    turn(cx, &notifier, id, TurnEvent::Ended);
    cx.executor().advance_clock(CONFIRMATION);
    cx.run_until_parked();

    let posted = cx.posted_notifications();
    assert_eq!(posted.len(), 1);
    assert_eq!(posted[0].body.as_ref(), "Finished answering");
    assert_eq!(posted[0].id.as_ref(), format!("{id}-answer"));
}

#[gpui::test]
async fn a_started_turn_retracts_an_armed_end(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    turn(cx, &notifier, id, TurnEvent::Ended);
    cx.executor().advance_clock(CONFIRMATION / 2);
    turn(cx, &notifier, id, TurnEvent::Started);
    cx.executor().advance_clock(CONFIRMATION * 4);
    cx.run_until_parked();

    assert!(cx.posted_notifications().is_empty());
}

#[gpui::test]
async fn an_interrupted_turn_is_never_announced(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    turn(cx, &notifier, id, TurnEvent::Ended);
    turn(cx, &notifier, id, TurnEvent::Interrupted);
    cx.executor().advance_clock(CONFIRMATION * 4);
    cx.run_until_parked();

    assert!(cx.posted_notifications().is_empty());
}

#[gpui::test]
async fn pty_activity_edges_never_notify_a_transcript_tab(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    notifier.update(cx, |n, cx| n.on_activity(id, true, true, TITLE.into(), cx));
    notifier.update(cx, |n, cx| n.on_activity(id, false, true, TITLE.into(), cx));
    cx.executor().advance_clock(HEURISTIC_PERIOD);
    cx.run_until_parked();

    assert!(
        cx.posted_notifications().is_empty(),
        "typing, a resize or a long command must not read as a finished answer"
    );
}

#[gpui::test]
async fn a_repeated_end_without_a_start_notifies_once(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    turn(cx, &notifier, id, TurnEvent::Ended);
    cx.executor().advance_clock(CONFIRMATION);
    cx.run_until_parked();
    turn(cx, &notifier, id, TurnEvent::Ended);
    cx.executor().advance_clock(CONFIRMATION);
    cx.run_until_parked();

    assert_eq!(cx.posted_notifications().len(), 1);
}

#[gpui::test]
async fn two_turns_notify_twice(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    for _ in 0..2 {
        turn(cx, &notifier, id, TurnEvent::Started);
        turn(cx, &notifier, id, TurnEvent::Ended);
        cx.executor().advance_clock(CONFIRMATION);
        cx.run_until_parked();
    }

    assert_eq!(cx.posted_notifications().len(), 2);
}

#[gpui::test]
async fn exiting_while_a_turn_end_is_armed_notifies_only_the_exit(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    turn(cx, &notifier, id, TurnEvent::Ended);
    notifier.update(cx, |n, cx| n.fire_exit(id, &TITLE.into(), cx));
    cx.executor().advance_clock(CONFIRMATION * 4);
    cx.run_until_parked();

    assert_eq!(bodies(cx), ["Session ended"]);
}

#[gpui::test]
async fn approval_notifies_once_per_pending_episode(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    turn(cx, &notifier, id, TurnEvent::ApprovalNeeded);
    turn(cx, &notifier, id, TurnEvent::ApprovalNeeded);
    let posted = cx.posted_notifications();
    assert_eq!(posted.len(), 1);
    assert_eq!(posted[0].body.as_ref(), "Waiting for your approval");
    assert_eq!(posted[0].id.as_ref(), format!("{id}-approval"));

    turn(cx, &notifier, id, TurnEvent::ApprovalCleared);
    turn(cx, &notifier, id, TurnEvent::ApprovalNeeded);
    assert_eq!(cx.posted_notifications().len(), 2);
}

#[gpui::test]
async fn a_disabled_setting_silences_both_notifications(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);
    cx.update(|_, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |content| {
                content
                    .agent_finished_notification
                    .get_or_insert_default()
                    .enabled = Some(false);
            });
        });
    });

    turn(cx, &notifier, id, TurnEvent::ApprovalNeeded);
    turn(cx, &notifier, id, TurnEvent::Ended);
    cx.executor().advance_clock(CONFIRMATION * 4);
    cx.run_until_parked();

    assert!(cx.posted_notifications().is_empty());
}

#[gpui::test]
async fn a_turn_event_on_a_heuristic_tab_changes_nothing(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    turn_on(cx, &notifier, id, TurnEvent::Ended, false);
    turn_on(cx, &notifier, id, TurnEvent::ApprovalNeeded, false);
    cx.executor().advance_clock(CONFIRMATION * 4);
    cx.run_until_parked();
    assert!(cx.posted_notifications().is_empty());

    notifier.update(cx, |n, cx| {
        n.on_activity(id, false, false, TITLE.into(), cx)
    });
    cx.executor().advance_clock(HEURISTIC_PERIOD);
    cx.run_until_parked();
    assert_eq!(bodies(cx), ["Finished answering"]);
}

#[gpui::test]
async fn a_click_on_an_approval_notification_reaches_its_tab(cx: &mut TestAppContext) {
    let (workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    notifier.update(cx, |n, cx| n.handle_click(&format!("{id}-approval"), cx));
    cx.run_until_parked();

    workspace.read_with(cx, |workspace, cx| {
        let active = workspace.active_item(cx).expect("the tab must be active");
        assert_eq!(active.item_id(), id);
    });
}

#[gpui::test]
fn every_notification_suffix_parses_back_to_its_tab(cx: &mut TestAppContext) {
    let id = cx.update(|cx| gpui::AppContext::new(cx, |_| ()).entity_id());
    for suffix in ["answer", "approval", "exit"] {
        assert_eq!(parse_notification_id(&format!("{id}-{suffix}")), Some(id));
    }
    assert_eq!(parse_notification_id(&format!("{id}-other")), None);
    assert_eq!(parse_notification_id("approval"), None);
}

/// The flag is read off the tab at every event, so a tab that stops reporting
/// turns mid-life (its own session is gone and it starts a new one) goes back
/// to the heuristic instead of staying silent for good.
#[gpui::test]
async fn a_tab_that_starts_a_new_session_goes_back_to_the_heuristic(cx: &mut TestAppContext) {
    let (workspace, id, cx) = open_tab(cx).await;
    let view = workspace
        .read_with(cx, |workspace, cx| {
            workspace.items_of_type::<agent_ui::AgentView>(cx).next()
        })
        .expect("the tab must exist");
    let notifier = notifier(cx);
    view.read_with(cx, |view, _| assert!(view.reports_turns()));

    let falling_edge = |cx: &mut gpui::VisualTestContext| {
        view.update(cx, |_, cx| cx.emit(agent_ui::AgentViewEvent::Activity));
        cx.executor().advance_clock(HEURISTIC_PERIOD);
        cx.run_until_parked();
    };
    falling_edge(cx);
    assert!(
        cx.posted_notifications().is_empty(),
        "a tracked Claude tab leaves the pty edges alone"
    );

    view.update_in(cx, |view, window, cx| view.start_new_session(window, cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert!(!view.reports_turns()));
    falling_edge(cx);

    assert_eq!(bodies(cx), ["Finished answering"]);
    assert_eq!(notifier.read_with(cx, |n, _| n.watched.len()), 1);
    assert!(notifier.read_with(cx, |n, _| n.watched.contains_key(&id)));
}

#[gpui::test]
async fn the_flag_follows_the_tab_for_each_agent(cx: &mut TestAppContext) {
    let (workspace, claude_id, cx) = open_tab(cx).await;
    workspace.update_in(cx, |workspace, window, cx| {
        agent_ui::AgentView::open(workspace, project::CODEX_AGENT_ID, None, window, cx);
    });
    cx.run_until_parked();

    let views = workspace.read_with(cx, |workspace, cx| {
        workspace
            .items_of_type::<agent_ui::AgentView>(cx)
            .collect::<Vec<_>>()
    });
    assert_eq!(views.len(), 2);
    for view in views {
        let reports = view.read_with(cx, |view, _| view.reports_turns());
        assert_eq!(
            reports,
            view.entity_id() == claude_id,
            "the tracked Claude tab reports turns; the Codex tab does not"
        );
    }
}

#[gpui::test]
async fn a_turn_end_delivered_after_the_exit_is_not_announced(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    notifier.update(cx, |n, cx| n.fire_exit(id, &TITLE.into(), cx));
    turn(cx, &notifier, id, TurnEvent::Ended);
    turn(cx, &notifier, id, TurnEvent::ApprovalNeeded);
    cx.executor().advance_clock(CONFIRMATION * 4);
    cx.run_until_parked();

    assert_eq!(bodies(cx), ["Session ended"]);
}

/// A transcript is polled on its own clock, so a `Started` and `Ended` read
/// after the exit are late deliveries, however they are ordered.
#[gpui::test]
async fn a_started_and_ended_delivered_after_the_exit_are_not_announced(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    notifier.update(cx, |n, cx| n.fire_exit(id, &TITLE.into(), cx));
    turn(cx, &notifier, id, TurnEvent::Started);
    turn(cx, &notifier, id, TurnEvent::Ended);
    cx.executor().advance_clock(CONFIRMATION * 4);
    cx.run_until_parked();

    assert_eq!(bodies(cx), ["Session ended"]);
}

#[gpui::test]
async fn a_restart_after_the_exit_lets_the_next_end_notify(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    notifier.update(cx, |n, cx| n.fire_exit(id, &TITLE.into(), cx));
    // What `reread` does on arming a waiter for the replacement terminal.
    notifier.update(cx, |n, _| {
        n.watched.get_mut(&id).expect("watched").exited = false;
    });
    turn(cx, &notifier, id, TurnEvent::Started);
    turn(cx, &notifier, id, TurnEvent::Ended);
    cx.executor().advance_clock(CONFIRMATION);
    cx.run_until_parked();

    assert_eq!(bodies(cx), ["Session ended", "Finished answering"]);
}

/// The tab under test has no real terminal, so the awaited terminal is seeded
/// with a synthetic id. What `reread` compares against after an exit is
/// `Watched::terminal`: if `fire_exit` cleared it, the exited terminal would
/// still be the tab's current one, read as a change, and get a fresh waiter
/// that resolves at once and posts "Session ended" a second time.
#[gpui::test]
async fn an_exit_keeps_the_awaited_terminal_so_reread_does_not_rearm(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);
    cx.run_until_parked();
    let exited_terminal = cx.update(|_, cx| gpui::AppContext::new(cx, |_| ()).entity_id());
    notifier.update(cx, |n, _| {
        n.watched.get_mut(&id).expect("watched").terminal = Some(exited_terminal);
    });

    notifier.update(cx, |n, cx| n.fire_exit(id, &TITLE.into(), cx));

    notifier.read_with(cx, |n, _| {
        let watched = &n.watched[&id];
        assert_eq!(watched.terminal, Some(exited_terminal));
        assert!(watched.exit.is_none());
        assert!(watched.exited);
    });
    assert_eq!(bodies(cx), ["Session ended"]);
}

#[gpui::test]
async fn an_exit_clears_the_approval_latch(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    turn(cx, &notifier, id, TurnEvent::ApprovalNeeded);
    notifier.update(cx, |n, cx| n.fire_exit(id, &TITLE.into(), cx));
    notifier.update(cx, |n, _| {
        n.watched.get_mut(&id).expect("watched").exited = false;
    });
    turn(cx, &notifier, id, TurnEvent::ApprovalNeeded);

    assert_eq!(
        bodies(cx),
        [
            "Waiting for your approval",
            "Session ended",
            "Waiting for your approval"
        ]
    );
}
