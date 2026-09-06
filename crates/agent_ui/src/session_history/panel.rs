use agent_sessions::{AgentKind, SessionCounts, SessionProvider, SessionSummary};
use anyhow::Result;
use collections::HashMap;
use editor::Editor;
use gpui::{
    App, AsyncWindowContext, Context, Entity, EventEmitter, FocusHandle, Focusable, Subscription,
    WeakEntity, Window,
};
use std::{path::PathBuf, sync::Arc};
use ui::{IconName, prelude::*};
use workspace::{
    Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

/// The element group the panel's root declares. Hover-scoped controls inside a
/// row answer to it, so one const rather than a literal per call site.
pub(crate) const PANEL_GROUP: &str = "agent-history-panel";

/// Where the expensive per-session numbers are in their journey.
#[derive(Clone, Copy, Debug)]
pub(crate) enum CountState {
    /// A background task is scanning the transcript. The row shows `…`.
    Pending,
    Ready(SessionCounts),
}

/// Past agent sessions, for the project this workspace has open.
///
/// **Scoped to the project on purpose.** Every workspace builds its own panel
/// (`zed::initialize_workspace` observes each new `Workspace`), and the list is
/// filtered by that workspace's own worktree roots — so switching project in the
/// rail switches the history with it, without this panel watching anything.
pub struct AgentHistoryPanel {
    workspace: WeakEntity<Workspace>,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) providers: Vec<Arc<dyn SessionProvider>>,
    /// The one sweep of both stores, shared with every other surface that needs
    /// it. Held rather than copied into a field of this panel: a second copy is
    /// a second thing to keep in step, and a second sweep to fill it. Filtering
    /// is a view over the index, recomputed per render.
    pub(crate) store: Entity<crate::SessionStore>,
    pub(crate) counts: HashMap<Arc<str>, CountState>,
    /// The real editor behind the search box. Its text is the filter — read on
    /// render rather than mirrored into a field, so the two cannot disagree.
    pub(crate) filter_editor: Entity<Editor>,
    /// Which agent sections are closed.
    pub(crate) collapsed_agents: collections::HashSet<AgentKind>,
    /// Which project sections are closed, keyed by the agent they sit under.
    ///
    /// The pair matters: the same project appears under every agent the user ran
    /// there, and keying by path alone would make closing it under Claude close it
    /// under Codex too.
    pub(crate) collapsed_groups: collections::HashSet<(AgentKind, PathBuf)>,
    pub(crate) expanded_rows: collections::HashSet<Arc<str>>,
    pub(crate) loading: bool,
    /// Set once the panel has been visible, so a closed panel never touches the
    /// disk: nothing about the history belongs on the startup path.
    loaded_once: bool,
    pub(crate) width: Option<Pixels>,
    /// A variable-height list, not a uniform one: a group header and a session
    /// row are different heights, and `uniform_list` measures the first item and
    /// spaces every row by it — which draws the rows overlapping each other.
    pub(crate) list_state: gpui::ListState,
    _subscriptions: Vec<Subscription>,
}

impl AgentHistoryPanel {
    pub fn new(workspace: &Workspace, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = crate::SessionStore::global(cx);
        let filter_editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("Search sessions", window, cx);
            editor
        });
        let subscriptions = vec![
            // A project whose worktrees change while the panel is open must
            // re-filter, and the roots are read straight from the project on every
            // render, so an observe that only notifies is all this needs.
            cx.observe(&workspace.project().clone(), |_, _, cx| cx.notify()),
            cx.subscribe(&filter_editor, |_, _, event, cx| {
                if matches!(event, editor::EditorEvent::BufferEdited) {
                    cx.notify();
                }
            }),
            // The sweep finishes on the store, not here. Without this the panel
            // would show an empty list until something else happened to notify it.
            cx.observe(&store, |this, _, cx| {
                this.loading = false;
                cx.notify();
            }),
        ];
        Self {
            filter_editor,
            _subscriptions: subscriptions,
            workspace: workspace.weak_handle(),
            focus_handle: cx.focus_handle(),
            providers: agent_sessions::default_providers(),
            store,
            counts: HashMap::default(),
            collapsed_agents: Default::default(),
            collapsed_groups: Default::default(),
            expanded_rows: Default::default(),
            loading: false,
            loaded_once: false,
            width: None,
            list_state: gpui::ListState::new(0, gpui::ListAlignment::Top, px(400.)),
        }
    }

    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> Result<Entity<Self>> {
        workspace.update_in(&mut cx, |workspace, window, cx| {
            cx.new(|cx| Self::new(workspace, window, cx))
        })
    }

    /// The worktree roots of this panel's own project.
    ///
    /// The list shows only sessions that ran inside one of these. A workspace with
    /// no worktree yet (a fresh window) shows nothing rather than everything —
    /// "every session on this machine" is not what the panel is for.
    pub(crate) fn project_roots(&self, cx: &App) -> Vec<PathBuf> {
        let Some(workspace) = self.workspace.upgrade() else {
            return Vec::new();
        };
        workspace
            .read(cx)
            .project()
            .read(cx)
            .visible_worktrees(cx)
            .map(|worktree| worktree.read(cx).abs_path().to_path_buf())
            .collect()
    }

    /// Asks the shared store for a fresh sweep. The result arrives through the
    /// observe registered in `new`, not through a task owned here -- two panels
    /// asking at once must still cost one sweep.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.counts.clear();
        self.store.update(cx, |store, cx| store.refresh(cx));
    }

    /// Every session both stores can see, unfiltered.
    pub(crate) fn sessions<'a>(&self, cx: &'a App) -> &'a [agent_sessions::SessionSummary] {
        self.store.read(cx).index().sessions()
    }

    /// Fetch the counts for one session, once.
    ///
    /// Called from the list for rows that are actually on screen. The `Pending`
    /// mark is what keeps a fast scroll from spawning the same scan repeatedly.
    pub(crate) fn ensure_counts(&mut self, session: &SessionSummary, cx: &mut Context<Self>) {
        if self.counts.contains_key(&session.id) {
            return;
        }
        let Some(provider) = self
            .providers
            .iter()
            .find(|provider| provider.agent() == session.agent)
            .cloned()
        else {
            return;
        };
        self.counts.insert(session.id.clone(), CountState::Pending);
        let session = session.clone();
        let id = session.id.clone();
        cx.spawn(async move |this, cx| {
            let counts = cx
                .background_spawn(async move { provider.counts(&session) })
                .await;
            this.update(cx, |this, cx| {
                match counts {
                    Ok(counts) => {
                        this.counts.insert(id, CountState::Ready(counts));
                    }
                    Err(error) => {
                        log::warn!("counting a session failed: {error}");
                        this.counts
                            .insert(id, CountState::Ready(SessionCounts::default()));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn provider_for(
        &self,
        session: &SessionSummary,
    ) -> Option<Arc<dyn SessionProvider>> {
        self.providers
            .iter()
            .find(|provider| provider.agent() == session.agent)
            .cloned()
    }

    pub(crate) fn workspace(&self) -> &WeakEntity<Workspace> {
        &self.workspace
    }

    pub(crate) fn query(&self, cx: &App) -> String {
        self.filter_editor.read(cx).text(cx)
    }

    /// Stores whose absence the list should mention rather than hide.
    pub(crate) fn unavailable(&self) -> Vec<(&'static str, String)> {
        self.providers
            .iter()
            .filter_map(|provider| match provider.availability() {
                agent_sessions::Availability::Ready => None,
                agent_sessions::Availability::Unavailable(reason) => {
                    Some((provider.agent().label(), reason))
                }
            })
            .collect()
    }
}

impl EventEmitter<PanelEvent> for AgentHistoryPanel {}

impl Focusable for AgentHistoryPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for AgentHistoryPanel {
    fn persistent_name() -> &'static str {
        "Agent History"
    }

    fn panel_key() -> &'static str {
        "AgentHistoryPanel"
    }

    /// The tool column, opposite the rail. Pinned for the same reason the project
    /// panel is: the button that opens it lives in that column's own header, and a
    /// panel that could wander away from its button is a panel you cannot find.
    fn position(&self, _window: &Window, _cx: &App) -> DockPosition {
        DockPosition::Right
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Right)
    }

    /// Ignored, like the project panel's.
    fn set_position(
        &mut self,
        _position: DockPosition,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    fn default_size(&self, _window: &Window, _cx: &App) -> Pixels {
        self.width.unwrap_or(px(420.))
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(IconName::Astroid)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("Agent History")
    }

    fn toggle_action(&self) -> Box<dyn gpui::Action> {
        Box::new(zed_actions::agent::ToggleHistory)
    }

    /// Closed until asked for. Opening it is what makes it read the disk.
    fn starts_open(&self, _window: &Window, _cx: &App) -> bool {
        false
    }

    /// After the project panel, so the tool column still opens on the file tree.
    fn activation_priority(&self) -> u32 {
        5
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active && !self.loaded_once {
            self.loaded_once = true;
            self.refresh(cx);
        }
    }
}
