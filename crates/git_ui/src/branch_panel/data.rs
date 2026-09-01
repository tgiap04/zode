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
use std::path::Path;
use std::sync::Arc;

use crate::branch_panel::tree::{AgentEntry, RepoData};
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
                let checkouts = crate::branch_panel::tree::all_checkouts(
                    repo.work_directory_abs_path.as_ref(),
                    repo.branch.as_ref(),
                    repo.head_commit.as_ref().map(|commit| commit.sha.clone()),
                    &repo.linked_worktrees,
                );
                RepoData {
                    id: repo.id,
                    path: repo.work_directory_abs_path.clone(),
                    name: repo_name(repo.work_directory_abs_path.as_ref()),
                    current_branch: repo
                        .branch
                        .as_ref()
                        .map(|branch| SharedString::from(branch.name().to_string())),
                    branches: process_branches(&repo.branch_list),
                    worktrees: std::sync::Arc::from(checkouts.clone()),
                    agents: self.agents_by_checkout(&checkouts, cx),
                }
            })
            .collect();

        // Stable order, or the tree reshuffles itself on every rebuild: the
        // store hands back a HashMap, whose iteration order is arbitrary.
        repos.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.0.cmp(&b.id.0)));
        repos
    }
}

impl BranchPanel {
    /// Which agents belong to which checkout.
    ///
    /// Keyed by the worktree's own path, which is what a checkout actually is:
    /// a worktree created here starts detached and has no branch name, and two
    /// repositories can both have a `main`.
    ///
    /// Two sources, gathered once per rebuild so `build_rows` stays pure:
    ///
    /// - **Running**: the agent tabs of this window whose CLI is still alive.
    ///   They belong to the checkout this workspace has open.
    /// - **Finished**: from the shared session index, one hash per checkout.
    fn agents_by_checkout(
        &self,
        worktrees: &[git::repository::Worktree],
        cx: &App,
    ) -> collections::HashMap<Arc<Path>, Arc<[AgentEntry]>> {
        let mut by_path: collections::HashMap<Arc<Path>, Vec<AgentEntry>> = Default::default();

        if let Some(workspace) = self.workspace.upgrade() {
            // The checkout this window is in: the worktree whose path a root of
            // the workspace sits under. A running agent has no other checkout
            // it could be editing.
            let roots = workspace.read(cx).root_paths(cx);
            let here = worktrees
                .iter()
                .find(|worktree| roots.iter().any(|root| root.starts_with(&worktree.path)));

            if let Some(here) = here {
                let running: Vec<_> = workspace
                    .read(cx)
                    .items_of_type::<agent_ui::AgentView>(cx)
                    .filter(|view| view.read(cx).is_working(cx))
                    .map(|view| AgentEntry::Running {
                        label: view.read(cx).tab_label(),
                        agent: view.read(cx).agent_id().to_string().into(),
                        view: view.downgrade(),
                    })
                    .collect();
                if !running.is_empty() {
                    by_path
                        .entry(Arc::from(here.path.as_path()))
                        .or_default()
                        .extend(running);
                }
            }
        }

        let Some(index) = self
            .session_store
            .as_ref()
            .map(|store| store.read(cx).index().clone())
        else {
            return finish(by_path);
        };

        for worktree in worktrees {
            let path: Arc<Path> = Arc::from(worktree.path.as_path());
            let past = index.sessions_for(&path).map(|session| AgentEntry::Past {
                label: session.title.clone().into(),
                agent: session.agent.builtin_agent_id().into(),
                id: session.id.clone(),
                updated_at: session.updated_at,
            });
            let past: Vec<_> = past.collect();
            if !past.is_empty() {
                by_path.entry(path).or_default().extend(past);
            }
        }

        finish(by_path)
    }
}

/// Freezes the gathered lists so a branch row can clone an `Arc` rather than a
/// `Vec` on every rebuild.
fn finish(
    by_path: collections::HashMap<Arc<Path>, Vec<AgentEntry>>,
) -> collections::HashMap<Arc<Path>, Arc<[AgentEntry]>> {
    by_path
        .into_iter()
        .map(|(path, entries)| (path, Arc::from(entries)))
        .collect()
}

/// The directory name is what the user calls the repository; the full path is
/// too long for a 280px panel and its tail is the part that identifies it.
fn repo_name(path: &std::path::Path) -> SharedString {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
        .into()
}
