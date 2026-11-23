//! What the issue tests share: an app with the GitHub tab's test environment
//! and a window holding a workspace.

use std::sync::Arc;

use fs::FakeFs;
use github::{GithubBackend, OAuthScopes, RepositoryId, TokenSource};
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use project::Project;
use settings::{Settings as _, SettingsStore};
use workspace::{MultiWorkspace, Workspace};

use crate::github_view::{
    AuthState, GithubContext, GithubSection, GithubView, ProjectAccess, Session,
};
use crate::issues::IssueDetailView;

pub(crate) fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let settings_store = SettingsStore::test(cx);
        cx.set_global(settings_store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        project::DisableAiSettings::register(cx);
        // The search field is an editor; its factory is installed here.
        editor::init(cx);
        crate::init(cx);
        crate::use_test_environment(cx);
        crate::github_view_session::hold_test_session(cx);
    });
}

pub(crate) fn repository() -> RepositoryId {
    RepositoryId::new("octo", "repo").expect("a valid repository")
}

pub(crate) fn context(backend: Arc<dyn GithubBackend>) -> GithubContext {
    GithubContext {
        backend,
        repository: repository(),
        login: "octocat".into(),
        access: ProjectAccess::Owner,
        session: crate::github_view_session::test_session(),
    }
}

pub(crate) async fn workspace_window(
    cx: &mut TestAppContext,
) -> (Entity<Workspace>, &mut VisualTestContext) {
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace =
        multi_workspace.read_with(cx, |multi_workspace, _| multi_workspace.workspace().clone());
    (workspace, cx)
}

pub(crate) fn detail_tabs(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Vec<Entity<IssueDetailView>> {
    workspace.read_with(cx, |workspace, cx| {
        workspace.items_of_type::<IssueDetailView>(cx).collect()
    })
}

/// A GitHub tab in a new workspace, signed in as `octocat` through
/// `backend` with `scopes`, showing `section` for [`repository`].
pub(crate) async fn signed_in_github_tab(
    backend: Arc<dyn GithubBackend>,
    scopes: OAuthScopes,
    section: GithubSection,
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<GithubView>,
    &mut VisualTestContext,
) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let project = workspace.read_with(cx, |workspace, _| workspace.project().clone());
    let view = workspace.update_in(cx, |workspace, window, cx| {
        let view = cx.new(|cx| GithubView::new(project, cx));
        workspace.add_item_to_active_pane(Box::new(view.clone()), None, true, window, cx);
        view
    });
    view.update(cx, |view, cx| {
        view.active_repository = Some(repository());
        view.section = section;
        view.set_auth(
            AuthState::SignedIn(Session {
                login: "octocat".into(),
                token_source: TokenSource::GhCli,
                backend,
                scopes,
            }),
            cx,
        );
    });
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    (workspace, view, cx)
}
