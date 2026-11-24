//! Pull request details for the fake backend.

use crate::model::PullRequest;
use crate::pull_requests::{
    HeadBranch, MergeSettings, MergeStateStatus, Mergeable, PullRequestDetails,
};

/// Details for `pull_request` as a plain repository would answer: mergeable,
/// every merge method allowed, an unprotected head branch at the pull
/// request's head, no merge queue, `main` as the default branch.
pub fn fake_pull_request_details(pull_request: PullRequest) -> PullRequestDetails {
    let head_branch = (!pull_request.is_cross_repository).then(|| HeadBranch {
        name: pull_request.head_ref_name.clone(),
        oid: Some(pull_request.head_ref_oid.clone()),
        is_protected: false,
    });
    PullRequestDetails {
        body: String::new(),
        mergeable: Mergeable::Mergeable,
        merge_state_status: MergeStateStatus::Clean,
        is_in_merge_queue: false,
        is_merge_queue_enabled: false,
        viewer_can_delete_head_ref: !pull_request.is_cross_repository,
        head_repository: Some("octo/repo".to_string()),
        head_branch,
        checks: Vec::new(),
        checks_total: 0,
        reviews: Vec::new(),
        files: Vec::new(),
        files_total: 0,
        settings: MergeSettings {
            repository_node_id: "R_octo_repo".to_string(),
            merge_commit_allowed: true,
            squash_merge_allowed: true,
            rebase_merge_allowed: true,
            delete_branch_on_merge: false,
            default_branch: Some("main".to_string()),
        },
        pull_request,
    }
}
