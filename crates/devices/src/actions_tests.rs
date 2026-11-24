use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::actions::*;
use crate::android_sdk::AndroidSdk;
use crate::fake_host::FakeHost;
use crate::host::{HostOs, HostSystem, ToolInvocation, ToolRunner};
use crate::input_validation::{parse_pairing_code, parse_wireless_address};
use crate::ios_simulators::XCRUN;
use crate::resources::emulator_log_file_name;
use crate::test_fixtures::tool_output;

const UDID: &str = "00000000-0000-4000-8000-000000000005";
const ADB: &str = "/sdk/platform-tools/adb";
const EMULATOR: &str = "/sdk/emulator/emulator";
const CODE: &str = "482915";

fn sdk() -> AndroidSdk {
    AndroidSdk {
        root: PathBuf::from("/sdk"),
        adb: PathBuf::from(ADB),
        emulator: Some(PathBuf::from(EMULATOR)),
        avd_home: None,
    }
}

#[test]
fn booting_or_shutting_down_a_simulator_already_in_that_state_succeeds() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        XCRUN,
        ["simctl", "boot", UDID],
        149,
        "",
        &tool_output("simctl-boot-already-booted.txt"),
    );
    host.script_output(
        XCRUN,
        ["simctl", "shutdown", UDID],
        149,
        "",
        &tool_output("simctl-shutdown-already-shutdown.txt"),
    );
    smol::block_on(boot_simulator(&host, UDID)).expect("already booted is success");
    smol::block_on(shutdown_simulator(&host, UDID)).expect("already shut down is success");
}

#[test]
fn other_simctl_failures_stay_failures() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        XCRUN,
        ["simctl", "boot", UDID],
        149,
        "",
        "Unable to boot device in current state: Shutting Down",
    );
    host.script_output(
        XCRUN,
        ["simctl", "shutdown", UDID],
        149,
        "",
        "Unable to shutdown device in current state: Booting",
    );
    let boot_error = smol::block_on(boot_simulator(&host, UDID)).expect_err("boot fails");
    assert!(
        boot_error.to_string().contains("Shutting Down"),
        "{boot_error}"
    );
    assert!(smol::block_on(shutdown_simulator(&host, UDID)).is_err());
}

#[test]
fn invalid_ids_are_refused_before_any_tool_runs() {
    let host = FakeHost::new(HostOs::MacOs);
    assert!(smol::block_on(boot_simulator(&host, "-h")).is_err());
    assert!(smol::block_on(shutdown_simulator(&host, "$(reboot)")).is_err());
    assert!(smol::block_on(open_simulator_window(&host, "all")).is_err());
    assert!(smol::block_on(stop_emulator(&host, &sdk(), "-a")).is_err());
    assert!(
        start_emulator(
            &host,
            &host,
            &sdk(),
            "-wipe-data",
            Path::new("/logs"),
            SystemTime::now()
        )
        .is_err()
    );
    assert!(host.invocations().is_empty(), "{:?}", host.invocations());
}

#[test]
fn the_simulator_window_is_opened_through_simulator_app() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        "/usr/bin/open",
        ["-a", "Simulator", "--args", "-CurrentDeviceUDID", UDID],
        0,
        "",
        "",
    );
    smol::block_on(open_simulator_window(&host, UDID)).expect("opens");
    assert_eq!(host.invocations().len(), 1);
}

#[test]
fn an_emulator_starts_detached_with_a_fresh_log_and_old_logs_pruned() {
    let host = FakeHost::new(HostOs::Linux);
    let log_dir = PathBuf::from("/logs");
    let first = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    for minute in 0..6 {
        host.add_file(
            log_dir.join(emulator_log_file_name(
                "Medium_Phone",
                first + Duration::from_secs(minute * 60),
            )),
            "",
        );
    }
    let started_at = first + Duration::from_secs(3600);

    let log_file = start_emulator(&host, &host, &sdk(), "Medium_Phone", &log_dir, started_at)
        .expect("starts")
        .log_file;

    assert_eq!(
        log_file,
        log_dir.join(emulator_log_file_name("Medium_Phone", started_at))
    );
    let detached = host.detached();
    assert_eq!(detached.len(), 1);
    assert_eq!(detached[0].0.program, PathBuf::from(EMULATOR));
    assert_eq!(detached[0].0.args, ["-avd", "Medium_Phone"]);
    assert_eq!(detached[0].1, log_file);
    let logs = host
        .read_dir_names(&log_dir)
        .expect("the log directory exists");
    assert_eq!(logs.len(), crate::resources::EMULATOR_LOGS_KEPT, "{logs:?}");
    assert!(!host.has_file(&log_dir.join(emulator_log_file_name("Medium_Phone", first))));
}

#[test]
fn starting_an_emulator_also_clears_stale_logs_of_other_avds() {
    let host = FakeHost::new(HostOs::Linux);
    let log_dir = PathBuf::from("/logs");
    let started_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let long_ago = started_at - Duration::from_secs(30 * 24 * 60 * 60);
    let stale = log_dir.join(emulator_log_file_name("Tablet", long_ago));
    let recent = log_dir.join(emulator_log_file_name(
        "Tablet",
        started_at - Duration::from_secs(60),
    ));
    host.add_file(&stale, "x");
    host.add_file(&recent, "x");

    start_emulator(&host, &host, &sdk(), "Medium_Phone", &log_dir, started_at).expect("starts");

    assert!(!host.has_file(&stale));
    assert!(host.has_file(&recent));
}

#[test]
fn a_started_emulator_that_never_reached_adb_is_stopped_through_its_process_group() {
    let host = FakeHost::new(HostOs::Linux);
    let mut started = start_emulator(
        &host,
        &host,
        &sdk(),
        "Medium_Phone",
        Path::new("/logs"),
        SystemTime::UNIX_EPOCH,
    )
    .expect("starts");
    smol::block_on(stop_detached_emulator(
        &host,
        &host,
        started.process_id,
        &mut started.exit,
    ))
    .expect("stops");
    assert_eq!(host.terminated_groups(), [started.process_id]);
    assert!(
        host.invocations()
            .iter()
            .all(|invocation| invocation.program != Path::new("/bin/kill")),
        "no external kill program is needed"
    );

    let refusing = FakeHost::new(HostOs::MacOs);
    refusing.fail_terminate("Operation not permitted");
    let mut exit = running_exit(&refusing);
    let error = smol::block_on(stop_detached_emulator(&refusing, &refusing, 9, &mut exit))
        .expect_err("a refused signal is an error");
    assert!(
        error.to_string().contains("Operation not permitted"),
        "{error}"
    );
}

#[test]
fn an_emulator_that_ended_before_the_kill_is_never_signalled_by_its_stale_id() {
    let host = FakeHost::new(HostOs::Linux);
    let mut started = start_emulator(
        &host,
        &host,
        &sdk(),
        "Medium_Phone",
        Path::new("/logs"),
        SystemTime::UNIX_EPOCH,
    )
    .expect("starts");
    host.exit_detached(0, Some(0), "");
    smol::block_on(stop_detached_emulator(
        &host,
        &host,
        started.process_id,
        &mut started.exit,
    ))
    .expect("an ended emulator is already stopped");
    assert!(host.terminated_groups().is_empty());

    let windows = FakeHost::new(HostOs::Windows);
    windows.set_environment_variable("SystemRoot", "/win");
    let mut started = start_emulator(
        &windows,
        &windows,
        &sdk(),
        "Medium_Phone",
        Path::new("/logs"),
        SystemTime::UNIX_EPOCH,
    )
    .expect("starts");
    windows.exit_detached(0, None, "");
    smol::block_on(stop_detached_emulator(
        &windows,
        &windows,
        started.process_id,
        &mut started.exit,
    ))
    .expect("an ended emulator is already stopped");
    let invocations = windows.invocations();
    assert!(
        invocations
            .iter()
            .all(|invocation| !invocation.program.ends_with("taskkill.exe")),
        "{invocations:?}"
    );
}

#[test]
fn on_windows_the_emulator_and_its_children_are_stopped_with_taskkill() {
    let host = FakeHost::new(HostOs::Windows);
    // A rooted path on every system the tests run on.
    host.set_environment_variable("SystemRoot", "/win");
    let taskkill = PathBuf::from("/win").join("System32").join("taskkill.exe");
    host.script_output(&taskkill, ["/PID", "77", "/T", "/F"], 0, "", "");
    let mut exit = running_exit(&host);
    smol::block_on(stop_detached_emulator(&host, &host, 77, &mut exit)).expect("stops");
    let invocation = host.invocations().pop().expect("taskkill ran");
    assert_eq!(invocation.program, taskkill);
    assert_eq!(invocation.args, ["/PID", "77", "/T", "/F"]);
}

/// The exit signal of a process that is still running.
fn running_exit(host: &FakeHost) -> crate::host::DetachedExit {
    host.spawn_detached(
        ToolInvocation::new("/bin/true", ["x"]),
        Path::new("/logs/running.log"),
    )
    .expect("starts")
    .exit
}

#[test]
fn an_sdk_without_the_emulator_cannot_start_one() {
    let host = FakeHost::new(HostOs::MacOs);
    let mut sdk = sdk();
    sdk.emulator = None;
    let error = start_emulator(
        &host,
        &host,
        &sdk,
        "Medium_Phone",
        Path::new("/logs"),
        SystemTime::now(),
    )
    .expect_err("no emulator binary");
    assert!(error.to_string().contains("Emulator"), "{error}");
    assert!(host.detached().is_empty());
}

#[test]
fn an_emulator_is_stopped_through_its_console() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        ADB,
        ["-s", "emulator-5554", "emu", "kill"],
        0,
        &tool_output("adb-emu-kill.txt"),
        "",
    );
    host.script_output(
        ADB,
        ["-s", "emulator-5556", "emu", "kill"],
        1,
        "",
        "error: could not connect to TCP port 5556: Connection refused",
    );
    smol::block_on(stop_emulator(&host, &sdk(), "emulator-5554")).expect("stops");
    assert!(smol::block_on(stop_emulator(&host, &sdk(), "emulator-5556")).is_err());
}

#[test]
fn the_pairing_code_goes_through_stdin_and_never_into_argv_or_errors() {
    let address = parse_wireless_address("192.168.1.5:37099").expect("valid");
    let code = parse_pairing_code(CODE).expect("valid");
    let recorded = tool_output("adb-pair-reads-code-from-stdin.txt");
    let protocol_fault = recorded
        .lines()
        .find(|line| line.starts_with("error:"))
        .expect("the fixture has the error line");
    let echoed = format!("Enter pairing code: {CODE}\nerror: protocol fault");
    for (exit_code, stdout, stderr) in [
        (
            0,
            "Enter pairing code: Failed: Wrong password or connection was dropped.",
            "",
        ),
        (1, "Enter pairing code: ", protocol_fault),
        (0, echoed.as_str(), ""),
    ] {
        let host = FakeHost::new(HostOs::MacOs);
        host.script_output(
            ADB,
            ["pair", "192.168.1.5:37099"],
            exit_code,
            stdout,
            stderr,
        );
        let error = smol::block_on(adb_pair(&host, &sdk(), &address, &code))
            .expect_err("a failed pairing is an error");
        let message = format!("{error:#}");
        assert!(!message.contains(CODE), "{message}");
        assert!(!message.is_empty());
        let invocation = host.invocations().pop().expect("adb ran");
        assert_eq!(invocation.stdin.as_deref(), Some("482915\n"));
        assert!(!invocation.args.iter().any(|arg| arg.contains(CODE)));
    }

    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        ADB,
        ["pair", "192.168.1.5:37099"],
        0,
        "Enter pairing code: Successfully paired to 192.168.1.5:37099 [guid=adb-R58M-x]",
        "",
    );
    smol::block_on(adb_pair(&host, &sdk(), &address, &code)).expect("paired");
}

#[test]
fn a_pairing_is_judged_by_its_success_line_not_by_words_in_the_guid() {
    let address = parse_wireless_address("192.168.1.5:37099").expect("valid");
    let code = parse_pairing_code(CODE).expect("valid");
    for (exit_code, stdout, paired) in [
        (
            0,
            "Enter pairing code: Successfully paired to 192.168.1.5:37099 [guid=adb-failsafe-error-x]",
            true,
        ),
        (
            0,
            "Successfully paired to 192.168.1.5:37099 [guid=adb-R58M-x]\n",
            true,
        ),
        // The tool exited 1: no success line can rescue it.
        (
            1,
            "Enter pairing code: Successfully paired to 192.168.1.5:37099 [guid=adb-R58M-x]",
            false,
        ),
        // Exit 0 without a success line is not a pairing, whatever it says.
        (0, "Enter pairing code: ", false),
        (0, "Enter pairing code: Wrong password", false),
        (
            0,
            "Enter pairing code: Failed: Wrong password or connection was dropped.",
            false,
        ),
    ] {
        let host = FakeHost::new(HostOs::MacOs);
        host.script_output(ADB, ["pair", "192.168.1.5:37099"], exit_code, stdout, "");
        let result = smol::block_on(adb_pair(&host, &sdk(), &address, &code));
        assert_eq!(result.is_ok(), paired, "{stdout:?}: {result:?}");
    }
}

#[test]
fn connect_and_disconnect_are_judged_by_their_output_not_the_exit_code() {
    let address = parse_wireless_address("192.168.1.5:5555").expect("valid");
    for (stdout, succeeds) in [
        ("connected to 192.168.1.5:5555", true),
        ("already connected to 192.168.1.5:5555", true),
        (
            "failed to connect to '192.168.1.5:5555': Connection refused",
            false,
        ),
        (
            "cannot connect to 192.168.1.5:5555: No route to host (65)",
            false,
        ),
    ] {
        let host = FakeHost::new(HostOs::MacOs);
        host.script_output(ADB, ["connect", "192.168.1.5:5555"], 0, stdout, "");
        let result = smol::block_on(adb_connect(&host, &sdk(), &address));
        assert_eq!(result.is_ok(), succeeds, "{stdout}: {result:?}");
    }

    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        ADB,
        ["disconnect", "192.168.1.5:5555"],
        0,
        "disconnected 192.168.1.5:5555",
        "",
    );
    smol::block_on(adb_disconnect(&host, &sdk(), &address)).expect("disconnects");
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        ADB,
        ["disconnect", "192.168.1.5:5555"],
        1,
        "",
        "error: no such device '192.168.1.5:5555'",
    );
    assert!(smol::block_on(adb_disconnect(&host, &sdk(), &address)).is_err());
}

#[test]
fn a_boot_or_shutdown_already_under_way_is_left_to_the_follower() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        XCRUN,
        ["simctl", "boot", UDID],
        149,
        "",
        "An error was encountered processing the command (domain=com.apple.CoreSimulator.SimError, code=405):\nUnable to boot device in current state: Booting",
    );
    host.script_output(
        XCRUN,
        ["simctl", "shutdown", UDID],
        149,
        "",
        "An error was encountered processing the command (domain=com.apple.CoreSimulator.SimError, code=405):\nUnable to shutdown device in current state: Shutting Down",
    );
    let booted = smol::block_on(boot_simulator(&host, UDID));
    assert!(booted.is_ok(), "{booted:?}");
    let shut_down = smol::block_on(shutdown_simulator(&host, UDID));
    assert!(shut_down.is_ok(), "{shut_down:?}");
}

#[test]
fn simctl_boot_and_shutdown_may_take_two_minutes() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(XCRUN, ["simctl", "boot", UDID], 0, "", "");
    host.script_output(XCRUN, ["simctl", "shutdown", UDID], 0, "", "");
    smol::block_on(boot_simulator(&host, UDID)).expect("boots");
    smol::block_on(shutdown_simulator(&host, UDID)).expect("shuts down");
    let timeouts: Vec<Duration> = host
        .invocations()
        .iter()
        .map(|invocation| invocation.timeout)
        .collect();
    assert_eq!(timeouts, [Duration::from_secs(120); 2]);
}
