//! Where the GitHub tab gets its token, its `client_id`, and the pieces it
//! talks to, gathered in one global so a test can swap every one of them.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use credentials_provider::CredentialsProvider;
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::Shared;
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

/// Where each keychain operation for the token waits its turn. The system
/// keychain applies calls as it gets them, so two of them in flight at once
/// could land in either order; one after the other, they land as issued.
///
/// Also the keychain writes that followed an approved sign-in and are still in
/// flight, each told apart by its ticket. A sign-in that is cancelled, or a
/// disconnect, abandons its write: the write itself cannot be called back, so
/// it removes what it wrote once it lands.
#[derive(Default)]
struct KeychainQueue {
    tail: Option<Shared<oneshot::Receiver<()>>>,
    next_ticket: u64,
    /// Ticket to whether its write was abandoned. An entry stays only while
    /// its write is in flight.
    in_flight: HashMap<u64, bool>,
}

impl Global for KeychainQueue {}

/// Names one keychain write that followed an approved sign-in, so that the
/// sign-in it belongs to can abandon it without touching another window's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeepTicket(u64);

fn is_abandoned(ticket: KeepTicket, cx: &App) -> bool {
    cx.try_global::<KeychainQueue>()
        .and_then(|queue| queue.in_flight.get(&ticket.0))
        .copied()
        .unwrap_or(false)
}

fn finish_keep(ticket: KeepTicket, cx: &mut App) {
    cx.default_global::<KeychainQueue>()
        .in_flight
        .remove(&ticket.0);
}

/// Ends the write behind `ticket` if it is still in flight: it deletes its
/// token as soon as it lands, and puts none in memory if it fails.
pub fn abandon_keep(ticket: KeepTicket, cx: &mut App) {
    if let Some(abandoned) = cx
        .default_global::<KeychainQueue>()
        .in_flight
        .get_mut(&ticket.0)
    {
        *abandoned = true;
    }
}

/// Ends every write in flight, whichever sign-in it belongs to.
fn abandon_all_keeps(cx: &mut App) {
    for abandoned in cx.default_global::<KeychainQueue>().in_flight.values_mut() {
        *abandoned = true;
    }
}

/// How long an operation waits for the one before it to finish.
const KEYCHAIN_TURN_LIMIT: Duration = Duration::from_secs(60);

/// Runs `work` once every operation queued before it has finished. It runs on
/// its own, not tied to the caller, so a caller that gives up does not leave
/// the keychain half-changed or the queue stuck.
fn enqueue<T: 'static>(
    cx: &mut App,
    work: impl AsyncFnOnce(&mut AsyncApp) -> T + 'static,
) -> oneshot::Receiver<T> {
    let queue = cx.default_global::<KeychainQueue>();
    let previous = queue.tail.take();
    let (turn_over, turn) = oneshot::channel();
    queue.tail = Some(turn.shared());
    let (result_sender, result) = oneshot::channel();
    cx.spawn(async move |cx| {
        if let Some(previous) = previous {
            // A keychain call stuck on a system prompt must not hold every
            // later one, a disconnect's delete included, for good.
            let waited_long_enough = cx.background_executor().timer(KEYCHAIN_TURN_LIMIT);
            futures::future::select(previous, waited_long_enough).await;
        }
        let value = work(cx).await;
        turn_over.send(()).ok();
        result_sender.send(value).ok();
    })
    .detach();
    result
}

/// How keeping a token from the device flow ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeepOutcome {
    /// The sign-in may go on; `note` tells the user when the token could
    /// not be kept as asked.
    Kept { note: Option<String> },
    /// A disconnect or cancel ended it first: nothing was kept, and the
    /// sign-in must not go on with the token.
    Abandoned,
}

/// A token from the device flow on its way to where this build keeps it.
pub struct TokenKeep {
    /// What [`abandon_keep`] takes; `None` when nothing is in flight.
    pub ticket: Option<KeepTicket>,
    pub outcome: oneshot::Receiver<KeepOutcome>,
}

/// Keeps a token from the device flow where this build keeps them.
pub fn keep_signed_in_token(
    environment: &GithubEnvironment,
    secret: String,
    cx: &mut App,
) -> TokenKeep {
    let token = GithubToken::new(&secret, TokenSource::Keychain);
    match environment.token_storage {
        TokenStorage::MemoryOnly => {
            set_session_token(Some(token), cx);
            let (outcome_sender, outcome) = oneshot::channel();
            outcome_sender.send(KeepOutcome::Kept { note: None }).ok();
            TokenKeep {
                ticket: None,
                outcome,
            }
        }
        TokenStorage::Keychain => {
            set_session_token(None, cx);
            let queue = cx.default_global::<KeychainQueue>();
            queue.next_ticket += 1;
            let ticket = KeepTicket(queue.next_ticket);
            queue.in_flight.insert(ticket.0, false);
            let credentials = environment.credentials.clone();
            let outcome = enqueue(cx, async move |cx| {
                let outcome = write_unless_abandoned(ticket, credentials, secret, token, cx).await;
                cx.update(|cx| finish_keep(ticket, cx));
                outcome
            });
            TokenKeep {
                ticket: Some(ticket),
                outcome,
            }
        }
    }
}

async fn write_unless_abandoned(
    ticket: KeepTicket,
    credentials: Arc<dyn CredentialsProvider>,
    secret: String,
    token: GithubToken,
    cx: &mut AsyncApp,
) -> KeepOutcome {
    if cx.update(|cx| is_abandoned(ticket, cx)) {
        return KeepOutcome::Abandoned;
    }
    let written = cx
        .update(|cx| write_keychain_token(credentials.clone(), secret, cx))
        .await;
    if cx.update(|cx| is_abandoned(ticket, cx)) {
        if written.is_ok()
            && let Err(error) = cx.update(|cx| delete_keychain_token(credentials, cx)).await
        {
            log::error!("could not remove an abandoned GitHub token: {error}");
        }
        return KeepOutcome::Abandoned;
    }
    if let Err(error) = written {
        log::error!("could not save the GitHub token to the keychain: {error}");
        cx.update(|cx| set_session_token(Some(token), cx));
        return KeepOutcome::Kept {
            note: Some("The system keychain refused the token, so you will be asked to sign in again next time.".to_string()),
        };
    }
    KeepOutcome::Kept { note: None }
}

/// Drops Zode's own token, wherever it was kept. Never touches `gh`'s.
pub fn forget_signed_in_token(
    environment: &GithubEnvironment,
    cx: &mut App,
) -> Task<anyhow::Result<()>> {
    abandon_all_keeps(cx);
    set_session_token(None, cx);
    let credentials = environment.credentials.clone();
    let deleted = enqueue(cx, async move |cx| {
        cx.update(|cx| delete_keychain_token(credentials, cx)).await
    });
    cx.spawn(async move |_| {
        deleted
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("the keychain operation was dropped")))
    })
}
