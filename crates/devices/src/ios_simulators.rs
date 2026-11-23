use std::collections::{BTreeMap, HashMap};

use anyhow::Context as _;
use serde::Deserialize;

use crate::host::{ToolFailure, ToolInvocation, ToolOutput, ToolRunner};
use crate::{Device, DeviceKey, DeviceKind, DeviceState, SectionState};

pub(crate) const XCRUN: &str = "/usr/bin/xcrun";
pub(crate) const XCODE_SELECT: &str = "/usr/bin/xcode-select";

const IOS_RUNTIME_PREFIX: &str = "com.apple.CoreSimulator.SimRuntime.iOS-";
const CORE_SIMULATOR_DROPPED: &str = "CoreSimulatorService connection became invalid";

#[derive(Deserialize)]
struct DeviceList {
    devices: BTreeMap<String, Vec<SimulatorEntry>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SimulatorEntry {
    udid: String,
    name: String,
    state: String,
    is_available: Option<bool>,
    device_type_identifier: Option<String>,
}

#[derive(Deserialize)]
struct RuntimeList {
    runtimes: Vec<RuntimeEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeEntry {
    identifier: String,
    name: Option<String>,
    #[serde(default)]
    supported_device_types: Vec<DeviceTypeEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceTypeEntry {
    identifier: String,
    product_family: Option<String>,
}

#[cfg(test)]
pub(crate) fn parse_simulators(
    devices_json: &str,
    runtimes_json: &str,
) -> anyhow::Result<Vec<Device>> {
    let runtime_list: RuntimeList =
        serde_json::from_str(runtimes_json).context("could not read the runtime list")?;
    simulators_with_runtimes(devices_json, &runtime_list)
}

fn simulators_with_runtimes(
    devices_json: &str,
    runtime_list: &RuntimeList,
) -> anyhow::Result<Vec<Device>> {
    let device_list: DeviceList =
        serde_json::from_str(devices_json).context("could not read the simulator list")?;
    let runtime_names: HashMap<&str, &str> = runtime_list
        .runtimes
        .iter()
        .filter_map(|runtime| Some((runtime.identifier.as_str(), runtime.name.as_deref()?)))
        .collect();
    let families: HashMap<&str, &str> = runtime_list
        .runtimes
        .iter()
        .flat_map(|runtime| &runtime.supported_device_types)
        .filter_map(|device_type| {
            Some((
                device_type.identifier.as_str(),
                device_type.product_family.as_deref()?,
            ))
        })
        .collect();

    let mut devices = Vec::new();
    for (runtime, entries) in &device_list.devices {
        let Some(version) = runtime.strip_prefix(IOS_RUNTIME_PREFIX) else {
            continue;
        };
        let detail = runtime_names
            .get(runtime.as_str())
            .map(|name| name.to_string())
            .unwrap_or_else(|| format!("iOS {}", version.replace('-', ".")));
        for entry in entries
            .iter()
            .filter(|entry| entry.is_available != Some(false))
        {
            devices.push(Device {
                key: DeviceKey::Simulator(entry.udid.clone()),
                name: entry.name.clone(),
                kind: DeviceKind::IosSimulator,
                state: simulator_state(&entry.state),
                detail: Some(detail.clone()),
                family: entry
                    .device_type_identifier
                    .as_deref()
                    .and_then(|identifier| families.get(identifier))
                    .map(|family| family.to_string()),
                serial: None,
                run_id: Some(entry.udid.clone()),
                experimental: false,
            });
        }
    }
    Ok(devices)
}

/// simctl's states are not documented; one it adds later is treated as busy
/// when its name says so, so the panel waits instead of offering an action.
fn simulator_state(state: &str) -> DeviceState {
    match state {
        "Shutdown" => DeviceState::Shutdown,
        "Booted" => DeviceState::Running,
        "Booting" => DeviceState::Starting,
        "Shutting Down" => DeviceState::Stopping,
        other => {
            let lowercase = other.to_lowercase();
            if lowercase.contains("shut") || lowercase.contains("stop") {
                DeviceState::Stopping
            } else if ["boot", "start", "creat"]
                .iter()
                .any(|word| lowercase.contains(word))
            {
                DeviceState::Starting
            } else {
                DeviceState::Unusable(other.to_string())
            }
        }
    }
}

pub(crate) async fn check_xcode(runner: &dyn ToolRunner) -> Result<(), SectionState> {
    const INSTALL_XCODE: &str = "Install Xcode from the App Store and open it once.";
    let output = match runner.run(ToolInvocation::new(XCODE_SELECT, ["-p"])).await {
        Ok(output) => output,
        Err(ToolFailure::NotFound(_)) => return Err(xcode_missing(INSTALL_XCODE)),
        Err(failure) => {
            return Err(SectionState::Failed {
                message: failure.to_string(),
            });
        }
    };
    let developer_dir = output.stdout.trim();
    if !output.success() || developer_dir.is_empty() {
        return Err(xcode_missing(INSTALL_XCODE));
    }
    if developer_dir
        .trim_end_matches('/')
        .ends_with("/CommandLineTools")
    {
        return Err(xcode_missing(
            "Only the Command Line Tools are selected. Install Xcode, then run \
             `sudo xcode-select -s /Applications/Xcode.app`.",
        ));
    }
    Ok(())
}

fn xcode_missing(hint: &str) -> SectionState {
    SectionState::ToolMissing {
        tool: "Xcode".to_string(),
        hint: hint.to_string(),
    }
}

/// Runs a simctl command, once more if CoreSimulator dropped its connection,
/// which it does after Xcode updates until the service restarts.
pub(crate) async fn run_simctl(
    runner: &dyn ToolRunner,
    invocation: ToolInvocation,
) -> Result<ToolOutput, ToolFailure> {
    let output = runner.run(invocation.clone()).await?;
    if !output.success() && output.combined().contains(CORE_SIMULATOR_DROPPED) {
        return runner.run(invocation).await;
    }
    Ok(output)
}

pub(crate) async fn read_simulators(runner: &dyn ToolRunner) -> SectionState {
    let (devices, runtimes) = futures::join!(
        run_simctl(
            runner,
            ToolInvocation::new(XCRUN, ["simctl", "list", "devices", "available", "-j"]),
        ),
        run_simctl(
            runner,
            ToolInvocation::new(XCRUN, ["simctl", "list", "runtimes", "-j"]),
        ),
    );
    let (devices, runtimes) = match (successful_stdout(devices), successful_stdout(runtimes)) {
        (Ok(devices), Ok(runtimes)) => (devices, runtimes),
        (Err(message), _) | (_, Err(message)) => return SectionState::Failed { message },
    };
    let runtime_list: RuntimeList = match serde_json::from_str(&runtimes) {
        Ok(runtime_list) => runtime_list,
        Err(error) => {
            return SectionState::Failed {
                message: format!("could not read the runtime list: {error}"),
            };
        }
    };
    if !runtime_list
        .runtimes
        .iter()
        .any(|runtime| runtime.identifier.starts_with(IOS_RUNTIME_PREFIX))
    {
        return SectionState::ToolMissing {
            tool: "iOS Simulator runtime".to_string(),
            hint: "Install an iOS runtime in Xcode → Settings → Components.".to_string(),
        };
    }
    match simulators_with_runtimes(&devices, &runtime_list) {
        Ok(devices) => SectionState::Ready(devices),
        Err(error) => SectionState::Failed {
            message: format!("{error:#}"),
        },
    }
}

fn successful_stdout(result: Result<ToolOutput, ToolFailure>) -> Result<String, String> {
    match result {
        Ok(output) if output.success() => Ok(output.stdout),
        Ok(output) => Err(format!("simctl failed: {}", output.failure_message())),
        Err(failure) => Err(failure.to_string()),
    }
}
