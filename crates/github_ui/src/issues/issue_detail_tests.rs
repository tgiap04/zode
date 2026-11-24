use std::sync::Arc;

use github::{Comment, FakeGithubBackend, GithubBackend, RepositoryId, fake_issue};
use gpui::{Entity, TestAppContext, VisualTestContext};
use workspace::Workspace;

use crate::github_view::DetailKey;
use crate::github_view_session::test_session;
use crate::issues::issues_test_support::{detail_tabs, init, repository, workspace_window};
use crate::issues::{IssueDetailView, open_issue_detail};

fn comment(id: u64) -> Comment {
    Comment {
        id,
        node_id: format!("IC_{id}"),
        author: Some("hubot".to_string()),
        body: format!("comment {id}"),
        html_url: format!("https://github.com/octo/repo/issues/1#issuecomment-{id}"),
        created_at: "2026-11-01T00:00:00Z".to_string(),
        updated_at: "2026-11-01T00:00:00Z".to_string(),
    }
}

fn open(
    workspace: &Entity<Workspace>,
    backend: Arc<dyn GithubBackend>,
    key: DetailKey,
    cx: &mut VisualTestContext,
) -> Entity<IssueDetailView> {
    let view = workspace.update_in(cx, |workspace, window, cx| {
        open_issue_detail(workspace, backend, test_session(), key, window, cx)
    });
    cx.run_until_parked();
    view
}

fn key(number: u64) -> DetailKey {
    DetailKey {
        repository: repository(),
        number,
    }
}

#[gpui::test]
async fn one_tab_per_repository_and_number(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let other = RepositoryId::new("octo", "other").expect("valid");
    let backend: Arc<dyn GithubBackend> = Arc::new(
        FakeGithubBackend::new()
            .with_issue(&repository(), fake_issue(1, "Here"))
            .with_issue(&other, fake_issue(1, "There")),
    );
    let first = open(&workspace, backend.clone(), key(1), cx);
    let again = open(&workspace, backend.clone(), key(1), cx);
    assert_eq!(first.entity_id(), again.entity_id());
    let elsewhere = DetailKey {
        repository: other,
        number: 1,
    };
    open(&workspace, backend, elsewhere, cx);
    assert_eq!(detail_tabs(&workspace, cx).len(), 2);
    first.read_with(cx, |view, _| {
        assert_eq!(view.issue().map(|issue| issue.title.as_str()), Some("Here"));
    });
}

#[gpui::test]
async fn comments_past_a_hundred_are_read_a_page_at_a_time(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = (1..=120).fold(
        FakeGithubBackend::new()
            .with_page_size(50)
            .with_issue(&repository(), fake_issue(1, "Busy")),
        |fake, id| fake.with_comment(&repository(), 1, comment(id)),
    );
    let view = open(&workspace, Arc::new(fake), key(1), cx);
    for expected in [50, 100, 120] {
        let (count, last, more) = view.read_with(cx, |view, _| {
            (
                view.comments().count(),
                view.comments().last().map(|comment| comment.id),
                view.has_more_comments(),
            )
        });
        assert_eq!(count, expected);
        assert_eq!(last, Some(expected as u64));
        assert_eq!(more, expected < 120);
        view.update(cx, |view, cx| view.load_more_comments(cx));
        cx.run_until_parked();
    }
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

#[gpui::test]
async fn open_on_github_and_copy_link_use_the_issue_address(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = FakeGithubBackend::new().with_issue(&repository(), fake_issue(9, "Nine"));
    let view = open(&workspace, Arc::new(fake), key(9), cx);
    view.update(cx, |view, cx| {
        view.copy_link(cx);
        view.open_on_github(cx);
    });
    let page = "https://github.com/octo/repo/issues/9";
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(page)
    );
    assert_eq!(cx.opened_url().as_deref(), Some(page));
}

#[gpui::test]
async fn reload_reads_the_issue_again(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(FakeGithubBackend::new().with_issue(&repository(), fake_issue(1, "Old")));
    let view = open(&workspace, fake.clone(), key(1), cx);
    futures::executor::block_on(fake.update_issue(
        &repository(),
        1,
        github::IssueUpdate {
            title: Some("New".to_string()),
            ..Default::default()
        },
    ))
    .expect("updated");
    view.update(cx, |view, cx| view.reload(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.issue().map(|issue| issue.title.as_str()), Some("New"));
    });
}
