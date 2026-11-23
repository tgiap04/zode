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
