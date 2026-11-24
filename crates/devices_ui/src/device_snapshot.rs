use std::collections::HashMap;

use devices::{
    Device, DeviceKey, DeviceSnapshot, DeviceState, MemoryStatus, SectionState, memory_status,
};
use gpui::SharedString;

use crate::devices_panel::DevicesPanel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SectionId {
    IosSimulators,
    AndroidEmulators,
    PhysicalIos,
    PhysicalAndroid,
}

impl DevicesPanel {
    pub(crate) fn all_devices(&self) -> impl Iterator<Item = &Device> {
        self.snapshot.iter().flat_map(|snapshot| {
            [
                &snapshot.ios_simulators,
                &snapshot.android_emulators,
                &snapshot.physical_ios,
                &snapshot.physical_android,
            ]
            .into_iter()
            .flat_map(|section| section.devices())
        })
    }

    pub(crate) fn find_device(&self, key: &DeviceKey) -> Option<&Device> {
        self.all_devices().find(|device| &device.key == key)
    }

    pub(crate) fn set_device_state(&mut self, key: &DeviceKey, state: DeviceState) {
        if let Some(device) = self.find_device_mut(key) {
            device.state = state;
        }
    }

    pub(crate) fn remove_device(&mut self, key: &DeviceKey) {
        let Some(snapshot) = self.snapshot.as_mut() else {
            return;
        };
        for section in [
            &mut snapshot.ios_simulators,
            &mut snapshot.android_emulators,
            &mut snapshot.physical_ios,
            &mut snapshot.physical_android,
        ] {
            if let SectionState::Ready(devices) = section {
                devices.retain(|device| &device.key != key);
            }
        }
    }

    pub(crate) fn replace_device(&mut self, replacement: Device) {
        if let Some(device) = self.find_device_mut(&replacement.key) {
            *device = replacement;
        }
    }

    fn find_device_mut(&mut self, key: &DeviceKey) -> Option<&mut Device> {
        let snapshot = self.snapshot.as_mut()?;
        [
            &mut snapshot.ios_simulators,
            &mut snapshot.android_emulators,
            &mut snapshot.physical_ios,
            &mut snapshot.physical_android,
        ]
        .into_iter()
        .find_map(|section| match section {
            SectionState::Ready(devices) => devices.iter_mut().find(|device| &device.key == key),
            _ => None,
        })
    }

    /// Recounts from the rows, and takes a fresh free-memory figure when the
    /// caller read one.
    pub(crate) fn recount_running(&mut self, available_bytes: Option<u64>) {
        let running_count = self
            .snapshot
            .iter()
            .flat_map(|snapshot| {
                [&snapshot.ios_simulators, &snapshot.android_emulators]
                    .into_iter()
                    .flat_map(|section| section.devices())
            })
            .filter(|device| matches!(device.state, DeviceState::Running | DeviceState::Starting))
            .count();
        if let Some(snapshot) = self.snapshot.as_mut() {
            let available_bytes = available_bytes.or(snapshot.memory.available_bytes);
            snapshot.memory = memory_status(available_bytes, running_count);
        }
    }
}

/// A section that failed to read keeps the list it had; the failure is shown
/// as a line of its own instead of emptying the panel.
pub(crate) fn merge_snapshot(
    old: Option<&DeviceSnapshot>,
    new: DeviceSnapshot,
) -> (DeviceSnapshot, HashMap<SectionId, SharedString>) {
    let mut errors = HashMap::new();
    let mut keep = |id: SectionId, old: Option<&SectionState>, new: SectionState| match (old, new) {
        (Some(old @ SectionState::Ready(_)), SectionState::Failed { message }) => {
            errors.insert(id, SharedString::from(message));
            old.clone()
        }
        (_, new) => new,
    };
    let merged = DeviceSnapshot {
        ios_simulators: keep(
            SectionId::IosSimulators,
            old.map(|old| &old.ios_simulators),
            new.ios_simulators,
        ),
        android_emulators: keep(
            SectionId::AndroidEmulators,
            old.map(|old| &old.android_emulators),
            new.android_emulators,
        ),
        physical_ios: keep(
            SectionId::PhysicalIos,
            old.map(|old| &old.physical_ios),
            new.physical_ios,
        ),
        physical_android: keep(
            SectionId::PhysicalAndroid,
            old.map(|old| &old.physical_android),
            new.physical_android,
        ),
        memory: new.memory,
    };
    (merged, errors)
}

/// The text of the memory line and whether it is a warning.
pub(crate) fn memory_line(memory: &MemoryStatus) -> (String, bool) {
    let text = match memory.available_bytes {
        Some(bytes) => format!(
            "{} running · {:.1} GB free",
            memory.running_count,
            bytes as f64 / (1024.0 * 1024.0 * 1024.0)
        ),
        None => format!("{} running", memory.running_count),
    };
    (text, memory.low)
}
