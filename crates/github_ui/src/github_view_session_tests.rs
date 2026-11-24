use std::path::Path;
use std::sync::Arc;

use fs::FakeFs;
use github::{
    FakeGithubBackend, GithubBackend, GithubError, IssueState, RepositoryId, SignInStatus,
    StateReason, fake_issue,
};
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use project::Project;
use workspace::{MultiWorkspace, Workspace};

use crate::github_view::{AuthState, DetailKey, GithubSection, GithubView};
use crate::github_view_session::{end_test_session, session_ended_message};
use crate::github_view_tests::{
    Setup, auth, draw, github_tab, install, press_toggle, status, workspace_over,
};
use crate::issues::issues_test_support::{self, context, detail_tabs, workspace_window};
use crate::issues::{IssueDetailView, IssueList, open_issue_detail};
use crate::sign_in::{GithubEnvironment, TokenStorage};
use crate::test_support::{FakeGhHost, MemoryKeychain, ScriptedHttp};

fn repository() -> RepositoryId {
    RepositoryId::new("octo", "repo").expect("a valid repository")
}

fn with_issues(_: &github::GithubToken) -> FakeGithubBackend {
    FakeGithubBackend::new()
        .with_issue(&repository(), fake_issue(1, "First"))
        .with_issue(&repository(), fake_issue(2, "Second"))
}

async fn signed_in_tab(
    setup: Setup,
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<GithubView>,
    &mut VisualTestContext,
) {
    install(setup, cx);
    let fs = FakeFs::new(cx.executor());
    let (workspace, cx) = workspace_over(fs, &[] as &[&Path], cx).await;
    press_toggle(cx);
    let view = github_tab(&workspace, cx);
    (workspace, view, cx)
}

fn keychain_setup() -> Setup {
    Setup {
        keychain: MemoryKeychain::holding("gho_work"),
        gh: FakeGhHost::signed_in("gho_personal"),
        backend: with_issues,
        ..Setup::default()
    }
}

fn open_detail(
    workspace: &Entity<Workspace>,
    github: &Entity<GithubView>,
    backend: Arc<dyn GithubBackend>,
    number: u64,
    cx: &mut VisualTestContext,
) -> Entity<IssueDetailView> {
    let key = DetailKey {
        repository: repository(),
        number,
    };
    let session = github
        .read_with(cx, |github, _| github.session_stamp())
        .expect("the GitHub tab is signed in");
    let view = workspace.update_in(cx, |workspace, window, cx| {
        open_issue_detail(workspace, backend, session, key, window, cx)
    });
    cx.run_until_parked();
    view
}

fn session_backend(
    view: &Entity<GithubView>,
    cx: &mut VisualTestContext,
) -> Arc<dyn GithubBackend> {
    match auth(view, cx) {
        AuthState::SignedIn(session) => session.backend,
        _ => panic!("expected to be signed in"),
    }
}

#[gpui::test]
async fn after_disconnect_a_detail_tab_refuses_to_write(cx: &mut TestAppContext) {
    let (workspace, view, cx) = signed_in_tab(keychain_setup(), cx).await;
    let fake = Arc::new(with_issues_fake());
    let detail = open_detail(&workspace, &view, fake.clone(), 1, cx);
    detail.update_in(cx, |detail, window, cx| {
        let editor = detail.comment_editor(window, cx);
        editor.update(cx, |editor, cx| {
            editor.set_text("internal notes", window, cx)
        });
    });

    view.update(cx, |view, cx| view.disconnect(cx));
    cx.run_until_parked();
    detail.update_in(cx, |detail, window, cx| {
        detail.send_comment(window, cx);
        detail.add_label("bug".to_string(), window, cx);
        detail.close(StateReason::Completed, window, cx);
    });
    cx.run_until_parked();
    draw(cx);

    assert_eq!(fake.writes(), Vec::new(), "the old backend writes nothing");
    detail.read_with(cx, |detail, cx| {
        assert!(detail.session_ended(cx));
        assert_eq!(
            detail.write_error(),
            Some(session_ended_message("issue").as_str())
        );
    });
}

fn with_issues_fake() -> FakeGithubBackend {
    FakeGithubBackend::new()
        .with_issue(&repository(), fake_issue(1, "First"))
        .with_issue(&repository(), fake_issue(2, "Second"))
}

#[gpui::test]
async fn after_an_account_change_the_same_issue_opens_with_the_new_backend(
    cx: &mut TestAppContext,
) {
    let (workspace, view, cx) = signed_in_tab(keychain_setup(), cx).await;
    let work = session_backend(&view, cx);
    let old_tab = open_detail(&workspace, &view, work.clone(), 1, cx);

    view.update(cx, |view, cx| view.use_github_cli(cx));
    cx.run_until_parked();
    let personal = session_backend(&view, cx);
    assert!(!Arc::ptr_eq(&work, &personal));

    let new_tab = open_detail(&workspace, &view, personal.clone(), 1, cx);
    assert_ne!(
        new_tab.entity_id(),
        old_tab.entity_id(),
        "the tab from the old account is replaced, not reused"
    );
    assert!(new_tab.read_with(cx, |tab, _| Arc::ptr_eq(&tab.backend, &personal)));
    let tabs = workspace.read_with(cx, |workspace, cx| {
        workspace.items_of_type::<IssueDetailView>(cx).count()
    });
    assert_eq!(tabs, 1, "the stale tab is closed");
}

#[gpui::test]
async fn a_stale_tab_reopens_with_the_current_account(cx: &mut TestAppContext) {
    let (workspace, view, cx) = signed_in_tab(keychain_setup(), cx).await;
    let work = session_backend(&view, cx);
    let old_tab = open_detail(&workspace, &view, work, 1, cx);
    view.update(cx, |view, cx| view.use_github_cli(cx));
    cx.run_until_parked();
    let personal = session_backend(&view, cx);

    old_tab.update_in(cx, |tab, window, cx| {
        tab.reopen_in_current_session(window, cx)
    });
    cx.run_until_parked();
    let tabs = workspace.read_with(cx, |workspace, cx| {
        workspace
            .items_of_type::<IssueDetailView>(cx)
            .collect::<Vec<_>>()
    });
    assert_eq!(tabs.len(), 1);
    assert!(tabs[0].read_with(cx, |tab, _| Arc::ptr_eq(&tab.backend, &personal)));
}

#[gpui::test]
async fn a_token_refused_in_a_detail_tab_signs_the_github_tab_out(cx: &mut TestAppContext) {
    let (workspace, view, cx) = signed_in_tab(keychain_setup(), cx).await;
    assert!(matches!(status(cx), SignInStatus::SignedIn { .. }));
    let refused = Arc::new(with_issues_fake().failing_with(GithubError::TokenRevoked));
    open_detail(&workspace, &view, refused, 1, cx);

    assert!(matches!(auth(&view, cx), AuthState::Revoked { .. }));
    assert_eq!(status(cx), SignInStatus::Revoked);
}

#[gpui::test]
async fn a_token_refused_on_a_row_write_signs_the_github_tab_out(cx: &mut TestAppContext) {
    let (_workspace, view, cx) = signed_in_tab(keychain_setup(), cx).await;
    view.update(cx, |view, cx| {
        view.active_repository = Some(repository());
        view.select_section(GithubSection::Issues, cx);
    });
    cx.run_until_parked();
    let list = view
        .read_with(cx, |view, _| view.issue_list().cloned())
        .expect("the issue list is shown");
    let refused = Arc::new(with_issues_fake());
    list.update(cx, |list, _| list.context.backend = refused.clone());
    refused.set_failure(Some(GithubError::TokenRevoked));
    list.update(cx, |list, cx| {
        list.set_issue_state(1, IssueState::Closed, Some(StateReason::Completed), cx)
    });
    cx.run_until_parked();

    assert!(matches!(auth(&view, cx), AuthState::Revoked { .. }));
    assert_eq!(status(cx), SignInStatus::Revoked);
}

#[gpui::test]
async fn reload_on_the_same_account_keeps_the_selection_and_draft(cx: &mut TestAppContext) {
    let (_workspace, view, cx) = signed_in_tab(keychain_setup(), cx).await;
    view.update(cx, |view, cx| {
        view.active_repository = Some(repository());
        view.select_section(GithubSection::Issues, cx);
    });
    cx.run_until_parked();
    let list: Entity<IssueList> = view
        .read_with(cx, |view, _| view.issue_list().cloned())
        .expect("the issue list is shown");
    list.update(cx, |list, cx| {
        list.toggle_selected(2, cx);
        list.saved_draft.title = "half-written".to_string();
    });

    view.update(cx, |view, cx| view.reload(cx));
    cx.run_until_parked();

    let after = view
        .read_with(cx, |view, _| view.issue_list().cloned())
        .expect("still shown");
    assert_eq!(after.entity_id(), list.entity_id(), "the same list");
    after.read_with(cx, |list, _| {
        assert!(list.selected().contains(&2));
        assert_eq!(list.saved_draft.title, "half-written");
        assert_eq!(list.rows().len(), 2, "read again");
    });
}

#[gpui::test]
async fn an_unreadable_keychain_token_can_be_disconnected(cx: &mut TestAppContext) {
    let (_workspace, view, cx) = signed_in_tab(
        Setup {
            keychain: MemoryKeychain::holding("not a token at all"),
            gh: FakeGhHost::signed_in("gho_personal"),
            ..Setup::default()
        },
        cx,
    )
    .await;
    assert!(matches!(
        auth(&view, cx),
        AuthState::Failed(GithubError::Keychain { .. })
    ));
    assert!(view.read_with(cx, |view, _| view.offers_disconnect()));
    draw(cx);

    view.update(cx, |view, cx| view.disconnect(cx));
    cx.run_until_parked();
    assert!(matches!(auth(&view, cx), AuthState::SignedOut { .. }));
}

#[test]
fn a_refused_environment_token_points_at_the_variable_not_gh_auth_login() {
    let advice = crate::github_view_render::revoked_advice(github::TokenSource::GhEnvironment(
        github::TokenVariable::GhToken,
    ));
    assert!(advice.contains("GH_TOKEN"), "{advice}");
    assert!(!advice.contains("gh auth login"), "{advice}");
    let cli = crate::github_view_render::revoked_advice(github::TokenSource::GhCli);
    assert!(cli.contains("gh auth login"), "{cli}");
}

/// What a second window finds: its own keychain and `gh`, the same fake
/// GitHub.
fn environment(keychain: MemoryKeychain, gh: FakeGhHost) -> GithubEnvironment {
    GithubEnvironment {
        token_storage: TokenStorage::Keychain,
        credentials: Arc::new(keychain),
        gh_host: Arc::new(gh),
        http_client: ScriptedHttp::default().client(),
        make_backend: Arc::new(|token, _cx| Arc::new(with_issues(&token))),
    }
}

/// Another window over another project, its GitHub tab opened with
/// `environment`.
async fn second_window(
    environment: GithubEnvironment,
    cx: &mut VisualTestContext,
) -> (Entity<Workspace>, Entity<GithubView>, VisualTestContext) {
    cx.update(|_, cx| cx.set_global(environment));
    let app: &mut TestAppContext = cx;
    let fs = FakeFs::new(app.executor());
    let project = Project::test(fs, [] as [&Path; 0], app).await;
    let window = app.add_window(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace = window
        .read_with(app, |multi_workspace, _| {
            multi_workspace.workspace().clone()
        })
        .expect("the window is open");
    let mut other_cx = VisualTestContext::from_window(window.into(), app);
    press_toggle(&mut other_cx);
    let view = github_tab(&workspace, &mut other_cx);
    (workspace, view, other_cx)
}

fn send_comment(detail: &Entity<IssueDetailView>, cx: &mut VisualTestContext) {
    detail.update_in(cx, |detail, window, cx| {
        let editor = detail.comment_editor(window, cx);
        editor.update(cx, |editor, cx| editor.set_text("A note", window, cx));
        detail.send_comment(window, cx);
    });
    cx.run_until_parked();
}

#[gpui::test]
async fn a_failed_check_or_cancelled_sign_in_elsewhere_keeps_this_windows_tabs(
    cx: &mut TestAppContext,
) {
    let (workspace, view, cx) = signed_in_tab(keychain_setup(), cx).await;
    let fake = Arc::new(with_issues_fake());
    let detail = open_detail(&workspace, &view, fake.clone(), 1, cx);

    let (_other_workspace, other, mut other_cx) = second_window(
        environment(
            MemoryKeychain::holding("not a token at all"),
            FakeGhHost::missing(),
        ),
        cx,
    )
    .await;
    assert!(matches!(auth(&other, &mut other_cx), AuthState::Failed(_)));
    other.update(&mut other_cx, |other, cx| {
        other.sign_in_with_github(cx);
        other.cancel_sign_in(cx);
    });
    other_cx.run_until_parked();

    send_comment(&detail, cx);
    assert!(!detail.read_with(cx, |detail, cx| detail.session_ended(cx)));
    assert_eq!(fake.writes().len(), 1, "this window's tab still writes");
    assert!(matches!(auth(&view, cx), AuthState::SignedIn(_)));
}

#[gpui::test]
async fn disconnecting_one_window_ends_only_that_windows_tabs(cx: &mut TestAppContext) {
    let (workspace, view, cx) = signed_in_tab(keychain_setup(), cx).await;
    let fake = Arc::new(with_issues_fake());
    let detail = open_detail(&workspace, &view, fake.clone(), 1, cx);
    let (other_workspace, other, mut other_cx) = second_window(
        environment(
            MemoryKeychain::default(),
            FakeGhHost::signed_in("gho_personal"),
        ),
        cx,
    )
    .await;
    assert!(matches!(
        auth(&other, &mut other_cx),
        AuthState::SignedIn(_)
    ));
    let other_fake = Arc::new(with_issues_fake());
    let other_detail = open_detail(
        &other_workspace,
        &other,
        other_fake.clone(),
        1,
        &mut other_cx,
    );

    view.update(cx, |view, cx| view.disconnect(cx));
    cx.run_until_parked();
    send_comment(&detail, cx);
    send_comment(&other_detail, &mut other_cx);

    assert!(detail.read_with(cx, |detail, cx| detail.session_ended(cx)));
    assert_eq!(
        fake.writes(),
        Vec::new(),
        "the disconnected tab writes nothing"
    );
    assert!(!other_detail.read_with(&mut other_cx, |detail, cx| detail.session_ended(cx)));
    assert_eq!(
        other_fake.writes().len(),
        1,
        "the other account still writes"
    );
    assert!(matches!(
        auth(&other, &mut other_cx),
        AuthState::SignedIn(_)
    ));
}

#[gpui::test]
async fn a_token_refused_in_one_window_leaves_the_other_signed_in(cx: &mut TestAppContext) {
    let (_workspace, view, cx) = signed_in_tab(keychain_setup(), cx).await;
    let (other_workspace, other, mut other_cx) = second_window(
        environment(
            MemoryKeychain::default(),
            FakeGhHost::signed_in("gho_personal"),
        ),
        cx,
    )
    .await;
    assert!(matches!(
        auth(&other, &mut other_cx),
        AuthState::SignedIn(_)
    ));

    let refused = Arc::new(with_issues_fake().failing_with(GithubError::TokenRevoked));
    open_detail(&other_workspace, &other, refused, 1, &mut other_cx);
    cx.run_until_parked();

    assert!(matches!(
        auth(&other, &mut other_cx),
        AuthState::Revoked { .. }
    ));
    assert!(
        matches!(auth(&view, cx), AuthState::SignedIn(_)),
        "another token is not refused with it"
    );
}

#[gpui::test]
async fn a_tab_opened_from_a_list_of_an_ended_session_refuses_to_write(cx: &mut TestAppContext) {
    issues_test_support::init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(with_issues_fake());
    let list = cx.new(|cx| {
        let mut list = IssueList::new(context(fake.clone()));
        list.reload(cx);
        list
    });
    cx.run_until_parked();

    cx.update(|_, cx| end_test_session(cx));
    list.update_in(cx, |list, window, cx| list.open_issue(1, window, cx));
    cx.run_until_parked();
    let tab = detail_tabs(&workspace, cx)
        .first()
        .cloned()
        .expect("the issue opened");
    send_comment(&tab, cx);

    assert!(tab.read_with(cx, |tab, cx| tab.session_ended(cx)));
    assert_eq!(
        fake.writes(),
        Vec::new(),
        "the old session's tab writes nothing"
    );
}

/// The stamp a GitHub tab holds, which says which token it holds.
fn stamp(
    view: &Entity<GithubView>,
    cx: &mut VisualTestContext,
) -> crate::github_view_session::SessionStamp {
    view.read_with(cx, |view, _| view.session_stamp())
        .expect("the GitHub tab is signed in")
}

/// A second window whose keychain holds the token this window signed in
/// with: the same account, the same token.
async fn second_window_with_the_same_token(
    cx: &mut VisualTestContext,
) -> (Entity<Workspace>, Entity<GithubView>, VisualTestContext) {
    second_window(
        environment(MemoryKeychain::holding("gho_work"), FakeGhHost::missing()),
        cx,
    )
    .await
}

#[gpui::test]
async fn a_token_refused_in_one_window_refuses_it_in_another_holding_it(cx: &mut TestAppContext) {
    let (workspace, view, cx) = signed_in_tab(keychain_setup(), cx).await;
    let fake = Arc::new(with_issues_fake());
    let detail = open_detail(&workspace, &view, fake.clone(), 1, cx);
    let (other_workspace, other, mut other_cx) = second_window_with_the_same_token(cx).await;
    assert!(matches!(
        auth(&other, &mut other_cx),
        AuthState::SignedIn(_)
    ));
    let (mine, theirs) = (stamp(&view, cx), stamp(&other, &mut other_cx));
    assert_ne!(mine, theirs, "each window holds its own session");

    let refused = Arc::new(with_issues_fake().failing_with(GithubError::TokenRevoked));
    open_detail(&other_workspace, &other, refused, 1, &mut other_cx);
    cx.run_until_parked();

    assert!(matches!(
        auth(&other, &mut other_cx),
        AuthState::Revoked { .. }
    ));
    assert!(
        matches!(auth(&view, cx), AuthState::Revoked { .. }),
        "the same token is refused here too"
    );
    send_comment(&detail, cx);
    assert!(detail.read_with(cx, |detail, cx| detail.session_ended(cx)));
    assert_eq!(
        fake.writes(),
        Vec::new(),
        "the refused token writes nothing"
    );
}

#[gpui::test]
async fn disconnecting_one_window_signs_out_another_holding_the_same_token(
    cx: &mut TestAppContext,
) {
    let (workspace, view, cx) = signed_in_tab(keychain_setup(), cx).await;
    let fake = Arc::new(with_issues_fake());
    let detail = open_detail(&workspace, &view, fake.clone(), 1, cx);
    let (other_workspace, other, mut other_cx) = second_window_with_the_same_token(cx).await;
    let other_fake = Arc::new(with_issues_fake());
    let other_detail = open_detail(
        &other_workspace,
        &other,
        other_fake.clone(),
        1,
        &mut other_cx,
    );

    view.update(cx, |view, cx| view.disconnect(cx));
    cx.run_until_parked();
    send_comment(&detail, cx);
    send_comment(&other_detail, &mut other_cx);

    assert!(detail.read_with(cx, |detail, cx| detail.session_ended(cx)));
    assert_eq!(
        fake.writes(),
        Vec::new(),
        "the disconnected tab writes nothing"
    );
    assert!(other_detail.read_with(&mut other_cx, |detail, cx| detail.session_ended(cx)));
    assert_eq!(
        other_fake.writes(),
        Vec::new(),
        "the token is disconnected, so the other window's tab writes nothing either"
    );
    assert!(matches!(
        auth(&other, &mut other_cx),
        AuthState::SignedOut { .. }
    ));
}
