//! The command line each framework's own CLI takes to run on one device.
//! Programs stay bare names: they run in the terminal's shell, which finds
//! them on its own `PATH`. On Windows the `.cmd`/`.bat` launcher is named
//! outright, because PowerShell would otherwise pick `npx.ps1`, which the
//! default execution policy blocks. cmd.exe looks in the working directory
//! before `PATH`, so a `flutter.bat` in the project would win: acceptable,
//! because a run only starts in a trusted folder and runs its code anyway.

use crate::host::HostOs;
use crate::run_app::{AppProject, Framework, PackageVersion, RunStep, TargetPlatform};

/// `run-android --deviceId` was renamed `--device` in CLI 13, which ships
/// with React Native 0.74. Newer CLIs still accept `--deviceId` with a
/// warning, but older ones do not know `--device`.
const FIRST_CLI_WITH_DEVICE_FLAG: u64 = 13;
const FIRST_REACT_NATIVE_WITH_DEVICE_FLAG: PackageVersion = PackageVersion::new(0, 74, 0);

pub(crate) fn run_step(
    project: &AppProject,
    platform: TargetPlatform,
    run_id: &str,
    host_os: HostOs,
) -> Option<RunStep> {
    let step = match project.framework {
        Framework::AndroidGradle | Framework::IosXcode => return None,
        Framework::Flutter => RunStep {
            program: flutter(host_os),
            args: owned(&["run", "-d", run_id]),
        },
        Framework::Expo => {
            let command = match platform {
                TargetPlatform::Ios => "run:ios",
                TargetPlatform::Android => "run:android",
            };
            npx(host_os, &["expo", command, "-d", run_id])
        }
        // `--no-packager`: Zode starts Metro itself, see `plan_metro`.
        Framework::ReactNative => match platform {
            TargetPlatform::Ios => npx(
                host_os,
                &["react-native", "run-ios", "--udid", run_id, "--no-packager"],
            ),
            TargetPlatform::Android => {
                let flag = react_native_android_device_flag(
                    project.react_native_cli_version,
                    project.react_native_version,
                );
                npx(
                    host_os,
                    &["react-native", "run-android", flag, run_id, "--no-packager"],
                )
            }
        },
    };
    Some(step)
}

pub(crate) fn metro_step(host_os: HostOs) -> RunStep {
    npx(host_os, &["react-native", "start"])
}

/// The CLI's own version decides; the React Native version stands in when the
/// CLI package cannot be read, and the current flag is used when neither can.
pub fn react_native_android_device_flag(
    cli: Option<PackageVersion>,
    react_native: Option<PackageVersion>,
) -> &'static str {
    let has_device_flag = match (cli, react_native) {
        (Some(cli), _) => cli.major >= FIRST_CLI_WITH_DEVICE_FLAG,
        (None, Some(react_native)) => react_native >= FIRST_REACT_NATIVE_WITH_DEVICE_FLAG,
        (None, None) => true,
    };
    if has_device_flag {
        "--device"
    } else {
        "--deviceId"
    }
}

/// `--no-install` only stops npx from downloading a package the project
/// lacks; the project's own installed code still runs.
fn npx(host_os: HostOs, args: &[&str]) -> RunStep {
    let program = match host_os {
        HostOs::Windows => "npx.cmd",
        HostOs::MacOs | HostOs::Linux => "npx",
    };
    let mut all_args = owned(&["--no-install"]);
    all_args.extend(owned(args));
    RunStep {
        program: program.to_string(),
        args: all_args,
    }
}

fn flutter(host_os: HostOs) -> String {
    match host_os {
        HostOs::Windows => "flutter.bat",
        HostOs::MacOs | HostOs::Linux => "flutter",
    }
    .to_string()
}

fn owned(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| arg.to_string()).collect()
}
