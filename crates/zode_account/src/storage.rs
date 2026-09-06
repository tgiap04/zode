use std::sync::Arc;

use credentials_provider::CredentialsProvider;
use gpui::AsyncApp;

use crate::tokens::StoredTokens;

/// The keychain entry this crate owns.
///
/// Not a real URL — `CredentialsProvider` keys on an arbitrary string, and a
/// scheme nobody serves makes it obvious in a keychain listing that this is
/// Zode's own entry rather than a saved website login.
const KEYCHAIN_URL: &str = "zode://account";

/// Reads the stored session, if there is one.
///
/// Returns `None` for "no session" AND for "the entry is there but unusable" —
/// a keychain that is locked, absent (a minimal Linux install with no
/// libsecret), or holding a payload from an older format. None of those should
/// stop the editor from starting; they only mean the user has to sign in
/// again.
pub async fn read(
    credentials: &Arc<dyn CredentialsProvider>,
    cx: &AsyncApp,
) -> Option<StoredTokens> {
    let stored = match credentials.read_credentials(KEYCHAIN_URL, cx).await {
        Ok(stored) => stored,
        Err(error) => {
            // Logged once, not propagated: an unreadable keychain is a reason
            // to be signed out, not a reason to fail.
            log::warn!("could not read the account keychain entry: {error}");
            return None;
        }
    };

    let (_user_id, payload) = stored?;
    match serde_json::from_slice::<StoredTokens>(&payload) {
        Ok(tokens) => Some(tokens),
        Err(error) => {
            log::warn!(
                "the stored account session could not be parsed, treating it as absent: {error}"
            );
            None
        }
    }
}

/// Persists the session. The user id is the keychain "username" so the entry
/// reads sensibly in Keychain Access / seahorse / credential manager.
pub async fn write(
    credentials: &Arc<dyn CredentialsProvider>,
    user_id: &str,
    tokens: &StoredTokens,
    cx: &AsyncApp,
) -> anyhow::Result<()> {
    let payload = serde_json::to_vec(tokens)?;
    credentials
        .write_credentials(KEYCHAIN_URL, user_id, &payload, cx)
        .await
}

/// Removes the session.
///
/// Errors are logged rather than returned: this runs on the sign-out path,
/// where a user who pressed the button must end up signed out in the running
/// process whatever the keychain says. A failure here means the entry outlives
/// the session, which the next `read` treats as a stale login — recoverable —
/// whereas refusing to sign out is not.
pub async fn delete(credentials: &Arc<dyn CredentialsProvider>, cx: &AsyncApp) {
    if let Err(error) = credentials.delete_credentials(KEYCHAIN_URL, cx).await {
        log::warn!("could not delete the account keychain entry: {error}");
    }
}
