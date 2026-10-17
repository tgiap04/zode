//! What a relay connection needs from the outside world, gathered in one place
//! so a test can supply every piece without a signed-in account, a keychain or
//! a network. Shared by the host and by the Zode that controls another.

use std::sync::Arc;

use credentials_provider::CredentialsProvider;
use gpui::{App, AsyncApp, Entity, Task};
use http_client::HttpClient;
use parking_lot::Mutex;
use remote_relay_protocol::KEY_LEN;

use crate::{
    AccountCredentials, CredentialError, DirectoryError, RelayCredential, RelayCredentials,
    local_device_name, register_public_key,
};
use zode_account::Account;

pub struct RelayEnvironment {
    pub credentials: Arc<dyn RelayCredentials>,
    pub keychain: Arc<dyn CredentialsProvider>,
    pub http_client: Arc<dyn HttpClient>,
    pub api_url: String,
    pub device_name: String,
}

impl RelayEnvironment {
    /// The real thing: the signed-in account's credential and keychain, its
    /// HTTP client, and this machine's name.
    pub fn from_account(account: &Entity<Account>, cx: &App) -> Self {
        let account_state = account.read(cx);
        Self {
            credentials: Arc::new(AccountCredentials(account.clone())),
            keychain: account_state.credentials_provider(),
            http_client: account_state.http_client(),
            api_url: account_state.api_url().to_string(),
            device_name: local_device_name(),
        }
    }
}

/// Hands out the account's credential, after telling the account service this
/// device's public key.
///
/// The relay only admits devices the account service knows, and a browser can
/// only pair with a key it can look up, so the key is registered before the
/// first connection and again whenever the relay says it has forgotten this
/// device. Registering is idempotent, so doing it twice is harmless.
pub struct RegisteringCredentials {
    inner: Arc<dyn RelayCredentials>,
    http_client: Arc<dyn HttpClient>,
    api_url: String,
    device_name: String,
    public_key: [u8; KEY_LEN],
    registered_as: Arc<Mutex<Option<String>>>,
}

impl RegisteringCredentials {
    pub fn new(environment: &RelayEnvironment, public_key: [u8; KEY_LEN]) -> Self {
        Self {
            inner: environment.credentials.clone(),
            http_client: environment.http_client.clone(),
            api_url: environment.api_url.clone(),
            device_name: environment.device_name.clone(),
            public_key,
            registered_as: Arc::default(),
        }
    }
}

impl RelayCredentials for RegisteringCredentials {
    fn credential(&self, cx: &mut AsyncApp) -> Task<Result<RelayCredential, CredentialError>> {
        use gpui::AppContext as _;

        let credential = self.inner.credential(cx);
        let http_client = self.http_client.clone();
        let api_url = self.api_url.clone();
        let device_name = self.device_name.clone();
        let public_key = self.public_key;
        let registered_as = self.registered_as.clone();
        cx.background_spawn(async move {
            let credential = credential.await?;
            if registered_as.lock().as_deref() == Some(credential.device_id.as_str()) {
                return Ok(credential);
            }
            match register_public_key(
                &http_client,
                &api_url,
                &credential.bearer,
                &device_name,
                &public_key,
            )
            .await
            {
                Ok(()) => {
                    *registered_as.lock() = Some(credential.device_id.clone());
                    Ok(credential)
                }
                Err(DirectoryError::Revoked) => Err(CredentialError::Revoked),
                Err(error) => Err(CredentialError::Unavailable(error.to_string())),
            }
        })
    }

    fn invalidate(&self) {
        *self.registered_as.lock() = None;
        self.inner.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{InMemoryKeychain, StaticCredentials};
    use gpui::TestAppContext;
    use http_client::{AsyncBody, FakeHttpClient, Response};

    fn environment(status: u16, body: &'static str, puts: Arc<Mutex<usize>>) -> RelayEnvironment {
        RelayEnvironment {
            credentials: Arc::new(StaticCredentials::new("user", "device-1", "token")),
            keychain: InMemoryKeychain::new(),
            http_client: FakeHttpClient::create(move |_| {
                let puts = puts.clone();
                async move {
                    *puts.lock() += 1;
                    Ok(Response::builder()
                        .status(status)
                        .body(AsyncBody::from(body))
                        .expect("a response"))
                }
            }),
            api_url: "https://api.test/api".into(),
            device_name: "Mac".into(),
        }
    }

    #[gpui::test]
    async fn the_key_is_registered_once_per_device_until_the_relay_forgets_it(
        cx: &mut TestAppContext,
    ) {
        let puts = Arc::new(Mutex::new(0));
        let credentials =
            RegisteringCredentials::new(&environment(200, "{}", puts.clone()), [4; KEY_LEN]);

        let first = credentials
            .credential(&mut cx.to_async())
            .await
            .expect("a credential");
        assert_eq!(
            (first.device_id.as_str(), first.bearer.as_str()),
            ("device-1", "token")
        );
        credentials
            .credential(&mut cx.to_async())
            .await
            .expect("again");
        assert_eq!(
            *puts.lock(),
            1,
            "registering is not repeated for the same device"
        );

        credentials.invalidate();
        credentials
            .credential(&mut cx.to_async())
            .await
            .expect("after invalidation");
        assert_eq!(
            *puts.lock(),
            2,
            "a relay that forgot the device is told again"
        );
    }

    #[gpui::test]
    async fn a_revoked_device_is_told_apart_from_a_service_that_is_down(cx: &mut TestAppContext) {
        let revoked = RegisteringCredentials::new(
            &environment(403, r#"{"error":"device_revoked"}"#, Arc::default()),
            [4; KEY_LEN],
        );
        assert_eq!(
            revoked.credential(&mut cx.to_async()).await.err(),
            Some(CredentialError::Revoked)
        );

        let down = RegisteringCredentials::new(
            &environment(503, "unavailable", Arc::default()),
            [4; KEY_LEN],
        );
        assert!(matches!(
            down.credential(&mut cx.to_async()).await.err(),
            Some(CredentialError::Unavailable(_))
        ));
    }
}
