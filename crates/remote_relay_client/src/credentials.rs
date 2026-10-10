use base64::{
    Engine as _,
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
};
use gpui::{AppContext as _, AsyncApp, Entity, Task};
use serde::Deserialize;
use zode_account::Account;

/// What the relay needs to know who is connecting: the access token it will
/// verify, and the account and device the token names.
#[derive(Clone)]
pub struct RelayCredential {
    pub bearer: String,
    pub user_id: String,
    pub device_id: String,
}

impl std::fmt::Debug for RelayCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RelayCredential")
            .field("user_id", &self.user_id)
            .field("device_id", &self.device_id)
            .field("bearer", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialError {
    /// Cannot connect as anyone right now — signed out, or the token could not
    /// be refreshed — and the caller tries again later.
    #[error("no credential is available: {0}")]
    Unavailable(String),
    /// The account has revoked this device. Trying again changes nothing.
    #[error("this device has been revoked")]
    Revoked,
}

/// Where the relay credential comes from. A trait so a connection can be
/// driven in a test without a signed-in account.
pub trait RelayCredentials: Send + Sync + 'static {
    fn credential(&self, cx: &mut AsyncApp) -> Task<Result<RelayCredential, CredentialError>>;

    /// The relay refused the last credential. A source that prepares anything
    /// before handing one out — registering a key, say — forgets that it did.
    fn invalidate(&self) {}
}

/// The signed-in Zode account.
pub struct AccountCredentials(pub Entity<Account>);

impl RelayCredentials for AccountCredentials {
    fn credential(&self, cx: &mut AsyncApp) -> Task<Result<RelayCredential, CredentialError>> {
        let credential = self.0.update(cx, |account, cx| account.api_credential(cx));
        cx.background_spawn(async move {
            let credential = credential.await.ok_or_else(|| {
                CredentialError::Unavailable("not signed in, or the session expired".into())
            })?;
            let device_id =
                device_id_from_access_token(&credential.access_token).ok_or_else(|| {
                    CredentialError::Unavailable("the access token names no device".into())
                })?;
            Ok(RelayCredential {
                bearer: credential.access_token,
                user_id: credential.user_id.to_string(),
                device_id,
            })
        })
    }
}

/// The `did` claim of an access token.
///
/// The signature is not checked: the token is this process's own, and the
/// only thing read from it is which device it was issued to. The relay is the
/// party that verifies it.
pub fn device_id_from_access_token(token: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Claims {
        did: Option<String>,
    }
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| URL_SAFE.decode(payload))
        .ok()?;
    let claims: Claims = serde_json::from_slice(&bytes).ok()?;
    claims.did.filter(|did| !did.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(claims: &str) -> String {
        format!(
            "{}.{}.signature",
            URL_SAFE_NO_PAD.encode(r#"{"alg":"HS256"}"#),
            URL_SAFE_NO_PAD.encode(claims)
        )
    }

    #[test]
    fn the_device_is_read_from_the_did_claim() {
        assert_eq!(
            device_id_from_access_token(&token(r#"{"sub":"u1","did":"device-7"}"#)).as_deref(),
            Some("device-7")
        );
    }

    #[test]
    fn a_token_without_a_device_yields_none() {
        assert_eq!(device_id_from_access_token(&token(r#"{"sub":"u1"}"#)), None);
        assert_eq!(
            device_id_from_access_token(&token(r#"{"sub":"u1","did":""}"#)),
            None
        );
        assert_eq!(device_id_from_access_token("not-a-jwt"), None);
        assert_eq!(device_id_from_access_token("a.@@@.c"), None);
    }

    #[test]
    fn the_bearer_never_appears_in_debug_output() {
        let credential = RelayCredential {
            bearer: "secret-token".into(),
            user_id: "u".into(),
            device_id: "d".into(),
        };
        assert!(!format!("{credential:?}").contains("secret-token"));
    }
}
