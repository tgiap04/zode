use crate::connection_store::{
    ConnectionConfig, DatabaseSettings, PINNED_CONNECTIONS_KEY, PinnedConnections,
    visible_connections,
};
use crate::driver_registry;
use crate::panel_layout::{self, LAYOUT_KEY, Split};
use crate::query::QueryState;
use crate::session::Session;
use collections::HashMap;
use database::protocol::{ColumnDef, SchemaRef, TableRef};
use database::registry::DriverRegistry;
use editor::Editor;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, Pixels, Point, SharedString, Task,
    WeakEntity, Window,
};
use settings::{Settings as _, SettingsStore};
use std::sync::Arc;
use ui::{ContextMenu, prelude::*};
use workspace::Workspace;
use workspace::dock::PanelEvent;

/// What a connection node is doing. `Failed` keeps the driver's own words --
/// "password authentication failed" is worth far more than "could not connect".
pub(crate) enum NodeState {
    Idle,
    Connecting,
    Connected(Arc<Session>),
    Failed(SharedString),
}

pub(crate) struct SchemaNode {
    pub(crate) schema: SchemaRef,
    pub(crate) expanded: bool,
    /// `None` until asked for. A Postgres with a few thousand tables would
    /// stall the first click if this were filled in eagerly.
    pub(crate) tables: Option<Vec<TableRef>>,
}

pub(crate) struct ConnectionNode {
    pub(crate) config: ConnectionConfig,
    pub(crate) state: NodeState,
    pub(crate) expanded: bool,
    pub(crate) schemas: Vec<SchemaNode>,
    /// Set when the driver says it has only one schema worth showing (SQLite),
    /// so the tree does not spend a level on a lone `main` node.
    pub(crate) collapse_schema_level: bool,
}

/// The table whose columns are showing, if any.
///
/// One at a time, and panel-wide rather than per node: opening a second table
/// is nearly always instead of the first, not as well as, and keeping every
/// table ever clicked expanded turns the tree into a wall.
pub(crate) struct OpenTable {
    pub(crate) connection: usize,
    pub(crate) schema: String,
    pub(crate) table: String,
    /// `None` while `describe_table` is still out.
    pub(crate) columns: Option<Vec<ColumnDef>>,
}

pub struct DatabasePanel {
    /// The width the view was last drawn at, when it is standing on its own.
    ///
    /// Measured rather than asked for: a tab is as wide as the pane holds and a
    /// window is as wide as somebody dragged it, and neither number exists until
    /// the frame is laid out. Read one frame late, which is what a `canvas`
    /// measurement costs and is invisible at this scale -- the layout only
    /// changes when the width crosses `SIDE_BY_SIDE_WIDTH`.
    pub(crate) measured_width: Option<Pixels>,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) workspace: WeakEntity<Workspace>,
    /// Shared with every other window: extensions add drivers to it after
    /// startup, and a panel holding its own copy would keep answering from the
    /// list it was built with.
    pub(crate) registry: Entity<DriverRegistry>,
    pub(crate) connections: Vec<ConnectionNode>,
    pub(crate) open_table: Option<OpenTable>,
    pub(crate) pinned: PinnedConnections,
    /// Which connection a query would run against: whichever was last opened or
    /// clicked. One editor for the view rather than one per connection -- people
    /// write a query, then decide where to point it.
    pub(crate) active: Option<usize>,
    pub(crate) sql_editor: Entity<Editor>,
    pub(crate) query: QueryState,
    /// The scratch text each connection was last left with, so switching away
    /// and back does not lose a half-written statement.
    pub(crate) scratch: HashMap<String, String>,
    /// One per connection being opened or expanded, dropped when the view is,
    /// so closing the tab cancels whatever it had in flight.
    pub(crate) tasks: Vec<Task<()>>,
    /// Counted rather than random, so two queries in the same millisecond
    /// cannot share a request id and have `cancel` stop the wrong one.
    pub(crate) request_counter: u64,
    /// The open connection menu, its place on screen, and the subscription that
    /// clears it. Held by the panel rather than by a row: rows are recycled by
    /// the virtualised list, and a menu owned by one would vanish on scroll.
    pub(crate) context_menu: Option<(Entity<ContextMenu>, Point<Pixels>, gpui::Subscription)>,
    pub(crate) tree_height: Pixels,
    pub(crate) sql_height: Pixels,
    /// How wide the table list stands when it stands beside the data. Only read
    /// there: stacked, the list is as wide as the view and there is no second
    /// region beside it to take the space from.
    pub(crate) tree_width: Pixels,
    /// Which handle is being dragged and where the pointer was last seen, so a
    /// drag can move a region by the distance travelled rather than by where in
    /// the window the pointer happens to be.
    pub(crate) split_drag: Option<(Split, Pixels)>,
    _settings_observer: gpui::Subscription,
}

impl DatabasePanel {
    pub fn new(workspace: &Workspace, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::detached(
            workspace.weak_handle(),
            workspace.project().read(cx).languages().clone(),
            window,
            cx,
        )
    }

    /// A view for a tab or a window of its own, with a session of its own.
    ///
    /// Takes the two things it needs rather than the workspace, because the
    /// callers cannot hand one over: both run inside `register_action`, which
    /// leases the workspace for the whole handler, and the floating window
    /// builds its view in a *different* window's context where a second
    /// `&Workspace` cannot be borrowed alongside the `App` that creates the
    /// entity. Reading these two out first is what makes both call sites legal.
    pub fn standalone(
        workspace: WeakEntity<Workspace>,
        languages: Arc<language::LanguageRegistry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::detached(workspace, languages, window, cx)
    }

    /// The one place a view is built.
    ///
    /// There is one host now: an editor tab, or a window of its own, which lays
    /// itself out the same way. The column this used to also live in is gone --
    /// a result grid is the one thing here that cannot be made narrow and stay
    /// readable, and a column is the one place that cannot be wide.
    fn detached(
        workspace: WeakEntity<Workspace>,
        languages: Arc<language::LanguageRegistry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings_observer = cx.observe_global::<SettingsStore>(|this, cx| {
            this.reload_connections(cx);
        });

        let sql_editor = cx.new(|cx| {
            let mut editor = Editor::multi_line(window, cx);
            editor.set_placeholder_text("SELECT …", window, cx);
            editor
        });
        Self::apply_sql_language(&sql_editor, languages, cx);

        let mut panel = Self {
            measured_width: None,
            focus_handle: cx.focus_handle(),
            workspace,
            registry: driver_registry::global(cx),
            connections: Vec::new(),
            open_table: None,
            pinned: PinnedConnections::default(),
            active: None,
            sql_editor,
            query: QueryState::Idle,
            scratch: HashMap::default(),
            tasks: Vec::new(),
            request_counter: 0,
            context_menu: None,
            tree_height: panel_layout::DEFAULT_TREE_HEIGHT,
            sql_height: panel_layout::DEFAULT_SQL_HEIGHT,
            tree_width: panel_layout::DEFAULT_TREE_WIDTH,
            split_drag: None,
            _settings_observer: settings_observer,
        };
        panel.reload_connections(cx);
        panel
    }

    /// Gives the scratch buffer SQL highlighting, if the language is installed.
    ///
    /// Best effort: the buffer is perfectly usable as plain text, and failing to
    /// open the column because a grammar is missing would be absurd.
    fn apply_sql_language(
        editor: &Entity<Editor>,
        languages: Arc<language::LanguageRegistry>,
        cx: &mut Context<Self>,
    ) {
        let editor = editor.downgrade();
        cx.spawn(async move |_this, cx| {
            let Ok(language) = languages.language_for_name("SQL").await else {
                return;
            };
            editor
                .update(cx, |editor, cx| {
                    editor.buffer().update(cx, |buffer, cx| {
                        if let Some(buffer) = buffer.as_singleton() {
                            buffer.update(cx, |buffer, cx| {
                                buffer.set_language(Some(language), cx);
                            });
                        }
                    });
                })
                .ok();
        })
        .detach();
    }

    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: gpui::AsyncWindowContext,
    ) -> anyhow::Result<Entity<Self>> {
        let pinned = Self::load_pins(&workspace, &mut cx);
        let layout = Self::load_layout(&workspace, &mut cx);
        workspace.update_in(&mut cx, |workspace, window, cx| {
            cx.new(|cx| {
                let mut panel = DatabasePanel::new(workspace, window, cx);
                panel.pinned = pinned;
                (panel.tree_height, panel.sql_height, panel.tree_width) = layout;
                panel.reload_connections(cx);
                panel
            })
        })
    }

    /// How this project last left the column's regions. Read once at load, for
    /// the same reason as the pins: nothing else writes them.
    fn load_layout(
        workspace: &WeakEntity<Workspace>,
        cx: &mut gpui::AsyncWindowContext,
    ) -> (Pixels, Pixels, Pixels) {
        let (heights, width) = workspace
            .update(cx, |workspace, cx| {
                (
                    workspace.load_workspace_state::<Vec<f32>>(LAYOUT_KEY, "heights", cx),
                    workspace.load_workspace_state::<f32>(LAYOUT_KEY, "tree-width", cx),
                )
            })
            .unwrap_or((None, None));
        let (tree_height, sql_height) = Self::saved_heights(heights);
        (tree_height, sql_height, Self::saved_tree_width(width))
    }

    /// Which connections this project pinned last time.
    ///
    /// Read once at load rather than watched: the pins only change through this
    /// panel, so there is nothing else to hear about.
    fn load_pins(
        workspace: &WeakEntity<Workspace>,
        cx: &mut gpui::AsyncWindowContext,
    ) -> PinnedConnections {
        workspace
            .update(cx, |workspace, cx| {
                workspace
                    .load_workspace_state::<Vec<String>>(PINNED_CONNECTIONS_KEY, "names", cx)
                    .unwrap_or_default()
            })
            .map(PinnedConnections::from_names)
            .unwrap_or_default()
    }

    /// Rebuilds the node list from settings, keeping whatever is already open.
    ///
    /// Matched by name rather than rebuilt wholesale: editing an unrelated
    /// connection in settings should not tear down a session someone is
    /// browsing, and settings fire on every keystroke in the settings file.
    pub(crate) fn reload_connections(&mut self, cx: &mut Context<Self>) {
        let configured = DatabaseSettings::get_global(cx).connections.clone();
        let wanted = visible_connections(&configured, &self.pinned);

        let mut existing = std::mem::take(&mut self.connections);
        self.connections = wanted
            .into_iter()
            .map(|config| {
                match existing
                    .iter()
                    .position(|node| node.config.name == config.name)
                {
                    // Same name, same url: leave the open session alone.
                    Some(index) if existing[index].config.url == config.url => {
                        let mut node = existing.remove(index);
                        node.config = config;
                        node
                    }
                    // The url moved, so whatever is open is open to the wrong
                    // database. Dropping the node drops the session with it.
                    _ => ConnectionNode {
                        config,
                        state: NodeState::Idle,
                        expanded: false,
                        schemas: Vec::new(),
                        collapse_schema_level: false,
                    },
                }
            })
            .collect();
        cx.notify();
    }

    /// Pins or unpins a connection for this project, and writes it down.
    ///
    /// A machine may hold thirty databases; a project usually cares about one
    /// or two. Pinning is how the tree stops being a list of everything.
    pub(crate) fn toggle_pin(&mut self, name: &str, cx: &mut Context<Self>) {
        self.pinned.toggle(name);
        let names = self.pinned.to_names();
        self.workspace
            .update(cx, |workspace, cx| {
                workspace.persist_workspace_state(PINNED_CONNECTIONS_KEY, "names", &names, cx);
            })
            .ok();
        self.reload_connections(cx);
    }

    /// Whether `name` is pinned. Read by the tree to mark the row, and by the
    /// context menu to name the action correctly.
    pub(crate) fn is_pinned(&self, name: &str) -> bool {
        self.pinned.is_pinned(name)
    }

    /// Where a connection sits in the tree, by name.
    ///
    /// Names are unique, which is what the add-connection dialog enforces, so
    /// this is how something that only knows a name reaches its node.
    pub(crate) fn index_of(&self, name: &str) -> Option<usize> {
        self.connections
            .iter()
            .position(|node| node.config.name == name)
    }

    /// The connections showing, in order. For tests: asserting against the node
    /// list beats taking an element tree apart to ask the same question.
    #[cfg(test)]
    pub(crate) fn connection_names(&self) -> Vec<String> {
        self.connections
            .iter()
            .map(|node| node.config.name.clone())
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn connection_url(&self, name: &str) -> Option<String> {
        self.connections
            .iter()
            .find(|node| node.config.name == name)
            .map(|node| node.config.url.clone())
    }
}

impl EventEmitter<PanelEvent> for DatabasePanel {}

impl Focusable for DatabasePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
