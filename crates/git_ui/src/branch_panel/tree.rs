//! Flattening the panel's contents into one list of rows.
//!
//! The tree is built by a plain function rather than rendered recursively, for
//! two reasons. `uniform_list` needs to know its length up front to virtualise,
//! and a pure `Vec<TreeRow>` can be asserted against in a test without standing
//! up a window. Everything about *what* the panel shows is decided here;
//! `render_tree` only decides how a row looks.

use std::sync::Arc;

use git::repository::{Branch, Tag as GitTag, Worktree as GitWorktree};
use git::stash::StashEntry;
use gpui::SharedString;
use project::git_store::RepositoryId;

/// The collapsible groupings under a repository.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum SectionKind {
    Local,
    Remote,
    Worktrees,
    Stashes,
    Tags,
}

impl SectionKind {
    pub(crate) const ALL: [SectionKind; 5] = [
        SectionKind::Local,
        SectionKind::Remote,
        SectionKind::Worktrees,
        SectionKind::Stashes,
        SectionKind::Tags,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            SectionKind::Local => "Local",
            SectionKind::Remote => "Remote",
            SectionKind::Worktrees => "Worktrees",
            SectionKind::Stashes => "Stashes",
            SectionKind::Tags => "Tags",
        }
    }
}

/// Identifies a collapsible row. Kept separate from [`TreeRow`] because the
/// expanded set outlives any particular build: rows are rebuilt on every change,
/// the set of what the user opened is not.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum RowKey {
    Repo(RepositoryId),
    Section(RepositoryId, SectionKind),
    RemoteGroup(RepositoryId, SharedString),
}

impl RowKey {
    pub(crate) fn repository_id(&self) -> RepositoryId {
        match self {
            RowKey::Repo(id) | RowKey::Section(id, _) | RowKey::RemoteGroup(id, _) => *id,
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
    Section {
        id: RepositoryId,
        kind: SectionKind,
        count: usize,
        expanded: bool,
    },
    RemoteGroup {
        id: RepositoryId,
        remote: SharedString,
        count: usize,
        expanded: bool,
    },
    Branch {
        id: RepositoryId,
        branch: Branch,
        depth: usize,
    },
    /// Acts on its own path, so it needs no repository id.
    Worktree {
        worktree: GitWorktree,
    },
    Stash {
        id: RepositoryId,
        entry: StashEntry,
    },
    Tag {
        id: RepositoryId,
        tag: GitTag,
    },
    /// A section that is open but has nothing in it. Without this the user
    /// cannot tell "empty" from "still loading" or from a stray click.
    Empty {
        label: SharedString,
    },
}

impl TreeRow {
    /// The key to toggle when this row is clicked, or `None` for a leaf.
    pub(crate) fn toggle_key(&self) -> Option<RowKey> {
        match self {
            TreeRow::Repo { id, .. } => Some(RowKey::Repo(*id)),
            TreeRow::Section { id, kind, .. } => Some(RowKey::Section(*id, *kind)),
            TreeRow::RemoteGroup { id, remote, .. } => {
                Some(RowKey::RemoteGroup(*id, remote.clone()))
            }
            _ => None,
        }
    }

    pub(crate) fn depth(&self) -> usize {
        match self {
            TreeRow::Repo { .. } => 0,
            TreeRow::Section { .. } => 1,
            TreeRow::RemoteGroup { .. } => 2,
            TreeRow::Branch { depth, .. } => *depth,
            TreeRow::Worktree { .. }
            | TreeRow::Stash { .. }
            | TreeRow::Tag { .. }
            | TreeRow::Empty { .. } => 2,
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
    pub(crate) worktrees: Arc<[GitWorktree]>,
    pub(crate) stashes: Arc<[StashEntry]>,
    /// Unlike the other fields this is not on `RepositorySnapshot`: the store
    /// does not track tags, so the panel loads them itself the first time the
    /// Tags section is opened, and never before.
    pub(crate) tags: Arc<[GitTag]>,
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
