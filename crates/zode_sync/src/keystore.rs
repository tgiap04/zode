use std::sync::Arc;

use credentials_provider::CredentialsProvider;
use gpui::AsyncApp;
use zeroize::Zeroizing;

use crate::dek::{DEK_LEN, Dek};

/// The keychain entry this crate owns.
///
/// Separate from `zode://account` on purpose: the tokens and the encryption
/// key have different lifetimes. Signing out drops the session; it must not
/// silently destroy the key that opens data still sitting on the server.
const KEYCHAIN_URL: &str = "zode://sync-key";

/// The two keys of a recovery-key rotation that has not finished: the old
/// one, then the new one, 64 bytes. Written before the first server write and
/// removed only once everything is under the new key, so neither key can be
/// lost while something still needs it, and a second attempt continues with
/// the same new key instead of making a third.
const ROTATION_KEYCHAIN_URL: &str = "zode://sync-key-rotation";

/// Reads the stored key, if there is one.
///
/// Follows `zode_account::storage`: an unreadable keychain means "no key", not
/// "error". A locked keychain, a Linux box with no libsecret, or a payload
/// from an older format all mean the same thing to the user — they will be
/// asked for their recovery key — and none of them should stop the editor.
pub async fn read(credentials: &Arc<dyn CredentialsProvider>, cx: &AsyncApp) -> Option<Dek> {
    let stored = match credentials.read_credentials(KEYCHAIN_URL, cx).await {
        Ok(stored) => stored,
        Err(error) => {
            log::warn!("could not read the sync key from the keychain: {error}");
            return None;
        }
    };

    let (_user_id, payload) = stored?;
    let payload = Zeroizing::new(payload);
    let dek = Dek::from_slice(&payload);
    if dek.is_none() {
        log::warn!(
            "the stored sync key is {} bytes, not {DEK_LEN}; treating it as absent",
            payload.len()
        );
    }
    dek
}

/// Persists the key. The user id is the keychain "username" so the entry reads
/// sensibly in Keychain Access / seahorse / Credential Manager.
pub async fn write(
    credentials: &Arc<dyn CredentialsProvider>,
    user_id: &str,
    dek: &Dek,
    cx: &AsyncApp,
) -> anyhow::Result<()> {
    credentials
        .write_credentials(KEYCHAIN_URL, user_id, dek.bytes(), cx)
        .await
}

/// Removes the key.
///
/// Unlike `read`, a failure here IS returned. Deleting the key is how a user
/// revokes local access to their synced data, and reporting success when the
/// key is still in the keychain would be a lie with security consequences.
pub async fn delete(
    credentials: &Arc<dyn CredentialsProvider>,
    cx: &AsyncApp,
) -> anyhow::Result<()> {
    credentials.delete_credentials(KEYCHAIN_URL, cx).await
}

/// The unfinished rotation's `(old, new)` keys, if there is one. Unreadable
/// means none, as for [`read`].
pub async fn read_rotation(
    credentials: &Arc<dyn CredentialsProvider>,
    cx: &AsyncApp,
) -> Option<(Dek, Dek)> {
    let stored = match credentials
        .read_credentials(ROTATION_KEYCHAIN_URL, cx)
        .await
    {
        Ok(stored) => stored,
        Err(error) => {
            log::warn!("could not read the unfinished key rotation from the keychain: {error}");
            return None;
        }
    };
    let (_user_id, payload) = stored?;
    let payload = Zeroizing::new(payload);
    if payload.len() != 2 * DEK_LEN {
        log::warn!(
            "the stored key rotation is {} bytes, not {}; treating it as absent",
            payload.len(),
            2 * DEK_LEN
        );
        return None;
    }
    let (old, new) = payload.split_at(DEK_LEN);
    Some((Dek::from_slice(old)?, Dek::from_slice(new)?))
}

/// Keeps both keys of a rotation about to start. A failure is returned: a
/// rotation must not begin with a new key this machine could lose.
pub async fn write_rotation(
    credentials: &Arc<dyn CredentialsProvider>,
    user_id: &str,
    old: &Dek,
    new: &Dek,
    cx: &AsyncApp,
) -> anyhow::Result<()> {
    let mut payload = Zeroizing::new(Vec::with_capacity(2 * DEK_LEN));
    payload.extend_from_slice(old.bytes());
    payload.extend_from_slice(new.bytes());
    credentials
        .write_credentials(ROTATION_KEYCHAIN_URL, user_id, &payload, cx)
        .await
}

/// Forgets a finished (or abandoned) rotation's keys.
pub async fn delete_rotation(
    credentials: &Arc<dyn CredentialsProvider>,
    cx: &AsyncApp,
) -> anyhow::Result<()> {
    credentials
        .delete_credentials(ROTATION_KEYCHAIN_URL, cx)
        .await
}
