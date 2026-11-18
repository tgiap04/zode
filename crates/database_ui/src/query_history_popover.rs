//! The history list over the SQL buffer: what each connection ran, searchable,
//! and the glue that keeps it recorded and stored.

use crate::database_panel::{ConnectionKey, DatabasePanel};
use crate::panel_queries::ChosenStatement;
use crate::query_history::{
    Change, HistoryEntry, MAX_ENTRIES, MAX_SQL_BYTES, Outcome, QueryHistory, StoreTurns,
    read_stored,
};
use db::kvp::KeyValueStore;
use gpui::{
    AnyElement, App, Context, DismissEvent, Entity, SharedString, Subscription, Task, WeakEntity,
    Window,
};
use picker::{Picker, PickerDelegate};
use std::sync::Arc;
use ui::{ListItem, ListItemSpacing, prelude::*};
use util::ResultExt as _;

/// One connection's history as the panel holds it.
pub(crate) enum HistoryState {
    /// Being read from the store. Whatever runs meanwhile waits here, and is
    /// added to the list -- and to the store -- when it arrives.
    Loading {
        newer: Vec<HistoryEntry>,
    },
    Ready(QueryHistory),
}

/// The open list, and the subscription that closes it.
pub(crate) type HistoryPopover = (Entity<Picker<HistoryDelegate>>, Subscription);

impl DatabasePanel {
    /// Remembers a statement that ran, for the connection it ran on.
    pub(crate) fn record_history(
        &mut self,
        connection: String,
        entry: HistoryEntry,
        cx: &mut Context<Self>,
    ) {
        if entry.sql.len() > MAX_SQL_BYTES {
            return;
        }
        match self.history.get_mut(&connection) {
            Some(HistoryState::Ready(history)) => {
                history.record(entry.clone());
                self.write_history(connection, Change::Append(vec![entry]), cx);
            }
            Some(HistoryState::Loading { newer }) => {
                if newer.len() >= MAX_ENTRIES {
                    newer.remove(0);
                }
                newer.push(entry);
            }
            None => {
                self.history.insert(
                    connection.clone(),
                    HistoryState::Loading { newer: vec![entry] },
                );
                self.load_history(connection, cx);
            }
        }
        cx.notify();
    }

    fn ensure_history_loaded(&mut self, connection: &str, cx: &mut Context<Self>) {
        if self.history.contains_key(connection) {
            return;
        }
        self.history.insert(
            connection.to_string(),
            HistoryState::Loading { newer: Vec::new() },
        );
        self.load_history(connection.to_string(), cx);
    }

    fn load_history(&mut self, connection: String, cx: &mut Context<Self>) {
        let store = KeyValueStore::global(cx);
        self.keep_task(cx.spawn(async move |this, cx| {
            let name = connection.clone();
            // A list that cannot be read is shown as empty for this session, and
            // stays as it is in the store: see `Change::apply`.
            let stored = cx
                .background_spawn(async move { read_stored(&store, &name) })
                .await
                .log_err()
                .unwrap_or_default();
            this.update(cx, |this, cx| {
                let newer = match this.history.remove(&connection) {
                    Some(HistoryState::Loading { newer }) => newer,
                    // Cleared while it was being read: that is the newer word.
                    Some(ready @ HistoryState::Ready(_)) => {
                        this.history.insert(connection, ready);
                        return;
                    }
                    None => Vec::new(),
                };
                let ran_meanwhile = newer.clone();
                this.history.insert(
                    connection.clone(),
                    HistoryState::Ready(QueryHistory::merged(stored, newer)),
                );
                if !ran_meanwhile.is_empty() {
                    this.write_history(connection.clone(), Change::Append(ran_meanwhile), cx);
                }
                this.show_loaded_history(&connection, cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// Hands the list that just arrived to the open popover, which was built
    /// while it was still being read.
    fn show_loaded_history(&self, connection: &str, cx: &mut Context<Self>) {
        let (Some(HistoryState::Ready(history)), Some((picker, _))) =
            (self.history.get(connection), &self.history_popover)
        else {
            return;
        };
        let entries = history.newest_first();
        picker.update(cx, |picker, cx| {
            if picker.delegate.connection.0 == connection {
                let query = picker.query(cx);
                picker.delegate.replace_entries(entries, &query);
                cx.notify();
            }
        });
    }

    /// Writes a change to a connection's stored list.
    ///
    /// The turn is taken where the change is made, in the order the changes
    /// were, and the work after it -- reading the list, serialising it,
    /// writing it -- is done off this thread: a list of the longest kept
    /// statements is megabytes of text. Detached, so that a write still under
    /// way outlives a panel closed straight after a run.
    fn write_history(&mut self, connection: String, change: Change, cx: &mut Context<Self>) {
        let store = KeyValueStore::global(cx);
        let turns = StoreTurns::of(cx);
        cx.spawn(async move |_this, cx| {
            let turn = turns.take().await;
            let written = cx
                .background_spawn(async move { change.apply(store, connection).await })
                .await;
            drop(turn);
            written
        })
        .detach_and_log_err(cx);
    }

    /// Forgets everything a connection ran.
    pub(crate) fn clear_history(&mut self, connection: String, cx: &mut Context<Self>) {
        self.history.insert(
            connection.clone(),
            HistoryState::Ready(QueryHistory::default()),
        );
        self.write_history(connection, Change::Clear, cx);
        cx.notify();
    }

    /// Opens the history list, or closes it if it is open.
    pub(crate) fn show_query_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history_popover.take().is_some() {
            cx.focus_self(window);
            cx.notify();
            return;
        }
        let Some(key) = self.active.and_then(|index| self.node_key(index)) else {
            return;
        };
        let connection = key.0.clone();
        self.ensure_history_loaded(&connection, cx);
        // Still being read: what ran meanwhile is shown, and the rest is handed
        // to the list when it arrives (`show_loaded_history`).
        let (entries, loading) = match self.history.get(&connection) {
            Some(HistoryState::Ready(history)) => (history.newest_first(), false),
            Some(HistoryState::Loading { newer }) => (newer.iter().rev().cloned().collect(), true),
            None => (Vec::new(), false),
        };

        let delegate = HistoryDelegate::new(cx.weak_entity(), key, entries, loading);
        let picker = cx.new(|cx| {
            Picker::uniform_list(delegate, window, cx)
                .width(rems(36.))
                .max_height(Some(rems(22.).into()))
        });
        let subscription =
            cx.subscribe_in(&picker, window, |this, _, _: &DismissEvent, window, cx| {
                this.history_popover = None;
                cx.focus_self(window);
                cx.notify();
            });
        cx.focus_view(&picker, window);
        self.history_popover = Some((picker, subscription));
        cx.notify();
    }

    /// What choosing an entry does: put it in the buffer, or with `run` run it
    /// there and then without touching the buffer.
    ///
    /// A run is a statement typed like any other, and goes through the same
    /// gate -- an entry a read-only connection refused is in its history -- on
    /// the connection that ran it, whichever is active by now.
    pub(crate) fn use_history_entry(
        &mut self,
        connection: &ConnectionKey,
        sql: String,
        run: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if run {
            self.stash_scratch(cx);
            if self.activate_open_connection(connection, window, cx) {
                self.run_gated(ChosenStatement { sql, range: None }, window, cx);
            }
            return;
        }
        let current = self.sql_editor.read(cx).text(cx);
        // Appended after what is there, apart from it: inserted at the cursor
        // it could land inside a statement and make one out of two.
        let text = if current.trim().is_empty() {
            sql
        } else {
            format!("{}\n\n{sql}", current.trim_end())
        };
        self.sql_editor.update(cx, |editor, cx| {
            editor.set_text(text, window, cx);
        });
        cx.focus_view(&self.sql_editor, window);
    }

    /// The list, drawn over the top right of the panel where its button is.
    pub(crate) fn render_history_popover(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (picker, _) = self.history_popover.as_ref()?;
        Some(
            gpui::deferred(
                div()
                    .absolute()
                    .top(ui::Tab::container_height(cx))
                    .right_2()
                    .occlude()
                    .elevation_3(cx)
                    .child(picker.clone()),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

pub(crate) struct HistoryDelegate {
    panel: WeakEntity<DatabasePanel>,
    /// Whose history this is: a statement chosen from it runs there.
    connection: ConnectionKey,
    /// Newest first.
    entries: Vec<HistoryEntry>,
    /// Indices into `entries` of what the search lets through.
    matches: Vec<usize>,
    selected: usize,
    /// The stored list has not arrived: an empty list is not yet "nothing ran".
    loading: bool,
}

impl HistoryDelegate {
    fn new(
        panel: WeakEntity<DatabasePanel>,
        connection: ConnectionKey,
        entries: Vec<HistoryEntry>,
        loading: bool,
    ) -> Self {
        let matches = (0..entries.len()).collect();
        Self {
            panel,
            connection,
            entries,
            matches,
            selected: 0,
            loading,
        }
    }

    fn replace_entries(&mut self, entries: Vec<HistoryEntry>, query: &str) {
        self.entries = entries;
        self.loading = false;
        self.filter(query);
    }

    fn filter(&mut self, query: &str) {
        self.matches = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.matches(query))
            .map(|(index, _)| index)
            .collect();
        self.selected = 0;
    }
}

impl PickerDelegate for HistoryDelegate {
    type ListItem = ListItem;

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        "Search what was run…".into()
    }

    fn no_matches_text(&self, _window: &mut Window, _cx: &mut App) -> Option<SharedString> {
        Some(
            if self.loading {
                "Loading what was run…"
            } else if self.entries.is_empty() {
                "Nothing has been run on this connection yet"
            } else {
                "No statement matches"
            }
            .into(),
        )
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected
    }

    fn set_selected_index(
        &mut self,
        ix: usize,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) {
        self.selected = ix;
    }

    fn update_matches(
        &mut self,
        query: String,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Task<()> {
        self.filter(&query);
        Task::ready(())
    }

    /// Enter inserts the statement; the secondary key (and a secondary click)
    /// runs it.
    fn confirm(&mut self, secondary: bool, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        let Some(entry) = self
            .matches
            .get(self.selected)
            .and_then(|index| self.entries.get(*index))
        else {
            return;
        };
        let sql = entry.sql.clone();
        let connection = self.connection.clone();
        self.panel
            .update(cx, |panel, cx| {
                panel.use_history_entry(&connection, sql, secondary, window, cx)
            })
            .ok();
        cx.emit(DismissEvent);
    }

    fn dismissed(&mut self, _window: &mut Window, _cx: &mut Context<Picker<Self>>) {}

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        let entry = self.entries.get(*self.matches.get(ix)?)?;
        let (summary, colour) = match &entry.outcome {
            Outcome::Rows { count } => (
                format!(
                    "{count} {} · {} ms",
                    if *count == 1 { "row" } else { "rows" },
                    entry.elapsed_ms
                ),
                Color::Muted,
            ),
            Outcome::Failed { .. } => (format!("failed · {} ms", entry.elapsed_ms), Color::Error),
        };
        Some(
            ListItem::new(ix)
                .inset(true)
                .spacing(ListItemSpacing::Sparse)
                .toggle_state(selected)
                .child(
                    v_flex()
                        .min_w_0()
                        .child(
                            Label::new(first_line(&entry.sql))
                                .size(LabelSize::Small)
                                .truncate(),
                        )
                        .child(Label::new(summary).size(LabelSize::XSmall).color(colour)),
                )
                .end_slot(
                    Label::new(age_label(
                        crate::query_history::now_ms(),
                        entry.started_at_ms,
                    ))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
                ),
        )
    }

    fn render_footer(
        &self,
        _window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Option<AnyElement> {
        let panel = self.panel.clone();
        let connection = self.connection.0.clone();
        Some(
            h_flex()
                .p_1p5()
                .gap_2()
                .justify_between()
                .border_t_1()
                .border_color(cx.theme().colors().border_variant)
                .child(
                    Label::new(
                        "Kept on this machine only. Enter inserts; with the secondary key it runs.",
                    )
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
                )
                .child(
                    Button::new("database-history-clear", "Clear History")
                        .label_size(LabelSize::Small)
                        .on_click(cx.listener(move |picker, _event, _window, cx| {
                            panel
                                .update(cx, |panel, cx| panel.clear_history(connection.clone(), cx))
                                .ok();
                            picker.delegate.entries.clear();
                            picker.delegate.matches.clear();
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }
}

/// The statement's first line that says anything, so a statement that opens
/// with a comment or a blank is still recognisable in a one-line row.
fn first_line(sql: &str) -> SharedString {
    sql.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_string()
        .into()
}

/// "5 min ago", from two timestamps in milliseconds.
fn age_label(now_ms: i64, then_ms: i64) -> String {
    let seconds = (now_ms - then_ms).max(0) / 1000;
    match seconds {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", seconds / 60),
        3600..=86_399 => format!("{} h ago", seconds / 3600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_read_in_the_largest_unit_that_fits() {
        assert_eq!(age_label(10_000, 9_000), "just now");
        assert_eq!(age_label(300_000, 0), "5 min ago");
        assert_eq!(age_label(2 * 3_600_000, 0), "2 h ago");
        assert_eq!(age_label(3 * 86_400_000, 0), "3 d ago");
    }

    #[test]
    fn a_time_in_the_future_is_just_now_rather_than_negative() {
        assert_eq!(age_label(0, 5_000_000), "just now");
    }

    #[test]
    fn the_row_shows_the_first_line_with_something_on_it() {
        assert_eq!(first_line("\n  \n  select 1\nfrom t"), "select 1");
        assert_eq!(first_line(""), "");
    }
}
