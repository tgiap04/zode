//! Running statements and paging their results.
//!
//! The page is what crosses the wire -- never a whole table. Every path here
//! goes through `Session::query` with a `limit` and an `offset`, including the
//! one that opens a table from the tree.

use crate::connection_store::DatabaseSettings;
use crate::database_panel::{ConnectionKey, DatabasePanel, NodeState};
use crate::grid_editing::PageEdits;
use crate::query::{Page, PageOrigin, QueryError, QueryState, TableBrowse};
use crate::query_history::{HistoryEntry, Outcome, now_ms};
use crate::row_serializers::{every_row, to_csv};
use crate::safe_mode::{StatementKind, classify};
use crate::session::Session;
use crate::statement_split::{SplitRules, statement_at};
use editor::HighlightKey;
use gpui::{Context, Window};
use multi_buffer::{MultiBufferOffset, ToOffset as _};
use settings::Settings as _;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long the statement that ran stays lit.
const FLASH_DURATION: Duration = Duration::from_millis(600);

/// What `RunQuery` and `RunAll` send when nothing is selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RunScope {
    /// The statement the cursor is in.
    Statement,
    /// The whole buffer.
    All,
}

/// The text to send and, when it is a statement cut out of the buffer, where
/// it lies -- which is what gets highlighted.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ChosenStatement {
    pub(crate) sql: String,
    pub(crate) range: Option<Range<usize>>,
}

/// Picks what to run from the buffer's `text` with the cursor at `cursor`.
///
/// A whole-buffer scope, or an engine with no `;` to split on (a document
/// store's buffer is one JSON document), sends everything. `None` when there
/// is nothing but blanks to send.
pub(crate) fn choose_statement(
    text: &str,
    cursor: usize,
    rules: Option<SplitRules>,
    scope: RunScope,
) -> Option<ChosenStatement> {
    if let (RunScope::Statement, Some(rules)) = (scope, rules) {
        let range = statement_at(text, cursor, rules)?;
        return Some(ChosenStatement {
            sql: text[range.clone()].to_string(),
            range: Some(range),
        });
    }
    (!text.trim().is_empty()).then(|| ChosenStatement {
        sql: text.to_string(),
        range: None,
    })
}

impl DatabasePanel {
    /// The session a query would run against, if there is one.
    fn active_session(&self) -> Option<(usize, Arc<Session>)> {
        let index = self.active?;
        match &self.connections.get(index)?.state {
            NodeState::Connected(session) => Some((index, session.clone())),
            _ => None,
        }
    }

    /// Remembers which connection queries go to, and swaps the scratch text for
    /// the one that connection was last left with.
    pub(crate) fn set_active(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.active == Some(index) {
            return;
        }
        self.stash_scratch(cx);
        self.active = Some(index);

        let text = self
            .connections
            .get(index)
            .and_then(|node| self.scratch.get(&node.config.name))
            .cloned()
            .unwrap_or_default();
        self.sql_editor.update(cx, |editor, cx| {
            editor.set_text(text, window, cx);
        });
        cx.notify();
    }

    /// Makes the connection `key` names the one queries go to, when it is still
    /// in the list and open. Otherwise says in the result area why nothing was
    /// sent, and leaves the active connection where it was.
    ///
    /// For what belongs to a connection that is not necessarily the active one
    /// by now -- a page turn, a statement chosen from a connection's history --
    /// and so must go to that connection or nowhere: the active one is another
    /// database, with its own level of safety.
    pub(crate) fn activate_open_connection(
        &mut self,
        key: &ConnectionKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let index = self.index_of_key(key);
        let open = index.filter(|index| {
            matches!(
                self.connections.get(*index).map(|node| &node.state),
                Some(NodeState::Connected(_))
            )
        });
        if let Some(index) = open {
            self.set_active(index, window, cx);
            return true;
        }
        let name = &key.0;
        let reason = if index.is_some() {
            format!("{name} is not connected, so nothing was sent. Connect to it and try again.")
        } else {
            format!("{name} is no longer in the connection list, so nothing was sent.")
        };
        self.show_failure(
            QueryError {
                message: reason.into(),
                read_only: false,
            },
            cx,
        );
        false
    }

    /// Files the editor's text under whichever connection is active.
    pub(crate) fn stash_scratch(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self
            .active
            .and_then(|index| self.connections.get(index))
            .map(|node| node.config.name.clone())
        else {
            return;
        };
        let text = self.sql_editor.read(cx).text(cx);
        self.scratch.insert(name, text);
    }

    /// What a run sends: the selection if there is one, otherwise the
    /// statement the cursor is in -- or the whole buffer for `RunAll`.
    ///
    /// Selection first because it is the one way to say exactly what is meant,
    /// including a block the splitter would cut in two.
    fn statement_to_run(&self, scope: RunScope, cx: &mut Context<Self>) -> Option<ChosenStatement> {
        let rules = self
            .active
            .and_then(|index| self.dialect(index))
            .and_then(|dialect| dialect.split_rules());
        let editor = self.sql_editor.read(cx);
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        // Anchors rather than offsets: `newest_anchor` is the one accessor that
        // does not need a `DisplaySnapshot`, which this panel never builds.
        let selection = editor.selections.newest_anchor();
        let start = selection.start.to_offset(&snapshot);
        let end = selection.end.to_offset(&snapshot);

        if start != end {
            let sql = snapshot.text_for_range(start..end).collect::<String>();
            return (!sql.trim().is_empty()).then_some(ChosenStatement { sql, range: None });
        }
        choose_statement(&editor.text(cx), start.0, rules, scope)
    }

    pub(crate) fn run_query(
        &mut self,
        scope: RunScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(chosen) = self.statement_to_run(scope, cx) else {
            return;
        };
        self.stash_scratch(cx);
        self.guard_unsaved(window, cx, move |this, window, cx| {
            this.run_gated(chosen, window, cx)
        });
    }

    /// Sends what the safety level of the active connection let through.
    pub(crate) fn send_statement(
        &mut self,
        chosen: ChosenStatement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(range) = chosen.range {
            self.flash_statement(range, cx);
        }
        let limit = DatabaseSettings::get_global(cx).page_size;
        self.start_query(chosen.sql, 0, limit, None, true, window, cx);
    }

    /// Lights the statement that was sent, so it is clear which one the cursor
    /// picked out of several. Cleared by the timer, or by the next edit.
    fn flash_statement(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.sql_editor.update(cx, |editor, cx| {
            let snapshot = editor.buffer().read(cx).snapshot(cx);
            let range = snapshot.anchor_before(MultiBufferOffset(range.start))
                ..snapshot.anchor_after(MultiBufferOffset(range.end));
            editor.highlight_background(
                HighlightKey::HighlightOnYank,
                &[range],
                |_, theme| theme.colors().editor_document_highlight_read_background,
                cx,
            );
        });
        // Replacing the task drops the previous timer, so a second run does not
        // get its highlight cleared by the first one's clock.
        self.flash_clear = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FLASH_DURATION).await;
            this.update(cx, |this, cx| this.clear_flash(cx)).ok();
        }));
    }

    pub(crate) fn clear_flash(&mut self, cx: &mut Context<Self>) {
        self.flash_clear = None;
        self.sql_editor.update(cx, |editor, cx| {
            editor.clear_background_highlights(HighlightKey::HighlightOnYank, cx);
        });
    }

    /// Opens a table's rows without anyone having to type the statement.
    ///
    /// The statement comes from the dialect, which quotes the names: this is
    /// where a name from the tree becomes SQL, and a table really can be
    /// called `"; drop table users; --`. The page is marked as a browse of this
    /// table, which is what lets it be sorted and filtered later.
    pub(crate) fn open_table_data(
        &mut self,
        index: usize,
        schema: &str,
        table: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(connection) = self.node_key(index) else {
            return;
        };
        if self.has_unsaved_edits(cx) {
            let (schema, table) = (schema.to_string(), table.to_string());
            self.guard_unsaved(window, cx, move |this, window, cx| {
                this.open_table_data(index, &schema, &table, window, cx)
            });
            return;
        }
        self.browse_bar.reset();
        self.set_notice(index, None);
        let browse = TableBrowse::new(connection, schema, table);

        // The tree already describes the table it has open, and asking again
        // would delay a click the answer to which is sitting right there.
        let known = self
            .open_table
            .as_ref()
            .filter(|open| open.connection == index && open.schema == schema && open.table == table)
            .and_then(|open| open.columns.as_deref());
        if let Some(columns) = known {
            let browse = browse.with_columns(columns);
            self.run_browse(browse, window, cx);
            return;
        }
        let Some(NodeState::Connected(session)) =
            self.connections.get(index).map(|node| &node.state)
        else {
            self.run_browse(browse, window, cx);
            return;
        };

        // The key is what makes the pages stable and the rows nameable, so it
        // is known before the first page is asked for.
        let session = session.clone();
        let load = self.next_load();
        self.pending_open = load;
        self.keep_task(cx.spawn_in(window, async move |this, cx| {
            let described = session.describe(&browse.schema, &browse.table).await;
            this.update_in(cx, |this, window, cx| {
                // The tree's own table gets the answer whether or not the page
                // is still wanted: it asked for these columns too.
                this.offer_columns(&browse, &described, cx);
                // Another table was opened, or something else was run, while
                // this one was being described: that is the newer word.
                if this.pending_open != load {
                    return;
                }
                this.pending_open = 0;
                // Without the key the table is still worth reading: an account
                // that may select from a table is not always allowed to ask
                // what its columns are.
                let browse = match described {
                    Ok(columns) => browse.with_columns(&columns),
                    Err(error) => {
                        log::warn!(
                            "could not describe `{}`.`{}`, reading it without its key: {error:#}",
                            browse.schema,
                            browse.table
                        );
                        browse
                    }
                };
                this.run_browse(browse, window, cx);
            })
            .ok();
        }));
    }

    /// Shows how many rows a table has, in the grid like any other result.
    pub(crate) fn count_rows(
        &mut self,
        index: usize,
        schema: &str,
        table: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_active(index, window, cx);
        let Some(sql) = self
            .dialect(index)
            .and_then(|dialect| dialect.count(schema, table, None))
        else {
            return;
        };
        let limit = DatabaseSettings::get_global(cx).page_size;
        self.run_page(sql, 0, limit, None, window, cx);
    }

    pub(crate) fn page(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let QueryState::Done(page) = &self.query else {
            return;
        };
        if !page.can_turn() {
            return;
        }
        // A page is read from the connection that gave it, not from the one
        // clicked since: the statement is not meant for another database.
        let Some(connection) = page.connection.clone() else {
            return;
        };
        let limit = page.limit;
        let offset = if forward {
            page.offset.saturating_add(limit as u64)
        } else {
            page.offset.saturating_sub(limit as u64)
        };
        let sql = page.sql.clone();
        let browse = page.browse.clone();
        if self.activate_open_connection(&connection, window, cx) {
            self.run_page(sql, offset, limit, browse, window, cx);
        }
    }

    /// Runs one page of a statement that was not typed just now -- a page
    /// turn, a table opened from the tree, a sort -- and so is not history.
    pub(crate) fn run_page(
        &mut self,
        sql: String,
        offset: u64,
        limit: u32,
        browse: Option<TableBrowse>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_query(sql, offset, limit, browse, false, window, cx);
    }

    /// `record` puts the statement in the connection's history once it is
    /// answered, whether it succeeded or not.
    pub(crate) fn start_query(
        &mut self,
        sql: String,
        offset: u64,
        limit: u32,
        browse: Option<TableBrowse>,
        record: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The one place a page is replaced, so the one place unsaved edits can
        // be lost: whatever got here -- a page turn, a sort, a statement from
        // the history -- is held until the user has said what becomes of them.
        if self.has_unsaved_edits(cx) {
            self.guard_unsaved(window, cx, move |this, window, cx| {
                this.start_query(sql, offset, limit, browse, record, window, cx)
            });
            return;
        }
        let Some((index, session)) = self.active_session() else {
            self.query = QueryState::Failed(QueryError {
                message: "Open a connection first".into(),
                read_only: false,
            });
            cx.notify();
            return;
        };

        self.pending_open = 0;
        self.browse_bar.note_run(browse.as_ref());

        // Decided here, from the statement being sent: a page turn sends it
        // again, and what changes data is shown once. An engine the dialect
        // cannot be built for is taken for a write.
        let kind = self
            .dialect(index)
            .map_or(StatementKind::Write, |dialect| classify(&dialect, &sql));
        let connection = self.node_key(index);
        let session_id = session.id;

        // Counted rather than random: two queries in the same millisecond would
        // otherwise share an id, and `cancel` would stop the wrong one.
        let read = self.next_request_id();
        let request_id = format!("q{read}");
        self.query = QueryState::Running {
            request_id: request_id.clone(),
            cancelling: false,
        };
        cx.notify();

        let history_connection = record
            .then(|| self.connections.get(index))
            .flatten()
            .map(|node| node.config.name.clone());
        let started_at_ms = now_ms();
        let started = Instant::now();

        self.keep_task(cx.spawn(async move |this, cx| {
            let result = session
                .query(sql.clone(), limit, offset, request_id.clone())
                .await;
            let elapsed_ms = started.elapsed().as_millis() as u64;
            this.update(cx, |this, cx| {
                // Recorded before the staleness check below: the statement ran
                // and the server answered, whether or not the user is still
                // looking at that grid.
                if let Some(connection) = history_connection {
                    let entry = match &result {
                        Ok(result) => HistoryEntry {
                            sql: sql.clone(),
                            started_at_ms,
                            elapsed_ms,
                            outcome: Outcome::Rows {
                                count: result.rows.len() as u64,
                            },
                        },
                        Err(error) => HistoryEntry::failed(
                            sql.clone(),
                            started_at_ms,
                            elapsed_ms,
                            &format!("{error:#}"),
                        ),
                    };
                    this.record_history(connection, entry, cx);
                }
                // A result that arrives after the user moved on belongs to a
                // query nobody is waiting for any more.
                if !matches!(&this.query, QueryState::Running { request_id: running, .. }
                    if running == &request_id)
                {
                    return;
                }
                let saved_note = this.saved_note.take();
                this.query = match result {
                    Ok(result) => {
                        if browse.is_some() {
                            this.browse_bar.note_columns(&result);
                        }
                        QueryState::Done(Page {
                            result,
                            sql,
                            offset,
                            limit,
                            selected_row: None,
                            selection: Default::default(),
                            browse,
                            connection,
                            origin: PageOrigin {
                                session: Some(session_id),
                                read,
                            },
                            kind,
                            edits: PageEdits::new(saved_note),
                        })
                    }
                    Err(error) => QueryState::Failed(QueryError::from_anyhow(&error)),
                };
                cx.notify();
            })
            .ok();
        }));
    }

    pub(crate) fn next_request_id(&mut self) -> u64 {
        self.request_counter += 1;
        self.request_counter
    }

    /// Asks the driver to stop, and says so while it is being asked.
    ///
    /// The state does not jump straight to idle: a driver whose engine cannot
    /// interrupt will finish the query anyway, and pretending otherwise would
    /// leave the grid showing rows from a query the user believes was stopped.
    pub(crate) fn cancel_query(&mut self, cx: &mut Context<Self>) {
        let QueryState::Running {
            request_id,
            cancelling,
        } = &mut self.query
        else {
            return;
        };
        if *cancelling {
            return;
        }
        *cancelling = true;
        let request_id = request_id.clone();
        cx.notify();

        let Some((_, session)) = self.active_session() else {
            return;
        };
        self.keep_task(cx.spawn(async move |_this, _cx| {
            session.cancel(request_id).await.ok();
        }));
    }

    /// The page on screen, as CSV on the clipboard.
    ///
    /// The clipboard rather than a file: a file needs a path prompt, and the
    /// overwhelmingly common next step is pasting into a spreadsheet anyway.
    pub(crate) fn copy_page_as_csv(&mut self, cx: &mut Context<Self>) {
        let QueryState::Done(page) = &self.query else {
            return;
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(to_csv(
            &page.result,
            &every_row(&page.result),
        )));
    }
}
