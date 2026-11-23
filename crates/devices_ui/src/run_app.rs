//! Run app: the click on a device row's Run button, up to the terminal task
//! that runs the framework's own CLI. Every run runs the project's own code,
//! so nothing of the project is read or run before the worktree trust check.

use std::path::PathBuf;
use std::rc::Rc;

use devices::{AppProject, Device, DeviceKey, DeviceState, Framework, TargetPlatform};
use gpui::{App, Context, Task, Window};
use util::ResultExt as _;

use crate::device_actions::offers;
use crate::device_operations::{DeviceError, OperationKind, shorten_home};
use crate::devices_panel::DevicesPanel;
use crate::run_app_start::start;
use crate::run_terminals::RunTerminal;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RunTarget {
    Device(DeviceKey),
    Metro,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RunKey {
    pub app_directory: PathBuf,
    pub target: RunTarget,
}

pub(crate) struct ActiveRun {
    pub device_name: String,
    /// Expo and React Native: no other device may run this project meanwhile.
    pub exclusive: bool,
    pub terminal: Rc<dyn RunTerminal>,
}

/// A click being carried out: waiting for the device, asking, spawning.
pub(crate) struct StartingRun {
    pub device_name: String,
    pub exclusive: bool,
    pub _task: Task<()>,
}

/// Several apps fit the device, or an Xcode project has several schemes:
/// the row asks which, on every click.
pub(crate) struct AppChoice {
    pub device: DeviceKey,
    pub choices: Vec<RunChoice>,
}

pub(crate) struct RunChoice {
    pub app: AppProject,
    pub scheme: Option<String>,
}

pub(crate) const NOT_LOCAL: &str = "Run app works with projects on this computer only.";

fn fits(app: &AppProject, device: &Device) -> bool {
    let has_folder = match TargetPlatform::of(device.kind) {
        TargetPlatform::Ios => app.has_ios_folder,
        TargetPlatform::Android => app.has_android_folder,
    };
    // Expo creates the missing folder itself, after asking.
    has_folder || app.framework == Framework::Expo
}

fn is_exclusive(app: &AppProject) -> bool {
    matches!(app.framework, Framework::Expo | Framework::ReactNative)
}

impl DevicesPanel {
    pub(crate) fn compatible_apps(&self, device: &Device) -> Vec<AppProject> {
        self.detected_apps
            .iter()
            .filter(|app| fits(app, device))
            .cloned()
            .collect()
    }

    /// A physical device cannot be started from here; a stopped simulator or
    /// emulator is booted first. A device starting outside Zode has no boot
    /// to wait on, so it offers Run once it is up.
    pub(crate) fn offers_run(&self, device: &Device, cx: &App) -> bool {
        let reachable = device.state == DeviceState::Running
            || self.is_booting_here(device)
            || offers(&OperationKind::Boot, device);
        reachable && self.is_supported(cx) && !self.compatible_apps(device).is_empty()
    }

    fn is_booting_here(&self, device: &Device) -> bool {
        device.state == DeviceState::Starting
            && self
                .operations
                .get(&device.key)
                .is_some_and(|operation| operation.kind == OperationKind::Boot)
    }

    pub(crate) fn is_run_starting(&self, device: &DeviceKey) -> bool {
        self.starting_runs
            .keys()
            .any(|key| key.target == RunTarget::Device(device.clone()))
    }
    /// Why this project may not run on `device` now, when another device runs
    /// or is starting the same Expo or React Native app.
    pub(crate) fn exclusive_conflict(
        &self,
        app: &AppProject,
        device: &DeviceKey,
    ) -> Option<String> {
        if !is_exclusive(app) {
            return None;
        }
        let elsewhere = |key: &RunKey| {
            key.app_directory == app.directory
                && matches!(&key.target, RunTarget::Device(other) if other != device)
        };
        let running = self
            .active_runs
            .iter()
            .filter(|(key, run)| run.exclusive && elsewhere(key))
            .map(|(_, run)| run.device_name.as_str());
        let starting = self
            .starting_runs
            .iter()
            .filter(|(key, run)| run.exclusive && elsewhere(key))
            .map(|(_, run)| run.device_name.as_str());
        running.chain(starting).next().map(|device_name| {
            format!(
                "This project is running on {device_name}. Stop it there first: Expo and \
                 React Native run on one device per project. Zode checks this in this window \
                 only."
            )
        })
    }

    /// Forgets the runs whose task ended or whose terminal was closed.
    pub(crate) fn prune_finished_runs(&mut self, cx: &App) {
        self.active_runs
            .retain(|_, run| run.terminal.is_running(cx));
    }

    pub(crate) fn show_run_error(&mut self, device: &DeviceKey, message: impl Into<String>) {
        let home = self.host.home_dir();
        let message = shorten_home(&message.into(), home.as_deref());
        self.device_errors
            .insert(device.clone(), DeviceError::new(message));
    }

    pub(crate) fn run_clicked(
        &mut self,
        device: &DeviceKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.app_choice = None;
        cx.notify();
        if !self.is_supported(cx) {
            self.show_run_error(device, NOT_LOCAL);
            return;
        }
        let Some(found) = self.find_device(device).cloned() else {
            return;
        };
        let mut apps = self.compatible_apps(&found);
        match apps.len() {
            0 => self.show_run_error(device, "No app in the open folders can run on this device."),
            1 => {
                let app = apps.remove(0);
                self.run_app(device, &app, window, cx);
            }
            _ => {
                self.app_choice = Some(AppChoice {
                    device: device.clone(),
                    choices: apps
                        .into_iter()
                        .map(|app| RunChoice { app, scheme: None })
                        .collect(),
                })
            }
        }
    }

    pub(crate) fn choose_app(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(choice) = self.app_choice.take() else {
            return;
        };
        if let Some(chosen) = choice.choices.get(index) {
            self.start_run(
                &choice.device,
                &chosen.app,
                chosen.scheme.clone(),
                window,
                cx,
            );
        }
        cx.notify();
    }

    pub(crate) fn cancel_app_choice(&mut self, cx: &mut Context<Self>) {
        self.app_choice = None;
        cx.notify();
    }

    pub(crate) fn run_app(
        &mut self,
        device: &DeviceKey,
        app: &AppProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_run(device, app, None, window, cx);
    }

    /// `scheme` is the Xcode scheme picked from the row's list.
    pub(crate) fn start_run(
        &mut self,
        device: &DeviceKey,
        app: &AppProject,
        scheme: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_supported(cx) {
            self.show_run_error(device, NOT_LOCAL);
            cx.notify();
            return;
        }
        if let Err(message) = self.ensure_trusted(&app.directory, cx) {
            self.show_run_error(device, message.to_string());
            cx.notify();
            return;
        }
        self.prune_finished_runs(cx);
        let key = RunKey {
            app_directory: app.directory.clone(),
            target: RunTarget::Device(device.clone()),
        };
        if self.starting_runs.contains_key(&key) {
            return;
        }
        if let Some(run) = self.active_runs.get(&key) {
            // Revealing may activate the dock this panel sits in, which must
            // not happen while the panel is being updated.
            let terminal = run.terminal.clone();
            window.defer(cx, move |window, cx| terminal.reveal(window, cx));
            return;
        }
        if let Some(message) = self.exclusive_conflict(app, device) {
            self.show_run_error(device, message);
            cx.notify();
            return;
        }
        let Some(found) = self.find_device(device).cloned() else {
            return;
        };
        if found.state == DeviceState::Starting && !self.is_booting_here(&found) {
            self.show_run_error(
                device,
                format!(
                    "{} is still starting outside Zode. Run the app once it is running.",
                    found.name
                ),
            );
            cx.notify();
            return;
        }
        self.device_errors.remove(device);
        let booted = self.wait_until_booted(&found, cx);
        let task = cx.spawn_in(window, {
            let key = key.clone();
            let device = device.clone();
            let app = app.clone();
            let found = found.clone();
            async move |this, cx| {
                let result = start(&this, &key, app, found, scheme, booted, cx).await;
                this.update(cx, |this, cx| {
                    this.starting_runs.remove(&key);
                    if let Err(message) = result {
                        this.show_run_error(&device, message);
                    }
                    cx.notify();
                })
                .log_err();
            }
        });
        self.starting_runs.insert(
            key,
            StartingRun {
                device_name: found.name,
                exclusive: is_exclusive(app),
                _task: task,
            },
        );
        cx.notify();
    }
}
