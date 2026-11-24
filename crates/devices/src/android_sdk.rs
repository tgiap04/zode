use std::path::{Path, PathBuf};

use crate::host::{HostOs, HostSystem};

/// The tools of one Android SDK, every path absolute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AndroidSdk {
    pub root: PathBuf,
    pub adb: PathBuf,
    /// `None` when the "Android Emulator" package is not installed.
    pub emulator: Option<PathBuf>,
    pub avd_home: Option<PathBuf>,
}

/// Looks only at Zode's environment, the default install directory and the
/// login shell's `PATH`. A project's `local.properties` or `.envrc` is never
/// read: listing devices must not depend on, or run, anything in a project.
pub fn locate_android_sdk(host: &dyn HostSystem) -> Option<AndroidSdk> {
    let os = host.os();
    let adb_name = os.executable_name("adb");
    let from_environment = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .into_iter()
        .filter_map(|name| absolute_path_from(host, name));
    let root = from_environment
        .chain(default_sdk_root(host, os))
        .find(|root| host.is_file(&root.join("platform-tools").join(&adb_name)))
        .or_else(|| sdk_root_above_adb_on_path(host))?;

    let adb = root.join("platform-tools").join(&adb_name);
    let emulator = root.join("emulator").join(os.executable_name("emulator"));
    Some(AndroidSdk {
        emulator: host.is_file(&emulator).then_some(emulator),
        avd_home: avd_home(host),
        adb,
        root,
    })
}

fn absolute_path_from(host: &dyn HostSystem, variable: &str) -> Option<PathBuf> {
    let value = PathBuf::from(host.environment_variable(variable)?);
    is_absolute_for(&value).then_some(value)
}

/// `Path::is_absolute` follows the OS the code runs on, so a Windows path seen
/// by a Mac test would be called relative. A drive letter counts as absolute.
fn is_absolute_for(path: &Path) -> bool {
    if crate::host::is_rooted(path) {
        return true;
    }
    let text = path.to_string_lossy();
    let mut characters = text.chars();
    matches!(
        (characters.next(), characters.next(), characters.next()),
        (Some(drive), Some(':'), Some('\\' | '/')) if drive.is_ascii_alphabetic()
    )
}

fn default_sdk_root(host: &dyn HostSystem, os: HostOs) -> Option<PathBuf> {
    match os {
        HostOs::MacOs => Some(host.home_dir()?.join("Library").join("Android").join("sdk")),
        HostOs::Linux => Some(host.home_dir()?.join("Android").join("Sdk")),
        HostOs::Windows => {
            let local_app_data = absolute_path_from(host, "LOCALAPPDATA")
                .or_else(|| Some(host.home_dir()?.join("AppData").join("Local")))?;
            Some(local_app_data.join("Android").join("Sdk"))
        }
    }
}

fn sdk_root_above_adb_on_path(host: &dyn HostSystem) -> Option<PathBuf> {
    let adb = host.find_on_path("adb")?;
    let platform_tools = adb.parent()?;
    if platform_tools.file_name()? != "platform-tools" {
        return None;
    }
    let root = platform_tools.parent()?.to_path_buf();
    is_absolute_for(&root).then_some(root)
}

fn avd_home(host: &dyn HostSystem) -> Option<PathBuf> {
    absolute_path_from(host, "ANDROID_AVD_HOME")
        .or_else(|| Some(absolute_path_from(host, "ANDROID_SDK_HOME")?.join("avd")))
        .or_else(|| Some(host.home_dir()?.join(".android").join("avd")))
}
