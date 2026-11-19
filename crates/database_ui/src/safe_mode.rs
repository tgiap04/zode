//! How careful a connection is about statements that change data.
//!
//! Every connection has a [`SafetyLevel`]; the editor reads the statement it is
//! about to send, decides whether it changes anything, and the level decides
//! what happens next. This is a guard against mistakes -- a `DELETE` run on the
//! wrong connection -- and not a security boundary: a function called from a
//! `SELECT` can write, and no classifier here will see it. A database nobody
//! may write to needs a database user that cannot.
//!
//! Only what somebody typed is gated. Open Data and Count send statements the
//! dialect generated, which are reads by construction. Paging and export re-send
//! a typed statement, so they refuse a page of one that changes data
//! (`Page::can_turn`).

use crate::database_panel::{DatabasePanel, NodeOrigin, NodeState};
use crate::panel_queries::ChosenStatement;
use crate::query::QueryError;
use crate::query_history::{HistoryEntry, now_ms};
use crate::sql_dialect::{Dialect, Engine};
use crate::statement_split::{SplitRules, TokenKind, statement_ranges, tokens};
use collections::HashMap;
use db::kvp::KeyValueStore;
use gpui::{App, Context, PromptLevel, Window};
use settings::DatabaseSafety;
use std::ops::Range;
use util::ResultExt as _;

/// The key-value namespace for the levels of connections that are not in
/// settings, keyed by [`override_key`].
const OVERRIDES_NAMESPACE: &str = "database-safety";

/// Longest stretch of a statement the confirmation shows.
const PREVIEW_CHARS: usize = 600;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SafetyLevel {
    Unrestricted,
    /// Asks before anything that changes data. The default, so that a
    /// connection nobody configured is one a stray `DELETE` cannot reach
    /// unannounced.
    #[default]
    ConfirmWrites,
    ReadOnly,
}

impl SafetyLevel {
    pub(crate) const ALL: [SafetyLevel; 3] = [
        SafetyLevel::ReadOnly,
        SafetyLevel::ConfirmWrites,
        SafetyLevel::Unrestricted,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Unrestricted => "Unrestricted",
            Self::ConfirmWrites => "Confirm Writes",
            Self::ReadOnly => "Read Only",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Unrestricted => "unrestricted",
            Self::ConfirmWrites => "confirm_writes",
            Self::ReadOnly => "read_only",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|level| level.key() == key)
    }
}

impl From<DatabaseSafety> for SafetyLevel {
    fn from(safety: DatabaseSafety) -> Self {
        match safety {
            DatabaseSafety::Unrestricted => Self::Unrestricted,
            DatabaseSafety::ConfirmWrites => Self::ConfirmWrites,
            DatabaseSafety::ReadOnly => Self::ReadOnly,
        }
    }
}

impl From<SafetyLevel> for DatabaseSafety {
    fn from(level: SafetyLevel) -> Self {
        match level {
            SafetyLevel::Unrestricted => Self::Unrestricted,
            SafetyLevel::ConfirmWrites => Self::ConfirmWrites,
            SafetyLevel::ReadOnly => Self::ReadOnly,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StatementKind {
    Read,
    Write,
}

/// What to do with a statement of some kind on a connection of some level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gate {
    Run,
    Confirm,
    Refuse,
}

pub(crate) fn gate(level: SafetyLevel, kind: StatementKind) -> Gate {
    match (level, kind) {
        (_, StatementKind::Read) | (SafetyLevel::Unrestricted, _) => Gate::Run,
        (SafetyLevel::ConfirmWrites, StatementKind::Write) => Gate::Confirm,
        (SafetyLevel::ReadOnly, StatementKind::Write) => Gate::Refuse,
    }
}

/// Whether running `sql` could change anything. Any statement that might counts.
///
/// Leans toward `Write` wherever it cannot tell: a statement wrongly taken for
/// a write costs one click, one wrongly taken for a read is the mistake this
/// exists to stop.
pub(crate) fn classify(dialect: &Dialect, sql: &str) -> StatementKind {
    // The document driver only runs `find` and `aggregate`, and refuses
    // `$out` and `$merge` itself.
    if dialect.engine() == Engine::Mongo {
        return StatementKind::Read;
    }
    // The server runs the code in a MySQL `/*! ... */` comment, and the
    // splitter, like any reader, takes it for a comment and finds no statement.
    if sql.contains("/*!") || sql.contains("/*M!") {
        return StatementKind::Write;
    }
    // One reading finding a write is enough: the server may be using any.
    let writes = dialect
        .classification_rules()
        .iter()
        .any(|&rules| write_ranges(sql, rules).next().is_some());
    if writes {
        StatementKind::Write
    } else {
        StatementKind::Read
    }
}

/// Where the statements of `sql` that change data lie, read under `rules`.
fn write_ranges(sql: &str, rules: SplitRules) -> impl Iterator<Item = Range<usize>> + '_ {
    statement_ranges(sql, rules)
        .into_iter()
        .filter(move |range| changes_data(&sql[range.clone()], rules))
}

/// Words that start a statement which only reads. Anything else is a write.
const READING_WORDS: [&str; 7] = [
    "SELECT", "VALUES", "TABLE", "SHOW", "DESCRIBE", "DESC", "USE",
];

/// Statements nested in another statement are the one way a read-looking
/// statement writes: `WITH gone AS (DELETE ... RETURNING *) SELECT ...`.
const NESTED_WRITES: [&str; 4] = ["INSERT", "UPDATE", "DELETE", "MERGE"];

/// What can follow `EXPLAIN` before the statement it explains.
const EXPLAIN_WORDS: [&str; 6] = ["ANALYZE", "ANALYSE", "VERBOSE", "QUERY", "PLAN", "EXTENDED"];

/// A statement's significant text: words, quoted text and punctuation, with
/// the comments and blanks between them gone.
#[derive(Clone, Copy)]
struct Piece<'a> {
    kind: TokenKind,
    text: &'a str,
}

impl Piece<'_> {
    fn is_word(&self, word: &str) -> bool {
        self.kind == TokenKind::Word && self.text.eq_ignore_ascii_case(word)
    }

    fn is_symbol(&self, symbol: &str) -> bool {
        self.kind == TokenKind::Symbol && self.text == symbol
    }
}

fn changes_data(statement: &str, rules: SplitRules) -> bool {
    let pieces: Vec<Piece> = tokens(statement, rules)
        .filter(|token| {
            matches!(
                token.kind,
                TokenKind::Word | TokenKind::Quoted | TokenKind::Symbol
            )
        })
        .map(|token| Piece {
            kind: token.kind,
            text: &statement[token.range],
        })
        .collect();
    pieces_change_data(&pieces)
}

fn pieces_change_data(pieces: &[Piece]) -> bool {
    // Up to the first word: `(SELECT 1) UNION (SELECT 2)` starts with a `(`.
    let Some(start) = pieces
        .iter()
        .position(|piece| piece.kind == TokenKind::Word)
    else {
        return true;
    };
    let pieces = &pieces[start..];
    // `SELECT ... INTO` creates a table in some engines and writes a file in
    // others.
    let selects_into = || pieces.iter().any(|piece| piece.is_word("INTO"));
    let first = pieces[0].text.to_ascii_uppercase();
    match first.as_str() {
        "WITH" => has_nested_write(pieces) || selects_into(),
        // `EXPLAIN ANALYZE` runs what it explains, whatever that is.
        "EXPLAIN" => pieces_change_data(explained(&pieces[1..])),
        first if READING_WORDS.contains(&first) => selects_into(),
        _ => true,
    }
}

fn has_nested_write(pieces: &[Piece]) -> bool {
    pieces.iter().enumerate().any(|(index, piece)| {
        NESTED_WRITES.iter().any(|write| piece.is_word(write)) && !is_row_lock(pieces, index)
    })
}

/// Whether the word at `index` is the `UPDATE` of `FOR UPDATE` or
/// `FOR NO KEY UPDATE`, which lock rows, and a locking read changes none.
///
/// Judged by the pieces right before it, with nothing between: a column called
/// `key` or `for`, then a `)`, then an `UPDATE` is an `UPDATE`.
fn is_row_lock(pieces: &[Piece], index: usize) -> bool {
    let is_word_before = |distance: usize, word: &str| {
        index
            .checked_sub(distance)
            .and_then(|before| pieces.get(before))
            .is_some_and(|piece| piece.is_word(word))
    };
    pieces[index].is_word("UPDATE")
        && (is_word_before(1, "FOR")
            || (is_word_before(1, "KEY") && is_word_before(2, "NO") && is_word_before(3, "FOR")))
}

/// What an `EXPLAIN` explains: the pieces after its options, whether they are a
/// parenthesised list -- `(ANALYZE, FORMAT JSON)` -- or words -- `ANALYZE
/// VERBOSE`, `QUERY PLAN`, `FORMAT = TREE`.
fn explained<'pieces, 'text>(mut rest: &'pieces [Piece<'text>]) -> &'pieces [Piece<'text>] {
    if rest.first().is_some_and(|piece| piece.is_symbol("("))
        && let Some(close) = rest.iter().position(|piece| piece.is_symbol(")"))
        // A list holding anything but option words is a statement in
        // parentheses, and is asked about as one.
        && rest[1..close]
            .iter()
            .all(|piece| piece.kind == TokenKind::Word || piece.is_symbol(","))
    {
        rest = &rest[close + 1..];
    }
    loop {
        match rest {
            [format, equals, _value, after @ ..]
                if format.is_word("FORMAT") && equals.is_symbol("=") =>
            {
                rest = after;
            }
            [first, after @ ..] if EXPLAIN_WORDS.iter().any(|word| first.is_word(word)) => {
                rest = after;
            }
            _ => return rest,
        }
    }
}

/// The statements of `sql` that change data, in the order they appear, as any
/// reading of the engine's quoting finds them.
fn write_statements<'sql>(dialect: &Dialect, sql: &'sql str) -> Vec<&'sql str> {
    let mut ranges: Vec<Range<usize>> = dialect
        .classification_rules()
        .iter()
        .flat_map(|&rules| write_ranges(sql, rules))
        .collect();
    ranges.sort_by_key(|range| (range.start, range.end));
    ranges.dedup();
    ranges.into_iter().map(|range| &sql[range]).collect()
}

/// What the confirmation shows of a statement: all of it when it fits a
/// dialog, otherwise the statements that change data, which are what the user
/// is being asked about -- a run of reads with a `DELETE` at its end must not
/// show the reads.
pub(crate) fn preview(dialect: Option<&Dialect>, sql: &str) -> String {
    let sql = sql.trim();
    let total = sql.chars().count();
    if total <= PREVIEW_CHARS {
        return sql.to_string();
    }
    let writes = dialect.map_or_else(Vec::new, |dialect| write_statements(dialect, sql));
    if writes.is_empty() {
        return truncated(sql, PREVIEW_CHARS);
    }
    let mut lines = Vec::new();
    let mut room = PREVIEW_CHARS;
    for (index, statement) in writes.iter().enumerate() {
        if room == 0 {
            lines.push(format!(
                "… and {} more statements that change data",
                writes.len() - index
            ));
            break;
        }
        lines.push(truncated(statement, room));
        room = room.saturating_sub(statement.chars().count());
    }
    lines.push(format!(
        "Showing only the statements that change data, of {total} characters in all."
    ));
    lines.join("\n")
}

/// `text` cut to `limit` characters on a character boundary, with a count of
/// what was left out.
fn truncated(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((cut, _)) => format!(
            "{}… ({} more characters)",
            &text[..cut],
            text.chars().count() - limit
        ),
        None => text.to_string(),
    }
}

/// The levels of connections that are not in settings, read once when the panel
/// is built, by [`override_key`].
///
/// A team or project connection has no entry in settings to carry a level, and
/// nothing about it should be shared: what one person is careful about on their
/// machine is not a decision for the team. Read here rather than on first use
/// because the gate is synchronous and a level that had not arrived yet would
/// read as the default, which is weaker than `ReadOnly`.
pub(crate) fn read_overrides(cx: &App) -> HashMap<String, SafetyLevel> {
    let store = KeyValueStore::global(cx);
    let scoped = store.scoped(OVERRIDES_NAMESPACE);
    let mut overrides = HashMap::default();
    for key in scoped.keys().log_err().unwrap_or_default() {
        let level = scoped
            .read(&key)
            .log_err()
            .flatten()
            .and_then(|level| SafetyLevel::from_key(&level));
        if let Some(level) = level {
            overrides.insert(key, level);
        }
    }
    overrides
}

/// What the grid says when a statement is held back. Not a promise that a
/// higher level makes the statement work: the level decides what the editor
/// sends, and the driver, or the database user, may still refuse to write.
fn refusal_message(name: &str) -> String {
    format!(
        "{name} is set to Read Only, so this statement, which changes data, was not sent. \
         A higher safety level lets the editor send it; the driver or the database user \
         may still refuse to write."
    )
}

/// What a connection's level is kept under in the local store: its name and
/// where it came from, because a team connection and a project connection may
/// share a name and must not share a level.
///
/// The origin leads and has no `:` in it, so no name can spell the key of
/// another origin.
pub(crate) fn override_key(name: &str, origin: NodeOrigin) -> String {
    let origin = match origin {
        NodeOrigin::Personal => "personal",
        NodeOrigin::Project => "project",
        NodeOrigin::Team => "team",
    };
    format!("{origin}:{name}")
}

impl DatabasePanel {
    /// Runs `chosen` on the active connection if its level allows, asks first if
    /// the level says to, and refuses if it forbids.
    pub(crate) fn run_gated(
        &mut self,
        chosen: ChosenStatement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Not connected: `start_query` says so, and there is nothing to guard.
        let Some(index) = self
            .active
            .filter(|index| matches!(self.connections[*index].state, NodeState::Connected(_)))
        else {
            self.send_statement(chosen, window, cx);
            return;
        };
        let kind = self.dialect(index).map_or(StatementKind::Write, |dialect| {
            classify(&dialect, &chosen.sql)
        });
        match gate(self.safety_of(index, cx), kind) {
            Gate::Run => self.send_statement(chosen, window, cx),
            Gate::Refuse => self.refuse_write(index, &chosen.sql, cx),
            Gate::Confirm => self.confirm_write(index, chosen, window, cx),
        }
    }

    fn refuse_write(&mut self, index: usize, sql: &str, cx: &mut Context<Self>) {
        let Some(name) = self
            .connections
            .get(index)
            .map(|node| node.config.name.clone())
        else {
            return;
        };
        let message = refusal_message(&name);
        let entry = HistoryEntry::failed(sql.to_string(), now_ms(), 0, &message);
        self.record_history(name, entry, cx);
        self.show_failure(
            QueryError {
                message: message.into(),
                read_only: true,
            },
            cx,
        );
    }

    fn confirm_write(
        &mut self,
        index: usize,
        chosen: ChosenStatement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(key), Some(name)) = (
            self.node_key(index),
            self.connections
                .get(index)
                .map(|node| node.config.name.clone()),
        ) else {
            return;
        };
        let detail = preview(self.dialect(index).as_ref(), &chosen.sql);
        // Cancel first, so that the key that confirms a dialog -- Return -- is
        // not the one that runs a write.
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("This statement changes data on {name}. Run?"),
            Some(&detail),
            &["Cancel", "Run"],
            cx,
        );
        let task = cx.spawn_in(window, async move |this, cx| {
            if answer.await.ok() != Some(1) {
                return;
            }
            this.update_in(cx, |this, window, cx| {
                this.run_confirmed(&key, chosen, window, cx)
            })
            .ok();
        });
        self.keep_task(task);
    }

    /// Carries out a run the user said yes to.
    ///
    /// Checked again, because the prompt stays up while other things happen: it
    /// was a yes for one connection, and the level may have been tightened
    /// since.
    fn run_confirmed(
        &mut self,
        key: &(String, NodeOrigin),
        chosen: ChosenStatement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .index_of_key(key)
            .filter(|index| self.active == Some(*index))
        else {
            return;
        };
        if self.safety_of(index, cx) == SafetyLevel::ReadOnly {
            self.refuse_write(index, &chosen.sql, cx);
        } else {
            self.send_statement(chosen, window, cx);
        }
    }

    /// The level in force on a connection.
    ///
    /// A personal connection's level is in settings beside it; any other
    /// connection has the local override, or the default.
    pub(crate) fn safety_of(&self, index: usize, cx: &App) -> SafetyLevel {
        use settings::Settings as _;
        let Some(node) = self.connections.get(index) else {
            return SafetyLevel::default();
        };
        let name = &node.config.name;
        let level = if node.is_personal() {
            crate::connection_store::DatabaseSettings::get_global(cx)
                .safety
                .get(name)
        } else {
            self.safety_overrides
                .get(&override_key(name, node.origin()))
        };
        level.copied().unwrap_or_default()
    }

    /// Changes a connection's level and writes it down: into settings for a
    /// personal connection, into the local store for the rest.
    pub(crate) fn set_safety(&mut self, index: usize, level: SafetyLevel, cx: &mut Context<Self>) {
        let Some(node) = self.connections.get(index) else {
            return;
        };
        let name = node.config.name.clone();
        if node.is_personal() {
            let Ok(fs) = self.workspace.read_with(cx, |workspace, cx| {
                workspace.project().read(cx).fs().clone()
            }) else {
                return;
            };
            settings::update_settings_file(fs, cx, move |content, _cx| {
                let connection = content
                    .database
                    .as_mut()
                    .and_then(|database| database.connections.as_mut())
                    .and_then(|connections| {
                        connections
                            .iter_mut()
                            .find(|connection| connection.name.as_deref() == Some(name.as_str()))
                    });
                if let Some(connection) = connection {
                    connection.safety = Some(level.into());
                }
            });
        } else {
            let key = override_key(&name, node.origin());
            self.safety_overrides.insert(key.clone(), level);
            let store = KeyValueStore::global(cx);
            cx.spawn(async move |_this, _cx| {
                store
                    .scoped(OVERRIDES_NAMESPACE)
                    .write(key, level.key().to_string())
                    .await
            })
            .detach_and_log_err(cx);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::protocol::Capabilities;

    fn sql_kind(driver: &str, sql: &str) -> StatementKind {
        classify(&Dialect::detect(driver, &Capabilities::default()), sql)
    }

    fn assert_writes(statements: &[&str]) {
        assert_writes_on("sqlite", statements);
    }

    fn assert_reads(statements: &[&str]) {
        assert_reads_on("sqlite", statements);
    }

    fn assert_writes_on(driver: &str, statements: &[&str]) {
        for sql in statements {
            assert_eq!(
                sql_kind(driver, sql),
                StatementKind::Write,
                "{driver}: `{sql}` changes data"
            );
        }
    }

    fn assert_reads_on(driver: &str, statements: &[&str]) {
        for sql in statements {
            assert_eq!(
                sql_kind(driver, sql),
                StatementKind::Read,
                "{driver}: `{sql}` only reads"
            );
        }
    }

    #[test]
    fn plain_writes_are_writes_in_any_case() {
        assert_writes(&[
            "INSERT INTO t VALUES (1)",
            "update t set a = 1",
            "Delete From t",
            "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE",
            "CREATE TABLE t (a int)",
            "alter table t add b int",
            "DROP TABLE t",
            "TRUNCATE t",
            "GRANT ALL ON t TO someone",
            "REVOKE ALL ON t FROM someone",
            "COPY t FROM '/tmp/x'",
            "CALL cleanup()",
            "DO $$ BEGIN DELETE FROM t; END $$",
            "REPLACE INTO t VALUES (1)",
        ]);
    }

    #[test]
    fn plain_reads_are_reads() {
        assert_reads(&[
            "SELECT 1",
            "select * from t where a = 1",
            "VALUES (1), (2)",
            "TABLE t",
            "SHOW TABLES",
            "DESCRIBE t",
            "(SELECT 1) UNION (SELECT 2)",
        ]);
    }

    #[test]
    fn comments_before_the_keyword_do_not_hide_it() {
        assert_writes(&[
            "-- tidy up\nDELETE FROM t",
            "/* tidy up */ delete from t",
            "/* a */ -- b\n  /* c */ UPDATE t SET a = 1",
        ]);
        assert_reads(&["-- look\nSELECT 1", "/* look */ select 1"]);
    }

    /// A comment that mentions a write is still a comment.
    #[test]
    fn a_keyword_in_a_comment_or_a_string_is_not_a_keyword() {
        assert_reads(&[
            "SELECT 1 -- delete everything",
            "SELECT 1 /* insert into x */",
            "SELECT 'insert into t' AS note",
            "SELECT \"into\" FROM t",
        ]);
    }

    /// The server runs the code in a MySQL `/*! ... */` comment.
    #[test]
    fn a_conditional_comment_is_code_not_a_comment() {
        assert_writes(&["/*!50000 DROP TABLE t */"]);
    }

    #[test]
    fn a_with_ending_in_a_write_is_a_write_and_one_ending_in_a_select_is_not() {
        assert_writes(&[
            "WITH x AS (SELECT 1) UPDATE t SET a = 1",
            "WITH x AS (SELECT 1) DELETE FROM t",
            "WITH x AS (SELECT 1) INSERT INTO t SELECT * FROM x",
            // PostgreSQL runs a write inside a CTE even when a select follows.
            "WITH gone AS (DELETE FROM t RETURNING *) SELECT * FROM gone",
        ]);
        assert_reads(&[
            "WITH x AS (SELECT 1) SELECT * FROM x",
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 3) SELECT * FROM n",
        ]);
    }

    /// Locking rows is a read: it is what a transaction does before updating.
    #[test]
    fn a_locking_select_is_a_read() {
        assert_reads(&[
            "SELECT * FROM t WHERE id = 1 FOR UPDATE",
            "SELECT * FROM t FOR NO KEY UPDATE",
            "SELECT * FROM t FOR SHARE",
            "WITH x AS (SELECT 1) SELECT * FROM t FOR UPDATE",
        ]);
    }

    /// A column called `key` is common, and `FOR` is a name in some engines:
    /// only a lock clause -- `FOR UPDATE`, `FOR NO KEY UPDATE` -- is exempt, and
    /// only when nothing stands between its words and the `UPDATE`.
    #[test]
    fn a_column_called_key_or_for_does_not_turn_an_update_into_a_lock() {
        assert_writes(&[
            "WITH c AS (SELECT * FROM kv ORDER BY key) UPDATE kv SET v = 1",
            "WITH x AS (SELECT 1 AS key) UPDATE t SET a = 1",
            "WITH c AS (SELECT * FROM t ORDER BY for) UPDATE t SET a = 1",
            "WITH c AS (SELECT * FROM t ORDER BY no key) UPDATE t SET a = 1",
        ]);
        assert_reads(&[
            "SELECT * FROM t FOR KEY SHARE",
            "SELECT * FROM t FOR UPDATE OF t NOWAIT",
            "SELECT * FROM t FOR NO KEY UPDATE SKIP LOCKED",
            "WITH x AS (SELECT 1 AS key) SELECT * FROM x FOR NO KEY UPDATE",
        ]);
    }

    #[test]
    fn a_select_that_creates_something_is_a_write() {
        assert_writes(&[
            "SELECT * INTO backup FROM t",
            "SELECT * FROM t INTO OUTFILE '/tmp/x'",
        ]);
    }

    /// `EXPLAIN ANALYZE` runs what it explains, whatever that is, so the
    /// statement after the options is the one that is asked about.
    #[test]
    fn explain_is_a_read_only_when_what_it_explains_is_one() {
        for driver in ["sqlite", "postgres", "mysql"] {
            assert_reads_on(
                driver,
                &[
                    "EXPLAIN SELECT * FROM t",
                    "EXPLAIN ANALYZE SELECT 1",
                    "EXPLAIN ANALYZE VERBOSE SELECT 1",
                    "EXPLAIN (ANALYZE, VERBOSE, FORMAT JSON) SELECT 1",
                    "explain (analyze true) with x as (select 1) select * from x",
                    "EXPLAIN QUERY PLAN SELECT * FROM t",
                    "EXPLAIN FORMAT = JSON SELECT * FROM t",
                    "EXPLAIN ANALYZE SELECT * FROM t FOR UPDATE",
                ],
            );
            assert_writes_on(
                driver,
                &[
                    "EXPLAIN ANALYZE DELETE FROM t",
                    "EXPLAIN (ANALYZE) UPDATE t SET a = 1",
                    "EXPLAIN ANALYZE CREATE TABLE t2 AS SELECT * FROM t",
                    "EXPLAIN ANALYZE SELECT * INTO t2 FROM t",
                    "EXPLAIN (ANALYZE, VERBOSE) SELECT * INTO t2 FROM t",
                    "EXPLAIN ANALYZE EXECUTE p",
                    "EXPLAIN (ANALYZE) EXECUTE p",
                    "EXPLAIN ANALYZE REFRESH MATERIALIZED VIEW v",
                    "EXPLAIN ANALYZE WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d",
                    "EXPLAIN",
                    "EXPLAIN ANALYZE",
                    "EXPLAIN (ANALYZE",
                    "EXPLAIN (DELETE FROM t)",
                    "EXPLAIN ANALYZE SELECT 1; DELETE FROM t",
                ],
            );
        }
    }

    #[test]
    fn any_write_among_several_statements_makes_the_whole_a_write() {
        assert_writes(&[
            "SELECT 1; DELETE FROM t",
            "DELETE FROM t; SELECT 1",
            "SELECT 1;\n-- now\nUPDATE t SET a = 1;",
            "SELECT 'a;b'; DROP TABLE t",
        ]);
        assert_reads(&["SELECT 1; SELECT 2;", "SELECT 'a;DELETE FROM t'"]);
    }

    /// Not knowing is not a licence: the statement is asked about.
    #[test]
    fn a_keyword_nobody_listed_is_a_write() {
        assert_writes(&[
            "VACUUM",
            "PRAGMA journal_mode = delete",
            "BEGIN",
            "frobnicate the table",
            "*",
            "(",
        ]);
    }

    #[test]
    fn nothing_to_run_changes_nothing() {
        assert_reads(&["", "   \n", "-- only a comment", ";;"]);
    }

    /// The reading of a quote the engine's server uses decides what is code,
    /// and the client cannot always know which: a string that ends differently
    /// under another setting is read both ways, and one reading finding a write
    /// is enough.
    #[test]
    fn a_string_that_ends_differently_on_two_servers_is_read_both_ways() {
        let sql = r"SELECT 'it\' into the dark' AS note";
        // Standard strings end at the `\'`, and `into` is a word of the statement.
        assert_eq!(sql_kind("sqlite", sql), StatementKind::Write);
        // MySQL keeps `into` in the string, unless NO_BACKSLASH_ESCAPES is on.
        assert_eq!(sql_kind("mysql", sql), StatementKind::Write);
        // Without a quote after the backslash there is nothing to disagree on.
        assert_reads_on(
            "mysql",
            &[r"SELECT 'a\\b into c' AS note", r"SELECT 'it\'s' AS note"],
        );
    }

    /// Each of these reads as a single harmless statement under the lexer the
    /// guard used to apply to every engine, and runs a `DELETE` on the engine
    /// it is written for.
    #[test]
    fn a_write_hidden_by_another_lexers_reading_of_quotes_and_comments_is_found() {
        assert_writes_on(
            "mysql",
            &[
                // `#` starts a comment in MySQL, so the `'` is inside it.
                "SELECT 1 # don't\n; DELETE FROM t",
                // `--` is a comment only before a space: here it is `1 - -1`.
                "WITH x AS (SELECT 1--1) DELETE FROM t",
                // With NO_BACKSLASH_ESCAPES the string ends at the `\'`.
                r"SELECT 'a\' ; DELETE FROM t; -- '",
                // A backslash means nothing inside a backtick name.
                "SELECT `a\\` FROM t; DELETE FROM t WHERE x = '",
                // With ANSI_QUOTES a `"` name ends at the quote.
                "SELECT \"x\\\" 'a\\'b' ; DELETE FROM t; --",
            ],
        );
        assert_writes_on(
            "postgres",
            &[
                // An `E` string honours the backslash whatever else is set.
                r"WITH a AS (SELECT E'\'') DELETE FROM t WHERE x = ''",
                // A doubled quote stays inside an `E` string, and so does a `\'`
                // after it.
                r"SELECT E'a''b\'' ; DELETE FROM t; --'",
                // With standard_conforming_strings off a plain string honours it too.
                r"SELECT 'a\'' ; DELETE FROM t; --'",
                // A dollar-quoted body is one string, so what follows it is a
                // statement of its own.
                "SELECT $$ ; $$; DELETE FROM t",
            ],
        );
        assert_writes_on(
            "sqlite",
            &["WITH x AS (SELECT [a'] FROM t) DELETE FROM t WHERE b = 'z'"],
        );
    }

    /// What the dialect used to write for a value ending in a backslash: with
    /// standard_conforming_strings off it runs the second filter's value as SQL,
    /// and the guard sees that, whichever way the server reads the string.
    #[test]
    fn a_string_closed_early_by_a_backslash_exposes_the_statement_after_it() {
        assert_writes_on(
            "postgres",
            &[r#"SELECT * FROM "t" WHERE "a" = 'x\' AND "b" = '; DELETE FROM t --'"#],
        );
        assert_reads_on(
            "postgres",
            &[r#"SELECT * FROM "t" WHERE "a" = E'x\\' AND "b" = '; DELETE FROM t --'"#],
        );
    }

    /// The other half: reading every way must not make ordinary reads writes.
    #[test]
    fn reads_that_use_each_engines_quoting_stay_reads() {
        assert_reads_on(
            "mysql",
            &[
                "SELECT 'a;b', \"c--d\", `e#f` FROM t # delete from t",
                "SELECT 1 -- delete from t",
                "SELECT 1 --\n, 2",
                "SELECT /* delete from t */ 1",
                "SELECT '#', '-- x' FROM t WHERE a = 'it''s' ORDER BY `a``b`",
            ],
        );
        assert_reads_on(
            "postgres",
            &[
                "SELECT E'it\\'s', 'a' FROM t",
                "SELECT $tag$ delete from t; $tag$",
                "SELECT a$b$c FROM t",
                "SELECT $1, $2",
                "SELECT '[a', ']' FROM t",
            ],
        );
        assert_reads_on(
            "sqlite",
            &[
                "SELECT [a b], [c;d] FROM [t u]",
                "SELECT \"a#b\" FROM t -- [",
                "SELECT '#' FROM t",
            ],
        );
    }

    #[test]
    fn a_document_store_only_ever_reads() {
        let documents = Dialect::detect(
            "mongodb",
            &Capabilities {
                read_table_template: Some("{}".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            classify(&documents, r#"{"find": "users", "$db": "app"}"#),
            StatementKind::Read
        );
    }

    /// Open Data, Count, paging and export are not gated, because what the
    /// dialect writes for them cannot change anything -- including when a
    /// filter value is built to look like more statements.
    #[test]
    fn what_the_dialect_generates_for_browsing_only_reads() {
        use crate::query::{Filter, FilterOp, TableBrowse, ValueKind};
        let documents = Capabilities {
            read_table_template: Some("{}".into()),
            ..Default::default()
        };
        for (driver, capabilities) in [
            ("sqlite", Capabilities::default()),
            ("postgres", Capabilities::default()),
            ("mysql", Capabilities::default()),
            ("mongodb", documents),
        ] {
            let dialect = Dialect::detect(driver, &capabilities);
            let mut browse = TableBrowse::new(
                ("db".into(), NodeOrigin::Personal),
                "s\"; DROP TABLE t; --",
                "t'; DELETE FROM t; --",
            );
            browse.key_columns = vec!["id".into()];
            let mut statements = vec![browse.statement(&dialect)];
            browse.cycle_sort("a\"; DROP TABLE t; --");
            statements.push(browse.statement(&dialect));
            browse.filters.push(Filter {
                column: "note".into(),
                op: FilterOp::Equals,
                value: "x'; DELETE FROM t; -- \\".into(),
                kind: ValueKind::Text,
            });
            statements.push(browse.statement(&dialect));
            // A backslash in one value and a statement in the next: where the
            // server reads a backslash as an escape, an unescaped one would
            // close its string over the second value's opening quote.
            for (column, value) in [("a", "x\\"), ("b", "; DELETE FROM t --")] {
                browse.filters.push(Filter {
                    column: column.into(),
                    op: FilterOp::Equals,
                    value: value.into(),
                    kind: ValueKind::Text,
                });
            }
            statements.push(browse.statement(&dialect));
            statements.push(dialect.count("s", "t", None));
            for statement in statements.into_iter().flatten() {
                assert_eq!(
                    classify(&dialect, &statement),
                    StatementKind::Read,
                    "{driver}: `{statement}`"
                );
            }
        }
    }

    #[test]
    fn each_level_decides_for_each_kind() {
        use Gate::*;
        use StatementKind::*;
        assert_eq!(gate(SafetyLevel::Unrestricted, Write), Run);
        assert_eq!(gate(SafetyLevel::ConfirmWrites, Write), Confirm);
        assert_eq!(gate(SafetyLevel::ReadOnly, Write), Refuse);
        for level in SafetyLevel::ALL {
            assert_eq!(gate(level, Read), Run, "{level:?} never stops a read");
        }
    }

    #[test]
    fn an_unconfigured_connection_confirms_writes() {
        assert_eq!(SafetyLevel::default(), SafetyLevel::ConfirmWrites);
        assert_eq!(
            SafetyLevel::from(DatabaseSafety::default()),
            SafetyLevel::ConfirmWrites
        );
    }

    #[test]
    fn a_level_survives_its_stored_key() {
        for level in SafetyLevel::ALL {
            assert_eq!(SafetyLevel::from_key(level.key()), Some(level));
            assert_eq!(SafetyLevel::from(DatabaseSafety::from(level)), level);
        }
        assert_eq!(SafetyLevel::from_key("read-only"), None);
    }

    fn dialect_of(driver: &str) -> Dialect {
        Dialect::detect(driver, &Capabilities::default())
    }

    /// A run of many reads with a `DELETE` at its end: the dialog is the only
    /// thing between the user and the `DELETE`, so it is what it shows.
    #[test]
    fn a_long_run_all_shows_the_write_it_would_run_not_the_reads_before_it() {
        let reads = "SELECT * FROM users WHERE id = 1;\n".repeat(30);
        let sql = format!("{reads}DELETE FROM users");
        assert!(sql.len() > PREVIEW_CHARS);
        for driver in ["sqlite", "postgres", "mysql"] {
            let shown = preview(Some(&dialect_of(driver)), &sql);
            assert!(shown.starts_with("DELETE FROM users"), "{driver}: {shown}");
            assert!(!shown.contains("SELECT"), "{driver}: {shown}");
            assert!(
                shown.contains(&format!("of {} characters in all", sql.chars().count())),
                "{driver}: {shown}"
            );
        }
    }

    #[test]
    fn writes_that_do_not_all_fit_are_cut_and_counted() {
        let write = format!("DELETE FROM users WHERE name = '{}'", "x".repeat(200));
        let sql = format!("SELECT 1;\n{}", [write.as_str(); 5].join(";\n"));
        let shown = preview(Some(&dialect_of("postgres")), &sql);
        assert!(shown.starts_with(&write), "the first is whole: {shown}");
        assert!(shown.contains("more characters)"), "one is cut: {shown}");
        assert!(
            shown.contains("… and 2 more statements that change data"),
            "the rest are counted: {shown}"
        );
        assert!(!shown.contains("SELECT"), "{shown}");
    }

    #[test]
    fn a_statement_that_fits_a_dialog_is_shown_whole_with_its_reads() {
        let sql = "SELECT 1; DELETE FROM users";
        assert_eq!(preview(Some(&dialect_of("sqlite")), sql), sql);
        assert_eq!(preview(None, "  DELETE FROM t \n"), "DELETE FROM t");
    }

    /// With no dialect to find the writes with, or no write found in text that
    /// long, the text is cut from its start and says how much is left.
    #[test]
    fn a_long_text_with_nothing_to_pick_out_is_cut_from_its_start() {
        let long = "x".repeat(PREVIEW_CHARS + 50);
        let shown = preview(None, &long);
        assert_eq!(
            shown,
            format!("{}… (50 more characters)", "x".repeat(PREVIEW_CHARS))
        );
    }

    #[test]
    fn a_team_and_a_project_connection_of_one_name_keep_separate_levels() {
        let team = override_key("prod", NodeOrigin::Team);
        let project = override_key("prod", NodeOrigin::Project);
        assert_ne!(team, project);
        let mut overrides = HashMap::default();
        overrides.insert(team.clone(), SafetyLevel::Unrestricted);
        assert_eq!(overrides.get(&project), None);
        assert_eq!(overrides.get(&team), Some(&SafetyLevel::Unrestricted));
        // The origin leads, so a name cannot spell another origin's key.
        assert_ne!(
            override_key("team:prod", NodeOrigin::Project),
            override_key("prod", NodeOrigin::Team)
        );
    }

    /// Every driver refuses writes today, so raising the level cannot be
    /// promised to make one work: it only lets the editor send it.
    #[test]
    fn a_refusal_does_not_promise_the_write_will_then_succeed() {
        let message = refusal_message("prod");
        assert!(
            message.contains("prod") && message.contains("Read Only"),
            "{message}"
        );
        assert!(!message.contains("to run it"), "{message}");
        assert!(message.contains("may still refuse"), "{message}");
    }

    #[test]
    fn a_long_statement_is_cut_for_the_dialog_on_a_character_boundary() {
        let long = "é".repeat(PREVIEW_CHARS + 50);
        let shown = preview(Some(&dialect_of("sqlite")), &long);
        assert_eq!(
            shown.lines().next(),
            Some(format!("{}… (50 more characters)", "é".repeat(PREVIEW_CHARS)).as_str())
        );
        assert_eq!(truncated("ééé", 2), "éé… (1 more characters)");
    }
}
