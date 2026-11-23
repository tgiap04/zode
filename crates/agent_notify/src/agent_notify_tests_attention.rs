//! Tests for the [`AgentAttention`] events another device is told through.
//! Same boundary as `agent_notify_tests`: the edges and turn events are passed
//! by hand. What these hold is that the events do not depend on the desktop
//! setting, come once per episode, and wait out the heuristic's longer grace.

use std::{cell::RefCell, rc::Rc, time::Duration};

use agent_ui::TurnEvent;
use gpui::{Entity, EntityId, Subscription, TestAppContext, UpdateGlobal as _, VisualTestContext};
use settings::SettingsStore;

use crate::agent_notify_tests::{notifier, open_tab, open_tab_where};
use crate::{
    AgentAttention, AgentNotifier, AttentionKind, HEURISTIC_ANSWER_MIN_GAP, HEURISTIC_ANSWER_QUIET,
};

const CONFIRMATION: Duration = Duration::from_secs(3);
const DESKTOP_PERIOD: Duration = Duration::from_millis(12_000);
const TITLE: &str = "Claude Code";

type Heard = Rc<RefCell<Vec<AttentionKind>>>;

fn listen(cx: &mut VisualTestContext, notifier: &Entity<AgentNotifier>) -> (Heard, Subscription) {
    notifier.update(cx, |notifier, _| notifier.set_attention_wanted(true));
    let heard = Heard::default();
    let subscription = cx.update(|_, cx| {
        let heard = heard.clone();
        cx.subscribe(notifier, move |_, event: &AgentAttention, _| {
            assert_eq!(event.title.as_ref(), TITLE);
            heard.borrow_mut().push(event.kind);
        })
    });
    (heard, subscription)
}

fn disable_desktop_notifications(cx: &mut VisualTestContext) {
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
}

fn edge(cx: &mut VisualTestContext, notifier: &Entity<AgentNotifier>, id: EntityId, up: bool) {
    notifier.update(cx, |n, cx| n.on_activity(id, up, false, TITLE.into(), cx));
}

fn turn(
    cx: &mut VisualTestContext,
    notifier: &Entity<AgentNotifier>,
    id: EntityId,
    event: TurnEvent,
) {
    notifier.update(cx, |n, cx| n.on_turn(id, event, true, TITLE.into(), cx));
}

fn wait(cx: &mut VisualTestContext, duration: Duration) {
    cx.executor().advance_clock(duration);
    cx.run_until_parked();
}

#[gpui::test]
async fn an_approval_is_raised_once_per_episode_with_desktop_notifications_off(
    cx: &mut TestAppContext,
) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);
    let (heard, _subscription) = listen(cx, &notifier);
    disable_desktop_notifications(cx);

    turn(cx, &notifier, id, TurnEvent::ApprovalNeeded);
    turn(cx, &notifier, id, TurnEvent::ApprovalNeeded);
    cx.run_until_parked();
    assert_eq!(*heard.borrow(), vec![AttentionKind::ApprovalNeeded]);
    assert!(
        cx.posted_notifications().is_empty(),
        "the desktop stays quiet"
    );

    turn(cx, &notifier, id, TurnEvent::ApprovalCleared);
    turn(cx, &notifier, id, TurnEvent::ApprovalNeeded);
    cx.run_until_parked();
    assert_eq!(heard.borrow().len(), 2, "a new episode is raised again");
}

#[gpui::test]
async fn a_transcript_answer_is_raised_after_its_confirmation(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);
    let (heard, _subscription) = listen(cx, &notifier);
    disable_desktop_notifications(cx);

    turn(cx, &notifier, id, TurnEvent::Ended);
    wait(cx, CONFIRMATION);
    turn(cx, &notifier, id, TurnEvent::Ended);
    wait(cx, CONFIRMATION);
    assert_eq!(
        *heard.borrow(),
        vec![AttentionKind::Answered {
            from_transcript: true
        }]
    );

    turn(cx, &notifier, id, TurnEvent::Started);
    turn(cx, &notifier, id, TurnEvent::Ended);
    wait(cx, CONFIRMATION);
    assert_eq!(heard.borrow().len(), 2, "the next turn is raised again");
}

#[gpui::test]
async fn a_heuristic_answer_waits_for_the_longer_quiet(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);
    let (heard, _subscription) = listen(cx, &notifier);

    edge(cx, &notifier, id, false);
    wait(cx, DESKTOP_PERIOD);
    assert_eq!(
        cx.posted_notifications().len(),
        1,
        "the desktop is unchanged"
    );
    assert!(heard.borrow().is_empty(), "too soon for another device");

    edge(cx, &notifier, id, false);
    wait(cx, HEURISTIC_ANSWER_QUIET - DESKTOP_PERIOD);
    assert_eq!(
        *heard.borrow(),
        vec![AttentionKind::Answered {
            from_transcript: false
        }],
        "the quiet is counted from the first falling edge"
    );
    wait(cx, HEURISTIC_ANSWER_QUIET * 3);
    assert_eq!(heard.borrow().len(), 1, "one answer is raised once");
}

#[gpui::test]
async fn activity_inside_the_quiet_retracts_a_heuristic_answer(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);
    let (heard, _subscription) = listen(cx, &notifier);
    disable_desktop_notifications(cx);

    edge(cx, &notifier, id, false);
    wait(cx, HEURISTIC_ANSWER_QUIET / 2);
    edge(cx, &notifier, id, true);
    wait(cx, HEURISTIC_ANSWER_QUIET);
    assert!(heard.borrow().is_empty());

    edge(cx, &notifier, id, false);
    wait(cx, HEURISTIC_ANSWER_QUIET);
    assert_eq!(heard.borrow().len(), 1);
}

#[gpui::test]
async fn an_exit_is_raised_and_retracts_a_pending_answer(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);
    let (heard, _subscription) = listen(cx, &notifier);
    disable_desktop_notifications(cx);

    edge(cx, &notifier, id, false);
    wait(cx, DESKTOP_PERIOD);
    notifier.update(cx, |n, cx| n.fire_exit(id, &TITLE.into(), cx));
    wait(cx, HEURISTIC_ANSWER_QUIET * 2);
    assert_eq!(*heard.borrow(), vec![AttentionKind::Ended]);
}

#[gpui::test]
async fn an_event_for_a_tab_that_is_not_watched_raises_nothing(cx: &mut TestAppContext) {
    let (_workspace, _id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);
    let (heard, _subscription) = listen(cx, &notifier);
    let stranger = EntityId::from(u64::MAX);

    turn(cx, &notifier, stranger, TurnEvent::ApprovalNeeded);
    edge(cx, &notifier, stranger, false);
    notifier.update(cx, |n, cx| n.fire_exit(stranger, &TITLE.into(), cx));
    wait(cx, HEURISTIC_ANSWER_QUIET * 2);
    assert!(heard.borrow().is_empty());
}

#[gpui::test]
async fn a_heuristic_answer_is_raised_at_most_once_per_gap(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);
    let (heard, _subscription) = listen(cx, &notifier);
    disable_desktop_notifications(cx);

    edge(cx, &notifier, id, false);
    wait(cx, HEURISTIC_ANSWER_QUIET);
    assert_eq!(heard.borrow().len(), 1);

    // A keystroke, then a minute of quiet: not a second answer to push.
    edge(cx, &notifier, id, true);
    edge(cx, &notifier, id, false);
    wait(cx, HEURISTIC_ANSWER_QUIET);
    assert_eq!(heard.borrow().len(), 1, "too soon after the last one");

    wait(cx, HEURISTIC_ANSWER_MIN_GAP);
    edge(cx, &notifier, id, true);
    edge(cx, &notifier, id, false);
    wait(cx, HEURISTIC_ANSWER_QUIET);
    assert_eq!(heard.borrow().len(), 2, "once the gap has passed");

    // The transcript path has no such gap.
    turn(cx, &notifier, id, TurnEvent::Started);
    turn(cx, &notifier, id, TurnEvent::Ended);
    wait(cx, CONFIRMATION);
    assert_eq!(heard.borrow().len(), 3);
}

#[gpui::test]
async fn nothing_is_armed_for_attention_while_nobody_listens(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    edge(cx, &notifier, id, false);
    let armed = notifier.read_with(cx, |n, _| {
        n.watched
            .get(&id)
            .is_some_and(|watched| watched.attention_quiet.is_some())
    });
    assert!(!armed, "remote control is off: no attention timer");
}

#[gpui::test]
async fn a_platform_that_cannot_post_arms_no_desktop_timer(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab_where(cx, false).await;
    let notifier = notifier(cx);

    edge(cx, &notifier, id, false);
    let armed = notifier.read_with(cx, |n, _| {
        n.watched
            .get(&id)
            .is_some_and(|watched| watched.quiet.is_some())
    });
    assert!(!armed, "nothing could be posted when it fired");
}
