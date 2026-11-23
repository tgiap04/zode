//! Where the GitHub tab gets its token, its `client_id`, and the pieces it
//! talks to, gathered in one global so a test can swap every one of them.

use std::sync::Arc;

use credentials_provider::CredentialsProvider;
use github::{
    BUILT_IN_CLIENT_ID, CLIENT_ID_VARIABLE, GhHost, GithubBackend, GithubClientId, GithubResult,
    GithubToken, HttpBackend, LocalGhHost, SignInStatus, TokenSource, client_id_for_build,
    delete_keychain_token, resolve_token, write_keychain_token,
};
use gpui::{App, AsyncApp, Global, Task};
use http_client::HttpClient;
use release_channel::ReleaseChannel;

use crate::github_view::AuthState;

const DEVELOPMENT_KEYCHAIN_VARIABLE: &str = "ZODE_DEVELOPMENT_USE_KEYCHAIN";

/// Where a token from Zode's own sign-in is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenStorage {
    Keychain,
    /// Dev builds: their "keychain" is a plain JSON file on disk, and a token
    /// that can write to every repository does not belong there.
    MemoryOnly,
}

impl TokenStorage {
    pub fn for_build(channel: Option<ReleaseChannel>, development_keychain: bool) -> Self {
        match channel {
            Some(ReleaseChannel::Dev) if !development_keychain => Self::MemoryOnly,
            _ => Self::Keychain,
        }
    }
}

pub type BackendFactory =
    Arc<dyn Fn(GithubToken, &mut App) -> Arc<dyn GithubBackend> + Send + Sync>;

/// Everything outside the tab that the tab depends on.
#[derive(Clone)]
pub struct GithubEnvironment {
    pub token_storage: TokenStorage,
    pub credentials: Arc<dyn CredentialsProvider>,
    pub gh_host: Arc<dyn GhHost>,
    /// Only for the device flow; GitHub's API goes through the backend.
    pub http_client: Arc<dyn HttpClient>,
    pub make_backend: BackendFactory,
}

impl Global for GithubEnvironment {}

impl GithubEnvironment {
    /// The one a test installed, or this build's.
    pub fn get(cx: &App) -> Self {
        cx.try_global::<Self>()
            .cloned()
            .unwrap_or_else(|| Self::for_this_build(cx))
    }

    fn for_this_build(cx: &App) -> Self {
        let variable = |name: &str| std::env::var(name).ok();
        Self {
            token_storage: TokenStorage::for_build(
                ReleaseChannel::try_global(cx),
                variable(DEVELOPMENT_KEYCHAIN_VARIABLE).is_some_and(|value| !value.is_empty()),
            ),
            credentials: zode_credentials_provider::global(cx),
            gh_host: Arc::new(LocalGhHost),
            http_client: cx.http_client(),
            make_backend: Arc::new(|token, cx| {
                Arc::new(HttpBackend::new(cx.http_client(), token, cx))
            }),
        }
    }
}

impl From<&AuthState> for SignInStatus {
    fn from(auth: &AuthState) -> Self {
        match auth {
            AuthState::Checking => Self::Checking,
            AuthState::SignedOut { .. } => Self::SignedOut,
            AuthState::SigningIn(_) => Self::SigningIn,
            AuthState::SignedIn(session) => Self::SignedIn {
                login: session.login.clone(),
                source: session.token_source,
            },
            AuthState::Revoked { .. } => Self::Revoked,
            AuthState::Failed(_) => Self::Failed,
        }
    }
}

/// Installs this build's `client_id`, which the settings page reads too.
/// Only a development build reads it from the environment.
pub(crate) fn install_client_id(cx: &mut App) {
    let development_build =
        cfg!(debug_assertions) || ReleaseChannel::try_global(cx) == Some(ReleaseChannel::Dev);
    let client_id = client_id_for_build(
        BUILT_IN_CLIENT_ID,
        std::env::var(CLIENT_ID_VARIABLE).ok(),
        development_build,
    );
    cx.set_global(GithubClientId(client_id));
}

/// A device-flow token kept for this run of Zode only (see
/// [`TokenStorage::MemoryOnly`]), or kept because the keychain refused it.
#[derive(Default)]
struct SessionToken(Option<GithubToken>);

impl Global for SessionToken {}

pub fn session_token(cx: &App) -> Option<GithubToken> {
    cx.try_global::<SessionToken>()
        .and_then(|session| session.0.clone())
}

fn set_session_token(token: Option<GithubToken>, cx: &mut App) {
    cx.set_global(SessionToken(token));
}

/// The session's token, then the keychain's, then `gh`'s.
pub async fn find_token(
    environment: &GithubEnvironment,
    cx: &AsyncApp,
) -> GithubResult<GithubToken> {
    if let Some(token) = cx.update(|cx| session_token(cx)) {
        return Ok(token);
    }
    resolve_token(&*environment.credentials, &*environment.gh_host, cx).await
}

/// Keeps a token from the device flow where this build keeps them. Returns
/// a note for the user when it could not be kept as asked.
pub fn keep_signed_in_token(
    environment: &GithubEnvironment,
    secret: String,
    cx: &mut App,
) -> Task<Option<String>> {
    let token = GithubToken::new(&secret, TokenSource::Keychain);
    match environment.token_storage {
        TokenStorage::MemoryOnly => {
            set_session_token(Some(token), cx);
            Task::ready(None)
        }
        TokenStorage::Keychain => {
            set_session_token(None, cx);
            let write = write_keychain_token(environment.credentials.clone(), secret, cx);
            cx.spawn(async move |cx| match write.await {
                Ok(()) => None,
                Err(error) => {
                    log::error!("could not save the GitHub token to the keychain: {error}");
                    cx.update(|cx| set_session_token(Some(token), cx));
                    Some("The system keychain refused the token, so you will be asked to sign in again next time.".to_string())
                }
            })
        }
    }
}

/// Drops Zode's own token, wherever it was kept. Never touches `gh`'s.
pub fn forget_signed_in_token(
    environment: &GithubEnvironment,
    cx: &mut App,
) -> Task<anyhow::Result<()>> {
    set_session_token(None, cx);
    delete_keychain_token(environment.credentials.clone(), cx)
}
