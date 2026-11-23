use std::sync::Arc;

use github::{
    FakeGithubBackend, FakeWrite, GithubError, OAuthScopes, ProjectDetails, ProjectField,
    ProjectFieldInput, ProjectFieldKind, ProjectIteration, ProjectViewLayout,
};
use gpui::{Entity, Focusable as _, KeyBinding, TestAppContext, VisualTestContext};

use crate::issues::issues_test_support::{detail_tabs, repository};
use crate::projects::field_editor::parse_typed_value;
use crate::projects::projects_section_tests::{
    PROJECT, details, granted, issue, item, project, projects_tab, section,
};
use crate::projects::{ProjectsSection, cell_text};

fn field(id: &str, name: &str, kind: ProjectFieldKind) -> ProjectField {
    ProjectField {
        id: id.into(),
        name: name.into(),
        kind,
    }
}

fn sprint(id: &str, title: &str, start_date: &str) -> ProjectIteration {
    ProjectIteration {
        id: id.into(),
        title: title.into(),
        start_date: start_date.into(),
        completed: false,
    }
}

/// The shared project plus one field of every editable kind and two of the
/// issue's own; the table view shows them all.
pub(super) fn rich_details() -> ProjectDetails {
    let mut details = details();
    details.fields.extend([
        field("F_notes", "Notes", ProjectFieldKind::Text),
        field("F_points", "Points", ProjectFieldKind::Number),
        field("F_due", "Due", ProjectFieldKind::Date),
        field(
            "F_sprint",
            "Sprint",
            ProjectFieldKind::Iteration(vec![
                sprint("it_1", "Sprint 1", "2026-11-02"),
                sprint("it_2", "Sprint 2", "2026-11-16"),
            ]),
        ),
        field("F_labels", "Labels", ProjectFieldKind::Labels),
        field("F_assignees", "Assignees", ProjectFieldKind::Assignees),
    ]);
    let all = details
        .fields
        .iter()
        .map(|field| field.id.clone())
        .collect::<Vec<_>>();
    for view in &mut details.views {
        view.visible_fields = all.clone();
    }
    details
}

pub(super) fn rich_backend() -> FakeGithubBackend {
    FakeGithubBackend::new()
        .with_project(&repository(), project())
        .with_project_details(PROJECT, rich_details())
        .with_project_item(PROJECT, item("PVTI_7", issue(7), Some("todo")))
        .with_project_item(PROJECT, item("PVTI_9", issue(9), None))
}

pub(super) fn writable() -> OAuthScopes {
    OAuthScopes::Granted(vec!["repo".into(), "project".into()])
}

fn set(item_id: &str, field_id: &str, value: Option<ProjectFieldInput>) -> FakeWrite {
    FakeWrite::SetProjectItemField(PROJECT.into(), item_id.into(), field_id.into(), value)
}

/// What the item shows in the field now.
fn shown(
    section: &Entity<ProjectsSection>,
    item_id: &str,
    field_id: &str,
    cx: &mut VisualTestContext,
) -> String {
    section.read_with(cx, |section, _| {
        let field = section
            .details()
            .and_then(|details| details.field(field_id))
            .expect("the field")
            .clone();
        let item = section
            .items()
            .iter()
            .find(|item| item.id == item_id)
            .expect("the item");
        cell_text(&field, item)
    })
}

fn edit_text(
    section: &Entity<ProjectsSection>,
    field_id: &str,
    cx: &mut VisualTestContext,
) -> Option<String> {
    section.read_with(cx, |section, cx| {
        let edit = section.field_edit()?;
        (edit.field_id == field_id).then(|| edit.editor.read(cx).text(cx))
    })
}

/// Opens the cell's editor, types `text` and saves.
fn type_and_save(
    section: &Entity<ProjectsSection>,
    item_id: &str,
    field_id: &str,
    text: &str,
    cx: &mut VisualTestContext,
) {
    section.update_in(cx, |section, window, cx| {
        section.start_field_edit(item_id, field_id, window, cx);
        if let Some(edit) = section.field_edit() {
            edit.editor
                .update(cx, |editor, cx| editor.set_text(text, window, cx));
        }
        section.save_field_edit(cx);
    });
    cx.run_until_parked();
}

async fn table_tab(
    backend: Arc<FakeGithubBackend>,
    scopes: OAuthScopes,
    cx: &mut TestAppContext,
) -> (Entity<ProjectsSection>, &mut VisualTestContext) {
    let (_workspace, view, cx) = projects_tab(backend, scopes, cx).await;
    let section = section(&view, cx);
    section.update(cx, |section, cx| section.select_view(2, cx));
    cx.run_until_parked();
    assert_eq!(
        section.read_with(cx, |section, _| section.view().map(|view| view.layout)),
        Some(ProjectViewLayout::Table)
    );
    (section, cx)
}

#[gpui::test]
async fn each_field_kind_sends_its_own_value(cx: &mut TestAppContext) {
    let backend = Arc::new(rich_backend());
    let (section, cx) = table_tab(backend.clone(), writable(), cx).await;

    type_and_save(&section, "PVTI_7", "F_notes", "Ship it", cx);
    type_and_save(&section, "PVTI_7", "F_points", " 2.5 ", cx);
    type_and_save(&section, "PVTI_7", "F_due", "2026-12-01", cx);
    section.update(cx, |section, cx| {
        section.set_field_choice("PVTI_7", "F_status", Some("done".into()), cx);
        section.set_field_choice("PVTI_7", "F_sprint", Some("it_2".into()), cx);
    });
    cx.run_until_parked();

    assert_eq!(
        backend.writes(),
        vec![
            set(
                "PVTI_7",
                "F_notes",
                Some(ProjectFieldInput::Text {
                    text: "Ship it".into()
                })
            ),
            set(
                "PVTI_7",
                "F_points",
                Some(ProjectFieldInput::Number { number: 2.5 })
            ),
            set(
                "PVTI_7",
                "F_due",
                Some(ProjectFieldInput::Date {
                    date: "2026-12-01".into()
                })
            ),
            set(
                "PVTI_7",
                "F_status",
                Some(ProjectFieldInput::SingleSelect {
                    option_id: "done".into()
                })
            ),
            set(
                "PVTI_7",
                "F_sprint",
                Some(ProjectFieldInput::Iteration {
                    iteration_id: "it_2".into()
                })
            ),
        ]
    );
    assert_eq!(shown(&section, "PVTI_7", "F_notes", cx), "Ship it");
    assert_eq!(shown(&section, "PVTI_7", "F_points", cx), "2.5");
    assert_eq!(shown(&section, "PVTI_7", "F_due", cx), "2026-12-01");
    assert_eq!(shown(&section, "PVTI_7", "F_status", cx), "Done");
    assert_eq!(shown(&section, "PVTI_7", "F_sprint", cx), "Sprint 2");
    assert!(
        section.read_with(cx, |section, _| section.field_edit().is_none()),
        "a saved edit closes"
    );

    // Emptying a typed cell clears the value.
    type_and_save(&section, "PVTI_7", "F_notes", "  ", cx);
    assert_eq!(
        backend.writes().last(),
        Some(&set("PVTI_7", "F_notes", None))
    );
    assert_eq!(shown(&section, "PVTI_7", "F_notes", cx), "");
}

#[gpui::test]
async fn an_invalid_number_or_date_is_refused_before_anything_is_sent(cx: &mut TestAppContext) {
    let backend = Arc::new(rich_backend());
    let (section, cx) = table_tab(backend.clone(), writable(), cx).await;

    type_and_save(&section, "PVTI_7", "F_points", "12abc", cx);
    let error = section.read_with(cx, |section, _| {
        section
            .field_edit()
            .and_then(|edit| edit.error.clone())
            .map(|error| error.to_string())
    });
    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("number")),
        "{error:?}"
    );
    assert_eq!(
        edit_text(&section, "F_points", cx).as_deref(),
        Some("12abc")
    );

    type_and_save(&section, "PVTI_7", "F_due", "2026-02-30", cx);
    let error = section.read_with(cx, |section, _| {
        section.field_edit().and_then(|edit| edit.error.clone())
    });
    assert!(error.is_some(), "February has no 30th");
    assert_eq!(backend.writes(), Vec::new(), "nothing was sent");

    let notes = rich_details().field("F_notes").cloned().expect("notes");
    let points = rich_details().field("F_points").cloned().expect("points");
    let due = rich_details().field("F_due").cloned().expect("due");
    assert!(parse_typed_value(&points, "NaN").is_err());
    assert!(parse_typed_value(&points, "inf").is_err());
    assert!(parse_typed_value(&due, "2026-13-01").is_err());
    assert!(parse_typed_value(&due, "26-1-1").is_err());
    assert!(parse_typed_value(&due, "2028-02-29").is_ok(), "a leap day");
    assert_eq!(
        parse_typed_value(&notes, " a note ").map(|value| value.map(|(input, _)| input)),
        Ok(Some(ProjectFieldInput::Text {
            text: "a note".into()
        }))
    );
}

#[gpui::test]
async fn a_failed_save_keeps_the_typed_value_and_says_why(cx: &mut TestAppContext) {
    let backend = Arc::new(rich_backend());
    let (section, cx) = table_tab(backend.clone(), writable(), cx).await;
    backend.fail_next_project_write(GithubError::Forbidden {
        message: "Resource not accessible".into(),
    });

    type_and_save(&section, "PVTI_7", "F_notes", "Hello", cx);

    assert_eq!(
        backend.writes(),
        vec![set(
            "PVTI_7",
            "F_notes",
            Some(ProjectFieldInput::Text {
                text: "Hello".into()
            })
        )]
    );
    assert_eq!(
        edit_text(&section, "F_notes", cx).as_deref(),
        Some("Hello"),
        "the typed value is still there to retry"
    );
    let error = section.read_with(cx, |section, _| {
        section
            .field_edit()
            .and_then(|edit| edit.error.clone())
            .map(|error| error.to_string())
    });
    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("Resource not accessible")),
        "{error:?}"
    );
    assert_eq!(
        shown(&section, "PVTI_7", "F_notes", cx),
        "",
        "the cell shows GitHub's value again"
    );
}

#[gpui::test]
async fn enter_saves_and_escape_cancels(cx: &mut TestAppContext) {
    let backend = Arc::new(rich_backend());
    let (section, cx) = table_tab(backend.clone(), writable(), cx).await;
    cx.update(|_, cx| {
        cx.bind_keys([
            KeyBinding::new("enter", menu::Confirm, None),
            KeyBinding::new("escape", editor::actions::Cancel, Some("Editor")),
        ])
    });

    let start = |field_id: &'static str, cx: &mut VisualTestContext| {
        section.update_in(cx, |section, window, cx| {
            section.start_field_edit("PVTI_9", field_id, window, cx);
            let editor = section.field_edit().expect("an editor").editor.clone();
            window.focus(&editor.focus_handle(cx), cx);
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    };

    start("F_notes", cx);
    cx.simulate_input("Typed");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        backend.writes(),
        vec![set(
            "PVTI_9",
            "F_notes",
            Some(ProjectFieldInput::Text {
                text: "Typed".into()
            })
        )]
    );
    assert!(section.read_with(cx, |section, _| section.field_edit().is_none()));

    start("F_points", cx);
    cx.simulate_input("5");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        section.read_with(cx, |section, _| section.field_edit().is_none()),
        "escape closes the editor"
    );
    assert_eq!(backend.writes().len(), 1, "escape sends nothing");
    assert_eq!(shown(&section, "PVTI_9", "F_points", cx), "");
}

#[gpui::test]
async fn labels_and_assignees_are_edited_on_the_issue_not_the_project(cx: &mut TestAppContext) {
    let backend = Arc::new(rich_backend());
    let (section, cx) = table_tab(backend.clone(), writable(), cx).await;
    for field_id in ["F_labels", "F_assignees", "F_title"] {
        section.update_in(cx, |section, window, cx| {
            section.start_field_edit("PVTI_7", field_id, window, cx);
            section.set_field_choice("PVTI_7", field_id, Some("bug".into()), cx);
        });
        cx.run_until_parked();
        let (editing, message) = section.read_with(cx, |section, _| {
            (
                section.field_edit().is_some(),
                section.field_message().map(str::to_string),
            )
        });
        assert!(!editing, "{field_id} opens no project editor");
        assert!(
            message
                .as_deref()
                .is_some_and(|message| message.contains("on the issue")),
            "{field_id}: {message:?}"
        );
    }
    assert_eq!(backend.writes(), Vec::new());
}

#[gpui::test]
async fn a_read_only_token_gets_guidance_and_writes_nothing(cx: &mut TestAppContext) {
    let backend = Arc::new(rich_backend());
    let (section, cx) = table_tab(backend.clone(), granted(), cx).await;
    section.update_in(cx, |section, window, cx| {
        section.start_field_edit("PVTI_7", "F_notes", window, cx);
        section.set_field_choice("PVTI_7", "F_status", Some("done".into()), cx);
        section.open_add_item(cx);
        section.request_remove_item("PVTI_7", cx);
        section.confirm_remove_item(cx);
    });
    cx.run_until_parked();
    section.read_with(cx, |section, _| {
        assert!(section.field_edit().is_none());
        assert!(section.add_item_picker().is_none());
        assert!(section.pending_removal().is_none());
        assert!(section.write_scope_missing(), "the guidance is shown");
        assert_eq!(section.items().len(), 2);
    });
    assert_eq!(backend.writes(), Vec::new());
    assert!(cx.debug_bounds("github-projects-write-guidance").is_some());
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

#[gpui::test]
async fn clicking_a_table_cell_edits_it_in_place(cx: &mut TestAppContext) {
    let backend = Arc::new(rich_backend());
    let (workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);
    section.update(cx, |section, cx| section.select_view(2, cx));
    redraw(cx);
    // Row 0 is PVTI_7; column 2 is Notes (Title, Status, Notes, …).
    let cell = cx
        .debug_bounds("github-project-cell-0-2")
        .expect("the Notes cell is editable");
    cx.simulate_click(cell.center(), gpui::Modifiers::none());
    redraw(cx);
    let edited = section.read_with(cx, |section, _| {
        section
            .field_edit()
            .map(|edit| (edit.item_id.clone(), edit.field_id.clone()))
    });
    assert_eq!(edited, Some(("PVTI_7".into(), "F_notes".into())));
    assert!(cx.debug_bounds("github-project-field-editor").is_some());
    assert!(
        detail_tabs(&workspace, cx).is_empty(),
        "the click edits; it does not open the issue"
    );
    assert!(
        cx.debug_bounds("github-project-cell-0-0").is_none(),
        "the title cell opens the item instead"
    );
}

#[gpui::test]
async fn the_boards_panel_edits_the_picked_card(cx: &mut TestAppContext) {
    let backend = Arc::new(rich_backend());
    let (_workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);
    assert!(cx.debug_bounds("github-project-item-panel").is_none());
    section.update(cx, |section, cx| section.select_card("PVTI_7".into(), cx));
    redraw(cx);
    assert!(cx.debug_bounds("github-project-item-panel").is_some());
    section.update_in(cx, |section, window, cx| {
        section.start_field_edit("PVTI_7", "F_due", window, cx)
    });
    redraw(cx);
    assert!(cx.debug_bounds("github-project-field-editor").is_some());
    assert_eq!(backend.writes(), Vec::new());
}

#[gpui::test]
async fn saving_an_edit_whose_item_is_gone_says_so_and_sends_nothing(cx: &mut TestAppContext) {
    let backend = Arc::new(rich_backend());
    let (section, cx) = table_tab(backend.clone(), writable(), cx).await;
    section.update_in(cx, |section, window, cx| {
        section.start_field_edit("PVTI_7", "F_notes", window, cx);
        if let Some(edit) = section.field_edit() {
            edit.editor
                .update(cx, |editor, cx| editor.set_text("Ship it", window, cx));
        }
        // Read again meanwhile, without it: someone removed it on GitHub.
        section.items.retain(|item| item.id != "PVTI_7");
        section.save_field_edit(cx);
    });
    cx.run_until_parked();
    let error = section.read_with(cx, |section, _| {
        section
            .field_edit()
            .and_then(|edit| edit.error.as_ref().map(|error| error.to_string()))
    });
    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("no longer in the project")),
        "{error:?}"
    );
    assert_eq!(backend.writes(), Vec::new());
}

#[gpui::test]
async fn clicking_inside_a_cell_editor_does_not_open_the_item(cx: &mut TestAppContext) {
    let backend = Arc::new(rich_backend());
    let (workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);
    section.update(cx, |section, cx| section.select_view(2, cx));
    redraw(cx);
    let cell = cx
        .debug_bounds("github-project-cell-0-2")
        .expect("the Notes cell is editable");
    cx.simulate_click(cell.center(), gpui::Modifiers::none());
    redraw(cx);
    let editor = cx
        .debug_bounds("github-project-field-editor")
        .expect("the cell's editor is open");
    cx.simulate_click(editor.center(), gpui::Modifiers::none());
    redraw(cx);
    assert!(
        detail_tabs(&workspace, cx).is_empty(),
        "a click in the editor stays in the editor"
    );
    assert!(section.read_with(cx, |section, _| section.field_edit().is_some()));
}
