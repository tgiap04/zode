use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use github::{
    FakeGithubBackend, FakeWrite, GithubBackend, GithubError, GithubToken, HttpBackend, IssueState,
    IssueUpdate, Label, MIN_WRITE_INTERVAL, RepositoryPermission, RepositoryVisibility,
    StateReason, TokenSource, fake_issue,
};
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use http_client::{AsyncBody, FakeHttpClient, Response};
use serde_json::json;
use workspace::Workspace;

use crate::issues::IssueList;
use crate::issues::bulk_issue_actions::{BulkIssueDialog, BulkItemStatus, BulkPhase};
use crate::issues::issue_actions::BulkIssueAction;
use crate::issues::issues_test_support::{context, init, repository, workspace_window};

fn list_over(backend: Arc<dyn GithubBackend>, cx: &mut VisualTestContext) -> Entity<IssueList> {
    let list = cx.new(|cx| {
        let mut list = IssueList::new(context(backend));
        list.reload(cx);
        list
    });
    cx.run_until_parked();
    list
}

fn start(
    workspace: &Entity<Workspace>,
    list: &Entity<IssueList>,
    selected: &[u64],
    action: BulkIssueAction,
    cx: &mut VisualTestContext,
) -> Entity<BulkIssueDialog> {
    list.update(cx, |list, cx| {
        for number in selected {
            list.toggle_selected(*number, cx);
        }
    });
    list.update_in(cx, |list, window, cx| list.start_bulk(action, window, cx));
    cx.run_until_parked();
    workspace
        .read_with(cx, |workspace, cx| {
            workspace.active_modal::<BulkIssueDialog>(cx)
        })
        .expect("the confirmation is open")
}

fn statuses(
    dialog: &Entity<BulkIssueDialog>,
    cx: &mut VisualTestContext,
) -> Vec<(u64, BulkItemStatus)> {
    dialog.read_with(cx, |dialog, _| {
        dialog
            .items()
            .iter()
            .map(|item| (item.number, item.status.clone()))
            .collect()
    })
}

fn closing(reason: StateReason) -> IssueUpdate {
    IssueUpdate {
        state: Some(IssueState::Closed),
        state_reason: Some(reason),
        ..IssueUpdate::default()
    }
}

#[gpui::test]
async fn a_bulk_close_lists_every_issue_and_waits_for_confirmation(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(
        FakeGithubBackend::new()
            .with_issue(&repository(), fake_issue(1, "One"))
            .with_issue(&repository(), fake_issue(2, "Two"))
            .with_issue(&repository(), fake_issue(3, "Three")),
    );
    let list = list_over(fake.clone(), cx);
    let dialog = start(
        &workspace,
        &list,
        &[3, 1],
        BulkIssueAction::Close(StateReason::NotPlanned),
        cx,
    );
    let listed = dialog.read_with(cx, |dialog, _| {
        dialog
            .items()
            .iter()
            .map(|item| (item.number, item.title.clone()))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        listed,
        vec![(1, "One".to_string()), (3, "Three".to_string())]
    );
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.target_line()),
        "@octocat → octo/repo (public)"
    );
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert!(fake.writes().is_empty(), "nothing is sent before Confirm");

    dialog.update(cx, |dialog, cx| dialog.confirm(cx));
    cx.run_until_parked();
    assert_eq!(
        fake.writes(),
        vec![
            FakeWrite::UpdateIssue(repository(), 1, closing(StateReason::NotPlanned)),
            FakeWrite::UpdateIssue(repository(), 3, closing(StateReason::NotPlanned)),
        ]
    );
    assert_eq!(
        statuses(&dialog, cx),
        vec![(1, BulkItemStatus::Done), (3, BulkItemStatus::Done)]
    );
    dialog.read_with(cx, |dialog, _| {
        assert_eq!(dialog.phase(), BulkPhase::Finished);
        assert_eq!(dialog.progress(), (2, 2));
        assert_eq!(dialog.report(), "2 done");
    });
    list.read_with(cx, |list, _| {
        assert!(list.selected().is_empty());
        let visible = list
            .visible_rows()
            .iter()
            .map(|issue| issue.number)
            .collect::<Vec<_>>();
        assert_eq!(visible, vec![2]);
    });
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

#[gpui::test]
async fn a_bulk_label_github_ignores_is_reported_per_issue(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(
        FakeGithubBackend::new()
            .with_issue(&repository(), fake_issue(1, "One"))
            .with_issue(&repository(), fake_issue(2, "Two"))
            .with_label(
                &repository(),
                Label {
                    name: "bug".to_string(),
                    color: "d73a4a".to_string(),
                    description: None,
                },
            )
            .with_repository_permission(&repository(), RepositoryPermission::Read),
    );
    let list = list_over(fake.clone(), cx);
    let dialog = start(
        &workspace,
        &list,
        &[1, 2],
        BulkIssueAction::AddLabel("bug".to_string()),
        cx,
    );
    dialog.update(cx, |dialog, cx| dialog.confirm(cx));
    cx.run_until_parked();
    assert_eq!(
        fake.writes(),
        vec![
            FakeWrite::AddLabels(repository(), 1, vec!["bug".to_string()]),
            FakeWrite::AddLabels(repository(), 2, vec!["bug".to_string()]),
        ]
    );
    assert_eq!(
        statuses(&dialog, cx),
        vec![
            (1, BulkItemStatus::NotApplied),
            (2, BulkItemStatus::NotApplied)
        ]
    );
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.report()),
        "2 not applied"
    );
}

/// A PATCH that reached the fake server, and when.
struct Sent {
    path: String,
    at: Instant,
}

fn issue_json(number: u64, state: &str) -> serde_json::Value {
    json!({
        "id": 1000 + number,
        "node_id": format!("I_{number}"),
        "number": number,
        "title": format!("Issue {number}"),
        "body": null,
        "state": state,
        "state_reason": null,
        "user": {"login": "octocat"},
        "labels": [],
        "assignees": [],
        "milestone": null,
        "comments": 0,
        "html_url": format!("https://github.com/octo/repo/issues/{number}"),
        "created_at": "2026-11-01T00:00:00Z",
        "updated_at": "2026-11-01T00:00:00Z"
    })
}

/// A real backend over a fake server holding issues #1 to #3, so writes go
/// through the token's write queue. Closing an issue in `failing` is
/// answered 404.
fn http_backend(
    failing: &'static [u64],
    cx: &mut TestAppContext,
) -> (Arc<dyn GithubBackend>, Arc<Mutex<Vec<Sent>>>) {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let executor = cx.executor();
    let client = FakeHttpClient::create({
        let sent = sent.clone();
        move |request| {
            let sent = sent.clone();
            let executor = executor.clone();
            async move {
                let path = request.uri().path().to_string();
                let method = request.method().as_str().to_string();
                let (status, body) = if method == "PATCH" {
                    sent.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(Sent {
                            path: path.clone(),
                            at: executor.now(),
                        });
                    let number = path
                        .rsplit('/')
                        .next()
                        .and_then(|number| number.parse::<u64>().ok())
                        .unwrap_or_default();
                    if failing.contains(&number) {
                        (404, json!({"message": "Not Found"}))
                    } else {
                        (200, issue_json(number, "closed"))
                    }
                } else if path == "/repos/octo/repo" {
                    (
                        200,
                        json!({"private": true, "visibility": "private",
                               "permissions": {"admin": false, "push": true, "triage": true}}),
                    )
                } else if path == "/repos/octo/repo/issues" {
                    (
                        200,
                        json!([
                            issue_json(1, "open"),
                            issue_json(2, "open"),
                            issue_json(3, "open")
                        ]),
                    )
                } else {
                    (200, json!([]))
                };
                Ok(Response::builder()
                    .status(status)
                    .body(AsyncBody::from(body.to_string()))?)
            }
        }
    });
    let backend = cx.update(|cx| {
        HttpBackend::new(client, GithubToken::new("gho_bulk", TokenSource::GhCli), cx)
    });
    (Arc::new(backend), sent)
}

fn patched(sent: &Arc<Mutex<Vec<Sent>>>) -> Vec<String> {
    sent.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .map(|sent| sent.path.clone())
        .collect()
}

#[gpui::test]
async fn bulk_writes_are_spaced_by_the_write_queue_and_stop_when_cancelled(
    cx: &mut TestAppContext,
) {
    init(cx);
    let (backend, sent) = http_backend(&[], cx);
    let (workspace, cx) = workspace_window(cx).await;
    let list = list_over(backend, cx);
    let dialog = start(
        &workspace,
        &list,
        &[1, 2, 3],
        BulkIssueAction::Close(StateReason::Completed),
        cx,
    );
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.target_line()),
        "@octocat → octo/repo (private)"
    );
    dialog.update(cx, |dialog, cx| dialog.confirm(cx));
    cx.run_until_parked();
    assert_eq!(patched(&sent), vec!["/repos/octo/repo/issues/1"]);

    cx.executor().advance_clock(MIN_WRITE_INTERVAL);
    cx.run_until_parked();
    assert_eq!(
        patched(&sent),
        vec!["/repos/octo/repo/issues/1", "/repos/octo/repo/issues/2"]
    );
    let gap = {
        let sent = sent.lock().unwrap_or_else(PoisonError::into_inner);
        sent[1].at - sent[0].at
    };
    assert!(gap >= MIN_WRITE_INTERVAL, "writes {gap:?} apart");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.progress()), (2, 3));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();

    dialog.update(cx, |dialog, cx| dialog.cancel(cx));
    cx.executor().advance_clock(Duration::from_secs(10));
    cx.run_until_parked();
    assert_eq!(patched(&sent).len(), 2, "nothing is sent after Stop");
    assert_eq!(
        statuses(&dialog, cx),
        vec![
            (1, BulkItemStatus::Done),
            (2, BulkItemStatus::Done),
            (3, BulkItemStatus::Interrupted),
        ]
    );
    dialog.read_with(cx, |dialog, _| {
        assert_eq!(dialog.phase(), BulkPhase::Finished);
        assert_eq!(dialog.report(), "2 done, 1 cancelled");
    });
}

#[gpui::test]
async fn a_failed_item_is_reported_and_the_others_still_run(cx: &mut TestAppContext) {
    init(cx);
    let (backend, sent) = http_backend(&[2], cx);
    let (workspace, cx) = workspace_window(cx).await;
    let list = list_over(backend, cx);
    let dialog = start(
        &workspace,
        &list,
        &[1, 2, 3],
        BulkIssueAction::Close(StateReason::Completed),
        cx,
    );
    dialog.update(cx, |dialog, cx| dialog.confirm(cx));
    for _ in 0..3 {
        cx.run_until_parked();
        cx.executor().advance_clock(MIN_WRITE_INTERVAL);
    }
    cx.run_until_parked();
    assert_eq!(patched(&sent).len(), 3);
    let statuses = statuses(&dialog, cx);
    assert_eq!(statuses[0], (1, BulkItemStatus::Done));
    assert!(
        matches!(&statuses[1], (2, BulkItemStatus::Failed(_))),
        "{statuses:?}"
    );
    assert_eq!(statuses[2], (3, BulkItemStatus::Done));
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.report()),
        "2 done, 1 failed"
    );
}

#[gpui::test]
async fn a_failed_visibility_read_is_shown_and_can_be_retried(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(
        FakeGithubBackend::new()
            .with_issue(&repository(), fake_issue(1, "First"))
            .with_repository_visibility(&repository(), RepositoryVisibility::Private),
    );
    let list = list_over(fake.clone(), cx);
    fake.set_failure(Some(GithubError::Network {
        message: "offline".to_string(),
    }));
    let dialog = start(
        &workspace,
        &list,
        &[1],
        BulkIssueAction::Close(StateReason::Completed),
        cx,
    );
    dialog.read_with(cx, |dialog, _| {
        assert!(dialog.visibility_error().is_some(), "the failure is shown");
    });
    dialog.update(cx, |dialog, cx| dialog.confirm(cx));
    cx.run_until_parked();
    assert_eq!(fake.writes(), Vec::new(), "nothing is sent unconfirmed");

    fake.set_failure(None);
    dialog.update(cx, |dialog, cx| dialog.retry_visibility(cx));
    cx.run_until_parked();
    dialog.read_with(cx, |dialog, _| {
        assert!(dialog.visibility_error().is_none());
        assert_eq!(dialog.target_line(), "@octocat → octo/repo (private)");
    });
}

#[gpui::test]
async fn confirming_after_signing_out_sends_nothing(cx: &mut TestAppContext) {
    init(cx);
    let (backend, sent) = http_backend(&[], cx);
    let (workspace, cx) = workspace_window(cx).await;
    let list = list_over(backend, cx);
    let dialog = start(
        &workspace,
        &list,
        &[1, 2],
        BulkIssueAction::Close(StateReason::Completed),
        cx,
    );
    cx.update(|_, cx| crate::github_view_session::end_test_session(cx));
    dialog.update(cx, |dialog, cx| dialog.confirm(cx));
    cx.executor().advance_clock(Duration::from_secs(10));
    cx.run_until_parked();

    assert_eq!(patched(&sent), Vec::<String>::new(), "nothing is sent");
    let report = dialog.read_with(cx, |dialog, _| dialog.report());
    assert_eq!(report, "2 not sent: signed out");
}

#[gpui::test]
async fn signing_out_during_a_run_stops_it_before_the_next_write(cx: &mut TestAppContext) {
    init(cx);
    let (backend, sent) = http_backend(&[], cx);
    let (workspace, cx) = workspace_window(cx).await;
    let list = list_over(backend, cx);
    let dialog = start(
        &workspace,
        &list,
        &[1, 2, 3],
        BulkIssueAction::Close(StateReason::Completed),
        cx,
    );
    dialog.update(cx, |dialog, cx| dialog.confirm(cx));
    cx.run_until_parked();
    assert_eq!(patched(&sent), vec!["/repos/octo/repo/issues/1"]);

    cx.update(|_, cx| crate::github_view_session::end_test_session(cx));
    cx.executor().advance_clock(Duration::from_secs(10));
    cx.run_until_parked();

    assert_eq!(patched(&sent).len(), 1, "nothing is sent after signing out");
    dialog.read_with(cx, |dialog, _| {
        assert_eq!(dialog.phase(), BulkPhase::Finished);
        assert_eq!(
            dialog.report(),
            "1 done, 1 cancelled, 1 not sent: signed out"
        );
    });
}
