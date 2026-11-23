use std::collections::HashMap;
use std::path::Path;

use crate::host::{ToolInvocation, ToolRunner};
use crate::{Device, DeviceKey, DeviceKind, DeviceState};

/// The reasons a physical Android row is unusable, said once so the panel can
/// tell which guidance fits without parsing prose.
pub const OFFLINE_REASON: &str = "Offline";
pub const UNAUTHORIZED_REASON: &str = "Allow USB debugging on the device";
pub const NO_PERMISSIONS_REASON: &str = "No permission to open the device; check its udev rules";

pub(crate) const ADB_MISMATCH_MESSAGE: &str = "Two different versions of adb are running. \
     Use one Android SDK: point ANDROID_HOME at it and remove other copies of adb from PATH.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdbState {
    Device,
    Offline,
    Unauthorized,
    NoPermissions,
    Recovery,
    Sideload,
    Bootloader,
    Connecting,
    Authorizing,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdbEntry {
    pub serial: String,
    pub state: AdbState,
    pub model: Option<String>,
}

/// Reads `adb devices -l`. Lines that are not devices (the header, `* daemon`
/// notices, a serial with no state) are skipped rather than failing the list.
pub fn parse_adb_devices(stdout: &str) -> Vec<AdbEntry> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty() && !line.starts_with('*') && !line.starts_with("List of devices")
        })
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let serial = words.next()?.to_string();
            let state = match words.next()? {
                "device" => AdbState::Device,
                "offline" => AdbState::Offline,
                "unauthorized" => AdbState::Unauthorized,
                "no" => AdbState::NoPermissions,
                "recovery" => AdbState::Recovery,
                "sideload" => AdbState::Sideload,
                "bootloader" => AdbState::Bootloader,
                "connecting" => AdbState::Connecting,
                "authorizing" => AdbState::Authorizing,
                other => AdbState::Other(other.to_string()),
            };
            let model = words
                .filter_map(|word| word.strip_prefix("model:"))
                .find(|model| !model.is_empty())
                .map(str::to_string);
            Some(AdbEntry {
                serial,
                state,
                model,
            })
        })
        .collect()
}

pub fn parse_avd_list(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty() && !line.starts_with("INFO") && !line.starts_with("WARNING")
        })
        .map(str::to_string)
        .collect()
}

/// `emu avd name` answers with the name, then `OK`, both CRLF-terminated.
pub(crate) fn parse_avd_name_reply(stdout: &str) -> Option<String> {
    let first_line = stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    if first_line == "OK" || first_line.starts_with("KO") {
        return None;
    }
    Some(first_line.to_string())
}

pub(crate) fn is_adb_server_mismatch(text: &str) -> bool {
    text.contains("doesn't match this client") || text.contains("adb server killed")
}

pub(crate) fn is_emulator_serial(serial: &str) -> bool {
    serial
        .strip_prefix("emulator-")
        .is_some_and(|port| !port.is_empty() && port.chars().all(|digit| digit.is_ascii_digit()))
}

pub(crate) fn emulator_serials(entries: &[AdbEntry]) -> Vec<String> {
    entries
        .iter()
        .filter(|entry| is_emulator_serial(&entry.serial))
        .map(|entry| entry.serial.clone())
        .collect()
}

fn is_wireless_serial(serial: &str) -> bool {
    serial.contains("._adb-tls-connect._tcp") || serial.parse::<std::net::SocketAddr>().is_ok()
}

fn display_name(raw: &str) -> String {
    raw.replace('_', " ")
}

fn usable_state(state: &AdbState) -> DeviceState {
    match state {
        AdbState::Device => DeviceState::Running,
        AdbState::Connecting | AdbState::Authorizing => DeviceState::Starting,
        AdbState::Offline => DeviceState::Unusable(OFFLINE_REASON.to_string()),
        AdbState::Unauthorized => DeviceState::Unusable(UNAUTHORIZED_REASON.to_string()),
        AdbState::NoPermissions => DeviceState::Unusable(NO_PERMISSIONS_REASON.to_string()),
        AdbState::Recovery => DeviceState::Unusable("In recovery mode".to_string()),
        AdbState::Sideload => DeviceState::Unusable("In sideload mode".to_string()),
        AdbState::Bootloader => DeviceState::Unusable("In the bootloader".to_string()),
        AdbState::Other(state) => DeviceState::Unusable(state.clone()),
    }
}

pub(crate) fn physical_devices(entries: &[AdbEntry]) -> Vec<Device> {
    entries
        .iter()
        .filter(|entry| !is_emulator_serial(&entry.serial))
        .map(|entry| Device {
            key: DeviceKey::AndroidSerial(entry.serial.clone()),
            name: entry
                .model
                .as_deref()
                .map_or_else(|| entry.serial.clone(), display_name),
            kind: if is_wireless_serial(&entry.serial) {
                DeviceKind::AndroidWireless
            } else {
                DeviceKind::AndroidUsb
            },
            state: usable_state(&entry.state),
            detail: None,
            family: None,
            serial: Some(entry.serial.clone()),
            run_id: Some(entry.serial.clone()),
            experimental: false,
        })
        .collect()
}

pub(crate) fn avd_device(avd_name: &str, running: Option<(&str, &AdbState)>) -> Device {
    let (state, serial) = match running {
        // An emulator is offline to adb while it boots, not broken.
        Some((serial, AdbState::Offline)) => (DeviceState::Starting, Some(serial.to_string())),
        Some((serial, state)) => (usable_state(state), Some(serial.to_string())),
        None => (DeviceState::Shutdown, None),
    };
    Device {
        key: DeviceKey::Avd(avd_name.to_string()),
        name: display_name(avd_name),
        kind: DeviceKind::AndroidEmulator,
        state,
        detail: None,
        family: None,
        run_id: serial.clone(),
        serial,
        experimental: false,
    }
}

/// One row per AVD, running or not. A running emulator joins its AVD's row;
/// only one whose AVD could not be learned gets a row of its own.
pub(crate) fn emulator_rows(
    avds: &[String],
    entries: &[AdbEntry],
    avd_by_serial: &HashMap<String, String>,
) -> Vec<Device> {
    let emulators: Vec<&AdbEntry> = entries
        .iter()
        .filter(|entry| is_emulator_serial(&entry.serial))
        .collect();
    let running_by_avd: HashMap<&str, &AdbEntry> = emulators
        .iter()
        .filter_map(|entry| Some((avd_by_serial.get(&entry.serial)?.as_str(), *entry)))
        .collect();

    let mut rows: Vec<Device> = avds
        .iter()
        .map(|avd| {
            let running = running_by_avd
                .get(avd.as_str())
                .map(|entry| (entry.serial.as_str(), &entry.state));
            avd_device(avd, running)
        })
        .collect();
    for entry in &emulators {
        if let Some(avd) = avd_by_serial.get(&entry.serial)
            && !avds.contains(avd)
            && running_by_avd.get(avd.as_str()) == Some(entry)
        {
            rows.push(avd_device(avd, Some((&entry.serial, &entry.state))));
        }
    }
    for entry in emulators
        .iter()
        .filter(|entry| !avd_by_serial.contains_key(&entry.serial))
    {
        let mut device = avd_device(&entry.serial, Some((&entry.serial, &entry.state)));
        device.key = DeviceKey::AndroidSerial(entry.serial.clone());
        device.name = format!("Emulator ({})", entry.serial);
        rows.push(device);
    }
    rows
}

pub(crate) async fn read_adb_devices(
    runner: &dyn ToolRunner,
    adb: &Path,
) -> Result<Vec<AdbEntry>, String> {
    let output = runner
        .run(ToolInvocation::new(adb, ["devices", "-l"]))
        .await
        .map_err(|failure| failure.to_string())?;
    if is_adb_server_mismatch(&output.combined()) {
        return Err(ADB_MISMATCH_MESSAGE.to_string());
    }
    if !output.success() {
        return Err(format!("adb failed: {}", output.failure_message()));
    }
    Ok(parse_adb_devices(&output.stdout))
}

pub(crate) async fn read_avds(
    runner: &dyn ToolRunner,
    emulator: &Path,
) -> Result<Vec<String>, String> {
    let output = runner
        .run(ToolInvocation::new(emulator, ["-list-avds"]))
        .await
        .map_err(|failure| failure.to_string())?;
    if !output.success() {
        return Err(format!(
            "could not list the AVDs: {}",
            output.failure_message()
        ));
    }
    Ok(parse_avd_list(&output.stdout))
}

/// The AVD behind each emulator serial, from the emulator console, else from
/// the guest's property. A serial neither answers for is left out.
pub(crate) async fn emulator_avd_names(
    runner: &dyn ToolRunner,
    adb: &Path,
    serials: &[String],
) -> HashMap<String, String> {
    let lookups = serials.iter().map(|serial| async move {
        let serial = serial.as_str();
        let from_console = runner
            .run(ToolInvocation::new(
                adb,
                ["-s", serial, "emu", "avd", "name"],
            ))
            .await
            .ok()
            .filter(|output| output.success())
            .and_then(|output| parse_avd_name_reply(&output.stdout));
        let name = match from_console {
            Some(name) => Some(name),
            None => runner
                .run(ToolInvocation::new(
                    adb,
                    ["-s", serial, "shell", "getprop", "ro.boot.qemu.avd_name"],
                ))
                .await
                .ok()
                .filter(|output| output.success())
                .map(|output| output.stdout.trim().to_string())
                .filter(|name| !name.is_empty()),
        };
        name.map(|name| (serial.to_string(), name))
    });
    futures::future::join_all(lookups)
        .await
        .into_iter()
        .flatten()
        .collect()
}
