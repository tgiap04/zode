use std::sync::Arc;

use credentials_provider::CredentialsProvider;
use gpui::AsyncApp;

use crate::account::AccountUser;
use crate::tokens::StoredTokens;

/// What the keychain entry holds.
///
/// The user rides along with the tokens so a start with no network can still
/// say who is signed in. Remembering the account that owns the credential
/// already in this entry is not extra disclosure — anyone who can read the
/// entry holds the credential itself, which is worth far more than an email
/// address.
///
/// `flatten` keeps the token fields at the top level, so an entry written
/// before this type existed still reads (`user` defaults to absent), and an
/// older build reading a newer entry ignores the field it does not know.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct StoredSession {
    #[serde(flatten)]
    pub tokens: StoredTokens,
    #[serde(default)]
    pub user: Option<AccountUser>,
}

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
) -> Option<StoredSession> {
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
    match serde_json::from_slice::<StoredSession>(&payload) {
        Ok(session) => Some(session),
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
    user: &AccountUser,
    tokens: &StoredTokens,
    cx: &AsyncApp,
) -> anyhow::Result<()> {
    let payload = serde_json::to_vec(&StoredSession {
        tokens: tokens.clone(),
        user: Some(user.clone()),
    })?;
    credentials
        .write_credentials(KEYCHAIN_URL, &user.id, &payload, cx)
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
