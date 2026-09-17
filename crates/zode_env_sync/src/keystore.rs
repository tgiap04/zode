use std::sync::Arc;

use credentials_provider::CredentialsProvider;
use gpui::AsyncApp;
use zode_sync::DEK_LEN;

use crate::env_dek::EnvDek;

/// The keychain entry this crate owns.
///
/// Separate from `zode://sync-key` because the two keys have different
/// lifetimes: rotating env access after losing a laptop must not force every
/// machine to re-enter a recovery key for settings as well.
const KEYCHAIN_URL: &str = "zode://env-key";

/// Reads the stored env key, if there is one.
///
/// Follows `zode_sync::keystore`: an unreadable keychain means "no key", not
/// "error". A locked keychain, a Linux box with no libsecret, and a payload
/// from an older format all mean the same thing to the user — the key will be
/// unwrapped from the server again — and none should stop the editor.
pub async fn read(credentials: &Arc<dyn CredentialsProvider>, cx: &AsyncApp) -> Option<EnvDek> {
    let stored = match credentials.read_credentials(KEYCHAIN_URL, cx).await {
        Ok(stored) => stored,
        Err(error) => {
            log::warn!("could not read the env key from the keychain: {error}");
            return None;
        }
    };

    let (_user_id, payload) = stored?;
    let bytes: [u8; DEK_LEN] = match payload.try_into() {
        Ok(bytes) => bytes,
        Err(payload) => {
            log::warn!(
                "the stored env key is {} bytes, not {DEK_LEN}; treating it as absent",
                payload.len()
            );
            return None;
        }
    };
    Some(EnvDek::from_bytes(bytes))
}

/// Persists the env key.
pub async fn write(
    credentials: &Arc<dyn CredentialsProvider>,
    user_id: &str,
    env_dek: &EnvDek,
    cx: &AsyncApp,
) -> anyhow::Result<()> {
    credentials
        .write_credentials(KEYCHAIN_URL, user_id, env_dek.expose(), cx)
        .await
}

/// Removes the env key.
///
/// Unlike [`read`], a failure here IS returned. Deleting the key is how a user
/// revokes this machine's access to their stored environment files, and
/// reporting success while it is still in the keychain would be a lie with
/// security consequences.
pub async fn delete(
    credentials: &Arc<dyn CredentialsProvider>,
    cx: &AsyncApp,
) -> anyhow::Result<()> {
    credentials.delete_credentials(KEYCHAIN_URL, cx).await
}
