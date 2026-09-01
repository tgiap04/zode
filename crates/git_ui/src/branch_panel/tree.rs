//! Flattening the panel's contents into one list of rows.
//!
//! The tree is built by a plain function rather than rendered recursively, for
//! two reasons. `uniform_list` needs to know its length up front to virtualise,
//! and a pure `Vec<TreeRow>` can be asserted against in a test without standing
//! up a window. Everything about *what* the panel shows is decided here;
//! `render_tree` only decides how a row looks.

use std::sync::Arc;

use git::repository::{Branch, Worktree as GitWorktree};
use gpui::SharedString;
use project::git_store::RepositoryId;

/// Identifies a collapsible row. Kept separate from [`TreeRow`] because the
/// expanded set outlives any particular build: rows are rebuilt on every change,
/// the set of what the user opened is not.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum RowKey {
    Repo(RepositoryId),
    /// The agents that have run in one worktree, keyed by its path.
    ///
    /// By path and not by branch: a worktree created here starts detached, and
    /// two repositories can both have a `main`. The path is what a checkout
    /// actually is.
    WorktreeAgents(RepositoryId, Arc<std::path::Path>),
}

impl RowKey {
    pub(crate) fn repository_id(&self) -> RepositoryId {
        match self {
            RowKey::Repo(id) | RowKey::WorktreeAgents(id, _) => *id,
        }
    }
}

/// One line in the panel. `depth` is indentation only -- the parent/child
/// relationship is already resolved by the time a row exists.
#[derive(Clone, Debug)]
pub(crate) enum TreeRow {
    Repo {
        id: RepositoryId,
        name: SharedString,
        current_branch: Option<SharedString>,
        expanded: bool,
    },
    /// One checkout of the repository: the main one, or a linked worktree.
    ///
    /// The panel's only row type below the repository. There are no sections
    /// any more -- branches, remotes, stashes and tags each have their own
    /// picker, and a tree of five collapsible groups was five things to read
    /// before finding the one that matters: which checkout am I in, and what is
    /// running there.
    Worktree {
        id: RepositoryId,
        worktree: GitWorktree,
        /// The agents that have run in this checkout. Carried by the row rather
        /// than emitted as rows of their own, so the card's border encloses
        /// them.
        agents: Arc<[AgentEntry]>,
        expanded: bool,
    },
    /// Shown when a repository has no checkout to list, which should not happen
    /// -- `git worktree list` always names the main one. Without this the panel
    /// would go blank instead of saying something is wrong.
    Empty { label: SharedString },
}

/// One agent of a checkout.
///
/// Two kinds and not one list: a CLI still running is what someone juggling
/// several worktrees is looking for, and burying it among finished transcripts
/// would be the same as not showing it.
///
/// Neither variant carries a `SessionSummary`. It holds four `String`s and two
/// `PathBuf`s, and the row needs a label -- the rest is fetched by id at click
/// time, which happens once, rather than copied on every rebuild.
#[derive(Clone, Debug)]
pub(crate) enum AgentEntry {
    Running {
        label: SharedString,
        /// The agent's builtin id, for its vendor mark and colour.
        agent: SharedString,
        view: gpui::WeakEntity<agent_ui::AgentView>,
    },
    Past {
        label: SharedString,
        agent: SharedString,
        id: std::sync::Arc<str>,
        updated_at: std::time::SystemTime,
    },
}

impl AgentEntry {
    pub(crate) fn label(&self) -> &SharedString {
        match self {
            AgentEntry::Running { label, .. } | AgentEntry::Past { label, .. } => label,
        }
    }

    pub(crate) fn agent(&self) -> &SharedString {
        match self {
            AgentEntry::Running { agent, .. } | AgentEntry::Past { agent, .. } => agent,
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        matches!(self, AgentEntry::Running { .. })
    }

    pub(crate) fn updated_at(&self) -> Option<std::time::SystemTime> {
        match self {
            AgentEntry::Running { .. } => None,
            AgentEntry::Past { updated_at, .. } => Some(*updated_at),
        }
    }
}

impl TreeRow {
    /// The key to toggle when this row is clicked, or `None` when there is
    /// nothing to open.
    pub(crate) fn toggle_key(&self) -> Option<RowKey> {
        match self {
            TreeRow::Repo { id, .. } => Some(RowKey::Repo(*id)),
            // Only when there is something to show. A disclosure that opens on
            // nothing reads as a broken control.
            TreeRow::Worktree {
                id,
                worktree,
                agents,
                ..
            } if !agents.is_empty() => Some(RowKey::WorktreeAgents(
                *id,
                Arc::from(worktree.path.as_path()),
            )),
            _ => None,
        }
    }
}

/// Everything the panel knows about one repository, read straight off
/// `RepositorySnapshot`. No git command is run to fill this in -- the git store
/// already maintains every field and tells us when one changes.
#[derive(Clone, Debug)]
pub(crate) struct RepoData {
    pub(crate) id: RepositoryId,
    /// Stable across sessions, unlike `id`. What the expanded set is keyed by
    /// on disk.
    pub(crate) path: Arc<std::path::Path>,
    pub(crate) name: SharedString,
    pub(crate) current_branch: Option<SharedString>,
    /// Already run through `branch_service::process_branches`, so a remote ref
    /// that a local branch tracks has been folded away.
    pub(crate) branches: Vec<Branch>,
    /// Agents per checkout, keyed by the worktree's own path and gathered once
    /// per rebuild so `build_rows` stays a pure function of its input.
    pub(crate) agents: collections::HashMap<Arc<std::path::Path>, std::sync::Arc<[AgentEntry]>>,
    /// Every checkout of this repository. `git worktree list` names the main
    /// one too, so this is the whole list rather than only the linked ones --
    /// `GitWorktree::is_main` tells them apart.
    pub(crate) worktrees: Arc<[GitWorktree]>,
}

/// A worktree reads best by its branch; the directory name is the fallback for a
/// detached one.
pub(crate) fn worktree_label(worktree: &GitWorktree) -> String {
    worktree
        .ref_name
        .as_ref()
        .map(|ref_name| {
            ref_name
                .strip_prefix("refs/heads/")
                .unwrap_or(ref_name.as_ref())
                .to_string()
        })
        .unwrap_or_else(|| {
            worktree
                .path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| worktree.path.display().to_string())
        })
}

mod build;
#[cfg(test)]
mod tests;

pub(crate) use build::build_rows;
