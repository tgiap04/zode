use crate::{
    AgentCommand, AgentKind, Availability, CompletedSubagents, Deletion, Fork, SessionCounts,
    SessionProvider, SessionSummary, SubagentSummary,
    claude_log::{self, HeadFacts, TailFacts},
    provider::{Untitled, is_safe_component},
};
use anyhow::{Context as _, Result};
use std::{
    fs::File,
    io::{Read as _, Seek as _, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
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

/// Claude Code's transcripts: `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`,
/// with a sidecar directory of the same stem holding `subagents/` and
/// `tool-results/`.
///
/// The directory name is an encoding of the working directory that **cannot be
/// decoded**: `/a/b-c` and `/a-b-c` both become `-a-b-c`. The real `cwd` is read
/// out of the file's own lines instead.
pub struct ClaudeProvider {
    projects_dir: PathBuf,
}

impl ClaudeProvider {
    pub fn new(projects_dir: PathBuf) -> Self {
        Self { projects_dir }
    }

    /// `~/.claude/projects` — Claude Code's own directory, not this editor's
    /// config dir.
    pub fn default_root() -> PathBuf {
        util::paths::home_dir().join(".claude").join("projects")
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
        if !is_safe_component(id) || !self.availability().is_ready() {
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

    fn completed_subagents(
        &self,
        session: &SessionSummary,
        from: u64,
    ) -> Result<CompletedSubagents> {
        let Some(log_path) = session.log_path.as_ref() else {
            return Ok(CompletedSubagents {
                tool_use_ids: Vec::new(),
                scanned_to: from,
            });
        };
        read_completed_tool_uses(log_path, from)
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
            "s1",
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
        assert_eq!(&*found.id, "s1");
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
            "empty",
            &[
                r#"{"type":"mode","mode":"default"}"#,
                r#"{"type":"permission-mode","mode":"default"}"#,
            ],
            0,
        );
        std::fs::write(dir.join("zero.jsonl"), "").unwrap();

        let provider = ClaudeProvider::new(root.path().to_path_buf());
        assert!(provider.list().unwrap().is_empty());
    }

    #[test]
    fn without_an_ai_title_the_first_user_message_is_the_title() {
        let root = projects_dir();
        session(
            &root.path().join("-w-one"),
            "s2",
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
            "housekeeping",
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
            "commanded",
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
            "renamed",
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
            "deep",
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
            "housekeeping",
            &[
                r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>Caveat: The messages below were generated by the user while running local commands."},"cwd":"/w/one"}"#,
                r#"{"type":"user","message":{"role":"user","content":"<command-name>/model</command-name> <command-args></command-args>"},"cwd":"/w/one"}"#,
            ],
            0,
        );
        let provider = ClaudeProvider::new(root.path().to_path_buf());
        assert!(provider.list().unwrap().is_empty());
        let found = provider
            .find("housekeeping")
            .unwrap()
            .expect("the transcript is on disk, so the id still names a session");
        assert_eq!(
            found.title, "housekeeping",
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
            "buried",
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
            provider.find("buried").unwrap().is_some(),
            "the transcript exists, and saying otherwise invites a second session on top of it"
        );
    }

    #[test]
    fn find_returns_the_session_the_store_holds() {
        let root = projects_dir();
        let dir = root.path().join("-w-one");
        session(
            &dir,
            "wanted",
            &[
                r#"{"type":"user","message":{"role":"user","content":"hello"},"cwd":"/w/one"}"#,
                r#"{"type":"ai-title","aiTitle":"Found me"}"#,
            ],
            0,
        );

        let provider = ClaudeProvider::new(root.path().to_path_buf());
        let found = provider.find("wanted").unwrap().expect("held");
        assert_eq!(&*found.id, "wanted");
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
            "present",
            &[r#"{"type":"user","message":{"role":"user","content":"hi"},"cwd":"/w/one"}"#],
            0,
        );

        let provider = ClaudeProvider::new(root.path().to_path_buf());
        assert!(
            provider.find("absent").unwrap().is_none(),
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
                &format!("decoy-{i}"),
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
            "wanted",
            &[r#"{"type":"user","message":{"role":"user","content":"real"},"cwd":"/w/two"}"#],
            0,
        );

        let provider = ClaudeProvider::new(root.path().to_path_buf());
        let found = provider.find("wanted").unwrap().expect("held");
        assert_eq!(&*found.id, "wanted");
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
            "s3",
            &[r#"{"type":"user","message":{"role":"user","content":"hi"},"cwd":"/w/one"}"#],
            0,
        );
        let provider = ClaudeProvider::new(root.path().to_path_buf());
        let session = &provider.list().unwrap()[0];

        let resume = provider.resume_command(session, Fork::Continue).unwrap();
        assert_eq!(resume.program, "claude");
        assert_eq!(resume.args, vec!["--resume".to_string(), "s3".to_string()]);
        assert_eq!(resume.cwd, PathBuf::from("/w/one"));

        let fork = provider.resume_command(session, Fork::New).unwrap();
        assert_eq!(
            fork.args,
            vec![
                "--resume".to_string(),
                "s3".to_string(),
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
            "s4",
            &[r#"{"type":"user","message":{"role":"user","content":"hi"},"cwd":"/w/one"}"#],
            2,
        );
        let provider = ClaudeProvider::new(root.path().to_path_buf());
        let session = &provider.list().unwrap()[0];
        assert_eq!(
            provider.paths_to_trash(session),
            vec![dir.join("s4"), dir.join("s4.jsonl")]
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
        let first = read_completed_tool_uses(&log, 0).unwrap();
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
        let second = read_completed_tool_uses(&log, first.scanned_to).unwrap();
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

        let pass = read_completed_tool_uses(&log, 10_000).unwrap();
        assert_eq!(
            pass.tool_use_ids.iter().map(|id| &**id).collect::<Vec<_>>(),
            vec!["toolu_only"]
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
                // `claude_log::completed_tool_uses`.
                spawned_at: entry.metadata().ok()?.modified().ok()?,
            })
        })
        .collect();
    subagents.sort_by(|a, b| b.spawned_at.cmp(&a.spawned_at));
    subagents
}

/// Reads `path` from `from` to its end and reports the tool results in it.
///
/// Reports the offset of the last complete line rather than the file's length.
/// A transcript being appended to right now ends mid-line, and counting that
/// partial line as read would lose whatever result it turns out to carry once
/// the rest of it lands.
fn read_completed_tool_uses(path: &Path, from: u64) -> Result<CompletedSubagents> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    // A transcript only ever grows. Shorter than where the last pass stopped
    // means a different file stands at this path now, so resuming would read
    // into the middle of someone else's line. Start again instead.
    let from = if from > length { 0 } else { from };
    file.seek(SeekFrom::Start(from))?;

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let Some(last_newline) = bytes.iter().rposition(|byte| *byte == b'\n') else {
        return Ok(CompletedSubagents {
            tool_use_ids: Vec::new(),
            scanned_to: from,
        });
    };
    // Lossy on the complete portion only, and the offset computed from the raw
    // bytes: a replacement character is a different length from what it stands
    // in for, so counting the converted string would drift the resume point.
    let complete = String::from_utf8_lossy(&bytes[..=last_newline]);
    Ok(CompletedSubagents {
        tool_use_ids: claude_log::completed_tool_uses(&complete)
            .into_iter()
            .map(Arc::from)
            .collect(),
        scanned_to: from + last_newline as u64 + 1,
    })
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
