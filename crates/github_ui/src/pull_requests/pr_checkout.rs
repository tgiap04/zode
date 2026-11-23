//! Checking out a pull request: fetch from the remote that points at the
//! selected repository, make a local branch from what was fetched, switch to
//! it. A same-repository head is fetched as `refs/remotes/<remote>/<head>`;
//! a fork's is fetched from the base repository's `refs/pull/<n>/head` into
//! `refs/remotes/<remote>/pull/<n>/head`, so the fork itself is never added
//! as a remote.
//!
//! What was fetched must be the head the user was shown, and the new branch
//! must start there: a host older than the branch's base field makes it from
//! HEAD instead and still reports success.
//!
//! Only through the git store's own fetch, create and switch, so the usual
//! git job queue and status updates apply. Never forced: a working tree with
//! changes is reported and left alone.

use std::rc::Rc;

use anyhow::Result;
use askpass::AskPassDelegate;
use futures::FutureExt as _;
use futures::future::LocalBoxFuture;
use git::repository::{FetchOptions, Remote};
use github::{PullRequest, RepositoryId, parse_github_remote};
use gpui::{App, AsyncApp, Entity};
use project::Project;
use project::git_store::{Repository, RepositoryState};

use crate::github_view::ProjectAccess;
use crate::pull_requests::branch_name::validate_branch_name;

/// Why a pull request offers no checkout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckoutBlocker {
    /// The project is a collaborator's; its files are not ours to switch.
    CollabGuest,
    /// No git repository in the project has a remote pointing at the
    /// selected repository.
    NoMatchingRemote,
    /// The head branch's name would not be safe to hand to git.
    UnsafeHeadName,
}

impl CheckoutBlocker {
    pub fn explanation(&self) -> &'static str {
        match self {
            Self::CollabGuest => "Checkout is off in a collaborator's project",
            Self::NoMatchingRemote => "No git remote in this project points at the repository",
            Self::UnsafeHeadName => "The head branch's name is not safe to check out",
        }
    }
}

/// The remote, by name, of a project repository whose `origin` or
/// `upstream` points at `repository`. GitHub ignores case in names.
pub fn matching_remote(
    origin_url: Option<&str>,
    upstream_url: Option<&str>,
    repository: &RepositoryId,
) -> Option<&'static str> {
    let wanted = repository.full_name().to_lowercase();
    let points_at = |url: Option<&str>| {
        url.and_then(parse_github_remote)
            .is_some_and(|found| found.full_name().to_lowercase() == wanted)
    };
    if points_at(origin_url) {
        Some("origin")
    } else if points_at(upstream_url) {
        Some("upstream")
    } else {
        None
    }
}

/// What stands in the way of checking `pull_request` out, if anything. A
/// fork's head name is never handed to git, so only a same-repository head's
/// name is checked.
pub fn checkout_blocker(
    pull_request: &PullRequest,
    access: ProjectAccess,
    has_matching_remote: bool,
) -> Option<CheckoutBlocker> {
    if !access.allows_local_writes() {
        Some(CheckoutBlocker::CollabGuest)
    } else if !has_matching_remote {
        Some(CheckoutBlocker::NoMatchingRemote)
    } else if !pull_request.is_cross_repository
        && validate_branch_name(&pull_request.head_ref_name).is_err()
    {
        Some(CheckoutBlocker::UnsafeHeadName)
    } else {
        None
    }
}

/// The git work a checkout needs, so it can be run against a fake in tests.
pub trait CheckoutGit {
    /// Changes to tracked files, staged or not, or conflicts. Untracked files
    /// are left out: a switch keeps them, and git refuses on its own when
    /// one would be overwritten.
    fn has_uncommitted_changes(&self, cx: &App) -> bool;
    fn local_branches(&self, cx: &mut App) -> LocalBoxFuture<'static, Result<Vec<String>>>;
    fn fetch(&self, options: FetchOptions, cx: &mut App) -> LocalBoxFuture<'static, Result<()>>;
    fn create_branch(
        &self,
        name: String,
        base: String,
        cx: &mut App,
    ) -> LocalBoxFuture<'static, Result<()>>;
    fn change_branch(&self, name: String, cx: &mut App) -> LocalBoxFuture<'static, Result<()>>;
    /// The commit `reference` points at.
    fn resolve_commit(
        &self,
        reference: String,
        cx: &mut App,
    ) -> LocalBoxFuture<'static, Result<String>>;
    /// Deletes `name` even though nothing merged it (`git branch -D`): only
    /// for a branch this checkout has just made, having seen it was absent.
    fn force_delete_new_branch<'a>(
        &'a self,
        name: String,
        cx: &'a mut AsyncApp,
    ) -> LocalBoxFuture<'a, Result<()>>;
}

/// Where the pull request's head is fetched from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckoutSource {
    /// A branch of the selected repository.
    SameRepository { head_ref_name: String },
    /// A fork, read through the base repository's `refs/pull/<number>/head`.
    /// Only made once the user has accepted the warning about running a
    /// stranger's code.
    Fork { number: u64 },
}

impl CheckoutSource {
    /// What to fetch from `remote`, and the ref it lands in.
    pub fn fetch_and_base(&self, remote: &str) -> (FetchOptions, String) {
        match self {
            Self::SameRepository { head_ref_name } => (
                FetchOptions::Remote(Remote {
                    name: remote.to_string().into(),
                }),
                format!("refs/remotes/{remote}/{head_ref_name}"),
            ),
            // `pull/<n>/head` rather than `pr/<n>`: a branch named `pr/<n>`
            // on the remote is fetched to `refs/remotes/<remote>/pr/<n>`.
            Self::Fork { number } => {
                let destination = format!("refs/remotes/{remote}/pull/{number}/head");
                (
                    FetchOptions::Refspec {
                        remote: Remote {
                            name: remote.to_string().into(),
                        },
                        source: format!("refs/pull/{number}/head").into(),
                        destination: destination.clone().into(),
                    },
                    destination,
                )
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckoutRequest {
    pub remote: String,
    pub source: CheckoutSource,
    pub branch_name: String,
    /// The user chose to switch to the branch of that name that exists.
    pub reuse_existing: bool,
    /// The head commit the user was shown, and for a fork accepted the
    /// warning about.
    pub expected_head: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckoutOutcome {
    CheckedOut {
        branch: String,
    },
    /// Switched to a branch that was already there; it was not moved.
    SwitchedToExisting {
        branch: String,
    },
    /// A local branch of that name exists: reuse it or pick another name.
    BranchExists {
        branch: String,
    },
    /// What was fetched is not the head the user was shown: nothing was
    /// created or switched.
    PullRequestChanged {
        fetched: String,
    },
    Refused(String),
    Failed(String),
}

pub async fn run_checkout(
    git: Rc<dyn CheckoutGit>,
    request: CheckoutRequest,
    cx: &mut AsyncApp,
) -> CheckoutOutcome {
    if let Err(error) = validate_branch_name(&request.branch_name) {
        return CheckoutOutcome::Refused(error.0);
    }
    if let CheckoutSource::SameRepository { head_ref_name } = &request.source
        && validate_branch_name(head_ref_name).is_err()
    {
        return CheckoutOutcome::Refused(CheckoutBlocker::UnsafeHeadName.explanation().to_string());
    }
    if cx.update(|cx| git.has_uncommitted_changes(cx)) {
        return CheckoutOutcome::Refused(
            "The working tree has uncommitted changes. Commit or stash them, then check out again."
                .to_string(),
        );
    }
    let branches = match cx.update(|cx| git.local_branches(cx)).await {
        Ok(branches) => branches,
        Err(error) => return CheckoutOutcome::Failed(format!("Could not list branches: {error}")),
    };
    let exists = branches.iter().any(|branch| branch == &request.branch_name);
    let branch = request.branch_name.clone();
    if exists && !request.reuse_existing {
        return CheckoutOutcome::BranchExists { branch };
    }
    if exists {
        return match cx.update(|cx| git.change_branch(branch.clone(), cx)).await {
            Ok(()) => CheckoutOutcome::SwitchedToExisting { branch },
            Err(error) => CheckoutOutcome::Failed(format!("Could not switch to {branch}: {error}")),
        };
    }
    let (fetch_options, base) = request.source.fetch_and_base(&request.remote);
    if let Err(error) = cx.update(|cx| git.fetch(fetch_options, cx)).await {
        return CheckoutOutcome::Failed(format!("Could not fetch {}: {error}", request.remote));
    }
    let fetched = match cx.update(|cx| git.resolve_commit(base.clone(), cx)).await {
        Ok(commit) => commit.trim().to_string(),
        Err(error) => {
            return CheckoutOutcome::Failed(format!(
                "Could not read {base} after the fetch: {error}"
            ));
        }
    };
    if !fetched.eq_ignore_ascii_case(request.expected_head.trim()) {
        return CheckoutOutcome::PullRequestChanged { fetched };
    }
    if let Err(error) = cx
        .update(|cx| git.create_branch(branch.clone(), base, cx))
        .await
    {
        return CheckoutOutcome::Failed(format!("Could not create {branch}: {error}"));
    }
    let created = cx
        .update(|cx| git.resolve_commit(format!("refs/heads/{branch}"), cx))
        .await;
    let started_at = match created {
        Ok(commit) if commit.trim().eq_ignore_ascii_case(&fetched) => None,
        Ok(commit) => Some(format!("starts at {}", short_commit(commit.trim()))),
        Err(error) => Some(format!("could not be read back ({error})")),
    };
    if let Some(started_at) = started_at {
        let cleanup = match git.force_delete_new_branch(branch.clone(), cx).await {
            Ok(()) => format!("{branch} was deleted."),
            Err(error) => format!("{branch} could not be deleted ({error}); delete it yourself."),
        };
        return CheckoutOutcome::Failed(format!(
            "The new branch {started_at}, not at the pull request's head {}: the git host may \
             be too old to start a branch from a fetched ref. {cleanup} Nothing was switched.",
            short_commit(&fetched)
        ));
    }
    match cx.update(|cx| git.change_branch(branch.clone(), cx)).await {
        Ok(()) => CheckoutOutcome::CheckedOut { branch },
        Err(error) => CheckoutOutcome::Failed(format!("Could not switch to {branch}: {error}")),
    }
}

fn short_commit(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
}

/// The project's git repository and remote name for `repository`, when it
/// has one. The active repository is preferred when several match.
pub fn find_checkout_target(
    project: &Project,
    repository: &RepositoryId,
    cx: &App,
) -> Option<(Entity<Repository>, &'static str)> {
    let git_store = project.git_store().read(cx);
    let remote_of = |candidate: &Entity<Repository>| {
        let candidate = candidate.read(cx);
        matching_remote(
            candidate.remote_origin_url.as_deref(),
            candidate.remote_upstream_url.as_deref(),
            repository,
        )
    };
    let active = git_store
        .active_repository()
        .and_then(|active| remote_of(&active).map(|remote| (active, remote)));
    active.or_else(|| {
        git_store
            .repositories()
            .values()
            .find_map(|candidate| remote_of(candidate).map(|remote| (candidate.clone(), remote)))
    })
}

/// [`CheckoutGit`] over a project repository. `askpass` makes the delegate
/// that answers git when a fetch needs a password or passphrase.
pub struct RepositoryGit {
    pub repository: Entity<Repository>,
    pub askpass: Box<dyn Fn(&mut App) -> AskPassDelegate>,
}

impl CheckoutGit for RepositoryGit {
    fn has_uncommitted_changes(&self, cx: &App) -> bool {
        self.repository
            .read(cx)
            .status()
            .any(|entry| !entry.status.is_untracked() && !entry.status.is_ignored())
    }

    fn local_branches(&self, cx: &mut App) -> LocalBoxFuture<'static, Result<Vec<String>>> {
        let branches = self
            .repository
            .update(cx, |repository, _| repository.branches());
        async move {
            Ok(branches
                .await??
                .into_iter()
                .filter(|branch| !branch.is_remote())
                .map(|branch| branch.name().to_string())
                .collect())
        }
        .boxed_local()
    }

    fn fetch(&self, options: FetchOptions, cx: &mut App) -> LocalBoxFuture<'static, Result<()>> {
        let askpass = (self.askpass)(cx);
        let fetch = self
            .repository
            .update(cx, |repository, cx| repository.fetch(options, askpass, cx));
        async move {
            fetch.await??;
            Ok(())
        }
        .boxed_local()
    }

    fn create_branch(
        &self,
        name: String,
        base: String,
        cx: &mut App,
    ) -> LocalBoxFuture<'static, Result<()>> {
        let create = self.repository.update(cx, |repository, _| {
            repository.create_branch(name, Some(base))
        });
        async move { create.await? }.boxed_local()
    }

    fn change_branch(&self, name: String, cx: &mut App) -> LocalBoxFuture<'static, Result<()>> {
        let change = self
            .repository
            .update(cx, |repository, _| repository.change_branch(name));
        async move { change.await? }.boxed_local()
    }

    fn resolve_commit(
        &self,
        reference: String,
        cx: &mut App,
    ) -> LocalBoxFuture<'static, Result<String>> {
        let show = self
            .repository
            .update(cx, |repository, _| repository.show(reference));
        async move { Ok(show.await??.sha.to_string()) }.boxed_local()
    }

    fn force_delete_new_branch<'a>(
        &'a self,
        name: String,
        cx: &'a mut AsyncApp,
    ) -> LocalBoxFuture<'a, Result<()>> {
        async move {
            let forced = cx.update(|cx| {
                self.repository.update(cx, |repository, _| {
                    let name = name.clone();
                    repository.send_job(Some(format!("git branch -D {name}").into()), {
                        move |state, _| async move {
                            match state {
                                RepositoryState::Local(local) => {
                                    Some(local.backend.force_delete_local_branch(name).await)
                                }
                                RepositoryState::Remote(_) => None,
                            }
                        }
                    })
                })
            });
            if let Some(result) = forced.await? {
                return result;
            }
            // A remote host's protocol has no forced delete; a plain one
            // still removes a branch that starts where the host's HEAD is.
            let plain = cx.update(|cx| {
                self.repository
                    .update(cx, |repository, _| repository.delete_branch(false, name))
            });
            plain.await?
        }
        .boxed_local()
    }
}
