//! Reading the session histories that agent CLIs leave on disk.
//!
//! Two agents, two unrelated stores, one trait:
//!
//! - **Claude Code** appends JSONL to `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`
//!   with a sidecar directory of subagent transcripts beside it.
//! - **Codex** keeps one row per thread in `~/.codex/state_<schema>.sqlite` and a
//!   rollout transcript under `~/.codex/sessions/`.
//!
//! Neither format is documented and neither belongs to this editor, so the rule
//! throughout is that a field degrades on its own: a store that has changed shape
//! costs a column, never the panel. Nothing here is async and nothing here
//! deletes — callers run these on a background executor, and the one destructive
//! act goes through the app's own `Fs` in the layer that owns the confirmation.

mod claude;
mod claude_log;
mod codex;
mod provider;
mod summary;

pub use claude::ClaudeProvider;
pub use codex::CodexProvider;
pub use provider::SessionProvider;
pub use summary::{
    AgentKind, Availability, Fork, ResumeCommand, SessionCounts, SessionSummary, Speaker,
};

use std::sync::Arc;

/// The stores this editor knows how to read, in the order their sessions should
/// be merged.
pub fn default_providers() -> Vec<Arc<dyn SessionProvider>> {
    vec![
        Arc::new(ClaudeProvider::new(ClaudeProvider::default_root())),
        Arc::new(CodexProvider::new(CodexProvider::default_root())),
    ]
}

/// Every session every provider can see, newest first.
///
/// A provider that fails is logged and skipped rather than failing the sweep: one
/// unreadable store must not hide the other's history.
pub fn list_all(providers: &[Arc<dyn SessionProvider>]) -> Vec<SessionSummary> {
    let mut sessions = Vec::new();
    for provider in providers {
        match provider.list() {
            Ok(mut found) => sessions.append(&mut found),
            Err(error) => log::warn!(
                "listing {} sessions failed: {error}",
                provider.agent().label()
            ),
        }
    }
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    sessions
}
