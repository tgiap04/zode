//! A terminal that never starts a shell, and projects whose folders are
//! trusted or not, for the Run app tests.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use devices::{AppProject, DeviceKey, FakeHost, Framework};
use fs::Fs as _;
use gpui::{App, Entity, Task, TestAppContext, VisualTestContext, Window};
use project::trusted_worktrees::{DbTrustedPaths, PathTrust, TrustedWorktrees};
use task::{ShellKind, SpawnInTerminal};

use crate::devices_panel::DevicesPanel;
use crate::run_terminals::{RunTerminal, RunTerminals};
use crate::test_support::{Rig, panel_for_project};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FakeRunState {
    Running,
    Finished,
    Closed,
}

pub(crate) struct FakeRun {
    pub state: Cell<FakeRunState>,
    pub reveals: Cell<usize>,
}

impl RunTerminal for FakeRun {
    fn is_running(&self, _cx: &App) -> bool {
        self.state.get() == FakeRunState::Running
    }

    fn reveal(&self, _window: &mut Window, _cx: &mut App) {
        self.reveals.set(self.reveals.get() + 1);
    }
}

#[derive(Default)]
pub(crate) struct FakeTerminals {
    pub spawned: RefCell<Vec<SpawnInTerminal>>,
    pub runs: RefCell<Vec<Rc<FakeRun>>>,
    /// Spawns from this index on fail, as a terminal that cannot start does.
    pub fail_from: Cell<Option<usize>>,
}

impl FakeTerminals {
    pub(crate) fn spawn_count(&self) -> usize {
        self.spawned.borrow().len()
    }

    pub(crate) fn run(&self, index: usize) -> Rc<FakeRun> {
        self.runs.borrow()[index].clone()
    }

    /// Program and arguments of each spawned task, in order.
    pub(crate) fn command_lines(&self) -> Vec<Vec<String>> {
        self.spawned
            .borrow()
            .iter()
            .map(|task| {
                task.command
                    .iter()
                    .cloned()
                    .chain(task.args.iter().cloned())
                    .collect()
            })
            .collect()
    }
}

impl RunTerminals for FakeTerminals {
    fn shell_kind(&self) -> ShellKind {
        ShellKind::Posix
    }

    fn spawn(
        &self,
        task: SpawnInTerminal,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<anyhow::Result<Rc<dyn RunTerminal>>> {
        self.spawned.borrow_mut().push(task);
        if self
            .fail_from
            .get()
            .is_some_and(|first| self.spawned.borrow().len() > first)
        {
            return Task::ready(Err(anyhow::anyhow!("the shell could not be started")));
        }
        let run = Rc::new(FakeRun {
            state: Cell::new(FakeRunState::Running),
            reveals: Cell::new(0),
        });
        self.runs.borrow_mut().push(run.clone());
        Task::ready(Ok(run as Rc<dyn RunTerminal>))
    }
}

pub(crate) enum Trust<'a> {
    /// No trust store at all, as in a build that never set one up.
    NoStore,
    Only(&'a [&'a str]),
    /// A store set up after the folders opened, so none was ever decided.
    Undecided,
}

/// A local project with `roots` open and a panel whose runs go to a fake
/// terminal.
pub(crate) async fn run_panel<'a>(
    cx: &'a mut TestAppContext,
    rig: &Rig,
    roots: &[&str],
    trust: Trust<'_>,
) -> (
    Entity<DevicesPanel>,
    Rc<FakeTerminals>,
    &'a mut VisualTestContext,
) {
    if let Trust::Only(_) = trust {
        cx.update(|cx| project::trusted_worktrees::init(DbTrustedPaths::default(), cx));
    }
    let fs = fs::FakeFs::new(cx.executor());
    for root in roots {
        fs.create_dir(Path::new(root)).await.expect("fake folder");
    }
    let project =
        project::Project::test_with_worktree_trust(fs, roots.iter().map(Path::new), cx).await;
    if let Trust::Undecided = trust {
        cx.update(|cx| project::trusted_worktrees::init(DbTrustedPaths::default(), cx));
    }
    if let Trust::Only(trusted) = trust {
        let store = cx
            .update(|cx| TrustedWorktrees::try_get_global(cx))
            .expect("trust store");
        let worktree_store = project.read_with(cx, |project, _| project.worktree_store());
        let trusted_ids = worktree_store.read_with(cx, |worktree_store, cx| {
            worktree_store
                .worktrees()
                .filter(|worktree| {
                    trusted
                        .iter()
                        .any(|path| worktree.read(cx).abs_path().as_ref() == Path::new(path))
                })
                .map(|worktree| PathTrust::Worktree(worktree.read(cx).id()))
                .collect()
        });
        store.update(cx, |store, cx| {
            store.trust(&worktree_store, trusted_ids, cx)
        });
    }
    let terminals = Rc::new(FakeTerminals::default());
    let (panel, cx) = panel_for_project(cx, rig, project);
    panel.update(cx, |panel, _| panel.run_terminals = terminals.clone());
    (panel, terminals, cx)
}

pub(crate) fn add_flutter_app(host: &FakeHost, directory: &str, with_ios: bool) {
    let directory = Path::new(directory);
    host.add_file(
        directory.join("pubspec.yaml"),
        "name: demo_app\ndependencies:\n  flutter:\n    sdk: flutter\n",
    );
    host.add_file(directory.join("lib/main.dart"), "void main() {}\n");
    host.add_file(directory.join("android/build.gradle"), "");
    if with_ios {
        host.add_file(directory.join("ios/Runner.xcodeproj/project.pbxproj"), "");
    }
}

pub(crate) fn add_expo_app(host: &FakeHost, directory: &str, with_ios: bool) {
    let directory = Path::new(directory);
    host.add_file(
        directory.join("package.json"),
        r#"{"name":"expo_app","dependencies":{"expo":"~58.0.0"}}"#,
    );
    host.add_file(directory.join("app.json"), "{}");
    host.add_file(
        directory.join("node_modules/expo/package.json"),
        r#"{"name":"expo","version":"58.0.3"}"#,
    );
    if with_ios {
        host.add_file(directory.join("ios/Podfile"), "");
    }
}

pub(crate) fn add_react_native_app(host: &FakeHost, directory: &str) {
    let directory = Path::new(directory);
    host.add_file(
        directory.join("package.json"),
        r#"{"name":"rn_app","dependencies":{"react-native":"0.80.0"}}"#,
    );
    host.add_file(directory.join("ios/Podfile"), "");
    host.add_file(
        directory.join("node_modules/react-native/package.json"),
        r#"{"name":"react-native","version":"0.80.0"}"#,
    );
}

/// An app as a stale list or a forged click would name it, never read.
pub(crate) fn flutter_project(directory: &str) -> AppProject {
    AppProject {
        framework: Framework::Flutter,
        directory: PathBuf::from(directory),
        display_name: "demo_app".to_string(),
        has_ios_folder: true,
        has_android_folder: true,
        dependencies_installed: true,
        react_native_cli_version: None,
        react_native_version: None,
        native: None,
    }
}

pub(crate) fn reads_under(rig: &Rig, directory: &str) -> usize {
    rig.host
        .reads()
        .iter()
        .filter(|path| path.starts_with(directory))
        .count()
}

pub(crate) fn detected(
    panel: &Entity<DevicesPanel>,
    cx: &mut VisualTestContext,
) -> Vec<AppProject> {
    panel.read_with(cx, |panel, _| panel.detected_apps.clone())
}

pub(crate) fn app_in(
    panel: &Entity<DevicesPanel>,
    directory: &str,
    cx: &mut VisualTestContext,
) -> AppProject {
    detected(panel, cx)
        .into_iter()
        .find(|app| app.directory == Path::new(directory))
        .expect("the app was found")
}

pub(crate) fn run_app(
    panel: &Entity<DevicesPanel>,
    device: &DeviceKey,
    app: &AppProject,
    cx: &mut VisualTestContext,
) {
    panel.update_in(cx, |panel, window, cx| {
        panel.run_app(device, app, window, cx)
    });
    cx.run_until_parked();
}

pub(crate) fn click_run(
    panel: &Entity<DevicesPanel>,
    device: &DeviceKey,
    cx: &mut VisualTestContext,
) {
    panel.update_in(cx, |panel, window, cx| {
        panel.run_clicked(device, window, cx)
    });
    cx.run_until_parked();
}

pub(crate) fn error_of(
    panel: &Entity<DevicesPanel>,
    device: &DeviceKey,
    cx: &mut VisualTestContext,
) -> String {
    panel.read_with(cx, |panel, _| {
        panel
            .device_errors
            .get(device)
            .map(|error| error.message.to_string())
            .unwrap_or_default()
    })
}
