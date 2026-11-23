use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use futures::AsyncReadExt as _;
use github::{
    FakeGithubBackend, GithubBackend, GithubError, GithubToken, HttpBackend, IssueState,
    IssueStateFilter, Label, Milestone, TokenSource, fake_issue,
};
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use http_client::{AsyncBody, FakeHttpClient, Response};
use serde_json::json;

use crate::issues::issues_test_support::{context, init, workspace_window};
use crate::issues::{IssueList, ListLoad, ListPlaceholder, MineFilter};
use crate::untrusted_text::reveal_hidden_characters;

fn milestone(number: u64) -> Milestone {
    Milestone {
        number,
        title: format!("v{number}"),
        is_open: true,
        due_on: None,
    }
}

fn label(name: &str) -> Label {
    Label {
        name: name.to_string(),
        color: "d73a4a".to_string(),
        description: None,
    }
}

/// Open #1 (bug, assigned to octocat), closed #2 (by octocat), open #3 (by
/// hubot, milestone 7, mentions octocat).
fn backend() -> FakeGithubBackend {
    let repository = crate::issues::issues_test_support::repository();
    let mut first = fake_issue(1, "Crash when saving");
    first.labels = vec![label("bug")];
    first.assignees = vec!["octocat".to_string()];
    first.author = Some("hubot".to_string());
    let mut second = fake_issue(2, "Docs are stale");
    second.state = IssueState::Closed;
    let mut third = fake_issue(3, "Slow start");
    third.author = Some("hubot".to_string());
    third.milestone = Some(milestone(7));
    third.body = Some("cc @octocat".to_string());
    FakeGithubBackend::new()
        .with_issue(&repository, first)
        .with_issue(&repository, second)
        .with_issue(&repository, third)
        .with_label(&repository, label("bug"))
        .with_milestone(&repository, milestone(7))
}

fn list_over(backend: Arc<dyn GithubBackend>, cx: &mut VisualTestContext) -> Entity<IssueList> {
    let list = cx.new(|cx| {
        let mut list = IssueList::new(context(backend));
        list.reload(cx);
        list
    });
    cx.run_until_parked();
    list
}

fn shown(list: &Entity<IssueList>, cx: &mut VisualTestContext) -> Vec<u64> {
    list.read_with(cx, |list, _| {
        list.visible_rows()
            .iter()
            .map(|issue| issue.number)
            .collect()
    })
}

#[gpui::test]
async fn the_state_and_mine_filters_narrow_the_list(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let list = list_over(Arc::new(backend()), cx);
    assert_eq!(shown(&list, cx), vec![1, 3], "open issues first");

    list.update(cx, |list, cx| list.set_state(IssueStateFilter::Closed, cx));
    cx.run_until_parked();
    assert_eq!(shown(&list, cx), vec![2]);

    list.update(cx, |list, cx| list.set_state(IssueStateFilter::All, cx));
    cx.run_until_parked();
    assert_eq!(shown(&list, cx), vec![1, 2, 3]);

    let mine = [
        (MineFilter::Assigned, vec![1]),
        (MineFilter::Created, vec![2]),
        (MineFilter::Mentioned, vec![3]),
    ];
    for (filter, expected) in mine {
        list.update(cx, |list, cx| list.set_mine(Some(filter), cx));
        cx.run_until_parked();
        assert_eq!(shown(&list, cx), expected, "{filter:?}");
    }
}

#[gpui::test]
async fn the_label_and_milestone_filters_narrow_the_list(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let list = list_over(Arc::new(backend()), cx);
    list.read_with(cx, |list, _| {
        assert_eq!(list.labels(), &[label("bug")]);
        assert_eq!(list.milestones(), &[milestone(7)]);
    });

    list.update(cx, |list, cx| list.set_label(Some("bug".to_string()), cx));
    cx.run_until_parked();
    assert_eq!(shown(&list, cx), vec![1]);

    list.update(cx, |list, cx| {
        list.set_label(None, cx);
        list.set_milestone(Some(milestone(7)), cx);
    });
    cx.run_until_parked();
    assert_eq!(shown(&list, cx), vec![3]);
}

#[gpui::test]
async fn typing_narrows_loaded_rows_and_only_enter_searches(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(backend());
    let list = list_over(fake.clone(), cx);

    list.update(cx, |list, cx| list.set_query("crash".to_string(), cx));
    cx.run_until_parked();
    assert_eq!(shown(&list, cx), vec![1]);
    list.update(cx, |list, cx| list.set_query("#3".to_string(), cx));
    assert_eq!(shown(&list, cx), vec![3]);
    assert!(fake.searches().is_empty(), "typing sends nothing");

    list.update(cx, |list, cx| {
        list.set_query("start".to_string(), cx);
        list.search(cx);
    });
    cx.run_until_parked();
    assert_eq!(fake.searches(), vec!["start".to_string()]);
    list.read_with(cx, |list, _| assert_eq!(list.searched(), Some("start")));
    assert_eq!(shown(&list, cx), vec![3]);

    list.update(cx, |list, cx| {
        list.set_query(String::new(), cx);
        list.search(cx);
    });
    cx.run_until_parked();
    assert_eq!(
        fake.searches().len(),
        1,
        "an empty search is the plain list"
    );
    assert_eq!(shown(&list, cx), vec![1, 3]);
}

#[gpui::test]
async fn load_more_appends_the_next_page(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let repository = crate::issues::issues_test_support::repository();
    let fake = (1..=5).fold(
        FakeGithubBackend::new().with_page_size(2),
        |fake, number| fake.with_issue(&repository, fake_issue(number, &format!("Issue {number}"))),
    );
    let list = list_over(Arc::new(fake), cx);
    assert_eq!(shown(&list, cx), vec![1, 2]);
    for expected in [vec![1, 2, 3, 4], vec![1, 2, 3, 4, 5]] {
        list.read_with(cx, |list, _| assert!(list.has_more()));
        list.update(cx, |list, cx| list.load_more(cx));
        cx.run_until_parked();
        assert_eq!(shown(&list, cx), expected);
    }
    list.read_with(cx, |list, _| assert!(!list.has_more()));
}

#[gpui::test]
async fn an_older_answer_never_replaces_a_newer_one(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(backend());
    let list = list_over(fake.clone(), cx);

    let release_open = fake.hold_next_issue_list();
    list.update(cx, |list, cx| list.set_state(IssueStateFilter::All, cx));
    cx.run_until_parked();
    list.read_with(cx, |list, _| {
        assert_eq!(list.load_state(), &ListLoad::Loading)
    });
    list.update(cx, |list, cx| list.set_state(IssueStateFilter::Closed, cx));
    cx.run_until_parked();
    assert_eq!(shown(&list, cx), vec![2]);
    release_open.send(()).ok();
    cx.run_until_parked();
    list.read_with(cx, |list, _| {
        let rows = list
            .rows()
            .iter()
            .map(|issue| issue.number)
            .collect::<Vec<_>>();
        assert_eq!(rows, vec![2], "the full list, asked first, lands too late");
        assert_eq!(list.load_state(), &ListLoad::Idle);
    });
}

#[gpui::test]
async fn a_failed_load_is_reported(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let fake = FakeGithubBackend::new().failing_with(GithubError::NotFound);
    let list = list_over(Arc::new(fake), cx);
    list.read_with(cx, |list, _| {
        assert_eq!(list.load_state(), &ListLoad::Failed(GithubError::NotFound));
    });
}

#[gpui::test]
async fn a_title_with_a_bidi_override_shows_a_marker(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let repository = crate::issues::issues_test_support::repository();
    let fake =
        FakeGithubBackend::new().with_issue(&repository, fake_issue(1, "Update \u{202E}gpj.exe"));
    let list = list_over(Arc::new(fake), cx);
    let titles = list.read_with(cx, |list, _| {
        list.visible_rows()
            .iter()
            .map(|issue| reveal_hidden_characters(&issue.title).into_owned())
            .collect::<Vec<_>>()
    });
    assert_eq!(titles, vec!["Update <U+202E>gpj.exe".to_string()]);
    // A real frame, so the row is drawn through the same helper.
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

/// Every request, and whether it carried an ETag.
#[derive(Clone, Default)]
struct Recorded(Arc<Mutex<Vec<(String, Option<String>)>>>);

#[gpui::test]
async fn reload_is_answered_from_the_etag_cache(cx: &mut TestAppContext) {
    init(cx);
    let recorded = Recorded::default();
    let answers = Arc::new(Mutex::new(VecDeque::from([
        (
            200,
            Some("W/\"v1\""),
            json!([{
                "id": 1001, "node_id": "I_1", "number": 1, "title": "Crash", "body": null,
                "state": "open", "state_reason": null, "user": {"login": "hubot"},
                "labels": [], "assignees": [], "milestone": null, "comments": 0,
                "html_url": "https://github.com/octo/repo/issues/1",
                "created_at": "2026-11-01T00:00:00Z", "updated_at": "2026-11-01T00:00:00Z"
            }])
            .to_string(),
        ),
        (304, Some("W/\"v1\""), String::new()),
    ])));
    let client = FakeHttpClient::create({
        let recorded = recorded.clone();
        move |request| {
            let recorded = recorded.clone();
            let answers = answers.clone();
            async move {
                let url = request.uri().to_string();
                let etag = request
                    .headers()
                    .get("If-None-Match")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string);
                let mut body = String::new();
                request.into_body().read_to_string(&mut body).await?;
                recorded
                    .0
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push((url.clone(), etag));
                if !url.contains("/issues?") {
                    return Ok(Response::builder()
                        .status(200)
                        .body(AsyncBody::from("[]"))?);
                }
                let (status, etag, body) = answers
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .pop_front()
                    .ok_or_else(|| anyhow::anyhow!("no more answers"))?;
                let mut response = Response::builder()
                    .status(status)
                    .header("x-ratelimit-limit", "5000")
                    .header("x-ratelimit-remaining", "4990")
                    .header("x-ratelimit-reset", "1900000000");
                if let Some(etag) = etag {
                    response = response.header("etag", etag);
                }
                Ok(response.body(AsyncBody::from(body))?)
            }
        }
    });
    let backend = cx.update(|cx| {
        HttpBackend::new(client, GithubToken::new("gho_test", TokenSource::GhCli), cx)
    });
    let (_workspace, cx) = workspace_window(cx).await;
    let list = list_over(Arc::new(backend), cx);
    assert_eq!(shown(&list, cx), vec![1]);

    list.update(cx, |list, cx| list.reload(cx));
    cx.run_until_parked();
    assert_eq!(shown(&list, cx), vec![1], "the cached rows are kept");
    let issue_requests = recorded
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter(|(url, _)| url.contains("/issues?"))
        .map(|(_, etag)| etag.clone())
        .collect::<Vec<_>>();
    assert_eq!(issue_requests, vec![None, Some("W/\"v1\"".to_string())]);
}

#[gpui::test]
async fn an_empty_list_says_why_it_is_empty(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;

    let held = Arc::new(backend());
    let release = held.hold_next_issue_list();
    let list = cx.new(|cx| {
        let mut list = IssueList::new(context(held.clone()));
        list.reload(cx);
        list
    });
    cx.run_until_parked();
    assert_eq!(
        list.read_with(cx, |list, _| list.placeholder()),
        Some(ListPlaceholder::Loading)
    );
    release.send(()).ok();
    cx.run_until_parked();
    assert_eq!(list.read_with(cx, |list, _| list.placeholder()), None);

    list.update(cx, |list, cx| {
        list.set_query("  nothing like it ".to_string(), cx)
    });
    assert_eq!(
        list.read_with(cx, |list, _| list.placeholder()),
        Some(ListPlaceholder::NoMatches("  nothing like it ".to_string())),
        "loaded issues hidden by the typed text"
    );

    list.update(cx, |list, cx| list.search(cx));
    cx.run_until_parked();
    assert_eq!(
        list.read_with(cx, |list, _| list.placeholder()),
        Some(ListPlaceholder::NoSearchResults(
            "nothing like it".to_string()
        )),
        "GitHub's search found nothing"
    );

    let empty = list_over(Arc::new(FakeGithubBackend::new()), cx);
    assert_eq!(
        empty.read_with(cx, |list, _| list.placeholder()),
        Some(ListPlaceholder::NoIssues)
    );

    let failing = list_over(
        Arc::new(FakeGithubBackend::new().failing_with(GithubError::NotFound)),
        cx,
    );
    assert_eq!(
        failing.read_with(cx, |list, _| list.placeholder()),
        Some(ListPlaceholder::Failed(GithubError::NotFound))
    );
}
