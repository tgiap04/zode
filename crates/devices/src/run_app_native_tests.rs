use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::fake_host::FakeHost;
use crate::host::HostOs;
use crate::run_app::{
    AppProject, Framework, NativeProject, OpenInstalledAndroidApp, PlannedRun, RunPlan, RunRefusal,
    RunStep, TargetPlatform, XcodeContainer,
};
use crate::run_app_detect::detect_app_projects;
use crate::run_app_native::plan_app_run;
use crate::run_app_tests::{device, snapshot};
use crate::test_fixtures::tool_output;
use crate::{Device, DeviceKind};

const ROOT: &str = "/work/open-folder";
const SIMULATOR_UDID: &str = "00000000-0000-4000-8000-000000000005";
const IPHONE_UDID: &str = "00008110-001A2C3E0A88801E";
const SDK: &str = "/home/tester/Android/Sdk";
const XCODEBUILD: &str = "/usr/bin/xcodebuild";
const XCRUN: &str = "/usr/bin/xcrun";
const APP_GRADLE: &str = "plugins {\n    alias(libs.plugins.android.application)\n}\nandroid {\n    defaultConfig { applicationId = \"dev.zodeverify.native1\" }\n    buildTypes { debug { applicationIdSuffix = \".debug\" } }\n}\n";
const LIBRARY_GRADLE: &str = "plugins {\n    id 'com.android.library'\n}\n";

fn root() -> PathBuf {
    PathBuf::from(ROOT)
}

fn add_gradle_project(host: &FakeHost, directory: &Path, wrapper: &str) {
    host.add_file(directory.join(wrapper), "#!/bin/sh\n");
    host.add_file(directory.join("settings.gradle.kts"), "include(\":app\")\n");
    host.add_file(directory.join("app/build.gradle.kts"), APP_GRADLE);
    host.add_file(directory.join("app/src/main/AndroidManifest.xml"), "");
}

fn add_xcode_container(host: &FakeHost, directory: &Path, name: &str) {
    host.add_file(directory.join(name).join("contents"), "");
}

fn found(projects: &[AppProject]) -> Vec<(Framework, PathBuf)> {
    projects
        .iter()
        .map(|project| (project.framework, project.directory.clone()))
        .collect()
}

#[test]
fn a_gradle_project_with_an_application_module_is_found() {
    let host = FakeHost::new(HostOs::MacOs);
    add_gradle_project(&host, &root(), "gradlew");
    host.add_file(root().join("core/build.gradle"), LIBRARY_GRADLE);

    let projects = detect_app_projects(&host, &root());
    assert_eq!(found(&projects), [(Framework::AndroidGradle, root())]);
    let project = &projects[0];
    assert_eq!(
        project.native,
        Some(NativeProject::Gradle {
            app_module: "app".to_string()
        })
    );
    assert!(project.has_android_folder && !project.has_ios_folder);
    assert_eq!(project.display_name, "open-folder");
}

#[test]
fn a_gradle_project_with_only_library_modules_is_not_an_app() {
    let host = FakeHost::new(HostOs::MacOs);
    host.add_file(root().join("gradlew"), "");
    host.add_file(root().join("settings.gradle"), "");
    host.add_file(root().join("lib/build.gradle"), LIBRARY_GRADLE);
    assert!(detect_app_projects(&host, &root()).is_empty());
}

#[test]
fn windows_needs_the_batch_wrapper() {
    let host = FakeHost::new(HostOs::Windows);
    add_gradle_project(&host, &root(), "gradlew");
    assert!(detect_app_projects(&host, &root()).is_empty());

    let host = FakeHost::new(HostOs::Windows);
    add_gradle_project(&host, &root(), "gradlew.bat");
    assert_eq!(
        found(&detect_app_projects(&host, &root())),
        [(Framework::AndroidGradle, root())]
    );
}

#[test]
fn the_native_folders_of_a_flutter_app_belong_to_flutter() {
    let host = FakeHost::new(HostOs::MacOs);
    host.add_file(
        root().join("pubspec.yaml"),
        "name: counter_app\ndependencies:\n  flutter:\n    sdk: flutter\n",
    );
    host.add_file(root().join("lib/main.dart"), "");
    add_gradle_project(&host, &root().join("android"), "gradlew");
    add_xcode_container(&host, &root().join("ios"), "Runner.xcworkspace");
    add_xcode_container(&host, &root().join("ios"), "Runner.xcodeproj");
    assert_eq!(
        found(&detect_app_projects(&host, &root())),
        [(Framework::Flutter, root())]
    );

    // Opening the native folder on its own does not make it an app either.
    let android = root().join("android");
    assert!(detect_app_projects(&host, &android).is_empty());
    assert!(detect_app_projects(&host, &root().join("ios")).is_empty());
}

#[test]
fn a_workspace_wins_over_a_project_and_pods_are_skipped() {
    let host = FakeHost::new(HostOs::MacOs);
    let app = root().join("MyApp");
    add_xcode_container(&host, &app, "Pods.xcworkspace");
    add_xcode_container(&host, &app, "MyApp.xcodeproj");
    add_xcode_container(&host, &app, "MyApp.xcworkspace");
    let pods_only = root().join("PodsOnly");
    add_xcode_container(&host, &pods_only, "Pods.xcodeproj");
    add_xcode_container(&host, &pods_only, "Pods.xcworkspace");
    let package = root().join("SwiftPackage");
    host.add_file(package.join("Package.swift"), "");

    let projects = detect_app_projects(&host, &root());
    assert_eq!(found(&projects), [(Framework::IosXcode, app)]);
    let project = &projects[0];
    assert_eq!(
        project.native,
        Some(NativeProject::Xcode(XcodeContainer {
            file_name: "MyApp.xcworkspace".to_string(),
            is_workspace: true,
        }))
    );
    assert!(project.has_ios_folder && !project.has_android_folder);
    assert_eq!(project.display_name, "MyApp");
}

#[test]
fn xcode_projects_are_only_found_on_a_mac() {
    let host = FakeHost::new(HostOs::Linux);
    add_xcode_container(&host, &root(), "MyApp.xcodeproj");
    assert!(detect_app_projects(&host, &root()).is_empty());
}

fn gradle_project(app_module: &str) -> AppProject {
    AppProject {
        framework: Framework::AndroidGradle,
        directory: PathBuf::from("/work/native"),
        display_name: "native".to_string(),
        has_ios_folder: false,
        has_android_folder: true,
        dependencies_installed: true,
        react_native_cli_version: None,
        react_native_version: None,
        native: Some(NativeProject::Gradle {
            app_module: app_module.to_string(),
        }),
    }
}

fn xcode_project(file_name: &str, is_workspace: bool) -> AppProject {
    AppProject {
        framework: Framework::IosXcode,
        directory: PathBuf::from("/work/ios app"),
        display_name: "MyApp".to_string(),
        has_ios_folder: true,
        has_android_folder: false,
        dependencies_installed: true,
        react_native_cli_version: None,
        react_native_version: None,
        native: Some(NativeProject::Xcode(XcodeContainer {
            file_name: file_name.to_string(),
            is_workspace,
        })),
    }
}

fn android_host(os: HostOs) -> FakeHost {
    let host = FakeHost::new(os);
    host.set_environment_variable("ANDROID_HOME", SDK);
    host.add_file(
        Path::new(SDK)
            .join("platform-tools")
            .join(os.executable_name("adb")),
        "",
    );
    host
}

fn plan_on(
    host: &FakeHost,
    project: &AppProject,
    device: &Device,
    scheme: Option<&str>,
) -> Result<PlannedRun, RunRefusal> {
    smol::block_on(plan_app_run(
        project,
        device,
        &snapshot(std::slice::from_ref(device)),
        host,
        host,
        scheme,
    ))
}

fn ready(result: Result<PlannedRun, RunRefusal>) -> RunPlan {
    match result {
        Ok(PlannedRun::Ready(plan)) => plan,
        other => panic!("expected a plan, got {other:?}"),
    }
}

fn step(program: &str, args: &[&str]) -> RunStep {
    RunStep {
        program: program.to_string(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
    }
}

fn emulator() -> Device {
    device(DeviceKind::AndroidEmulator, "Medium_Phone", "emulator-5556")
}

#[test]
fn gradle_installs_on_the_chosen_device_then_opens_the_id_the_build_wrote() {
    let host = android_host(HostOs::MacOs);
    let plan = ready(plan_on(&host, &gradle_project("app"), &emulator(), None));
    assert_eq!(plan.steps, [step("./gradlew", &[":app:installDebug"])]);
    assert_eq!(
        plan.env,
        [("ANDROID_SERIAL".to_string(), "emulator-5556".to_string())]
    );
    // Not `applicationId` from build.gradle: with `applicationIdSuffix` the
    // installed id differs, and monkey exits 252.
    assert_eq!(
        plan.open_installed_android_app,
        Some(OpenInstalledAndroidApp {
            adb: Path::new(SDK).join("platform-tools/adb"),
            serial: "emulator-5556".to_string(),
            output_metadata: "app/build/outputs/apk/debug/output-metadata.json".to_string(),
        })
    );
    assert!(plan.is_chained());
    assert_eq!(plan.cwd, PathBuf::from("/work/native"));
    assert_eq!(plan.label, "Run native on Medium_Phone");
    assert!(!plan.exclusive_per_project && !plan.needs_metro);
    assert!(host.invocations().is_empty(), "nothing runs while planning");
}

#[test]
fn gradle_on_windows_uses_the_batch_wrapper_and_backslashes() {
    let host = android_host(HostOs::Windows);
    let plan = ready(plan_on(&host, &gradle_project("app"), &emulator(), None));
    assert_eq!(plan.steps, [step(".\\gradlew.bat", &[":app:installDebug"])]);
    let open = plan.open_installed_android_app.expect("opens the app");
    assert_eq!(
        open.output_metadata,
        "app\\build\\outputs\\apk\\debug\\output-metadata.json"
    );
    assert_eq!(
        open.adb,
        Path::new(SDK).join("platform-tools").join("adb.exe")
    );
}

#[test]
fn the_installed_id_is_in_the_metadata_fixture_where_the_plan_reads_it() {
    let metadata = tool_output("gradle-output-metadata-debug.json");
    assert!(metadata.contains("  \"applicationId\": \"dev.zodeverify.native1.debug\",\n"));
}

#[test]
fn gradle_refuses_an_unsafe_module_an_ios_device_and_a_missing_adb() {
    let host = android_host(HostOs::MacOs);
    for module in ["-app", "app$x", "app;rm", "my app"] {
        match plan_on(&host, &gradle_project(module), &emulator(), None) {
            Err(RunRefusal::UnsafeArgument { value, .. }) => assert_eq!(value, module),
            other => panic!("{module:?} gave {other:?}"),
        }
    }
    let simulator = device(DeviceKind::IosSimulator, "iPhone 17", SIMULATOR_UDID);
    assert_eq!(
        plan_on(&host, &gradle_project("app"), &simulator, None),
        Err(RunRefusal::PlatformMissing {
            framework: Framework::AndroidGradle,
            platform: TargetPlatform::Ios,
        })
    );
    let no_sdk = FakeHost::new(HostOs::MacOs);
    let refused = plan_on(&no_sdk, &gradle_project("app"), &emulator(), None);
    assert!(
        matches!(&refused, Err(RunRefusal::Unsupported { reason }) if reason.contains("adb")),
        "{refused:?}"
    );
}

fn simulator() -> Device {
    device(DeviceKind::IosSimulator, "iPhone 17", SIMULATOR_UDID)
}

/// The container path as the plan builds it: joined with the host's separator,
/// so on Windows it is `/work/ios app\MyApp.xcworkspace`.
fn container_path(file_name: &str) -> String {
    Path::new("/work/ios app")
        .join(file_name)
        .to_string_lossy()
        .into_owned()
}

fn list_args() -> Vec<String> {
    [
        "-list",
        "-json",
        "-workspace",
        &container_path("MyApp.xcworkspace"),
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect()
}

fn settings_args(scheme: &str) -> Vec<String> {
    [
        "-showBuildSettings",
        "-json",
        "-workspace",
        &container_path("MyApp.xcworkspace"),
        "-scheme",
        scheme,
        "-configuration",
        "Debug",
        "-destination",
        &format!("id={SIMULATOR_UDID}"),
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect()
}

fn schemes_json(schemes: &[&str]) -> String {
    serde_json::json!({ "workspace": { "name": "MyApp", "schemes": schemes } }).to_string()
}

fn app_target(target: &str, directory: &str, product: &str, bundle_id: &str) -> serde_json::Value {
    serde_json::json!({
        "action": "build",
        "target": target,
        "buildSettings": {
            "TARGET_BUILD_DIR": directory,
            "FULL_PRODUCT_NAME": product,
            "PRODUCT_BUNDLE_IDENTIFIER": bundle_id,
            "WRAPPER_EXTENSION": product.rsplit('.').next().unwrap_or_default(),
        }
    })
}

fn xcodebuild_calls(host: &FakeHost) -> Vec<Vec<String>> {
    host.invocations()
        .into_iter()
        .filter(|invocation| invocation.program == Path::new(XCODEBUILD))
        .map(|invocation| invocation.args)
        .collect()
}

#[test]
fn one_scheme_builds_installs_and_launches_on_the_simulator() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(XCODEBUILD, list_args(), 0, &schemes_json(&["MyApp"]), "");
    host.script_output(
        XCODEBUILD,
        settings_args("MyApp"),
        0,
        &tool_output("xcodebuild-showbuildsettings-native-prebuild.json"),
        "Writing error result bundle to /tmp/x.xcresult\n",
    );
    let plan = ready(plan_on(
        &host,
        &xcode_project("MyApp.xcworkspace", true),
        &simulator(),
        None,
    ));
    let app =
        "/tmp/devices-verify/tmp/dd-native/Build/Products/Debug-iphonesimulator/NativeSample.app";
    let destination = format!("id={SIMULATOR_UDID}");
    assert_eq!(
        plan.steps,
        [
            step(
                XCODEBUILD,
                &[
                    "-workspace",
                    "MyApp.xcworkspace",
                    "-scheme",
                    "MyApp",
                    "-configuration",
                    "Debug",
                    "-destination",
                    &destination,
                    "build",
                ]
            ),
            step(XCRUN, &["simctl", "install", SIMULATOR_UDID, app]),
            step(
                XCRUN,
                &[
                    "simctl",
                    "launch",
                    "--terminate-running-process",
                    SIMULATOR_UDID,
                    "dev.zodeverify.nativeios",
                ]
            ),
        ]
    );
    assert!(plan.is_chained());
    assert_eq!(plan.cwd, PathBuf::from("/work/ios app"));
    let invocations = host.invocations();
    assert_eq!(invocations.len(), 2);
    assert!(
        invocations
            .iter()
            .all(|invocation| invocation.timeout == Duration::from_secs(180))
    );
}

#[test]
fn a_project_file_lists_its_schemes_with_the_project_flag() {
    let host = FakeHost::new(HostOs::MacOs);
    let container = container_path("Runner.xcodeproj");
    host.script_output(
        XCODEBUILD,
        ["-list", "-json", "-project", container.as_str()],
        0,
        &tool_output("xcodebuild-list-project.json"),
        "",
    );
    let mut settings: Vec<String> = settings_args("Runner");
    settings[2] = "-project".to_string();
    settings[3] = container;
    host.script_output(
        XCODEBUILD,
        settings,
        0,
        &serde_json::json!([app_target(
            "Runner",
            "/dd/Debug-iphonesimulator",
            "Runner.app",
            "dev.zodeverify.vsample"
        )])
        .to_string(),
        "",
    );
    let plan = ready(plan_on(
        &host,
        &xcode_project("Runner.xcodeproj", false),
        &simulator(),
        None,
    ));
    assert_eq!(
        plan.steps[0].args[..4],
        ["-project", "Runner.xcodeproj", "-scheme", "Runner"]
    );
}

#[test]
fn several_schemes_ask_and_pods_schemes_are_left_out() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        XCODEBUILD,
        list_args(),
        0,
        &schemes_json(&["MyApp", "Pods-MyApp", "MyApp Staging"]),
        "",
    );
    assert_eq!(
        plan_on(
            &host,
            &xcode_project("MyApp.xcworkspace", true),
            &simulator(),
            None
        ),
        Ok(PlannedRun::ChooseScheme(vec![
            "MyApp".to_string(),
            "MyApp Staging".to_string()
        ]))
    );
    assert_eq!(xcodebuild_calls(&host).len(), 1, "no build settings yet");
}

#[test]
fn a_chosen_scheme_skips_the_list_and_may_hold_spaces() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        XCODEBUILD,
        settings_args("MyApp Staging"),
        0,
        &serde_json::json!([app_target(
            "MyApp Staging",
            "/dd/Debug-iphonesimulator",
            "My App.app",
            "dev.example.staging"
        )])
        .to_string(),
        "",
    );
    let plan = ready(plan_on(
        &host,
        &xcode_project("MyApp.xcworkspace", true),
        &simulator(),
        Some("MyApp Staging"),
    ));
    assert_eq!(plan.steps[0].args[3], "MyApp Staging");
    assert_eq!(
        plan.steps[1].args[3],
        "/dd/Debug-iphonesimulator/My App.app"
    );
    assert_eq!(xcodebuild_calls(&host), [settings_args("MyApp Staging")]);
}

#[test]
fn only_pods_schemes_or_none_is_refused() {
    for schemes in [&["Pods-MyApp"][..], &[][..]] {
        let host = FakeHost::new(HostOs::MacOs);
        host.script_output(XCODEBUILD, list_args(), 0, &schemes_json(schemes), "");
        let refused = plan_on(
            &host,
            &xcode_project("MyApp.xcworkspace", true),
            &simulator(),
            None,
        );
        assert!(
            matches!(&refused, Err(RunRefusal::Unsupported { reason }) if reason.contains("scheme")),
            "{schemes:?} gave {refused:?}"
        );
    }
}

#[test]
fn unsafe_schemes_are_refused_before_xcodebuild_reads_them() {
    for scheme in ["-quiet", "a$b", "a%b", "a`id`", "a\"b"] {
        let host = FakeHost::new(HostOs::MacOs);
        host.script_output(XCODEBUILD, list_args(), 0, &schemes_json(&[scheme]), "");
        match plan_on(
            &host,
            &xcode_project("MyApp.xcworkspace", true),
            &simulator(),
            None,
        ) {
            Err(RunRefusal::UnsafeArgument {
                what,
                value,
                reason,
            }) => {
                assert_eq!((what, value.as_str()), ("scheme", scheme));
                assert!(!reason.is_empty());
            }
            other => panic!("{scheme:?} gave {other:?}"),
        }
        assert_eq!(
            xcodebuild_calls(&host).len(),
            1,
            "{scheme:?} reached -showBuildSettings"
        );

        let host = FakeHost::new(HostOs::MacOs);
        let chosen = plan_on(
            &host,
            &xcode_project("MyApp.xcworkspace", true),
            &simulator(),
            Some(scheme),
        );
        assert!(matches!(chosen, Err(RunRefusal::UnsafeArgument { .. })));
        assert!(xcodebuild_calls(&host).is_empty());
    }
}

#[test]
fn an_unsafe_container_name_is_refused_before_anything_runs() {
    let host = FakeHost::new(HostOs::MacOs);
    let refused = plan_on(
        &host,
        &xcode_project("My$App.xcodeproj", false),
        &simulator(),
        None,
    );
    assert!(
        matches!(refused, Err(RunRefusal::UnsafeArgument { .. })),
        "{refused:?}"
    );
    assert!(host.invocations().is_empty());
}

#[test]
fn the_app_target_named_like_the_scheme_wins_and_ambiguity_is_refused() {
    let two_apps = serde_json::json!([
        app_target("Widget", "/dd", "Widget.appex", "dev.example.widget"),
        app_target("Other", "/dd", "Other.app", "dev.example.other"),
        app_target("MyApp", "/dd", "MyApp.app", "dev.example.app"),
    ])
    .to_string();
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(XCODEBUILD, settings_args("MyApp"), 0, &two_apps, "");
    let plan = ready(plan_on(
        &host,
        &xcode_project("MyApp.xcworkspace", true),
        &simulator(),
        Some("MyApp"),
    ));
    assert_eq!(plan.steps[2].args[4], "dev.example.app");

    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(XCODEBUILD, settings_args("Suite"), 0, &two_apps, "");
    let refused = plan_on(
        &host,
        &xcode_project("MyApp.xcworkspace", true),
        &simulator(),
        Some("Suite"),
    );
    assert!(
        matches!(&refused, Err(RunRefusal::Unsupported { reason })
            if reason.contains("Other") && reason.contains("MyApp") && !reason.contains("Widget")),
        "{refused:?}"
    );
}

#[test]
fn an_app_path_or_bundle_id_a_shell_would_rewrite_is_refused() {
    for (directory, bundle_id) in [
        ("/dd/`id`", "dev.example.app"),
        ("/dd/it's", "dev.example.app"),
        ("/dd", "dev.example;reboot"),
        ("/dd", "-dev"),
    ] {
        let host = FakeHost::new(HostOs::MacOs);
        host.script_output(
            XCODEBUILD,
            settings_args("MyApp"),
            0,
            &serde_json::json!([app_target("MyApp", directory, "MyApp.app", bundle_id)])
                .to_string(),
            "",
        );
        let refused = plan_on(
            &host,
            &xcode_project("MyApp.xcworkspace", true),
            &simulator(),
            Some("MyApp"),
        );
        assert!(
            matches!(refused, Err(RunRefusal::UnsafeArgument { .. })),
            "{directory} {bundle_id}: {refused:?}"
        );
    }
}

#[test]
fn a_physical_iphone_is_refused_before_xcodebuild_runs() {
    let host = FakeHost::new(HostOs::MacOs);
    let iphone = device(DeviceKind::IosPhysical, "iPhone", IPHONE_UDID);
    let refused = plan_on(
        &host,
        &xcode_project("MyApp.xcworkspace", true),
        &iphone,
        None,
    );
    assert!(
        matches!(&refused, Err(RunRefusal::Unsupported { reason }) if reason.contains("simulator") && reason.contains("sign")),
        "{refused:?}"
    );
    assert!(host.invocations().is_empty());
}

#[test]
fn a_failed_probe_is_reported_with_xcodebuilds_message() {
    let host = FakeHost::new(HostOs::MacOs);
    host.script_output(
        XCODEBUILD,
        list_args(),
        74,
        "",
        "xcodebuild: error: 'MyApp.xcworkspace' does not exist.\n",
    );
    let refused = plan_on(
        &host,
        &xcode_project("MyApp.xcworkspace", true),
        &simulator(),
        None,
    );
    assert!(
        matches!(&refused, Err(RunRefusal::ProjectToolFailed { tool: "xcodebuild", message })
            if message.contains("does not exist")),
        "{refused:?}"
    );
}

#[test]
fn flutter_expo_and_react_native_still_plan_through_the_same_entry() {
    let host = FakeHost::new(HostOs::MacOs);
    let mut project = xcode_project("unused", true);
    project.framework = Framework::Flutter;
    project.native = None;
    let plan = ready(plan_on(&host, &project, &simulator(), None));
    assert_eq!(
        plan.steps,
        [step("flutter", &["run", "-d", SIMULATOR_UDID])]
    );
    assert!(!plan.is_chained());
    assert!(host.invocations().is_empty());
}

#[test]
fn a_preamble_line_with_brackets_is_skipped_before_the_json() {
    let host = FakeHost::new(HostOs::MacOs);
    let preamble = "xcodebuild[1234:5678] Requested but did not find extension point\n\
                    --- xcodebuild: WARNING: Using the first of multiple matching destinations:\n\
                    { platform:iOS Simulator, id:00000000-0000-4000-8000-000000000005 }\n";
    host.script_output(
        XCODEBUILD,
        list_args(),
        0,
        &format!("{preamble}{}", schemes_json(&["MyApp"])),
        "",
    );
    host.script_output(
        XCODEBUILD,
        settings_args("MyApp"),
        0,
        &format!(
            "{preamble}{}",
            serde_json::json!([app_target(
                "MyApp",
                "/dd/Debug-iphonesimulator",
                "MyApp.app",
                "dev.zodeverify.app"
            )])
        ),
        "",
    );
    let plan = ready(plan_on(
        &host,
        &xcode_project("MyApp.xcworkspace", true),
        &simulator(),
        None,
    ));
    assert_eq!(
        plan.steps[1],
        step(
            XCRUN,
            &[
                "simctl",
                "install",
                SIMULATOR_UDID,
                "/dd/Debug-iphonesimulator/MyApp.app"
            ]
        )
    );
}
