//! Writes that overlap: an edit's save with a triage change, two row writes,
//! and the header that names where an issue's tab writes.

use std::sync::Arc;

use editor::Editor;
use github::{
    Comment, FakeGithubBackend, FakeWrite, GithubBackend, GithubError, IssueState, Label,
    RepositoryId, RepositoryVisibility, StateReason, fake_issue,
};
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use workspace::Workspace;

use crate::github_view::DetailKey;
use crate::github_view_session::{end_test_session, session_ended_message, test_session};
use crate::issues::issues_test_support::{context, init, repository, workspace_window};
use crate::issues::{IssueDetailView, IssueList, open_issue_detail};

fn bug() -> Label {
    Label {
        name: "bug".to_string(),
        color: "d73a4a".to_string(),
        description: None,
    }
}

fn fake() -> FakeGithubBackend {
    FakeGithubBackend::new()
        .with_issue(&repository(), fake_issue(1, "First"))
        .with_issue(&repository(), fake_issue(2, "Second"))
        .with_label(&repository(), bug())
}

fn open_detail(
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

fn title_editor(view: &Entity<IssueDetailView>, cx: &mut VisualTestContext) -> Entity<Editor> {
    view.read_with(cx, |view, _| {
        view.edit.as_ref().map(|edit| edit.title.clone())
    })
    .expect("editing")
}

/// A triage write asked for while the title and description are being
/// saved is refused with a reason, not dropped without a word; once the
/// save lands it goes through.
#[gpui::test]
async fn a_triage_write_during_an_edit_save_is_refused_with_a_reason(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let view = open_detail(&workspace, fake.clone(), key(2), cx);
    view.update_in(cx, |view, window, cx| view.start_editing(window, cx));
    let title = title_editor(&view, cx);
    title.update_in(cx, |editor, window, cx| {
        editor.set_text("Renamed", window, cx)
    });

    view.update_in(cx, |view, window, cx| {
        view.save_edit(cx);
        assert!(
            view.is_busy(),
            "triage controls wait while a save is on its way"
        );
        view.add_label("bug".to_string(), window, cx);
        let error = view.write_error().map(str::to_string);
        assert!(
            error
                .as_deref()
                .is_some_and(|error| error.contains("saved")),
            "{error:?}"
        );
    });
    cx.run_until_parked();

    let label_writes = |fake: &FakeGithubBackend| {
        fake.writes()
            .into_iter()
            .filter(|write| matches!(write, FakeWrite::AddLabels(..)))
            .count()
    };
    assert_eq!(label_writes(&fake), 0, "the label was not sent");
    view.read_with(cx, |view, _| {
        assert!(!view.is_saving_edit(), "no stuck “Saving…”");
        assert!(!view.is_editing(), "the save landed");
        assert!(!view.is_busy(), "the controls are enabled again");
        assert_eq!(view.issue().expect("loaded").title, "Renamed");
    });

    view.update_in(cx, |view, window, cx| {
        view.add_label("bug".to_string(), window, cx)
    });
    cx.run_until_parked();
    assert_eq!(label_writes(&fake), 1, "after the save, the label goes");
    view.read_with(cx, |view, _| {
        assert_eq!(view.issue().expect("loaded").labels.len(), 1);
    });
}

#[gpui::test]
async fn an_own_triage_write_moves_the_edit_base_forward(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let view = open_detail(&workspace, fake.clone(), key(2), cx);
    view.update_in(cx, |view, window, cx| view.start_editing(window, cx));
    // GitHub moves `updated_at` on every change, this tab's own included.
    fake.edit_issue(&repository(), 2, |issue| {
        issue.updated_at = "2026-11-02T00:00:00Z".to_string();
    });
    view.update_in(cx, |view, window, cx| {
        view.add_label("bug".to_string(), window, cx)
    });
    cx.run_until_parked();
    let title = title_editor(&view, cx);
    title.update_in(cx, |editor, window, cx| {
        editor.set_text("Renamed", window, cx)
    });

    view.update(cx, |view, cx| view.save_edit(cx));
    cx.run_until_parked();

    view.read_with(cx, |view, _| {
        assert_eq!(view.edit_error(), None);
        assert_eq!(
            view.issue().map(|issue| issue.title.as_str()),
            Some("Renamed")
        );
    });
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

fn row_state(list: &Entity<IssueList>, number: u64, cx: &mut VisualTestContext) -> IssueState {
    list.read_with(cx, |list, _| {
        list.rows()
            .iter()
            .find(|issue| issue.number == number)
            .map(|issue| issue.state)
    })
    .expect("the row is shown")
}

#[gpui::test]
async fn two_row_closes_both_land(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let list = list_over(fake, cx);

    list.update(cx, |list, cx| {
        list.set_issue_state(1, IssueState::Closed, Some(StateReason::Completed), cx);
        list.set_issue_state(2, IssueState::Closed, Some(StateReason::Completed), cx);
    });
    cx.run_until_parked();

    assert_eq!(row_state(&list, 1, cx), IssueState::Closed);
    assert_eq!(row_state(&list, 2, cx), IssueState::Closed);
    assert_eq!(
        list.read_with(cx, |list, _| list.write_error().map(str::to_string)),
        None
    );
}

#[gpui::test]
async fn a_second_write_to_the_same_row_is_refused_while_one_is_pending(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let list = list_over(fake.clone(), cx);

    list.update(cx, |list, cx| {
        list.set_issue_state(1, IssueState::Closed, Some(StateReason::Completed), cx);
        list.set_issue_state(1, IssueState::Open, Some(StateReason::Reopened), cx);
    });
    cx.run_until_parked();

    let sent_for_first = fake
        .writes()
        .iter()
        .filter(|write| matches!(write, FakeWrite::UpdateIssue(_, 1, _)))
        .count();
    assert_eq!(sent_for_first, 1, "the second is not sent");
    assert_eq!(row_state(&list, 1, cx), IssueState::Closed);
    assert_eq!(
        list.read_with(cx, |list, _| list.write_error().map(str::to_string)),
        Some("#1 is still being changed. Try again once it finishes.".to_string())
    );
}

#[gpui::test]
async fn the_detail_header_names_the_issues_own_repository(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let other = RepositoryId::new("attacker", "public").expect("valid");
    let fake = Arc::new(
        fake()
            .with_issue(&other, fake_issue(12, "Elsewhere"))
            .with_repository_visibility(&repository(), RepositoryVisibility::Private)
            .with_repository_visibility(&other, RepositoryVisibility::Public),
    );
    let view = open_detail(
        &workspace,
        fake,
        DetailKey {
            repository: other,
            number: 12,
        },
        cx,
    );
    assert_eq!(
        view.read_with(cx, |view, _| view.target_line()),
        "@octocat → attacker/public (public)"
    );
}

#[gpui::test]
async fn a_failed_header_visibility_read_is_shown_and_retried(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake =
        Arc::new(fake().with_repository_visibility(&repository(), RepositoryVisibility::Private));
    fake.set_failure(Some(GithubError::Network {
        message: "offline".to_string(),
    }));
    let view = open_detail(&workspace, fake.clone(), key(1), cx);
    assert!(view.read_with(cx, |view, _| view.visibility_error().is_some()));

    fake.set_failure(None);
    view.update(cx, |view, cx| view.retry_visibility(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.visibility_error().is_none());
        assert!(view.target_line().ends_with("→ octo/repo (private)"));
    });
}

/// A sign-out that lands while the save reads the issue again stops the
/// save: the old session sends nothing.
#[gpui::test]
async fn signing_out_while_a_save_reads_the_issue_again_sends_no_update(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let view = open_detail(&workspace, fake.clone(), key(2), cx);
    view.update_in(cx, |view, window, cx| view.start_editing(window, cx));
    let title = title_editor(&view, cx);
    title.update_in(cx, |editor, window, cx| {
        editor.set_text("Renamed", window, cx)
    });

    let release = fake.hold_next_issue_read();
    view.update(cx, |view, cx| view.save_edit(cx));
    cx.run_until_parked();
    cx.update(|_, cx| end_test_session(cx));
    release.send(()).expect("the read is waiting");
    cx.run_until_parked();

    let updates = fake
        .writes()
        .into_iter()
        .filter(|write| matches!(write, FakeWrite::UpdateIssue(..)))
        .count();
    assert_eq!(updates, 0, "the ended session sent no update");
    view.read_with(cx, |view, _| {
        let edit = view.edit.as_ref().expect("the text being edited stays");
        assert!(!edit.saving, "no stuck “Saving…”");
        assert_eq!(
            edit.error.as_deref(),
            Some(session_ended_message("issue").as_str())
        );
    });
}

fn comment(id: u64) -> Comment {
    Comment {
        id,
        node_id: format!("IC_{id}"),
        author: Some("hubot".to_string()),
        body: format!("comment {id}"),
        html_url: format!("https://github.com/octo/repo/issues/2#issuecomment-{id}"),
        created_at: "2026-11-01T00:00:00Z".to_string(),
        updated_at: "2026-11-01T00:00:00Z".to_string(),
    }
}

fn title_of(view: &Entity<IssueDetailView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.issue().expect("loaded").title.clone())
}

/// A Reload asked for before an edit was saved reads the old title; its
/// late answer must not put it back over the saved one.
#[gpui::test]
async fn an_older_reload_does_not_overwrite_a_saved_edit(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let view = open_detail(&workspace, fake.clone(), key(2), cx);
    view.update_in(cx, |view, window, cx| view.start_editing(window, cx));
    let title = title_editor(&view, cx);
    title.update_in(cx, |editor, window, cx| {
        editor.set_text("Renamed", window, cx)
    });

    let release = fake.hold_next_issue_read();
    view.update(cx, |view, cx| view.reload_issue(cx));
    cx.run_until_parked();
    view.update(cx, |view, cx| view.save_edit(cx));
    cx.run_until_parked();
    assert_eq!(title_of(&view, cx), "Renamed");

    // The superseded read may already be gone; either way it must not show.
    release.send(()).ok();
    cx.run_until_parked();
    assert_eq!(title_of(&view, cx), "Renamed", "the saved title stays");
}

/// The same race for a triage write, which shows the issue it read back.
#[gpui::test]
async fn an_older_reload_does_not_overwrite_a_triage_write(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let view = open_detail(&workspace, fake.clone(), key(2), cx);

    let release = fake.hold_next_issue_read();
    view.update(cx, |view, cx| view.reload_issue(cx));
    cx.run_until_parked();
    view.update_in(cx, |view, window, cx| {
        view.add_label("bug".to_string(), window, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.issue().map(|issue| issue.labels.len())),
        Some(1)
    );

    // The superseded read may already be gone; either way it must not show.
    release.send(()).ok();
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.issue().map(|issue| issue.labels.len())),
        Some(1),
        "the label stays"
    );
}

/// A Reload asked for after the write was sent is the newer answer; the
/// write's read-back, which landed later, must not cover it.
#[gpui::test]
async fn a_read_back_does_not_overwrite_a_newer_reload(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let view = open_detail(&workspace, fake.clone(), key(2), cx);

    let release = fake.hold_next_issue_read();
    view.update_in(cx, |view, window, cx| {
        view.add_label("bug".to_string(), window, cx)
    });
    cx.run_until_parked();
    fake.edit_issue(&repository(), 2, |issue| {
        issue.title = "Newer".to_string();
    });
    view.update(cx, |view, cx| view.reload_issue(cx));
    cx.run_until_parked();
    assert_eq!(title_of(&view, cx), "Newer");

    release.send(()).expect("the read-back is waiting");
    cx.run_until_parked();
    assert_eq!(
        title_of(&view, cx),
        "Newer",
        "the older read-back is dropped"
    );
    view.read_with(cx, |view, _| {
        assert!(!view.is_busy(), "the controls are enabled again");
    });
}

/// Two writes asked for together: the second is not sent, and the user is
/// told why instead of nothing happening.
#[gpui::test]
async fn a_write_asked_for_during_another_says_it_is_still_being_saved(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let view = open_detail(&workspace, fake.clone(), key(2), cx);

    view.update_in(cx, |view, window, cx| {
        view.add_label("bug".to_string(), window, cx);
        view.add_assignees(vec!["octocat".to_string()], None, window, cx);
        let error = view.write_error().map(str::to_string);
        assert!(
            error
                .as_deref()
                .is_some_and(|error| error.contains("still being saved")),
            "{error:?}"
        );
    });
    cx.run_until_parked();
    assert_eq!(fake.writes().len(), 1, "only the first was sent");
    assert_eq!(
        view.read_with(cx, |view, _| view.write_error().map(str::to_string)),
        None,
        "the refusal is not left on screen once the first write has landed"
    );
}

/// A comment sent while the comments are being read again is kept: the
/// older read did not have it.
#[gpui::test]
async fn a_comment_sent_during_a_comments_reload_is_not_lost(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake().with_comment(&repository(), 2, comment(1)));
    let view = open_detail(&workspace, fake.clone(), key(2), cx);

    let release = fake.hold_next_comment_list();
    view.update(cx, |view, cx| view.reload(cx));
    cx.run_until_parked();
    let editor = view.update_in(cx, |view, window, cx| view.comment_editor(window, cx));
    editor.update_in(cx, |editor, window, cx| editor.set_text("mine", window, cx));
    view.update_in(cx, |view, window, cx| view.send_comment(window, cx));
    cx.run_until_parked();

    // The superseded read may already be gone; either way it must not show.
    release.send(()).ok();
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        let bodies = view
            .comments()
            .map(|comment| comment.body.clone())
            .collect::<Vec<_>>();
        assert_eq!(bodies, vec!["comment 1".to_string(), "mine".to_string()]);
        assert!(!view.loading_comments, "no stuck “loading”");
    });
}

/// With older comments still unread, the new one would sit before them;
/// it is said to come later and shows in its place once they are read.
#[gpui::test]
async fn a_comment_sent_with_unread_pages_takes_its_place_when_they_are_read(
    cx: &mut TestAppContext,
) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(
        fake()
            .with_page_size(1)
            .with_comment(&repository(), 2, comment(1))
            .with_comment(&repository(), 2, comment(2)),
    );
    let view = open_detail(&workspace, fake, key(2), cx);
    assert!(view.read_with(cx, |view, _| view.has_more_comments()));

    let editor = view.update_in(cx, |view, window, cx| view.comment_editor(window, cx));
    editor.update_in(cx, |editor, window, cx| editor.set_text("mine", window, cx));
    view.update_in(cx, |view, window, cx| view.send_comment(window, cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.comments().count(), 1, "not put before comment 2");
        assert!(
            view.write_notice().is_some(),
            "the user is told where it went"
        );
    });

    for _ in 0..2 {
        view.update(cx, |view, cx| view.load_more_comments(cx));
        cx.run_until_parked();
    }
    view.read_with(cx, |view, _| {
        let bodies = view
            .comments()
            .map(|comment| comment.body.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            bodies,
            vec![
                "comment 1".to_string(),
                "comment 2".to_string(),
                "mine".to_string()
            ]
        );
    });
}

/// A Reload dropped for a newer answer must not take the own-write refresh
/// with it: that read is what moves an open edit's base forward.
#[gpui::test]
async fn dropping_older_reloads_keeps_the_own_write_refresh_alive(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let view = open_detail(&workspace, fake.clone(), key(2), cx);
    view.update_in(cx, |view, window, cx| view.start_editing(window, cx));
    fake.edit_issue(&repository(), 2, |issue| {
        issue.updated_at = "2026-11-02T00:00:00Z".to_string();
    });
    let release = fake.hold_next_issue_read();
    let editor = view.update_in(cx, |view, window, cx| view.comment_editor(window, cx));
    editor.update_in(cx, |editor, window, cx| {
        editor.set_text("hello", window, cx)
    });
    view.update_in(cx, |view, window, cx| view.send_comment(window, cx));
    cx.run_until_parked();

    view.update(cx, |view, _| {
        view.supersede_issue_reads();
    });
    release.send(()).ok();
    cx.run_until_parked();

    view.read_with(cx, |view, _| {
        let edit = view.edit.as_ref().expect("still editing");
        assert_eq!(edit.base_updated_at, "2026-11-02T00:00:00Z");
    });
}

/// "Load more" on its way when a comment is sent: its page is added to the
/// ones shown, and the sent comment comes last, instead of the first page
/// replacing everything.
#[gpui::test]
async fn a_comment_sent_during_load_more_keeps_the_pages_already_read(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(
        fake()
            .with_page_size(2)
            .with_comment(&repository(), 2, comment(1))
            .with_comment(&repository(), 2, comment(2))
            .with_comment(&repository(), 2, comment(3)),
    );
    let view = open_detail(&workspace, fake.clone(), key(2), cx);
    assert!(view.read_with(cx, |view, _| view.has_more_comments()));

    let release = fake.hold_next_comment_list();
    view.update(cx, |view, cx| view.load_more_comments(cx));
    cx.run_until_parked();
    let editor = view.update_in(cx, |view, window, cx| view.comment_editor(window, cx));
    editor.update_in(cx, |editor, window, cx| editor.set_text("mine", window, cx));
    view.update_in(cx, |view, window, cx| view.send_comment(window, cx));
    cx.run_until_parked();

    release.send(()).ok();
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        let bodies = view
            .comments()
            .map(|comment| comment.body.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            bodies,
            vec![
                "comment 1".to_string(),
                "comment 2".to_string(),
                "comment 3".to_string(),
                "mine".to_string()
            ]
        );
        assert!(!view.loading_comments, "no stuck “loading”");
    });
}

/// Commenting moves the issue's `updated_at` on GitHub; a description being
/// edited must not take that for someone else's change.
#[gpui::test]
async fn an_own_comment_moves_the_edit_base_forward(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let view = open_detail(&workspace, fake.clone(), key(2), cx);
    view.update_in(cx, |view, window, cx| view.start_editing(window, cx));
    fake.edit_issue(&repository(), 2, |issue| {
        issue.updated_at = "2026-11-02T00:00:00Z".to_string();
    });
    let editor = view.update_in(cx, |view, window, cx| view.comment_editor(window, cx));
    editor.update_in(cx, |editor, window, cx| {
        editor.set_text("hello", window, cx)
    });
    view.update_in(cx, |view, window, cx| view.send_comment(window, cx));
    cx.run_until_parked();
    let title = title_editor(&view, cx);
    title.update_in(cx, |editor, window, cx| {
        editor.set_text("Renamed", window, cx)
    });

    view.update(cx, |view, cx| view.save_edit(cx));
    cx.run_until_parked();

    view.read_with(cx, |view, _| {
        assert_eq!(view.edit_error(), None);
        assert_eq!(
            view.issue().map(|issue| issue.title.as_str()),
            Some("Renamed")
        );
    });
}

/// A list row's close or reopen is a write like any other: once the session
/// is over the old backend sends nothing.
#[gpui::test]
async fn a_row_write_after_signing_out_sends_nothing(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let list = list_over(fake.clone(), cx);
    cx.update(|_, cx| end_test_session(cx));

    list.update(cx, |list, cx| {
        list.set_issue_state(1, IssueState::Closed, Some(StateReason::Completed), cx)
    });
    cx.run_until_parked();

    assert!(fake.writes().is_empty(), "nothing was sent");
    assert_eq!(row_state(&list, 1, cx), IssueState::Open);
    let error = list.read_with(cx, |list, _| list.write_error().map(str::to_string));
    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("Signed out")),
        "{error:?}"
    );
}

/// The tab's menus offer every label and milestone, not the first page.
#[gpui::test]
async fn the_tab_reads_every_page_of_labels_and_milestones(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let repository = repository();
    let fake = (1..=5).fold(fake().with_page_size(2), |fake, number| {
        fake.with_label(
            &repository,
            Label {
                name: format!("label-{number}"),
                color: "d73a4a".to_string(),
                description: None,
            },
        )
        .with_milestone(
            &repository,
            github::Milestone {
                number,
                title: format!("v{number}"),
                is_open: true,
                due_on: None,
            },
        )
    });
    let view = open_detail(&workspace, Arc::new(fake), key(2), cx);
    view.read_with(cx, |view, _| {
        assert_eq!(view.labels.len(), 6, "bug and the five more");
        assert_eq!(view.milestones.len(), 5);
        assert_eq!(view.options_note(), None);
    });
}

#[gpui::test]
async fn the_tab_stops_at_the_page_limit_and_says_so(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let repository = repository();
    let fake = (1..=12).fold(fake().with_page_size(1), |fake, number| {
        fake.with_label(
            &repository,
            Label {
                name: format!("label-{number}"),
                color: "d73a4a".to_string(),
                description: None,
            },
        )
    });
    let view = open_detail(&workspace, Arc::new(fake), key(2), cx);
    view.read_with(cx, |view, _| {
        assert_eq!(view.labels.len(), 10);
        let note = view.options_note();
        assert!(
            note.as_deref()
                .is_some_and(|note| note.contains("first 10")),
            "{note:?}"
        );
    });
}
