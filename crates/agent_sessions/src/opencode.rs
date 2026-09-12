use crate::{
    AgentCommand, AgentKind, Availability, Deletion, Fork, SessionCounts, SessionProvider,
    SessionSummary, Speaker, summary::millis_to_time,
};
use anyhow::{Context as _, Result};
use rusqlite::{Connection, OpenFlags};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// opencode keeps every session, message and part in one sqlite database at a
/// fixed name -- unlike Codex's `state_<schema>.sqlite`, there is no version
/// number to pick the newest of.
const DB_NAME: &str = "opencode.db";

/// `time_archived is not null` rows are sessions the user put away; `parent_id
/// is not null` rows are subagent invocations, not conversations a person had
/// -- both are load-bearing (Key Insight 3). Ordered newest-updated first, the
/// order the panel groups on.
const LIST_QUERY: &str = "select id, title, directory, model, time_updated \
     from session where time_archived is null and parent_id is null \
     order by time_updated desc";

/// Deliberately without either of `LIST_QUERY`'s filters, matching the same
/// call in `codex.rs`: `find` answers "does this id still name a session the
/// CLI could resume", and neither archiving nor being a subagent takes that
/// away. A tab may legitimately be sitting on either kind of row.
const FIND_QUERY: &str =
    "select id, title, directory, model, time_updated from session where id = ?1 limit 1";

/// One message and the one part of it that holds the preview text, joined so
/// the speaker (`message.data.role`) and the text (`part.data.text`) can never
/// come from two different rows -- see the module comment on
/// [`parse_preview`] for why that matters. `like` is a prefix narrowing, not
/// the parse: the candidate is handed to `serde_json` afterwards, and a shape
/// that does not hold yields no preview rather than a wrong one.
const PREVIEW_QUERY: &str = "select m.data, p.data from message m \
     join part p on p.message_id = m.id \
     where m.session_id = ?1 and p.data like '{\"type\":\"text\"%' \
     order by m.time_created desc, m.id desc, p.id asc limit 1";

const MESSAGE_COUNT_QUERY: &str = "select count(*) from message where session_id = ?1";
const SUBAGENT_COUNT_QUERY: &str = "select count(*) from session where parent_id = ?1";

/// Read-only access to `~/.local/share/opencode/opencode.db`.
///
/// This database belongs to a program that may be running and writing to it
/// right now. Every open is `SQLITE_OPEN_READ_ONLY`, which makes "we do not
/// write here" a guarantee from sqlite rather than a promise from this
/// module -- opencode is the one store this editor cannot delete from
/// directly, and that is the whole reason [`Deletion::Command`] exists.
pub struct OpenCodeProvider {
    opencode_dir: PathBuf,
}

impl OpenCodeProvider {
    pub fn new(opencode_dir: PathBuf) -> Self {
        Self { opencode_dir }
    }

    /// `~/.local/share/opencode`. `XDG_DATA_HOME` is deliberately not
    /// consulted: Claude, Codex and Copilot each hardcode one root of their
    /// own, and honouring one environment variable for the fourth provider
    /// only is inconsistency, not flexibility. If it is ever wanted, this is a
    /// one-line change with a test -- recorded here so the question is not
    /// re-opened from scratch.
    pub fn default_root() -> PathBuf {
        util::paths::home_dir()
            .join(".local")
            .join("share")
            .join("opencode")
    }

    fn database_path(&self) -> PathBuf {
        self.opencode_dir.join(DB_NAME)
    }

    /// Runs `read` against the session store.
    ///
    /// No snapshot-copy fallback, unlike Codex's: the live store on the
    /// machine this was verified against is 52 MB after seven sessions, and
    /// `list` runs on every sweep -- worse, `availability` runs on every
    /// render of the filter box (H10), so a fallback that copies the file on
    /// failure would put a 52 MB copy on the keystroke path. A read-only open
    /// failing here degrades straight to `Unavailable`, which is what the
    /// sealed design calls for and what `list`/`find`'s callers already treat
    /// as a legitimate state rather than a crash.
    fn with_connection<T>(&self, read: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let path = self.database_path();
        let connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("{} is not readable", path.display()))?;
        read(&connection)
    }
}

impl SessionProvider for OpenCodeProvider {
    fn agent(&self) -> AgentKind {
        AgentKind::OpenCode
    }

    fn availability(&self) -> Availability {
        let path = self.database_path();
        if !path.is_file() {
            return Availability::Unavailable(format!(
                "no {DB_NAME} in {}",
                self.opencode_dir.display()
            ));
        }
        let opened = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY);
        let connection = match opened {
            Ok(connection) => connection,
            Err(error) => {
                return Availability::Unavailable(format!(
                    "{} is not readable: {error}",
                    path.display()
                ));
            }
        };
        // Prepare the real query: it names every column the list needs, so a
        // schema that lost one -- opencode's own `__drizzle_migrations` says
        // the shape moves -- is caught here rather than row by row later.
        match connection.prepare(LIST_QUERY) {
            Ok(_) => Availability::Ready,
            Err(error) => Availability::Unavailable(format!(
                "{} is not an opencode session store: {error}",
                path.display()
            )),
        }
    }

    fn list(&self) -> Result<Vec<SessionSummary>> {
        let rows = self.with_connection(|connection| {
            let mut list_statement = connection.prepare(LIST_QUERY)?;
            let mut preview_statement = connection.prepare(PREVIEW_QUERY)?;
            let rows = list_statement
                .query_map([], Row::from_sqlite)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut summaries = Vec::with_capacity(rows.len());
            for row in rows {
                let preview = fetch_preview(&mut preview_statement, &row.id)?;
                if let Some(summary) = row.into_summary(preview) {
                    summaries.push(summary);
                }
            }
            Ok(summaries)
        });
        // An unreadable or absent store is a state, not a failure: whoever has
        // never run opencode should still see their other agents' sessions.
        match rows {
            Ok(rows) => Ok(rows),
            Err(error) => {
                log::warn!("opencode session store unreadable: {error}");
                Ok(Vec::new())
            }
        }
    }

    /// One indexed lookup by primary key, plus the same preview lookup `list`
    /// does for whichever single row this finds -- `find` still has to hand
    /// back a complete `SessionSummary`, and a caller that resumed through
    /// `find` deserves the same preview a sweep would have shown it.
    fn find(&self, id: &str) -> Result<Option<SessionSummary>> {
        let id = id.to_string();
        let found = self.with_connection(move |connection| {
            let mut statement = connection.prepare(FIND_QUERY)?;
            let mut rows = statement.query_map([&id], Row::from_sqlite)?;
            let Some(row) = rows.next().transpose()? else {
                return Ok(None);
            };
            let mut preview_statement = connection.prepare(PREVIEW_QUERY)?;
            let preview = fetch_preview(&mut preview_statement, &row.id)?;
            Ok(Some((row, preview)))
        });
        match found {
            Ok(found) => Ok(found.and_then(|(row, preview)| row.into_summary(preview))),
            Err(error) => {
                log::warn!("opencode session store unreadable: {error}");
                Ok(None)
            }
        }
    }

    fn new_session_command(&self, _id: &str, _cwd: &Path) -> Option<AgentCommand> {
        // `-s` continues a session that already exists; there is no flag to
        // start a fresh one under an id of our choosing. Same reasoning Codex
        // records for its own `resume`: an id the CLI never agreed to is an
        // id that will not be there to resume.
        None
    }

    fn counts(&self, session: &SessionSummary) -> Result<SessionCounts> {
        let id = session.id.to_string();
        let counts = self.with_connection(move |connection| {
            let messages: i64 =
                connection.query_row(MESSAGE_COUNT_QUERY, [&id], |row| row.get(0))?;
            let subagents: i64 =
                connection.query_row(SUBAGENT_COUNT_QUERY, [&id], |row| row.get(0))?;
            Ok((messages, subagents))
        });
        match counts {
            Ok((messages, subagents)) => Ok(SessionCounts {
                messages: Some(messages.max(0) as usize),
                subagents: subagents.max(0) as usize,
            }),
            Err(error) => {
                // `SessionCounts::default()` is `messages: None`, never
                // `Some(0)` -- the doc comment on that struct forbids using
                // zero as a stand-in for "cannot answer", which would read as
                // an empty conversation rather than an unreadable store.
                log::warn!("opencode session store unreadable: {error}");
                Ok(SessionCounts::default())
            }
        }
    }

    fn resume_command(&self, session: &SessionSummary, fork: Fork) -> Option<AgentCommand> {
        let mut args = vec!["-s".to_string(), session.id.to_string()];
        if fork == Fork::New {
            args.push("--fork".to_string());
        }
        Some(AgentCommand {
            program: "opencode".to_string(),
            args,
            cwd: session.cwd.clone(),
        })
    }

    fn deletion(&self, session: &SessionSummary) -> Deletion {
        // No `--` before the id, despite an earlier draft of this design
        // calling for one as a second, belt-and-braces control (H1).
        // Verified against the installed CLI (1.14.20): a `--` placed
        // anywhere before the id does not protect it, it breaks the command.
        // `opencode session delete -- <id>` never reaches "Session not
        // found" -- it prints `session delete`'s own usage and exits 1,
        // because yargs treats everything after `--` as raw, unparsed
        // values and never binds the required `sessionID` positional from
        // them. A `--` placed before `session` is worse: `opencode -- session
        // delete <id>` stops subcommand matching outright and falls through
        // to the default `opencode [project]` action, launching the
        // interactive TUI as a detached child process -- exactly the hang
        // M6's timeout exists to survive, except every delete would take
        // that path, not just a wedged one.
        //
        // The `--` was defence in depth for a leading-dash id being read as
        // a flag. `is_valid_session_id` below is the actual guard, anchored
        // to `ses_` plus alphanumerics only, so nothing it lets through can
        // ever start with `-`. There is nothing left for a `--` to defend
        // against here, and on this CLI it does not defend -- it breaks the
        // command outright.
        Deletion::Command(AgentCommand {
            program: "opencode".to_string(),
            args: vec![
                "session".to_string(),
                "delete".to_string(),
                session.id.to_string(),
            ],
            cwd: session.cwd.clone(),
        })
    }
}

/// H1 -- verified against the live store: every session id is `ses_`-prefixed,
/// 7 of 7, zero exceptions. Anchored to that shape rather than to a permissive
/// character class: `[A-Za-z0-9_-]` would admit `id = "--force"`, which is
/// exactly the value opencode's own argument parser would read as an option
/// rather than a positional.
///
/// This is a shape check and defence in depth, not an injection guard --
/// `deletion` and `resume_command` spawn through a real argv
/// (`util::command::Command::args`), with no shell in between, so there is
/// nowhere for the id to be interpreted except as opencode's own CLI reads
/// it. It is checked here, once, at the one place a [`SessionSummary`] comes
/// into being (H2): `resume_command` and `deletion` both build a command from
/// `session.id` with no check of their own, and `agent_task` drops a resumed
/// session's args straight into `SpawnInTerminal`, so validating anywhere
/// other than construction would mean re-proving the guarantee at every call
/// site rather than once.
fn is_valid_session_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("ses_") else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric())
}

/// One `session` row before it becomes a [`SessionSummary`] or is dropped.
///
/// The one place column order is written down: `LIST_QUERY` and `FIND_QUERY`
/// select the same columns in the same order, and writing the mapping out
/// once per query would misparse silently the day one of them gained a
/// column -- every field would still have a type that fits, just the wrong
/// value in it. Matches the idiom `codex.rs` uses for the same reason.
struct Row {
    id: String,
    title: String,
    directory: String,
    model: Option<String>,
    time_updated: i64,
}

impl Row {
    fn from_sqlite(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            title: row.get(1)?,
            directory: row.get(2)?,
            model: row.get(3)?,
            time_updated: row.get(4)?,
        })
    }

    /// `preview` is whatever [`fetch_preview`] found for this row's id --
    /// `None` when the session has no text part to show, which still lists,
    /// just with an empty preview.
    ///
    /// Returns `None` -- dropping the row from the listing entirely -- when
    /// the id fails [`is_valid_session_id`] (H1/H2). A row that fails the
    /// shape check is unusable for every operation the panel offers: resume,
    /// fork and delete all build a command from `session.id`, so listing it
    /// would only produce a row whose every control fails. The id's contents
    /// are never logged, only the fact that the shape was wrong.
    fn into_summary(self, preview: Option<(Option<Speaker>, String)>) -> Option<SessionSummary> {
        if !is_valid_session_id(&self.id) {
            log::warn!(
                "dropping an opencode session whose id is not shaped like ^ses_[A-Za-z0-9]+$"
            );
            return None;
        }
        let title = non_empty(self.title).unwrap_or_else(|| self.id.clone());
        let (preview_speaker, preview_text) = match preview {
            Some((speaker, text)) => (speaker, text),
            None => (None, String::new()),
        };
        Some(SessionSummary {
            id: Arc::from(self.id.as_str()),
            agent: AgentKind::OpenCode,
            title,
            preview: preview_text,
            preview_speaker,
            cwd: PathBuf::from(self.directory),
            // opencode records no branch of its own.
            branch: None,
            model: self.model.filter(|model| !model.is_empty()),
            updated_at: millis_to_time(self.time_updated),
            // No transcript file -- everything opencode knows about a
            // session lives in the database, not on disk beside it.
            log_path: None,
            log_bytes: 0,
        })
    }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Runs [`PREVIEW_QUERY`] for one session id and parses whatever it finds.
fn fetch_preview(
    statement: &mut rusqlite::Statement,
    id: &str,
) -> rusqlite::Result<Option<(Option<Speaker>, String)>> {
    let mut rows = statement.query_map([id], |row| {
        let message_data: String = row.get(0)?;
        let part_data: String = row.get(1)?;
        Ok((message_data, part_data))
    })?;
    let Some(found) = rows.next().transpose()? else {
        return Ok(None);
    };
    Ok(parse_preview(&found.0, &found.1))
}

/// Turns one message's `data` and one of its parts' `data` into a speaker and
/// a preview line.
///
/// The two always come from the same message -- `PREVIEW_QUERY` joins them on
/// `part.message_id = message.id` in one row -- which is what makes it safe
/// to read the role from one JSON blob and the text from the other. Reading
/// "the newest text part" and separately "the newest message's role" would
/// eventually disagree: a store whose newest message is the assistant's but
/// whose newest *text part* belongs to an older user message would then show
/// the user's words labelled as the agent's.
///
/// Each field degrades on its own, matching this crate's rule that an
/// unrecognised shape costs a field, never the row: a message whose `data`
/// does not parse, or whose `role` is neither `"user"` nor `"assistant"`,
/// still yields the preview text with no speaker, rather than nothing at
/// all. Only a missing or blank part `text` drops the preview entirely --
/// there is nothing left to show.
fn parse_preview(message_data: &str, part_data: &str) -> Option<(Option<Speaker>, String)> {
    let part: serde_json::Value = serde_json::from_str(part_data).ok()?;
    let text = part.get("text").and_then(serde_json::Value::as_str)?;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let speaker = serde_json::from_str::<serde_json::Value>(message_data)
        .ok()
        .and_then(|message| {
            message
                .get("role")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .and_then(|role| match role.as_str() {
            "user" => Some(Speaker::User),
            "assistant" => Some(Speaker::Agent),
            _ => None,
        });
    Some((speaker, text.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    /// The schema is the one read from the live store on the author's
    /// machine (opencode 1.14.20), trimmed to the columns this provider
    /// reads. `__drizzle_migrations` is left out on purpose -- its presence
    /// or absence never matters to any query here, only the shape of
    /// `session`/`message`/`part` does.
    fn session_store(dir: &Path) -> Connection {
        let connection = Connection::open(dir.join(DB_NAME)).unwrap();
        connection
            .execute_batch(
                "create table session (
                    id text primary key, project_id text not null, parent_id text,
                    slug text not null, directory text not null, title text not null,
                    version text not null, share_url text,
                    time_created integer not null, time_updated integer not null,
                    time_compacting integer, time_archived integer,
                    workspace_id text, path text, agent text, model text
                 );
                 create index session_parent_idx on session (parent_id);

                 create table message (
                    id text primary key, session_id text not null,
                    time_created integer not null, time_updated integer not null,
                    data text not null
                 );
                 create index message_session_time_created_id_idx
                    on message (session_id, time_created, id);

                 create table part (
                    id text primary key, message_id text not null, session_id text not null,
                    time_created integer not null, data text not null
                 );
                 create index part_session_idx on part (session_id);
                 create index part_message_id_id_idx on part (message_id, id);",
            )
            .unwrap();
        connection
    }

    fn insert_session(
        connection: &Connection,
        id: &str,
        directory: &str,
        title: &str,
        time_updated: i64,
        parent_id: Option<&str>,
        time_archived: Option<i64>,
    ) {
        connection
            .execute(
                "insert into session
                    (id, project_id, parent_id, slug, directory, title, version,
                     time_created, time_updated, time_archived)
                 values (?1, 'proj', ?2, 'slug', ?3, ?4, '1.14.20', ?5, ?5, ?6)",
                rusqlite::params![id, parent_id, directory, title, time_updated, time_archived],
            )
            .unwrap();
    }

    fn insert_message(
        connection: &Connection,
        id: &str,
        session_id: &str,
        time_created: i64,
        role: &str,
    ) {
        let data = format!(r#"{{"role":"{role}"}}"#);
        connection
            .execute(
                "insert into message (id, session_id, time_created, time_updated, data)
                 values (?1, ?2, ?3, ?3, ?4)",
                rusqlite::params![id, session_id, time_created, data],
            )
            .unwrap();
    }

    fn insert_text_part(
        connection: &Connection,
        id: &str,
        message_id: &str,
        session_id: &str,
        time_created: i64,
        text: &str,
    ) {
        let data = serde_json::json!({"type": "text", "text": text}).to_string();
        connection
            .execute(
                "insert into part (id, message_id, session_id, time_created, data)
                 values (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![id, message_id, session_id, time_created, data],
            )
            .unwrap();
    }

    #[test]
    fn a_missing_store_is_a_state_and_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let provider = OpenCodeProvider::new(dir.path().to_path_buf());
        assert!(matches!(
            provider.availability(),
            Availability::Unavailable(_)
        ));
        assert!(provider.list().unwrap().is_empty());
    }

    #[test]
    fn a_file_that_is_not_sqlite_is_unavailable_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(DB_NAME), b"not a database").unwrap();
        let provider = OpenCodeProvider::new(dir.path().to_path_buf());
        assert!(matches!(
            provider.availability(),
            Availability::Unavailable(_)
        ));
        assert!(provider.list().unwrap().is_empty());
    }

    #[test]
    fn a_store_whose_session_table_lost_a_column_is_unavailable_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let connection = Connection::open(dir.path().join(DB_NAME)).unwrap();
        connection
            .execute("create table session (id text primary key)", [])
            .unwrap();
        drop(connection);

        let provider = OpenCodeProvider::new(dir.path().to_path_buf());
        assert!(matches!(
            provider.availability(),
            Availability::Unavailable(_)
        ));
        assert!(provider.list().unwrap().is_empty());
    }

    #[test]
    fn a_well_formed_store_lists_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let connection = session_store(dir.path());
        insert_session(&connection, "ses_older", "/w/one", "first", 100, None, None);
        insert_session(
            &connection,
            "ses_newer",
            "/w/two",
            "second",
            200,
            None,
            None,
        );
        drop(connection);

        let provider = OpenCodeProvider::new(dir.path().to_path_buf());
        assert!(provider.availability().is_ready());
        let sessions = provider.list().unwrap();
        assert_eq!(
            sessions
                .iter()
                .map(|s| s.id.to_string())
                .collect::<Vec<_>>(),
            vec!["ses_newer", "ses_older"]
        );
        assert_eq!(sessions[0].cwd, PathBuf::from("/w/two"));
        assert_eq!(sessions[0].agent, AgentKind::OpenCode);
        assert_eq!(sessions[0].branch, None);
        assert_eq!(sessions[0].log_path, None);
        assert_eq!(sessions[0].log_bytes, 0);
    }

    #[test]
    fn an_archived_session_does_not_list() {
        let dir = tempfile::tempdir().unwrap();
        let connection = session_store(dir.path());
        insert_session(
            &connection,
            "ses_filed",
            "/w/one",
            "filed away",
            100,
            None,
            Some(50),
        );
        drop(connection);

        let provider = OpenCodeProvider::new(dir.path().to_path_buf());
        assert!(provider.list().unwrap().is_empty());
    }

    #[test]
    fn a_child_session_does_not_list_and_counts_as_a_subagent() {
        let dir = tempfile::tempdir().unwrap();
        let connection = session_store(dir.path());
        insert_session(
            &connection,
            "ses_parent",
            "/w/one",
            "top level",
            100,
            None,
            None,
        );
        insert_session(
            &connection,
            "ses_child",
            "/w/one",
            "a subagent run",
            150,
            Some("ses_parent"),
            None,
        );
        drop(connection);

        let provider = OpenCodeProvider::new(dir.path().to_path_buf());
        let sessions = provider.list().unwrap();
        assert_eq!(
            sessions.len(),
            1,
            "the child must not appear as its own row"
        );
        assert_eq!(&*sessions[0].id, "ses_parent");

        let counts = provider.counts(&sessions[0]).unwrap();
        assert_eq!(counts.subagents, 1);
    }

    #[test]
    fn updated_at_is_read_as_milliseconds() {
        let dir = tempfile::tempdir().unwrap();
        let connection = session_store(dir.path());
        insert_session(
            &connection,
            "ses_time",
            "/w/one",
            "t",
            1_789_211_281_497,
            None,
            None,
        );
        drop(connection);

        let provider = OpenCodeProvider::new(dir.path().to_path_buf());
        let sessions = provider.list().unwrap();
        assert_eq!(
            sessions[0].updated_at,
            UNIX_EPOCH + Duration::from_millis(1_789_211_281_497)
        );
    }

    /// The test the naive version fails: the newest *message* is the
    /// assistant's, but the newest *text part* sits on an older user message.
    /// Reading the role and the text from separately-chosen rows would call
    /// this `Speaker::User`; reading both from one joined row calls it
    /// correctly.
    #[test]
    fn preview_speaker_and_text_come_from_the_same_message() {
        let dir = tempfile::tempdir().unwrap();
        let connection = session_store(dir.path());
        insert_session(&connection, "ses_conv", "/w/one", "t", 300, None, None);
        insert_message(&connection, "msg_user", "ses_conv", 100, "user");
        insert_text_part(
            &connection,
            "part_user",
            "msg_user",
            "ses_conv",
            100,
            "an older user message",
        );
        insert_message(&connection, "msg_assistant", "ses_conv", 200, "assistant");
        insert_text_part(
            &connection,
            "part_assistant",
            "msg_assistant",
            "ses_conv",
            200,
            "the newest reply",
        );
        drop(connection);

        let provider = OpenCodeProvider::new(dir.path().to_path_buf());
        let sessions = provider.list().unwrap();
        assert_eq!(sessions[0].preview, "the newest reply");
        assert_eq!(
            sessions[0].preview_speaker,
            Some(Speaker::Agent),
            "the newest message is the assistant's; taking speaker and text from \
             different rows would have called this Speaker::User instead"
        );
    }

    #[test]
    fn a_message_with_no_text_part_yields_an_empty_preview_and_still_lists() {
        let dir = tempfile::tempdir().unwrap();
        let connection = session_store(dir.path());
        insert_session(&connection, "ses_quiet", "/w/one", "t", 100, None, None);
        insert_message(&connection, "msg_1", "ses_quiet", 100, "user");
        // A tool-call part, not a text one -- `PREVIEW_QUERY`'s `like` filter
        // must not match it.
        connection
            .execute(
                "insert into part (id, message_id, session_id, time_created, data)
                 values ('part_1', 'msg_1', 'ses_quiet', 100, '{\"type\":\"tool\"}')",
                [],
            )
            .unwrap();
        drop(connection);

        let provider = OpenCodeProvider::new(dir.path().to_path_buf());
        let sessions = provider.list().unwrap();
        assert_eq!(sessions.len(), 1, "a quiet session still lists");
        assert_eq!(sessions[0].preview, "");
        assert_eq!(sessions[0].preview_speaker, None);
    }

    #[test]
    fn counts_returns_the_exact_message_count() {
        let dir = tempfile::tempdir().unwrap();
        let connection = session_store(dir.path());
        insert_session(&connection, "ses_conv", "/w/one", "t", 100, None, None);
        for n in 0..5 {
            insert_message(
                &connection,
                &format!("msg_{n}"),
                "ses_conv",
                100 + n,
                "user",
            );
        }
        drop(connection);

        let provider = OpenCodeProvider::new(dir.path().to_path_buf());
        let sessions = provider.list().unwrap();
        let counts = provider.counts(&sessions[0]).unwrap();
        assert_eq!(counts.messages, Some(5));
        assert_eq!(counts.subagents, 0);
    }

    #[test]
    fn find_answers_yes_for_a_held_id_and_no_for_an_absent_one() {
        let dir = tempfile::tempdir().unwrap();
        let connection = session_store(dir.path());
        insert_session(&connection, "ses_here", "/w/two", "t", 100, None, None);
        drop(connection);

        let provider = OpenCodeProvider::new(dir.path().to_path_buf());
        let found = provider.find("ses_here").unwrap().expect("held");
        assert_eq!(&*found.id, "ses_here");
        assert_eq!(found.cwd, PathBuf::from("/w/two"));
        assert!(provider.find("ses_absent").unwrap().is_none());
    }

    /// `find` deliberately ignores both of `LIST_QUERY`'s filters -- archiving
    /// or being a subagent does not make a tab's own session stop existing.
    #[test]
    fn find_sees_archived_and_child_sessions_that_list_hides() {
        let dir = tempfile::tempdir().unwrap();
        let connection = session_store(dir.path());
        insert_session(&connection, "ses_filed", "/w/one", "t", 100, None, Some(1));
        insert_session(&connection, "ses_parent", "/w/one", "t", 100, None, None);
        insert_session(
            &connection,
            "ses_child",
            "/w/one",
            "t",
            100,
            Some("ses_parent"),
            None,
        );
        drop(connection);

        let provider = OpenCodeProvider::new(dir.path().to_path_buf());
        assert!(provider.find("ses_filed").unwrap().is_some());
        assert!(provider.find("ses_child").unwrap().is_some());
    }

    fn bare_session(id: &str, cwd: &str) -> SessionSummary {
        SessionSummary {
            id: Arc::from(id),
            agent: AgentKind::OpenCode,
            title: String::new(),
            preview: String::new(),
            preview_speaker: None,
            cwd: PathBuf::from(cwd),
            branch: None,
            model: None,
            updated_at: UNIX_EPOCH,
            log_path: None,
            log_bytes: 0,
        }
    }

    #[test]
    fn resume_is_dash_s_and_fork_adds_dash_dash_fork() {
        let provider = OpenCodeProvider::new(PathBuf::from("/does/not/exist"));
        let session = bare_session("ses_abc", "/w/one");

        let resume = provider
            .resume_command(&session, Fork::Continue)
            .expect("continue is supported");
        assert_eq!(resume.program, "opencode");
        assert_eq!(resume.args, vec!["-s".to_string(), "ses_abc".to_string()]);
        assert_eq!(resume.cwd, PathBuf::from("/w/one"));

        let fork = provider
            .resume_command(&session, Fork::New)
            .expect("opencode is the second agent, after Claude, that can honour Fork::New");
        assert_eq!(
            fork.args,
            vec![
                "-s".to_string(),
                "ses_abc".to_string(),
                "--fork".to_string()
            ]
        );
    }

    #[test]
    fn opencode_cannot_be_told_which_id_to_start_under() {
        let provider = OpenCodeProvider::new(PathBuf::from("/does/not/exist"));
        assert!(
            provider
                .new_session_command("some-id", Path::new("/w/one"))
                .is_none()
        );
    }

    #[test]
    fn deletion_is_a_command_with_no_dash_dash_before_the_id() {
        let provider = OpenCodeProvider::new(PathBuf::from("/does/not/exist"));
        let session = bare_session("ses_abc", "/w/one");
        let Deletion::Command(command) = provider.deletion(&session) else {
            panic!("opencode has no files of its own to trash");
        };
        assert_eq!(command.program, "opencode");
        assert_eq!(
            command.args,
            vec![
                "session".to_string(),
                "delete".to_string(),
                "ses_abc".to_string()
            ],
            "verified against the real CLI: a `--` anywhere before the id \
             breaks dispatch instead of protecting it -- see the comment on \
             `deletion` for the transcript"
        );
        assert_eq!(command.cwd, PathBuf::from("/w/one"));
    }

    /// H1/H2 -- a row shaped like an attempted flag injection never becomes a
    /// `SessionSummary` at all, so it cannot reach `resume_command` or
    /// `deletion` either: both take a `&SessionSummary`, and this row never
    /// produces one.
    #[test]
    fn a_row_whose_id_is_not_shaped_like_a_session_id_is_dropped_from_the_listing() {
        let dir = tempfile::tempdir().unwrap();
        let connection = session_store(dir.path());
        insert_session(&connection, "--force", "/w/one", "t", 100, None, None);
        insert_session(&connection, "ses_ok", "/w/one", "t", 50, None, None);
        drop(connection);

        let provider = OpenCodeProvider::new(dir.path().to_path_buf());
        let sessions = provider.list().unwrap();
        assert_eq!(
            sessions
                .iter()
                .map(|s| s.id.to_string())
                .collect::<Vec<_>>(),
            vec!["ses_ok"],
            "the `--force`-shaped id must never reach a SessionSummary"
        );
    }

    #[test]
    fn is_valid_session_id_rejects_everything_but_the_verified_shape() {
        assert!(is_valid_session_id("ses_1e8dbbecfffeD7mchzalwXUIL1"));
        assert!(!is_valid_session_id("--force"));
        assert!(!is_valid_session_id("ses_"));
        assert!(!is_valid_session_id("ses_has-a-dash"));
        assert!(!is_valid_session_id(""));
        assert!(!is_valid_session_id("not_ses_prefixed"));
    }

    /// M5 -- the gate is a number, not a feeling. 500 sessions and roughly
    /// 50,000 messages (100 each), each with one text part, is enough to make
    /// an accidental full scan felt at a scale the bound below would catch;
    /// the bound itself stays generous on purpose so it fails on a real
    /// regression, not on machine noise.
    #[test]
    fn list_over_500_sessions_completes_well_under_the_measured_bound() {
        let dir = tempfile::tempdir().unwrap();
        let connection = session_store(dir.path());
        connection.execute_batch("begin;").unwrap();
        for session_index in 0..500 {
            let session_id = format!("ses_perf{session_index:04}");
            insert_session(
                &connection,
                &session_id,
                "/w/perf",
                "performance fixture",
                session_index as i64,
                None,
                None,
            );
            for message_index in 0..100 {
                let message_id = format!("msg_{session_index}_{message_index}");
                let role = if message_index % 2 == 0 {
                    "user"
                } else {
                    "assistant"
                };
                insert_message(
                    &connection,
                    &message_id,
                    &session_id,
                    message_index as i64,
                    role,
                );
                let part_id = format!("part_{session_index}_{message_index}");
                insert_text_part(
                    &connection,
                    &part_id,
                    &message_id,
                    &session_id,
                    message_index as i64,
                    "hello",
                );
            }
        }
        connection.execute_batch("commit;").unwrap();
        drop(connection);

        let provider = OpenCodeProvider::new(dir.path().to_path_buf());
        let started = std::time::Instant::now();
        let sessions = provider.list().unwrap();
        let elapsed = started.elapsed();
        assert_eq!(sessions.len(), 500);
        assert!(
            elapsed < Duration::from_millis(250),
            "list() over 500 sessions took {elapsed:?}, expected under 250ms; an \
             accidental full-table scan would miss this bound by orders of \
             magnitude, not by a little"
        );
    }

    /// M7 -- everything this module believes about opencode's CLI comes from
    /// one probe of 1.14.20, and the test matrix forbids reading the real
    /// store in an ordinary test. This is the re-verification hook: run
    /// deliberately after an opencode upgrade, against the real binary, on a
    /// bogus id that cannot name a real session.
    #[test]
    #[ignore = "spawns the real opencode CLI; run by hand after an opencode upgrade"]
    fn reading_a_live_opencode_delete_of_a_bogus_id() {
        // `util::command::Command` rather than `std::process::Command`: the
        // workspace's clippy config forbids the blocking std form outright
        // (it can stall the thread it runs on for an unknown duration), so
        // even a one-off `#[ignore]`d test goes through the async wrapper --
        // driven here with `smol::block_on` since this is an ordinary test,
        // not an async one.
        let mut command = util::command::Command::new("opencode");
        command.args(["session", "delete", "ses_bogus0000000000000000"]);
        let output =
            smol::block_on(command.output()).expect("opencode must be on PATH for this test");
        assert_eq!(
            output.status.code(),
            Some(1),
            "a bogus id must fail, not succeed silently"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("not found") || stderr.contains("Not found"),
            "expected a not-found shape in stderr, got: {stderr}"
        );
    }
}
