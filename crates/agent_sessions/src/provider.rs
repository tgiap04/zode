use crate::{
    AgentCommand, Availability, CompletedSubagents, Deletion, Fork, SessionCounts, SessionSummary,
    SubagentSummary,
};
use anyhow::Result;
use std::path::Path;

/// Whether `id` is safe to use as a single path component.
///
/// Session ids reach this crate from a database the editor owns, and two of the
/// three stores locate a session by joining the id onto a directory. An id
/// carrying a separator or `..` would therefore read outside the store, so the
/// join sites check first. Callers validate shape (a UUID) at their own layer;
/// this is the narrower guarantee that a lookup cannot escape its root.
pub(crate) fn is_safe_component(id: &str) -> bool {
    !id.is_empty()
        && id != "."
        && id != ".."
        && !id.contains('/')
        && !id.contains('\\')
        && !id.contains('\0')
}

/// One agent's session store.
///
/// Every method is blocking: these read files and sqlite. Callers run them on a
/// background executor — keeping the trait synchronous is what lets the whole
/// crate be tested without a window or an async runtime.
///
/// Nothing here deletes anything. [`Self::deletion`] only *builds* what a
/// delete would take — a list of paths to trash, or a command to hand another
/// CLI — and the caller performs it: paths through its own `Fs`, a command
/// through a process spawn. The one destructive act in this feature therefore
/// happens in the layer that also owns the confirmation dialog, not behind a
/// trait method that could be called by accident.
pub trait SessionProvider: Send + Sync {
    fn agent(&self) -> crate::AgentKind;

    /// Whether the store can be read right now. Checked cheaply, without
    /// listing.
    fn availability(&self) -> Availability;

    /// Every session in the store, newest first. An unavailable store returns an
    /// empty list rather than an error — see [`Availability`].
    fn list(&self) -> Result<Vec<SessionSummary>>;

    /// The session with this id, if this store holds it.
    ///
    /// Returns the summary rather than a bool because the caller's next move is
    /// [`Self::resume_command`], which takes exactly this — a bool would force a
    /// second pass over the store.
    ///
    /// `Ok(None)` covers every "no" there is: the store does not hold it, the
    /// store cannot be read, the store does not exist. A caller deciding whether
    /// to resume has the same response to all three, and an `Err` here would
    /// only invite it to treat an absent CLI as a failure.
    ///
    /// Implementations must not degrade into [`Self::list`] where the store is
    /// large enough for that to be felt — Claude's is.
    fn find(&self, id: &str) -> Result<Option<SessionSummary>>;

    /// The command that starts a **new** session under an id the caller chose,
    /// rather than one the CLI picks for itself.
    ///
    /// `None` when the agent has no way to be told: only Claude has a flag for
    /// it (`--session-id`). The caller must not invent one — an id the CLI never
    /// agreed to is an id that will not be there to resume.
    fn new_session_command(&self, id: &str, cwd: &Path) -> Option<AgentCommand>;

    /// The numbers that need a full scan of one transcript.
    fn counts(&self, session: &SessionSummary) -> Result<SessionCounts>;

    /// The subagents this session spawned, newest first.
    ///
    /// Empty for every agent but Claude, and that is the honest answer rather
    /// than an unfinished one — the same split [`Self::counts`] already records.
    /// Codex writes spawn edges to `thread_spawn_edges` that nothing reads,
    /// Copilot's `--agent` runs inside the session's own transcript so there is
    /// nothing separate to name, and opencode's store can count them but cannot
    /// say what they were.
    fn subagents(&self, _session: &SessionSummary) -> Result<Vec<SubagentSummary>> {
        Ok(Vec::new())
    }

    /// The tool calls this session has reported a result for, reading from byte
    /// `from` onward.
    ///
    /// Incremental because the whole-file alternative is not affordable: a live
    /// transcript grows continuously and reaches megabytes, so re-reading it on
    /// every refresh would scan the same bytes over and over to learn one new
    /// fact. A result is only ever appended, never amended, so a forward scan
    /// never has to look back at what it already read.
    ///
    /// The caller pairs these against [`SubagentSummary::tool_use_id`]: spawned,
    /// and not yet reported, is a subagent still running. Nothing here decides
    /// that — a provider reports what the store says and no more.
    fn completed_subagents(
        &self,
        _session: &SessionSummary,
        from: u64,
    ) -> Result<CompletedSubagents> {
        Ok(CompletedSubagents {
            tool_use_ids: Vec::new(),
            scanned_to: from,
        })
    }

    /// `None` when this agent cannot honour the request — Codex and Copilot have
    /// no fork flag, so [`Fork::New`] has no command to build for them. The caller
    /// disables the control rather than inventing one.
    fn resume_command(&self, session: &SessionSummary, fork: Fork) -> Option<AgentCommand>;

    /// What a delete has to do to really remove this session. See [`Deletion`].
    fn deletion(&self, session: &SessionSummary) -> Deletion;
}
