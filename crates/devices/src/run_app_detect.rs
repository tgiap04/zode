//! Finds the apps in an open folder by reading their project files. Nothing
//! is executed: Expo's `app.config.ts` counts by its name alone.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::host::HostSystem;
use crate::run_app::{AppProject, Framework, PackageVersion};
use crate::run_app_native::native_app;

pub const MAX_PROJECT_FILE_BYTES: u64 = 1024 * 1024;

pub(crate) const SKIPPED_DIRECTORIES: &[&str] =
    &["node_modules", "Pods", "build", "DerivedData", "target"];

const NATIVE_FOLDERS: &[&str] = &["ios", "android"];

const EXPO_CONFIG_FILES: &[&str] = &[
    "app.json",
    "app.config.js",
    "app.config.ts",
    "app.config.mjs",
    "app.config.cjs",
    "app.config.json",
];

/// Looks at `root` and its direct children only, so opening a home folder
/// stays cheap. The one exception is a Flutter plugin's `example/` app.
/// Results are the root first, then children by name.
pub fn detect_app_projects(host: &dyn HostSystem, root: &Path) -> Vec<AppProject> {
    detect_app_projects_excluding(host, root, &[])
}

/// Like [`detect_app_projects`], but never reads below `excluded`: open
/// folders inside `root` that are not trusted.
pub fn detect_app_projects_excluding(
    host: &dyn HostSystem,
    root: &Path,
    excluded: &[PathBuf],
) -> Vec<AppProject> {
    // A restricted folder around `root` says nothing about `root`, which was
    // found trusted on its own; only folders inside it are skipped.
    let excluded: Vec<PathBuf> = excluded
        .iter()
        .filter(|folder| folder.starts_with(root) && folder.as_path() != root)
        .cloned()
        .collect();
    let excluded = excluded.as_slice();
    let mut projects = Vec::new();
    let root_is_app = detect_into(host, root, root, excluded, &mut projects);
    let Ok(mut names) = host.read_dir_names(root) else {
        return projects;
    };
    names.sort();
    for name in names {
        let skipped = name.starts_with('.')
            || SKIPPED_DIRECTORIES.contains(&name.as_str())
            // The app at the root owns these; they are not apps of their own.
            || (root_is_app && NATIVE_FOLDERS.contains(&name.as_str()));
        let child = root.join(&name);
        let skipped = skipped || is_link_or_excluded(host, &child, excluded);
        if !skipped && host.is_dir(&child) {
            detect_into(host, root, &child, excluded, &mut projects);
        }
    }
    projects
}

/// A link may lead out of the trusted folder, so it is never followed.
pub(crate) fn is_link_or_excluded(
    host: &dyn HostSystem,
    path: &Path,
    excluded: &[PathBuf],
) -> bool {
    host.is_symlink(path) || is_excluded(path, excluded)
}

fn is_excluded(path: &Path, excluded: &[PathBuf]) -> bool {
    excluded.iter().any(|folder| path.starts_with(folder))
}

/// Returns whether `directory` itself is an app. Nothing below `excluded` is
/// read, including a plugin's `example/` or a Gradle module.
fn detect_into(
    host: &dyn HostSystem,
    root: &Path,
    directory: &Path,
    excluded: &[PathBuf],
    projects: &mut Vec<AppProject>,
) -> bool {
    let mut push = |project: AppProject| {
        if !projects
            .iter()
            .any(|known| known.directory == project.directory)
        {
            projects.push(project);
        }
    };
    let pubspec = read_pubspec(host, directory);
    if let Some(pubspec) = &pubspec
        && let Some(project) = flutter_app(host, directory, pubspec)
    {
        push(project);
        return true;
    }
    if let Some(project) = node_app(host, root, directory) {
        push(project);
        return true;
    }
    if let Some(project) = native_app(host, directory, excluded) {
        push(project);
        return true;
    }
    let example = directory.join("example");
    if pubspec.is_some_and(|pubspec| pubspec.depends_on_flutter)
        && !is_link_or_excluded(host, &example, excluded)
        && let Some(pubspec) = read_pubspec(host, &example)
        && let Some(project) = flutter_app(host, &example, &pubspec)
    {
        push(project);
    }
    false
}

fn flutter_app(host: &dyn HostSystem, directory: &Path, pubspec: &Pubspec) -> Option<AppProject> {
    let has_ios_folder = host.is_dir(&directory.join("ios"));
    let has_android_folder = host.is_dir(&directory.join("android"));
    let is_app = pubspec.depends_on_flutter
        && (has_ios_folder || has_android_folder)
        && host.is_file(&directory.join("lib").join("main.dart"));
    is_app.then(|| AppProject {
        framework: Framework::Flutter,
        directory: directory.to_path_buf(),
        display_name: display_name(pubspec.name.as_deref(), directory),
        has_ios_folder,
        has_android_folder,
        dependencies_installed: true,
        react_native_cli_version: None,
        react_native_version: None,
        native: None,
    })
}

fn node_app(host: &dyn HostSystem, root: &Path, directory: &Path) -> Option<AppProject> {
    let text = read_project_file(host, &directory.join("package.json"))?;
    let package: PackageJson = serde_json::from_str(&text).ok()?;
    let depends_on = |name: &str| {
        package.dependencies.contains_key(name) || package.dev_dependencies.contains_key(name)
    };
    let has_ios_folder = host.is_dir(&directory.join("ios"));
    let has_android_folder = host.is_dir(&directory.join("android"));
    let has_expo_config = EXPO_CONFIG_FILES
        .iter()
        .any(|file| host.is_file(&directory.join(file)));
    let framework = if depends_on("expo") && has_expo_config {
        Framework::Expo
    } else if depends_on("react-native") && (has_ios_folder || has_android_folder) {
        Framework::ReactNative
    } else {
        return None;
    };

    // A workspace may hoist packages to the opened folder.
    let module_folders: Vec<PathBuf> = [directory, root]
        .iter()
        .map(|folder| folder.join("node_modules"))
        .collect();
    let installed_version = |package: &str| {
        module_folders.iter().find_map(|folder| {
            let manifest = folder.join(package).join("package.json");
            host.is_file(&manifest)
                .then(|| read_package_version(host, &manifest))
        })
    };
    let (dependencies_installed, react_native_cli_version, react_native_version) = match framework {
        Framework::Expo => (installed_version("expo").is_some(), None, None),
        Framework::ReactNative
        | Framework::Flutter
        | Framework::AndroidGradle
        | Framework::IosXcode => {
            let react_native = installed_version("react-native");
            let cli = installed_version("@react-native-community/cli");
            (
                react_native.is_some(),
                cli.flatten(),
                react_native.flatten(),
            )
        }
    };
    let name = package.name.as_ref().and_then(|name| name.as_str());
    Some(AppProject {
        framework,
        directory: directory.to_path_buf(),
        display_name: display_name(name, directory),
        has_ios_folder,
        has_android_folder,
        dependencies_installed,
        react_native_cli_version,
        react_native_version,
        native: None,
    })
}

#[derive(Deserialize)]
struct PackageJson {
    name: Option<serde_json::Value>,
    #[serde(default)]
    dependencies: HashMap<String, serde_json::Value>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: HashMap<String, serde_json::Value>,
}

fn read_package_version(host: &dyn HostSystem, manifest: &Path) -> Option<PackageVersion> {
    let text = read_project_file(host, manifest)?;
    let package: serde_json::Value = serde_json::from_str(&text).ok()?;
    parse_version(package.get("version")?.as_str()?)
}

/// Reads `13.6.9` and `0.74.0-rc.1`; a missing minor or patch is 0.
fn parse_version(text: &str) -> Option<PackageVersion> {
    let core = text.trim().split(['-', '+']).next()?;
    let mut numbers = core.split('.').map(str::parse::<u64>);
    let major = numbers.next()?.ok()?;
    let minor = numbers.next().map_or(Some(0), Result::ok)?;
    let patch = numbers.next().map_or(Some(0), Result::ok)?;
    Some(PackageVersion::new(major, minor, patch))
}

struct Pubspec {
    name: Option<String>,
    depends_on_flutter: bool,
}

fn read_pubspec(host: &dyn HostSystem, directory: &Path) -> Option<Pubspec> {
    read_project_file(host, &directory.join("pubspec.yaml")).map(|text| parse_pubspec(&text))
}

/// Line by line rather than a YAML parser: all that matters is a top-level
/// `name:` and `flutter: sdk: flutter` under `dependencies:` (not
/// `dev_dependencies:`, which every pure Dart package with tests may have).
fn parse_pubspec(text: &str) -> Pubspec {
    let mut name = None;
    let mut in_dependencies = false;
    let mut flutter_indent = None;
    let mut depends_on_flutter = false;
    for raw_line in text.lines() {
        let line = raw_line.split(" #").next().unwrap_or_default().trim_end();
        let content = line.trim_start();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let indent = line.len() - content.len();
        if indent == 0 {
            in_dependencies = content == "dependencies:";
            flutter_indent = None;
            if let Some(value) = content.strip_prefix("name:") {
                name = Some(value.trim().trim_matches(['"', '\'']).to_string());
            }
            continue;
        }
        if !in_dependencies {
            continue;
        }
        match flutter_indent {
            Some(flutter) if indent > flutter => {
                let value = content
                    .split_once(':')
                    .map(|(key, value)| (key, value.trim()));
                if value == Some(("sdk", "flutter")) {
                    depends_on_flutter = true;
                }
            }
            _ => flutter_indent = (content == "flutter:").then_some(indent),
        }
    }
    Pubspec {
        name: name.filter(|name| !name.is_empty()),
        depends_on_flutter,
    }
}

pub(crate) fn read_project_file(host: &dyn HostSystem, path: &Path) -> Option<String> {
    if !host.is_file(path) {
        return None;
    }
    match host.read_to_string_at_most(path, MAX_PROJECT_FILE_BYTES) {
        Ok(text) => Some(text),
        Err(error) => {
            log::debug!("skipping {}: {error}", path.display());
            None
        }
    }
}

/// How much of a name is shown; one from a project file may be a megabyte.
const MAX_DISPLAY_NAME_CHARS: usize = 80;

/// The name reaches a terminal tab, a task id and a notification, and comes
/// from a file anyone may have put in the project, so it is cut down to plain
/// text of a bounded length: an escape sequence in it would reach the terminal.
pub(crate) fn display_name(name: Option<&str>, directory: &Path) -> String {
    let from_directory = || {
        directory
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    };
    name.map(plain_text)
        .filter(|name| !name.trim().is_empty())
        .or_else(|| from_directory().map(|name| plain_text(&name)))
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| plain_text(&directory.display().to_string()))
}

/// Characters that draw nothing but can reorder or hide text around them
/// (bidi overrides and isolates, zero-width and blank filler characters).
fn is_invisible(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{00AD}'
                | '\u{034F}'
                | '\u{061C}'
                | '\u{115F}'
                | '\u{1160}'
                | '\u{180E}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{3164}'
                | '\u{FEFF}'
                | '\u{FFA0}'
                | '\u{FFF9}'..='\u{FFFB}'
                | '\u{E0000}'..='\u{E007F}'
        )
}

fn plain_text(text: &str) -> String {
    let mut characters = text.chars().filter(|character| !is_invisible(*character));
    let mut shown: String = characters.by_ref().take(MAX_DISPLAY_NAME_CHARS).collect();
    if characters.next().is_some() {
        shown.pop();
        shown.push('…');
    }
    shown
}
