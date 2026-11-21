use crate::connection_store::{
    ConnectionConfig, DatabaseSettings, PINNED_CONNECTIONS_KEY, PinnedConnections, delete_secret,
    visible_connections,
};
use crate::driver_registry;
use crate::panel_layout::{self, LAYOUT_KEY, Split};
use crate::query::QueryState;
use crate::session::Session;
use crate::tree_filter::TreeFilter;
use collections::HashMap;
use database::protocol::{ColumnDef, SchemaRef, TableRef};
use editor::Editor;
use gpui::{
    AbsoluteLength, App, Context, Entity, EventEmitter, FocusHandle, Focusable, Pixels, Point,
    SharedString, Task, WeakEntity, Window,
};
use settings::{Settings as _, SettingsStore};
use std::sync::Arc;
use ui::{ContextMenu, prelude::*};
use ui_input::{ErasedEditorEvent, InputField};
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
    /// The load whose answer this node is waiting for, `0` for none. See
    /// [`DatabasePanel::next_load`].
    pub(crate) tables_load: u64,
}

pub(crate) struct ConnectionNode {
    pub(crate) config: ConnectionConfig,
    pub(crate) state: NodeState,
    pub(crate) expanded: bool,
    pub(crate) schemas: Vec<SchemaNode>,
    /// Set when the driver says it has only one schema worth showing (SQLite),
    /// so the tree does not spend a level on a lone `main` node.
    pub(crate) collapse_schema_level: bool,
    /// Set for a connection shared by the team. Never written to settings.
    pub(crate) team: Option<crate::team_connections::TeamOrigin>,
    /// Set for a connection the project's own `.idea/` names. Never written to
    /// settings either.
    pub(crate) from_project: bool,
    /// Opened on its own rather than by a click, so a refusal for want of a
    /// password waits for one instead of raising a prompt.
    pub(crate) unattended: bool,
    /// The list-of-schemas load this node is waiting for, `0` for none.
    pub(crate) schemas_load: u64,
}

impl ConnectionNode {
    /// Whether this is one of the user's own connections, kept in settings.
    pub(crate) fn is_personal(&self) -> bool {
        self.team.is_none() && !self.from_project
    }

    pub(crate) fn origin(&self) -> NodeOrigin {
        if self.team.is_some() {
            NodeOrigin::Team
        } else if self.from_project {
            NodeOrigin::Project
        } else {
            NodeOrigin::Personal
        }
    }

    pub(crate) fn idle(config: ConnectionConfig) -> Self {
        Self {
            config,
            state: NodeState::Idle,
            expanded: false,
            schemas: Vec::new(),
            collapse_schema_level: false,
            team: None,
            from_project: false,
            unattended: false,
            schemas_load: 0,
        }
    }
}

/// See [`DatabasePanel::remember_places`].
pub(crate) struct Places {
    active: Option<(String, NodeOrigin)>,
    open_table: Option<(String, NodeOrigin)>,
}

/// Where a node came from, which tells two nodes of the same name apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NodeOrigin {
    Personal,
    Project,
    Team,
}

/// What identifies a connection across a rebuild of the list: unlike its place
/// in `connections`, it survives another connection being added or removed.
/// Anything that waits -- an answer from a driver, a menu, a page that will be
/// sorted -- holds this and looks the place up when it acts.
pub(crate) type ConnectionKey = (String, NodeOrigin);

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
    /// The `describe_table` this is waiting for, `0` for none.
    pub(crate) columns_load: u64,
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
    /// How wide each column of the results stands, and the names they were
    /// sized for (see `result_columns`).
    pub(crate) result_column_widths: Entity<ui::ResizableColumnsState>,
    pub(crate) result_column_names: Vec<SharedString>,
    /// The results' scroll position, sideways as well as down.
    pub(crate) result_table: Entity<ui::TableInteractionState>,
    pub(crate) focus_handle: FocusHandle,
    /// The result grid's own focus, and the key context it brings
    /// (`DatabaseResults`): the arrow keys are bound there, because bound for
    /// the whole panel they also reach a text box in it that hands them on.
    pub(crate) results_focus: FocusHandle,
    pub(crate) workspace: WeakEntity<Workspace>,
    /// Whose drivers this panel's connections run on: this machine's, or the
    /// remote host's the project lives on. Found on first use, because the
    /// panel is built while its workspace is being updated and cannot read it.
    pub(crate) drivers: Option<driver_registry::DriverHost>,
    pub(crate) connections: Vec<ConnectionNode>,
    /// A line under a connection saying what just failed without taking
    /// anything away: a refresh that could not replace the rows still showing,
    /// or a table whose columns could not be read. Cleared by the next attempt,
    /// and kept by identity so it stays with its connection through a reload.
    pub(crate) notices: HashMap<ConnectionKey, SharedString>,
    pub(crate) open_table: Option<OpenTable>,
    pub(crate) pinned: PinnedConnections,
    /// The safety level of each team and project connection that has been given
    /// one on this machine, keyed by `safe_mode::override_key` (name and origin).
    pub(crate) safety_overrides: HashMap<String, crate::safe_mode::SafetyLevel>,
    /// The Open Data waiting for its table to be described, `0` for none. See
    /// [`DatabasePanel::next_load`].
    pub(crate) pending_open: u64,
    /// Which connection a query would run against: whichever was last opened or
    /// clicked. One editor for the view rather than one per connection -- people
    /// write a query, then decide where to point it.
    pub(crate) active: Option<usize>,
    pub(crate) sql_editor: Entity<Editor>,
    pub(crate) query: QueryState,
    /// What each connection ran, by connection name. See `query_history`.
    pub(crate) history: HashMap<String, crate::query_history_popover::HistoryState>,
    pub(crate) history_popover: Option<crate::query_history_popover::HistoryPopover>,
    pub(crate) export: crate::export_results::Export,
    /// Puts out the highlight on the statement that just ran.
    pub(crate) flash_clear: Option<Task<()>>,
    /// What the grid says about the save that just landed, for the page that
    /// reads the table again to show. See `grid_edit_save`.
    pub(crate) saved_note: Option<SharedString>,
    /// The filter bar over a table opened with Open Data. See
    /// `browse_filter_bar`.
    pub(crate) browse_bar: crate::browse_filter_bar::BrowseFilterBar,
    /// The scratch text each connection was last left with, so switching away
    /// and back does not lose a half-written statement.
    pub(crate) scratch: HashMap<String, String>,
    /// One per connection being opened or expanded, dropped when the view is,
    /// so closing the tab cancels whatever it had in flight. Added to through
    /// [`DatabasePanel::keep_task`], which lets go of the ones that finished.
    pub(crate) tasks: Vec<Task<()>>,
    /// Counted rather than random, so two queries in the same millisecond
    /// cannot share a request id and have `cancel` stop the wrong one.
    pub(crate) request_counter: u64,
    /// See [`DatabasePanel::next_load`].
    pub(crate) load_counter: u64,
    /// The open connection menu, its place on screen, and the subscription that
    /// clears it. Held by the panel rather than by a row: rows are recycled by
    /// the virtualised list, and a menu owned by one would vanish on scroll.
    pub(crate) context_menu: Option<(Entity<ContextMenu>, Point<Pixels>, gpui::Subscription)>,
    /// The box above the tree, and what was last typed into it. The query is
    /// parsed when it is typed rather than when a row is drawn, and lives here
    /// rather than on a node so a refresh or a settings reload keeps it.
    pub(crate) tree_filter_input: Entity<InputField>,
    pub(crate) tree_filter: TreeFilter,
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
    /// The "Team" group: its header, and what it says when it lists nothing.
    pub(crate) team_group: crate::team_connections::TeamGroup,
    _settings_observer: gpui::Subscription,
    _tree_filter_edited: gpui::Subscription,
    _sql_edited: gpui::Subscription,
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
        // An edit moves the text out from under the highlight, so it goes with
        // the edit rather than waiting for its timer.
        let sql_edited = cx.subscribe(
            &sql_editor,
            |this, _editor, event: &editor::EditorEvent, cx| {
                if matches!(event, editor::EditorEvent::BufferEdited) && this.flash_clear.is_some()
                {
                    this.clear_flash(cx);
                }
            },
        );

        let tree_filter_input = cx.new(|cx| {
            InputField::new(window, cx, "Filter tables").start_icon(IconName::MagnifyingGlass)
        });
        let tree_filter_edited = tree_filter_input.read(cx).editor().clone().subscribe(
            Box::new({
                let panel = cx.weak_entity();
                let input = tree_filter_input.downgrade();
                move |event, _window, cx| {
                    if event != ErasedEditorEvent::BufferEdited {
                        return;
                    }
                    let Some(text) = input.read_with(cx, |input, cx| input.text(cx)).ok() else {
                        return;
                    };
                    panel
                        .update(cx, |panel, cx| panel.set_tree_filter(&text, cx))
                        .ok();
                }
            }),
            window,
            cx,
        );

        let mut panel = Self {
            measured_width: None,
            result_column_widths: cx.new(|_| {
                ui::ResizableColumnsState::new(0, Vec::<AbsoluteLength>::new(), Vec::new())
            }),
            result_column_names: Vec::new(),
            result_table: cx.new(|cx| ui::TableInteractionState::new(cx)),
            focus_handle: cx.focus_handle(),
            results_focus: cx.focus_handle(),
            workspace,
            drivers: None,
            connections: Vec::new(),
            notices: HashMap::default(),
            open_table: None,
            pinned: PinnedConnections::default(),
            safety_overrides: crate::safe_mode::read_overrides(cx),
            pending_open: 0,
            active: None,
            sql_editor,
            query: QueryState::Idle,
            history: HashMap::default(),
            history_popover: None,
            flash_clear: None,
            saved_note: None,
            export: Default::default(),
            browse_bar: Default::default(),
            scratch: HashMap::default(),
            tasks: Vec::new(),
            request_counter: 0,
            load_counter: 0,
            context_menu: None,
            tree_filter_input,
            tree_filter: TreeFilter::default(),
            tree_height: panel_layout::DEFAULT_TREE_HEIGHT,
            sql_height: panel_layout::DEFAULT_SQL_HEIGHT,
            tree_width: panel_layout::DEFAULT_TREE_WIDTH,
            split_drag: None,
            team_group: Default::default(),
            _settings_observer: settings_observer,
            _tree_filter_edited: tree_filter_edited,
            _sql_edited: sql_edited,
        };
        panel.reload_connections(cx);
        panel.watch_team(cx);
        // Not read here: the panel is built while its workspace is being
        // updated, and reading the project's files needs the workspace.
        panel.keep_task(cx.spawn_in(window, async move |this, cx| {
            this.update_in(cx, |this, window, cx| {
                this.load_project_connections(window, cx)
            })
            .ok();
        }));
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
        // An entry of an open menu names its connection by its place, which is
        // about to move. Dismissed rather than dropped, so focus goes back to
        // the panel the way it does for any other way a menu closes. Deferred
        // because a menu entry can be what reloads the list (pinning does), and
        // the menu is being updated while its entry runs.
        if let Some((menu, _, _)) = &self.context_menu {
            let menu = menu.clone();
            cx.defer(move |cx| {
                menu.update(cx, |_, cx| cx.emit(gpui::DismissEvent));
            });
        }
        // `active` and the open table hold places in the list, and a removed
        // node shifts every place after it. Remembered by name across the
        // rebuild, or they would point at a different database than the one
        // the user opened.
        let places = self.remember_places();

        let mut existing = std::mem::take(&mut self.connections);
        self.connections = wanted
            .into_iter()
            .map(|config| {
                match existing
                    .iter()
                    .position(|node| node.is_personal() && node.config.name == config.name)
                {
                    // Same name, same url: leave the open session alone.
                    Some(index) if existing[index].config.url == config.url => {
                        let mut node = existing.remove(index);
                        node.config = config;
                        node
                    }
                    // The url moved, so whatever is open is open to the wrong
                    // database. Dropping the node drops the session with it.
                    _ => ConnectionNode::idle(config),
                }
            })
            .collect();
        // Project and team connections come from elsewhere, not from settings,
        // and stay after the personal ones in the order they already had.
        self.connections
            .extend(existing.into_iter().filter(|node| !node.is_personal()));
        self.restore_places(places);
        let connections = &self.connections;
        self.notices.retain(|(name, origin), _| {
            connections
                .iter()
                .any(|node| node.config.name == *name && node.origin() == *origin)
        });
        cx.notify();
    }

    /// Where the active connection and the open table are, by name: a rebuild
    /// of the list moves both, and a place remembered by position would point
    /// at a different database than the one the user opened.
    pub(crate) fn remember_places(&self) -> Places {
        Places {
            active: self.active.and_then(|index| self.node_key(index)),
            open_table: self
                .open_table
                .as_ref()
                .and_then(|open| self.node_key(open.connection)),
        }
    }

    /// Puts the active connection and the open table back where their
    /// databases now are, or clears them when the database is gone.
    pub(crate) fn restore_places(&mut self, places: Places) {
        let Places {
            active: active_key,
            open_table: open_key,
        } = places;
        if let Some(key) = active_key {
            self.active = self.index_of_key(&key);
            if self.active.is_none() {
                self.clear_result_of(&key);
            }
        }
        if let Some(key) = open_key {
            match self.index_of_key(&key) {
                Some(index) => {
                    if let Some(open) = self.open_table.as_mut() {
                        open.connection = index;
                    }
                }
                None => self.open_table = None,
            }
        }
    }

    /// What identifies a node across a rebuild: its name, and whether it came
    /// from the team, since a team connection may share a personal one's name.
    pub(crate) fn node_key(&self, index: usize) -> Option<ConnectionKey> {
        self.connections
            .get(index)
            .map(|node| (node.config.name.clone(), node.origin()))
    }

    pub(crate) fn index_of_key(&self, (name, origin): &ConnectionKey) -> Option<usize> {
        self.connections
            .iter()
            .position(|node| node.config.name == *name && node.origin() == *origin)
    }

    /// Holds a task for as long as the panel lives, letting go of the ones
    /// that already finished -- a finished task has nothing left to cancel, and
    /// kept for good they would add one per query to the end of the session.
    pub(crate) fn keep_task(&mut self, task: Task<()>) {
        self.tasks.retain(|kept| !kept.is_ready());
        self.tasks.push(task);
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

    /// Asks before removing a connection, then removes it.
    pub(crate) fn confirm_remove_connection(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self
            .connections
            .get(index)
            .filter(|node| node.is_personal())
        else {
            return;
        };
        let name = node.config.name.clone();
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &format!("Remove the connection \"{name}\"?"),
            Some(
                "Its saved password is removed from this machine as well. \
                 The database itself is not touched.",
            ),
            &["Remove", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await.ok() == Some(0) {
                this.update(cx, |this, cx| this.remove_connection(&name, cx))
                    .ok();
            }
        })
        .detach();
    }

    /// Removes a personal connection and everything this machine kept for it:
    /// the open session, the half-written query, the pin, the keychain entry,
    /// and the entry in settings -- which is what takes the node off the tree.
    ///
    /// Team connections are not in settings and are not removed from here.
    pub(crate) fn remove_connection(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(index) = self
            .connections
            .iter()
            .position(|node| node.is_personal() && node.config.name == name)
        else {
            return;
        };
        let url = self.connections[index].config.url.clone();

        self.disconnect(index, cx);
        self.scratch.remove(name);
        if self.pinned.is_pinned(name) {
            self.toggle_pin(name, cx);
        }

        // The keychain holds one password per URL, and a team connection finds
        // its password the same way, so another connection to the same server
        // is still using it.
        let still_used = DatabaseSettings::get_global(cx)
            .connections
            .iter()
            .any(|connection| connection.name != name && connection.url == url)
            || self
                .connections
                .iter()
                .any(|node| !node.is_personal() && node.config.url == url);
        if !still_used {
            delete_secret(zode_credentials_provider::global(cx), url, cx).detach_and_log_err(cx);
        }

        let Ok(fs) = self.workspace.read_with(cx, |workspace, cx| {
            workspace.project().read(cx).fs().clone()
        }) else {
            return;
        };
        let name = name.to_string();
        settings::update_settings_file(fs, cx, move |content, _cx| {
            if let Some(connections) = content
                .database
                .as_mut()
                .and_then(|database| database.connections.as_mut())
            {
                connections.retain(|connection| connection.name.as_deref() != Some(name.as_str()));
            }
        });
    }

    pub(crate) fn set_tree_filter(&mut self, text: &str, cx: &mut Context<Self>) {
        let filter = TreeFilter::new(text);
        if filter != self.tree_filter {
            self.tree_filter = filter;
            cx.notify();
        }
    }

    /// Whether `name` is pinned. Read by the tree to mark the row, and by the
    /// context menu to name the action correctly.
    pub(crate) fn is_pinned(&self, name: &str) -> bool {
        self.pinned.is_pinned(name)
    }

    /// The statement builder for a connection, `None` until it is open: the
    /// quote character is something the driver says once it has been asked.
    pub(crate) fn dialect(&self, index: usize) -> Option<crate::sql_dialect::Dialect> {
        let node = self.connections.get(index)?;
        let NodeState::Connected(session) = &node.state else {
            return None;
        };
        Some(crate::sql_dialect::Dialect::detect(
            &node.config.driver,
            &session.capabilities,
        ))
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
