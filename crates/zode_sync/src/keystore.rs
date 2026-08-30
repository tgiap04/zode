use std::sync::Arc;

use credentials_provider::CredentialsProvider;
use gpui::AsyncApp;

use crate::dek::{DEK_LEN, Dek};

/// The keychain entry this crate owns.
///
/// Separate from `zode://account` on purpose: the tokens and the encryption
/// key have different lifetimes. Signing out drops the session; it must not
/// silently destroy the key that opens data still sitting on the server.
const KEYCHAIN_URL: &str = "zode://sync-key";

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
    let bytes: [u8; DEK_LEN] = match payload.try_into() {
        Ok(bytes) => bytes,
        Err(payload) => {
            log::warn!(
                "the stored sync key is {} bytes, not {DEK_LEN}; treating it as absent",
                payload.len()
            );
            return None;
        }
    };
    Some(Dek::from_bytes(bytes))
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
