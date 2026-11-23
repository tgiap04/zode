//! Finding, starting and stopping iOS simulators, Android emulators and
//! physical devices through their own command-line tools. No UI: every tool
//! runs behind [`ToolRunner`] and [`HostSystem`], so the whole crate can be
//! tested against scripted output.

mod actions;
mod android_devices;
mod android_sdk;
mod detached_spawn;
#[cfg(any(test, feature = "test-support"))]
mod fake_host;
mod host;
mod input_validation;
mod ios_devices;
mod ios_simulators;
mod local_host;
mod resources;
mod run_app;
mod run_app_cli_frameworks;
mod run_app_detect;
mod run_app_native;
mod run_app_xcode;
mod tracking;

#[cfg(test)]
mod actions_tests;
#[cfg(test)]
mod android_devices_tests;
#[cfg(test)]
mod android_sdk_tests;
#[cfg(test)]
mod discovery_tests;
#[cfg(test)]
mod input_validation_tests;
#[cfg(test)]
mod ios_devices_tests;
#[cfg(test)]
mod ios_simulators_tests;
#[cfg(test)]
mod local_host_tests;
#[cfg(test)]
mod resources_tests;
#[cfg(test)]
mod run_app_detect_tests;
#[cfg(test)]
mod run_app_native_tests;
#[cfg(test)]
mod run_app_tests;
#[cfg(test)]
mod test_fixtures;
#[cfg(test)]
mod tracking_tests;

pub use actions::{
    StartedEmulator, adb_connect, adb_disconnect, adb_pair, boot_simulator, open_simulator_window,
    shutdown_simulator, start_emulator, stop_detached_emulator, stop_emulator,
};
pub use android_devices::{
    AdbEntry, AdbState, NO_PERMISSIONS_REASON, OFFLINE_REASON, UNAUTHORIZED_REASON,
    parse_adb_devices, parse_avd_list,
};
pub use android_sdk::{AndroidSdk, locate_android_sdk};
#[cfg(any(test, feature = "test-support"))]
pub use fake_host::{FIRST_PROCESS_ID, FakeHost};
pub use host::{
    DEFAULT_TOOL_TIMEOUT, DetachedExit, DetachedTool, HostOs, HostSystem, ToolFailure,
    ToolInvocation, ToolOutput, ToolRunner,
};
pub use input_validation::{
    InvalidInput, PairingCode, WirelessAddress, parse_pairing_code, parse_wireless_address,
    validate_app_path, validate_argument, validate_avd_name, validate_identifier, validate_serial,
    validate_udid, validate_xcode_name,
};
pub use local_host::LocalHost;
pub use resources::{
    EMULATOR_LOG_MAX_AGE, EMULATOR_LOGS_KEPT, EMULATOR_LOGS_TOTAL_BYTES,
    LOW_MEMORY_THRESHOLD_BYTES, explain_emulator_failure, memory_status, prune_emulator_logs,
    prune_stale_emulator_logs, read_emulator_log_tail,
};
pub use run_app::{
    AppProject, Framework, NativeProject, OpenInstalledAndroidApp, PackageVersion, PlannedRun,
    RunConfirmation, RunPlan, RunRefusal, RunStep, TargetPlatform, XcodeContainer, plan_metro,
    plan_run,
};
pub use run_app_cli_frameworks::react_native_android_device_flag;
pub use run_app_detect::{
    MAX_PROJECT_FILE_BYTES, detect_app_projects, detect_app_projects_excluding,
};
pub use run_app_native::plan_app_run;
pub use tracking::{ProbeResult, TrackTarget, TrackingVerdict, probe, tracking_verdict};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeviceKey {
    Simulator(String),
    Avd(String),
    AndroidSerial(String),
    IosDevice(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    IosSimulator,
    AndroidEmulator,
    IosPhysical,
    AndroidUsb,
    AndroidWireless,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceState {
    Shutdown,
    Starting,
    Running,
    Stopping,
    Unusable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub key: DeviceKey,
    pub name: String,
    pub kind: DeviceKind,
    pub state: DeviceState,
    /// The operating system shown next to the name, such as "iOS 26.4".
    pub detail: Option<String>,
    /// "iPhone", "iPad", when the tool says.
    pub family: Option<String>,
    /// The adb serial of a running emulator or a connected Android device.
    pub serial: Option<String>,
    /// The id a framework CLI takes to pick this device: a simulator's UDID,
    /// an adb serial, or a physical iPhone's hardware UDID.
    pub run_id: Option<String>,
    /// Physical iPhones are read from `devicectl`, whose JSON is undocumented.
    pub experimental: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SectionState {
    Ready(Vec<Device>),
    ToolMissing { tool: String, hint: String },
    Unsupported { reason: String },
    Failed { message: String },
}

impl SectionState {
    pub fn devices(&self) -> &[Device] {
        match self {
            Self::Ready(devices) => devices,
            Self::ToolMissing { .. } | Self::Unsupported { .. } | Self::Failed { .. } => &[],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryStatus {
    pub running_count: usize,
    pub available_bytes: Option<u64>,
    pub low: bool,
}

/// One full read. Physical devices come from two tools that fail separately,
/// so they keep two states even though the panel shows them as one group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceSnapshot {
    pub ios_simulators: SectionState,
    pub android_emulators: SectionState,
    pub physical_ios: SectionState,
    pub physical_android: SectionState,
    pub memory: MemoryStatus,
}

/// Reads every section once. The sections fail independently: a broken
/// simctl leaves the Android sections intact, and the other way round.
pub async fn discover(host: &dyn HostSystem, runner: &dyn ToolRunner) -> DeviceSnapshot {
    let ((ios_simulators, physical_ios), (android_emulators, physical_android)) =
        futures::join!(discover_ios(host, runner), discover_android(host, runner));
    let running_count = ios_simulators
        .devices()
        .iter()
        .chain(android_emulators.devices())
        .filter(|device| matches!(device.state, DeviceState::Running | DeviceState::Starting))
        .count();
    DeviceSnapshot {
        ios_simulators,
        android_emulators,
        physical_ios,
        physical_android,
        memory: memory_status(host.available_memory_bytes(), running_count),
    }
}

async fn discover_ios(
    host: &dyn HostSystem,
    runner: &dyn ToolRunner,
) -> (SectionState, SectionState) {
    if host.os() != HostOs::MacOs {
        let unsupported = SectionState::Unsupported {
            reason: "iOS devices need a Mac with Xcode.".to_string(),
        };
        return (unsupported.clone(), unsupported);
    }
    if let Err(state) = ios_simulators::check_xcode(runner).await {
        return (state.clone(), state);
    }
    futures::join!(
        ios_simulators::read_simulators(runner),
        ios_devices::read_physical_iphones(host, runner)
    )
}

async fn discover_android(
    host: &dyn HostSystem,
    runner: &dyn ToolRunner,
) -> (SectionState, SectionState) {
    let Some(sdk) = locate_android_sdk(host) else {
        let missing = SectionState::ToolMissing {
            tool: "Android SDK".to_string(),
            hint: "Install Android Studio, or set ANDROID_HOME to your SDK.".to_string(),
        };
        return (missing.clone(), missing);
    };
    let read_avds = async {
        match &sdk.emulator {
            Some(emulator) => Some(android_devices::read_avds(runner, emulator).await),
            None => None,
        }
    };
    let (entries, avds) = futures::join!(
        android_devices::read_adb_devices(runner, &sdk.adb),
        read_avds
    );
    let entries = match entries {
        Ok(entries) => entries,
        Err(message) => {
            let failed = SectionState::Failed { message };
            return (failed.clone(), failed);
        }
    };
    let physical = SectionState::Ready(android_devices::physical_devices(&entries));
    let emulators = match avds {
        None => SectionState::ToolMissing {
            tool: "Android Emulator".to_string(),
            hint: "Install \"Android Emulator\" from the Android Studio SDK Manager.".to_string(),
        },
        Some(Err(message)) => SectionState::Failed { message },
        Some(Ok(avds)) => {
            let emulator_serials = android_devices::emulator_serials(&entries);
            let names =
                android_devices::emulator_avd_names(runner, &sdk.adb, &emulator_serials).await;
            SectionState::Ready(android_devices::emulator_rows(&avds, &entries, &names))
        }
    };
    (emulators, physical)
}
