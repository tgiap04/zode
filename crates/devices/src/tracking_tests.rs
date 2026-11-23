use std::time::{Duration, Instant};

use crate::fake_host::FakeHost;
use crate::host::{HostOs, ToolFailure};
use crate::ios_simulators::XCRUN;
use crate::test_fixtures::tool_output;
use crate::tracking::*;
use crate::{Device, DeviceKey, DeviceKind, DeviceState};

const UDID: &str = "00000000-0000-4000-8000-000000000005";
const ADB: &str = "/sdk/platform-tools/adb";
const AVD: &str = "Medium_Phone";
const TIME_LEFT: Duration = Duration::from_secs(100);

fn android_host() -> FakeHost {
    let host = FakeHost::new(HostOs::MacOs);
    host.set_environment_variable("ANDROID_HOME", "/sdk");
    host.add_file(ADB, "");
    host.add_file("/sdk/emulator/emulator", "");
    host
}

fn device(key: DeviceKey, state: DeviceState) -> Device {
    Device {
        key,
        name: "Test".to_string(),
        kind: DeviceKind::IosSimulator,
        state,
        detail: None,
        family: None,
        serial: None,
        run_id: None,
        experimental: false,
    }
}

fn simulator(state: DeviceState) -> Device {
    device(DeviceKey::Simulator(UDID.to_string()), state)
}

fn avd(state: DeviceState) -> Device {
    device(DeviceKey::Avd(AVD.to_string()), state)
}

fn observed(device: Option<Device>, ready: bool) -> ProbeResult {
    ProbeResult {
        device,
        ready,
        error: None,
    }
}

fn boot() -> TrackTarget {
    TrackTarget::SimulatorBoot {
        udid: UDID.to_string(),
    }
}

fn emulator_start(serials_before: &[&str]) -> TrackTarget {
    TrackTarget::EmulatorStart {
        avd_name: AVD.to_string(),
        serials_before: serials_before
            .iter()
            .map(|serial| serial.to_string())
            .collect(),
    }
}

#[test]
fn simulators_get_two_minutes_and_emulators_three() {
    assert_eq!(boot().time_limit(), Duration::from_secs(120));
    assert_eq!(
        TrackTarget::SimulatorShutdown {
            udid: UDID.to_string()
        }
        .time_limit(),
        Duration::from_secs(120)
    );
    assert_eq!(emulator_start(&[]).time_limit(), Duration::from_secs(180));
}

#[test]
fn the_deadline_is_reached_exactly_at_the_deadline() {
    let deadline = Instant::now() + Duration::from_secs(120);
    let waiting = observed(Some(simulator(DeviceState::Starting)), false);
    let one_millisecond = Duration::from_millis(1);

    assert_eq!(
        tracking_verdict(&boot(), &waiting, deadline - one_millisecond, deadline),
        TrackingVerdict::Continue
    );
    assert!(matches!(
        tracking_verdict(&boot(), &waiting, deadline, deadline),
        TrackingVerdict::TimedOut(_)
    ));
    assert!(matches!(
        tracking_verdict(&boot(), &waiting, deadline + one_millisecond, deadline),
        TrackingVerdict::TimedOut(_)
    ));
    let finished = observed(Some(simulator(DeviceState::Running)), true);
    assert_eq!(
        tracking_verdict(&boot(), &finished, deadline, deadline),
        TrackingVerdict::Done(Some(simulator(DeviceState::Running))),
        "an answer that arrives at the deadline still counts"
    );
}

#[test]
fn a_booted_simulator_is_done_only_once_bootstatus_finishes() {
    let now = Instant::now();
    let deadline = now + Duration::from_secs(120);
    let booted_not_ready = observed(Some(simulator(DeviceState::Running)), false);
    assert_eq!(
        tracking_verdict(&boot(), &booted_not_ready, now, deadline),
        TrackingVerdict::Continue
    );
}

#[test]
fn a_shutdown_is_done_when_the_device_is_shut_down_or_gone() {
    let now = Instant::now();
    let deadline = now + Duration::from_secs(120);
    let target = TrackTarget::SimulatorShutdown {
        udid: UDID.to_string(),
    };
    assert_eq!(
        tracking_verdict(&target, &observed(None, false), now, deadline),
        TrackingVerdict::Done(None)
    );
    assert_eq!(
        tracking_verdict(
            &target,
            &observed(Some(simulator(DeviceState::Shutdown)), false),
            now,
            deadline
        ),
        TrackingVerdict::Done(Some(simulator(DeviceState::Shutdown)))
    );
    assert_eq!(
        tracking_verdict(
            &target,
            &observed(Some(simulator(DeviceState::Stopping)), false),
            now,
            deadline
        ),
        TrackingVerdict::Continue
    );

    let stop = TrackTarget::EmulatorStop {
        avd_name: AVD.to_string(),
        serial: "emulator-5554".to_string(),
    };
    assert_eq!(
        tracking_verdict(
            &stop,
            &observed(Some(avd(DeviceState::Shutdown)), false),
            now,
            deadline
        ),
        TrackingVerdict::Done(Some(avd(DeviceState::Shutdown)))
    );
    assert_eq!(
        tracking_verdict(
            &stop,
            &observed(Some(avd(DeviceState::Running)), true),
            now,
            deadline
        ),
        TrackingVerdict::Continue
    );
}

#[test]
fn the_recorded_emulator_boot_finishes_only_when_boot_completed_is_one() {
    let poll = tool_output("adb-emulator-boot-poll.txt");
    let mut verdicts = Vec::new();
    for line in poll.lines() {
        let adb_line = line
            .split_once('[')
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(adb_line, _)| adb_line)
            .expect("an adb line in brackets");
        let boot_completed = line
            .rsplit_once("boot_completed=[")
            .and_then(|(_, rest)| rest.strip_suffix(']'))
            .expect("a boot_completed value");
        let host = android_host();
        host.script_output(
            ADB,
            ["devices", "-l"],
            0,
            &format!("List of devices attached\n{adb_line}\n\n"),
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
            ADB,
            [
                "-s",
                "emulator-5554",
                "shell",
                "getprop",
                "sys.boot_completed",
            ],
            if boot_completed.starts_with("adb:") {
                1
            } else {
                0
            },
            &format!("{boot_completed}\n"),
            "",
        );
        let target = emulator_start(&[]);
        let result = smol::block_on(probe(&host, &host, &target, TIME_LEFT));
        let now = Instant::now();
        verdicts.push(tracking_verdict(
            &target,
            &result,
            now,
            now + Duration::from_secs(180),
        ));
    }
    assert_eq!(verdicts.len(), 5);
    assert!(
        verdicts[..4]
            .iter()
            .all(|verdict| *verdict == TrackingVerdict::Continue),
        "{verdicts:?}"
    );
    let TrackingVerdict::Done(Some(done)) = &verdicts[4] else {
        panic!("the last sample finishes the boot: {verdicts:?}");
    };
    assert_eq!(done.key, DeviceKey::Avd(AVD.to_string()));
    assert_eq!(done.state, DeviceState::Running);
    assert_eq!(done.serial.as_deref(), Some("emulator-5554"));
}

fn script_unnamed_emulators(host: &FakeHost, serials: &[&str]) {
    let mut stdout = "List of devices attached\n".to_string();
    for serial in serials {
        stdout.push_str(&format!("{serial} device\n"));
        host.script_output(
            ADB,
            ["-s", serial, "emu", "avd", "name"],
            1,
            "",
            "error: console refused",
        );
        host.script_output(
            ADB,
            ["-s", serial, "shell", "getprop", "ro.boot.qemu.avd_name"],
            1,
            "",
            "error: closed",
        );
        host.script_output(
            ADB,
            ["-s", serial, "shell", "getprop", "sys.boot_completed"],
            0,
            "1\n",
            "",
        );
    }
    host.script_output(ADB, ["devices", "-l"], 0, &stdout, "");
}

#[test]
fn an_unnamed_emulator_is_ours_only_when_it_is_the_single_new_one() {
    let host = android_host();
    script_unnamed_emulators(&host, &["emulator-5554", "emulator-5556"]);
    let result = smol::block_on(probe(
        &host,
        &host,
        &emulator_start(&["emulator-5554"]),
        TIME_LEFT,
    ));
    let device = result
        .device
        .expect("the single new serial is the pending AVD");
    assert_eq!(device.key, DeviceKey::Avd(AVD.to_string()));
    assert_eq!(device.serial.as_deref(), Some("emulator-5556"));
    assert!(result.ready);

    let host = android_host();
    script_unnamed_emulators(&host, &["emulator-5554", "emulator-5556"]);
    let result = smol::block_on(probe(&host, &host, &emulator_start(&[]), TIME_LEFT));
    assert_eq!(result.device, None, "two new serials are ambiguous");
}

#[test]
fn an_emulator_named_for_another_avd_is_never_taken() {
    let host = android_host();
    host.script_output(
        ADB,
        ["devices", "-l"],
        0,
        "List of devices attached\nemulator-5556 device\n",
        "",
    );
    host.script_output(
        ADB,
        ["-s", "emulator-5556", "emu", "avd", "name"],
        0,
        "Tablet\r\nOK\r\n",
        "",
    );
    let result = smol::block_on(probe(&host, &host, &emulator_start(&[]), TIME_LEFT));
    assert_eq!(result.device, None);
}

#[test]
fn a_stopped_emulator_is_its_avd_shut_down() {
    let host = android_host();
    host.script_output(
        ADB,
        ["devices", "-l"],
        0,
        &tool_output("adb-devices-l-no-devices.txt"),
        "",
    );
    let target = TrackTarget::EmulatorStop {
        avd_name: AVD.to_string(),
        serial: "emulator-5554".to_string(),
    };
    let result = smol::block_on(probe(&host, &host, &target, TIME_LEFT));
    let device = result.device.expect("the AVD row is reported");
    assert_eq!(device.key, DeviceKey::Avd(AVD.to_string()));
    assert_eq!(device.state, DeviceState::Shutdown);
    assert_eq!(device.serial, None);
}

/// A probe asks for the one simulator it follows, and nothing about runtimes.
fn script_simulator_list(host: &FakeHost, devices_fixture: &str) {
    host.script_output(
        XCRUN,
        ["simctl", "list", "devices", "available", "-j", UDID],
        0,
        &tool_output(devices_fixture),
        "",
    );
}

#[test]
fn a_booted_simulator_waits_for_bootstatus_and_a_booting_one_does_not_ask() {
    let host = FakeHost::new(HostOs::MacOs);
    script_simulator_list(&host, "simctl-list-devices-one-booted.json");
    host.script_output(
        XCRUN,
        ["simctl", "bootstatus", UDID, "-b"],
        0,
        "Device already booted, nothing to do.",
        "",
    );
    let result = smol::block_on(probe(&host, &host, &boot(), TIME_LEFT));
    assert_eq!(
        result.device.map(|device| device.state),
        Some(DeviceState::Running)
    );
    assert!(result.ready);

    let host = FakeHost::new(HostOs::MacOs);
    script_simulator_list(&host, "simctl-list-devices-one-booted.json");
    host.script(
        XCRUN,
        ["simctl", "bootstatus", UDID, "-b"],
        Err(ToolFailure::TimedOut {
            program: XCRUN.into(),
            timeout: Duration::from_secs(30),
        }),
    );
    let result = smol::block_on(probe(&host, &host, &boot(), TIME_LEFT));
    assert!(!result.ready);
    assert_eq!(result.error, None, "a slow boot is not an error");

    let host = FakeHost::new(HostOs::MacOs);
    script_simulator_list(&host, "simctl-list-devices-one-booting.json");
    let result = smol::block_on(probe(&host, &host, &boot(), TIME_LEFT));
    assert_eq!(
        result.device.map(|device| device.state),
        Some(DeviceState::Starting)
    );
    assert!(
        !host
            .invocations()
            .iter()
            .any(|invocation| invocation.args.iter().any(|arg| arg == "bootstatus"))
    );
}

#[test]
fn a_probe_that_cannot_run_its_tool_reports_the_error_and_keeps_waiting() {
    let host = FakeHost::new(HostOs::MacOs);
    let target = emulator_start(&[]);
    let result = smol::block_on(probe(&host, &host, &target, TIME_LEFT));
    assert!(result.error.is_some(), "no SDK must be reported");
    let now = Instant::now();
    assert_eq!(
        tracking_verdict(&target, &result, now, now + Duration::from_secs(1)),
        TrackingVerdict::Continue
    );
    let TrackingVerdict::TimedOut(reason) = tracking_verdict(
        &target,
        &result,
        now + Duration::from_secs(1),
        now + Duration::from_secs(1),
    ) else {
        panic!("the deadline passed");
    };
    assert!(
        reason.contains(result.error.as_deref().unwrap_or_default()),
        "{reason}"
    );
}

#[test]
fn following_a_simulator_asks_for_that_one_device_and_never_for_runtimes() {
    let host = FakeHost::new(HostOs::MacOs);
    script_simulator_list(&host, "simctl-list-devices-one-booted.json");
    host.script_output(
        XCRUN,
        ["simctl", "bootstatus", UDID, "-b"],
        0,
        "Device already booted, nothing to do.",
        "",
    );
    let result = smol::block_on(probe(&host, &host, &boot(), TIME_LEFT));
    assert!(result.ready, "{result:?}");
    let listings: Vec<Vec<String>> = host
        .invocations()
        .into_iter()
        .map(|invocation| invocation.args)
        .filter(|args| args.iter().any(|arg| arg == "list"))
        .collect();
    assert_eq!(
        listings,
        [["simctl", "list", "devices", "available", "-j", UDID]],
        "one listing, of this device only"
    );

    let host = FakeHost::new(HostOs::MacOs);
    script_simulator_list(&host, "simctl-list-devices-one-booted.json");
    let result = smol::block_on(probe(
        &host,
        &host,
        &TrackTarget::SimulatorShutdown {
            udid: UDID.to_string(),
        },
        TIME_LEFT,
    ));
    assert_eq!(result.error, None);
    assert_eq!(host.invocations().len(), 1, "a shutdown asks nothing more");
}

#[test]
fn bootstatus_never_waits_past_the_time_left_of_the_operation() {
    let bootstatus_timeout = |time_left: Duration| {
        let host = FakeHost::new(HostOs::MacOs);
        script_simulator_list(&host, "simctl-list-devices-one-booted.json");
        host.script_output(XCRUN, ["simctl", "bootstatus", UDID, "-b"], 0, "", "");
        smol::block_on(probe(&host, &host, &boot(), time_left));
        host.invocations()
            .into_iter()
            .find(|invocation| invocation.args.iter().any(|arg| arg == "bootstatus"))
            .map(|invocation| invocation.timeout)
    };
    assert_eq!(
        bootstatus_timeout(Duration::from_secs(100)),
        Some(Duration::from_secs(30)),
        "plenty of time left: the usual wait"
    );
    assert_eq!(
        bootstatus_timeout(Duration::from_secs(7)),
        Some(Duration::from_secs(7))
    );
    let at_the_deadline = bootstatus_timeout(Duration::ZERO).expect("still asked once");
    assert!(
        at_the_deadline > Duration::ZERO && at_the_deadline <= Duration::from_secs(2),
        "{at_the_deadline:?}"
    );
}
