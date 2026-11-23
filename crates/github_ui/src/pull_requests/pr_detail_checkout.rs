//! The pull request tab's checkout: the branch name to use, asked for before
//! anything runs, and what git did with it.

use std::rc::Rc;

use editor::Editor;
use gpui::{App, Context, Entity, PromptLevel, Window};

use crate::untrusted_text::reveal_hidden_characters;

use crate::issues::single_line_editor;
use crate::pull_requests::branch_name::{default_branch_name, validate_branch_name};
use crate::pull_requests::fork_checkout::{ForkWarning, pull_request_fetch_askpass};
use crate::pull_requests::pr_checkout::{
    CheckoutBlocker, CheckoutGit, CheckoutOutcome, CheckoutRequest, CheckoutSource, RepositoryGit,
    checkout_blocker, find_checkout_target, run_checkout,
};
use crate::pull_requests::pr_detail_view::PullRequestDetailView;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CheckoutState {
    #[default]
    Idle,
    /// The warning about a fork's code is open; nothing is fetched until it
    /// is accepted.
    ForkWarning,
    /// The branch name field is shown; `error` is why the typed name was
    /// refused.
    Naming {
        error: Option<String>,
    },
    /// A local branch of that name exists: reuse it, or rename.
    BranchExists {
        branch: String,
    },
    Running,
    /// The pull request's head moved after it was shown; nothing was
    /// checked out. Reload and check out again.
    PullRequestChanged(String),
    Finished(String),
    Failed(String),
}

impl PullRequestDetailView {
    pub fn checkout_state(&self) -> &CheckoutState {
        &self.checkout
    }

    /// `None` when the pull request can be checked out here.
    pub fn checkout_blocker(&self, cx: &App) -> Option<CheckoutBlocker> {
        let details = self.details.as_ref()?;
        let has_matching_remote = self.project.upgrade().is_some_and(|project| {
            find_checkout_target(project.read(cx), &self.key.repository, cx).is_some()
        });
        checkout_blocker(&details.pull_request, self.access, has_matching_remote)
    }

    pub(crate) fn branch_editor(&mut self, window: &mut Window, cx: &mut App) -> Entity<Editor> {
        if let Some(editor) = &self.branch_editor {
            return editor.clone();
        }
        let default = self
            .details
            .as_ref()
            .map(|details| {
                default_branch_name(
                    details.pull_request.number,
                    &details.pull_request.head_ref_name,
                )
            })
            .unwrap_or_default();
        let editor = single_line_editor("Local branch name", &default, window, cx);
        self.branch_editor = Some(editor.clone());
        editor
    }

    /// Shows the branch name to check out to; nothing runs yet. A fork's
    /// pull request first asks the user to accept running its code, every
    /// time.
    pub fn start_checkout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let idle = matches!(
            self.checkout,
            CheckoutState::Idle | CheckoutState::Finished(_) | CheckoutState::Failed(_)
        );
        if !idle || self.checkout_blocker(cx).is_some() {
            return;
        }
        self.fork_warning_accepted = false;
        let worktree = self.checkout_worktree_path(cx);
        let warning = self
            .details
            .as_ref()
            .and_then(ForkWarning::for_pull_request)
            .map(|warning| warning.switching(worktree));
        let Some(warning) = warning else {
            self.checkout = CheckoutState::Naming { error: None };
            cx.notify();
            return;
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &warning.title(),
            Some(&warning.message()),
            &["Check out anyway", "Cancel"],
            cx,
        );
        self.checkout = CheckoutState::ForkWarning;
        self.checkout_task = Some(cx.spawn(async move |this, cx| {
            let accepted = matches!(answer.await, Ok(0));
            this.update(cx, |this, cx| {
                if this.checkout == CheckoutState::ForkWarning {
                    this.fork_warning_accepted = accepted;
                    this.checkout = if accepted {
                        CheckoutState::Naming { error: None }
                    } else {
                        CheckoutState::Idle
                    };
                    cx.notify();
                }
            })
            .ok();
        }));
        cx.notify();
    }

    pub fn cancel_checkout(&mut self, cx: &mut Context<Self>) {
        if self.checkout != CheckoutState::Running {
            self.checkout = CheckoutState::Idle;
            self.fork_warning_accepted = false;
            cx.notify();
        }
    }

    /// Checks out to the name in the field. `reuse_existing` is the answer
    /// to "a branch of that name exists".
    pub fn confirm_checkout(
        &mut self,
        reuse_existing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.checkout == CheckoutState::Running || self.checkout_blocker(cx).is_some() {
            return;
        }
        let Some(editor) = self.branch_editor.clone() else {
            return;
        };
        let branch_name = editor.read(cx).text(cx).trim().to_string();
        if let Err(error) = validate_branch_name(&branch_name) {
            self.checkout = CheckoutState::Naming {
                error: Some(error.0),
            };
            cx.notify();
            return;
        }
        let (Some(details), Some(project)) = (self.details.as_ref(), self.project.upgrade()) else {
            return;
        };
        let Some((repository, remote)) =
            find_checkout_target(project.read(cx), &self.key.repository, cx)
        else {
            return;
        };
        let source = if details.pull_request.is_cross_repository {
            if !self.fork_warning_accepted {
                return;
            }
            CheckoutSource::Fork {
                number: details.pull_request.number,
            }
        } else {
            CheckoutSource::SameRepository {
                head_ref_name: details.pull_request.head_ref_name.clone(),
            }
        };
        let request = CheckoutRequest {
            remote: remote.to_string(),
            source,
            branch_name,
            reuse_existing,
            expected_head: details.pull_request.head_ref_oid.clone(),
        };
        let workspace = self.workspace.clone();
        let window_handle = window.window_handle();
        let git: Rc<dyn CheckoutGit> = Rc::new(RepositoryGit {
            repository,
            askpass: Box::new(move |cx| {
                pull_request_fetch_askpass(workspace.clone(), window_handle, cx)
            }),
        });
        self.checkout = CheckoutState::Running;
        self.checkout_task = Some(cx.spawn(async move |this, cx| {
            let outcome = run_checkout(git, request, cx).await;
            this.update(cx, |this, cx| {
                this.checkout = match outcome {
                    CheckoutOutcome::CheckedOut { branch } => {
                        CheckoutState::Finished(format!("Checked out {branch}."))
                    }
                    CheckoutOutcome::SwitchedToExisting { branch } => CheckoutState::Finished(
                        format!("Switched to the existing {branch}; it was not updated."),
                    ),
                    CheckoutOutcome::BranchExists { branch } => {
                        CheckoutState::BranchExists { branch }
                    }
                    CheckoutOutcome::PullRequestChanged { fetched } => {
                        CheckoutState::PullRequestChanged(fetched)
                    }
                    CheckoutOutcome::Refused(reason) | CheckoutOutcome::Failed(reason) => {
                        CheckoutState::Failed(reason)
                    }
                };
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// The working tree whose branch a checkout would switch: with several
    /// pointing at the repository, the one picked may not be the one the
    /// user has in mind.
    pub fn checkout_worktree_path(&self, cx: &App) -> Option<String> {
        let project = self.project.upgrade()?;
        let (repository, _) = find_checkout_target(project.read(cx), &self.key.repository, cx)?;
        let path = repository.read(cx).work_directory_abs_path.clone();
        Some(reveal_hidden_characters(&path.to_string_lossy()).into_owned())
    }

    /// Back to the name field after "a branch of that name exists".
    pub fn rename_checkout_branch(&mut self, cx: &mut Context<Self>) {
        self.checkout = CheckoutState::Naming { error: None };
        cx.notify();
    }
}
