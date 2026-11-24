use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use fs::FakeFs;
use github::{
    FakeGithubBackend, GithubClientId, GithubError, GithubToken, RemoteName, RepositoryId,
    RepositoryVisibility, SignInStatus, TokenSource, sign_in_status, sign_in_with_github_available,
};
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use project::Project;
use serde_json::json;
use settings::{Settings as _, SettingsStore};
use workspace::item::test::TestItem;
use workspace::{MultiWorkspace, Workspace};

use crate::device_flow::{ACCESS_TOKEN_URL, DEVICE_CODE_URL};
use crate::github_view::{AuthState, DeviceFlowStep, GithubView, ProjectAccess};
use crate::sign_in::{GithubEnvironment, TokenStorage, session_token};
use crate::test_support::{FakeGhHost, MemoryKeychain, ScriptedHttp};

const CLIENT_ID: &str = "Iv1.zodetest";
const DEVICE_TOKEN: &str = "gho_from_the_device_flow";

pub(crate) struct Setup {
    pub client_id: Option<&'static str>,
    pub storage: TokenStorage,
    pub keychain: MemoryKeychain,
    pub gh: FakeGhHost,
    pub backend: fn(&GithubToken) -> FakeGithubBackend,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            client_id: Some(CLIENT_ID),
            storage: TokenStorage::Keychain,
            keychain: MemoryKeychain::default(),
            gh: FakeGhHost::missing(),
            backend: |_| FakeGithubBackend::new(),
        }
    }
}

pub(crate) struct Harness {
    keychain: Arc<MemoryKeychain>,
    gh: Arc<FakeGhHost>,
    http: ScriptedHttp,
    /// Every token a backend was built with: the only way a request to
    /// api.github.com could be made.
    built: Arc<Mutex<Vec<GithubToken>>>,
}

impl Harness {
    fn built(&self) -> Vec<GithubToken> {
        self.built
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

pub(crate) fn install(setup: Setup, cx: &mut TestAppContext) -> Harness {
    let keychain = Arc::new(setup.keychain);
    let gh = Arc::new(setup.gh);
    let http = ScriptedHttp::default();
    let built: Arc<Mutex<Vec<GithubToken>>> = Arc::default();
    let backend = setup.backend;
    let client_id = setup.client_id.map(str::to_string);
    let environment = GithubEnvironment {
        token_storage: setup.storage,
        credentials: keychain.clone(),
        gh_host: gh.clone(),
        http_client: http.client(),
        make_backend: Arc::new({
            let built = built.clone();
            move |token, _cx| {
                let fake = backend(&token);
                built
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(token);
                Arc::new(fake)
            }
        }),
    };
    cx.update(|cx| {
        let settings_store = SettingsStore::test(cx);
        cx.set_global(settings_store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        project::DisableAiSettings::register(cx);
        // The issue list's search field is an editor.
        editor::init(cx);
        crate::init(cx);
        cx.set_global(environment);
        cx.set_global(GithubClientId(client_id));
    });
    Harness {
        keychain,
        gh,
        http,
        built,
    }
}

pub(crate) async fn workspace_over<'a>(
    fs: Arc<FakeFs>,
    roots: &[&Path],
    cx: &'a mut TestAppContext,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    let project = Project::test(fs, roots.iter().copied(), cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace =
        multi_workspace.read_with(cx, |multi_workspace, _| multi_workspace.workspace().clone());
    (workspace, cx)
}

/// Through the window, the way the rail button sends it.
pub(crate) fn press_toggle(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.dispatch_action(Box::new(zode_actions::github::Toggle), cx));
    cx.run_until_parked();
}

pub(crate) fn github_tab(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Entity<GithubView> {
    workspace
        .read_with(cx, |workspace, cx| {
            workspace.items_of_type::<GithubView>(cx).next()
        })
        .expect("the GitHub tab is open")
}

async fn opened_tab(cx: &mut TestAppContext) -> (Entity<GithubView>, &mut VisualTestContext) {
    let fs = FakeFs::new(cx.executor());
    let (workspace, cx) = workspace_over(fs, &[], cx).await;
    press_toggle(cx);
    let view = github_tab(&workspace, cx);
    (view, cx)
}

/// A real frame, so each state's `Render` runs rather than only its data.
pub(crate) fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

pub(crate) fn auth(view: &Entity<GithubView>, cx: &mut VisualTestContext) -> AuthState {
    view.read_with(cx, |view, _| view.auth().clone())
}

fn advance(seconds: u64, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_secs(seconds));
    cx.run_until_parked();
}

fn device_code_answer() -> String {
    json!({
        "device_code": "device-secret",
        "user_code": "WDJB-MJHT",
        "verification_uri": "https://github.com/login/device",
        "expires_in": 900,
        "interval": 5
    })
    .to_string()
}

fn error_answer(error: &str) -> String {
    json!({ "error": error }).to_string()
}

fn token_answer() -> String {
    json!({ "access_token": DEVICE_TOKEN, "token_type": "bearer", "scope": "project,read:org,repo" })
        .to_string()
}

#[gpui::test]
async fn the_rail_action_opens_one_tab_and_brings_it_back(cx: &mut TestAppContext) {
    install(Setup::default(), cx);
    let fs = FakeFs::new(cx.executor());
    let (workspace, cx) = workspace_over(fs, &[], cx).await;

    press_toggle(cx);
    let github = github_tab(&workspace, cx);
    workspace.update_in(cx, |workspace, window, cx| {
        let other = cx.new(TestItem::new);
        workspace.add_item_to_active_pane(Box::new(other), None, true, window, cx);
    });
    press_toggle(cx);

    workspace.read_with(cx, |workspace, cx| {
        assert_eq!(workspace.items_of_type::<GithubView>(cx).count(), 1);
        let active = workspace.active_item(cx).expect("an active item");
        assert_eq!(
            active.item_id(),
            github.entity_id(),
            "the second press focuses it"
        );
    });

    press_toggle(cx);
    workspace.read_with(cx, |workspace, cx| {
        let active = workspace.active_item(cx).expect("an active item");
        assert_ne!(
            active.item_id(),
            github.entity_id(),
            "a third press puts it away"
        );
        assert_eq!(workspace.items_of_type::<GithubView>(cx).count(), 1);
    });
}

#[gpui::test]
async fn without_a_token_nothing_is_sent(cx: &mut TestAppContext) {
    let harness = install(Setup::default(), cx);
    let (view, cx) = opened_tab(cx).await;
    draw(cx);

    assert!(matches!(
        auth(&view, cx),
        AuthState::SignedOut { notice: None }
    ));
    assert!(harness.built().is_empty(), "no backend without a token");
    assert!(harness.http.sent().is_empty(), "no request without a token");
}

#[gpui::test]
async fn the_device_flow_slows_down_then_keeps_the_token_in_the_keychain(cx: &mut TestAppContext) {
    let harness = install(Setup::default(), cx);
    harness
        .http
        .answer(DEVICE_CODE_URL, 200, device_code_answer());
    harness
        .http
        .answer(ACCESS_TOKEN_URL, 200, error_answer("authorization_pending"));
    harness
        .http
        .answer(ACCESS_TOKEN_URL, 200, error_answer("slow_down"));
    harness.http.answer(ACCESS_TOKEN_URL, 200, token_answer());
    let (view, cx) = opened_tab(cx).await;

    view.update(cx, |view, cx| view.sign_in_with_github(cx));
    cx.run_until_parked();
    assert_eq!(
        auth_step(&view, cx),
        Some(DeviceFlowStep::WaitingForUser {
            user_code: "WDJB-MJHT".into(),
            verification_uri: "https://github.com/login/device".into(),
        })
    );
    draw(cx);
    let sent = harness.http.sent();
    assert_eq!(
        sent.len(),
        1,
        "only the code is asked for before the first wait"
    );
    assert_eq!(sent[0].accept.as_deref(), Some("application/json"));
    assert_eq!(
        sent[0].body,
        format!("client_id={CLIENT_ID}&scope=repo+read%3Aorg+project")
    );

    advance(5, cx);
    assert_eq!(harness.http.sent().len(), 2, "first poll: pending");
    advance(5, cx);
    assert_eq!(harness.http.sent().len(), 3, "second poll: slow down");
    advance(5, cx);
    assert_eq!(
        harness.http.sent().len(),
        3,
        "the interval grew by five seconds"
    );
    advance(5, cx);
    let sent = harness.http.sent();
    assert_eq!(sent.len(), 4, "third poll: the token");
    assert!(
        sent[1]
            .body
            .contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code")
    );

    assert_eq!(
        harness.keychain.github_token().as_deref(),
        Some(DEVICE_TOKEN)
    );
    cx.update(|_, cx| assert!(session_token(cx).is_none()));
    let AuthState::SignedIn(session) = auth(&view, cx) else {
        panic!("expected to be signed in");
    };
    assert_eq!(session.login.as_ref(), "octocat");
    assert_eq!(session.token_source, TokenSource::Keychain);
    let built = harness.built();
    assert_eq!(built.len(), 1);
    assert_eq!(
        built[0].identity(),
        GithubToken::new(DEVICE_TOKEN, TokenSource::Keychain).identity()
    );
}

fn auth_step(view: &Entity<GithubView>, cx: &mut VisualTestContext) -> Option<DeviceFlowStep> {
    match auth(view, cx) {
        AuthState::SigningIn(step) => Some(step),
        _ => None,
    }
}

#[gpui::test]
async fn a_dev_build_keeps_the_device_flow_token_in_memory_only(cx: &mut TestAppContext) {
    let harness = install(
        Setup {
            storage: TokenStorage::MemoryOnly,
            ..Setup::default()
        },
        cx,
    );
    harness
        .http
        .answer(DEVICE_CODE_URL, 200, device_code_answer());
    harness.http.answer(ACCESS_TOKEN_URL, 200, token_answer());
    let (view, cx) = opened_tab(cx).await;

    view.update(cx, |view, cx| view.sign_in_with_github(cx));
    cx.run_until_parked();
    advance(5, cx);

    assert!(matches!(auth(&view, cx), AuthState::SignedIn(_)));
    assert_eq!(
        harness.keychain.github_token(),
        None,
        "nothing written to disk"
    );
    let kept = cx
        .update(|_, cx| session_token(cx))
        .expect("kept in memory");
    assert_eq!(
        kept.identity(),
        GithubToken::new(DEVICE_TOKEN, TokenSource::Keychain).identity()
    );

    // A reload finds it again without asking the keychain or `gh`.
    view.update(cx, |view, cx| view.reload(cx));
    cx.run_until_parked();
    assert!(matches!(auth(&view, cx), AuthState::SignedIn(_)));
    assert_eq!(harness.gh.runs(), 0);
}

#[gpui::test]
async fn a_denied_sign_in_says_so_and_stops(cx: &mut TestAppContext) {
    let harness = install(Setup::default(), cx);
    harness
        .http
        .answer(DEVICE_CODE_URL, 200, device_code_answer());
    harness
        .http
        .answer(ACCESS_TOKEN_URL, 200, error_answer("access_denied"));
    let (view, cx) = opened_tab(cx).await;

    view.update(cx, |view, cx| view.sign_in_with_github(cx));
    cx.run_until_parked();
    advance(5, cx);
    advance(30, cx);

    let AuthState::SignedOut {
        notice: Some(notice),
    } = auth(&view, cx)
    else {
        panic!("expected a signed-out notice");
    };
    assert!(notice.contains("denied"), "{notice}");
    assert_eq!(harness.http.sent().len(), 2, "no poll after the refusal");
    assert_eq!(harness.keychain.github_token(), None);
}

#[gpui::test]
async fn an_expired_code_says_so(cx: &mut TestAppContext) {
    let harness = install(Setup::default(), cx);
    harness
        .http
        .answer(DEVICE_CODE_URL, 200, device_code_answer());
    harness
        .http
        .answer(ACCESS_TOKEN_URL, 200, error_answer("expired_token"));
    let (view, cx) = opened_tab(cx).await;

    view.update(cx, |view, cx| view.sign_in_with_github(cx));
    cx.run_until_parked();
    advance(5, cx);

    let AuthState::SignedOut {
        notice: Some(notice),
    } = auth(&view, cx)
    else {
        panic!("expected a signed-out notice");
    };
    assert!(notice.contains("expired"), "{notice}");
}

#[gpui::test]
async fn cancelling_the_sign_in_stops_polling(cx: &mut TestAppContext) {
    let harness = install(Setup::default(), cx);
    harness
        .http
        .answer(DEVICE_CODE_URL, 200, device_code_answer());
    harness
        .http
        .answer(ACCESS_TOKEN_URL, 200, error_answer("authorization_pending"));
    let (view, cx) = opened_tab(cx).await;

    view.update(cx, |view, cx| view.sign_in_with_github(cx));
    cx.run_until_parked();
    advance(5, cx);
    view.update(cx, |view, cx| view.cancel_sign_in(cx));
    advance(60, cx);

    assert!(matches!(
        auth(&view, cx),
        AuthState::SignedOut { notice: None }
    ));
    assert_eq!(harness.http.sent().len(), 2);
}

/// A device-flow sign-in the user approved, its keychain write still in
/// flight: `decide` lets the write land (`true`) or fail (`false`).
async fn approved_with_the_save_held(
    cx: &mut TestAppContext,
) -> (
    Harness,
    Entity<GithubView>,
    futures::channel::oneshot::Sender<bool>,
    &mut VisualTestContext,
) {
    let harness = install(Setup::default(), cx);
    harness
        .http
        .answer(DEVICE_CODE_URL, 200, device_code_answer());
    harness.http.answer(ACCESS_TOKEN_URL, 200, token_answer());
    let decide = harness.keychain.hold_next_write();
    let (view, cx) = opened_tab(cx).await;
    view.update(cx, |view, cx| view.sign_in_with_github(cx));
    cx.run_until_parked();
    advance(5, cx);
    assert!(
        matches!(
            auth_step(&view, cx),
            Some(DeviceFlowStep::WaitingForUser { .. })
        ),
        "approved, and the token is being saved"
    );
    (harness, view, decide, cx)
}

fn assert_no_token_anywhere(harness: &Harness, cx: &mut VisualTestContext) {
    assert_eq!(
        harness.keychain.github_token(),
        None,
        "nothing in the keychain"
    );
    cx.update(|_, cx| assert!(session_token(cx).is_none(), "nothing kept in memory"));
    assert!(harness.built().is_empty(), "no backend was made");
}

#[gpui::test]
async fn cancelling_after_approval_leaves_no_token_in_the_keychain(cx: &mut TestAppContext) {
    let (harness, view, decide, cx) = approved_with_the_save_held(cx).await;
    view.update(cx, |view, cx| view.cancel_sign_in(cx));
    decide.send(true).ok();
    cx.run_until_parked();

    assert!(matches!(
        auth(&view, cx),
        AuthState::SignedOut { notice: None }
    ));
    assert_no_token_anywhere(&harness, cx);
}

#[gpui::test]
async fn disconnecting_while_the_token_is_being_saved_leaves_none_behind(cx: &mut TestAppContext) {
    let (harness, view, decide, cx) = approved_with_the_save_held(cx).await;
    view.update(cx, |view, cx| view.disconnect(cx));
    decide.send(true).ok();
    cx.run_until_parked();

    assert!(matches!(auth(&view, cx), AuthState::SignedOut { .. }));
    assert_no_token_anywhere(&harness, cx);
}

#[gpui::test]
async fn a_save_that_fails_after_cancel_or_disconnect_keeps_no_token_in_memory(
    cx: &mut TestAppContext,
) {
    let (harness, view, decide, cx) = approved_with_the_save_held(cx).await;
    view.update(cx, |view, cx| view.cancel_sign_in(cx));
    decide.send(false).ok();
    cx.run_until_parked();
    assert_no_token_anywhere(&harness, cx);
    assert!(matches!(
        auth(&view, cx),
        AuthState::SignedOut { notice: None }
    ));
}

#[gpui::test]
async fn cancelling_in_another_window_leaves_this_windows_saved_token_alone(
    cx: &mut TestAppContext,
) {
    let (harness, view, decide, cx) = approved_with_the_save_held(cx).await;
    let project = view.read_with(cx, |view, _| view.project.clone());
    let other = cx.new(|cx| GithubView::new(project, cx));
    other.update(cx, |other, cx| other.sign_in_with_github(cx));
    cx.run_until_parked();
    other.update(cx, |other, cx| other.cancel_sign_in(cx));
    decide.send(true).ok();
    cx.run_until_parked();

    assert_eq!(
        harness.keychain.github_token().as_deref(),
        Some(DEVICE_TOKEN),
        "the token approved here is kept"
    );
    assert!(matches!(auth(&view, cx), AuthState::SignedIn(_)));
}

#[gpui::test]
async fn disconnecting_in_another_window_ends_a_sign_in_still_being_saved(cx: &mut TestAppContext) {
    let (harness, view, decide, cx) = approved_with_the_save_held(cx).await;
    let project = view.read_with(cx, |view, _| view.project.clone());
    let other = cx.new(|cx| GithubView::new(project, cx));
    other.update(cx, |other, cx| other.disconnect(cx));
    decide.send(true).ok();
    cx.run_until_parked();

    assert_eq!(
        harness.keychain.github_token(),
        None,
        "nothing is left stored"
    );
    assert!(
        !matches!(auth(&view, cx), AuthState::SignedIn(_)),
        "a window whose saved token was just disconnected does not stay signed in with it"
    );
}

#[gpui::test]
async fn a_cancelled_save_landing_late_does_not_replace_a_newer_sign_ins_token(
    cx: &mut TestAppContext,
) {
    let harness = install(Setup::default(), cx);
    harness
        .http
        .answer(DEVICE_CODE_URL, 200, device_code_answer());
    for secret in ["gho_first_attempt", "gho_second_attempt"] {
        harness.http.answer(
            ACCESS_TOKEN_URL,
            200,
            json!({ "access_token": secret, "token_type": "bearer" }).to_string(),
        );
    }
    let first_save = harness.keychain.hold_next_write();
    let (view, cx) = opened_tab(cx).await;

    view.update(cx, |view, cx| view.sign_in_with_github(cx));
    cx.run_until_parked();
    advance(5, cx);
    view.update(cx, |view, cx| view.cancel_sign_in(cx));
    view.update(cx, |view, cx| view.sign_in_with_github(cx));
    cx.run_until_parked();
    advance(5, cx);
    first_save.send(true).ok();
    cx.run_until_parked();

    assert_eq!(
        harness.keychain.github_token().as_deref(),
        Some("gho_second_attempt"),
        "the newest approved token ends up stored"
    );
    assert!(matches!(auth(&view, cx), AuthState::SignedIn(_)));
}

#[gpui::test]
async fn a_save_that_fails_while_waiting_keeps_the_token_for_this_run(cx: &mut TestAppContext) {
    let (harness, view, decide, cx) = approved_with_the_save_held(cx).await;
    decide.send(false).ok();
    cx.run_until_parked();

    assert_eq!(harness.keychain.github_token(), None);
    cx.update(|_, cx| assert!(session_token(cx).is_some(), "kept in memory instead"));
    assert!(matches!(auth(&view, cx), AuthState::SignedIn(_)));
    assert!(
        view.read_with(cx, |view, _| view.notice.is_some()),
        "the user is told the keychain refused it"
    );
}

#[gpui::test]
async fn without_a_client_id_only_the_github_cli_is_offered(cx: &mut TestAppContext) {
    let harness = install(
        Setup {
            client_id: None,
            ..Setup::default()
        },
        cx,
    );
    let (view, cx) = opened_tab(cx).await;

    let options = cx.update(|_, cx| view.read(cx).sign_in_options(cx));
    assert!(!options.device_flow);
    assert!(options.github_cli);

    view.update(cx, |view, cx| view.sign_in_with_github(cx));
    cx.run_until_parked();
    assert!(matches!(auth(&view, cx), AuthState::SignedOut { .. }));
    assert!(harness.http.sent().is_empty());
}

#[gpui::test]
async fn a_refused_keychain_token_never_falls_back_to_gh_on_its_own(cx: &mut TestAppContext) {
    let harness = install(
        Setup {
            keychain: MemoryKeychain::holding("gho_revoked"),
            gh: FakeGhHost::signed_in("gho_other_account"),
            backend: |token| match token.source() {
                TokenSource::Keychain => {
                    FakeGithubBackend::new().failing_with(GithubError::TokenRevoked)
                }
                _ => FakeGithubBackend::new(),
            },
            ..Setup::default()
        },
        cx,
    );
    let (view, cx) = opened_tab(cx).await;

    assert!(matches!(
        auth(&view, cx),
        AuthState::Revoked {
            source: TokenSource::Keychain
        }
    ));
    assert_eq!(
        harness.gh.runs(),
        0,
        "gh is not asked until the user chooses it"
    );
    draw(cx);

    view.update(cx, |view, cx| view.reload(cx));
    cx.run_until_parked();
    assert!(matches!(auth(&view, cx), AuthState::Revoked { .. }));
    assert_eq!(harness.gh.runs(), 0, "nor on reload");

    view.update(cx, |view, cx| view.use_github_cli(cx));
    cx.run_until_parked();
    let AuthState::SignedIn(session) = auth(&view, cx) else {
        panic!("expected the gh account");
    };
    assert_eq!(session.token_source, TokenSource::GhCli);
    assert_eq!(harness.gh.runs(), 1);
    assert_eq!(
        harness.keychain.github_token(),
        None,
        "the refused token is gone"
    );
}

#[gpui::test]
async fn disconnect_deletes_the_keychain_token_and_opens_the_revoke_page(cx: &mut TestAppContext) {
    let harness = install(
        Setup {
            keychain: MemoryKeychain::holding("gho_signed_in"),
            ..Setup::default()
        },
        cx,
    );
    let (view, cx) = opened_tab(cx).await;
    assert!(matches!(auth(&view, cx), AuthState::SignedIn(_)));

    view.update(cx, |view, cx| view.disconnect(cx));
    cx.run_until_parked();

    assert_eq!(harness.keychain.github_token(), None);
    assert!(matches!(auth(&view, cx), AuthState::SignedOut { .. }));
    assert_eq!(
        cx.opened_url().as_deref(),
        Some("https://github.com/settings/connections/applications/Iv1.zodetest")
    );
    assert_eq!(harness.gh.runs(), 0);
}

#[gpui::test]
async fn a_gh_token_shows_the_account_and_the_repository_visibility(cx: &mut TestAppContext) {
    let harness = install(
        Setup {
            gh: FakeGhHost::signed_in("gho_cli"),
            backend: |_| {
                FakeGithubBackend::new().with_repository_visibility(
                    &RepositoryId::new("octo", "repo").expect("valid"),
                    RepositoryVisibility::Private,
                )
            },
            ..Setup::default()
        },
        cx,
    );
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ ".git": {}, "main.rs": "" }))
        .await;
    fs.with_git_state(Path::new("/root/.git"), true, |state| {
        state
            .remotes
            .insert("origin".into(), "https://github.com/octo/repo.git".into());
    })
    .expect("fake git state");
    let (workspace, cx) = workspace_over(fs, &[Path::new("/root")], cx).await;
    press_toggle(cx);
    let view = github_tab(&workspace, cx);

    draw(cx);
    let AuthState::SignedIn(session) = auth(&view, cx) else {
        panic!("expected to be signed in");
    };
    assert_eq!(session.token_source, TokenSource::GhCli);
    assert_eq!(harness.gh.runs(), 1);
    view.read_with(cx, |view, _| {
        let repository = RepositoryId::new("octo", "repo").expect("valid");
        assert_eq!(view.active_repository(), Some(&repository));
        assert_eq!(
            view.visibility.get(&repository),
            Some(&RepositoryVisibility::Private)
        );
        let context = view.context().expect("signed in with a repository");
        assert_eq!(context.login.as_ref(), "octocat");
        assert!(context.access.allows_local_writes());
    });
}

#[gpui::test]
async fn an_sso_protected_repository_is_reported(cx: &mut TestAppContext) {
    install(
        Setup {
            keychain: MemoryKeychain::holding("gho_signed_in"),
            ..Setup::default()
        },
        cx,
    );
    let (view, cx) = opened_tab(cx).await;
    let sso = GithubError::SsoRequired {
        authorize_url: "https://github.com/orgs/corp/sso?authorization_request=abc".into(),
    };
    view.update(cx, |view, cx| {
        // GitHub answers the account check but refuses the org's repository.
        if let AuthState::SignedIn(session) = &mut view.auth {
            session.backend = Arc::new(FakeGithubBackend::new().failing_with(sso.clone()));
        }
        view.select_repository(RepositoryId::new("corp", "secret").expect("valid"), cx);
    });
    draw(cx);

    view.read_with(cx, |view, _| {
        assert_eq!(view.repository_error, Some(sso.clone()));
        assert!(
            matches!(view.auth(), AuthState::SignedIn(_)),
            "still signed in"
        );
    });
}

#[gpui::test]
async fn two_remotes_offer_two_repositories(cx: &mut TestAppContext) {
    install(Setup::default(), cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ ".git": {}, "main.rs": "" }))
        .await;
    fs.with_git_state(Path::new("/root/.git"), true, |state| {
        state
            .remotes
            .insert("origin".into(), "git@github.com:me/repo.git".into());
        state
            .remotes
            .insert("upstream".into(), "https://github.com/octo/repo".into());
    })
    .expect("fake git state");
    let (workspace, cx) = workspace_over(fs, &[Path::new("/root")], cx).await;
    press_toggle(cx);
    let view = github_tab(&workspace, cx);

    view.read_with(cx, |view, _| {
        let offered = view
            .repositories()
            .iter()
            .map(|choice| (choice.repository.full_name(), choice.remote))
            .collect::<Vec<_>>();
        assert_eq!(
            offered,
            vec![
                ("me/repo".to_string(), RemoteName::Origin),
                ("octo/repo".to_string(), RemoteName::Upstream),
            ]
        );
        assert_eq!(
            view.active_repository()
                .map(RepositoryId::full_name)
                .as_deref(),
            Some("me/repo"),
            "origin first; neither is preferred over the user's pick"
        );
    });
    view.update(cx, |view, cx| {
        view.select_repository(RepositoryId::new("octo", "repo").expect("valid"), cx)
    });
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.active_repository()
                .map(RepositoryId::full_name)
                .as_deref(),
            Some("octo/repo")
        );
    });
}

#[gpui::test]
async fn a_collab_guest_gets_no_local_writes(cx: &mut TestAppContext) {
    install(
        Setup {
            keychain: MemoryKeychain::holding("gho_signed_in"),
            ..Setup::default()
        },
        cx,
    );
    let (view, cx) = opened_tab(cx).await;
    view.read_with(cx, |view, _| {
        assert_eq!(view.access(), ProjectAccess::Owner, "a local project");
    });

    assert!(!ProjectAccess::CollabGuest.allows_local_writes());
    view.update(cx, |view, cx| {
        view.access = ProjectAccess::CollabGuest;
        view.select_repository(RepositoryId::new("octo", "repo").expect("valid"), cx);
    });
    draw(cx);
    view.read_with(cx, |view, _| {
        let context = view.context().expect("signed in with a repository");
        assert!(!context.access.allows_local_writes());
    });
}

#[gpui::test]
async fn signing_in_with_a_repository_lists_its_issues(cx: &mut TestAppContext) {
    install(
        Setup {
            gh: FakeGhHost::signed_in("gho_cli"),
            backend: |_| {
                let repository = RepositoryId::new("octo", "repo").expect("valid");
                FakeGithubBackend::new()
                    .with_issue(&repository, github::fake_issue(1, "First"))
                    .with_issue(&repository, github::fake_issue(2, "Second"))
            },
            ..Setup::default()
        },
        cx,
    );
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ ".git": {}, "main.rs": "" }))
        .await;
    fs.with_git_state(Path::new("/root/.git"), true, |state| {
        state
            .remotes
            .insert("origin".into(), "https://github.com/octo/repo.git".into());
    })
    .expect("fake git state");
    let (workspace, cx) = workspace_over(fs, &[Path::new("/root")], cx).await;
    press_toggle(cx);
    let view = github_tab(&workspace, cx);
    draw(cx);

    let list = view.read_with(cx, |view, _| view.issue_list().cloned());
    assert!(list.is_some(), "the Issues section shows a list");
    let Some(list) = list else {
        return;
    };
    let numbers = list.read_with(cx, |list, _| {
        list.visible_rows()
            .iter()
            .map(|issue| issue.number)
            .collect::<Vec<_>>()
    });
    assert_eq!(numbers, vec![1, 2]);

    view.update(cx, |view, cx| view.disconnect(cx));
    cx.run_until_parked();
    assert!(
        view.read_with(cx, |view, _| view.issue_list().is_none()),
        "signing out drops the list"
    );
}

fn dispatch(action: impl gpui::Action, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.dispatch_action(Box::new(action), cx));
    cx.run_until_parked();
}

pub(crate) fn status(cx: &mut VisualTestContext) -> SignInStatus {
    cx.update(|_, cx| sign_in_status(cx))
}

#[gpui::test]
async fn the_settings_sign_in_opens_the_tab_and_starts_the_device_flow(cx: &mut TestAppContext) {
    let harness = install(Setup::default(), cx);
    harness
        .http
        .answer(DEVICE_CODE_URL, 200, device_code_answer());
    let fs = FakeFs::new(cx.executor());
    let (workspace, cx) = workspace_over(fs, &[], cx).await;
    assert_eq!(status(cx), SignInStatus::NotChecked);

    dispatch(zode_actions::github::SignIn, cx);

    let view = github_tab(&workspace, cx);
    assert!(matches!(
        auth_step(&view, cx),
        Some(DeviceFlowStep::WaitingForUser { .. })
    ));
    assert_eq!(status(cx), SignInStatus::SigningIn);
    assert_eq!(harness.http.sent().len(), 1, "only the device code");
    assert_eq!(harness.gh.runs(), 0, "the token lookup never ran");

    dispatch(zode_actions::github::SignIn, cx);
    assert_eq!(
        harness.http.sent().len(),
        1,
        "a second press does not start another sign-in"
    );
}

#[gpui::test]
async fn the_settings_disconnect_signs_out_the_tab(cx: &mut TestAppContext) {
    let harness = install(
        Setup {
            keychain: MemoryKeychain::holding("gho_signed_in"),
            ..Setup::default()
        },
        cx,
    );
    let (view, cx) = opened_tab(cx).await;
    let SignInStatus::SignedIn { login, source } = status(cx) else {
        panic!("expected the status to follow the tab");
    };
    assert_eq!(login.as_ref(), "octocat");
    assert_eq!(source, TokenSource::Keychain);
    assert_eq!(status(cx).describe(), "Signed in as @octocat via Zode.");

    dispatch(zode_actions::github::Disconnect, cx);

    assert!(matches!(auth(&view, cx), AuthState::SignedOut { .. }));
    assert_eq!(status(cx), SignInStatus::SignedOut);
    assert_eq!(harness.keychain.github_token(), None);
    assert_eq!(
        cx.opened_url().as_deref(),
        Some("https://github.com/settings/connections/applications/Iv1.zodetest")
    );
}

#[gpui::test]
async fn a_refused_token_is_reported_to_the_settings_page(cx: &mut TestAppContext) {
    install(
        Setup {
            keychain: MemoryKeychain::holding("gho_revoked"),
            backend: |_| FakeGithubBackend::new().failing_with(GithubError::TokenRevoked),
            ..Setup::default()
        },
        cx,
    );
    let (_view, cx) = opened_tab(cx).await;
    assert_eq!(status(cx), SignInStatus::Revoked);
}

#[gpui::test]
async fn sign_in_with_github_is_available_only_with_a_client_id(cx: &mut TestAppContext) {
    install(
        Setup {
            client_id: None,
            ..Setup::default()
        },
        cx,
    );
    cx.update(|cx| assert!(!sign_in_with_github_available(cx)));
    cx.update(|cx| {
        cx.set_global(GithubClientId(Some(CLIENT_ID.to_string())));
        assert!(sign_in_with_github_available(cx));
    });
}
