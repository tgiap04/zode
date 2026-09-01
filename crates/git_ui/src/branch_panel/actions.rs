//! What happens when a row is clicked.
//!
//! Every path through here ends in one of two places: a git operation via
//! `branch_service`, or a toast saying why it did not happen. Nothing fails
//! silently, and nothing that could lose uncommitted work runs without asking.

use anyhow::Result;
use git::repository::Branch;
use git::status::GitSummary;
use gpui::{Entity, SharedString, Task, Window};
use project::git_store::{Repository, RepositoryId};
use ui::prelude::*;

use crate::branch_panel::dirty_prompt::{DirtyChoice, DirtyPrompt};
use crate::branch_panel::panel::BranchPanel;
use crate::branch_service;
use crate::git_panel::show_error_toast;

/// Whether a checkout could destroy work in progress.
///
/// Untracked files are excluded on purpose: git carries them across a checkout
/// without complaint, so counting them would raise the prompt on every
/// repository that has build output sitting in it -- which is most of them.
/// Split out from the entity read so the rule itself is testable.
pub(crate) fn summary_is_dirty(summary: GitSummary) -> bool {
    let tracked = summary.index.added
        + summary.index.modified
        + summary.index.deleted
        + summary.worktree.added
        + summary.worktree.modified
        + summary.worktree.deleted;
    tracked + summary.conflict > 0
}

/// The local branch name a remote branch should be checked out as.
///
/// `Branch::name()` has already stripped `refs/remotes/`, leaving
/// `origin/feature`. What the user wants checked out is `feature`. A branch
/// name may itself contain slashes (`origin/release/1.0`), so only the first
/// segment -- the remote -- comes off.
pub(crate) fn local_name_for_remote_branch(remote_branch_name: &str) -> String {
    remote_branch_name
        .split_once('/')
        .map(|(_remote, rest)| rest.to_string())
        .unwrap_or_else(|| remote_branch_name.to_string())
}

impl BranchPanel {
    pub(crate) fn repository(&self, id: RepositoryId, cx: &App) -> Option<Entity<Repository>> {
        self.git_store(cx)?
            .read(cx)
            .repositories()
            .get(&id)
            .cloned()
    }

    /// Whether the checkout has changes that a branch switch could destroy.
    ///
    /// Untracked files are excluded on purpose: git carries them across a
    /// checkout without complaint, so stopping the user for them would be a
    /// false alarm on every repository with build output in it.
    fn has_uncommitted_changes(repo: &Entity<Repository>, cx: &App) -> bool {
        summary_is_dirty(repo.read(cx).status_summary())
    }

    /// Switches to `branch`, asking first if that would put uncommitted work at
    /// risk. A remote branch is resolved to a local tracking branch on the way.
    pub(crate) fn checkout_branch(
        &mut self,
        id: RepositoryId,
        branch: Branch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.repository(id, cx) else {
            return;
        };
        if branch.is_head {
            return;
        }

        if !Self::has_uncommitted_changes(&repo, cx) {
            self.run_checkout(repo, branch, window, cx);
            return;
        }

        self.ask_about_dirty_tree(repo, branch, window, cx);
    }

    /// Puts the three-way choice on screen and acts on it once the modal
    /// dismisses. Repo and branch ride along in the closure rather than in a
    /// field on the panel -- a half-finished checkout parked in panel state is
    /// a bug waiting for the next entry point to trip over it.
    fn ask_about_dirty_tree(
        &mut self,
        repo: Entity<Repository>,
        branch: Branch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let target: SharedString = branch.name().to_string().into();

        let prompt = workspace.update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, |_, cx| DirtyPrompt::new(target, cx));
            workspace.active_modal::<DirtyPrompt>(cx)
        });
        let Some(prompt) = prompt else {
            return;
        };

        cx.subscribe_in(
            &prompt,
            window,
            move |panel, prompt, _: &gpui::DismissEvent, window, cx| match prompt.read(cx).choice()
            {
                Some(DirtyChoice::Stash) => {
                    panel.stash_then_checkout(repo.clone(), branch.clone(), window, cx)
                }
                Some(DirtyChoice::Worktree) => panel.open_worktree_for(branch.clone(), window, cx),
                None => {}
            },
        )
        .detach();
    }

    fn run_checkout(
        &mut self,
        repo: Entity<Repository>,
        branch: Branch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let task = if branch.is_remote() {
            self.checkout_remote(repo, branch, cx)
        } else {
            let name = branch.name().to_string();
            cx.spawn(async move |_, cx| branch_service::checkout(repo, name, cx).await)
        };
        self.report_failure(task, "git switch", window, cx);
    }

    /// A remote branch cannot be checked out directly. If a local branch of the
    /// same name already exists, that is what the user means; otherwise create
    /// one tracking the remote.
    fn checkout_remote(
        &mut self,
        repo: Entity<Repository>,
        branch: Branch,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let short_name = local_name_for_remote_branch(branch.name());

        let local_exists = repo
            .read(cx)
            .branch_list
            .iter()
            .any(|other| !other.is_remote() && other.name() == short_name);

        let remote_ref = branch.name().to_string();
        cx.spawn(async move |_, cx| {
            if !local_exists {
                branch_service::create_branch(
                    repo.clone(),
                    short_name.clone(),
                    Some(remote_ref),
                    cx,
                )
                .await?;
            }
            branch_service::checkout(repo, short_name, cx).await
        })
    }

    fn stash_then_checkout(
        &mut self,
        repo: Entity<Repository>,
        branch: Branch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = branch.name().to_string();
        let stash = repo.update(cx, |repo, cx| repo.stash_all(cx));
        let task = cx.spawn(async move |_, cx| {
            stash.await?;
            branch_service::checkout(repo, name, cx).await
        });
        // The wording matters: if the checkout half fails the user must know
        // their work is in the stash, not lost.
        self.report_failure(task, "git stash && git switch", window, cx);
    }

    /// Runs a git task and raises a toast carrying git's own words if it fails.
    /// The panel itself needs no success handling: the repository will emit the
    /// change and the tree rebuilds from it.
    pub(crate) fn report_failure(
        &self,
        task: Task<Result<()>>,
        action: impl Into<SharedString>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = action.into();
        let workspace = self.workspace.clone();
        cx.spawn(async move |_, cx| {
            if let Err(error) = task.await {
                let Some(workspace) = workspace.upgrade() else {
                    return;
                };
                cx.update(|cx| show_error_toast(workspace, action, error, cx));
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests;
