use std::time::Duration;

use devices::{DeviceKey, DeviceState};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use workspace::dock::Panel as _;

use crate::device_operations::shorten_home;
use crate::devices_panel::DevicesPanel;
use crate::test_support::{
    ADB, EMULATOR, LOG_DIR, Rig, SIMULATOR_A, SIMULATOR_B, XCRUN, advance, draw, init_test,
    panel_in_window,
};

const TEN_MINUTES: Duration = Duration::from_secs(600);

fn key(udid: &str) -> DeviceKey {
    DeviceKey::Simulator(udid.to_string())
}

fn state_of(
    panel: &Entity<DevicesPanel>,
    device: &DeviceKey,
    cx: &mut VisualTestContext,
) -> Option<DeviceState> {
    panel.read_with(cx, |panel, _| {
        panel.find_device(device).map(|device| device.state.clone())
    })
}

fn operations_in_flight(panel: &Entity<DevicesPanel>, cx: &mut VisualTestContext) -> usize {
    panel.read_with(cx, |panel, _| panel.operations.len())
}

fn error_of(
    panel: &Entity<DevicesPanel>,
    device: &DeviceKey,
    cx: &mut VisualTestContext,
) -> Option<String> {
    panel.read_with(cx, |panel, _| {
        panel
            .device_errors
            .get(device)
            .map(|error| error.message.to_string())
    })
}

fn open(panel: &Entity<DevicesPanel>, cx: &mut VisualTestContext) {
    panel.update_in(cx, |panel, window, cx| panel.set_active(true, window, cx));
    cx.run_until_parked();
}

fn boot(panel: &Entity<DevicesPanel>, udid: &str, cx: &mut VisualTestContext) {
    panel.update(cx, |panel, cx| panel.boot_device(&key(udid), cx));
}

fn boot_count(rig: &Rig, udid: &str) -> usize {
    rig.command_count(XCRUN, &["simctl", "boot", udid])
}

/// Two shut-down simulators; the follower then sees `A` booted.
fn mac_where_a_boots() -> Rig {
    let rig = Rig::mac_booting();
    rig.script_simulator_list("Shutdown", "Shutdown");
    rig.script_simulator_list("Booted", "Shutdown");
    rig
}

#[gpui::test]
async fn boot_shows_the_row_as_starting_before_the_command_answers(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = mac_where_a_boots();
    let release = rig.runner.hold(&["simctl", "boot", SIMULATOR_A]);
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    open(&panel, cx);
    assert_eq!(
        state_of(&panel, &key(SIMULATOR_A), cx),
        Some(DeviceState::Shutdown)
    );

    boot(&panel, SIMULATOR_A, cx);
    assert_eq!(
        state_of(&panel, &key(SIMULATOR_A), cx),
        Some(DeviceState::Starting)
    );

    cx.run_until_parked();
    assert_eq!(boot_count(&rig, SIMULATOR_A), 1);
    assert_eq!(
        state_of(&panel, &key(SIMULATOR_A), cx),
        Some(DeviceState::Starting)
    );
    assert_eq!(operations_in_flight(&panel, cx), 1);

    release.send(()).ok();
    advance(cx, Duration::from_secs(3));
    assert_eq!(
        state_of(&panel, &key(SIMULATOR_A), cx),
        Some(DeviceState::Running)
    );
    assert_eq!(operations_in_flight(&panel, cx), 0);
}

#[gpui::test]
async fn two_boots_in_a_row_are_two_commands_that_do_not_wait_for_each_other(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let rig = mac_where_a_boots();
    let _release_first = rig.runner.hold(&["simctl", "boot", SIMULATOR_A]);
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    open(&panel, cx);

    boot(&panel, SIMULATOR_A, cx);
    boot(&panel, SIMULATOR_B, cx);
    cx.run_until_parked();

    assert_eq!(boot_count(&rig, SIMULATOR_A), 1);
    assert_eq!(boot_count(&rig, SIMULATOR_B), 1, "the second did not wait");
    assert_eq!(operations_in_flight(&panel, cx), 2);
}

#[gpui::test]
async fn booting_the_same_device_twice_runs_one_command(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = mac_where_a_boots();
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    open(&panel, cx);

    boot(&panel, SIMULATOR_A, cx);
    boot(&panel, SIMULATOR_A, cx);
    cx.run_until_parked();

    assert_eq!(boot_count(&rig, SIMULATOR_A), 1);
}

#[gpui::test]
async fn following_stops_when_the_device_is_ready_and_nothing_runs_afterwards(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let rig = mac_where_a_boots();
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    open(&panel, cx);

    boot(&panel, SIMULATOR_A, cx);
    advance(cx, Duration::from_secs(3));
    assert_eq!(
        state_of(&panel, &key(SIMULATOR_A), cx),
        Some(DeviceState::Running)
    );
    let settled = rig.host.invocations().len();

    advance(cx, TEN_MINUTES);

    assert_eq!(rig.host.invocations().len(), settled);
    assert_eq!(operations_in_flight(&panel, cx), 0);
    let low_memory_free = panel.read_with(cx, |panel, _| {
        panel
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.memory.running_count)
    });
    assert_eq!(
        low_memory_free,
        Some(1),
        "the memory line counts the new device"
    );
}

#[gpui::test]
async fn a_device_that_never_boots_times_out_with_the_error_on_that_device_only(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let rig = Rig::mac_booting();
    rig.script_simulator_list("Shutdown", "Shutdown");
    rig.script_simulator_list("Booting", "Shutdown");
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    open(&panel, cx);

    boot(&panel, SIMULATOR_A, cx);
    advance(cx, Duration::from_secs(130));

    let message = error_of(&panel, &key(SIMULATOR_A), cx);
    assert!(
        message.is_some_and(|message| message.contains("did not finish booting")),
        "the timeout says what did not happen"
    );
    assert_eq!(error_of(&panel, &key(SIMULATOR_B), cx), None);
    assert_eq!(
        state_of(&panel, &key(SIMULATOR_B), cx),
        Some(DeviceState::Shutdown)
    );
    assert_eq!(operations_in_flight(&panel, cx), 0);

    let settled = rig.host.invocations().len();
    advance(cx, TEN_MINUTES);
    assert_eq!(
        rig.host.invocations().len(),
        settled,
        "no command after the deadline"
    );
}

#[gpui::test]
async fn hiding_the_panel_does_not_stop_following_a_device(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = mac_where_a_boots();
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    open(&panel, cx);

    boot(&panel, SIMULATOR_A, cx);
    panel.update_in(cx, |panel, window, cx| panel.set_active(false, window, cx));
    advance(cx, Duration::from_secs(3));

    assert_eq!(
        state_of(&panel, &key(SIMULATOR_A), cx),
        Some(DeviceState::Running)
    );
    assert_eq!(operations_in_flight(&panel, cx), 0);
}

#[gpui::test]
async fn a_failed_boot_puts_the_error_on_that_device_and_frees_it(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::mac();
    rig.script_simulator_list("Shutdown", "Shutdown");
    rig.host.script_output(
        XCRUN,
        ["simctl", "boot", SIMULATOR_A],
        1,
        "",
        "Unable to boot: no disk space",
    );
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    open(&panel, cx);

    boot(&panel, SIMULATOR_A, cx);
    cx.run_until_parked();

    let message = error_of(&panel, &key(SIMULATOR_A), cx);
    assert!(message.is_some_and(|message| message.contains("no disk space")));
    assert_eq!(error_of(&panel, &key(SIMULATOR_B), cx), None);
    assert_eq!(
        state_of(&panel, &key(SIMULATOR_A), cx),
        Some(DeviceState::Shutdown)
    );
    assert_eq!(
        operations_in_flight(&panel, cx),
        0,
        "the failed device is not stuck busy"
    );
}

#[gpui::test]
async fn a_reload_in_the_middle_does_not_wipe_the_starting_state(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::mac_booting();
    rig.script_simulator_list("Shutdown", "Shutdown");
    // The list read by the reload still says Shutdown: the command was only
    // just sent.
    rig.script_simulator_list("Shutdown", "Shutdown");
    rig.script_simulator_list("Booted", "Shutdown");
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    open(&panel, cx);

    boot(&panel, SIMULATOR_A, cx);
    cx.run_until_parked();
    panel.update(cx, |panel, cx| panel.reload(cx));
    cx.run_until_parked();

    assert_eq!(
        state_of(&panel, &key(SIMULATOR_A), cx),
        Some(DeviceState::Starting)
    );

    advance(cx, Duration::from_secs(3));
    assert_eq!(
        state_of(&panel, &key(SIMULATOR_A), cx),
        Some(DeviceState::Running)
    );
}

#[gpui::test]
async fn shutdown_follows_the_simulator_down_and_show_window_opens_it(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::mac();
    rig.script_simulator_list("Booted", "Shutdown");
    rig.script_simulator_list("Shutdown", "Shutdown");
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    open(&panel, cx);

    panel.update(cx, |panel, cx| {
        panel.show_simulator_window(&key(SIMULATOR_A), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        rig.command_count(
            "/usr/bin/open",
            &[
                "-a",
                "Simulator",
                "--args",
                "-CurrentDeviceUDID",
                SIMULATOR_A
            ]
        ),
        1
    );
    assert_eq!(operations_in_flight(&panel, cx), 0);

    panel.update(cx, |panel, cx| panel.shutdown_device(&key(SIMULATOR_A), cx));
    assert_eq!(
        state_of(&panel, &key(SIMULATOR_A), cx),
        Some(DeviceState::Stopping)
    );
    advance(cx, Duration::from_secs(3));

    assert_eq!(
        rig.command_count(XCRUN, &["simctl", "shutdown", SIMULATOR_A]),
        1
    );
    assert_eq!(
        state_of(&panel, &key(SIMULATOR_A), cx),
        Some(DeviceState::Shutdown)
    );
    assert_eq!(operations_in_flight(&panel, cx), 0);
}

#[gpui::test]
async fn buttons_follow_the_device_kind_and_state(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::mac();
    rig.script_simulator_list("Shutdown", "Booted");
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    open(&panel, cx);
    draw(cx);

    assert!(
        cx.debug_bounds("sim-11111111-1111-1111-1111-111111111111-boot")
            .is_some()
    );
    assert!(
        cx.debug_bounds("sim-11111111-1111-1111-1111-111111111111-shutdown")
            .is_none()
    );
    assert!(
        cx.debug_bounds("sim-22222222-2222-2222-2222-222222222222-boot")
            .is_none()
    );
    assert!(
        cx.debug_bounds("sim-22222222-2222-2222-2222-222222222222-shutdown")
            .is_some()
    );
    assert!(
        cx.debug_bounds("sim-22222222-2222-2222-2222-222222222222-window")
            .is_some()
    );
}

#[gpui::test]
async fn clicking_the_boot_button_boots_that_simulator(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = mac_where_a_boots();
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    open(&panel, cx);
    draw(cx);

    let bounds = cx
        .debug_bounds("sim-11111111-1111-1111-1111-111111111111-boot")
        .expect("the Boot button is drawn");
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();

    assert_eq!(boot_count(&rig, SIMULATOR_A), 1);
    assert_eq!(boot_count(&rig, SIMULATOR_B), 0);
}

#[gpui::test]
async fn a_physical_device_has_no_boot_or_shutdown(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.script_adb_devices("R58M123ABC device usb:1-1 model:Galaxy_S24 transport_id:2\n");
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    open(&panel, cx);
    draw(cx);

    assert!(
        cx.debug_bounds("android-R58M123ABC").is_some(),
        "the phone is listed"
    );
    assert!(cx.debug_bounds("android-R58M123ABC-boot").is_none());
    assert!(cx.debug_bounds("android-R58M123ABC-shutdown").is_none());
    panel.update(cx, |panel, cx| {
        panel.boot_device(&DeviceKey::AndroidSerial("R58M123ABC".to_string()), cx);
        panel.shutdown_device(&DeviceKey::AndroidSerial("R58M123ABC".to_string()), cx);
    });
    cx.run_until_parked();
    assert_eq!(operations_in_flight(&panel, cx), 0);
}

fn avd(name: &str) -> DeviceKey {
    DeviceKey::Avd(name.to_string())
}

#[gpui::test]
async fn starting_an_emulator_logs_into_the_zode_folder_and_follows_it_up(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.script_adb_devices("");
    rig.script_adb_devices("emulator-5554 device product:sdk model:sdk transport_id:1\n");
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    open(&panel, cx);

    panel.update(cx, |panel, cx| panel.boot_device(&avd("Pixel_8"), cx));
    cx.run_until_parked();
    let detached = rig.host.detached();
    assert_eq!(detached.len(), 1);
    assert_eq!(detached[0].0.args, ["-avd", "Pixel_8"]);
    assert!(detached[0].1.starts_with(LOG_DIR));
    assert_eq!(
        panel.read_with(cx, |panel, _| panel
            .find_device(&avd("Pixel_8"))
            .map(|d| d.state.clone())),
        Some(DeviceState::Starting)
    );

    advance(cx, Duration::from_secs(3));
    assert_eq!(
        panel.read_with(cx, |panel, _| panel
            .find_device(&avd("Pixel_8"))
            .map(|d| d.state.clone())),
        Some(DeviceState::Running)
    );
    let settled = rig.host.invocations().len();
    advance(cx, TEN_MINUTES);
    assert_eq!(rig.host.invocations().len(), settled);

    panel.update(cx, |panel, cx| panel.shutdown_device(&avd("Pixel_8"), cx));
    cx.run_until_parked();
    assert_eq!(
        rig.command_count(ADB, &["-s", "emulator-5554", "emu", "kill"]),
        1
    );
    assert_eq!(
        panel.read_with(cx, |panel, _| panel
            .find_device(&avd("Pixel_8"))
            .map(|d| d.state.clone())),
        Some(DeviceState::Stopping)
    );
}

#[gpui::test]
async fn an_emulator_that_never_comes_up_explains_itself_and_offers_the_log(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let rig = Rig::linux();
    rig.script_adb_devices("");
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    open(&panel, cx);

    panel.update(cx, |panel, cx| panel.boot_device(&avd("Pixel_8"), cx));
    cx.run_until_parked();
    let log_file = rig.host.detached()[0].1.clone();
    rig.host.add_file(
        &log_file,
        "emulator: ERROR: Running multiple emulators with the same AVD is an experimental feature.",
    );
    advance(cx, Duration::from_secs(190));

    let (message, offered_log) = panel.read_with(cx, |panel, _| {
        let error = panel.device_errors.get(&avd("Pixel_8")).cloned();
        (
            error.as_ref().map(|error| error.message.to_string()),
            error.and_then(|error| error.log_file),
        )
    });
    assert!(message.is_some_and(|message| message.contains("already running")));
    assert_eq!(offered_log, Some(log_file));
    assert_eq!(operations_in_flight(&panel, cx), 0);
    assert_eq!(rig.command_count(EMULATOR, &["-avd", "Pixel_9"]), 0);
}

#[test]
fn the_home_directory_is_shortened_in_messages() {
    let home = std::path::Path::new("/Users/someone");
    assert_eq!(
        shorten_home("log at /Users/someone/Library/Logs/Zode/a.log", Some(home)),
        "log at ~/Library/Logs/Zode/a.log"
    );
    assert_eq!(shorten_home("no path here", Some(home)), "no path here");
    assert_eq!(shorten_home("/Users/someone/x", None), "/Users/someone/x");
}

#[gpui::test]
async fn an_emulator_that_exits_at_once_fails_at_once_with_its_log(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.script_adb_devices("");
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    open(&panel, cx);

    panel.update(cx, |panel, cx| panel.boot_device(&avd("Pixel_8"), cx));
    cx.run_until_parked();
    let log_file = rig.host.detached()[0].1.clone();
    rig.host.exit_detached(
        0,
        Some(1),
        "emulator: ERROR: x86_64 emulation currently requires hardware acceleration!\n",
    );
    advance(cx, Duration::from_secs(3));

    let (message, offered_log) = panel.read_with(cx, |panel, _| {
        let error = panel.device_errors.get(&avd("Pixel_8")).cloned();
        (
            error.as_ref().map(|error| error.message.to_string()),
            error.and_then(|error| error.log_file),
        )
    });
    let message = message.unwrap_or_default();
    assert!(message.contains("hardware acceleration"), "{message}");
    assert!(!message.contains("within"), "{message}");
    assert_eq!(offered_log, Some(log_file));
    assert_eq!(operations_in_flight(&panel, cx), 0);
    assert_eq!(
        state_of(&panel, &avd("Pixel_8"), cx),
        Some(DeviceState::Shutdown)
    );
}

#[gpui::test]
async fn an_emulator_without_a_known_avd_can_still_be_shut_down(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    let serial = DeviceKey::AndroidSerial("emulator-5556".to_string());
    rig.script_adb_devices("emulator-5556 device product:sdk model:sdk transport_id:3\n");
    rig.script_adb_devices("");
    rig.host.script_output(
        ADB,
        ["-s", "emulator-5556", "emu", "kill"],
        0,
        "OK: killing emulator, bye bye\r\n",
        "",
    );
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    open(&panel, cx);
    draw(cx);
    assert_eq!(state_of(&panel, &serial, cx), Some(DeviceState::Running));
    assert!(cx.debug_bounds("android-emulator-5556-shutdown").is_some());

    panel.update(cx, |panel, cx| panel.shutdown_device(&serial, cx));
    cx.run_until_parked();
    assert_eq!(
        rig.command_count(ADB, &["-s", "emulator-5556", "emu", "kill"]),
        1
    );
    advance(cx, Duration::from_secs(3));
    assert_eq!(operations_in_flight(&panel, cx), 0);
    assert_eq!(
        state_of(&panel, &serial, cx),
        None,
        "its row lived only while it ran"
    );
    draw(cx);
    assert!(cx.debug_bounds("android-emulator-5556").is_none());
}

#[test]
fn every_offered_action_can_be_planned() {
    use crate::device_actions::offers;
    use crate::device_operations::{OperationKind, plan_for};
    use devices::{Device, DeviceKind};

    let device = |key: DeviceKey, kind: DeviceKind, state: DeviceState| Device {
        key,
        name: "device".to_string(),
        kind,
        state,
        detail: None,
        family: None,
        run_id: Some("emulator-5556".to_string()),
        serial: Some("emulator-5556".to_string()),
        experimental: false,
    };
    let keys = [
        (key(SIMULATOR_A), DeviceKind::IosSimulator),
        (avd("Pixel_8"), DeviceKind::AndroidEmulator),
        (
            DeviceKey::AndroidSerial("emulator-5556".to_string()),
            DeviceKind::AndroidEmulator,
        ),
        (
            DeviceKey::AndroidSerial("R58M123ABC".to_string()),
            DeviceKind::AndroidUsb,
        ),
        (
            DeviceKey::IosDevice("00008110-001A2C3E0A88801E".to_string()),
            DeviceKind::IosPhysical,
        ),
    ];
    for (key, kind) in keys {
        for state in [
            DeviceState::Shutdown,
            DeviceState::Starting,
            DeviceState::Running,
            DeviceState::Stopping,
        ] {
            let device = device(key.clone(), kind, state);
            for operation in [
                OperationKind::Boot,
                OperationKind::Shutdown,
                OperationKind::ShowWindow,
            ] {
                if offers(&operation, &device) {
                    assert!(
                        plan_for(&operation, &device, Vec::new()).is_some(),
                        "{operation:?} is offered on {device:?} but cannot be planned"
                    );
                }
            }
        }
    }
}
