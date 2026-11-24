//! Run app: finds the apps in an open folder and turns an app and a device
//! into the framework's own command line. Nothing here spawns a process or
//! runs project code; every plan runs the project's own code, so the caller
//! applies the worktree trust gate before it spawns one.

use std::fmt;
use std::path::PathBuf;

use crate::host::HostOs;
use crate::input_validation::{InvalidInput, validate_serial, validate_udid};
use crate::run_app_cli_frameworks::{metro_step, run_step};
use crate::{Device, DeviceKind, DeviceSnapshot, DeviceState};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Framework {
    Flutter,
    Expo,
    ReactNative,
    AndroidGradle,
    IosXcode,
}

impl Framework {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Flutter => "Flutter",
            Self::Expo => "Expo",
            Self::ReactNative => "React Native",
            Self::AndroidGradle => "Android (Gradle)",
            Self::IosXcode => "Xcode",
        }
    }
}

/// What a native project's build tool needs to find the app in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeProject {
    Gradle { app_module: String },
    Xcode(XcodeContainer),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XcodeContainer {
    /// `MyApp.xcworkspace` or `MyApp.xcodeproj`, in the app's directory.
    pub file_name: String,
    pub is_workspace: bool,
}

impl XcodeContainer {
    pub fn flag(&self) -> &'static str {
        if self.is_workspace {
            "-workspace"
        } else {
            "-project"
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl PackageVersion {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppProject {
    pub framework: Framework,
    pub directory: PathBuf,
    /// From `package.json` or `pubspec.yaml`. Shown to the user and put in
    /// task labels, never in a command line.
    pub display_name: String,
    pub has_ios_folder: bool,
    pub has_android_folder: bool,
    /// Expo and React Native: the framework's package is in `node_modules`,
    /// where `npx --no-install` looks for it. Always true for Flutter, whose
    /// `flutter run` fetches packages itself.
    pub dependencies_installed: bool,
    pub react_native_cli_version: Option<PackageVersion>,
    pub react_native_version: Option<PackageVersion>,
    /// Gradle and Xcode projects only.
    pub native: Option<NativeProject>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetPlatform {
    Ios,
    Android,
}

impl TargetPlatform {
    pub fn of(kind: DeviceKind) -> Self {
        match kind {
            DeviceKind::IosSimulator | DeviceKind::IosPhysical => Self::Ios,
            DeviceKind::AndroidEmulator | DeviceKind::AndroidUsb | DeviceKind::AndroidWireless => {
                Self::Android
            }
        }
    }

    pub fn folder_name(self) -> &'static str {
        match self {
            Self::Ios => "ios",
            Self::Android => "android",
        }
    }
}

/// One program and its arguments. The program is a bare name that the
/// terminal's shell finds on its own `PATH`, the project's own wrapper
/// (`./gradlew`), or an absolute path; the arguments are raw and have passed
/// the shell-safety check, and quoting them is the spawner's job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunStep {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunConfirmation {
    ExpoPrebuildCreatesNativeFolders { platform: TargetPlatform },
}

impl RunConfirmation {
    pub fn message(self) -> String {
        match self {
            Self::ExpoPrebuildCreatesNativeFolders { platform } => {
                let folder = platform.folder_name();
                format!(
                    "This project has no {folder} folder. Expo runs \"prebuild\" first, which \
                     creates the {folder} folder and edits app.json and package.json."
                )
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunPlan {
    /// "Run <app> on <device>"; the spawner makes it unique per run.
    pub label: String,
    pub steps: Vec<RunStep>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    /// Ask the user before the first run.
    pub confirmation: Option<RunConfirmation>,
    /// Expo and React Native share Metro's default port, so only one of their
    /// runs may be live per project.
    pub exclusive_per_project: bool,
    /// React Native runs with `--no-packager`: start [`plan_metro`] too.
    pub needs_metro: bool,
    /// Gradle: after the install, open the app the build just installed.
    pub open_installed_android_app: Option<OpenInstalledAndroidApp>,
}

impl RunPlan {
    /// Steps that must stop at the first failure. The spawner runs them in a
    /// shell it picks, never the user's: PowerShell 5, Nushell and Elvish
    /// would chain them with `;`, which installs an old build after a failed
    /// one.
    pub fn is_chained(&self) -> bool {
        self.steps.len() > 1 || self.open_installed_android_app.is_some()
    }
}

/// The installed id is read from the build's own output after the install,
/// inside the same command: `applicationIdSuffix` and flavors make the id in
/// `build.gradle` differ from the one on the device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenInstalledAndroidApp {
    pub adb: PathBuf,
    pub serial: String,
    /// Relative to the plan's `cwd`, with the host's path separator.
    pub output_metadata: String,
}

/// Xcode projects with several schemes ask which one, on every click.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannedRun {
    Ready(RunPlan),
    ChooseScheme(Vec<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunRefusal {
    DeviceNotFound {
        device_name: String,
    },
    DeviceNotReady {
        device_name: String,
    },
    PlatformMissing {
        framework: Framework,
        platform: TargetPlatform,
    },
    DependenciesNotInstalled {
        framework: Framework,
        directory: PathBuf,
    },
    UnsafeArgument {
        what: &'static str,
        value: String,
        reason: String,
    },
    Unsupported {
        reason: String,
    },
    /// A read-only probe of the project's build tool failed.
    ProjectToolFailed {
        tool: &'static str,
        message: String,
    },
}

impl From<InvalidInput> for RunRefusal {
    fn from(invalid: InvalidInput) -> Self {
        Self::UnsafeArgument {
            what: invalid.what,
            value: invalid.value,
            reason: invalid.reason,
        }
    }
}

impl fmt::Display for RunRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeviceNotFound { device_name } => write!(
                formatter,
                "{device_name} is no longer connected. Reload the device list and try again."
            ),
            Self::DeviceNotReady { device_name } => write!(
                formatter,
                "{device_name} is not running. Start it and wait until it has booted."
            ),
            Self::PlatformMissing {
                framework,
                platform,
            } => write!(
                formatter,
                "This {} project has no {} folder, so it cannot run on this device.",
                framework.display_name(),
                platform.folder_name()
            ),
            Self::DependenciesNotInstalled {
                framework,
                directory,
            } => write!(
                formatter,
                "The {} packages are not installed. Run your package manager's install in {}.",
                framework.display_name(),
                directory.display()
            ),
            Self::UnsafeArgument {
                what,
                value,
                reason,
            } => write!(
                formatter,
                "The {what} \"{value}\" cannot be passed to a shell safely: {reason}."
            ),
            Self::Unsupported { reason } => formatter.write_str(reason),
            Self::ProjectToolFailed { tool, message } => {
                write!(formatter, "{tool} could not read the project: {message}")
            }
        }
    }
}

impl std::error::Error for RunRefusal {}

/// Plans a run of a Flutter, Expo or React Native `project` on `device`; see
/// `plan_app_run` for every framework. The device is looked up again in a
/// fresh `snapshot`: the React Native CLI exits 0 when its device is gone, so
/// a stale row must be refused here rather than reported as a success later.
pub fn plan_run(
    project: &AppProject,
    device: &Device,
    snapshot: &DeviceSnapshot,
    host_os: HostOs,
) -> Result<RunPlan, RunRefusal> {
    let ready = ready_device(device, snapshot)?;
    let platform = ready.platform;
    let has_folder = match platform {
        TargetPlatform::Ios => project.has_ios_folder,
        TargetPlatform::Android => project.has_android_folder,
    };
    let mut confirmation = None;
    if !has_folder {
        if project.framework == Framework::Expo {
            confirmation = Some(RunConfirmation::ExpoPrebuildCreatesNativeFolders { platform });
        } else {
            return Err(RunRefusal::PlatformMissing {
                framework: project.framework,
                platform,
            });
        }
    }
    if !project.dependencies_installed {
        return Err(RunRefusal::DependenciesNotInstalled {
            framework: project.framework,
            directory: project.directory.clone(),
        });
    }
    let step = run_step(project, platform, &ready.run_id, host_os).ok_or_else(|| {
        RunRefusal::Unsupported {
            reason: format!(
                "{} projects are planned with their build tool.",
                project.framework.display_name()
            ),
        }
    })?;

    let uses_metro = matches!(project.framework, Framework::Expo | Framework::ReactNative);
    Ok(RunPlan {
        label: run_label(project, &ready.device),
        steps: vec![step],
        env: Vec::new(),
        cwd: project.directory.clone(),
        confirmation,
        exclusive_per_project: uses_metro,
        needs_metro: project.framework == Framework::ReactNative,
        open_installed_android_app: None,
    })
}

pub(crate) fn run_label(project: &AppProject, device: &Device) -> String {
    format!("Run {} on {}", project.display_name, device.name)
}

pub(crate) struct ReadyDevice {
    pub device: Device,
    pub run_id: String,
    pub platform: TargetPlatform,
}

/// The device as a fresh `snapshot` sees it, running, with an id that is safe
/// in a command line.
pub(crate) fn ready_device(
    device: &Device,
    snapshot: &DeviceSnapshot,
) -> Result<ReadyDevice, RunRefusal> {
    let fresh = all_devices(snapshot)
        .find(|candidate| candidate.key == device.key)
        .ok_or_else(|| RunRefusal::DeviceNotFound {
            device_name: device.name.clone(),
        })?;
    let not_ready = || RunRefusal::DeviceNotReady {
        device_name: fresh.name.clone(),
    };
    if fresh.state != DeviceState::Running {
        return Err(not_ready());
    }
    let run_id = fresh.run_id.as_deref().ok_or_else(not_ready)?;
    let platform = TargetPlatform::of(fresh.kind);
    match platform {
        TargetPlatform::Ios => validate_udid(run_id),
        TargetPlatform::Android => validate_serial(run_id),
    }?;
    Ok(ReadyDevice {
        device: fresh.clone(),
        run_id: run_id.to_string(),
        platform,
    })
}

/// The Metro bundler for a React Native run, as its own task: started by the
/// CLI instead, it would open a new window in the app named by
/// `TERM_PROGRAM`, which Zode's terminal sets to `zode`.
pub fn plan_metro(project: &AppProject, host_os: HostOs) -> Option<RunPlan> {
    if project.framework != Framework::ReactNative {
        return None;
    }
    Some(RunPlan {
        label: format!("Metro for {}", project.display_name),
        steps: vec![metro_step(host_os)],
        env: Vec::new(),
        cwd: project.directory.clone(),
        confirmation: None,
        exclusive_per_project: true,
        needs_metro: false,
        open_installed_android_app: None,
    })
}

fn all_devices(snapshot: &DeviceSnapshot) -> impl Iterator<Item = &Device> {
    [
        &snapshot.ios_simulators,
        &snapshot.android_emulators,
        &snapshot.physical_ios,
        &snapshot.physical_android,
    ]
    .into_iter()
    .flat_map(|section| section.devices())
}
