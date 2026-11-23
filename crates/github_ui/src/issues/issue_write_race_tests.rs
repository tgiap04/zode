//! Writes that overlap: an edit's save with a triage change, two row writes,
//! and the header that names where an issue's tab writes.

use std::sync::Arc;

use editor::Editor;
use github::{
    FakeGithubBackend, FakeWrite, GithubBackend, GithubError, IssueState, Label, RepositoryId,
    RepositoryVisibility, StateReason, fake_issue,
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
