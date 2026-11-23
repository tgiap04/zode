//! The words the panel and the background work share: what is asked of a
//! device, what came of it, and which command each ask is.

use std::path::{Path, PathBuf};

use devices::{Device, DeviceKey, DeviceKind};
use gpui::SharedString;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OperationKind {
    Boot,
    Shutdown,
    ShowWindow,
}

/// A fault on one device, shown right under its row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeviceError {
    pub message: SharedString,
    /// The emulator's own log of the start that failed.
    pub log_file: Option<PathBuf>,
}

impl DeviceError {
    pub(crate) fn new(message: impl Into<SharedString>) -> Self {
        Self {
            message: message.into(),
            log_file: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Settled {
    /// `None` when a shut-down simulator is no longer listed at all.
    Done(Option<Device>),
    /// The last time the device was seen, when it was seen.
    Failed(Option<Device>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OperationOutcome {
    pub settled: Settled,
    /// May accompany `Done`: a window that would not open does not undo a boot.
    pub error: Option<DeviceError>,
}

impl OperationOutcome {
    pub(crate) fn failed(error: DeviceError, last_seen: Option<Device>) -> Self {
        Self {
            settled: Settled::Failed(last_seen),
            error: Some(error),
        }
    }
}

/// What to do to which device. Built from the row while the panel is still
/// being updated, so the background work never reads the panel.
#[derive(Clone, Debug)]
pub(crate) enum OperationPlan {
    BootSimulator {
        udid: String,
    },
    ShutdownSimulator {
        udid: String,
    },
    ShowSimulatorWindow {
        udid: String,
    },
    StartEmulator {
        avd_name: String,
        serials_before: Vec<String>,
    },
    StopEmulator {
        avd_name: String,
        serial: String,
    },
}

pub(crate) fn plan_for(
    kind: &OperationKind,
    device: &Device,
    emulator_serials: Vec<String>,
) -> Option<OperationPlan> {
    match (kind, &device.key) {
        (OperationKind::Boot, DeviceKey::Simulator(udid)) => {
            Some(OperationPlan::BootSimulator { udid: udid.clone() })
        }
        (OperationKind::Shutdown, DeviceKey::Simulator(udid)) => {
            Some(OperationPlan::ShutdownSimulator { udid: udid.clone() })
        }
        (OperationKind::ShowWindow, DeviceKey::Simulator(udid)) => {
            Some(OperationPlan::ShowSimulatorWindow { udid: udid.clone() })
        }
        (OperationKind::Boot, DeviceKey::Avd(avd_name)) => Some(OperationPlan::StartEmulator {
            avd_name: avd_name.clone(),
            serials_before: emulator_serials,
        }),
        (OperationKind::Shutdown, DeviceKey::Avd(avd_name)) => Some(OperationPlan::StopEmulator {
            avd_name: avd_name.clone(),
            serial: device.serial.clone()?,
        }),
        // A running emulator whose AVD could not be learned is stopped by its
        // serial, which also names it in messages.
        (OperationKind::Shutdown, DeviceKey::AndroidSerial(serial))
            if device.kind == DeviceKind::AndroidEmulator =>
        {
            Some(OperationPlan::StopEmulator {
                avd_name: serial.clone(),
                serial: serial.clone(),
            })
        }
        _ => None,
    }
}

/// Replaces the home directory with `~` so a message pasted into a bug report
/// does not carry the user's name.
pub(crate) fn shorten_home(message: &str, home: Option<&Path>) -> String {
    match home.and_then(Path::to_str).filter(|home| !home.is_empty()) {
        Some(home) => message.replace(home, "~"),
        None => message.to_string(),
    }
}
