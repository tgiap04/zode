use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use futures::AsyncReadExt as _;
use futures::executor::block_on;
use gpui::TestAppContext;
use http_client::{AsyncBody, FakeHttpClient, Response};
use serde_json::{Value, json};

use crate::backend::GithubBackend;
use crate::error::GithubError;
use crate::fake_backend::{FakeGithubBackend, FakeWrite, fake_pull_request};
use crate::graphql::{data_or_error, parse_pull_request, parse_pull_request_details};
use crate::http_backend::HttpBackend;
use crate::model::RepositoryId;
use crate::pull_requests::{
    CheckState, ChecksState, MergeMethod, MergeStateStatus, Mergeable, ReviewDecision,
};
use crate::token::{GithubToken, TokenSource};

fn repository() -> RepositoryId {
    RepositoryId::new("octo", "repo").expect("a valid repository")
}

fn summary_json(rollup: Value) -> Value {
    json!({
        "id": "PR_7", "number": 7, "title": "Fix", "state": "OPEN", "isDraft": true,
        "author": {"login": "hubot"}, "headRefName": "fix", "headRefOid": "abc",
        "baseRefName": "main", "isCrossRepository": false,
        "url": "https://github.com/octo/repo/pull/7", "updatedAt": "2026-11-01T00:00:00Z",
        "reviewDecision": "CHANGES_REQUESTED",
        "statusCheckRollup": rollup,
        "reviewRequests": {"nodes": [
            {"requestedReviewer": {"__typename": "User", "login": "octocat"}},
            {"requestedReviewer": {"__typename": "Team"}}
        ]}
    })
}

#[test]
fn the_checks_rollup_reads_as_passing_failing_or_running() {
    let cases = [
        (json!({"state": "SUCCESS"}), Some(ChecksState::Passing)),
        (json!({"state": "FAILURE"}), Some(ChecksState::Failing)),
        (json!({"state": "ERROR"}), Some(ChecksState::Failing)),
        (json!({"state": "PENDING"}), Some(ChecksState::Running)),
        (json!({"state": "EXPECTED"}), Some(ChecksState::Running)),
        (Value::Null, None),
    ];
    for (rollup, expected) in cases {
        let pull_request = parse_pull_request(&summary_json(rollup.clone())).expect("parses");
        assert_eq!(pull_request.checks, expected, "{rollup}");
    }
    let pull_request = parse_pull_request(&summary_json(Value::Null)).expect("parses");
    assert_eq!(
        pull_request.review_decision,
        Some(ReviewDecision::ChangesRequested)
    );
    assert_eq!(
        pull_request.requested_reviewers,
        vec!["octocat".to_string()]
    );
    assert_eq!(pull_request.head_ref_oid, "abc");
    assert!(pull_request.is_draft);
}

fn details_json(head_ref: Value, mergeable: &str) -> Value {
    let mut pull_request = summary_json(json!({
        "state": "FAILURE",
        "contexts": {"totalCount": 3, "nodes": [
            {"__typename": "CheckRun", "name": "build", "status": "COMPLETED", "conclusion": "FAILURE", "detailsUrl": "https://ci.example/1"},
            {"__typename": "CheckRun", "name": "lint", "status": "IN_PROGRESS", "conclusion": null, "detailsUrl": null},
            {"__typename": "StatusContext", "context": "deploy", "state": "SUCCESS", "targetUrl": "https://deploy.example"}
        ]}
    }));
    pull_request["body"] = json!("Body");
    pull_request["mergeable"] = json!(mergeable);
    pull_request["mergeStateStatus"] = json!("BLOCKED");
    pull_request["isInMergeQueue"] = json!(false);
    pull_request["isMergeQueueEnabled"] = json!(true);
    pull_request["viewerCanDeleteHeadRef"] = json!(true);
    pull_request["headRepository"] = json!({"nameWithOwner": "octo/repo"});
    pull_request["headRef"] = head_ref;
    pull_request["latestReviews"] = json!({"nodes": [
        {"author": {"login": "hubot"}, "state": "APPROVED", "submittedAt": "2026-11-01T00:00:00Z"}
    ]});
    pull_request["files"] = json!({"totalCount": 1, "nodes": [
        {"path": "src/lib.rs", "additions": 3, "deletions": 1, "changeType": "MODIFIED"}
    ]});
    json!({
        "id": "R_1", "mergeCommitAllowed": false, "squashMergeAllowed": true,
        "rebaseMergeAllowed": true, "deleteBranchOnMerge": false,
        "defaultBranchRef": {"name": "main"},
        "pullRequest": pull_request
    })
}

fn head_ref(protection: Value, update_rule: Value, rules: Value) -> Value {
    json!({
        "name": "fix", "target": {"oid": "abc"},
        "branchProtectionRule": protection, "refUpdateRule": update_rule,
        "rules": {"nodes": rules}
    })
}

#[test]
fn details_carry_checks_reviews_files_and_merge_settings() {
    let unprotected = head_ref(Value::Null, Value::Null, json!([]));
    let details =
        parse_pull_request_details(&details_json(unprotected, "UNKNOWN")).expect("parses");
    assert_eq!(details.mergeable, Mergeable::Unknown);
    assert_eq!(details.merge_state_status, MergeStateStatus::Blocked);
    assert!(details.is_merge_queue_enabled);
    let states = details
        .checks
        .iter()
        .map(|check| (check.name.as_str(), check.state))
        .collect::<Vec<_>>();
    assert_eq!(
        states,
        vec![
            ("build", CheckState::Failed),
            ("lint", CheckState::Running),
            ("deploy", CheckState::Passed)
        ]
    );
    assert_eq!(
        details.checks[0].url.as_deref(),
        Some("https://ci.example/1")
    );
    assert_eq!(details.reviews.len(), 1);
    assert_eq!(details.files[0].additions, 3);
    assert_eq!(
        details.settings.allowed_methods(),
        vec![MergeMethod::Squash, MergeMethod::Rebase]
    );
    assert_eq!(details.settings.default_branch.as_deref(), Some("main"));
    let head = details.head_branch.expect("a head branch");
    assert_eq!(head.oid.as_deref(), Some("abc"));
    assert!(!head.is_protected);
}

#[test]
fn a_head_branch_is_protected_by_a_rule_a_ruleset_or_an_update_rule() {
    let protected_heads = [
        head_ref(json!({"id": "BPR_1"}), Value::Null, json!([])),
        head_ref(Value::Null, json!({"allowsDeletions": false}), json!([])),
        head_ref(Value::Null, Value::Null, json!([{"type": "DELETION"}])),
    ];
    for head in protected_heads {
        let details =
            parse_pull_request_details(&details_json(head.clone(), "MERGEABLE")).expect("parses");
        assert!(
            details.head_branch.expect("a head branch").is_protected,
            "{head}"
        );
    }
    let deleted =
        parse_pull_request_details(&details_json(Value::Null, "CONFLICTING")).expect("parses");
    assert_eq!(deleted.head_branch, None);
    assert_eq!(deleted.mergeable, Mergeable::Conflicting);
}

#[test]
fn a_refused_mutation_reads_as_rejected() {
    let body = json!({
        "data": {"mergePullRequest": null},
        "errors": [{"type": "UNPROCESSABLE", "message": "Head branch was modified. Review and try the merge again."}]
    });
    assert_eq!(
        data_or_error(&body.to_string()),
        Err(GithubError::Rejected {
            message: "Head branch was modified. Review and try the merge again.".to_string()
        })
    );
}

/// Each request's JSON body, answered in turn with `answers`.
fn recording_backend(
    answers: Vec<Value>,
    cx: &mut TestAppContext,
) -> (HttpBackend, Arc<Mutex<Vec<Value>>>) {
    let sent: Arc<Mutex<Vec<Value>>> = Arc::default();
    let answers = Arc::new(Mutex::new(VecDeque::from(answers)));
    let client = FakeHttpClient::create({
        let sent = sent.clone();
        move |request| {
            let sent = sent.clone();
            let answer = answers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front();
            async move {
                let mut bytes = Vec::new();
                request.into_body().read_to_end(&mut bytes).await?;
                sent.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(serde_json::from_slice(&bytes).unwrap_or(Value::Null));
                let answer = answer.ok_or_else(|| anyhow::anyhow!("no answer was set up"))?;
                Ok(Response::builder()
                    .status(200)
                    .body(AsyncBody::from(answer.to_string()))?)
            }
        }
    });
    let token = GithubToken::new("ghu_secret", TokenSource::GhCli);
    let backend = cx.update(|cx| HttpBackend::new(client, token, cx));
    (backend, sent)
}

#[gpui::test]
async fn a_merge_is_always_guarded_by_the_head_it_was_shown(cx: &mut TestAppContext) {
    let merged =
        json!({"data": {"mergePullRequest": {"pullRequest": {"id": "PR_7", "state": "MERGED"}}}});
    let (backend, sent) = recording_backend(vec![merged], cx);
    backend
        .merge_pull_request("PR_7".to_string(), MergeMethod::Squash, "abc".to_string())
        .await
        .expect("merged");
    let sent = sent.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let variables = &sent[0]["variables"];
    assert_eq!(variables["pullRequestId"], json!("PR_7"));
    assert_eq!(variables["mergeMethod"], json!("SQUASH"));
    assert_eq!(variables["expectedHeadOid"], json!("abc"));
    assert!(
        sent[0]["query"]
            .as_str()
            .is_some_and(|query| query.contains("expectedHeadOid: $expectedHeadOid")),
        "the guard is part of the mutation"
    );
}

#[gpui::test]
async fn a_branch_is_deleted_only_at_the_merged_commit(cx: &mut TestAppContext) {
    let done = json!({"data": {"updateRefs": {"clientMutationId": null}}});
    let (backend, sent) = recording_backend(vec![done], cx);
    backend
        .delete_branch_at("R_1".to_string(), "fix".to_string(), "abc".to_string())
        .await
        .expect("deleted");
    let sent = sent.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let variables = &sent[0]["variables"];
    assert_eq!(variables["refName"], json!("refs/heads/fix"));
    assert_eq!(variables["expectedOid"], json!("abc"));
    assert_eq!(
        variables["deletedOid"],
        json!("0000000000000000000000000000000000000000")
    );
}

#[test]
fn the_fake_refuses_a_merge_once_the_head_moved() {
    let backend =
        FakeGithubBackend::new().with_pull_request(&repository(), fake_pull_request(7, "Fix"));
    let shown = block_on(backend.pull_request(&repository(), 7))
        .expect("read")
        .head_ref_oid;
    backend.push_to_pull_request(&repository(), 7, "def");
    let refused =
        block_on(backend.merge_pull_request("PR_7".to_string(), MergeMethod::Merge, shown.clone()));
    assert!(matches!(refused, Err(GithubError::Rejected { .. })));
    assert_eq!(
        backend.writes(),
        vec![FakeWrite::MergePullRequest(
            "PR_7".to_string(),
            MergeMethod::Merge,
            shown
        )]
    );
}
