use collections::HashSet;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, Pixels, Subscription, Task,
    WeakEntity, Window, actions,
};
use settings::Settings as _;
use workspace::Workspace;
use workspace::dock::{DockPosition, Panel, PanelEvent};

use crate::branch_panel::settings::BranchPanelSettings;
use crate::branch_panel::state::{BRANCH_PANEL_KEY, StoredKey};
use crate::branch_panel::tree::{RepoData, RowKey, TreeRow};

actions!(
    branch_panel,
    [
        /// Toggles the branch panel.
        Toggle,
        /// Toggles focus on the branch panel.
        ToggleFocus,
        /// Creates a new branch in the active repository.
        NewBranch,
    ]
);

pub fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
        workspace.toggle_panel_focus::<BranchPanel>(window, cx);
    });
    workspace.register_action(|workspace, _: &Toggle, window, cx| {
        if !workspace.toggle_panel_focus::<BranchPanel>(window, cx) {
            workspace.close_panel::<BranchPanel>(window, cx);
        }
    });
}

pub struct BranchPanel {
    /// Weak by construction: the workspace owns the dock which owns this panel,
    /// so a strong handle back would close the cycle and keep both alive for
    /// the life of the process.
    pub(crate) workspace: WeakEntity<Workspace>,
    pub(crate) focus_handle: FocusHandle,
    /// Height cache and scroll position for the row list. Rows are not a
    /// uniform height -- a branch card is two lines, a section header one --
    /// so `uniform_list` cannot draw them; `ListState` virtualizes variable
    /// heights instead.
    pub(crate) list_state: gpui::ListState,
    /// The one sweep of the agents' session stores, shared with the history
    /// panel. `None` until the panel is first drawn: reading the histories
    /// opens every transcript on disk, and none of that belongs on the startup
    /// path of a panel nobody has opened.
    pub(crate) session_store: Option<Entity<agent_ui::SessionStore>>,
    /// Dropped with the panel, so the store never notifies a dead handle.
    pub(crate) _session_subscription: Option<Subscription>,
    /// The variant of each row as `list_state` last saw it. A row's height is
    /// decided entirely by its variant, so this is enough to work out which
    /// slice of the list actually changed and splice only that -- resetting the
    /// whole list would throw the scroll position back to the top on every
    /// expand and collapse.
    pub(crate) row_kinds: Vec<std::mem::Discriminant<TreeRow>>,
    /// Whether the dock currently shows this panel. Everything expensive is
    /// gated on this: a closed panel reads nothing and rebuilds nothing.
    pub(crate) is_active: bool,
    /// Something changed while we were hidden (or just now). The rows are
    /// rebuilt on the next render rather than on the event, so a burst of
    /// events costs one rebuild instead of one each.
    pub(crate) stale: bool,
    /// How many times the tree has actually been rebuilt. The performance
    /// invariant this panel is built around -- a hidden panel does no work --
    /// is otherwise only true by construction, and construction is not
    /// evidence. See `lifecycle::tests`.
    pub(crate) rebuild_count: usize,
    pub(crate) repos: Vec<RepoData>,
    pub(crate) rows: Vec<TreeRow>,
    pub(crate) expanded: HashSet<RowKey>,
    /// Restored from disk before the repositories are known. Row keys carry a
    /// session-local `RepositoryId`, so what was stored is matched back by path
    /// the first time each repository is seen.
    pub(crate) stored_expanded: HashSet<StoredKey>,
    /// Network operations currently in flight, one slot per kind. Leaning on
    /// the fetch button must not spawn a queue of git processes.
    pub(crate) running_remote_ops: HashSet<crate::branch_panel::remote::RemoteOp>,
    /// The open right-click menu, its anchor, and the subscription that clears
    /// it on dismiss. Dropping the tuple drops all three together.
    pub(crate) context_menu: Option<(Entity<ui::ContextMenu>, gpui::Point<Pixels>, Subscription)>,
    pub(crate) pending_serialization: Task<Option<()>>,
    /// Subscriptions live and die with the panel. Never `.detach()` one that is
    /// tied to panel state -- a detached subscription outlives the entity it
    /// updates and fires into a dropped handle forever after.
    pub(crate) _subscriptions: Vec<Subscription>,
}

impl Focusable for BranchPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for BranchPanel {}

impl Panel for BranchPanel {
    fn persistent_name() -> &'static str {
        "BranchPanel"
    }

    fn panel_key() -> &'static str {
        BRANCH_PANEL_KEY
    }

    fn position(&self, _: &Window, cx: &App) -> DockPosition {
        BranchPanelSettings::get_global(cx).dock
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    fn set_position(&mut self, position: DockPosition, _: &mut Window, cx: &mut Context<Self>) {
        settings::update_settings_file(<dyn fs::Fs>::global(cx), cx, move |settings, _| {
            settings.branch_panel.get_or_insert_default().dock = Some(position.into())
        });
    }

    fn default_size(&self, _: &Window, cx: &App) -> Pixels {
        BranchPanelSettings::get_global(cx).default_width
    }

    fn icon(&self, _: &Window, cx: &App) -> Option<ui::IconName> {
        Some(ui::IconName::LayoutGrid).filter(|_| BranchPanelSettings::get_global(cx).button)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("Branches")
    }

    fn toggle_action(&self) -> Box<dyn gpui::Action> {
        Box::new(ToggleFocus)
    }

    fn starts_open(&self, _: &Window, cx: &App) -> bool {
        BranchPanelSettings::get_global(cx).starts_open
    }

    /// The dock tells the panel when it is shown or hidden. A panel that is
    /// hidden marks itself stale and does nothing further until it is shown
    /// again -- see the performance constraints in the plan.
    fn set_active(&mut self, active: bool, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_active == active {
            return;
        }
        self.is_active = active;
        if active {
            self.stale = true;
            cx.notify();
        }
    }

    fn activation_priority(&self) -> u32 {
        4
    }
}
