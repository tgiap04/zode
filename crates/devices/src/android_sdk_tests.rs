use std::path::{Path, PathBuf};

use crate::android_sdk::locate_android_sdk;
use crate::fake_host::FakeHost;
use crate::host::HostOs;

fn install_sdk(host: &FakeHost, root: &Path, os: HostOs, with_emulator: bool) {
    host.add_file(
        root.join("platform-tools").join(os.executable_name("adb")),
        "",
    );
    if with_emulator {
        host.add_file(
            root.join("emulator").join(os.executable_name("emulator")),
            "",
        );
    }
}

#[test]
fn android_home_comes_before_sdk_root_and_the_default_directory() {
    let host = FakeHost::new(HostOs::MacOs);
    let home_sdk = PathBuf::from("/sdks/home");
    let root_sdk = PathBuf::from("/sdks/root");
    let default_sdk = PathBuf::from("/home/tester/Library/Android/sdk");
    for root in [&home_sdk, &root_sdk, &default_sdk] {
        install_sdk(&host, root, HostOs::MacOs, true);
    }
    host.set_environment_variable("ANDROID_HOME", "/sdks/home");
    host.set_environment_variable("ANDROID_SDK_ROOT", "/sdks/root");

    let sdk = locate_android_sdk(&host).expect("an SDK is found");
    assert_eq!(sdk.root, home_sdk);
    assert_eq!(sdk.adb, home_sdk.join("platform-tools/adb"));
    assert_eq!(sdk.emulator, Some(home_sdk.join("emulator/emulator")));
}

#[test]
fn an_android_home_without_adb_falls_through_to_sdk_root() {
    let host = FakeHost::new(HostOs::Linux);
    install_sdk(&host, Path::new("/sdks/root"), HostOs::Linux, true);
    host.set_environment_variable("ANDROID_HOME", "/sdks/empty");
    host.set_environment_variable("ANDROID_SDK_ROOT", "/sdks/root");

    let sdk = locate_android_sdk(&host).expect("an SDK is found");
    assert_eq!(sdk.root, PathBuf::from("/sdks/root"));
}

#[test]
fn a_relative_sdk_variable_is_ignored() {
    let host = FakeHost::new(HostOs::Linux);
    install_sdk(&host, Path::new("sdk"), HostOs::Linux, true);
    host.set_environment_variable("ANDROID_HOME", "sdk");

    assert_eq!(locate_android_sdk(&host), None);
}

#[test]
fn each_host_os_has_its_own_default_directory() {
    let mac = FakeHost::new(HostOs::MacOs);
    install_sdk(
        &mac,
        Path::new("/home/tester/Library/Android/sdk"),
        HostOs::MacOs,
        true,
    );
    assert_eq!(
        locate_android_sdk(&mac).map(|sdk| sdk.root),
        Some(PathBuf::from("/home/tester/Library/Android/sdk"))
    );

    let linux = FakeHost::new(HostOs::Linux);
    install_sdk(
        &linux,
        Path::new("/home/tester/Android/Sdk"),
        HostOs::Linux,
        true,
    );
    assert_eq!(
        locate_android_sdk(&linux).map(|sdk| sdk.root),
        Some(PathBuf::from("/home/tester/Android/Sdk"))
    );

    let windows = FakeHost::new(HostOs::Windows);
    let local_app_data = PathBuf::from("C:\\Users\\tester\\AppData\\Local");
    let windows_root = local_app_data.join("Android").join("Sdk");
    install_sdk(&windows, &windows_root, HostOs::Windows, true);
    windows.set_environment_variable("LOCALAPPDATA", "C:\\Users\\tester\\AppData\\Local");
    let sdk = locate_android_sdk(&windows).expect("the Windows default is found");
    assert_eq!(sdk.root, windows_root);
    assert_eq!(sdk.adb, windows_root.join("platform-tools").join("adb.exe"));
    assert_eq!(
        sdk.emulator,
        Some(windows_root.join("emulator").join("emulator.exe"))
    );
}

#[test]
fn the_mac_default_is_not_used_on_linux() {
    let linux = FakeHost::new(HostOs::Linux);
    install_sdk(
        &linux,
        Path::new("/home/tester/Library/Android/sdk"),
        HostOs::Linux,
        true,
    );
    assert_eq!(locate_android_sdk(&linux), None);
}

#[test]
fn adb_on_the_login_shell_path_gives_the_sdk_above_platform_tools() {
    let host = FakeHost::new(HostOs::MacOs);
    install_sdk(&host, Path::new("/opt/android"), HostOs::MacOs, false);
    host.set_on_path("adb", "/opt/android/platform-tools/adb");

    let sdk = locate_android_sdk(&host).expect("adb on PATH is used");
    assert_eq!(sdk.root, PathBuf::from("/opt/android"));
    assert_eq!(sdk.adb, PathBuf::from("/opt/android/platform-tools/adb"));
    assert_eq!(sdk.emulator, None, "no emulator directory was installed");
}

#[test]
fn nothing_found_is_none_and_no_project_file_is_read() {
    let host = FakeHost::new(HostOs::MacOs);
    host.add_file(
        "/work/project/local.properties",
        "sdk.dir=/sdks/from-project\n",
    );
    install_sdk(&host, Path::new("/sdks/from-project"), HostOs::MacOs, true);

    assert_eq!(locate_android_sdk(&host), None);
    assert!(host.reads().is_empty(), "read {:?}", host.reads());
    assert!(host.invocations().is_empty());
}

#[test]
fn the_avd_home_follows_its_own_variables_then_the_user_directory() {
    let host = FakeHost::new(HostOs::Linux);
    install_sdk(
        &host,
        Path::new("/home/tester/Android/Sdk"),
        HostOs::Linux,
        true,
    );
    assert_eq!(
        locate_android_sdk(&host).and_then(|sdk| sdk.avd_home),
        Some(PathBuf::from("/home/tester/.android/avd"))
    );

    host.set_environment_variable("ANDROID_SDK_HOME", "/sdk-home");
    assert_eq!(
        locate_android_sdk(&host).and_then(|sdk| sdk.avd_home),
        Some(PathBuf::from("/sdk-home/avd"))
    );

    host.set_environment_variable("ANDROID_AVD_HOME", "/avds");
    assert_eq!(
        locate_android_sdk(&host).and_then(|sdk| sdk.avd_home),
        Some(PathBuf::from("/avds"))
    );
}
