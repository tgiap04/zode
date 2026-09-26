//! Tests for the dedup and quiet-period rules in `agent_notify`.
//!
//! Most of these drive [`AgentNotifier::on_activity`] and
//! [`AgentNotifier::fire_exit`] directly against a real tab's `EntityId`,
//! passing the answering signal and the title by hand rather than through a
//! real falling/rising edge. That is a deliberate boundary, not a shortcut:
//! `AgentView::is_answering()`'s debounce is `agent_ui`'s own to test (and it
//! does, in `agent_view.rs`), and nothing outside that crate can flip it --
//! there is no test-only setter, and driving it for real needs an actual CLI
//! process, which the pty tests here are flaky enough without. What belongs
//! to *this* crate is what happens once an edge is known: arm, cancel, latch,
//! re-arm, dedup, and the settings gate -- all exercised here for real.
//!
//! `agent_notify_tests_more` covers the rest: the init gate, the focus-check
//! guard, click routing, and the restart rule -- see its own doc for why the
//! restart rule is asserted the same indirect way `keep_awake_tests.rs`
//! asserts it.

use std::time::Duration;

use agent_ui::AgentView;
use gpui::{Entity, EntityId, TestAppContext, UpdateGlobal as _, VisualTestContext};
use settings::{Settings as _, SettingsStore};
use workspace::Workspace;

use crate::agent_notify_settings::AgentFinishedNotificationSetting;
use crate::{AgentNotifier, GlobalAgentNotifier};

const PERIOD: Duration = Duration::from_millis(12_000);
const TITLE: &str = "Claude Code";

fn set_enabled(cx: &mut TestAppContext, enabled: bool) {
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |content| {
                content
                    .agent_finished_notification
                    .get_or_insert_default()
                    .enabled = Some(enabled);
            });
        });
    });
}

/// Opens one real workspace with one real (CLI-less) agent tab, with
/// `agent_notify::init` already run against it. Returns the tab's id: the
/// key everything else in this file drives by hand.
pub(crate) async fn open_tab(
    cx: &mut TestAppContext,
) -> (Entity<Workspace>, EntityId, &mut VisualTestContext) {
    cx.set_notifications_supported(true);
    cx.update(|cx| {
        let store = settings::SettingsStore::test(cx);
        cx.set_global(store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        editor::init(cx);
        AgentFinishedNotificationSetting::register(cx);
        project::DisableAiSettings::register(cx);
        crate::init(cx);
    });
    let fs = fs::FakeFs::new(cx.executor());
    let project = project::Project::test(fs, [], cx).await;
    let (multi, cx) =
        cx.add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
    let workspace = multi.read_with(cx, |multi, _| multi.workspace().clone());

    workspace.update_in(cx, |workspace, window, cx| {
        AgentView::open(workspace, project::CLAUDE_CODE_AGENT_ID, None, window, cx);
    });
    cx.run_until_parked();

    let id = workspace
        .read_with(cx, |workspace, cx| {
            workspace.items_of_type::<AgentView>(cx).next()
        })
        .expect("the tab has to exist for this to mean anything")
        .entity_id();

    (workspace, id, cx)
}

pub(crate) fn notifier(cx: &mut TestAppContext) -> Entity<AgentNotifier> {
    cx.update(|cx| cx.global::<GlobalAgentNotifier>().0.clone())
}

#[gpui::test]
async fn a_falling_edge_held_quiet_notifies_once(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    notifier.update(cx, |n, cx| n.on_activity(id, false, TITLE.into(), cx));
    cx.executor().advance_clock(PERIOD);
    cx.run_until_parked();

    let posted = cx.posted_notifications();
    assert_eq!(posted.len(), 1);
    assert_eq!(posted[0].body.as_ref(), "Finished answering");
}

#[gpui::test]
async fn a_reversed_edge_before_the_period_elapses_notifies_nothing(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    notifier.update(cx, |n, cx| n.on_activity(id, false, TITLE.into(), cx));
    cx.executor().advance_clock(PERIOD / 2);
    notifier.update(cx, |n, cx| n.on_activity(id, true, TITLE.into(), cx));
    cx.executor().advance_clock(PERIOD);
    cx.run_until_parked();

    assert!(
        cx.posted_notifications().is_empty(),
        "a pause inside one answer must not notify"
    );
}

#[gpui::test]
async fn two_answers_notify_twice(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    notifier.update(cx, |n, cx| n.on_activity(id, false, TITLE.into(), cx));
    cx.executor().advance_clock(PERIOD);
    cx.run_until_parked();
    assert_eq!(cx.posted_notifications().len(), 1);

    notifier.update(cx, |n, cx| n.on_activity(id, true, TITLE.into(), cx));
    notifier.update(cx, |n, cx| n.on_activity(id, false, TITLE.into(), cx));
    cx.executor().advance_clock(PERIOD);
    cx.run_until_parked();

    assert_eq!(
        cx.posted_notifications().len(),
        2,
        "a second answer re-arms the latch"
    );
}

#[gpui::test]
async fn one_answer_stays_at_one_no_matter_how_long_the_clock_runs(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    notifier.update(cx, |n, cx| n.on_activity(id, false, TITLE.into(), cx));
    cx.executor().advance_clock(PERIOD * 10);
    cx.run_until_parked();

    assert_eq!(cx.posted_notifications().len(), 1, "the notified latch");
}

#[gpui::test]
async fn exiting_while_a_quiet_timer_is_armed_notifies_only_the_exit(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    notifier.update(cx, |n, cx| n.on_activity(id, false, TITLE.into(), cx));
    notifier.update(cx, |n, cx| n.fire_exit(id, &TITLE.into(), cx));
    cx.executor().advance_clock(PERIOD);
    cx.run_until_parked();

    let posted = cx.posted_notifications();
    assert_eq!(
        posted.len(),
        1,
        "the exit must cancel the armed answer timer"
    );
    assert_eq!(posted[0].body.as_ref(), "Session ended");
}

#[gpui::test]
async fn exiting_long_after_a_delivered_answer_notifies_again_with_a_distinct_id(
    cx: &mut TestAppContext,
) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    notifier.update(cx, |n, cx| n.on_activity(id, false, TITLE.into(), cx));
    cx.executor().advance_clock(PERIOD);
    cx.run_until_parked();
    assert_eq!(cx.posted_notifications().len(), 1);

    notifier.update(cx, |n, cx| n.fire_exit(id, &TITLE.into(), cx));

    let posted = cx.posted_notifications();
    assert_eq!(
        posted.len(),
        2,
        "finishing an answer and later exiting are two events the user asked to hear about"
    );
    assert_ne!(posted[0].id, posted[1].id);
}

#[gpui::test]
async fn disabling_the_setting_mid_period_notifies_nothing(cx: &mut TestAppContext) {
    let (_workspace, id, cx) = open_tab(cx).await;
    let notifier = notifier(cx);

    notifier.update(cx, |n, cx| n.on_activity(id, false, TITLE.into(), cx));
    set_enabled(cx, false);
    cx.executor().advance_clock(PERIOD);
    cx.run_until_parked();

    assert!(
        cx.posted_notifications().is_empty(),
        "the setting is the user's own answer and outranks a timer already armed"
    );
}
