//! What one Boot or Shutdown does once it is clicked: the command, then
//! following that one device until it settles or the time is up. All of it
//! runs on the background executor and ends in a single [`OperationOutcome`],
//! so the panel only has to apply a result.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context as _, anyhow};
use devices::{
    DetachedExit, Device, HostSystem, ToolRunner, TrackTarget, TrackingVerdict, boot_simulator,
    explain_emulator_failure, locate_android_sdk, open_simulator_window, probe,
    read_emulator_log_tail, shutdown_simulator, start_emulator, stop_emulator, tracking_verdict,
};
use futures::future::Either;
use gpui::BackgroundExecutor;

use crate::device_operations::{DeviceError, OperationOutcome, OperationPlan, Settled};

/// How often the one device being followed is looked at. Nothing else in the
/// panel runs on a timer.
pub(crate) const FOLLOW_INTERVAL: Duration = Duration::from_millis(1500);

#[derive(Clone)]
pub(crate) struct OperationContext {
    pub host: Arc<dyn HostSystem>,
    pub runner: Arc<dyn ToolRunner>,
    pub log_dir: PathBuf,
    pub executor: BackgroundExecutor,
}

struct Started {
    target: TrackTarget,
    log_file: Option<PathBuf>,
    /// The started emulator process, which may die before adb ever sees it.
    exit: Option<DetachedExit>,
    process_id: Option<u32>,
    error: Option<DeviceError>,
}

/// An emulator that Zode started and gave up on, whose process may still be
/// running although adb never listed it: nothing else can stop it.
pub(crate) struct StrayEmulator {
    pub process_id: u32,
    /// Still unsettled only while `process_id` is still this emulator's.
    pub exit: DetachedExit,
}

pub(crate) struct Executed {
    pub outcome: OperationOutcome,
    pub available_bytes: Option<u64>,
    pub stray: Option<StrayEmulator>,
}

pub(crate) async fn execute(context: OperationContext, plan: OperationPlan) -> Executed {
    let (outcome, stray) = run(&context, plan).await;
    Executed {
        outcome,
        available_bytes: context.host.available_memory_bytes(),
        stray,
    }
}

async fn run(
    context: &OperationContext,
    plan: OperationPlan,
) -> (OperationOutcome, Option<StrayEmulator>) {
    if let OperationPlan::ShowSimulatorWindow { udid } = &plan {
        let outcome = match open_simulator_window(&*context.runner, udid).await {
            Ok(()) => OperationOutcome {
                settled: Settled::Done(None),
                error: None,
            },
            Err(error) => OperationOutcome::failed(DeviceError::new(format!("{error:#}")), None),
        };
        return (outcome, None);
    }
    let started = match start(context, plan).await {
        Ok(started) => started,
        Err(error) => {
            let outcome = OperationOutcome::failed(DeviceError::new(format!("{error:#}")), None);
            return (outcome, None);
        }
    };
    follow(context, started).await
}

async fn start(context: &OperationContext, plan: OperationPlan) -> anyhow::Result<Started> {
    let runner = &*context.runner;
    match plan {
        OperationPlan::BootSimulator { udid } => {
            boot_simulator(runner, &udid).await?;
            let error = open_simulator_window(runner, &udid)
                .await
                .err()
                .map(|error| {
                    DeviceError::new(format!(
                        "Booted, but the Simulator window did not open: {error:#}"
                    ))
                });
            Ok(Started {
                target: TrackTarget::SimulatorBoot { udid },
                log_file: None,
                exit: None,
                process_id: None,
                error,
            })
        }
        OperationPlan::ShutdownSimulator { udid } => {
            shutdown_simulator(runner, &udid).await?;
            Ok(Started {
                target: TrackTarget::SimulatorShutdown { udid },
                log_file: None,
                exit: None,
                process_id: None,
                error: None,
            })
        }
        OperationPlan::StartEmulator {
            avd_name,
            serials_before,
        } => {
            let sdk =
                locate_android_sdk(&*context.host).context("The Android SDK was not found")?;
            let started = start_emulator(
                &*context.host,
                runner,
                &sdk,
                &avd_name,
                &context.log_dir,
                SystemTime::now(),
            )?;
            Ok(Started {
                target: TrackTarget::EmulatorStart {
                    avd_name,
                    serials_before,
                },
                log_file: Some(started.log_file),
                exit: Some(started.exit),
                process_id: Some(started.process_id),
                error: None,
            })
        }
        OperationPlan::StopEmulator { avd_name, serial } => {
            let sdk =
                locate_android_sdk(&*context.host).context("The Android SDK was not found")?;
            stop_emulator(runner, &sdk, &serial).await?;
            Ok(Started {
                target: TrackTarget::EmulatorStop { avd_name, serial },
                log_file: None,
                exit: None,
                process_id: None,
                error: None,
            })
        }
        OperationPlan::ShowSimulatorWindow { .. } => Err(anyhow!("not a followed operation")),
    }
}

/// One probe per interval for the one device acted on, until the verdict says
/// stop or the started emulator process exits. This loop ending is the only
/// thing that ever stops it.
async fn follow(
    context: &OperationContext,
    started: Started,
) -> (OperationOutcome, Option<StrayEmulator>) {
    let Started {
        target,
        log_file,
        mut exit,
        process_id,
        error,
    } = started;
    let deadline = context.executor.now() + target.time_limit();
    let mut last_seen: Option<Device> = None;
    loop {
        let interval = context.executor.timer(FOLLOW_INTERVAL);
        match exit.as_mut() {
            Some(receiver) => match futures::future::select(interval, receiver).await {
                Either::Left(_) => {}
                Either::Right((Ok(exit_code), _)) => {
                    return (exited_early(context, &target, exit_code, log_file), None);
                }
                // Its exit cannot be known; adb alone decides from here.
                Either::Right((Err(_), _)) => {
                    exit = None;
                    continue;
                }
            },
            None => interval.await,
        }
        let time_left = deadline.saturating_duration_since(context.executor.now());
        let result = probe(&*context.host, &*context.runner, &target, time_left).await;
        if result.device.is_some() {
            last_seen = result.device.clone();
        }
        match tracking_verdict(&target, &result, context.executor.now(), deadline) {
            TrackingVerdict::Continue => {}
            TrackingVerdict::Done(device) => {
                let outcome = OperationOutcome {
                    settled: Settled::Done(device),
                    error,
                };
                return (outcome, None);
            }
            TrackingVerdict::TimedOut(reason) => {
                // With no serial adb cannot stop it; the process can still be
                // running, so it stays reachable instead of being forgotten.
                let stray = match (&target, exit, process_id) {
                    (TrackTarget::EmulatorStart { .. }, Some(exit), Some(process_id))
                        if last_seen
                            .as_ref()
                            .is_none_or(|device| device.serial.is_none()) =>
                    {
                        Some(StrayEmulator { process_id, exit })
                    }
                    _ => None,
                };
                let mut message = explained(context, reason, log_file.as_deref());
                if stray.is_some() {
                    message.push_str(STILL_RUNNING_NOTE);
                }
                let outcome = OperationOutcome::failed(
                    DeviceError {
                        message: message.into(),
                        log_file,
                    },
                    last_seen,
                );
                return (outcome, stray);
            }
        }
    }
}

pub(crate) const STILL_RUNNING_NOTE: &str =
    " It may still be starting in the background; stop it to start this one again.";

/// The emulator process ended before the device came up: nothing will come
/// up, and its log already says why.
fn exited_early(
    context: &OperationContext,
    target: &TrackTarget,
    exit_code: Option<i32>,
    log_file: Option<PathBuf>,
) -> OperationOutcome {
    let name = match target {
        TrackTarget::EmulatorStart { avd_name, .. } => avd_name.as_str(),
        _ => "the device",
    };
    let how = match exit_code {
        Some(code) => format!("with code {code}"),
        None => "on a signal".to_string(),
    };
    let reason = format!("The emulator {name} quit {how} before it finished booting.");
    let message = explained(context, reason, log_file.as_deref());
    OperationOutcome::failed(
        DeviceError {
            message: message.into(),
            log_file,
        },
        None,
    )
}

fn explained(context: &OperationContext, reason: String, log_file: Option<&Path>) -> String {
    let explanation = log_file
        .and_then(|file| read_emulator_log_tail(&*context.host, file))
        .and_then(|tail| explain_emulator_failure(&tail));
    match explanation {
        Some(explanation) => format!("{reason} {explanation}"),
        None => reason,
    }
}
