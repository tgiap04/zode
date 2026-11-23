use std::path::Path;
use std::time::Duration;

use crate::fake_host::FakeHost;
use crate::host::{HostOs, HostSystem};
use crate::ios_devices::{parse_devicectl_devices, read_physical_iphones};
use crate::ios_simulators::XCRUN;
use crate::test_fixtures::tool_output;
use crate::{DeviceKey, DeviceKind, DeviceState, SectionState};

const HARDWARE_UDID: &str = "00008120-0000000000000001";
const COREDEVICE_IDENTIFIER: &str = "00000000-0000-4000-8000-0000000000a1";

fn recorded() -> String {
    tool_output("devicectl-list-devices-paired-unreachable.json")
}

#[test]
fn a_paired_but_unplugged_iphone_is_listed_as_not_usable_and_experimental() {
    let devices = parse_devicectl_devices(&recorded()).expect("the fixture parses");
    assert_eq!(devices.len(), 1);
    let iphone = &devices[0];
    assert_eq!(iphone.key, DeviceKey::IosDevice(HARDWARE_UDID.to_string()));
    assert_eq!(iphone.kind, DeviceKind::IosPhysical);
    assert_eq!(iphone.name, "Test iPhone");
    assert_eq!(iphone.detail.as_deref(), Some("iOS 27.0"));
    assert_eq!(iphone.family.as_deref(), Some("iPhone"));
    assert!(iphone.experimental);
    assert!(
        matches!(iphone.state, DeviceState::Unusable(_)),
        "{:?}",
        iphone.state
    );
}

#[test]
fn the_run_id_is_the_hardware_udid_not_the_coredevice_identifier() {
    let devices = parse_devicectl_devices(&recorded()).expect("the fixture parses");
    assert_eq!(devices[0].run_id.as_deref(), Some(HARDWARE_UDID));
    assert_ne!(devices[0].run_id.as_deref(), Some(COREDEVICE_IDENTIFIER));
}

#[test]
fn a_paired_iphone_with_a_connected_tunnel_is_running() {
    let json = recorded().replace(
        "\"tunnelState\" : \"unavailable\"",
        "\"tunnelState\" : \"connected\"",
    );
    let devices = parse_devicectl_devices(&json).expect("parses");
    assert_eq!(devices[0].state, DeviceState::Running);
}

#[test]
fn missing_optional_fields_give_an_unknown_state_instead_of_a_panic() {
    let json = r#"{"info":{"outcome":"success"},"result":{"devices":[
        {"identifier":"x","hardwareProperties":{"udid":"00008120-0000000000000002"},
         "deviceProperties":{"name":"Bare"}},
        {"identifier":"y","deviceProperties":{"name":"No UDID"}},
        {"hardwareProperties":{"udid":"00008120-0000000000000003"},"deviceProperties":{"name":"No id"}}
    ]}}"#;
    let devices = parse_devicectl_devices(json).expect("parses");
    assert_eq!(
        devices.len(),
        1,
        "entries without identifier or udid are left out"
    );
    assert_eq!(devices[0].name, "Bare");
    assert!(matches!(devices[0].state, DeviceState::Unusable(_)));
    assert_eq!(devices[0].detail, None);
}

#[test]
fn an_outcome_other_than_success_is_an_error() {
    let json = recorded().replace("\"outcome\" : \"success\"", "\"outcome\" : \"failed\"");
    assert!(parse_devicectl_devices(&json).is_err());
    assert!(parse_devicectl_devices(r#"{"result":{"devices":[]}}"#).is_err());
    assert!(parse_devicectl_devices("{").is_err());
}

#[test]
fn devicectl_writes_json_to_a_temporary_file_that_is_removed_after_reading() {
    let host = FakeHost::new(HostOs::MacOs);
    let output_path = host.temp_file_path("devicectl-devices.json");
    host.add_file(&output_path, recorded());
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
        &tool_output("devicectl-list-devices-stderr.txt"),
    );

    let state = smol::block_on(read_physical_iphones(&host, &host));
    assert_eq!(state.devices().len(), 1, "{state:?}");
    assert!(!host.has_file(Path::new(&output_path)));
    let invocation = host.invocations().pop().expect("devicectl ran");
    assert_eq!(invocation.timeout, Duration::from_secs(15));
}

#[test]
fn an_xcode_without_devicectl_is_reported_as_missing_the_tool() {
    let host = FakeHost::new(HostOs::MacOs);
    let path_argument = host
        .temp_file_path("devicectl-devices.json")
        .to_string_lossy()
        .into_owned();
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
        72,
        "",
        "xcrun: error: unable to find utility \"devicectl\", not a developer tool or in PATH",
    );

    let state = smol::block_on(read_physical_iphones(&host, &host));
    assert!(
        matches!(&state, SectionState::ToolMissing { tool, .. } if tool == "devicectl"),
        "{state:?}"
    );
}

#[test]
fn the_temporary_file_is_removed_when_devicectl_fails() {
    for answer in [
        Ok(crate::host::ToolOutput {
            exit_code: Some(1),
            stdout: String::new(),
            stderr: "ERROR: The operation couldn't be completed.".to_string(),
        }),
        Err(crate::host::ToolFailure::TimedOut {
            program: XCRUN.into(),
            timeout: Duration::from_secs(15),
        }),
    ] {
        let host = FakeHost::new(HostOs::MacOs);
        let output_path = host.temp_file_path("devicectl-devices.json");
        // devicectl may write the file before it fails or is stopped.
        host.add_file(&output_path, recorded());
        let path_argument = output_path.to_string_lossy().into_owned();
        host.script(
            XCRUN,
            [
                "devicectl",
                "list",
                "devices",
                "--json-output",
                path_argument.as_str(),
                "-q",
            ],
            answer.clone(),
        );

        let state = smol::block_on(read_physical_iphones(&host, &host));
        assert!(matches!(state, SectionState::Failed { .. }), "{state:?}");
        assert!(!host.has_file(&output_path), "{answer:?}");
    }
}
