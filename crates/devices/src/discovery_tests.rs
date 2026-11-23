use std::time::Duration;

use crate::fake_host::FakeHost;
use crate::host::{HostOs, HostSystem, ToolFailure};
use crate::ios_simulators::{XCODE_SELECT, XCRUN};
use crate::test_fixtures::tool_output;
use crate::{DeviceKey, DeviceState, SectionState, discover};

const ADB: &str = "/sdk/platform-tools/adb";
const EMULATOR: &str = "/sdk/emulator/emulator";

fn install_android(host: &FakeHost, with_emulator: bool) {
    host.set_environment_variable("ANDROID_HOME", "/sdk");
    host.add_file(ADB, "");
    if with_emulator {
        host.add_file(EMULATOR, "");
    }
}

fn script_xcode(host: &FakeHost) {
    host.script_output(
        XCRUN,
        ["simctl", "list", "devices", "available", "-j"],
        0,
        &tool_output("simctl-list-devices-one-booted.json"),
        "",
    );
    script_xcode_without_simulator_list(host);
}

fn script_xcode_without_simulator_list(host: &FakeHost) {
    host.script_output(
        XCODE_SELECT,
        ["-p"],
        0,
        "/Applications/Xcode.app/Contents/Developer\n",
        "",
    );
    host.script_output(
        XCRUN,
        ["simctl", "list", "runtimes", "-j"],
        0,
        &tool_output("simctl-list-runtimes.json"),
        "",
    );
    let output_path = host
        .create_temp_dir("devicectl")
        .expect("the fake host makes folders")
        .join("devicectl-devices.json");
    host.add_file(
        &output_path,
        tool_output("devicectl-list-devices-paired-unreachable.json"),
    );
    let path_argument = output_path.to_string_lossy().into_owned();
    host.script_output(
        XCRUN,
        [
            "devicectl",
            "list",
            "devices",
            "--json-output",
            path_argument.as_str(),
            "-q",
        ],
        0,
        "",
        "",
    );
}

fn script_android(host: &FakeHost) {
    host.script_output(
        ADB,
        ["devices", "-l"],
        0,
        "List of devices attached\n\
         emulator-5554 device product:sdk model:sdk transport_id:1\n\
         R58M123ABC device usb:1-1 model:Galaxy_S24 transport_id:2\n",
        "",
    );
    host.script_output(
        ADB,
        ["-s", "emulator-5554", "emu", "avd", "name"],
        0,
        &tool_output("adb-emu-avd-name.txt"),
        "",
    );
    host.script_output(
        EMULATOR,
        ["-list-avds"],
        0,
        &tool_output("emulator-list-avds.txt"),
        "",
    );
}

/// Holds for every run: no relative program, nothing that restarts adb's
/// server.
fn assert_invocations_are_safe(host: &FakeHost) {
    for invocation in host.invocations() {
        assert!(invocation.program.is_absolute(), "{invocation:?}");
        assert!(
            !invocation
                .args
                .iter()
                .any(|arg| arg == "kill-server" || arg == "start-server"),
            "{invocation:?}"
        );
    }
}

#[test]
fn a_mac_with_every_tool_fills_all_four_sections() {
    let host = FakeHost::new(HostOs::MacOs);
    install_android(&host, true);
    script_xcode(&host);
    script_android(&host);

    let snapshot = smol::block_on(discover(&host, &host));

    assert_eq!(snapshot.ios_simulators.devices().len(), 11);
    let emulators = snapshot.android_emulators.devices();
    assert_eq!(emulators.len(), 1, "{emulators:?}");
    assert_eq!(emulators[0].key, DeviceKey::Avd("Medium_Phone".to_string()));
    assert_eq!(emulators[0].state, DeviceState::Running);
    assert_eq!(snapshot.physical_ios.devices().len(), 1);
    let android = snapshot.physical_android.devices();
    assert_eq!(android.len(), 1);
    assert_eq!(android[0].name, "Galaxy S24");
    assert_eq!(
        snapshot.memory.running_count, 2,
        "one simulator, one emulator"
    );
    assert!(!snapshot.memory.low);
    assert_invocations_are_safe(&host);
}

#[test]
fn linux_and_windows_never_run_an_ios_tool() {
    for os in [HostOs::Linux, HostOs::Windows] {
        let host = FakeHost::new(os);
        let snapshot = smol::block_on(discover(&host, &host));
        assert!(
            matches!(snapshot.ios_simulators, SectionState::Unsupported { .. }),
            "{:?}",
            snapshot.ios_simulators
        );
        assert!(matches!(
            snapshot.physical_ios,
            SectionState::Unsupported { .. }
        ));
        assert!(matches!(
            snapshot.android_emulators,
            SectionState::ToolMissing { .. }
        ));
        assert!(matches!(
            snapshot.physical_android,
            SectionState::ToolMissing { .. }
        ));
        assert!(host.invocations().is_empty(), "{:?}", host.invocations());
    }
}

#[test]
fn a_timed_out_simctl_leaves_the_android_sections_intact() {
    let host = FakeHost::new(HostOs::MacOs);
    install_android(&host, true);
    script_android(&host);
    script_xcode_without_simulator_list(&host);
    host.script(
        XCRUN,
        ["simctl", "list", "devices", "available", "-j"],
        Err(ToolFailure::TimedOut {
            program: XCRUN.into(),
            timeout: Duration::from_secs(10),
        }),
    );

    let snapshot = smol::block_on(discover(&host, &host));

    assert!(
        matches!(snapshot.ios_simulators, SectionState::Failed { .. }),
        "{:?}",
        snapshot.ios_simulators
    );
    assert_eq!(snapshot.android_emulators.devices().len(), 1);
    assert_eq!(snapshot.physical_android.devices().len(), 1);
    assert_eq!(snapshot.physical_ios.devices().len(), 1);
}

#[test]
fn an_sdk_without_the_emulator_still_lists_physical_android_devices() {
    let host = FakeHost::new(HostOs::Linux);
    install_android(&host, false);
    script_android(&host);

    let snapshot = smol::block_on(discover(&host, &host));
    assert!(
        matches!(&snapshot.android_emulators, SectionState::ToolMissing { tool, .. } if tool.contains("Emulator")),
        "{:?}",
        snapshot.android_emulators
    );
    assert_eq!(snapshot.physical_android.devices().len(), 1);
}

#[test]
fn an_adb_mismatch_fails_both_android_sections_with_the_same_message() {
    let host = FakeHost::new(HostOs::Linux);
    install_android(&host, true);
    host.script_output(
        ADB,
        ["devices", "-l"],
        0,
        "List of devices attached\n",
        "adb server version (41) doesn't match this client (39); killing...\n",
    );
    host.script_output(
        EMULATOR,
        ["-list-avds"],
        0,
        &tool_output("emulator-list-avds.txt"),
        "",
    );

    let snapshot = smol::block_on(discover(&host, &host));
    let SectionState::Failed { message } = &snapshot.physical_android else {
        panic!("{:?}", snapshot.physical_android);
    };
    assert!(message.contains("one Android SDK"), "{message}");
    assert_eq!(snapshot.android_emulators, snapshot.physical_android);
    assert_invocations_are_safe(&host);
}

#[test]
fn a_mac_without_xcode_reports_it_for_both_ios_sections() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script(
        XCODE_SELECT,
        ["-p"],
        Err(ToolFailure::NotFound(XCODE_SELECT.into())),
    );

    let snapshot = smol::block_on(discover(&host, &host));
    assert!(
        matches!(&snapshot.ios_simulators, SectionState::ToolMissing { tool, .. } if tool == "Xcode")
    );
    assert_eq!(snapshot.ios_simulators, snapshot.physical_ios);
    let ran: Vec<_> = host
        .invocations()
        .into_iter()
        .map(|invocation| invocation.program)
        .collect();
    assert_eq!(ran, [std::path::PathBuf::from(XCODE_SELECT)]);
}

#[test]
fn little_free_memory_is_flagged() {
    let host = FakeHost::new(HostOs::Linux);
    host.set_available_memory(Some(3 * 1024 * 1024 * 1024));
    let snapshot = smol::block_on(discover(&host, &host));
    assert!(snapshot.memory.low);
    assert_eq!(
        snapshot.memory.available_bytes,
        Some(3 * 1024 * 1024 * 1024)
    );
}
