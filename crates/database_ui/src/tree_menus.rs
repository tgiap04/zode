//! The right-click menus below a connection: schema, table and column, and
//! what their entries do.
//!
//! What a menu offers is decided by [`table_actions`], a plain function of the
//! dialect, so "a document store has no `INSERT` to generate" is a fact a test
//! can state without taking a menu apart.

use crate::database_panel::{DatabasePanel, NodeState};
use crate::query::QueryError;
use crate::sql_dialect::{Dialect, Feature};
use database::protocol::ColumnDef;
use gpui::{ClipboardItem, DismissEvent, Focusable as _, Pixels, Point, WeakEntity, Window};
use ui::{ContextMenu, ContextMenuEntry, prelude::*};

/// A statement to write into the editor for a table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Generate {
    Select,
    Insert,
    Update,
    Delete,
}

impl Generate {
    const ALL: [Generate; 4] = [
        Generate::Select,
        Generate::Insert,
        Generate::Update,
        Generate::Delete,
    ];

    fn icon(self) -> IconName {
        match self {
            Generate::Select => IconName::MagnifyingGlass,
            Generate::Insert => IconName::SquarePlus,
            Generate::Update => IconName::Pencil,
            Generate::Delete => IconName::Trash,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Generate::Select => "SELECT",
            Generate::Insert => "INSERT",
            Generate::Update => "UPDATE",
            Generate::Delete => "DELETE",
        }
    }
}

/// One thing the table menu can do. `Generate` stands for one entry of the
/// "Generate" submenu each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TableAction {
    OpenData,
    CountRows,
    CopyName,
    CopyQualifiedName,
    Generate(Generate),
    Refresh,
}

impl TableAction {
    fn label(self) -> &'static str {
        match self {
            TableAction::OpenData => "Open Data",
            TableAction::CountRows => "Count Rows",
            TableAction::CopyName => "Copy Name",
            TableAction::CopyQualifiedName => "Copy Qualified Name",
            TableAction::Generate(kind) => kind.label(),
            TableAction::Refresh => "Refresh",
        }
    }

    fn icon(self) -> IconName {
        match self {
            TableAction::OpenData => IconName::Table,
            TableAction::CountRows => IconName::Hash,
            TableAction::CopyName => IconName::Copy,
            TableAction::CopyQualifiedName => IconName::BookCopy,
            TableAction::Generate(kind) => kind.icon(),
            TableAction::Refresh => IconName::RotateCw,
        }
    }

    /// Whether a separator goes above this entry.
    fn starts_group(self) -> bool {
        matches!(self, TableAction::CopyName | TableAction::Refresh)
    }
}

/// What the table menu offers on this engine, in order.
pub(crate) fn table_actions(dialect: &Dialect) -> Vec<TableAction> {
    let mut actions = vec![TableAction::OpenData];
    if dialect.supports(Feature::Count) {
        actions.push(TableAction::CountRows);
    }
    actions.push(TableAction::CopyName);
    actions.push(TableAction::CopyQualifiedName);
    if dialect.supports(Feature::WriteTemplates) {
        actions.extend(Generate::ALL.map(TableAction::Generate));
    }
    actions.push(TableAction::Refresh);
    actions
}

/// The statement for `kind`, or `None` when the table has no columns to build
/// it from or the engine cannot express it.
///
/// A table with no primary key still gets an `UPDATE` or `DELETE`, with its
/// `WHERE` left as a comment to write.
pub(crate) fn generated_statement(
    dialect: &Dialect,
    kind: Generate,
    schema: &str,
    table: &str,
    columns: &[ColumnDef],
) -> Option<String> {
    let names: Vec<&str> = columns.iter().map(|column| column.name.as_str()).collect();
    let keys: Vec<&str> = columns
        .iter()
        .filter(|column| column.primary_key)
        .map(|column| column.name.as_str())
        .collect();
    // Setting a key column to itself is noise, but a table made only of key
    // columns still has to be able to produce an `UPDATE`.
    let settable: Vec<&str> = columns
        .iter()
        .filter(|column| !column.primary_key)
        .map(|column| column.name.as_str())
        .collect();
    let settable = if settable.is_empty() || keys.is_empty() {
        names.clone()
    } else {
        settable
    };

    match kind {
        Generate::Select => dialect
            .select_page(schema, table, Some(&names), None, None)
            .map(|sql| format!("{sql};")),
        Generate::Insert => dialect.insert_template(schema, table, &names),
        Generate::Update if keys.is_empty() => {
            dialect.update_template_without_key(schema, table, &settable)
        }
        Generate::Update => dialect.update_template(schema, table, &settable, &keys),
        Generate::Delete if keys.is_empty() => dialect.delete_template_without_key(schema, table),
        Generate::Delete => dialect.delete_template(schema, table, &keys),
    }
}

/// An entry with its icon at the start, the way every database menu draws one.
pub(crate) fn icon_entry(
    menu: ContextMenu,
    label: impl Into<SharedString>,
    icon: IconName,
    handler: impl Fn(&mut Window, &mut App) + 'static,
) -> ContextMenu {
    menu.item(
        ContextMenuEntry::new(label)
            .icon(icon)
            .icon_position(IconPosition::Start)
            .icon_color(Color::Muted)
            .handler(handler),
    )
}

/// An entry that calls back into the panel.
fn panel_entry(
    menu: ContextMenu,
    label: &'static str,
    icon: IconName,
    panel: &WeakEntity<DatabasePanel>,
    action: impl Fn(&mut DatabasePanel, &mut Window, &mut Context<DatabasePanel>) + 'static,
) -> ContextMenu {
    let panel = panel.clone();
    icon_entry(menu, label, icon, move |window, cx| {
        panel.update(cx, |panel, cx| action(panel, window, cx)).ok();
    })
}

impl DatabasePanel {
    /// Puts a menu on the panel at `position` and clears it when it closes.
    ///
    /// Held by the panel rather than by a row: rows are recycled by the
    /// virtualised list, and a menu owned by one would vanish on scroll.
    pub(crate) fn open_context_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
        build: impl FnOnce(ContextMenu, &mut Window, &mut Context<ContextMenu>) -> ContextMenu,
    ) {
        let menu = ContextMenu::build(window, cx, build);
        cx.focus_view(&menu, window);
        let subscription =
            cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, window, cx| {
                if this
                    .context_menu
                    .as_ref()
                    .is_some_and(|(menu, _, _)| menu.focus_handle(cx).contains_focused(window, cx))
                {
                    cx.focus_self(window);
                }
                this.context_menu.take();
                cx.notify();
            });

        self.context_menu = Some((menu, position, subscription));
        cx.notify();
    }

    pub(crate) fn deploy_schema_menu(
        &mut self,
        index: usize,
        schema: String,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_connected(index) {
            return;
        }
        let panel = cx.weak_entity();
        self.open_context_menu(position, window, cx, move |menu, _window, _cx| {
            let menu = panel_entry(menu, "Refresh", IconName::RotateCw, &panel, {
                let schema = schema.clone();
                move |panel, _window, cx| panel.refresh_schema(index, &schema, cx)
            });
            let menu = panel_entry(menu, "Copy Name", IconName::Copy, &panel, {
                let schema = schema.clone();
                move |_panel, _window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(schema.clone()));
                }
            });
            panel_entry(
                menu,
                "New Query Here",
                IconName::SquarePlus,
                &panel,
                move |panel, window, cx| panel.new_query(index, window, cx),
            )
        });
    }

    pub(crate) fn deploy_table_menu(
        &mut self,
        index: usize,
        schema: String,
        table: String,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dialect) = self.dialect(index) else {
            return;
        };
        let actions = table_actions(&dialect);
        let panel = cx.weak_entity();
        self.open_context_menu(position, window, cx, move |mut menu, _window, _cx| {
            let generating: Vec<TableAction> = actions
                .iter()
                .copied()
                .filter(|action| matches!(action, TableAction::Generate(_)))
                .collect();
            let mut submenu_added = false;
            for action in actions {
                if matches!(action, TableAction::Generate(_)) {
                    if submenu_added {
                        continue;
                    }
                    submenu_added = true;
                    let (panel, schema, table, generating) = (
                        panel.clone(),
                        schema.clone(),
                        table.clone(),
                        generating.clone(),
                    );
                    menu = menu.separator().submenu_with_icon(
                        "Generate",
                        IconName::Code,
                        move |mut submenu, _window, _cx| {
                            for action in &generating {
                                submenu =
                                    table_entry(submenu, *action, &panel, index, &schema, &table);
                            }
                            submenu
                        },
                    );
                    continue;
                }
                if action.starts_group() {
                    menu = menu.separator();
                }
                menu = table_entry(menu, action, &panel, index, &schema, &table);
            }
            menu
        });
    }

    pub(crate) fn deploy_column_menu(
        &mut self,
        index: usize,
        column: String,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dialect) = self.dialect(index) else {
            return;
        };
        let panel = cx.weak_entity();
        self.open_context_menu(position, window, cx, move |menu, _window, _cx| {
            let menu = panel_entry(menu, "Copy Name", IconName::Copy, &panel, {
                let column = column.clone();
                move |_panel, _window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(column.clone()));
                }
            });
            panel_entry(
                menu,
                "Insert Name into Editor",
                IconName::CursorIBeam,
                &panel,
                move |panel, window, cx| {
                    panel.insert_into_editor(index, dialect.quote_ident(&column), window, cx)
                },
            )
        });
    }

    fn run_table_action(
        &mut self,
        action: TableAction,
        index: usize,
        schema: &str,
        table: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            TableAction::OpenData => self.open_table_data(index, schema, table, window, cx),
            TableAction::CountRows => self.count_rows(index, schema, table, window, cx),
            TableAction::CopyName => {
                cx.write_to_clipboard(ClipboardItem::new_string(table.to_string()));
            }
            TableAction::CopyQualifiedName => {
                if let Some(dialect) = self.dialect(index) {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        dialect.qualified_name(schema, table),
                    ));
                }
            }
            TableAction::Generate(kind) => {
                self.generate_statement(index, schema, table, kind, window, cx)
            }
            TableAction::Refresh => self.refresh_table(index, schema, table, cx),
        }
    }

    /// Writes a statement for a table into the editor, never runs it.
    ///
    /// Uses the columns on screen when this table is the open one, and asks
    /// the driver for them otherwise: a template with no column names is not
    /// worth generating.
    pub(crate) fn generate_statement(
        &mut self,
        index: usize,
        schema: &str,
        table: &str,
        kind: Generate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let known = self
            .open_table
            .as_ref()
            .filter(|open| open.connection == index && open.schema == schema && open.table == table)
            .and_then(|open| open.columns.clone());
        if let Some(columns) = known {
            self.insert_generated(index, schema, table, kind, &columns, window, cx);
            return;
        }
        let Some(key) = self.node_key(index) else {
            return;
        };
        let Some(NodeState::Connected(session)) =
            self.connections.get(index).map(|node| &node.state)
        else {
            return;
        };
        let (session, schema, table) = (session.clone(), schema.to_string(), table.to_string());
        self.keep_task(cx.spawn_in(window, async move |this, cx| {
            let described = session.describe(&schema, &table).await;
            this.update_in(cx, |this, window, cx| {
                // The connection the statement was asked for, wherever the list
                // has put it since: the place it had may be another's now, and a
                // `DELETE` written into the wrong connection's buffer is the
                // one mistake a template must not make.
                let Some(index) = this.index_of_key(&key) else {
                    return;
                };
                match described {
                    Ok(columns) => {
                        this.insert_generated(index, &schema, &table, kind, &columns, window, cx)
                    }
                    Err(error) => this.fail_generating(index, QueryError::from_anyhow(&error), cx),
                }
            })
            .ok();
        }));
    }

    fn insert_generated(
        &mut self,
        index: usize,
        schema: &str,
        table: &str,
        kind: Generate,
        columns: &[ColumnDef],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dialect) = self.dialect(index) else {
            return;
        };
        match generated_statement(&dialect, kind, schema, table, columns) {
            Some(sql) => self.insert_into_editor(index, sql, window, cx),
            None => self.fail_generating(
                index,
                QueryError {
                    message: format!("`{table}` has no columns to build a statement from").into(),
                    read_only: false,
                },
                cx,
            ),
        }
    }

    /// Says why nothing was written: a menu entry that does nothing at all
    /// reads as a click that missed.
    fn fail_generating(&mut self, index: usize, error: QueryError, cx: &mut Context<Self>) {
        if self.active == Some(index) {
            self.show_failure(error, cx);
        }
    }

    /// Puts `text` into the editor at the cursor, on the connection's own
    /// scratch buffer, and leaves the cursor there to keep typing.
    pub(crate) fn insert_into_editor(
        &mut self,
        index: usize,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_active(index, window, cx);
        self.sql_editor
            .update(cx, |editor, cx| editor.insert(&text, window, cx));
        cx.focus_view(&self.sql_editor, window);
    }

    /// Points the editor at this connection and focuses it.
    pub(crate) fn new_query(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_connected(index) {
            return;
        }
        self.set_active(index, window, cx);
        cx.focus_view(&self.sql_editor, window);
    }
}

fn table_entry(
    menu: ContextMenu,
    action: TableAction,
    panel: &WeakEntity<DatabasePanel>,
    index: usize,
    schema: &str,
    table: &str,
) -> ContextMenu {
    let (schema, table) = (schema.to_string(), table.to_string());
    panel_entry(
        menu,
        action.label(),
        action.icon(),
        panel,
        move |panel, window, cx| panel.run_table_action(action, index, &schema, &table, window, cx),
    )
}
