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

use crate::branch_panel::tree::{AgentEntry, RepoData, TreeRow};
use crate::branch_service::process_branches;
use workspace::Workspace;

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

    /// Marks the tree stale when an agent tab opens or closes.
    ///
    /// Without this the panel only learned about agents when something *else*
    /// rebuilt it -- a git event, or switching checkouts, which builds a whole
    /// new panel. So pressing New Agent on the rail or in the editor added
    /// nothing visible, and the list only caught up once you switched away and
    /// back. That is not a refresh; that is a new panel.
    ///
    /// `ItemAdded` is filtered to agent tabs, because opening a file is not
    /// news here. `ItemRemoved` carries only an id, so it cannot be filtered
    /// the same way -- it is acted on only while the panel is actually showing
    /// an open tab, which is the only case where a close could change anything.
    pub(crate) fn observe_agent_tabs(
        cx: &mut Context<Self>,
        workspace: &Entity<Workspace>,
    ) -> Subscription {
        cx.subscribe(workspace, |panel, _, event, cx| match event {
            workspace::Event::ItemAdded { item } => {
                if item.downcast::<agent_ui::AgentView>().is_some() {
                    panel.mark_stale(cx);
                }
            }
            workspace::Event::ItemRemoved { .. } if panel.lists_an_open_tab() => {
                panel.mark_stale(cx);
            }
            _ => {}
        })
    }

    /// Whether any row currently shows a tab, rather than only transcripts.
    fn lists_an_open_tab(&self) -> bool {
        self.rows.iter().any(|row| match row {
            TreeRow::Worktree { agents, .. } => agents.iter().any(|agent| agent.is_open()),
            _ => false,
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
                let checkouts = crate::branch_panel::tree::order_checkouts(
                    crate::branch_panel::tree::all_checkouts(
                        repo.work_directory_abs_path.as_ref(),
                        repo.branch.as_ref(),
                        repo.head_commit.as_ref().map(|commit| commit.sha.clone()),
                        &repo.linked_worktrees,
                    ),
                    &self.pinned,
                    &self.manual_order,
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
    /// - **Open**: every agent tab of this window. They belong to the checkout
    ///   this workspace has open, so opening one from the editor's own New
    ///   Agent button lands under the checkout you are in -- from the moment it
    ///   opens, not from the moment its CLI answers. Whether it is working is
    ///   the mark's business, not this list's.
    /// - **Finished**: from the shared session index, one hash per checkout.
    fn agents_by_checkout(
        &self,
        worktrees: &[git::repository::Worktree],
        cx: &App,
    ) -> collections::HashMap<Arc<Path>, Arc<[AgentEntry]>> {
        let mut by_path: collections::HashMap<Arc<Path>, Vec<AgentEntry>> = Default::default();
        // Sessions an open tab is already showing. Their transcripts are on
        // disk and the index finds them, so without this the same session is
        // listed twice -- once as the tab you are looking at, once as a
        // finished one you could resume into a second copy of itself.
        let mut open_sessions: collections::HashSet<String> = Default::default();

        if let Some(workspace) = self.workspace.upgrade() {
            // The checkout this window is in: the worktree whose path a root of
            // the workspace sits under. A running agent has no other checkout
            // it could be editing.
            let roots = workspace.read(cx).root_paths(cx);
            let here = worktrees
                .iter()
                .find(|worktree| roots.iter().any(|root| root.starts_with(&worktree.path)));

            if let Some(here) = here {
                let mut open = Vec::new();
                for view in workspace.read(cx).items_of_type::<agent_ui::AgentView>(cx) {
                    let tab = view.read(cx);
                    if let Some(id) = tab.session_id() {
                        open_sessions.insert(id.to_string());
                    }
                    open.push(AgentEntry::Open {
                        label: tab.tab_label(),
                        agent: tab.agent_id().to_string().into(),
                        view: view.downgrade(),
                    });
                }
                if !open.is_empty() {
                    by_path
                        .entry(Arc::from(here.path.as_path()))
                        .or_default()
                        .extend(open);
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
            let past = index
                .sessions_for(&path)
                .filter(|session| !open_sessions.contains(session.id.as_ref()))
                .map(|session| AgentEntry::Past {
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
