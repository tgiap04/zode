use std::sync::Arc;

use gpui::{App, Context, Entity, EventEmitter, Global, SharedString, Task};
use zode_account::{Account, PlanLimit};

use crate::artifact::Artifact;
use crate::dek::Dek;
use crate::diff::TextDiff;
use crate::envelope::Kind;
use crate::extensions;
use crate::keystore;
use crate::recovery_key;
use crate::rotate;
use crate::sync::{self, PullOutcome, PushOutcome, SyncContext, SyncError};

/// A difference waiting on the user.
///
/// Held rather than acted on: applying it either overwrites the local file or
/// overwrites the server's, and neither is a decision this crate gets to make.
pub struct PendingDivergence {
    pub kind: Kind,
    pub diff: TextDiff,
    pub remote: String,
    pub revision: String,
    /// True when the local file has not been touched since the last sync, so
    /// taking the remote costs nothing. The modal says so; it does not decide.
    pub safe_to_apply: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncStatus {
    Idle,
    Working,
    /// Something finished and there is a sentence to show.
    Done(SharedString),
    /// No key on this machine yet.
    NeedsKey,
    /// Data on the server was written with a key this machine does not have.
    KeyMismatch,
    /// The account's plan does not cover this (HTTP 402). Never a reason to
    /// sign in again; the window offers the plan pages instead.
    PlanLimited(PlanLimit),
    Failed(SharedString),
}

pub struct SyncStatusChanged;

/// How a recovery-key replacement ended. Both carry the new recovery key.
#[derive(Debug)]
pub enum KeyRotation {
    /// Everything stored is under the new key, and it is this machine's key.
    Complete { phrase: String },
    /// Stopped before the end; running it again continues with this same
    /// key. When `adopted`, the new key is already this machine's key (every
    /// key slot moved), so the phrase must be written down now. When not, the
    /// old recovery key still opens everything.
    Interrupted {
        phrase: String,
        adopted: bool,
        reason: String,
    },
}

struct GlobalSyncSession(Entity<SyncSession>);
impl Global for GlobalSyncSession {}

/// Everything the sync UI talks to.
///
/// # The invariants this type exists to hold
///
/// **Nothing here runs on its own.** No sync at startup, no polling, no
/// background reconciliation. Every request below is the direct consequence of
/// a button the user pressed — the same promise `Account` makes about signing
/// in, extended to the data.
///
/// **A failed decryption never reaches a write.** `pull` returns an outcome;
/// writing is a separate call that takes already-decrypted text. That is
/// structural, not a rule someone has to remember.
pub struct SyncSession {
    account: Entity<Account>,
    /// `Arc` so a spawned task can hold the key without copying its bytes into
    /// a second allocation that nothing zeroes.
    dek: Option<Arc<Dek>>,
    status: SyncStatus,
    pending: Option<PendingDivergence>,
    /// Last known installed set, so a decision taken in the diff window is
    /// compared against the same list the pull was made from.
    installed_extensions: Vec<String>,
    /// Stored elsewhere, absent here. Offered to the user; never acted on.
    missing_extensions: Vec<String>,
    task: Option<Task<()>>,
}

impl EventEmitter<SyncStatusChanged> for SyncSession {}

impl SyncSession {
    pub fn new(account: Entity<Account>) -> Self {
        Self {
            account,
            dek: None,
            status: SyncStatus::Idle,
            pending: None,
            installed_extensions: Vec::new(),
            missing_extensions: Vec::new(),
            task: None,
        }
    }

    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalSyncSession>()
            .map(|global| global.0.clone())
    }

    pub fn set_global(session: Entity<Self>, cx: &mut App) {
        cx.set_global(GlobalSyncSession(session));
    }

    pub fn status(&self) -> &SyncStatus {
        &self.status
    }

    pub fn pending(&self) -> Option<&PendingDivergence> {
        self.pending.as_ref()
    }

    pub fn has_key(&self) -> bool {
        self.dek.is_some()
    }

    /// Loads the key from the keychain if it is there.
    ///
    /// Reads the OS keychain and nothing else — no network — so calling it on
    /// menu open costs nothing and cannot breach the no-request promise.
    pub fn load_key(&mut self, cx: &mut Context<Self>) -> Task<()> {
        if self.dek.is_some() {
            return Task::ready(());
        }
        let credentials = self.account.read(cx).credentials_provider();
        cx.spawn(async move |this, cx| {
            let found = keystore::read(&credentials, cx).await;
            _ = this.update(cx, |this, cx| {
                this.dek = found.map(Arc::new);
                if this.dek.is_none() && this.status == SyncStatus::Idle {
                    this.set_status(SyncStatus::NeedsKey, cx);
                }
                cx.notify();
            });
        })
    }

    /// Creates a key for a user who has never synced, and returns the recovery
    /// key to show them.
    ///
    /// Returns the string rather than storing it anywhere: it must be shown
    /// once, confirmed, and then exist only in the user's own records.
    pub fn create_key(&mut self, cx: &mut Context<Self>) -> Task<anyhow::Result<String>> {
        let credentials = self.account.read(cx).credentials_provider();
        let user_id = self
            .account
            .read(cx)
            .status()
            .user()
            .map(|user| user.id.to_string());

        cx.spawn(async move |this, cx| {
            let user_id = user_id.ok_or_else(|| anyhow::anyhow!("no account is signed in"))?;
            let dek = Dek::generate()?;
            let phrase = recovery_key::encode(&dek);

            // Written to the keychain before it is shown. A key the user was
            // told to write down but which this machine then failed to store
            // is the worst of both worlds.
            keystore::write(&credentials, &user_id, &dek, cx).await?;

            _ = this.update(cx, |this, cx| {
                this.dek = Some(Arc::new(dek));
                this.set_status(SyncStatus::Idle, cx);
            });
            Ok(phrase)
        })
    }

    /// Adopts a key the user typed in from another machine.
    pub fn accept_recovery_key(
        &mut self,
        phrase: String,
        cx: &mut Context<Self>,
    ) -> Task<anyhow::Result<()>> {
        let credentials = self.account.read(cx).credentials_provider();
        let user_id = self
            .account
            .read(cx)
            .status()
            .user()
            .map(|user| user.id.to_string());

        cx.spawn(async move |this, cx| {
            let user_id = user_id.ok_or_else(|| anyhow::anyhow!("no account is signed in"))?;
            let dek = recovery_key::decode(&phrase)?;
            keystore::write(&credentials, &user_id, &dek, cx).await?;
            _ = this.update(cx, |this, cx| {
                this.dek = Some(Arc::new(dek));
                this.set_status(SyncStatus::Idle, cx);
            });
            Ok(())
        })
    }

    /// Renders the current key for someone who needs to write it down again.
    pub fn reveal_recovery_key(&self) -> Option<String> {
        self.dek.as_ref().map(|dek| recovery_key::encode(dek))
    }

    /// Replaces the key and re-encrypts everything stored under it.
    ///
    /// This is the only mechanism that actually cuts a lost machine off from
    /// synced data. Revoking a device on the web ends its server session but
    /// leaves it holding a key that opens anything it already downloaded.
    ///
    /// Both keys are kept in the keychain (`keystore::write_rotation`) before
    /// the first server write and until everything is under the new one. A
    /// second call after an interruption continues with that same new key, so
    /// nothing is ever stranded under a key nobody holds.
    ///
    /// Every other machine will report `KeyRotated` on its next pull until it
    /// is given the new recovery key.
    pub fn rotate_key(&mut self, cx: &mut Context<Self>) -> Task<anyhow::Result<KeyRotation>> {
        let Some(current) = self.dek.clone() else {
            return Task::ready(Err(anyhow::anyhow!(
                "this machine has no recovery key to replace"
            )));
        };
        let credentials = self.account.read(cx).credentials_provider();
        let user_id = self
            .account
            .read(cx)
            .status()
            .user()
            .map(|user| user.id.to_string());
        let account = self.account.clone();

        cx.spawn(async move |this, cx| {
            let user_id = user_id.ok_or_else(|| anyhow::anyhow!("no account is signed in"))?;
            let Some(context) = build_context(&account, cx).await else {
                anyhow::bail!("this session could not be used — sign in again");
            };

            // An unfinished rotation of this key is continued, never restarted:
            // its new key may already seal something.
            let (old, new) = match keystore::read_rotation(&credentials, cx).await {
                Some((old, new)) if old.kid() == current.kid() || new.kid() == current.kid() => {
                    (Arc::new(old), Arc::new(new))
                }
                _ => {
                    let new = Dek::generate()?;
                    keystore::write_rotation(&credentials, &user_id, &current, &new, cx)
                        .await
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "nothing was changed: the new key could not be kept in the keychain ({error})"
                            )
                        })?;
                    (current, Arc::new(new))
                }
            };
            let phrase = recovery_key::encode(&new);
            let state_path = paths::sync_state_file().clone();

            let outcome = rotate::rotate(&context, &old, &new, &state_path, || {
                let credentials = credentials.clone();
                let user_id = user_id.clone();
                let new = new.clone();
                let this = this.clone();
                let mut cx = cx.clone();
                async move {
                    if let Err(error) = keystore::write(&credentials, &user_id, &new, &cx).await {
                        log::warn!("rotated the key but could not save it: {error}");
                    }
                    _ = this.update(&mut cx, |this, _| this.dek = Some(new));
                }
            })
            .await;

            let adopted = this
                .read_with(cx, |this, _| {
                    this.dek.as_ref().is_some_and(|dek| dek.kid() == new.kid())
                })
                .unwrap_or(false);
            let reason = match outcome {
                Ok(rotate::RotationOutcome::Complete) => {
                    if let Err(error) = keystore::delete_rotation(&credentials, cx).await {
                        log::warn!("could not forget the finished key rotation: {error}");
                    }
                    _ = this.update(cx, |this, cx| {
                        this.set_status(SyncStatus::Done("recovery key replaced".into()), cx)
                    });
                    return Ok(KeyRotation::Complete { phrase });
                }
                Ok(rotate::RotationOutcome::Interrupted { remaining }) => format!(
                    "another machine wrote while the key was being replaced; {} item(s) still use the old key",
                    remaining.len()
                ),
                Err(error) => error.to_string(),
            };
            Ok(KeyRotation::Interrupted {
                phrase,
                adopted,
                reason,
            })
        })
    }

    /// Forgets the key on this machine only. The server copy is untouched —
    /// this is not a way to delete synced data.
    pub fn forget_key(&mut self, cx: &mut Context<Self>) -> Task<anyhow::Result<()>> {
        self.dek = None;
        let credentials = self.account.read(cx).credentials_provider();
        self.set_status(SyncStatus::NeedsKey, cx);
        cx.spawn(async move |_, cx| {
            keystore::delete_rotation(&credentials, cx).await?;
            keystore::delete(&credentials, cx).await
        })
    }

    pub fn pull(&mut self, kind: Kind, cx: &mut Context<Self>) {
        self.run(kind, Direction::Pull, cx);
    }

    pub fn push(&mut self, kind: Kind, cx: &mut Context<Self>) {
        self.run(kind, Direction::Push, cx);
    }

    /// Extensions installed elsewhere and missing here.
    ///
    /// A list to read, never a list that has been acted on. Installing is the
    /// caller's move, and only after the user asks for it — see invariant 7.
    pub fn missing_extensions(&self) -> &[String] {
        &self.missing_extensions
    }

    /// Sends the installed set. The list comes from the caller because
    /// `zode_sync` must not depend on `extension_host`: that crate can reach
    /// `telemetry`, and the sync crates are held to a graph rule that says
    /// they cannot.
    pub fn push_extensions(&mut self, installed: Vec<String>, cx: &mut Context<Self>) {
        self.run_content(
            Kind::Extensions,
            extensions::render(installed),
            Direction::Push,
            cx,
        );
    }

    /// Fetches the stored set and reports what is missing here.
    ///
    /// **Installs nothing.** The outcome is a list and a sentence.
    pub fn pull_extensions(&mut self, installed: Vec<String>, cx: &mut Context<Self>) {
        self.installed_extensions = installed.clone();
        self.run_content(
            Kind::Extensions,
            extensions::render(installed),
            Direction::Pull,
            cx,
        );
    }

    /// Takes the server's version, after backing up what is being replaced.
    pub fn apply_pending(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending.take() else {
            return;
        };

        // Invariant 7. There is deliberately no path from here to an install:
        // a sync payload that could install code would be a supply-chain hole
        // with the user's own account as the key.
        if pending.kind == Kind::Extensions {
            self.set_status(
                SyncStatus::Done(
                    format!(
                        "{} extension(s) from your account are not installed here",
                        self.missing_extensions.len()
                    )
                    .into(),
                ),
                cx,
            );
            return;
        }

        let artifact = Artifact::for_kind(pending.kind);
        let state_path = paths::sync_state_file().clone();

        let result = sync::apply_remote(&artifact, &pending.remote, pending.revision, &state_path);
        match result {
            Ok(()) => self.set_status(
                SyncStatus::Done(format!("{} replaced from the server", pending.kind).into()),
                cx,
            ),
            Err(error) => self.set_status(SyncStatus::Failed(error.to_string().into()), cx),
        }
    }

    /// Keeps the local file and overwrites the server's copy.
    ///
    /// Still a conditional write against the revision the conflict reported: a
    /// third machine that wrote in between conflicts again rather than losing
    /// its work to a decision made about older content.
    pub fn keep_local(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let Some(dek) = self.dek.clone() else {
            self.set_status(SyncStatus::NeedsKey, cx);
            return;
        };

        let kind = pending.kind;
        let revision = pending.revision;
        let account = self.account.clone();
        self.set_status(SyncStatus::Working, cx);

        self.task = Some(cx.spawn(async move |this, cx| {
            let Some(context) = build_context(&account, cx).await else {
                _ = this.update(cx, |this, cx| this.set_status(unavailable(), cx));
                return;
            };
            let artifact = Artifact::for_kind(kind);
            let state_path = paths::sync_state_file().clone();
            let outcome =
                sync::overwrite_remote(&context, &dek, &artifact, &revision, &state_path).await;
            _ = this.update(cx, |this, cx| this.absorb_push(kind, outcome, cx));
        }));
    }

    pub fn dismiss_pending(&mut self, cx: &mut Context<Self>) {
        self.pending = None;
        self.set_status(SyncStatus::Idle, cx);
    }

    fn run(&mut self, kind: Kind, direction: Direction, cx: &mut Context<Self>) {
        let Some(dek) = self.dek.clone() else {
            self.set_status(SyncStatus::NeedsKey, cx);
            return;
        };
        let account = self.account.clone();
        self.pending = None;
        self.set_status(SyncStatus::Working, cx);

        self.task = Some(cx.spawn(async move |this, cx| {
            let Some(context) = build_context(&account, cx).await else {
                _ = this.update(cx, |this, cx| this.set_status(unavailable(), cx));
                return;
            };
            let artifact = Artifact::for_kind(kind);
            let state_path = paths::sync_state_file().clone();

            match direction {
                Direction::Pull => {
                    let outcome = sync::pull(&context, &dek, &artifact, &state_path).await;
                    _ = this.update(cx, |this, cx| this.absorb_pull(kind, outcome, cx));
                }
                Direction::Push => {
                    let outcome = sync::push(&context, &dek, &artifact, &state_path).await;
                    _ = this.update(cx, |this, cx| this.absorb_push(kind, outcome, cx));
                }
            }
        }));
    }

    /// The same run, for content that does not come from a file.
    fn run_content(
        &mut self,
        kind: Kind,
        local: String,
        direction: Direction,
        cx: &mut Context<Self>,
    ) {
        let Some(dek) = self.dek.clone() else {
            self.set_status(SyncStatus::NeedsKey, cx);
            return;
        };
        let account = self.account.clone();
        self.pending = None;
        self.missing_extensions.clear();
        self.set_status(SyncStatus::Working, cx);

        self.task = Some(cx.spawn(async move |this, cx| {
            let Some(context) = build_context(&account, cx).await else {
                _ = this.update(cx, |this, cx| this.set_status(unavailable(), cx));
                return;
            };
            let state_path = paths::sync_state_file().clone();

            match direction {
                Direction::Pull => {
                    let outcome =
                        sync::pull_content(&context, &dek, kind, &local, &state_path).await;
                    _ = this.update(cx, |this, cx| this.absorb_pull(kind, outcome, cx));
                }
                Direction::Push => {
                    let outcome =
                        sync::push_content(&context, &dek, kind, &local, &state_path).await;
                    _ = this.update(cx, |this, cx| this.absorb_push(kind, outcome, cx));
                }
            }
        }));
    }

    /// Records which extensions the stored list has that this machine does not.
    fn note_missing_extensions(&mut self, remote: &str) {
        let Some(stored) = extensions::parse(remote) else {
            self.missing_extensions.clear();
            return;
        };
        self.missing_extensions = extensions::compare(&self.installed_extensions, &stored).missing;
    }

    fn absorb_pull(
        &mut self,
        kind: Kind,
        outcome: Result<PullOutcome, SyncError>,
        cx: &mut Context<Self>,
    ) {
        match outcome {
            Ok(PullOutcome::UpToDate) => {
                self.set_status(SyncStatus::Done(format!("{kind} is up to date").into()), cx)
            }
            Ok(PullOutcome::LocalOnly) => self.set_status(
                SyncStatus::Done(format!("nothing has been pushed for {kind} yet").into()),
                cx,
            ),
            Ok(PullOutcome::RemoteNewer(divergence)) => {
                if kind == Kind::Extensions {
                    self.note_missing_extensions(&divergence.remote);
                }
                self.pending = Some(PendingDivergence {
                    kind,
                    diff: divergence.diff,
                    remote: divergence.remote,
                    revision: divergence.revision,
                    safe_to_apply: true,
                });
                self.set_status(SyncStatus::Idle, cx);
            }
            Ok(PullOutcome::Conflict(divergence)) => {
                if kind == Kind::Extensions {
                    self.note_missing_extensions(&divergence.remote);
                }
                self.pending = Some(PendingDivergence {
                    kind,
                    diff: divergence.diff,
                    remote: divergence.remote,
                    revision: divergence.revision,
                    safe_to_apply: false,
                });
                self.set_status(SyncStatus::Idle, cx);
            }
            Ok(PullOutcome::KeyMismatch(_)) => self.set_status(SyncStatus::KeyMismatch, cx),
            Err(error) => self.fail(error, cx),
        }
    }

    fn absorb_push(
        &mut self,
        kind: Kind,
        outcome: Result<PushOutcome, SyncError>,
        cx: &mut Context<Self>,
    ) {
        match outcome {
            Ok(PushOutcome::Stored { .. }) => {
                self.set_status(SyncStatus::Done(format!("{kind} pushed").into()), cx)
            }
            Ok(PushOutcome::UpToDate) => {
                self.set_status(SyncStatus::Done(format!("{kind} is up to date").into()), cx)
            }
            Ok(PushOutcome::NothingToPush) => self.set_status(
                SyncStatus::Done(format!("there is no local {kind} to push").into()),
                cx,
            ),
            Ok(PushOutcome::Conflict(divergence)) => {
                self.pending = Some(PendingDivergence {
                    kind,
                    diff: divergence.diff,
                    remote: divergence.remote,
                    revision: divergence.revision,
                    safe_to_apply: false,
                });
                self.set_status(SyncStatus::Idle, cx);
            }
            Err(SyncError::Crypto(_)) => self.set_status(SyncStatus::KeyMismatch, cx),
            Err(error) => self.fail(error, cx),
        }
    }

    /// Records why a transfer did not happen. A plan refusal gets its own
    /// status and a fresh read of the plan, so the window can say which plan
    /// refused it; there is no retry, because the answer would be the same.
    fn fail(&mut self, error: SyncError, cx: &mut Context<Self>) {
        match error {
            SyncError::Client(crate::client::ClientError::PlanLimit(limit)) => {
                self.set_status(SyncStatus::PlanLimited(limit), cx);
                self.account
                    .update(cx, |account, cx| account.refresh_plan(cx));
            }
            error => self.set_status(SyncStatus::Failed(error.to_string().into()), cx),
        }
    }

    /// Parks the session in a given state so a window can be drawn at it.
    ///
    /// Reaching these states for real needs a server and a keychain; a
    /// rendering test has business with neither.
    #[cfg(feature = "test-support")]
    pub fn set_status_for_test(&mut self, status: SyncStatus, cx: &mut Context<Self>) {
        self.set_status(status, cx);
    }

    #[cfg(feature = "test-support")]
    pub fn set_pending_for_test(&mut self, pending: PendingDivergence, cx: &mut Context<Self>) {
        self.pending = Some(pending);
        cx.notify();
    }

    fn set_status(&mut self, status: SyncStatus, cx: &mut Context<Self>) {
        self.status = status;
        cx.emit(SyncStatusChanged);
        cx.notify();
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Pull,
    Push,
}

fn unavailable() -> SyncStatus {
    SyncStatus::Failed("sign in again — this session could not be used".into())
}

async fn build_context(account: &Entity<Account>, cx: &mut gpui::AsyncApp) -> Option<SyncContext> {
    // `api_credential` refreshes the access token when it is close to expiry,
    // and answers `None` for every reason the caller must not proceed on:
    // signed out, credential rejected, service unreachable.
    let credential = account
        .update(cx, |account, cx| account.api_credential(cx))
        .await?;
    let (http_client, api_url) = account.read_with(cx, |account, _| {
        (account.http_client(), account.api_url().to_string())
    });
    Some(SyncContext {
        http_client,
        api_url,
        credential,
    })
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use credentials_provider::CredentialsProvider;
    use gpui::{AppContext as _, AsyncApp, TestAppContext};
    use http_client::{FakeHttpClient, HttpClient, Response};
    use zode_account::{AccountStatus, AccountUser, PlanLimitReason};

    use super::*;
    use crate::client::ClientError;

    struct NoKeychain;

    impl CredentialsProvider for NoKeychain {
        fn read_credentials<'a>(
            &'a self,
            _url: &'a str,
            _cx: &'a AsyncApp,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Option<(String, Vec<u8>)>>> + 'a>> {
            Box::pin(async { Ok(None) })
        }

        fn write_credentials<'a>(
            &'a self,
            _url: &'a str,
            _username: &'a str,
            _password: &'a [u8],
            _cx: &'a AsyncApp,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + 'a>> {
            Box::pin(async { Ok(()) })
        }

        fn delete_credentials<'a>(
            &'a self,
            _url: &'a str,
            _cx: &'a AsyncApp,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + 'a>> {
            Box::pin(async { Ok(()) })
        }
    }

    /// A signed-in session whose account counts the plan reads it is asked for.
    fn signed_in_session(cx: &mut TestAppContext) -> (Entity<SyncSession>, Arc<AtomicUsize>) {
        let plan_reads = Arc::new(AtomicUsize::new(0));
        let counter = plan_reads.clone();
        let http_client: Arc<dyn HttpClient> = FakeHttpClient::create(move |request| {
            let counter = counter.clone();
            async move {
                if request.uri().path().ends_with("/account/plan") {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
                Ok(Response::builder()
                    .status(500)
                    .body(Default::default())
                    .expect("a well-formed response"))
            }
        });
        let account = cx.update(|cx| {
            cx.new(|_| {
                let mut account = Account::for_test_with(
                    AccountStatus::SignedIn(AccountUser {
                        id: "1".into(),
                        email: "ada@example.com".into(),
                        name: None,
                        avatar_url: None,
                    }),
                    http_client,
                    Arc::new(NoKeychain),
                );
                account.set_tokens_for_test();
                account
            })
        });
        let session = cx.update(|cx| cx.new(|_| SyncSession::new(account)));
        (session, plan_reads)
    }

    fn device_limit() -> PlanLimit {
        PlanLimit {
            reason: PlanLimitReason::SyncDeviceLimit,
            limit: Some(2),
            used: Some(3),
            upgrade_url: zode_account::pricing_url(),
        }
    }

    fn refused() -> SyncError {
        SyncError::Client(ClientError::PlanLimit(device_limit()))
    }

    #[gpui::test]
    fn a_push_the_plan_refuses_says_so_and_reads_the_plan(cx: &mut TestAppContext) {
        let (session, plan_reads) = signed_in_session(cx);
        session.update(cx, |session, cx| {
            session.absorb_push(Kind::Settings, Err(refused()), cx)
        });
        cx.run_until_parked();

        session.read_with(cx, |session, _| {
            assert_eq!(session.status(), &SyncStatus::PlanLimited(device_limit()));
        });
        assert_eq!(plan_reads.load(Ordering::SeqCst), 1);
    }

    #[gpui::test]
    fn a_pull_the_plan_refuses_says_so_and_reads_the_plan(cx: &mut TestAppContext) {
        let (session, plan_reads) = signed_in_session(cx);
        session.update(cx, |session, cx| {
            session.absorb_pull(Kind::Keymap, Err(refused()), cx)
        });
        cx.run_until_parked();

        session.read_with(cx, |session, _| {
            assert_eq!(session.status(), &SyncStatus::PlanLimited(device_limit()));
        });
        assert_eq!(plan_reads.load(Ordering::SeqCst), 1);
    }

    /// A refused session is still "sign in again", and is not a plan question.
    #[gpui::test]
    fn an_unauthorized_answer_is_not_a_plan_refusal(cx: &mut TestAppContext) {
        let (session, plan_reads) = signed_in_session(cx);
        session.update(cx, |session, cx| {
            session.absorb_push(
                Kind::Settings,
                Err(SyncError::Client(ClientError::Unauthorized)),
                cx,
            )
        });
        cx.run_until_parked();

        session.read_with(cx, |session, _| {
            assert!(
                matches!(session.status(), SyncStatus::Failed(_)),
                "{:?}",
                session.status()
            );
        });
        assert_eq!(plan_reads.load(Ordering::SeqCst), 0);
    }

    #[gpui::test]
    fn a_later_success_clears_the_refusal(cx: &mut TestAppContext) {
        let (session, _plan_reads) = signed_in_session(cx);
        session.update(cx, |session, cx| {
            session.absorb_push(Kind::Settings, Err(refused()), cx);
            session.absorb_push(
                Kind::Settings,
                Ok(PushOutcome::Stored {
                    revision: "r2".into(),
                }),
                cx,
            );
            assert!(
                matches!(session.status(), SyncStatus::Done(_)),
                "{:?}",
                session.status()
            );
        });
    }
}
