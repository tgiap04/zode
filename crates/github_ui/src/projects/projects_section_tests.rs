use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use github::{
    FakeGithubBackend, GithubError, OAuthScopes, ProjectDetails, ProjectField, ProjectFieldKind,
    ProjectFieldOption, ProjectFieldValue, ProjectItemContent, ProjectV2, ProjectV2Item,
    ProjectValue, ProjectView, ProjectViewLayout,
};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use workspace::Workspace;

use crate::github_view::{AuthState, GithubSection, GithubView};
use crate::issues::issues_test_support::{detail_tabs, repository, signed_in_github_tab};
use crate::projects::move_item::{FieldWriteKey, PendingFieldWrites};
use crate::projects::project_layout::value_of;
use crate::projects::{ColumnKey, PROJECT_ITEM_CAP, ProjectsSection, board_columns};

pub(super) const PROJECT: &str = "PVT_1";

pub(super) fn project() -> ProjectV2 {
    ProjectV2 {
        id: PROJECT.into(),
        number: 1,
        title: "Roadmap".into(),
        url: "https://github.com/orgs/octo/projects/1".into(),
        closed: false,
    }
}

pub(super) fn details() -> ProjectDetails {
    let option = |id: &str, name: &str| ProjectFieldOption {
        id: id.into(),
        name: name.into(),
        color: None,
    };
    ProjectDetails {
        fields: vec![
            ProjectField {
                id: "F_title".into(),
                name: "Title".into(),
                kind: ProjectFieldKind::Title,
            },
            ProjectField {
                id: "F_status".into(),
                name: "Status".into(),
                kind: ProjectFieldKind::SingleSelect(vec![
                    option("todo", "Todo"),
                    option("done", "Done"),
                ]),
            },
        ],
        views: vec![
            ProjectView {
                id: "V_1".into(),
                number: 1,
                name: "Board".into(),
                layout: ProjectViewLayout::Board,
                filter: Some("is:open".into()),
                group_by_fields: vec!["F_status".into()],
                vertical_group_by_fields: Vec::new(),
                sort_by_fields: Vec::new(),
                visible_fields: vec!["F_title".into(), "F_status".into()],
            },
            ProjectView {
                id: "V_2".into(),
                number: 2,
                name: "Table".into(),
                layout: ProjectViewLayout::Table,
                filter: None,
                group_by_fields: Vec::new(),
                vertical_group_by_fields: Vec::new(),
                sort_by_fields: Vec::new(),
                visible_fields: vec!["F_title".into(), "F_status".into()],
            },
        ],
    }
}

pub(super) fn item(id: &str, content: ProjectItemContent, status: Option<&str>) -> ProjectV2Item {
    ProjectV2Item {
        id: id.into(),
        content,
        assignees: vec!["octocat".into()],
        assignee_avatars: Default::default(),
        state: None,
        sub_issues: None,
        labels: Vec::new(),
        field_values: status
            .map(|status| ProjectFieldValue {
                field_id: "F_status".into(),
                value: ProjectValue::SingleSelect {
                    option_id: status.into(),
                    name: status.into(),
                },
            })
            .into_iter()
            .collect(),
    }
}

pub(super) fn issue(number: u64) -> ProjectItemContent {
    ProjectItemContent::Issue {
        number,
        title: format!("Issue {number}"),
        repository: "octo/repo".into(),
    }
}

/// An issue in Todo, a hidden item in Done, an issue with no status.
pub(super) fn backend() -> FakeGithubBackend {
    FakeGithubBackend::new()
        .with_project(&repository(), project())
        .with_project_details(PROJECT, details())
        .with_project_item(PROJECT, item("PVTI_7", issue(7), Some("todo")))
        .with_project_item(
            PROJECT,
            item("PVTI_hidden", ProjectItemContent::Redacted, Some("done")),
        )
        .with_project_item(PROJECT, item("PVTI_9", issue(9), None))
}

pub(super) fn granted() -> OAuthScopes {
    OAuthScopes::Granted(vec!["repo".into(), "read:project".into()])
}

/// A GitHub tab in the workspace, signed in with `scopes`, showing Projects.
pub(super) async fn projects_tab(
    backend: Arc<FakeGithubBackend>,
    scopes: OAuthScopes,
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<GithubView>,
    &mut VisualTestContext,
) {
    signed_in_github_tab(backend, scopes, GithubSection::Projects, cx).await
}

pub(super) fn section(
    view: &Entity<GithubView>,
    cx: &mut VisualTestContext,
) -> Entity<ProjectsSection> {
    view.read_with(cx, |view, _| view.projects_section().cloned())
        .expect("the Projects section is shown")
}

#[gpui::test]
async fn without_read_project_the_section_guides_and_reads_nothing(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    let scopes = OAuthScopes::Granted(vec!["repo".into(), "read:org".into()]);
    let (_workspace, view, cx) = projects_tab(backend.clone(), scopes, cx).await;
    let section = section(&view, cx);
    assert!(section.read_with(cx, |section, _| section.missing_scope()));
    assert_eq!(backend.project_requests(), 0, "no project request is sent");
    assert!(
        cx.debug_bounds("github-projects-guidance").is_some(),
        "the guidance is drawn"
    );
    section.update(cx, |section, cx| section.reload(cx));
    cx.run_until_parked();
    assert_eq!(backend.project_requests(), 0, "Reload sends nothing either");
}

#[gpui::test]
async fn a_scope_error_from_github_turns_into_the_guidance(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    backend.set_failure(Some(GithubError::MissingScope {
        scope: "read:project".into(),
    }));
    let (_workspace, view, cx) = projects_tab(backend.clone(), OAuthScopes::Unknown, cx).await;
    let section = section(&view, cx);
    assert!(section.read_with(cx, |section, _| section.missing_scope()));
    assert_eq!(
        backend.project_requests(),
        1,
        "only the projects list was tried"
    );
}

#[gpui::test]
async fn the_board_groups_by_the_views_field_and_keeps_hidden_items(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    let (_workspace, view, cx) = projects_tab(backend, granted(), cx).await;
    let section = section(&view, cx);
    let columns = section.read_with(cx, |section, _| {
        let details = section.details().expect("the project's details");
        board_columns(section.view(), details, section.items())
            .into_iter()
            .map(|column| {
                let ids = column
                    .items
                    .iter()
                    .map(|index| section.items()[*index].id.clone())
                    .collect::<Vec<_>>();
                (column.key, column.name, ids)
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(
        columns,
        vec![
            (
                ColumnKey::NoValue,
                "No Status".into(),
                vec!["PVTI_9".into()]
            ),
            (
                ColumnKey::Option("todo".into()),
                "Todo".into(),
                vec!["PVTI_7".into()]
            ),
            (
                ColumnKey::Option("done".into()),
                "Done".into(),
                vec!["PVTI_hidden".into()]
            ),
        ]
    );
    assert!(cx.debug_bounds("github-project-card-PVTI_hidden").is_some());
    assert!(
        section.read_with(cx, |section, _| !section.has_more_items()),
        "three items need no notice"
    );
    section.update(cx, |section, cx| section.select_view(2, cx));
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("github-project-row-0").is_some(),
        "the table view draws rows"
    );
}

#[gpui::test]
async fn clicking_a_card_opens_the_issue_and_a_hidden_card_opens_nothing(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    let (workspace, _view, cx) = projects_tab(backend, granted(), cx).await;
    let hidden = cx
        .debug_bounds("github-project-card-PVTI_hidden")
        .expect("the hidden card is drawn");
    cx.simulate_click(hidden.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(detail_tabs(&workspace, cx).is_empty());

    let card = cx
        .debug_bounds("github-project-card-PVTI_7")
        .expect("the card is drawn");
    cx.simulate_click(card.center(), Modifiers::none());
    cx.run_until_parked();
    let tabs = detail_tabs(&workspace, cx);
    assert_eq!(tabs.len(), 1);
    let key = tabs[0].read_with(cx, |tab, _| (tab.key.repository.clone(), tab.key.number));
    assert_eq!(key, (repository(), 7));
}

#[gpui::test]
async fn past_the_cap_reading_stops_and_says_so(cx: &mut TestAppContext) {
    let mut fake = FakeGithubBackend::new()
        .with_project(&repository(), project())
        .with_project_details(PROJECT, details());
    for number in 0..(PROJECT_ITEM_CAP as u64 + 20) {
        fake = fake.with_project_item(
            PROJECT,
            item(&format!("PVTI_{number}"), issue(number), None),
        );
    }
    let backend = Arc::new(fake);
    let (_workspace, view, cx) = projects_tab(backend.clone(), granted(), cx).await;
    let section = section(&view, cx);
    let (loaded, more) = section.read_with(cx, |section, _| {
        (section.items().len(), section.has_more_items())
    });
    assert_eq!(loaded, PROJECT_ITEM_CAP);
    assert!(more);
    assert_eq!(
        backend.project_requests(),
        2 + PROJECT_ITEM_CAP / 50,
        "projects, details, then pages of 50 up to the cap"
    );
    assert!(cx.debug_bounds("github-project-cap-notice").is_some());

    section.update(cx, |section, cx| section.load_more(cx));
    cx.run_until_parked();
    let (loaded, more) = section.read_with(cx, |section, _| {
        (section.items().len(), section.has_more_items())
    });
    assert_eq!((loaded, more), (PROJECT_ITEM_CAP + 20, false));

    section.update(cx, |section, cx| section.reread_project(cx));
    cx.run_until_parked();
    let loaded = section.read_with(cx, |section, _| section.items().len());
    assert_eq!(
        loaded,
        PROJECT_ITEM_CAP + 20,
        "reading the project again keeps what \"Load more\" asked for"
    );
}

/// A project whose item pages claim more under a cursor that never moves
/// on, for the first 20 pages.
fn stuck_backend(items: u64) -> Arc<FakeGithubBackend> {
    let mut fake = FakeGithubBackend::new()
        .with_project(&repository(), project())
        .with_project_details(PROJECT, details())
        .with_stuck_project_item_cursor(20);
    for number in 0..items {
        fake = fake.with_project_item(
            PROJECT,
            item(&format!("PVTI_{number}"), issue(number), None),
        );
    }
    Arc::new(fake)
}

/// GitHub answering a page under a cursor that does not move on would
/// otherwise be asked again and again.
#[gpui::test]
async fn an_item_cursor_that_repeats_stops_the_reading(cx: &mut TestAppContext) {
    let backend = stuck_backend(3);
    let (_workspace, view, cx) = projects_tab(backend.clone(), granted(), cx).await;
    let section = section(&view, cx);
    assert_eq!(
        backend.project_requests(),
        4,
        "projects, details, the first page, then the page whose cursor repeats"
    );
    let (loaded, more) = section.read_with(cx, |section, _| {
        (section.items().len(), section.has_more_items())
    });
    assert_eq!(
        (loaded, more),
        (3, false),
        "each item once, and no more offered"
    );
}

/// Cursors that take turns (A, B, A, …) never repeat the one just sent,
/// yet would have GitHub asked the same pages until the rate limit.
#[gpui::test]
async fn item_cursors_that_take_turns_stop_the_reading(cx: &mut TestAppContext) {
    let backend = Arc::new(
        FakeGithubBackend::new()
            .with_project(&repository(), project())
            .with_project_details(PROJECT, details())
            .with_project_item_cursors(&["0~a", "0~b"].repeat(10)),
    );
    let (_workspace, view, cx) = projects_tab(backend.clone(), granted(), cx).await;
    let section = section(&view, cx);
    assert_eq!(
        backend.project_requests(),
        5,
        "projects, details, the first page, A, then B, which leads back to A"
    );
    assert!(section.read_with(cx, |section, _| !section.has_more_items()));
}

#[gpui::test]
async fn empty_item_pages_stop_once_their_cursor_repeats(cx: &mut TestAppContext) {
    let backend = stuck_backend(0);
    let (_workspace, view, cx) = projects_tab(backend.clone(), granted(), cx).await;
    let section = section(&view, cx);
    assert_eq!(
        backend.project_requests(),
        4,
        "projects, details, an empty page with a new cursor, then one under the same cursor"
    );
    assert!(section.read_with(cx, |section, _| !section.has_more_items()));
}

#[gpui::test]
async fn the_section_is_kept_across_tabs_and_dropped_on_sign_out(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    let (_workspace, view, cx) = projects_tab(backend.clone(), granted(), cx).await;
    let first = section(&view, cx);
    let requests = backend.project_requests();
    view.update(cx, |view, cx| {
        view.select_section(GithubSection::Issues, cx);
        view.select_section(GithubSection::Projects, cx);
    });
    cx.run_until_parked();
    assert_eq!(section(&view, cx).entity_id(), first.entity_id());
    assert_eq!(
        backend.project_requests(),
        requests,
        "coming back reads nothing"
    );
    view.update(cx, |view, cx| view.disconnect(cx));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.projects_section().is_none()));
}

#[gpui::test]
async fn a_token_refused_in_projects_signs_the_github_tab_out(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    let (_workspace, view, cx) = projects_tab(backend.clone(), granted(), cx).await;
    backend.set_failure(Some(GithubError::TokenRevoked));
    section(&view, cx).update(cx, |section, cx| section.reload(cx));
    cx.run_until_parked();
    assert!(matches!(
        view.read_with(cx, |view, _| view.auth().clone()),
        AuthState::Revoked { .. }
    ));
}

/// A page can hold nothing (every item on it hidden) and still lead on.
#[gpui::test]
async fn an_empty_item_page_under_a_new_cursor_keeps_reading(cx: &mut TestAppContext) {
    let mut fake = FakeGithubBackend::new()
        .with_project(&repository(), project())
        .with_project_details(PROJECT, details())
        .with_empty_project_item_pages(2);
    for number in 0..3 {
        fake = fake.with_project_item(
            PROJECT,
            item(&format!("PVTI_{number}"), issue(number), None),
        );
    }
    let backend = Arc::new(fake);
    let (_workspace, view, cx) = projects_tab(backend.clone(), granted(), cx).await;
    let section = section(&view, cx);
    assert_eq!(
        section.read_with(cx, |section, _| section.items().len()),
        3,
        "the items after the empty pages are read"
    );
    assert_eq!(
        backend.project_requests(),
        5,
        "projects, details, two empty pages, then the items"
    );
}

const OTHER_PROJECT: &str = "PVT_2";

/// Two projects: the first (`PROJECT`) is picked, the second holds one item.
fn two_projects() -> FakeGithubBackend {
    backend()
        .with_project(
            &repository(),
            ProjectV2 {
                id: OTHER_PROJECT.into(),
                number: 2,
                title: "Other".into(),
                url: "https://github.com/orgs/octo/projects/2".into(),
                closed: false,
            },
        )
        .with_project_details(OTHER_PROJECT, details())
        .with_project_item(OTHER_PROJECT, item("PVTI_other", issue(30), Some("todo")))
}

fn write_scope() -> OAuthScopes {
    OAuthScopes::Granted(vec!["repo".into(), "project".into()])
}

fn item_ids(section: &Entity<ProjectsSection>, cx: &mut VisualTestContext) -> Vec<String> {
    section.read_with(cx, |section, _| {
        section.items().iter().map(|item| item.id.clone()).collect()
    })
}

/// The cards of the project that was left are not the new project's: a
/// write aimed at one of them must not go out under the new project's id.
#[gpui::test]
async fn switching_projects_drops_the_old_items_at_once(cx: &mut TestAppContext) {
    let backend = Arc::new(two_projects());
    let (_workspace, view, cx) = projects_tab(backend.clone(), write_scope(), cx).await;
    let section = section(&view, cx);
    assert_eq!(item_ids(&section, cx).len(), 3);

    section.update(cx, |section, cx| {
        section.select_project(OTHER_PROJECT.into(), cx);
        assert!(
            section.items().is_empty(),
            "the old project's items are not shown for the new one"
        );
        section.move_item("PVTI_7", &ColumnKey::Option("done".into()), cx);
    });
    assert_eq!(
        backend.writes(),
        Vec::new(),
        "nothing is written for an item of the other project"
    );
    cx.run_until_parked();
    assert_eq!(item_ids(&section, cx), vec!["PVTI_other".to_string()]);
}

/// A write of PVTI_7 to Done is out, and GitHub still says Todo.
fn pretend_a_write_to_done_is_out(section: &Entity<ProjectsSection>, cx: &mut VisualTestContext) {
    section.update(cx, |section, _| {
        let item = section
            .items
            .iter_mut()
            .find(|item| item.id == "PVTI_7")
            .expect("the item");
        item.field_values
            .retain(|value| value.field_id != "F_status");
        item.field_values.push(ProjectFieldValue {
            field_id: "F_status".into(),
            value: ProjectValue::SingleSelect {
                option_id: "done".into(),
                name: "Done".into(),
            },
        });
        section.layout_changed();
        section.field_writes.insert(
            FieldWriteKey {
                item_id: "PVTI_7".into(),
                field_id: "F_status".into(),
            },
            PendingFieldWrites {
                latest: 1,
                in_flight: 1,
                confirmed: Some(ProjectValue::SingleSelect {
                    option_id: "todo".into(),
                    name: "Todo".into(),
                }),
                shown: Some(ProjectValue::SingleSelect {
                    option_id: "done".into(),
                    name: "Done".into(),
                }),
            },
        );
    });
}

/// Reading a project again replaces its items in place: the board never
/// goes blank.
#[gpui::test]
async fn reading_again_never_blanks_the_board(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    let (_workspace, view, cx) = projects_tab(backend.clone(), write_scope(), cx).await;
    let section = section(&view, cx);
    let counts = Rc::new(RefCell::new(Vec::new()));
    cx.update(|_, cx| {
        let counts = counts.clone();
        cx.observe(&section, move |section, cx| {
            counts.borrow_mut().push(section.read(cx).items().len());
        })
        .detach();
    });
    section.update(cx, |section, cx| section.reload(cx));
    cx.run_until_parked();
    assert!(
        counts.borrow().iter().all(|count| *count > 0),
        "the board was blank while reading again: {:?}",
        counts.borrow()
    );
    assert_eq!(item_ids(&section, cx).len(), 3);
}

/// A value being written stays shown whatever GitHub still reports.
#[gpui::test]
async fn reading_again_keeps_the_values_being_written(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    let (_workspace, view, cx) = projects_tab(backend.clone(), write_scope(), cx).await;
    let section = section(&view, cx);
    pretend_a_write_to_done_is_out(&section, cx);
    section.update(cx, |section, cx| section.reload(cx));
    cx.run_until_parked();
    let shown = section.read_with(cx, |section, _| {
        let item = section.items().iter().find(|item| item.id == "PVTI_7");
        item.and_then(|item| value_of(item, "F_status").cloned())
    });
    assert_eq!(
        shown,
        Some(ProjectValue::SingleSelect {
            option_id: "done".into(),
            name: "Done".into(),
        }),
        "the value being written is not undone by a read that predates it"
    );
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

/// Sorting and grouping every item is worked out once, not on every frame;
/// a change to the items or the view works it out again.
#[gpui::test]
async fn the_layout_is_worked_out_again_only_when_something_it_depends_on_changes(
    cx: &mut TestAppContext,
) {
    let backend = Arc::new(backend());
    let (_workspace, view, cx) = projects_tab(backend.clone(), write_scope(), cx).await;
    let section = section(&view, cx);
    redraw(cx);
    let builds = |cx: &mut VisualTestContext| section.read_with(cx, |s, _| s.layout.builds.get());
    let first = builds(cx);
    assert!(first > 0, "the board was laid out");
    for _ in 0..3 {
        redraw(cx);
    }
    assert_eq!(builds(cx), first, "drawing again changes nothing");

    section.update(cx, |section, cx| {
        section.move_item("PVTI_7", &ColumnKey::Option("done".into()), cx)
    });
    redraw(cx);
    let after_move = builds(cx);
    assert!(after_move > first, "a moved card changes the columns");
    redraw(cx);
    assert_eq!(builds(cx), after_move);

    section.update(cx, |section, cx| section.select_view(2, cx));
    redraw(cx);
    let table = builds(cx);
    assert!(table > after_move, "another view is laid out again");
    redraw(cx);
    assert_eq!(builds(cx), table);
}

/// The cards' scrolling lists are kept per column; those of columns the
/// view no longer has are dropped, not kept for good.
#[gpui::test]
async fn the_lists_of_columns_that_are_gone_are_dropped(cx: &mut TestAppContext) {
    let mut project_details = crate::projects::field_editor_tests::rich_details();
    project_details.views.push(ProjectView {
        id: "V_3".into(),
        number: 3,
        name: "Sprints".into(),
        layout: ProjectViewLayout::Board,
        filter: None,
        group_by_fields: vec!["F_sprint".into()],
        vertical_group_by_fields: Vec::new(),
        sort_by_fields: Vec::new(),
        visible_fields: vec!["F_title".into()],
    });
    let backend = Arc::new(
        FakeGithubBackend::new()
            .with_project(&repository(), project())
            .with_project_details(PROJECT, project_details)
            .with_project_item(PROJECT, item("PVTI_7", issue(7), Some("todo"))),
    );
    let (_workspace, view, cx) = projects_tab(backend, write_scope(), cx).await;
    let section = section(&view, cx);
    redraw(cx);
    let kept = |cx: &mut VisualTestContext| {
        section.read_with(cx, |section, _| section.layout.column_list_keys())
    };
    let status_columns = kept(cx);
    assert!(
        status_columns.contains(&ColumnKey::Option("todo".into())),
        "{status_columns:?}"
    );

    section.update(cx, |section, cx| section.select_view(3, cx));
    redraw(cx);
    let sprint_columns = kept(cx);
    assert!(
        sprint_columns.contains(&ColumnKey::Iteration("it_1".into())),
        "{sprint_columns:?}"
    );
    assert!(
        !sprint_columns.contains(&ColumnKey::Option("todo".into())),
        "the status columns are gone: {sprint_columns:?}"
    );
}

/// What a read brings is put under the writes made since it began (kept in
/// `recent_writes` once they land) and the ones still out, which win.
#[gpui::test]
async fn a_read_is_overlaid_with_the_writes_that_landed_and_the_ones_still_out(
    cx: &mut TestAppContext,
) {
    let backend = Arc::new(backend());
    let (_workspace, view, cx) = projects_tab(backend.clone(), write_scope(), cx).await;
    let section = section(&view, cx);
    let option = |id: &str| ProjectValue::SingleSelect {
        option_id: id.into(),
        name: id.into(),
    };
    let status_of = |items: &[ProjectV2Item], id: &str| {
        items
            .iter()
            .find(|item| item.id == id)
            .and_then(|item| value_of(item, "F_status").cloned())
    };
    let overlaid = section.update(cx, |section, _| {
        let key = |id: &str| FieldWriteKey {
            item_id: id.into(),
            field_id: "F_status".into(),
        };
        // PVTI_7 landed as Done; PVTI_9 landed, then another write is out.
        section
            .recent_writes
            .insert(key("PVTI_7"), Some(option("done")));
        section
            .recent_writes
            .insert(key("PVTI_9"), Some(option("todo")));
        section.field_writes.insert(
            key("PVTI_9"),
            PendingFieldWrites {
                latest: 2,
                in_flight: 1,
                confirmed: Some(option("todo")),
                shown: None,
            },
        );
        let mut read = section.items.clone();
        section.overlay_field_writes(&mut read);
        read
    });
    assert_eq!(status_of(&overlaid, "PVTI_7"), Some(option("done")));
    assert_eq!(
        status_of(&overlaid, "PVTI_9"),
        None,
        "the write still out wins"
    );
    assert_eq!(
        status_of(&overlaid, "PVTI_hidden"),
        Some(option("done")),
        "an item nothing was written to is as it was read"
    );
}

/// A column holding hundreds of cards draws the ones in view (and a margin),
/// not every one of them on every frame.
#[gpui::test]
async fn a_long_column_draws_only_the_cards_in_view(cx: &mut TestAppContext) {
    let mut fake = FakeGithubBackend::new()
        .with_project(&repository(), project())
        .with_project_details(PROJECT, details());
    for number in 0..300 {
        fake = fake.with_project_item(
            PROJECT,
            item(&format!("PVTI_{number}"), issue(number), Some("todo")),
        );
    }
    let backend = Arc::new(fake);
    let (_workspace, view, cx) = projects_tab(backend, write_scope(), cx).await;
    let section = section(&view, cx);
    redraw(cx);
    assert_eq!(
        section.read_with(cx, |section, _| section.items().len()),
        300
    );
    let drawn = (0..300)
        .filter(|number| {
            cx.debug_bounds(format!("github-project-card-PVTI_{number}").leak())
                .is_some()
        })
        .count();
    assert!(drawn > 0, "the cards in view are drawn");
    assert!(drawn < 60, "{drawn} of 300 cards were drawn");
}

/// A read of several pages commits only once its last page is in; a move
/// that lands after the item's own page was read is not undone by it.
#[gpui::test]
async fn a_move_that_lands_during_a_long_read_survives_it(cx: &mut TestAppContext) {
    let backend = Arc::new(backend().with_stuck_project_item_cursor(3));
    let (_workspace, view, cx) = projects_tab(backend.clone(), write_scope(), cx).await;
    let section = section(&view, cx);
    let first_page = backend.hold_next_project_item_list();
    let last_page = backend.hold_next_project_item_list();
    section.update(cx, |section, cx| section.reload(cx));
    cx.run_until_parked();
    first_page.send(()).ok();
    cx.run_until_parked();

    section.update(cx, |section, cx| {
        section.move_item("PVTI_7", &ColumnKey::Option("done".into()), cx)
    });
    cx.run_until_parked();
    last_page.send(()).ok();
    cx.run_until_parked();

    let shown = section.read_with(cx, |section, _| {
        let item = section.items().iter().find(|item| item.id == "PVTI_7");
        item.and_then(|item| value_of(item, "F_status").cloned())
    });
    assert!(
        matches!(&shown, Some(ProjectValue::SingleSelect { option_id, .. }) if option_id == "done"),
        "the landed move is kept when the read commits: {shown:?}"
    );
}

/// A token from the GitHub CLI is missing a permission, not a sign-in: the way
/// out it is offered first is granting that permission through the CLI, not
/// signing in again -- which read as being asked to log in once more.
#[gpui::test]
async fn a_cli_token_without_read_project_is_offered_the_cli_grant(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    let scopes = OAuthScopes::Granted(vec!["repo".into(), "read:org".into()]);
    let (_workspace, _view, cx) = projects_tab(backend, scopes, cx).await;
    assert!(
        cx.debug_bounds("github-projects-grant-cli").is_some(),
        "the GitHub CLI grant is offered"
    );
}

/// A token that is not the GitHub CLI's is not offered the CLI grant: `gh auth
/// refresh` would change a different token, or none.
#[gpui::test]
async fn a_token_from_elsewhere_is_not_offered_the_cli_grant(cx: &mut TestAppContext) {
    let backend = Arc::new(backend());
    let scopes = OAuthScopes::Granted(vec!["repo".into(), "read:org".into()]);
    let (_workspace, view, cx) = projects_tab(backend.clone(), scopes.clone(), cx).await;
    view.update(cx, |view, cx| {
        view.set_auth(
            crate::github_view::AuthState::SignedIn(crate::github_view::Session {
                login: "octocat".into(),
                token_source: github::TokenSource::Keychain,
                backend: Arc::new(super::projects_section_tests::backend()),
                scopes,
            }),
            cx,
        );
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("github-projects-guidance").is_some(),
        "the guidance is still drawn"
    );
    assert!(
        cx.debug_bounds("github-projects-grant-cli").is_none(),
        "but without the GitHub CLI grant"
    );
}
