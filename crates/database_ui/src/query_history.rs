//! What was run, kept so it can be found again.
//!
//! One list per connection, newest last, capped: a list nobody prunes grows
//! until it is the slowest thing in the app database. Stored through the app's
//! key-value store, which is local to this machine -- a statement may carry a
//! literal somebody would not want synced, so nothing here ever leaves it.

use anyhow::{Context as _, Result};
use db::kvp::KeyValueStore;
use gpui::{App, Global};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;

/// How many statements a connection remembers. The oldest go first.
pub(crate) const MAX_ENTRIES: usize = 1000;
/// A statement longer than this is not remembered. It is nearly always a
/// pasted dump, which is both useless to search for and the one way a thousand
/// entries could add up to something large.
pub(crate) const MAX_SQL_BYTES: usize = 64 * 1024;
/// What one connection's list may add up to, whatever the entry count says: a
/// thousand statements of the longest kept length are 64 MB, which is
/// serialised and written with every run.
pub(crate) const MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_ERROR_CHARS: usize = 500;

const NAMESPACE: &str = "database-query-history";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct HistoryEntry {
    pub(crate) sql: String,
    /// Milliseconds since the Unix epoch.
    pub(crate) started_at_ms: i64,
    pub(crate) elapsed_ms: u64,
    pub(crate) outcome: Outcome,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Outcome {
    /// Rows on the first page it answered with.
    Rows {
        count: u64,
    },
    Failed {
        message: String,
    },
}

impl HistoryEntry {
    pub(crate) fn failed(
        sql: String,
        started_at_ms: i64,
        elapsed_ms: u64,
        message: &str,
    ) -> HistoryEntry {
        HistoryEntry {
            sql,
            started_at_ms,
            elapsed_ms,
            outcome: Outcome::Failed {
                message: message.chars().take(MAX_ERROR_CHARS).collect(),
            },
        }
    }

    /// What the entry weighs against [`MAX_TOTAL_BYTES`].
    fn bytes(&self) -> usize {
        self.sql.len()
            + match &self.outcome {
                Outcome::Rows { .. } => 0,
                Outcome::Failed { message } => message.len(),
            }
    }

    /// Whether the statement contains `query`, ignoring case. Substring rather
    /// than fuzzy: what is being looked for is a table or a column name that is
    /// known to be in there.
    pub(crate) fn matches(&self, query: &str) -> bool {
        query.is_empty() || self.sql.to_lowercase().contains(&query.to_lowercase())
    }
}

/// One connection's statements, oldest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct QueryHistory {
    entries: VecDeque<HistoryEntry>,
    /// What `entries` add up to, kept as they change rather than summed again
    /// by every run.
    bytes: usize,
}

impl QueryHistory {
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Adds a statement, dropping the oldest ones past [`MAX_ENTRIES`] or
    /// [`MAX_TOTAL_BYTES`]. `false` when the statement is too long to be kept.
    pub(crate) fn record(&mut self, entry: HistoryEntry) -> bool {
        if entry.sql.len() > MAX_SQL_BYTES {
            return false;
        }
        self.bytes += entry.bytes();
        self.entries.push_back(entry);
        self.trim();
        true
    }

    fn trim(&mut self) {
        while self.entries.len() > MAX_ENTRIES || self.bytes > MAX_TOTAL_BYTES {
            let Some(oldest) = self.entries.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(oldest.bytes());
        }
    }

    /// What was stored followed by what was run while it was being read.
    pub(crate) fn merged(stored: QueryHistory, newer: Vec<HistoryEntry>) -> QueryHistory {
        let mut history = stored;
        for entry in newer {
            history.record(entry);
        }
        history
    }

    pub(crate) fn newest_first(&self) -> Vec<HistoryEntry> {
        self.entries.iter().rev().cloned().collect()
    }

    pub(crate) fn to_json(&self) -> Result<String> {
        serde_json::to_string(&self.entries).context("serialising the query history")
    }

    /// Reads a stored list. One written by a longer-lived build is trimmed to
    /// the caps rather than rejected.
    pub(crate) fn from_json(json: &str) -> Result<QueryHistory> {
        let entries: VecDeque<HistoryEntry> =
            serde_json::from_str(json).context("reading the stored query history")?;
        let mut history = QueryHistory {
            bytes: entries.iter().map(HistoryEntry::bytes).sum(),
            entries,
        };
        history.trim();
        Ok(history)
    }
}

/// Milliseconds since the epoch, or zero if the clock is set before it.
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

/// A connection's stored history, empty when it has none. Blocks on the
/// database, so it belongs on a background thread.
pub(crate) fn read_stored(store: &KeyValueStore, connection: &str) -> Result<QueryHistory> {
    match store.scoped(NAMESPACE).read(connection)? {
        Some(json) => QueryHistory::from_json(&json),
        None => Ok(QueryHistory::default()),
    }
}

pub(crate) async fn write_stored(
    store: KeyValueStore,
    connection: String,
    history_json: String,
) -> Result<()> {
    store
        .scoped(NAMESPACE)
        .write(connection, history_json)
        .await
}

/// What a write does to the stored list.
pub(crate) enum Change {
    /// Entries to add after whatever is stored.
    Append(Vec<HistoryEntry>),
    /// Forget everything: the one change that replaces what is stored.
    Clear,
}

impl Change {
    /// Applies the change to what is stored *now*. Two windows share one list
    /// per connection, and a window that wrote down the list as it remembered it
    /// would erase what the other had added since it last read.
    ///
    /// A list that cannot be read is left as it is: it is not an empty one, and
    /// writing the new entries over it would turn a read error into the loss of
    /// everything stored. Blocks on the database, so it belongs on a background
    /// thread, and under a turn from [`StoreTurns`].
    pub(crate) async fn apply(self, store: KeyValueStore, connection: String) -> Result<()> {
        let history = match self {
            Change::Clear => QueryHistory::default(),
            Change::Append(entries) => {
                let mut history = read_stored(&store, &connection)?;
                for entry in entries {
                    history.record(entry);
                }
                history
            }
        };
        write_stored(store, connection, history.to_json()?).await
    }
}

/// One change to the store at a time, in the order they asked: a change reads
/// the list and writes it back, and two that overlapped would each write over
/// the other. Held by the app so that every window takes its turn in the same
/// line.
#[derive(Clone, Default)]
pub(crate) struct StoreTurns(Arc<db::smol::lock::Mutex<()>>);

impl Global for StoreTurns {}

impl StoreTurns {
    pub(crate) fn of(cx: &mut App) -> Self {
        cx.default_global::<Self>().clone()
    }

    /// First polled where the change was made, which is what keeps the order: a
    /// clear followed by a run is not undone by the run reading the list the
    /// clear has not yet emptied.
    pub(crate) async fn take(&self) -> db::smol::lock::MutexGuard<'_, ()> {
        self.0.lock().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sql: &str) -> HistoryEntry {
        HistoryEntry {
            sql: sql.into(),
            started_at_ms: 1_700_000_000_000,
            elapsed_ms: 3,
            outcome: Outcome::Rows { count: 1 },
        }
    }

    #[test]
    fn the_newest_statement_comes_first() {
        let mut history = QueryHistory::default();
        history.record(entry("select 1"));
        history.record(entry("select 2"));
        let sql: Vec<_> = history
            .newest_first()
            .into_iter()
            .map(|entry| entry.sql)
            .collect();
        assert_eq!(sql, ["select 2", "select 1"]);
    }

    #[test]
    fn the_cap_drops_the_oldest_and_keeps_the_newest() {
        let mut history = QueryHistory::default();
        for number in 0..MAX_ENTRIES + 5 {
            history.record(entry(&format!("select {number}")));
        }
        assert_eq!(history.len(), MAX_ENTRIES);
        let newest = history.newest_first();
        assert_eq!(newest[0].sql, format!("select {}", MAX_ENTRIES + 4));
        assert_eq!(
            newest[MAX_ENTRIES - 1].sql,
            "select 5",
            "the five oldest are the ones that went"
        );
    }

    #[test]
    fn a_list_read_back_over_the_cap_is_trimmed_to_the_newest() {
        let mut history = QueryHistory::default();
        for number in 0..MAX_ENTRIES {
            history.record(entry(&format!("select {number}")));
        }
        let mut stored: Vec<HistoryEntry> =
            serde_json::from_str(&history.to_json().unwrap()).expect("what was written parses");
        stored.push(entry("select newest"));
        let json = serde_json::to_string(&stored).unwrap();

        let read = QueryHistory::from_json(&json).expect("an over-long list still reads");
        assert_eq!(read.len(), MAX_ENTRIES);
        assert_eq!(read.newest_first()[0].sql, "select newest");
        assert_eq!(read.newest_first()[MAX_ENTRIES - 1].sql, "select 1");
    }

    #[test]
    fn merging_what_ran_during_the_read_keeps_the_cap() {
        let mut stored = QueryHistory::default();
        for number in 0..MAX_ENTRIES {
            stored.record(entry(&format!("select {number}")));
        }
        let merged = QueryHistory::merged(stored, vec![entry("late 1"), entry("late 2")]);
        assert_eq!(merged.len(), MAX_ENTRIES);
        assert_eq!(merged.newest_first()[0].sql, "late 2");
        assert_eq!(merged.newest_first()[1].sql, "late 1");
    }

    #[test]
    fn a_pasted_dump_is_not_remembered() {
        let mut history = QueryHistory::default();
        assert!(!history.record(entry(&"x".repeat(MAX_SQL_BYTES + 1))));
        assert_eq!(history.len(), 0);
        assert!(history.record(entry(&"x".repeat(MAX_SQL_BYTES))));
    }

    #[test]
    fn the_total_size_is_capped_by_dropping_the_oldest() {
        let mut history = QueryHistory::default();
        let statements = MAX_TOTAL_BYTES / MAX_SQL_BYTES + 10;
        for number in 0..statements {
            let padding = "x".repeat(MAX_SQL_BYTES - 10);
            assert!(history.record(entry(&format!("{number:<9} {padding}"))));
        }
        assert!(
            history.len() <= MAX_TOTAL_BYTES / (MAX_SQL_BYTES - 10),
            "{} of {statements} statements kept",
            history.len()
        );
        assert!(history.len() >= MAX_TOTAL_BYTES / MAX_SQL_BYTES - 1);
        assert!(
            history.newest_first()[0]
                .sql
                .starts_with(&(statements - 1).to_string()),
            "the newest is the one kept"
        );
    }

    #[test]
    fn a_list_read_back_over_the_size_cap_is_trimmed_to_the_newest() {
        let padding = "x".repeat(MAX_SQL_BYTES);
        let entries: Vec<HistoryEntry> = (0..MAX_TOTAL_BYTES / MAX_SQL_BYTES + 20)
            .map(|number| entry(&format!("{number}{padding}")))
            .collect();
        let json = serde_json::to_string(&entries).unwrap();

        let read = QueryHistory::from_json(&json).expect("an over-large list still reads");
        assert!(read.len() <= MAX_TOTAL_BYTES / MAX_SQL_BYTES);
        assert!(
            read.newest_first()[0]
                .sql
                .starts_with(&(entries.len() - 1).to_string())
        );
    }

    #[test]
    fn a_failure_keeps_a_bounded_message() {
        let failed = HistoryEntry::failed("select".into(), 0, 1, &"é".repeat(MAX_ERROR_CHARS * 2));
        let Outcome::Failed { message } = failed.outcome else {
            panic!("a failure is recorded as one");
        };
        assert_eq!(message.chars().count(), MAX_ERROR_CHARS);
    }

    #[test]
    fn search_ignores_case_and_matches_anywhere_in_the_statement() {
        let users = entry("SELECT * FROM Users WHERE id = 1");
        assert!(users.matches("users"));
        assert!(users.matches("WHERE"));
        assert!(users.matches(""));
        assert!(!users.matches("orders"));
    }

    #[test]
    fn a_list_survives_a_round_trip_through_json() {
        let mut history = QueryHistory::default();
        history.record(entry("select 1"));
        history.record(HistoryEntry::failed("bad".into(), 5, 6, "no such table"));
        let read = QueryHistory::from_json(&history.to_json().unwrap()).unwrap();
        assert_eq!(read, history);
    }

    #[test]
    fn garbage_in_the_store_is_an_error_not_a_panic() {
        assert!(QueryHistory::from_json("{not json").is_err());
    }
}
