//! The agent session history: past conversations with Claude and Codex, for the
//! project this workspace has open.
//!
//! Split three ways for the usual reason — the rules, the layout, and the things
//! the rows do are separately readable and separately testable:
//!
//! - [`panel`] holds the state and the `Panel` impl.
//! - [`list`] turns sessions into rows: which project's, which order, which group.
//!   Its rules are pure functions with tests that need no window.
//! - [`row`] draws one row, and [`actions`] is what its controls do.

mod actions;
mod list;
mod panel;
#[cfg(test)]
mod panel_tests;
mod row;

pub use actions::resume_session;
pub use panel::AgentHistoryPanel;
