use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

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

/// What a pending read or write of an [`EnvFile`] resolves to.
pub type EnvFileFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, EnvSyncError>> + 'a>>;

/// The one environment file a pull or push reads and writes.
///
/// Only bytes cross this seam. The key, the decryption and the sync state all
/// stay on the machine running the UI, so an implementation that reaches a
/// remote host hands that host the file's content and nothing that opens any
/// other copy of it.
pub trait EnvFile: Send + Sync {
    /// The current content, or `None` when there is no file.
    fn read(&self) -> EnvFileFuture<'_, Option<Vec<u8>>>;

    /// Replaces the file so that no reader ever sees half of it, readable only
    /// by its owner.
    fn write_atomic(&self, contents: Vec<u8>) -> EnvFileFuture<'_, ()>;

    /// The path on this machine, when the file is on this machine.
    fn local_path(&self) -> Option<&Path> {
        None
    }
}

/// A file on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalEnvFile(PathBuf);

impl LocalEnvFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl EnvFile for LocalEnvFile {
    fn read(&self) -> EnvFileFuture<'_, Option<Vec<u8>>> {
        let read = read_local(&self.0).map(|content| content.map(String::into_bytes));
        Box::pin(async move { read })
    }

    fn write_atomic(&self, contents: Vec<u8>) -> EnvFileFuture<'_, ()> {
        let written = write_local(&self.0, &contents);
        Box::pin(async move { written })
    }

    fn local_path(&self) -> Option<&Path> {
        Some(&self.0)
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
    /// The hash of the local content the difference was worked out against,
    /// or `None` when there was no file. Writing the remote over anything else
    /// would replace content the user was never shown.
    pub local_hash: Option<String>,
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
    pull_file(
        context,
        env_dek,
        entry,
        &LocalEnvFile::new(local_path),
        state_path,
    )
    .await
}

/// [`pull`], for a file wherever it lives.
pub async fn pull_file(
    context: &SyncContext,
    env_dek: &EnvDek,
    entry: &EntryId,
    file: &dyn EnvFile,
    state_path: &Path,
) -> Result<PullOutcome, EnvSyncError> {
    let local = read_text(file).await?;
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

    let existed = local.is_some();
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
        local_hash: existed.then_some(local_hash),
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
    let current = read_local(local_path)?;
    replace_local(
        entry,
        local_path,
        current,
        backups_root,
        remote,
        revision,
        seq,
        state_path,
    )
}

/// [`apply_remote`], provided the file still holds what the difference was
/// worked out against (`seen_local_hash`, `None` for no file).
pub fn apply_remote_if_unchanged(
    entry: &EntryId,
    local_path: &Path,
    seen_local_hash: Option<&str>,
    backups_root: &Path,
    remote: &str,
    revision: String,
    seq: u64,
    state_path: &Path,
) -> Result<(), EnvSyncError> {
    let current = read_local(local_path)?;
    ensure_unchanged(seen_local_hash, current.as_deref().map(str::as_bytes))?;
    replace_local(
        entry,
        local_path,
        current,
        backups_root,
        remote,
        revision,
        seq,
        state_path,
    )
}

#[allow(clippy::too_many_arguments)]
fn replace_local(
    entry: &EntryId,
    local_path: &Path,
    current: Option<String>,
    backups_root: &Path,
    remote: &str,
    revision: String,
    seq: u64,
    state_path: &Path,
) -> Result<(), EnvSyncError> {
    // The backup runs FIRST, and a failure here stops the write: losing a
    // user's credentials because the safety net could not be laid is the exact
    // outcome the safety net exists to prevent.
    if let Some(current) = current {
        back_up_current(backups_root, entry, current.as_bytes())?;
    }

    write_local(local_path, remote.as_bytes())?;
    record_applied(entry, remote, revision, seq, state_path)
}

/// [`apply_remote_if_unchanged`], for a file wherever it lives.
///
/// The current content is read and backed up on this machine before the write
/// is even asked for, so a host that cannot be backed up is a host that is
/// never written to.
#[allow(clippy::too_many_arguments)]
pub async fn apply_remote_to(
    entry: &EntryId,
    file: &dyn EnvFile,
    seen_local_hash: Option<&str>,
    backups_root: &Path,
    remote: &str,
    revision: String,
    seq: u64,
    state_path: &Path,
) -> Result<(), EnvSyncError> {
    let current = file.read().await?;
    ensure_unchanged(seen_local_hash, current.as_deref())?;
    if let Some(current) = current {
        back_up_current(backups_root, entry, &current)?;
    }

    file.write_atomic(remote.as_bytes().to_vec()).await?;
    record_applied(entry, remote, revision, seq, state_path)
}

fn ensure_unchanged(
    seen_local_hash: Option<&str>,
    current: Option<&[u8]>,
) -> Result<(), EnvSyncError> {
    let current_hash = current.map(env_state::hash);
    if current_hash.as_deref() == seen_local_hash {
        return Ok(());
    }
    Err(EnvSyncError::Io(
        "the file changed since you looked; fetch it again to see the difference".into(),
    ))
}

fn back_up_current(
    backups_root: &Path,
    entry: &EntryId,
    current: &[u8],
) -> Result<(), EnvSyncError> {
    secret_file::back_up_secret(backups_root, entry, current).map_err(|error| {
        EnvSyncError::Io(format!(
            "could not back up the current file, so it was not replaced: {error}"
        ))
    })?;
    Ok(())
}

fn record_applied(
    entry: &EntryId,
    remote: &str,
    revision: String,
    seq: u64,
    state_path: &Path,
) -> Result<(), EnvSyncError> {
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
    let local = read_local(local_path)?;
    prepare_push_of(env_dek, user_id, entry, local, state_path)
}

/// [`prepare_push`], for a file wherever it lives.
pub async fn prepare_push_from(
    env_dek: &EnvDek,
    user_id: &str,
    entry: &EntryId,
    file: &dyn EnvFile,
    state_path: &Path,
) -> Result<Option<PreparedPush>, EnvSyncError> {
    let local = read_text(file).await?;
    prepare_push_of(env_dek, user_id, entry, local, state_path)
}

fn prepare_push_of(
    env_dek: &EnvDek,
    user_id: &str,
    entry: &EntryId,
    local: Option<String>,
    state_path: &Path,
) -> Result<Option<PreparedPush>, EnvSyncError> {
    let Some(local) = local else {
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
                local_hash: Some(prepared.local_hash.clone()),
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
    push_file(
        context,
        env_dek,
        entry,
        &LocalEnvFile::new(local_path),
        state_path,
    )
    .await
}

/// [`push`], for a file wherever it lives.
pub async fn push_file(
    context: &SyncContext,
    env_dek: &EnvDek,
    entry: &EntryId,
    file: &dyn EnvFile,
    state_path: &Path,
) -> Result<PushOutcome, EnvSyncError> {
    let local = read_text(file).await?;
    let exists = local.is_some();
    let Some(prepared) = prepare_push_of(
        env_dek,
        &context.credential.user_id,
        entry,
        local,
        state_path,
    )?
    else {
        // Told apart by the caller: an absent file and an already-current one
        // are both "nothing sent", and neither is an error.
        return Ok(if exists {
            PushOutcome::UpToDate
        } else {
            PushOutcome::NothingToPush
        });
    };

    send_prepared(context, env_dek, prepared, state_path).await
}

/// Reads a file as the text a `.env` is.
async fn read_text(file: &dyn EnvFile) -> Result<Option<String>, EnvSyncError> {
    match file.read().await? {
        Some(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| EnvSyncError::Io("could not read the file: it is not UTF-8 text".into())),
        None => Ok(None),
    }
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

fn write_local(path: &Path, contents: &[u8]) -> Result<(), EnvSyncError> {
    secret_file::write_secret_atomic(path, contents)
        .map_err(|error| EnvSyncError::Io(format!("could not write the file: {error}")))
}

fn save(state: &EnvState, path: &Path) -> Result<(), EnvSyncError> {
    state
        .save(path)
        .map_err(|error| EnvSyncError::Io(format!("could not record the sync state: {error}")))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// A file that records what was asked of it, standing in for one on a
    /// remote host.
    struct RecordingFile {
        content: Option<Vec<u8>>,
        writes: Mutex<Vec<Vec<u8>>>,
        write_fails: bool,
    }

    impl RecordingFile {
        fn holding(content: &[u8]) -> Self {
            Self {
                content: Some(content.to_vec()),
                writes: Mutex::default(),
                write_fails: false,
            }
        }
    }

    impl EnvFile for RecordingFile {
        fn read(&self) -> EnvFileFuture<'_, Option<Vec<u8>>> {
            let content = self.content.clone();
            Box::pin(async move { Ok(content) })
        }

        fn write_atomic(&self, contents: Vec<u8>) -> EnvFileFuture<'_, ()> {
            Box::pin(async move {
                if self.write_fails {
                    return Err(EnvSyncError::Io("the connection was lost".into()));
                }
                self.writes.lock().expect("not poisoned").push(contents);
                Ok(())
            })
        }
    }

    struct Sandbox(PathBuf);

    impl Sandbox {
        fn new(name: &str) -> Self {
            let directory =
                std::env::temp_dir().join(format!("zode-env-apply-{name}-{}", std::process::id()));
            std::fs::remove_dir_all(&directory).ok();
            std::fs::create_dir_all(&directory).expect("the sandbox must be creatable");
            Self(directory)
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn entry() -> EntryId {
        EntryId::parse(&"a1".repeat(16)).expect("a valid fixture id")
    }

    #[test]
    fn a_backup_that_fails_means_the_file_is_never_written() {
        let sandbox = Sandbox::new("backup-fails");
        // A regular file where the backup directory must go.
        let backups_root = sandbox.0.join("backups");
        std::fs::write(&backups_root, b"not a directory").unwrap();
        let state_path = sandbox.0.join("state.json");
        let file = RecordingFile::holding(b"OLD=1\n");

        let result = futures::executor::block_on(apply_remote_to(
            &entry(),
            &file,
            Some(&env_state::hash(b"OLD=1\n")),
            &backups_root,
            "NEW=1\n",
            "rev-2".into(),
            2,
            &state_path,
        ));

        let error = result.expect_err("a failed backup must stop the apply");
        assert!(
            error.to_string().contains("could not back up"),
            "unexpected error: {error}"
        );
        assert!(
            file.writes.lock().unwrap().is_empty(),
            "the write was sent without a backup"
        );
        assert!(
            !state_path.exists(),
            "a write that never happened was recorded"
        );
    }

    #[test]
    fn the_current_content_is_backed_up_before_it_is_replaced() {
        let sandbox = Sandbox::new("backup-first");
        let backups_root = sandbox.0.join("backups");
        let state_path = sandbox.0.join("state.json");
        let file = RecordingFile::holding(b"OLD=1\n");

        futures::executor::block_on(apply_remote_to(
            &entry(),
            &file,
            Some(&env_state::hash(b"OLD=1\n")),
            &backups_root,
            "NEW=1\n",
            "rev-2".into(),
            2,
            &state_path,
        ))
        .expect("the apply succeeds");

        assert_eq!(*file.writes.lock().unwrap(), vec![b"NEW=1\n".to_vec()]);
        let backups: Vec<Vec<u8>> = std::fs::read_dir(backups_root.join(entry().as_hex()))
            .unwrap()
            .filter_map(Result::ok)
            .map(|backup| std::fs::read(backup.path()).unwrap())
            .collect();
        assert_eq!(backups, vec![b"OLD=1\n".to_vec()]);
        assert!(EnvState::load(&state_path).get(&entry()).is_some());
    }

    #[test]
    fn a_write_lost_in_transit_is_not_recorded_as_applied() {
        let sandbox = Sandbox::new("write-lost");
        let backups_root = sandbox.0.join("backups");
        let state_path = sandbox.0.join("state.json");
        let file = RecordingFile {
            write_fails: true,
            ..RecordingFile::holding(b"OLD=1\n")
        };

        let result = futures::executor::block_on(apply_remote_to(
            &entry(),
            &file,
            Some(&env_state::hash(b"OLD=1\n")),
            &backups_root,
            "NEW=1\n",
            "rev-2".into(),
            2,
            &state_path,
        ));

        assert!(result.is_err(), "a lost write must be reported");
        assert!(
            backups_root.join(entry().as_hex()).is_dir(),
            "the backup taken before the write must survive it"
        );
        assert!(
            EnvState::load(&state_path).get(&entry()).is_none(),
            "the next pull must still see the difference"
        );
    }

    #[test]
    fn a_remote_file_edited_since_the_difference_was_shown_is_not_replaced() {
        let sandbox = Sandbox::new("remote-edited");
        let backups_root = sandbox.0.join("backups");
        let state_path = sandbox.0.join("state.json");

        for (seen, file) in [
            (
                Some(env_state::hash(b"OLD=1\n")),
                RecordingFile::holding(b"EDITED=1\n"),
            ),
            (None, RecordingFile::holding(b"CREATED=1\n")),
            (
                Some(env_state::hash(b"OLD=1\n")),
                RecordingFile {
                    content: None,
                    ..RecordingFile::holding(b"")
                },
            ),
        ] {
            let result = futures::executor::block_on(apply_remote_to(
                &entry(),
                &file,
                seen.as_deref(),
                &backups_root,
                "NEW=1\n",
                "rev-2".into(),
                2,
                &state_path,
            ));

            let error = result.expect_err("content nobody was shown must not be replaced");
            assert!(
                error.to_string().contains("changed since you looked"),
                "unexpected error: {error}"
            );
            assert!(file.writes.lock().unwrap().is_empty());
        }
        assert!(EnvState::load(&state_path).get(&entry()).is_none());
    }

    #[test]
    fn a_local_file_edited_since_the_difference_was_shown_is_not_replaced() {
        let sandbox = Sandbox::new("local-edited");
        let target = sandbox.0.join(".env");
        std::fs::write(&target, b"EDITED=1\n").unwrap();
        let state_path = sandbox.0.join("state.json");

        let result = apply_remote_if_unchanged(
            &entry(),
            &target,
            Some(&env_state::hash(b"OLD=1\n")),
            &sandbox.0.join("backups"),
            "NEW=1\n",
            "rev-2".into(),
            2,
            &state_path,
        );

        let error = result.expect_err("content nobody was shown must not be replaced");
        assert!(
            error.to_string().contains("changed since you looked"),
            "unexpected error: {error}"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"EDITED=1\n");

        apply_remote_if_unchanged(
            &entry(),
            &target,
            Some(&env_state::hash(b"EDITED=1\n")),
            &sandbox.0.join("backups"),
            "NEW=1\n",
            "rev-2".into(),
            2,
            &state_path,
        )
        .expect("the content that was shown is replaced");
        assert_eq!(std::fs::read(&target).unwrap(), b"NEW=1\n");
    }
}
