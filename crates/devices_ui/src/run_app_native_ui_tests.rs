use std::path::{Path, PathBuf};

use devices::{AppProject, DeviceKey, Framework, NativeProject, XcodeContainer};
use gpui::{Entity, TestAppContext, VisualTestContext};
use project::trusted_worktrees::{PathTrust, TrustedWorktrees};
use task::Shell;
use workspace::dock::Panel as _;

use crate::devices_panel::DevicesPanel;
use crate::run_app_test_support::{
    FakeRunState, Trust, add_flutter_app, click_run, detected, error_of, reads_under, run_app,
    run_panel,
};
use crate::test_support::{Rig, SIMULATOR_A, init_test};

const XCODEBUILD: &str = "/usr/bin/xcodebuild";
const WORKSPACE_NAME: &str = "MyApp.xcworkspace";

fn key() -> DeviceKey {
    DeviceKey::Simulator(SIMULATOR_A.to_string())
}

fn open(panel: &Entity<DevicesPanel>, cx: &mut VisualTestContext) {
    panel.update_in(cx, |panel, window, cx| panel.set_active(true, window, cx));
    cx.run_until_parked();
}

fn booted() -> Rig {
    let rig = Rig::mac();
    rig.script_simulator_list("Booted", "Booted");
    rig
}

fn add_xcode_app(rig: &Rig) {
    rig.host.add_file(
        Path::new("/app")
            .join(WORKSPACE_NAME)
            .join("contents.xcworkspacedata"),
        "",
    );
}

fn xcode_project() -> AppProject {
    AppProject {
        framework: Framework::IosXcode,
        directory: PathBuf::from("/app"),
        display_name: "MyApp".to_string(),
        has_ios_folder: true,
        has_android_folder: false,
        dependencies_installed: true,
        react_native_cli_version: None,
        react_native_version: None,
        native: Some(NativeProject::Xcode(XcodeContainer {
            file_name: "MyApp.xcworkspace".to_string(),
            is_workspace: true,
        })),
    }
}

/// The container as the plan spells it for xcodebuild: the app folder the
/// panel detected, joined with the host's separator. Taken from the panel
/// because the worktree decides how that folder is written.
fn workspace_of(panel: &Entity<DevicesPanel>, cx: &mut VisualTestContext) -> String {
    let apps = detected(panel, cx);
    let app = apps.first().expect("the Xcode app is detected");
    app.directory
        .join(WORKSPACE_NAME)
        .to_string_lossy()
        .into_owned()
}

fn script_schemes(rig: &Rig, workspace: &str, schemes: &[&str]) {
    rig.host.script_output(
        XCODEBUILD,
        ["-list", "-json", "-workspace", workspace],
        0,
        &serde_json::json!({ "workspace": { "name": "MyApp", "schemes": schemes } }).to_string(),
        "",
    );
}

fn script_settings(rig: &Rig, workspace: &str, scheme: &str) {
    let destination = format!("id={SIMULATOR_A}");
    rig.host.script_output(
        XCODEBUILD,
        [
            "-showBuildSettings",
            "-json",
            "-workspace",
            workspace,
            "-scheme",
            scheme,
            "-configuration",
            "Debug",
            "-destination",
            &destination,
        ],
        0,
        &serde_json::json!([{
            "target": scheme,
            "buildSettings": {
                "WRAPPER_EXTENSION": "app",
                "TARGET_BUILD_DIR": "/dd/Debug-iphonesimulator",
                "FULL_PRODUCT_NAME": "MyApp.app",
                "PRODUCT_BUNDLE_IDENTIFIER": "dev.example.app",
            }
        }])
        .to_string(),
        "",
    );
}

fn xcodebuild_count(rig: &Rig) -> usize {
    rig.host
        .invocations()
        .iter()
        .filter(|invocation| invocation.program == Path::new(XCODEBUILD))
        .count()
}

fn scheme_choices(panel: &Entity<DevicesPanel>, cx: &mut VisualTestContext) -> Vec<Option<String>> {
    panel.read_with(cx, |panel, _| {
        panel
            .app_choice
            .iter()
            .flat_map(|choice| choice.choices.iter().map(|choice| choice.scheme.clone()))
            .collect()
    })
}

#[gpui::test]
async fn an_untrusted_xcode_project_runs_no_xcodebuild(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = booted();
    add_xcode_app(&rig);
    let workspace = Path::new("/app").join(WORKSPACE_NAME);
    let workspace = workspace.to_string_lossy();
    script_schemes(&rig, &workspace, &["MyApp"]);
    script_settings(&rig, &workspace, "MyApp");
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&[])).await;
    open(&panel, cx);

    run_app(&panel, &key(), &xcode_project(), cx);

    assert_eq!(xcodebuild_count(&rig), 0);
    assert_eq!(terminals.spawn_count(), 0);
    assert!(error_of(&panel, &key(), cx).contains("Trust"));
}

#[gpui::test]
async fn several_schemes_are_asked_about_on_every_click(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = booted();
    add_xcode_app(&rig);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);
    let workspace = workspace_of(&panel, cx);
    script_schemes(&rig, &workspace, &["MyApp", "Pods-MyApp", "MyApp Staging"]);
    script_settings(&rig, &workspace, "MyApp Staging");
    assert_eq!(xcodebuild_count(&rig), 0, "detection never runs xcodebuild");

    click_run(&panel, &key(), cx);
    assert_eq!(
        scheme_choices(&panel, cx),
        [Some("MyApp".to_string()), Some("MyApp Staging".to_string())]
    );
    assert_eq!(terminals.spawn_count(), 0);

    panel.update_in(cx, |panel, window, cx| panel.choose_app(1, window, cx));
    cx.run_until_parked();

    let task = terminals.spawned.borrow()[0].clone();
    assert_eq!(task.shell, Shell::Program("/bin/sh".to_string()));
    assert_eq!(task.command.as_deref(), Some(XCODEBUILD));
    assert!(
        task.args.contains(&"'MyApp Staging'".to_string()),
        "{:?}",
        task.args
    );
    assert_eq!(task.args.iter().filter(|arg| *arg == "&&").count(), 2);
    assert!(error_of(&panel, &key(), cx).is_empty());

    terminals.run(0).state.set(FakeRunState::Finished);
    click_run(&panel, &key(), cx);
    assert_eq!(
        scheme_choices(&panel, cx).len(),
        2,
        "the choice is not remembered"
    );
}

#[gpui::test]
async fn an_app_in_two_open_folders_is_listed_once(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = booted();
    add_flutter_app(&rig.host, "/parent/app", true);
    let (panel, _, cx) = run_panel(
        cx,
        &rig,
        // The child first: opened the other way round it would join the
        // parent's worktree.
        &["/parent/app", "/parent"],
        Trust::Only(&["/parent", "/parent/app"]),
    )
    .await;
    open(&panel, cx);
    assert_eq!(detected(&panel, cx).len(), 1);
}

#[gpui::test]
async fn a_restricted_folder_inside_a_trusted_one_is_not_read(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = booted();
    add_flutter_app(&rig.host, "/parent/app", true);
    let (panel, _, cx) = run_panel(
        cx,
        &rig,
        &["/parent/app", "/parent"],
        Trust::Only(&["/parent"]),
    )
    .await;
    let project = panel.read_with(cx, |panel, _| panel.project.upgrade().expect("project"));
    let worktree_store = project.read_with(cx, |project, _| project.worktree_store());
    let child = worktree_store.read_with(cx, |worktree_store, cx| {
        worktree_store
            .worktrees()
            .filter(|worktree| worktree.read(cx).abs_path().as_ref() == Path::new("/parent/app"))
            .map(|worktree| PathTrust::Worktree(worktree.read(cx).id()))
            .collect::<Vec<_>>()
    });
    assert_eq!(child.len(), 1, "the child is a worktree of its own");
    let store = cx
        .update(|_, cx| TrustedWorktrees::try_get_global(cx))
        .expect("trust store");
    store.update(cx, |store, cx| {
        store.restrict(worktree_store.downgrade(), child.into_iter().collect(), cx)
    });
    open(&panel, cx);

    assert!(detected(&panel, cx).is_empty());
    assert_eq!(reads_under(&rig, "/parent/app"), 0);
}

#[gpui::test]
async fn a_boot_wait_dropped_without_an_answer_says_so(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::mac_booting();
    rig.script_simulator_list("Shutdown", "Shutdown");
    add_flutter_app(&rig.host, "/app", true);
    let (panel, terminals, cx) = run_panel(cx, &rig, &["/app"], Trust::Only(&["/app"])).await;
    open(&panel, cx);
    click_run(&panel, &key(), cx);

    panel.update(cx, |panel, _| panel.operations.remove(&key()));
    cx.run_until_parked();

    assert_eq!(terminals.spawn_count(), 0);
    assert!(
        error_of(&panel, &key(), cx).contains("did not finish starting"),
        "{}",
        error_of(&panel, &key(), cx)
    );
}
