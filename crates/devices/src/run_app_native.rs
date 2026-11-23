//! Android Gradle and Xcode projects: found by their files, planned as one
//! chained command that builds, installs and opens the app.

use std::path::{Path, PathBuf};

use crate::android_sdk::locate_android_sdk;
use crate::host::{HostOs, HostSystem, ToolRunner};
use crate::input_validation::validate_identifier;
use crate::run_app::{
    AppProject, Framework, NativeProject, OpenInstalledAndroidApp, PlannedRun, RunPlan, RunRefusal,
    RunStep, TargetPlatform, XcodeContainer, plan_run, ready_device, run_label,
};
use crate::run_app_detect::{SKIPPED_DIRECTORIES, display_name, is_excluded, read_project_file};
use crate::run_app_xcode::plan_xcode;
use crate::{Device, DeviceSnapshot};

/// Plans a run of any detected app. Xcode projects run `xcodebuild` to read
/// their schemes and build settings, which runs project code: call this only
/// after the worktree trust check.
pub async fn plan_app_run(
    project: &AppProject,
    device: &Device,
    snapshot: &DeviceSnapshot,
    host: &dyn HostSystem,
    runner: &dyn ToolRunner,
    scheme: Option<&str>,
) -> Result<PlannedRun, RunRefusal> {
    match &project.native {
        None => plan_run(project, device, snapshot, host.os()).map(PlannedRun::Ready),
        Some(NativeProject::Gradle { app_module }) => {
            plan_gradle(project, app_module, device, snapshot, host).map(PlannedRun::Ready)
        }
        Some(NativeProject::Xcode(container)) => {
            plan_xcode(project, container, device, snapshot, host, runner, scheme).await
        }
    }
}

fn plan_gradle(
    project: &AppProject,
    app_module: &str,
    device: &Device,
    snapshot: &DeviceSnapshot,
    host: &dyn HostSystem,
) -> Result<RunPlan, RunRefusal> {
    let ready = ready_device(device, snapshot)?;
    if ready.platform != TargetPlatform::Android {
        return Err(RunRefusal::PlatformMissing {
            framework: Framework::AndroidGradle,
            platform: ready.platform,
        });
    }
    validate_identifier("Gradle module", app_module)?;
    let sdk = locate_android_sdk(host).ok_or_else(|| RunRefusal::Unsupported {
        reason: "adb was not found. Install the Android SDK platform tools, or set ANDROID_HOME."
            .to_string(),
    })?;
    let os = host.os();
    let (wrapper, separator) = match os {
        HostOs::Windows => (".\\gradlew.bat", "\\"),
        HostOs::MacOs | HostOs::Linux => ("./gradlew", "/"),
    };
    let output_metadata = format!(
        "{app_module}{separator}build{separator}outputs{separator}apk{separator}debug\
         {separator}output-metadata.json"
    );
    Ok(RunPlan {
        label: run_label(project, &ready.device),
        steps: vec![RunStep {
            program: wrapper.to_string(),
            args: vec![format!(":{app_module}:installDebug")],
        }],
        // Gradle has no device flag; the Android Gradle plugin reads this.
        env: vec![("ANDROID_SERIAL".to_string(), ready.run_id.clone())],
        cwd: project.directory.clone(),
        confirmation: None,
        exclusive_per_project: false,
        needs_metro: false,
        open_installed_android_app: Some(OpenInstalledAndroidApp {
            adb: sdk.adb,
            serial: ready.run_id,
            output_metadata,
        }),
    })
}

/// A Gradle or Xcode project in `directory`, read from file names and build
/// scripts only. The `android` and `ios` folders of a Flutter, Expo or React
/// Native app are left to that app, even when opened on their own.
/// Modules below `excluded` are not read.
pub(crate) fn native_app(
    host: &dyn HostSystem,
    directory: &Path,
    excluded: &[PathBuf],
) -> Option<AppProject> {
    if is_framework_native_folder(host, directory) {
        return None;
    }
    let mut names = host.read_dir_names(directory).ok()?;
    names.sort();
    if let Some(app_module) = gradle_app_module(host, directory, &names, excluded) {
        return Some(native_project(
            Framework::AndroidGradle,
            directory,
            display_name(None, directory),
            NativeProject::Gradle { app_module },
        ));
    }
    if host.os() != HostOs::MacOs {
        return None;
    }
    let container = xcode_container(host, directory, &names)?;
    let stem = Path::new(&container.file_name)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned());
    Some(native_project(
        Framework::IosXcode,
        directory,
        display_name(stem.as_deref(), directory),
        NativeProject::Xcode(container),
    ))
}

fn native_project(
    framework: Framework,
    directory: &Path,
    display_name: String,
    native: NativeProject,
) -> AppProject {
    AppProject {
        framework,
        directory: directory.to_path_buf(),
        display_name,
        has_ios_folder: framework == Framework::IosXcode,
        has_android_folder: framework == Framework::AndroidGradle,
        dependencies_installed: true,
        react_native_cli_version: None,
        react_native_version: None,
        native: Some(native),
    }
}

fn is_framework_native_folder(host: &dyn HostSystem, directory: &Path) -> bool {
    let is_native_folder = directory
        .file_name()
        .is_some_and(|name| name == "ios" || name == "android");
    let Some(parent) = directory.parent().filter(|_| is_native_folder) else {
        return false;
    };
    ["pubspec.yaml", "package.json"]
        .iter()
        .any(|file| host.is_file(&parent.join(file)))
}

/// The module `installDebug` runs on: `app` when it applies the Android
/// application plugin, otherwise the first such module by name.
fn gradle_app_module(
    host: &dyn HostSystem,
    directory: &Path,
    names: &[String],
    excluded: &[PathBuf],
) -> Option<String> {
    let wrapper = match host.os() {
        HostOs::Windows => "gradlew.bat",
        HostOs::MacOs | HostOs::Linux => "gradlew",
    };
    let is_gradle_root = host.is_file(&directory.join(wrapper))
        && ["settings.gradle", "settings.gradle.kts"]
            .iter()
            .any(|file| host.is_file(&directory.join(file)));
    if !is_gradle_root {
        return None;
    }
    let is_application = |name: &String| {
        ["build.gradle", "build.gradle.kts"].iter().any(|file| {
            // Matches `com.android.application` and the version catalog's
            // `libs.plugins.android.application`.
            read_project_file(host, &directory.join(name).join(file))
                .is_some_and(|script| script.contains("android.application"))
        })
    };
    let modules: Vec<&String> = names
        .iter()
        .filter(|name| !name.starts_with('.') && !SKIPPED_DIRECTORIES.contains(&name.as_str()))
        .filter(|name| !is_excluded(&directory.join(name.as_str()), excluded))
        .filter(|name| is_application(name))
        .collect();
    modules
        .iter()
        .find(|name| name.as_str() == "app")
        .or(modules.first())
        .map(|name| (*name).clone())
}

/// A workspace wins over a project: with CocoaPods only the workspace builds.
fn xcode_container(
    host: &dyn HostSystem,
    directory: &Path,
    names: &[String],
) -> Option<XcodeContainer> {
    let find = |extension: &str| {
        names.iter().find(|name| {
            name.ends_with(extension)
                && !name.starts_with("Pods.")
                && host.is_dir(&directory.join(name.as_str()))
        })
    };
    if let Some(workspace) = find(".xcworkspace") {
        return Some(XcodeContainer {
            file_name: workspace.clone(),
            is_workspace: true,
        });
    }
    find(".xcodeproj").map(|project| XcodeContainer {
        file_name: project.clone(),
        is_workspace: false,
    })
}
