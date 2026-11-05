//! The session stores as a remote host reports them.
//!
//! A project opened on another machine keeps its sessions there, and the
//! providers have to run where the files and databases are. What crosses the
//! connection is what this module defines: requests a client sends, the answers
//! a host gives, and plain shapes of the summaries in both.
//!
//! The domain types stay free of serde on purpose. Two of their fields cannot
//! cross as they are: a `PathBuf` deserialized on a host with the other path
//! style loses what made it absolute, and a `SystemTime` has no portable form.
//! Paths travel as the strings the host wrote and times as milliseconds since
//! the epoch, and the conversions below are the only place either is touched.

use crate::{
    AgentKind, SessionCounts, SessionSummary, Speaker, SubagentEvent, SubagentSummary,
    TranscriptProgress, TurnMark,
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// One question a client asks the host its project lives on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SessionOp {
    List {
        agents: Vec<WireAgent>,
    },
    Find {
        agent: WireAgent,
        id: String,
    },
    Counts {
        session: WireSession,
    },
    Subagents {
        session: WireSession,
    },
    TranscriptProgress {
        session: WireSession,
        from: u64,
        max_bytes: u64,
    },
    Successor {
        session: WireSession,
        since_ms: u64,
        claimed: Vec<String>,
    },
    BackgroundQuietFor {
        session: WireSession,
    },
    DirectoriesExist {
        paths: Vec<String>,
    },
    PathsExist {
        paths: Vec<String>,
        kind: WirePathKind,
    },
    /// Names the session and nothing else. The host finds it in its own store
    /// and decides what deleting it takes; a client never says which paths go.
    Delete {
        agent: WireAgent,
        id: String,
    },
}

/// The host's answer to one [`SessionOp`], in the same order of variants.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum SessionOpResult {
    List {
        sessions: Vec<WireSession>,
        unavailable: Vec<WireUnavailable>,
    },
    Find {
        session: Option<WireSession>,
    },
    Counts {
        counts: WireCounts,
    },
    Subagents {
        subagents: Vec<WireSubagent>,
    },
    TranscriptProgress {
        progress: WireProgress,
    },
    Successor {
        session: Option<WireSession>,
    },
    BackgroundQuietFor {
        quiet_for_ms: Option<u64>,
    },
    DirectoriesExist {
        exists: Vec<bool>,
    },
    PathsExist {
        exists: Vec<bool>,
    },
    Delete {
        outcome: WireDeleteOutcome,
    },
}

/// What a path has to be on the host to count as there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WirePathKind {
    File,
    Directory,
}

/// What a delete on the host came to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum WireDeleteOutcome {
    /// Nothing of the session is left in its store, whether this delete took
    /// it or it was already gone.
    Gone,
    /// The store still lists the session, but nothing it owns is left to
    /// take. Kept apart from [`Self::Gone`]: a row forgotten for this comes
    /// back on the next sweep, so the client asks before it drops one.
    NothingToDelete,
    /// The CLI that owns the session's store is not installed on the host.
    BinaryMissing { binary: String },
    /// The session is still there. `message` says why, fit for a toast.
    Failed { message: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireAgent {
    Claude,
    Codex,
    Copilot,
    OpenCode,
}

impl From<AgentKind> for WireAgent {
    fn from(agent: AgentKind) -> Self {
        match agent {
            AgentKind::Claude => Self::Claude,
            AgentKind::Codex => Self::Codex,
            AgentKind::Copilot => Self::Copilot,
            AgentKind::OpenCode => Self::OpenCode,
        }
    }
}

impl From<WireAgent> for AgentKind {
    fn from(agent: WireAgent) -> Self {
        match agent {
            WireAgent::Claude => Self::Claude,
            WireAgent::Codex => Self::Codex,
            WireAgent::Copilot => Self::Copilot,
            WireAgent::OpenCode => Self::OpenCode,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireSpeaker {
    User,
    Agent,
}

impl From<Speaker> for WireSpeaker {
    fn from(speaker: Speaker) -> Self {
        match speaker {
            Speaker::User => Self::User,
            Speaker::Agent => Self::Agent,
        }
    }
}

impl From<WireSpeaker> for Speaker {
    fn from(speaker: WireSpeaker) -> Self {
        match speaker {
            WireSpeaker::User => Self::User,
            WireSpeaker::Agent => Self::Agent,
        }
    }
}

/// A store the host could not read, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireUnavailable {
    pub agent: WireAgent,
    pub reason: String,
}

/// [`SessionSummary`], as it crosses the connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireSession {
    pub id: String,
    pub agent: WireAgent,
    pub title: String,
    pub preview: String,
    pub preview_speaker: Option<WireSpeaker>,
    /// The host's own spelling of the path, never re-resolved on the way.
    pub cwd: String,
    pub branch: Option<String>,
    pub model: Option<String>,
    pub updated_at_ms: u64,
    pub log_path: Option<String>,
    pub log_bytes: u64,
}

impl From<&SessionSummary> for WireSession {
    fn from(session: &SessionSummary) -> Self {
        Self {
            id: session.id.to_string(),
            agent: session.agent.into(),
            title: session.title.clone(),
            preview: session.preview.clone(),
            preview_speaker: session.preview_speaker.map(Into::into),
            cwd: session.cwd.to_string_lossy().into_owned(),
            branch: session.branch.clone(),
            model: session.model.clone(),
            updated_at_ms: time_to_millis(session.updated_at),
            log_path: session
                .log_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            log_bytes: session.log_bytes,
        }
    }
}

impl From<WireSession> for SessionSummary {
    fn from(session: WireSession) -> Self {
        Self {
            id: Arc::from(session.id),
            agent: session.agent.into(),
            title: session.title,
            preview: session.preview,
            preview_speaker: session.preview_speaker.map(Into::into),
            cwd: PathBuf::from(session.cwd),
            branch: session.branch,
            model: session.model,
            updated_at: millis_to_time(session.updated_at_ms),
            log_path: session.log_path.map(PathBuf::from),
            log_bytes: session.log_bytes,
        }
    }
}

/// [`SessionCounts`], as it crosses the connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireCounts {
    pub messages: Option<u64>,
    pub subagents: u64,
}

impl From<SessionCounts> for WireCounts {
    fn from(counts: SessionCounts) -> Self {
        Self {
            messages: counts.messages.map(|messages| messages as u64),
            subagents: counts.subagents as u64,
        }
    }
}

impl From<WireCounts> for SessionCounts {
    fn from(counts: WireCounts) -> Self {
        Self {
            messages: counts
                .messages
                .map(|messages| usize::try_from(messages).unwrap_or(usize::MAX)),
            subagents: usize::try_from(counts.subagents).unwrap_or(usize::MAX),
        }
    }
}

/// [`SubagentSummary`], as it crosses the connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireSubagent {
    pub id: String,
    pub kind: String,
    pub description: String,
    pub tool_use_id: String,
    pub background: bool,
    pub spawned_at_ms: u64,
}

impl From<&SubagentSummary> for WireSubagent {
    fn from(subagent: &SubagentSummary) -> Self {
        Self {
            id: subagent.id.to_string(),
            kind: subagent.kind.to_string(),
            description: subagent.description.to_string(),
            tool_use_id: subagent.tool_use_id.to_string(),
            background: subagent.background,
            spawned_at_ms: time_to_millis(subagent.spawned_at),
        }
    }
}

impl From<WireSubagent> for SubagentSummary {
    fn from(subagent: WireSubagent) -> Self {
        Self {
            id: Arc::from(subagent.id),
            kind: Arc::from(subagent.kind),
            description: Arc::from(subagent.description),
            tool_use_id: Arc::from(subagent.tool_use_id),
            background: subagent.background,
            spawned_at: millis_to_time(subagent.spawned_at_ms),
        }
    }
}

/// [`TurnMark`], as it crosses the connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mark", rename_all = "snake_case")]
pub enum WireTurnMark {
    Prompt,
    Working,
    ToolCall { id: String },
    EndTurn { message_id: Option<String> },
    TurnDuration { background_pending: bool },
    Interrupt,
}

impl From<&TurnMark> for WireTurnMark {
    fn from(mark: &TurnMark) -> Self {
        match mark {
            TurnMark::Prompt => Self::Prompt,
            TurnMark::Working => Self::Working,
            TurnMark::ToolCall(id) => Self::ToolCall { id: id.to_string() },
            TurnMark::EndTurn { message_id } => Self::EndTurn {
                message_id: message_id.as_ref().map(|id| id.to_string()),
            },
            TurnMark::TurnDuration { background_pending } => Self::TurnDuration {
                background_pending: *background_pending,
            },
            TurnMark::Interrupt => Self::Interrupt,
        }
    }
}

impl From<WireTurnMark> for TurnMark {
    fn from(mark: WireTurnMark) -> Self {
        match mark {
            WireTurnMark::Prompt => Self::Prompt,
            WireTurnMark::Working => Self::Working,
            WireTurnMark::ToolCall { id } => Self::ToolCall(Arc::from(id)),
            WireTurnMark::EndTurn { message_id } => Self::EndTurn {
                message_id: message_id.map(Arc::from),
            },
            WireTurnMark::TurnDuration { background_pending } => {
                Self::TurnDuration { background_pending }
            }
            WireTurnMark::Interrupt => Self::Interrupt,
        }
    }
}

/// [`SubagentEvent`], as it crosses the connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WireSubagentEvent {
    Stopped { id: String },
    Resumed { id: String },
}

impl From<&SubagentEvent> for WireSubagentEvent {
    fn from(event: &SubagentEvent) -> Self {
        match event {
            SubagentEvent::Stopped(id) => Self::Stopped { id: id.to_string() },
            SubagentEvent::Resumed(id) => Self::Resumed { id: id.to_string() },
        }
    }
}

impl From<WireSubagentEvent> for SubagentEvent {
    fn from(event: WireSubagentEvent) -> Self {
        match event {
            WireSubagentEvent::Stopped { id } => Self::Stopped(Arc::from(id)),
            WireSubagentEvent::Resumed { id } => Self::Resumed(Arc::from(id)),
        }
    }
}

/// [`TranscriptProgress`], as it crosses the connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireProgress {
    pub tool_use_ids: Vec<String>,
    pub turn_marks: Vec<WireTurnMark>,
    pub subagent_events: Vec<WireSubagentEvent>,
    pub scanned_to: u64,
    pub restarted: bool,
    pub identity: Option<u64>,
    /// Absent from an older build, and then read as nothing more to read.
    #[serde(default)]
    pub more_to_read: bool,
}

impl From<&TranscriptProgress> for WireProgress {
    fn from(progress: &TranscriptProgress) -> Self {
        Self {
            tool_use_ids: progress
                .tool_use_ids
                .iter()
                .map(|id| id.to_string())
                .collect(),
            turn_marks: progress.turn_marks.iter().map(Into::into).collect(),
            subagent_events: progress.subagent_events.iter().map(Into::into).collect(),
            scanned_to: progress.scanned_to,
            restarted: progress.restarted,
            identity: progress.identity,
            more_to_read: progress.more_to_read,
        }
    }
}

impl From<WireProgress> for TranscriptProgress {
    fn from(progress: WireProgress) -> Self {
        Self {
            tool_use_ids: progress.tool_use_ids.into_iter().map(Arc::from).collect(),
            turn_marks: progress.turn_marks.into_iter().map(Into::into).collect(),
            subagent_events: progress
                .subagent_events
                .into_iter()
                .map(Into::into)
                .collect(),
            scanned_to: progress.scanned_to,
            restarted: progress.restarted,
            identity: progress.identity,
            more_to_read: progress.more_to_read,
        }
    }
}

/// Milliseconds since the epoch, saturating: a time before the epoch reads as
/// the epoch and one past `u64` milliseconds as the last of them.
pub fn time_to_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// The inverse of [`time_to_millis`], to the millisecond.
pub fn millis_to_time(millis: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(millis)
}

pub fn duration_to_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn linux_session() -> SessionSummary {
        SessionSummary {
            id: Arc::from("00000000-0000-4000-8000-000000000001"),
            agent: AgentKind::Claude,
            title: "Fix the parser".to_string(),
            preview: "Done".to_string(),
            preview_speaker: Some(Speaker::Agent),
            cwd: PathBuf::from("/home/u/p"),
            branch: Some("main".to_string()),
            model: Some("claude-opus-5".to_string()),
            updated_at: UNIX_EPOCH + Duration::from_millis(1_762_000_000_123),
            log_path: Some(PathBuf::from(
                "/home/u/.claude/projects/-home-u-p/00000000-0000-4000-8000-000000000001.jsonl",
            )),
            log_bytes: 4096,
        }
    }

    /// The host is Linux and the client may well be Windows. The path a
    /// session ran in has to come back as the host spelled it, and still sit
    /// inside the project's root, or the list filters it away.
    #[test]
    fn a_linux_path_survives_the_round_trip_whatever_the_client() {
        let session = linux_session();
        let json = serde_json::to_string(&WireSession::from(&session)).unwrap();
        let back: SessionSummary = serde_json::from_str::<WireSession>(&json).unwrap().into();

        assert_eq!(back.cwd.to_string_lossy(), "/home/u/p");
        assert_eq!(
            back.log_path.as_deref().map(Path::to_string_lossy),
            session.log_path.as_deref().map(Path::to_string_lossy)
        );
        assert!(back.is_within(&[PathBuf::from("/home/u/p")]));
        assert!(!back.is_within(&[PathBuf::from("/home/u/p-other")]));
        assert_eq!(back, session);
    }

    /// Sorting is by `updated_at`, and a time that drifts on the way would
    /// reorder the list. Whole milliseconds are what crosses, so two sessions a
    /// millisecond apart keep their order.
    #[test]
    fn times_cross_to_the_millisecond_and_keep_their_order() {
        let earlier = UNIX_EPOCH + Duration::from_millis(1_000);
        let later = UNIX_EPOCH + Duration::from_millis(1_001);
        assert!(millis_to_time(time_to_millis(earlier)) < millis_to_time(time_to_millis(later)));
        assert_eq!(time_to_millis(UNIX_EPOCH - Duration::from_secs(1)), 0);
    }

    #[test]
    fn progress_from_a_server_that_never_says_whether_more_is_left_has_none_left() {
        let json = r#"{"tool_use_ids":[],"turn_marks":[],"subagent_events":[],"scanned_to":5,"restarted":false,"identity":null}"#;
        let progress: WireProgress = serde_json::from_str(json).unwrap();
        assert!(!progress.more_to_read);
    }

    #[test]
    fn every_op_and_result_survives_serde() {
        let session = WireSession::from(&linux_session());
        let ops = vec![
            SessionOp::List {
                agents: vec![WireAgent::Claude, WireAgent::OpenCode],
            },
            SessionOp::Find {
                agent: WireAgent::Codex,
                id: "x".to_string(),
            },
            SessionOp::Counts {
                session: session.clone(),
            },
            SessionOp::Subagents {
                session: session.clone(),
            },
            SessionOp::TranscriptProgress {
                session: session.clone(),
                from: 10,
                max_bytes: 1024,
            },
            SessionOp::Successor {
                session: session.clone(),
                since_ms: 5,
                claimed: vec!["a".to_string()],
            },
            SessionOp::BackgroundQuietFor {
                session: session.clone(),
            },
            SessionOp::DirectoriesExist {
                paths: vec!["/home/u/p".to_string()],
            },
            SessionOp::PathsExist {
                paths: vec!["/home/u/p/s.jsonl".to_string()],
                kind: WirePathKind::File,
            },
            SessionOp::Delete {
                agent: WireAgent::Copilot,
                id: "y".to_string(),
            },
        ];
        for op in ops {
            let json = serde_json::to_string(&op).unwrap();
            assert_eq!(serde_json::from_str::<SessionOp>(&json).unwrap(), op);
        }

        let progress = TranscriptProgress {
            tool_use_ids: vec![Arc::from("toolu_1")],
            turn_marks: vec![
                TurnMark::Prompt,
                TurnMark::Working,
                TurnMark::ToolCall(Arc::from("toolu_1")),
                TurnMark::EndTurn {
                    message_id: Some(Arc::from("m1")),
                },
                TurnMark::TurnDuration {
                    background_pending: true,
                },
                TurnMark::Interrupt,
            ],
            subagent_events: vec![
                SubagentEvent::Stopped(Arc::from("agent-a")),
                SubagentEvent::Resumed(Arc::from("agent-a")),
            ],
            scanned_to: 99,
            restarted: true,
            identity: Some(u64::MAX),
            more_to_read: true,
        };
        let subagent = SubagentSummary {
            id: Arc::from("agent-a"),
            kind: Arc::from("reviewer"),
            description: Arc::from("Review it"),
            tool_use_id: Arc::from("toolu_1"),
            background: true,
            spawned_at: UNIX_EPOCH + Duration::from_millis(77),
        };
        let results = vec![
            SessionOpResult::List {
                sessions: vec![session.clone()],
                unavailable: vec![WireUnavailable {
                    agent: WireAgent::Codex,
                    reason: "missing".to_string(),
                }],
            },
            SessionOpResult::Find { session: None },
            SessionOpResult::Counts {
                counts: SessionCounts {
                    messages: Some(3),
                    subagents: 1,
                }
                .into(),
            },
            SessionOpResult::Subagents {
                subagents: vec![WireSubagent::from(&subagent)],
            },
            SessionOpResult::TranscriptProgress {
                progress: WireProgress::from(&progress),
            },
            SessionOpResult::Successor {
                session: Some(session),
            },
            SessionOpResult::BackgroundQuietFor {
                quiet_for_ms: Some(1500),
            },
            SessionOpResult::DirectoriesExist {
                exists: vec![true, false],
            },
            SessionOpResult::PathsExist {
                exists: vec![false, true],
            },
            SessionOpResult::Delete {
                outcome: WireDeleteOutcome::BinaryMissing {
                    binary: "opencode".to_string(),
                },
            },
            SessionOpResult::Delete {
                outcome: WireDeleteOutcome::NothingToDelete,
            },
        ];
        for result in results {
            let json = serde_json::to_string(&result).unwrap();
            assert_eq!(
                serde_json::from_str::<SessionOpResult>(&json).unwrap(),
                result
            );
        }

        let back: TranscriptProgress = serde_json::from_str::<WireProgress>(
            &serde_json::to_string(&WireProgress::from(&progress)).unwrap(),
        )
        .unwrap()
        .into();
        assert_eq!(back, progress);
        let back: SubagentSummary = WireSubagent::from(&subagent).into();
        assert_eq!(back, subagent);
    }
}
