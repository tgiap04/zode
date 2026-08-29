use std::sync::Arc;
use std::time::SystemTime;

use credentials_provider::CredentialsProvider;
use futures::AsyncReadExt as _;
use gpui::{App, Entity, EventEmitter, Global, SharedString, Task};
use http_client::{AsyncBody, HttpClient, Request};
use serde::Deserialize;

use crate::device_flow::{self, DeviceFlowError};
use crate::storage;
use crate::tokens::StoredTokens;

/// Who is signed in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountUser {
    pub id: SharedString,
    pub email: SharedString,
    pub name: Option<SharedString>,
    pub avatar_url: Option<SharedString>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountStatus {
    SignedOut,
    /// A sign-in is in flight and the user has something to do about it.
    WaitingForApproval {
        user_code: SharedString,
        verification_uri: SharedString,
        verification_uri_complete: SharedString,
    },
    SignedIn(AccountUser),
    /// Signed in, but the service cannot be reached. The identity is what was
    /// last known; it is deliberately kept rather than discarded, because
    /// losing the network is not the same as losing the account.
    Offline(AccountUser),
}

impl AccountStatus {
    pub fn user(&self) -> Option<&AccountUser> {
        match self {
            Self::SignedIn(user) | Self::Offline(user) => Some(user),
            Self::SignedOut | Self::WaitingForApproval { .. } => None,
        }
    }

    pub fn is_signed_in(&self) -> bool {
        matches!(self, Self::SignedIn(_) | Self::Offline(_))
    }
}

/// Emitted whenever `status` changes, so the rail and any modal can observe
/// one entity instead of polling it.
pub struct AccountStatusChanged;

struct GlobalAccount(Entity<Account>);
impl Global for GlobalAccount {}

/// The account, and nothing else — no UI, no settings sync.
///
/// # The invariant this type exists to hold
///
/// **While signed out, this crate performs no network requests at all.** Not at
/// startup, not on a timer, not to warm anything. Signing in is optional in
/// Zode, and an editor that phones home before you have an account is not
/// optional in any meaningful sense. `offline_invariants.rs` asserts it by
/// counting requests rather than by reading the code.
///
/// The corollary: there is no background polling anywhere in here. Every
/// request is the direct consequence of something the user did.
pub struct Account {
    status: AccountStatus,
    http_client: Arc<dyn HttpClient>,
    credentials: Arc<dyn CredentialsProvider>,
    api_url: String,
    tokens: Option<StoredTokens>,
    /// Held so dropping the entity — or cancelling the sign-in — stops the
    /// poll. A detached task would keep asking after the modal was closed.
    sign_in_task: Option<Task<()>>,
}

impl EventEmitter<AccountStatusChanged> for Account {}

#[derive(Deserialize)]
struct MeResponse {
    id: String,
    email: String,
    name: Option<String>,
    #[serde(rename = "avatarUrl")]
    avatar_url: Option<String>,
}

impl Account {
    pub fn new(
        http_client: Arc<dyn HttpClient>,
        credentials: Arc<dyn CredentialsProvider>,
        api_url: String,
    ) -> Self {
        Self {
            status: AccountStatus::SignedOut,
            http_client,
            credentials,
            api_url,
            tokens: None,
            sign_in_task: None,
        }
    }

    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalAccount>()
            .map(|global| global.0.clone())
    }

    pub fn set_global(account: Entity<Self>, cx: &mut App) {
        cx.set_global(GlobalAccount(account));
    }

    pub fn status(&self) -> &AccountStatus {
        &self.status
    }

    /// Restores a session saved by a previous run.
    ///
    /// Reads the keychain first and only calls `/auth/me` when it finds
    /// something. A machine that has never signed in reaches no network here —
    /// that ordering IS the invariant, not an optimization of it.
    pub fn restore(&mut self, cx: &mut gpui::Context<Self>) -> Task<()> {
        let credentials = self.credentials.clone();
        let http_client = self.http_client.clone();
        let api_url = self.api_url.clone();

        cx.spawn(async move |this, cx| {
            let Some(stored) = storage::read(&credentials, cx).await else {
                return;
            };

            let refreshed =
                Self::ensure_fresh(&http_client, &api_url, stored, SystemTime::now()).await;
            match refreshed {
                Ok(tokens) => {
                    let identity =
                        fetch_identity(&http_client, &api_url, &tokens.access_token).await;
                    _ = this.update(cx, |this, cx| match identity {
                        Ok(user) => {
                            this.tokens = Some(tokens);
                            this.set_status(AccountStatus::SignedIn(user), cx);
                        }
                        Err(IdentityError::Unauthorized) => {
                            this.forget_locally(cx);
                        }
                        Err(IdentityError::Unreachable(_)) => {
                            // No identity to show and no way to get one, but
                            // the credential is still good. Stay signed out
                            // visually rather than inventing a user.
                            this.tokens = Some(tokens);
                        }
                    });
                }
                Err(RefreshOutcome::Rejected) => {
                    _ = this.update(cx, |this, cx| this.forget_locally(cx));
                }
                Err(RefreshOutcome::Unreachable) => {
                    // Keep the credential; the user simply is not online.
                }
            }
        })
    }

    /// Begins a device sign-in.
    ///
    /// Moves to `WaitingForApproval` as soon as there is a code to display,
    /// then polls in the background until the user decides.
    pub fn sign_in(&mut self, cx: &mut gpui::Context<Self>) {
        if self.status.is_signed_in() {
            return;
        }

        let http_client = self.http_client.clone();
        let credentials = self.credentials.clone();
        let api_url = self.api_url.clone();

        self.sign_in_task = Some(cx.spawn(async move |this, cx| {
            let pending = match device_flow::request_authorization(&http_client, &api_url).await {
                Ok(pending) => pending,
                Err(error) => {
                    log::warn!("could not start a device sign-in: {error}");
                    _ = this.update(cx, |this, cx| this.set_status(AccountStatus::SignedOut, cx));
                    return;
                }
            };

            _ = this.update(cx, |this, cx| {
                this.set_status(
                    AccountStatus::WaitingForApproval {
                        user_code: pending.user_code.clone().into(),
                        verification_uri: pending.verification_uri.clone().into(),
                        verification_uri_complete: pending.verification_uri_complete.clone().into(),
                    },
                    cx,
                );
            });

            let executor = cx.background_executor().clone();
            let outcome =
                device_flow::poll_until_authorized(&http_client, &api_url, &pending, &executor)
                    .await;

            let tokens = match outcome {
                Ok(tokens) => tokens,
                Err(error) => {
                    log::info!("device sign-in ended: {error}");
                    _ = this.update(cx, |this, cx| this.set_status(AccountStatus::SignedOut, cx));
                    return;
                }
            };

            match fetch_identity(&http_client, &api_url, &tokens.access_token).await {
                Ok(user) => {
                    if let Err(error) = storage::write(&credentials, &user.id, &tokens, cx).await {
                        // The session works for this run; it just will not
                        // survive a restart. Better than refusing the sign-in.
                        log::warn!("signed in, but the session could not be saved: {error}");
                    }
                    _ = this.update(cx, |this, cx| {
                        this.tokens = Some(tokens);
                        this.set_status(AccountStatus::SignedIn(user), cx);
                    });
                }
                Err(error) => {
                    log::warn!("signed in, but the account could not be read: {error}");
                    _ = this.update(cx, |this, cx| this.set_status(AccountStatus::SignedOut, cx));
                }
            }
        }));
    }

    /// Abandons a sign-in in progress. Dropping the task stops the polling.
    pub fn cancel_sign_in(&mut self, cx: &mut gpui::Context<Self>) {
        self.sign_in_task = None;
        if matches!(self.status, AccountStatus::WaitingForApproval { .. }) {
            self.set_status(AccountStatus::SignedOut, cx);
        }
    }

    /// Signs out.
    ///
    /// Local state is cleared first and unconditionally. Telling the server is
    /// best effort: a user who pressed Sign Out with the network down must
    /// still end up signed out.
    pub fn sign_out(&mut self, cx: &mut gpui::Context<Self>) -> Task<()> {
        let tokens = self.tokens.take();
        let credentials = self.credentials.clone();
        let http_client = self.http_client.clone();
        let api_url = self.api_url.clone();

        self.sign_in_task = None;
        self.set_status(AccountStatus::SignedOut, cx);

        cx.spawn(async move |_, cx| {
            storage::delete(&credentials, cx).await;
            if let Some(tokens) = tokens {
                device_flow::revoke(&http_client, &api_url, &tokens.refresh_token).await;
            }
        })
    }

    /// Drops the local session without touching the network. Used when the
    /// server has already told us the credential is dead.
    fn forget_locally(&mut self, cx: &mut gpui::Context<Self>) {
        self.tokens = None;
        self.set_status(AccountStatus::SignedOut, cx);
        let credentials = self.credentials.clone();
        cx.spawn(async move |_, cx| storage::delete(&credentials, cx).await)
            .detach();
    }

    fn set_status(&mut self, status: AccountStatus, cx: &mut gpui::Context<Self>) {
        if self.status == status {
            return;
        }
        self.status = status;
        cx.emit(AccountStatusChanged);
        cx.notify();
    }

    /// Refreshes the access token when it is close to expiry.
    ///
    /// The two failure modes are kept apart on purpose. A rejection means the
    /// credential is genuinely dead and the user must be signed out; anything
    /// else means the network is down, and signing someone out for a flaky
    /// connection is the bug this distinction exists to prevent.
    async fn ensure_fresh(
        http_client: &Arc<dyn HttpClient>,
        api_url: &str,
        tokens: StoredTokens,
        now: SystemTime,
    ) -> Result<StoredTokens, RefreshOutcome> {
        if !tokens.needs_refresh(now) {
            return Ok(tokens);
        }

        match device_flow::refresh(http_client, api_url, &tokens.refresh_token).await {
            Ok(fresh) => Ok(fresh),
            Err(DeviceFlowError::Unreachable(_)) => Err(RefreshOutcome::Unreachable),
            Err(_) => Err(RefreshOutcome::Rejected),
        }
    }
}

enum RefreshOutcome {
    Rejected,
    Unreachable,
}

enum IdentityError {
    Unauthorized,
    Unreachable(String),
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized => write!(f, "the session was rejected"),
            Self::Unreachable(detail) => write!(f, "{detail}"),
        }
    }
}

async fn fetch_identity(
    http_client: &Arc<dyn HttpClient>,
    api_url: &str,
    access_token: &str,
) -> Result<AccountUser, IdentityError> {
    let request = Request::builder()
        .uri(format!("{api_url}/auth/me"))
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {access_token}"))
        .body(AsyncBody::default())
        .map_err(|_| {
            IdentityError::Unreachable("the identity request could not be built".into())
        })?;

    let mut response = http_client.send(request).await.map_err(|_| {
        IdentityError::Unreachable("the account service could not be reached".into())
    })?;

    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(IdentityError::Unauthorized);
    }
    if !status.is_success() {
        return Err(IdentityError::Unreachable(format!(
            "the account service answered {}",
            status.as_u16()
        )));
    }

    let mut body = String::new();
    if response.body_mut().read_to_string(&mut body).await.is_err() {
        return Err(IdentityError::Unreachable(
            "the identity response could not be read".into(),
        ));
    }

    let parsed: MeResponse = serde_json::from_str(&body).map_err(|_| {
        IdentityError::Unreachable("the identity response could not be parsed".into())
    })?;

    Ok(AccountUser {
        id: parsed.id.into(),
        email: parsed.email.into(),
        name: parsed.name.map(Into::into),
        avatar_url: parsed.avatar_url.map(Into::into),
    })
}
