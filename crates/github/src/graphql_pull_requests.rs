//! Pull requests as GitHub's GraphQL API answers them.

use serde_json::Value;

use crate::error::{GithubResult, unexpected};
use crate::graphql::{bool_at, nodes, number_at, optional_string_at, string_at};
use crate::model::{PullRequest, PullRequestState};
use crate::pull_requests::{
    ChangedFile, CheckState, ChecksState, FileChange, HeadBranch, MergeSettings, MergeStateStatus,
    Mergeable, PullRequestCheck, PullRequestDetails, PullRequestReview, ReviewDecision,
    ReviewState,
};

fn enum_at<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

fn present<'a>(value: &'a Value, field: &str) -> Option<&'a Value> {
    value.get(field).filter(|inner| !inner.is_null())
}

pub(crate) fn parse_pull_request(node: &Value) -> GithubResult<PullRequest> {
    let state = match enum_at(node, "state") {
        Some("OPEN") => PullRequestState::Open,
        Some("CLOSED") => PullRequestState::Closed,
        Some("MERGED") => PullRequestState::Merged,
        other => return Err(unexpected(format!("unknown pull request state {other:?}"))),
    };
    let checks = present(node, "statusCheckRollup")
        .and_then(|rollup| enum_at(rollup, "state"))
        .and_then(checks_state);
    let review_decision = match enum_at(node, "reviewDecision") {
        Some("APPROVED") => Some(ReviewDecision::Approved),
        Some("CHANGES_REQUESTED") => Some(ReviewDecision::ChangesRequested),
        Some("REVIEW_REQUIRED") => Some(ReviewDecision::ReviewRequired),
        _ => None,
    };
    let requested_reviewers = present(node, "reviewRequests")
        .map(|requests| {
            nodes(requests)
                .filter_map(|request| present(request, "requestedReviewer"))
                .filter_map(|reviewer| optional_string_at(reviewer, "login"))
                .collect()
        })
        .unwrap_or_default();
    Ok(PullRequest {
        id: string_at(node, "id")?,
        number: number_at(node, "number")?,
        title: string_at(node, "title")?,
        state,
        is_draft: bool_at(node, "isDraft"),
        author: present(node, "author").and_then(|author| optional_string_at(author, "login")),
        head_ref_name: string_at(node, "headRefName")?,
        head_ref_oid: optional_string_at(node, "headRefOid").unwrap_or_default(),
        base_ref_name: string_at(node, "baseRefName")?,
        is_cross_repository: bool_at(node, "isCrossRepository"),
        url: string_at(node, "url")?,
        updated_at: string_at(node, "updatedAt")?,
        checks,
        review_decision,
        requested_reviewers,
    })
}

/// `EXPECTED` is a required status nothing has reported yet: still running
/// as far as the user can tell.
fn checks_state(state: &str) -> Option<ChecksState> {
    match state {
        "SUCCESS" => Some(ChecksState::Passing),
        "FAILURE" | "ERROR" => Some(ChecksState::Failing),
        "PENDING" | "EXPECTED" => Some(ChecksState::Running),
        _ => None,
    }
}

/// The pull request with its repository's merge settings: `repository` is
/// the answer's `repository` object, which holds `pullRequest`.
pub(crate) fn parse_pull_request_details(repository: &Value) -> GithubResult<PullRequestDetails> {
    let node = present(repository, "pullRequest").ok_or(crate::GithubError::NotFound)?;
    let pull_request = parse_pull_request(node)?;
    let mergeable = match enum_at(node, "mergeable") {
        Some("MERGEABLE") => Mergeable::Mergeable,
        Some("CONFLICTING") => Mergeable::Conflicting,
        _ => Mergeable::Unknown,
    };
    let merge_state_status = match enum_at(node, "mergeStateStatus") {
        Some("BEHIND") => MergeStateStatus::Behind,
        Some("BLOCKED") => MergeStateStatus::Blocked,
        Some("CLEAN") => MergeStateStatus::Clean,
        Some("DIRTY") => MergeStateStatus::Dirty,
        Some("DRAFT") => MergeStateStatus::Draft,
        Some("HAS_HOOKS") => MergeStateStatus::HasHooks,
        Some("UNSTABLE") => MergeStateStatus::Unstable,
        _ => MergeStateStatus::Unknown,
    };
    let contexts =
        present(node, "statusCheckRollup").and_then(|rollup| present(rollup, "contexts"));
    let files = present(node, "files");
    Ok(PullRequestDetails {
        body: optional_string_at(node, "body").unwrap_or_default(),
        mergeable,
        merge_state_status,
        is_in_merge_queue: bool_at(node, "isInMergeQueue"),
        is_merge_queue_enabled: bool_at(node, "isMergeQueueEnabled"),
        viewer_can_delete_head_ref: bool_at(node, "viewerCanDeleteHeadRef"),
        head_repository: present(node, "headRepository")
            .and_then(|head| optional_string_at(head, "nameWithOwner")),
        head_branch: present(node, "headRef")
            .map(parse_head_branch)
            .transpose()?,
        checks: contexts
            .map(|contexts| nodes(contexts).filter_map(parse_check).collect())
            .unwrap_or_default(),
        checks_total: contexts
            .and_then(|contexts| contexts.get("totalCount"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        reviews: present(node, "latestReviews")
            .map(|reviews| nodes(reviews).filter_map(parse_review).collect())
            .unwrap_or_default(),
        files: files
            .map(|files| nodes(files).map(parse_changed_file).collect())
            .transpose()?
            .unwrap_or_default(),
        files_total: files
            .and_then(|files| files.get("totalCount"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        settings: MergeSettings {
            repository_node_id: string_at(repository, "id")?,
            merge_commit_allowed: bool_at(repository, "mergeCommitAllowed"),
            squash_merge_allowed: bool_at(repository, "squashMergeAllowed"),
            rebase_merge_allowed: bool_at(repository, "rebaseMergeAllowed"),
            delete_branch_on_merge: bool_at(repository, "deleteBranchOnMerge"),
            default_branch: present(repository, "defaultBranchRef")
                .and_then(|branch| optional_string_at(branch, "name")),
        },
        pull_request,
    })
}

/// Protected when anything GitHub reports would stop or govern its
/// deletion. Erring towards protected only hides an offer.
fn parse_head_branch(head: &Value) -> GithubResult<HeadBranch> {
    let refuses_deletion = present(head, "refUpdateRule")
        .is_some_and(|rule| rule.get("allowsDeletions").and_then(Value::as_bool) != Some(true));
    let ruleset_forbids_deletion = present(head, "rules")
        .is_some_and(|rules| nodes(rules).any(|rule| enum_at(rule, "type") == Some("DELETION")));
    Ok(HeadBranch {
        name: string_at(head, "name")?,
        oid: present(head, "target").and_then(|target| optional_string_at(target, "oid")),
        is_protected: present(head, "branchProtectionRule").is_some()
            || refuses_deletion
            || ruleset_forbids_deletion,
    })
}

fn parse_check(node: &Value) -> Option<PullRequestCheck> {
    match enum_at(node, "__typename")? {
        "CheckRun" => {
            let state = match (enum_at(node, "status"), enum_at(node, "conclusion")) {
                (Some("COMPLETED"), Some("SUCCESS")) => CheckState::Passed,
                (
                    Some("COMPLETED"),
                    Some("FAILURE" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE"),
                ) => CheckState::Failed,
                (Some("COMPLETED"), _) => CheckState::Skipped,
                _ => CheckState::Running,
            };
            Some(PullRequestCheck {
                name: optional_string_at(node, "name")?,
                state,
                url: optional_string_at(node, "detailsUrl"),
            })
        }
        "StatusContext" => {
            let state = match enum_at(node, "state") {
                Some("SUCCESS") => CheckState::Passed,
                Some("FAILURE" | "ERROR") => CheckState::Failed,
                _ => CheckState::Running,
            };
            Some(PullRequestCheck {
                name: optional_string_at(node, "context")?,
                state,
                url: optional_string_at(node, "targetUrl"),
            })
        }
        _ => None,
    }
}

fn parse_review(node: &Value) -> Option<PullRequestReview> {
    let state = match enum_at(node, "state")? {
        "APPROVED" => ReviewState::Approved,
        "CHANGES_REQUESTED" => ReviewState::ChangesRequested,
        "COMMENTED" => ReviewState::Commented,
        "DISMISSED" => ReviewState::Dismissed,
        "PENDING" => ReviewState::Pending,
        _ => return None,
    };
    Some(PullRequestReview {
        author: present(node, "author").and_then(|author| optional_string_at(author, "login")),
        state,
        submitted_at: optional_string_at(node, "submittedAt"),
    })
}

fn parse_changed_file(node: &Value) -> GithubResult<ChangedFile> {
    let change = match enum_at(node, "changeType") {
        Some("ADDED") => FileChange::Added,
        Some("COPIED") => FileChange::Copied,
        Some("DELETED") => FileChange::Deleted,
        Some("RENAMED") => FileChange::Renamed,
        Some("MODIFIED") => FileChange::Modified,
        _ => FileChange::Changed,
    };
    Ok(ChangedFile {
        path: string_at(node, "path")?,
        additions: number_at(node, "additions")?,
        deletions: number_at(node, "deletions")?,
        change,
    })
}
