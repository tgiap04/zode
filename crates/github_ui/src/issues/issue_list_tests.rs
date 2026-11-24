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

use crate::issues::issues_test_support::{context, init, repository, workspace_window};
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

fn many_labels_and_milestones(count: u64, page_size: usize) -> FakeGithubBackend {
    let repository = repository();
    (1..=count).fold(
        FakeGithubBackend::new().with_page_size(page_size),
        |fake, number| {
            fake.with_label(&repository, label(&format!("label-{number}")))
                .with_milestone(&repository, milestone(number))
        },
    )
}

/// The filter menus offer every label and milestone, not the first page.
#[gpui::test]
async fn the_filter_menus_read_every_page_of_labels_and_milestones(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let list = list_over(Arc::new(many_labels_and_milestones(5, 2)), cx);
    list.read_with(cx, |list, _| {
        assert_eq!(list.labels().len(), 5);
        assert_eq!(list.milestones().len(), 5);
        assert_eq!(list.options_note(), None, "nothing was left out");
    });
}

/// A repository with more than the limit of labels gets the first ones and
/// is told so.
#[gpui::test]
async fn the_filter_menus_stop_at_the_page_limit_and_say_so(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let list = list_over(Arc::new(many_labels_and_milestones(12, 1)), cx);
    list.read_with(cx, |list, _| {
        assert_eq!(list.labels().len(), 10);
        assert_eq!(list.milestones().len(), 10);
        let note = list.options_note();
        assert!(
            note.as_deref()
                .is_some_and(|note| note.contains("first 10")),
            "{note:?}"
        );
    });
}

/// Load more after a failed page tries that page again; the rows already
/// read stay.
#[gpui::test]
async fn a_failed_load_more_can_be_tried_again(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let repository = repository();
    let fake = Arc::new((1..=5).fold(
        FakeGithubBackend::new().with_page_size(2),
        |fake, number| fake.with_issue(&repository, fake_issue(number, &format!("Issue {number}"))),
    ));
    let list = list_over(fake.clone(), cx);

    fake.set_failure(Some(GithubError::Network {
        message: "offline".to_string(),
    }));
    list.update(cx, |list, cx| list.load_more(cx));
    cx.run_until_parked();
    list.read_with(cx, |list, _| {
        assert!(matches!(list.load_state(), ListLoad::Failed(_)));
        assert!(list.has_more(), "the next page is still there to ask for");
    });
    assert_eq!(shown(&list, cx), vec![1, 2]);

    fake.set_failure(None);
    list.update(cx, |list, cx| list.load_more(cx));
    cx.run_until_parked();
    assert_eq!(shown(&list, cx), vec![1, 2, 3, 4]);
    list.read_with(cx, |list, _| assert_eq!(list.load_state(), &ListLoad::Idle));
}

/// A failed reload is not a failed page: its cursor belongs to the list
/// before it, so no "Load more" is left that would append the wrong page.
#[gpui::test]
async fn a_failed_reload_leaves_no_load_more(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let repository = repository();
    let fake = Arc::new((1..=5).fold(
        FakeGithubBackend::new().with_page_size(2),
        |fake, number| fake.with_issue(&repository, fake_issue(number, &format!("Issue {number}"))),
    ));
    let list = list_over(fake.clone(), cx);
    list.read_with(cx, |list, _| assert!(list.has_more()));

    fake.set_failure(Some(GithubError::Network {
        message: "offline".to_string(),
    }));
    list.update(cx, |list, cx| list.reload(cx));
    cx.run_until_parked();
    list.read_with(cx, |list, _| {
        assert!(matches!(list.load_state(), ListLoad::Failed(_)));
        assert!(!list.has_more());
    });
    fake.set_failure(None);
    list.update(cx, |list, cx| list.load_more(cx));
    cx.run_until_parked();
    assert_eq!(shown(&list, cx), vec![1, 2], "load more did nothing");
}

/// Ticked rows the typed text hides are still in the selection; the bar
/// counts them.
#[gpui::test]
async fn the_selection_counts_the_rows_the_text_filter_hides(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let list = list_over(Arc::new(backend()), cx);
    list.update(cx, |list, cx| {
        list.toggle_selected(1, cx);
        list.toggle_selected(3, cx);
    });
    list.read_with(cx, |list, _| assert_eq!(list.hidden_selected_count(), 0));

    list.update(cx, |list, cx| list.set_query("slow".to_string(), cx));
    list.read_with(cx, |list, _| {
        assert_eq!(list.hidden_selected_count(), 1, "#1 is ticked but hidden");
        assert_eq!(list.selected().len(), 2);
    });
    list.update(cx, |list, cx| list.set_query("nothing".to_string(), cx));
    list.read_with(cx, |list, _| assert_eq!(list.hidden_selected_count(), 2));
}

/// Drawing again with nothing changed does not filter and copy every row
/// again; each change that matters does.
#[gpui::test]
async fn the_rows_are_built_once_until_something_changes(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let repository = repository();
    let fake = Arc::new((1..=30).fold(FakeGithubBackend::new(), |fake, number| {
        fake.with_issue(&repository, fake_issue(number, &format!("Issue {number}")))
    }));
    let list = list_over(fake, cx);
    let rebuilds = |cx: &mut VisualTestContext| list.read_with(cx, |list, _| list.row_rebuilds());
    list.update(cx, |list, _| list.ensure_rows());
    let first = rebuilds(cx);
    assert_eq!(first, 1);
    for _ in 0..3 {
        list.update(cx, |list, _| list.ensure_rows());
    }
    assert_eq!(rebuilds(cx), first, "nothing changed, nothing rebuilt");

    list.update(cx, |list, cx| list.set_query("Issue 1".to_string(), cx));
    list.update(cx, |list, _| list.ensure_rows());
    assert_eq!(rebuilds(cx), first + 1, "typing changes the rows");
    list.update(cx, |list, cx| {
        list.update_row(1, |issue| issue.title = "Changed".to_string(), cx)
    });
    list.update(cx, |list, _| list.ensure_rows());
    assert_eq!(rebuilds(cx), first + 2, "a changed row is shown changed");
}

/// Drawn in a window, a long list builds its rows once however many frames
/// are drawn, and still draws after something changes.
#[gpui::test]
async fn drawing_a_long_list_builds_its_rows_once(cx: &mut TestAppContext) {
    init(cx);
    let repository = repository();
    let fake = (1..=300).fold(FakeGithubBackend::new(), |fake, number| {
        fake.with_issue(&repository, fake_issue(number, &format!("Issue {number}")))
    });
    let backend: Arc<dyn GithubBackend> = Arc::new(fake);
    let (list, cx) = cx.add_window_view(|_window, cx| {
        let mut list = IssueList::new(context(backend));
        list.reload(cx);
        list
    });
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let built = list.read_with(cx, |list, _| list.row_rebuilds());
    assert!(built >= 1);
    for _ in 0..3 {
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }
    list.read_with(cx, |list, _| {
        assert_eq!(list.row_rebuilds(), built, "three more frames, no build");
        assert_eq!(list.rows().len(), 50, "the first page");
    });

    let first = cx.debug_bounds("github-issue-row-0");
    assert!(
        first.is_some_and(|bounds| bounds.size.height > gpui::px(0.)),
        "the first row is drawn: {first:?}"
    );
    assert!(
        cx.debug_bounds("github-issue-row-49").is_none(),
        "a row far below the view is not built"
    );

    list.update(cx, |list, cx| list.set_query("Issue 4".to_string(), cx));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    list.read_with(cx, |list, _| assert_eq!(list.row_rebuilds(), built + 1));
}
