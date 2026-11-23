use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context as _, anyhow, bail};

use crate::android_sdk::AndroidSdk;
use crate::host::{DetachedExit, HostOs, HostSystem, ToolInvocation, ToolOutput, ToolRunner};
use crate::input_validation::{
    PairingCode, WirelessAddress, validate_avd_name, validate_serial, validate_udid,
};
use crate::ios_simulators::{XCRUN, run_simctl};
use crate::resources::{
    EMULATOR_LOGS_KEPT, emulator_log_file_name, prune_emulator_logs, prune_stale_emulator_logs,
};

const OPEN: &str = "/usr/bin/open";
/// The first boot after an Xcode or runtime update migrates data, and a cold
/// CoreSimulatorService or a busy simulator is slow to stop; simctl blocks
/// meanwhile.
const SIMCTL_STATE_CHANGE_TIMEOUT: Duration = Duration::from_secs(120);

pub async fn boot_simulator(runner: &dyn ToolRunner, udid: &str) -> anyhow::Result<()> {
    change_simulator_state(runner, "boot", udid, &["Booted", "Booting"]).await
}

pub async fn shutdown_simulator(runner: &dyn ToolRunner, udid: &str) -> anyhow::Result<()> {
    change_simulator_state(runner, "shutdown", udid, &["Shutdown", "Shutting Down"]).await
}

/// A redundant boot or shutdown exits 149 with "Unable to <verb> device in
/// current state: <state>". Being in the wanted state, or on the way to it,
/// is success: the caller follows the device until it settles.
async fn change_simulator_state(
    runner: &dyn ToolRunner,
    verb: &str,
    udid: &str,
    accepted_states: &[&str],
) -> anyhow::Result<()> {
    validate_udid(udid)?;
    let invocation = ToolInvocation::new(XCRUN, ["simctl", verb, udid])
        .with_timeout(SIMCTL_STATE_CHANGE_TIMEOUT);
    let output = run_simctl(runner, invocation).await?;
    let current_state = output
        .stderr
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix(&format!("Unable to {verb} device in current state:"))
        })
        .map(str::trim);
    if output.success() || current_state.is_some_and(|state| accepted_states.contains(&state)) {
        Ok(())
    } else {
        Err(anyhow!(
            "simctl {verb} failed: {}",
            output.failure_message()
        ))
    }
}

/// The one place to change if Xcode moves simulators into another app. When
/// Simulator.app is already running this only brings it forward.
pub async fn open_simulator_window(runner: &dyn ToolRunner, udid: &str) -> anyhow::Result<()> {
    validate_udid(udid)?;
    let output = runner
        .run(ToolInvocation::new(
            OPEN,
            ["-a", "Simulator", "--args", "-CurrentDeviceUDID", udid],
        ))
        .await?;
    ensure_success(&output, "could not open Simulator")
}

#[derive(Debug)]
pub struct StartedEmulator {
    /// Its tail explains a failed start.
    pub log_file: PathBuf,
    /// Settles when the emulator process exits.
    pub exit: DetachedExit,
    /// For [`stop_detached_emulator`], while `exit` has not settled.
    pub process_id: u32,
}

/// Starts the emulator detached so it outlives Zode, logging to a new file in
/// `log_dir`.
pub fn start_emulator(
    host: &dyn HostSystem,
    runner: &dyn ToolRunner,
    sdk: &AndroidSdk,
    avd_name: &str,
    log_dir: &Path,
    started_at: SystemTime,
) -> anyhow::Result<StartedEmulator> {
    validate_avd_name(avd_name)?;
    let emulator = sdk.emulator.as_ref().context(
        "the Android Emulator is not installed; install \"Android Emulator\" from the SDK Manager",
    )?;
    prune_stale_emulator_logs(host, log_dir, started_at);
    // One slot is left for the log this start creates.
    prune_emulator_logs(host, log_dir, avd_name, EMULATOR_LOGS_KEPT - 1);
    let log_file = log_dir.join(emulator_log_file_name(avd_name, started_at));
    let started =
        runner.spawn_detached(ToolInvocation::new(emulator, ["-avd", avd_name]), &log_file)?;
    Ok(StartedEmulator {
        log_file,
        exit: started.exit,
        process_id: started.process_id,
    })
}

/// Ends an emulator that never showed up in adb, so there is no serial to
/// send `emu kill` to, together with the processes it started. `exit` is the
/// emulator's own exit signal, read right before the kill: once the process
/// has ended its id may belong to another process, so nothing is signalled
/// and the emulator counts as stopped.
pub async fn stop_detached_emulator(
    host: &dyn HostSystem,
    runner: &dyn ToolRunner,
    process_id: u32,
    exit: &mut DetachedExit,
) -> anyhow::Result<()> {
    if !matches!(exit.try_recv(), Ok(None)) {
        return Ok(());
    }
    match host.os() {
        // The emulator leads its own process group, whose id is its own.
        HostOs::MacOs | HostOs::Linux => runner.terminate_process_group(process_id),
        HostOs::Windows => {
            let system_root = host
                .environment_variable("SystemRoot")
                .unwrap_or_else(|| "C:\\Windows".to_string());
            let output = runner
                .run(ToolInvocation::new(
                    PathBuf::from(system_root)
                        .join("System32")
                        .join("taskkill.exe"),
                    [
                        "/PID".to_string(),
                        process_id.to_string(),
                        "/T".to_string(),
                        "/F".to_string(),
                    ],
                ))
                .await?;
            ensure_success(&output, "could not stop the emulator process")
        }
    }
}

pub async fn stop_emulator(
    runner: &dyn ToolRunner,
    sdk: &AndroidSdk,
    serial: &str,
) -> anyhow::Result<()> {
    validate_serial(serial)?;
    let output = runner
        .run(ToolInvocation::new(&sdk.adb, ["-s", serial, "emu", "kill"]))
        .await?;
    ensure_success(&output, "could not stop the emulator")
}

const PAIRING_PROMPT: &str = "Enter pairing code:";

/// The code goes in on stdin, which adb reads when it is not an argument, so
/// it never shows in a process list. adb exits 0 on some failures, so the
/// output decides; the code is masked out of anything returned.
pub async fn adb_pair(
    runner: &dyn ToolRunner,
    sdk: &AndroidSdk,
    address: &WirelessAddress,
    code: &PairingCode,
) -> anyhow::Result<()> {
    let output = runner
        .run(
            ToolInvocation::new(&sdk.adb, ["pair".to_string(), address.to_string()])
                .with_stdin(format!("{}\n", code.as_str())),
        )
        .await?;
    // The prompt has no newline, so the first line starts with it. Matching
    // the success line rather than hunting for failure words keeps a device
    // whose guid or serial says "fail" from reading as a failed pairing.
    let paired = output.stdout.lines().any(|line| {
        let line = line.trim_start();
        line.strip_prefix(PAIRING_PROMPT)
            .unwrap_or(line)
            .trim_start()
            .starts_with("Successfully paired")
    });
    if output.success() && paired {
        return Ok(());
    }
    let message = output.failure_message().replace(code.as_str(), "******");
    bail!("pairing with {address} failed: {message}")
}

pub async fn adb_connect(
    runner: &dyn ToolRunner,
    sdk: &AndroidSdk,
    address: &WirelessAddress,
) -> anyhow::Result<()> {
    let output = runner
        .run(ToolInvocation::new(
            &sdk.adb,
            ["connect".to_string(), address.to_string()],
        ))
        .await?;
    let stdout = output.stdout.trim_start();
    if output.success()
        && (stdout.starts_with("connected to") || stdout.starts_with("already connected to"))
    {
        Ok(())
    } else {
        Err(anyhow!(
            "could not connect to {address}: {}",
            output.failure_message()
        ))
    }
}

pub async fn adb_disconnect(
    runner: &dyn ToolRunner,
    sdk: &AndroidSdk,
    address: &WirelessAddress,
) -> anyhow::Result<()> {
    let output = runner
        .run(ToolInvocation::new(
            &sdk.adb,
            ["disconnect".to_string(), address.to_string()],
        ))
        .await?;
    if output.success() && output.stdout.trim_start().starts_with("disconnected") {
        Ok(())
    } else {
        Err(anyhow!(
            "could not disconnect {address}: {}",
            output.failure_message()
        ))
    }
}

fn ensure_success(output: &ToolOutput, context: &str) -> anyhow::Result<()> {
    if output.success() {
        Ok(())
    } else {
        Err(anyhow!("{context}: {}", output.failure_message()))
    }
}
