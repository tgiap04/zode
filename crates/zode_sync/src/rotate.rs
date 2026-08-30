use std::path::Path;

use crate::client::{self, Precondition, WriteOutcome};
use crate::dek::{Dek, KID_LEN};
use crate::envelope::{self, Kind, SyncCryptoError};
use crate::state::SyncState;
use crate::sync::{SyncContext, SyncError};

/// What a rotation found and what it still has to do.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RotationPlan {
    /// Kinds still encrypted under the old key.
    pub pending: Vec<Kind>,
    /// Kinds already carrying the new key's fingerprint — a previous attempt
    /// got this far before it was interrupted.
    pub done: Vec<Kind>,
    /// Kinds the account has never stored. Nothing to re-encrypt.
    pub absent: Vec<Kind>,
}

#[derive(Debug)]
pub enum RotationOutcome {
    /// Everything stored is now under the new key.
    Complete,
    /// Another machine wrote while this was running. Nothing is left in a
    /// broken state; the user can start again.
    Interrupted { remaining: Vec<Kind> },
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

    for kind in Kind::ALL {
        let document = client::fetch(
            &context.http_client,
            &context.api_url,
            &context.credential.access_token,
            kind,
        )
        .await?;

        let Some(document) = document else {
            plan.absent.push(kind);
            continue;
        };

        match fingerprint(&document.blob) {
            Some(kid) if kid == new_kid => plan.done.push(kind),
            Some(kid) if kid == old_kid => plan.pending.push(kind),
            // Neither key opens it. Re-encrypting would mean decrypting first,
            // which is impossible — so it is left alone rather than destroyed.
            _ => {}
        }
    }

    Ok(plan)
}

/// Re-encrypts everything stored under a new key.
///
/// # Ordering, and why it is what it is
///
/// The new key is written to the keychain only AFTER the first successful
/// upload. Writing it first and then losing the network would leave this
/// machine holding a key that opens nothing, having discarded the one that
/// opened everything.
///
/// Each write is still conditional on the revision just read, so a third
/// machine writing mid-rotation conflicts rather than being overwritten.
///
/// The caller passes `persist`, awaited once at the first safe moment. It is
/// injected — and async — so the keychain write happens at exactly this point
/// in the sequence rather than after the function returns, where a crash would
/// land between the two.
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
    let mut persisted = !plan.done.is_empty();
    if persisted {
        // A previous attempt already wrote under the new key, so this machine
        // must be holding it to have got here.
        persist().await;
    }

    let mut remaining = Vec::new();

    for kind in plan.pending {
        let Some(document) = client::fetch(
            &context.http_client,
            &context.api_url,
            &context.credential.access_token,
            kind,
        )
        .await?
        else {
            continue;
        };

        let plaintext = decrypt_with(context, old, kind, &document.blob)?;
        let blob = encrypt_with(context, new, kind, &plaintext)?;

        let written = client::store(
            &context.http_client,
            &context.api_url,
            &context.credential.access_token,
            kind,
            &blob,
            Precondition::Replace(&document.revision),
        )
        .await?;

        match written {
            WriteOutcome::Stored { revision } => {
                if !persisted {
                    // The first artifact is safely under the new key, so the
                    // new key is now the one worth keeping.
                    persist().await;
                    persisted = true;
                }
                let mut state = SyncState::load(state_path);
                state.record(kind, revision, crate::artifact::hash(&plaintext));
                state
                    .save(state_path)
                    .map_err(|error| SyncError::Io(error.to_string()))?;
            }
            // Someone else wrote in between. Stop cleanly rather than looping:
            // the remaining artifacts stay readable under the old key, and the
            // fingerprints make a second attempt resume exactly here.
            WriteOutcome::Conflict(_) | WriteOutcome::Gone => remaining.push(kind),
        }
    }

    Ok(if remaining.is_empty() {
        RotationOutcome::Complete
    } else {
        RotationOutcome::Interrupted { remaining }
    })
}

fn fingerprint(blob: &str) -> Option<[u8; KID_LEN]> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    let envelope = envelope::from_blob(blob).ok()?;
    BASE64.decode(&envelope.kid).ok()?.try_into().ok()
}

fn decrypt_with(
    context: &SyncContext,
    dek: &Dek,
    kind: Kind,
    blob: &str,
) -> Result<String, SyncError> {
    let parsed = envelope::from_blob(blob).map_err(SyncError::Crypto)?;
    let bytes = envelope::decrypt(dek, &context.credential.user_id, kind, &parsed)
        .map_err(SyncError::Crypto)?;
    String::from_utf8(bytes).map_err(|_| {
        SyncError::Crypto(SyncCryptoError::Malformed(
            "the decrypted content is not text".into(),
        ))
    })
}

fn encrypt_with(
    context: &SyncContext,
    dek: &Dek,
    kind: Kind,
    plaintext: &str,
) -> Result<String, SyncError> {
    let envelope = envelope::encrypt(dek, &context.credential.user_id, kind, plaintext.as_bytes())
        .map_err(SyncError::Crypto)?;
    envelope::to_blob(&envelope).map_err(SyncError::Crypto)
}
