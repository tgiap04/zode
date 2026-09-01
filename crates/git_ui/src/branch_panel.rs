//! The branch panel: repositories, branches, worktrees, stashes and tags of the
//! current project, as a collapsible tree docked beside the editor.
//!
//! The rail draws this panel's button on its own, straight off `Panel::icon` --
//! see `sidebar::Sidebar::render_rail_panels`. Nothing in the `sidebar` crate
//! knows this panel exists, and nothing there needs to.
//!
//! The panel never runs a git command. `RepositorySnapshot` already carries the
//! branch list, worktrees and stashes, and the git store announces every change
//! to them -- so this is a pure reader, and a closed panel costs nothing.

mod actions;
mod context_menu;
mod create_remote_repo;
mod create_repo_modal;
mod data;
mod dirty_prompt;
mod lifecycle;
mod new_branch;
mod panel;
mod remote;
mod render;
mod render_tree;
mod settings;
mod state;
mod tags;
mod tree;

pub use panel::{BranchPanel, Toggle, ToggleFocus, register};
pub use settings::BranchPanelSettings;
