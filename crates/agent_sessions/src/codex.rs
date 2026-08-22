use crate::{
    AgentKind, Availability, Fork, ResumeCommand, SessionCounts, SessionProvider, SessionSummary,
};
use anyhow::{Context as _, Result};
use rusqlite::{Connection, OpenFlags};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Codex's threads live in a sqlite database whose **filename carries the schema
/// version** — `state_5.sqlite` today, `state_6.sqlite` after the next migration.
/// Hardcoding the name would fail silently on a Codex update, so the highest
/// numbered file wins.
const DB_PREFIX: &str = "state_";
const DB_SUFFIX: &str = ".sqlite";

const LIST_QUERY: &str = "select id, title, preview, first_user_message, cwd, git_branch, model, \
     coalesce(recency_at_ms, updated_at_ms, updated_at * 1000) as at_ms, rollout_path \
     from threads where archived = 0 order by at_ms desc";

/// Read-only access to `~/.codex/state_*.sqlite`.
///
/// This database belongs to another program that may be running. Every open is
/// `SQLITE_OPEN_READ_ONLY`, which makes "we do not write here" a guarantee from
/// sqlite rather than a promise from this module.
pub struct CodexProvider {
    codex_dir: PathBuf,
}

impl CodexProvider {
    pub fn new(codex_dir: PathBuf) -> Self {
        Self { codex_dir }
    }

    pub fn default_root() -> PathBuf {
        util::paths::home_dir().join(".codex")
    }

    /// The state database with the highest schema number.
    ///
    /// Compared numerically, not as strings: `state_10` is newer than `state_9`,
    /// which a lexical sort gets backwards.
    fn database_path(&self) -> Option<PathBuf> {
        let mut best: Option<(u32, PathBuf)> = None;
        for entry in std::fs::read_dir(&self.codex_dir).ok()?.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(version) = name
                .strip_prefix(DB_PREFIX)
                .and_then(|rest| rest.strip_suffix(DB_SUFFIX))
                .and_then(|digits| digits.parse::<u32>().ok())
            else {
                continue;
            };
            if best.as_ref().is_none_or(|(found, _)| version > *found) {
                best = Some((version, entry.path()));
            }
        }
        best.map(|(_, path)| path)
    }

    /// Whether `path` really resolves inside the Codex directory.
    ///
    /// Both sides are canonicalised first, because `Path::starts_with` is a
    /// component comparison that never touches the filesystem: it says yes to
    /// `~/.codex/../../etc/passwd`, and yes to a symlink under `~/.codex` pointing
    /// anywhere at all. This guard exists to keep a doctored database row from
    /// becoming a delete somewhere else, and a lexical check does not do that.
    ///
    /// A path that cannot be canonicalised — it is gone, or a dangling symlink —
    /// answers `false`: there is nothing there to trash anyway.
    fn contains(&self, path: &Path) -> bool {
        let Ok(root) = std::fs::canonicalize(&self.codex_dir) else {
            return false;
        };
        let Ok(path) = std::fs::canonicalize(path) else {
            return false;
        };
        path.starts_with(root)
    }

    /// Runs `read` against the thread store.
    ///
    /// Scoped as a closure rather than handing back a connection so the snapshot
    /// copy — which contains the user's own conversation titles — is deleted the
    /// moment the read is done, not left in the temp directory.
    fn with_connection<T>(&self, read: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let path = self
            .database_path()
            .context("no state_*.sqlite in the Codex directory")?;
        match Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(connection) => read(&connection),
            // A read-only open of a database with a hot WAL fails when the WAL
            // needs recovery, because recovery wants to write. A snapshot copy is
            // the only way through without touching Codex's own files.
            Err(error) => {
                log::warn!(
                    "reading {} read-only failed ({error}); using a snapshot copy",
                    path.display()
                );
                let snapshot = tempfile::tempdir()?;
                let copy = copy_database(&path, snapshot.path())?;
                let connection = Connection::open(&copy)?;
                let result = read(&connection);
                drop(connection);
                drop(snapshot);
                result
            }
        }
    }
}

impl SessionProvider for CodexProvider {
    fn agent(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn availability(&self) -> Availability {
        let Some(path) = self.database_path() else {
            return Availability::Unavailable(format!(
                "no {DB_PREFIX}*{DB_SUFFIX} in {}",
                self.codex_dir.display()
            ));
        };
        // Prepare the real query: it names every column the list needs, so a
        // schema that lost one is caught here rather than row by row later.
        match self.with_connection(|connection| {
            connection.prepare(LIST_QUERY)?;
            Ok(())
        }) {
            Ok(()) => Availability::Ready,
            Err(error) => Availability::Unavailable(format!(
                "{} is not readable as a thread store: {error}",
                path.display()
            )),
        }
    }

    fn list(&self) -> Result<Vec<SessionSummary>> {
        let rows = self.with_connection(|connection| {
            let mut statement = connection.prepare(LIST_QUERY)?;
            let rows = statement
                .query_map([], |row| {
                    Ok(Row {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        preview: row.get(2)?,
                        first_user_message: row.get(3)?,
                        cwd: row.get(4)?,
                        branch: row.get(5)?,
                        model: row.get(6)?,
                        at_ms: row.get(7)?,
                        rollout: row.get(8)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        });
        // An unreadable or absent store is a state, not a failure: whoever has
        // never run Codex should still see their Claude sessions.
        let rows = match rows {
            Ok(rows) => rows,
            Err(error) => {
                log::warn!("Codex thread store unreadable: {error}");
                return Ok(Vec::new());
            }
        };
        Ok(rows.into_iter().map(Row::into_summary).collect())
    }

    fn counts(&self, session: &SessionSummary) -> Result<SessionCounts> {
        let Some(path) = session.log_path.as_ref() else {
            return Ok(SessionCounts::default());
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return Ok(SessionCounts::default());
        };
        Ok(SessionCounts {
            messages: Some(count_rollout_messages(&text)),
            // Codex records spawned agents in `thread_spawn_edges`, which nothing
            // reads yet. Zero, not a guess.
            subagents: 0,
        })
    }

    fn resume_command(&self, session: &SessionSummary, fork: Fork) -> Option<ResumeCommand> {
        // `codex resume <id>` continues a thread. There is no fork — the CLI has
        // no flag for it — so the caller disables that control rather than
        // building a command that would not do what its label says.
        if fork == Fork::New {
            return None;
        }
        Some(ResumeCommand {
            program: "codex".to_string(),
            args: vec!["resume".to_string(), session.id.to_string()],
            cwd: session.cwd.clone(),
        })
    }

    fn paths_to_trash(&self, session: &SessionSummary) -> Vec<PathBuf> {
        // Only the rollout transcript, and only while it really sits inside the
        // Codex directory: `rollout_path` comes out of a database this editor does
        // not own, and a doctored row must not turn into a delete somewhere else.
        // The thread row itself stays — writing to Codex's database is not ours.
        session
            .log_path
            .iter()
            .filter(|path| self.contains(path))
            .cloned()
            .collect()
    }
}

struct Row {
    id: String,
    title: Option<String>,
    preview: Option<String>,
    first_user_message: Option<String>,
    cwd: Option<String>,
    branch: Option<String>,
    model: Option<String>,
    at_ms: Option<i64>,
    rollout: Option<String>,
}

impl Row {
    fn into_summary(self) -> SessionSummary {
        let preview =
            non_empty(self.preview).or_else(|| non_empty(self.first_user_message.clone()));
        let title = non_empty(self.title)
            .or_else(|| non_empty(self.first_user_message))
            .unwrap_or_else(|| self.id.clone());
        let log_path = self.rollout.map(PathBuf::from);
        let log_bytes = log_path
            .as_ref()
            .and_then(|path| std::fs::metadata(path).ok())
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        SessionSummary {
            id: Arc::from(self.id.as_str()),
            agent: AgentKind::Codex,
            title,
            preview: preview.unwrap_or_default(),
            // Codex stores no speaker for its preview, and the row would rather
            // show the text unlabelled than label it wrongly.
            preview_speaker: None,
            cwd: self.cwd.map(PathBuf::from).unwrap_or_default(),
            branch: self.branch.filter(|branch| !branch.is_empty()),
            model: self.model.filter(|model| !model.is_empty()),
            updated_at: self.at_ms.map(millis_to_time).unwrap_or(UNIX_EPOCH),
            log_path,
            log_bytes,
        }
    }
}

/// A rollout line is one JSON object. The conversation is the `response_item`
/// lines whose role is the user or the assistant — `developer` lines are the
/// harness's own prompts, not something anyone said.
fn count_rollout_messages(text: &str) -> usize {
    text.lines()
        .filter(|line| line.contains(r#""type":"response_item""#))
        .filter(|line| line.contains(r#""role":"user""#) || line.contains(r#""role":"assistant""#))
        .count()
}

fn copy_database(path: &Path, into: &Path) -> Result<PathBuf> {
    let name = path
        .file_name()
        .context("state database has no file name")?;
    let copy = into.join(name);
    std::fs::copy(path, &copy)?;
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        let sidecar = PathBuf::from(sidecar);
        if sidecar.exists() {
            let mut target = copy.as_os_str().to_os_string();
            target.push(suffix);
            std::fs::copy(&sidecar, PathBuf::from(target))?;
        }
    }
    Ok(copy)
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn millis_to_time(millis: i64) -> SystemTime {
    if millis <= 0 {
        return UNIX_EPOCH;
    }
    UNIX_EPOCH + Duration::from_millis(millis as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_schema_wins_numerically_not_lexically() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["state_2.sqlite", "state_10.sqlite", "state_9.sqlite"] {
            std::fs::write(dir.path().join(name), b"").unwrap();
        }
        let provider = CodexProvider::new(dir.path().to_path_buf());
        assert_eq!(
            provider.database_path().unwrap().file_name().unwrap(),
            "state_10.sqlite",
            "a lexical sort would have picked state_9"
        );
    }

    #[test]
    fn a_missing_store_is_a_state_and_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let provider = CodexProvider::new(dir.path().to_path_buf());
        assert!(matches!(
            provider.availability(),
            Availability::Unavailable(_)
        ));
        assert!(provider.list().unwrap().is_empty());
    }

    #[test]
    fn a_store_without_the_threads_table_is_unavailable_rather_than_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state_1.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute("create table something_else (id text)", [])
            .unwrap();
        drop(connection);

        let provider = CodexProvider::new(dir.path().to_path_buf());
        assert!(matches!(
            provider.availability(),
            Availability::Unavailable(_)
        ));
        assert!(provider.list().unwrap().is_empty());
    }

    /// The schema is the one observed in `~/.codex/state_5.sqlite`, trimmed to the
    /// columns the list reads.
    fn thread_store(dir: &Path) -> Connection {
        let connection = Connection::open(dir.join("state_5.sqlite")).unwrap();
        connection
            .execute_batch(
                "create table threads (
                    id text primary key, rollout_path text not null, created_at integer not null,
                    updated_at integer not null, source text not null, model_provider text not null,
                    cwd text not null, title text not null, sandbox_policy text not null,
                    approval_mode text not null, tokens_used integer not null default 0,
                    has_user_event integer not null default 0, archived integer not null default 0,
                    git_branch text, first_user_message text not null default '',
                    model text, preview text not null default '',
                    recency_at_ms integer, updated_at_ms integer);",
            )
            .unwrap();
        connection
    }

    #[test]
    fn a_thread_the_user_typed_in_lists_even_with_has_user_event_zero() {
        let dir = tempfile::tempdir().unwrap();
        let connection = thread_store(dir.path());
        connection
            .execute(
                "insert into threads (id, rollout_path, created_at, updated_at, source, \
                 model_provider, cwd, title, sandbox_policy, approval_mode, has_user_event, \
                 git_branch, first_user_message, model, preview, recency_at_ms) \
                 values ('t1', '/nowhere/rollout.jsonl', 1, 2, 'cli', 'openai', '/w/one', \
                 'xin chào bạn', 'ro', 'auto', 0, 'main', 'xin chào bạn', 'gpt-5.6-terra', \
                 'xin chào bạn', 1787416263443)",
                [],
            )
            .unwrap();
        drop(connection);

        let provider = CodexProvider::new(dir.path().to_path_buf());
        assert!(provider.availability().is_ready());
        let sessions = provider.list().unwrap();
        assert_eq!(
            sessions.len(),
            1,
            "has_user_event = 0 must not hide a thread"
        );
        let session = &sessions[0];
        assert_eq!(session.title, "xin chào bạn");
        assert_eq!(session.cwd, PathBuf::from("/w/one"));
        assert_eq!(session.branch.as_deref(), Some("main"));
        assert_eq!(session.model.as_deref(), Some("gpt-5.6-terra"));
        assert_eq!(session.agent, AgentKind::Codex);
    }

    #[test]
    fn codex_offers_a_resume_but_never_a_fork() {
        let dir = tempfile::tempdir().unwrap();
        let provider = CodexProvider::new(dir.path().to_path_buf());
        let session = SessionSummary {
            id: "t1".into(),
            agent: AgentKind::Codex,
            title: String::new(),
            preview: String::new(),
            preview_speaker: None,
            cwd: PathBuf::from("/w/one"),
            branch: None,
            model: None,
            updated_at: UNIX_EPOCH,
            log_path: None,
            log_bytes: 0,
        };
        let resume = provider
            .resume_command(&session, Fork::Continue)
            .expect("continue is supported");
        assert_eq!(resume.program, "codex");
        assert_eq!(resume.args, vec!["resume".to_string(), "t1".to_string()]);
        assert_eq!(resume.cwd, PathBuf::from("/w/one"));
        assert!(
            provider.resume_command(&session, Fork::New).is_none(),
            "there is no --fork-session for codex, and inventing one would lie"
        );
    }

    #[test]
    fn only_conversation_lines_of_a_rollout_are_counted() {
        let rollout = concat!(
            r#"{"type":"session_meta","payload":{}}"#,
            "\n",
            r#"{"type":"response_item","payload":{"type":"message","role":"developer"}}"#,
            "\n",
            r#"{"type":"response_item","payload":{"type":"message","role":"user"}}"#,
            "\n",
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant"}}"#,
            "\n",
            r#"{"type":"event_msg","payload":{}}"#,
            "\n",
        );
        assert_eq!(count_rollout_messages(rollout), 2);
    }

    /// Against the real `~/.codex`. Ignored for the same reason as the Claude
    /// one: CI has no home directory worth reading. This is the test that proves
    /// the schema was read rather than guessed.
    #[test]
    #[ignore = "reads ~/.codex and only holds on the author's machine"]
    fn reads_the_real_store() {
        let provider = CodexProvider::new(CodexProvider::default_root());
        assert!(
            provider.availability().is_ready(),
            "expected a readable thread store, got {:?}",
            provider.availability()
        );
        let sessions = provider.list().unwrap();
        eprintln!("{} codex threads", sessions.len());
        assert!(
            !sessions.is_empty(),
            "the author has run codex at least once"
        );
        let session = &sessions[0];
        assert_eq!(session.agent, AgentKind::Codex);
        assert!(!session.title.is_empty());
        assert!(session.cwd.is_absolute(), "cwd came from the row");
        if let Some(log) = &session.log_path {
            let counts = provider.counts(session).unwrap();
            eprintln!("{} -> {counts:?}", log.display());
            assert!(counts.messages.is_some());
        }
    }

    /// The database is a file this editor does not own. Every arm here is a way a
    /// doctored `rollout_path` could aim the delete somewhere else — and the
    /// lexical `starts_with` this used to do answered "inside" to two of them.
    #[test]
    fn a_rollout_outside_the_codex_directory_is_never_trashed() {
        let dir = tempfile::tempdir().unwrap();
        let codex = dir.path().join("codex");
        std::fs::create_dir_all(codex.join("sessions")).unwrap();
        let provider = CodexProvider::new(codex.clone());
        let mut session = SessionSummary {
            id: "t1".into(),
            agent: AgentKind::Codex,
            title: String::new(),
            preview: String::new(),
            preview_speaker: None,
            cwd: PathBuf::new(),
            branch: None,
            model: None,
            updated_at: UNIX_EPOCH,
            log_path: None,
            log_bytes: 0,
        };

        // A real rollout in its proper place: the one case that may be trashed.
        let inside = codex.join("sessions").join("rollout.jsonl");
        std::fs::write(&inside, "{}").unwrap();
        session.log_path = Some(inside.clone());
        // The path as recorded, not the canonicalised one: canonicalising is how
        // the guard decides, but what gets trashed is the path the confirmation
        // dialog showed the user.
        assert_eq!(provider.paths_to_trash(&session), vec![inside]);

        // Somewhere else entirely.
        let outside = dir.path().join("elsewhere.jsonl");
        std::fs::write(&outside, "{}").unwrap();
        session.log_path = Some(outside.clone());
        assert!(
            provider.paths_to_trash(&session).is_empty(),
            "a path from the database must not become a delete outside the store"
        );

        // Climbing out with `..` while still starting with the store's own
        // components — this is what a component-wise `starts_with` waves through.
        let climbing = codex
            .join("sessions")
            .join("..")
            .join("..")
            .join("elsewhere.jsonl");
        session.log_path = Some(climbing);
        assert!(
            provider.paths_to_trash(&session).is_empty(),
            "`..` must not smuggle a path out of the store"
        );

        // A symlink inside the store pointing out of it.
        #[cfg(unix)]
        {
            let link = codex.join("sessions").join("link.jsonl");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            session.log_path = Some(link);
            assert!(
                provider.paths_to_trash(&session).is_empty(),
                "a symlink out of the store must not be followed into a delete"
            );
        }

        // Nothing there at all: nothing to trash, and nothing to guess about.
        session.log_path = Some(codex.join("sessions").join("gone.jsonl"));
        assert!(provider.paths_to_trash(&session).is_empty());
    }
}
