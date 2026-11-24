use std::sync::Arc;

use github::{FakeGithubBackend, FakeWrite, GithubError, OAuthScopes, ProjectOwner, ProjectOwners};
use gpui::{Entity, TestAppContext, VisualTestContext};
use workspace::Workspace;

use crate::github_view::GithubView;
use crate::github_view_session::{release_session, session_ended_message};
use crate::issues::issues_test_support::repository;
use crate::projects::create_project::{CreateProjectDialog, MAX_PROJECT_TITLE_CHARS, OwnerChoice};
use crate::projects::projects_section_tests::{backend, projects_tab, section};

fn writable() -> OAuthScopes {
    OAuthScopes::Granted(vec!["repo".into(), "project".into()])
}

fn organization_owners() -> ProjectOwners {
    ProjectOwners {
        repository_id: "R_octo_repo".into(),
        viewer: ProjectOwner {
            id: "U_octocat".into(),
            login: "octocat".into(),
            avatar_url: None,
        },
        organization: Some(ProjectOwner {
            id: "O_octo".into(),
            login: "octo".into(),
            avatar_url: Some("https://avatars.githubusercontent.com/u/2?s=40".into()),
        }),
    }
}

/// The GitHub tab on Projects, and the dialog its "New project" opened.
async fn open_dialog(
    backend: Arc<FakeGithubBackend>,
    scopes: OAuthScopes,
    cx: &mut TestAppContext,
) -> (
    Entity<GithubView>,
    Entity<CreateProjectDialog>,
    &mut VisualTestContext,
) {
    let (workspace, view, cx) = projects_tab(backend, scopes, cx).await;
    let section = section(&view, cx);
    section.update_in(cx, |section, window, cx| {
        section.open_create_project(window, cx)
    });
    cx.run_until_parked();
    let dialog = active_dialog(&workspace, cx).expect("the New project dialog is open");
    (view, dialog, cx)
}

fn active_dialog(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Option<Entity<CreateProjectDialog>> {
    workspace.read_with(cx, |workspace, cx| {
        workspace.active_modal::<CreateProjectDialog>(cx)
    })
}

fn type_title(dialog: &Entity<CreateProjectDialog>, text: &str, cx: &mut VisualTestContext) {
    let title = dialog.read_with(cx, |dialog, _| dialog.title.clone());
    title.update_in(cx, |editor, window, cx| editor.set_text(text, window, cx));
    cx.run_until_parked();
}

fn submit(dialog: &Entity<CreateProjectDialog>, cx: &mut VisualTestContext) {
    dialog.update(cx, |dialog, cx| dialog.submit(cx));
    cx.run_until_parked();
}

#[gpui::test]
async fn a_personal_repository_offers_only_the_signed_in_account(cx: &mut TestAppContext) {
    let (_view, dialog, cx) = open_dialog(Arc::new(backend()), writable(), cx).await;
    let choices = dialog.read_with(cx, |dialog, _| dialog.owner_choices());
    assert_eq!(choices, vec![(OwnerChoice::Viewer, "octocat".to_string())]);
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.target_line()),
        "@octocat"
    );
}

#[gpui::test]
async fn an_organization_repository_offers_the_account_and_the_organization(
    cx: &mut TestAppContext,
) {
    let fake = backend().with_project_owners(&repository(), organization_owners());
    let (_view, dialog, cx) = open_dialog(Arc::new(fake), writable(), cx).await;
    let choices = dialog.read_with(cx, |dialog, _| dialog.owner_choices());
    assert_eq!(
        choices,
        vec![
            (OwnerChoice::Viewer, "octocat".to_string()),
            (OwnerChoice::Organization, "octo".to_string())
        ]
    );
    assert!(
        cx.debug_bounds("github-create-project-owners").is_some(),
        "the owner selector is drawn"
    );
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.owner()),
        OwnerChoice::Organization,
        "a project for an organization's repository starts under the organization"
    );
}

#[gpui::test]
async fn creating_sends_the_owner_picked_the_trimmed_title_and_the_repository(
    cx: &mut TestAppContext,
) {
    let fake = Arc::new(backend().with_project_owners(&repository(), organization_owners()));
    let (_view, dialog, cx) = open_dialog(fake.clone(), writable(), cx).await;
    dialog.update(cx, |dialog, cx| {
        dialog.select_owner(OwnerChoice::Organization, cx)
    });
    type_title(&dialog, "  Launch plan  ", cx);
    submit(&dialog, cx);
    assert_eq!(
        fake.writes(),
        vec![FakeWrite::CreateProject(
            "O_octo".into(),
            "Launch plan".into(),
            "R_octo_repo".into()
        )]
    );
}

#[gpui::test]
async fn an_empty_blank_or_too_long_title_cannot_be_created(cx: &mut TestAppContext) {
    let fake = Arc::new(backend());
    let (_view, dialog, cx) = open_dialog(fake.clone(), writable(), cx).await;
    let can_create = |dialog: &Entity<CreateProjectDialog>, cx: &mut VisualTestContext| {
        dialog.read_with(cx, |dialog, cx| dialog.can_create(cx))
    };
    assert!(!can_create(&dialog, cx), "nothing typed yet");
    type_title(&dialog, "   ", cx);
    assert!(!can_create(&dialog, cx), "only spaces");
    submit(&dialog, cx);
    assert_eq!(fake.writes(), Vec::new(), "a blank title is not sent");
    type_title(&dialog, &"x".repeat(MAX_PROJECT_TITLE_CHARS + 1), cx);
    assert!(!can_create(&dialog, cx), "longer than GitHub allows");
    type_title(&dialog, &"x".repeat(MAX_PROJECT_TITLE_CHARS), cx);
    assert!(can_create(&dialog, cx), "exactly as long as GitHub allows");
}

#[gpui::test]
async fn githubs_refusal_is_shown_in_the_dialog_and_the_form_is_kept(cx: &mut TestAppContext) {
    let fake = Arc::new(backend());
    let (_view, dialog, cx) = open_dialog(fake.clone(), writable(), cx).await;
    let message = "Members of this organization cannot create projects.";
    fake.fail_next_project_write(GithubError::Forbidden {
        message: message.into(),
    });
    type_title(&dialog, "Launch", cx);
    submit(&dialog, cx);
    let (error, title, can_create) = dialog.read_with(cx, |dialog, cx| {
        (
            dialog.error().map(str::to_string),
            dialog.title_text(cx),
            dialog.can_create(cx),
        )
    });
    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains(message)),
        "{error:?}"
    );
    assert_eq!(title, "Launch");
    assert!(can_create, "it can be tried again");
    assert!(cx.debug_bounds("github-create-project-error").is_some());
}

#[gpui::test]
async fn without_the_project_scope_the_dialog_guides_and_sends_nothing(cx: &mut TestAppContext) {
    let fake = Arc::new(backend());
    let read_only = OAuthScopes::Granted(vec!["repo".into(), "read:project".into()]);
    let (_view, dialog, cx) = open_dialog(fake.clone(), read_only, cx).await;
    type_title(&dialog, "Launch", cx);
    submit(&dialog, cx);
    assert!(dialog.read_with(cx, |dialog, _| dialog.missing_scope()));
    assert_eq!(fake.writes(), Vec::new());
    assert!(
        cx.debug_bounds("github-create-project-guidance").is_some(),
        "the guidance is drawn in place of the form"
    );
}

#[gpui::test]
async fn a_scope_refusal_from_github_turns_the_dialog_into_the_guidance(cx: &mut TestAppContext) {
    let fake = Arc::new(backend());
    let (_view, dialog, cx) = open_dialog(fake.clone(), OAuthScopes::Unknown, cx).await;
    fake.fail_next_project_write(GithubError::MissingScope {
        scope: "project".into(),
    });
    type_title(&dialog, "Launch", cx);
    submit(&dialog, cx);
    assert!(dialog.read_with(cx, |dialog, _| dialog.missing_scope()));
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.error().map(str::to_string)),
        None
    );
}

#[gpui::test]
async fn a_dialog_whose_session_ended_refuses_to_create(cx: &mut TestAppContext) {
    let fake = Arc::new(backend());
    let (view, dialog, cx) = open_dialog(fake.clone(), writable(), cx).await;
    type_title(&dialog, "Launch", cx);
    cx.update(|_, cx| release_session(view.entity_id(), cx));
    submit(&dialog, cx);
    assert_eq!(fake.writes(), Vec::new());
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.error().map(str::to_string)),
        Some(session_ended_message("dialog"))
    );
}

#[gpui::test]
async fn a_created_project_is_picked_and_the_dialog_closes(cx: &mut TestAppContext) {
    let fake = Arc::new(backend());
    let (workspace, view, cx) = projects_tab(fake.clone(), writable(), cx).await;
    let section = section(&view, cx);
    assert_eq!(
        section.read_with(cx, |section, _| section
            .selected_project()
            .map(|project| project.title.clone())),
        Some("Roadmap".to_string())
    );
    section.update_in(cx, |section, window, cx| {
        section.open_create_project(window, cx)
    });
    cx.run_until_parked();
    let dialog = active_dialog(&workspace, cx).expect("the dialog is open");
    type_title(&dialog, "Launch", cx);
    let reads_before = fake.project_requests();
    submit(&dialog, cx);
    assert!(active_dialog(&workspace, cx).is_none(), "the dialog closes");
    let (titles, selected) = section.read_with(cx, |section, _| {
        (
            section
                .projects()
                .iter()
                .map(|project| project.title.clone())
                .collect::<Vec<_>>(),
            section
                .selected_project()
                .map(|project| project.title.clone()),
        )
    });
    assert_eq!(titles, vec!["Roadmap".to_string(), "Launch".to_string()]);
    assert_eq!(selected, Some("Launch".to_string()));
    assert!(
        fake.project_requests() > reads_before,
        "the projects are read again"
    );
}

#[gpui::test]
async fn an_empty_repository_offers_to_create_its_first_project(cx: &mut TestAppContext) {
    let (_workspace, _view, cx) =
        projects_tab(Arc::new(FakeGithubBackend::new()), writable(), cx).await;
    assert!(
        cx.debug_bounds("github-projects-empty").is_some(),
        "the empty state offers Create project"
    );
}
