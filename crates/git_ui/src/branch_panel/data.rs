//! Reading the repositories out of the git store.
//!
//! Nothing here runs a git command. `RepositorySnapshot` already carries
//! `branch_list`, `linked_worktrees`, `stash_entries` and the current `branch`,
//! and the store keeps them current and announces every change. The panel is a
//! pure reader on top of that -- which is why a closed panel costs nothing at
//! all, and why two panels open at once do not double the git work.

use gpui::{App, Context, Entity, SharedString, Subscription};
use project::git_store::{GitStore, GitStoreEvent, RepositoryEvent};

use crate::branch_panel::panel::BranchPanel;
use crate::branch_panel::tree::RepoData;
use crate::branch_service::process_branches;

/// The repository events that can change what the tree shows.
///
/// `StatusesChanged` is deliberately absent: it fires on every keystroke that
/// dirties a file and says nothing about branches, so reacting to it would
/// rebuild the tree continuously while the user types.
fn event_changes_the_tree(event: &RepositoryEvent) -> bool {
    matches!(
        event,
        RepositoryEvent::BranchListChanged
            | RepositoryEvent::HeadChanged
            | RepositoryEvent::StashEntriesChanged
            | RepositoryEvent::GitWorktreeListChanged
    )
}

impl BranchPanel {
    pub(crate) fn git_store(&self, cx: &App) -> Option<Entity<GitStore>> {
        let workspace = self.workspace.upgrade()?;
        Some(workspace.read(cx).project().read(cx).git_store().clone())
    }

    /// Subscribes to the one event stream that matters. Held in the panel's own
    /// `_subscriptions`, so it dies with the panel rather than firing into a
    /// dropped handle forever after.
    pub(crate) fn observe_git_store(
        cx: &mut Context<Self>,
        store: &Entity<GitStore>,
    ) -> Subscription {
        cx.subscribe(store, |panel, _, event, cx| match event {
            GitStoreEvent::RepositoryUpdated(_, repository_event, _) => {
                if event_changes_the_tree(repository_event) {
                    panel.mark_stale(cx);
                }
            }
            GitStoreEvent::RepositoryAdded | GitStoreEvent::RepositoryRemoved(_) => {
                panel.mark_stale(cx);
            }
            _ => {}
        })
    }

    /// Re-reads every repository snapshot into the shape the tree wants.
    ///
    /// Cheap by construction -- it clones already-materialised `Arc`s and runs
    /// the branch list through the same folding the picker uses. Called only
    /// when the panel is visible and something actually changed.
    pub(crate) fn collect_repos(&self, cx: &App) -> Vec<RepoData> {
        let Some(store) = self.git_store(cx) else {
            return Vec::new();
        };

        let mut repos: Vec<RepoData> = store
            .read(cx)
            .repositories()
            .values()
            .map(|repo| {
                let repo = repo.read(cx);
                RepoData {
                    id: repo.id,
                    path: repo.work_directory_abs_path.clone(),
                    name: repo_name(repo.work_directory_abs_path.as_ref()),
                    current_branch: repo
                        .branch
                        .as_ref()
                        .map(|branch| SharedString::from(branch.name().to_string())),
                    branches: process_branches(&repo.branch_list),
                    worktrees: repo.linked_worktrees.clone(),
                    stashes: repo.stash_entries.entries.clone(),
                    tags: self.tags_for(repo.id),
                }
            })
            .collect();

        // Stable order, or the tree reshuffles itself on every rebuild: the
        // store hands back a HashMap, whose iteration order is arbitrary.
        repos.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.0.cmp(&b.id.0)));
        repos
    }
}

/// The directory name is what the user calls the repository; the full path is
/// too long for a 280px panel and its tail is the part that identifies it.
fn repo_name(path: &std::path::Path) -> SharedString {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
        .into()
}
