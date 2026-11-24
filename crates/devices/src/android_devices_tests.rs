use std::collections::HashMap;
use std::path::Path;

use crate::android_devices::*;
use crate::fake_host::FakeHost;
use crate::host::HostOs;
use crate::test_fixtures::tool_output;
use crate::{DeviceKey, DeviceKind, DeviceState};

const ADB: &str = "/sdk/platform-tools/adb";
const EMULATOR: &str = "/sdk/emulator/emulator";

#[test]
fn recorded_adb_lists_parse_into_entries() {
    assert_eq!(
        parse_adb_devices(&tool_output("adb-devices-l-no-devices.txt")),
        []
    );
    assert_eq!(
        parse_adb_devices(&tool_output("adb-devices-l-emulator-offline.txt")),
        [AdbEntry {
            serial: "emulator-5554".to_string(),
            state: AdbState::Offline,
            model: None,
        }]
    );
    assert_eq!(
        parse_adb_devices(&tool_output("adb-devices-l-emulator-online.txt")),
        [AdbEntry {
            serial: "emulator-5554".to_string(),
            state: AdbState::Device,
            model: Some("sdk_gphone16k_arm64".to_string()),
        }]
    );
}

#[test]
fn every_adb_state_crlf_and_noise_lines_are_handled() {
    let stdout = "* daemon not running; starting now at tcp:5037\r\n\
        * daemon started successfully\r\n\
        List of devices attached\r\n\
        A1 device usb:1-1 product:p model:Pixel_8 device:shiba transport_id:2\r\n\
        A2 offline\r\n\
        A3 unauthorized usb:1-2 transport_id:3\r\n\
        A4 no permissions (missing udev rules? user is in the plugdev group); see [http://developer.android.com/tools/device.html] usb:1-3\r\n\
        A5 recovery\r\n\
        A6 sideload\r\n\
        A7 bootloader\r\n\
        A8 connecting\r\n\
        A9 authorizing\r\n\
        A10 rescue\r\n\
        lonely\r\n\
        \r\n";
    let states: Vec<_> = parse_adb_devices(stdout)
        .into_iter()
        .map(|entry| (entry.serial, entry.state))
        .collect();
    assert_eq!(
        states,
        [
            ("A1".to_string(), AdbState::Device),
            ("A2".to_string(), AdbState::Offline),
            ("A3".to_string(), AdbState::Unauthorized),
            ("A4".to_string(), AdbState::NoPermissions),
            ("A5".to_string(), AdbState::Recovery),
            ("A6".to_string(), AdbState::Sideload),
            ("A7".to_string(), AdbState::Bootloader),
            ("A8".to_string(), AdbState::Connecting),
            ("A9".to_string(), AdbState::Authorizing),
            ("A10".to_string(), AdbState::Other("rescue".to_string())),
        ]
    );
}

#[test]
fn physical_devices_are_split_by_serial_shape_and_offline_is_not_usable() {
    let entries = parse_adb_devices(
        "List of devices attached\n\
         emulator-5554 device model:sdk\n\
         R58M123ABC device usb:1-1 model:Galaxy_S24\n\
         192.168.1.5:5555 device model:Pixel_8\n\
         adb-R58M123ABC-x1y2._adb-tls-connect._tcp device model:Pixel_8\n\
         R58M999 offline\n",
    );
    let devices = physical_devices(&entries);
    let summary: Vec<_> = devices
        .iter()
        .map(|device| (device.name.as_str(), device.kind, device.state.clone()))
        .collect();
    assert_eq!(
        summary,
        [
            ("Galaxy S24", DeviceKind::AndroidUsb, DeviceState::Running),
            ("Pixel 8", DeviceKind::AndroidWireless, DeviceState::Running),
            ("Pixel 8", DeviceKind::AndroidWireless, DeviceState::Running),
            (
                "R58M999",
                DeviceKind::AndroidUsb,
                DeviceState::Unusable("Offline".to_string())
            ),
        ]
    );
    assert_eq!(
        devices[0].key,
        DeviceKey::AndroidSerial("R58M123ABC".to_string())
    );
    assert_eq!(devices[0].run_id.as_deref(), Some("R58M123ABC"));
    assert!(is_emulator_serial("emulator-5554"));
    assert!(!is_emulator_serial("emulator-"));
    assert!(!is_emulator_serial("emulator-55x4"));
}

#[test]
fn avd_lists_drop_blank_and_log_lines() {
    assert_eq!(
        parse_avd_list(&tool_output("emulator-list-avds.txt")),
        ["Medium_Phone"]
    );
    assert_eq!(
        parse_avd_list(
            "INFO    | Storing crashdata in: /tmp\r\nWARNING | old\r\n\r\nPixel_7\r\nTablet\n"
        ),
        ["Pixel_7", "Tablet"]
    );
}

#[test]
fn the_avd_name_reply_drops_crlf_and_the_ok_line() {
    assert_eq!(
        parse_avd_name_reply(&tool_output("adb-emu-avd-name.txt")),
        Some("Medium_Phone".to_string())
    );
    assert_eq!(parse_avd_name_reply("OK\r\n"), None);
    assert_eq!(parse_avd_name_reply(""), None);
    assert_eq!(parse_avd_name_reply("KO: unknown command\r\n"), None);
}

#[test]
fn an_avd_name_reply_that_is_no_avd_name_is_dropped() {
    let too_long = "a".repeat(101);
    for reply in [
        "Pixel 7\r\nOK\r\n",
        "\u{1b}]0;title\u{7}\r\nOK\r\n",
        "-wipe-data\r\nOK\r\n",
        "$(reboot)\r\nOK\r\n",
        &format!("{too_long}\r\nOK\r\n"),
    ] {
        assert_eq!(parse_avd_name_reply(reply), None, "{reply:?}");
    }
    let longest = "a".repeat(100);
    assert_eq!(
        parse_avd_name_reply(&format!("{longest}\r\nOK\r\n")),
        Some(longest)
    );
}

#[test]
fn a_hostile_listener_on_an_emulator_port_cannot_name_a_row() {
    let host = FakeHost::new(HostOs::MacOs);
    let flood = "x".repeat(10_000);
    host.script_output(
        ADB,
        ["-s", "emulator-5554", "emu", "avd", "name"],
        0,
        &format!("{flood}\r\nOK\r\n"),
        "",
    );
    host.script_output(
        ADB,
        [
            "-s",
            "emulator-5554",
            "shell",
            "getprop",
            "ro.boot.qemu.avd_name",
        ],
        0,
        "evil name\u{1b}[2J\n",
        "",
    );
    let names = smol::block_on(emulator_avd_names(
        &host,
        Path::new(ADB),
        &["emulator-5554".to_string()],
    ));
    assert_eq!(names, HashMap::new());
}

#[test]
fn a_running_emulator_shares_one_row_with_its_avd() {
    let avds = vec!["Medium_Phone".to_string(), "Tablet".to_string()];
    let entries = parse_adb_devices(&tool_output("adb-devices-l-emulator-online.txt"));
    let names = HashMap::from([("emulator-5554".to_string(), "Medium_Phone".to_string())]);

    let rows = emulator_rows(&avds, &entries, &names);
    assert_eq!(rows.len(), 2, "{rows:?}");
    let phone = &rows[0];
    assert_eq!(phone.key, DeviceKey::Avd("Medium_Phone".to_string()));
    assert_eq!(phone.name, "Medium Phone");
    assert_eq!(phone.state, DeviceState::Running);
    assert_eq!(phone.serial.as_deref(), Some("emulator-5554"));
    assert_eq!(phone.run_id.as_deref(), Some("emulator-5554"));
    assert_eq!(rows[1].state, DeviceState::Shutdown);
    assert_eq!(rows[1].run_id, None);
}

#[test]
fn a_booting_emulator_is_starting_and_an_unnamed_one_gets_its_own_row() {
    let avds = vec!["Medium_Phone".to_string()];
    let entries = parse_adb_devices(
        "List of devices attached\nemulator-5554 offline\nemulator-5556 device\n",
    );
    let names = HashMap::from([("emulator-5554".to_string(), "Medium_Phone".to_string())]);

    let rows = emulator_rows(&avds, &entries, &names);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0].state, DeviceState::Starting);
    assert_eq!(
        rows[1].key,
        DeviceKey::AndroidSerial("emulator-5556".to_string())
    );
    assert_eq!(rows[1].name, "Emulator (emulator-5556)");
    assert_eq!(rows[1].kind, DeviceKind::AndroidEmulator);
}

#[test]
fn an_adb_client_server_mismatch_fails_with_a_one_sdk_message() {
    for stderr in [
        "adb server version (41) doesn't match this client (39); killing...\n",
        "error: adb server killed by remote request\n",
    ] {
        let host = FakeHost::new(HostOs::Linux);
        host.script_output(
            ADB,
            ["devices", "-l"],
            0,
            "List of devices attached\n",
            stderr,
        );
        let error = smol::block_on(read_adb_devices(&host, Path::new(ADB)))
            .expect_err("a mismatch is an error");
        assert_eq!(error, ADB_MISMATCH_MESSAGE);
        assert!(error.contains("one Android SDK"), "{error}");
        let commands: Vec<Vec<String>> = host
            .invocations()
            .into_iter()
            .map(|invocation| invocation.args)
            .collect();
        assert_eq!(commands, [["devices", "-l"]], "adb's server is left alone");
    }
}

#[test]
fn a_cold_start_daemon_message_is_not_an_error() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        ADB,
        ["devices", "-l"],
        0,
        &tool_output("adb-devices-l-no-devices.txt"),
        &tool_output("adb-devices-l-cold-start-stderr.txt"),
    );
    assert_eq!(
        smol::block_on(read_adb_devices(&host, Path::new(ADB))),
        Ok(Vec::new())
    );
}

#[test]
fn the_avd_name_falls_back_to_getprop_when_the_console_fails() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        ADB,
        ["-s", "emulator-5554", "emu", "avd", "name"],
        1,
        "",
        "error: could not connect to TCP port 5554: Connection refused",
    );
    host.script_output(
        ADB,
        [
            "-s",
            "emulator-5554",
            "shell",
            "getprop",
            "ro.boot.qemu.avd_name",
        ],
        0,
        "Medium_Phone\n",
        "",
    );
    host.script_output(
        ADB,
        ["-s", "emulator-5556", "emu", "avd", "name"],
        0,
        &tool_output("adb-emu-avd-name.txt").replace("Medium_Phone", "Tablet"),
        "",
    );

    let names = smol::block_on(emulator_avd_names(
        &host,
        Path::new(ADB),
        &["emulator-5554".to_string(), "emulator-5556".to_string()],
    ));
    assert_eq!(
        names,
        HashMap::from([
            ("emulator-5554".to_string(), "Medium_Phone".to_string()),
            ("emulator-5556".to_string(), "Tablet".to_string()),
        ])
    );
}

#[test]
fn reading_avds_reports_a_failing_emulator_binary() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        EMULATOR,
        ["-list-avds"],
        0,
        &tool_output("emulator-list-avds.txt"),
        "",
    );
    assert_eq!(
        smol::block_on(read_avds(&host, Path::new(EMULATOR))),
        Ok(vec!["Medium_Phone".to_string()])
    );

    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(EMULATOR, ["-list-avds"], 1, "", "emulator: ERROR: broken");
    assert!(smol::block_on(read_avds(&host, Path::new(EMULATOR))).is_err());
}
