use crate::fake_host::FakeHost;
use crate::host::{HostOs, ToolFailure};
use crate::ios_simulators::{XCODE_SELECT, XCRUN, check_xcode, parse_simulators, read_simulators};
use crate::test_fixtures::tool_output;
use crate::{DeviceKey, DeviceKind, DeviceState, SectionState};

const IPHONE_17: &str = "00000000-0000-4000-8000-000000000005";
const LIST_DEVICES: [&str; 5] = ["simctl", "list", "devices", "available", "-j"];
const LIST_RUNTIMES: [&str; 4] = ["simctl", "list", "runtimes", "-j"];

fn state_of_iphone_17(devices_fixture: &str) -> DeviceState {
    let devices = parse_simulators(
        &tool_output(devices_fixture),
        &tool_output("simctl-list-runtimes.json"),
    )
    .expect("the fixture parses");
    devices
        .into_iter()
        .find(|device| device.key == DeviceKey::Simulator(IPHONE_17.to_string()))
        .expect("iPhone 17 is listed")
        .state
}

#[test]
fn every_recorded_simulator_state_maps_to_its_device_state() {
    assert_eq!(
        state_of_iphone_17("simctl-list-devices-all-shutdown.json"),
        DeviceState::Shutdown
    );
    assert_eq!(
        state_of_iphone_17("simctl-list-devices-one-booting.json"),
        DeviceState::Starting
    );
    assert_eq!(
        state_of_iphone_17("simctl-list-devices-one-booted.json"),
        DeviceState::Running
    );
    assert_eq!(
        state_of_iphone_17("simctl-list-devices-one-shutting-down.json"),
        DeviceState::Stopping
    );
}

#[test]
fn simulators_carry_the_runtime_name_family_and_udid_as_run_id() {
    let devices = parse_simulators(
        &tool_output("simctl-list-devices-one-booted.json"),
        &tool_output("simctl-list-runtimes.json"),
    )
    .expect("the fixture parses");
    assert_eq!(devices.len(), 11);
    let iphone = devices
        .iter()
        .find(|device| device.name == "iPhone 17")
        .expect("iPhone 17 is listed");
    assert_eq!(iphone.kind, DeviceKind::IosSimulator);
    assert_eq!(iphone.detail.as_deref(), Some("iOS 26.4"));
    assert_eq!(iphone.family.as_deref(), Some("iPhone"));
    assert_eq!(iphone.run_id.as_deref(), Some(IPHONE_17));
    assert!(!iphone.experimental);
    let ipad = devices
        .iter()
        .find(|device| device.name == "iPad (A16)")
        .expect("iPad (A16) is listed");
    assert_eq!(ipad.family.as_deref(), Some("iPad"));
}

#[test]
fn a_runtime_missing_from_the_runtime_list_still_gets_a_readable_name() {
    let devices_json = r#"{"devices":{"com.apple.CoreSimulator.SimRuntime.iOS-18-2":[
        {"udid":"00000000-0000-4000-8000-0000000000AA","name":"iPhone 16","state":"Shutdown",
         "isAvailable":true,"deviceTypeIdentifier":"com.apple.CoreSimulator.SimDeviceType.iPhone-16"}]}}"#;
    let devices = parse_simulators(devices_json, r#"{"runtimes":[]}"#).expect("parses");
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].detail.as_deref(), Some("iOS 18.2"));
    assert_eq!(devices[0].family, None);
}

#[test]
fn unknown_states_are_busy_by_name_and_otherwise_unusable() {
    let devices_json = r#"{"devices":{"com.apple.CoreSimulator.SimRuntime.iOS-26-4":[
        {"udid":"00000000-0000-4000-8000-0000000000A1","name":"A","state":"Creating"},
        {"udid":"00000000-0000-4000-8000-0000000000A2","name":"B","state":"Shutting Down Hard"},
        {"udid":"00000000-0000-4000-8000-0000000000A3","name":"C","state":"Bewildered"},
        {"udid":"00000000-0000-4000-8000-0000000000A4","name":"D","state":"Shutdown","isAvailable":false}]}}"#;
    let devices = parse_simulators(devices_json, r#"{"runtimes":[]}"#).expect("parses");
    let states: Vec<_> = devices.iter().map(|device| device.state.clone()).collect();
    assert_eq!(
        states,
        [
            DeviceState::Starting,
            DeviceState::Stopping,
            DeviceState::Unusable("Bewildered".to_string()),
        ],
        "an unavailable simulator is left out"
    );
}

#[test]
fn only_ios_runtimes_are_listed() {
    let devices_json = r#"{"devices":{
        "com.apple.CoreSimulator.SimRuntime.watchOS-11-0":[
            {"udid":"00000000-0000-4000-8000-0000000000B1","name":"Watch","state":"Shutdown"}],
        "com.apple.CoreSimulator.SimRuntime.iOS-26-4":[
            {"udid":"00000000-0000-4000-8000-0000000000B2","name":"Phone","state":"Shutdown"}]}}"#;
    let devices = parse_simulators(devices_json, r#"{"runtimes":[]}"#).expect("parses");
    let names: Vec<_> = devices.iter().map(|device| device.name.as_str()).collect();
    assert_eq!(names, ["Phone"]);
}

#[test]
fn malformed_json_is_an_error_not_a_panic() {
    assert!(parse_simulators("{\"devices\": [", r#"{"runtimes":[]}"#).is_err());
    assert!(parse_simulators(r#"{"devices":{}}"#, "not json").is_err());
}

fn script_lists(host: &FakeHost) {
    host.script_output(
        XCRUN,
        LIST_DEVICES,
        0,
        &tool_output("simctl-list-devices-one-booted.json"),
        "",
    );
    host.script_output(
        XCRUN,
        LIST_RUNTIMES,
        0,
        &tool_output("simctl-list-runtimes.json"),
        "",
    );
}

#[test]
fn a_dropped_core_simulator_connection_is_retried_once() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        XCRUN,
        LIST_DEVICES,
        1,
        "",
        "CoreSimulatorService connection became invalid. Simulator services will no longer be available.",
    );
    script_lists(&host);

    let state = smol::block_on(read_simulators(&host));
    assert_eq!(state.devices().len(), 11, "{state:?}");
}

#[test]
fn no_ios_runtime_means_the_runtime_is_missing() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(XCRUN, LIST_DEVICES, 0, r#"{"devices":{}}"#, "");
    host.script_output(XCRUN, LIST_RUNTIMES, 0, r#"{"runtimes":[]}"#, "");

    let state = smol::block_on(read_simulators(&host));
    assert!(
        matches!(&state, SectionState::ToolMissing { tool, .. } if tool.contains("runtime")),
        "{state:?}"
    );
}

#[test]
fn a_timed_out_or_broken_simctl_is_a_failed_section() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script(
        XCRUN,
        LIST_DEVICES,
        Err(ToolFailure::TimedOut {
            program: XCRUN.into(),
            timeout: std::time::Duration::from_secs(10),
        }),
    );
    host.script_output(
        XCRUN,
        LIST_RUNTIMES,
        0,
        &tool_output("simctl-list-runtimes.json"),
        "",
    );
    let state = smol::block_on(read_simulators(&host));
    assert!(matches!(state, SectionState::Failed { .. }), "{state:?}");

    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(XCRUN, LIST_DEVICES, 0, "{ not json", "");
    host.script_output(
        XCRUN,
        LIST_RUNTIMES,
        0,
        &tool_output("simctl-list-runtimes.json"),
        "",
    );
    let state = smol::block_on(read_simulators(&host));
    assert!(matches!(state, SectionState::Failed { .. }), "{state:?}");
}

#[test]
fn xcode_missing_and_command_line_tools_only_are_told_apart() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        XCODE_SELECT,
        ["-p"],
        2,
        "",
        "xcode-select: error: unable to get active developer directory",
    );
    let missing = smol::block_on(check_xcode(&host)).expect_err("no Xcode");
    let SectionState::ToolMissing { tool, hint } = missing else {
        panic!("expected ToolMissing, got {missing:?}");
    };
    assert_eq!(tool, "Xcode");
    assert!(!hint.contains("Command Line Tools"), "{hint}");

    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        XCODE_SELECT,
        ["-p"],
        0,
        "/Library/Developer/CommandLineTools\n",
        "",
    );
    let tools_only = smol::block_on(check_xcode(&host)).expect_err("tools only");
    let SectionState::ToolMissing { hint, .. } = tools_only else {
        panic!("expected ToolMissing, got {tools_only:?}");
    };
    assert!(hint.contains("Command Line Tools"), "{hint}");

    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        XCODE_SELECT,
        ["-p"],
        0,
        "/Applications/Xcode.app/Contents/Developer\n",
        "",
    );
    assert_eq!(smol::block_on(check_xcode(&host)), Ok(()));
}
