//! What is shown before a fork's pull request is checked out, and how git is
//! answered when that fetch needs a password.
//!
//! Worktree trust is kept by path, so switching a trusted worktree to a
//! stranger's branch lets that branch's project settings, tasks, build
//! scripts and language servers run with the trust the folder already has.
//! The trust store has no way to drop one worktree back to restricted mode
//! while its language servers are running, so the user is warned instead,
//! before anything is fetched.

use askpass::AskPassDelegate;
use git_ui::askpass_modal::askpass_delegate_for_workspace;
use github::PullRequestDetails;
use gpui::{AnyWindowHandle, App, WeakEntity};
use workspace::Workspace;

use crate::untrusted_text::reveal_hidden_characters;

/// The facts the user is asked to accept before a fork is fetched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForkWarning {
    pub number: u64,
    /// `@login`, or "a deleted account".
    pub author: String,
    /// `owner/name`, or "a deleted repository".
    pub source_repository: String,
    /// The working tree whose branch the checkout switches.
    pub worktree: Option<String>,
}

impl ForkWarning {
    /// `None` when the head lives in the selected repository.
    pub fn for_pull_request(details: &PullRequestDetails) -> Option<Self> {
        let pull_request = &details.pull_request;
        if !pull_request.is_cross_repository {
            return None;
        }
        let author = pull_request
            .author
            .as_deref()
            .map(|login| format!("@{}", reveal_hidden_characters(login)))
            .unwrap_or_else(|| "a deleted account".to_string());
        let source_repository = details
            .head_repository
            .as_deref()
            .map(|name| reveal_hidden_characters(name).into_owned())
            .unwrap_or_else(|| "a deleted repository".to_string());
        Some(Self {
            number: pull_request.number,
            author,
            source_repository,
            worktree: None,
        })
    }

    pub fn switching(self, worktree: Option<String>) -> Self {
        Self { worktree, ..self }
    }

    pub fn title(&self) -> String {
        format!(
            "Pull request #{} comes from a fork you may not trust",
            self.number
        )
    }

    pub fn message(&self) -> String {
        let worktree = self
            .worktree
            .as_deref()
            .map(|worktree| format!(" It switches the branch of {worktree}."))
            .unwrap_or_default();
        format!(
            "Its code was written by {} in {}. Once checked out, this code will run in a \
             trusted worktree: its project settings, tasks, build scripts and language servers \
             can run without asking. Check it out only if you trust it. Nothing is run for you \
             after the checkout.{worktree}",
            self.author, self.source_repository
        )
    }
}

/// Shows git's password, passphrase or username prompt in `workspace` while
/// a pull request is fetched.
pub fn pull_request_fetch_askpass(
    workspace: WeakEntity<Workspace>,
    window: AnyWindowHandle,
    cx: &mut App,
) -> AskPassDelegate {
    askpass_delegate_for_workspace(workspace, window, "git fetch (pull request)".into(), cx)
}
