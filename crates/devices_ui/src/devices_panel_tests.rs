use std::time::Duration;

use devices::{DeviceKey, DeviceState, MemoryStatus, SectionState};
use gpui::{TestAppContext, UpdateGlobal as _};
use settings::SettingsStore;
use workspace::dock::Panel as _;

use crate::device_snapshot::{SectionId, memory_line, merge_snapshot};
use crate::devices_panel::DevicesPanel;
use crate::test_support::{ADB, Rig, advance, draw, init_test, panel_in_window};

const TEN_MINUTES: Duration = Duration::from_secs(600);
const GIB: u64 = 1024 * 1024 * 1024;

fn activate(panel: &gpui::Entity<DevicesPanel>, active: bool, cx: &mut gpui::VisualTestContext) {
    panel.update_in(cx, |panel, window, cx| panel.set_active(active, window, cx));
    cx.run_until_parked();
}

#[gpui::test]
async fn a_panel_nobody_opened_runs_no_command(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.script_adb_devices("");
    let (_panel, cx) = panel_in_window(cx, &rig, false).await;

    advance(cx, TEN_MINUTES);

    assert_eq!(rig.host.invocations().len(), 0);
}

#[gpui::test]
async fn the_first_activation_reads_once_and_later_ones_read_nothing(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.script_adb_devices("");
    let (panel, cx) = panel_in_window(cx, &rig, false).await;

    activate(&panel, true, cx);
    assert_eq!(rig.command_count(ADB, &["devices", "-l"]), 1);
    let after_first_read = rig.host.invocations().len();

    activate(&panel, false, cx);
    activate(&panel, true, cx);
    activate(&panel, false, cx);
    activate(&panel, true, cx);

    assert_eq!(rig.host.invocations().len(), after_first_read);
}

#[gpui::test]
async fn nothing_runs_in_the_background_after_the_read(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.script_adb_devices("");
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    activate(&panel, true, cx);
    let after_read = rig.host.invocations().len();

    advance(cx, TEN_MINUTES);

    assert_eq!(rig.host.invocations().len(), after_read);
    assert!(panel.read_with(cx, |panel, _| panel.operations.is_empty()));
}

#[gpui::test]
async fn reload_reads_again_but_never_on_top_of_a_read_in_flight(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.script_adb_devices("");
    let release = rig.runner.hold(&["devices", "-l"]);
    let (panel, cx) = panel_in_window(cx, &rig, false).await;

    activate(&panel, true, cx);
    panel.update(cx, |panel, cx| panel.reload(cx));
    panel.update(cx, |panel, cx| panel.reload(cx));
    cx.run_until_parked();
    assert!(panel.read_with(cx, |panel, _| panel.is_reading()));
    assert_eq!(rig.command_count(ADB, &["devices", "-l"]), 1);

    release.send(()).ok();
    cx.run_until_parked();
    assert!(!panel.read_with(cx, |panel, _| panel.is_reading()));
    panel.update(cx, |panel, cx| panel.reload(cx));
    cx.run_until_parked();
    assert_eq!(rig.command_count(ADB, &["devices", "-l"]), 2);
}

#[gpui::test]
async fn a_failed_read_keeps_the_old_list_and_says_so(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.script_adb_devices("");
    rig.host
        .script_output(ADB, ["devices", "-l"], 1, "", "adb is stuck");
    let (panel, cx) = panel_in_window(cx, &rig, false).await;

    activate(&panel, true, cx);
    let names = |panel: &DevicesPanel| -> Vec<String> {
        panel
            .all_devices()
            .map(|device| device.name.clone())
            .collect()
    };
    assert_eq!(
        panel.read_with(cx, |panel, _| names(panel)),
        ["Pixel 8", "Pixel 9"]
    );

    panel.update(cx, |panel, cx| panel.reload(cx));
    cx.run_until_parked();

    assert_eq!(
        panel.read_with(cx, |panel, _| names(panel)),
        ["Pixel 8", "Pixel 9"]
    );
    let message = panel.read_with(cx, |panel, _| {
        panel
            .section_errors
            .get(&SectionId::AndroidEmulators)
            .cloned()
    });
    assert!(
        message.is_some_and(|message| message.contains("adb is stuck")),
        "the failed read is reported next to the kept list"
    );
}

#[gpui::test]
async fn the_memory_line_counts_running_devices_and_warns_when_low(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.script_adb_devices("emulator-5554 device product:sdk model:sdk transport_id:1\n");
    rig.host.set_available_memory(Some(GIB));
    let (panel, cx) = panel_in_window(cx, &rig, false).await;

    activate(&panel, true, cx);
    draw(cx);

    let memory = panel.read_with(cx, |panel, _| {
        panel
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.memory.clone())
    });
    let memory = memory.expect("the panel read the devices");
    assert_eq!(memory.running_count, 1);
    assert_eq!(
        memory_line(&memory),
        ("1 running · 1.0 GB free".to_string(), true)
    );
    assert!(cx.debug_bounds("devices-memory").is_some());
}

#[test]
fn the_memory_line_is_calm_with_room_and_still_shown_without_a_figure() {
    let roomy = MemoryStatus {
        running_count: 2,
        available_bytes: Some(16 * GIB),
        low: false,
    };
    assert_eq!(
        memory_line(&roomy),
        ("2 running · 16.0 GB free".to_string(), false)
    );
    let unknown = MemoryStatus {
        running_count: 0,
        available_bytes: None,
        low: false,
    };
    assert_eq!(memory_line(&unknown), ("0 running".to_string(), false));
}

#[gpui::test]
async fn a_shared_project_runs_nothing_and_says_why(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.script_adb_devices("");
    let (panel, cx) = panel_in_window(cx, &rig, true).await;

    activate(&panel, true, cx);
    panel.update(cx, |panel, cx| panel.reload(cx));
    draw(cx);

    assert_eq!(rig.host.invocations().len(), 0);
    assert!(cx.debug_bounds("devices-remote-message").is_some());
    assert!(cx.debug_bounds("devices-reload").is_none());
}

#[gpui::test]
async fn a_local_project_shows_a_reload_button_and_no_remote_message(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.script_adb_devices("");
    let (panel, cx) = panel_in_window(cx, &rig, false).await;

    activate(&panel, true, cx);
    draw(cx);

    assert!(cx.debug_bounds("devices-reload").is_some());
    assert!(cx.debug_bounds("devices-remote-message").is_none());
}

#[gpui::test]
async fn the_button_setting_decides_whether_the_dock_shows_the_icon(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    let icon =
        |cx: &mut gpui::VisualTestContext| cx.update(|window, cx| panel.read(cx).icon(window, cx));

    assert_eq!(icon(cx), Some(ui::IconName::Smartphone));

    cx.update(|_, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.devices_panel.get_or_insert_default().button = Some(false);
            });
        });
    });

    assert_eq!(icon(cx), None);
}

#[gpui::test]
async fn the_panel_sits_in_the_right_dock_after_agent_history(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    let (panel, cx) = panel_in_window(cx, &rig, false).await;

    cx.update(|window, cx| {
        let panel = panel.read(cx);
        assert_eq!(
            panel.position(window, cx),
            workspace::dock::DockPosition::Right
        );
        assert!(!panel.position_is_valid(workspace::dock::DockPosition::Left));
        assert!(!panel.starts_open(window, cx));
        assert_eq!(panel.activation_priority(), 6);
        assert_eq!(panel.default_size(window, cx), gpui::px(320.));
    });
}

fn snapshot_with(
    emulators: SectionState,
    physical_android: SectionState,
) -> devices::DeviceSnapshot {
    devices::DeviceSnapshot {
        ios_simulators: SectionState::Unsupported {
            reason: String::new(),
        },
        android_emulators: emulators,
        physical_ios: SectionState::Unsupported {
            reason: String::new(),
        },
        physical_android,
        memory: devices::memory_status(None, 0),
    }
}

#[test]
fn merging_keeps_a_ready_list_over_a_failure_and_takes_everything_else() {
    let device = devices::Device {
        key: DeviceKey::Avd("Pixel_8".to_string()),
        name: "Pixel 8".to_string(),
        kind: devices::DeviceKind::AndroidEmulator,
        state: DeviceState::Shutdown,
        detail: None,
        family: None,
        serial: None,
        run_id: None,
        experimental: false,
    };
    let old = snapshot_with(
        SectionState::Ready(vec![device]),
        SectionState::Ready(Vec::new()),
    );
    let failed = || SectionState::Failed {
        message: "boom".to_string(),
    };

    let (merged, errors) = merge_snapshot(Some(&old), snapshot_with(failed(), failed()));
    assert_eq!(merged.android_emulators, old.android_emulators);
    assert_eq!(merged.physical_android, old.physical_android);
    assert_eq!(errors.len(), 2);

    let (merged, errors) = merge_snapshot(None, snapshot_with(failed(), failed()));
    assert_eq!(merged.android_emulators, failed());
    assert!(
        errors.is_empty(),
        "nothing old to keep, so the failure is the state"
    );
}

const UNAUTHORIZED_PHONE: &str = "R58M unauthorized usb:1-1 transport_id:3\n";
const OFFLINE_PHONE: &str = "A2 offline usb:1-2 transport_id:4\n";
const NO_PERMISSIONS_PHONE: &str = "A4 no permissions (missing udev rules? user is in the plugdev group); see [http://developer.android.com/tools/device.html] usb:1-3\n";
const READY_PHONE: &str = "A5 device usb:1-4 model:Pixel_8 transport_id:5\n";

fn phone(
    panel: &gpui::Entity<DevicesPanel>,
    serial: &str,
    cx: &mut gpui::VisualTestContext,
) -> devices::Device {
    panel
        .read_with(cx, |panel, _| {
            panel
                .find_device(&DeviceKey::AndroidSerial(serial.to_string()))
                .cloned()
        })
        .expect("the phone is listed")
}

#[gpui::test]
async fn a_phone_that_cannot_be_used_says_what_to_do_and_a_ready_one_says_nothing(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let rig = Rig::linux();
    rig.script_adb_devices(&format!(
        "{UNAUTHORIZED_PHONE}{OFFLINE_PHONE}{NO_PERMISSIONS_PHONE}{READY_PHONE}"
    ));
    let (panel, cx) = panel_in_window(cx, &rig, false).await;

    activate(&panel, true, cx);
    draw(cx);

    for (serial, selector) in [
        ("R58M", "android-R58M-guidance"),
        ("A2", "android-A2-guidance"),
        ("A4", "android-A4-guidance"),
    ] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "{serial} needs guidance"
        );
        let device = phone(&panel, serial, cx);
        assert!(crate::physical_device_guidance::guidance_for(&device).is_some());
    }
    assert!(cx.debug_bounds("android-A5-guidance").is_none());
    let guidance = |serial: &str, cx: &mut gpui::VisualTestContext| {
        crate::physical_device_guidance::guidance_for(&phone(&panel, serial, cx))
            .unwrap_or_default()
    };
    assert!(guidance("R58M", cx).contains("Allow USB debugging"));
    assert!(guidance("A2", cx).contains("Reload"));
    assert!(guidance("A4", cx).contains("udev"));
}

#[gpui::test]
async fn the_wireless_button_is_offered_only_where_adb_can_be_used(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.script_adb_devices("");
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    activate(&panel, true, cx);
    draw(cx);
    assert!(cx.debug_bounds("devices-connect-wireless").is_some());

    let rig = Rig::mac();
    rig.script_simulator_list("Shutdown", "Shutdown");
    let (panel, cx) = panel_in_window(cx, &rig, false).await;
    activate(&panel, true, cx);
    draw(cx);
    assert!(
        cx.debug_bounds("devices-connect-wireless").is_none(),
        "no Android SDK, nothing for the button to run"
    );
}

#[gpui::test]
async fn two_adb_versions_are_reported_once_with_the_way_out(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.host.script_output(
        ADB,
        ["devices", "-l"],
        0,
        "adb server version (41) doesn't match this client (40); killing...\n",
        "",
    );
    let (panel, cx) = panel_in_window(cx, &rig, false).await;

    activate(&panel, true, cx);
    draw(cx);

    let message = panel.read_with(cx, |panel, _| match panel.snapshot.as_ref() {
        Some(snapshot) => match &snapshot.android_emulators {
            SectionState::Failed { message } => message.clone(),
            other => format!("{other:?}"),
        },
        None => String::new(),
    });
    assert!(message.contains("Use one Android SDK"), "{message}");
    assert!(cx.debug_bounds("devices-section-failed").is_some());
}
