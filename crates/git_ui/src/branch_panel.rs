//! The worktree panel: every checkout of the current project's repositories,
//! and the agents that have run in each of them.
//!
//! It began as a tree of branches, remotes, stashes and tags. Those each have
//! their own picker, and five collapsible groups turned out to be five things
//! to read before finding the one that matters: which checkout am I in, and
//! what is running there. So the tree is gone and the list is checkouts.
//!
//! The rail draws this panel's button on its own, straight off `Panel::icon` --
//! see `sidebar::Sidebar::render_rail_panels`. Nothing in the `sidebar` crate
//! knows this panel exists, and nothing there needs to.
//!
//! The panel never runs a git command. `RepositorySnapshot` already carries the
//! branch list, worktrees and stashes, and the git store announces every change
//! to them -- so this is a pure reader, and a closed panel costs nothing.

mod context_menu;
mod create_remote_repo;
mod create_repo_modal;
mod create_worktree_modal;
mod data;
mod lifecycle;
mod panel;
mod remote;
mod render;
mod render_tree;
mod settings;
mod state;
mod tree;

pub use panel::{BranchPanel, Toggle, ToggleFocus, register};
pub use settings::BranchPanelSettings;

/// The agents the create-worktree form offers.
///
/// The same roster the rail draws, and for the same reason it is a list rather
/// than a lookup: an agent's mark is its vendor's, so adding one is a decision
/// about icons and colour, not something to derive.
pub(crate) fn agent_choices() -> &'static [(&'static str, &'static str)] {
    &[
        (project::CLAUDE_CODE_AGENT_ID, "Claude"),
        (project::CODEX_AGENT_ID, "Codex"),
        (project::ANTIGRAVITY_AGENT_ID, "Antigravity"),
        (project::COPILOT_AGENT_ID, "Copilot"),
    ]
}
