use crate::database_panel::{ConnectionNode, DatabasePanel, NodeState};
use crate::safe_mode::SafetyLevel;
use database::protocol::{ColumnDef, TableKind, TableRef};
use gpui::{ClickEvent, Context, Div, MouseButton, MouseDownEvent, Stateful, Window};
use ui::{
    CommonAnimationExt as _, HighlightedLabel, ListItem, ListItemSpacing, Tooltip, prelude::*,
};

/// One step per level, the same for every row: a connection, a schema and a
/// table line up with each other only if they agree on it.
pub(crate) const INDENT_STEP: Pixels = px(16.);

/// One line of the tree, already flattened.
///
/// Flat rather than nested because the list is virtualised: `uniform_list` asks
/// for row `n`, and a nested model would have to walk the tree to answer that.
pub(crate) enum Row {
    /// Above the connections the project's own `.idea/` names.
    ProjectHeader,
    /// "Team · Acme", above the connections the team shares.
    TeamHeader {
        name: SharedString,
    },
    Connection {
        index: usize,
    },
    Schema {
        index: usize,
        schema_index: usize,
    },
    Table {
        index: usize,
        schema: SharedString,
        name: SharedString,
        kind: TableKind,
        depth: usize,
        open: bool,
    },
    /// One column of the open table: name, type, and whether it is part of the
    /// primary key -- which is what tells you this is the table you meant.
    Column {
        index: usize,
        name: SharedString,
        type_name: SharedString,
        primary_key: bool,
        nullable: bool,
        depth: usize,
    },
    /// A schema that has been opened but whose tables have not arrived, or one
    /// that genuinely has none. Silence here reads as a hang.
    Note {
        text: SharedString,
        depth: usize,
    },
}

impl DatabasePanel {
    /// The tree as lines. With text in the filter box it narrows to the tables
    /// (and, for the open table, columns) that match, looking inside schemas
    /// and connections whether or not they are expanded: a filter that only
    /// searched what happened to be open would miss most of the tree.
    pub(crate) fn rows(&self) -> Vec<Row> {
        let filter = &self.tree_filter;
        let mut rows = Vec::new();
        let mut project_header_pending = true;
        let mut team_header_pending = true;
        for (index, node) in self.connections.iter().enumerate() {
            let mut children = Vec::new();
            let matched = self.push_connection_children(index, node, &mut children);
            if filter.is_active() && !matched && !filter.matches(&node.config.name) {
                continue;
            }
            if node.from_project && project_header_pending {
                rows.push(Row::ProjectHeader);
                project_header_pending = false;
            }
            if node.team.is_some() && team_header_pending {
                self.push_team_header(&mut rows);
                team_header_pending = false;
            }
            rows.push(Row::Connection { index });
            rows.extend(children);
        }
        if filter.is_active() {
            if rows.is_empty() {
                rows.push(Row::Note {
                    text: format!("Nothing matches “{}”", filter.text()).into(),
                    depth: 0,
                });
            }
        } else if team_header_pending {
            self.push_team_header(&mut rows);
        }
        rows
    }

    /// What sits under a connection, and whether any table in it matched the
    /// filter (always `false` without one).
    fn push_connection_children(
        &self,
        index: usize,
        node: &ConnectionNode,
        rows: &mut Vec<Row>,
    ) -> bool {
        let filtering = self.tree_filter.is_active();
        if !matches!(node.state, NodeState::Connected(_)) || !(node.expanded || filtering) {
            return false;
        }
        let mut matched = false;
        if !filtering && !self.notices.is_empty() {
            let key = (node.config.name.clone(), node.origin());
            if let Some(text) = self.notices.get(&key) {
                rows.push(Row::Note {
                    text: text.clone(),
                    depth: 1,
                });
            }
        }
        for (schema_index, schema) in node.schemas.iter().enumerate() {
            let depth = if node.collapse_schema_level { 1 } else { 2 };
            // A driver with a single schema for good (SQLite) gets its tables
            // drawn straight under the connection.
            let schema_row = (!node.collapse_schema_level).then_some(Row::Schema {
                index,
                schema_index,
            });
            let schema_open = node.collapse_schema_level || schema.expanded;

            if !filtering {
                rows.extend(schema_row);
                if !schema_open {
                    continue;
                }
                match &schema.tables {
                    None => rows.push(Row::Note {
                        text: "Loading…".into(),
                        depth,
                    }),
                    Some(tables) if tables.is_empty() => rows.push(Row::Note {
                        text: "No tables".into(),
                        depth,
                    }),
                    Some(tables) => {
                        self.push_table_rows(index, &schema.schema.name, tables, depth, rows);
                    }
                }
                continue;
            }

            match &schema.tables {
                Some(tables) => {
                    let mut table_rows = Vec::new();
                    if self.push_table_rows(
                        index,
                        &schema.schema.name,
                        tables,
                        depth,
                        &mut table_rows,
                    ) {
                        matched = true;
                        rows.extend(schema_row);
                        rows.extend(table_rows);
                    }
                }
                // Says why this schema contributed nothing, but only where the
                // user has the connection open: the note is a pointer into a
                // part of the tree they are looking at.
                None if node.expanded => {
                    rows.extend(schema_row);
                    rows.push(Row::Note {
                        text: if schema_open {
                            "Loading…"
                        } else {
                            "Not loaded — expand to search"
                        }
                        .into(),
                        depth,
                    });
                }
                None => {}
            }
        }
        matched
    }

    /// The tables of one schema that pass the filter, each followed by its
    /// columns when it is the open table. Whether any table was drawn.
    ///
    /// A table that matches by name shows every column; one that matches only
    /// through a column shows just the columns that did.
    fn push_table_rows(
        &self,
        index: usize,
        schema: &str,
        tables: &[TableRef],
        depth: usize,
        rows: &mut Vec<Row>,
    ) -> bool {
        let filter = &self.tree_filter;
        let mut drawn = false;
        for table in tables {
            let open_table = self.open_table.as_ref().filter(|open| {
                open.connection == index && open.schema == schema && open.table == table.name
            });
            let columns = open_table.and_then(|open| open.columns.as_deref());
            let name_matches = filter.matches(&table.name);
            let column_matches = |column: &ColumnDef| filter.matches(&column.name);
            if !name_matches && !columns.is_some_and(|columns| columns.iter().any(column_matches)) {
                continue;
            }
            drawn = true;
            rows.push(Row::Table {
                index,
                schema: schema.to_string().into(),
                name: table.name.clone().into(),
                kind: table.kind,
                depth,
                open: open_table.is_some(),
            });
            if open_table.is_none() {
                continue;
            }
            match columns {
                None => rows.push(Row::Note {
                    text: "Loading columns…".into(),
                    depth: depth + 1,
                }),
                Some(columns) => {
                    rows.extend(
                        columns
                            .iter()
                            .filter(|column| name_matches || column_matches(column))
                            .map(|column| Row::Column {
                                index,
                                name: column.name.clone().into(),
                                type_name: column.type_name.clone().into(),
                                primary_key: column.primary_key,
                                nullable: column.nullable,
                                depth: depth + 1,
                            }),
                    );
                }
            }
        }
        drawn
    }

    fn push_team_header(&self, rows: &mut Vec<Row>) {
        let Some(name) = self.team_group.name.clone() else {
            return;
        };
        rows.push(Row::TeamHeader { name });
        if let Some(text) = self.team_group.note.clone() {
            rows.push(Row::Note { text, depth: 0 });
        }
    }

    pub(crate) fn render_row(
        &self,
        position: usize,
        row: &Row,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match row {
            Row::ProjectHeader => tree_row(("database-project-header", position), 0)
                .selectable(false)
                .child(
                    row_content()
                        .child(
                            Icon::new(IconName::Folder)
                                .size(IconSize::XSmall)
                                .color(Color::Muted),
                        )
                        .child(
                            Label::new("Project")
                                .size(LabelSize::Small)
                                .color(Color::Muted)
                                .truncate(),
                        ),
                )
                .end_slot(
                    IconButton::new("database-project-reload", IconName::RotateCw)
                        .icon_size(IconSize::Small)
                        .tooltip(|_window, cx| {
                            Tooltip::simple("Read this project's .idea data sources again", cx)
                        })
                        .on_click(cx.listener(|this, _event, window, cx| {
                            this.load_project_connections(window, cx)
                        })),
                )
                .into_any_element(),
            Row::TeamHeader { name } => tree_row(("database-team-header", position), 0)
                .selectable(false)
                .child(
                    row_content()
                        .child(
                            Icon::new(IconName::Person)
                                .size(IconSize::XSmall)
                                .color(Color::Muted),
                        )
                        .child(
                            Label::new(format!("Team · {name}"))
                                .size(LabelSize::Small)
                                .color(Color::Muted)
                                .truncate(),
                        ),
                )
                .end_slot(
                    IconButton::new("database-team-reload", IconName::RotateCw)
                        .icon_size(IconSize::Small)
                        .tooltip(|_window, cx| {
                            Tooltip::simple("Load the connections your team shares", cx)
                        })
                        .on_click(
                            cx.listener(|this, _event, _window, cx| this.load_team_connections(cx)),
                        ),
                )
                .into_any_element(),
            Row::Connection { index } => self.render_connection(*index, cx),
            Row::Schema {
                index,
                schema_index,
            } => self.render_schema(*index, *schema_index, cx),
            Row::Table {
                index,
                schema,
                name,
                kind,
                depth,
                open,
            } => {
                let (index, schema, name) = (*index, schema.clone(), name.clone());
                let id = gpui::ElementId::Name(format!("db-table-{index}-{schema}-{name}").into());
                let item = tree_row(id.clone(), *depth)
                    .toggle_state(*open)
                    .start_slot(
                        Icon::new(match kind {
                            TableKind::Table => IconName::Table,
                            TableKind::View | TableKind::MaterializedView => IconName::Eye,
                        })
                        .size(IconSize::Small)
                        .color(Color::Muted),
                    )
                    .child(
                        row_content().child(
                            HighlightedLabel::new(name.clone(), self.tree_filter.highlight(&name))
                                .truncate(),
                        ),
                    );
                let (menu_schema, menu_name) = (schema.clone(), name.clone());
                clickable(id, item)
                    // Both at once: clicking a table means "show me this
                    // table", and that is its shape *and* its rows. Splitting
                    // them across two gestures would make the common one two
                    // clicks.
                    .on_click(cx.listener(move |this, _event, window, cx| {
                        this.click_table(index, &schema, &name, window, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.deploy_table_menu(
                                index,
                                menu_schema.to_string(),
                                menu_name.to_string(),
                                event.position,
                                window,
                                cx,
                            );
                        }),
                    )
                    .into_any_element()
            }
            Row::Column {
                index,
                name,
                type_name,
                primary_key,
                nullable,
                depth,
            } => {
                let (index, column) = (*index, name.to_string());
                let item = tree_row(("database-column", position), *depth)
                    .selectable(false)
                    .child(
                        row_content()
                            .child(
                                HighlightedLabel::new(
                                    name.clone(),
                                    self.tree_filter.highlight(name),
                                )
                                .truncate(),
                            )
                            // The primary key is what tells you this is the table you
                            // meant.
                            .when(*primary_key, |content| {
                                content.child(
                                    Icon::new(IconName::Hash)
                                        .size(IconSize::XSmall)
                                        .color(Color::Accent),
                                )
                            })
                            .child(
                                Label::new(type_name.clone())
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            // Only nullability is marked, and only when it holds: marking
                            // both would put a badge on every row and say nothing.
                            .when(*nullable, |content| {
                                content.child(
                                    Label::new("null")
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted),
                                )
                            }),
                    );
                // A plain wrapper rather than `clickable`: a column has no
                // left-click, so no pointer cursor either.
                div()
                    .id(("database-column-menu", position))
                    .w_full()
                    .child(item)
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.deploy_column_menu(
                                index,
                                column.clone(),
                                event.position,
                                window,
                                cx,
                            );
                        }),
                    )
                    .into_any_element()
            }
            // Truncated: a driver's own words can run to a page, and a row is
            // one line high.
            Row::Note { text, depth } => tree_row(("database-note", position), *depth)
                .selectable(false)
                .child(
                    row_content().child(
                        Label::new(text.clone())
                            .size(LabelSize::Small)
                            .color(Color::Muted)
                            .truncate(),
                    ),
                )
                .into_any_element(),
        }
    }

    /// What a click anywhere on a connection does, and what its arrow does.
    fn choose_connection(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_connection(index, window, cx);
        // Queries go where you last clicked, so the scratch buffer and
        // the grid follow the tree rather than needing their own picker.
        self.set_active(index, window, cx);
    }

    fn render_connection(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(node) = self.connections.get(index) else {
            return div().into_any_element();
        };

        let glyph = |icon: IconName, colour: Color| {
            Icon::new(icon)
                .size(IconSize::Small)
                .color(colour)
                .into_any_element()
        };
        let (leading, tooltip) = match &node.state {
            NodeState::Idle => (glyph(IconName::Database, Color::Muted), None),
            // Keyed by the row: the list recycles them, and an animation keyed
            // by position in the code would restart whenever one scrolls away.
            NodeState::Connecting => (
                Icon::new(IconName::LoadCircle)
                    .size(IconSize::Small)
                    .color(Color::Muted)
                    .with_keyed_rotate_animation(("database-connecting", index), 2)
                    .into_any_element(),
                None,
            ),
            // Names the driver that actually answered, which is the only place
            // anyone can check that `postgres` in settings reached a PostgreSQL
            // driver rather than whatever else claimed the id.
            NodeState::Connected(session) => (
                glyph(IconName::Database, Color::Success),
                Some(SharedString::from(format!(
                    "{} · {}",
                    session.driver_name, node.config.url
                ))),
            ),
            NodeState::Failed(error) => {
                (glyph(IconName::Warning, Color::Error), Some(error.clone()))
            }
        };
        let expanded = node.expanded && matches!(node.state, NodeState::Connected(_));
        let connecting = matches!(node.state, NodeState::Connecting);
        let connected = matches!(node.state, NodeState::Connected(_));
        let name = node.config.name.clone();
        let pinned = self.is_pinned(&name);
        let safety = self.safety_of(index, cx);

        // One button, two jobs, and never both at once: what a connection
        // needs is either to be closed or to be opened again, and the row
        // already says which of those it is.
        //
        // Nothing while connecting -- a stop button that cannot stop
        // anything would be the third thing this row does not need.
        let power = (!connecting).then(|| {
            IconButton::new(
                ("database-connection-power", index),
                if connected {
                    IconName::Power
                } else {
                    IconName::RotateCw
                },
            )
            .icon_size(IconSize::Small)
            .tooltip(move |_window, cx| {
                Tooltip::simple(
                    if connected {
                        "Disconnect, stopping the driver"
                    } else {
                        "Connect again"
                    },
                    cx,
                )
            })
            .on_click(cx.listener(move |this, _event, window, cx| {
                if connected {
                    this.disconnect(index, cx);
                } else {
                    this.reconnect(index, window, cx);
                }
            }))
        });

        let item = tree_row(("database-connection-item", index), 0)
            .toggle(expanded)
            .on_toggle(cx.listener(move |this, _event: &ClickEvent, window, cx| {
                this.choose_connection(index, window, cx);
            }))
            .start_slot(leading)
            .child(
                row_content()
                    .child(Label::new(name).truncate())
                    // A pinned connection is marked rather than reordered: a tree that
                    // rearranges itself on a right-click loses the place you were in.
                    .when(pinned, |content| {
                        content.child(
                            Icon::new(IconName::Pin)
                                .size(IconSize::XSmall)
                                .color(Color::Accent),
                        )
                    })
                    // Only the two levels that differ from what a connection
                    // does when nobody said anything: a badge on every row
                    // would be a badge on none.
                    .when_some(safety_badge(safety), |content, (text, colour)| {
                        content.child(
                            div()
                                .id(("database-safety-badge", index))
                                .debug_selector(|| format!("database-safety-badge-{index}"))
                                .child(Label::new(text).size(LabelSize::XSmall).color(colour)),
                        )
                    }),
            )
            .end_slot::<IconButton>(power);

        clickable(("database-connection", index), item)
            // The driver's own words, in full. "Could not connect" is worth
            // nothing next to "password authentication failed for user".
            .when_some(tooltip, |element, text: SharedString| {
                element.tooltip(move |_window, cx| Tooltip::simple(text.clone(), cx))
            })
            .on_click(cx.listener(move |this, _event, window, cx| {
                this.choose_connection(index, window, cx);
            }))
            // `on_mouse_down(Right, ..)` rather than `on_right_click`, which
            // only exists on the button components, not on a plain element.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.deploy_connection_menu(index, event.position, window, cx);
                }),
            )
            .into_any_element()
    }

    fn render_schema(
        &self,
        index: usize,
        schema_index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(schema) = self
            .connections
            .get(index)
            .and_then(|node| node.schemas.get(schema_index))
        else {
            return div().into_any_element();
        };
        let key = index * 1024 + schema_index;

        let item = tree_row(("database-schema-item", key), 1)
            .toggle(schema.expanded)
            .on_toggle(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                this.toggle_schema(index, schema_index, cx);
            }))
            .child(row_content().child(Label::new(schema.schema.name.clone()).truncate()));

        let name = schema.schema.name.clone();
        clickable(("database-schema", key), item)
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.toggle_schema(index, schema_index, cx);
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.deploy_schema_menu(index, name.clone(), event.position, window, cx);
                }),
            )
            .into_any_element()
    }
}

/// One tree line at `depth`.
///
/// One level deeper than the depth says: the arrow hangs in the gutter to the
/// left of a row, so the shallowest rows need a gutter of their own, and rows
/// with no arrow still line up with the ones that have one.
fn tree_row(id: impl Into<ElementId>, depth: usize) -> ListItem {
    ListItem::new(id)
        .spacing(ListItemSpacing::Dense)
        .indent_step_size(INDENT_STEP)
        .indent_level(depth + 1)
        .always_show_disclosure_icon(true)
}

/// What a row says, as tall as a button.
///
/// `uniform_list` measures the first row and gives every row that height, so a
/// row that carries a button and one that carries none have to agree on one.
fn row_content() -> Div {
    h_flex().h(ButtonSize::Default.rems()).min_w_0().gap_1()
}

/// The click target for a line that does something.
///
/// `ListItem` indents with a margin on the part that takes the click, so its
/// own handler leaves the indent dead while the hover highlight spans it.
fn clickable(id: impl Into<ElementId>, item: ListItem) -> Stateful<Div> {
    div().id(id).w_full().cursor_pointer().child(item)
}

/// What the row says about a connection's safety level, if anything.
fn safety_badge(level: SafetyLevel) -> Option<(&'static str, Color)> {
    match level {
        SafetyLevel::ReadOnly => Some(("read-only", Color::Muted)),
        SafetyLevel::Unrestricted => Some(("unrestricted", Color::Warning)),
        SafetyLevel::ConfirmWrites => None,
    }
}
