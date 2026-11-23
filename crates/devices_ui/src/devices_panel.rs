use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use devices::{
    AppProject, DeviceKey, DeviceSnapshot, DeviceState, HostSystem, LocalHost, ToolRunner,
};
use futures::channel::oneshot;
use gpui::{
    App, AppContext as _, AsyncWindowContext, Context, Entity, FocusHandle, SharedString, Task,
    WeakEntity,
};
use project::Project;
use util::ResultExt as _;
use workspace::Workspace;

use crate::device_operations::{DeviceError, OperationKind};
use crate::device_snapshot::{SectionId, merge_snapshot};
use crate::run_app::{ActiveRun, AppChoice, RunKey, StartingRun};
use crate::run_terminals::{RunTerminals, WorkspaceTerminals};

/// A Boot, Shutdown or Show window in flight. The task is the follower of this
/// one device; dropping the panel drops it, and a device already started keeps
/// running.
pub(crate) struct Operation {
    pub kind: OperationKind,
    /// What the row goes back to if the operation fails without a sighting.
    pub previous: DeviceState,
    /// Runs waiting for this boot; each is told whether the device came up.
    pub waiters: Vec<oneshot::Sender<bool>>,
    pub _task: Task<()>,
}

impl Operation {
    pub(crate) fn shown_state(&self) -> Option<DeviceState> {
        match self.kind {
            OperationKind::Boot => Some(DeviceState::Starting),
            OperationKind::Shutdown => Some(DeviceState::Stopping),
            OperationKind::ShowWindow => None,
        }
    }
}

pub struct DevicesPanel {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) workspace: WeakEntity<Workspace>,
    /// Held apart from the workspace: the panel is built inside the
    /// workspace's own update, where reading the workspace back panics.
    pub(crate) project: WeakEntity<Project>,
    pub(crate) host: Arc<dyn HostSystem>,
    pub(crate) runner: Arc<dyn ToolRunner>,
    pub(crate) log_dir: PathBuf,
    pub(crate) snapshot: Option<DeviceSnapshot>,
    /// A section whose last read failed while an older list was kept.
    pub(crate) section_errors: HashMap<SectionId, SharedString>,
    reading: Option<Task<()>>,
    pub(crate) loaded_once: bool,
    pub(crate) operations: HashMap<DeviceKey, Operation>,
    pub(crate) device_errors: HashMap<DeviceKey, DeviceError>,
    /// Apps found in the trusted folders at the last read.
    pub(crate) detected_apps: Vec<AppProject>,
    pub(crate) app_choice: Option<AppChoice>,
    pub(crate) starting_runs: HashMap<RunKey, StartingRun>,
    pub(crate) active_runs: HashMap<RunKey, ActiveRun>,
    pub(crate) run_count: u64,
    pub(crate) run_terminals: Rc<dyn RunTerminals>,
    /// Numbers each read of the devices as it starts.
    read_generation: u64,
    /// The number of the read whose result is shown.
    applied_generation: u64,
}

impl DevicesPanel {
    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> Result<Entity<Self>> {
        workspace.update_in(&mut cx, |workspace, _window, cx| {
            let host = Arc::new(LocalHost::new());
            let log_dir = paths::logs_dir().join("devices");
            cx.new(|cx| {
                Self::new(
                    workspace.project(),
                    workspace.weak_handle(),
                    host.clone(),
                    host,
                    log_dir,
                    cx,
                )
            })
        })
    }

    pub(crate) fn new(
        project: &Entity<Project>,
        workspace: WeakEntity<Workspace>,
        host: Arc<dyn HostSystem>,
        runner: Arc<dyn ToolRunner>,
        log_dir: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            workspace: workspace.clone(),
            project: project.downgrade(),
            host,
            runner,
            log_dir,
            snapshot: None,
            section_errors: HashMap::new(),
            reading: None,
            loaded_once: false,
            operations: HashMap::new(),
            device_errors: HashMap::new(),
            detected_apps: Vec::new(),
            app_choice: None,
            starting_runs: HashMap::new(),
            active_runs: HashMap::new(),
            run_count: 0,
            run_terminals: Rc::new(WorkspaceTerminals { workspace }),
            read_generation: 0,
            applied_generation: 0,
        }
    }

    /// Simulators and emulators of a remote or shared project would be this
    /// machine's, which have nothing to do with it.
    pub(crate) fn is_supported(&self, cx: &App) -> bool {
        self.project
            .upgrade()
            .is_some_and(|project| project.read(cx).is_local())
    }

    pub(crate) fn is_reading(&self) -> bool {
        self.reading.is_some()
    }

    pub(crate) fn reload(&mut self, cx: &mut Context<Self>) {
        if !self.is_supported(cx) || self.reading.is_some() {
            return;
        }
        self.loaded_once = true;
        let host = self.host.clone();
        let runner = self.runner.clone();
        // Project files are read only in folders that passed the trust check.
        let app_roots = self.app_roots(cx);
        let generation = self.begin_read();
        self.reading = Some(cx.spawn(async move |this, cx| {
            let (snapshot, apps) = cx
                .background_spawn(async move {
                    let apps = app_roots.detect(&*host);
                    (devices::discover(&*host, &*runner).await, apps)
                })
                .await;
            this.update(cx, |this, cx| {
                this.reading = None;
                // Only a Reload looks for apps, and one Reload runs at a
                // time, so this list is the newest even when the devices
                // are not.
                this.detected_apps = apps;
                this.apply_read(generation, snapshot);
                cx.notify();
            })
            .log_err();
        }));
        cx.notify();
    }

    /// The read after a wireless connect. One already in flight began before
    /// the device existed, so it is replaced, not waited for.
    pub(crate) fn reload_fresh(&mut self, cx: &mut Context<Self>) {
        self.reading = None;
        self.reload(cx);
    }

    /// Numbers a read of the devices as it starts. Reads overlap (a Reload
    /// and a Run's own look), and the one started last knows most.
    pub(crate) fn begin_read(&mut self) -> u64 {
        self.read_generation += 1;
        self.read_generation
    }

    /// Shows the result of read `generation`, unless a read started after it
    /// is already shown. Returns whether it was shown.
    pub(crate) fn apply_read(&mut self, generation: u64, new: DeviceSnapshot) -> bool {
        if generation < self.applied_generation {
            return false;
        }
        self.applied_generation = generation;
        self.apply_snapshot(new);
        true
    }

    fn apply_snapshot(&mut self, new: DeviceSnapshot) {
        let (merged, errors) = merge_snapshot(self.snapshot.as_ref(), new);
        self.snapshot = Some(merged);
        self.section_errors = errors;
        // A device being started or stopped keeps its transient state across a
        // read: the read cannot know about a command still being followed.
        let transient: Vec<(DeviceKey, DeviceState)> = self
            .operations
            .iter()
            .filter_map(|(key, operation)| Some((key.clone(), operation.shown_state()?)))
            .collect();
        for (key, state) in transient {
            self.set_device_state(&key, state);
        }
        let live: Vec<DeviceKey> = self
            .all_devices()
            .map(|device| device.key.clone())
            .collect();
        self.device_errors.retain(|key, _| live.contains(key));
        self.recount_running(None);
    }
}
