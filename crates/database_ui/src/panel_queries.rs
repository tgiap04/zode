//! Running statements and paging their results.
//!
//! The page is what crosses the wire -- never a whole table. Every path here
//! goes through `Session::query` with a `limit` and an `offset`, including the
//! one that opens a table from the tree.

use crate::connection_store::DatabaseSettings;
use crate::database_panel::{DatabasePanel, NodeState};
use crate::query::{Page, QueryError, QueryState, to_csv};
use crate::session::Session;
use gpui::{Context, Window};
use multi_buffer::ToOffset as _;
use settings::Settings as _;
use std::sync::Arc;

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

    /// Files the editor's text under whichever connection is active.
    fn stash_scratch(&mut self, cx: &mut Context<Self>) {
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

    /// What `cmd-enter` runs: the selection if there is one, otherwise the whole
    /// buffer.
    ///
    /// Selection-first because a scratch buffer collects several statements, and
    /// running all of them because the cursor happened to be in one is not what
    /// anybody means.
    fn statement_to_run(&self, cx: &mut Context<Self>) -> Option<String> {
        let editor = self.sql_editor.read(cx);
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        // Anchors rather than offsets: `newest_anchor` is the one accessor that
        // does not need a `DisplaySnapshot`, which this panel never builds.
        let selection = editor.selections.newest_anchor();
        let start = selection.start.to_offset(&snapshot);
        let end = selection.end.to_offset(&snapshot);

        let sql = if start == end {
            editor.text(cx)
        } else {
            snapshot.text_for_range(start..end).collect::<String>()
        };
        (!sql.trim().is_empty()).then_some(sql)
    }

    pub(crate) fn run_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sql) = self.statement_to_run(cx) else {
            return;
        };
        self.stash_scratch(cx);
        let limit = DatabaseSettings::get_global(cx).page_size;
        self.run_page(sql, 0, limit, window, cx);
    }

    /// Opens a table's rows without anyone having to type the statement.
    ///
    /// The identifier is quoted here rather than in the driver because this is
    /// where a name from the tree becomes SQL -- and a table really can be
    /// called `"; drop table users; --`.
    pub(crate) fn preview_table(
        &mut self,
        index: usize,
        schema: &str,
        table: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_active(index, window, cx);
        // Quoted by the driver's own rule, not by a rule this crate picked:
        // engines disagree about the quote character, and getting it wrong
        // turns every click on a table into a syntax error.
        let Some((_, session)) = self.active_session() else {
            return;
        };
        let quote = &session.capabilities;
        let sql = format!(
            "SELECT * FROM {}.{}",
            quote.quote_identifier(schema),
            quote.quote_identifier(table)
        );
        let limit = DatabaseSettings::get_global(cx).page_size;
        self.run_page(sql, 0, limit, window, cx);
    }

    pub(crate) fn page(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let QueryState::Done(page) = &self.query else {
            return;
        };
        let limit = page.limit;
        let offset = if forward {
            page.offset.saturating_add(limit as u64)
        } else {
            page.offset.saturating_sub(limit as u64)
        };
        let sql = page.sql.clone();
        self.run_page(sql, offset, limit, window, cx);
    }

    fn run_page(
        &mut self,
        sql: String,
        offset: u64,
        limit: u32,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((index, session)) = self.active_session() else {
            self.query = QueryState::Failed(QueryError {
                message: "Open a connection first".into(),
                read_only: false,
            });
            cx.notify();
            return;
        };

        // Counted rather than random: two queries in the same millisecond would
        // otherwise share an id, and `cancel` would stop the wrong one.
        let request_id = format!("q{}", self.next_request_id());
        self.query = QueryState::Running {
            request_id: request_id.clone(),
            cancelling: false,
        };
        cx.notify();

        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = session
                .query(sql.clone(), limit, offset, request_id.clone())
                .await;
            this.update(cx, |this, cx| {
                // A result that arrives after the user moved on belongs to a
                // query nobody is waiting for any more.
                if !matches!(&this.query, QueryState::Running { request_id: running, .. }
                    if running == &request_id)
                {
                    return;
                }
                this.query = match result {
                    Ok(result) => QueryState::Done(Page {
                        result,
                        sql,
                        offset,
                        limit,
                    }),
                    Err(error) => QueryState::Failed(QueryError::from_anyhow(&error)),
                };
                let _ = index;
                cx.notify();
            })
            .ok();
        }));
    }

    fn next_request_id(&mut self) -> u64 {
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
        self.tasks.push(cx.spawn(async move |_this, _cx| {
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
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(to_csv(&page.result)));
    }
}
