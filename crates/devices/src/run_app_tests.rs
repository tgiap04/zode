use std::path::PathBuf;

use crate::host::HostOs;
use crate::run_app::{
    AppProject, Framework, PackageVersion, RunConfirmation, RunPlan, RunRefusal, RunStep,
    TargetPlatform, plan_metro, plan_run,
};
use crate::run_app_cli_frameworks::react_native_android_device_flag;
use crate::test_fixtures::tool_output;
use crate::{
    Device, DeviceKey, DeviceKind, DeviceSnapshot, DeviceState, MemoryStatus, SectionState,
};

const SIMULATOR_UDID: &str = "00000000-0000-4000-8000-000000000005";
const IPHONE_UDID: &str = "00008110-001A2C3E0A88801E";

pub(crate) fn device(kind: DeviceKind, name: &str, run_id: &str) -> Device {
    let key = match kind {
        DeviceKind::IosSimulator => DeviceKey::Simulator(run_id.to_string()),
        DeviceKind::IosPhysical => DeviceKey::IosDevice(run_id.to_string()),
        DeviceKind::AndroidEmulator => DeviceKey::Avd(name.to_string()),
        DeviceKind::AndroidUsb | DeviceKind::AndroidWireless => {
            DeviceKey::AndroidSerial(run_id.to_string())
        }
    };
    Device {
        key,
        name: name.to_string(),
        kind,
        state: DeviceState::Running,
        detail: None,
        family: None,
        serial: None,
        run_id: Some(run_id.to_string()),
        experimental: false,
    }
}

fn simulator() -> Device {
    device(DeviceKind::IosSimulator, "iPhone 17", SIMULATOR_UDID)
}

fn emulator() -> Device {
    device(DeviceKind::AndroidEmulator, "Medium_Phone", "emulator-5554")
}

pub(crate) fn snapshot(devices: &[Device]) -> DeviceSnapshot {
    let section = |platform: TargetPlatform| {
        SectionState::Ready(
            devices
                .iter()
                .filter(|device| TargetPlatform::of(device.kind) == platform)
                .cloned()
                .collect(),
        )
    };
    DeviceSnapshot {
        ios_simulators: section(TargetPlatform::Ios),
        android_emulators: section(TargetPlatform::Android),
        physical_ios: SectionState::Ready(Vec::new()),
        physical_android: SectionState::Ready(Vec::new()),
        memory: MemoryStatus {
            running_count: 0,
            available_bytes: None,
            low: false,
        },
    }
}

fn project(framework: Framework) -> AppProject {
    AppProject {
        framework,
        directory: PathBuf::from("/work/app"),
        display_name: "counter_app".to_string(),
        has_ios_folder: true,
        has_android_folder: true,
        dependencies_installed: true,
        react_native_cli_version: Some(PackageVersion::new(20, 2, 0)),
        react_native_version: Some(PackageVersion::new(0, 88, 1)),
        native: None,
    }
}

fn plan(project: &AppProject, device: &Device, host_os: HostOs) -> Result<RunPlan, RunRefusal> {
    plan_run(
        project,
        device,
        &snapshot(std::slice::from_ref(device)),
        host_os,
    )
}

fn argv(plan: &RunPlan) -> Vec<String> {
    plan.steps
        .iter()
        .flat_map(|step| std::iter::once(step.program.clone()).chain(step.args.clone()))
        .collect()
}

fn words(text: &str) -> Vec<String> {
    text.split(' ').map(str::to_string).collect()
}

#[test]
fn each_framework_and_platform_gets_its_own_command_line() {
    let iphone = device(DeviceKind::IosPhysical, "iPhone", IPHONE_UDID);
    let usb = device(DeviceKind::AndroidUsb, "Pixel 9", "R5CT1234ABC");
    let wireless = device(
        DeviceKind::AndroidWireless,
        "Pixel 9",
        "adb-R5CT1234ABC-x1y2z3._adb-tls-connect._tcp",
    );
    let cases = [
        (
            Framework::Flutter,
            simulator(),
            format!("flutter run -d {SIMULATOR_UDID}"),
        ),
        (
            Framework::Flutter,
            iphone.clone(),
            format!("flutter run -d {IPHONE_UDID}"),
        ),
        (
            Framework::Flutter,
            emulator(),
            "flutter run -d emulator-5554".to_string(),
        ),
        (
            Framework::Flutter,
            wireless,
            "flutter run -d adb-R5CT1234ABC-x1y2z3._adb-tls-connect._tcp".to_string(),
        ),
        (
            Framework::Expo,
            simulator(),
            format!("npx --no-install expo run:ios -d {SIMULATOR_UDID}"),
        ),
        (
            Framework::Expo,
            usb.clone(),
            "npx --no-install expo run:android -d R5CT1234ABC".to_string(),
        ),
        (
            Framework::ReactNative,
            iphone,
            format!("npx --no-install react-native run-ios --udid {IPHONE_UDID} --no-packager"),
        ),
        (
            Framework::ReactNative,
            emulator(),
            "npx --no-install react-native run-android --device emulator-5554 --no-packager"
                .to_string(),
        ),
        (
            Framework::ReactNative,
            usb,
            "npx --no-install react-native run-android --device R5CT1234ABC --no-packager"
                .to_string(),
        ),
    ];
    for (framework, device, expected) in cases {
        let plan = plan(&project(framework), &device, HostOs::MacOs)
            .unwrap_or_else(|refusal| panic!("{framework:?} on {:?}: {refusal}", device.kind));
        assert_eq!(
            argv(&plan),
            words(&expected),
            "{framework:?} {:?}",
            device.kind
        );
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.cwd, PathBuf::from("/work/app"));
        assert!(plan.env.is_empty());
        let args = argv(&plan);
        assert!(!args.iter().any(|arg| arg == "--port" || arg == "-p"));
        assert!(!args.iter().any(|arg| arg == "--list-devices"));
    }
}

#[test]
fn every_option_used_is_in_the_recorded_help() {
    let checks = [
        (Framework::Expo, simulator(), "expo-run-ios-help.txt"),
        (Framework::Expo, emulator(), "expo-run-android-help.txt"),
        (Framework::ReactNative, simulator(), "rn-run-ios-help.txt"),
        (
            Framework::ReactNative,
            emulator(),
            "rn-run-android-help.txt",
        ),
    ];
    for (framework, device, help_file) in checks {
        let help = tool_output(help_file);
        let plan = plan(&project(framework), &device, HostOs::MacOs).expect("plans");
        let step = &plan.steps[0];
        let options = step
            .args
            .iter()
            .skip_while(|arg| !arg.starts_with("run"))
            .filter(|arg| arg.starts_with('-'));
        for option in options {
            assert!(
                help.contains(&format!("{option} ")) || help.contains(&format!("{option},")),
                "{option} is not in {help_file}"
            );
        }
    }
}

#[test]
fn windows_runs_the_cmd_and_bat_launchers() {
    let expo = plan(&project(Framework::Expo), &emulator(), HostOs::Windows).expect("plans");
    assert_eq!(expo.steps[0].program, "npx.cmd");
    let flutter = plan(&project(Framework::Flutter), &emulator(), HostOs::Windows).expect("plans");
    assert_eq!(flutter.steps[0].program, "flutter.bat");
    let metro = plan_metro(&project(Framework::ReactNative), HostOs::Windows).expect("metro");
    assert_eq!(metro.steps[0].program, "npx.cmd");
    let linux = plan(&project(Framework::Flutter), &emulator(), HostOs::Linux).expect("plans");
    assert_eq!(linux.steps[0].program, "flutter");
}

#[test]
fn react_native_starts_metro_as_a_separate_task() {
    let react_native = project(Framework::ReactNative);
    let run = plan(&react_native, &simulator(), HostOs::MacOs).expect("plans");
    assert!(run.needs_metro);
    let metro = plan_metro(&react_native, HostOs::MacOs).expect("metro");
    assert_eq!(
        metro.steps,
        vec![RunStep {
            program: "npx".to_string(),
            args: words("--no-install react-native start"),
        }]
    );
    assert_eq!(metro.label, "Metro for counter_app");
    assert_eq!(metro.cwd, react_native.directory);
    assert!(metro.exclusive_per_project);

    for framework in [Framework::Expo, Framework::Flutter] {
        assert_eq!(plan_metro(&project(framework), HostOs::MacOs), None);
        let run = plan(&project(framework), &simulator(), HostOs::MacOs).expect("plans");
        assert!(!run.needs_metro, "{framework:?}");
    }
}

#[test]
fn only_metro_frameworks_are_exclusive_per_project() {
    for (framework, exclusive) in [
        (Framework::Flutter, false),
        (Framework::Expo, true),
        (Framework::ReactNative, true),
    ] {
        let run = plan(&project(framework), &emulator(), HostOs::MacOs).expect("plans");
        assert_eq!(run.exclusive_per_project, exclusive, "{framework:?}");
        assert_eq!(run.label, "Run counter_app on Medium_Phone");
    }
}

#[test]
fn a_device_missing_from_the_fresh_list_is_refused() {
    let stale = emulator();
    let result = plan_run(
        &project(Framework::ReactNative),
        &stale,
        &snapshot(&[simulator()]),
        HostOs::MacOs,
    );
    assert_eq!(
        result,
        Err(RunRefusal::DeviceNotFound {
            device_name: "Medium_Phone".to_string()
        })
    );
}

#[test]
fn a_device_that_is_not_running_now_is_refused() {
    let clicked = simulator();
    for (state, run_id) in [
        (DeviceState::Shutdown, Some(SIMULATOR_UDID)),
        (DeviceState::Starting, Some(SIMULATOR_UDID)),
        (DeviceState::Stopping, Some(SIMULATOR_UDID)),
        (DeviceState::Running, None),
    ] {
        let mut fresh = clicked.clone();
        fresh.state = state.clone();
        fresh.run_id = run_id.map(str::to_string);
        let result = plan_run(
            &project(Framework::Flutter),
            &clicked,
            &snapshot(&[fresh]),
            HostOs::MacOs,
        );
        assert_eq!(
            result,
            Err(RunRefusal::DeviceNotReady {
                device_name: "iPhone 17".to_string()
            }),
            "{state:?}"
        );
    }
}

#[test]
fn run_ids_that_a_shell_or_cli_would_rewrite_are_refused() {
    let android = [
        "-h",
        "--help",
        "$HOME",
        "%PATH%",
        "emulator-5554;rm",
        "emulator 5554",
        "[::1]:5555",
        "emu`id`",
    ];
    let ios = ["-00000000-0000-4000-8000-000000000005", "00008110-$X"];
    let cases = android
        .iter()
        .map(|id| device(DeviceKind::AndroidUsb, "Pixel", id))
        .chain(
            ios.iter()
                .map(|id| device(DeviceKind::IosSimulator, "iPhone", id)),
        );
    for device in cases {
        let run_id = device.run_id.clone().unwrap_or_default();
        for framework in [Framework::Flutter, Framework::Expo, Framework::ReactNative] {
            match plan(&project(framework), &device, HostOs::MacOs) {
                Err(RunRefusal::UnsafeArgument { value, reason, .. }) => {
                    assert_eq!(value, run_id);
                    assert!(!reason.is_empty());
                }
                other => panic!("{run_id} for {framework:?} gave {other:?}"),
            }
        }
    }
}

#[test]
fn a_missing_native_folder_refuses_flutter_and_react_native() {
    let mut no_ios = project(Framework::Flutter);
    no_ios.has_ios_folder = false;
    assert_eq!(
        plan(&no_ios, &simulator(), HostOs::MacOs),
        Err(RunRefusal::PlatformMissing {
            framework: Framework::Flutter,
            platform: TargetPlatform::Ios,
        })
    );
    assert!(plan(&no_ios, &emulator(), HostOs::MacOs).is_ok());

    let mut no_android = project(Framework::ReactNative);
    no_android.has_android_folder = false;
    assert_eq!(
        plan(&no_android, &emulator(), HostOs::MacOs),
        Err(RunRefusal::PlatformMissing {
            framework: Framework::ReactNative,
            platform: TargetPlatform::Android,
        })
    );
}

#[test]
fn expo_without_the_native_folder_asks_before_prebuild() {
    let mut expo = project(Framework::Expo);
    expo.has_android_folder = false;
    let android = plan(&expo, &emulator(), HostOs::MacOs).expect("plans");
    let confirmation = android.confirmation.expect("asks first");
    assert_eq!(
        confirmation,
        RunConfirmation::ExpoPrebuildCreatesNativeFolders {
            platform: TargetPlatform::Android
        }
    );
    let message = confirmation.message();
    assert!(message.contains("android") && message.contains("app.json"));
    assert!(message.contains("package.json"));

    let ios = plan(&expo, &simulator(), HostOs::MacOs).expect("plans");
    assert_eq!(ios.confirmation, None);
}

#[test]
fn node_frameworks_need_their_packages_installed() {
    for framework in [Framework::Expo, Framework::ReactNative] {
        let mut missing = project(framework);
        missing.dependencies_installed = false;
        assert_eq!(
            plan(&missing, &simulator(), HostOs::MacOs),
            Err(RunRefusal::DependenciesNotInstalled {
                framework,
                directory: PathBuf::from("/work/app"),
            })
        );
    }
}

#[test]
fn the_project_name_reaches_the_label_but_never_the_command_line() {
    let mut hostile = project(Framework::Expo);
    hostile.display_name = "$(rm -rf ~) --port 1".to_string();
    let run = plan(&hostile, &simulator(), HostOs::MacOs).expect("plans");
    assert_eq!(run.label, "Run $(rm -rf ~) --port 1 on iPhone 17");
    assert!(argv(&run).iter().all(|arg| !arg.contains("rm")));
}

#[test]
fn the_android_device_flag_follows_the_cli_version() {
    let version = PackageVersion::new;
    let cases = [
        (Some(version(12, 3, 6)), None, "--deviceId"),
        (Some(version(13, 0, 0)), None, "--device"),
        (Some(version(20, 2, 0)), None, "--device"),
        (None, None, "--device"),
        (None, Some(version(0, 73, 6)), "--deviceId"),
        (None, Some(version(0, 74, 0)), "--device"),
        (Some(version(13, 0, 0)), Some(version(0, 73, 6)), "--device"),
    ];
    for (cli, react_native, expected) in cases {
        assert_eq!(
            react_native_android_device_flag(cli, react_native),
            expected,
            "cli {cli:?}, react-native {react_native:?}"
        );
    }

    let mut old = project(Framework::ReactNative);
    old.react_native_cli_version = Some(version(12, 3, 6));
    let run = plan(&old, &emulator(), HostOs::MacOs).expect("plans");
    assert_eq!(
        argv(&run),
        words("npx --no-install react-native run-android --deviceId emulator-5554 --no-packager")
    );
}
