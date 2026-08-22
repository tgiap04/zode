use crate::{Availability, Fork, ResumeCommand, SessionCounts, SessionSummary};
use anyhow::Result;
use std::path::PathBuf;

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
