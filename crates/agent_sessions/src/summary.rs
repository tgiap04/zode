use std::{path::PathBuf, sync::Arc, time::SystemTime};

/// Which CLI wrote the session.
///
/// Two agents, two entirely different stores: Claude appends JSONL under
/// `~/.claude/projects`, Codex keeps a row per thread in `~/.codex/state_*.sqlite`.
/// Nothing about them is shared except this enum and the trait that hides them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum AgentKind {
    Claude,
    Codex,
    Copilot,
}

impl AgentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Copilot => "Copilot",
        }
    }

    /// The id `project::builtin_agent` knows this agent by, so a resumed session
    /// reaches the same launch path a fresh one does.
    pub fn builtin_agent_id(self) -> &'static str {
        match self {
            Self::Claude => "claude-acp",
            Self::Codex => "codex-acp",
            Self::Copilot => "github-copilot-cli",
        }
    }
}

/// Who spoke last, so the preview can say so without the caller guessing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Speaker {
    User,
    Agent,
}

impl Speaker {
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "You",
            Self::Agent => "Agent",
        }
    }
}

/// One row of the history list.
///
/// Everything here is cheap to obtain — a bounded read of the log's head and
/// tail, or one sqlite row. The expensive numbers live in [`SessionCounts`],
/// fetched separately and only for rows that are actually on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionSummary {
    /// The id the agent's own `resume` takes.
    pub id: Arc<str>,
    pub agent: AgentKind,
    pub title: String,
    pub preview: String,
    pub preview_speaker: Option<Speaker>,
    /// Where the session ran. The group key of the list, and the cwd a resume
    /// runs in.
    pub cwd: PathBuf,
    pub branch: Option<String>,
    pub model: Option<String>,
    pub updated_at: SystemTime,
    /// The transcript itself. `None` for a Codex thread whose rollout file is
    /// gone — the row still lists, it just has no log to open.
    pub log_path: Option<PathBuf>,
    /// Of the transcript, for the delete confirmation to be able to say what it
    /// is about to move.
    pub log_bytes: u64,
}

impl SessionSummary {
    /// Whether this session ran inside one of `roots`.
    ///
    /// Prefix matching on whole components, not on strings: `/a/zode` must not
    /// match `/a/zode-kit`. `Path::starts_with` compares components, which is
    /// exactly that.
    pub fn is_within(&self, roots: &[PathBuf]) -> bool {
        roots.iter().any(|root| self.cwd.starts_with(root))
    }
}

/// The numbers that cost a full scan of the transcript.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionCounts {
    /// `None` when the store cannot answer — never `Some(0)` as a stand-in,
    /// which would read as "an empty conversation".
    pub messages: Option<usize>,
    pub subagents: usize,
}

/// Continue the same session, or branch a new one off it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fork {
    /// Keep writing to this session.
    Continue,
    /// Start a new session seeded with this one's history. Claude's
    /// `--fork-session`; Codex has no equivalent.
    New,
}

/// A resume, as data. This crate spawns nothing — the caller turns this into
/// whatever its terminal wants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResumeCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

impl ResumeCommand {
    /// The line to put on the clipboard. Quoted for a POSIX shell, because that
    /// is where the user is going to paste it.
    pub fn to_shell_string(&self) -> String {
        let mut out = shell_quote(&self.program);
        for arg in &self.args {
            out.push(' ');
            out.push_str(&shell_quote(arg));
        }
        out
    }
}

/// Single-quote anything that is not plainly safe, and escape embedded quotes
/// the only way a POSIX shell allows: end the quote, emit `\'`, reopen it.
fn shell_quote(value: &str) -> String {
    let safe = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '=' | ':'));
    if safe {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for c in value.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Whether a store can be read at all right now.
///
/// A missing store is a legitimate state, not an error: someone who has never
/// run Codex should see the Claude list, not a failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    Ready,
    Unavailable(String),
}

impl Availability {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_is_within_a_root_only_on_whole_components() {
        let session = |cwd: &str| SessionSummary {
            id: "x".into(),
            agent: AgentKind::Claude,
            title: String::new(),
            preview: String::new(),
            preview_speaker: None,
            cwd: PathBuf::from(cwd),
            branch: None,
            model: None,
            updated_at: SystemTime::UNIX_EPOCH,
            log_path: None,
            log_bytes: 0,
        };
        let roots = vec![PathBuf::from("/a/zode")];

        assert!(session("/a/zode").is_within(&roots));
        assert!(session("/a/zode/crates/ui").is_within(&roots));
        // The whole point: a sibling whose name merely starts with the root's
        // name is a different project.
        assert!(!session("/a/zode-kit").is_within(&roots));
        assert!(!session("/b/zode").is_within(&roots));
        assert!(!session("/a").is_within(&roots));
    }

    #[test]
    fn a_resume_command_survives_a_path_with_spaces() {
        let command = ResumeCommand {
            program: "claude".into(),
            args: vec!["--resume".into(), "abc-123".into()],
            cwd: PathBuf::from("/tmp"),
        };
        assert_eq!(command.to_shell_string(), "claude --resume abc-123");

        let awkward = ResumeCommand {
            program: "/Users/a b/claude".into(),
            args: vec!["--resume".into(), "it's".into()],
            cwd: PathBuf::from("/tmp"),
        };
        assert_eq!(
            awkward.to_shell_string(),
            r#"'/Users/a b/claude' --resume 'it'\''s'"#
        );
    }
}
