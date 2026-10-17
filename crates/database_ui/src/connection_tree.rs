use crate::database_panel::{DatabasePanel, NodeState};
use database::protocol::TableKind;
use gpui::{ClickEvent, Context, Div, MouseButton, MouseDownEvent, Stateful, Window};
use ui::{CommonAnimationExt as _, ListItem, ListItemSpacing, Tooltip, prelude::*};

/// One step per level, the same for every row: a connection, a schema and a
/// table line up with each other only if they agree on it.
pub(crate) const INDENT_STEP: Pixels = px(16.);

/// One line of the tree, already flattened.
///
/// Flat rather than nested because the list is virtualised: `uniform_list` asks
/// for row `n`, and a nested model would have to walk the tree to answer that.
pub(crate) enum Row {
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
    pub(crate) fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for (index, node) in self.connections.iter().enumerate() {
            rows.push(Row::Connection { index });
            if !node.expanded || !matches!(node.state, NodeState::Connected(_)) {
                continue;
            }

            for (schema_index, schema) in node.schemas.iter().enumerate() {
                // A driver with a single schema for good (SQLite) gets its
                // tables drawn straight under the connection.
                if !node.collapse_schema_level {
                    rows.push(Row::Schema {
                        index,
                        schema_index,
                    });
                    if !schema.expanded {
                        continue;
                    }
                }

                let depth = if node.collapse_schema_level { 1 } else { 2 };
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
                        for table in tables {
                            let open = self.open_table.as_ref().is_some_and(|open| {
                                open.connection == index
                                    && open.schema == schema.schema.name
                                    && open.table == table.name
                            });
                            rows.push(Row::Table {
                                index,
                                schema: schema.schema.name.clone().into(),
                                name: table.name.clone().into(),
                                kind: table.kind,
                                depth,
                                open,
                            });
                            if !open {
                                continue;
                            }
                            match self
                                .open_table
                                .as_ref()
                                .and_then(|open| open.columns.as_ref())
                            {
                                None => rows.push(Row::Note {
                                    text: "Loading columns…".into(),
                                    depth: depth + 1,
                                }),
                                Some(columns) => {
                                    rows.extend(columns.iter().map(|column| Row::Column {
                                        name: column.name.clone().into(),
                                        type_name: column.type_name.clone().into(),
                                        primary_key: column.primary_key,
                                        nullable: column.nullable,
                                        depth: depth + 1,
                                    }))
                                }
                            }
                        }
                    }
                }
            }
        }
        rows
    }

    pub(crate) fn render_row(
        &self,
        position: usize,
        row: &Row,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match row {
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
                    .child(row_content().child(Label::new(name.clone()).truncate()));
                clickable(id, item)
                    // Both at once: clicking a table means "show me this
                    // table", and that is its shape *and* its rows. Splitting
                    // them across two gestures would make the common one two
                    // clicks.
                    .on_click(cx.listener(move |this, _event, window, cx| {
                        this.toggle_table(index, &schema, &name, cx);
                        this.preview_table(index, &schema, &name, window, cx);
                    }))
                    .into_any_element()
            }
            Row::Column {
                name,
                type_name,
                primary_key,
                nullable,
                depth,
            } => tree_row(("database-column", position), *depth)
                .selectable(false)
                .child(
                    row_content()
                        .child(Label::new(name.clone()).truncate())
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
                )
                .into_any_element(),
            Row::Note { text, depth } => tree_row(("database-note", position), *depth)
                .selectable(false)
                .child(
                    row_content().child(
                        Label::new(text.clone())
                            .size(LabelSize::Small)
                            .color(Color::Muted),
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

        clickable(("database-schema", key), item)
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.toggle_schema(index, schema_index, cx);
            }))
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
