//! A run after its click: the boot it waits for, a fresh look at the app
//! and the device, the user's answer, and the terminal tasks.

use devices::{
    AppProject, Device, DeviceState, HostOs, PlannedRun, RunPlan, plan_app_run, plan_metro,
};
use futures::channel::oneshot;
use gpui::{AppContext as _, AsyncWindowContext, Context, PromptLevel, SharedString, WeakEntity};
use task::SpawnInTerminal;

use crate::device_actions::offers;
use crate::device_operations::OperationKind;
use crate::devices_panel::DevicesPanel;
use crate::run_app::{ActiveRun, AppChoice, NOT_LOCAL, RunChoice, RunKey, RunTarget};
use crate::run_app_trust::AppRoots;
use crate::run_task::spawn_in_terminal;

impl DevicesPanel {
    /// Boots a stopped simulator or emulator through the panel's own Boot,
    /// so the row shows it, and returns what the run waits on.
    pub(crate) fn wait_until_booted(
        &mut self,
        device: &Device,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<bool>> {
        if device.state == DeviceState::Running {
            return None;
        }
        if offers(&OperationKind::Boot, device) {
            self.boot_device(&device.key, cx);
        }
        let operation = self
            .operations
            .get_mut(&device.key)
            .filter(|operation| operation.kind == OperationKind::Boot)?;
        let (sender, receiver) = oneshot::channel();
        operation.waiters.push(sender);
        Some(receiver)
    }

    /// The last checks, made in the same update as the tasks are built, right
    /// before they are spawned: the wait for a device may have taken minutes.
    fn tasks_to_spawn(
        &mut self,
        key: &RunKey,
        app: &AppProject,
        plan: &RunPlan,
        metro: Option<RunPlan>,
        cx: &mut Context<Self>,
    ) -> Result<Vec<(RunKey, SpawnInTerminal, bool)>, String> {
        if !self.is_supported(cx) {
            return Err(NOT_LOCAL.to_string());
        }
        self.ensure_trusted(&app.directory, cx)
            .map_err(|message| message.to_string())?;
        self.prune_finished_runs(cx);
        if let RunTarget::Device(device) = &key.target
            && let Some(message) = self.exclusive_conflict(app, device)
        {
            return Err(message);
        }
        let shell_kind = self.run_terminals.shell_kind();
        let mut tasks = Vec::new();
        if let Some(metro) = metro {
            let metro_key = RunKey {
                app_directory: app.directory.clone(),
                target: RunTarget::Metro,
            };
            if !self.active_runs.contains_key(&metro_key) {
                self.run_count += 1;
                let task = spawn_in_terminal(&metro, shell_kind, self.host.os(), self.run_count)
                    .map_err(|error| error.to_string())?;
                tasks.push((metro_key, task, true));
            }
        }
        self.run_count += 1;
        let task = spawn_in_terminal(plan, shell_kind, self.host.os(), self.run_count)
            .map_err(|error| error.to_string())?;
        tasks.push((key.clone(), task, plan.exclusive_per_project));
        Ok(tasks)
    }
}

/// Everything after the click that has to wait: the boot, a fresh look at
/// the app and the device, the Xcode probes, the user's answer, and the
/// terminal itself. Trust is checked again after every wait, before anything
/// of the project is read or run.
pub(crate) async fn start(
    this: &WeakEntity<DevicesPanel>,
    key: &RunKey,
    app: AppProject,
    device: Device,
    scheme: Option<String>,
    booted: Option<oneshot::Receiver<bool>>,
    cx: &mut AsyncWindowContext,
) -> Result<(), String> {
    if let Some(booted) = booted {
        match booted.await {
            Ok(true) => {}
            // The boot's own error is already on the row.
            Ok(false) => return Ok(()),
            Err(oneshot::Canceled) => {
                return Err(format!(
                    "{} did not finish starting, so the app was not run.",
                    device.name
                ));
            }
        }
    }
    let (host, runner, terminals, roots, generation) = this
        .update(cx, |this, cx| {
            let root = this.ensure_trusted(&app.directory, cx)?;
            let untrusted = this.app_roots(cx).untrusted;
            Ok::<_, SharedString>((
                this.host.clone(),
                this.runner.clone(),
                this.run_terminals.clone(),
                AppRoots {
                    trusted: vec![root],
                    untrusted,
                },
                this.begin_read(),
            ))
        })
        .map_err(|error| error.to_string())?
        .map_err(|message| message.to_string())?;
    let directory = app.directory.clone();
    let (fresh_app, snapshot) = cx
        .background_spawn({
            let host = host.clone();
            let runner = runner.clone();
            async move {
                let fresh_app = roots
                    .detect(&*host)
                    .into_iter()
                    .find(|candidate| candidate.directory == directory);
                (fresh_app, devices::discover(&*host, &*runner).await)
            }
        })
        .await;
    // The Xcode probes run project code, and the look above may have taken
    // a while: trust is checked again right before them.
    this.update(cx, |this, cx| {
        this.apply_read(generation, snapshot.clone());
        cx.notify();
        this.ensure_trusted(&app.directory, cx)
    })
    .map_err(|error| error.to_string())?
    .map_err(|message| message.to_string())?;
    let app = fresh_app.ok_or_else(|| {
        format!(
            "{} is no longer found in {}.",
            app.display_name,
            app.directory.display()
        )
    })?;
    let planned = cx
        .background_spawn({
            let app = app.clone();
            let device = device.clone();
            async move {
                plan_app_run(
                    &app,
                    &device,
                    &snapshot,
                    &*host,
                    &*runner,
                    scheme.as_deref(),
                )
                .await
            }
        })
        .await
        .map_err(|refusal| refusal.to_string())?;
    let plan = match planned {
        PlannedRun::Ready(plan) => plan,
        PlannedRun::ChooseScheme(schemes) => {
            this.update(cx, |this, cx| {
                this.app_choice = Some(AppChoice {
                    device: device.key.clone(),
                    choices: schemes
                        .into_iter()
                        .map(|scheme| RunChoice {
                            app: app.clone(),
                            scheme: Some(scheme),
                        })
                        .collect(),
                });
                cx.notify();
            })
            .map_err(|error| error.to_string())?;
            return Ok(());
        }
    };
    this.update(cx, |this, _| {
        if let Some(run) = this.starting_runs.get_mut(key) {
            run.exclusive = plan.exclusive_per_project;
        }
    })
    .map_err(|error| error.to_string())?;
    if let Some(confirmation) = plan.confirmation {
        let answer = cx
            .update(|window, cx| {
                window.prompt(
                    PromptLevel::Warning,
                    &confirmation.message(),
                    None,
                    &["Run", "Cancel"],
                    cx,
                )
            })
            .map_err(|error| error.to_string())?;
        if answer.await != Ok(0) {
            return Ok(());
        }
    }
    let metro = if plan.needs_metro {
        plan_metro(&app, host_os(this, cx)?)
    } else {
        None
    };
    let tasks = this
        .update(cx, |this, cx| {
            this.tasks_to_spawn(key, &app, &plan, metro, cx)
        })
        .map_err(|error| error.to_string())??;
    let mut metro_started = false;
    for (run_key, task, exclusive) in tasks {
        let is_metro = run_key.target == RunTarget::Metro;
        // Metro is spawned first and keeps running in its own terminal when
        // the run itself cannot start; the user is told where it is.
        let failed = |message: String| {
            if metro_started {
                format!("{message} Metro is still running in its own terminal.")
            } else {
                message
            }
        };
        // Spawning Metro may take a while; the folder may lose trust meanwhile.
        this.update(cx, |this, cx| this.ensure_trusted(&app.directory, cx))
            .map_err(|error| failed(error.to_string()))?
            .map_err(|message| failed(message.to_string()))?;
        let spawned = cx
            .update(|window, cx| terminals.spawn(task, window, cx))
            .map_err(|error| failed(error.to_string()))?;
        let terminal = spawned
            .await
            .map_err(|error| failed(format!("The terminal could not start the run: {error:#}")))?;
        metro_started |= is_metro;
        let device_name = device.name.clone();
        this.update(cx, |this, _| {
            this.active_runs.insert(
                run_key,
                ActiveRun {
                    device_name,
                    exclusive,
                    terminal,
                },
            );
        })
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn host_os(this: &WeakEntity<DevicesPanel>, cx: &mut AsyncWindowContext) -> Result<HostOs, String> {
    this.read_with(cx, |this, _| this.host.os())
        .map_err(|error| error.to_string())
}
