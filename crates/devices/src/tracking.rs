//! Following one device after an action until it settles. There is no
//! periodic polling anywhere: the caller probes the one device it acted on,
//! asks for a verdict, and stops at `Done` or `TimedOut`.

use std::time::{Duration, Instant};

use crate::android_devices::{
    AdbState, avd_device, emulator_avd_names, emulator_serials, read_adb_devices,
};
use crate::android_sdk::locate_android_sdk;
use crate::host::{HostSystem, ToolInvocation, ToolRunner};
use crate::ios_simulators::{XCRUN, read_simulator};
use crate::{Device, DeviceState};

const SIMULATOR_TIME_LIMIT: Duration = Duration::from_secs(120);
const EMULATOR_TIME_LIMIT: Duration = Duration::from_secs(180);
/// A cold boot spent about 9 s in `bootstatus` after simctl already said
/// Booted; the margin covers slower machines without stalling one probe.
const BOOTSTATUS_TIMEOUT: Duration = Duration::from_secs(30);
const MIN_BOOTSTATUS_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrackTarget {
    SimulatorBoot {
        udid: String,
    },
    SimulatorShutdown {
        udid: String,
    },
    EmulatorStart {
        avd_name: String,
        /// Emulator serials already running before the start, so a new one
        /// that cannot name its AVD can still be recognised.
        serials_before: Vec<String>,
    },
    EmulatorStop {
        avd_name: String,
        serial: String,
    },
}

impl TrackTarget {
    pub fn time_limit(&self) -> Duration {
        match self {
            Self::SimulatorBoot { .. } | Self::SimulatorShutdown { .. } => SIMULATOR_TIME_LIMIT,
            Self::EmulatorStart { .. } | Self::EmulatorStop { .. } => EMULATOR_TIME_LIMIT,
        }
    }

    fn description(&self) -> String {
        match self {
            Self::SimulatorBoot { .. } => "The simulator did not finish booting".to_string(),
            Self::SimulatorShutdown { .. } => "The simulator did not shut down".to_string(),
            Self::EmulatorStart { avd_name, .. } => {
                format!("The emulator {avd_name} did not finish booting")
            }
            Self::EmulatorStop { avd_name, .. } => format!("The emulator {avd_name} did not stop"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProbeResult {
    /// The device as seen now; `None` when it is not listed at all.
    pub device: Option<Device>,
    /// The extra readiness signal after a start: `bootstatus` finished, or
    /// `sys.boot_completed` is 1.
    pub ready: bool,
    /// Why the probe could not look. The verdict keeps waiting until the
    /// deadline, then reports it.
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrackingVerdict {
    Continue,
    /// `None` when a stopped device is no longer listed.
    Done(Option<Device>),
    TimedOut(String),
}

pub async fn probe(
    host: &dyn HostSystem,
    runner: &dyn ToolRunner,
    target: &TrackTarget,
    time_left: Duration,
) -> ProbeResult {
    match target {
        TrackTarget::SimulatorBoot { udid } => probe_simulator(runner, udid, Some(time_left)).await,
        TrackTarget::SimulatorShutdown { udid } => probe_simulator(runner, udid, None).await,
        TrackTarget::EmulatorStart {
            avd_name,
            serials_before,
        } => probe_emulator_start(host, runner, avd_name, serials_before).await,
        TrackTarget::EmulatorStop { avd_name, serial } => {
            probe_emulator_stop(host, runner, avd_name, serial).await
        }
    }
}

fn probe_error(message: impl Into<String>) -> ProbeResult {
    ProbeResult {
        error: Some(message.into()),
        ..ProbeResult::default()
    }
}

/// `wait_for_boot` is the time left to the operation when a boot is followed.
async fn probe_simulator(
    runner: &dyn ToolRunner,
    udid: &str,
    wait_for_boot: Option<Duration>,
) -> ProbeResult {
    let device = match read_simulator(runner, udid).await {
        Ok(device) => device,
        Err(message) => return probe_error(message),
    };
    // Only a simulator that is already Booted is asked, since `-b` would boot
    // one that the user shut down in the meantime.
    let booted = device
        .as_ref()
        .is_some_and(|device| device.state == DeviceState::Running);
    let ready = match wait_for_boot {
        Some(time_left) if booted => {
            // Never past the deadline of the whole operation, but long enough
            // to hear an answer that is just about to come.
            let timeout = time_left.clamp(MIN_BOOTSTATUS_TIMEOUT, BOOTSTATUS_TIMEOUT);
            runner
                .run(
                    ToolInvocation::new(XCRUN, ["simctl", "bootstatus", udid, "-b"])
                        .with_timeout(timeout),
                )
                .await
                .is_ok_and(|output| output.success())
        }
        _ => false,
    };
    ProbeResult {
        device,
        ready,
        error: None,
    }
}

async fn probe_emulator_start(
    host: &dyn HostSystem,
    runner: &dyn ToolRunner,
    avd_name: &str,
    serials_before: &[String],
) -> ProbeResult {
    let Some(sdk) = locate_android_sdk(host) else {
        return probe_error("The Android SDK was not found");
    };
    let entries = match read_adb_devices(runner, &sdk.adb).await {
        Ok(entries) => entries,
        Err(message) => return probe_error(message),
    };
    let emulator_serials = emulator_serials(&entries);
    let names = emulator_avd_names(runner, &sdk.adb, &emulator_serials).await;
    let by_name = names
        .iter()
        .find(|(_, name)| name.as_str() == avd_name)
        .map(|(serial, _)| serial.clone());
    let serial = by_name.or_else(|| {
        let mut unnamed_new = emulator_serials
            .iter()
            .filter(|serial| !serials_before.contains(serial) && !names.contains_key(*serial));
        match (unnamed_new.next(), unnamed_new.next()) {
            (Some(only), None) => Some(only.clone()),
            _ => None,
        }
    });
    let Some(entry) = serial.and_then(|serial| entries.iter().find(|entry| entry.serial == serial))
    else {
        return ProbeResult::default();
    };

    let ready = entry.state == AdbState::Device
        && runner
            .run(ToolInvocation::new(
                &sdk.adb,
                [
                    "-s",
                    entry.serial.as_str(),
                    "shell",
                    "getprop",
                    "sys.boot_completed",
                ],
            ))
            .await
            .is_ok_and(|output| output.success() && output.stdout.trim() == "1");
    ProbeResult {
        device: Some(avd_device(avd_name, Some((&entry.serial, &entry.state)))),
        ready,
        error: None,
    }
}

async fn probe_emulator_stop(
    host: &dyn HostSystem,
    runner: &dyn ToolRunner,
    avd_name: &str,
    serial: &str,
) -> ProbeResult {
    let Some(sdk) = locate_android_sdk(host) else {
        return probe_error("The Android SDK was not found");
    };
    match read_adb_devices(runner, &sdk.adb).await {
        Ok(entries) => {
            let running = entries
                .iter()
                .find(|entry| entry.serial == serial)
                .map(|entry| (entry.serial.as_str(), &entry.state));
            ProbeResult {
                device: Some(avd_device(avd_name, running)),
                ready: false,
                error: None,
            }
        }
        Err(message) => probe_error(message),
    }
}

/// Pure, with the clock passed in. An answer that arrives at the deadline
/// still counts; waiting stops there.
pub fn tracking_verdict(
    target: &TrackTarget,
    probe: &ProbeResult,
    now: Instant,
    deadline: Instant,
) -> TrackingVerdict {
    let state = probe.device.as_ref().map(|device| &device.state);
    let done = match target {
        TrackTarget::SimulatorBoot { .. } | TrackTarget::EmulatorStart { .. } => {
            state == Some(&DeviceState::Running) && probe.ready
        }
        TrackTarget::SimulatorShutdown { .. } | TrackTarget::EmulatorStop { .. } => {
            probe.error.is_none() && matches!(state, None | Some(DeviceState::Shutdown))
        }
    };
    if done {
        return TrackingVerdict::Done(probe.device.clone());
    }
    if now < deadline {
        return TrackingVerdict::Continue;
    }
    let mut reason = format!(
        "{} within {} s.",
        target.description(),
        target.time_limit().as_secs()
    );
    if let Some(error) = &probe.error {
        reason.push(' ');
        reason.push_str(error);
    }
    TrackingVerdict::TimedOut(reason)
}
