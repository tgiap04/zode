use std::sync::Arc;

use editor::Editor;
use github::{
    FakeGithubBackend, FakeWrite, GithubBackend, GithubError, IssueState, IssueUpdate, Label,
    NewIssue, RepositoryPermission, RepositoryVisibility, StateReason, fake_issue,
};
use gpui::{
    AppContext as _, Entity, Focusable as _, KeyBinding, TestAppContext, VisualTestContext,
};
use workspace::Workspace;

use crate::github_view::DetailKey;
use crate::github_view_session::test_session;
use crate::issues::assignee_picker_tests::user as picker_user;
use crate::issues::issue_actions::IssueMenuEntry;
use crate::issues::issue_detail_writes::EDIT_CONFLICT;
use crate::issues::issue_editor::CreateIssueDialog;
use crate::issues::issues_test_support::{context, init, repository, workspace_window};
use crate::issues::{IssueDetailView, IssueList, open_issue_detail};

fn bug() -> Label {
    Label {
        name: "bug".to_string(),
        color: "d73a4a".to_string(),
        description: None,
    }
}

/// #1 by hubot, #2 and #3 by octocat, and a `bug` label to pick.
fn fake() -> FakeGithubBackend {
    let mut first = fake_issue(1, "First");
    first.author = Some("hubot".to_string());
    FakeGithubBackend::new()
        .with_issue(&repository(), first)
        .with_issue(&repository(), fake_issue(2, "Second"))
        .with_issue(&repository(), fake_issue(3, "Third"))
        .with_label(&repository(), bug())
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

fn row_numbers(list: &Entity<IssueList>, cx: &mut VisualTestContext) -> Vec<u64> {
    list.read_with(cx, |list, _| {
        list.rows().iter().map(|issue| issue.number).collect()
    })
}

fn open_create_dialog(
    workspace: &Entity<Workspace>,
    list: &Entity<IssueList>,
    cx: &mut VisualTestContext,
) -> Entity<CreateIssueDialog> {
    list.update_in(cx, |list, window, cx| list.new_issue(window, cx));
    cx.run_until_parked();
    workspace
        .read_with(cx, |workspace, cx| {
            workspace.active_modal::<CreateIssueDialog>(cx)
        })
        .expect("the New issue dialog is open")
}

fn set_text(editor: &Entity<Editor>, text: &str, cx: &mut VisualTestContext) {
    editor.update_in(cx, |editor, window, cx| editor.set_text(text, window, cx));
}

fn text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, cx| editor.text(cx))
}

fn open_detail(
    workspace: &Entity<Workspace>,
    backend: Arc<dyn GithubBackend>,
    number: u64,
    cx: &mut VisualTestContext,
) -> Entity<IssueDetailView> {
    let key = DetailKey {
        repository: repository(),
        number,
    };
    let view = workspace.update_in(cx, |workspace, window, cx| {
        open_issue_detail(workspace, backend, test_session(), key, window, cx)
    });
    cx.run_until_parked();
    view
}

#[gpui::test]
async fn a_created_issue_is_sent_once_and_goes_to_the_top_of_the_list(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let list = list_over(fake.clone(), cx);
    assert_eq!(row_numbers(&list, cx), vec![1, 2, 3]);

    let dialog = open_create_dialog(&workspace, &list, cx);
    let (title, body) =
        dialog.read_with(cx, |dialog, _| (dialog.title.clone(), dialog.body.clone()));
    set_text(&title, "  Brand new  ", cx);
    set_text(&body, "Steps:\n1. open", cx);
    dialog.update(cx, |dialog, cx| dialog.submit(cx));
    cx.run_until_parked();

    assert_eq!(
        fake.writes(),
        vec![FakeWrite::CreateIssue(
            repository(),
            NewIssue {
                title: "Brand new".to_string(),
                body: Some("Steps:\n1. open".to_string()),
                ..NewIssue::default()
            }
        )]
    );
    assert_eq!(row_numbers(&list, cx), vec![4, 1, 2, 3]);
    assert_eq!(
        list.read_with(cx, |list, _| list.notice().map(str::to_string)),
        Some("Created #4.".to_string())
    );
    assert!(
        workspace
            .read_with(cx, |workspace, cx| workspace
                .active_modal::<CreateIssueDialog>(cx))
            .is_none(),
        "the dialog closes once the issue exists"
    );
}

#[gpui::test]
async fn the_create_dialog_names_the_account_and_the_repository_with_its_visibility(
    cx: &mut TestAppContext,
) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake =
        Arc::new(fake().with_repository_visibility(&repository(), RepositoryVisibility::Private));
    let list = list_over(fake.clone(), cx);
    let dialog = open_create_dialog(&workspace, &list, cx);
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.target_line()),
        "@octocat → octo/repo (private)"
    );
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();

    dialog.update(cx, |dialog, cx| dialog.submit(cx));
    cx.run_until_parked();
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.error().map(str::to_string)),
        Some("A title is required.".to_string())
    );
    assert!(fake.writes().is_empty(), "no title, nothing sent");
}

#[gpui::test]
async fn a_failed_create_keeps_the_dialog_and_everything_typed(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let list = list_over(fake.clone(), cx);
    let dialog = open_create_dialog(&workspace, &list, cx);
    let (title, body) =
        dialog.read_with(cx, |dialog, _| (dialog.title.clone(), dialog.body.clone()));
    set_text(&title, "Keep me", cx);
    set_text(&body, "A long description", cx);
    fake.set_failure(Some(GithubError::Rejected {
        message: "Validation Failed".to_string(),
    }));
    dialog.update(cx, |dialog, cx| dialog.submit(cx));
    cx.run_until_parked();

    let error = dialog.read_with(cx, |dialog, _| dialog.error().map(str::to_string));
    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("Validation Failed")),
        "{error:?}"
    );
    assert!(
        workspace
            .read_with(cx, |workspace, cx| workspace
                .active_modal::<CreateIssueDialog>(cx))
            .is_some(),
        "the dialog stays open"
    );
    assert_eq!(text(&title, cx), "Keep me");
    assert_eq!(text(&body, cx), "A long description");
    assert_eq!(row_numbers(&list, cx), vec![1, 2, 3]);
}

#[gpui::test]
async fn labels_github_drops_from_a_new_issue_are_reported(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake =
        Arc::new(fake().with_repository_permission(&repository(), RepositoryPermission::Read));
    let list = list_over(fake.clone(), cx);
    let dialog = open_create_dialog(&workspace, &list, cx);
    let title = dialog.read_with(cx, |dialog, _| dialog.title.clone());
    set_text(&title, "Needs a label", cx);
    dialog.update(cx, |dialog, cx| dialog.toggle_label("bug".to_string(), cx));
    dialog.update(cx, |dialog, cx| dialog.submit(cx));
    cx.run_until_parked();

    let notice = list.read_with(cx, |list, _| list.notice().map(str::to_string));
    assert_eq!(
        notice.as_deref(),
        Some(
            "Created #4. GitHub did not apply the labels. @octocat may not have push access to octo/repo."
        )
    );
    assert_eq!(
        fake.writes(),
        vec![FakeWrite::CreateIssue(
            repository(),
            NewIssue {
                title: "Needs a label".to_string(),
                labels: vec!["bug".to_string()],
                ..NewIssue::default()
            }
        )],
        "the label was asked for; GitHub dropped it"
    );
}

#[gpui::test]
async fn adding_a_label_uses_the_add_endpoint(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let view = open_detail(&workspace, fake.clone(), 2, cx);
    view.update_in(cx, |view, window, cx| {
        view.add_label("bug".to_string(), window, cx)
    });
    cx.run_until_parked();

    assert_eq!(
        fake.writes(),
        vec![FakeWrite::AddLabels(
            repository(),
            2,
            vec!["bug".to_string()]
        )],
        "never a whole-set update"
    );
    view.read_with(cx, |view, _| {
        let labels = view.issue().map(|issue| {
            issue
                .labels
                .iter()
                .map(|label| label.name.clone())
                .collect::<Vec<_>>()
        });
        assert_eq!(labels, Some(vec!["bug".to_string()]));
        assert_eq!(view.write_notice(), None);
        assert_eq!(view.write_error(), None);
    });
}

#[gpui::test]
async fn labels_assignees_and_milestones_github_ignores_are_reported(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(
        fake()
            .with_milestone(
                &repository(),
                github::Milestone {
                    number: 7,
                    title: "v7".to_string(),
                    is_open: true,
                    due_on: None,
                },
            )
            .with_repository_permission(&repository(), RepositoryPermission::Read),
    );
    let view = open_detail(&workspace, fake.clone(), 2, cx);
    let notice_after = |write: &dyn Fn(
        &mut IssueDetailView,
        &mut gpui::Window,
        &mut gpui::Context<IssueDetailView>,
    ),
                        cx: &mut VisualTestContext| {
        view.update_in(cx, |view, window, cx| write(view, window, cx));
        cx.run_until_parked();
        view.read_with(cx, |view, _| view.write_notice().map(str::to_string))
    };
    assert_eq!(
        notice_after(
            &|view, window, cx| view.add_label("bug".to_string(), window, cx),
            cx
        )
        .as_deref(),
        Some(
            "GitHub did not apply the label “bug”. @octocat may not have permission to triage issues in octo/repo."
        )
    );
    let assigned = notice_after(
        &|view, window, cx| view.add_assignees(vec!["hubot".to_string()], None, window, cx),
        cx,
    );
    assert!(
        assigned
            .as_deref()
            .is_some_and(|notice| notice.starts_with("GitHub did not apply assigning @hubot.")),
        "{assigned:?}"
    );
    let milestone = notice_after(
        &|view, window, cx| view.set_milestone(Some(7), window, cx),
        cx,
    );
    assert!(
        milestone
            .as_deref()
            .is_some_and(|notice| notice.starts_with("GitHub did not apply the milestone.")),
        "{milestone:?}"
    );
    assert_eq!(
        fake.writes(),
        vec![
            FakeWrite::AddLabels(repository(), 2, vec!["bug".to_string()]),
            FakeWrite::AddAssignees(repository(), 2, vec!["hubot".to_string()]),
            FakeWrite::SetMilestone(repository(), 2, Some(7)),
        ]
    );
}

#[gpui::test]
async fn a_second_triage_write_is_not_sent_while_the_first_is_read_back(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let view = open_detail(&workspace, fake.clone(), 2, cx);
    view.update_in(cx, |view, window, cx| {
        view.add_label("bug".to_string(), window, cx);
        view.remove_label("bug".to_string(), window, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        fake.writes(),
        vec![FakeWrite::AddLabels(
            repository(),
            2,
            vec!["bug".to_string()]
        )],
        "the second is not sent while the first is being read back"
    );
    view.update_in(cx, |view, window, cx| {
        view.remove_label("bug".to_string(), window, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        fake.writes().last(),
        Some(&FakeWrite::RemoveLabel(repository(), 2, "bug".to_string()))
    );
    view.read_with(cx, |view, _| {
        assert_eq!(view.issue().map(|issue| issue.labels.len()), Some(0));
        assert_eq!(view.write_notice(), None);
    });
}

#[gpui::test]
async fn an_edit_over_someone_elses_change_is_stopped_and_keeps_the_text(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let view = open_detail(&workspace, fake.clone(), 2, cx);
    view.update_in(cx, |view, window, cx| view.start_editing(window, cx));
    let title = view.read_with(cx, |view, _| {
        view.edit.as_ref().map(|edit| edit.title.clone())
    });
    let title = title.expect("editing");
    set_text(&title, "Mine", cx);
    fake.edit_issue(&repository(), 2, |issue| {
        issue.title = "Theirs".to_string();
        issue.updated_at = "2026-11-02T00:00:00Z".to_string();
    });

    view.update(cx, |view, cx| view.save_edit(cx));
    cx.run_until_parked();
    assert!(fake.writes().is_empty(), "nothing overwritten");
    view.read_with(cx, |view, _| {
        assert_eq!(view.edit_error(), Some(EDIT_CONFLICT));
        assert_eq!(
            view.issue().map(|issue| issue.title.as_str()),
            Some("Theirs")
        );
    });
    assert_eq!(text(&title, cx), "Mine", "the user's text is kept");

    view.update(cx, |view, cx| view.save_edit(cx));
    cx.run_until_parked();
    assert_eq!(
        fake.writes(),
        vec![FakeWrite::UpdateIssue(
            repository(),
            2,
            IssueUpdate {
                title: Some("Mine".to_string()),
                body: Some(String::new()),
                ..IssueUpdate::default()
            }
        )],
        "saving again, knowingly, goes through"
    );
    view.read_with(cx, |view, _| {
        assert!(!view.is_editing());
        assert_eq!(view.issue().map(|issue| issue.title.as_str()), Some("Mine"));
    });
}

#[gpui::test]
async fn closing_sends_the_reason_and_reopening_opens(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let view = open_detail(&workspace, fake.clone(), 2, cx);
    view.update_in(cx, |view, window, cx| {
        view.close(StateReason::NotPlanned, window, cx)
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.issue().map(|issue| issue.state),
            Some(IssueState::Closed)
        );
    });
    view.update_in(cx, |view, window, cx| view.reopen(window, cx));
    cx.run_until_parked();
    assert_eq!(
        fake.writes(),
        vec![
            FakeWrite::UpdateIssue(
                repository(),
                2,
                IssueUpdate {
                    state: Some(IssueState::Closed),
                    state_reason: Some(StateReason::NotPlanned),
                    ..IssueUpdate::default()
                }
            ),
            FakeWrite::UpdateIssue(
                repository(),
                2,
                IssueUpdate {
                    state: Some(IssueState::Open),
                    state_reason: Some(StateReason::Reopened),
                    ..IssueUpdate::default()
                }
            ),
        ]
    );
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

#[gpui::test]
async fn a_failed_comment_keeps_its_text(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let view = open_detail(&workspace, fake.clone(), 2, cx);
    let editor = view.update_in(cx, |view, window, cx| view.comment_editor(window, cx));
    set_text(&editor, "Thanks!", cx);
    fake.set_failure(Some(GithubError::Rejected {
        message: "Body is too long".to_string(),
    }));
    view.update_in(cx, |view, window, cx| view.send_comment(window, cx));
    cx.run_until_parked();
    let error = view.read_with(cx, |view, _| view.comment_error().map(str::to_string));
    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("Body is too long")),
        "{error:?}"
    );
    assert_eq!(text(&editor, cx), "Thanks!");

    fake.set_failure(None);
    let composer = view
        .update_in(cx, |view, window, cx| view.comment_composer(window, cx))
        .expect("the comment box has its tabs");
    composer.update(cx, |composer, cx| composer.show_preview(cx));
    view.update_in(cx, |view, window, cx| view.send_comment(window, cx));
    cx.run_until_parked();
    assert!(
        !composer.read_with(cx, |composer, _| composer.is_previewing()),
        "a sent comment leaves the box on Write, ready for the next"
    );
    assert_eq!(
        fake.writes(),
        vec![FakeWrite::AddComment(
            repository(),
            2,
            "Thanks!".to_string()
        )]
    );
    assert_eq!(text(&editor, cx), "");
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.comments().last().map(|comment| comment.body.as_str()),
            Some("Thanks!")
        );
        assert_eq!(view.comment_error(), None);
    });
}

#[gpui::test]
async fn the_secondary_enter_sends_the_comment(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    // The bindings of the default keymaps this relies on.
    cx.update(|_, cx| {
        cx.bind_keys([
            KeyBinding::new("cmd-enter", menu::SecondaryConfirm, None),
            KeyBinding::new(
                "ctrl-enter",
                editor::actions::Newline,
                Some("Editor && mode == auto_height"),
            ),
        ])
    });
    let fake = Arc::new(fake());
    let view = open_detail(&workspace, fake.clone(), 2, cx);
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let editor = view.update_in(cx, |view, window, cx| view.comment_editor(window, cx));
    editor.update_in(cx, |editor, window, cx| {
        window.focus(&editor.focus_handle(cx), cx)
    });
    cx.simulate_input("Looks good");
    let keystroke = if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        // A real keyboard reports Ctrl held before the Enter arrives; the
        // handler reads that state, and a simulated keystroke alone does not
        // set it.
        cx.simulate_modifiers_change(gpui::Modifiers::control());
        "ctrl-enter"
    };
    cx.simulate_keystrokes(keystroke);
    cx.run_until_parked();
    assert_eq!(
        fake.writes(),
        vec![FakeWrite::AddComment(
            repository(),
            2,
            "Looks good".to_string()
        )]
    );
}

#[gpui::test]
async fn the_row_menu_follows_the_role_and_closes_with_the_reason(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let fake =
        Arc::new(fake().with_repository_permission(&repository(), RepositoryPermission::Read));
    let list = list_over(fake.clone(), cx);
    let (theirs, mine) = list.read_with(cx, |list, _| (list.row_menu(1), list.row_menu(2)));
    assert!(!theirs.contains(&IssueMenuEntry::Close(StateReason::Completed)));
    assert!(mine.contains(&IssueMenuEntry::Close(StateReason::Duplicate)));

    list.update_in(cx, |list, window, cx| {
        list.run_menu_entry(2, IssueMenuEntry::Close(StateReason::Duplicate), window, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        fake.writes(),
        vec![FakeWrite::UpdateIssue(
            repository(),
            2,
            IssueUpdate {
                state: Some(IssueState::Closed),
                state_reason: Some(StateReason::Duplicate),
                ..IssueUpdate::default()
            }
        )]
    );
    let visible = list.read_with(cx, |list, _| {
        list.visible_rows()
            .iter()
            .map(|issue| issue.number)
            .collect::<Vec<_>>()
    });
    assert_eq!(visible, vec![1, 3], "the closed issue leaves the open list");
    list.update_in(cx, |list, window, cx| {
        list.deploy_row_menu(1, gpui::point(gpui::px(10.), gpui::px(10.)), window, cx)
    });
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

#[gpui::test]
async fn the_create_button_waits_for_a_title(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let list = list_over(fake, cx);
    let dialog = open_create_dialog(&workspace, &list, cx);
    let title = dialog.read_with(cx, |dialog, _| dialog.title.clone());
    let can_create =
        |cx: &mut VisualTestContext| dialog.read_with(cx, |dialog, cx| dialog.can_create(cx));

    assert!(!can_create(cx), "no title yet");
    set_text(&title, "   ", cx);
    assert!(!can_create(cx), "blank is no title");
    set_text(&title, "A real title", cx);
    assert!(can_create(cx));
}

#[gpui::test]
async fn assignees_picked_in_the_dialog_go_with_the_new_issue(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(
        fake()
            .with_assignable_user(&repository(), picker_user("hubot", None))
            .with_assignable_user(&repository(), picker_user("monalisa", Some("Mona Lisa"))),
    );
    let list = list_over(fake.clone(), cx);
    let dialog = open_create_dialog(&workspace, &list, cx);
    let (title, picker) = dialog.read_with(cx, |dialog, _| {
        (dialog.title.clone(), dialog.assignee_picker().clone())
    });
    set_text(&title, "Needs owners", cx);
    for login in ["hubot", "monalisa", "hubot"] {
        let user = if login == "hubot" {
            picker_user("hubot", None)
        } else {
            picker_user("monalisa", Some("Mona Lisa"))
        };
        picker.update_in(cx, |picker, window, cx| picker.pick(user, window, cx));
        cx.run_until_parked();
    }
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.chosen_assignees().to_vec()),
        vec![picker_user("monalisa", Some("Mona Lisa"))],
        "picking someone already chosen takes them off"
    );
    assert_eq!(
        picker.read_with(cx, |picker, _| picker.selected.clone()),
        vec!["monalisa".to_string()]
    );

    dialog.update(cx, |dialog, cx| dialog.submit(cx));
    cx.run_until_parked();
    assert_eq!(
        fake.writes(),
        vec![FakeWrite::CreateIssue(
            repository(),
            NewIssue {
                title: "Needs owners".to_string(),
                assignees: vec!["monalisa".to_string()],
                ..NewIssue::default()
            }
        )]
    );
}

#[gpui::test]
async fn a_type_chosen_in_the_dialog_is_set_on_the_new_issue(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let bug_type = github::IssueType {
        id: "IT_bug".to_string(),
        name: "Bug".to_string(),
        description: None,
        color: Some("RED".to_string()),
        is_enabled: true,
    };
    let fake = Arc::new(fake().with_issue_type(&repository(), bug_type));
    let list = list_over(fake.clone(), cx);
    let dialog = open_create_dialog(&workspace, &list, cx);
    let title = dialog.read_with(cx, |dialog, _| dialog.title.clone());
    set_text(&title, "Typed", cx);
    dialog.update(cx, |dialog, cx| {
        dialog.set_issue_type(Some("IT_bug".to_string()), cx);
        dialog.submit(cx);
    });
    cx.run_until_parked();

    assert_eq!(
        fake.writes(),
        vec![
            FakeWrite::CreateIssue(
                repository(),
                NewIssue {
                    title: "Typed".to_string(),
                    ..NewIssue::default()
                }
            ),
            FakeWrite::SetIssueType("I_4".to_string(), Some("IT_bug".to_string())),
        ]
    );
    let created_type = list.read_with(cx, |list, _| {
        list.rows()
            .iter()
            .find(|issue| issue.number == 4)
            .and_then(|issue| issue.issue_type.clone())
    });
    assert_eq!(created_type.as_deref(), Some("Bug"));
}

#[gpui::test]
async fn picking_a_person_in_the_tab_assigns_and_picking_again_unassigns(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake().with_assignable_user(&repository(), picker_user("hubot", None)));
    let view = open_detail(&workspace, fake.clone(), 2, cx);
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let picker = view
        .read_with(cx, |view, _| {
            view.assignee_picker
                .as_ref()
                .map(|(picker, _)| picker.clone())
        })
        .expect("the tab shows the assignee search");

    picker.update_in(cx, |picker, window, cx| {
        picker.pick(picker_user("hubot", None), window, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view
            .issue()
            .map(|issue| issue.assignees.clone())),
        Some(vec!["hubot".to_string()])
    );
    picker.update_in(cx, |picker, window, cx| {
        picker.pick(picker_user("hubot", None), window, cx)
    });
    cx.run_until_parked();

    assert_eq!(
        fake.writes(),
        vec![
            FakeWrite::AddAssignees(repository(), 2, vec!["hubot".to_string()]),
            FakeWrite::RemoveAssignees(repository(), 2, vec!["hubot".to_string()]),
        ],
        "the add and remove endpoints, never a whole-set update"
    );
    assert_eq!(
        view.read_with(cx, |view, _| view
            .issue()
            .map(|issue| issue.assignees.clone())),
        Some(Vec::new())
    );
}

/// Text typed while the comment is on its way is not the sent comment, and
/// is not thrown away with it.
#[gpui::test]
async fn text_typed_while_a_comment_is_sent_is_kept(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let view = open_detail(&workspace, fake.clone(), 2, cx);
    let editor = view.update_in(cx, |view, window, cx| view.comment_editor(window, cx));
    set_text(&editor, "First", cx);
    view.update_in(cx, |view, window, cx| view.send_comment(window, cx));
    set_text(&editor, "First, and then more", cx);
    cx.run_until_parked();

    assert_eq!(
        fake.writes(),
        vec![FakeWrite::AddComment(repository(), 2, "First".to_string())]
    );
    assert_eq!(text(&editor, cx), "First, and then more");
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.comments().last().map(|comment| comment.body.as_str()),
            Some("First")
        );
    });
}

/// A write whose outcome is not known says to check before trying again,
/// keeps what was typed, and sends nothing a second time by itself.
#[gpui::test]
async fn an_unknown_outcome_tells_the_user_to_check_before_retrying(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(fake());
    let list = list_over(fake.clone(), cx);
    let unknown = GithubError::OutcomeUnknown {
        message: "connection closed".to_string(),
    };

    let dialog = open_create_dialog(&workspace, &list, cx);
    let title = dialog.read_with(cx, |dialog, _| dialog.title.clone());
    set_text(&title, "Maybe created", cx);
    fake.set_failure(Some(unknown.clone()));
    dialog.update(cx, |dialog, cx| dialog.submit(cx));
    cx.run_until_parked();
    let error = dialog.read_with(cx, |dialog, _| dialog.error().map(str::to_string));
    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("Reload and check before trying again")),
        "{error:?}"
    );
    assert_eq!(text(&title, cx), "Maybe created");

    fake.set_failure(None);
    let view = open_detail(&workspace, fake.clone(), 2, cx);
    let editor = view.update_in(cx, |view, window, cx| view.comment_editor(window, cx));
    set_text(&editor, "Maybe sent", cx);
    fake.set_failure(Some(unknown));
    view.update_in(cx, |view, window, cx| view.send_comment(window, cx));
    cx.run_until_parked();
    let error = view.read_with(cx, |view, _| view.comment_error().map(str::to_string));
    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("Reload and check before trying again")),
        "{error:?}"
    );
    assert_eq!(text(&editor, cx), "Maybe sent");
    assert!(fake.writes().is_empty(), "nothing was sent again");
}
