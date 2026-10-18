use crate::{
    AgentCommand, AgentKind, Availability, Deletion, Fork, SessionCounts, SessionProvider,
    SessionSummary, SubagentSummary, TranscriptProgress,
    claude_log::{self, HeadFacts, TailFacts},
    provider::{Untitled, is_safe_component},
};
use anyhow::{Context as _, Result};
use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    fs::File,
    hash::{DefaultHasher, Hash as _, Hasher as _},
    io::{ErrorKind, Read, Seek as _, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, SystemTime},
};

/// How much of the end of a transcript to read before giving up on finding an
/// `ai-title`. Grown in steps rather than read whole: the largest transcript on
/// the author's machine is 13 MB and the title is normally in the last few KB.
const TAIL_STEPS: &[u64] = &[256 * 1024, 1024 * 1024];
/// How much of the beginning to read looking for the first thing the user said.
/// The first step clears the handful of `mode` / `permission-mode` lines every
/// session opens with; the second exists because a session opened by a slash
/// command carries that command's expanded body before the user gets a word in,
/// and one such body measured 122 KB.
const HEAD_STEPS: &[u64] = &[16 * 1024, 256 * 1024];
/// The most of a transcript read into memory at once while following it.
///
/// A first pass over a long conversation has the whole history to get through
/// and a restored window does it for every tab at once, so reading it in one
/// piece costs the file's size, per tab, plus every copy made on the way to
/// parsing it. A piece is cut at the last newline, so it holds the window plus
/// whatever line straddles its end -- one base64 image line is several megabytes.
const TRANSCRIPT_WINDOW: usize = 4 * 1024 * 1024;
/// How much of the beginning of a transcript fingerprints it. Its first line is
/// a few dozen bytes; the bound only matters for a file whose first line is
/// still being written or is absurdly long.
const IDENTITY_BYTES: u64 = 4096;
/// How much of the beginning of a candidate successor is searched for the
/// `/clear` that opens it. The command is among the first few lines, and a
/// transcript that opens on a long slash-command body is read at most this far.
const CLEAR_SEARCH_BYTES: u64 = 32 * 1024;
/// How many transcripts' heads are remembered at once. A project holds a few
/// hundred at most; past this the whole cache is dropped and refilled, which
/// costs one more read of each head that is still asked about.
const HEAD_CACHE_ENTRIES: usize = 256;

/// Claude Code's transcripts: `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`,
/// with a sidecar directory of the same stem holding `subagents/` and
/// `tool-results/`.
///
/// The directory name is an encoding of the working directory that **cannot be
/// decoded**: `/a/b-c` and `/a-b-c` both become `-a-b-c`. The real `cwd` is read
/// out of the file's own lines instead.
pub struct ClaudeProvider {
    projects_dir: PathBuf,
    /// What the head of each candidate successor said, so a tab that asks again
    /// every few seconds does not read the same heads again.
    heads: Mutex<HeadCache>,
}

impl ClaudeProvider {
    pub fn new(projects_dir: PathBuf) -> Self {
        Self {
            projects_dir,
            heads: Mutex::new(HeadCache::default()),
        }
    }

    /// How many heads have been read from disk, for a test of the cache.
    #[cfg(test)]
    fn head_reads(&self) -> usize {
        self.heads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reads
    }

    /// `~/.claude/projects` — Claude Code's own directory, not this editor's
    /// config dir — or `projects` under `CLAUDE_CONFIG_DIR` when that is set,
    /// which is where the CLI writes for anyone who has moved its config.
    pub fn default_root() -> PathBuf {
        projects_dir_for(
            std::env::var_os("CLAUDE_CONFIG_DIR"),
            util::paths::home_dir().as_path(),
        )
    }

    fn summary_for(&self, log_path: &Path, untitled: Untitled) -> Result<Option<SessionSummary>> {
        let id = log_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .context("session log has no usable stem")?
            .to_string();
        let metadata = std::fs::metadata(log_path)?;
        let len = metadata.len();
        if len == 0 {
            return Ok(None);
        }
        let updated_at = metadata.modified()?;

        let mut file = File::open(log_path)?;
        let tail = read_tail_until_title(&mut file, len)?;
        let head = read_head_until_prompt(&mut file, len)?;

        let TailFacts {
            custom_title,
            title,
            preview,
            preview_speaker,
            cwd,
            branch,
            model,
        } = tail;
        let HeadFacts {
            first_user_message,
            cwd: head_cwd,
        } = head;

        // Not a name the user typed, not one the CLI generated, not a first word
        // they never said.
        let title = custom_title
            .or(title)
            .or_else(|| first_user_message.clone());
        let title = match (title, untitled) {
            (Some(title), _) => title,
            // A session nobody spoke in is a session nobody is browsing for.
            (None, Untitled::Drop) => {
                log::debug!("a session with nothing to name it is not listed: {id}");
                return Ok(None);
            }
            (None, Untitled::KeepAsId) => id.clone(),
        };
        // The head's cwd is the session's original one; the tail's is where it
        // ended up. They differ only if the user moved the directory mid-session,
        // in which case the later one is the one that still exists.
        let cwd = cwd.or(head_cwd).unwrap_or_default();

        Ok(Some(SessionSummary {
            id: Arc::from(id.as_str()),
            agent: AgentKind::Claude,
            title,
            preview: preview.or(first_user_message).unwrap_or_default(),
            preview_speaker,
            cwd,
            branch,
            model,
            updated_at,
            log_path: Some(log_path.to_path_buf()),
            log_bytes: len,
        }))
    }

    /// `<log dir>/<session id>` — the sidecar holding `subagents/` and
    /// `tool-results/`.
    fn sidecar_dir(session: &SessionSummary) -> Option<PathBuf> {
        let log_path = session.log_path.as_ref()?;
        Some(log_path.parent()?.join(session.id.as_ref()))
    }

    fn paths_to_trash(&self, session: &SessionSummary) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Some(sidecar) = Self::sidecar_dir(session) {
            paths.push(sidecar);
        }
        if let Some(log) = session.log_path.clone() {
            paths.push(log);
        }
        paths
    }
}

impl SessionProvider for ClaudeProvider {
    fn agent(&self) -> AgentKind {
        AgentKind::Claude
    }

    fn availability(&self) -> Availability {
        if self.projects_dir.is_dir() {
            Availability::Ready
        } else {
            Availability::Unavailable(format!("{} does not exist", self.projects_dir.display()))
        }
    }

    fn list(&self) -> Result<Vec<SessionSummary>> {
        if !self.availability().is_ready() {
            return Ok(Vec::new());
        }
        let mut sessions = Vec::new();
        for project in std::fs::read_dir(&self.projects_dir)? {
            let Ok(project) = project else { continue };
            if !project
                .file_type()
                .map(|kind| kind.is_dir())
                .unwrap_or(false)
            {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(project.path()) else {
                continue;
            };
            for entry in entries {
                let Ok(entry) = entry else { continue };
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                    continue;
                }
                // The stem goes into `--resume` as an argument. A transcript
                // Claude did not name with a UUID is not one it can resume.
                if !path
                    .file_stem()
                    .and_then(OsStr::to_str)
                    .is_some_and(is_uuid)
                {
                    continue;
                }
                match self.summary_for(&path, Untitled::Drop) {
                    Ok(Some(summary)) => sessions.push(summary),
                    Ok(None) => {}
                    // One unreadable transcript must not cost the whole list.
                    Err(error) => {
                        log::warn!("skipping session log {}: {error}", path.display())
                    }
                }
            }
        }
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(sessions)
    }

    /// One `stat` per project directory, never a read of one.
    ///
    /// The directory name is an undecodable encoding of the cwd, so which project
    /// holds `<id>` cannot be computed — but the file inside it is named by the
    /// id, so each candidate is a single existence check. That is why this does
    /// not go through `list`, which reads the head and tail of all 45 transcripts
    /// to build summaries this caller would throw away.
    fn find(&self, id: &str) -> Result<Option<SessionSummary>> {
        if !is_safe_component(id) || !is_uuid(id) || !self.availability().is_ready() {
            return Ok(None);
        }
        let file_name = format!("{id}.jsonl");
        let Ok(projects) = std::fs::read_dir(&self.projects_dir) else {
            return Ok(None);
        };
        for project in projects.flatten() {
            if !project
                .file_type()
                .map(|kind| kind.is_dir())
                .unwrap_or(false)
            {
                continue;
            }
            let candidate = project.path().join(&file_name);
            if !candidate.is_file() {
                continue;
            }
            return match self.summary_for(&candidate, Untitled::KeepAsId) {
                Ok(summary) => Ok(summary),
                // The file is there but unreadable. Answering "not held" would
                // send the caller off to start a fresh session on top of a
                // transcript that exists, so this is a real error.
                Err(error) => Err(error),
            };
        }
        Ok(None)
    }

    fn new_session_command(&self, id: &str, cwd: &Path) -> Option<AgentCommand> {
        // Verified against the installed CLI: the transcript lands at
        // `~/.claude/projects/<encoded-cwd>/<id>.jsonl` under exactly this id,
        // which is what makes `find` above able to recognise it later.
        Some(AgentCommand {
            program: "claude".to_string(),
            args: vec!["--session-id".to_string(), id.to_string()],
            cwd: cwd.to_path_buf(),
        })
    }

    fn counts(&self, session: &SessionSummary) -> Result<SessionCounts> {
        let messages = session
            .log_path
            .as_ref()
            .map(count_messages)
            .transpose()?
            .flatten();
        let subagents = Self::sidecar_dir(session)
            .map(|dir| count_meta_files(&dir.join("subagents")))
            .unwrap_or(0);
        Ok(SessionCounts {
            messages,
            subagents,
        })
    }

    fn subagents(&self, session: &SessionSummary) -> Result<Vec<SubagentSummary>> {
        let Some(sidecar) = Self::sidecar_dir(session) else {
            return Ok(Vec::new());
        };
        Ok(read_subagents(&sidecar.join("subagents")))
    }

    fn transcript_progress(
        &self,
        session: &SessionSummary,
        from: u64,
    ) -> Result<TranscriptProgress> {
        let Some(log_path) = session.log_path.as_ref() else {
            return Ok(TranscriptProgress {
                scanned_to: from,
                ..TranscriptProgress::default()
            });
        };
        read_transcript_progress(log_path, from)
    }

    fn successor(
        &self,
        session: &SessionSummary,
        since: SystemTime,
        claimed: &[Arc<str>],
    ) -> Result<Option<SessionSummary>> {
        let Some(current) = session.log_path.as_deref() else {
            return Ok(None);
        };
        let Some(path) =
            newest_cleared_transcript(&self.heads, current, &session.id, since, claimed)?
        else {
            return Ok(None);
        };
        self.summary_for(&path, Untitled::KeepAsId)
    }

    fn background_quiet_for(&self, session: &SessionSummary) -> Result<Option<Duration>> {
        let Some(sidecar) = Self::sidecar_dir(session) else {
            return Ok(None);
        };
        Ok(quiet_for(&sidecar.join("subagents"), SystemTime::now()))
    }

    fn reports_turns(&self) -> bool {
        true
    }

    fn resume_command(&self, session: &SessionSummary, fork: Fork) -> Option<AgentCommand> {
        let mut args = vec!["--resume".to_string(), session.id.to_string()];
        if fork == Fork::New {
            args.push("--fork-session".to_string());
        }
        Some(AgentCommand {
            program: "claude".to_string(),
            args,
            cwd: session.cwd.clone(),
        })
    }

    fn deletion(&self, session: &SessionSummary) -> Deletion {
        let paths = self.paths_to_trash(session);
        if paths.is_empty() {
            return Deletion::Nothing;
        }
        Deletion::Trash(paths)
    }
}

/// Read growing slices of the end of the file until one contains a title, or the
/// steps run out. Falling short is not an error — the caller falls back to the
/// first user message.
fn read_tail_until_title(file: &mut File, len: u64) -> Result<TailFacts> {
    let mut best = TailFacts::default();
    for step in TAIL_STEPS {
        let want = (*step).min(len);
        let text = read_at(file, len - want, want as usize)?;
        // Only when the read started mid-file can the first line be a fragment.
        let text = if want < len {
            match text.find('\n') {
                Some(newline) => &text[newline + 1..],
                None => "",
            }
        } else {
            text.as_str()
        };
        best = claude_log::parse_tail(text);
        if best.custom_title.is_some() || best.title.is_some() || want == len {
            break;
        }
    }
    Ok(best)
}

/// Read growing slices of the beginning of the file until one holds something
/// the user actually said. Only a session with no title of any kind pays for the
/// second step, and only that session's row depends on the answer.
fn read_head_until_prompt(file: &mut File, len: u64) -> Result<HeadFacts> {
    let mut best = HeadFacts::default();
    for step in HEAD_STEPS {
        let want = (*step).min(len);
        best = claude_log::parse_head(&read_at(file, 0, want as usize)?);
        if best.first_user_message.is_some() || want == len {
            break;
        }
    }
    Ok(best)
}

/// Lossy on purpose: a byte window into a UTF-8 file will cut a character, and a
/// mangled glyph in a preview is better than losing the row.
fn read_at(file: &mut File, offset: u64, len: usize) -> Result<String> {
    file.seek(SeekFrom::Start(offset))?;
    let mut buffer = vec![0; len];
    let mut read = 0;
    while read < len {
        match file.read(&mut buffer[read..])? {
            0 => break,
            n => read += n,
        }
    }
    buffer.truncate(read);
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

/// Count conversation lines without deserializing any of them.
fn count_messages(path: &PathBuf) -> Result<Option<usize>> {
    let mut file = File::open(path)?;
    let mut buffer = vec![0; 256 * 1024];
    let mut carry = String::new();
    let mut count = 0;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        carry.push_str(&String::from_utf8_lossy(&buffer[..read]));
        // Keep the final partial line for the next chunk; a `"type"` split
        // across a chunk boundary would otherwise be missed.
        let last_newline = carry.rfind('\n');
        let (complete, rest) = match last_newline {
            Some(at) => (carry[..at].to_string(), carry[at + 1..].to_string()),
            None => (String::new(), carry.clone()),
        };
        count += complete
            .lines()
            .filter(|line| claude_log::line_is_message(line))
            .count();
        carry = rest;
    }
    if claude_log::line_is_message(&carry) {
        count += 1;
    }
    Ok(Some(count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Speaker;

    // Claude names a transcript by a UUID, and `list` and `find` hold nothing
    // else to that shape, so every fixture is named by one.
    const ONE: &str = "00000000-0000-4000-8000-000000000001";
    const TWO: &str = "00000000-0000-4000-8000-000000000002";
    const THREE: &str = "00000000-0000-4000-8000-000000000003";
    const FOUR: &str = "00000000-0000-4000-8000-000000000004";
    const WANTED: &str = "00000000-0000-4000-8000-0000000000aa";
    const ABSENT: &str = "00000000-0000-4000-8000-0000000000ab";
    const PRESENT: &str = "00000000-0000-4000-8000-0000000000ac";
    const HOUSEKEEPING: &str = "00000000-0000-4000-8000-0000000000ad";
    const EMPTY: &str = "00000000-0000-4000-8000-0000000000ae";
    const ZERO: &str = "00000000-0000-4000-8000-0000000000af";
    const COMMANDED: &str = "00000000-0000-4000-8000-0000000000b0";
    const RENAMED: &str = "00000000-0000-4000-8000-0000000000b1";
    const DEEP: &str = "00000000-0000-4000-8000-0000000000b2";
    const BURIED: &str = "00000000-0000-4000-8000-0000000000b3";

    /// One transcript, written the way Claude writes them: opening `mode` lines,
    /// a repeated `ai-title`, and a sidecar directory of subagent metadata.
    fn session(dir: &Path, id: &str, lines: &[&str], subagents: usize) {
        std::fs::write(dir.join(format!("{id}.jsonl")), lines.join("\n")).unwrap();
        if subagents > 0 {
            let subagent_dir = dir.join(id).join("subagents");
            std::fs::create_dir_all(&subagent_dir).unwrap();
            for i in 0..subagents {
                std::fs::write(subagent_dir.join(format!("agent-{i}.meta.json")), "{}").unwrap();
                // A `.jsonl` beside each `.meta.json`, as Claude leaves them —
                // counting files blindly would double the number.
                std::fs::write(subagent_dir.join(format!("agent-{i}.jsonl")), "").unwrap();
            }
        }
    }

    fn projects_dir() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("-w-one")).unwrap();
        root
    }

    #[test]
    fn a_session_reads_its_title_model_cwd_and_branch() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        session(
            &dir,
            ONE,
            &[
                r#"{"type":"mode","mode":"default"}"#,
                r#"{"type":"user","message":{"role":"user","content":"do the thing"},"cwd":"/w/one","gitBranch":"main"}"#,
                r#"{"type":"ai-title","aiTitle":"Early"}"#,
                r#"{"type":"assistant","message":{"role":"assistant","model":"claude-opus-5","content":"done"},"cwd":"/w/one","gitBranch":"feat/x"}"#,
                r#"{"type":"ai-title","aiTitle":"Doing the thing"}"#,
            ],
            13,
        );

        let provider = ClaudeProvider::new(root.path().to_path_buf());
        assert!(provider.availability().is_ready());
        let sessions = provider.list().unwrap();
        assert_eq!(sessions.len(), 1);
        let found = &sessions[0];
        assert_eq!(&*found.id, ONE);
        assert_eq!(found.title, "Doing the thing", "the last ai-title wins");
        assert_eq!(found.preview, "done");
        assert_eq!(found.preview_speaker, Some(Speaker::Agent));
        assert_eq!(found.cwd, PathBuf::from("/w/one"));
        assert_eq!(found.branch.as_deref(), Some("feat/x"));
        assert_eq!(found.model.as_deref(), Some("claude-opus-5"));

        let counts = provider.counts(found).unwrap();
        assert_eq!(counts.messages, Some(2));
        assert_eq!(
            counts.subagents, 13,
            "one per .meta.json, not one per file in the directory"
        );
    }

    #[test]
    fn a_session_nobody_spoke_in_is_not_listed() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        session(
            &dir,
            EMPTY,
            &[
                r#"{"type":"mode","mode":"default"}"#,
                r#"{"type":"permission-mode","mode":"default"}"#,
            ],
            0,
        );
        std::fs::write(dir.join(format!("{ZERO}.jsonl")), "").unwrap();

        let provider = ClaudeProvider::new(root.path().to_path_buf());
        assert!(provider.list().unwrap().is_empty());
    }

    #[test]
    fn without_an_ai_title_the_first_user_message_is_the_title() {
        let root = projects_dir();
        session(
            &root.path().join("-w-one"),
            TWO,
            &[
                r#"{"type":"user","message":{"role":"user","content":"just this once"},"cwd":"/w/one"}"#,
                r#"{"type":"assistant","message":{"role":"assistant","content":"sure"},"cwd":"/w/one"}"#,
            ],
            0,
        );
        let provider = ClaudeProvider::new(root.path().to_path_buf());
        let sessions = provider.list().unwrap();
        assert_eq!(sessions[0].title, "just this once");
    }

    #[test]
    fn a_session_of_nothing_but_local_commands_is_not_listed() {
        let root = projects_dir();
        session(
            &root.path().join("-w-one"),
            HOUSEKEEPING,
            &[
                r#"{"type":"mode","mode":"default"}"#,
                r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>Caveat: The messages below were generated by the user while running local commands."},"cwd":"/w/one"}"#,
                r#"{"type":"user","message":{"role":"user","content":"<command-name>/model</command-name> <command-args></command-args>"},"cwd":"/w/one"}"#,
                r#"{"type":"assistant","message":{"role":"assistant","content":"No response requested."},"cwd":"/w/one"}"#,
            ],
            0,
        );
        let provider = ClaudeProvider::new(root.path().to_path_buf());
        assert!(
            provider.list().unwrap().is_empty(),
            "switching model is not a conversation, and the caveat above it is not its title"
        );
    }

    #[test]
    fn a_session_opened_by_a_slash_command_is_titled_by_its_arguments() {
        let root = projects_dir();
        session(
            &root.path().join("-w-one"),
            COMMANDED,
            &[
                r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>Caveat: The messages below were generated by the user while running local commands."},"cwd":"/w/one"}"#,
                r#"{"type":"user","message":{"role":"user","content":"<command-message>debug-code</command-message> <command-name>/debug-code</command-name> <command-args>the titles are wrong</command-args>"},"cwd":"/w/one"}"#,
                r#"{"type":"assistant","message":{"role":"assistant","content":"looking"},"cwd":"/w/one"}"#,
            ],
            0,
        );
        let provider = ClaudeProvider::new(root.path().to_path_buf());
        let sessions = provider.list().unwrap();
        assert_eq!(sessions[0].title, "/debug-code the titles are wrong");
    }

    #[test]
    fn the_title_the_user_typed_is_the_one_shown() {
        let root = projects_dir();
        session(
            &root.path().join("-w-one"),
            RENAMED,
            &[
                r#"{"type":"user","message":{"role":"user","content":"do the thing"},"cwd":"/w/one"}"#,
                r#"{"type":"ai-title","aiTitle":"Doing the thing"}"#,
                r#"{"type":"custom-title","customTitle":"Renamed by hand"}"#,
            ],
            0,
        );
        let provider = ClaudeProvider::new(root.path().to_path_buf());
        let sessions = provider.list().unwrap();
        assert_eq!(
            sessions[0].title, "Renamed by hand",
            "a name the user typed outranks one generated for them"
        );
    }

    /// A session whose opening scaffolding runs past the first read step. The
    /// window has to grow to reach the prompt, and the row would otherwise be
    /// dropped for having no name.
    #[test]
    fn a_prompt_past_the_first_window_is_still_found() {
        let root = projects_dir();
        let filler = "x".repeat(20 * 1024);
        let padded = format!(
            r#"{{"type":"user","isMeta":true,"message":{{"role":"user","content":"{filler}"}},"cwd":"/w/one"}}"#
        );
        session(
            &root.path().join("-w-one"),
            DEEP,
            &[
                &padded,
                r#"{"type":"user","message":{"role":"user","content":"here is the real question"},"cwd":"/w/one"}"#,
            ],
            0,
        );
        let provider = ClaudeProvider::new(root.path().to_path_buf());
        let sessions = provider.list().unwrap();
        assert_eq!(sessions[0].title, "here is the real question");
    }

    /// The counterpart to the test above. `list` and `find` ask different
    /// questions of the same file, and only one of them is allowed to answer
    /// "gone": a tab whose session cannot be found is started fresh *under the
    /// same id*, which would put a second session on top of this transcript.
    #[test]
    fn a_session_the_list_hides_is_still_found_by_id() {
        let root = projects_dir();
        session(
            &root.path().join("-w-one"),
            HOUSEKEEPING,
            &[
                r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>Caveat: The messages below were generated by the user while running local commands."},"cwd":"/w/one"}"#,
                r#"{"type":"user","message":{"role":"user","content":"<command-name>/model</command-name> <command-args></command-args>"},"cwd":"/w/one"}"#,
            ],
            0,
        );
        let provider = ClaudeProvider::new(root.path().to_path_buf());
        assert!(provider.list().unwrap().is_empty());
        let found = provider
            .find(HOUSEKEEPING)
            .unwrap()
            .expect("the transcript is on disk, so the id still names a session");
        assert_eq!(
            found.title, HOUSEKEEPING,
            "listed under its id, for want of anything better"
        );
    }

    /// The accepted loss at the far end of the growing head read: scaffolding
    /// deeper than every step, and no title ever generated. The row goes, and
    /// `find` still holds the id so the transcript cannot be written over.
    #[test]
    fn a_prompt_past_every_window_costs_the_row_but_not_the_id() {
        let root = projects_dir();
        let filler = "x".repeat(300 * 1024);
        let padded = format!(
            r#"{{"type":"user","isMeta":true,"message":{{"role":"user","content":"{filler}"}},"cwd":"/w/one"}}"#
        );
        session(
            &root.path().join("-w-one"),
            BURIED,
            &[
                &padded,
                r#"{"type":"user","message":{"role":"user","content":"here is the real question"},"cwd":"/w/one"}"#,
            ],
            0,
        );
        let provider = ClaudeProvider::new(root.path().to_path_buf());
        assert!(
            provider.list().unwrap().is_empty(),
            "300 KB of scaffolding before the first word is past every step the head grows to"
        );
        assert!(
            provider.find(BURIED).unwrap().is_some(),
            "the transcript exists, and saying otherwise invites a second session on top of it"
        );
    }

    #[test]
    fn find_returns_the_session_the_store_holds() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        session(
            &dir,
            WANTED,
            &[
                r#"{"type":"user","message":{"role":"user","content":"hello"},"cwd":"/w/one"}"#,
                r#"{"type":"ai-title","aiTitle":"Found me"}"#,
            ],
            0,
        );

        let provider = ClaudeProvider::new(root.path().to_path_buf());
        let found = provider.find(WANTED).unwrap().expect("held");
        assert_eq!(&*found.id, WANTED);
        assert_eq!(found.title, "Found me");
        assert_eq!(
            found.cwd,
            PathBuf::from("/w/one"),
            "the cwd a resume has to run in comes back with the summary"
        );
    }

    #[test]
    fn find_returns_none_for_an_id_the_store_does_not_hold() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        session(
            &dir,
            PRESENT,
            &[r#"{"type":"user","message":{"role":"user","content":"hi"},"cwd":"/w/one"}"#],
            0,
        );

        let provider = ClaudeProvider::new(root.path().to_path_buf());
        assert!(
            provider.find(ABSENT).unwrap().is_none(),
            "an id nobody wrote is Ok(None), not an error"
        );
    }

    #[test]
    fn find_is_none_when_the_store_does_not_exist() {
        let provider = ClaudeProvider::new(PathBuf::from("/nonexistent-claude-projects"));
        assert!(provider.find("anything").unwrap().is_none());
    }

    /// The id arrives from a database and is joined onto the store root, so a
    /// value carrying a separator would read outside it. Answering `None` keeps
    /// the traversal from ever being attempted.
    #[test]
    fn find_refuses_an_id_that_is_not_a_single_path_component() {
        let root = projects_dir();
        let provider = ClaudeProvider::new(root.path().to_path_buf());
        for hostile in ["../escape", "a/b", "..", ".", "", "a\\b"] {
            assert!(
                provider.find(hostile).unwrap().is_none(),
                "{hostile:?} must not be looked up"
            );
        }
    }

    /// A lookup picks its session out of a crowded store, and out of the right
    /// project directory — which is the part that cannot be computed, since the
    /// directory name is an undecodable encoding of the cwd.
    ///
    /// This does **not** prove `find` avoids reading the other transcripts. It
    /// cannot: a `list`-then-filter implementation would pass this too, and
    /// telling them apart needs a count of files opened, which is more machinery
    /// than the property is worth. The cheap-lookup shape is stated on `find`
    /// itself and held by review, not by this test.
    #[test]
    fn find_picks_its_session_out_of_a_crowded_store() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        // Twenty decoys, each with content `summary_for` would have to read.
        for i in 0..20 {
            session(
                &dir,
                &format!("00000000-0000-4000-8000-{i:012}"),
                &[
                    r#"{"type":"user","message":{"role":"user","content":"noise"},"cwd":"/w/one"}"#,
                    r#"{"type":"ai-title","aiTitle":"Noise"}"#,
                ],
                0,
            );
        }
        // In a different project directory from the decoys, so the lookup has to
        // walk past one directory to reach it.
        std::fs::create_dir_all(root.path().join("-w-two")).unwrap();
        session(
            &root.path().join("-w-two"),
            WANTED,
            &[r#"{"type":"user","message":{"role":"user","content":"real"},"cwd":"/w/two"}"#],
            0,
        );

        let provider = ClaudeProvider::new(root.path().to_path_buf());
        let found = provider.find(WANTED).unwrap().expect("held");
        assert_eq!(&*found.id, WANTED);
        assert_eq!(
            found.cwd,
            PathBuf::from("/w/two"),
            "found in the second project directory, not the one the decoys are in"
        );
        assert_eq!(
            provider.list().unwrap().len(),
            21,
            "the decoys are really there — the lookup had a crowd to pick out of"
        );
    }

    #[test]
    fn claude_can_be_told_which_id_to_use() {
        let provider = ClaudeProvider::new(PathBuf::from("/anywhere"));
        let command = provider
            .new_session_command("11111111-2222-3333-4444-555555555555", Path::new("/w/one"))
            .expect("Claude has --session-id");
        assert_eq!(command.program, "claude");
        assert_eq!(
            command.args,
            vec![
                "--session-id".to_string(),
                "11111111-2222-3333-4444-555555555555".to_string()
            ]
        );
        assert_eq!(command.cwd, PathBuf::from("/w/one"));
    }

    #[test]
    fn a_missing_projects_directory_is_a_state_and_not_an_error() {
        let provider = ClaudeProvider::new(PathBuf::from("/nowhere/at/all"));
        assert!(!provider.availability().is_ready());
        assert!(provider.list().unwrap().is_empty());
    }

    #[test]
    fn claude_offers_both_a_resume_and_a_fork() {
        let root = projects_dir();
        session(
            &root.path().join("-w-one"),
            THREE,
            &[r#"{"type":"user","message":{"role":"user","content":"hi"},"cwd":"/w/one"}"#],
            0,
        );
        let provider = ClaudeProvider::new(root.path().to_path_buf());
        let session = &provider.list().unwrap()[0];

        let resume = provider.resume_command(session, Fork::Continue).unwrap();
        assert_eq!(resume.program, "claude");
        assert_eq!(resume.args, vec!["--resume".to_string(), THREE.to_string()]);
        assert_eq!(resume.cwd, PathBuf::from("/w/one"));

        let fork = provider.resume_command(session, Fork::New).unwrap();
        assert_eq!(
            fork.args,
            vec![
                "--resume".to_string(),
                THREE.to_string(),
                "--fork-session".to_string()
            ]
        );
    }

    #[test]
    fn a_delete_names_the_sidecar_before_the_log() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        session(
            &dir,
            FOUR,
            &[r#"{"type":"user","message":{"role":"user","content":"hi"},"cwd":"/w/one"}"#],
            2,
        );
        let provider = ClaudeProvider::new(root.path().to_path_buf());
        let session = &provider.list().unwrap()[0];
        assert_eq!(
            provider.paths_to_trash(session),
            vec![dir.join(FOUR), dir.join(format!("{FOUR}.jsonl"))]
        );
    }

    /// Against the author's real store. Ignored so CI never depends on a home
    /// directory, but this is the test that settles whether the format reading is
    /// right — the fixtures above only prove the code does what it was told.
    #[test]
    #[ignore = "reads ~/.claude and only holds on the author's machine"]
    fn reads_the_real_store() {
        let provider = ClaudeProvider::new(ClaudeProvider::default_root());
        let started = std::time::Instant::now();
        let sessions = provider.list().unwrap();
        let elapsed = started.elapsed();
        eprintln!("{} sessions in {elapsed:?}", sessions.len());
        // 46 files match `~/.claude/projects/*/*.jsonl`; one of them is an
        // `ai-title` and nothing else, so 45 list. `find -name '*.jsonl'` reports
        // 292 because it descends into each session's `subagents/` directory —
        // those are subagent transcripts, not sessions.
        assert_eq!(
            sessions.len(),
            45,
            "45 of the 46 session logs have conversation"
        );

        let known = sessions
            .iter()
            .find(|session| &*session.id == "16819818-0732-422f-ba8d-4202c6005f37")
            .expect("the known session must be listed");
        assert_eq!(known.title, "Push and continue implementation");
        assert_eq!(known.model.as_deref(), Some("claude-opus-5"));
        assert_eq!(known.cwd, PathBuf::from("/Users/tgiap.dev/devs/zode"));
        assert_eq!(
            known.branch.as_deref(),
            Some("feat/vscode-parity-git-panel")
        );

        let counts = provider.counts(known).unwrap();
        assert_eq!(counts.messages, Some(1981), "grep -c of user|assistant");
        assert_eq!(counts.subagents, 13, "ls subagents/*.meta.json | wc -l");
    }

    /// A sidecar written the way Claude writes one, read off a live session:
    /// `{"agentType":"reviewer","description":"…","toolUseId":"toolu_…","spawnDepth":1}`.
    fn sidecar(dir: &Path, id: &str, meta: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(format!("{id}.meta.json")), meta).unwrap();
    }

    #[test]
    fn a_subagent_row_comes_from_its_sidecar_newest_first() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("subagents");
        sidecar(
            &dir,
            "agent-older",
            r#"{"agentType":"tester","description":"Run the suites","toolUseId":"toolu_a","spawnDepth":1}"#,
        );
        // Ordering is by the sidecar's own mtime, so the two have to be written
        // far enough apart for a filesystem to tell them apart at all.
        std::thread::sleep(std::time::Duration::from_millis(20));
        sidecar(
            &dir,
            "agent-newer",
            r#"{"agentType":"reviewer","description":"Review the diff","toolUseId":"toolu_b","spawnDepth":1}"#,
        );
        // No tool call, so nothing could ever report it finished.
        sidecar(&dir, "agent-mute", r#"{"agentType":"doc-writer"}"#);

        let subagents = read_subagents(&dir);
        let names: Vec<&str> = subagents.iter().map(|one| &*one.kind).collect();
        assert_eq!(
            names,
            vec!["reviewer", "tester"],
            "newest first, and a sidecar that can never be marked finished is \
             left out rather than pinned as running forever"
        );
        assert_eq!(&*subagents[0].description, "Review the diff");
        assert_eq!(&*subagents[0].tool_use_id, "toolu_b");
        assert_eq!(&*subagents[0].id, "agent-newer");
    }

    #[test]
    fn a_sidecar_naming_no_type_still_gets_a_row() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("subagents");
        sidecar(&dir, "agent-x", r#"{"toolUseId":"toolu_x"}"#);

        let subagents = read_subagents(&dir);
        assert_eq!(subagents.len(), 1);
        assert_eq!(&*subagents[0].kind, "agent");
        assert_eq!(&*subagents[0].description, "");
    }

    #[test]
    fn a_sidecar_says_whether_the_subagent_was_started_in_the_background() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("subagents");
        sidecar(
            &dir,
            "agent-bg",
            r#"{"agentType":"tester","toolUseId":"toolu_1","spawnDepth":1,"requestShape":"background","requestNonInteractive":true}"#,
        );
        sidecar(
            &dir,
            "agent-fg",
            r#"{"agentType":"tester","toolUseId":"toolu_2","requestShape":"foreground"}"#,
        );
        sidecar(
            &dir,
            "agent-old",
            r#"{"agentType":"tester","toolUseId":"toolu_3"}"#,
        );
        let flags: std::collections::HashMap<String, bool> = read_subagents(&dir)
            .into_iter()
            .map(|one| (one.id.to_string(), one.background))
            .collect();
        assert_eq!(flags["agent-bg"], true);
        assert_eq!(flags["agent-fg"], false);
        assert_eq!(flags["agent-old"], false, "no field means today's rule");
    }

    #[test]
    fn a_subagent_event_in_a_half_written_line_arrives_exactly_once() {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("s1.jsonl");
        let notification = r#"{"type":"queue-operation","operation":"enqueue","content":"<task-notification>\n<task-id>abc</task-id>\n<status>completed</status>\n</task-notification>"}"#;
        std::fs::write(&log, format!("{notification}\n{notification}")).unwrap();
        let first = read_transcript_progress(&log, 0).unwrap();
        assert_eq!(first.subagent_events.len(), 1);

        std::fs::write(&log, format!("{notification}\n{notification}\n")).unwrap();
        let second = read_transcript_progress(&log, first.scanned_to).unwrap();
        assert_eq!(second.subagent_events.len(), 1);
        let third = read_transcript_progress(&log, second.scanned_to).unwrap();
        assert!(third.subagent_events.is_empty());
    }

    #[test]
    fn a_missing_sidecar_directory_is_no_subagents_not_an_error() {
        let root = tempfile::tempdir().unwrap();
        assert!(read_subagents(&root.path().join("nothing-here")).is_empty());
    }

    /// The claim the incremental scan lives or dies on.
    ///
    /// A transcript is appended to while it is read, so a pass regularly ends
    /// mid-line. Counting that partial line as read would drop whatever result
    /// it turns out to carry once the rest of it lands — and a result that is
    /// missed once is missed forever, leaving a finished subagent marked as
    /// running for the life of the session.
    #[test]
    fn a_half_written_line_is_left_for_the_next_pass() {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("s1.jsonl");
        let finished = |id: &str| {
            format!(
                r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}"}}]}}}}"#
            )
        };

        // Ends without a newline, exactly as a live transcript does.
        std::fs::write(
            &log,
            format!("{}\n{}", finished("toolu_a"), finished("toolu_b")),
        )
        .unwrap();
        let first = read_transcript_progress(&log, 0).unwrap();
        assert_eq!(
            first
                .tool_use_ids
                .iter()
                .map(|id| &**id)
                .collect::<Vec<_>>(),
            vec!["toolu_a"],
            "the unterminated second line has not finished arriving"
        );

        let complete_line_bytes = finished("toolu_a").len() as u64 + 1;
        assert_eq!(
            first.scanned_to, complete_line_bytes,
            "the pass must stop at the last newline, not at the end of the file"
        );

        // The rest of that line lands.
        std::fs::write(
            &log,
            format!("{}\n{}\n", finished("toolu_a"), finished("toolu_b")),
        )
        .unwrap();
        let second = read_transcript_progress(&log, first.scanned_to).unwrap();
        assert_eq!(
            second
                .tool_use_ids
                .iter()
                .map(|id| &**id)
                .collect::<Vec<_>>(),
            vec!["toolu_b"],
            "the second pass reads the line it skipped, and not the one it already had"
        );
    }

    /// A shorter file is a different file. Resuming into it would read from the
    /// middle of a line that belongs to someone else's session.
    #[test]
    fn a_transcript_that_shrank_is_read_from_the_start() {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("s1.jsonl");
        std::fs::write(
            &log,
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_only\"}]}}\n",
        )
        .unwrap();

        let pass = read_transcript_progress(&log, 10_000).unwrap();
        assert_eq!(
            pass.tool_use_ids.iter().map(|id| &**id).collect::<Vec<_>>(),
            vec!["toolu_only"]
        );
        assert!(pass.restarted, "the caller must be told to start over");
        assert!(!read_transcript_progress(&log, 0).unwrap().restarted);
    }

    fn result_line(id: &str) -> String {
        format!(
            r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}"}}]}}}}"#
        )
    }

    /// A transcript the first pass has to read whole, cut into windows far
    /// smaller than it, must hand over only complete lines and never a piece
    /// bigger than a window plus the line that straddles it. That bound is the
    /// whole difference from reading the file into one buffer.
    #[test]
    fn a_long_transcript_is_read_in_bounded_windows() {
        let mut text = String::new();
        for n in 0..200 {
            text.push_str(&result_line(&format!("toolu_{n}")));
            text.push('\n');
        }
        let longest = result_line("toolu_199").len() + 1;
        let window = 1000;

        let mut pieces: Vec<String> = Vec::new();
        let consumed = read_complete_lines(text.as_bytes(), window, |piece| {
            pieces.push(piece.to_owned())
        })
        .unwrap();

        assert_eq!(consumed as usize, text.len());
        assert!(pieces.len() > 10, "it must really have been cut up");
        for piece in &pieces {
            assert!(piece.len() <= window + longest, "{} bytes", piece.len());
            assert!(piece.ends_with('\n'), "a piece ends on a line boundary");
        }
        assert_eq!(pieces.concat(), text, "nothing lost, nothing read twice");
    }

    #[test]
    fn a_line_longer_than_a_window_is_still_handed_over_whole() {
        let long = result_line(&"x".repeat(5000));
        let text = format!("{}\n{}\n", result_line("toolu_a"), long);
        let mut pieces: Vec<String> = Vec::new();
        read_complete_lines(text.as_bytes(), 100, |piece| pieces.push(piece.to_owned())).unwrap();
        assert_eq!(pieces.concat(), text);
        assert!(
            pieces.iter().any(|piece| piece.contains(&long)),
            "a line is never split across two pieces"
        );
    }

    #[test]
    fn a_trailing_partial_line_is_not_consumed() {
        let text = format!("{}\n{}", result_line("toolu_a"), result_line("toolu_b"));
        let mut seen = String::new();
        let consumed =
            read_complete_lines(text.as_bytes(), 64, |piece| seen.push_str(piece)).unwrap();
        assert_eq!(consumed as usize, result_line("toolu_a").len() + 1);
        assert_eq!(seen, format!("{}\n", result_line("toolu_a")));
    }

    /// Windowing is a way of reading, not of deciding: a window of a few bytes
    /// must find exactly what one read of the whole file finds, and stop at
    /// the same offset.
    #[test]
    fn a_small_window_finds_what_a_single_read_finds() {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("s1.jsonl");
        let mut text = String::new();
        for n in 0..30 {
            text.push_str(r#"{"type":"user","message":{"role":"user","content":"go"}}"#);
            text.push('\n');
            text.push_str(&format!(
                r#"{{"type":"assistant","message":{{"id":"m{n}","role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"toolu_{n}"}}]}}}}"#
            ));
            text.push('\n');
            text.push_str(&result_line(&format!("toolu_{n}")));
            text.push('\n');
        }
        text.push_str(&result_line("toolu_partial"));
        std::fs::write(&log, &text).unwrap();

        let whole = read_transcript_progress_in(&log, 0, usize::MAX / 2).unwrap();
        let windowed = read_transcript_progress_in(&log, 0, 37).unwrap();
        assert_eq!(windowed, whole);
        assert_eq!(whole.tool_use_ids.len(), 30);
        assert!(
            whole.scanned_to < text.len() as u64,
            "the partial line waits"
        );
    }

    /// The tab's transcript can be moved out from under it. Reading nothing
    /// is the honest answer, and an error here is logged on every pass.
    #[test]
    fn a_transcript_deleted_under_a_tab_reads_as_nothing_new() {
        let root = tempfile::tempdir().unwrap();
        let pass = read_transcript_progress(&root.path().join("gone.jsonl"), 500).unwrap();
        assert_eq!(pass.scanned_to, 500, "the cursor stays where it was");
        assert!(!pass.restarted);
        assert!(pass.turn_marks.is_empty() && pass.tool_use_ids.is_empty());
    }

    /// A different file at the same path can be exactly as long as the cursor
    /// or longer, which `from > length` cannot see. Its first line can, and a
    /// first line is the one thing a growing transcript never changes.
    #[test]
    fn a_transcripts_identity_is_its_first_line_once_that_line_is_whole() {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("s1.jsonl");
        let first = r#"{"type":"mode","mode":"default","sessionId":"aaaa"}"#;

        std::fs::write(&log, &first[..20]).unwrap();
        assert_eq!(
            read_transcript_progress(&log, 0).unwrap().identity,
            None,
            "a first line still being written is not an identity yet"
        );

        std::fs::write(&log, format!("{first}\n")).unwrap();
        let identity = read_transcript_progress(&log, 0).unwrap().identity;
        assert!(identity.is_some());

        std::fs::write(&log, format!("{first}\n{}\n", result_line("toolu_a"))).unwrap();
        assert_eq!(
            read_transcript_progress(&log, 0).unwrap().identity,
            identity,
            "appending never changes it"
        );

        let other = r#"{"type":"mode","mode":"default","sessionId":"bbbb"}"#;
        std::fs::write(&log, format!("{other}\n{}\n", result_line("toolu_a"))).unwrap();
        assert_ne!(
            read_transcript_progress(&log, 0).unwrap().identity,
            identity,
            "another conversation at the same path reads differently"
        );
    }

    #[test]
    fn a_first_line_longer_than_the_fingerprint_still_has_a_stable_identity() {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("s1.jsonl");
        let first = format!(r#"{{"type":"mode","pad":"{}"}}"#, "x".repeat(8000));
        std::fs::write(&log, &first).unwrap();
        let before = read_transcript_progress(&log, 0).unwrap().identity;
        assert!(before.is_some());
        std::fs::write(&log, format!("{first}\n{}\n", result_line("toolu_a"))).unwrap();
        assert_eq!(read_transcript_progress(&log, 0).unwrap().identity, before);
    }

    // --- the shape of an id ---

    #[test]
    fn a_transcript_whose_name_is_not_a_uuid_is_never_listed() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        let prompt = r#"{"type":"user","message":{"role":"user","content":"hi"},"cwd":"/w/one"}"#;
        session(&dir, "--dangerously-skip-permissions", &[prompt], 0);
        session(&dir, "agent-a0118bd7", &[prompt], 0);
        session(&dir, ONE, &[prompt], 0);

        let provider = ClaudeProvider::new(root.path().to_path_buf());
        let listed: Vec<String> = provider
            .list()
            .unwrap()
            .into_iter()
            .map(|found| found.id.to_string())
            .collect();
        assert_eq!(
            listed,
            vec![ONE.to_string()],
            "a stem reaches `--resume` as an argument, so only a UUID may"
        );
    }

    #[test]
    fn an_id_that_is_not_a_uuid_is_never_found_even_when_its_file_exists() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        let prompt = r#"{"type":"user","message":{"role":"user","content":"hi"},"cwd":"/w/one"}"#;
        let provider = ClaudeProvider::new(root.path().to_path_buf());
        for hostile in [
            "--dangerously-skip-permissions",
            "-x",
            "not-a-uuid",
            "00000000-0000-4000-8000-00000000000g",
            "{00000000-0000-4000-8000-000000000001}",
            "urn:uuid:00000000-0000-4000-8000-000000000001",
            "00000000000040008000000000000001",
            "00000000-0000-4000-8000-00000000000",
            "00000000-0000-4000-8000-0000000000001",
        ] {
            // Windows refuses `:` in a file name, so that one cannot even be planted there.
            if cfg!(windows) && hostile.contains(':') {
                continue;
            }
            session(&dir, hostile, &[prompt], 0);
            assert!(
                provider.find(hostile).unwrap().is_none(),
                "{hostile:?} must not be looked up"
            );
        }
        session(&dir, ONE, &[prompt], 0);
        assert!(provider.find(ONE).unwrap().is_some());
        assert!(
            provider
                .find("00000000-0000-4000-8000-0000000000AB")
                .unwrap()
                .is_none(),
            "an upper-case UUID is still a UUID, and this one is simply absent"
        );
    }

    // --- where Claude keeps its transcripts ---

    #[test]
    fn the_projects_directory_follows_claude_config_dir() {
        let home = Path::new("/home/me");
        let configured = std::env::temp_dir().join("claude-work");
        assert_eq!(
            projects_dir_for(Some(configured.clone().into_os_string()), home),
            configured.join("projects"),
            "a user who moves Claude's config moves its transcripts with it"
        );
        let default = home.join(".claude").join("projects");
        assert_eq!(projects_dir_for(None, home), default);
        assert_eq!(
            projects_dir_for(Some(std::ffi::OsString::new()), home),
            default,
            "an empty variable is an unset one"
        );
        assert_eq!(
            projects_dir_for(Some("relative/claude".into()), home),
            default,
            "a relative path means a directory this process cannot know"
        );
    }

    // --- a conversation that moved to another transcript ---

    const CLEARED: &str = "00000000-0000-4000-8000-0000000000c1";
    const OTHER_CLEARED: &str = "00000000-0000-4000-8000-0000000000c2";
    const SIBLING: &str = "00000000-0000-4000-8000-0000000000c3";

    const PROMPT_LINE: &str =
        r#"{"type":"user","message":{"role":"user","content":"hi"},"cwd":"/w/one"}"#;

    /// How Claude opens the transcript a `/clear` starts, read off real ones:
    /// the command is recorded in the first few lines of the new file.
    const CLEAR_LINES: [&str; 3] = [
        r#"{"type":"mode","mode":"default","sessionId":"x"}"#,
        r#"{"type":"user","message":{"role":"user","content":"<command-name>/clear</command-name>\n<command-message>clear</command-message>\n<command-args></command-args>"},"cwd":"/w/one"}"#,
        r#"{"type":"user","message":{"role":"user","content":"<local-command-stdout></local-command-stdout>"},"cwd":"/w/one"}"#,
    ];

    fn touch(path: &Path, at: SystemTime) {
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(at)
            .unwrap();
    }

    fn an_hour_ago() -> SystemTime {
        SystemTime::now() - Duration::from_secs(3600)
    }

    /// The tab's own transcript as it stands when the conversation is cleared: last
    /// written a while ago, which is the point -- whatever the CLI writes next is
    /// newer than it. Set outright, not left to the clock: two files written
    /// back to back can share a timestamp on a coarse file system.
    fn written_a_while_ago(dir: &Path, id: &str) {
        touch(
            &dir.join(format!("{id}.jsonl")),
            SystemTime::now() - Duration::from_secs(120),
        );
    }

    fn successor_of(
        provider: &ClaudeProvider,
        id: &str,
        since: SystemTime,
        claimed: &[Arc<str>],
    ) -> Option<String> {
        let current = provider
            .find(id)
            .unwrap()
            .expect("the tab's own transcript");
        provider
            .successor(&current, since, claimed)
            .unwrap()
            .map(|found| found.id.to_string())
    }

    #[test]
    fn a_transcript_that_opens_on_a_clear_after_the_tab_began_is_its_successor() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        session(&dir, ONE, &[PROMPT_LINE], 0);
        session(&dir, CLEARED, &CLEAR_LINES, 0);
        written_a_while_ago(&dir, ONE);
        let provider = ClaudeProvider::new(root.path().to_path_buf());

        assert_eq!(
            successor_of(&provider, ONE, an_hour_ago(), &[]),
            Some(CLEARED.to_string())
        );
    }

    #[test]
    fn a_transcript_written_before_the_tab_began_is_not() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        session(&dir, ONE, &[PROMPT_LINE], 0);
        session(&dir, CLEARED, &CLEAR_LINES, 0);
        touch(&dir.join(format!("{CLEARED}.jsonl")), an_hour_ago());
        let provider = ClaudeProvider::new(root.path().to_path_buf());

        assert_eq!(
            successor_of(
                &provider,
                ONE,
                SystemTime::now() - Duration::from_secs(60),
                &[]
            ),
            None,
            "an old descendant of a session resumed just now is not what that CLI is writing"
        );
    }

    #[test]
    fn a_new_conversation_that_did_not_start_with_a_clear_is_not() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        session(&dir, ONE, &[PROMPT_LINE], 0);
        session(&dir, SIBLING, &[PROMPT_LINE], 0);
        let provider = ClaudeProvider::new(root.path().to_path_buf());

        assert_eq!(
            successor_of(&provider, ONE, an_hour_ago(), &[]),
            None,
            "another tab opened in the same project is not this one's continuation"
        );
    }

    #[test]
    fn a_session_another_tab_follows_is_not() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        session(&dir, ONE, &[PROMPT_LINE], 0);
        session(&dir, CLEARED, &CLEAR_LINES, 0);
        written_a_while_ago(&dir, ONE);
        let provider = ClaudeProvider::new(root.path().to_path_buf());

        assert_eq!(
            successor_of(&provider, ONE, an_hour_ago(), &[Arc::from(CLEARED)]),
            None
        );
    }

    #[test]
    fn the_newest_of_several_cleared_transcripts_is_chosen_and_never_the_current_one() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        session(&dir, ONE, &CLEAR_LINES, 0);
        session(&dir, CLEARED, &CLEAR_LINES, 0);
        session(&dir, OTHER_CLEARED, &CLEAR_LINES, 0);
        let now = SystemTime::now();
        touch(
            &dir.join(format!("{ONE}.jsonl")),
            now - Duration::from_secs(60),
        );
        touch(
            &dir.join(format!("{CLEARED}.jsonl")),
            now - Duration::from_secs(20),
        );
        touch(
            &dir.join(format!("{OTHER_CLEARED}.jsonl")),
            now - Duration::from_secs(10),
        );
        let provider = ClaudeProvider::new(root.path().to_path_buf());

        assert_eq!(
            successor_of(&provider, ONE, an_hour_ago(), &[]),
            Some(OTHER_CLEARED.to_string()),
            "cleared twice before anyone looked: the conversation on screen is the later one"
        );
    }

    /// After two clears the tab is on the third transcript, and the second is
    /// also a transcript that opened on a clear after the tab began. It is also
    /// older than what the tab is writing now, which is what rules it out: a
    /// successor is written *after* the transcript it succeeds.
    #[test]
    fn a_cleared_transcript_older_than_the_current_ones_last_write_is_not_a_successor() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        session(&dir, CLEARED, &CLEAR_LINES, 0);
        session(&dir, ONE, &CLEAR_LINES, 0);
        let now = SystemTime::now();
        touch(
            &dir.join(format!("{CLEARED}.jsonl")),
            now - Duration::from_secs(30),
        );
        touch(
            &dir.join(format!("{ONE}.jsonl")),
            now - Duration::from_secs(5),
        );
        let provider = ClaudeProvider::new(root.path().to_path_buf());

        assert_eq!(
            successor_of(&provider, ONE, an_hour_ago(), &[]),
            None,
            "going back to the conversation the tab already left is not following it"
        );
    }

    /// The literal is in the first line of a file that is not a continuation: a
    /// session that read a file containing the tag. A search for the substring
    /// would adopt it.
    #[test]
    fn a_transcript_that_merely_contains_the_command_is_not_a_successor() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        session(&dir, ONE, &[PROMPT_LINE], 0);
        session(
            &dir,
            SIBLING,
            &[
                r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_a","content":"const CLEAR: &str = \"<command-name>/clear</command-name>\";"}]}}"#,
            ],
            0,
        );
        written_a_while_ago(&dir, ONE);
        let provider = ClaudeProvider::new(root.path().to_path_buf());

        assert_eq!(successor_of(&provider, ONE, an_hour_ago(), &[]), None);
    }

    /// A stalled tab asks every few seconds, and each ask would read the head of
    /// every candidate. A head only changes when its file does.
    #[test]
    fn a_head_is_read_once_until_the_file_changes() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        session(&dir, ONE, &[PROMPT_LINE], 0);
        session(&dir, CLEARED, &CLEAR_LINES, 0);
        session(&dir, SIBLING, &[PROMPT_LINE], 0);
        written_a_while_ago(&dir, ONE);
        // Ordered outright: a candidate older than the best so far is not read
        // at all, so the count below would otherwise depend on the order the
        // directory lists them in.
        let now = SystemTime::now();
        touch(
            &dir.join(format!("{CLEARED}.jsonl")),
            now - Duration::from_secs(30),
        );
        touch(
            &dir.join(format!("{SIBLING}.jsonl")),
            now - Duration::from_secs(10),
        );
        let provider = ClaudeProvider::new(root.path().to_path_buf());

        for _ in 0..4 {
            assert_eq!(
                successor_of(&provider, ONE, an_hour_ago(), &[]),
                Some(CLEARED.to_string())
            );
        }
        assert_eq!(
            provider.head_reads(),
            2,
            "one read for each of the two candidates, however often it is asked"
        );

        // The sibling grows: its head is read again, and the others' are not.
        std::fs::write(
            dir.join(format!("{SIBLING}.jsonl")),
            format!("{PROMPT_LINE}\n{PROMPT_LINE}\n"),
        )
        .unwrap();
        successor_of(&provider, ONE, an_hour_ago(), &[]);
        assert_eq!(provider.head_reads(), 3);
    }

    #[test]
    fn the_head_cache_is_bounded() {
        let mut cache = HeadCache::default();
        let directory = tempfile::tempdir().unwrap();
        for n in 0..(HEAD_CACHE_ENTRIES * 2 + 3) {
            let path = directory.path().join(format!("{n}.jsonl"));
            std::fs::write(&path, PROMPT_LINE).unwrap();
            cache.opens_on_a_clear(&path, PROMPT_LINE.len() as u64, SystemTime::UNIX_EPOCH);
        }
        assert!(cache.entries.len() <= HEAD_CACHE_ENTRIES);
    }

    #[test]
    fn only_the_tabs_own_project_directory_is_searched() {
        let root = projects_dir();
        session(&root.path().join("-w-one"), ONE, &[PROMPT_LINE], 0);
        std::fs::create_dir_all(root.path().join("-w-two")).unwrap();
        session(&root.path().join("-w-two"), CLEARED, &CLEAR_LINES, 0);
        let provider = ClaudeProvider::new(root.path().to_path_buf());

        assert_eq!(successor_of(&provider, ONE, an_hour_ago(), &[]), None);
    }

    #[test]
    fn a_session_with_no_transcript_has_no_successor() {
        let provider = ClaudeProvider::new(PathBuf::from("/nowhere/at/all"));
        let session = SessionSummary {
            id: Arc::from(ONE),
            agent: AgentKind::Claude,
            title: String::new(),
            preview: String::new(),
            preview_speaker: None,
            cwd: PathBuf::new(),
            branch: None,
            model: None,
            updated_at: SystemTime::UNIX_EPOCH,
            log_path: None,
            log_bytes: 0,
        };
        assert_eq!(
            provider.successor(&session, an_hour_ago(), &[]).unwrap(),
            None
        );
    }
}

/// Every subagent the sidecar directory names, newest first.
///
/// Reads each file where [`count_meta_files`] only counts names. Both stay:
/// the history row wants a number and would be paying twenty-five file reads
/// for it, and this wants the names and cannot get them from a count.
///
/// One unreadable or reshaped file costs its own row and nothing else — the
/// rule the whole crate follows for formats it does not own. A sidecar with no
/// `toolUseId` is dropped rather than shown, because that id is the only way to
/// learn the subagent ever finished, and a row that can only ever say "running"
/// is worse than no row at all.
fn read_subagents(dir: &Path) -> Vec<SubagentSummary> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut subagents: Vec<SubagentSummary> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let id = path.file_name()?.to_str()?.strip_suffix(".meta.json")?;
            let text = std::fs::read_to_string(&path).ok()?;
            let meta: serde_json::Value = serde_json::from_str(&text).ok()?;
            let tool_use_id = meta.get("toolUseId").and_then(serde_json::Value::as_str)?;
            Some(SubagentSummary {
                id: Arc::from(id),
                // A sidecar naming no type still describes a real subagent, so
                // it keeps its row under a neutral word rather than vanishing.
                kind: Arc::from(
                    meta.get("agentType")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("agent"),
                ),
                description: Arc::from(
                    meta.get("description")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default(),
                ),
                tool_use_id: Arc::from(tool_use_id),
                background: meta.get("requestShape").and_then(serde_json::Value::as_str)
                    == Some("background"),
                // An assumption about a format this editor does not own, and the
                // one claim here no test can settle: every sidecar observed was
                // written once, at spawn, and never touched again, so its own
                // mtime is the start time. If Claude Code ever writes to it a
                // second time — to record a completion, say — this field and the
                // newest-first order built on it both go quietly wrong. A
                // subagent list in a nonsensical order is the symptom that sends
                // a reader back here.
                //
                // The transcript beside it is the file that keeps moving, and it
                // is deliberately not consulted — see
                // `claude_log::scan_chunk`.
                spawned_at: entry.metadata().ok()?.modified().ok()?,
            })
        })
        .collect();
    subagents.sort_by(|a, b| b.spawned_at.cmp(&a.spawned_at));
    subagents
}

/// Reads `path` from `from` to its end and reports the tool results and turn
/// marks in it.
///
/// Reports the offset of the last complete line rather than the file's length.
/// A transcript being appended to right now ends mid-line, and counting that
/// partial line as read would lose whatever result it turns out to carry once
/// the rest of it lands.
fn read_transcript_progress(path: &Path, from: u64) -> Result<TranscriptProgress> {
    read_transcript_progress_in(path, from, TRANSCRIPT_WINDOW)
}

/// [`read_transcript_progress`] with the window it reads in chosen by the
/// caller, so a test can cut a small file into many.
fn read_transcript_progress_in(
    path: &Path,
    from: u64,
    window: usize,
) -> Result<TranscriptProgress> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        // A transcript moved or deleted under a running tab has nothing new in
        // it. It is not a failure, and reporting one makes the tab say so on
        // every pass until the file comes back.
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(TranscriptProgress {
                scanned_to: from,
                ..TranscriptProgress::default()
            });
        }
        Err(error) => return Err(error.into()),
    };
    let length = file.metadata()?.len();
    // A transcript only ever grows. Shorter than where the last pass stopped
    // means a different file stands at this path now, so resuming would read
    // into the middle of someone else's line. Start again instead.
    let restarted = from > length;
    let from = if restarted { 0 } else { from };
    let identity = head_identity(&mut file, length)?;
    file.seek(SeekFrom::Start(from))?;

    // Bounded by the length measured above, so a transcript that is being
    // written faster than it is read still ends this pass.
    let mut scan = claude_log::ChunkScan::default();
    let read = read_complete_lines(file.take(length - from), window, |lines| {
        scan.extend(claude_log::scan_chunk(lines));
    })?;
    Ok(TranscriptProgress {
        tool_use_ids: scan.tool_use_ids.into_iter().map(Arc::from).collect(),
        turn_marks: scan.turn_marks,
        subagent_events: scan.subagent_events,
        scanned_to: from + read,
        restarted,
        identity,
    })
}

/// Reads `reader` to its end and hands the complete lines in it to `visit`, a
/// window at a time. Returns how many bytes those lines took up; a trailing
/// line with no newline yet is not among them.
///
/// Each piece is cut at the last newline in what has been read, so a piece is
/// never more than a window beyond whatever partial line the one before it left,
/// and never ends inside a line. A line longer than a window is the one case a
/// piece grows past it: the buffer keeps filling until the line ends.
fn read_complete_lines<R: Read>(
    mut reader: R,
    window: usize,
    mut visit: impl FnMut(&str),
) -> std::io::Result<u64> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut consumed = 0u64;
    loop {
        let read = reader
            .by_ref()
            .take(window as u64)
            .read_to_end(&mut buffer)?;
        if read == 0 {
            return Ok(consumed);
        }
        let Some(last_newline) = buffer.iter().rposition(|byte| *byte == b'\n') else {
            continue;
        };
        // Lossy on the complete portion only, and the offset computed from the
        // raw bytes: a replacement character is a different length from what
        // it stands in for, so counting the converted string would drift the
        // resume point.
        visit(&String::from_utf8_lossy(&buffer[..=last_newline]));
        consumed += last_newline as u64 + 1;
        buffer.drain(..=last_newline);
    }
}

/// A fingerprint of the first line of a transcript of `length` bytes, once that
/// line is whole.
///
/// What tells one file from another standing at the same path. The first line
/// of a growing transcript never changes, so a different fingerprint is a
/// different file whatever the two lengths are. A first line longer than the
/// bytes read is fingerprinted by those bytes, which are as fixed as the line
/// is once the file is that long.
fn head_identity(file: &mut File, length: u64) -> std::io::Result<Option<u64>> {
    file.seek(SeekFrom::Start(0))?;
    let mut head = Vec::new();
    file.by_ref()
        .take(length.min(IDENTITY_BYTES))
        .read_to_end(&mut head)?;
    let first_line = match head.iter().position(|byte| *byte == b'\n') {
        Some(end) => &head[..end],
        None if head.len() as u64 >= IDENTITY_BYTES => &head[..],
        None => return Ok(None),
    };
    let mut hasher = DefaultHasher::new();
    first_line.hash(&mut hasher);
    Ok(Some(hasher.finish()))
}

/// Whether `text` has the shape of a session id Claude names a transcript by: a
/// UUID in its hyphenated form, nothing around it.
///
/// Strict on purpose, where `uuid::Uuid::parse_str` also takes braces, a `urn:`
/// prefix and the form with no hyphens. The id is handed to the CLI as an
/// argument, so this is also what keeps a file named like a flag from being
/// read as one.
fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(position, byte)| {
            if matches!(position, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

/// Where Claude keeps its transcripts, given the environment's
/// `CLAUDE_CONFIG_DIR` and the home directory.
///
/// An unset or empty variable is the default. A relative one is too: it would
/// name a directory relative to the CLI's working directory, which this process
/// does not have, and guessing one is worse than reading the default.
fn projects_dir_for(config_dir: Option<OsString>, home: &Path) -> PathBuf {
    match config_dir.map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => dir.join("projects"),
        _ => home.join(".claude").join("projects"),
    }
}

/// The newest transcript beside `current` that opens on a cleared conversation,
/// was written after the tab began, and was written after `current` was.
///
/// Opening on the `/clear` is what makes a transcript a continuation and not
/// merely a neighbour: another tab opened in the same project writes a new file
/// beside this one too, and the project directory is all the two have in
/// common. The command is recorded in the first lines of the file the CLI
/// starts, which was read off real transcripts rather than assumed.
///
/// A successor is written after what it succeeds, so a transcript no newer than
/// `current`'s last write is one the tab has already left -- the first after two
/// clears, say -- and going back to it would be following nothing. `claimed`
/// holds the rest of the sessions that are not this tab's to take.
fn newest_cleared_transcript(
    heads: &Mutex<HeadCache>,
    current: &Path,
    current_id: &str,
    since: SystemTime,
    claimed: &[Arc<str>],
) -> Result<Option<PathBuf>> {
    let Some(directory) = current.parent() else {
        return Ok(None);
    };
    // A transcript deleted under the tab leaves only the time the tab began to
    // measure from.
    let current_modified = std::fs::metadata(current)
        .and_then(|metadata| metadata.modified())
        .ok();
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(directory)?.flatten() {
        let path = entry.path();
        if path.extension().and_then(OsStr::to_str) != Some("jsonl") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(OsStr::to_str) else {
            continue;
        };
        if stem == current_id || !is_uuid(stem) || claimed.iter().any(|id| &**id == stem) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        // Cheapest tests first: the head of a file is only consulted for a
        // candidate that could be chosen.
        if modified < since
            || current_modified.is_some_and(|written| modified <= written)
            || newest.as_ref().is_some_and(|(best, _)| *best >= modified)
        {
            continue;
        }
        let opens_on_a_clear = heads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .opens_on_a_clear(&path, metadata.len(), modified);
        if opens_on_a_clear {
            newest = Some((modified, path));
        }
    }
    Ok(newest.map(|(_, path)| path))
}

/// What the head of each transcript said about whether it opens on a clear,
/// keyed by the file's length and modification time: a head only changes when
/// its file does.
#[derive(Default)]
struct HeadCache {
    entries: HashMap<PathBuf, (u64, SystemTime, bool)>,
    /// Heads read from disk, as opposed to answered from `entries`.
    reads: usize,
}

impl HeadCache {
    fn opens_on_a_clear(&mut self, path: &Path, len: u64, modified: SystemTime) -> bool {
        if let Some((cached_len, cached_modified, answer)) = self.entries.get(path)
            && *cached_len == len
            && *cached_modified == modified
        {
            return *answer;
        }
        self.reads += 1;
        // A head that could not be read is not remembered: the next ask tries
        // again, where an answer from a file that read fine holds until it grows.
        let Some(answer) = read_opens_on_a_clear(path) else {
            return false;
        };
        if self.entries.len() >= HEAD_CACHE_ENTRIES {
            self.entries.clear();
        }
        self.entries
            .insert(path.to_path_buf(), (len, modified, answer));
        answer
    }
}

/// Whether the beginning of the transcript at `path` records a conversation
/// being cleared; `None` when it could not be read.
fn read_opens_on_a_clear(path: &Path) -> Option<bool> {
    let mut head = Vec::new();
    let read =
        File::open(path).and_then(|file| file.take(CLEAR_SEARCH_BYTES).read_to_end(&mut head));
    if let Err(error) = read {
        log::debug!("reading the head of {}: {error}", path.display());
        return None;
    }
    Some(claude_log::opens_with_clear(&String::from_utf8_lossy(
        &head,
    )))
}

/// How long before `now` the newest `agent-*.jsonl` in `dir` was written.
///
/// Stats the files and reads nothing from them. `None` when there are none, or
/// none whose time can be read: no evidence, which is different from "quiet".
fn quiet_for(dir: &Path, now: SystemTime) -> Option<Duration> {
    let newest = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            name.to_str()
                .is_some_and(|name| name.starts_with("agent-") && name.ends_with(".jsonl"))
        })
        .filter_map(|entry| entry.metadata().ok()?.modified().ok())
        .max()?;
    // A clock that moved backwards makes the file newer than now: not quiet.
    Some(now.duration_since(newest).unwrap_or_default())
}

fn count_meta_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(".meta.json"))
        })
        .count()
}

/// `deletion()` is new in phase 04's refactor; kept in its own module so the
/// port of `paths_to_trash` above stays provably untouched -- `git diff` on
/// `mod tests` is the check, and it must show nothing.
#[cfg(test)]
mod deletion_wrapping {
    use super::*;

    fn bare_session(id: &str, log_path: Option<PathBuf>) -> SessionSummary {
        SessionSummary {
            id: Arc::from(id),
            agent: AgentKind::Claude,
            title: String::new(),
            preview: String::new(),
            preview_speaker: None,
            cwd: PathBuf::new(),
            branch: None,
            model: None,
            updated_at: std::time::SystemTime::UNIX_EPOCH,
            log_path,
            log_bytes: 0,
        }
    }

    #[test]
    fn a_session_with_nothing_to_take_is_nothing() {
        let provider = ClaudeProvider::new(PathBuf::from("/does/not/exist"));
        let session = bare_session("none", None);
        assert_eq!(provider.deletion(&session), Deletion::Nothing);
    }

    #[test]
    fn a_session_with_a_log_path_is_a_nonempty_trash() {
        let provider = ClaudeProvider::new(PathBuf::from("/does/not/exist"));
        let log = PathBuf::from("/does/not/exist/-w-one/s.jsonl");
        let session = bare_session("s", Some(log.clone()));
        let sidecar = log.parent().unwrap().join("s");
        assert_eq!(
            provider.deletion(&session),
            Deletion::Trash(vec![sidecar, log])
        );
    }
}

#[cfg(test)]
mod quiet_tests {
    use super::*;

    #[test]
    fn quiet_is_measured_from_the_newest_subagent_transcript_only() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(quiet_for(root.path(), SystemTime::now()), None);
        std::fs::write(root.path().join("agent-a.meta.json"), "{}").unwrap();
        assert_eq!(
            quiet_for(root.path(), SystemTime::now()),
            None,
            "a sidecar is not a transcript"
        );

        std::fs::write(root.path().join("agent-a.jsonl"), "x\n").unwrap();
        let later = SystemTime::now() + Duration::from_secs(120);
        let quiet = quiet_for(root.path(), later).unwrap();
        assert!(quiet >= Duration::from_secs(119) && quiet <= Duration::from_secs(121));
        assert_eq!(
            quiet_for(root.path(), SystemTime::UNIX_EPOCH),
            Some(Duration::ZERO)
        );
    }
}
