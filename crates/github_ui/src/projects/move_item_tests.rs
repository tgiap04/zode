use std::sync::Arc;

use github::{
    FakeGithubBackend, FakeWrite, GithubError, OAuthScopes, ProjectFieldInput, ProjectSort,
    ProjectView, ProjectViewLayout,
};
use gpui::{Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext, px};

use crate::issues::issues_test_support::repository;
use crate::projects::projects_section_tests::{
    PROJECT, backend, details, issue, item, project, projects_tab, section,
};
use crate::projects::{ColumnKey, ProjectsSection, board_columns};

fn set(item_id: &str, option: Option<&str>) -> FakeWrite {
    FakeWrite::SetProjectItemField(
        PROJECT.into(),
        item_id.into(),
        "F_status".into(),
        option.map(|option| ProjectFieldInput::SingleSelect {
            option_id: option.into(),
        }),
    )
}

fn moved(item_id: &str, after: Option<&str>) -> FakeWrite {
    FakeWrite::MoveProjectItem(PROJECT.into(), item_id.into(), after.map(Into::into))
}

/// `project` reads and writes.
fn writable() -> OAuthScopes {
    OAuthScopes::Granted(vec!["repo".into(), "project".into()])
}

fn todo() -> ColumnKey {
    ColumnKey::Option("todo".into())
}

fn done() -> ColumnKey {
    ColumnKey::Option("done".into())
}

/// The board's columns as item ids, as drawn.
fn columns(
    section: &Entity<ProjectsSection>,
    cx: &mut VisualTestContext,
) -> Vec<(ColumnKey, Vec<String>)> {
    section.read_with(cx, |section, _| {
        let details = section.details().expect("the project's details");
        board_columns(section.view(), details, section.items())
            .into_iter()
            .map(|column| {
                let ids = column
                    .items
                    .iter()
                    .map(|index| section.items()[*index].id.clone())
                    .collect();
                (column.key, ids)
            })
            .collect()
    })
}

fn column_of(
    section: &Entity<ProjectsSection>,
    item_id: &str,
    cx: &mut VisualTestContext,
) -> ColumnKey {
    columns(section, cx)
        .into_iter()
        .find(|(_, ids)| ids.iter().any(|id| id == item_id))
        .map(|(key, _)| key)
        .expect("the item is on the board")
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

fn card(item_id: &str, cx: &mut VisualTestContext) -> gpui::Bounds<Pixels> {
    // `debug_bounds` takes a `&'static str`; a leaked test string is fine.
    cx.debug_bounds(format!("github-project-card-{item_id}").leak())
        .unwrap_or_else(|| panic!("card {item_id} is drawn"))
}

/// Press, a path of moves with the button held, release.
fn drag(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) {
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    for step in 1..=4 {
        let fraction = step as f32 / 4.0;
        cx.simulate_mouse_move(
            Point {
                x: from.x + (to.x - from.x) * fraction,
                y: from.y + (to.y - from.y) * fraction,
            },
            Some(MouseButton::Left),
            Modifiers::none(),
        );
        cx.run_until_parked();
    }
    assert!(cx.update(|_, cx| cx.has_active_drag()), "the drag is armed");
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    redraw(cx);
}

fn right_click(item_id: &str, cx: &mut VisualTestContext) {
    let center = card(item_id, cx).center();
    cx.simulate_mouse_down(center, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(center, MouseButton::Right, Modifiers::none());
    redraw(cx);
}

fn pick_menu_entry(action: impl gpui::Action, cx: &mut VisualTestContext) {
    cx.dispatch_action(action);
    cx.dispatch_action(menu::Confirm);
    redraw(cx);
}

#[gpui::test]
async fn move_to_and_clear_from_the_card_menu_send_the_fields_option(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    let (_workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);

    right_click("PVTI_7", cx);
    assert_eq!(
        section.read_with(cx, |section, _| section
            .card_menu_item()
            .map(str::to_string)),
        Some("PVTI_7".to_string())
    );
    // Todo's card offers "Move to Done", then "Clear Status".
    pick_menu_entry(menu::SelectFirst, cx);
    assert_eq!(backend.writes(), vec![set("PVTI_7", Some("done"))]);
    assert_eq!(column_of(&section, "PVTI_7", cx), done());

    right_click("PVTI_7", cx);
    // The last entry is "Remove from project…"; "Clear Status" is above it.
    cx.dispatch_action(menu::SelectLast);
    pick_menu_entry(menu::SelectPrevious, cx);
    assert_eq!(
        backend.writes(),
        vec![set("PVTI_7", Some("done")), set("PVTI_7", None)]
    );
    assert_eq!(column_of(&section, "PVTI_7", cx), ColumnKey::NoValue);
    assert_eq!(
        backend.writes().len(),
        2,
        "a move within the same column field sends no position"
    );
}

#[gpui::test]
async fn a_failed_move_puts_the_card_back_and_says_why(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    let (_workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);
    backend.fail_next_project_write(GithubError::Forbidden {
        message: "Resource not accessible".into(),
    });
    section.update(cx, |section, cx| section.move_item("PVTI_7", &done(), cx));
    redraw(cx);
    assert_eq!(backend.writes(), vec![set("PVTI_7", Some("done"))]);
    assert_eq!(
        column_of(&section, "PVTI_7", cx),
        todo(),
        "back where it was"
    );
    let message = section.read_with(cx, |section, _| section.move_message().map(str::to_string));
    let message = message.expect("the failure is told");
    assert!(message.contains("Todo"), "{message}");
    assert!(message.contains("Resource not accessible"), "{message}");
    assert!(cx.debug_bounds("github-project-move-message").is_some());
}

#[gpui::test]
async fn a_failed_move_does_not_undo_a_later_one_that_went_through(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    let (_workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);
    backend.fail_next_project_write(GithubError::NotFound);
    section.update(cx, |section, cx| {
        section.move_item("PVTI_7", &done(), cx);
        section.move_item("PVTI_7", &ColumnKey::NoValue, cx);
        section.move_item("PVTI_9", &todo(), cx);
    });
    redraw(cx);
    assert_eq!(
        backend.writes(),
        vec![
            set("PVTI_7", Some("done")),
            set("PVTI_7", None),
            set("PVTI_9", Some("todo")),
        ],
        "quick moves are sent in the order they were made"
    );
    assert_eq!(column_of(&section, "PVTI_7", cx), ColumnKey::NoValue);
    assert_eq!(column_of(&section, "PVTI_9", cx), todo());
}

#[gpui::test]
async fn a_missing_scope_on_write_puts_the_card_back_and_guides(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    let (_workspace, view, cx) = projects_tab(backend.clone(), OAuthScopes::Unknown, cx).await;
    let section = section(&view, cx);
    backend.fail_next_project_write(GithubError::MissingScope {
        scope: "project".into(),
    });
    section.update(cx, |section, cx| section.move_item("PVTI_7", &done(), cx));
    redraw(cx);
    assert_eq!(column_of(&section, "PVTI_7", cx), todo());
    assert!(section.read_with(cx, |section, _| section.write_scope_missing()));
    assert!(cx.debug_bounds("github-projects-write-guidance").is_some());
}

#[gpui::test]
async fn with_read_project_only_a_move_guides_and_sends_nothing(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    let scopes = OAuthScopes::Granted(vec!["repo".into(), "read:project".into()]);
    let (_workspace, view, cx) = projects_tab(backend.clone(), scopes, cx).await;
    let section = section(&view, cx);

    right_click("PVTI_7", cx);
    assert_eq!(
        section.read_with(cx, |section, _| section
            .card_menu_item()
            .map(str::to_string)),
        None,
        "no menu of moves that cannot be sent"
    );
    assert!(section.read_with(cx, |section, _| section.write_scope_missing()));
    assert!(cx.debug_bounds("github-projects-write-guidance").is_some());

    section.update(cx, |section, cx| section.move_item("PVTI_7", &done(), cx));
    let from = card("PVTI_9", cx).center();
    let to = card("PVTI_7", cx).center();
    drag(cx, from, to);
    assert_eq!(backend.writes(), Vec::new());
    assert_eq!(column_of(&section, "PVTI_7", cx), todo());
    assert_eq!(column_of(&section, "PVTI_9", cx), ColumnKey::NoValue);
}

#[gpui::test]
async fn dragging_a_card_to_another_column_moves_it(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    let (_workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);

    let todo_column = cx
        .debug_bounds("github-project-column-1")
        .expect("the Todo column is drawn");
    let empty_space = Point {
        x: todo_column.center().x,
        y: todo_column.bottom() - px(8.),
    };
    let from = card("PVTI_9", cx).center();
    drag(cx, from, empty_space);
    assert_eq!(
        backend.writes(),
        vec![set("PVTI_9", Some("todo"))],
        "a drop on empty space keeps the card's place"
    );
    assert_eq!(column_of(&section, "PVTI_9", cx), todo());

    let from = card("PVTI_7", cx).center();
    let to = card("PVTI_hidden", cx).center();
    drag(cx, from, to);
    assert_eq!(
        backend.writes(),
        vec![
            set("PVTI_9", Some("todo")),
            set("PVTI_7", Some("done")),
            moved("PVTI_7", None),
        ],
        "a drop on a card takes that card's place"
    );
    assert_eq!(
        columns(&section, cx)[2],
        (
            done(),
            vec!["PVTI_7".to_string(), "PVTI_hidden".to_string()]
        )
    );
}

fn sorted_view() -> ProjectView {
    ProjectView {
        id: "V_3".into(),
        number: 3,
        name: "Sorted".into(),
        layout: ProjectViewLayout::Board,
        filter: None,
        group_by_fields: vec!["F_status".into()],
        vertical_group_by_fields: Vec::new(),
        sort_by_fields: vec![ProjectSort {
            field_id: "F_title".into(),
            descending: false,
        }],
        visible_fields: Vec::new(),
    }
}

#[gpui::test]
async fn a_card_is_reordered_within_its_column_only_when_the_view_has_no_sort(
    cx: &mut TestAppContext,
) {
    let mut project_details = details();
    project_details.views.push(sorted_view());
    let backend = Arc::new(
        FakeGithubBackend::new()
            .with_project(&repository(), project())
            .with_project_details(PROJECT, project_details)
            .with_project_item(PROJECT, item("PVTI_7", issue(7), Some("todo")))
            .with_project_item(PROJECT, item("PVTI_8", issue(8), Some("todo")))
            .with_project_item(PROJECT, item("PVTI_9", issue(9), None)),
    );
    let (_workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);
    assert!(section.read_with(cx, |section, _| section.can_reorder()));

    let from = card("PVTI_8", cx).center();
    let to = card("PVTI_7", cx).center();
    drag(cx, from, to);
    assert_eq!(backend.writes(), vec![moved("PVTI_8", None)]);
    assert_eq!(
        columns(&section, cx)[1],
        (todo(), vec!["PVTI_8".to_string(), "PVTI_7".to_string()])
    );

    section.update(cx, |section, cx| section.select_view(3, cx));
    redraw(cx);
    assert!(!section.read_with(cx, |section, _| section.can_reorder()));
    let before = columns(&section, cx);
    let from = card("PVTI_8", cx).center();
    let to = card("PVTI_7", cx).center();
    drag(cx, from, to);
    assert_eq!(backend.writes().len(), 1, "no reorder under a sort");
    assert_eq!(columns(&section, cx), before);

    let from = card("PVTI_9", cx).center();
    let to = card("PVTI_7", cx).center();
    drag(cx, from, to);
    assert_eq!(
        backend.writes(),
        vec![moved("PVTI_8", None), set("PVTI_9", Some("todo"))],
        "across columns the value changes, the place does not"
    );
}

#[gpui::test]
async fn the_keyboard_picks_a_card_and_opens_its_move_to_menu(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    let (_workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);
    section.update_in(cx, |section, window, cx| {
        window.focus(&section.focus_handle, cx)
    });
    redraw(cx);
    cx.dispatch_action(menu::SelectNext);
    redraw(cx);
    assert_eq!(
        section.read_with(cx, |section, _| section.selected_card().map(str::to_string)),
        Some("PVTI_9".to_string()),
        "the first card of the first column"
    );
    cx.dispatch_action(menu::SecondaryConfirm);
    redraw(cx);
    assert_eq!(
        section.read_with(cx, |section, _| section
            .card_menu_item()
            .map(str::to_string)),
        Some("PVTI_9".to_string())
    );
    // A card with no status offers "Move to Todo", "Move to Done".
    pick_menu_entry(menu::SelectFirst, cx);
    assert_eq!(backend.writes(), vec![set("PVTI_9", Some("todo"))]);
    assert_eq!(column_of(&section, "PVTI_9", cx), todo());
}

#[gpui::test]
async fn a_failed_reorder_says_so_and_reads_githubs_order_again(cx: &mut TestAppContext) {
    let backend = Arc::new(
        FakeGithubBackend::new()
            .with_project(&repository(), project())
            .with_project_details(PROJECT, details())
            .with_project_item(PROJECT, item("PVTI_7", issue(7), Some("todo")))
            .with_project_item(PROJECT, item("PVTI_8", issue(8), Some("todo"))),
    );
    let (_workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);
    backend.fail_next_project_write(GithubError::NotFound);
    let reads = backend.project_requests();
    let from = card("PVTI_8", cx).center();
    let to = card("PVTI_7", cx).center();
    drag(cx, from, to);
    assert_eq!(backend.writes(), vec![moved("PVTI_8", None)]);
    assert!(
        backend.project_requests() > reads,
        "the project is read again"
    );
    assert_eq!(
        columns(&section, cx)[1],
        (todo(), vec!["PVTI_7".to_string(), "PVTI_8".to_string()]),
        "GitHub's order, which the failed write did not change"
    );
    let message = section.read_with(cx, |section, _| section.move_message().map(str::to_string));
    assert!(message.is_some_and(|message| message.contains("reorder")));
}
