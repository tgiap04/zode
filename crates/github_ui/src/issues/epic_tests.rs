use std::sync::Arc;

use editor::Editor;
use github::{
    FakeGithubBackend, FakeWrite, GithubBackend, GithubError, Issue, IssueState, IssueType,
    NewIssue, RepositoryPermission, SubIssuePosition, SubIssuesSummary, fake_issue,
};
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use workspace::Workspace;

use crate::github_view::DetailKey;
use crate::github_view_session::test_session;
use crate::issues::epic_tree::{ChildrenLoad, progress_label};
use crate::issues::issue_editor::CreateIssueDialog;
use crate::issues::issue_type_picker::type_choices;
use crate::issues::issues_test_support::{context, init, repository, workspace_window};
use crate::issues::sub_issue_section::{
    MoveDirection, PendingRemoval, PickerPurpose, move_position, sub_issue_write_error,
};
use crate::issues::{IssueDetailView, IssueList, open_issue_detail};

fn issue_type(id: &str, name: &str, is_enabled: bool) -> IssueType {
    IssueType {
        id: id.to_string(),
        name: name.to_string(),
        description: None,
        color: Some("RED".to_string()),
        is_enabled,
    }
}

fn closed(number: u64, title: &str) -> Issue {
    let mut issue = fake_issue(number, title);
    issue.state = IssueState::Closed;
    issue
}

/// #1 is an epic over #2 (open) and #3 (closed); #4 and #5 stand alone.
fn epic_fake() -> FakeGithubBackend {
    FakeGithubBackend::new()
        .with_issue(&repository(), fake_issue(1, "Epic"))
        .with_issue(&repository(), fake_issue(2, "Second"))
        .with_issue(&repository(), closed(3, "Third"))
        .with_issue(&repository(), fake_issue(4, "Fourth"))
        .with_issue(&repository(), fake_issue(5, "Fifth"))
        .with_sub_issue(&repository(), 1, fake_issue(2, "Second"))
        .with_sub_issue(&repository(), 1, closed(3, "Third"))
}

fn key(number: u64) -> DetailKey {
    DetailKey {
        repository: repository(),
        number,
    }
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

fn open_detail(
    workspace: &Entity<Workspace>,
    backend: Arc<dyn GithubBackend>,
    number: u64,
    cx: &mut VisualTestContext,
) -> Entity<IssueDetailView> {
    let view = workspace.update_in(cx, |workspace, window, cx| {
        open_issue_detail(workspace, backend, test_session(), key(number), window, cx)
    });
    cx.run_until_parked();
    view
}

fn numbers(issues: &[Issue]) -> Vec<u64> {
    issues.iter().map(|issue| issue.number).collect()
}

fn sub_issue_numbers(view: &Entity<IssueDetailView>, cx: &mut VisualTestContext) -> Vec<u64> {
    view.read_with(cx, |view, _| numbers(view.sub_issues()))
}

fn epic_error(view: &Entity<IssueDetailView>, cx: &mut VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, _| view.epic_error().map(str::to_string))
}

#[gpui::test]
async fn a_parent_row_shows_its_progress_and_opens_its_sub_issues_on_demand(
    cx: &mut TestAppContext,
) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let list = list_over(Arc::new(epic_fake()), cx);
    let rows = |cx: &mut VisualTestContext| {
        list.read_with(cx, |list, _| {
            list.tree_rows()
                .into_iter()
                .map(|row| (row.depth, row.issue.number))
                .collect::<Vec<_>>()
        })
    };
    let epic = list.read_with(cx, |list, _| list.rows()[0].clone());
    assert_eq!(
        epic.sub_issues_summary,
        SubIssuesSummary {
            total: 2,
            completed: 1
        }
    );
    assert_eq!(
        progress_label(epic.sub_issues_summary).as_deref(),
        Some("1/2 done")
    );
    assert_eq!(progress_label(SubIssuesSummary::default()), None);
    assert_eq!(rows(cx), vec![(0, 1), (0, 2), (0, 4), (0, 5)]);
    assert_eq!(
        list.read_with(cx, |list, _| list.sub_issues_of(&key(1)).cloned()),
        None,
        "nothing is read before the row is opened"
    );

    list.update(cx, |list, cx| list.toggle_sub_issues(key(1), cx));
    cx.run_until_parked();
    assert!(matches!(
        list.read_with(cx, |list, _| list.sub_issues_of(&key(1)).cloned()),
        Some(ChildrenLoad::Loaded(children)) if numbers(&children) == vec![2, 3]
    ));
    assert_eq!(
        rows(cx),
        vec![(0, 1), (1, 2), (1, 3), (0, 2), (0, 4), (0, 5)]
    );

    list.update(cx, |list, cx| list.toggle_sub_issues(key(1), cx));
    assert_eq!(rows(cx), vec![(0, 1), (0, 2), (0, 4), (0, 5)]);
}

#[gpui::test]
async fn the_type_filter_narrows_the_list_and_only_shows_when_there_are_types(
    cx: &mut TestAppContext,
) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let mut bug = fake_issue(2, "Crash");
    bug.issue_type = Some("Bug".to_string());
    let typed = FakeGithubBackend::new()
        .with_issue(&repository(), fake_issue(1, "Plain"))
        .with_issue(&repository(), bug)
        .with_issue_type(&repository(), issue_type("IT_bug", "Bug", true));
    let list = list_over(Arc::new(typed), cx);
    assert!(list.read_with(cx, |list, _| list.offers_type_filter()));

    list.update(cx, |list, cx| {
        list.set_issue_type_filter(Some("Bug".to_string()), cx)
    });
    cx.run_until_parked();
    assert_eq!(list.read_with(cx, |list, _| numbers(list.rows())), vec![2]);
    assert_eq!(
        list.read_with(cx, |list, _| list.filters().issue_type.clone()),
        Some("Bug".to_string())
    );

    let untyped = list_over(Arc::new(epic_fake()), cx);
    assert!(!untyped.read_with(cx, |list, _| list.offers_type_filter()));
}

#[gpui::test]
async fn the_tab_lists_sub_issues_with_progress_and_names_the_parent(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(epic_fake().with_sub_issue(&repository(), 5, fake_issue(1, "Epic")));
    let epic = open_detail(&workspace, fake.clone(), 1, cx);
    assert_eq!(sub_issue_numbers(&epic, cx), vec![2, 3]);
    epic.read_with(cx, |view, _| {
        assert_eq!(
            view.issue().map(|issue| issue.sub_issues_summary),
            Some(SubIssuesSummary {
                total: 2,
                completed: 1
            })
        );
        assert_eq!(view.parent_issue().map(|parent| parent.number), Some(5));
    });
    let child = open_detail(&workspace, fake, 2, cx);
    child.read_with(cx, |view, _| {
        assert_eq!(view.parent_issue().map(|parent| parent.number), Some(1));
        assert!(view.sub_issues().is_empty());
    });
}

#[gpui::test]
async fn a_hundred_and_first_sub_issue_is_refused_in_words_without_a_write(
    cx: &mut TestAppContext,
) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(
        (2..=101).fold(
            FakeGithubBackend::new()
                .with_issue(&repository(), fake_issue(1, "Full"))
                .with_issue(&repository(), fake_issue(200, "One too many")),
            |fake, number| fake.with_sub_issue(&repository(), 1, fake_issue(number, "Child")),
        ),
    );
    let view = open_detail(&workspace, fake.clone(), 1, cx);
    assert_eq!(sub_issue_numbers(&view, cx).len(), 100);

    view.update(cx, |view, cx| {
        view.add_sub_issue(fake_issue(200, "One too many"), cx)
    });
    cx.run_until_parked();
    let error = epic_error(&view, cx).expect("a readable error");
    assert!(error.contains("100 sub-issues"), "{error}");
    assert_eq!(fake.writes(), Vec::new());

    view.update_in(cx, |view, window, cx| view.create_sub_issue(window, cx));
    cx.run_until_parked();
    assert!(
        workspace
            .read_with(cx, |workspace, cx| workspace
                .active_modal::<CreateIssueDialog>(cx))
            .is_none(),
        "no dialog opens for an issue that cannot take another sub-issue"
    );
}

#[test]
fn githubs_limit_errors_are_put_in_words() {
    let full = GithubError::Rejected {
        message: "Parent cannot have more than 100 sub-issues".to_string(),
    };
    assert!(sub_issue_write_error(&full).contains("100 sub-issues"));
    let deep = GithubError::Rejected {
        message: "Sub-issue exceeds the maximum depth of nested sub-issues".to_string(),
    };
    assert!(sub_issue_write_error(&deep).contains("8 levels"));
    assert_eq!(
        sub_issue_write_error(&GithubError::NotFound),
        GithubError::NotFound.to_string()
    );
}

#[gpui::test]
async fn without_issue_types_no_type_controls_show(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let untyped = open_detail(&workspace, Arc::new(epic_fake()), 1, cx);
    assert!(!untyped.read_with(cx, |view, _| view.shows_type_picker()));

    let typed = FakeGithubBackend::new()
        .with_issue(&repository(), fake_issue(9, "Typed"))
        .with_issue_type(&repository(), issue_type("IT_bug", "Bug", true));
    let typed = open_detail(&workspace, Arc::new(typed), 9, cx);
    assert!(typed.read_with(cx, |view, _| view.shows_type_picker()));
}

#[test]
fn a_disabled_type_is_not_offered() {
    let types = [
        issue_type("IT_bug", "Bug", true),
        issue_type("IT_old", "Old", false),
    ];
    assert_eq!(
        type_choices(&types)
            .into_iter()
            .map(|issue_type| issue_type.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Bug"]
    );
}

#[gpui::test]
async fn changing_the_type_sends_update_issue_issue_type(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let bug = issue_type("IT_bug", "Bug", true);
    let fake = Arc::new(
        FakeGithubBackend::new()
            .with_issue(&repository(), fake_issue(2, "Crash"))
            .with_issue_type(&repository(), bug.clone()),
    );
    let view = open_detail(&workspace, fake.clone(), 2, cx);
    view.update_in(cx, |view, window, cx| {
        view.set_issue_type(Some(bug), window, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view
            .issue()
            .and_then(|issue| issue.issue_type.clone())),
        Some("Bug".to_string())
    );
    view.update_in(cx, |view, window, cx| view.set_issue_type(None, window, cx));
    cx.run_until_parked();
    assert_eq!(
        fake.writes(),
        vec![
            FakeWrite::SetIssueType("I_2".to_string(), Some("IT_bug".to_string())),
            FakeWrite::SetIssueType("I_2".to_string(), None),
        ]
    );
}

#[test]
fn a_move_names_the_sibling_it_lands_after_or_before() {
    let children = [fake_issue(2, "a"), fake_issue(3, "b"), fake_issue(4, "c")];
    let after = |id: &str| Some(SubIssuePosition::After(id.to_string()));
    assert_eq!(move_position(&children, 2, MoveDirection::Up), after("I_2"));
    assert_eq!(
        move_position(&children, 1, MoveDirection::Up),
        Some(SubIssuePosition::Before("I_2".to_string())),
        "the new first has nothing to go after"
    );
    assert_eq!(move_position(&children, 0, MoveDirection::Up), None);
    assert_eq!(
        move_position(&children, 0, MoveDirection::Down),
        after("I_3")
    );
    assert_eq!(
        move_position(&children, 1, MoveDirection::Down),
        after("I_4")
    );
    assert_eq!(move_position(&children, 2, MoveDirection::Down), None);
    assert_eq!(move_position(&children, 7, MoveDirection::Down), None);
}

#[gpui::test]
async fn moving_a_sub_issue_reprioritizes_it_after_its_new_neighbour(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(epic_fake().with_sub_issue(&repository(), 1, fake_issue(4, "Fourth")));
    let view = open_detail(&workspace, fake.clone(), 1, cx);
    assert_eq!(sub_issue_numbers(&view, cx), vec![2, 3, 4]);

    view.update(cx, |view, cx| {
        view.move_sub_issue("I_4", MoveDirection::Up, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        fake.writes(),
        vec![FakeWrite::ReprioritizeSubIssue(
            "I_1".to_string(),
            "I_4".to_string(),
            SubIssuePosition::After("I_2".to_string())
        )]
    );
    assert_eq!(sub_issue_numbers(&view, cx), vec![2, 4, 3]);

    view.update(cx, |view, cx| {
        view.move_sub_issue("I_2", MoveDirection::Up, cx)
    });
    cx.run_until_parked();
    assert_eq!(fake.writes().len(), 1, "the first cannot go higher");
}

#[gpui::test]
async fn removing_a_sub_issue_waits_for_a_confirmation(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(epic_fake());
    let view = open_detail(&workspace, fake.clone(), 1, cx);

    view.update(cx, |view, cx| view.request_remove_sub_issue("I_3", cx));
    cx.run_until_parked();
    assert_eq!(fake.writes(), Vec::new(), "asking is not removing");
    assert!(matches!(
        view.read_with(cx, |view, _| view.pending_removal().cloned()),
        Some(PendingRemoval::SubIssue(issue)) if issue.number == 3
    ));
    view.update(cx, |view, cx| view.cancel_removal(cx));
    assert_eq!(
        view.read_with(cx, |view, _| view.pending_removal().cloned()),
        None
    );

    view.update(cx, |view, cx| {
        view.request_remove_sub_issue("I_3", cx);
        view.confirm_removal(cx);
    });
    cx.run_until_parked();
    assert_eq!(
        fake.writes(),
        vec![FakeWrite::RemoveSubIssue(repository(), 1, 1003)]
    );
    assert_eq!(sub_issue_numbers(&view, cx), vec![2]);
}

#[gpui::test]
async fn below_triage_the_epic_and_type_controls_are_hidden_and_inert(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(
        epic_fake()
            .with_issue_type(&repository(), issue_type("IT_bug", "Bug", true))
            .with_repository_permission(&repository(), RepositoryPermission::Read),
    );
    let view = open_detail(&workspace, fake.clone(), 1, cx);
    view.read_with(cx, |view, _| {
        assert!(!view.may_edit_epic());
        assert!(!view.shows_type_picker());
        assert_eq!(
            numbers(view.sub_issues()),
            vec![2, 3],
            "reading still works"
        );
    });
    view.update(cx, |view, cx| {
        view.request_remove_sub_issue("I_3", cx);
        view.move_sub_issue("I_3", MoveDirection::Up, cx);
        view.add_sub_issue(fake_issue(4, "Fourth"), cx);
    });
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.pending_removal().cloned()),
        None
    );
    assert_eq!(fake.writes(), Vec::new());

    let writer = Arc::new(epic_fake());
    let view = open_detail(&workspace, writer, 4, cx);
    assert!(view.read_with(cx, |view, _| view.may_edit_epic()));
}

#[gpui::test]
async fn a_new_sub_issue_is_created_with_the_dialog_then_added_under_the_parent(
    cx: &mut TestAppContext,
) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(epic_fake());
    let view = open_detail(&workspace, fake.clone(), 1, cx);
    view.update_in(cx, |view, window, cx| view.create_sub_issue(window, cx));
    cx.run_until_parked();
    let dialog = workspace
        .read_with(cx, |workspace, cx| {
            workspace.active_modal::<CreateIssueDialog>(cx)
        })
        .expect("the New issue dialog is open");
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.target_line()),
        "@octocat → octo/repo (public)"
    );
    let title: Entity<Editor> = dialog.read_with(cx, |dialog, _| dialog.title.clone());
    title.update_in(cx, |editor, window, cx| {
        editor.set_text("A step", window, cx)
    });
    dialog.update(cx, |dialog, cx| dialog.submit(cx));
    cx.run_until_parked();

    assert_eq!(
        fake.writes(),
        vec![
            FakeWrite::CreateIssue(
                repository(),
                NewIssue {
                    title: "A step".to_string(),
                    ..NewIssue::default()
                }
            ),
            FakeWrite::AddSubIssue(repository(), 1, 1006),
        ]
    );
    assert_eq!(sub_issue_numbers(&view, cx), vec![2, 3, 6]);
}

#[gpui::test]
async fn an_existing_issue_is_found_by_number_or_words_and_added(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(epic_fake());
    let view = open_detail(&workspace, fake.clone(), 1, cx);
    view.update(cx, |view, cx| {
        view.open_picker(PickerPurpose::AddSubIssue, cx);
        view.search_picker("#4".to_string(), cx);
    });
    cx.run_until_parked();
    let found = view.read_with(cx, |view, _| view.picker_results().to_vec());
    assert_eq!(numbers(&found), vec![4]);

    view.update(cx, |view, cx| view.search_picker("#1".to_string(), cx));
    cx.run_until_parked();
    assert!(
        view.read_with(cx, |view, _| view.picker_results().is_empty()),
        "an issue is not its own sub-issue"
    );
    view.update(cx, |view, cx| view.search_picker("Fifth".to_string(), cx));
    cx.run_until_parked();
    assert_eq!(fake.searches(), vec!["Fifth".to_string()]);
    assert_eq!(
        view.read_with(cx, |view, _| numbers(view.picker_results())),
        vec![5]
    );

    view.update(cx, |view, cx| view.add_sub_issue(found[0].clone(), cx));
    cx.run_until_parked();
    assert_eq!(
        fake.writes(),
        vec![FakeWrite::AddSubIssue(repository(), 1, 1004)]
    );
    assert_eq!(sub_issue_numbers(&view, cx), vec![2, 3, 4]);
}

#[gpui::test]
async fn the_parent_is_changed_and_cleared_after_a_confirmation(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(epic_fake());
    let view = open_detail(&workspace, fake.clone(), 2, cx);
    view.update(cx, |view, cx| view.set_parent(fake_issue(5, "Fifth"), cx));
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view
            .parent_issue()
            .map(|parent| parent.number)),
        Some(5)
    );

    view.update(cx, |view, cx| view.request_clear_parent(cx));
    cx.run_until_parked();
    assert_eq!(fake.writes().len(), 1, "clearing waits for a confirmation");
    view.update(cx, |view, cx| view.confirm_removal(cx));
    cx.run_until_parked();
    assert_eq!(
        fake.writes(),
        vec![
            FakeWrite::ReplaceParent(repository(), 5, 1002),
            FakeWrite::RemoveSubIssue(repository(), 5, 1002),
        ]
    );
    assert_eq!(
        view.read_with(cx, |view, _| view.parent_issue().cloned()),
        None
    );
}

#[gpui::test]
async fn a_sub_issue_is_found_by_node_id_not_by_its_number(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let other = github::RepositoryId::new("octo", "other").expect("valid");
    let mut elsewhere = fake_issue(3, "Same number, other repository");
    elsewhere.id = 9003;
    elsewhere.node_id = "I_other_3".to_string();
    elsewhere.html_url = "https://github.com/octo/other/issues/3".to_string();
    let fake = Arc::new(
        epic_fake()
            .with_issue(&other, elsewhere.clone())
            .with_sub_issue(&repository(), 1, elsewhere),
    );
    let view = open_detail(&workspace, fake.clone(), 1, cx);

    view.update(cx, |view, cx| {
        view.request_remove_sub_issue("I_other_3", cx);
        view.confirm_removal(cx);
    });
    cx.run_until_parked();
    assert_eq!(
        fake.writes(),
        vec![FakeWrite::RemoveSubIssue(repository(), 1, 9003)],
        "the other repository's #3 is the one removed"
    );
}
