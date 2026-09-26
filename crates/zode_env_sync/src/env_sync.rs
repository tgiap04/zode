use std::path::Path;

use zode_sync::client::{self, ClientError, Precondition, WriteOutcome};
use zode_sync::sync::SyncContext;
use zode_sync::{TextDiff, diff, from_blob};

use crate::EnvCryptoError;
use crate::env_dek::EnvDek;
use crate::env_state::{self, EnvState};
use crate::ids::EntryId;
use crate::{seal, secret_file};

#[derive(Debug)]
pub enum EnvSyncError {
    Client(ClientError),
    Crypto(EnvCryptoError),
    Io(String),
}

impl std::fmt::Display for EnvSyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Client(error) => write!(f, "{error}"),
            Self::Crypto(error) => write!(f, "{error}"),
            Self::Io(detail) => write!(f, "{detail}"),
        }
    }
}

impl From<ClientError> for EnvSyncError {
    fn from(error: ClientError) -> Self {
        Self::Client(error)
    }
}

impl From<EnvCryptoError> for EnvSyncError {
    fn from(error: EnvCryptoError) -> Self {
        Self::Crypto(error)
    }
}

/// The remote side of a difference, plus the difference itself.
#[derive(Debug)]
pub struct Divergence {
    pub diff: TextDiff,
    /// Decrypted remote content, ready to be written if the user says so.
    pub remote: String,
    pub revision: String,
    pub seq: u64,
}

/// What a pull found. **Nothing here has written anything.**
///
/// Deciding and writing are separate calls, which is what makes the two
/// refusals structural rather than promised: `KeyMismatch` and `Rollback`
/// cannot reach a write, because the write lives in a different function that
/// takes already-decrypted text as an argument.
#[derive(Debug)]
pub enum PullOutcome {
    UpToDate,
    /// Nothing has ever been pushed for this entry.
    LocalOnly,
    /// Remote moved on; local is untouched since the last sync.
    RemoteNewer(Divergence),
    /// Both sides changed. Applying this loses local work, so the user chooses.
    Conflict(Divergence),
    /// The stored blob cannot be opened with the key this machine holds.
    KeyMismatch(EnvCryptoError),
    /// The server offered a version older than one already applied here.
    ///
    /// Not a decryption failure — the bytes were authentic. It is a server
    /// handing back a credential that was rotated away, and it is the reason
    /// `seq` exists.
    Rollback {
        seen: u64,
        got: u64,
    },
}

#[derive(Debug)]
pub enum PushOutcome {
    Stored {
        revision: String,
        seq: u64,
    },
    /// The local file is already what the server holds.
    UpToDate,
    /// The server moved on. Resolving means taking the remote or overwriting
    /// it deliberately — never both, and never silently.
    Conflict(Divergence),
    /// There is no local file to push.
    NothingToPush,
}

/// Reads the server's copy and works out how it relates to the local file.
pub async fn pull(
    context: &SyncContext,
    env_dek: &EnvDek,
    entry: &EntryId,
    local_path: &Path,
    state_path: &Path,
) -> Result<PullOutcome, EnvSyncError> {
    let local = read_local(local_path)?;
    let resource = seal::entry_resource(entry)?;

    let Some(document) = client::fetch(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        &resource,
    )
    .await?
    else {
        return Ok(PullOutcome::LocalOnly);
    };

    let mut state = EnvState::load(state_path);
    let envelope = from_blob(&document.blob).map_err(EnvCryptoError::from)?;

    let opened = match seal::open(
        env_dek,
        &context.credential.user_id,
        &resource,
        state.seen_seq(entry),
        &envelope,
    ) {
        Ok(opened) => opened,
        // Both returned, not recovered from. There is no branch below this
        // that writes anything.
        Err(EnvCryptoError::Rollback { seen, got }) => {
            return Ok(PullOutcome::Rollback { seen, got });
        }
        Err(error) => return Ok(PullOutcome::KeyMismatch(error)),
    };

    let remote = String::from_utf8(opened.payload)
        .map_err(|_| EnvCryptoError::Malformed("the stored file is not text".into()))?;

    let local_text = local.unwrap_or_default();
    let local_hash = env_state::hash(local_text.as_bytes());

    if local_hash == env_state::hash(remote.as_bytes()) {
        // Agreement is worth recording: it is what lets the next push use a
        // precondition instead of guessing.
        state.record(entry, document.revision, local_hash, opened.seq);
        save(&state, state_path)?;
        return Ok(PullOutcome::UpToDate);
    }

    let unchanged_since_sync = state
        .get(entry)
        .is_some_and(|synced| synced.local_hash == local_hash);

    let divergence = Divergence {
        diff: diff::between(&local_text, &remote),
        remote,
        revision: document.revision,
        seq: opened.seq,
    };

    Ok(if unchanged_since_sync {
        PullOutcome::RemoteNewer(divergence)
    } else {
        PullOutcome::Conflict(divergence)
    })
}

/// Writes remote content over the local file, after copying the current one
/// aside.
///
/// Takes the text rather than fetching it, so the only way to reach this
/// function is to have already decrypted successfully — and to have got past
/// the rollback check, which happens inside `seal::open`.
pub fn apply_remote(
    entry: &EntryId,
    local_path: &Path,
    backups_root: &Path,
    remote: &str,
    revision: String,
    seq: u64,
    state_path: &Path,
) -> Result<(), EnvSyncError> {
    // The backup runs FIRST, and a failure here stops the write: losing a
    // user's credentials because the safety net could not be laid is the exact
    // outcome the safety net exists to prevent.
    if let Some(current) = read_local(local_path)? {
        secret_file::back_up_secret(backups_root, entry, current.as_bytes()).map_err(|error| {
            EnvSyncError::Io(format!(
                "could not back up the current file, so it was not replaced: {error}"
            ))
        })?;
    }

    secret_file::write_secret_atomic(local_path, remote.as_bytes())
        .map_err(|error| EnvSyncError::Io(format!("could not write the file: {error}")))?;

    let mut state = EnvState::load(state_path);
    state.record(entry, revision, env_state::hash(remote.as_bytes()), seq);
    save(&state, state_path)
}

/// One push, built and not yet sent.
///
/// Exists so the bytes a user is shown and the bytes that travel are the SAME
/// VALUE rather than two calls that ought to agree. They could not be compared
/// even if someone tried: the nonce is fresh per seal, so two independent
/// sealings of one file differ, and a test that re-sealed to check the panel
/// would prove nothing at all.
pub struct PreparedPush {
    pub entry: EntryId,
    /// Exactly what the request body will carry.
    pub wire: crate::WireBytes,
    pub seq: u64,
    local_hash: String,
    /// The text that was sealed.
    ///
    /// Kept so a conflict can be diffed against exactly what this push
    /// carried. Re-reading the file at conflict time would diff a third
    /// version nobody chose — the file may have been edited in the seconds
    /// between building the request and the server refusing it.
    local: String,
    /// What this machine believes it is replacing, if anything.
    revision: Option<String>,
}

impl Drop for PreparedPush {
    fn drop(&mut self) {
        // The one place plaintext env content is held longer than a single
        // call. Cleared rather than left for the allocator to hand on.
        use zeroize::Zeroize as _;
        self.local.zeroize();
    }
}

/// Builds a push without sending it.
///
/// `Ok(None)` means there is nothing to do — no local file, or the server
/// already holds exactly this content.
pub fn prepare_push(
    env_dek: &EnvDek,
    user_id: &str,
    entry: &EntryId,
    local_path: &Path,
    state_path: &Path,
) -> Result<Option<PreparedPush>, EnvSyncError> {
    let Some(local) = read_local(local_path)? else {
        return Ok(None);
    };

    let state = EnvState::load(state_path);
    let local_hash = env_state::hash(local.as_bytes());
    let synced = state.get(entry).cloned();

    if synced
        .as_ref()
        .is_some_and(|synced| synced.local_hash == local_hash)
    {
        return Ok(None);
    }

    // One past whatever this machine last applied. A server cannot forge a
    // higher one, so the counter only has to be monotonic per writer.
    let seq = synced.as_ref().map_or(1, |synced| synced.seq + 1);
    let wire = crate::wire::wire_bytes(
        env_dek,
        user_id,
        &seal::entry_resource(entry)?,
        seq,
        local.as_bytes(),
    )?;

    Ok(Some(PreparedPush {
        entry: *entry,
        wire,
        seq,
        local_hash,
        local,
        revision: synced.map(|synced| synced.revision),
    }))
}

/// Sends what [`prepare_push`] built, byte for byte.
pub async fn send_prepared(
    context: &SyncContext,
    env_dek: &EnvDek,
    prepared: PreparedPush,
    state_path: &Path,
) -> Result<PushOutcome, EnvSyncError> {
    let resource = seal::entry_resource(&prepared.entry)?;
    let precondition = match prepared.revision.as_deref() {
        Some(revision) => Precondition::Replace(revision),
        None => Precondition::Create,
    };

    let outcome = client::store(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        &resource,
        // The value built earlier, not a fresh sealing. This line is the whole
        // evidence claim.
        &prepared.wire.blob_base64,
        precondition,
    )
    .await?;

    let mut state = EnvState::load(state_path);
    match outcome {
        WriteOutcome::Stored { revision } => {
            state.record(
                &prepared.entry,
                revision.clone(),
                prepared.local_hash.clone(),
                prepared.seq,
            );
            save(&state, state_path)?;
            Ok(PushOutcome::Stored {
                revision,
                seq: prepared.seq,
            })
        }
        WriteOutcome::Conflict(document) => {
            let envelope = from_blob(&document.blob).map_err(EnvCryptoError::from)?;
            // The rollback check is skipped here on purpose: this blob is being
            // shown for a decision, not applied, and refusing to display an
            // older conflicting version would leave the user unable to see what
            // they are about to overwrite.
            let opened = seal::open(
                env_dek,
                &context.credential.user_id,
                &resource,
                None,
                &envelope,
            )?;
            let remote = String::from_utf8(opened.payload)
                .map_err(|_| EnvCryptoError::Malformed("the stored file is not text".into()))?;
            Ok(PushOutcome::Conflict(Divergence {
                diff: diff::between(&prepared.local, &remote),
                remote,
                revision: document.revision,
                seq: opened.seq,
            }))
        }
        WriteOutcome::Gone => {
            // The server no longer has what this machine believed it was
            // replacing. Forgetting the stale revision turns the next attempt
            // into a create rather than a permanently failing replace.
            state.forget(&prepared.entry);
            save(&state, state_path)?;
            Ok(PushOutcome::NothingToPush)
        }
    }
}

/// Sends the local file, conditional on what this machine last saw.
pub async fn push(
    context: &SyncContext,
    env_dek: &EnvDek,
    entry: &EntryId,
    local_path: &Path,
    state_path: &Path,
) -> Result<PushOutcome, EnvSyncError> {
    let Some(prepared) = prepare_push(
        env_dek,
        &context.credential.user_id,
        entry,
        local_path,
        state_path,
    )?
    else {
        // Told apart by the caller: an absent file and an already-current one
        // are both "nothing sent", and neither is an error.
        return Ok(if read_local(local_path)?.is_some() {
            PushOutcome::UpToDate
        } else {
            PushOutcome::NothingToPush
        });
    };

    send_prepared(context, env_dek, prepared, state_path).await
}

fn read_local(path: &Path) -> Result<Option<String>, EnvSyncError> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(EnvSyncError::Io(format!(
            "could not read {}: {error}",
            path.display()
        ))),
    }
}

fn save(state: &EnvState, path: &Path) -> Result<(), EnvSyncError> {
    state
        .save(path)
        .map_err(|error| EnvSyncError::Io(format!("could not record the sync state: {error}")))
}
