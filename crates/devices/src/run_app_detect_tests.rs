use std::path::{Path, PathBuf};

use crate::fake_host::FakeHost;
use crate::host::HostOs;
use crate::run_app::{AppProject, Framework, PackageVersion};
use crate::run_app_detect::{
    MAX_PROJECT_FILE_BYTES, detect_app_projects, detect_app_projects_excluding,
};
use crate::test_fixtures::tool_output;

const ROOT: &str = "/work/open-folder";

fn project_file(name: &str) -> String {
    tool_output(&format!("projects/{name}"))
}

fn add_flutter_app(host: &FakeHost, directory: &Path) {
    host.add_file(
        directory.join("pubspec.yaml"),
        project_file("flutter-app-pubspec.yaml"),
    );
    host.add_file(directory.join("lib/main.dart"), "void main() {}\n");
    host.add_file(directory.join("ios/Runner.xcodeproj/project.pbxproj"), "");
    host.add_file(directory.join("android/build.gradle.kts"), "");
}

fn add_expo_app(host: &FakeHost, directory: &Path) {
    host.add_file(
        directory.join("package.json"),
        project_file("expo-package.json"),
    );
    host.add_file(directory.join("app.json"), project_file("expo-app.json"));
    host.add_file(
        directory.join("node_modules/expo/package.json"),
        r#"{"name":"expo","version":"58.0.3"}"#,
    );
}

fn add_react_native_app(host: &FakeHost, directory: &Path) {
    host.add_file(
        directory.join("package.json"),
        project_file("react-native-package.json"),
    );
    host.add_file(directory.join("ios/Podfile"), "");
    host.add_file(directory.join("android/build.gradle"), "");
    host.add_file(
        directory.join("node_modules/react-native/package.json"),
        r#"{"name":"react-native","version":"0.88.1"}"#,
    );
    host.add_file(
        directory.join("node_modules/@react-native-community/cli/package.json"),
        r#"{"name":"@react-native-community/cli","version":"20.2.0"}"#,
    );
}

fn detect(host: &FakeHost) -> Vec<AppProject> {
    detect_app_projects(host, Path::new(ROOT))
}

fn found(projects: &[AppProject]) -> Vec<(Framework, PathBuf)> {
    projects
        .iter()
        .map(|project| (project.framework, project.directory.clone()))
        .collect()
}

fn root() -> PathBuf {
    PathBuf::from(ROOT)
}

#[test]
fn a_flutter_app_at_the_root_owns_its_native_folders() {
    let host = FakeHost::new(HostOs::MacOs);
    add_flutter_app(&host, &root());
    // Without the ownership rule this would read as a second, React Native app.
    add_react_native_app(&host, &root().join("android"));

    let projects = detect(&host);
    assert_eq!(found(&projects), vec![(Framework::Flutter, root())]);
    let project = &projects[0];
    assert_eq!(project.display_name, "counter_app");
    assert!(project.has_ios_folder && project.has_android_folder);
    assert!(project.dependencies_installed);
    assert!(host.invocations().is_empty(), "detection runs no program");
}

#[test]
fn a_pubspec_is_not_enough_to_be_a_flutter_app() {
    let host = FakeHost::new(HostOs::MacOs);
    // Pure Dart package: Flutter only in dev_dependencies.
    let package = root().join("string_tools");
    host.add_file(
        package.join("pubspec.yaml"),
        project_file("dart-package-pubspec.yaml"),
    );
    host.add_file(package.join("lib/main.dart"), "");
    host.add_file(package.join("android/build.gradle"), "");
    // Flutter app pubspec without lib/main.dart.
    let no_entry = root().join("no_entry");
    host.add_file(
        no_entry.join("pubspec.yaml"),
        project_file("flutter-app-pubspec.yaml"),
    );
    host.add_file(no_entry.join("ios/Podfile"), "");
    // Flutter app pubspec without ios/ or android/.
    let no_platform = root().join("no_platform");
    host.add_file(
        no_platform.join("pubspec.yaml"),
        project_file("flutter-app-pubspec.yaml"),
    );
    host.add_file(no_platform.join("lib/main.dart"), "");
    host.add_file(no_platform.join("web/index.html"), "");

    assert_eq!(found(&detect(&host)), vec![]);
}

#[test]
fn a_flutter_plugin_runs_its_example_even_one_level_deeper() {
    let host = FakeHost::new(HostOs::MacOs);
    let plugin = root().join("battery_plugin");
    host.add_file(
        plugin.join("pubspec.yaml"),
        project_file("flutter-plugin-pubspec.yaml"),
    );
    host.add_file(plugin.join("lib/battery_plugin.dart"), "");
    host.add_file(plugin.join("ios/Classes/BatteryPlugin.swift"), "");
    host.add_file(plugin.join("android/build.gradle"), "");
    add_flutter_app(&host, &plugin.join("example"));

    assert_eq!(
        found(&detect(&host)),
        vec![(Framework::Flutter, plugin.join("example"))]
    );
}

#[test]
fn a_plugin_opened_as_the_root_lists_its_example_once() {
    let host = FakeHost::new(HostOs::MacOs);
    host.add_file(
        root().join("pubspec.yaml"),
        project_file("flutter-plugin-pubspec.yaml"),
    );
    host.add_file(root().join("ios/Classes/Plugin.swift"), "");
    add_flutter_app(&host, &root().join("example"));

    assert_eq!(
        found(&detect(&host)),
        vec![(Framework::Flutter, root().join("example"))]
    );
}

#[test]
fn a_pubspec_with_windows_line_endings_is_read() {
    let host = FakeHost::new(HostOs::Windows);
    add_flutter_app(&host, &root());
    host.add_file(
        root().join("pubspec.yaml"),
        project_file("flutter-app-pubspec.yaml").replace('\n', "\r\n"),
    );
    assert_eq!(found(&detect(&host)), vec![(Framework::Flutter, root())]);
}

#[test]
fn expo_needs_the_expo_package_and_an_app_config() {
    let host = FakeHost::new(HostOs::MacOs);
    add_expo_app(&host, &root().join("with-app-json"));
    let config = root().join("with-app-config");
    host.add_file(
        config.join("package.json"),
        project_file("expo-package.json"),
    );
    host.add_file(config.join("app.config.ts"), "export default {};\n");
    // Expo modules in a bare React Native app, with no Expo app config.
    let bare = root().join("bare");
    host.add_file(
        bare.join("package.json"),
        project_file("react-native-with-expo-modules-package.json"),
    );
    host.add_file(bare.join("ios/Podfile"), "");

    assert_eq!(
        found(&detect(&host)),
        vec![
            (Framework::ReactNative, root().join("bare")),
            (Framework::Expo, root().join("with-app-config")),
            (Framework::Expo, root().join("with-app-json")),
        ]
    );
}

#[test]
fn react_native_needs_a_native_folder() {
    let host = FakeHost::new(HostOs::MacOs);
    let no_native = root().join("no-native");
    host.add_file(
        no_native.join("package.json"),
        project_file("react-native-package.json"),
    );
    let library = root().join("library");
    host.add_file(
        library.join("package.json"),
        project_file("javascript-library-package.json"),
    );
    host.add_file(library.join("dist/index.js"), "");

    assert_eq!(found(&detect(&host)), vec![]);
}

#[test]
fn flutter_wins_over_expo_in_the_same_folder() {
    let host = FakeHost::new(HostOs::MacOs);
    add_expo_app(&host, &root());
    add_flutter_app(&host, &root());
    assert_eq!(found(&detect(&host)), vec![(Framework::Flutter, root())]);
}

#[test]
fn only_the_root_and_its_direct_children_are_searched() {
    let host = FakeHost::new(HostOs::MacOs);
    add_expo_app(&host, &root());
    add_flutter_app(&host, &root().join("mobile"));
    add_react_native_app(&host, &root().join("packages/rn-app"));
    add_flutter_app(&host, &root().join("packages/a/b"));

    assert_eq!(
        found(&detect(&host)),
        vec![
            (Framework::Expo, root()),
            (Framework::Flutter, root().join("mobile")),
        ]
    );
}

#[test]
fn dependency_build_and_hidden_folders_are_skipped() {
    let host = FakeHost::new(HostOs::MacOs);
    for skipped in [
        "node_modules",
        "Pods",
        "build",
        ".git",
        ".dart_tool",
        "DerivedData",
        "target",
        ".hidden-app",
    ] {
        add_flutter_app(&host, &root().join(skipped));
    }
    assert_eq!(found(&detect(&host)), vec![]);
}

#[test]
fn broken_or_oversized_project_files_are_skipped_without_error() {
    let host = FakeHost::new(HostOs::MacOs);
    let broken = root().join("broken");
    host.add_file(broken.join("package.json"), "{\"dependencies\": {");
    host.add_file(broken.join("app.json"), "{}");
    host.add_file(broken.join("ios/Podfile"), "");
    let wrong_shape = root().join("wrong-shape");
    host.add_file(
        wrong_shape.join("package.json"),
        r#"{"name": 7, "dependencies": null}"#,
    );
    host.add_file(wrong_shape.join("android/build.gradle"), "");

    let padding = " ".repeat(MAX_PROJECT_FILE_BYTES as usize);
    let huge_json = root().join("huge-json");
    add_react_native_app(&host, &huge_json);
    host.add_file(
        huge_json.join("package.json"),
        project_file("react-native-package.json") + &padding,
    );
    let huge_pubspec = root().join("huge-pubspec");
    add_flutter_app(&host, &huge_pubspec);
    host.add_file(
        huge_pubspec.join("pubspec.yaml"),
        project_file("flutter-app-pubspec.yaml") + &padding,
    );
    add_react_native_app(&host, &root().join("healthy"));

    assert_eq!(
        found(&detect(&host)),
        vec![(Framework::ReactNative, root().join("healthy"))]
    );
}

#[test]
fn an_empty_or_missing_folder_has_no_apps() {
    let host = FakeHost::new(HostOs::MacOs);
    assert_eq!(found(&detect(&host)), vec![]);
    host.add_file(root().join("README.md"), "hello");
    assert_eq!(found(&detect(&host)), vec![]);
}

#[test]
fn installed_packages_are_found_in_the_app_or_the_opened_folder() {
    let host = FakeHost::new(HostOs::MacOs);
    let missing = root().join("not-installed");
    host.add_file(
        missing.join("package.json"),
        project_file("expo-package.json"),
    );
    host.add_file(missing.join("app.json"), project_file("expo-app.json"));
    // A workspace that hoists packages to the opened folder.
    let hoisted = root().join("hoisted");
    host.add_file(
        hoisted.join("package.json"),
        project_file("react-native-package.json"),
    );
    host.add_file(hoisted.join("android/build.gradle"), "");
    host.add_file(
        root().join("node_modules/react-native/package.json"),
        r#"{"version":"0.73.6"}"#,
    );

    let projects = detect(&host);
    assert_eq!(
        found(&projects),
        vec![
            (Framework::ReactNative, hoisted),
            (Framework::Expo, missing),
        ]
    );
    assert!(projects[0].dependencies_installed);
    assert_eq!(
        projects[0].react_native_version,
        Some(PackageVersion::new(0, 73, 6))
    );
    assert_eq!(projects[0].react_native_cli_version, None);
    assert!(projects[0].has_android_folder && !projects[0].has_ios_folder);
    assert!(!projects[1].dependencies_installed);
    assert!(!projects[1].has_ios_folder && !projects[1].has_android_folder);
}

#[test]
fn react_native_versions_and_names_come_from_the_package_files() {
    let host = FakeHost::new(HostOs::MacOs);
    add_react_native_app(&host, &root().join("rn"));
    let unnamed = root().join("unnamed-expo");
    add_expo_app(&host, &unnamed);
    host.add_file(
        unnamed.join("package.json"),
        r#"{"dependencies": {"expo": "~58.0.0"}}"#,
    );

    let projects = detect(&host);
    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].display_name, "RnSample");
    assert_eq!(
        projects[0].react_native_cli_version,
        Some(PackageVersion::new(20, 2, 0))
    );
    assert_eq!(
        projects[0].react_native_version,
        Some(PackageVersion::new(0, 88, 1))
    );
    assert_eq!(projects[1].display_name, "unnamed-expo");
    assert_eq!(projects[1].react_native_cli_version, None);
}

#[test]
fn a_symlinked_child_folder_is_not_followed() {
    let host = FakeHost::new(HostOs::MacOs);
    add_flutter_app(&host, &root().join("linked"));
    host.add_symlink(root().join("linked"));
    add_expo_app(&host, &root().join("real"));

    assert_eq!(
        found(&detect(&host)),
        vec![(Framework::Expo, root().join("real"))]
    );
    assert!(
        host.reads()
            .iter()
            .all(|path| !path.starts_with(root().join("linked")))
    );
}

#[test]
fn an_excluded_child_folder_is_never_read() {
    let host = FakeHost::new(HostOs::MacOs);
    add_flutter_app(&host, &root().join("restricted"));
    add_expo_app(&host, &root().join("trusted"));

    let projects =
        detect_app_projects_excluding(&host, Path::new(ROOT), &[root().join("restricted")]);
    assert_eq!(
        found(&projects),
        vec![(Framework::Expo, root().join("trusted"))]
    );
    assert!(
        host.reads()
            .iter()
            .all(|path| !path.starts_with(root().join("restricted")))
    );
}

#[test]
fn an_excluded_example_app_or_gradle_module_is_never_read() {
    let host = FakeHost::new(HostOs::MacOs);
    host.add_file(
        root().join("pubspec.yaml"),
        "name: a_plugin\ndependencies:\n  flutter:\n    sdk: flutter\n",
    );
    add_flutter_app(&host, &root().join("example"));
    let projects = detect_app_projects_excluding(&host, Path::new(ROOT), &[root().join("example")]);
    assert!(projects.is_empty(), "{:?}", found(&projects));
    assert!(
        host.reads()
            .iter()
            .all(|path| !path.starts_with(root().join("example"))),
        "{:?}",
        host.reads()
    );

    let host = FakeHost::new(HostOs::Linux);
    host.add_file(root().join("gradlew"), "#!/bin/sh\n");
    host.add_file(root().join("settings.gradle"), "include ':app'\n");
    host.add_file(
        root().join("app/build.gradle"),
        "plugins { id 'com.android.application' }\n",
    );
    let projects = detect_app_projects_excluding(&host, Path::new(ROOT), &[root().join("app")]);
    assert!(projects.is_empty(), "{:?}", found(&projects));
    assert!(
        host.reads()
            .iter()
            .all(|path| !path.starts_with(root().join("app"))),
        "{:?}",
        host.reads()
    );
}

#[test]
fn a_trusted_folder_inside_a_restricted_one_is_still_scanned() {
    let host = FakeHost::new(HostOs::MacOs);
    let inner = root().join("inner");
    add_expo_app(&host, &inner.join("mobile"));

    let projects = detect_app_projects_excluding(&host, &inner, &[root()]);
    assert_eq!(
        found(&projects),
        vec![(Framework::Expo, inner.join("mobile"))]
    );
}

#[test]
fn a_symlinked_example_folder_is_not_followed() {
    let host = FakeHost::new(HostOs::MacOs);
    host.add_file(
        root().join("pubspec.yaml"),
        "name: a_plugin\ndependencies:\n  flutter:\n    sdk: flutter\n",
    );
    add_flutter_app(&host, &root().join("example"));
    host.add_symlink(root().join("example"));

    assert_eq!(found(&detect(&host)), vec![]);
    assert!(
        host.reads()
            .iter()
            .all(|path| !path.starts_with(root().join("example"))),
        "{:?}",
        host.reads()
    );
}

#[test]
fn a_symlinked_gradle_module_is_not_followed() {
    let host = FakeHost::new(HostOs::Linux);
    host.add_file(root().join("gradlew"), "#!/bin/sh\n");
    host.add_file(root().join("settings.gradle"), "include ':app'\n");
    host.add_file(
        root().join("app/build.gradle"),
        "plugins { id 'com.android.application' }\n",
    );
    host.add_symlink(root().join("app"));

    assert_eq!(found(&detect(&host)), vec![]);
    assert!(
        host.reads()
            .iter()
            .all(|path| !path.starts_with(root().join("app"))),
        "{:?}",
        host.reads()
    );
}

#[test]
fn a_name_from_a_project_file_cannot_carry_control_characters_or_run_on() {
    let host = FakeHost::new(HostOs::MacOs);
    let escaped = root().join("escaped");
    add_expo_app(&host, &escaped);
    host.add_file(
        escaped.join("package.json"),
        r#"{"name":"\u001b[2Jevil\u001b]0;pwned\u0007\nname\r\u0000","dependencies":{"expo":"~58.0.0"}}"#,
    );
    let long = root().join("long");
    add_expo_app(&host, &long);
    host.add_file(
        long.join("package.json"),
        format!(
            r#"{{"name":"{}","dependencies":{{"expo":"~58.0.0"}}}}"#,
            "n".repeat(500_000)
        ),
    );
    let plain = root().join("plain");
    add_flutter_app(&host, &plain);
    host.add_file(
        plain.join("pubspec.yaml"),
        "name: \"fl\u{1b}[31mutter\u{7}_app\"\ndependencies:\n  flutter:\n    sdk: flutter\n",
    );

    let names: Vec<String> = detect(&host)
        .into_iter()
        .map(|project| project.display_name)
        .collect();
    assert_eq!(names.len(), 3, "{names:?}");
    for name in &names {
        assert!(
            !name.chars().any(char::is_control),
            "{name:?} must carry no control character"
        );
        assert!(name.chars().count() <= 80, "{} chars", name.chars().count());
    }
    assert_eq!(names[0], "[2Jevil]0;pwnedname");
    assert!(
        names[1].starts_with("nnnn") && names[1].ends_with('…'),
        "{}",
        names[1]
    );
    assert_eq!(names[2], "fl[31mutter_app");
}

#[test]
fn a_name_cannot_carry_bidi_overrides_or_other_invisible_characters() {
    use crate::run_app_detect::display_name;

    for hidden in [
        '\u{202A}',
        '\u{202D}',
        '\u{202E}',
        '\u{2066}',
        '\u{2067}',
        '\u{2068}',
        '\u{2069}',
        '\u{200B}',
        '\u{200F}',
        '\u{2028}',
        '\u{2060}',
        '\u{2064}',
        '\u{206F}',
        '\u{00AD}',
        '\u{3164}',
        '\u{FEFF}',
        '\u{FFA0}',
        '\u{E0041}',
    ] {
        let name = display_name(Some(&format!("app{hidden}name")), Path::new("/x"));
        assert_eq!(name, "appname", "U+{:04X}", hidden as u32);
    }
    // Ordinary non-ASCII text is not touched.
    assert_eq!(
        display_name(Some("ứng dụng 日本語"), Path::new("/x")),
        "ứng dụng 日本語"
    );
}

#[test]
fn a_folder_name_with_control_characters_is_cleaned_too() {
    let host = FakeHost::new(HostOs::MacOs);
    add_expo_app(&host, &root().join("bad\u{1b}[2Jfolder"));
    host.add_file(
        root().join("bad\u{1b}[2Jfolder/package.json"),
        r#"{"dependencies":{"expo":"~58.0.0"}}"#,
    );
    let names: Vec<String> = detect(&host)
        .into_iter()
        .map(|project| project.display_name)
        .collect();
    assert_eq!(names, ["bad[2Jfolder"]);
}

#[test]
fn a_name_of_exactly_the_limit_is_kept_whole() {
    use crate::run_app_detect::display_name;

    let at_limit = "n".repeat(80);
    assert_eq!(display_name(Some(&at_limit), Path::new("/x")), at_limit);
    let over = "n".repeat(81);
    let shown = display_name(Some(&over), Path::new("/x"));
    assert_eq!(shown.chars().count(), 80);
    assert!(shown.ends_with('…'));
    // Cleaning can leave nothing; the folder name then stands in.
    assert_eq!(
        display_name(Some("\u{1b}\u{7}"), Path::new("/work/folder")),
        "folder"
    );
}
