use std::collections::HashMap;
use std::sync::Arc;

use github::{
    DetectedRepository, GithubBackend, GithubError, GithubToken, OAuthScopes, RateLimits,
    RepositoryId, RepositoryVisibility, SignInStatus, TokenIdentity, TokenSource,
    configured_client_id, publish_sign_in_status, revoke_page, sign_in_with_github_available,
    token_from_gh,
};
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, EventEmitter, FocusHandle, Focusable,
    SharedString, Subscription, Task,
};
use project::Project;

use crate::device_flow::{poll_for_token, request_device_code};
use crate::github_view_session::{
    HeldSessions, SessionAccount, SessionStamp, held_session, hold_session, is_refused,
    release_session, report_if_revoked,
};
use crate::issues::{IssueList, IssueListEvent};
use crate::projects::ProjectsSection;
use crate::pull_requests::PullRequestList;
use crate::repo_picker::{choose_active, project_repositories};
use crate::sign_in::{GithubEnvironment, find_token, forget_signed_in_token, keep_signed_in_token};

/// Whether this Zode owns the project's working tree. A collaborator's guest
/// view must not change the host's files (checkout, branch switches).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectAccess {
    Owner,
    CollabGuest,
}

impl ProjectAccess {
    pub fn of(project: &Project) -> Self {
        if project.is_via_collab() {
            Self::CollabGuest
        } else {
            Self::Owner
        }
    }

    pub fn allows_local_writes(self) -> bool {
        self == Self::Owner
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GithubSection {
    Issues,
    PullRequests,
    Projects,
}

impl GithubSection {
    pub const ALL: [Self; 3] = [Self::Issues, Self::PullRequests, Self::Projects];

    pub fn label(self) -> &'static str {
        match self {
            Self::Issues => "Issues",
            Self::PullRequests => "Pull requests",
            Self::Projects => "Projects",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceFlowStep {
    RequestingCode,
    WaitingForUser {
        user_code: SharedString,
        verification_uri: SharedString,
    },
}

#[derive(Clone)]
pub struct Session {
    pub login: SharedString,
    pub token_source: TokenSource,
    pub backend: Arc<dyn GithubBackend>,
    /// As GitHub reported them when the session started.
    pub scopes: OAuthScopes,
}

#[derive(Clone)]
pub enum AuthState {
    Checking,
    SignedOut {
        notice: Option<SharedString>,
    },
    SigningIn(DeviceFlowStep),
    SignedIn(Session),
    /// GitHub refused the token. Nothing falls back to another token on its
    /// own: that would quietly act as a different account.
    Revoked {
        source: TokenSource,
    },
    Failed(GithubError),
}

/// What a section needs to read or write: the signed-in backend and the
/// repository picked. `None` from [`GithubView::context`] while either is
/// missing.
#[derive(Clone)]
pub struct GithubContext {
    pub backend: Arc<dyn GithubBackend>,
    pub repository: RepositoryId,
    pub login: SharedString,
    pub access: ProjectAccess,
    /// The sign-in this context belongs to; see [`SessionStamp`].
    pub session: SessionStamp,
}

/// The key a detail tab (one issue or pull request) is found by, so the same
/// number in two repositories never shares a tab.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DetailKey {
    pub repository: RepositoryId,
    pub number: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignInOptions {
    pub device_flow: bool,
    pub github_cli: bool,
}

/// The GitHub tab. Nothing reaches GitHub until a token is found, and the
/// tab is never restored at start-up, so opening Zode sends nothing.
pub struct GithubView {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) project: Entity<Project>,
    pub(crate) access: ProjectAccess,
    pub(crate) auth: AuthState,
    /// The project's entity: what sessions made here are scoped to.
    scope: EntityId,
    /// The token behind the session being checked or signed in.
    token_identity: Option<TokenIdentity>,
    pub(crate) notice: Option<SharedString>,
    pub(crate) repositories: Vec<DetectedRepository>,
    pub(crate) active_repository: Option<RepositoryId>,
    pub(crate) visibility: HashMap<RepositoryId, RepositoryVisibility>,
    pub(crate) repository_error: Option<GithubError>,
    pub(crate) section: GithubSection,
    pub(crate) rate_limits: RateLimits,
    /// Made when the Issues section is first shown with a repository and an
    /// account, and replaced when either changes.
    pub(crate) issue_list: Option<Entity<IssueList>>,
    _issue_list_subscription: Option<Subscription>,
    /// Made when the Projects section is first shown, like the issue list.
    pub(crate) projects_section: Option<Entity<ProjectsSection>>,
    pub(crate) _projects_section_subscription: Option<Subscription>,
    /// Made when the Pull requests section is first shown, like the others.
    pub(crate) pull_request_list: Option<Entity<PullRequestList>>,
    pub(crate) _pull_request_list_subscription: Option<Subscription>,
    /// Token lookup, sign-in, sign-out. Replacing it cancels the one before.
    auth_task: Option<Task<()>>,
    /// The signed-in account check, kept apart from `auth_task` because the
    /// sign-in task is what starts it.
    session_task: Option<Task<()>>,
    repository_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

pub enum GithubViewEvent {}

impl EventEmitter<GithubViewEvent> for GithubView {}

impl Focusable for GithubView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl GithubView {
    /// Silent: builds the view and reads the project's remotes, nothing more.
    /// [`Self::connect`] is what looks for a token.
    pub fn new(project: Entity<Project>, cx: &mut Context<Self>) -> Self {
        let access = ProjectAccess::of(project.read(cx));
        let git_store = project.read(cx).git_store().clone();
        let subscription = cx.subscribe(&git_store, |this, _, _event, cx| {
            this.refresh_repositories(cx);
        });
        // A list or detail tab that GitHub refused says so through the
        // held sessions; the account shown must follow.
        let session_subscription = cx.observe_global::<HeldSessions>(|this, cx| {
            let holder = cx.entity_id();
            if held_session(holder, cx).is_some_and(|held| is_refused(&held, cx)) {
                this.token_refused(cx);
            }
        });
        let holder = cx.entity_id();
        let release_subscription = cx.on_release(move |_, cx| release_session(holder, cx));
        let scope = project.entity_id();
        let mut view = Self {
            focus_handle: cx.focus_handle(),
            project,
            access,
            auth: AuthState::SignedOut { notice: None },
            scope,
            token_identity: None,
            notice: None,
            repositories: Vec::new(),
            active_repository: None,
            visibility: HashMap::default(),
            repository_error: None,
            section: GithubSection::Issues,
            rate_limits: RateLimits::default(),
            issue_list: None,
            _issue_list_subscription: None,
            projects_section: None,
            _projects_section_subscription: None,
            pull_request_list: None,
            _pull_request_list_subscription: None,
            auth_task: None,
            session_task: None,
            repository_task: None,
            _subscriptions: vec![subscription, session_subscription, release_subscription],
        };
        view.refresh_repositories(cx);
        view
    }

    pub fn auth(&self) -> &AuthState {
        &self.auth
    }

    pub fn access(&self) -> ProjectAccess {
        self.access
    }

    pub fn section(&self) -> GithubSection {
        self.section
    }

    pub fn repositories(&self) -> &[DetectedRepository] {
        &self.repositories
    }

    pub fn active_repository(&self) -> Option<&RepositoryId> {
        self.active_repository.as_ref()
    }

    /// A keychain entry Zode cannot use: Disconnect deletes it.
    pub fn offers_disconnect(&self) -> bool {
        matches!(self.auth, AuthState::Failed(GithubError::Keychain { .. }))
    }

    pub fn rate_limits(&self) -> &RateLimits {
        &self.rate_limits
    }

    pub fn issue_list(&self) -> Option<&Entity<IssueList>> {
        self.issue_list.as_ref()
    }

    /// Keeps the issue list in step with the account and repository: dropped
    /// while either is missing, made (and read) when the Issues section is
    /// shown for a new pair.
    fn sync_issue_list(&mut self, cx: &mut Context<Self>) {
        let Some(context) = self.context() else {
            // Kept through a token check: Reload on the same account keeps
            // the list's selection and the "New issue" draft.
            if !matches!(self.auth, AuthState::Checking) {
                self.issue_list = None;
                self._issue_list_subscription = None;
            }
            return;
        };
        if let Some(list) = &self.issue_list
            && list.read(cx).serves(&context)
        {
            if !std::ptr::addr_eq(
                Arc::as_ptr(&list.read(cx).context().backend),
                Arc::as_ptr(&context.backend),
            ) {
                list.update(cx, |list, cx| {
                    list.context = context;
                    list.reload(cx);
                });
            }
            return;
        }
        if self.section != GithubSection::Issues {
            self.issue_list = None;
            self._issue_list_subscription = None;
            return;
        }
        let list = cx.new(|cx| {
            let mut list = IssueList::new(context);
            list.reload(cx);
            list
        });
        self._issue_list_subscription = Some(cx.subscribe(
            &list,
            |this, list: Entity<IssueList>, IssueListEvent::Loaded, cx| {
                this.rate_limits = list.read(cx).context().backend.rate_limits();
                cx.notify();
            },
        ));
        self.issue_list = Some(list);
    }

    pub fn context(&self) -> Option<GithubContext> {
        let AuthState::SignedIn(session) = &self.auth else {
            return None;
        };
        Some(GithubContext {
            backend: session.backend.clone(),
            repository: self.active_repository.clone()?,
            login: session.login.clone(),
            access: self.access,
            session: self.session_stamp()?,
        })
    }

    /// The session this tab is signed in as, while it is signed in.
    pub(crate) fn session_stamp(&self) -> Option<SessionStamp> {
        let AuthState::SignedIn(session) = &self.auth else {
            return None;
        };
        Some(SessionStamp::new(
            self.scope,
            SessionAccount {
                login: session.login.clone(),
                token: self.token_identity.clone(),
            },
        ))
    }

    pub fn sign_in_options(&self, cx: &App) -> SignInOptions {
        SignInOptions {
            device_flow: sign_in_with_github_available(cx),
            github_cli: true,
        }
    }

    pub(crate) fn set_auth(&mut self, auth: AuthState, cx: &mut Context<Self>) {
        self.auth = auth;
        publish_sign_in_status(SignInStatus::from(&self.auth), cx);
        let holder = cx.entity_id();
        match &self.auth {
            AuthState::SignedIn(_) => {
                if let Some(stamp) = self.session_stamp() {
                    hold_session(holder, stamp, cx);
                }
            }
            AuthState::SignedOut { .. } | AuthState::Revoked { .. } => release_session(holder, cx),
            // A check still running, or one that failed (offline, the
            // keychain locked), says nothing about the token held: what was
            // made under it stays usable.
            AuthState::Checking | AuthState::SigningIn(_) | AuthState::Failed(_) => {}
        }
        self.sync_sections(cx);
    }

    fn sync_sections(&mut self, cx: &mut Context<Self>) {
        self.sync_issue_list(cx);
        self.sync_projects_section(cx);
        self.sync_pull_request_list(cx);
        cx.notify();
    }

    /// Looks for a token (this session's, the keychain's, then `gh`'s) and
    /// checks it with GitHub. Also what Reload does, so a `gh auth switch`
    /// is picked up.
    pub fn connect(&mut self, cx: &mut Context<Self>) {
        let environment = GithubEnvironment::get(cx);
        self.session_task = None;
        // A reload may land on another account (`gh auth switch`), so what
        // the last one could see is not kept.
        self.repository_task = None;
        self.visibility.clear();
        self.repository_error = None;
        self.set_auth(AuthState::Checking, cx);
        self.auth_task = Some(cx.spawn(async move |this, cx| {
            let token = find_token(&environment, cx).await;
            this.update(cx, |this, cx| match token {
                Ok(token) => this.start_session(token, cx),
                Err(GithubError::NotSignedIn) => {
                    this.set_auth(AuthState::SignedOut { notice: None }, cx)
                }
                Err(error) => this.set_auth(AuthState::Failed(error), cx),
            })
            .ok();
        }));
    }

    fn start_session(&mut self, token: GithubToken, cx: &mut Context<Self>) {
        let environment = GithubEnvironment::get(cx);
        let source = token.source();
        let identity = token.identity().clone();
        let backend = (environment.make_backend)(token, cx);
        self.set_auth(AuthState::Checking, cx);
        self.session_task = Some(cx.spawn(async move |this, cx| {
            let viewer = backend.viewer().await;
            this.update(cx, |this, cx| {
                this.rate_limits = backend.rate_limits();
                this.token_identity = Some(identity);
                match viewer {
                    Ok(viewer) => {
                        this.set_auth(
                            AuthState::SignedIn(Session {
                                login: viewer.login.into(),
                                scopes: viewer.scopes,
                                token_source: source,
                                backend,
                            }),
                            cx,
                        );
                        this.load_repository_details(cx);
                    }
                    Err(GithubError::TokenRevoked) => {
                        this.set_auth(AuthState::Revoked { source }, cx)
                    }
                    Err(error) => this.set_auth(AuthState::Failed(error), cx),
                }
            })
            .ok();
        }));
    }

    /// GitHub's device flow. Hidden, and a no-op, without a `client_id`.
    pub fn sign_in_with_github(&mut self, cx: &mut Context<Self>) {
        let environment = GithubEnvironment::get(cx);
        let Some(client_id) = configured_client_id(cx) else {
            return;
        };
        self.session_task = None;
        self.notice = None;
        self.set_auth(AuthState::SigningIn(DeviceFlowStep::RequestingCode), cx);
        self.auth_task = Some(cx.spawn(async move |this, cx| {
            let executor = cx.background_executor().clone();
            let http_client = environment.http_client.clone();
            let code = match request_device_code(&http_client, &client_id, &executor).await {
                Ok(code) => code,
                Err(error) => {
                    this.update(cx, |this, cx| this.signed_out_with(error.to_string(), cx))
                        .ok();
                    return;
                }
            };
            let step = DeviceFlowStep::WaitingForUser {
                user_code: code.user_code.clone().into(),
                verification_uri: code.verification_uri.clone().into(),
            };
            if this
                .update(cx, |this, cx| this.set_auth(AuthState::SigningIn(step), cx))
                .is_err()
            {
                return;
            }
            let secret = match poll_for_token(&http_client, &client_id, &code, &executor).await {
                Ok(secret) => secret,
                Err(error) => {
                    this.update(cx, |this, cx| this.signed_out_with(error.to_string(), cx))
                        .ok();
                    return;
                }
            };
            let Ok(keep) = this.update(cx, |_, cx| {
                keep_signed_in_token(&environment, secret.clone(), cx)
            }) else {
                return;
            };
            let note = keep.await;
            this.update(cx, |this, cx| {
                this.notice = note.map(SharedString::from);
                this.start_session(GithubToken::new(&secret, TokenSource::Keychain), cx);
            })
            .ok();
        }));
    }

    pub fn cancel_sign_in(&mut self, cx: &mut Context<Self>) {
        self.auth_task = None;
        self.set_auth(AuthState::SignedOut { notice: None }, cx);
    }

    fn signed_out_with(&mut self, notice: String, cx: &mut Context<Self>) {
        self.set_auth(
            AuthState::SignedOut {
                notice: Some(notice.into()),
            },
            cx,
        );
    }

    /// The user's choice after a refused token: drop Zode's token and use
    /// whatever `gh` is signed in as.
    pub fn use_github_cli(&mut self, cx: &mut Context<Self>) {
        let environment = GithubEnvironment::get(cx);
        let forget = forget_signed_in_token(&environment, cx);
        self.session_task = None;
        self.set_auth(AuthState::Checking, cx);
        self.auth_task = Some(cx.spawn(async move |this, cx| {
            if let Err(error) = forget.await {
                log::error!("could not remove the GitHub token from the keychain: {error}");
            }
            let executor = cx.background_executor().clone();
            let token = token_from_gh(&*environment.gh_host, &executor).await;
            this.update(cx, |this, cx| match token {
                Ok(token) => this.start_session(token, cx),
                Err(GithubError::NotSignedIn) => this.signed_out_with(
                    "The GitHub CLI is not signed in. Run `gh auth login`, then check again."
                        .to_string(),
                    cx,
                ),
                Err(error) => this.set_auth(AuthState::Failed(error), cx),
            })
            .ok();
        }));
    }

    /// Forgets Zode's own token and opens GitHub's page for revoking the
    /// app's access, since Zode cannot revoke a token itself.
    pub fn disconnect(&mut self, cx: &mut Context<Self>) {
        let environment = GithubEnvironment::get(cx);
        let forget = forget_signed_in_token(&environment, cx);
        if let Some(page) = revoke_page(cx) {
            cx.open_url(&page);
        }
        self.session_task = None;
        self.repository_task = None;
        self.visibility.clear();
        self.repository_error = None;
        self.rate_limits = RateLimits::default();
        self.notice = None;
        self.signed_out_with(
            "Disconnected. Revoke Zode's access on GitHub to end the token for good.".to_string(),
            cx,
        );
        self.auth_task = Some(cx.spawn(async move |this, cx| {
            if let Err(error) = forget.await {
                this.update(cx, |this, cx| {
                    this.signed_out_with(
                        format!("Could not remove the token from the system keychain: {error}"),
                        cx,
                    )
                })
                .ok();
            }
        }));
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.connect(cx);
    }

    pub fn select_section(&mut self, section: GithubSection, cx: &mut Context<Self>) {
        self.section = section;
        self.sync_issue_list(cx);
        self.sync_projects_section(cx);
        self.sync_pull_request_list(cx);
        cx.notify();
    }

    pub fn select_repository(&mut self, repository: RepositoryId, cx: &mut Context<Self>) {
        if self.active_repository.as_ref() == Some(&repository) {
            return;
        }
        self.active_repository = Some(repository);
        self.load_repository_details(cx);
        cx.notify();
    }

    pub(crate) fn refresh_repositories(&mut self, cx: &mut Context<Self>) {
        let repositories = project_repositories(self.project.read(cx), cx);
        if repositories == self.repositories {
            return;
        }
        let active = choose_active(&repositories, self.active_repository.as_ref());
        self.repositories = repositories;
        if active != self.active_repository {
            self.active_repository = active;
            self.load_repository_details(cx);
        }
        cx.notify();
    }

    /// Public or private, and the first sign of an SSO or organization
    /// restriction on the repository.
    fn load_repository_details(&mut self, cx: &mut Context<Self>) {
        self.repository_error = None;
        self.sync_issue_list(cx);
        self.sync_projects_section(cx);
        self.sync_pull_request_list(cx);
        let (AuthState::SignedIn(session), Some(repository)) =
            (&self.auth, self.active_repository.clone())
        else {
            self.repository_task = None;
            return;
        };
        let backend = session.backend.clone();
        let stamp = self.session_stamp();
        self.repository_task = Some(cx.spawn(async move |this, cx| {
            let visibility = backend.repository_visibility(&repository).await;
            this.update(cx, |this, cx| {
                this.rate_limits = backend.rate_limits();
                match visibility {
                    Ok(visibility) => {
                        this.visibility.insert(repository, visibility);
                    }
                    Err(error) => {
                        if let Some(stamp) = &stamp {
                            report_if_revoked(&error, stamp, cx);
                        }
                        if error != GithubError::TokenRevoked {
                            this.repository_error = Some(error);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// GitHub refused the token this tab holds: the tab shows it, and
    /// nothing falls back to another token. Mid-check, the session simply
    /// ends.
    fn token_refused(&mut self, cx: &mut Context<Self>) {
        match &self.auth {
            AuthState::SignedIn(session) => {
                let source = session.token_source;
                self.set_auth(AuthState::Revoked { source }, cx);
            }
            _ => release_session(cx.entity_id(), cx),
        }
    }
}
