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
//! The agent half of that gap is still open, and honestly so. Proving that a
//! *live* CLI sitting at its prompt holds nothing needs a real process in the
//! test, and the pty tests in this tree are flaky enough already. `grace`
//! covers the rule that decides it; nothing covers the wiring that asks.
//!
//! The terminal half does not have that excuse: a plain terminal has no
//! install screen to hide behind, so `a_real_terminal` drives an actual
//! spawned shell and pays the real-process cost `grace` alone cannot stand in
//! for.

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

    assert_eq!(
        cx.display_wake_reasons(),
        vec!["Claude Code is producing output"]
    );
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
    assert_eq!(holds.reason(), "Claude Code is producing output");

    cx.update(|cx| holds.set(codex, "Codex".into(), cx));
    assert_eq!(holds.reason(), "2 tabs are producing output");
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

    /// `pub(super)`: `a_real_terminal` reuses this rather than standing up its
    /// own workspace, since the two ways to get either rule wrong both need the
    /// same real workspace and the same real `KeepAwake`.
    pub(super) async fn workspace_with_keep_awake(
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
            terminal_view::init(cx);
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

/// The terminal half of the same rule `a_real_tab` proves for agents: a tab
/// being open is not a tab producing output, whichever kind of tab it is.
///
/// Real, not simulated -- there is no public seam to inject a fake pty write
/// into a `Terminal` built outside its own crate, so these drive an actual
/// spawned shell the way `terminal_view`'s and `container_ui`'s own real-pty
/// tests do, and pay the same real-process cost they do.
mod a_real_terminal {
    use gpui::VisualTestContext;
    use terminal::Terminal;
    use terminal_view::terminal_panel::TerminalPanel;

    use super::a_real_tab::workspace_with_keep_awake;
    use super::*;

    /// Spawns a real shell as a centre-pane terminal tab and returns its
    /// `Terminal` entity so a test can drive real writes into it through
    /// `Terminal::input`.
    ///
    /// Centre pane, not the dock: `TerminalPanel::add_center_terminal` is one
    /// of two paths to a real terminal this crate can reach from outside
    /// `terminal_view` -- `add_terminal_shell` and its dock-revealing siblings
    /// are private to that crate. `rescan` reads both centre and dock through
    /// the same `TerminalView`; `a_real_dock_terminal` below exercises the
    /// dock half through `TerminalPanel::add_terminal_task`, the other public
    /// path, since this one never registers a panel to put one in.
    pub(super) async fn spawn_terminal(
        workspace: &gpui::Entity<workspace::Workspace>,
        cx: &mut VisualTestContext,
    ) -> gpui::Entity<Terminal> {
        let terminal = workspace
            .update_in(cx, |workspace, window, cx| {
                TerminalPanel::add_center_terminal(workspace, window, cx, |project, cx| {
                    project.create_terminal_shell(None, cx)
                })
            })
            .await
            .expect("a real shell must spawn")
            .upgrade()
            .expect("the terminal must still be alive right after spawning");
        cx.run_until_parked();
        terminal
    }

    /// Sends one line to the shell and polls its real output until that
    /// line's own echo shows up, so the caller knows this write actually
    /// reached the terminal before it goes on to send the next one.
    ///
    /// Polling rather than a single `run_until_parked`: the pty round trip
    /// runs on a real thread against a real child process, which
    /// `run_until_parked` does not block for -- it only drains work already
    /// scheduled, and returns the instant nothing is queued yet, whether or
    /// not the shell has replied. `wait_for_terminal_content` in `terminal`'s
    /// own tests exists for the identical race; this is the same fix, sized
    /// to milliseconds so sixteen of them still land inside
    /// [`RESPONDING_WINDOW`].
    pub(super) async fn send_line(
        terminal: &gpui::Entity<Terminal>,
        marker: &str,
        cx: &mut VisualTestContext,
    ) {
        terminal.update(cx, |terminal, _cx| {
            terminal.input(format!("echo {marker}\n").into_bytes())
        });
        for _ in 0..500 {
            let content = terminal.update(cx, |terminal, _cx| terminal.get_content());
            if content.contains(marker) {
                return;
            }
            cx.background_executor.timer(Duration::from_millis(1)).await;
        }
        panic!("timed out waiting for {marker:?} in the terminal's own output");
    }

    /// Sends `count` distinguishable lines in a row, waiting for each one's
    /// echo before sending the next.
    pub(super) async fn send_lines(
        terminal: &gpui::Entity<Terminal>,
        count: usize,
        cx: &mut VisualTestContext,
    ) {
        for i in 0..count {
            send_line(terminal, &format!("keep-awake-{i}"), cx).await;
        }
    }

    #[gpui::test]
    #[ignore = "spawns a real shell: the write rate it observes depends on the \
machine keeping up, and a loaded one can smear a burst under the threshold or \
bunch a repaint over it"]
    async fn a_shell_sitting_at_its_prompt_holds_nothing(cx: &mut TestAppContext) {
        // `add_center_terminal` ends in a real PTY spawn, which parks.
        cx.executor().allow_parking();
        let (keep_awake, workspace, cx) = workspace_with_keep_awake(cx).await;
        spawn_terminal(&workspace, cx).await;

        cx.executor().advance_clock(Duration::from_secs(2));
        cx.run_until_parked();

        keep_awake.read_with(cx, |keep_awake, _| {
            assert!(
                !keep_awake.is_holding(),
                "a shell at its prompt writes nothing, so it must hold nothing"
            );
        });
        assert!(
            cx.display_wake_reasons().is_empty(),
            "and nothing may reach the platform for it"
        );
    }

    #[gpui::test]
    #[ignore = "needs the machine to keep up: crossing the write-rate threshold \
with a real shell depends on wall-clock timing, which the test scheduler folds \
into the virtual clock"]
    async fn a_terminal_producing_output_holds_the_display(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (keep_awake, workspace, cx) = workspace_with_keep_awake(cx).await;
        let terminal = spawn_terminal(&workspace, cx).await;

        send_lines(&terminal, 16, cx).await;
        cx.executor().advance_clock(ACTIVITY_CHECK_INTERVAL);
        cx.run_until_parked();

        keep_awake.read_with(cx, |keep_awake, _| {
            assert!(
                keep_awake.is_holding(),
                "steady output from a real shell must hold the display"
            );
            assert_eq!(
                keep_awake.holders().count(),
                1,
                "one terminal writing must not pin two holders"
            );
        });
    }

    #[gpui::test]
    #[ignore = "needs the machine to keep up: crossing the write-rate threshold \
with a real shell depends on wall-clock timing, which the test scheduler folds \
into the virtual clock"]
    async fn output_that_stops_lets_the_display_sleep(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (keep_awake, workspace, cx) = workspace_with_keep_awake(cx).await;
        let terminal = spawn_terminal(&workspace, cx).await;

        send_lines(&terminal, 16, cx).await;
        cx.executor().advance_clock(ACTIVITY_CHECK_INTERVAL);
        cx.run_until_parked();
        keep_awake.read_with(cx, |keep_awake, _| {
            assert!(
                keep_awake.is_holding(),
                "the setup must actually be holding first"
            );
        });

        // Only reachable because `pty_writes_within` ages out on the
        // executor's own clock: it moves when this advances, where the wall
        // clock would have gone on running in real time regardless.
        cx.executor()
            .advance_clock(RESPONDING_GRACE + Duration::from_secs(1));
        cx.run_until_parked();

        keep_awake.read_with(cx, |keep_awake, _| {
            assert!(
                !keep_awake.is_holding(),
                "output stopped a full grace period ago, so the hold must have expired"
            );
        });
        assert!(cx.display_wake_reasons().is_empty());
    }

    /// The counterpart to steady output: a slow repaint stays under
    /// `RESPONDING_WRITES` inside any one-second window because each write is
    /// its own second apart, and must never cross into holding.
    #[gpui::test]
    #[ignore = "spawns a real shell: the write rate it observes depends on the \
machine keeping up, and a loaded one can smear a burst under the threshold or \
bunch a repaint over it"]
    async fn a_slow_repaint_never_holds(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (keep_awake, workspace, cx) = workspace_with_keep_awake(cx).await;
        let terminal = spawn_terminal(&workspace, cx).await;

        for i in 0..5 {
            send_line(&terminal, &format!("slow-repaint-{i}"), cx).await;
            cx.executor().advance_clock(ACTIVITY_CHECK_INTERVAL);
            cx.run_until_parked();
            keep_awake.read_with(cx, |keep_awake, _| {
                assert!(
                    !keep_awake.is_holding(),
                    "one write a second is a repaint, not a stream"
                );
            });
        }
    }

    /// The poll must not run forever once nothing is left to ask about --
    /// otherwise an editor with one terminal that was briefly busy, once, keeps
    /// a task alive for the rest of the session.
    #[gpui::test]
    #[ignore = "needs the machine to keep up: crossing the write-rate threshold \
with a real shell depends on wall-clock timing, which the test scheduler folds \
into the virtual clock"]
    async fn the_poll_stops_once_every_terminal_is_quiet(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (keep_awake, workspace, cx) = workspace_with_keep_awake(cx).await;
        let terminal = spawn_terminal(&workspace, cx).await;

        send_lines(&terminal, 16, cx).await;
        cx.executor().advance_clock(ACTIVITY_CHECK_INTERVAL);
        cx.run_until_parked();
        keep_awake.read_with(cx, |keep_awake, _| {
            assert!(
                keep_awake.is_polling(),
                "output just arrived, so the poll must be running"
            );
        });

        cx.executor()
            .advance_clock(RESPONDING_GRACE + Duration::from_secs(1));
        cx.run_until_parked();

        keep_awake.read_with(cx, |keep_awake, _| {
            assert!(
                !keep_awake.is_polling(),
                "nothing is left to poll for once every terminal has gone quiet"
            );
        });
    }
}

/// The dock half. Every other test here drives a centre-pane terminal, because
/// `add_terminal_shell` and its dock-revealing siblings are private to
/// `terminal_view` — so without this, `rescan`'s dock branch would never have
/// seen a real dock terminal in any test.
///
/// `TerminalPanel::add_terminal_task` is the one public path this crate can
/// reach that lands a real terminal in the panel's own pane -- the dock, not
/// the workspace's centre pane `add_center_terminal` reaches -- which is
/// exactly the gap: people run commands in the terminal dock, and reaching
/// dock terminals is the entire reason `TerminalPanelEvent::TerminalsChanged`
/// exists.
mod a_real_dock_terminal {
    use gpui::VisualTestContext;
    use task::{RevealStrategy, SpawnInTerminal};
    use terminal::Terminal;
    use terminal_view::terminal_panel::TerminalPanel;

    use super::a_real_tab::workspace_with_keep_awake;
    use super::a_real_terminal::send_lines;
    use super::*;

    /// Stands up a real `TerminalPanel` and registers it with the workspace,
    /// then spawns a real shell into the panel's own pane through
    /// `add_terminal_task`.
    ///
    /// Registering the panel here, after `KeepAwake` already exists, matters:
    /// `KeepAwake::new`'s initial sweep only reads `workspace.items_of_type`,
    /// not the panel, and only takes the panel's own terminals into account
    /// once `workspace::Event::PanelAdded` fires and its handler both
    /// subscribes to `TerminalPanelEvent` and calls `rescan`. A panel already
    /// present before `KeepAwake` is built would never fire that event and
    /// would leave its terminals invisible for the rest of the test.
    ///
    /// Built directly through `TerminalPanel::new` rather than `::load`,
    /// which is async and wants a `KeepValueStore` global this test has no
    /// reason to stand up -- the same shortcut `container_panel_tests.rs`
    /// takes for the same reason.
    async fn spawn_dock_terminal(
        workspace: &gpui::Entity<workspace::Workspace>,
        cx: &mut VisualTestContext,
    ) -> gpui::Entity<Terminal> {
        let terminal_panel = workspace.update_in(cx, |workspace, window, cx| {
            cx.new(|cx| TerminalPanel::new(workspace, window, cx))
        });
        workspace.update_in(cx, |workspace, window, cx| {
            workspace.add_panel(terminal_panel.clone(), window, cx)
        });
        cx.run_until_parked();

        let terminal = terminal_panel
            .update_in(cx, |panel, window, cx| {
                panel.add_terminal_task(
                    SpawnInTerminal {
                        full_label: "keep-awake dock terminal".into(),
                        reveal: RevealStrategy::Never,
                        ..SpawnInTerminal::default()
                    },
                    RevealStrategy::Never,
                    window,
                    cx,
                )
            })
            .await
            .expect("a real shell must spawn in the dock")
            .upgrade()
            .expect("the terminal must still be alive right after spawning");
        cx.run_until_parked();
        terminal
    }

    /// The main case dock discovery exists for: someone running commands in
    /// the terminal dock, not the editor's centre pane.
    #[gpui::test]
    #[ignore = "needs the machine to keep up: crossing the write-rate threshold \
with a real shell depends on wall-clock timing, which the test scheduler folds \
into the virtual clock"]
    async fn a_dock_terminal_producing_output_holds_the_display(cx: &mut TestAppContext) {
        // The panel setup and the task spawn both end in a real PTY spawn,
        // which parks.
        cx.executor().allow_parking();
        let (keep_awake, workspace, cx) = workspace_with_keep_awake(cx).await;
        let terminal = spawn_dock_terminal(&workspace, cx).await;

        send_lines(&terminal, 16, cx).await;
        cx.executor().advance_clock(ACTIVITY_CHECK_INTERVAL);
        cx.run_until_parked();

        keep_awake.read_with(cx, |keep_awake, _| {
            assert!(
                keep_awake.is_holding(),
                "rescan's dock branch and the TerminalsChanged subscription must \
                 reach a real terminal sitting in the dock, not just the centre pane"
            );
        });
        assert_eq!(cx.display_wake_reasons().len(), 1);
    }
}

/// The gap neither `keep_awake`'s own tests nor `gpui`'s own tests close on
/// their own: two `KeepAwake` entities in one process, each taking a lock, and
/// one of them releasing before the other. `App::keep_display_awake` refcounts
/// every caller down to one platform assertion (see its own doc), and
/// `cx.display_wake_reasons()` is process-wide in the test platform
/// (`gpui/src/platform/test/platform.rs`), which is exactly what makes a
/// two-workspace assertion observable from here.
///
/// This is the direct expression of the Windows defect the refactor fixes:
/// `SetThreadExecutionState` overwrites rather than nests, so two
/// platform-level locks was never a wasteful duplicate there, it was a broken
/// hold -- the first lock's release could silently cancel the second's.
mod several_workspaces {
    use super::a_real_terminal::{send_lines, spawn_terminal};
    use super::*;

    /// Two workspaces each holding must reach the platform once.
    ///
    /// Driven through `Holds` rather than two real shells on purpose. What is
    /// under test here is arithmetic -- that a second holder finds the
    /// assertion already taken -- and a real pty drags in an independently
    /// scheduled OS thread whose output can be delivered late and stamped
    /// against an already-advanced test clock, which is a source of
    /// flakiness and proves nothing extra about the count. That terminals
    /// reach `Holds` at all is what `a_real_terminal` covers, with a real
    /// shell, in the centre pane and in the dock.
    #[gpui::test]
    fn two_workspaces_take_one_assertion(cx: &mut TestAppContext) {
        let first_tab = tab(cx);
        let second_tab = tab(cx);
        // Two separate `Holds` is what two workspaces are: each `KeepAwake`
        // keeps its own, and neither can see the other's.
        let mut first_workspace = Holds::default();
        let mut second_workspace = Holds::default();

        cx.update(|cx| {
            assert!(first_workspace.set(first_tab, "build".into(), cx));
            assert!(second_workspace.set(second_tab, "test".into(), cx));
        });

        assert_eq!(
            cx.display_wake_reasons().len(),
            1,
            "two workspaces holding at once must still be one platform assertion"
        );
    }

    /// One workspace going quiet must not take the assertion the other still
    /// needs, and the last one out must release it.
    #[gpui::test]
    fn one_workspace_releasing_leaves_the_other_held(cx: &mut TestAppContext) {
        let first_tab = tab(cx);
        let second_tab = tab(cx);
        let mut first_workspace = Holds::default();
        let mut second_workspace = Holds::default();

        cx.update(|cx| {
            first_workspace.set(first_tab, "build".into(), cx);
            second_workspace.set(second_tab, "test".into(), cx);
        });

        // Asserted before the release as well as after: with an assertion per
        // workspace rather than one shared, clearing the first would also
        // leave exactly one behind, and this test would pass over the very
        // defect it exists to catch.
        assert_eq!(
            cx.display_wake_reasons().len(),
            1,
            "both workspaces working is still one platform assertion"
        );

        cx.update(|cx| first_workspace.clear(first_tab, cx));

        assert_eq!(
            cx.display_wake_reasons().len(),
            1,
            "the workspace still working must keep the display lit"
        );

        cx.update(|cx| second_workspace.clear(second_tab, cx));

        assert!(
            cx.display_wake_reasons().is_empty(),
            "the last workspace out releases the assertion"
        );
    }

    /// The activity poll is spawned on the app foreground executor in
    /// `settled` (`keep_awake.rs`'s own doc on the field), not bound to a
    /// window, so it keeps ticking for a window that is not focused. Proven
    /// with `VisualTestContext::deactivate_window` directly, rather than a
    /// second window fighting the first for platform focus, since blurring
    /// the one window under test is the whole of what "unfocused" means here.
    #[gpui::test]
    #[ignore = "spawns a real shell: the write rate it observes depends on the \
machine keeping up, and a loaded one can smear a burst under the threshold or \
bunch a repaint over it"]
    async fn an_unfocused_window_still_holds(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (keep_awake, workspace, cx) = super::a_real_tab::workspace_with_keep_awake(cx).await;
        let terminal = spawn_terminal(&workspace, cx).await;

        cx.deactivate_window();
        cx.run_until_parked();

        send_lines(&terminal, 16, cx).await;
        cx.executor().advance_clock(ACTIVITY_CHECK_INTERVAL);
        cx.run_until_parked();

        keep_awake.read_with(cx, |keep_awake, _| {
            assert!(
                keep_awake.is_holding(),
                "the activity poll is not bound to window focus, so an \
                 unfocused window's terminal must still hold"
            );
        });
        assert_eq!(cx.display_wake_reasons().len(), 1);
    }
}
