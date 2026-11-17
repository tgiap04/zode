//! Reading the tree again: a connection's schemas, one schema's tables, or the
//! columns of the open table.
//!
//! A refresh keeps what the user arranged -- which schemas are open, which
//! table is showing -- and replaces what the database told it. The old rows
//! stay on screen until the new ones land, so a refresh never blanks the tree.

use crate::database_panel::DatabasePanel;
use gpui::Context;

/// Whether a schema is open after its list is read, again or for the first
/// time.
///
/// A schema the user had open stays open and one they closed stays closed; a
/// schema that is new to the tree opens when it is the default. A lone schema
/// has no node to close, so it is always open.
pub(crate) fn schema_expanded(
    previous: Option<bool>,
    is_default: bool,
    collapsed_level: bool,
) -> bool {
    collapsed_level || previous.unwrap_or(is_default)
}

impl DatabasePanel {
    /// Names a driver round trip, so an answer can tell whether it is still
    /// the one being waited for.
    ///
    /// Counted across the whole panel rather than per node: nodes are replaced
    /// by a refresh, and a fresh node counting from the same start as the one
    /// it replaced would accept the replaced node's late answer. `0` is left
    /// for "waiting for nothing".
    pub(crate) fn next_load(&mut self) -> u64 {
        self.load_counter = self.load_counter.wrapping_add(1).max(1);
        self.load_counter
    }

    /// Sets or clears the line a connection shows under itself about what just
    /// failed. See `DatabasePanel::notices`.
    pub(crate) fn set_notice(&mut self, index: usize, notice: Option<String>) {
        let Some(key) = self.node_key(index) else {
            return;
        };
        match notice {
            Some(text) => {
                self.notices.insert(key, text.into());
            }
            None => {
                self.notices.remove(&key);
            }
        }
    }

    /// What the header button and the `RefreshDatabase` action do.
    pub(crate) fn refresh_active(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.active {
            self.refresh_connection(index, cx);
        }
    }

    /// Reads a connection's schemas again, then the tables of every schema
    /// that is open and the columns of the open table.
    ///
    /// Only an open connection is refreshed: a closed one has nothing cached,
    /// and refreshing is not a way to open it.
    pub(crate) fn refresh_connection(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.is_connected(index) {
            return;
        }
        self.set_notice(index, None);
        // The tables of the open schemas follow from `read_schemas`, once the
        // new list says which schemas still exist.
        self.read_schemas(index, true, cx);
        if self
            .open_table
            .as_ref()
            .is_some_and(|open| open.connection == index)
        {
            self.load_columns(true, cx);
        }
    }

    /// Reads one schema's tables again, and the open table's columns when the
    /// open table is one of them.
    ///
    /// A schema nobody opened has nothing on screen to read: it forgets its
    /// tables, and the next expand reads them.
    pub(crate) fn refresh_schema(&mut self, index: usize, schema: &str, cx: &mut Context<Self>) {
        if !self.is_connected(index) {
            return;
        }
        let Some(schema_index) = self
            .connections
            .get(index)
            .and_then(|node| node.schemas.iter().position(|s| s.schema.name == schema))
        else {
            return;
        };
        self.set_notice(index, None);
        let Some(node) = self.connections.get_mut(index) else {
            return;
        };
        let node_schema = &mut node.schemas[schema_index];
        if node_schema.expanded {
            self.load_tables(index, schema_index, cx);
        } else {
            node_schema.tables = None;
            // Names no load, so an answer still out for it is dropped rather
            // than filling in what was just forgotten.
            node_schema.tables_load = 0;
            cx.notify();
        }
        if self
            .open_table
            .as_ref()
            .is_some_and(|open| open.connection == index && open.schema == schema)
        {
            self.load_columns(true, cx);
        }
    }

    /// Reads the columns of the open table again. Any other table has no
    /// columns on screen, so there is nothing to read.
    pub(crate) fn refresh_table(
        &mut self,
        index: usize,
        schema: &str,
        table: &str,
        cx: &mut Context<Self>,
    ) {
        let is_open = self.open_table.as_ref().is_some_and(|open| {
            open.connection == index && open.schema == schema && open.table == table
        });
        if is_open {
            self.load_columns(true, cx);
        }
    }
}
