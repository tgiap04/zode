use crate::{
    AgentKind, Availability, Fork, ResumeCommand, SessionCounts, SessionProvider, SessionSummary,
    claude_log::{self, HeadFacts, TailFacts},
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
/// Enough of the beginning to reach the first user message past the handful of
/// `mode` / `permission-mode` lines every session opens with.
const HEAD_BYTES: u64 = 16 * 1024;

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

    fn summary_for(&self, log_path: &Path) -> Result<Option<SessionSummary>> {
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
        let head = read_head(&mut file, len)?;
        let head = claude_log::parse_head(&head);

        // A session nobody spoke in is a session nobody is looking for.
        let TailFacts {
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
        if preview.is_none() && first_user_message.is_none() {
            return Ok(None);
        }

        let title = title
            .or_else(|| first_user_message.clone())
            .unwrap_or_else(|| id.clone());
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
                match self.summary_for(&path) {
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

    fn resume_command(&self, session: &SessionSummary, fork: Fork) -> Option<ResumeCommand> {
        let mut args = vec!["--resume".to_string(), session.id.to_string()];
        if fork == Fork::New {
            args.push("--fork-session".to_string());
        }
        Some(ResumeCommand {
            program: "claude".to_string(),
            args,
            cwd: session.cwd.clone(),
        })
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

/// Read growing slices of the end of the file until one contains an `ai-title`,
/// or the steps run out. Falling short is not an error — the caller falls back
/// to the first user message.
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
        if best.title.is_some() || want == len {
            break;
        }
    }
    Ok(best)
}

fn read_head(file: &mut File, len: u64) -> Result<String> {
    read_at(file, 0, HEAD_BYTES.min(len) as usize)
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
