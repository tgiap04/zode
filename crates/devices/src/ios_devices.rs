use std::path::Path;
use std::time::Duration;

use anyhow::{Context as _, bail};
use serde_json::Value;

use crate::host::{HostSystem, ToolInvocation, ToolRunner};
use crate::ios_simulators::XCRUN;
use crate::{Device, DeviceKey, DeviceKind, DeviceState, SectionState};

/// A device list is a few KB; this only stops a runaway file.
pub(crate) const MAX_DEVICECTL_JSON_BYTES: u64 = 4 * 1024 * 1024;
const DEVICECTL_TIMEOUT: Duration = Duration::from_secs(15);

/// devicectl's JSON has no published schema. Only `identifier`, the hardware
/// `udid` and the name are required; an entry without them is left out, and
/// every other field may be missing.
pub(crate) fn parse_devicectl_devices(json: &str) -> anyhow::Result<Vec<Device>> {
    let root: Value = serde_json::from_str(json).context("could not read devicectl's output")?;
    let outcome = root.pointer("/info/outcome").and_then(Value::as_str);
    if outcome != Some("success") {
        bail!(
            "devicectl reported {}",
            outcome.map_or_else(
                || "no outcome".to_string(),
                |outcome| format!("\"{outcome}\"")
            )
        );
    }
    let entries = root
        .pointer("/result/devices")
        .and_then(Value::as_array)
        .context("devicectl's output has no device list")?;
    Ok(entries.iter().filter_map(physical_iphone).collect())
}

fn physical_iphone(entry: &Value) -> Option<Device> {
    let text = |pointer: &str| entry.pointer(pointer).and_then(Value::as_str);
    text("/identifier")?;
    let udid = text("/hardwareProperties/udid")?.to_string();
    let name = text("/deviceProperties/name")?.to_string();
    let state = match (
        text("/connectionProperties/pairingState"),
        text("/connectionProperties/tunnelState"),
    ) {
        (Some("paired"), Some("connected")) => DeviceState::Running,
        (Some("paired"), Some(_)) => DeviceState::Unusable("Not connected".to_string()),
        (Some(_), Some(_)) => DeviceState::Unusable("Not paired".to_string()),
        _ => DeviceState::Unusable("Unknown state".to_string()),
    };
    let detail = text("/deviceProperties/osVersionNumber").map(|version| {
        let platform = text("/hardwareProperties/platform").unwrap_or("iOS");
        format!("{platform} {version}")
    });
    Some(Device {
        key: DeviceKey::IosDevice(udid.clone()),
        name,
        kind: DeviceKind::IosPhysical,
        state,
        detail,
        family: text("/hardwareProperties/deviceType").map(str::to_string),
        serial: None,
        run_id: Some(udid),
        experimental: true,
    })
}

/// devicectl prints a table to stdout and writes its JSON only to a file;
/// stderr carries harmless noise such as "No provider was found".
pub(crate) async fn read_physical_iphones(
    host: &dyn HostSystem,
    runner: &dyn ToolRunner,
) -> SectionState {
    // A folder of its own, open to this user only, so no other process can
    // plant the list devicectl is about to be read back from.
    let folder = match host.create_temp_dir("devicectl") {
        Ok(folder) => folder,
        Err(error) => {
            return SectionState::Failed {
                message: format!("could not make a folder for devicectl's output: {error}"),
            };
        }
    };
    let state = list_in_folder(host, runner, &folder.join("devicectl-devices.json")).await;
    // devicectl may have written the file before it failed or was stopped;
    // it holds device names and ids, so it never stays behind.
    if let Err(error) = host.remove_dir_all(&folder)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("could not remove {}: {error}", folder.display());
    }
    state
}

async fn list_in_folder(
    host: &dyn HostSystem,
    runner: &dyn ToolRunner,
    output_path: &Path,
) -> SectionState {
    let invocation = ToolInvocation::new(
        XCRUN,
        [
            "devicectl".to_string(),
            "list".to_string(),
            "devices".to_string(),
            "--json-output".to_string(),
            output_path.to_string_lossy().into_owned(),
            "-q".to_string(),
        ],
    )
    .with_timeout(DEVICECTL_TIMEOUT);
    let result = runner.run(invocation).await;
    let json = host.read_to_string_at_most(output_path, MAX_DEVICECTL_JSON_BYTES);
    let output = match result {
        Ok(output) => output,
        Err(failure) => {
            return SectionState::Failed {
                message: failure.to_string(),
            };
        }
    };
    if !output.success() {
        if output
            .stderr
            .contains("unable to find utility \"devicectl\"")
        {
            return SectionState::ToolMissing {
                tool: "devicectl".to_string(),
                hint: "Physical iPhones need Xcode 15 or later.".to_string(),
            };
        }
        return SectionState::Failed {
            message: format!("devicectl failed: {}", output.failure_message()),
        };
    }
    let json = match json {
        Ok(json) => json,
        Err(error) => {
            return SectionState::Failed {
                message: format!("devicectl wrote no device list: {error}"),
            };
        }
    };
    match parse_devicectl_devices(&json) {
        Ok(devices) => SectionState::Ready(devices),
        Err(error) => SectionState::Failed {
            message: format!("{error:#}"),
        },
    }
}
