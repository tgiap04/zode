//! Reading the session histories that agent CLIs leave on disk.
//!
//! Four agents, four unrelated stores, one trait:
//!
//! - **Claude Code** appends JSONL to `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`
//!   with a sidecar directory of subagent transcripts beside it.
//! - **Codex** keeps one row per thread in `~/.codex/state_<schema>.sqlite` and a
//!   rollout transcript under `~/.codex/sessions/`.
//! - **Copilot** keeps a directory per session under `~/.copilot/session-state/`,
//!   pairing a `workspace.yaml` of flat scalars with an `events.jsonl` of typed
//!   events.
//! - **opencode** keeps sessions, messages and parts as rows in one sqlite
//!   database, `~/.local/share/opencode/opencode.db`, and offers no way for
//!   another program to write to it -- so unlike the other three, deleting an
//!   opencode session means asking its own CLI, not moving a file to the trash.
//!
//! Neither format is documented and neither belongs to this editor, so the rule
//! throughout is that a field degrades on its own: a store that has changed shape
//! costs a column, never the panel. Nothing here is async and nothing here
//! deletes — callers run these on a background executor, and the one destructive
//! act goes through the app's own `Fs`, or the agent's own CLI, in the layer that
//! owns the confirmation.

mod claude;
mod claude_log;
mod codex;
mod copilot;
mod opencode;
mod provider;
mod session_index;
mod summary;

pub use claude::ClaudeProvider;
pub use codex::CodexProvider;
pub use copilot::CopilotProvider;
pub use opencode::OpenCodeProvider;
pub use provider::SessionProvider;
pub use session_index::SessionIndex;
pub use summary::{
    AgentCommand, AgentKind, Availability, Deletion, Fork, SessionCounts, SessionSummary, Speaker,
};

use std::sync::Arc;

/// The stores this editor knows how to read, in the order their sessions should
/// be merged.
pub fn default_providers() -> Vec<Arc<dyn SessionProvider>> {
    vec![
        Arc::new(ClaudeProvider::new(ClaudeProvider::default_root())),
        Arc::new(CodexProvider::new(CodexProvider::default_root())),
        Arc::new(CopilotProvider::new(CopilotProvider::default_root())),
        Arc::new(OpenCodeProvider::new(OpenCodeProvider::default_root())),
    ]
}

/// The store for one agent.
///
/// Built on demand rather than looked up in a registry: a provider is a path and
/// nothing else, so constructing one costs less than caching it would. Kept
/// beside [`default_providers`] so the roster of known stores stays in one place.
pub fn provider_for(agent: AgentKind) -> Arc<dyn SessionProvider> {
    match agent {
        AgentKind::Claude => Arc::new(ClaudeProvider::new(ClaudeProvider::default_root())),
        AgentKind::Codex => Arc::new(CodexProvider::new(CodexProvider::default_root())),
        AgentKind::Copilot => Arc::new(CopilotProvider::new(CopilotProvider::default_root())),
        AgentKind::OpenCode => Arc::new(OpenCodeProvider::new(OpenCodeProvider::default_root())),
    }
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
