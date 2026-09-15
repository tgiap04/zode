use std::path::Path;
use std::sync::Arc;

use collections::{HashMap, HashSet};
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, Pixels, Subscription, Task,
    WeakEntity, Window, actions,
};
use settings::Settings as _;
use workspace::Workspace;
use workspace::dock::{DockPosition, Panel, PanelEvent};

use crate::branch_panel::checkout_state::CheckoutViewState;
use crate::branch_panel::settings::BranchPanelSettings;
use crate::branch_panel::state::BRANCH_PANEL_KEY;
use crate::branch_panel::tree::{AgentEntry, RepoData, SubagentKey, TreeRow};

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
    /// What the reader decided about the checkouts -- open/closed, pinned,
    /// dragged order -- held once for the whole process rather than copied
    /// into every panel. See `checkout_state`'s own module doc for why: a
    /// checkout switch used to build a second `Workspace`, a second panel, and
    /// read a *different* per-workspace record, so nothing was ever reset --
    /// a different drawer was opened. Keyed by path, there is one drawer, and
    /// every panel watching this entity sees the same one.
    pub(crate) checkout_state: Entity<CheckoutViewState>,
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
    /// One per open agent tab, so a rename redraws the row that names it.
    ///
    /// Rebuilt whenever the panel rebuilds, which is exactly when the set of
    /// tabs can have changed. Each holds only a weak handle to its view, so a
    /// closed tab is never kept alive by being listened to, and the listener
    /// retires itself once the view is gone.
    pub(crate) _agent_tab_names: Vec<Subscription>,
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
    /// Per checkout, the last non-empty agent list this panel actually saw.
    ///
    /// Two sources feed a checkout's live list, and each has its own reason to
    /// blink empty for a moment that says nothing about the checkout itself:
    /// an open tab is workspace-local and vanishes from `repo.agents` the
    /// instant it closes, while the disk index is process-global and only as
    /// fresh as its last sweep -- so a checkout can sit truthfully non-empty
    /// while the two sources hand off and the merged answer, just for that
    /// rebuild, is `None`. This is what a checkout last actually showed, kept
    /// so `hold_known_agents` can hand it back for exactly that window rather
    /// than the row blinking closed and reopening around a fact that never
    /// changed. See `hold_known_agents` for the three-case rule and why this
    /// is never written to disk.
    pub(crate) last_known_agents: HashMap<Arc<Path>, Arc<[AgentEntry]>>,
    pub(crate) rows: Vec<TreeRow>,
    /// Which agent rows have their subagent list open.
    ///
    /// In memory rather than in `checkout_state`, unlike every other disclosure
    /// here. The others are a shape you arrange once and expect back tomorrow;
    /// this is a look inside one session's work while you are watching it, and
    /// persisting it would put a row in the store for every session anyone ever
    /// opened. Closed by default for the same reason a list is worth having at
    /// all: one session here had spawned twenty-five subagents, and twenty-five
    /// rows under each of several sessions is a panel nobody can read.
    pub(crate) expanded_subagents: HashSet<SubagentKey>,
    /// Subagents of sessions with no tab open on them. An open tab keeps its own
    /// tracker and is never in here.
    ///
    /// Filled once per time the session is drawn as finished, because the read
    /// touches disk and a row redraws sixty times a second. Dropped when the
    /// session stops being drawn that way -- including when it is resumed into
    /// a tab, which is what stops the subagents it gains while open from being
    /// missed after it closes again.
    pub(crate) past_subagents: HashMap<Arc<str>, agent_ui::SubagentTracker>,
    /// The reads in flight, one per session, held so they stop with the panel.
    ///
    /// Trimmed to the sessions currently drawn on every rebuild, along with the
    /// two maps above -- see `follow_listed_subagents`, which is also where a
    /// resumed session gets its stale entry dropped. Nothing here removes an
    /// entry from inside the task that is running it, which is the hazard that
    /// shape would otherwise invite.
    pub(crate) _subagent_reads: HashMap<Arc<str>, Task<()>>,
    /// Network operations currently in flight, one slot per kind. Leaning on
    /// the fetch button must not spawn a queue of git processes.
    pub(crate) running_remote_ops: HashSet<crate::branch_panel::remote::RemoteOp>,
    /// Whether a reload the user asked for is still running, which is what turns the
    /// header's reload icon into a spinner.
    pub(crate) reloading: bool,
    /// Held so the reload stops when the panel is dropped, and so a second press replaces
    /// the first wait rather than stacking another one behind it.
    pub(crate) _reload_task: Option<Task<()>>,
    /// The open right-click menu, its anchor, and the subscription that clears
    /// it on dismiss. Dropping the tuple drops all three together.
    pub(crate) context_menu: Option<(Entity<ui::ContextMenu>, gpui::Point<Pixels>, Subscription)>,
    /// Redraws the panel while a live agent is listed.
    ///
    /// An agent's mark changes when nothing else about the row does -- no git
    /// event, no rebuild -- so without a tick a spinner would never settle to
    /// a dot and a dot would never become a spinner. Held in a field so it
    /// stops when the panel is dropped, and cleared when the panel is hidden
    /// or nothing live is listed: this is the only thing here that costs
    /// anything while the user is doing nothing.
    pub(crate) _activity_tick: Option<Task<()>>,
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
        } else {
            // Nothing is drawing it, so nothing needs waking.
            self._activity_tick = None;
        }
    }

    fn activation_priority(&self) -> u32 {
        3
    }
}
