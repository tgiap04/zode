//! Tests for the lock-lifetime rules in `keep_awake`.
//!
//! In their own file the way `agent_ui::session_history::panel_tests` is: the
//! rules being checked are worth reading on their own, and they outnumber the
//! code that implements them.
//!
//! Most of it drives `Holds` directly, which proves the bookkeeping and not the
//! question feeding it. `a_real_tab` closes the half of that gap a test can
//! reach: a tab open with no CLI behind it, through a real workspace.
//!
//! The other half is still open, and honestly so. Proving that a *live* CLI
//! sitting at its prompt holds nothing needs a real process in the test, and
//! the pty tests in this tree are flaky enough already. `grace` covers the rule
//! that decides it; nothing covers the wiring that asks.

use super::*;
use gpui::{TestAppContext, UpdateGlobal as _};

/// A stand-in for an agent tab. Only its identity matters to `Holds`.
fn tab(cx: &mut TestAppContext) -> EntityId {
    cx.update(|cx| cx.new(|_| ()).entity_id())
}

/// Installs a settings store so `KeepDisplayAwakeSetting` resolves from a
/// real value rather than falling back. Tests that do not call this exercise
/// the fallback, which is the same `true` as the shipped default.
fn init_settings(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let store = settings::SettingsStore::test(cx);
        cx.set_global(store);
        KeepDisplayAwakeSetting::register(cx);
    });
}

fn set_enabled(cx: &mut TestAppContext, enabled: bool) {
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |content| {
                content.keep_display_awake = Some(enabled);
            });
        });
    });
}

#[gpui::test]
fn the_lock_is_taken_when_the_first_agent_starts(cx: &mut TestAppContext) {
    let claude = tab(cx);
    let mut holds = Holds::default();

    cx.update(|cx| assert!(holds.set(claude, "Claude Code".into(), cx)));

    assert_eq!(cx.display_wake_reasons(), vec!["Claude Code is answering"]);
}

#[gpui::test]
fn the_lock_is_released_when_the_last_agent_finishes(cx: &mut TestAppContext) {
    let claude = tab(cx);
    let mut holds = Holds::default();

    cx.update(|cx| {
        holds.set(claude, "Claude Code".into(), cx);
        assert!(holds.clear(claude, cx));
    });

    assert!(cx.display_wake_reasons().is_empty());
}

/// The pair that would break if the lock were dropped on the first exit
/// rather than the last.
#[gpui::test]
fn one_agent_finishing_leaves_the_lock_with_the_other(cx: &mut TestAppContext) {
    let claude = tab(cx);
    let codex = tab(cx);
    let mut holds = Holds::default();

    cx.update(|cx| {
        holds.set(claude, "Claude Code".into(), cx);
        holds.set(codex, "Codex".into(), cx);
        assert!(!holds.clear(claude, cx), "the other agent is still working");
    });
    assert_eq!(cx.display_wake_reasons().len(), 1);

    cx.update(|cx| assert!(holds.clear(codex, cx)));
    assert!(cx.display_wake_reasons().is_empty());
}

/// Two agents starting must not stack two OS assertions -- the second start
/// finds the lock already held and leaves it alone.
#[gpui::test]
fn a_second_agent_does_not_stack_another_assertion(cx: &mut TestAppContext) {
    let claude = tab(cx);
    let codex = tab(cx);
    let mut holds = Holds::default();

    cx.update(|cx| {
        assert!(holds.set(claude, "Claude Code".into(), cx));
        assert!(!holds.set(codex, "Codex".into(), cx));
    });

    assert_eq!(cx.display_wake_reasons().len(), 1);
}

/// Recording the same tab twice is not an error -- `reread` can be reached
/// more than once for one tab -- and must not take a second assertion.
#[gpui::test]
fn recording_the_same_tab_twice_holds_one_lock(cx: &mut TestAppContext) {
    let claude = tab(cx);
    let mut holds = Holds::default();

    cx.update(|cx| {
        holds.set(claude, "Claude Code".into(), cx);
        holds.set(claude, "Claude Code".into(), cx);
    });
    assert_eq!(cx.display_wake_reasons().len(), 1);

    cx.update(|cx| assert!(holds.clear(claude, cx)));
    assert!(cx.display_wake_reasons().is_empty());
}

/// Clearing a tab that was never working must not disturb a lock held for
/// somebody else.
#[gpui::test]
fn clearing_an_unknown_tab_changes_nothing(cx: &mut TestAppContext) {
    let claude = tab(cx);
    let stranger = tab(cx);
    let mut holds = Holds::default();

    cx.update(|cx| {
        holds.set(claude, "Claude Code".into(), cx);
        assert!(!holds.clear(stranger, cx));
    });

    assert_eq!(cx.display_wake_reasons().len(), 1);
}

#[gpui::test]
fn dropping_the_bookkeeping_releases_the_lock(cx: &mut TestAppContext) {
    let claude = tab(cx);
    let mut holds = Holds::default();
    cx.update(|cx| holds.set(claude, "Claude Code".into(), cx));
    assert_eq!(cx.display_wake_reasons().len(), 1);

    drop(holds);

    assert!(
        cx.display_wake_reasons().is_empty(),
        "the lock must not outlive the thing holding it"
    );
}

/// The restart case: a tab whose terminal has been replaced must be looked at
/// again, even though a completion task for the dead one is still around. A
/// gate on "is a task running" answers this wrongly, and the restarted agent
/// then never holds the display for the rest of that tab's life.
#[gpui::test]
fn a_replaced_terminal_must_be_reread(cx: &mut TestAppContext) {
    let first = tab(cx);
    let second = tab(cx);

    assert!(
        needs_rereading(Some(first), Some(second)),
        "a restart swaps the terminal, so the tab has to be looked at again"
    );
    assert!(
        !needs_rereading(Some(first), Some(first)),
        "the same terminal is already being awaited"
    );
    assert!(
        needs_rereading(None, Some(first)),
        "a tab that has just started its terminal has to be picked up"
    );
    assert!(
        needs_rereading(Some(first), None),
        "a terminal that has gone away has to release its hold"
    );
    assert!(
        !needs_rereading(None, None),
        "a tab with no terminal, still with no terminal, is no news"
    );
}

#[gpui::test]
fn running_on_battery_holds_no_lock(cx: &mut TestAppContext) {
    cx.set_on_battery(Some(true));
    let claude = tab(cx);
    let mut holds = Holds::default();

    cx.update(|cx| assert!(!holds.set(claude, "Claude Code".into(), cx)));

    assert!(
        cx.display_wake_reasons().is_empty(),
        "an agent working on battery must not pin the display on"
    );
}

/// The pinned decision, and the one that is easy to get backwards: a machine
/// that cannot report a power source is almost certainly a desktop, so it must
/// keep the lock. Inverting this would disable the feature on exactly the
/// machines the request named.
#[gpui::test]
fn a_machine_that_cannot_report_its_power_source_keeps_the_lock(cx: &mut TestAppContext) {
    cx.set_on_battery(None);
    let claude = tab(cx);
    let mut holds = Holds::default();

    cx.update(|cx| assert!(holds.set(claude, "Claude Code".into(), cx)));

    assert_eq!(cx.display_wake_reasons().len(), 1);
}

/// Covers `sync` alone, as above.
#[gpui::test]
fn sync_releases_the_lock_once_the_machine_is_on_battery(cx: &mut TestAppContext) {
    cx.set_on_battery(Some(false));
    let claude = tab(cx);
    let mut holds = Holds::default();
    cx.update(|cx| holds.set(claude, "Claude Code".into(), cx));
    assert_eq!(cx.display_wake_reasons().len(), 1);

    cx.set_on_battery(Some(true));
    cx.update(|cx| assert!(holds.sync(cx), "the next check must let go"));

    assert!(cx.display_wake_reasons().is_empty());
}

/// Covers `sync` alone. The timer that would call it after the charger goes
/// back in is not exercised here -- see the note on `settled`.
#[gpui::test]
fn sync_takes_the_lock_again_once_mains_returns(cx: &mut TestAppContext) {
    cx.set_on_battery(Some(true));
    let claude = tab(cx);
    let mut holds = Holds::default();
    cx.update(|cx| holds.set(claude, "Claude Code".into(), cx));
    assert!(cx.display_wake_reasons().is_empty());

    cx.set_on_battery(Some(false));
    cx.update(|cx| assert!(holds.sync(cx)));

    assert_eq!(cx.display_wake_reasons().len(), 1);
}

#[gpui::test]
fn the_setting_being_off_refuses_the_lock(cx: &mut TestAppContext) {
    init_settings(cx);
    set_enabled(cx, false);
    let claude = tab(cx);
    let mut holds = Holds::default();

    cx.update(|cx| assert!(!holds.set(claude, "Claude Code".into(), cx)));

    assert!(
        cx.display_wake_reasons().is_empty(),
        "the setting is the user's answer and outranks a working agent"
    );
}

/// Turning the setting off has to let go of a hold already in place, not just
/// stop the next one. Without this, switching it off would appear to do
/// nothing until the agent happened to finish.
#[gpui::test]
fn turning_the_setting_off_releases_a_lock_already_held(cx: &mut TestAppContext) {
    init_settings(cx);
    set_enabled(cx, true);
    let claude = tab(cx);
    let mut holds = Holds::default();
    cx.update(|cx| holds.set(claude, "Claude Code".into(), cx));
    assert_eq!(cx.display_wake_reasons().len(), 1);

    set_enabled(cx, false);
    cx.update(|cx| assert!(holds.sync(cx)));

    assert!(cx.display_wake_reasons().is_empty());
}

#[gpui::test]
fn turning_the_setting_back_on_takes_the_lock_again(cx: &mut TestAppContext) {
    init_settings(cx);
    set_enabled(cx, false);
    let claude = tab(cx);
    let mut holds = Holds::default();
    cx.update(|cx| holds.set(claude, "Claude Code".into(), cx));
    assert!(cx.display_wake_reasons().is_empty());

    set_enabled(cx, true);
    cx.update(|cx| assert!(holds.sync(cx)));

    assert_eq!(cx.display_wake_reasons().len(), 1);
}

/// The shipped default. Asserted through a real settings store rather than
/// through the `try_get` fallback, so a change to `default.json` shows up
/// here rather than passing quietly.
#[gpui::test]
fn the_default_is_on(cx: &mut TestAppContext) {
    init_settings(cx);
    let claude = tab(cx);
    let mut holds = Holds::default();

    cx.update(|cx| assert!(holds.set(claude, "Claude Code".into(), cx)));

    assert_eq!(cx.display_wake_reasons().len(), 1);
}

/// The five states the footer icon and its menu have to tell apart. A dimmed
/// icon looks the same in four of them, so the menu's one line is the only place
/// the difference is visible -- which makes these worth pinning.
#[gpui::test]
fn the_status_names_why_the_display_is_not_held(cx: &mut TestAppContext) {
    init_settings(cx);
    let claude = tab(cx);
    let mut holds = Holds::default();

    cx.update(|cx| assert_eq!(holds.status(cx), Status::Idle, "no agent is working"));

    cx.update(|cx| holds.set(claude, "Claude Code".into(), cx));
    cx.update(|cx| assert_eq!(holds.status(cx), Status::Holding));

    cx.set_on_battery(Some(true));
    cx.update(|cx| {
        holds.sync(cx);
        assert_eq!(holds.status(cx), Status::OnBattery);
    });

    cx.set_on_battery(Some(false));
    set_enabled(cx, false);
    cx.update(|cx| {
        holds.sync(cx);
        assert_eq!(
            holds.status(cx),
            Status::Disabled,
            "the user's own answer outranks the rest"
        );
    });
}

/// A refused request is reported, not hidden: the setting is on, an agent is
/// working, the machine is plugged in, and the platform still says no.
///
/// This is no longer what Windows and Linux look like -- `crates/zed` asks
/// `can_keep_display_awake` first and builds nothing where the answer is no. It
/// is the narrower case of a platform that claims support and then fails, which
/// still has to say something truthful.
#[gpui::test]
fn a_refused_hold_is_reported_rather_than_hidden(cx: &mut TestAppContext) {
    init_settings(cx);
    set_enabled(cx, true);
    cx.set_on_battery(Some(false));
    cx.set_display_wake_supported(false);
    let claude = tab(cx);
    let mut holds = Holds::default();

    cx.update(|cx| {
        assert!(
            !holds.set(claude, "Claude Code".into(), cx),
            "nothing was taken, so nothing changed hands"
        );
        assert_eq!(holds.status(cx), Status::Unsupported);
    });
    assert!(cx.display_wake_reasons().is_empty());
}

/// The window between the charger coming out and the 60-second timer noticing.
///
/// `status` runs every frame; `sync` runs on events and on the timer. So a live
/// battery read can be `true` while the lock is still held and the screen still
/// genuinely will not sleep. Reporting `OnBattery` there would be an explanation
/// that contradicts `pmset -g assertions`. Every other test in this file calls
/// `sync` before `status`, which is exactly why none of them could catch it.
#[gpui::test]
fn a_held_lock_outranks_a_live_battery_reading(cx: &mut TestAppContext) {
    init_settings(cx);
    set_enabled(cx, true);
    cx.set_on_battery(Some(false));
    let claude = tab(cx);
    let mut holds = Holds::default();
    cx.update(|cx| holds.set(claude, "Claude Code".into(), cx));
    assert_eq!(cx.display_wake_reasons().len(), 1);

    // The charger comes out. Deliberately no `sync` -- that is the point.
    cx.set_on_battery(Some(true));

    cx.update(|cx| {
        assert_eq!(
            holds.status(cx),
            Status::Holding,
            "the assertion is still live, so the menu must not claim it is paused"
        );
    });

    // And once the timer does run, the answer changes honestly.
    cx.update(|cx| {
        holds.sync(cx);
        assert_eq!(holds.status(cx), Status::OnBattery);
    });
    assert!(cx.display_wake_reasons().is_empty());
}

/// `Disabled` is reported ahead of `Idle`: with the setting off and no agent
/// working, "turned off" is the answer worth showing.
#[gpui::test]
fn being_turned_off_outranks_being_idle(cx: &mut TestAppContext) {
    init_settings(cx);
    set_enabled(cx, false);
    let holds = Holds::default();

    cx.update(|cx| assert_eq!(holds.status(cx), Status::Disabled));
}

/// The defect this guards against: `sync` is reachable from any settings
/// change anywhere in the app, so a refused acquisition retried on every one
/// of them costs a foreground stall unrelated to what the user just did.
/// Flipping `display_wake_supported` to `true` right after the refusal proves
/// the *skip* is real -- if `sync` retried anyway, this would take the lock
/// immediately rather than waiting out the cooldown.
#[gpui::test]
fn a_failed_acquisition_is_not_retried_before_the_cooldown_elapses(cx: &mut TestAppContext) {
    init_settings(cx);
    set_enabled(cx, true);
    cx.set_on_battery(Some(false));
    cx.set_display_wake_supported(false);
    let claude = tab(cx);
    let mut holds = Holds::default();

    cx.update(|cx| assert!(!holds.set(claude, "Claude Code".into(), cx)));
    assert!(cx.display_wake_reasons().is_empty());

    // The platform would now succeed, but nothing has happened that could
    // plausibly change the outcome, and the cooldown has not elapsed.
    cx.set_display_wake_supported(true);
    cx.update(|cx| {
        assert!(!holds.sync(cx), "still cooling down from the refusal");
    });
    assert!(
        cx.display_wake_reasons().is_empty(),
        "a sync shortly after a failure must not re-attempt acquisition"
    );

    cx.executor().advance_clock(FAILED_ACQUISITION_COOLDOWN);
    cx.update(|cx| {
        assert!(
            holds.sync(cx),
            "the cooldown has elapsed, so this may retry"
        );
    });
    assert_eq!(
        cx.display_wake_reasons().len(),
        1,
        "a sync once the cooldown has elapsed must retry"
    );
}

/// The setting being toggled off and back on is one of the two events allowed
/// to end a cooldown early -- see the note on `Holds::sync`. Without it, a
/// user who notices the refusal, is told by the tooltip to check their
/// session bus, and flips the setting off and back on to force a retry would
/// instead sit out the rest of the cooldown.
#[gpui::test]
fn toggling_the_setting_ends_a_cooldown_early(cx: &mut TestAppContext) {
    init_settings(cx);
    set_enabled(cx, true);
    cx.set_on_battery(Some(false));
    cx.set_display_wake_supported(false);
    let claude = tab(cx);
    let mut holds = Holds::default();

    cx.update(|cx| assert!(!holds.set(claude, "Claude Code".into(), cx)));

    cx.set_display_wake_supported(true);
    set_enabled(cx, false);
    cx.update(|cx| assert!(!holds.sync(cx), "nothing to hold while disabled"));

    set_enabled(cx, true);
    cx.update(|cx| {
        assert!(
            holds.sync(cx),
            "toggling the setting back on must not still be cooling down"
        );
    });
    assert_eq!(cx.display_wake_reasons().len(), 1);
}

#[gpui::test]
fn the_reason_names_one_agent_and_counts_several(cx: &mut TestAppContext) {
    let claude = tab(cx);
    let codex = tab(cx);
    let mut holds = Holds::default();

    cx.update(|cx| holds.set(claude, "Claude Code".into(), cx));
    assert_eq!(holds.reason(), "Claude Code is answering");

    cx.update(|cx| holds.set(codex, "Codex".into(), cx));
    assert_eq!(holds.reason(), "2 agents are answering");
}

/// The grace that keeps one answer from flapping the lock.
///
/// An agent reading a file or waiting on a model writes nothing for seconds at
/// a time. Without the grace the lock would be taken and dropped through a
/// single answer, and each acquisition can cost a foreground stall.
mod grace {
    use super::*;

    #[gpui::test]
    fn a_tab_that_has_never_answered_holds_nothing(_cx: &mut TestAppContext) {
        assert!(!still_answering(None, Instant::now()));
    }

    #[gpui::test]
    fn a_pause_inside_an_answer_still_counts(_cx: &mut TestAppContext) {
        let now = Instant::now();
        let mid_answer = now - (RESPONDING_GRACE / 2);
        assert!(
            still_answering(Some(mid_answer), now),
            "a quiet stretch inside one answer must not release the display"
        );
    }

    #[gpui::test]
    fn an_answer_that_finished_lets_the_display_sleep(_cx: &mut TestAppContext) {
        let now = Instant::now();
        let finished = now - (RESPONDING_GRACE + Duration::from_secs(1));
        assert!(
            !still_answering(Some(finished), now),
            "walking away from an idle agent must not pin the display on"
        );
    }
}

/// The one rule this module exists to get right, end to end.
///
/// Every test above drives `Holds` directly, which proves the bookkeeping and
/// not the question feeding it. This one goes through a real workspace and a
/// real agent tab, because the two ways to get this wrong both live in that
/// path: a tab open after its CLI exited, and a CLI alive at its prompt with
/// nothing to say. Neither may hold the display.
mod a_real_tab {
    use super::*;
    use gpui::VisualTestContext;

    async fn workspace_with_keep_awake(
        cx: &mut TestAppContext,
    ) -> (
        gpui::Entity<KeepAwake>,
        gpui::Entity<workspace::Workspace>,
        &mut VisualTestContext,
    ) {
        cx.update(|cx| {
            let store = settings::SettingsStore::test(cx);
            cx.set_global(store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            editor::init(cx);
            KeepDisplayAwakeSetting::register(cx);
            project::DisableAiSettings::register(cx);
        });
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (multi, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi.read_with(cx, |multi, _| multi.workspace().clone());

        let keep_awake = workspace.update(cx, |workspace, cx| {
            let handle = cx.entity();
            cx.new(|cx| KeepAwake::new(workspace, &handle, cx))
        });
        (keep_awake, workspace, cx)
    }

    /// A tab with no CLI behind it. There is no agent binary in a test, so the
    /// tab lands on its install screen: open, and running nothing.
    #[gpui::test]
    async fn a_tab_that_runs_nothing_holds_nothing(cx: &mut TestAppContext) {
        let (keep_awake, workspace, cx) = workspace_with_keep_awake(cx).await;

        workspace.update_in(cx, |workspace, window, cx| {
            agent_ui::AgentView::open(workspace, project::CLAUDE_CODE_AGENT_ID, None, window, cx);
        });
        cx.run_until_parked();

        let tabs = workspace.read_with(cx, |workspace, cx| {
            workspace.items_of_type::<AgentView>(cx).count()
        });
        assert_eq!(tabs, 1, "the tab has to exist for this to mean anything");

        keep_awake.read_with(cx, |keep_awake, _| {
            assert!(
                !keep_awake.is_holding(),
                "an open tab is not an agent answering"
            );
        });
        assert!(
            cx.display_wake_reasons().is_empty(),
            "and nothing may reach the platform for it"
        );
    }

    /// The status has to say so too, or the indicator explains a hold that is
    /// not happening.
    #[gpui::test]
    async fn the_status_reads_idle_for_an_open_tab(cx: &mut TestAppContext) {
        let (keep_awake, workspace, cx) = workspace_with_keep_awake(cx).await;

        workspace.update_in(cx, |workspace, window, cx| {
            agent_ui::AgentView::open(workspace, project::CLAUDE_CODE_AGENT_ID, None, window, cx);
        });
        cx.run_until_parked();

        keep_awake.read_with(cx, |keep_awake, cx| {
            assert_eq!(keep_awake.status(cx), Status::Idle);
        });
    }
}
