use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, Context, Entity, EventEmitter, Global, SharedString, Task};
use zode_account::Account;
use zode_sync::sync::SyncContext;
use zode_sync::{Dek, Resource, client, from_blob, to_blob};

use crate::env_dek::{self, EnvDek};
use crate::env_sync::{self, PullOutcome, PushOutcome};
use crate::ids::EntryId;
use crate::manifest::Manifest;
use crate::{Bindings, EnvCryptoError, keystore, seal};

/// A difference waiting on the user.
///
/// Held rather than acted on: applying it overwrites a file full of
/// credentials, and that is not a decision this crate gets to make.
pub struct PendingEnvDivergence {
    pub entry: EntryId,
    pub local_path: PathBuf,
    pub diff: zode_sync::TextDiff,
    pub remote: String,
    pub revision: String,
    pub seq: u64,
    /// True when the local file has not been touched since the last sync, so
    /// taking the remote costs nothing. The modal says so; it does not decide.
    pub safe_to_apply: bool,
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
    /// A push that has been built and shown, waiting to be sent.
    prepared: Option<(crate::env_sync::PreparedPush, PathBuf)>,
    task: Option<Task<()>>,
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
            status: EnvStatus::Idle,
            pending: None,
            prepared: None,
            task: None,
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
        self.bindings.bind(&worktree_root, project);
        if let Err(error) = self.bindings.save(paths::env_bindings_file()) {
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
                Err(EnvCryptoError::KeyRotated { .. } | EnvCryptoError::WrongKey) => {
                    _ = this.update(cx, |this, cx| this.set_status(EnvStatus::KeyMismatch, cx));
                }
                Err(error) => {
                    _ = this.update(cx, |this, cx| {
                        this.set_status(EnvStatus::Failed(format!("{error}").into()), cx);
                    });
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

            client::store(
                &context.http_client,
                &context.api_url,
                &context.credential.access_token,
                &Resource::env_singleton(crate::ENV_KEY_RESOURCE)
                    .map_err(|error| anyhow::anyhow!("{error}"))?,
                &blob,
                client::Precondition::Create,
            )
            .await
            .map_err(|error| anyhow::anyhow!("{error}"))?;

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

        match crate::env_sync::prepare_push(
            &env_dek,
            &user_id,
            &entry,
            &local_path,
            paths::env_state_file(),
        ) {
            Ok(Some(prepared)) => {
                self.prepared = Some((prepared, local_path));
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
    pub fn send_prepared(&mut self, cx: &mut Context<Self>) {
        let Some((prepared, local_path)) = self.prepared.take() else {
            return;
        };
        let Some(env_dek) = self.env_dek.clone() else {
            self.set_status(EnvStatus::NeedsRecoveryKey, cx);
            return;
        };
        let account = self.account.clone();
        let entry = prepared.entry;
        self.set_status(EnvStatus::Working, cx);

        self.task = Some(cx.spawn(async move |this, cx| {
            let Some(context) = build_context(&account, cx).await else {
                _ = this.update(cx, |this, cx| this.set_status(unavailable(), cx));
                return;
            };
            let outcome = crate::env_sync::send_prepared(
                &context,
                &env_dek,
                prepared,
                paths::env_state_file(),
            )
            .await;

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
                    this.hold(entry, local_path, divergence, false, cx);
                }
                Err(error) => {
                    this.set_status(EnvStatus::Failed(format!("{error}").into()), cx);
                }
            });
        }));
    }

    /// Throws away a built push without sending it.
    pub fn discard_prepared(&mut self, cx: &mut Context<Self>) {
        self.prepared = None;
        self.set_status(EnvStatus::Idle, cx);
    }

    /// Sends one file.
    pub fn push(&mut self, entry: EntryId, local_path: PathBuf, cx: &mut Context<Self>) {
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
            let outcome = env_sync::push(
                &context,
                &env_dek,
                &entry,
                &local_path,
                paths::env_state_file(),
            )
            .await;

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
                    this.hold(entry, local_path, divergence, false, cx);
                }
                Err(error) => {
                    this.set_status(EnvStatus::Failed(format!("{error}").into()), cx);
                }
            });
        }));
    }

    /// Reads one file from the server and works out what it would change.
    pub fn pull(&mut self, entry: EntryId, local_path: PathBuf, cx: &mut Context<Self>) {
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
            let outcome = env_sync::pull(
                &context,
                &env_dek,
                &entry,
                &local_path,
                paths::env_state_file(),
            )
            .await;

            _ = this.update(cx, |this, cx| match outcome {
                Ok(PullOutcome::UpToDate) => {
                    this.set_status(EnvStatus::Done("already up to date".into()), cx);
                }
                Ok(PullOutcome::LocalOnly) => {
                    this.set_status(EnvStatus::Done("nothing stored for this file".into()), cx);
                }
                Ok(PullOutcome::RemoteNewer(divergence)) => {
                    this.hold(entry, local_path, divergence, true, cx);
                }
                Ok(PullOutcome::Conflict(divergence)) => {
                    this.hold(entry, local_path, divergence, false, cx);
                }
                Ok(PullOutcome::KeyMismatch(_)) => {
                    this.set_status(EnvStatus::KeyMismatch, cx);
                }
                Ok(PullOutcome::Rollback { seen, got }) => {
                    this.set_status(EnvStatus::Rollback { seen, got }, cx);
                }
                Err(error) => {
                    this.set_status(EnvStatus::Failed(format!("{error}").into()), cx);
                }
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
    pub fn apply_pending(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(pending) = self.pending.as_ref() else {
            return false;
        };
        let applied = env_sync::apply_remote(
            &pending.entry,
            &pending.local_path,
            paths::env_backups_dir(),
            &pending.remote,
            pending.revision.clone(),
            pending.seq,
            paths::env_state_file(),
        );

        match applied {
            Ok(()) => {
                self.pending = None;
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
        if !matches!(self.status, EnvStatus::Working) {
            self.set_status(EnvStatus::Idle, cx);
        }
    }

    /// Discards the held difference without writing anything.
    pub fn dismiss_pending(&mut self, cx: &mut Context<Self>) {
        self.pending = None;
        self.set_status(EnvStatus::Idle, cx);
    }

    fn hold(
        &mut self,
        entry: EntryId,
        local_path: PathBuf,
        divergence: crate::env_sync::Divergence,
        safe_to_apply: bool,
        cx: &mut Context<Self>,
    ) {
        self.pending = Some(PendingEnvDivergence {
            entry,
            local_path,
            diff: divergence.diff,
            remote: divergence.remote,
            revision: divergence.revision,
            seq: divergence.seq,
            safe_to_apply,
        });
        self.set_status(EnvStatus::Idle, cx);
    }

    /// Fetches the catalogue and reconciles it against what the server holds.
    ///
    /// Two requests, both deliberate: the manifest says what the user calls
    /// their files, the listing says which of them still exist. Neither alone
    /// answers "was this deleted on my other machine".
    pub fn load_vault(&mut self, cx: &mut Context<Self>) {
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
                (Err(EnvCryptoError::KeyRotated { .. } | EnvCryptoError::WrongKey), _) => {
                    this.set_status(EnvStatus::KeyMismatch, cx);
                }
                (Err(error), _) | (_, Err(error)) => {
                    this.set_status(EnvStatus::Failed(format!("{error}").into()), cx);
                }
            });
        }));
    }

    /// Finds the catalogue entry for a file on disk, if there is one.
    ///
    /// Answers `None` for three different situations the caller must tell
    /// apart — checkout not bound, project gone, file not in the catalogue —
    /// so the caller asks [`Self::project_for`] first when it needs to say
    /// which.
    pub fn entry_for(
        &self,
        worktree_root: &std::path::Path,
        absolute: &std::path::Path,
    ) -> Option<EntryId> {
        let project = self.bindings.project_for(worktree_root)?;
        let relative = absolute
            .strip_prefix(worktree_root)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/");
        self.manifest
            .projects
            .get(&project)?
            .entries
            .iter()
            .find(|(_, entry)| entry.path == relative)
            .map(|(id, _)| *id)
    }

    pub fn project_for(&self, worktree_root: &std::path::Path) -> Option<crate::ProjectId> {
        self.bindings.project_for(worktree_root)
    }

    /// The path recorded alongside a stored entry, relative to its checkout.
    pub fn relative_path_for(&self, project: crate::ProjectId, entry: EntryId) -> Option<String> {
        Some(
            self.manifest
                .projects
                .get(&project)?
                .entries
                .get(&entry)?
                .path
                .clone(),
        )
    }

    /// Where a stored entry belongs on this machine, when a checkout says so.
    ///
    /// `None` is a real answer rather than a gap: without a binding there is
    /// nothing that knows where the file goes, and inventing a directory would
    /// write somebody's production environment somewhere they never named. The
    /// caller asks instead.
    pub fn local_path_for(&self, project: crate::ProjectId, entry: EntryId) -> Option<PathBuf> {
        let relative = self.relative_path_for(project, entry)?;
        let root = self.bindings.worktrees_for(project).into_iter().next()?;
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
                _ = this.update(cx, |this, cx| {
                    this.set_status(EnvStatus::Failed(format!("{error}").into()), cx);
                });
            }
        })
        .detach();
    }

    /// What the catalogue and the server disagree about, as of the last load.
    pub fn reconciliation(&self) -> &crate::Reconciliation {
        &self.reconciliation
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
}

/// Reads and opens the catalogue. An account with none yet gets an empty one.
async fn load_manifest(
    context: &SyncContext,
    env_dek: &EnvDek,
) -> Result<Manifest, EnvCryptoError> {
    let resource = seal::manifest_resource()?;
    let document = client::fetch(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        &resource,
    )
    .await
    .map_err(|error| EnvCryptoError::Malformed(format!("{error}")))?;

    let Some(document) = document else {
        return Ok(Manifest::new());
    };
    let envelope = from_blob(&document.blob)?;
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
    Manifest::from_json(&opened.payload)
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
) -> Result<(), EnvCryptoError> {
    let resource = seal::manifest_resource()?;
    let current = client::fetch(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        &resource,
    )
    .await
    .map_err(|error| EnvCryptoError::Malformed(format!("{error}")))?;

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
    let blob = to_blob(&envelope)?;

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
    .await
    .map_err(|error| EnvCryptoError::Malformed(format!("{error}")))?;
    Ok(())
}

/// Asks which entries the server actually holds.
async fn list_entries(context: &SyncContext) -> Result<Vec<crate::RemoteEntry>, EnvCryptoError> {
    let body = client::fetch_body(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        &Resource::env_listing(),
    )
    .await
    .map_err(|error| EnvCryptoError::Malformed(format!("{error}")))?;

    match body {
        Some(body) => crate::vault::parse_listing(&body),
        // A 404 on the collection means the route is not there at all, which
        // is not the same as an empty vault and must not be reported as one.
        None => Err(EnvCryptoError::Malformed(
            "the sync service has no environment store".into(),
        )),
    }
}

fn unavailable() -> EnvStatus {
    EnvStatus::Failed("sign in again — this session could not be used".into())
}

/// Reads and unwraps the stored environment key. `Ok(None)` means none exists.
async fn fetch_env_key(context: &SyncContext, dek: &Dek) -> Result<Option<EnvDek>, EnvCryptoError> {
    let resource = Resource::env_singleton(crate::ENV_KEY_RESOURCE)?;
    let document = client::fetch(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        &resource,
    )
    .await
    .map_err(|error| EnvCryptoError::Malformed(format!("{error}")))?;

    let Some(document) = document else {
        return Ok(None);
    };
    let envelope = from_blob(&document.blob)?;
    env_dek::unwrap_key(dek, &context.credential.user_id, &envelope).map(Some)
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
            EnvStatus::Failed("the service is unreachable".into()),
        ] {
            assert!(
                !status.sentence().is_empty(),
                "{status:?} has nothing to say",
            );
        }
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
                session.local_path_for(project, entry),
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
        let (project, entry, manifest) = one_entry();

        session.update(cx, |session, cx| {
            session.set_manifest_for_test(manifest, cx);

            assert_eq!(session.local_path_for(project, entry), None);
            assert_eq!(
                session.relative_path_for(project, entry).as_deref(),
                Some("services/api/.env"),
                "the recorded path still answers, so the caller can offer it a folder",
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
