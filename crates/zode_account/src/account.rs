use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use credentials_provider::CredentialsProvider;
use futures::AsyncReadExt as _;
use futures::FutureExt as _;
use futures::future::Shared;
use gpui::{
    App, AsyncApp, Entity, EventEmitter, Global, SharedString, Subscription, Task, WeakEntity,
};
use http_client::{AsyncBody, HttpClient, Request};
use serde::Deserialize;

use crate::device_flow::{self, DeviceFlowError};
use crate::storage;
use crate::tokens::StoredTokens;

/// Who is signed in.
///
/// Serialisable because the keychain entry remembers it, so a start with no
/// network can still name the account rather than looking signed out.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

/// A live credential for calling the API as the signed-in user.
///
/// `user_id` travels with the token because the sync layer binds it into the
/// AAD of every envelope it writes — without it the server could serve one
/// user's blob into another user's slot and the tag would still verify.
#[derive(Clone)]
pub struct ApiCredential {
    pub access_token: String,
    pub user_id: SharedString,
}

impl std::fmt::Debug for ApiCredential {
    /// A derived `Debug` would print the access token into any log line or
    /// failed assertion that formats a value holding this one.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiCredential")
            .field("user_id", &self.user_id)
            .field("access_token", &"<redacted>")
            .finish()
    }
}

impl std::fmt::Display for ApiCredential {
    /// Shape, never the token — same reasoning as `StoredTokens`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ApiCredential {{ user: {}, access: <redacted> }}",
            self.user_id
        )
    }
}

/// Pauses between the attempts to write a rotated session before giving up and
/// removing the entry. A keychain that was merely busy takes the second or
/// third try; one that is locked takes none of them, and the cost is a second.
const SAVE_ATTEMPT_PAUSES: [Duration; 2] = [Duration::from_millis(250), Duration::from_millis(750)];

/// The wait before a save that failed is tried again by a credential request,
/// doubled by every further failure up to the cap. A keychain that never
/// answers (a Linux machine with no secret service) is then asked a few times an
/// hour, not on every request.
const SAVE_RETRY_BASE: Duration = Duration::from_secs(30);
const SAVE_RETRY_CAP: Duration = Duration::from_secs(15 * 60);

/// A rotation the keychain has not taken yet, and when it may be tried again.
struct UnsavedRotation {
    failures: u32,
    retry_not_before: Instant,
}

impl UnsavedRotation {
    fn after_failure(failures: u32, now: Instant) -> Self {
        let doublings = failures.saturating_sub(1).min(16);
        let wait = (SAVE_RETRY_BASE * 2u32.pow(doublings)).min(SAVE_RETRY_CAP);
        Self {
            failures,
            retry_not_before: now + wait,
        }
    }
}

/// What came of an attempt to keep the keychain current.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Persisted {
    /// Nothing needed writing, or a retry was not yet due.
    NotAttempted,
    Saved,
    Failed,
}

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
    /// A refresh already running, joined by every caller that arrives while it
    /// does.
    ///
    /// Not an optimisation. The refresh token rotates on use and the server
    /// reads a second use of a spent one as theft, revoking the whole family —
    /// so two callers refreshing at once do not waste a request, they sign the
    /// user out. `zode_sync` and `zode_env_sync` reach for a credential
    /// independently and know nothing about each other, which is exactly the
    /// race this closes.
    refreshing: Option<Shared<Task<Option<ApiCredential>>>>,
    /// Counts the sessions this account has ended, by sign-out or by the server
    /// refusing the credential.
    ///
    /// A task that began under one session carries the number it saw and checks
    /// it after every await. Without that, a refresh already on the wire when
    /// the user signs out lands afterwards and writes the session back, and the
    /// next start signs them in again.
    session_generation: u64,
    /// Set while the keychain does not hold what `tokens` holds, because a save
    /// after a rotation failed. A credential request writes it again once the
    /// backoff allows, and quitting makes a last attempt.
    unsaved_rotation: Option<UnsavedRotation>,
    /// Registered while `unsaved_rotation` is set, so quitting can write the
    /// session or, failing that, remove the entry.
    quit_flush: Option<Subscription>,
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
            refreshing: None,
            session_generation: 0,
            unsaved_rotation: None,
            quit_flush: None,
        }
    }

    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalAccount>()
            .map(|global| global.0.clone())
    }

    pub fn set_global(account: Entity<Self>, cx: &mut App) {
        cx.set_global(GlobalAccount(account));
    }

    /// Builds an account parked in a given state, with no client behind it.
    ///
    /// For rendering tests: the rail has five states to draw and no business
    /// standing up an HTTP client to draw them.
    #[cfg(feature = "test-support")]
    pub fn for_test(status: AccountStatus) -> Self {
        Self {
            status,
            http_client: Arc::new(http_client::BlockedHttpClient::new()),
            credentials: Arc::new(NoCredentials),
            api_url: "http://test.invalid/api".into(),
            tokens: None,
            sign_in_task: None,
            refreshing: None,
            session_generation: 0,
            unsaved_rotation: None,
            quit_flush: None,
        }
    }

    /// The same, but wired to a real client and keychain.
    ///
    /// For tests that COUNT requests rather than draw pictures. `for_test`
    /// blocks the network outright, which makes "no request was sent" true by
    /// construction and therefore worth nothing as an assertion.
    #[cfg(feature = "test-support")]
    pub fn for_test_with(
        status: AccountStatus,
        http_client: Arc<dyn HttpClient>,
        credentials: Arc<dyn CredentialsProvider>,
    ) -> Self {
        Self {
            status,
            http_client,
            credentials,
            api_url: "http://test.invalid/api".into(),
            tokens: None,
            sign_in_task: None,
            refreshing: None,
            session_generation: 0,
            unsaved_rotation: None,
            quit_flush: None,
        }
    }

    pub fn status(&self) -> &AccountStatus {
        &self.status
    }

    /// Gives the account a credential that is valid but meaningless.
    ///
    /// Lets a test reach the code paths that require one without standing up a
    /// whole device grant. The expiry is far enough out that `needs_refresh`
    /// answers false, so no refresh request is made either.
    /// The same, but already expired, so a refresh really is attempted.
    #[cfg(feature = "test-support")]
    pub fn set_expired_tokens_for_test(&mut self) {
        self.tokens = Some(StoredTokens {
            access_token: "test-access".into(),
            refresh_token: "test-refresh".into(),
            expires_at: SystemTime::now() - std::time::Duration::from_secs(1),
        });
    }

    #[cfg(feature = "test-support")]
    pub fn set_tokens_for_test(&mut self) {
        self.tokens = Some(StoredTokens {
            access_token: "test-access".into(),
            refresh_token: "test-refresh".into(),
            expires_at: SystemTime::now() + std::time::Duration::from_secs(3600),
        });
    }

    /// The HTTP client this account was built with.
    ///
    /// Exposed so `zode_sync` talks to the same client rather than
    /// constructing a second one — one client means one proxy configuration
    /// and one place a test can substitute a fake.
    pub fn http_client(&self) -> Arc<dyn HttpClient> {
        self.http_client.clone()
    }

    pub fn api_url(&self) -> &str {
        &self.api_url
    }

    /// The keychain the account is stored in, so the sync key can live beside
    /// it under its own entry.
    pub fn credentials_provider(&self) -> Arc<dyn CredentialsProvider> {
        self.credentials.clone()
    }

    /// Hands out a usable access token, refreshing it first if it is close to
    /// expiring.
    ///
    /// `None` means "cannot act as this user right now" and covers three
    /// different situations on purpose: signed out, credential rejected (the
    /// account is signed out as a side effect), and service unreachable. The
    /// caller's answer is the same in all three — do not proceed — while the
    /// user-facing message is the caller's to choose from `status`.
    ///
    /// This performs network I/O ONLY when a token is already held, so it
    /// cannot break the signed-out invariant: no tokens, no request, early
    /// return.
    pub fn api_credential(&mut self, cx: &mut gpui::Context<Self>) -> Task<Option<ApiCredential>> {
        let Some(tokens) = self.tokens.clone() else {
            return Task::ready(None);
        };
        // The whole user, not just the id: rewriting the entry after a
        // refresh must not drop the name this machine falls back on when it
        // starts with no network.
        let Some(user) = self.status.user().cloned() else {
            return Task::ready(None);
        };
        let user_id = user.id.clone();

        // A refresh already in flight is joined, never raced -- see the
        // `refreshing` field for why a second one signs the user out. A
        // finished one is not reused: the token it produced may itself be near
        // expiry by now, and deciding that is the whole job.
        if let Some(inflight) = self
            .refreshing
            .clone()
            .filter(|inflight| inflight.peek().is_none())
        {
            return cx.spawn(async move |_this, _cx| inflight.await);
        }

        let http_client = self.http_client.clone();
        let credentials = self.credentials.clone();
        let api_url = self.api_url.clone();
        let generation = self.session_generation;
        let now = cx.background_executor().now();
        let previous_failures = self
            .unsaved_rotation
            .as_ref()
            .map_or(0, |unsaved| unsaved.failures);
        let retry_unsaved = self
            .unsaved_rotation
            .as_ref()
            .is_some_and(|unsaved| now >= unsaved.retry_not_before);

        let refresh = cx
            .spawn(async move |this, cx| {
                let had = tokens.access_token.clone();
                match Self::ensure_fresh(&http_client, &api_url, tokens, SystemTime::now()).await {
                    Ok(fresh) => {
                        let refreshed = fresh.access_token != had;
                        // The user may have signed out while the request was
                        // on the wire. The server has rotated the token by now,
                        // so the new one is live, unknown to anyone, and must
                        // be revoked rather than kept.
                        if Self::session_ended_since(&this, generation, cx) {
                            if refreshed {
                                revoke_in_background(
                                    &http_client,
                                    &api_url,
                                    &fresh.refresh_token,
                                    cx,
                                );
                            }
                            return None;
                        }

                        // Persist immediately. A refresh that only lives in
                        // memory means the rotated refresh token is lost on
                        // restart, and the old one is already spent — which
                        // the server reads as reuse and revokes the family.
                        // A save that failed on an earlier call is made again
                        // here, on the same path, so two writers never overlap.
                        let persisted = if refreshed || retry_unsaved {
                            let saved = save_rotated_session(
                                &credentials,
                                EntryOwner::Known(&user),
                                &fresh,
                                previous_failures == 0,
                                cx,
                            )
                            .await;
                            if saved {
                                Persisted::Saved
                            } else {
                                Persisted::Failed
                            }
                        } else {
                            Persisted::NotAttempted
                        };

                        let access_token = fresh.access_token.clone();
                        let finished_at = cx.background_executor().now();
                        let session_ended = this
                            .update(cx, |this, cx| {
                                if this.session_generation != generation {
                                    return true;
                                }
                                this.tokens = Some(fresh.clone());
                                this.record_save(persisted, previous_failures, finished_at, cx);
                                false
                            })
                            .unwrap_or(false);
                        if session_ended {
                            // Signing out finished its delete before this
                            // write did, so the write has put the session back.
                            storage::delete(&credentials, cx).await;
                            if refreshed {
                                revoke_in_background(
                                    &http_client,
                                    &api_url,
                                    &fresh.refresh_token,
                                    cx,
                                );
                            }
                            return None;
                        }
                        Some(ApiCredential {
                            access_token,
                            user_id,
                        })
                    }
                    Err(RefreshOutcome::Rejected) => {
                        // Only the session this answer belongs to is forgotten;
                        // a newer one signed in since is none of its business.
                        _ = this.update(cx, |this, cx| {
                            if this.session_generation == generation {
                                this.forget_locally(cx);
                            }
                        });
                        None
                    }
                    Err(RefreshOutcome::Unreachable) => None,
                }
            })
            .shared();

        self.refreshing = Some(refresh.clone());
        // A driver of its own. Ending a session drops `refreshing`, and the
        // callers may have given up, but the request is already on the wire and
        // its answer has to be dealt with: applied, or its token revoked.
        cx.spawn({
            let refresh = refresh.clone();
            async move |_this, _cx| {
                refresh.await;
            }
        })
        .detach();
        cx.spawn(async move |_this, _cx| refresh.await)
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
            let Some(entry) = storage::read(&credentials, cx).await else {
                return;
            };
            let stored_user_id = entry.user_id;
            let remembered = entry.session.user;
            let presented_refresh_token = entry.session.tokens.refresh_token.clone();

            let refreshed = Self::ensure_fresh(
                &http_client,
                &api_url,
                entry.session.tokens,
                SystemTime::now(),
            )
            .await;
            match refreshed {
                Ok(tokens) => {
                    // The server spent the token we presented the moment it
                    // answered. Until the new one is on disk the keychain holds
                    // a spent token, and the next start presenting it is read as
                    // theft, which signs the user out everywhere. Hence before
                    // `/auth/me`: quitting during that call must not lose it, and
                    // neither must `/auth/me` failing. An entry that remembers
                    // no user is saved under the id it is already filed under.
                    let rotated = tokens.refresh_token != presented_refresh_token;
                    let mut persisted = Persisted::NotAttempted;
                    if rotated {
                        let owner = match &remembered {
                            Some(user) => EntryOwner::Known(user),
                            None => EntryOwner::FiledUnder(&stored_user_id),
                        };
                        persisted =
                            if save_rotated_session(&credentials, owner, &tokens, true, cx).await {
                                Persisted::Saved
                            } else {
                                Persisted::Failed
                            };
                    }

                    let identity =
                        fetch_identity(&http_client, &api_url, &tokens.access_token).await;
                    if rotated
                        && remembered.is_none()
                        && let Ok(user) = &identity
                    {
                        // Names the owner in an entry that had none. When the
                        // write above failed this is also its second attempt,
                        // with the same retries and the same removal on failure.
                        if persisted == Persisted::Failed {
                            if save_rotated_session(
                                &credentials,
                                EntryOwner::Known(user),
                                &tokens,
                                false,
                                cx,
                            )
                            .await
                            {
                                persisted = Persisted::Saved;
                            }
                        } else if let Err(error) =
                            storage::write(&credentials, user, &tokens, cx).await
                        {
                            log::warn!("could not record whose session was saved: {error}");
                        }
                    }
                    let finished_at = cx.background_executor().now();
                    _ = this.update(cx, |this, cx| match identity {
                        Ok(user) => {
                            this.tokens = Some(tokens);
                            this.record_save(persisted, 0, finished_at, cx);
                            this.set_status(AccountStatus::SignedIn(user), cx);
                        }
                        Err(IdentityError::Unauthorized) => {
                            this.forget_locally(cx);
                        }
                        Err(IdentityError::Unreachable(_)) => {
                            // The credential is good and the network is not.
                            // Naming the account this entry was written for is
                            // remembering, not inventing -- and the
                            // alternative was looking signed out while holding
                            // a perfectly valid session, with no way back,
                            // since nothing here polls to try again. The first
                            // real request corrects it either way: a
                            // credential the server actually rejects signs the
                            // user out then.
                            this.tokens = Some(tokens);
                            this.record_save(persisted, 0, finished_at, cx);
                            if let Some(user) = remembered {
                                this.set_status(AccountStatus::SignedIn(user), cx);
                            }
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
                    if let Err(error) = storage::write(&credentials, &user, &tokens, cx).await {
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
        self.end_session();
        self.set_status(AccountStatus::SignedOut, cx);

        // Deleting is the whole of what the keychain needs here. An entry whose
        // last rotation could not be saved is cleared by the same call, so no
        // second write is attempted: signing out is the one moment a write
        // would be wrong.
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
        self.end_session();
        self.set_status(AccountStatus::SignedOut, cx);
        let credentials = self.credentials.clone();
        cx.spawn(async move |_, cx| storage::delete(&credentials, cx).await)
            .detach();
    }

    /// Marks the session as over, so every task that began under it discards
    /// what it was about to do.
    ///
    /// `refreshing` goes too: a credential request in the next session must not
    /// join this one's pending refresh, which is certain to answer nothing. That
    /// refresh keeps running on the driver `api_credential` gave it.
    fn end_session(&mut self) {
        self.session_generation += 1;
        self.refreshing = None;
        self.unsaved_rotation = None;
        self.quit_flush = None;
    }

    /// Notes how the last attempt to keep the keychain current went.
    ///
    /// While a save is outstanding a quit observer stands by to settle it, and
    /// it is dropped the moment the keychain is current again.
    fn record_save(
        &mut self,
        persisted: Persisted,
        previous_failures: u32,
        now: Instant,
        cx: &mut gpui::Context<Self>,
    ) {
        match persisted {
            Persisted::NotAttempted => {}
            Persisted::Saved => {
                self.unsaved_rotation = None;
                self.quit_flush = None;
            }
            Persisted::Failed => {
                self.unsaved_rotation = Some(UnsavedRotation::after_failure(
                    previous_failures.saturating_add(1),
                    now,
                ));
                if self.quit_flush.is_none() {
                    self.quit_flush =
                        Some(cx.on_app_quit(|this, cx| this.flush_unsaved_rotation(cx)));
                }
            }
        }
    }

    /// The last chance to keep a rotation the keychain refused.
    ///
    /// Whatever the entry still holds is a token the server has spent, and the
    /// next start presenting it is read as theft. So the session is written once
    /// more, and if the keychain still refuses, the entry is removed. The app
    /// gives this about a tenth of a second, so there are no retries and no
    /// pauses here.
    fn flush_unsaved_rotation(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) -> impl std::future::Future<Output = ()> + use<> {
        let outstanding = self.unsaved_rotation.is_some();
        let tokens = self.tokens.clone();
        let user = self.status.user().cloned();
        let credentials = self.credentials.clone();
        let async_cx = cx.to_async();
        async move {
            if !outstanding {
                return;
            }
            if let (Some(tokens), Some(user)) = (&tokens, &user)
                && storage::write(&credentials, user, tokens, &async_cx)
                    .await
                    .is_ok()
            {
                return;
            }
            storage::delete(&credentials, &async_cx).await;
        }
    }

    /// Whether the session `generation` belongs to has ended since. An account
    /// that no longer exists has no session to protect, so it answers no.
    fn session_ended_since(this: &WeakEntity<Self>, generation: u64, cx: &AsyncApp) -> bool {
        this.read_with(cx, |this, _| this.session_generation != generation)
            .unwrap_or(false)
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
            Err(DeviceFlowError::Rejected(_)) => Err(RefreshOutcome::Rejected),
            // Anything that is not the server refusing the token keeps the
            // session: the safe direction to be wrong in.
            Err(error) => {
                log::warn!("could not refresh the session, keeping it: {error}");
                Err(RefreshOutcome::Unreachable)
            }
        }
    }
}

enum RefreshOutcome {
    Rejected,
    Unreachable,
}

/// How the session is filed in the keychain: under the user it names, or under
/// the id an entry that never recorded its user was already kept under.
enum EntryOwner<'a> {
    Known(&'a AccountUser),
    FiledUnder(&'a str),
}

/// Hands the revocation of a token to a task of its own, so that nobody who is
/// waiting on a credential also waits on a network round trip that only
/// tidies up. Failing is already logged by `device_flow::revoke`.
fn revoke_in_background(
    http_client: &Arc<dyn HttpClient>,
    api_url: &str,
    refresh_token: &str,
    cx: &AsyncApp,
) {
    let http_client = http_client.clone();
    let api_url = api_url.to_string();
    let refresh_token = refresh_token.to_string();
    cx.spawn(async move |_cx| device_flow::revoke(&http_client, &api_url, &refresh_token).await)
        .detach();
}

/// Writes a session whose refresh token has just rotated, and says whether the
/// keychain now holds it.
///
/// A failure does not sign the user out: the session works for this run, and a
/// locked keychain is no reason to stop. The write is tried three times, with a
/// pause between, because a keychain that is only busy takes the second. After
/// that, whatever the entry still holds is the token the server has just spent,
/// and the next start presenting it is read as theft, which revokes every
/// session the user has. So the entry is removed instead; the account keeps the
/// tokens in memory and writes them again later. `first_failure` keeps a
/// keychain that stays broken to one warning rather than one per request. The
/// messages name no token.
async fn save_rotated_session(
    credentials: &Arc<dyn CredentialsProvider>,
    owner: EntryOwner<'_>,
    tokens: &StoredTokens,
    first_failure: bool,
    cx: &AsyncApp,
) -> bool {
    let mut pauses = SAVE_ATTEMPT_PAUSES.iter();
    let error = loop {
        let written = match owner {
            EntryOwner::Known(user) => storage::write(credentials, user, tokens, cx).await,
            EntryOwner::FiledUnder(user_id) => {
                storage::write_without_user(credentials, user_id, tokens, cx).await
            }
        };
        match written {
            Ok(()) => return true,
            Err(error) => match pauses.next() {
                Some(pause) => cx.background_executor().timer(*pause).await,
                None => break error,
            },
        }
    };

    let removal = storage::try_delete(credentials, cx).await;
    let level = if first_failure {
        log::Level::Warn
    } else {
        log::Level::Debug
    };
    log::log!(
        level,
        "refreshed the session but could not save it ({error}); {}",
        match removal {
            Ok(()) => "the stale keychain entry was removed".to_string(),
            Err(error) => format!("the stale keychain entry could not be removed either ({error})"),
        }
    );
    false
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

/// A keychain that holds nothing, for tests that only need the account to
/// exist. `BlockedHttpClient` beside it makes the pair loud rather than quiet:
/// a rendering test that somehow issues a request fails instead of passing.
#[cfg(feature = "test-support")]
struct NoCredentials;

#[cfg(feature = "test-support")]
impl CredentialsProvider for NoCredentials {
    fn read_credentials<'a>(
        &'a self,
        _url: &'a str,
        _cx: &'a gpui::AsyncApp,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<Option<(String, Vec<u8>)>>> + 'a>,
    > {
        Box::pin(async { Ok(None) })
    }

    fn write_credentials<'a>(
        &'a self,
        _url: &'a str,
        _username: &'a str,
        _password: &'a [u8],
        _cx: &'a gpui::AsyncApp,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn delete_credentials<'a>(
        &'a self,
        _url: &'a str,
        _cx: &'a gpui::AsyncApp,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_prints_the_access_token() {
        let credential = ApiCredential {
            access_token: "super-secret-access".into(),
            user_id: "user-1".into(),
        };
        let rendered = format!("{credential:?}");
        assert!(!rendered.contains("super-secret-access"), "{rendered}");
        assert!(rendered.contains("user-1"), "the user id is not secret");
    }
}
