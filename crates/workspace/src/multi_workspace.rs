use anyhow::Result;
use collections::HashMap;
use fs::Fs;

use gpui::{
    AnyView, App, Context, DragMoveEvent, Entity, EntityId, EventEmitter, FocusHandle, Focusable,
    ManagedView, MouseButton, Pixels, Render, Subscription, Task, Tiling, WeakEntity, Window,
    WindowId, actions, deferred, px,
};
pub use project::ProjectGroupKey;
use project::{DisableAiSettings, Project, ProjectActivity};
use remote::RemoteConnectionOptions;
use settings::Settings;
pub use settings::SidebarSide;
use std::future::Future;

#[cfg(any(test, feature = "test-support"))]
use gpui::UpdateGlobal;

use std::path::PathBuf;
use std::time::{Duration, Instant};
use ui::prelude::*;
use util::ResultExt;
use util::path_list::PathList;

use crate::Toast;
use crate::notifications::NotificationId;

const SIDEBAR_RESIZE_HANDLE_SIZE: Pixels = px(6.0);

use crate::open_remote_project_with_existing_connection;
use crate::{
    CloseIntent, CloseWindow, DockPosition, Event as WorkspaceEvent, Item, ModalView, OpenMode,
    Panel, Workspace, WorkspaceId, WorkspaceSettings, client_side_decorations,
    persistence::model::MultiWorkspaceState,
};

actions!(
    multi_workspace,
    [
        /// Toggles the workspace switcher sidebar.
        ToggleWorkspaceSidebar,
        /// Closes the workspace sidebar.
        CloseWorkspaceSidebar,
        /// Moves focus to or from the workspace sidebar without closing it.
        FocusWorkspaceSidebar,
        /// Activates the next project in the sidebar.
        NextProject,
        /// Activates the previous project in the sidebar.
        PreviousProject,
        /// Moves the active project to a new window.
        MoveProjectToNewWindow,
        /// Logs resource stats (language servers, buffers, worktree
        /// entries, terminal scrollback, activity) for every project
        /// tracked by this window.
        DumpProjectResourceStats,
    ]
);

#[derive(Default)]
pub struct SidebarRenderState {
    /// Whether the wide project panel is showing.
    pub open: bool,
    /// Whether the always-visible project rail is present. Independent of
    /// `open`: the rail occupies the window's `side` edge even with the
    /// panel closed.
    pub rail: bool,
    pub side: SidebarSide,
    /// How much of the `side` edge the sidebar actually covers. The title bar
    /// needs the width, not just `occupies`: the rail alone is narrower than
    /// the strip macOS draws its window controls over, so the title bar still
    /// has to reserve the remainder or the controls land on its content.
    pub edge_width: Pixels,
}

impl SidebarRenderState {
    /// Whether the sidebar covers the window edge on `side` -- either
    /// component is enough to push the title bar off that edge.
    pub fn occupies(&self, side: SidebarSide) -> bool {
        self.side == side && (self.open || self.rail)
    }
}

pub enum MultiWorkspaceEvent {
    ActiveWorkspaceChanged {
        source_workspace: Option<WeakEntity<Workspace>>,
    },
    WorkspaceAdded(Entity<Workspace>),
    WorkspaceRemoved(EntityId),
    ProjectGroupsChanged,
}

pub enum SidebarEvent {
    SerializeNeeded,
}

pub trait Sidebar: Focusable + Render + EventEmitter<SidebarEvent> + Sized {
    /// Width of the wide project panel alone, excluding the rail.
    fn width(&self, cx: &App) -> Pixels;
    fn set_width(&mut self, width: Option<Pixels>, cx: &mut Context<Self>);
    /// Width of the always-visible rail, which the sidebar renders whether
    /// or not the panel is open. `MultiWorkspace` needs it to size the
    /// sidebar container and to offset the panel's resize drag.
    fn rail_width(&self, _cx: &App) -> Pixels {
        px(0.0)
    }
    fn has_notifications(&self, cx: &App) -> bool;
    fn side(&self, _cx: &App) -> SidebarSide;

    /// Makes focus reset back to the search editor upon toggling the sidebar from outside
    fn prepare_for_focus(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    // TODO(phase-07): project cycling now lives on `MultiWorkspace::cycle_project`,
    // driven directly by `retained_workspaces` order, so `NextProject`/
    // `PreviousProject` no longer call this. Revisit whether the sidebar
    // should still own or intercept it now that the sidebar crate has been
    // rebuilt as a plain project list with no secondary view to branch on.
    /// Activates the next or previous project.
    fn cycle_project(&mut self, _forward: bool, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// Return an opaque JSON blob of sidebar-specific state to persist.
    fn serialized_state(&self, _cx: &App) -> Option<String> {
        None
    }

    /// Restore sidebar state from a previously-serialized blob.
    fn restore_serialized_state(
        &mut self,
        _state: &str,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}

pub trait SidebarHandle: 'static + Send + Sync {
    fn width(&self, cx: &App) -> Pixels;
    fn set_width(&self, width: Option<Pixels>, cx: &mut App);
    fn rail_width(&self, cx: &App) -> Pixels;
    fn focus_handle(&self, cx: &App) -> FocusHandle;
    fn focus(&self, window: &mut Window, cx: &mut App);
    fn prepare_for_focus(&self, window: &mut Window, cx: &mut App);
    fn has_notifications(&self, cx: &App) -> bool;
    fn to_any(&self) -> AnyView;
    fn entity_id(&self) -> EntityId;
    fn cycle_project(&self, forward: bool, window: &mut Window, cx: &mut App);

    fn side(&self, cx: &App) -> SidebarSide;
    fn serialized_state(&self, cx: &App) -> Option<String>;
    fn restore_serialized_state(&self, state: &str, window: &mut Window, cx: &mut App);
}

#[derive(Clone)]
pub struct DraggedSidebar;

impl Render for DraggedSidebar {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// Every method here borrows the sidebar entity (`read`/`update`), so any
/// `MultiWorkspace` method that reaches this trait — `sidebar_side`,
/// `open_sidebar`, `close_sidebar`, `toggle_sidebar`, `focus_sidebar`,
/// `sidebar_has_notifications` — inherits that borrow.
///
/// UI that lives *inside* the sidebar therefore must not call those directly:
/// a `cx.listener` body runs within `Sidebar::update`, so re-entering panics
/// with "cannot read Sidebar while it is already being updated". Dispatch the
/// matching action instead (`ToggleWorkspaceSidebar`, `FocusWorkspaceSidebar`,
/// `CloseWorkspaceSidebar`) — `Window::dispatch_action` defers, so the borrow
/// is released before the handler runs.
impl<T: Sidebar> SidebarHandle for Entity<T> {
    fn width(&self, cx: &App) -> Pixels {
        self.read(cx).width(cx)
    }

    fn set_width(&self, width: Option<Pixels>, cx: &mut App) {
        self.update(cx, |this, cx| this.set_width(width, cx))
    }

    fn rail_width(&self, cx: &App) -> Pixels {
        self.read(cx).rail_width(cx)
    }

    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.read(cx).focus_handle(cx)
    }

    fn focus(&self, window: &mut Window, cx: &mut App) {
        let handle = self.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }

    fn prepare_for_focus(&self, window: &mut Window, cx: &mut App) {
        self.update(cx, |this, cx| this.prepare_for_focus(window, cx));
    }

    fn has_notifications(&self, cx: &App) -> bool {
        self.read(cx).has_notifications(cx)
    }

    fn to_any(&self) -> AnyView {
        self.clone().into()
    }

    fn entity_id(&self) -> EntityId {
        Entity::entity_id(self)
    }

    fn cycle_project(&self, forward: bool, window: &mut Window, cx: &mut App) {
        let entity = self.clone();
        window.defer(cx, move |window, cx| {
            entity.update(cx, |this, cx| {
                this.cycle_project(forward, window, cx);
            });
        });
    }

    fn side(&self, cx: &App) -> SidebarSide {
        self.read(cx).side(cx)
    }

    fn serialized_state(&self, cx: &App) -> Option<String> {
        self.read(cx).serialized_state(cx)
    }

    fn restore_serialized_state(&self, state: &str, window: &mut Window, cx: &mut App) {
        self.update(cx, |this, cx| {
            this.restore_serialized_state(state, window, cx)
        })
    }
}

#[derive(Clone)]
pub struct ProjectGroup {
    pub key: ProjectGroupKey,
    pub workspaces: Vec<Entity<Workspace>>,
    pub expanded: bool,
}

pub struct SerializedProjectGroupState {
    pub key: ProjectGroupKey,
    pub expanded: bool,
}

#[derive(Clone)]
pub struct ProjectGroupState {
    pub key: ProjectGroupKey,
    pub expanded: bool,
    pub last_active_workspace: Option<WeakEntity<Workspace>>,
}

/// FR3 (Phase 6 of multi-project-window-switching): how the memory-pressure
/// fuse learns about system memory pressure, abstracted behind a trait so
/// the fuse's victim-selection logic (see `MultiWorkspace::memory_governor_tick`)
/// is unit-testable without touching the real OS. Production uses
/// `SysinfoMemoryPressureReader`; tests inject their own — deliberately
/// never read `sysinfo` directly from the decision logic itself (phase-06's
/// Implementation Steps, step 7).
pub trait MemoryPressureReader {
    /// Percentage (0.0-100.0) of total system memory currently available,
    /// or `None` if it could not be read this cycle (e.g. the platform
    /// call failed) — the fuse treats that as "try again next poll"
    /// rather than as pressure.
    fn available_memory_percent(&mut self) -> Option<f32>;
}

struct SysinfoMemoryPressureReader {
    system: sysinfo::System,
}

impl SysinfoMemoryPressureReader {
    fn new() -> Self {
        Self {
            system: sysinfo::System::new(),
        }
    }
}

impl MemoryPressureReader for SysinfoMemoryPressureReader {
    fn available_memory_percent(&mut self) -> Option<f32> {
        // Memory-only refresh (no process list) — matches
        // `system_specs.rs`'s existing convention and NFR1's "polling must
        // not cost anything noticeable".
        self.system.refresh_memory();
        let total = self.system.total_memory();
        if total == 0 {
            return None;
        }
        Some(self.system.available_memory() as f32 / total as f32 * 100.0)
    }
}

/// FR3: how often the memory-pressure fuse polls system memory. NFR1
/// requires this to be infrequent and off the foreground thread — the
/// governor task runs entirely on `cx.background_executor()`.
const MEMORY_FUSE_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// FR4b: a project must have sat `Warm` for at least this long before the
/// fuse may pick it as a victim — it must never hibernate a project the
/// user only just defocused.
///
/// **Invariant this relies on staying true:** this must stay `>
/// MEMORY_FUSE_POLL_INTERVAL`. `manually_woken_at`'s own "immune for one
/// poll cycle after a manual wake" check (`select_memory_fuse_victim`) can
/// only ever matter for a candidate this check has *not* already
/// rejected, because `wake_project` and `schedule_hibernate` always stamp
/// `manually_woken_at` and `warm_since` at the same moment (a workspace
/// only re-enters `Warm` by losing focus, and the same `activate()` call
/// that stamps a fresh `manually_woken_at` for the incoming workspace also
/// stamps a fresh `warm_since` for the outgoing one) — so
/// `now - manually_woken_at` and `now - warm_since` can never diverge by
/// more than one `activate()` round trip. If this constant is ever
/// lowered to `<= MEMORY_FUSE_POLL_INTERVAL`, the manual-wake-immunity
/// check becomes live (not merely redundant) and needs its own isolated
/// test — today it is provably always shadowed by this one, which is why
/// no such test exists yet (see phase-06's Todo List).
const MEMORY_FUSE_MIN_WARM_DURATION: Duration = Duration::from_secs(60);

/// FR4b hysteresis: once the fuse hibernates a victim, it will not trigger
/// again for this many poll cycles, even if pressure is still (or once
/// more) under the threshold. Prevents the fuse from re-arming and
/// flapping while memory oscillates right at the boundary.
const MEMORY_FUSE_HYSTERESIS_CYCLES: u32 = 2;

/// Marker type for the fuse's toast notification (`NotificationId::unique`
/// needs a type, not a value) — see `MultiWorkspace::notify_memory_fuse_triggered`.
/// `pub(crate)` so tests can assert on it via `Workspace::notification_ids`.
pub(crate) struct MemoryPressureFuseToast;

pub struct MultiWorkspace {
    window_id: WindowId,
    retained_workspaces: Vec<Entity<Workspace>>,
    project_groups: Vec<ProjectGroupState>,
    active_workspace: Entity<Workspace>,
    sidebar: Option<Box<dyn SidebarHandle>>,
    sidebar_open: bool,
    sidebar_overlay: Option<AnyView>,
    pending_removal_tasks: Vec<Task<()>>,
    _serialize_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
    previous_focus_handle: Option<FocusHandle>,
    /// Pending hibernate-after-idle timers, keyed by the `EntityId` of the
    /// workspace whose project they'll hibernate. Dropping the `Task`
    /// cancels it (GPUI semantics), so removing an entry is the entire
    /// cancellation mechanism — see `wake_project`/`schedule_hibernate`.
    hibernate_timers: HashMap<EntityId, Task<()>>,
    /// FR4b: instant a workspace's project most recently entered `Warm`,
    /// plus a weak handle to that *project*, keyed by the workspace's
    /// `EntityId`. Lets the memory-pressure fuse require a project to have
    /// sat idle at least `MEMORY_FUSE_MIN_WARM_DURATION` before picking it
    /// as a victim, and enumerate every `Warm` project directly rather
    /// than through `self.workspaces()` — that iterator is
    /// `retained_workspaces` plus the active workspace, and `activate()`'s
    /// outgoing workspace is deliberately *not* added to
    /// `retained_workspaces` unless it already was (see `activate()`'s own
    /// comment), so a window's very first workspace going `Warm` for the
    /// first time would otherwise be invisible to victim selection despite
    /// legitimately being `Warm`.
    ///
    /// Deliberately a *project* handle, not a workspace one: nothing in
    /// `MultiWorkspace` keeps a defocused, never-independently-retained
    /// workspace's `Entity<Workspace>` alive at all once `activate()`
    /// reassigns `self.active_workspace` away from it (the struct's only
    /// strong reference to it) — the shell can be dropped out from under
    /// this bookkeeping while its `Project` lives on, held by its buffers,
    /// editors, and everything else that actually cares about project
    /// state. A `WeakEntity<Workspace>` here would go stale exactly when
    /// that happens; the project itself does not.
    ///
    /// The handle is `Weak` regardless (matching `hibernate_timers`'s own
    /// `weak_project` inside its timer closure, and
    /// `ProjectGroupState::last_active_workspace`'s convention in this
    /// file) so this map is never what keeps a project alive either;
    /// `detach_workspace` still removes the entry outright on close. Set
    /// in `schedule_hibernate`, cleared in `wake_project` and
    /// `detach_workspace` so a stale entry never outlives the `Warm` state
    /// it describes.
    warm_since: HashMap<EntityId, (Instant, WeakEntity<Project>)>,
    /// FR4b: instant a workspace was last woken *manually* via
    /// `activate()`, keyed by `EntityId`. The fuse skips a project for one
    /// full poll cycle after this — "a manual wake always beats the
    /// fuse". Set in `wake_project`, cleared in `detach_workspace`.
    manually_woken_at: HashMap<EntityId, Instant>,
    /// FR4b hysteresis: instant the fuse last actually hibernated a
    /// victim, or `None` before the first trigger.
    fuse_last_triggered_at: Option<Instant>,
    /// FR3: how the fuse reads system memory pressure. Boxed so tests can
    /// inject synthetic pressure (see `MemoryPressureReader`) — production
    /// always starts with `SysinfoMemoryPressureReader`.
    memory_pressure_reader: Box<dyn MemoryPressureReader>,
    /// FR3: recurring background timer driving `memory_governor_tick`.
    /// Stored so dropping `MultiWorkspace` cancels it (GPUI `Task` drop
    /// semantics) rather than leaking a loop that outlives its window.
    _memory_governor_task: Task<()>,
}

impl EventEmitter<MultiWorkspaceEvent> for MultiWorkspace {}

impl MultiWorkspace {
    pub fn sidebar_side(&self, cx: &App) -> SidebarSide {
        self.sidebar
            .as_ref()
            .map_or(SidebarSide::Left, |s| s.side(cx))
    }

    pub fn sidebar_render_state(&self, cx: &App) -> SidebarRenderState {
        let enabled = self.multi_workspace_enabled(cx);
        let open = self.sidebar_open() && enabled;
        let rail = self.sidebar.is_some() && enabled;
        SidebarRenderState {
            open,
            rail,
            side: self.sidebar_side(cx),
            // Mirrors the container width in `render`: the rail is always drawn,
            // the panel only when open.
            edge_width: match (&self.sidebar, rail) {
                (Some(sidebar), true) => {
                    sidebar.rail_width(cx) + if open { sidebar.width(cx) } else { px(0.) }
                }
                _ => px(0.),
            },
        }
    }

    pub fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let release_subscription = cx.on_release(|this: &mut MultiWorkspace, _cx| {
            if let Some(task) = this._serialize_task.take() {
                task.detach();
            }
            for task in std::mem::take(&mut this.pending_removal_tasks) {
                task.detach();
            }
        });
        let quit_subscription = cx.on_app_quit(Self::app_will_quit);
        let settings_subscription = cx.observe_global_in::<settings::SettingsStore>(window, {
            let mut previous_disable_ai = DisableAiSettings::get_global(cx).disable_ai;
            move |this, window, cx| {
                if DisableAiSettings::get_global(cx).disable_ai != previous_disable_ai {
                    this.collapse_to_single_workspace(window, cx);
                    previous_disable_ai = DisableAiSettings::get_global(cx).disable_ai;
                }
            }
        });
        // Hibernation can be turned off live (`hibernate_after_ms: 0`).
        // Whenever settings change and it currently resolves to disabled,
        // make sure no hibernate timer is left pending — dropping the
        // `Task` cancels it. Re-checked unconditionally rather than only on
        // a Some->None transition, since clearing an already-empty map is a
        // harmless no-op and this avoids tracking yet another `previous_*`.
        //
        // Deliberately one-directional (flagged in Phase 2 review, not
        // fixed here): disabling does not wake an already-`Hibernated`
        // project, and re-enabling does not retroactively schedule a timer
        // for a project that's already `Warm`. Both are left to a later
        // phase rather than guessed at now, because nothing consumes
        // `ProjectActivity` for a real resource yet (every transition in
        // this phase is a no-op with an event) — there is nothing to check
        // a "wake to what" answer against. And it isn't just unvalidated,
        // it's actively constrained: waking straight to `Active` would put
        // an unfocused project in the one state defined as "the window's
        // focused workspace" (see `ProjectActivity::Active`'s doc comment),
        // and waking to `Warm` would require legitimizing the
        // `Hibernated -> Warm` edge that `Project::set_activity` guards
        // against precisely because it's off the state diagram. Whichever
        // phase gives hibernation a real resource effect should design
        // reactivation semantics against that effect, not against a guess
        // made here.
        let hibernate_settings_subscription =
            cx.observe_global_in::<settings::SettingsStore>(window, |this, _window, cx| {
                if WorkspaceSettings::get_global(cx)
                    .multi_project
                    .hibernate_after
                    .is_none()
                {
                    this.hibernate_timers.clear();
                }
            });
        Self::subscribe_to_workspace(&workspace, window, cx);
        let weak_self = cx.weak_entity();
        workspace.update(cx, |workspace, cx| {
            workspace.set_multi_workspace(weak_self, cx);
        });
        // FR3: one long-lived poll loop per window, started unconditionally
        // — `memory_governor_tick` itself checks
        // `multi_project.memory_pressure_threshold_percent` every cycle and
        // no-ops when the fuse is disabled, the same "always running,
        // cheaply re-checks settings" shape as `hibernate_settings_subscription`
        // above, so there's nothing to start/stop when the setting changes live.
        let memory_governor_task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(MEMORY_FUSE_POLL_INTERVAL)
                    .await;
                if this
                    .update(cx, |this, cx| this.memory_governor_tick(cx))
                    .log_err()
                    .is_none()
                {
                    break;
                }
            }
        });
        Self {
            window_id: window.window_handle().window_id(),
            retained_workspaces: Vec::new(),
            project_groups: Vec::new(),
            active_workspace: workspace,
            sidebar: None,
            sidebar_open: false,
            sidebar_overlay: None,
            pending_removal_tasks: Vec::new(),
            _serialize_task: None,
            _subscriptions: vec![
                release_subscription,
                quit_subscription,
                settings_subscription,
                hibernate_settings_subscription,
            ],
            previous_focus_handle: None,
            hibernate_timers: HashMap::default(),
            warm_since: HashMap::default(),
            manually_woken_at: HashMap::default(),
            fuse_last_triggered_at: None,
            memory_pressure_reader: Box::new(SysinfoMemoryPressureReader::new()),
            _memory_governor_task: memory_governor_task,
        }
    }

    pub fn register_sidebar<T: Sidebar>(&mut self, sidebar: Entity<T>, cx: &mut Context<Self>) {
        self._subscriptions
            .push(cx.observe(&sidebar, |_this, _, cx| {
                cx.notify();
            }));
        self._subscriptions
            .push(cx.subscribe(&sidebar, |this, _, event, cx| match event {
                SidebarEvent::SerializeNeeded => {
                    this.serialize(cx);
                }
            }));
        self.sidebar = Some(Box::new(sidebar));
    }

    pub fn sidebar(&self) -> Option<&dyn SidebarHandle> {
        self.sidebar.as_deref()
    }

    pub fn set_sidebar_overlay(&mut self, overlay: Option<AnyView>, cx: &mut Context<Self>) {
        self.sidebar_overlay = overlay;
        cx.notify();
    }

    pub fn sidebar_open(&self) -> bool {
        self.sidebar_open
    }

    pub fn sidebar_has_notifications(&self, cx: &App) -> bool {
        self.sidebar
            .as_ref()
            .map_or(false, |s| s.has_notifications(cx))
    }

    pub fn multi_workspace_enabled(&self, cx: &App) -> bool {
        !DisableAiSettings::get_global(cx).disable_ai
    }

    pub fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.multi_workspace_enabled(cx) {
            return;
        }

        if self.sidebar_open() {
            self.close_sidebar(window, cx);
        } else {
            self.previous_focus_handle = window.focused(cx);
            self.open_sidebar(cx);
            if let Some(sidebar) = &self.sidebar {
                sidebar.prepare_for_focus(window, cx);
                sidebar.focus(window, cx);
            }
        }
    }

    pub fn close_sidebar_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.multi_workspace_enabled(cx) {
            return;
        }

        if self.sidebar_open() {
            self.close_sidebar(window, cx);
        }
    }

    pub fn focus_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.multi_workspace_enabled(cx) {
            return;
        }

        if self.sidebar_open() {
            let sidebar_is_focused = self
                .sidebar
                .as_ref()
                .is_some_and(|s| s.focus_handle(cx).contains_focused(window, cx));

            if sidebar_is_focused {
                self.restore_previous_focus(false, window, cx);
            } else {
                self.previous_focus_handle = window.focused(cx);
                if let Some(sidebar) = &self.sidebar {
                    sidebar.prepare_for_focus(window, cx);
                    sidebar.focus(window, cx);
                }
            }
        } else {
            self.previous_focus_handle = window.focused(cx);
            self.open_sidebar(cx);
            if let Some(sidebar) = &self.sidebar {
                sidebar.prepare_for_focus(window, cx);
                sidebar.focus(window, cx);
            }
        }
    }

    /// Opens the sidebar. This is a live, user-driven trigger — bound to a
    /// real keybinding (`ToggleWorkspaceSidebar`/`FocusWorkspaceSidebar`)
    /// even before Phase 7 rebuilds the sidebar UI — so retention respects
    /// `should_retain()` like every other live path, rather than
    /// unconditionally retaining the active workspace.
    pub fn open_sidebar(&mut self, cx: &mut Context<Self>) {
        let side = match self.sidebar_side(cx) {
            SidebarSide::Left => "left",
            SidebarSide::Right => "right",
        };
        telemetry::event!("Sidebar Toggled", action = "open", side = side);
        self.apply_open_sidebar(true, cx);
    }

    /// Restores the sidebar to open state from persisted session data
    /// without firing a telemetry event, since this is not a user-initiated
    /// action. Always retains the active workspace regardless of the live
    /// `retain_background_projects` setting — this is reconstructing a
    /// previously-saved `sidebar_open: true` session (NFR2), not a new
    /// user decision being made right now.
    pub(crate) fn restore_open_sidebar(&mut self, cx: &mut Context<Self>) {
        self.apply_open_sidebar(false, cx);
    }

    fn apply_open_sidebar(&mut self, respect_retention_policy: bool, cx: &mut Context<Self>) {
        self.sidebar_open = true;
        if !respect_retention_policy || self.should_retain(cx) {
            self.retain_active_workspace(cx);
        }
        let sidebar_focus_handle = self.sidebar.as_ref().map(|s| s.focus_handle(cx));
        for workspace in self.retained_workspaces.clone() {
            workspace.update(cx, |workspace, _cx| {
                workspace.set_sidebar_focus_handle(sidebar_focus_handle.clone());
            });
        }
        self.serialize(cx);
        cx.notify();
    }

    pub fn close_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let side = match self.sidebar_side(cx) {
            SidebarSide::Left => "left",
            SidebarSide::Right => "right",
        };
        telemetry::event!("Sidebar Toggled", action = "close", side = side);
        self.sidebar_open = false;
        for workspace in self.retained_workspaces.clone() {
            workspace.update(cx, |workspace, _cx| {
                workspace.set_sidebar_focus_handle(None);
            });
        }
        let sidebar_has_focus = self
            .sidebar
            .as_ref()
            .is_some_and(|s| s.focus_handle(cx).contains_focused(window, cx));
        if sidebar_has_focus {
            self.restore_previous_focus(true, window, cx);
        } else {
            self.previous_focus_handle.take();
        }
        self.serialize(cx);
        cx.notify();
    }

    fn restore_previous_focus(&mut self, clear: bool, window: &mut Window, cx: &mut Context<Self>) {
        let focus_handle = if clear {
            self.previous_focus_handle.take()
        } else {
            self.previous_focus_handle.clone()
        };

        if let Some(previous_focus) = focus_handle {
            previous_focus.focus(window, cx);
        } else {
            let pane = self.workspace().read(cx).active_pane().clone();
            window.focus(&pane.read(cx).focus_handle(cx), cx);
        }
    }

    pub fn close_window(&mut self, _: &CloseWindow, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            let workspaces = this.update(cx, |multi_workspace, _cx| {
                multi_workspace.workspaces().cloned().collect::<Vec<_>>()
            })?;

            for workspace in &workspaces {
                let should_continue = workspace
                    .update_in(cx, |workspace, window, cx| {
                        workspace.prepare_to_close(CloseIntent::CloseWindow, window, cx)
                    })?
                    .await?;
                if !should_continue {
                    return anyhow::Ok(());
                }
            }

            // Flush every workspace's pending debounced serialization before
            // the window closes. If this turns out to be the last window,
            // `remove_window()` below triggers `cx.quit()` directly (see
            // `bind_on_window_closed` in zed.rs), which bypasses the `Quit`
            // action's own manual flush loop (crates/zed/src/zed.rs
            // `quit()`) — `app_will_quit` alone only covers this
            // `MultiWorkspace`'s own state, not each workspace's pane
            // layout. Best-effort: a flush failing here must not block the
            // window from closing.
            let flush_tasks: Vec<Task<()>> = workspaces
                .iter()
                .filter_map(|workspace| {
                    workspace
                        .update_in(cx, |workspace, window, cx| {
                            workspace.flush_serialization(window, cx)
                        })
                        .log_err()
                })
                .collect();
            futures::future::join_all(flush_tasks).await;

            cx.update(|window, _cx| {
                window.remove_window();
            })?;

            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
    }

    fn subscribe_to_workspace(
        workspace: &Entity<Workspace>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let project = workspace.read(cx).project().clone();
        cx.subscribe_in(&project, window, {
            let workspace = workspace.downgrade();
            move |this, _project, event, _window, cx| match event {
                project::Event::WorktreePathsChanged { old_worktree_paths } => {
                    if let Some(workspace) = workspace.upgrade() {
                        let host = workspace
                            .read(cx)
                            .project()
                            .read(cx)
                            .remote_connection_options(cx);
                        let old_key =
                            ProjectGroupKey::from_worktree_paths(old_worktree_paths, host);
                        this.handle_project_group_key_change(&workspace, &old_key, cx);
                    }
                }
                _ => {}
            }
        })
        .detach();

        cx.subscribe_in(workspace, window, |this, workspace, event, window, cx| {
            if let WorkspaceEvent::Activate = event {
                this.activate(workspace.clone(), None, window, cx);
            }
        })
        .detach();
    }

    fn handle_project_group_key_change(
        &mut self,
        workspace: &Entity<Workspace>,
        old_key: &ProjectGroupKey,
        cx: &mut Context<Self>,
    ) {
        if !self.is_workspace_retained(workspace) {
            return;
        }

        let new_key = workspace.read(cx).project_group_key(cx);
        if new_key.path_list().paths().is_empty() {
            return;
        }

        // The Project already emitted WorktreePathsChanged which the
        // sidebar handles for thread migration.
        self.rekey_project_group(old_key, &new_key, cx);
        self.serialize(cx);
        cx.notify();
    }

    pub fn is_workspace_retained(&self, workspace: &Entity<Workspace>) -> bool {
        self.retained_workspaces
            .iter()
            .any(|retained| retained == workspace)
    }

    pub fn active_workspace_is_retained(&self) -> bool {
        self.is_workspace_retained(&self.active_workspace)
    }

    pub fn retained_workspaces(&self) -> &[Entity<Workspace>] {
        &self.retained_workspaces
    }

    /// Ensures a project group exists for `key`, creating one if needed.
    fn ensure_project_group_state(&mut self, key: ProjectGroupKey) {
        if key.path_list().paths().is_empty() {
            return;
        }

        if self.project_groups.iter().any(|group| group.key == key) {
            return;
        }

        self.project_groups.insert(
            0,
            ProjectGroupState {
                key,
                expanded: true,
                last_active_workspace: None,
            },
        );
    }

    /// Transitions a project group from `old_key` to `new_key`.
    ///
    /// On collision (both keys have groups), the active workspace's
    /// Re-keys a project group from `old_key` to `new_key`, handling
    /// collisions. When two groups collide, the active workspace's
    /// group always wins. Otherwise the old key's state is preserved
    /// — it represents the group the user or system just acted on.
    /// The losing group is removed, and the winner is re-keyed in
    /// place to preserve sidebar order.
    fn rekey_project_group(
        &mut self,
        old_key: &ProjectGroupKey,
        new_key: &ProjectGroupKey,
        cx: &App,
    ) {
        if old_key == new_key {
            return;
        }

        if new_key.path_list().paths().is_empty() {
            return;
        }

        let old_key_exists = self.project_groups.iter().any(|g| g.key == *old_key);
        let new_key_exists = self.project_groups.iter().any(|g| g.key == *new_key);

        if !old_key_exists {
            self.ensure_project_group_state(new_key.clone());
            return;
        }

        if new_key_exists {
            let active_key = self.active_workspace.read(cx).project_group_key(cx);
            if active_key == *new_key {
                self.project_groups.retain(|g| g.key != *old_key);
            } else {
                self.project_groups.retain(|g| g.key != *new_key);
                if let Some(group) = self.project_groups.iter_mut().find(|g| g.key == *old_key) {
                    group.key = new_key.clone();
                }
            }
        } else {
            if let Some(group) = self.project_groups.iter_mut().find(|g| g.key == *old_key) {
                group.key = new_key.clone();
            }
        }

        // If another retained workspace still has the old key (e.g. a
        // linked worktree workspace), re-create the old group so it
        // remains reachable in the sidebar.
        let other_workspace_needs_old_key = self
            .retained_workspaces
            .iter()
            .any(|ws| ws.read(cx).project_group_key(cx) == *old_key);
        if other_workspace_needs_old_key {
            self.ensure_project_group_state(old_key.clone());
        }
    }

    pub(crate) fn retain_workspace(
        &mut self,
        workspace: Entity<Workspace>,
        key: ProjectGroupKey,
        cx: &mut Context<Self>,
    ) {
        self.ensure_project_group_state(key);
        if self.is_workspace_retained(&workspace) {
            return;
        }

        self.retained_workspaces.push(workspace.clone());
        cx.emit(MultiWorkspaceEvent::WorkspaceAdded(workspace));
    }

    pub(crate) fn activate_provisional_workspace(
        &mut self,
        workspace: Entity<Workspace>,
        provisional_key: ProjectGroupKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if workspace != self.active_workspace {
            self.register_workspace(&workspace, window, cx);
        }

        // The project-group metadata is created either way — only the
        // workspace's own retention is gated. Routed through the same
        // policy as `activate()` (see `should_retain()`), so this live,
        // user-driven path (opening a remote/SSH project) doesn't retain
        // in the background when `retain_background_projects` is `false`.
        self.ensure_project_group_state(provisional_key.clone());
        if self.should_retain(cx) {
            self.retain_workspace(workspace.clone(), provisional_key, cx);
        }

        self.activate(workspace, None, window, cx);
    }

    fn register_workspace(
        &mut self,
        workspace: &Entity<Workspace>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        Self::subscribe_to_workspace(workspace, window, cx);
        let weak_self = cx.weak_entity();
        workspace.update(cx, |workspace, cx| {
            workspace.set_multi_workspace(weak_self, cx);
        });

        let entity = cx.entity();
        cx.defer({
            let workspace = workspace.clone();
            move |cx| {
                entity.update(cx, |this, cx| {
                    this.sync_sidebar_to_workspace(&workspace, cx);
                })
            }
        });
    }

    pub fn project_group_key_for_workspace(
        &self,
        workspace: &Entity<Workspace>,
        cx: &App,
    ) -> ProjectGroupKey {
        workspace.read(cx).project_group_key(cx)
    }

    pub fn restore_project_groups(
        &mut self,
        groups: Vec<SerializedProjectGroupState>,
        _cx: &mut Context<Self>,
    ) {
        let mut restored: Vec<ProjectGroupState> = Vec::new();
        for SerializedProjectGroupState { key, expanded } in groups {
            if key.path_list().paths().is_empty() {
                continue;
            }
            if restored.iter().any(|group| group.key == key) {
                continue;
            }
            restored.push(ProjectGroupState {
                key,
                expanded,
                last_active_workspace: None,
            });
        }
        for existing in std::mem::take(&mut self.project_groups) {
            if !restored.iter().any(|group| group.key == existing.key) {
                restored.push(existing);
            }
        }
        self.project_groups = restored;
    }

    pub fn project_group_keys(&self) -> Vec<ProjectGroupKey> {
        self.project_groups
            .iter()
            .map(|group| group.key.clone())
            .collect()
    }

    fn derived_project_groups(&self, cx: &App) -> Vec<ProjectGroup> {
        let mut groups: Vec<ProjectGroup> = self
            .project_groups
            .iter()
            .map(|group| ProjectGroup {
                key: group.key.clone(),
                workspaces: self
                    .retained_workspaces
                    .iter()
                    .filter(|workspace| workspace.read(cx).project_group_key(cx) == group.key)
                    .cloned()
                    .collect(),
                expanded: group.expanded,
            })
            .collect();

        // The window's own workspace has to appear in its own group, and two
        // separate things can leave it out:
        //
        // - No group at all. Both paths into `ensure_project_group_state` are
        //   about ADDING a project, and `restore_project_groups` only replays
        //   what an earlier session wrote -- nothing, the first time. A window
        //   opened straight onto one folder drew an empty rail.
        // - A group with the right key but no workspaces. Group state is
        //   restored from disk while `retained_workspaces` starts empty, and the
        //   active workspace is only retained when the sidebar opens. Reopening
        //   a project with the sidebar closed left the rail listing it while
        //   nothing marked it active -- the entry is matched by workspace, not
        //   by key (see `sidebar::project_list`).
        //
        // So this attaches the workspace rather than merely ensuring a key, and
        // does it on read rather than on open: the paths arrive asynchronously,
        // so a write would have to pick a moment, and picking the wrong one is
        // how this went missing in the first place.
        let active_key = self.active_workspace.read(cx).project_group_key(cx);
        if !active_key.path_list().paths().is_empty() {
            match groups.iter_mut().find(|group| group.key == active_key) {
                Some(group) if !group.workspaces.contains(&self.active_workspace) => {
                    group.workspaces.insert(0, self.active_workspace.clone());
                }
                Some(_) => {}
                None => groups.insert(
                    0,
                    ProjectGroup {
                        key: active_key,
                        workspaces: vec![self.active_workspace.clone()],
                        expanded: true,
                    },
                ),
            }
        }

        groups
    }

    pub fn project_groups(&self, cx: &App) -> Vec<ProjectGroup> {
        self.derived_project_groups(cx)
    }

    pub fn last_active_workspace_for_group(
        &self,
        key: &ProjectGroupKey,
        cx: &App,
    ) -> Option<Entity<Workspace>> {
        let group = self.project_groups.iter().find(|g| g.key == *key)?;
        let weak = group.last_active_workspace.as_ref()?;
        let workspace = weak.upgrade()?;
        (workspace.read(cx).project_group_key(cx) == *key).then_some(workspace)
    }

    pub fn group_state_by_key(&self, key: &ProjectGroupKey) -> Option<&ProjectGroupState> {
        self.project_groups.iter().find(|group| group.key == *key)
    }

    pub fn group_state_by_key_mut(
        &mut self,
        key: &ProjectGroupKey,
    ) -> Option<&mut ProjectGroupState> {
        self.project_groups
            .iter_mut()
            .find(|group| group.key == *key)
    }

    pub fn set_all_groups_expanded(&mut self, expanded: bool) {
        for group in &mut self.project_groups {
            group.expanded = expanded;
        }
    }

    pub fn workspaces_for_project_group(
        &self,
        key: &ProjectGroupKey,
        cx: &App,
    ) -> Option<Vec<Entity<Workspace>>> {
        let has_group = self.project_groups.iter().any(|group| group.key == *key)
            || self
                .retained_workspaces
                .iter()
                .any(|workspace| workspace.read(cx).project_group_key(cx) == *key);

        has_group.then(|| {
            self.retained_workspaces
                .iter()
                .filter(|workspace| workspace.read(cx).project_group_key(cx) == *key)
                .cloned()
                .collect()
        })
    }

    pub fn close_workspace(
        &mut self,
        workspace: &Entity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<bool>> {
        let group_key = workspace.read(cx).project_group_key(cx);
        let excluded_workspace = workspace.clone();

        self.remove(
            [workspace.clone()],
            move |this, window, cx| {
                if let Some(workspace) = this
                    .workspaces_for_project_group(&group_key, cx)
                    .unwrap_or_default()
                    .into_iter()
                    .find(|candidate| candidate != &excluded_workspace)
                {
                    return Task::ready(Ok(workspace));
                }

                let current_group_index = this
                    .project_groups
                    .iter()
                    .position(|group| group.key == group_key);

                if let Some(current_group_index) = current_group_index {
                    for distance in 1..this.project_groups.len() {
                        for neighboring_index in [
                            current_group_index.checked_add(distance),
                            current_group_index.checked_sub(distance),
                        ]
                        .into_iter()
                        .flatten()
                        {
                            let Some(neighboring_group) =
                                this.project_groups.get(neighboring_index)
                            else {
                                continue;
                            };

                            if let Some(workspace) = this
                                .last_active_workspace_for_group(&neighboring_group.key, cx)
                                .or_else(|| {
                                    this.workspaces_for_project_group(&neighboring_group.key, cx)
                                        .unwrap_or_default()
                                        .into_iter()
                                        .find(|candidate| candidate != &excluded_workspace)
                                })
                            {
                                return Task::ready(Ok(workspace));
                            }
                        }
                    }
                }

                let neighboring_group_key = current_group_index.and_then(|index| {
                    this.project_groups
                        .get(index + 1)
                        .or_else(|| {
                            index
                                .checked_sub(1)
                                .and_then(|previous| this.project_groups.get(previous))
                        })
                        .map(|group| group.key.clone())
                });

                if let Some(neighboring_group_key) = neighboring_group_key {
                    return this.find_or_create_local_workspace(
                        neighboring_group_key.path_list().clone(),
                        Some(neighboring_group_key),
                        std::slice::from_ref(&excluded_workspace),
                        None,
                        OpenMode::Activate,
                        window,
                        cx,
                    );
                }

                let app_state = this.workspace().read(cx).app_state().clone();
                let project = Project::local(
                    app_state.client.clone(),
                    app_state.node_runtime.clone(),
                    app_state.user_store.clone(),
                    app_state.languages.clone(),
                    app_state.fs.clone(),
                    None,
                    project::LocalProjectFlags::default(),
                    cx,
                );
                let new_workspace =
                    cx.new(|cx| Workspace::new(None, project, app_state, window, cx));
                Task::ready(Ok(new_workspace))
            },
            window,
            cx,
        )
    }

    pub fn remove_project_group(
        &mut self,
        group_key: &ProjectGroupKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<bool>> {
        let pos = self
            .project_groups
            .iter()
            .position(|group| group.key == *group_key);
        let workspaces = self
            .workspaces_for_project_group(group_key, cx)
            .unwrap_or_default();

        // Compute the neighbor while the group is still in the list.
        let neighbor_key = pos.and_then(|pos| {
            self.project_groups
                .get(pos + 1)
                .or_else(|| pos.checked_sub(1).and_then(|i| self.project_groups.get(i)))
                .map(|group| group.key.clone())
        });

        // Now remove the group.
        self.project_groups.retain(|group| group.key != *group_key);
        cx.emit(MultiWorkspaceEvent::ProjectGroupsChanged);

        let excluded_workspaces = workspaces.clone();
        self.remove(
            workspaces,
            move |this, window, cx| {
                if let Some(neighbor_key) = neighbor_key {
                    return this.find_or_create_local_workspace(
                        neighbor_key.path_list().clone(),
                        Some(neighbor_key.clone()),
                        &excluded_workspaces,
                        None,
                        OpenMode::Activate,
                        window,
                        cx,
                    );
                }

                // No other project groups remain — create an empty workspace.
                let app_state = this.workspace().read(cx).app_state().clone();
                let project = Project::local(
                    app_state.client.clone(),
                    app_state.node_runtime.clone(),
                    app_state.user_store.clone(),
                    app_state.languages.clone(),
                    app_state.fs.clone(),
                    None,
                    project::LocalProjectFlags::default(),
                    cx,
                );
                let new_workspace =
                    cx.new(|cx| Workspace::new(None, project, app_state, window, cx));
                Task::ready(Ok(new_workspace))
            },
            window,
            cx,
        )
    }

    /// Goes through sqlite: serialize -> close -> open new window
    /// This avoids issues with pending tasks having the wrong window
    pub fn open_project_group_in_new_window(
        &mut self,
        key: &ProjectGroupKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let paths: Vec<PathBuf> = key.path_list().ordered_paths().cloned().collect();
        if paths.is_empty() {
            return Task::ready(Ok(()));
        }

        let app_state = self.workspace().read(cx).app_state().clone();

        let workspaces: Vec<_> = self
            .workspaces_for_project_group(key, cx)
            .unwrap_or_default();
        let mut serialization_tasks = Vec::new();
        for workspace in &workspaces {
            serialization_tasks.push(workspace.update(cx, |workspace, inner_cx| {
                workspace.flush_serialization(window, inner_cx)
            }));
        }

        let remove_task = self.remove_project_group(key, window, cx);

        cx.spawn(async move |_this, cx| {
            futures::future::join_all(serialization_tasks).await;

            let removed = remove_task.await?;
            if !removed {
                return Ok(());
            }

            cx.update(|cx| {
                Workspace::new_local(paths, app_state, None, None, None, OpenMode::NewWindow, cx)
            })
            .await?;

            Ok(())
        })
    }

    /// Finds an existing workspace whose root paths and host exactly match.
    pub fn workspace_for_paths(
        &self,
        path_list: &PathList,
        host: Option<&RemoteConnectionOptions>,
        cx: &App,
    ) -> Option<Entity<Workspace>> {
        self.workspace_for_paths_excluding(path_list, host, &[], cx)
    }

    fn workspace_for_paths_excluding(
        &self,
        path_list: &PathList,
        host: Option<&RemoteConnectionOptions>,
        excluding: &[Entity<Workspace>],
        cx: &App,
    ) -> Option<Entity<Workspace>> {
        for workspace in self.workspaces() {
            if excluding.contains(workspace) {
                continue;
            }
            let root_paths = PathList::new(&workspace.read(cx).root_paths(cx));
            let key = workspace.read(cx).project_group_key(cx);
            let host_matches = key.host().as_ref() == host;
            let paths_match = root_paths == *path_list;
            if host_matches && paths_match {
                return Some(workspace.clone());
            }
        }

        None
    }

    /// Finds an existing workspace whose paths match, or creates a new one.
    ///
    /// For local projects (`host` is `None`), this delegates to
    /// [`Self::find_or_create_local_workspace`]. For remote projects, it
    /// tries an exact path match and, if no existing workspace is found,
    /// calls `connect_remote` to establish a connection and creates a new
    /// remote workspace.
    ///
    /// The `connect_remote` closure is responsible for any user-facing
    /// connection UI (e.g. password prompts). It receives the connection
    /// options and should return a [`Task`] that resolves to the
    /// [`RemoteClient`] session, or `None` if the connection was
    /// cancelled.
    pub fn find_or_create_workspace(
        &mut self,
        paths: PathList,
        host: Option<RemoteConnectionOptions>,
        provisional_project_group_key: Option<ProjectGroupKey>,
        connect_remote: impl FnOnce(
            RemoteConnectionOptions,
            &mut Window,
            &mut Context<Self>,
        ) -> Task<Result<Option<Entity<remote::RemoteClient>>>>
        + 'static,
        excluding: &[Entity<Workspace>],
        init: Option<Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + Send>>,
        open_mode: OpenMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Workspace>>> {
        self.find_or_create_workspace_with_source_workspace(
            paths,
            host,
            provisional_project_group_key,
            connect_remote,
            excluding,
            init,
            open_mode,
            None,
            window,
            cx,
        )
    }

    pub fn find_or_create_workspace_with_source_workspace(
        &mut self,
        paths: PathList,
        host: Option<RemoteConnectionOptions>,
        provisional_project_group_key: Option<ProjectGroupKey>,
        connect_remote: impl FnOnce(
            RemoteConnectionOptions,
            &mut Window,
            &mut Context<Self>,
        ) -> Task<Result<Option<Entity<remote::RemoteClient>>>>
        + 'static,
        excluding: &[Entity<Workspace>],
        init: Option<Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + Send>>,
        open_mode: OpenMode,
        source_workspace: Option<WeakEntity<Workspace>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Workspace>>> {
        if let Some(workspace) = self.workspace_for_paths(&paths, host.as_ref(), cx) {
            self.activate(workspace.clone(), source_workspace, window, cx);
            return Task::ready(Ok(workspace));
        }

        let Some(connection_options) = host else {
            return self.find_or_create_local_workspace_with_source_workspace(
                paths,
                provisional_project_group_key,
                excluding,
                init,
                open_mode,
                source_workspace,
                window,
                cx,
            );
        };

        let app_state = self.workspace().read(cx).app_state().clone();
        let window_handle = window.window_handle().downcast::<MultiWorkspace>();
        let connect_task = connect_remote(connection_options.clone(), window, cx);
        let paths_vec = paths.paths().to_vec();

        cx.spawn(async move |_this, cx| {
            let session = connect_task
                .await?
                .ok_or_else(|| anyhow::anyhow!("Remote connection was cancelled"))?;

            let new_project = cx.update(|cx| {
                Project::remote(
                    session,
                    app_state.client.clone(),
                    app_state.node_runtime.clone(),
                    app_state.user_store.clone(),
                    app_state.languages.clone(),
                    app_state.fs.clone(),
                    true,
                    cx,
                )
            });

            let effective_paths_vec =
                if let Some(project_group) = provisional_project_group_key.as_ref() {
                    let resolve_tasks = cx.update(|cx| {
                        let project = new_project.read(cx);
                        paths_vec
                            .iter()
                            .map(|path| project.resolve_abs_path(&path.to_string_lossy(), cx))
                            .collect::<Vec<_>>()
                    });
                    let resolved = futures::future::join_all(resolve_tasks).await;
                    // `resolve_abs_path` returns `None` for both "definitely
                    // absent" and transport errors (it swallows the error via
                    // `log_err`). This is a weaker guarantee than the local
                    // `Ok(None)` check, but it matches how the rest of the
                    // codebase consumes this API.
                    let all_paths_missing =
                        !paths_vec.is_empty() && resolved.iter().all(|resolved| resolved.is_none());

                    if all_paths_missing {
                        project_group.path_list().paths().to_vec()
                    } else {
                        paths_vec
                    }
                } else {
                    paths_vec
                };

            let window_handle =
                window_handle.ok_or_else(|| anyhow::anyhow!("Window is not a MultiWorkspace"))?;

            // `open_remote_project_with_existing_connection` already routes
            // through `activate()`/`activate_provisional_workspace()`
            // internally (see `open_remote_project_inner`), which correctly
            // apply `should_retain()`. Do NOT also call `add()` here — that
            // would unconditionally re-retain the workspace those calls just
            // correctly decided (possibly) not to retain, silently
            // overriding the policy for every remote/SSH project opened
            // this way.
            open_remote_project_with_existing_connection(
                connection_options,
                new_project,
                effective_paths_vec,
                app_state,
                window_handle,
                provisional_project_group_key,
                source_workspace,
                cx,
            )
            .await?;

            window_handle.update(cx, |multi_workspace, _window, _cx| {
                multi_workspace.workspace().clone()
            })
        })
    }

    /// Finds an existing workspace in this multi-workspace whose paths match,
    /// or creates a new one (deserializing its saved state from the database).
    /// Never searches other windows or matches workspaces with a superset of
    /// the requested paths.
    ///
    /// `excluding` lists workspaces that should be skipped during the search
    /// (e.g. workspaces that are about to be removed).
    pub fn find_or_create_local_workspace(
        &mut self,
        path_list: PathList,
        project_group: Option<ProjectGroupKey>,
        excluding: &[Entity<Workspace>],
        init: Option<Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + Send>>,
        open_mode: OpenMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Workspace>>> {
        self.find_or_create_local_workspace_with_source_workspace(
            path_list,
            project_group,
            excluding,
            init,
            open_mode,
            None,
            window,
            cx,
        )
    }

    pub fn find_or_create_local_workspace_with_source_workspace(
        &mut self,
        path_list: PathList,
        project_group: Option<ProjectGroupKey>,
        excluding: &[Entity<Workspace>],
        init: Option<Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + Send>>,
        open_mode: OpenMode,
        source_workspace: Option<WeakEntity<Workspace>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Workspace>>> {
        if let Some(workspace) = self.workspace_for_paths_excluding(&path_list, None, excluding, cx)
        {
            self.activate(workspace.clone(), source_workspace, window, cx);
            return Task::ready(Ok(workspace));
        }

        let paths = path_list.paths().to_vec();
        let app_state = self.workspace().read(cx).app_state().clone();
        let requesting_window = window.window_handle().downcast::<MultiWorkspace>();
        let fs = <dyn Fs>::global(cx);
        let excluding = excluding.to_vec();

        cx.spawn(async move |_this, cx| {
            let effective_path_list = if let Some(project_group) = project_group {
                let metadata_tasks: Vec<_> = paths
                    .iter()
                    .map(|path| fs.metadata(path.as_path()))
                    .collect();
                let metadata_results = futures::future::join_all(metadata_tasks).await;
                // Only fall back when every path is definitely absent; real
                // filesystem errors should not be treated as "missing".
                let all_paths_missing = !paths.is_empty()
                    && metadata_results
                        .into_iter()
                        // Ok(None) means the path is definitely absent
                        .all(|result| matches!(result, Ok(None)));

                if all_paths_missing {
                    project_group.path_list().clone()
                } else {
                    PathList::new(&paths)
                }
            } else {
                PathList::new(&paths)
            };

            if let Some(requesting_window) = requesting_window
                && let Some(workspace) = requesting_window
                    .update(cx, |multi_workspace, window, cx| {
                        multi_workspace
                            .workspace_for_paths_excluding(
                                &effective_path_list,
                                None,
                                &excluding,
                                cx,
                            )
                            .inspect(|workspace| {
                                multi_workspace.activate(
                                    workspace.clone(),
                                    source_workspace.clone(),
                                    window,
                                    cx,
                                );
                            })
                    })
                    .ok()
                    .flatten()
            {
                return Ok(workspace);
            }

            let result = cx
                .update(|cx| {
                    Workspace::new_local(
                        effective_path_list.paths().to_vec(),
                        app_state,
                        requesting_window,
                        None,
                        init,
                        open_mode,
                        cx,
                    )
                })
                .await?;
            Ok(result.workspace)
        })
    }

    pub fn workspace(&self) -> &Entity<Workspace> {
        &self.active_workspace
    }

    pub fn workspaces(&self) -> impl Iterator<Item = &Entity<Workspace>> {
        let active_is_retained = self.is_workspace_retained(&self.active_workspace);
        self.retained_workspaces
            .iter()
            .chain(std::iter::once(&self.active_workspace).filter(move |_| !active_is_retained))
    }

    /// Adds a workspace to this window as persistent without changing which
    /// workspace is active, unconditionally — regardless of the
    /// `retain_background_projects` setting. Only call this directly from
    /// **session-restore/deserialization** call sites (`persistence.rs`'s
    /// restore path, `open_workspace_by_id`), which must faithfully
    /// reconstruct a previously-saved session rather than lose project
    /// groups because the user's *current* live setting says not to retain
    /// new ones going forward (see NFR2 in the multi-project-window-switching
    /// plan — an old session must deserialize without losing project
    /// groups).
    ///
    /// Live, user-driven flows that pass `OpenMode::Add` (e.g. creating or
    /// switching a linked git worktree, or `find_or_create_workspace`'s
    /// remote/SSH path) must go through `add_or_activate()` instead, so the
    /// policy is actually enforced — calling `add()` directly from a live
    /// flow silently overrides whatever `should_retain()` decided.
    ///
    /// Unlike retention, the `ProjectActivity` governor is not gated by any
    /// setting: a workspace added here that isn't the active one is routed
    /// through `schedule_hibernate()`, exactly like the outgoing workspace
    /// in `activate()`. Without this, a workspace added in the background
    /// would sit at `Active` forever and never become a hibernation
    /// candidate.
    pub fn add(&mut self, workspace: Entity<Workspace>, window: &Window, cx: &mut Context<Self>) {
        if self.is_workspace_retained(&workspace) {
            return;
        }

        if workspace != self.active_workspace {
            self.register_workspace(&workspace, window, cx);
            self.schedule_hibernate(&workspace, cx);
        }

        let key = workspace.read(cx).project_group_key(cx);
        self.retain_workspace(workspace, key, cx);
        telemetry::event!(
            "Workspace Added",
            workspace_count = self.retained_workspaces.len()
        );
        cx.notify();
    }

    /// Adds `workspace` to this window, or activates it, depending on
    /// `should_retain()`. This is the entry point live, user-driven flows
    /// should use for a workspace they don't necessarily want to force into
    /// the foreground (mirroring `OpenMode::Add`'s original intent) — when
    /// retention is on, it's added in the background exactly like `add()`;
    /// when retention is off, adding it in the background would leave it
    /// unreachable once the caller's local scope ends (nothing retains it,
    /// nothing keeps it active), so it's activated instead, matching
    /// `activate()`'s own "at most one live project" policy.
    pub fn add_or_activate(
        &mut self,
        workspace: Entity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.should_retain(cx) {
            self.add(workspace, &*window, cx);
        } else {
            self.activate(workspace, None, window, cx);
        }
    }

    /// Whether a workspace that loses focus should stay retained in the
    /// background, per the `workspace.multi_project.retain_background_projects`
    /// setting. This governs the live `activate()` retain/detach policy,
    /// `apply_open_sidebar()`'s live (non-restore) path, and — via
    /// `add_or_activate()` — live callers of `OpenMode::Add`.
    ///
    /// It intentionally does NOT gate `add()` directly (used for
    /// deserialization and other system-initiated insertions, which must
    /// always retain to faithfully restore a previously-saved session
    /// regardless of the user's current live setting).
    fn should_retain(&self, cx: &App) -> bool {
        WorkspaceSettings::get_global(cx)
            .multi_project
            .retain_background_projects
    }

    /// Clears any pending hibernate timer for `workspace` and marks its
    /// project `Active` immediately (FR2 — synchronous, no task involved).
    /// Safe to call on a workspace whose project is already `Active`:
    /// `Project::set_activity` only emits/notifies when the value actually
    /// changes.
    ///
    /// This is the multi-workspace's *only* manual-wake path (the reverse
    /// of `schedule_hibernate`, called only from `activate()`), so it also
    /// records the memory-pressure fuse's FR4b guarantees: `warm_since` no
    /// longer applies (the project isn't `Warm` anymore), and
    /// `manually_woken_at` is stamped so the fuse gives this project at
    /// least one full poll cycle of immunity — "a manual wake always beats
    /// the fuse."
    fn wake_project(&mut self, workspace: &Entity<Workspace>, cx: &mut Context<Self>) {
        let entity_id = workspace.entity_id();
        self.hibernate_timers.remove(&entity_id);
        self.warm_since.remove(&entity_id);
        self.manually_woken_at
            .insert(entity_id, cx.background_executor().now());
        let project = workspace.read(cx).project().clone();
        project.update(cx, |project, cx| {
            project.set_activity(ProjectActivity::Active, cx);
        });
    }

    /// Marks `workspace`'s project `Warm` and, unless hibernation is
    /// disabled (`multi_project.hibernate_after` setting resolves to
    /// `None`, i.e. `hibernate_after_ms: 0`), schedules a timer that moves
    /// it to `Hibernated` once idle for that long. Replaces any previously
    /// pending timer for this workspace.
    ///
    /// Also stamps `warm_since` (FR4b): the memory-pressure fuse requires a
    /// project to have sat `Warm` for at least
    /// `MEMORY_FUSE_MIN_WARM_DURATION` before it's eligible as a victim, and
    /// this is the only place a project's activity transitions *into*
    /// `Warm`.
    fn schedule_hibernate(&mut self, workspace: &Entity<Workspace>, cx: &mut Context<Self>) {
        let project = workspace.read(cx).project().clone();
        project.update(cx, |project, cx| {
            project.set_activity(ProjectActivity::Warm, cx);
        });

        let entity_id = workspace.entity_id();
        self.warm_since.insert(
            entity_id,
            (cx.background_executor().now(), project.downgrade()),
        );
        let Some(hibernate_after) = WorkspaceSettings::get_global(cx)
            .multi_project
            .hibernate_after
        else {
            // Dropping the previous entry (if any) cancels its timer.
            self.hibernate_timers.remove(&entity_id);
            return;
        };

        let weak_project = project.downgrade();
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(hibernate_after).await;
            // The workspace (and its project) may have been closed while
            // this timer was pending — expected (see
            // `detach_workspace`/`close_workspace`/`remove`) — but a failure
            // still gets logged rather than silently discarded, per repo
            // convention. Mirrors `DelayedDebouncedEditAction::fire_new`'s
            // handling of the same "entity may be gone by the time the
            // timer fires" race in this same crate: it uses `.log_err()`,
            // not a bare `.ok()`/`.is_ok()`.
            if weak_project
                .update(cx, |project, cx| {
                    project.set_activity(ProjectActivity::Hibernated, cx);
                })
                .log_err()
                .is_some()
            {
                this.update(cx, |this, _cx| {
                    this.hibernate_timers.remove(&entity_id);
                })
                .log_err();
            }
        });
        self.hibernate_timers.insert(entity_id, task);
    }

    /// Evicts stale entries from `warm_since`/`manually_woken_at`, run at
    /// the top of every tick regardless of whether the fuse itself is
    /// enabled (these maps are populated by ordinary Warm/Active
    /// transitions, independent of the fuse setting):
    ///
    /// - `warm_since`: dropped once its `WeakEntity<Project>` fails to
    ///   upgrade. `detach_workspace` already removes an entry on an
    ///   ordinary close, but it can't see the one case that bypasses it —
    ///   a workspace that goes `Warm` via `schedule_hibernate` without
    ///   ever having been independently retained (e.g. a window's very
    ///   first workspace, the first time it loses focus; see
    ///   `warm_since`'s own doc comment) has no strong reference left
    ///   anywhere once `activate()` reassigns `self.active_workspace`
    ///   away from it, and its `Entity<Workspace>` — though not its
    ///   `Project` — can vanish without `detach_workspace` ever running.
    ///   Left unpruned, that would be exactly one permanently-dead entry
    ///   per such workspace: bounded, not unbounded, but still a cache
    ///   with no eviction policy, which this closes.
    /// - `manually_woken_at`: evicted once older than
    ///   `MEMORY_FUSE_POLL_INTERVAL` outright, alive or not — past that
    ///   age an entry can never again satisfy the immunity check
    ///   (`select_memory_fuse_victim`), so there is no reason to wait for
    ///   `detach_workspace` to clear it.
    fn prune_dead_warm_entries(&mut self, now: Instant) {
        self.warm_since
            .retain(|_, (_, weak_project)| weak_project.upgrade().is_some());
        self.manually_woken_at
            .retain(|_, woken_at| now.duration_since(*woken_at) < MEMORY_FUSE_POLL_INTERVAL);
    }

    /// FR3/FR4/FR4b: one poll cycle of the memory-pressure fuse. Reads
    /// system memory pressure through the injected `memory_pressure_reader`
    /// (never `sysinfo` directly — see `MemoryPressureReader`'s own doc
    /// comment) and, if it's under the configured threshold, hibernates at
    /// most one eligible victim.
    ///
    /// Deliberately at most one victim per tick rather than looping inside
    /// a single tick until pressure eases: `Project::set_activity` kicks
    /// off `LspStore::hibernate`'s shutdown as a *detached* async task
    /// (see its own doc comment — the LSP protocol shutdown takes real
    /// time), so a same-tick re-measurement would not yet reflect memory
    /// the just-hibernated victim is in the process of freeing. Spacing
    /// victims one per `MEMORY_FUSE_POLL_INTERVAL` gives that shutdown
    /// time to actually land before the next reading is trusted — the
    /// alternative (loop within one tick) would, given that timing gap,
    /// degenerate into hibernating every eligible project in one shot
    /// rather than the measured, one-at-a-time response FR4b calls for.
    fn memory_governor_tick(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        self.prune_dead_warm_entries(now);

        let Some(threshold_percent) = WorkspaceSettings::get_global(cx)
            .multi_project
            .memory_pressure_threshold_percent
        else {
            return; // Fuse disabled (`memory_pressure_threshold_percent: 0`).
        };

        if let Some(last_triggered) = self.fuse_last_triggered_at
            && now.duration_since(last_triggered)
                < MEMORY_FUSE_POLL_INTERVAL * MEMORY_FUSE_HYSTERESIS_CYCLES
        {
            return; // FR4b hysteresis: still cooling down from the last trigger.
        }

        let Some(available_percent) = self.memory_pressure_reader.available_memory_percent() else {
            return; // Could not read memory this cycle; try again next poll.
        };
        if available_percent >= threshold_percent {
            return; // Pressure is within bounds.
        }

        let Some(victim) = self.select_memory_fuse_victim(now, cx) else {
            return; // No eligible `Warm` victim this cycle; try again next poll.
        };
        victim.update(cx, |project, cx| {
            project.set_activity(ProjectActivity::Hibernated, cx);
        });
        self.fuse_last_triggered_at = Some(now);
        self.notify_memory_fuse_triggered(cx);
    }

    /// FR4/FR4b: the best memory-fuse victim among this window's tracked
    /// workspaces, or `None` if none are eligible right now. Eligible
    /// means: currently `Warm` (never `Active` — FR4 — and `Hibernated`
    /// is already done, not a candidate); has sat `Warm` for at least
    /// `MEMORY_FUSE_MIN_WARM_DURATION` (FR4b); not within one poll cycle
    /// of a manual wake (FR4b — "a manual wake always beats the fuse");
    /// and not blocked by the exact same barriers
    /// `Project::try_hibernate_resources` itself checks (FR4 — "no
    /// shortcut through the same barriers": picking a debugging project,
    /// or one with autosave racing a dirty buffer, would flip its
    /// `activity()` label to `Hibernated` without its resources actually
    /// stopping). Among eligible candidates, the one that has sat `Warm`
    /// the longest is picked, matching phase-06's "least recently active"
    /// intent.
    fn select_memory_fuse_victim(&self, now: Instant, cx: &App) -> Option<Entity<Project>> {
        self.warm_since
            .iter()
            .filter_map(|(entity_id, (warm_since, weak_project))| {
                let warm_since = *warm_since;
                if now.duration_since(warm_since) < MEMORY_FUSE_MIN_WARM_DURATION {
                    return None;
                }
                if let Some(woken_at) = self.manually_woken_at.get(entity_id)
                    && now.duration_since(*woken_at) < MEMORY_FUSE_POLL_INTERVAL
                {
                    return None;
                }
                // A dead weak handle means the project closed without
                // `detach_workspace` cleaning up this entry yet (or ever,
                // in some future refactor) — treat it the same as any
                // other "not a candidate", rather than panicking or
                // asserting an invariant this map doesn't itself enforce.
                let project = weak_project.upgrade()?;
                if project.read(cx).activity() != ProjectActivity::Warm {
                    return None;
                }
                if project.read(cx).would_defer_hibernation(cx) {
                    return None;
                }
                Some((warm_since, project))
            })
            .min_by_key(|(warm_since, _)| *warm_since)
            .map(|(_, project)| project)
    }

    /// FR6: tells the user the fuse just acted, rather than silently
    /// hibernating a project and leaving them to wonder why it slowed
    /// down. Shown on the active workspace's window since the victim
    /// itself (by construction, not `Active`) may not have any window
    /// surface currently focused to show it on.
    fn notify_memory_fuse_triggered(&self, cx: &mut Context<Self>) {
        self.active_workspace.update(cx, |workspace, cx| {
            workspace.show_toast(
                Toast::new(
                    NotificationId::unique::<MemoryPressureFuseToast>(),
                    "Hibernated 1 project to free up memory",
                ),
                cx,
            );
        });
    }

    /// Ensures the workspace is in the multiworkspace and makes it the active one.
    pub fn activate(
        &mut self,
        workspace: Entity<Workspace>,
        source_workspace: Option<WeakEntity<Workspace>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace() == &workspace {
            self.focus_active_workspace(window, cx);
            return;
        }

        let old_active_workspace = self.active_workspace.clone();
        let old_active_was_retained = self.active_workspace_is_retained();
        let workspace_was_retained = self.is_workspace_retained(&workspace);
        let should_retain = self.should_retain(cx);

        if !workspace_was_retained {
            self.register_workspace(&workspace, window, cx);

            if should_retain {
                let key = workspace.read(cx).project_group_key(cx);
                self.retain_workspace(workspace.clone(), key, cx);
            }
        }

        // FR2/FR3: the incoming workspace's project goes `Active`
        // synchronously; the outgoing one goes `Warm` and gets an idle
        // timer. If `old_active_workspace` is about to be detached below
        // (retention off and it wasn't already retained), that detach's own
        // cleanup drops the timer `Task` we just scheduled, cancelling it —
        // ordering this before the detach check is what makes that cleanup
        // actually cancel it instead of racing it.
        self.wake_project(&workspace, cx);
        self.schedule_hibernate(&old_active_workspace, cx);

        self.active_workspace = workspace;

        let active_key = self.active_workspace.read(cx).project_group_key(cx);
        if let Some(group) = self.project_groups.iter_mut().find(|g| g.key == active_key) {
            group.last_active_workspace = Some(self.active_workspace.downgrade());
        }

        if !should_retain && !old_active_was_retained {
            self.detach_workspace(&old_active_workspace, cx);
        }

        cx.emit(MultiWorkspaceEvent::ActiveWorkspaceChanged { source_workspace });
        self.serialize(cx);
        self.focus_active_workspace(window, cx);
        cx.notify();
    }

    /// Activates the next (`forward = true`) or previous workspace in
    /// `workspaces()` order, wrapping around at either end. A no-op when
    /// fewer than two workspaces are open.
    ///
    /// This is what makes every retained workspace reachable regardless of
    /// how it entered `retained_workspaces` (`add()`, `activate()`, or
    /// `activate_provisional_workspace()`) — previously only a `Sidebar`
    /// implementation could drive project switching, so a workspace added
    /// while no sidebar existed (e.g. `OpenMode::Add` from a second CLI
    /// invocation) was retained but unreachable.
    pub fn cycle_project(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let workspaces: Vec<Entity<Workspace>> = self.workspaces().cloned().collect();
        if workspaces.len() < 2 {
            return;
        }

        let Some(current_index) = workspaces
            .iter()
            .position(|workspace| workspace == &self.active_workspace)
        else {
            return;
        };

        let next_index = if forward {
            (current_index + 1) % workspaces.len()
        } else {
            (current_index + workspaces.len() - 1) % workspaces.len()
        };

        self.activate(workspaces[next_index].clone(), None, window, cx);
    }

    /// FR2 (Phase 6 of multi-project-window-switching): logs
    /// `Project::resource_stats` for every project this window tracks.
    /// Identifies each row by index only, deliberately never by project
    /// path or file name — see phase-06's Security Considerations, this
    /// is an `info`-level log and paths/file names don't belong there.
    fn dump_project_resource_stats(&self, cx: &App) {
        let workspaces: Vec<Entity<Workspace>> = self.workspaces().cloned().collect();
        log::info!("project resource stats ({} tracked):", workspaces.len());
        for (index, workspace) in workspaces.iter().enumerate() {
            let stats = workspace.read(cx).project().read(cx).resource_stats(cx);
            log::info!(
                "  [{index}] activity={:?} language_servers={} buffers={} \
                 worktree_entries={} terminal_scrollback_lines={} \
                 language_server_rss_bytes={:?}",
                stats.activity,
                stats.running_language_servers,
                stats.open_buffers,
                stats.worktree_entries,
                stats.terminal_scrollback_lines,
                stats.language_server_rss_bytes,
            );
        }
    }

    /// Promotes the currently active workspace to persistent if it is
    /// transient, so it is retained across workspace switches even when
    /// the sidebar is closed. No-op if the workspace is already persistent.
    pub fn retain_active_workspace(&mut self, cx: &mut Context<Self>) {
        let workspace = self.active_workspace.clone();
        if self.is_workspace_retained(&workspace) {
            return;
        }

        let key = workspace.read(cx).project_group_key(cx);
        self.retain_workspace(workspace, key, cx);
        self.serialize(cx);
        cx.notify();
    }

    /// Collapses to a single workspace, discarding all groups.
    /// Used when multi-workspace is disabled (e.g. disable_ai).
    fn collapse_to_single_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_open {
            self.close_sidebar(window, cx);
        }

        let active_workspace = self.active_workspace.clone();
        for workspace in self.retained_workspaces.clone() {
            if workspace != active_workspace {
                self.detach_workspace(&workspace, cx);
            }
        }

        self.retained_workspaces.clear();
        self.project_groups.clear();
        cx.notify();
    }

    /// Detaches a workspace: clears session state, DB binding, cached
    /// group key, and emits `WorkspaceRemoved`. The DB row is preserved
    /// so the workspace still appears in the recent-projects list.
    ///
    /// Also drops any pending hibernate timer for `workspace`, which
    /// cancels it (GPUI `Task` semantics) — the single place this is
    /// needed, since `close_workspace` and `remove` both call this for
    /// every workspace they remove that was actually retained (the only
    /// state a pending timer can exist in outside `activate()`'s own
    /// synchronous body).
    fn detach_workspace(&mut self, workspace: &Entity<Workspace>, cx: &mut Context<Self>) {
        self.hibernate_timers.remove(&workspace.entity_id());
        self.warm_since.remove(&workspace.entity_id());
        self.manually_woken_at.remove(&workspace.entity_id());
        self.retained_workspaces
            .retain(|retained| retained != workspace);
        for group in &mut self.project_groups {
            if group
                .last_active_workspace
                .as_ref()
                .and_then(WeakEntity::upgrade)
                .as_ref()
                == Some(workspace)
            {
                group.last_active_workspace = None;
            }
        }
        cx.emit(MultiWorkspaceEvent::WorkspaceRemoved(workspace.entity_id()));
        workspace.update(cx, |workspace, _cx| {
            workspace.session_id.take();
            workspace._schedule_serialize_workspace.take();
            workspace._serialize_workspace_task.take();
        });

        if let Some(workspace_id) = workspace.read(cx).database_id() {
            let db = crate::persistence::WorkspaceDb::global(cx);
            self.pending_removal_tasks.retain(|task| !task.is_ready());
            self.pending_removal_tasks
                .push(cx.background_spawn(async move {
                    db.set_session_binding(workspace_id, None, None)
                        .await
                        .log_err();
                }));
        }
    }

    fn sync_sidebar_to_workspace(&self, workspace: &Entity<Workspace>, cx: &mut Context<Self>) {
        if self.sidebar_open() {
            let sidebar_focus_handle = self.sidebar.as_ref().map(|s| s.focus_handle(cx));
            workspace.update(cx, |workspace, _| {
                workspace.set_sidebar_focus_handle(sidebar_focus_handle);
            });
        }
    }

    pub fn serialize(&mut self, cx: &mut Context<Self>) {
        self._serialize_task = Some(cx.spawn(async move |this, cx| {
            let Some((window_id, state)) = this
                .read_with(cx, |this, cx| {
                    let state = MultiWorkspaceState {
                        active_workspace_id: this.workspace().read(cx).database_id(),
                        project_groups: this
                            .project_groups
                            .iter()
                            .map(|group| {
                                crate::persistence::model::SerializedProjectGroup::from_group(
                                    &group.key,
                                    group.expanded,
                                )
                            })
                            .collect::<Vec<_>>(),
                        sidebar_open: this.sidebar_open,
                        sidebar_state: this.sidebar.as_ref().and_then(|s| s.serialized_state(cx)),
                    };
                    (this.window_id, state)
                })
                .ok()
            else {
                return;
            };
            let kvp = cx.update(|cx| db::kvp::KeyValueStore::global(cx));
            crate::persistence::write_multi_workspace_state(&kvp, window_id, state).await;
        }));
    }

    /// Returns the in-flight serialization task (if any) so the caller can
    /// await it. Used by the quit handler to ensure pending DB writes
    /// complete before the process exits.
    pub fn flush_serialization(&mut self) -> Task<()> {
        self._serialize_task.take().unwrap_or(Task::ready(()))
    }

    fn app_will_quit(&mut self, _cx: &mut Context<Self>) -> impl Future<Output = ()> + use<> {
        let mut tasks: Vec<Task<()>> = Vec::new();
        if let Some(task) = self._serialize_task.take() {
            tasks.push(task);
        }
        tasks.extend(std::mem::take(&mut self.pending_removal_tasks));

        async move {
            futures::future::join_all(tasks).await;
        }
    }

    pub fn focus_active_workspace(&self, window: &mut Window, cx: &mut App) {
        // If a dock panel is zoomed, focus it instead of the center pane.
        // Otherwise, focusing the center pane triggers dismiss_zoomed_items_to_reveal
        // which closes the zoomed dock.
        let focus_handle = {
            let workspace = self.workspace().read(cx);
            let mut target = None;
            for dock in workspace.all_docks() {
                let dock = dock.read(cx);
                if dock.is_open() {
                    if let Some(panel) = dock.active_panel() {
                        if panel.is_zoomed(window, cx) {
                            target = Some(panel.panel_focus_handle(cx));
                            break;
                        }
                    }
                }
            }
            target.unwrap_or_else(|| {
                let pane = workspace.active_pane().clone();
                pane.read(cx).focus_handle(cx)
            })
        };
        window.focus(&focus_handle, cx);
    }

    pub fn panel<T: Panel>(&self, cx: &App) -> Option<Entity<T>> {
        self.workspace().read(cx).panel::<T>(cx)
    }

    pub fn active_modal<V: ManagedView + 'static>(&self, cx: &App) -> Option<Entity<V>> {
        self.workspace().read(cx).active_modal::<V>(cx)
    }

    pub fn add_panel<T: Panel>(
        &mut self,
        panel: Entity<T>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace().update(cx, |workspace, cx| {
            workspace.add_panel(panel, window, cx);
        });
    }

    pub fn focus_panel<T: Panel>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<T>> {
        self.workspace()
            .update(cx, |workspace, cx| workspace.focus_panel::<T>(window, cx))
    }

    // used in a test
    pub fn toggle_modal<V: ModalView, B>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        build: B,
    ) where
        B: FnOnce(&mut Window, &mut gpui::Context<V>) -> V,
    {
        self.workspace().update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, build);
        });
    }

    pub fn toggle_dock(
        &mut self,
        dock_side: DockPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace().update(cx, |workspace, cx| {
            workspace.toggle_dock(dock_side, window, cx);
        });
    }

    pub fn active_item_as<I: 'static>(&self, cx: &App) -> Option<Entity<I>> {
        self.workspace().read(cx).active_item_as::<I>(cx)
    }

    pub fn items_of_type<'a, T: Item>(
        &'a self,
        cx: &'a App,
    ) -> impl 'a + Iterator<Item = Entity<T>> {
        self.workspace().read(cx).items_of_type::<T>(cx)
    }

    pub fn database_id(&self, cx: &App) -> Option<WorkspaceId> {
        self.workspace().read(cx).database_id()
    }

    pub fn take_pending_removal_tasks(&mut self) -> Vec<Task<()>> {
        let tasks: Vec<Task<()>> = std::mem::take(&mut self.pending_removal_tasks)
            .into_iter()
            .filter(|task| !task.is_ready())
            .collect();
        tasks
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn test_expand_all_groups(&mut self) {
        self.set_all_groups_expanded(true);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn assert_project_group_key_integrity(&self, cx: &App) -> anyhow::Result<()> {
        let mut retained_ids: collections::HashSet<EntityId> = Default::default();
        for workspace in &self.retained_workspaces {
            anyhow::ensure!(
                retained_ids.insert(workspace.entity_id()),
                "workspace {:?} is retained more than once",
                workspace.entity_id(),
            );

            let live_key = workspace.read(cx).project_group_key(cx);
            anyhow::ensure!(
                self.project_groups
                    .iter()
                    .any(|group| group.key == live_key),
                "workspace {:?} has live key {:?} but no project-group metadata",
                workspace.entity_id(),
                live_key,
            );
        }
        Ok(())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_random_database_id(&mut self, cx: &mut Context<Self>) {
        self.workspace().update(cx, |workspace, _cx| {
            workspace.set_random_database_id();
        });
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn test_new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let workspace = cx.new(|cx| Workspace::test_new(project, window, cx));
        Self::new(workspace, window, cx)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn test_add_workspace(
        &mut self,
        project: Entity<Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Workspace> {
        let workspace = cx.new(|cx| Workspace::test_new(project, window, cx));
        self.activate(workspace.clone(), None, window, cx);
        workspace
    }

    /// Sets `workspace.multi_project.retain_background_projects` and
    /// immediately retains the currently active workspace, reproducing the
    /// retention side effect that `open_sidebar()` used to provide
    /// unconditionally before retention was decoupled from the sidebar UI
    /// (phase 1 of multi-project-window-switching). `activate()` never
    /// implicitly retains the *outgoing* workspace on its own — only an
    /// explicit `retain_workspace()`/`add()`/`retain_active_workspace()`
    /// call does — so tests across crates that need an earlier workspace to
    /// survive once a second one activates in the same window should call
    /// this instead of relying on `open_sidebar()`'s old side effect.
    #[cfg(any(test, feature = "test-support"))]
    pub fn test_enable_background_retention(&mut self, cx: &mut Context<Self>) {
        settings::SettingsStore::update_global(cx, |settings, cx| {
            settings.update_user_settings(cx, |settings| {
                settings.workspace.multi_project = Some(settings::MultiProjectContent {
                    retain_background_projects: Some(true),
                    // `None` here is a no-op merge (see `MultiProjectContent`'s
                    // own doc comment) — it leaves `hibernate_after_ms`,
                    // `memory_pressure_threshold_percent`, and
                    // `background_scroll_history_lines` whatever the
                    // default/global layers already resolved them to,
                    // since this helper exists to toggle retention, not
                    // hibernation, the memory fuse, or terminal scroll
                    // history.
                    hibernate_after_ms: None,
                    memory_pressure_threshold_percent: None,
                    background_scroll_history_lines: None,
                    sidebar_side: None,
                });
            });
        });
        self.retain_active_workspace(cx);
    }

    /// Whether `workspace` currently has a pending hibernate timer, i.e.
    /// its project is `Warm` and will hibernate on its own unless woken or
    /// re-hibernated first. Exposed only so tests can assert that a timer
    /// was actually cancelled (e.g. on close) rather than merely inferring
    /// it from the absence of a later, hard-to-time-deterministically
    /// transition.
    #[cfg(any(test, feature = "test-support"))]
    pub fn has_pending_hibernate_timer(&self, workspace: &Entity<Workspace>) -> bool {
        self.hibernate_timers.contains_key(&workspace.entity_id())
    }

    /// Swaps in a synthetic memory-pressure reader so tests can drive the
    /// fuse (`memory_governor_tick`) without touching the real OS. See
    /// `MemoryPressureReader`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_memory_pressure_reader_for_test(&mut self, reader: Box<dyn MemoryPressureReader>) {
        self.memory_pressure_reader = reader;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn test_add_project_group(&mut self, group: ProjectGroup) {
        self.project_groups.push(ProjectGroupState {
            key: group.key,
            expanded: group.expanded,
            last_active_workspace: None,
        });
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn create_test_workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let app_state = self.workspace().read(cx).app_state().clone();
        let project = Project::local(
            app_state.client.clone(),
            app_state.node_runtime.clone(),
            app_state.user_store.clone(),
            app_state.languages.clone(),
            app_state.fs.clone(),
            None,
            project::LocalProjectFlags::default(),
            cx,
        );
        let new_workspace = cx.new(|cx| Workspace::new(None, project, app_state, window, cx));
        self.activate(new_workspace.clone(), None, window, cx);

        let weak_workspace = new_workspace.downgrade();
        let db = crate::persistence::WorkspaceDb::global(cx);
        cx.spawn_in(window, async move |this, cx| {
            let workspace_id = db.next_id().await.unwrap();
            let workspace = weak_workspace.upgrade().unwrap();
            let task: Task<()> = this
                .update_in(cx, |this, window, cx| {
                    let session_id = workspace.read(cx).session_id();
                    let window_id = window.window_handle().window_id().as_u64();
                    workspace.update(cx, |workspace, _cx| {
                        workspace.set_database_id(workspace_id);
                    });
                    this.serialize(cx);
                    let db = db.clone();
                    cx.background_spawn(async move {
                        db.set_session_binding(workspace_id, session_id, Some(window_id))
                            .await
                            .log_err();
                    })
                })
                .unwrap();
            task.await
        })
    }

    /// Assigns random database IDs to all retained workspaces, flushes
    /// workspace serialization (SQLite) and multi-workspace state (KVP),
    /// and writes session bindings so the serialized data can be read
    /// back by `last_session_workspace_locations` +
    /// `read_serialized_multi_workspaces`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn flush_all_serialization(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Task<()>> {
        for workspace in self.workspaces() {
            workspace.update(cx, |ws, _cx| {
                if ws.database_id().is_none() {
                    ws.set_random_database_id();
                }
            });
        }

        let session_id = self.workspace().read(cx).session_id();
        let window_id_u64 = window.window_handle().window_id().as_u64();

        let mut tasks: Vec<Task<()>> = Vec::new();
        for workspace in self.workspaces() {
            tasks.push(workspace.update(cx, |ws, cx| ws.flush_serialization(window, cx)));
            if let Some(db_id) = workspace.read(cx).database_id() {
                let db = crate::persistence::WorkspaceDb::global(cx);
                let session_id = session_id.clone();
                tasks.push(cx.background_spawn(async move {
                    db.set_session_binding(db_id, session_id, Some(window_id_u64))
                        .await
                        .log_err();
                }));
            }
        }
        self.serialize(cx);
        tasks
    }

    /// Removes one or more workspaces from this multi-workspace.
    ///
    /// If the active workspace is among those being removed,
    /// `fallback_workspace` is called **synchronously before the removal
    /// begins** to produce a `Task` that resolves to the workspace that
    /// should become active. The fallback must not be one of the
    /// workspaces being removed.
    ///
    /// Returns `true` if any workspaces were actually removed.
    pub fn remove(
        &mut self,
        workspaces: impl IntoIterator<Item = Entity<Workspace>>,
        fallback_workspace: impl FnOnce(
            &mut Self,
            &mut Window,
            &mut Context<Self>,
        ) -> Task<Result<Entity<Workspace>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<bool>> {
        let workspaces: Vec<_> = workspaces.into_iter().collect();

        if workspaces.is_empty() {
            return Task::ready(Ok(false));
        }

        let removing_active = workspaces.iter().any(|ws| ws == self.workspace());
        let original_active = self.workspace().clone();

        let fallback_task = removing_active.then(|| fallback_workspace(self, window, cx));

        cx.spawn_in(window, async move |this, cx| {
            // Run the standard workspace close lifecycle for every workspace
            // being removed from this window. This handles save prompting and
            // session cleanup consistently with other replace-in-window flows.
            for workspace in &workspaces {
                let should_continue = workspace
                    .update_in(cx, |workspace, window, cx| {
                        workspace.prepare_to_close(CloseIntent::ReplaceWindow, window, cx)
                    })?
                    .await?;

                if !should_continue {
                    return Ok(false);
                }
            }

            // If we're removing the active workspace, await the
            // fallback and switch to it before tearing anything down.
            // Otherwise restore the original active workspace in case
            // prompting switched away from it.
            if let Some(fallback_task) = fallback_task {
                let new_active = fallback_task.await?;

                this.update_in(cx, |this, window, cx| {
                    assert!(
                        !workspaces.contains(&new_active),
                        "fallback workspace must not be one of the workspaces being removed"
                    );
                    this.activate(new_active, None, window, cx);
                })?;
            } else {
                this.update_in(cx, |this, window, cx| {
                    if *this.workspace() != original_active {
                        this.activate(original_active, None, window, cx);
                    }
                })?;
            }

            // Actually remove the workspaces.
            this.update_in(cx, |this, _, cx| {
                let mut removed_any = false;

                for workspace in &workspaces {
                    let was_retained = this.is_workspace_retained(workspace);
                    if was_retained {
                        this.detach_workspace(workspace, cx);
                        removed_any = true;
                    }
                }

                if removed_any {
                    this.serialize(cx);
                    cx.notify();
                }

                Ok(removed_any)
            })?
        })
    }

    pub fn open_project(
        &mut self,
        paths: Vec<PathBuf>,
        open_mode: OpenMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Workspace>>> {
        if self.multi_workspace_enabled(cx) {
            let empty_workspace = if self
                .active_workspace
                .read(cx)
                .project()
                .read(cx)
                .visible_worktrees(cx)
                .next()
                .is_none()
            {
                Some(self.active_workspace.clone())
            } else {
                None
            };

            cx.spawn_in(window, async move |this, cx| {
                if let Some(empty_workspace) = empty_workspace.as_ref() {
                    let should_continue = empty_workspace
                        .update_in(cx, |workspace, window, cx| {
                            workspace.prepare_to_close(CloseIntent::ReplaceWindow, window, cx)
                        })?
                        .await?;
                    if !should_continue {
                        return Ok(empty_workspace.clone());
                    }
                }

                let create_task = this.update_in(cx, |this, window, cx| {
                    this.find_or_create_local_workspace(
                        PathList::new(&paths),
                        None,
                        empty_workspace.as_slice(),
                        None,
                        OpenMode::Activate,
                        window,
                        cx,
                    )
                })?;
                let new_workspace = create_task.await?;

                if let Some(empty_workspace) = empty_workspace {
                    this.update(cx, |this, cx| {
                        if this.is_workspace_retained(&empty_workspace) {
                            this.detach_workspace(&empty_workspace, cx);
                        }
                    })?;
                }

                Ok(new_workspace)
            })
        } else {
            let workspace = self.workspace().clone();
            cx.spawn_in(window, async move |_this, cx| {
                let should_continue = workspace
                    .update_in(cx, |workspace, window, cx| {
                        workspace.prepare_to_close(crate::CloseIntent::ReplaceWindow, window, cx)
                    })?
                    .await?;
                if should_continue {
                    workspace
                        .update_in(cx, |workspace, window, cx| {
                            workspace.open_workspace_for_paths(open_mode, paths, window, cx)
                        })?
                        .await
                } else {
                    Ok(workspace)
                }
            })
        }
    }
}

impl Render for MultiWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let multi_workspace_enabled = self.multi_workspace_enabled(cx);
        let sidebar_side = self.sidebar_side(cx);
        let sidebar_on_right = sidebar_side == SidebarSide::Right;

        let panel_open = self.sidebar_open();
        // `None` when nothing is mounted in the title bar slot -- reserving a
        // row for a header that is not drawn would leave a bare strip.
        let title_bar_row_height = self
            .workspace()
            .read(cx)
            .titlebar_item()
            .map(|_| ui::utils::platform_title_bar_height(window));
        // A column that has taken the whole window takes the rail with it. The
        // rail is drawn here, as the workspace's sibling, so this is the only
        // place that can -- the workspace's own zoom overlay cannot reach it.
        //
        // Read, never written: the panel decides, and nothing here calls back
        // into it while it is being drawn.
        let column_fills_the_window = self
            .workspace()
            .read(cx)
            .a_column_fills_the_window(window, cx);

        let sidebar: Option<AnyElement> = if multi_workspace_enabled && !column_fills_the_window {
            self.sidebar.as_ref().map(|sidebar_handle| {
                let weak = cx.weak_entity();

                // The rail is always drawn; the panel only when open. The
                // sidebar view renders both, so the container has to
                // reserve the sum.
                let sidebar_width = sidebar_handle.rail_width(cx)
                    + if panel_open {
                        sidebar_handle.width(cx)
                    } else {
                        px(0.)
                    };
                let resize_handle = deferred(
                    div()
                        .id("sidebar-resize-handle")
                        .absolute()
                        .when(!sidebar_on_right, |el| {
                            el.right(-SIDEBAR_RESIZE_HANDLE_SIZE / 2.)
                        })
                        .when(sidebar_on_right, |el| {
                            el.left(-SIDEBAR_RESIZE_HANDLE_SIZE / 2.)
                        })
                        .top(px(0.))
                        .h_full()
                        .w(SIDEBAR_RESIZE_HANDLE_SIZE)
                        .cursor_col_resize()
                        .on_drag(DraggedSidebar, |dragged, _, _, cx| {
                            cx.stop_propagation();
                            cx.new(|_| dragged.clone())
                        })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .on_mouse_up(MouseButton::Left, move |event, _, cx| {
                            if event.click_count == 2 {
                                weak.update(cx, |this, cx| {
                                    if let Some(sidebar) = this.sidebar.as_mut() {
                                        sidebar.set_width(None, cx);
                                    }
                                    this.serialize(cx);
                                })
                                .ok();
                                cx.stop_propagation();
                            } else {
                                weak.update(cx, |this, cx| {
                                    this.serialize(cx);
                                })
                                .ok();
                            }
                        })
                        .occlude(),
                );

                // No surface treatment here, unlike the docks. Its margin and
                // border would have to come out of the column's own 48px, which
                // is exactly the rail's width -- the rail was being clipped by
                // the difference, and the margin read as a grey channel between
                // the rail and the editor. The rail draws its own right border
                // as the single separator instead.
                //
                // The `Workspace` beside it stacks title bar over centre over
                // status bar, so a full-height sidebar would run alongside the
                // title bar rather than starting where the centre does. The
                // spacer above reserves that row and paints it as title bar, so
                // the header reads as spanning the whole window the way VS Code's
                // does -- and on macOS the window controls land on it rather
                // than on the sidebar.
                // No `h_full` on the container — an explicit 100% height beats
                // `self_stretch` and, together with the surface margin, overflows
                // the column.
                v_flex()
                    .h_full()
                    .w(sidebar_width)
                    .flex_shrink_0()
                    .children(title_bar_row_height.map(|height| {
                        div()
                            .h(height)
                            .flex_none()
                            .bg(cx.theme().colors().title_bar_background)
                    }))
                    .child(
                        div()
                            .id("sidebar-container")
                            .debug_selector(|| "sidebar-container".into())
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .child(sidebar_handle.to_any())
                            // Nothing to resize while only the rail is showing --
                            // its width is fixed.
                            .when(panel_open, |this| this.child(resize_handle)),
                    )
                    .into_any_element()
            })
        } else {
            None
        };

        let (left_sidebar, right_sidebar) = if sidebar_on_right {
            (None, sidebar)
        } else {
            (sidebar, None)
        };

        let ui_font = theme_settings::setup_ui_font(window, cx);
        let text_color = cx.theme().colors().text;

        let workspace = self.workspace().clone();
        let workspace_key_context = workspace.update(cx, |workspace, cx| workspace.key_context(cx));
        let root = workspace.update(cx, |workspace, cx| workspace.actions(h_flex(), window, cx));

        client_side_decorations(
            root.key_context(workspace_key_context)
                .relative()
                .size_full()
                .font(ui_font)
                .text_color(text_color)
                .on_action(cx.listener(Self::close_window))
                .when(self.multi_workspace_enabled(cx), |this| {
                    this.on_action(cx.listener(
                        |this: &mut Self, _: &ToggleWorkspaceSidebar, window, cx| {
                            this.toggle_sidebar(window, cx);
                        },
                    ))
                    .on_action(cx.listener(
                        |this: &mut Self, _: &CloseWorkspaceSidebar, window, cx| {
                            this.close_sidebar_action(window, cx);
                        },
                    ))
                    .on_action(cx.listener(
                        |this: &mut Self, _: &FocusWorkspaceSidebar, window, cx| {
                            this.focus_sidebar(window, cx);
                        },
                    ))
                    // Project cycling is driven by `MultiWorkspace` directly so it
                    // works with no sidebar present (see `cycle_project`).
                    .on_action(cx.listener(|this: &mut Self, _: &NextProject, window, cx| {
                        this.cycle_project(true, window, cx);
                    }))
                    .on_action(
                        cx.listener(|this: &mut Self, _: &PreviousProject, window, cx| {
                            this.cycle_project(false, window, cx);
                        }),
                    )
                    .on_action(cx.listener(
                        |this: &mut Self, _: &DumpProjectResourceStats, _window, cx| {
                            this.dump_project_resource_stats(cx);
                        },
                    ))
                    .when(self.project_group_keys().len() >= 2, |el| {
                        el.on_action(cx.listener(
                            |this: &mut Self, _: &MoveProjectToNewWindow, window, cx| {
                                let key =
                                    this.project_group_key_for_workspace(this.workspace(), cx);
                                this.open_project_group_in_new_window(&key, window, cx)
                                    .detach_and_log_err(cx);
                            },
                        ))
                    })
                })
                .when(
                    self.sidebar_open() && self.multi_workspace_enabled(cx),
                    |this| {
                        this.on_drag_move(cx.listener(
                            move |this: &mut Self,
                                  e: &DragMoveEvent<DraggedSidebar>,
                                  window,
                                  cx| {
                                if let Some(sidebar) = &this.sidebar {
                                    // The pointer is over the outer edge of
                                    // rail + panel, but `width` measures the
                                    // panel alone.
                                    let new_width = if sidebar_on_right {
                                        window.bounds().size.width - e.event.position.x
                                    } else {
                                        e.event.position.x
                                    } - sidebar.rail_width(cx);
                                    sidebar.set_width(Some(new_width.max(px(0.))), cx);
                                }
                            },
                        ))
                    },
                )
                .children(left_sidebar)
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .size_full()
                        .overflow_hidden()
                        .child(self.workspace().clone()),
                )
                .children(right_sidebar)
                .child(self.workspace().read(cx).modal_layer.clone())
                .children(self.sidebar_overlay.as_ref().map(|view| {
                    deferred(div().absolute().size_full().inset_0().occlude().child(
                        v_flex().h(px(0.0)).top_20().items_center().child(
                            h_flex().occlude().child(view.clone()).on_mouse_down(
                                MouseButton::Left,
                                |_, _, cx| {
                                    cx.stop_propagation();
                                },
                            ),
                        ),
                    ))
                    .with_priority(2)
                })),
            window,
            cx,
            Tiling {
                left: !sidebar_on_right && multi_workspace_enabled && self.sidebar_open(),
                right: sidebar_on_right && multi_workspace_enabled && self.sidebar_open(),
                ..Tiling::default()
            },
        )
    }
}
