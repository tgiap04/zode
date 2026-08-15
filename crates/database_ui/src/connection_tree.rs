use crate::database_panel::{DatabasePanel, NodeState};
use database::protocol::TableKind;
use gpui::{Context, Window};
use ui::{Tooltip, prelude::*};

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
                indented(*depth)
                    .id(gpui::ElementId::Name(
                        format!("db-table-{index}-{schema}-{name}").into(),
                    ))
                    .cursor_pointer()
                    .hover(|style| style.bg(cx.theme().colors().element_hover))
                    .when(*open, |element| {
                        element.bg(cx.theme().colors().element_selected)
                    })
                    .child(
                        Icon::new(match kind {
                            TableKind::Table => IconName::Table,
                            TableKind::View | TableKind::MaterializedView => IconName::Eye,
                        })
                        .size(IconSize::Small)
                        .color(Color::Muted),
                    )
                    .child(Label::new(name.clone()).size(LabelSize::Small))
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
            } => indented(*depth)
                .child(
                    Icon::new(if *primary_key {
                        IconName::Hash
                    } else {
                        IconName::Dash
                    })
                    .size(IconSize::XSmall)
                    .color(if *primary_key {
                        Color::Accent
                    } else {
                        Color::Muted
                    }),
                )
                .child(Label::new(name.clone()).size(LabelSize::Small))
                .child(
                    Label::new(type_name.clone())
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
                // Only nullability is marked, and only when it holds: marking
                // both would put a badge on every row and say nothing.
                .when(*nullable, |element| {
                    element.child(
                        Label::new("null")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                })
                .into_any_element(),
            Row::Note { text, depth } => indented(*depth)
                .child(
                    Label::new(text.clone())
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .into_any_element(),
        }
    }

    fn render_connection(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(node) = self.connections.get(index) else {
            return div().into_any_element();
        };

        let (icon, colour, tooltip) = match &node.state {
            NodeState::Idle => (IconName::Database, Color::Muted, None),
            NodeState::Connecting => (IconName::ArrowCircle, Color::Muted, None),
            // Names the driver that actually answered, which is the only place
            // anyone can check that `postgres` in settings reached a PostgreSQL
            // driver rather than whatever else claimed the id.
            NodeState::Connected(session) => (
                IconName::Database,
                Color::Success,
                Some(SharedString::from(format!(
                    "{} · {}",
                    session.driver_name, node.config.url
                ))),
            ),
            NodeState::Failed(error) => (IconName::Warning, Color::Error, Some(error.clone())),
        };
        let expanded = node.expanded && matches!(node.state, NodeState::Connected(_));
        let name = node.config.name.clone();
        let pinned = self.is_pinned(&name);

        indented(0)
            .id(("database-connection", index))
            .cursor_pointer()
            .hover(|style| style.bg(cx.theme().colors().element_hover))
            .child(
                Icon::new(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(IconSize::XSmall)
                .color(Color::Muted),
            )
            .child(Icon::new(icon).size(IconSize::Small).color(colour))
            .child(Label::new(node.config.name.clone()).size(LabelSize::Small))
            .when_some(tooltip, |element, error: SharedString| {
                // The driver's own words, in full. "Could not connect" is worth
                // nothing next to "password authentication failed for user".
                element.tooltip(move |_window, cx| Tooltip::simple(error.clone(), cx))
            })
            // Only while open, and only then: a button offering to close what
            // is already closed is a button that does nothing.
            .when(matches!(node.state, NodeState::Connected(_)), |element| {
                element.child(
                    IconButton::new(("database-disconnect", index), IconName::Power)
                        .icon_size(IconSize::XSmall)
                        .tooltip(|_window, cx| {
                            Tooltip::simple("Disconnect, stopping the driver", cx)
                        })
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.disconnect(index, cx);
                        })),
                )
            })
            // A pinned connection is marked rather than reordered: a tree that
            // rearranges itself on a right-click loses the place you were in.
            .when(pinned, |element| {
                element.child(
                    Icon::new(IconName::Pin)
                        .size(IconSize::XSmall)
                        .color(Color::Accent),
                )
            })
            .on_click(cx.listener(move |this, _event, window, cx| {
                this.toggle_connection(index, window, cx);
                // Queries go where you last clicked, so the scratch buffer and
                // the grid follow the tree rather than needing their own picker.
                this.set_active(index, window, cx);
            }))
            // `on_mouse_down(Right, ..)` rather than `on_right_click`, which
            // only exists on the button components, not on a plain element.
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
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

        indented(1)
            .id(("database-schema", index * 1024 + schema_index))
            .cursor_pointer()
            .hover(|style| style.bg(cx.theme().colors().element_hover))
            .child(
                Icon::new(if schema.expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(IconSize::XSmall)
                .color(Color::Muted),
            )
            .child(Label::new(schema.schema.name.clone()).size(LabelSize::Small))
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.toggle_schema(index, schema_index, cx);
            }))
            .into_any_element()
    }
}

/// One tree line at `depth`. Padding rather than a spacer child, so a row with
/// no icon still lines up with one that has.
fn indented(depth: usize) -> Div {
    h_flex()
        .w_full()
        .flex_none()
        .gap_1()
        .px_2()
        .py_0p5()
        .pl(px(8.0 + depth as f32 * 14.0))
}
