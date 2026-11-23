//! What the Pull requests section reads beyond a list row: checks, reviews,
//! changed files, and what the repository lets a merge do.

use crate::model::PullRequest;

/// The combined state of a pull request's checks and commit statuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChecksState {
    Passing,
    Failing,
    Running,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

/// GitHub works mergeability out in the background; `Unknown` means it has
/// not yet, and asking again later is the only way to learn it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mergeable {
    Mergeable,
    Conflicting,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeStateStatus {
    Behind,
    /// Requirements such as reviews or required checks are not met. An admin
    /// could still merge, which Zode never does on their behalf.
    Blocked,
    Clean,
    Dirty,
    Draft,
    HasHooks,
    Unknown,
    /// Only checks that are not required are failing.
    Unstable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    pub fn graphql_name(self) -> &'static str {
        match self {
            Self::Merge => "MERGE",
            Self::Squash => "SQUASH",
            Self::Rebase => "REBASE",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Merge => "Create a merge commit",
            Self::Squash => "Squash and merge",
            Self::Rebase => "Rebase and merge",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckState {
    Passed,
    Failed,
    Running,
    /// Skipped, neutral, cancelled or stale: neither passed nor failed.
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequestCheck {
    pub name: String,
    pub state: CheckState,
    /// Where GitHub (or the CI service) shows the check's details.
    pub url: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
    Pending,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequestReview {
    pub author: Option<String>,
    pub state: ReviewState,
    pub submitted_at: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileChange {
    Added,
    Changed,
    Copied,
    Deleted,
    Modified,
    Renamed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub additions: u64,
    pub deletions: u64,
    pub change: FileChange,
}

/// The pull request's head branch as it is now on GitHub. `None` on the
/// details when the branch was deleted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadBranch {
    pub name: String,
    pub oid: Option<String>,
    /// Branch protection, a ruleset forbidding deletion, or a rule GitHub
    /// applies to the viewer.
    pub is_protected: bool,
}

/// What the base repository allows a merge to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeSettings {
    /// GraphQL node id of the base repository.
    pub repository_node_id: String,
    pub merge_commit_allowed: bool,
    pub squash_merge_allowed: bool,
    pub rebase_merge_allowed: bool,
    /// GitHub deletes head branches itself after a merge.
    pub delete_branch_on_merge: bool,
    pub default_branch: Option<String>,
}

impl MergeSettings {
    pub fn allowed_methods(&self) -> Vec<MergeMethod> {
        [
            (self.merge_commit_allowed, MergeMethod::Merge),
            (self.squash_merge_allowed, MergeMethod::Squash),
            (self.rebase_merge_allowed, MergeMethod::Rebase),
        ]
        .into_iter()
        .filter_map(|(allowed, method)| allowed.then_some(method))
        .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequestDetails {
    pub pull_request: PullRequest,
    pub body: String,
    pub mergeable: Mergeable,
    pub merge_state_status: MergeStateStatus,
    pub is_in_merge_queue: bool,
    /// The base branch merges through a merge queue.
    pub is_merge_queue_enabled: bool,
    pub viewer_can_delete_head_ref: bool,
    /// `owner/name`; `None` when the head repository was deleted.
    pub head_repository: Option<String>,
    pub head_branch: Option<HeadBranch>,
    pub checks: Vec<PullRequestCheck>,
    /// All checks, including those past the first page.
    pub checks_total: u64,
    pub reviews: Vec<PullRequestReview>,
    pub files: Vec<ChangedFile>,
    /// All changed files, including those past the first page.
    pub files_total: u64,
    pub settings: MergeSettings,
}
