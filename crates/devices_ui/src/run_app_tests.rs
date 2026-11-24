use std::path::PathBuf;
use std::time::Duration;

use devices::DeviceKey;
use gpui::{Entity, TestAppContext, VisualTestContext};
use project::trusted_worktrees::{PathTrust, TrustedWorktrees};
use workspace::dock::Panel as _;

use crate::devices_panel::DevicesPanel;
use crate::run_app_test_support::{
    FakeRunState, Trust, add_expo_app, add_flutter_app, add_react_native_app, app_in, click_run,
    detected, error_of, flutter_project, reads_under, run_app, run_panel,
};
use crate::test_support::{
    Rig, SIMULATOR_A, SIMULATOR_B, XCRUN, advance, draw, init_test, panel_for_project,
    panel_in_window,
};

const RUN_A: &str = "sim-11111111-1111-1111-1111-111111111111-run";
const APPS_A: &str = "sim-11111111-1111-1111-1111-111111111111-apps";

fn key(udid: &str) -> DeviceKey {
    DeviceKey::Simulator(udid.to_string())
}

fn open(panel: &Entity<DevicesPanel>, cx: &mut VisualTestContext) {
    panel.update_in(cx, |panel, window, cx| panel.set_active(true, window, cx));
    cx.run_until_parked();
}

fn both_booted() -> Rig {
    let rig = Rig::mac();
    rig.script_simulator_list("Booted", "Booted");
    rig
}

fn active_runs(panel: &Entity<DevicesPanel>, cx: &mut VisualTestContext) -> usize {
    panel.read_with(cx, |panel, _| panel.active_runs.len())
}

#[gpui::test]
async fn without_a_trust_store_nothing_is_read_and_nothing_runs(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    add_flutter_app(&rig.host, "/app", true);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::NoStore).await;
    open(&panel, cx);
    assert!(detected(&panel, cx).is_empty());
    assert_eq!(reads_under(&rig, "/app"), 0, "no project file was read");

    run_app(&panel, &key(SIMULATOR_A), &flutter_project("/app"), cx);

    assert_eq!(terminals.spawn_count(), 0);
    assert_eq!(reads_under(&rig, "/app"), 0);
    assert!(error_of(&panel, &key(SIMULATOR_A), cx).contains("trust"));
    let note = panel.read_with(cx, |panel, cx| panel.run_note(cx));
    assert!(note.is_some_and(|note| note.contains("trust")));
}

#[gpui::test]
async fn an_untrusted_folder_lists_no_app_and_refuses_a_click(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    add_flutter_app(&rig.host, "/app", true);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&[])).await;
    open(&panel, cx);
    draw(cx);
    assert!(detected(&panel, cx).is_empty());
    assert!(cx.debug_bounds(RUN_A).is_none());
    assert!(cx.debug_bounds("devices-run-note").is_some());

    run_app(&panel, &key(SIMULATOR_A), &flutter_project("/app"), cx);

    assert_eq!(terminals.spawn_count(), 0);
    assert_eq!(reads_under(&rig, "/app"), 0, "no project file was read");
    assert!(error_of(&panel, &key(SIMULATOR_A), cx).contains("Trust"));
}

#[gpui::test]
async fn a_trusted_folder_runs_flutter_on_a_booted_simulator(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    add_flutter_app(&rig.host, "/app", true);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);
    draw(cx);
    assert!(cx.debug_bounds(RUN_A).is_some());
    assert!(cx.debug_bounds("devices-run-note").is_none());

    click_run(&panel, &key(SIMULATOR_A), cx);

    assert_eq!(
        terminals.command_lines(),
        [["flutter", "run", "-d", SIMULATOR_A]]
    );
    let task = terminals.spawned.borrow()[0].clone();
    assert_eq!(task.cwd, Some(PathBuf::from("/app")));
    assert!(task.use_new_terminal && task.allow_concurrent_runs);
    assert_eq!(active_runs(&panel, cx), 1);
}

#[gpui::test]
async fn the_folder_holding_the_app_is_the_one_checked(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    add_flutter_app(&rig.host, "/trusted", true);
    add_flutter_app(&rig.host, "/untrusted", true);
    let (panel, terminals, cx) = run_panel(
        cx,
        &rig,
        &["/trusted", "/untrusted"],
        Trust::Only(&["/trusted"]),
    )
    .await;
    open(&panel, cx);
    let directories: Vec<PathBuf> = detected(&panel, cx)
        .into_iter()
        .map(|app| app.directory)
        .collect();
    assert_eq!(directories, [PathBuf::from("/trusted")]);
    assert_eq!(reads_under(&rig, "/untrusted"), 0);

    run_app(
        &panel,
        &key(SIMULATOR_A),
        &flutter_project("/untrusted"),
        cx,
    );
    assert_eq!(terminals.spawn_count(), 0);
    assert_eq!(reads_under(&rig, "/untrusted"), 0);

    run_app(&panel, &key(SIMULATOR_A), &flutter_project("/trusted"), cx);
    assert_eq!(terminals.spawn_count(), 1);
}

#[gpui::test]
async fn a_second_click_on_a_live_run_shows_it_instead_of_starting_another(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let rig = both_booted();
    add_flutter_app(&rig.host, "/app", true);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);

    click_run(&panel, &key(SIMULATOR_A), cx);
    click_run(&panel, &key(SIMULATOR_A), cx);

    assert_eq!(terminals.spawn_count(), 1);
    assert_eq!(terminals.run(0).reveals.get(), 1);
}

#[gpui::test]
async fn a_finished_run_starts_again_under_a_new_label(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    add_flutter_app(&rig.host, "/app", true);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);

    click_run(&panel, &key(SIMULATOR_A), cx);
    terminals.run(0).state.set(FakeRunState::Finished);
    click_run(&panel, &key(SIMULATOR_A), cx);

    assert_eq!(terminals.spawn_count(), 2);
    let spawned = terminals.spawned.borrow();
    assert_ne!(spawned[0].full_label, spawned[1].full_label);
    assert_ne!(spawned[0].id, spawned[1].id);
}

#[gpui::test]
async fn a_closed_terminal_is_forgotten(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    add_flutter_app(&rig.host, "/app", true);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);
    click_run(&panel, &key(SIMULATOR_A), cx);
    assert_eq!(active_runs(&panel, cx), 1);

    terminals.run(0).state.set(FakeRunState::Closed);
    panel.update(cx, |panel, cx| panel.prune_finished_runs(cx));

    assert_eq!(active_runs(&panel, cx), 0);
}

#[gpui::test]
async fn expo_runs_on_one_device_per_project_and_flutter_on_several(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    add_expo_app(&rig.host, "/work/expo_app", true);
    add_flutter_app(&rig.host, "/work/flutter_app", true);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/work"], Trust::Only(&["/work"])).await;
    open(&panel, cx);
    let expo = app_in(&panel, "/work/expo_app", cx);
    let flutter = app_in(&panel, "/work/flutter_app", cx);

    run_app(&panel, &key(SIMULATOR_A), &expo, cx);
    run_app(&panel, &key(SIMULATOR_B), &expo, cx);
    assert_eq!(terminals.spawn_count(), 1);
    let refusal = error_of(&panel, &key(SIMULATOR_B), cx);
    assert!(refusal.contains("iPhone A"), "{refusal}");

    run_app(&panel, &key(SIMULATOR_A), &flutter, cx);
    run_app(&panel, &key(SIMULATOR_B), &flutter, cx);
    assert_eq!(terminals.spawn_count(), 3);
}

#[gpui::test]
async fn two_apps_are_asked_about_on_every_click(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    add_expo_app(&rig.host, "/work/expo_app", true);
    add_flutter_app(&rig.host, "/work/flutter_app", true);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/work"], Trust::Only(&["/work"])).await;
    open(&panel, cx);

    click_run(&panel, &key(SIMULATOR_A), cx);
    draw(cx);
    assert!(cx.debug_bounds(APPS_A).is_some());
    assert_eq!(terminals.spawn_count(), 0);

    panel.update_in(cx, |panel, window, cx| panel.choose_app(1, window, cx));
    cx.run_until_parked();
    assert_eq!(terminals.spawn_count(), 1);
    assert!(panel.read_with(cx, |panel, _| panel.app_choice.is_none()));

    click_run(&panel, &key(SIMULATOR_A), cx);
    assert!(
        panel.read_with(cx, |panel, _| panel.app_choice.is_some()),
        "the last choice is not remembered"
    );
}

#[gpui::test]
async fn a_device_id_a_shell_would_rewrite_never_reaches_the_terminal(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::mac();
    let entry = |udid: &str, name: &str| {
        format!(
            r#"{{"udid":"{udid}","name":"{name}","state":"Booted","isAvailable":true,"deviceTypeIdentifier":"com.apple.CoreSimulator.SimDeviceType.iPhone-16"}}"#
        )
    };
    rig.host.script_output(
        XCRUN,
        ["simctl", "list", "devices", "available", "-j"],
        0,
        &format!(
            r#"{{"devices":{{"com.apple.CoreSimulator.SimRuntime.iOS-18-0":[{},{},{}]}}}}"#,
            entry("-help", "Dash"),
            entry("$HOME", "Dollar"),
            entry("100%", "Percent")
        ),
        "",
    );
    add_flutter_app(&rig.host, "/app", true);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);

    for udid in ["-help", "$HOME", "100%"] {
        click_run(&panel, &key(udid), cx);
        let refusal = error_of(&panel, &key(udid), cx);
        assert!(refusal.contains("cannot be passed"), "{udid}: {refusal}");
    }
    assert_eq!(terminals.spawn_count(), 0);
}

#[gpui::test]
async fn a_collab_guest_cannot_run(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    add_flutter_app(&rig.host, "/app", true);
    let (panel, cx) = panel_in_window(cx, &rig, true).await;
    let terminals = std::rc::Rc::new(crate::run_app_test_support::FakeTerminals::default());
    panel.update(cx, |panel, _| panel.run_terminals = terminals.clone());
    open(&panel, cx);

    run_app(&panel, &key(SIMULATOR_A), &flutter_project("/app"), cx);

    assert_eq!(terminals.spawn_count(), 0);
    assert_eq!(reads_under(&rig, "/app"), 0);
    assert!(error_of(&panel, &key(SIMULATOR_A), cx).contains("this computer"));
}

#[gpui::test]
async fn a_remote_project_cannot_run(cx: &mut TestAppContext, server_cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    add_flutter_app(&rig.host, "/app", true);
    let fs = fs::FakeFs::new(cx.executor());
    let (project, _server) = project::Project::test_remote(fs, cx, server_cx).await;
    let (panel, cx) = panel_for_project(cx, &rig, project);
    let terminals = std::rc::Rc::new(crate::run_app_test_support::FakeTerminals::default());
    panel.update(cx, |panel, _| panel.run_terminals = terminals.clone());

    run_app(&panel, &key(SIMULATOR_A), &flutter_project("/app"), cx);

    assert_eq!(terminals.spawn_count(), 0);
    assert_eq!(reads_under(&rig, "/app"), 0);
    assert!(error_of(&panel, &key(SIMULATOR_A), cx).contains("this computer"));
}

#[gpui::test]
async fn a_stopped_simulator_boots_first_and_the_run_follows_even_when_hidden(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let rig = Rig::mac_booting();
    rig.script_simulator_list("Shutdown", "Shutdown");
    rig.script_simulator_list("Booted", "Shutdown");
    add_flutter_app(&rig.host, "/app", true);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);

    click_run(&panel, &key(SIMULATOR_A), cx);
    click_run(&panel, &key(SIMULATOR_A), cx);
    assert_eq!(
        rig.command_count(XCRUN, &["simctl", "boot", SIMULATOR_A]),
        1
    );
    assert_eq!(terminals.spawn_count(), 0, "nothing runs before the boot");

    panel.update_in(cx, |panel, window, cx| panel.set_active(false, window, cx));
    advance(cx, Duration::from_secs(3));

    assert_eq!(
        terminals.command_lines(),
        [["flutter", "run", "-d", SIMULATOR_A]]
    );
}

#[gpui::test]
async fn a_boot_that_never_finishes_runs_nothing(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::mac_booting();
    rig.script_simulator_list("Shutdown", "Shutdown");
    add_flutter_app(&rig.host, "/app", true);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);

    click_run(&panel, &key(SIMULATOR_A), cx);
    advance(cx, Duration::from_secs(600));

    assert_eq!(terminals.spawn_count(), 0);
    assert!(!error_of(&panel, &key(SIMULATOR_A), cx).is_empty());
    assert!(panel.read_with(cx, |panel, _| panel.starting_runs.is_empty()));
}

#[gpui::test]
async fn expo_asks_before_prebuild_and_cancel_runs_nothing(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    add_expo_app(&rig.host, "/app", false);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);

    click_run(&panel, &key(SIMULATOR_A), cx);
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(terminals.spawn_count(), 0);

    click_run(&panel, &key(SIMULATOR_A), cx);
    cx.simulate_prompt_answer("Run");
    cx.run_until_parked();
    assert_eq!(
        terminals.command_lines(),
        [["npx", "--no-install", "expo", "run:ios", "-d", SIMULATOR_A]]
    );
}

#[gpui::test]
async fn react_native_starts_metro_once_per_project(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    add_react_native_app(&rig.host, "/app");
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);

    click_run(&panel, &key(SIMULATOR_A), cx);
    let lines = terminals.command_lines();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].contains(&"start".to_string()), "Metro first");
    assert!(lines[1].contains(&"run-ios".to_string()));

    terminals.run(1).state.set(FakeRunState::Finished);
    click_run(&panel, &key(SIMULATOR_A), cx);
    let lines = terminals.command_lines();
    assert_eq!(lines.len(), 3, "Metro is still running: {lines:?}");
    assert!(lines[2].contains(&"run-ios".to_string()));
}

#[gpui::test]
async fn no_fitting_app_means_no_run_button(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    add_flutter_app(&rig.host, "/app", false);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);
    draw(cx);

    assert_eq!(detected(&panel, cx).len(), 1, "an Android-only app");
    assert!(cx.debug_bounds(RUN_A).is_none());
    click_run(&panel, &key(SIMULATOR_A), cx);
    assert_eq!(terminals.spawn_count(), 0);
}

#[gpui::test]
async fn trust_withdrawn_while_the_device_boots_stops_the_run(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::mac_booting();
    rig.script_simulator_list("Shutdown", "Shutdown");
    rig.script_simulator_list("Booted", "Shutdown");
    add_flutter_app(&rig.host, "/app", true);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);
    click_run(&panel, &key(SIMULATOR_A), cx);
    let reads_before_the_wait = reads_under(&rig, "/app");

    let project = panel.read_with(cx, |panel, _| panel.project.upgrade().expect("project"));
    let worktree_store = project.read_with(cx, |project, _| project.worktree_store());
    let restricted = worktree_store.read_with(cx, |worktree_store, cx| {
        worktree_store
            .worktrees()
            .map(|worktree| PathTrust::Worktree(worktree.read(cx).id()))
            .collect()
    });
    let store = cx
        .update(|_, cx| TrustedWorktrees::try_get_global(cx))
        .expect("trust store");
    store.update(cx, |store, cx| {
        store.restrict(worktree_store.downgrade(), restricted, cx)
    });
    advance(cx, Duration::from_secs(3));

    assert_eq!(terminals.spawn_count(), 0);
    assert!(error_of(&panel, &key(SIMULATOR_A), cx).contains("Trust"));
    assert_eq!(
        reads_under(&rig, "/app"),
        reads_before_the_wait,
        "no project file is read once trust is gone"
    );
}

#[gpui::test]
async fn opening_the_panel_leaves_an_undecided_folder_undecided(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    add_flutter_app(&rig.host, "/app", true);
    let (panel, _terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Undecided).await;
    let store = cx
        .update(|_, cx| TrustedWorktrees::try_get_global(cx))
        .expect("trust store");
    let events = std::rc::Rc::new(std::cell::Cell::new(0));
    cx.update(|_, cx| {
        let events = events.clone();
        cx.subscribe(
            &store,
            move |_, _: &project::trusted_worktrees::TrustedWorktreesEvent, _| {
                events.set(events.get() + 1)
            },
        )
        .detach();
    });

    open(&panel, cx);
    panel.update(cx, |panel, cx| panel.reload_fresh(cx));
    cx.run_until_parked();

    let project = panel.read_with(cx, |panel, _| panel.project.upgrade().expect("project"));
    let worktree_store = project.read_with(cx, |project, _| project.worktree_store());
    let restricted = store.read_with(cx, |store, cx| {
        store.restricted_worktrees(&worktree_store, cx).len()
    });
    assert_eq!(restricted, 0, "opening the panel decided nothing");
    assert_eq!(events.get(), 0, "and told nobody it had");
}

#[gpui::test]
async fn a_device_booting_outside_zode_offers_no_run_until_it_is_up(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::mac();
    rig.script_simulator_list("Booting", "Booted");
    add_flutter_app(&rig.host, "/app", true);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);
    draw(cx);

    assert!(
        cx.debug_bounds(RUN_A).is_none(),
        "no Run while it boots elsewhere"
    );
    assert!(
        cx.debug_bounds("sim-22222222-2222-2222-2222-222222222222-run")
            .is_some()
    );
    run_app(&panel, &key(SIMULATOR_A), &flutter_project("/app"), cx);
    assert_eq!(terminals.spawn_count(), 0);
    let error = error_of(&panel, &key(SIMULATOR_A), cx);
    assert!(error.contains("still starting"), "{error}");
}

#[gpui::test]
async fn an_older_read_never_replaces_a_newer_one(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::mac();
    rig.script_simulator_list("Shutdown", "Shutdown");
    rig.script_simulator_list("Shutdown", "Shutdown");
    rig.script_simulator_list("Booted", "Shutdown");
    let (panel, _terminals, cx) = run_panel(cx, &rig, &[], Trust::NoStore).await;
    open(&panel, cx);

    let held = rig
        .runner
        .hold(&["simctl", "list", "devices", "available", "-j"]);
    panel.update(cx, |panel, cx| panel.reload_fresh(cx));
    cx.run_until_parked();
    let generation = panel.update(cx, |panel, _| panel.begin_read());
    let fresher = smol::block_on(devices::discover(&*rig.host, &*rig.host));
    panel.update(cx, |panel, _| panel.apply_read(generation, fresher));
    assert_eq!(
        panel.read_with(cx, |panel, _| panel
            .find_device(&key(SIMULATOR_A))
            .map(|device| device.state.clone())),
        Some(devices::DeviceState::Running)
    );

    drop(held);
    cx.run_until_parked();
    assert_eq!(
        panel.read_with(cx, |panel, _| panel
            .find_device(&key(SIMULATOR_A))
            .map(|device| device.state.clone())),
        Some(devices::DeviceState::Running),
        "the Reload that started first landed last and was dropped"
    );
}

#[gpui::test]
async fn a_run_that_cannot_start_after_metro_says_metro_is_still_running(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    add_react_native_app(&rig.host, "/app");
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);
    terminals.fail_from.set(Some(1));

    click_run(&panel, &key(SIMULATOR_A), cx);

    assert_eq!(terminals.spawn_count(), 2, "Metro, then the failed run");
    let error = error_of(&panel, &key(SIMULATOR_A), cx);
    assert!(error.contains("Metro is still running"), "{error}");
}

#[gpui::test]
async fn trust_withdrawn_before_the_xcode_probes_runs_no_xcodebuild(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    rig.host.add_file("/app/MyApp.xcodeproj/contents", "");
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);
    assert_eq!(detected(&panel, cx).len(), 1, "the Xcode project is found");
    let held = rig
        .runner
        .hold(&["simctl", "list", "devices", "available", "-j"]);

    click_run(&panel, &key(SIMULATOR_A), cx);
    let project = panel.read_with(cx, |panel, _| panel.project.upgrade().expect("project"));
    let worktree_store = project.read_with(cx, |project, _| project.worktree_store());
    let restricted = worktree_store.read_with(cx, |worktree_store, cx| {
        worktree_store
            .worktrees()
            .map(|worktree| PathTrust::Worktree(worktree.read(cx).id()))
            .collect()
    });
    let store = cx
        .update(|_, cx| TrustedWorktrees::try_get_global(cx))
        .expect("trust store");
    store.update(cx, |store, cx| {
        store.restrict(worktree_store.downgrade(), restricted, cx)
    });
    drop(held);
    cx.run_until_parked();

    let xcodebuild_runs = rig
        .host
        .invocations()
        .iter()
        .filter(|invocation| invocation.program == std::path::Path::new("/usr/bin/xcodebuild"))
        .count();
    assert_eq!(xcodebuild_runs, 0);
    assert_eq!(terminals.spawn_count(), 0);
    assert!(error_of(&panel, &key(SIMULATOR_A), cx).contains("Trust"));
}

#[gpui::test]
async fn a_dropped_older_read_still_brings_its_app_list(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = both_booted();
    let (panel, _terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);
    assert!(detected(&panel, cx).is_empty());

    add_flutter_app(&rig.host, "/app", true);
    let held = rig
        .runner
        .hold(&["simctl", "list", "devices", "available", "-j"]);
    panel.update(cx, |panel, cx| panel.reload_fresh(cx));
    cx.run_until_parked();
    let generation = panel.update(cx, |panel, _| panel.begin_read());
    let fresher = smol::block_on(devices::discover(&*rig.host, &*rig.host));
    panel.update(cx, |panel, _| panel.apply_read(generation, fresher));
    drop(held);
    cx.run_until_parked();

    assert_eq!(
        detected(&panel, cx).len(),
        1,
        "only a Reload looks for apps, so its list is the newest"
    );
}
