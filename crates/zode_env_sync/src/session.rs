use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{App, Context, Entity, EventEmitter, Global, SharedString, Task};
use util::ResultExt as _;
use zode_account::{Account, PlanLimit};
use zode_sync::client::ClientError;
use zode_sync::sync::SyncContext;
use zode_sync::{Dek, Resource, client, from_blob, to_blob};

use crate::env_dek::{self, EnvDek};
use crate::env_sync::{self, EnvFile, EnvSyncError, LocalEnvFile, PullOutcome, PushOutcome};
use crate::ids::EntryId;
use crate::manifest::Manifest;
use crate::{Bindings, Checkout, EnvCryptoError, keystore, seal};

/// A difference waiting on the user.
///
/// Held rather than acted on: applying it overwrites a file full of
/// credentials, and that is not a decision this crate gets to make.
pub struct PendingEnvDivergence {
    pub entry: EntryId,
    /// The file's path where it lives, which for a remote project is a path on
    /// that host. Shown, never opened on this machine: the write goes through
    /// the file the difference was read from.
    pub local_path: PathBuf,
    pub diff: zode_sync::TextDiff,
    pub remote: String,
    pub revision: String,
    pub seq: u64,
    /// True when the local file has not been touched since the last sync, so
    /// taking the remote costs nothing. The modal says so; it does not decide.
    pub safe_to_apply: bool,
    /// The hash of the local content `diff` was worked out against, `None`
    /// when there was no file. Applying refuses once the file holds anything
    /// else, because that is content the user was never shown.
    pub local_hash: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvStatus {
    Idle,
    Working,
    Done(SharedString),
    /// The account has no recovery key on this machine, so there is nothing to
    /// unwrap the environment key with.
    NeedsRecoveryKey,
    /// Stored data was written under an environment key this machine does not
    /// have — almost always a rotation elsewhere.
    KeyMismatch,
    /// The server offered a version older than one already applied here.
    ///
    /// Its own status rather than a `Failed` string, because it is the one
    /// failure that means somebody may be attacking the account rather than
    /// that something broke.
    Rollback {
        seen: u64,
        got: u64,
    },
    /// The account's plan does not cover this (HTTP 402). Its own status
    /// rather than a `Failed` string so the windows can offer the plan pages,
    /// and so nothing ever reads it as "sign in again".
    PlanLimited(PlanLimit),
    Failed(SharedString),
}

impl EnvStatus {
    /// One sentence for a status bar or a menu item.
    ///
    /// Every branch answers, so a new status cannot be added without deciding
    /// what the user is told about it.
    pub fn sentence(&self) -> SharedString {
        match self {
            EnvStatus::Idle => "ready".into(),
            EnvStatus::Working => "Syncing environment file…".into(),
            EnvStatus::Done(message) => message.clone(),
            EnvStatus::NeedsRecoveryKey => "enter your recovery key first".into(),
            EnvStatus::KeyMismatch => "encrypted with a different key".into(),
            EnvStatus::Rollback { seen, got } => format!(
                "the server offered version {got} of a file already at {seen} here — nothing was written"
            )
            .into(),
            EnvStatus::PlanLimited(limit) => limit.sentence().into(),
            EnvStatus::Failed(message) => message.clone(),
        }
    }
}

pub struct EnvStatusChanged;

struct GlobalEnvSession(Entity<EnvSession>);
impl Global for GlobalEnvSession {}

/// Everything the environment-sync UI talks to.
///
/// # The invariants this type exists to hold
///
/// **Nothing here runs on its own.** No sync at startup, no polling, no
/// background reconciliation — the same promise `SyncSession` makes, and it
/// matters more here.
///
/// **A refused pull never reaches a write.** `pull` returns an outcome;
/// writing is a separate call taking already-decrypted text. `KeyMismatch` and
/// `Rollback` cannot reach a write because the write is a different function.
pub struct EnvSession {
    account: Entity<Account>,
    /// `Arc` so a spawned task can hold the key without copying its bytes into
    /// a second allocation that nothing zeroes.
    env_dek: Option<Arc<EnvDek>>,
    manifest: Manifest,
    reconciliation: crate::Reconciliation,
    bindings: Bindings,
    status: EnvStatus,
    pending: Option<PendingEnvDivergence>,
    /// Where `pending` would be written. Kept beside it rather than inside it
    /// so the difference stays plain data the windows can build and show.
    pending_file: Option<Arc<dyn EnvFile>>,
    /// A push that has been built and shown, waiting to be sent.
    prepared: Option<(crate::env_sync::PreparedPush, Target)>,
    task: Option<Task<()>>,
    /// A write of `pending` to a remote host, on its way.
    ///
    /// A slot of its own: anything that replaced `task` would cancel the
    /// write mid-flight, and with it the step that clears `pending` and
    /// records the sync once the host has answered.
    applying: Option<Task<()>>,
    files: SessionFiles,
}

/// What every operation refused during a remote write says.
const APPLYING: &str = "a write to the host is still running";

/// Where the session keeps what it records on this machine.
#[derive(Clone)]
struct SessionFiles {
    bindings: PathBuf,
    state: PathBuf,
    backups: PathBuf,
}

impl SessionFiles {
    fn configured() -> Self {
        Self {
            bindings: paths::env_bindings_file().clone(),
            state: paths::env_state_file().clone(),
            backups: paths::env_backups_dir().clone(),
        }
    }
}

/// A file a difference was read from, and the path it is shown under.
#[derive(Clone)]
struct Target {
    file: Arc<dyn EnvFile>,
    shown_as: PathBuf,
}

impl Target {
    fn local(path: PathBuf) -> Self {
        Self {
            file: Arc::new(LocalEnvFile::new(path.clone())),
            shown_as: path,
        }
    }
}

/// Which held difference a write was started for, so its answer clears that
/// one and not a difference held since.
#[derive(PartialEq)]
struct DifferenceIdentity {
    entry: EntryId,
    local_path: PathBuf,
    revision: String,
    seq: u64,
    local_hash: Option<String>,
}

impl DifferenceIdentity {
    fn of(pending: &PendingEnvDivergence) -> Self {
        Self {
            entry: pending.entry,
            local_path: pending.local_path.clone(),
            revision: pending.revision.clone(),
            seq: pending.seq,
            local_hash: pending.local_hash.clone(),
        }
    }
}

impl EventEmitter<EnvStatusChanged> for EnvSession {}

impl EnvSession {
    pub fn new(account: Entity<Account>) -> Self {
        Self {
            account,
            env_dek: None,
            manifest: Manifest::new(),
            reconciliation: crate::Reconciliation::default(),
            bindings: Bindings::load(paths::env_bindings_file()),
            files: SessionFiles::configured(),
            status: EnvStatus::Idle,
            pending: None,
            pending_file: None,
            prepared: None,
            task: None,
            applying: None,
        }
    }

    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalEnvSession>()
            .map(|global| global.0.clone())
    }

    pub fn set_global(session: Entity<Self>, cx: &mut App) {
        cx.set_global(GlobalEnvSession(session));
    }

    pub fn status(&self) -> &EnvStatus {
        &self.status
    }

    pub fn pending(&self) -> Option<&PendingEnvDivergence> {
        self.pending.as_ref()
    }

    /// Whether the held difference is being written to a remote host.
    pub fn is_applying(&self) -> bool {
        self.applying.is_some()
    }

    /// Refuses, saying why, while a remote write is on its way.
    ///
    /// Once sent, that write cannot be called back. Dropping its difference
    /// would hide a write that still lands, holding another in its place would
    /// have the first one's answer clear it, and starting other work would
    /// report over the one outcome the user is waiting for.
    fn refuse_while_applying(&mut self, cx: &mut Context<Self>) -> bool {
        if self.applying.is_none() {
            return false;
        }
        self.set_status(EnvStatus::Failed(APPLYING.into()), cx);
        true
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn bindings(&self) -> &Bindings {
        &self.bindings
    }

    pub fn is_unlocked(&self) -> bool {
        self.env_dek.is_some()
    }

    /// Records which cloud project a checkout stands for.
    ///
    /// Local only, and saved immediately: a binding the user set and lost to a
    /// crash is worse than one they have to set twice.
    pub fn bind(
        &mut self,
        worktree_root: PathBuf,
        project: crate::ProjectId,
        cx: &mut Context<Self>,
    ) {
        self.bind_checkout(&Checkout::local(worktree_root), project, cx);
    }

    /// [`Self::bind`], for a checkout on this machine or on a remote host.
    pub fn bind_checkout(
        &mut self,
        checkout: &Checkout,
        project: crate::ProjectId,
        cx: &mut Context<Self>,
    ) {
        self.bindings.bind_checkout(checkout, project);
        if let Err(error) = self.bindings.save(&self.files.bindings) {
            self.set_status(EnvStatus::Failed(format!("{error}").into()), cx);
            return;
        }
        cx.notify();
    }

    /// Loads the environment key: keychain first, then the server.
    ///
    /// Reads the OS keychain and only reaches the network when the keychain
    /// has nothing — so opening the menu on a machine that has synced before
    /// costs no request at all.
    pub fn unlock(&mut self, cx: &mut Context<Self>) -> Task<()> {
        if self.env_dek.is_some() {
            return Task::ready(());
        }
        let account = self.account.clone();
        let credentials = account.read(cx).credentials_provider();

        cx.spawn(async move |this, cx| {
            if let Some(found) = keystore::read(&credentials, cx).await {
                _ = this.update(cx, |this, cx| {
                    this.env_dek = Some(Arc::new(found));
                    this.set_status(EnvStatus::Idle, cx);
                });
                return;
            }

            // Nothing cached. The wrapped key lives on the server and only the
            // recovery key opens it, so this machine must already hold that.
            let Some(dek) = zode_sync::keystore::read(&credentials, cx).await else {
                _ = this.update(cx, |this, cx| {
                    this.set_status(EnvStatus::NeedsRecoveryKey, cx);
                });
                return;
            };

            let Some(context) = build_context(&account, cx).await else {
                _ = this.update(cx, |this, cx| this.set_status(unavailable(), cx));
                return;
            };

            match fetch_env_key(&context, &dek).await {
                Ok(Some(env_dek)) => {
                    let stored =
                        keystore::write(&credentials, &context.credential.user_id, &env_dek, cx)
                            .await;
                    if let Err(error) = stored {
                        // Reported, not swallowed: the key still works for
                        // this run, but the next launch will fetch it again.
                        log::warn!("the env key could not be cached in the keychain: {error}");
                    }
                    _ = this.update(cx, |this, cx| {
                        this.env_dek = Some(Arc::new(env_dek));
                        this.set_status(EnvStatus::Idle, cx);
                    });
                }
                Ok(None) => {
                    // No environment key has ever been made for this account.
                    // Creating one is a deliberate act, not something a menu
                    // opening should do.
                    _ = this.update(cx, |this, cx| this.set_status(EnvStatus::Idle, cx));
                }
                Err(EnvSyncError::Crypto(
                    EnvCryptoError::KeyRotated { .. } | EnvCryptoError::WrongKey,
                )) => {
                    _ = this.update(cx, |this, cx| this.set_status(EnvStatus::KeyMismatch, cx));
                }
                Err(error) => {
                    _ = this.update(cx, |this, cx| this.fail(error, cx));
                }
            }
        })
    }

    /// Creates the environment key for an account that has never had one.
    ///
    /// Wrapped under the recovery key and pushed before it is cached locally,
    /// so a machine that crashes between the two ends up with no key rather
    /// than a key nothing else can reach.
    pub fn create_key(&mut self, cx: &mut Context<Self>) -> Task<anyhow::Result<()>> {
        let account = self.account.clone();
        let credentials = account.read(cx).credentials_provider();

        cx.spawn(async move |this, cx| {
            let dek = zode_sync::keystore::read(&credentials, cx)
                .await
                .ok_or_else(|| anyhow::anyhow!("this machine has no recovery key yet"))?;
            let context = build_context(&account, cx)
                .await
                .ok_or_else(|| anyhow::anyhow!("the sync service could not be reached"))?;

            let env_dek = EnvDek::generate()?;
            let wrapped = env_dek::wrap_key(&dek, &context.credential.user_id, &env_dek)
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            let blob = to_blob(&wrapped).map_err(|error| anyhow::anyhow!("{error}"))?;

            let stored = client::store(
                &context.http_client,
                &context.api_url,
                &context.credential.access_token,
                &Resource::env_singleton(crate::ENV_KEY_RESOURCE)
                    .map_err(|error| anyhow::anyhow!("{error}"))?,
                &blob,
                client::Precondition::Create,
            )
            .await;
            if let Err(error) = stored {
                // Said in the window as well as returned: the caller only logs
                // the error, and a plan refusal here is the one answer the user
                // can act on.
                let error = EnvSyncError::from(error);
                let message = error.to_string();
                this.update(cx, |this, cx| this.fail(error, cx))?;
                anyhow::bail!(message);
            }

            keystore::write(&credentials, &context.credential.user_id, &env_dek, cx).await?;
            this.update(cx, |this, cx| {
                this.env_dek = Some(Arc::new(env_dek));
                this.set_status(EnvStatus::Done("environment sync is ready".into()), cx);
            })?;
            Ok(())
        })
    }

    /// The bytes a push would put on the wire, if one is waiting.
    pub fn prepared(&self) -> Option<&crate::WireBytes> {
        self.prepared.as_ref().map(|(prepared, _)| &prepared.wire)
    }

    /// Builds a push and holds it, so the exact bytes can be shown first.
    ///
    /// Building and sending are separate calls for one reason: the panel that
    /// shows what leaves the machine must display the value that travels, not
    /// a second encryption of the same file. Two encryptions of one file
    /// differ — the nonce is fresh each time — so a panel that re-encrypted
    /// would be showing something the server never sees.
    pub fn prepare(&mut self, entry: EntryId, local_path: PathBuf, cx: &mut Context<Self>) {
        if self.refuse_while_applying(cx) {
            return;
        }
        let Some(env_dek) = self.env_dek.clone() else {
            self.set_status(EnvStatus::NeedsRecoveryKey, cx);
            return;
        };
        let Some(user_id) = self
            .account
            .read(cx)
            .status()
            .user()
            .map(|user| user.id.to_string())
        else {
            self.set_status(unavailable(), cx);
            return;
        };

        let prepared = crate::env_sync::prepare_push(
            &env_dek,
            &user_id,
            &entry,
            &local_path,
            &self.files.state,
        );
        self.hold_prepared(prepared, Target::local(local_path), cx);
    }

    /// [`Self::prepare`], for a file that may live on another host.
    ///
    /// Reading it may take a round-trip, so the push is held once the returned
    /// task finishes; [`Self::prepared`] is empty until then.
    pub fn prepare_file(
        &mut self,
        entry: EntryId,
        file: Arc<dyn EnvFile>,
        shown_as: PathBuf,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        if self.refuse_while_applying(cx) {
            return Task::ready(());
        }
        self.drop_stale_difference();
        let Some(env_dek) = self.env_dek.clone() else {
            self.set_status(EnvStatus::NeedsRecoveryKey, cx);
            return Task::ready(());
        };
        let Some(user_id) = self
            .account
            .read(cx)
            .status()
            .user()
            .map(|user| user.id.to_string())
        else {
            self.set_status(unavailable(), cx);
            return Task::ready(());
        };
        self.prepared = None;
        self.set_status(EnvStatus::Working, cx);
        let state_path = self.files.state.clone();

        cx.spawn(async move |this, cx| {
            let prepared = crate::env_sync::prepare_push_from(
                &env_dek,
                &user_id,
                &entry,
                file.as_ref(),
                &state_path,
            )
            .await;
            this.update(cx, |this, cx| {
                this.hold_prepared(prepared, Target { file, shown_as }, cx)
            })
            .log_err();
        })
    }

    fn hold_prepared(
        &mut self,
        prepared: Result<Option<crate::env_sync::PreparedPush>, crate::EnvSyncError>,
        target: Target,
        cx: &mut Context<Self>,
    ) {
        match prepared {
            Ok(Some(prepared)) => {
                self.prepared = Some((prepared, target));
                self.set_status(EnvStatus::Idle, cx);
            }
            Ok(None) => {
                self.prepared = None;
                self.set_status(EnvStatus::Done("already up to date".into()), cx);
            }
            Err(error) => {
                self.prepared = None;
                self.set_status(EnvStatus::Failed(format!("{error}").into()), cx);
            }
        }
    }

    /// Sends exactly what [`Self::prepare`] built and the panel displayed.
    ///
    /// `shown_entry` and `shown` are what the panel displayed. A push built
    /// since then -- another file, or this one edited and prepared again --
    /// is not what the user agreed to send, so nothing is sent.
    pub fn send_prepared(
        &mut self,
        shown_entry: EntryId,
        shown: &crate::WireBytes,
        cx: &mut Context<Self>,
    ) {
        if self.refuse_while_applying(cx) {
            return;
        }
        let Some((held, _)) = self.prepared.as_ref() else {
            return;
        };
        if held.entry != shown_entry || held.wire != *shown {
            self.set_status(
                EnvStatus::Failed(
                    "what the window showed is no longer what would be sent; nothing was sent"
                        .into(),
                ),
                cx,
            );
            return;
        }
        let Some((prepared, target)) = self.prepared.take() else {
            return;
        };
        let Some(env_dek) = self.env_dek.clone() else {
            self.set_status(EnvStatus::NeedsRecoveryKey, cx);
            return;
        };
        let account = self.account.clone();
        let entry = prepared.entry;
        let state_path = self.files.state.clone();
        self.set_status(EnvStatus::Working, cx);

        self.task = Some(cx.spawn(async move |this, cx| {
            let Some(context) = build_context(&account, cx).await else {
                _ = this.update(cx, |this, cx| this.set_status(unavailable(), cx));
                return;
            };
            let outcome =
                crate::env_sync::send_prepared(&context, &env_dek, prepared, &state_path).await;

            _ = this.update(cx, |this, cx| match outcome {
                Ok(PushOutcome::Stored { .. }) => {
                    this.set_status(EnvStatus::Done("sent".into()), cx);
                }
                Ok(PushOutcome::UpToDate) => {
                    this.set_status(EnvStatus::Done("already up to date".into()), cx);
                }
                Ok(PushOutcome::NothingToPush) => {
                    this.set_status(EnvStatus::Done("there was nothing to send".into()), cx);
                }
                Ok(PushOutcome::Conflict(divergence)) => {
                    this.hold(entry, target, divergence, false, cx);
                }
                Err(error) => this.fail(error, cx),
            });
        }));
    }

    /// Throws away a built push without sending it.
    pub fn discard_prepared(&mut self, cx: &mut Context<Self>) {
        self.prepared = None;
        if self.applying.is_none() {
            self.set_status(EnvStatus::Idle, cx);
        }
    }

    /// Sends one file.
    pub fn push(&mut self, entry: EntryId, local_path: PathBuf, cx: &mut Context<Self>) {
        if self.refuse_while_applying(cx) {
            return;
        }
        let Some(env_dek) = self.env_dek.clone() else {
            self.set_status(EnvStatus::NeedsRecoveryKey, cx);
            return;
        };
        let account = self.account.clone();
        let state_path = self.files.state.clone();
        self.set_status(EnvStatus::Working, cx);

        self.task = Some(cx.spawn(async move |this, cx| {
            let Some(context) = build_context(&account, cx).await else {
                _ = this.update(cx, |this, cx| this.set_status(unavailable(), cx));
                return;
            };
            let outcome =
                env_sync::push(&context, &env_dek, &entry, &local_path, &state_path).await;

            _ = this.update(cx, |this, cx| match outcome {
                Ok(PushOutcome::Stored { .. }) => {
                    this.set_status(EnvStatus::Done("sent".into()), cx);
                }
                Ok(PushOutcome::UpToDate) => {
                    this.set_status(EnvStatus::Done("already up to date".into()), cx);
                }
                Ok(PushOutcome::NothingToPush) => {
                    this.set_status(EnvStatus::Done("there is no file to send".into()), cx);
                }
                Ok(PushOutcome::Conflict(divergence)) => {
                    this.hold(entry, Target::local(local_path), divergence, false, cx);
                }
                Err(error) => this.fail(error, cx),
            });
        }));
    }

    /// Reads one file from the server and works out what it would change.
    pub fn pull(&mut self, entry: EntryId, local_path: PathBuf, cx: &mut Context<Self>) {
        self.pull_file(
            entry,
            Arc::new(LocalEnvFile::new(local_path.clone())),
            local_path,
            cx,
        );
    }

    /// [`Self::pull`], for a file that may live on another host.
    pub fn pull_file(
        &mut self,
        entry: EntryId,
        file: Arc<dyn EnvFile>,
        shown_as: PathBuf,
        cx: &mut Context<Self>,
    ) {
        if self.refuse_while_applying(cx) {
            return;
        }
        self.drop_stale_difference();
        let Some(env_dek) = self.env_dek.clone() else {
            self.set_status(EnvStatus::NeedsRecoveryKey, cx);
            return;
        };
        let account = self.account.clone();
        // Named before the path moves, so the answer can say which file it is
        // about. "already up to date" on its own reads as a refusal to
        // download rather than as the answer it is.
        let name = shown_as
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "that file".into());
        let state_path = self.files.state.clone();
        self.set_status(EnvStatus::Working, cx);

        self.task = Some(cx.spawn(async move |this, cx| {
            let Some(context) = build_context(&account, cx).await else {
                _ = this.update(cx, |this, cx| this.set_status(unavailable(), cx));
                return;
            };
            let outcome =
                env_sync::pull_file(&context, &env_dek, &entry, file.as_ref(), &state_path).await;
            let target = Target { file, shown_as };

            _ = this.update(cx, |this, cx| match outcome {
                Ok(PullOutcome::UpToDate) => {
                    this.set_status(
                        EnvStatus::Done(
                            format!("{name} here already matches the stored copy").into(),
                        ),
                        cx,
                    );
                }
                Ok(PullOutcome::LocalOnly) => {
                    this.set_status(
                        EnvStatus::Done(format!("your account holds nothing for {name}").into()),
                        cx,
                    );
                }
                Ok(PullOutcome::RemoteNewer(divergence)) => {
                    this.hold(entry, target, divergence, true, cx);
                }
                Ok(PullOutcome::Conflict(divergence)) => {
                    this.hold(entry, target, divergence, false, cx);
                }
                Ok(PullOutcome::KeyMismatch(_)) => {
                    this.set_status(EnvStatus::KeyMismatch, cx);
                }
                Ok(PullOutcome::Rollback { seen, got }) => {
                    this.set_status(EnvStatus::Rollback { seen, got }, cx);
                }
                Err(error) => this.fail(error, cx),
            });
        }));
    }

    /// Writes the held remote content over the local file.
    ///
    /// Answers whether the file was actually replaced, and on failure KEEPS
    /// the difference. Taking it either way is what made a failed write look
    /// like a successful one: the window closed because nothing was pending
    /// any more, and the reason went to a status line nobody was looking at
    /// while the file sat unchanged.
    ///
    /// A file on another host cannot be written before this returns, so for
    /// one the answer is always `false`: the write runs in the background, and
    /// on success the difference is cleared and the status says so. Until it
    /// answers, [`Self::is_applying`] holds and every other operation waits.
    ///
    /// Other work in flight does not stop a local write: it touches neither
    /// the held difference nor the file.
    pub fn apply_pending(&mut self, cx: &mut Context<Self>) -> bool {
        if self.refuse_while_applying(cx) {
            return false;
        }
        let Some(pending) = self.pending.as_ref() else {
            return false;
        };
        let Some(file) = self.pending_file.clone() else {
            self.set_status(
                EnvStatus::Failed("nothing says where this difference would be written".into()),
                cx,
            );
            return false;
        };
        let identity = DifferenceIdentity::of(pending);

        if let Some(local_path) = file.local_path() {
            let applied = env_sync::apply_remote_if_unchanged(
                &pending.entry,
                local_path,
                pending.local_hash.as_deref(),
                &self.files.backups,
                &pending.remote,
                pending.revision.clone(),
                pending.seq,
                &self.files.state,
            );
            return self.finish_apply(applied, identity, cx);
        }

        let entry = pending.entry;
        let seen_local_hash = pending.local_hash.clone();
        let remote = pending.remote.clone();
        let revision = pending.revision.clone();
        let seq = pending.seq;
        let files = self.files.clone();
        self.set_status(EnvStatus::Working, cx);
        self.applying = Some(cx.spawn(async move |this, cx| {
            let applied = env_sync::apply_remote_to(
                &entry,
                file.as_ref(),
                seen_local_hash.as_deref(),
                &files.backups,
                &remote,
                revision,
                seq,
                &files.state,
            )
            .await;
            this.update(cx, |this, cx| this.finish_apply(applied, identity, cx))
                .log_err();
        }));
        false
    }

    fn finish_apply(
        &mut self,
        applied: Result<(), crate::EnvSyncError>,
        identity: DifferenceIdentity,
        cx: &mut Context<Self>,
    ) -> bool {
        self.applying = None;
        match applied {
            Ok(()) => {
                if self
                    .pending
                    .as_ref()
                    .is_some_and(|pending| DifferenceIdentity::of(pending) == identity)
                {
                    self.pending = None;
                    self.pending_file = None;
                }
                self.set_status(EnvStatus::Done("written".into()), cx);
                true
            }
            Err(error) => {
                self.set_status(EnvStatus::Failed(format!("{error}").into()), cx);
                false
            }
        }
    }

    /// Clears a finished message so a status bar stops carrying it.
    ///
    /// Refuses while work is in flight: the message would come straight back
    /// on the next notification, and clearing it would only look broken.
    pub fn acknowledge(&mut self, cx: &mut Context<Self>) {
        if self.applying.is_none() && !matches!(self.status, EnvStatus::Working) {
            self.set_status(EnvStatus::Idle, cx);
        }
    }

    /// Discards the held difference without writing anything, answering
    /// whether it did. Refused while that difference is being written.
    pub fn dismiss_pending(&mut self, cx: &mut Context<Self>) -> bool {
        if self.refuse_while_applying(cx) {
            return false;
        }
        self.pending = None;
        self.pending_file = None;
        self.set_status(EnvStatus::Idle, cx);
        true
    }

    /// Forgets a difference nobody decided -- its window was closed by
    /// clicking outside it -- before work that opens a window of its own,
    /// titled for another file, which would otherwise show this one.
    fn drop_stale_difference(&mut self) {
        self.pending = None;
        self.pending_file = None;
    }

    fn hold(
        &mut self,
        entry: EntryId,
        target: Target,
        divergence: crate::env_sync::Divergence,
        safe_to_apply: bool,
        cx: &mut Context<Self>,
    ) {
        self.pending_file = Some(target.file);
        self.pending = Some(PendingEnvDivergence {
            entry,
            local_path: target.shown_as,
            diff: divergence.diff,
            remote: divergence.remote,
            revision: divergence.revision,
            seq: divergence.seq,
            safe_to_apply,
            local_hash: divergence.local_hash,
        });
        // A pull or push already on its way when a remote write started can
        // land here while that write runs; the status is the write's until
        // it answers.
        if self.applying.is_none() {
            self.set_status(EnvStatus::Idle, cx);
        } else {
            cx.notify();
        }
    }

    /// Fetches the catalogue and reconciles it against what the server holds.
    ///
    /// Two requests, both deliberate: the manifest says what the user calls
    /// their files, the listing says which of them still exist. Neither alone
    /// answers "was this deleted on my other machine".
    pub fn load_vault(&mut self, cx: &mut Context<Self>) {
        if self.refuse_while_applying(cx) {
            return;
        }
        let Some(env_dek) = self.env_dek.clone() else {
            self.set_status(EnvStatus::NeedsRecoveryKey, cx);
            return;
        };
        let account = self.account.clone();
        self.set_status(EnvStatus::Working, cx);

        self.task = Some(cx.spawn(async move |this, cx| {
            let Some(context) = build_context(&account, cx).await else {
                _ = this.update(cx, |this, cx| this.set_status(unavailable(), cx));
                return;
            };

            let loaded = load_manifest(&context, &env_dek).await;
            let listed = list_entries(&context).await;

            _ = this.update(cx, |this, cx| match (loaded, listed) {
                (Ok(manifest), Ok(remote)) => {
                    let reconciliation = crate::vault::reconcile(&manifest, &remote);
                    this.manifest = manifest;
                    this.reconciliation = reconciliation;
                    this.set_status(EnvStatus::Idle, cx);
                }
                (
                    Err(EnvSyncError::Crypto(
                        EnvCryptoError::KeyRotated { .. } | EnvCryptoError::WrongKey,
                    )),
                    _,
                ) => {
                    this.set_status(EnvStatus::KeyMismatch, cx);
                }
                (Err(error), _) | (_, Err(error)) => this.fail(error, cx),
            });
        }));
    }

    /// The catalogue entry recorded at `relative` (slash-separated) inside a
    /// checkout on this machine or on a remote host.
    ///
    /// Answers `None` for three different situations the caller must tell
    /// apart — checkout not bound, project gone, file not in the catalogue —
    /// so the caller asks [`Self::project_for_checkout`] first when it needs
    /// to say which.
    pub fn entry_at(&self, checkout: &Checkout, relative: &str) -> Option<EntryId> {
        let project = self.bindings.project_for_checkout(checkout)?;
        self.manifest
            .projects
            .get(&project)?
            .entries
            .iter()
            .find(|(_, entry)| entry.path == relative)
            .map(|(id, _)| *id)
    }

    pub fn project_for(&self, worktree_root: &Path) -> Option<crate::ProjectId> {
        self.bindings.project_for(worktree_root)
    }

    pub fn project_for_checkout(&self, checkout: &Checkout) -> Option<crate::ProjectId> {
        self.bindings.project_for_checkout(checkout)
    }

    /// The path recorded alongside a stored entry, relative to its checkout.
    ///
    /// The entry alone answers: ids are unique across the catalogue, and a
    /// caller made to carry the project as well is a caller that can pass a
    /// pair which disagrees.
    pub fn relative_path_for(&self, entry: EntryId) -> Option<String> {
        let (_, holder) = self.manifest.project_of(&entry)?;
        Some(holder.entries.get(&entry)?.path.clone())
    }

    /// Where a stored entry belongs on this machine, when a checkout says so.
    ///
    /// `None` is a real answer rather than a gap: without a binding there is
    /// nothing that knows where the file goes, and inventing a directory would
    /// write somebody's production environment somewhere they never named. The
    /// caller asks instead.
    pub fn local_path_for(&self, entry: EntryId) -> Option<PathBuf> {
        let (project, holder) = self.manifest.project_of(&entry)?;
        let relative = holder.entries.get(&entry)?.path.clone();
        let root = self.bindings.worktrees_for(*project).into_iter().next()?;
        Some(root.join(relative))
    }

    /// Adds a file on disk to a project and answers with its new entry.
    ///
    /// The path recorded in the catalogue is relative to the checkout bound to
    /// that project, so pulling on another machine lands it in the right place
    /// there too. A file chosen from outside any bound checkout keeps only its
    /// name — an absolute path would be both a disclosure and wrong on every
    /// other machine.
    ///
    /// Recording and sending are deliberately separate. This used to call
    /// [`Self::push`] itself, which made the one path that creates an entry
    /// also the one path that reached the network without showing the bytes
    /// first — quietly contradicting what `docs/src/env-sync-security.md`
    /// promises. The caller now prepares the push and displays it, like every
    /// other send.
    pub fn add_file(
        &mut self,
        project: crate::ProjectId,
        absolute_path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Option<EntryId> {
        let relative = self
            .bindings
            .worktrees_for(project)
            .into_iter()
            .find_map(|root| {
                absolute_path
                    .strip_prefix(&root)
                    .ok()
                    .map(|rest| rest.to_string_lossy().replace('\\', "/"))
            })
            .unwrap_or_else(|| {
                absolute_path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| ".env".into())
            });

        self.insert_entry(project, relative, cx)
    }

    /// [`Self::add_file`], for a file in a known checkout, which may be on a
    /// remote host. A remote file's path is recorded relative to its own
    /// checkout: the bound roots `add_file` searches are this machine's.
    pub fn add_file_in(
        &mut self,
        project: crate::ProjectId,
        checkout: &Checkout,
        absolute_path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Option<EntryId> {
        if checkout.is_local() {
            return self.add_file(project, absolute_path, cx);
        }
        let relative = checkout.relative(&absolute_path).unwrap_or_else(|| {
            absolute_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| ".env".into())
        });
        self.insert_entry(project, relative, cx)
    }

    fn insert_entry(
        &mut self,
        project: crate::ProjectId,
        relative: String,
        cx: &mut Context<Self>,
    ) -> Option<EntryId> {
        // Two files can honestly want the same name, and the vault would then
        // show two rows nobody can tell apart -- worse, both would pull onto
        // one file on disk.
        let relative = self.manifest.unique_path(project, &relative, None);

        let entry = match EntryId::generate() {
            Ok(entry) => entry,
            Err(error) => {
                self.set_status(EnvStatus::Failed(format!("{error}").into()), cx);
                return None;
            }
        };

        let Some(holder) = self.manifest.projects.get_mut(&project) else {
            self.set_status(
                EnvStatus::Failed("that project is no longer in your vault".into()),
                cx,
            );
            return None;
        };
        holder.entries.insert(
            entry,
            crate::manifest::ManifestEntry {
                path: relative,
                seq: 0,
            },
        );

        self.save_manifest(cx);
        Some(entry)
    }

    /// Renames a stored file, answering whether the name was taken.
    ///
    /// The name is not a label: it is the path a pull writes to, on every
    /// machine. So it is held to the same rule as the writer itself
    /// (`names_a_file_inside`) rather than a looser one here — a name accepted
    /// when typed and refused when used would be the worst of both — and it is
    /// made unique within its project for the same reason adding is.
    pub fn rename_entry(&mut self, entry: EntryId, wanted: &str, cx: &mut Context<Self>) -> bool {
        let wanted = wanted.trim();
        if !crate::secret_file::names_a_file_inside(wanted) {
            self.set_status(
                EnvStatus::Failed("that name does not describe a file inside a project".into()),
                cx,
            );
            return false;
        }

        let Some(project) = self.manifest.project_of(&entry).map(|(id, _)| *id) else {
            self.set_status(
                EnvStatus::Failed("that file is no longer in your vault".into()),
                cx,
            );
            return false;
        };

        let unique = self.manifest.unique_path(project, wanted, Some(entry));
        let Some(held) = self
            .manifest
            .projects
            .get_mut(&project)
            .and_then(|holder| holder.entries.get_mut(&entry))
        else {
            self.set_status(
                EnvStatus::Failed("that file is no longer in your vault".into()),
                cx,
            );
            return false;
        };

        if held.path == unique {
            return true;
        }
        held.path = unique;
        self.save_manifest(cx);
        true
    }

    /// Creates an empty project in the catalogue.
    pub fn add_project(
        &mut self,
        name: String,
        cx: &mut Context<Self>,
    ) -> Option<crate::ProjectId> {
        let id = crate::ProjectId::generate().ok()?;
        self.manifest.projects.insert(
            id,
            crate::manifest::ManifestProject {
                name,
                entries: Default::default(),
            },
        );
        self.save_manifest(cx);
        Some(id)
    }

    /// Removes an entry from the catalogue and from the server.
    ///
    /// The local file is left alone. Deleting a user's `.env` because they
    /// tidied their vault is not a trade anyone would accept, and there is no
    /// way to undo it.
    pub fn forget_entry(&mut self, entry: EntryId, cx: &mut Context<Self>) {
        for project in self.manifest.projects.values_mut() {
            project.entries.remove(&entry);
        }
        self.save_manifest(cx);

        let Some(account) = Some(self.account.clone()) else {
            return;
        };
        cx.spawn(async move |_this, cx| {
            let Some(context) = build_context(&account, cx).await else {
                return;
            };
            let Ok(resource) = seal::entry_resource(&entry) else {
                return;
            };
            if let Err(error) = client::forget(
                &context.http_client,
                &context.api_url,
                &context.credential.access_token,
                &resource,
            )
            .await
            {
                log::warn!("the stored environment file could not be removed: {error}");
            }
        })
        .detach();
    }

    /// Writes the catalogue back to the server.
    fn save_manifest(&mut self, cx: &mut Context<Self>) {
        let Some(env_dek) = self.env_dek.clone() else {
            self.set_status(EnvStatus::NeedsRecoveryKey, cx);
            return;
        };
        let manifest = self.manifest.clone();
        let account = self.account.clone();

        cx.spawn(async move |this, cx| {
            let Some(context) = build_context(&account, cx).await else {
                _ = this.update(cx, |this, cx| this.set_status(unavailable(), cx));
                return;
            };
            if let Err(error) = write_manifest(&context, &env_dek, &manifest).await {
                _ = this.update(cx, |this, cx| this.fail(error, cx));
            }
        })
        .detach();
    }

    /// What the catalogue and the server disagree about, as of the last load.
    pub fn reconciliation(&self) -> &crate::Reconciliation {
        &self.reconciliation
    }

    /// Records why something did not happen. A plan refusal gets its own
    /// status and a fresh read of the plan; nothing retries it.
    fn fail(&mut self, error: EnvSyncError, cx: &mut Context<Self>) {
        match error {
            EnvSyncError::Client(ClientError::PlanLimit(limit)) => {
                self.set_status(EnvStatus::PlanLimited(limit), cx);
                self.account
                    .update(cx, |account, cx| account.refresh_plan(cx));
            }
            error => self.set_status(EnvStatus::Failed(format!("{error}").into()), cx),
        }
    }

    fn set_status(&mut self, status: EnvStatus, cx: &mut Context<Self>) {
        self.status = status;
        cx.emit(EnvStatusChanged);
        cx.notify();
    }
}

#[cfg(any(test, feature = "test-support"))]
impl EnvSession {
    pub fn set_pending_for_test(&mut self, pending: PendingEnvDivergence, cx: &mut Context<Self>) {
        self.pending_file = Some(Arc::new(LocalEnvFile::new(pending.local_path.clone())));
        self.pending = Some(pending);
        cx.notify();
    }

    /// Holds a difference that would be written through `file`.
    pub fn set_pending_in_for_test(
        &mut self,
        pending: PendingEnvDivergence,
        file: Arc<dyn EnvFile>,
        cx: &mut Context<Self>,
    ) {
        self.pending_file = Some(file);
        self.pending = Some(pending);
        cx.notify();
    }

    pub fn set_manifest_for_test(&mut self, manifest: Manifest, cx: &mut Context<Self>) {
        self.manifest = manifest;
        cx.notify();
    }

    pub fn set_status_for_test(&mut self, status: EnvStatus, cx: &mut Context<Self>) {
        self.set_status(status, cx);
    }

    /// Binds in memory only. [`Self::bind`] also writes the bindings file,
    /// which in a test means writing into the real configuration directory.
    pub fn bind_for_test(&mut self, worktree_root: &std::path::Path, project: crate::ProjectId) {
        self.bindings.bind(worktree_root, project);
    }

    /// Keeps the bindings, the sync state and the backups in `directory`
    /// rather than in this machine's real configuration.
    pub fn keep_files_in_for_test(&mut self, directory: &Path) {
        self.bindings = Bindings::load(&directory.join("env_bindings.json"));
        self.files = SessionFiles {
            bindings: directory.join("env_bindings.json"),
            state: directory.join("env_state.json"),
            backups: directory.join("env-backups"),
        };
    }

    /// Unlocks with a key made up for the test.
    pub fn unlock_for_test(&mut self, env_dek: EnvDek) {
        self.env_dek = Some(Arc::new(env_dek));
    }
}

/// Reads and opens the catalogue. An account with none yet gets an empty one.
async fn load_manifest(context: &SyncContext, env_dek: &EnvDek) -> Result<Manifest, EnvSyncError> {
    let resource = seal::manifest_resource()?;
    let document = client::fetch(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        &resource,
    )
    .await?;

    let Some(document) = document else {
        return Ok(Manifest::new());
    };
    let envelope = from_blob(&document.blob).map_err(EnvCryptoError::from)?;
    // No rollback check on the catalogue read itself: a stale catalogue is
    // surfaced by the reconciliation below rather than refused, and refusing
    // it would leave the user with no list at all.
    let opened = seal::open(
        env_dek,
        &context.credential.user_id,
        &resource,
        None,
        &envelope,
    )?;
    Ok(Manifest::from_json(&opened.payload)?)
}

/// Seals and stores the catalogue.
///
/// Unconditional on the client side and conditional on the server's: the
/// precondition is read back from what is there, because a catalogue write
/// that loses a race must be retried rather than silently dropped — the losing
/// machine has just added a file nobody else knows about.
async fn write_manifest(
    context: &SyncContext,
    env_dek: &EnvDek,
    manifest: &Manifest,
) -> Result<(), EnvSyncError> {
    let resource = seal::manifest_resource()?;
    let current = client::fetch(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        &resource,
    )
    .await?;

    let seq = match &current {
        Some(document) => from_blob(&document.blob)
            .ok()
            .and_then(|envelope| {
                seal::open(
                    env_dek,
                    &context.credential.user_id,
                    &resource,
                    None,
                    &envelope,
                )
                .ok()
            })
            .map_or(1, |opened| opened.seq + 1),
        None => 1,
    };

    let envelope = seal::seal(
        env_dek,
        &context.credential.user_id,
        &resource,
        seq,
        &manifest.to_json()?,
    )?;
    let blob = to_blob(&envelope).map_err(EnvCryptoError::from)?;

    let precondition = match &current {
        Some(document) => client::Precondition::Replace(&document.revision),
        None => client::Precondition::Create,
    };

    client::store(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        &resource,
        &blob,
        precondition,
    )
    .await?;
    Ok(())
}

/// Asks which entries the server actually holds.
async fn list_entries(context: &SyncContext) -> Result<Vec<crate::RemoteEntry>, EnvSyncError> {
    let body = client::fetch_body(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        &Resource::env_listing(),
    )
    .await?;

    match body {
        Some(body) => Ok(crate::vault::parse_listing(&body)?),
        // A 404 on the collection means the route is not there at all, which
        // is not the same as an empty vault and must not be reported as one.
        None => Err(
            EnvCryptoError::Malformed("the sync service has no environment store".into()).into(),
        ),
    }
}

fn unavailable() -> EnvStatus {
    EnvStatus::Failed("sign in again — this session could not be used".into())
}

/// Reads and unwraps the stored environment key. `Ok(None)` means none exists.
async fn fetch_env_key(context: &SyncContext, dek: &Dek) -> Result<Option<EnvDek>, EnvSyncError> {
    let resource =
        Resource::env_singleton(crate::ENV_KEY_RESOURCE).map_err(EnvCryptoError::from)?;
    let document = client::fetch(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        &resource,
    )
    .await?;

    let Some(document) = document else {
        return Ok(None);
    };
    let envelope = from_blob(&document.blob).map_err(EnvCryptoError::from)?;
    Ok(env_dek::unwrap_key(dek, &context.credential.user_id, &envelope).map(Some)?)
}

async fn build_context(account: &Entity<Account>, cx: &mut gpui::AsyncApp) -> Option<SyncContext> {
    // `api_credential` refreshes a near-expired token and answers `None` for
    // every reason the caller must not proceed on: signed out, credential
    // rejected, service unreachable.
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

/// Installs the global [`EnvSession`].
///
/// Reads nothing and sends nothing. The environment key is loaded the first
/// time the user opens the vault, not here — starting the editor must not
/// touch the keychain for a feature nobody has asked for yet, and it must
/// never touch the network.
pub fn init(account: Entity<Account>, cx: &mut App) {
    use gpui::AppContext as _;
    let session = cx.new(|_| EnvSession::new(account));
    EnvSession::set_global(session, cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, TestAppContext};
    use zode_account::{AccountStatus, AccountUser};

    fn session(cx: &mut TestAppContext) -> Entity<EnvSession> {
        let account = cx.update(|cx| {
            cx.new(|_| {
                Account::for_test(AccountStatus::SignedIn(AccountUser {
                    id: "1".into(),
                    email: "ada@example.com".into(),
                    name: None,
                    avatar_url: None,
                }))
            })
        });
        cx.update(|cx| cx.new(|_| EnvSession::new(account)))
    }

    #[gpui::test]
    fn every_status_has_a_sentence(_cx: &mut TestAppContext) {
        // The status bar renders this directly, so a status with nothing to
        // say would show an icon beside an empty line.
        for status in [
            EnvStatus::Idle,
            EnvStatus::Working,
            EnvStatus::Done("sent".into()),
            EnvStatus::NeedsRecoveryKey,
            EnvStatus::KeyMismatch,
            EnvStatus::Rollback { seen: 9, got: 4 },
            EnvStatus::PlanLimited(env_refusal()),
            EnvStatus::Failed("the service is unreachable".into()),
        ] {
            assert!(
                !status.sentence().is_empty(),
                "{status:?} has nothing to say",
            );
        }
    }

    fn env_refusal() -> PlanLimit {
        PlanLimit::from_body(
            r#"{"error":"env_sync_not_in_plan","tier":"free"}"#,
            &zode_account::web_url(),
        )
    }

    struct NoKeychain;

    impl credentials_provider::CredentialsProvider for NoKeychain {
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

    /// A server that refuses everything with a plan 402 and counts plan reads.
    fn refusing_server() -> (
        Arc<dyn http_client::HttpClient>,
        Arc<std::sync::atomic::AtomicUsize>,
    ) {
        let plan_reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = plan_reads.clone();
        let client: Arc<dyn http_client::HttpClient> =
            http_client::FakeHttpClient::create(move |request| {
                let counter = counter.clone();
                async move {
                    if request.uri().path().ends_with("/account/plan") {
                        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        return Ok(http_client::Response::builder()
                            .status(500)
                            .body(Default::default())
                            .expect("a well-formed response"));
                    }
                    Ok(http_client::Response::builder()
                        .status(402)
                        .body(r#"{"error":"sync_device_limit","limit":2,"used":3}"#.into())
                        .expect("a well-formed response"))
                }
            });
        (client, plan_reads)
    }

    fn refused_context(client: Arc<dyn http_client::HttpClient>) -> SyncContext {
        SyncContext {
            http_client: client,
            api_url: "http://test.invalid/api".into(),
            credential: zode_account::ApiCredential {
                access_token: "token".into(),
                user_id: "1".into(),
            },
        }
    }

    fn is_device_refusal<T>(result: &Result<T, EnvSyncError>) -> bool {
        matches!(
            result,
            Err(EnvSyncError::Client(ClientError::PlanLimit(limit)))
                if limit.reason == zode_account::PlanLimitReason::SyncDeviceLimit
        )
    }

    /// The vault's own reads used to fold every client error into "this is not
    /// a Zode environment blob", which turned a plan refusal into a claim of
    /// corrupt data. Each of them must hand the refusal on intact.
    #[gpui::test]
    fn the_vault_reads_hand_a_plan_refusal_on_intact(_cx: &mut TestAppContext) {
        let (client, _) = refusing_server();
        let context = refused_context(client);
        let env_dek = EnvDek::generate().expect("a key");
        let dek = Dek::generate().expect("a key");

        let listed = futures::executor::block_on(list_entries(&context));
        assert!(is_device_refusal(&listed), "list: {listed:?}");
        let loaded = futures::executor::block_on(load_manifest(&context, &env_dek));
        assert!(is_device_refusal(&loaded), "manifest read");
        let written =
            futures::executor::block_on(write_manifest(&context, &env_dek, &Manifest::new()));
        assert!(is_device_refusal(&written), "manifest write: {written:?}");
        let key = futures::executor::block_on(fetch_env_key(&context, &dek));
        assert!(is_device_refusal(&key), "env key");
    }

    #[gpui::test]
    fn a_plan_refusal_becomes_its_own_status_and_reads_the_plan(cx: &mut TestAppContext) {
        let (client, plan_reads) = refusing_server();
        let account = cx.update(|cx| {
            cx.new(|_| {
                let mut account = Account::for_test_with(
                    AccountStatus::SignedIn(AccountUser {
                        id: "1".into(),
                        email: "ada@example.com".into(),
                        name: None,
                        avatar_url: None,
                    }),
                    client,
                    Arc::new(NoKeychain),
                );
                account.set_tokens_for_test();
                account
            })
        });
        let session = cx.update(|cx| cx.new(|_| EnvSession::new(account)));

        session.update(cx, |session, cx| {
            session.fail(
                EnvSyncError::Client(ClientError::PlanLimit(env_refusal())),
                cx,
            )
        });
        cx.run_until_parked();

        session.read_with(cx, |session, _| {
            assert_eq!(session.status(), &EnvStatus::PlanLimited(env_refusal()));
            assert_eq!(
                session.status().sentence(),
                ".env sync is part of Pro. Nothing was uploaded; your synced files are still readable."
            );
        });
        assert_eq!(plan_reads.load(std::sync::atomic::Ordering::SeqCst), 1);

        // Anything else stays a failure, and is not a reason to read the plan.
        session.update(cx, |session, cx| {
            session.fail(EnvSyncError::Client(ClientError::Unauthorized), cx)
        });
        cx.run_until_parked();
        session.read_with(cx, |session, _| {
            assert!(matches!(session.status(), EnvStatus::Failed(_)));
        });
        assert_eq!(plan_reads.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    fn one_entry() -> (crate::ProjectId, EntryId, Manifest) {
        let project = crate::ProjectId::parse(&"11".repeat(16)).expect("a valid id");
        let entry = EntryId::parse(&"a1".repeat(16)).expect("a valid id");
        let mut manifest = Manifest::new();
        manifest.projects.insert(
            project,
            crate::manifest::ManifestProject {
                name: "acme-api".into(),
                entries: std::collections::BTreeMap::from([(
                    entry,
                    crate::manifest::ManifestEntry {
                        path: "services/api/.env".into(),
                        seq: 3,
                    },
                )]),
            },
        );
        (project, entry, manifest)
    }

    #[gpui::test]
    fn a_bound_checkout_says_where_a_stored_file_goes(cx: &mut TestAppContext) {
        let session = session(cx);
        let (project, entry, manifest) = one_entry();

        session.update(cx, |session, cx| {
            session.set_manifest_for_test(manifest, cx);
            session.bind_for_test(&PathBuf::from("/work/acme"), project);

            assert_eq!(
                session.local_path_for(entry),
                Some(PathBuf::from("/work/acme/services/api/.env")),
                "the destination is the checkout joined with the recorded path",
            );
        });
    }

    #[gpui::test]
    fn without_a_checkout_there_is_no_destination_to_invent(cx: &mut TestAppContext) {
        // The caller has to ask instead. Guessing a directory would write
        // somebody's production environment somewhere they never named.
        let session = session(cx);
        let (_project, entry, manifest) = one_entry();

        session.update(cx, |session, cx| {
            session.set_manifest_for_test(manifest, cx);

            assert_eq!(session.local_path_for(entry), None);
            assert_eq!(
                session.relative_path_for(entry).as_deref(),
                Some("services/api/.env"),
                "the recorded path still answers, so the caller can offer it a folder",
            );
        });
    }

    fn empty_project() -> (crate::ProjectId, Manifest) {
        let project = crate::ProjectId::parse(&"11".repeat(16)).expect("a valid id");
        let mut manifest = Manifest::new();
        manifest.projects.insert(
            project,
            crate::manifest::ManifestProject {
                name: "acme-api".into(),
                entries: Default::default(),
            },
        );
        (project, manifest)
    }

    #[gpui::test]
    fn two_files_with_the_same_name_stay_two_rows(cx: &mut TestAppContext) {
        // Both come from outside any bound checkout, so both keep only their
        // bare name. Left alone they would be two rows reading `.env` that
        // nobody can tell apart, and both would pull onto one file on disk.
        let session = session(cx);
        let (project, manifest) = empty_project();

        session.update(cx, |session, cx| {
            session.set_manifest_for_test(manifest, cx);
            let first = session
                .add_file(project, PathBuf::from("/one/.env"), cx)
                .expect("the first file");
            let second = session
                .add_file(project, PathBuf::from("/two/.env"), cx)
                .expect("the second file");

            assert_eq!(session.relative_path_for(first).as_deref(), Some(".env"));
            assert_eq!(
                session.relative_path_for(second).as_deref(),
                Some(".env (2)"),
                "the second must not silently take the first one's place",
            );
        });
    }

    #[gpui::test]
    fn a_name_that_leaves_the_project_is_refused(cx: &mut TestAppContext) {
        // The name is the path a pull writes to, so a rename is a file write
        // waiting to happen. Held to the same rule as the writer itself.
        let session = session(cx);
        let (project, manifest) = empty_project();

        session.update(cx, |session, cx| {
            session.set_manifest_for_test(manifest, cx);
            let entry = session
                .add_file(project, PathBuf::from("/one/.env"), cx)
                .expect("the file");

            for wanted in ["", "   ", "../elsewhere/.env", "/etc/passwd"] {
                assert!(
                    !session.rename_entry(entry, wanted, cx),
                    "{wanted:?} must be refused",
                );
                assert_eq!(
                    session.relative_path_for(entry).as_deref(),
                    Some(".env"),
                    "a refused rename must change nothing",
                );
            }
        });
    }

    #[gpui::test]
    fn renaming_onto_a_name_a_sibling_holds_takes_the_next_index(cx: &mut TestAppContext) {
        let session = session(cx);
        let (project, manifest) = empty_project();

        session.update(cx, |session, cx| {
            session.set_manifest_for_test(manifest, cx);
            let first = session
                .add_file(project, PathBuf::from("/one/.env"), cx)
                .expect("the first file");
            let second = session
                .add_file(project, PathBuf::from("/two/.env.local"), cx)
                .expect("the second file");

            assert!(session.rename_entry(second, ".env", cx));
            assert_eq!(
                session.relative_path_for(second).as_deref(),
                Some(".env (2)"),
                "the rename must not take a name a sibling already holds",
            );
            assert_eq!(
                session.relative_path_for(first).as_deref(),
                Some(".env"),
                "and must not disturb the sibling",
            );
        });
    }

    #[gpui::test]
    fn a_write_that_cannot_happen_keeps_the_difference_and_records_why(cx: &mut TestAppContext) {
        // The bug this locks down: applying took the pending difference before
        // knowing whether the write succeeded, so a failure closed the only
        // window that could explain it and left the file silently unchanged.
        //
        // A regular file standing where a directory must be is a write that
        // cannot succeed. Nothing global is touched on this path: a local file
        // that does not exist needs no backup, and the state file is only
        // written after a successful write.
        let session = session(cx);
        let blocker =
            std::env::temp_dir().join(format!("zode-env-apply-{}-{}", std::process::id(), line!()));
        std::fs::write(&blocker, b"a file, not a directory").expect("the fixture");

        session.update(cx, |session, cx| {
            session.set_pending_for_test(
                PendingEnvDivergence {
                    entry: EntryId::parse(&"a1".repeat(16)).expect("a valid id"),
                    local_path: blocker.join("inside").join(".env"),
                    diff: zode_sync::diff::between("", "A=1\n"),
                    remote: "A=1\n".into(),
                    revision: "rev-1".into(),
                    seq: 1,
                    safe_to_apply: true,
                    local_hash: None,
                },
                cx,
            );

            assert!(
                !session.apply_pending(cx),
                "the write could not happen, so it must not report success",
            );
            assert!(
                session.pending().is_some(),
                "the difference must survive, or the window explaining it closes",
            );
            assert!(
                matches!(session.status(), EnvStatus::Failed(_)),
                "the reason must be recorded, got {:?}",
                session.status(),
            );
        });

        std::fs::remove_file(&blocker).ok();
    }

    /// A file on a host the connection to which drops on every write.
    struct UnreachableFile;

    impl EnvFile for UnreachableFile {
        fn read(&self) -> crate::EnvFileFuture<'_, Option<Vec<u8>>> {
            Box::pin(async { Ok(None) })
        }

        fn write_atomic(&self, _contents: Vec<u8>) -> crate::EnvFileFuture<'_, ()> {
            Box::pin(async {
                Err(crate::EnvSyncError::Io(
                    "Update the remote server to sync this file".into(),
                ))
            })
        }
    }

    #[gpui::test]
    fn a_remote_write_that_fails_keeps_the_difference(cx: &mut TestAppContext) {
        let session = session(cx);
        session.update(cx, |session, cx| {
            session.set_pending_in_for_test(
                PendingEnvDivergence {
                    entry: EntryId::parse(&"a1".repeat(16)).expect("a valid id"),
                    local_path: PathBuf::from("/home/me/acme/.env"),
                    diff: zode_sync::diff::between("", "A=1\n"),
                    remote: "A=1\n".into(),
                    revision: "rev-1".into(),
                    seq: 1,
                    safe_to_apply: true,
                    local_hash: None,
                },
                Arc::new(UnreachableFile),
                cx,
            );
            assert!(
                !session.apply_pending(cx),
                "a remote write cannot have happened before it returns"
            );
            assert_eq!(*session.status(), EnvStatus::Working);
            assert!(
                !session.apply_pending(cx),
                "a second press must not start a second write"
            );
        });
        cx.run_until_parked();

        session.read_with(cx, |session, _| {
            assert!(
                session.pending().is_some(),
                "the difference must survive a failed remote write"
            );
            match session.status() {
                EnvStatus::Failed(reason) => assert!(
                    reason.contains("Update the remote server"),
                    "the reason was lost: {reason}"
                ),
                other => panic!("expected a failure, got {other:?}"),
            }
        });
    }

    /// A file on a remote host whose write waits until the test lets it go.
    struct SlowFile {
        content: Option<Vec<u8>>,
        released: std::sync::Mutex<Option<futures::channel::oneshot::Receiver<()>>>,
        writes: std::sync::Mutex<Vec<Vec<u8>>>,
    }

    impl SlowFile {
        fn holding(content: &[u8]) -> (Arc<Self>, futures::channel::oneshot::Sender<()>) {
            let (release, released) = futures::channel::oneshot::channel();
            let file = Arc::new(Self {
                content: Some(content.to_vec()),
                released: std::sync::Mutex::new(Some(released)),
                writes: std::sync::Mutex::default(),
            });
            (file, release)
        }
    }

    impl EnvFile for SlowFile {
        fn read(&self) -> crate::EnvFileFuture<'_, Option<Vec<u8>>> {
            let content = self.content.clone();
            Box::pin(async move { Ok(content) })
        }

        fn write_atomic(&self, contents: Vec<u8>) -> crate::EnvFileFuture<'_, ()> {
            let released = self.released.lock().expect("not poisoned").take();
            Box::pin(async move {
                if let Some(released) = released {
                    released
                        .await
                        .map_err(|_| crate::EnvSyncError::Io("the test went away".into()))?;
                }
                self.writes.lock().expect("not poisoned").push(contents);
                Ok(())
            })
        }
    }

    struct Directory(PathBuf);

    impl Directory {
        fn new(name: &str) -> Self {
            let directory = std::env::temp_dir()
                .join(format!("zode-env-session-{name}-{}", std::process::id()));
            std::fs::remove_dir_all(&directory).ok();
            std::fs::create_dir_all(&directory).expect("the fixture directory");
            Self(directory)
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn pending_over(local_path: PathBuf, local: &str) -> PendingEnvDivergence {
        PendingEnvDivergence {
            entry: EntryId::parse(&"a1".repeat(16)).expect("a valid id"),
            local_path,
            diff: zode_sync::diff::between(local, "NEW=1\n"),
            remote: "NEW=1\n".into(),
            revision: "rev-2".into(),
            seq: 2,
            safe_to_apply: true,
            local_hash: Some(crate::env_state::hash(local.as_bytes())),
        }
    }

    /// Starts a remote apply that stays in flight until `release` is sent.
    fn start_slow_apply(
        session: &Entity<EnvSession>,
        directory: &Directory,
        cx: &mut TestAppContext,
    ) -> (Arc<SlowFile>, futures::channel::oneshot::Sender<()>) {
        let (file, release) = SlowFile::holding(b"OLD=1\n");
        session.update(cx, |session, cx| {
            session.keep_files_in_for_test(&directory.0);
            session.set_pending_in_for_test(
                pending_over(PathBuf::from("/home/me/acme/.env"), "OLD=1\n"),
                file.clone(),
                cx,
            );
            assert!(!session.apply_pending(cx));
        });
        cx.run_until_parked();
        (file, release)
    }

    fn assert_written_through(
        session: &Entity<EnvSession>,
        file: &SlowFile,
        directory: &Directory,
        cx: &mut TestAppContext,
    ) {
        assert_eq!(
            *file.writes.lock().expect("not poisoned"),
            vec![b"NEW=1\n".to_vec()]
        );
        session.read_with(cx, |session, _| {
            assert!(
                session.pending().is_none(),
                "the written difference lingers"
            );
            assert_eq!(*session.status(), EnvStatus::Done("written".into()));
        });
        let entry = EntryId::parse(&"a1".repeat(16)).expect("a valid id");
        assert!(
            crate::EnvState::load(&directory.0.join("env_state.json"))
                .get(&entry)
                .is_some(),
            "a write that landed must be recorded, or the next pull calls it a conflict"
        );
    }

    #[gpui::test]
    fn a_remote_write_in_flight_cannot_be_dismissed_and_still_lands(cx: &mut TestAppContext) {
        let session = session(cx);
        let directory = Directory::new("dismiss-in-flight");
        let (file, release) = start_slow_apply(&session, &directory, cx);

        session.update(cx, |session, cx| {
            session.dismiss_pending(cx);
            assert!(
                session.pending().is_some(),
                "the difference was dropped while its write was still on the way"
            );
            assert!(
                session.status().sentence().contains("still running"),
                "the refusal must say why, got {:?}",
                session.status()
            );
        });

        release.send(()).expect("the write is waiting");
        cx.run_until_parked();
        assert_written_through(&session, &file, &directory, cx);
    }

    #[gpui::test]
    fn a_pull_during_a_remote_write_is_refused_and_the_write_survives(cx: &mut TestAppContext) {
        let session = session(cx);
        let directory = Directory::new("pull-in-flight");
        let (file, release) = start_slow_apply(&session, &directory, cx);

        session.update(cx, |session, cx| {
            session.unlock_for_test(EnvDek::from_bytes([0x33; zode_sync::DEK_LEN]));
            session.pull(
                EntryId::parse(&"b2".repeat(16)).expect("a valid id"),
                directory.0.join("other.env"),
                cx,
            );
            assert!(
                session.status().sentence().contains("still running"),
                "the pull must be refused while the write runs, got {:?}",
                session.status()
            );
        });

        release.send(()).expect("the write is waiting");
        cx.run_until_parked();
        assert_written_through(&session, &file, &directory, cx);
    }

    #[gpui::test]
    fn a_pull_that_lands_during_a_remote_write_keeps_its_difference(cx: &mut TestAppContext) {
        // Started before the write and answering while it runs: the write's
        // answer must neither drop this difference nor call it written.
        let session = session(cx);
        let directory = Directory::new("pull-lands-during-write");
        let (file, release) = start_slow_apply(&session, &directory, cx);
        let other = EntryId::parse(&"b2".repeat(16)).expect("a valid id");
        let (other_file, _other_release) = SlowFile::holding(b"OTHER=1\n");

        session.update(cx, |session, cx| {
            session.hold(
                other,
                Target {
                    file: other_file,
                    shown_as: PathBuf::from("/home/me/acme/other.env"),
                },
                crate::env_sync::Divergence {
                    diff: zode_sync::diff::between("OTHER=1\n", "OTHER=2\n"),
                    remote: "OTHER=2\n".into(),
                    revision: "rev-9".into(),
                    seq: 9,
                    local_hash: Some(crate::env_state::hash(b"OTHER=1\n")),
                },
                false,
                cx,
            );
            assert!(session.is_applying());
            assert_eq!(
                *session.status(),
                EnvStatus::Working,
                "the write is still running"
            );
        });
        release.send(()).expect("the write is waiting");
        cx.run_until_parked();

        assert_eq!(
            *file.writes.lock().expect("not poisoned"),
            vec![b"NEW=1\n".to_vec()]
        );
        session.read_with(cx, |session, _| {
            let pending = session
                .pending()
                .expect("the write's answer dropped a difference it never wrote");
            assert_eq!(pending.entry, other);
            assert_eq!(pending.local_path, PathBuf::from("/home/me/acme/other.env"));
            assert_eq!(*session.status(), EnvStatus::Done("written".into()));
        });
    }

    #[gpui::test]
    fn a_new_pull_or_push_never_shows_the_difference_left_from_before(cx: &mut TestAppContext) {
        // A diff window closed by clicking outside it leaves its difference
        // held. The next window is titled for its own file, and must not show
        // the old file's difference while its own answer is on the way.
        let session = session(cx);
        let directory = Directory::new("stale-difference");
        let other = EntryId::parse(&"b2".repeat(16)).expect("a valid id");
        let (file, _release) = SlowFile::holding(b"OTHER=1\n");
        let shown_as = PathBuf::from("/home/me/acme/other.env");

        session.update(cx, |session, cx| {
            session.keep_files_in_for_test(&directory.0);
            session.unlock_for_test(EnvDek::from_bytes([0x33; zode_sync::DEK_LEN]));

            session.set_pending_for_test(pending_over(directory.0.join(".env"), "OLD=1\n"), cx);
            session.pull_file(other, file.clone(), shown_as.clone(), cx);
            assert!(
                session.pending().is_none(),
                "a pull of another file shows the previous file's difference"
            );

            session.set_pending_for_test(pending_over(directory.0.join(".env"), "OLD=1\n"), cx);
            session
                .prepare_file(other, file.clone(), shown_as.clone(), cx)
                .detach();
            assert!(
                session.pending().is_none(),
                "a push of another file shows the previous file's difference"
            );
        });
        cx.run_until_parked();

        session.read_with(cx, |session, _| {
            assert!(session.pending().is_none(), "the old difference came back");
        });
    }

    #[gpui::test]
    fn a_local_apply_while_other_work_runs_still_writes(cx: &mut TestAppContext) {
        let session = session(cx);
        let directory = Directory::new("local-while-working");
        let target = directory.0.join(".env");
        std::fs::write(&target, b"OLD=1\n").expect("the fixture");

        session.update(cx, |session, cx| {
            session.keep_files_in_for_test(&directory.0);
            session.set_status_for_test(EnvStatus::Working, cx);
            session.set_pending_for_test(pending_over(target.clone(), "OLD=1\n"), cx);
            assert!(
                session.apply_pending(cx),
                "a local write has nothing in flight to wait for, got {:?}",
                session.status()
            );
        });
        assert_eq!(std::fs::read(&target).unwrap(), b"NEW=1\n");
    }

    #[gpui::test]
    fn only_the_push_that_was_shown_is_sent(cx: &mut TestAppContext) {
        let session = session(cx);
        let directory = Directory::new("send-what-was-shown");
        let entry = EntryId::parse(&"a1".repeat(16)).expect("a valid id");
        let (file, _release) = SlowFile::holding(b"A=1\n");
        let prepare = |session: &Entity<EnvSession>, cx: &mut TestAppContext| {
            session
                .update(cx, |session, cx| {
                    session.prepare_file(
                        entry,
                        file.clone(),
                        PathBuf::from("/home/me/acme/.env"),
                        cx,
                    )
                })
                .detach();
            cx.run_until_parked();
            session
                .read_with(cx, |session, _| session.prepared().cloned())
                .expect("a push is built")
        };
        session.update(cx, |session, _cx| {
            session.keep_files_in_for_test(&directory.0);
            session.unlock_for_test(EnvDek::from_bytes([0x33; zode_sync::DEK_LEN]));
        });

        let shown = prepare(&session, cx);
        // Built again, so sealed under a fresh nonce: other bytes.
        let current = prepare(&session, cx);
        assert_ne!(shown, current);

        for (claimed_entry, claimed_wire) in [
            (entry, &shown),
            (
                EntryId::parse(&"b2".repeat(16)).expect("a valid id"),
                &current,
            ),
        ] {
            session.update(cx, |session, cx| {
                session.send_prepared(claimed_entry, claimed_wire, cx);
                assert!(
                    session.prepared().is_some(),
                    "a push nobody was shown was sent"
                );
                assert!(
                    session.status().sentence().contains("nothing was sent"),
                    "the refusal must say so, got {:?}",
                    session.status()
                );
            });
        }

        session.update(cx, |session, cx| {
            session.send_prepared(entry, &current, cx);
            assert!(session.prepared().is_none(), "what was shown is sent");
        });
    }

    #[gpui::test]
    fn acknowledging_clears_a_finished_message_but_never_work_in_flight(cx: &mut TestAppContext) {
        let session = session(cx);

        session.update(cx, |session, cx| {
            session.set_status_for_test(EnvStatus::Failed("the service is unreachable".into()), cx);
            session.acknowledge(cx);
            assert_eq!(
                session.status(),
                &EnvStatus::Idle,
                "a finished message must be dismissable, or it sits in the bar forever",
            );
        });

        session.update(cx, |session, cx| {
            session.set_status_for_test(EnvStatus::Working, cx);
            session.acknowledge(cx);
            assert_eq!(
                session.status(),
                &EnvStatus::Working,
                "clearing work in flight would only come straight back and look broken",
            );
        });
    }
}
