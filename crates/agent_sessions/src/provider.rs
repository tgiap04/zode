use crate::{Availability, Fork, ResumeCommand, SessionCounts, SessionSummary};
use anyhow::Result;
use std::path::{Path, PathBuf};

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
/// Nothing here deletes anything. [`Self::paths_to_trash`] only *names* what a
/// delete would take, and the caller moves those to the OS trash through its own
/// `Fs`. The one destructive act in this feature therefore happens in the layer
/// that also owns the confirmation dialog, not behind a trait method that could
/// be called by accident.
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
    fn new_session_command(&self, id: &str, cwd: &Path) -> Option<ResumeCommand>;

    /// The numbers that need a full scan of one transcript.
    fn counts(&self, session: &SessionSummary) -> Result<SessionCounts>;

    /// `None` when this agent cannot honour the request — Codex has no
    /// `--fork-session`, so [`Fork::New`] has no command to build. The caller
    /// disables the control rather than inventing one.
    fn resume_command(&self, session: &SessionSummary, fork: Fork) -> Option<ResumeCommand>;

    /// What a delete would move to the trash, outermost first. Paths that do not
    /// exist are still listed; the caller ignores what is already gone.
    fn paths_to_trash(&self, session: &SessionSummary) -> Vec<PathBuf>;
}
