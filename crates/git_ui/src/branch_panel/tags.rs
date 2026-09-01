//! Loading and checking out tags.
//!
//! Tags are the one thing the panel does fetch for itself: unlike branches,
//! worktrees and stashes, the git store keeps no tag cache. So they are loaded
//! lazily -- the first time someone opens the Tags section for a repository,
//! and never for a repository whose section stays shut. A release repository
//! with a thousand tags costs nothing until it is asked for.

use std::sync::Arc;

use git::repository::Tag as GitTag;
use gpui::Window;
use project::git_store::RepositoryId;
use ui::prelude::*;

use crate::branch_panel::panel::BranchPanel;

impl BranchPanel {
    pub(crate) fn tags_for(&self, id: RepositoryId) -> Arc<[GitTag]> {
        self.tags.get(&id).cloned().unwrap_or_else(|| Arc::from([]))
    }

    /// Loads a repository's tags once. Repeat calls while a load is in flight,
    /// or after one has completed, do nothing.
    pub(crate) fn load_tags(&mut self, id: RepositoryId, cx: &mut Context<Self>) {
        if self.tags.contains_key(&id) || self.tags_loading.contains(&id) {
            return;
        }
        let Some(repo) = self.repository(id, cx) else {
            return;
        };

        self.tags_loading.insert(id);
        cx.spawn(async move |panel, cx| {
            let tags = repo.update(cx, |repo, _| repo.tags()).await;
            panel
                .update(cx, |panel, cx| {
                    panel.tags_loading.remove(&id);
                    // A failed load stores an empty list rather than nothing:
                    // otherwise every rebuild would retry a repository whose
                    // git binary is unhappy, forever.
                    let tags = tags.ok().and_then(|tags| tags.ok()).unwrap_or_default();
                    panel.tags.insert(id, Arc::from(tags));
                    panel.stale = true;
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Checks out a tag after saying plainly what that does. Detached HEAD is
    /// normal git, but a user who lands there without warning reads it as the
    /// editor having broken something.
    /// Checks out a tag after saying plainly what that does. Detached HEAD is
    /// normal git, but a user who lands there without warning reads it as the
    /// editor having broken something.
    ///
    /// A dirty checkout is the same hazard as it is for a branch, so it gets
    /// the same treatment: say so, and offer to stash first. There is no
    /// worktree option here -- a worktree is created at a branch, and a tag is
    /// not one.
    pub(crate) fn checkout_tag(
        &mut self,
        id: RepositoryId,
        tag: GitTag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.repository(id, cx) else {
            return;
        };

        let dirty = crate::branch_panel::actions::summary_is_dirty(repo.read(cx).status_summary());

        let detail = if dirty {
            "This checkout has uncommitted changes, and the repository will be \
             left in detached HEAD. Stashing puts your changes in the stash list; \
             selecting a branch afterwards returns you to normal."
        } else {
            "The repository will be left in detached HEAD. \
             Selecting a branch afterwards returns you to normal."
        };
        let choices: &[&str] = if dirty {
            &["Stash and check out", "Cancel"]
        } else {
            &["Check Out", "Cancel"]
        };

        let confirm = window.prompt(
            gpui::PromptLevel::Info,
            &format!("Check out {}?", tag.name),
            Some(detail),
            choices,
            cx,
        );

        let name = tag.name.to_string();
        let stash = dirty.then(|| repo.update(cx, |repo, cx| repo.stash_all(cx)));
        let task = cx.spawn(async move |_, cx| {
            if confirm.await.ok() != Some(0) {
                return Ok(());
            }
            if let Some(stash) = stash {
                stash.await?;
            }
            repo.update(cx, |repo, _| repo.checkout_tag(name)).await?
        });
        let label = if dirty {
            "git stash && git checkout tags/"
        } else {
            "git checkout tags/"
        };
        self.report_failure(task, label, window, cx);
    }
}
