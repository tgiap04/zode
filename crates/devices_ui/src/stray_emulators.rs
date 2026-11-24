//! An emulator that never showed up in adb within its time: Zode started it
//! and still holds the means to stop it, which nothing else can offer.

use devices::DeviceKey;
use gpui::{AppContext as _, Context, Task};
use util::ResultExt as _;

use crate::device_follow::{STILL_RUNNING_NOTE, StrayEmulator};
use crate::device_operations::DeviceError;
use crate::devices_panel::DevicesPanel;

impl DevicesPanel {
    /// Whether this AVD's earlier start is still running, which makes another
    /// start fail ("multiple emulators with the same AVD"). One that has since
    /// ended is forgotten, as its process id may now be someone else's.
    pub(crate) fn has_running_stray(&mut self, key: &DeviceKey) -> bool {
        let Some(stray) = self.stray_emulators.get_mut(key) else {
            return false;
        };
        if matches!(stray.exit.try_recv(), Ok(None)) {
            return true;
        }
        self.stray_emulators.remove(key);
        false
    }

    pub(crate) fn stop_stray_emulator(&mut self, key: &DeviceKey, cx: &mut Context<Self>) {
        if !self.has_running_stray(key) {
            // It ended by itself, so there is nothing left to report either.
            self.device_errors.remove(key);
            cx.notify();
            return;
        }
        let Some(mut stray) = self.stray_emulators.remove(key) else {
            return;
        };
        let host = self.host.clone();
        let runner = self.runner.clone();
        let key = key.clone();
        // The exit signal is read again on the background thread, right before
        // the kill, so an emulator that ends in between is not signalled by an
        // id that may be someone else's by then.
        let stopping: Task<(StrayEmulator, anyhow::Result<()>)> = cx.background_spawn(async move {
            let result = devices::stop_detached_emulator(
                &*host,
                &*runner,
                stray.process_id,
                &mut stray.exit,
            )
            .await;
            (stray, result)
        });
        // Bounded by the tool's own timeout, so it needs no owner.
        cx.spawn(async move |this, cx| {
            let (stray, result) = stopping.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.device_errors.remove(&key);
                    }
                    Err(error) => {
                        this.stray_emulators.insert(key.clone(), stray);
                        this.device_errors.insert(
                            key,
                            DeviceError::new(format!(
                                "Could not stop the emulator process: {error:#}"
                            )),
                        );
                    }
                }
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    pub(crate) fn remember_stray(&mut self, key: &DeviceKey, stray: Option<StrayEmulator>) {
        match stray {
            Some(stray) => {
                self.stray_emulators.insert(key.clone(), stray);
            }
            None => {
                self.stray_emulators.remove(key);
            }
        }
    }

    pub(crate) fn refuse_start_while_stray_runs(&mut self, key: &DeviceKey) {
        self.device_errors.insert(
            key.clone(),
            DeviceError::new(format!(
                "The earlier start of this emulator is still running.{STILL_RUNNING_NOTE}"
            )),
        );
    }
}
