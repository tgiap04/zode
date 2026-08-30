use std::path::Path;
use std::sync::Arc;

use http_client::HttpClient;
use zode_account::ApiCredential;

use crate::artifact::{self, Artifact};
use crate::client::{self, ClientError, Precondition, WriteOutcome};
use crate::dek::Dek;
use crate::diff::{self, TextDiff};
use crate::envelope::{Kind, SyncCryptoError};
use crate::state::SyncState;

/// Everything a sync operation needs from the outside world.
///
/// Passed in rather than reached for, so every function below is drivable by a
/// test with a `FakeHttpClient` and a temporary directory.
pub struct SyncContext {
    pub http_client: Arc<dyn HttpClient>,
    pub api_url: String,
    pub credential: ApiCredential,
}

#[derive(Debug)]
pub enum SyncError {
    Client(ClientError),
    Crypto(SyncCryptoError),
    Io(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Client(error) => write!(f, "{error}"),
            Self::Crypto(error) => write!(f, "{error}"),
            Self::Io(detail) => write!(f, "{detail}"),
        }
    }
}

impl From<ClientError> for SyncError {
    fn from(error: ClientError) -> Self {
        Self::Client(error)
    }
}

/// What a pull found. **Nothing here has written anything.**
///
/// Deciding and writing are separate calls on purpose: it is what makes
/// invariant 5 — a wrong recovery key never touches the local file —
/// structural rather than a promise. `KeyMismatch` cannot reach a write
/// because the write lives in a different function that takes the decrypted
/// text as an argument.
#[derive(Debug)]
pub enum PullOutcome {
    /// Local and remote already agree.
    UpToDate,
    /// Nothing has ever been pushed for this kind.
    LocalOnly,
    /// Remote moved on; local is untouched since the last sync. Applying this
    /// loses nothing.
    RemoteNewer(Divergence),
    /// Both sides changed since the last sync. Applying this loses local work,
    /// so the user must choose.
    Conflict(Divergence),
    /// The stored data cannot be opened with the key this machine holds.
    KeyMismatch(SyncCryptoError),
}

/// The remote side of a difference, plus the difference itself.
#[derive(Debug)]
pub struct Divergence {
    pub diff: TextDiff,
    /// Decrypted remote content, ready to be written if the user says so.
    pub remote: String,
    pub revision: String,
}

#[derive(Debug)]
pub enum PushOutcome {
    Stored {
        revision: String,
    },
    /// The local file is already what the server holds.
    UpToDate,
    /// The server moved on. Resolving means either taking the remote or
    /// overwriting it deliberately — never both, and never silently.
    Conflict(Divergence),
    /// There is no local file to push.
    NothingToPush,
}

/// Reads the server's copy and works out how it relates to the local file.
///
/// The three-way answer is the whole reason `sync_state.json` exists. Without
/// the recorded hash, `RemoteNewer` and `Conflict` are indistinguishable, and
/// a client that cannot tell them apart either asks the user every single time
/// or quietly discards their edits.
pub async fn pull(
    context: &SyncContext,
    dek: &Dek,
    artifact: &Artifact,
    state_path: &Path,
) -> Result<PullOutcome, SyncError> {
    let local = read_local(artifact)?;
    pull_content(context, dek, artifact.kind, &local, state_path).await
}

/// The same decision, for an artifact that is not a file.
///
/// The extension list is derived from what is installed rather than read from
/// disk, so it has content but no path. Splitting the file read off the
/// decision is what lets both go through one implementation instead of two
/// that drift.
pub async fn pull_content(
    context: &SyncContext,
    dek: &Dek,
    kind: Kind,
    local: &str,
    state_path: &Path,
) -> Result<PullOutcome, SyncError> {
    let local = local.to_string();

    let Some(document) = client::fetch(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        kind,
    )
    .await?
    else {
        return Ok(PullOutcome::LocalOnly);
    };

    let remote = match decrypt_document(context, dek, kind, &document.blob) {
        Ok(remote) => remote,
        // Returned, not recovered from. There is no branch below this that
        // writes anything.
        Err(error) => return Ok(PullOutcome::KeyMismatch(error)),
    };

    let local_hash = artifact::hash(&local);
    if local_hash == artifact::hash(&remote) {
        // Agreement is worth recording: it is what lets the next push use a
        // precondition instead of guessing.
        let mut state = SyncState::load(state_path);
        state.record(kind, document.revision, local_hash);
        save(&state, state_path)?;
        return Ok(PullOutcome::UpToDate);
    }

    let divergence = Divergence {
        diff: diff::between(&local, &remote),
        remote,
        revision: document.revision,
    };

    let unchanged_since_sync = SyncState::load(state_path)
        .get(kind)
        .is_some_and(|synced| synced.local_hash == local_hash);

    Ok(if unchanged_since_sync {
        PullOutcome::RemoteNewer(divergence)
    } else {
        PullOutcome::Conflict(divergence)
    })
}

/// Writes remote content over the local file, after copying the current file
/// aside.
///
/// Takes the text rather than fetching it, so the only way to reach this
/// function is to have already decrypted successfully.
pub fn apply_remote(
    artifact: &Artifact,
    remote: &str,
    revision: String,
    state_path: &Path,
) -> Result<(), SyncError> {
    let kind = artifact.kind;

    // Invariant 8, and it runs FIRST. A backup written after the overwrite is
    // a copy of the new file, which is worth nothing to someone trying to get
    // their old one back.
    artifact::back_up(artifact).map_err(|error| {
        SyncError::Io(format!(
            "could not back up the current file, so it was not replaced: {error}"
        ))
    })?;

    artifact::write_atomic(&artifact.path, remote)
        .map_err(|error| SyncError::Io(format!("could not write the file: {error}")))?;

    let mut state = SyncState::load(state_path);
    state.record(kind, revision, artifact::hash(remote));
    save(&state, state_path)
}

/// Sends the local file, conditional on what this machine last saw.
pub async fn push(
    context: &SyncContext,
    dek: &Dek,
    artifact: &Artifact,
    state_path: &Path,
) -> Result<PushOutcome, SyncError> {
    let Some(local) = artifact::read(&artifact.path)
        .map_err(|error| SyncError::Io(format!("could not read the local file: {error}")))?
    else {
        return Ok(PushOutcome::NothingToPush);
    };
    push_content(context, dek, artifact.kind, &local, state_path).await
}

/// The same write, for an artifact that is not a file.
pub async fn push_content(
    context: &SyncContext,
    dek: &Dek,
    kind: Kind,
    local: &str,
    state_path: &Path,
) -> Result<PushOutcome, SyncError> {
    let local = local.to_string();

    let state = SyncState::load(state_path);
    let precondition = match state.get(kind) {
        Some(synced) => Precondition::Replace(&synced.revision),
        None => Precondition::Create,
    };

    match store(context, dek, kind, &local, precondition).await? {
        WriteOutcome::Stored { revision } => {
            record(kind, revision.clone(), &local, state_path)?;
            Ok(PushOutcome::Stored { revision })
        }
        WriteOutcome::Conflict(document) => {
            let remote = match decrypt_document(context, dek, kind, &document.blob) {
                Ok(remote) => remote,
                Err(error) => return Err(SyncError::Crypto(error)),
            };
            if artifact::hash(&remote) == artifact::hash(&local) {
                // The other machine pushed the same bytes. Nothing is in
                // conflict; the revisions merely disagree.
                record(kind, document.revision, &local, state_path)?;
                return Ok(PushOutcome::UpToDate);
            }
            Ok(PushOutcome::Conflict(Divergence {
                diff: diff::between(&local, &remote),
                remote,
                revision: document.revision,
            }))
        }
        WriteOutcome::Gone => {
            // The document was deleted while this machine still remembered a
            // revision for it. Creating it is not destructive — there is
            // nothing left to destroy — so complete what the user asked for
            // and repair the stale state.
            log::info!("the stored {kind} was gone, so the push created it again");
            match store(context, dek, kind, &local, Precondition::Create).await? {
                WriteOutcome::Stored { revision } => {
                    record(kind, revision.clone(), &local, state_path)?;
                    Ok(PushOutcome::Stored { revision })
                }
                // Something wrote in between. Report it rather than looping.
                WriteOutcome::Conflict(document) => {
                    let remote = decrypt_document(context, dek, kind, &document.blob)
                        .map_err(SyncError::Crypto)?;
                    Ok(PushOutcome::Conflict(Divergence {
                        diff: diff::between(&local, &remote),
                        remote,
                        revision: document.revision,
                    }))
                }
                WriteOutcome::Gone => Err(SyncError::Client(ClientError::Rejected(
                    "the sync service could neither replace nor create the document".into(),
                ))),
            }
        }
    }
}

/// Resolves a conflict in favour of the local file.
///
/// Requires the revision the conflict reported, so this is still a conditional
/// write: a third machine that wrote in between conflicts again rather than
/// being overwritten by a decision made about older content.
pub async fn overwrite_remote(
    context: &SyncContext,
    dek: &Dek,
    artifact: &Artifact,
    revision: &str,
    state_path: &Path,
) -> Result<PushOutcome, SyncError> {
    let Some(local) = artifact::read(&artifact.path)
        .map_err(|error| SyncError::Io(format!("could not read the local file: {error}")))?
    else {
        return Ok(PushOutcome::NothingToPush);
    };
    overwrite_remote_content(context, dek, artifact.kind, &local, revision, state_path).await
}

/// The same resolution, for an artifact that is not a file.
pub async fn overwrite_remote_content(
    context: &SyncContext,
    dek: &Dek,
    kind: Kind,
    local: &str,
    revision: &str,
    state_path: &Path,
) -> Result<PushOutcome, SyncError> {
    let local = local.to_string();

    match store(context, dek, kind, &local, Precondition::Replace(revision)).await? {
        WriteOutcome::Stored { revision } => {
            record(kind, revision.clone(), &local, state_path)?;
            Ok(PushOutcome::Stored { revision })
        }
        WriteOutcome::Conflict(document) => {
            let remote =
                decrypt_document(context, dek, kind, &document.blob).map_err(SyncError::Crypto)?;
            Ok(PushOutcome::Conflict(Divergence {
                diff: diff::between(&local, &remote),
                remote,
                revision: document.revision,
            }))
        }
        WriteOutcome::Gone => Ok(PushOutcome::NothingToPush),
    }
}

fn read_local(artifact: &Artifact) -> Result<String, SyncError> {
    Ok(artifact::read(&artifact.path)
        .map_err(|error| SyncError::Io(format!("could not read the local file: {error}")))?
        .unwrap_or_default())
}

async fn store(
    context: &SyncContext,
    dek: &Dek,
    kind: Kind,
    plaintext: &str,
    precondition: Precondition<'_>,
) -> Result<WriteOutcome, SyncError> {
    let envelope =
        crate::envelope::encrypt(dek, &context.credential.user_id, kind, plaintext.as_bytes())
            .map_err(SyncError::Crypto)?;
    let blob = crate::envelope::to_blob(&envelope).map_err(SyncError::Crypto)?;

    client::store(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        kind,
        &blob,
        precondition,
    )
    .await
    .map_err(SyncError::Client)
}

fn decrypt_document(
    context: &SyncContext,
    dek: &Dek,
    kind: Kind,
    blob: &str,
) -> Result<String, SyncCryptoError> {
    let envelope = crate::envelope::from_blob(blob)?;
    let plaintext = crate::envelope::decrypt(dek, &context.credential.user_id, kind, &envelope)?;
    String::from_utf8(plaintext)
        .map_err(|_| SyncCryptoError::Malformed("the decrypted content is not text".into()))
}

fn record(kind: Kind, revision: String, local: &str, state_path: &Path) -> Result<(), SyncError> {
    let mut state = SyncState::load(state_path);
    state.record(kind, revision, artifact::hash(local));
    save(&state, state_path)
}

fn save(state: &SyncState, state_path: &Path) -> Result<(), SyncError> {
    state
        .save(state_path)
        .map_err(|error| SyncError::Io(format!("could not record the sync state: {error}")))
}
