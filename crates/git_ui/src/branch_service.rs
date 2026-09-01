//! Branch operations, free of any UI.
//!
//! Both the branch picker (a modal) and the branch panel (a dock panel) drive
//! git through this module, so the two can never drift apart on what "checkout"
//! or "delete" means. Split out along the same seam as `worktree_service.rs`
//! beside it.
//!
//! Two rules hold every function here honest:
//!
//! - **No UI state.** Nothing in this file knows about selection, queries, or
//!   modifiers. If a function needs a `selected_index`, it belongs in the caller.
//! - **No toasts, no dismissals.** Every function hands back a `Task` carrying a
//!   `Result`. The caller decides what a failure looks like — the modal prompts,
//!   the panel raises a toast, a test just asserts.

use std::sync::Arc;

use anyhow::Result;
use collections::HashSet;
use git::repository::Branch;
use gpui::{AsyncApp, Entity};
use project::git_store::Repository;

/// Collapses the raw ref list into what a human wants to see: a remote branch
/// that some local branch already tracks is dropped in favour of that local
/// branch, and the rest are ordered head-first, then by recency.
///
/// Moved here verbatim from `branch_picker` — the ordering is load-bearing for
/// both callers, so it is tested here rather than through either UI.
pub(crate) fn process_branches(branches: &Arc<[Branch]>) -> Vec<Branch> {
    let remote_upstreams: HashSet<_> = branches
        .iter()
        .filter_map(|branch| {
            branch
                .upstream
                .as_ref()
                .filter(|upstream| upstream.is_remote())
                .map(|upstream| upstream.ref_name.clone())
        })
        .collect();

    let mut result: Vec<Branch> = branches
        .iter()
        .filter(|branch| !remote_upstreams.contains(&branch.ref_name))
        .cloned()
        .collect();

    result.sort_by_key(|branch| {
        (
            !branch.is_head,
            branch
                .most_recent_commit
                .as_ref()
                .map(|commit| 0 - commit.commit_timestamp),
        )
    });

    result
}

/// Switches the repository to `branch_name`.
///
/// Says nothing about the working tree: a dirty checkout fails here and the
/// caller decides whether to stash, branch off into a worktree, or give up.
pub(crate) async fn checkout(
    repo: Entity<Repository>,
    branch_name: String,
    cx: &mut AsyncApp,
) -> Result<()> {
    repo.update(cx, |repo, _| repo.change_branch(branch_name))
        .await??;
    Ok(())
}

/// Creates a branch, optionally from `base_branch` rather than from HEAD.
///
/// Spaces in `name` become hyphens, matching what the picker has always done —
/// git would reject them and the user plainly meant a hyphen.
pub(crate) async fn create_branch(
    repo: Entity<Repository>,
    name: String,
    base_branch: Option<String>,
    cx: &mut AsyncApp,
) -> Result<()> {
    let name = name.replace(' ', "-");
    repo.update(cx, |repo, _| repo.create_branch(name, base_branch))
        .await??;
    Ok(())
}

pub(crate) async fn rename_branch(
    repo: Entity<Repository>,
    branch: String,
    new_name: String,
    cx: &mut AsyncApp,
) -> Result<()> {
    let new_name = new_name.replace(' ', "-");
    repo.update(cx, |repo, _| repo.rename_branch(branch, new_name))
        .await??;
    Ok(())
}

pub(crate) async fn delete_branch(
    repo: Entity<Repository>,
    is_remote: bool,
    name: String,
    cx: &mut AsyncApp,
) -> Result<()> {
    repo.update(cx, |repo, _| repo.delete_branch(is_remote, name))
        .await??;
    Ok(())
}

/// Adds a remote to `.git/config`. Note what this is *not*: it does not create
/// a repository on GitHub or anywhere else, it only records a URL locally.
pub(crate) async fn create_remote(
    repo: Entity<Repository>,
    remote_name: String,
    remote_url: String,
    cx: &mut AsyncApp,
) -> Result<()> {
    repo.update(cx, |repo, _| repo.create_remote(remote_name, remote_url))
        .await??;
    Ok(())
}

#[cfg(test)]
mod tests;
