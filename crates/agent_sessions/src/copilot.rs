use crate::{
    AgentKind, Availability, Fork, ResumeCommand, SessionCounts, SessionProvider, SessionSummary,
    Speaker, provider::is_safe_component,
};
use anyhow::{Context as _, Result};
use std::{
    fs::File,
    io::{Read as _, Seek as _, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

/// Enough of the beginning to reach `session.start`, which the CLI writes first.
const HEAD_BYTES: u64 = 16 * 1024;
/// Enough of the end to reach the last message and the last model change.
const TAIL_BYTES: u64 = 256 * 1024;

/// GitHub Copilot CLI's sessions: one directory per session under
/// `~/.copilot/session-state/<uuid>/`, holding
///
/// - `workspace.yaml` — flat scalars: `id`, `cwd`, `summary`, `created_at`, `updated_at`
/// - `events.jsonl` — one typed event per line
/// - `checkpoints/`, `files/`, `research/` — not read here
///
/// Unlike Claude, the store hands over a title and a `cwd` already computed, so
/// most of this provider is reading rather than deriving. Two cautions the
/// on-disk shape imposes:
///
/// - A session written by the VS Code extension rather than the CLI has a
///   `vscode.metadata.json` and **no `events.jsonl`**. It still lists; it simply
///   has no transcript to open.
/// - `user.message` carries both `content` and `transformedContent`. The latter
///   is the prompt after the CLI stuffed `<current_datetime>`, `<reminder>` and
///   tool preambles into it. Only `content` is what the person typed.
///
/// Neither the directory layout nor the event vocabulary is documented, so every
/// field degrades on its own: a store that changed shape costs a column, never
/// the panel.
pub struct CopilotProvider {
    session_state_dir: PathBuf,
}

/// What one pass over `events.jsonl` can say about a session.
#[derive(Default)]
struct EventFacts {
    id: Option<String>,
    cwd: Option<PathBuf>,
    model: Option<String>,
    preview: Option<String>,
    preview_speaker: Option<Speaker>,
    first_user_message: Option<String>,
}

impl CopilotProvider {
    pub fn new(session_state_dir: PathBuf) -> Self {
        Self { session_state_dir }
    }

    /// `~/.copilot/session-state` — Copilot CLI's own directory. `--config-dir`
    /// can move it, but a session this editor cannot see is one it cannot list,
    /// and guessing at an override the CLI was launched with would be worse than
    /// reading the default.
    pub fn default_root() -> PathBuf {
        util::paths::home_dir()
            .join(".copilot")
            .join("session-state")
    }

    fn summary_for(&self, dir: &Path) -> Result<Option<SessionSummary>> {
        let dir_id = dir
            .file_name()
            .and_then(|name| name.to_str())
            .context("session directory has no usable name")?
            .to_string();

        let events_path = dir.join("events.jsonl");
        let events_metadata = std::fs::metadata(&events_path).ok();
        let facts = match events_metadata.as_ref().map(|metadata| metadata.len()) {
            Some(len) if len > 0 => read_event_facts(&events_path, len)?,
            // No transcript, or an empty one: the row can still be built from
            // `workspace.yaml` alone.
            _ => EventFacts::default(),
        };

        let workspace = read_flat_scalars(&dir.join("workspace.yaml")).unwrap_or_default();

        // The event's own id over the directory name: the directory is what the
        // filesystem calls it, the event is what the CLI calls it, and `--resume`
        // takes the latter. They agree in every session seen so far.
        let id = facts
            .id
            .or_else(|| workspace.get("id").cloned())
            .unwrap_or(dir_id);

        let cwd = facts
            .cwd
            .or_else(|| workspace.get("cwd").map(PathBuf::from))
            .unwrap_or_default();

        let title = workspace
            .get("summary")
            .filter(|summary| !summary.is_empty())
            .cloned()
            .or_else(|| facts.first_user_message.clone())
            .unwrap_or_else(|| id.clone());

        let updated_at = events_metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok())
            .or_else(|| {
                std::fs::metadata(dir.join("workspace.yaml"))
                    .and_then(|metadata| metadata.modified())
                    .ok()
            })
            .unwrap_or(SystemTime::UNIX_EPOCH);

        let log_bytes = events_metadata
            .as_ref()
            .map(|metadata| metadata.len())
            .unwrap_or(0);

        Ok(Some(SessionSummary {
            id: Arc::from(id.as_str()),
            agent: AgentKind::Copilot,
            title,
            preview: facts
                .preview
                .or(facts.first_user_message)
                .unwrap_or_default(),
            preview_speaker: facts.preview_speaker,
            cwd,
            // Copilot writes no branch. Deriving one from `cwd` would be this
            // editor inventing a fact the store never recorded.
            branch: None,
            model: facts.model,
            updated_at,
            log_path: (log_bytes > 0).then(|| events_path.clone()),
            log_bytes,
        }))
    }

    /// Whether `path` really *resolves* inside the directory this provider owns.
    ///
    /// Both sides are canonicalised first, for the reason `CodexProvider::contains`
    /// spells out: `Path::starts_with` is a component comparison that never
    /// touches the filesystem, so it says yes to
    /// `~/.copilot/session-state/../../../etc` and yes to a symlink under the
    /// store pointing anywhere at all. This guard is what stands between a path
    /// read off disk and a delete, and a lexical check does not do that job.
    ///
    /// A path that cannot be canonicalised — gone, or a dangling symlink —
    /// answers `false`: there is nothing there to trash.
    fn contains(&self, path: &Path) -> bool {
        let Ok(root) = std::fs::canonicalize(&self.session_state_dir) else {
            return false;
        };
        let Ok(path) = std::fs::canonicalize(path) else {
            return false;
        };
        path.starts_with(root)
    }
}

impl SessionProvider for CopilotProvider {
    fn agent(&self) -> AgentKind {
        AgentKind::Copilot
    }

    fn availability(&self) -> Availability {
        if self.session_state_dir.is_dir() {
            Availability::Ready
        } else {
            Availability::Unavailable(format!(
                "{} does not exist",
                self.session_state_dir.display()
            ))
        }
    }

    fn list(&self) -> Result<Vec<SessionSummary>> {
        if !self.availability().is_ready() {
            return Ok(Vec::new());
        }
        let mut sessions = Vec::new();
        for entry in std::fs::read_dir(&self.session_state_dir)? {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();
            match self.summary_for(&path) {
                Ok(Some(summary)) => sessions.push(summary),
                Ok(None) => {}
                // One unreadable session must not cost the whole list.
                Err(error) => log::warn!("skipping session {}: {error}", path.display()),
            }
        }
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(sessions)
    }

    /// One directory, reached directly: the store names each session's directory
    /// by its id, so there is nothing to search.
    fn find(&self, id: &str) -> Result<Option<SessionSummary>> {
        if !is_safe_component(id) || !self.availability().is_ready() {
            return Ok(None);
        }
        let dir = self.session_state_dir.join(id);
        if !dir.is_dir() {
            return Ok(None);
        }
        // Unlike a missing directory, a directory that will not read is an error:
        // answering "not held" would send the caller off to start a session on
        // top of one that exists.
        self.summary_for(&dir)
    }

    fn new_session_command(&self, _id: &str, _cwd: &Path) -> Option<ResumeCommand> {
        // `--resume=<id>` takes an id the CLI already wrote; there is no flag for
        // choosing the id of a new session. See the Codex impl for why inventing
        // one would be worse than declining.
        None
    }

    fn counts(&self, session: &SessionSummary) -> Result<SessionCounts> {
        let messages = session.log_path.as_ref().map(count_messages).transpose()?;
        Ok(SessionCounts {
            messages,
            // Copilot's `--agent` runs a custom agent in the same session rather
            // than spawning a transcript of its own, so there is no subagent
            // count to report. Reporting 0 is the honest answer, not a placeholder.
            subagents: 0,
        })
    }

    fn resume_command(&self, session: &SessionSummary, fork: Fork) -> Option<ResumeCommand> {
        // `copilot --resume=<id>` continues a session. There is no fork flag, so
        // the caller disables that control rather than building a command that
        // would not do what its label says.
        if fork == Fork::New {
            return None;
        }
        Some(ResumeCommand {
            program: "copilot".to_string(),
            args: vec![format!("--resume={}", session.id)],
            cwd: session.cwd.clone(),
        })
    }

    fn paths_to_trash(&self, session: &SessionSummary) -> Vec<PathBuf> {
        // The whole session directory: the transcript is only one of the files
        // Copilot writes for a session, and leaving `checkpoints/` and `files/`
        // behind would leave the session half-deleted.
        let Some(log_path) = session.log_path.as_ref() else {
            return Vec::new();
        };
        let root = std::fs::canonicalize(&self.session_state_dir).ok();
        log_path
            .parent()
            .filter(|dir| {
                // `contains` alone would accept the store root itself, which
                // would trash every session at once.
                self.contains(dir) && std::fs::canonicalize(dir).ok() != root
            })
            .map(|dir| vec![dir.to_path_buf()])
            .unwrap_or_default()
    }
}

/// One head read for `session.start` and one tail read for the last message.
///
/// Bounded rather than whole-file: `counts` is the only caller that needs every
/// line, and it is fetched separately for rows actually on screen.
fn read_event_facts(path: &Path, len: u64) -> Result<EventFacts> {
    let mut file = File::open(path)?;
    let mut facts = EventFacts::default();

    let head = read_at(&mut file, 0, HEAD_BYTES.min(len) as usize)?;
    for line in head.lines() {
        let Some(event) = parse_event(line) else {
            continue;
        };
        match event.0 {
            "session.start" => {
                facts.id = event.1["sessionId"].as_str().map(str::to_string);
                facts.cwd = event.1["context"]["cwd"].as_str().map(PathBuf::from);
            }
            "user.message" if facts.first_user_message.is_none() => {
                facts.first_user_message = message_content(&event.1);
            }
            _ => {}
        }
    }

    let want = TAIL_BYTES.min(len);
    let tail = read_at(&mut file, len - want, want as usize)?;
    // Only a read that started mid-file can open on a fragment of a line.
    let tail = if want < len {
        match tail.find('\n') {
            Some(newline) => &tail[newline + 1..],
            None => "",
        }
    } else {
        tail.as_str()
    };
    for line in tail.lines() {
        let Some(event) = parse_event(line) else {
            continue;
        };
        match event.0 {
            "session.model_change" => {
                if let Some(model) = event.1["newModel"].as_str() {
                    facts.model = Some(model.to_string());
                }
            }
            "user.message" => {
                if let Some(content) = message_content(&event.1) {
                    facts.preview = Some(content);
                    facts.preview_speaker = Some(Speaker::User);
                }
            }
            "assistant.message" => {
                if let Some(content) = message_content(&event.1) {
                    facts.preview = Some(content);
                    facts.preview_speaker = Some(Speaker::Agent);
                }
            }
            _ => {}
        }
    }

    Ok(facts)
}

/// `(type, data)` for a line that is an event, `None` for anything else — a
/// blank line, a partial write, or an event with no type. The vocabulary seen so
/// far is `session.start`, `session.info`, `session.model_change`,
/// `user.message`, `assistant.turn_start`, `assistant.message`,
/// `assistant.turn_end`, `session.shutdown`; an unknown type is skipped rather
/// than treated as a defect, since that list came from reading sessions, not
/// from a specification.
fn parse_event(line: &str) -> Option<(&'static str, serde_json::Value)> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let kind = match value.get("type")?.as_str()? {
        "session.start" => "session.start",
        "session.model_change" => "session.model_change",
        "user.message" => "user.message",
        "assistant.message" => "assistant.message",
        _ => return None,
    };
    Some((kind, value.get("data")?.clone()))
}

/// `content`, never `transformedContent`: the latter is the prompt after the CLI
/// injected `<current_datetime>`, `<reminder>` and tool preambles, which is not
/// what anyone typed and reads as XML in a preview.
fn message_content(data: &serde_json::Value) -> Option<String> {
    let content = data.get("content")?.as_str()?.trim();
    (!content.is_empty()).then(|| content.to_string())
}

fn count_messages(path: &PathBuf) -> Result<usize> {
    let text = std::fs::read_to_string(path)?;
    Ok(text
        .lines()
        .filter_map(|line| parse_event(line))
        .filter(|(kind, _)| matches!(*kind, "user.message" | "assistant.message"))
        .count())
}

fn read_at(file: &mut File, offset: u64, len: usize) -> Result<String> {
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0u8; len];
    let mut filled = 0;
    while filled < len {
        match file.read(&mut bytes[filled..])? {
            0 => break,
            read => filled += read,
        }
    }
    bytes.truncate(filled);
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `key: value` pairs at the top level of a YAML file, and nothing else.
///
/// Deliberately not a YAML parser. `workspace.yaml` is six flat scalars, and
/// pulling a YAML dependency into the whole workspace to read one of them —
/// `summary`, the only field `events.jsonl` does not also carry — is a poor
/// trade. An indented line or a sequence entry is skipped, so should the file
/// grow structure, the fields already understood keep working and the new ones
/// are simply not read.
fn read_flat_scalars(path: &Path) -> Result<std::collections::HashMap<String, String>> {
    let text = std::fs::read_to_string(path)?;
    let mut out = std::collections::HashMap::default();
    for line in text.lines() {
        // Anything indented belongs to a structure this reader does not model.
        if line.starts_with(' ') || line.starts_with('\t') || line.starts_with('-') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || key.starts_with('#') {
            continue;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'');
        out.insert(key.to_string(), value.to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape read off a real `~/.copilot/session-state` directory.
    fn write_session(root: &Path, id: &str, events: Option<&str>, workspace: &str) -> PathBuf {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("workspace.yaml"), workspace).unwrap();
        if let Some(events) = events {
            std::fs::write(dir.join("events.jsonl"), events).unwrap();
        }
        dir
    }

    const WORKSPACE: &str = "id: 6454ea85-a0cc-4961-8f75-26c414f668e1\n\
         cwd: /work/project\n\
         summary: Vietnamese Greeting\n\
         summary_count: 0\n\
         created_at: 2026-04-01T11:50:58.666Z\n\
         updated_at: 2026-04-01T11:52:43.011Z\n";

    const EVENTS: &str = concat!(
        r#"{"type":"session.start","data":{"sessionId":"6454ea85-a0cc-4961-8f75-26c414f668e1","context":{"cwd":"/work/project"}},"id":"a"}"#,
        "\n",
        r#"{"type":"session.info","data":{"infoType":"mcp","message":"GitHub MCP Server: Connected"},"id":"b"}"#,
        "\n",
        r#"{"type":"session.model_change","data":{"previousModel":"claude-sonnet-4.5","newModel":"gpt-5-mini"},"id":"c"}"#,
        "\n",
        r#"{"type":"user.message","data":{"content":"chào bạn","transformedContent":"<current_datetime>2026-04-01</current_datetime>\n\nchào bạn\n\n<reminder>noise</reminder>"},"id":"d"}"#,
        "\n",
        r#"{"type":"assistant.message","data":{"messageId":"e","content":"Chào bạn!"},"id":"e"}"#,
        "\n",
        r#"{"type":"session.shutdown","data":{},"id":"f"}"#,
        "\n",
    );

    #[test]
    fn a_session_reads_its_title_cwd_and_last_speaker() {
        let root = tempfile::tempdir().unwrap();
        write_session(root.path(), "6454ea85", Some(EVENTS), WORKSPACE);
        let provider = CopilotProvider::new(root.path().to_path_buf());

        let sessions = provider.list().unwrap();
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.agent, AgentKind::Copilot);
        // The event's id, not the directory's shortened name.
        assert_eq!(session.id.as_ref(), "6454ea85-a0cc-4961-8f75-26c414f668e1");
        assert_eq!(session.title, "Vietnamese Greeting");
        assert_eq!(session.cwd, PathBuf::from("/work/project"));
        assert_eq!(session.model.as_deref(), Some("gpt-5-mini"));
        assert_eq!(session.preview, "Chào bạn!");
        assert_eq!(session.preview_speaker, Some(Speaker::Agent));
        assert_eq!(session.branch, None);
        assert!(session.log_path.is_some());
        assert_eq!(provider.counts(session).unwrap().messages, Some(2));
    }

    /// The trap the on-disk shape sets: `transformedContent` holds the prompt
    /// after the CLI stuffed reminders into it. A preview built from it is a
    /// wall of XML rather than the sentence someone typed.
    #[test]
    fn the_preview_is_what_the_person_typed_not_the_transformed_prompt() {
        let root = tempfile::tempdir().unwrap();
        let user_last = concat!(
            r#"{"type":"session.start","data":{"sessionId":"s","context":{"cwd":"/w"}},"id":"a"}"#,
            "\n",
            r#"{"type":"user.message","data":{"content":"real question","transformedContent":"<reminder>lots of injected context</reminder>\n\nreal question"},"id":"b"}"#,
            "\n",
        );
        write_session(root.path(), "s", Some(user_last), "id: s\ncwd: /w\n");
        let provider = CopilotProvider::new(root.path().to_path_buf());

        let sessions = provider.list().unwrap();
        assert_eq!(sessions[0].preview, "real question");
        assert_eq!(sessions[0].preview_speaker, Some(Speaker::User));
        // No `summary` in the yaml, so the title falls back to the first thing
        // the user said rather than to the raw id.
        assert_eq!(sessions[0].title, "real question");
    }

    /// A session written by the VS Code extension has no `events.jsonl`. It must
    /// still list — with no transcript to open and nothing to delete.
    #[test]
    fn a_session_with_no_transcript_still_lists() {
        let root = tempfile::tempdir().unwrap();
        write_session(root.path(), "3f0a6c3e", None, WORKSPACE);
        let provider = CopilotProvider::new(root.path().to_path_buf());

        let sessions = provider.list().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "Vietnamese Greeting");
        assert_eq!(sessions[0].log_path, None);
        assert_eq!(sessions[0].log_bytes, 0);
        assert_eq!(sessions[0].preview, "");
        assert_eq!(provider.counts(&sessions[0]).unwrap().messages, None);
        // Nothing to trash: there is no transcript to anchor the directory to.
        assert!(provider.paths_to_trash(&sessions[0]).is_empty());
    }

    #[test]
    fn resume_continues_but_cannot_fork() {
        let root = tempfile::tempdir().unwrap();
        write_session(root.path(), "s", Some(EVENTS), WORKSPACE);
        let provider = CopilotProvider::new(root.path().to_path_buf());
        let session = &provider.list().unwrap()[0];

        let resume = provider.resume_command(session, Fork::Continue).unwrap();
        assert_eq!(resume.program, "copilot");
        assert_eq!(
            resume.args,
            vec!["--resume=6454ea85-a0cc-4961-8f75-26c414f668e1"]
        );
        assert_eq!(resume.cwd, PathBuf::from("/work/project"));
        assert_eq!(provider.resume_command(session, Fork::New), None);
    }

    #[test]
    fn deleting_takes_the_whole_session_directory() {
        let root = tempfile::tempdir().unwrap();
        let dir = write_session(root.path(), "s", Some(EVENTS), WORKSPACE);
        let provider = CopilotProvider::new(root.path().to_path_buf());
        let session = &provider.list().unwrap()[0];

        assert_eq!(provider.paths_to_trash(session), vec![dir]);
    }

    /// A transcript whose path was tampered with must not turn into a delete
    /// somewhere else on the disk.
    ///
    /// Every arm here is a way a lexical `starts_with` would say yes: an
    /// unrelated absolute path, a `..` climb back out of the store, a symlink
    /// inside the store aiming outside it, and the store root itself — which
    /// would take every session at once.
    #[test]
    fn a_transcript_outside_the_store_is_never_trashed() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("victim")).unwrap();
        std::fs::write(outside.path().join("victim/events.jsonl"), "{}").unwrap();

        let root = tempfile::tempdir().unwrap();
        write_session(root.path(), "s", Some(EVENTS), WORKSPACE);
        let provider = CopilotProvider::new(root.path().to_path_buf());
        let session = provider.list().unwrap().remove(0);

        // Built by pushing rather than as a literal: the symlink arm below is
        // `cfg(unix)`, and a `vec![]` literal would leave `mut` unused on
        // Windows — which clippy denies.
        let mut cases: Vec<(&str, PathBuf)> = Vec::new();
        cases.push((
            "an unrelated absolute path",
            outside.path().join("victim/events.jsonl"),
        ));
        cases.push((
            "a `..` climb back out of the store",
            root.path()
                .join("s")
                .join("..")
                .join("..")
                .join(outside.path().file_name().unwrap())
                .join("victim/events.jsonl"),
        ));
        // Not a session directory: trashing this would take every session at once.
        cases.push(("the store root itself", root.path().join("events.jsonl")));

        #[cfg(unix)]
        {
            let escape = root.path().join("escape");
            std::os::unix::fs::symlink(outside.path().join("victim"), &escape).unwrap();
            cases.push((
                "a symlink inside the store aiming outside it",
                escape.join("events.jsonl"),
            ));
        }

        for (what, log_path) in cases {
            let mut candidate = session.clone();
            candidate.log_path = Some(log_path);
            assert!(
                provider.paths_to_trash(&candidate).is_empty(),
                "{what} must never be trashed"
            );
        }
    }

    #[test]
    fn an_absent_store_is_unavailable_rather_than_an_error() {
        let provider = CopilotProvider::new(PathBuf::from("/nonexistent/session-state"));
        assert!(!provider.availability().is_ready());
        assert!(provider.list().unwrap().is_empty());
    }

    /// The fixtures above were written from what a real store contains, so this
    /// is the test that catches the fixtures having drifted from it. Ignored
    /// because it only holds where Copilot has actually been run.
    #[test]
    #[ignore = "reads ~/.copilot and only holds on the author's machine"]
    fn reads_the_real_store() {
        let provider = CopilotProvider::new(CopilotProvider::default_root());
        assert!(
            provider.availability().is_ready(),
            "expected a readable session store, got {:?}",
            provider.availability()
        );
        let sessions = provider.list().unwrap();
        eprintln!("{} copilot sessions", sessions.len());
        assert!(
            !sessions.is_empty(),
            "the author has run copilot at least once"
        );
        for session in &sessions {
            assert_eq!(session.agent, AgentKind::Copilot);
            assert!(!session.title.is_empty(), "every row needs a label");
            assert!(
                session.cwd.is_absolute(),
                "{}: cwd came out of the store, so it is a real path",
                session.id
            );
            // The trap this provider exists to avoid: a preview that is the
            // transformed prompt rather than the sentence someone typed.
            assert!(
                !session.preview.contains("<current_datetime>")
                    && !session.preview.contains("<reminder>"),
                "{}: preview picked up the transformed prompt: {:?}",
                session.id,
                session.preview
            );
            let counts = provider.counts(session).unwrap();
            eprintln!(
                "{} | {} | {:?} | {counts:?}",
                session.id,
                session.cwd.display(),
                session.model
            );
        }
    }

    /// Should `workspace.yaml` grow nesting, the flat fields must keep reading
    /// and the structured ones must simply not be picked up.
    #[test]
    fn nested_yaml_costs_the_nested_field_not_the_file() {
        let root = tempfile::tempdir().unwrap();
        let workspace = "id: s\ncwd: /w\nsummary: Kept\nmetadata:\n  nested: value\n- item\n";
        write_session(root.path(), "s", None, workspace);
        let provider = CopilotProvider::new(root.path().to_path_buf());

        let sessions = provider.list().unwrap();
        assert_eq!(sessions[0].title, "Kept");
        assert_eq!(sessions[0].cwd, PathBuf::from("/w"));
    }
    #[test]
    fn find_returns_the_session_the_store_holds() {
        let root = tempfile::tempdir().unwrap();
        write_session(
            root.path(),
            "6454ea85-a0cc-4961-8f75-26c414f668e1",
            Some(EVENTS),
            WORKSPACE,
        );

        let provider = CopilotProvider::new(root.path().to_path_buf());
        let found = provider
            .find("6454ea85-a0cc-4961-8f75-26c414f668e1")
            .unwrap()
            .expect("held");
        assert_eq!(&*found.id, "6454ea85-a0cc-4961-8f75-26c414f668e1");
        assert_eq!(found.cwd, PathBuf::from("/work/project"));
        assert!(provider.find("no-such-session").unwrap().is_none());
    }

    #[test]
    fn find_is_none_when_the_store_does_not_exist() {
        let provider = CopilotProvider::new(PathBuf::from("/nonexistent-copilot-sessions"));
        assert!(provider.find("anything").unwrap().is_none());
    }

    /// The id is joined straight onto the store root here, so this is the join
    /// site the traversal guard exists for.
    #[test]
    fn find_refuses_an_id_that_is_not_a_single_path_component() {
        let root = tempfile::tempdir().unwrap();
        write_session(root.path(), "real", Some(EVENTS), WORKSPACE);
        let provider = CopilotProvider::new(root.path().to_path_buf());
        for hostile in ["../real", "a/b", "..", ".", ""] {
            assert!(
                provider.find(hostile).unwrap().is_none(),
                "{hostile:?} must not be looked up"
            );
        }
    }

    #[test]
    fn copilot_cannot_be_told_which_id_to_use() {
        let provider = CopilotProvider::new(PathBuf::from("/anywhere"));
        assert!(
            provider
                .new_session_command("some-id", Path::new("/w/one"))
                .is_none()
        );
    }
}
