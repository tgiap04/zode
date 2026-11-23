//! The panel's side of Boot, Shutdown and Show window: take the click, show
//! the row as busy at once, and apply the single outcome when it arrives. Also
//! the two adb commands of the wireless dialog.

use std::path::PathBuf;

use anyhow::Context as _;
use devices::{
    AndroidSdk, Device, DeviceKey, DeviceKind, DeviceState, HostSystem, PairingCode,
    WirelessAddress,
};
use gpui::{AppContext as _, Context, Task, Window};
use util::ResultExt as _;
use workspace::OpenOptions;

use crate::device_follow::{OperationContext, execute};
use crate::device_operations::{
    DeviceError, OperationKind, OperationOutcome, Settled, plan_for, shorten_home,
};
use crate::devices_panel::{DevicesPanel, Operation};

/// Whether a row offers this action at all. Physical devices never do: Zode
/// cannot turn a phone on or off. Nor is anything offered that
/// [`plan_for`] cannot carry out, such as booting an emulator known only by
/// its serial.
pub(crate) fn offers(kind: &OperationKind, device: &Device) -> bool {
    let virtual_device = matches!(
        device.kind,
        DeviceKind::IosSimulator | DeviceKind::AndroidEmulator
    );
    let offered = match kind {
        OperationKind::Boot => virtual_device && device.state == DeviceState::Shutdown,
        OperationKind::Shutdown => virtual_device && device.state == DeviceState::Running,
        OperationKind::ShowWindow => {
            device.kind == DeviceKind::IosSimulator && device.state == DeviceState::Running
        }
    };
    offered && plan_for(kind, device, Vec::new()).is_some()
}

fn locate_sdk(host: &dyn HostSystem) -> anyhow::Result<AndroidSdk> {
    devices::locate_android_sdk(host).context(
        "The Android SDK was not found. Install Android Studio or set ANDROID_HOME to your SDK.",
    )
}

impl DevicesPanel {
    /// The caller awaits the returned task without holding the panel, so a slow
    /// adb never blocks it. The task owns everything it needs.
    pub(crate) fn pair(
        &self,
        address: WirelessAddress,
        code: PairingCode,
        cx: &mut Context<Self>,
    ) -> Task<anyhow::Result<()>> {
        let host = self.host.clone();
        let runner = self.runner.clone();
        cx.background_spawn(async move {
            let sdk = locate_sdk(&*host)?;
            devices::adb_pair(&*runner, &sdk, &address, &code).await
        })
    }

    pub(crate) fn connect(
        &self,
        address: WirelessAddress,
        cx: &mut Context<Self>,
    ) -> Task<anyhow::Result<()>> {
        let host = self.host.clone();
        let runner = self.runner.clone();
        cx.background_spawn(async move {
            let sdk = locate_sdk(&*host)?;
            devices::adb_connect(&*runner, &sdk, &address).await
        })
    }

    pub(crate) fn boot_device(&mut self, key: &DeviceKey, cx: &mut Context<Self>) {
        self.begin(key, OperationKind::Boot, cx);
    }

    pub(crate) fn shutdown_device(&mut self, key: &DeviceKey, cx: &mut Context<Self>) {
        self.begin(key, OperationKind::Shutdown, cx);
    }

    pub(crate) fn show_simulator_window(&mut self, key: &DeviceKey, cx: &mut Context<Self>) {
        self.begin(key, OperationKind::ShowWindow, cx);
    }

    fn begin(&mut self, key: &DeviceKey, kind: OperationKind, cx: &mut Context<Self>) {
        if !self.is_supported(cx) || self.operations.contains_key(key) {
            return;
        }
        let Some(device) = self.find_device(key).filter(|device| offers(&kind, device)) else {
            return;
        };
        let device = device.clone();
        let emulator_serials = self
            .all_devices()
            .filter(|other| other.kind == DeviceKind::AndroidEmulator)
            .filter_map(|other| other.serial.clone())
            .collect();
        let Some(plan) = plan_for(&kind, &device, emulator_serials) else {
            return;
        };
        let context = OperationContext {
            host: self.host.clone(),
            runner: self.runner.clone(),
            log_dir: self.log_dir.clone(),
            executor: cx.background_executor().clone(),
        };
        self.device_errors.remove(key);
        let task = cx.spawn({
            let key = key.clone();
            let kind = kind.clone();
            async move |this, cx| {
                let (outcome, available_bytes) = cx.background_spawn(execute(context, plan)).await;
                this.update(cx, |this, cx| {
                    this.finish(&key, &kind, outcome, available_bytes, cx)
                })
                .log_err();
            }
        });
        let operation = Operation {
            kind,
            previous: device.state,
            waiters: Vec::new(),
            _task: task,
        };
        let shown_state = operation.shown_state();
        self.operations.insert(key.clone(), operation);
        if let Some(state) = shown_state {
            self.set_device_state(key, state);
            self.recount_running(None);
        }
        cx.notify();
    }

    fn finish(
        &mut self,
        key: &DeviceKey,
        kind: &OperationKind,
        outcome: OperationOutcome,
        available_bytes: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let Some(operation) = self.operations.remove(key) else {
            return;
        };
        if *kind != OperationKind::ShowWindow {
            match outcome.settled {
                // An emulator stopped by its serial comes back under its
                // serial's name, not under the row's key.
                Settled::Done(Some(device)) | Settled::Failed(Some(device))
                    if device.key != *key =>
                {
                    self.set_device_state(key, device.state)
                }
                Settled::Done(Some(device)) | Settled::Failed(Some(device)) => {
                    self.replace_device(device)
                }
                Settled::Done(None) => self.set_device_state(key, DeviceState::Shutdown),
                Settled::Failed(None) => self.set_device_state(key, operation.previous),
            }
            // An emulator known only by its serial has a row only while it
            // runs: once stopped there is nothing left to show or start.
            let stopped_unnamed_emulator = matches!(key, DeviceKey::AndroidSerial(_))
                && *kind == OperationKind::Shutdown
                && self
                    .find_device(key)
                    .is_some_and(|device| device.state == DeviceState::Shutdown);
            if stopped_unnamed_emulator {
                self.remove_device(key);
            }
            self.recount_running(available_bytes);
        }
        let ready = self
            .find_device(key)
            .is_some_and(|device| device.state == DeviceState::Running);
        for waiter in operation.waiters {
            waiter.send(ready).ok();
        }
        if let Some(error) = outcome.error {
            let home = self.host.home_dir();
            self.device_errors.insert(
                key.clone(),
                DeviceError {
                    message: shorten_home(&error.message, home.as_deref()).into(),
                    log_file: error.log_file,
                },
            );
        }
        cx.notify();
    }

    /// Only a file inside Zode's own device log folder is opened.
    pub(crate) fn open_log(&self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if !path.starts_with(&self.log_dir) {
            return;
        }
        self.workspace
            .update(cx, |workspace, cx| {
                workspace
                    .open_abs_path(path, OpenOptions::default(), window, cx)
                    .detach_and_log_err(cx);
            })
            .log_err();
    }
}
