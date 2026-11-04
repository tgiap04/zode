use std::path::Path;

use zeroize::Zeroizing;

use crate::client::{self, Precondition, WriteOutcome};
use crate::dek::{Dek, KID_LEN};
use crate::envelope::{self, Kind, Resource, SyncCryptoError};
use crate::state::SyncState;
use crate::sync::{SyncContext, SyncError};

/// The `env-key` slot's name. Owned by `zode_env_sync` (`ENV_KEY_RESOURCE`),
/// which depends on this crate, so the wire name is repeated here.
pub const ENV_KEY_SLOT: &str = "env-key";

/// One stored document sealed directly under the recovery key, and so one
/// that a rotation must re-seal.
///
/// `SharingKey` and `EnvKey` hold other keys (`zode_team_vault`,
/// `zode_env_sync`). Skipping them would leave both unreadable after a
/// rotation, and a lost sharing key costs the user every team envelope. They
/// are re-sealed as opaque bytes under the same resource their owners use, so
/// rotation never needs to understand what is inside. `env-manifest` and the
/// env entries are NOT here: they are sealed under the env key, which a
/// recovery-key rotation does not change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    Artifact(Kind),
    SharingKey,
    EnvKey,
}

impl Slot {
    /// The key slots come first: a rotation moves them all, or none, before
    /// it touches an artifact. See [`rotate`].
    pub const ALL: [Slot; 5] = [
        Slot::SharingKey,
        Slot::EnvKey,
        Slot::Artifact(Kind::Settings),
        Slot::Artifact(Kind::Keymap),
        Slot::Artifact(Kind::Extensions),
    ];

    /// Whether this slot holds another key rather than content. Losing one
    /// costs every `.env` or every team envelope, not one file.
    pub fn holds_key(self) -> bool {
        matches!(self, Slot::SharingKey | Slot::EnvKey)
    }

    pub fn resource(self) -> Result<Resource, SyncCryptoError> {
        match self {
            Slot::Artifact(kind) => Ok(Resource::sync(kind)),
            Slot::SharingKey => Ok(Resource::sharing_key()),
            Slot::EnvKey => Resource::env_singleton(ENV_KEY_SLOT),
        }
    }
}

impl From<Kind> for Slot {
    fn from(kind: Kind) -> Self {
        Slot::Artifact(kind)
    }
}

impl PartialEq<Kind> for Slot {
    fn eq(&self, kind: &Kind) -> bool {
        *self == Slot::Artifact(*kind)
    }
}

impl std::fmt::Display for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Slot::Artifact(kind) => kind.fmt(f),
            Slot::SharingKey => f.write_str(envelope::SHARING_KEY_RESOURCE),
            Slot::EnvKey => f.write_str(ENV_KEY_SLOT),
        }
    }
}

/// What a rotation found and what it still has to do.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RotationPlan {
    /// Slots still encrypted under the old key.
    pub pending: Vec<Slot>,
    /// Slots already carrying the new key's fingerprint — a previous attempt
    /// got this far before it was interrupted.
    pub done: Vec<Slot>,
    /// Slots the account has never stored. Nothing to re-encrypt.
    pub absent: Vec<Slot>,
}

#[derive(Debug)]
pub enum RotationOutcome {
    /// Everything stored is now under the new key.
    Complete,
    /// Another machine wrote while this was running. Nothing is left in a
    /// broken state; the user can start again.
    Interrupted { remaining: Vec<Slot> },
}

/// Works out which stored artifacts still need re-encrypting.
///
/// Reads the key fingerprint out of each envelope rather than tracking
/// progress in a local file. That is what makes a rotation resumable after a
/// crash on a different machine, and what makes the answer true even when the
/// local record is wrong.
pub async fn plan(context: &SyncContext, old: &Dek, new: &Dek) -> Result<RotationPlan, SyncError> {
    let old_kid = old.kid();
    let new_kid = new.kid();
    let mut plan = RotationPlan::default();

    for slot in Slot::ALL {
        let document = client::fetch(
            &context.http_client,
            &context.api_url,
            &context.credential.access_token,
            &slot.resource().map_err(SyncError::Crypto)?,
        )
        .await?;

        let Some(document) = document else {
            plan.absent.push(slot);
            continue;
        };

        match fingerprint(&document.blob) {
            Some(kid) if kid == new_kid => plan.done.push(slot),
            Some(kid) if kid == old_kid => plan.pending.push(slot),
            // Neither key opens it. Re-encrypting would mean decrypting first,
            // which is impossible — so it is left alone rather than destroyed.
            _ => {}
        }
    }

    Ok(plan)
}

/// Re-encrypts every [`Slot`] stored under a new key.
///
/// # Ordering, and why it is what it is
///
/// **Key slots first, all or none.** `sharing-key` and `env-key` are
/// re-sealed before any artifact. If one of them cannot be written — a 402, a
/// conflict, the network — the ones this call already moved are put back
/// under the old key and the call stops: nothing that holds a key is left
/// under a key this machine has not adopted. A 402 is never read as success.
///
/// **The new key is adopted only once every key slot is under it.** `persist`
/// is awaited at that moment, or, for an account with no key slot, after the
/// first artifact lands. Adopting earlier and then losing the network would
/// leave this machine holding a key that opens nothing it needs.
///
/// The caller must keep both keys (`old` and `new`) until this answers
/// [`RotationOutcome::Complete`], and pass the same pair to a second attempt:
/// the fingerprints in the envelopes make that attempt continue exactly where
/// this one stopped, including a key slot whose putting-back failed.
///
/// Each write is conditional on the revision just read, so a third machine
/// writing mid-rotation conflicts rather than being overwritten.
pub async fn rotate<F, Fut>(
    context: &SyncContext,
    old: &Dek,
    new: &Dek,
    state_path: &Path,
    mut persist: F,
) -> Result<RotationOutcome, SyncError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let plan = plan(context, old, new).await?;
    let (key_slots, artifacts): (Vec<Slot>, Vec<Slot>) =
        plan.pending.iter().partition(|slot| slot.holds_key());

    let mut moved: Vec<(Resource, String)> = Vec::new();
    for (index, slot) in key_slots.iter().enumerate() {
        let resource = slot.resource().map_err(SyncError::Crypto)?;
        let stopped = match reseal(context, old, new, &resource, None).await {
            Ok(Resealed::Stored { revision, .. }) => {
                moved.push((resource, revision));
                continue;
            }
            Ok(Resealed::Missing) => continue,
            Ok(Resealed::Lost) => Ok(RotationOutcome::Interrupted {
                remaining: key_slots[index..]
                    .iter()
                    .chain(&artifacts)
                    .copied()
                    .collect(),
            }),
            Err(error) => Err(error),
        };
        put_back(context, old, new, &moved).await;
        return stopped;
    }

    // Every key slot is under the new key now (moved here, or by an earlier
    // attempt), so the new key is the one worth keeping.
    let mut persisted = !moved.is_empty() || !plan.done.is_empty();
    if persisted {
        persist().await;
    }

    let mut remaining = Vec::new();
    for slot in artifacts {
        let Slot::Artifact(kind) = slot else {
            continue;
        };
        let resource = slot.resource().map_err(SyncError::Crypto)?;
        match reseal(context, old, new, &resource, None).await? {
            Resealed::Stored {
                revision,
                plaintext,
            } => {
                if !persisted {
                    persist().await;
                    persisted = true;
                }
                // Only the artifacts have a local sync record, and it hashes
                // text. The key slots are opaque bytes: rotation re-seals them
                // without reading them, which keeps their owners' formats out
                // of this crate.
                let local_hash = text_hash(&plaintext)?;
                let mut state = SyncState::load(state_path);
                state.record(kind, revision, local_hash);
                state
                    .save(state_path)
                    .map_err(|error| SyncError::Io(error.to_string()))?;
            }
            Resealed::Missing => {}
            // Someone else wrote in between. Stop cleanly rather than looping:
            // the remaining slots stay readable under the old key, and the
            // fingerprints make a second attempt resume exactly here.
            Resealed::Lost => remaining.push(slot),
        }
    }

    Ok(if remaining.is_empty() {
        RotationOutcome::Complete
    } else {
        RotationOutcome::Interrupted { remaining }
    })
}

enum Resealed {
    Stored {
        revision: String,
        /// Zeroed on drop: for the key slots these bytes ARE a key.
        plaintext: Zeroizing<Vec<u8>>,
    },
    /// Nothing stored there (any more), or not the revision expected.
    Missing,
    /// Another write landed between the read and this one.
    Lost,
}

/// Moves one stored document from `from` to `to`, conditional on the
/// revision just read. With `expected`, only that revision is moved.
async fn reseal(
    context: &SyncContext,
    from: &Dek,
    to: &Dek,
    resource: &Resource,
    expected: Option<&str>,
) -> Result<Resealed, SyncError> {
    let Some(document) = client::fetch(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        resource,
    )
    .await?
    else {
        return Ok(Resealed::Missing);
    };
    if expected.is_some_and(|expected| expected != document.revision) {
        return Ok(Resealed::Missing);
    }
    let plaintext = open_with(context, from, resource, &document.blob)?;
    let blob = seal_with(context, to, resource, &plaintext)?;
    let written = client::store(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        resource,
        &blob,
        Precondition::Replace(&document.revision),
    )
    .await?;
    Ok(match written {
        WriteOutcome::Stored { revision } => Resealed::Stored {
            revision,
            plaintext,
        },
        WriteOutcome::Conflict(_) | WriteOutcome::Gone => Resealed::Lost,
    })
}

/// Puts key slots this call moved back under the old key, each only if it
/// is still the revision this call wrote. Best effort: one that cannot be put
/// back stays under the new key, which the caller still holds, and the next
/// attempt with the same pair counts it as done.
async fn put_back(context: &SyncContext, old: &Dek, new: &Dek, moved: &[(Resource, String)]) {
    for (resource, revision) in moved.iter().rev() {
        match reseal(context, new, old, resource, Some(revision)).await {
            Ok(Resealed::Stored { .. }) => {}
            Ok(_) => log::warn!("{resource} changed before it could be put back under the old key"),
            Err(error) => log::warn!("could not put {resource} back under the old key: {error}"),
        }
    }
}

fn fingerprint(blob: &str) -> Option<[u8; KID_LEN]> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    let envelope = envelope::from_blob(blob).ok()?;
    BASE64.decode(&envelope.kid).ok()?.try_into().ok()
}

/// Opens one slot as bytes. Zeroed on drop: for the key slots these bytes ARE
/// a key.
fn open_with(
    context: &SyncContext,
    dek: &Dek,
    resource: &Resource,
    blob: &str,
) -> Result<Zeroizing<Vec<u8>>, SyncError> {
    let parsed = envelope::from_blob(blob).map_err(SyncError::Crypto)?;
    envelope::decrypt_at(dek, &context.credential.user_id, resource, &parsed)
        .map(Zeroizing::new)
        .map_err(SyncError::Crypto)
}

fn text_hash(plaintext: &[u8]) -> Result<String, SyncError> {
    let text = std::str::from_utf8(plaintext).map_err(|_| {
        SyncError::Crypto(SyncCryptoError::Malformed(
            "the decrypted content is not text".into(),
        ))
    })?;
    Ok(crate::artifact::hash(text))
}

fn seal_with(
    context: &SyncContext,
    dek: &Dek,
    resource: &Resource,
    plaintext: &[u8],
) -> Result<String, SyncError> {
    let envelope = envelope::encrypt_at(dek, &context.credential.user_id, resource, plaintext)
        .map_err(SyncError::Crypto)?;
    envelope::to_blob(&envelope).map_err(SyncError::Crypto)
}
