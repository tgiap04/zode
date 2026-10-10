//! This device's long-term key, and telling the account service about its
//! public half.
//!
//! The private key lives in the OS keychain and nowhere else: not in settings,
//! not in the database, not in a log. Losing it is recoverable — pair again —
//! while leaking it lets anyone who also controls the relay impersonate every
//! browser this device trusts.

use std::sync::Arc;

use anyhow::{Context as _, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use credentials_provider::CredentialsProvider;
use futures::AsyncReadExt as _;
use gpui::AsyncApp;
use http_client::{AsyncBody, HttpClient, Request};
use remote_relay_protocol::{DeviceKeypair, KEY_LEN};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// The account service refuses names longer than this.
const MAX_DEVICE_NAME_CHARS: usize = 64;

fn keychain_url(user_id: &str) -> String {
    // One entry per account: a key made for one account must not answer to
    // another's pins, and signing out of one account must not delete the
    // other's key.
    format!("zode://remote-device-key/{user_id}")
}

/// Reads this device's key for `user_id`, making and storing one first if the
/// keychain has none.
///
/// An unreadable keychain is an error and not "no key": minting a fresh key
/// every time the keychain hiccups would orphan every pin in silence. The same
/// goes for a key that was made but could not be saved, which would be
/// forgotten at exit.
pub async fn load_or_create_keypair(
    provider: &Arc<dyn CredentialsProvider>,
    user_id: &str,
    cx: &AsyncApp,
) -> Result<Arc<DeviceKeypair>> {
    let url = keychain_url(user_id);
    let stored = provider
        .read_credentials(&url, cx)
        .await
        .context("the keychain could not be read")?;

    if let Some((_, payload)) = stored {
        // The keychain hands back a plain Vec holding the private key.
        let payload = Zeroizing::new(payload);
        match decode_keypair(&payload) {
            Some(keypair) => return Ok(Arc::new(keypair)),
            None => log::warn!(
                "the stored device key is {} bytes, not {}; making a new one, which means \
                 paired devices must pair again",
                payload.len(),
                KEY_LEN * 2
            ),
        }
    }

    let keypair = DeviceKeypair::generate().context("a device key could not be generated")?;
    let payload = Zeroizing::new(encode_keypair(&keypair));
    provider
        .write_credentials(&url, user_id, &payload, cx)
        .await
        .context("the new device key could not be saved to the keychain")?;
    Ok(Arc::new(keypair))
}

/// Removes the key, for a device the account has revoked.
pub async fn delete_keypair(
    provider: &Arc<dyn CredentialsProvider>,
    user_id: &str,
    cx: &AsyncApp,
) -> Result<()> {
    provider
        .delete_credentials(&keychain_url(user_id), cx)
        .await
        .context("the device key could not be removed from the keychain")
}

fn encode_keypair(keypair: &DeviceKeypair) -> Vec<u8> {
    let mut payload = Vec::with_capacity(KEY_LEN * 2);
    payload.extend_from_slice(keypair.private_key());
    payload.extend_from_slice(keypair.public_key());
    payload
}

fn decode_keypair(payload: &[u8]) -> Option<DeviceKeypair> {
    let (private_key, public_key) = payload.split_first_chunk::<KEY_LEN>()?;
    let public_key: [u8; KEY_LEN] = public_key.try_into().ok()?;
    Some(DeviceKeypair::from_parts(
        Zeroizing::new(*private_key),
        public_key,
    ))
}

pub fn encode_public_key(public_key: &[u8; KEY_LEN]) -> String {
    URL_SAFE_NO_PAD.encode(public_key)
}

pub fn decode_public_key(encoded: &str) -> Option<[u8; KEY_LEN]> {
    URL_SAFE_NO_PAD.decode(encoded).ok()?.try_into().ok()
}

/// A name for this machine that a person recognises in a list of devices.
pub fn local_device_name() -> String {
    let name = sysinfo::System::host_name()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Zode".to_string());
    name.chars().take(MAX_DEVICE_NAME_CHARS).collect()
}

#[derive(Debug, thiserror::Error)]
pub enum DirectoryError {
    /// The account has revoked this device. Nothing it does will be accepted
    /// until someone signs in on it again.
    #[error("this device has been revoked")]
    Revoked,
    #[error("the account service answered {0}")]
    Rejected(u16),
    #[error("the account service could not be reached: {0}")]
    Unreachable(String),
}

#[derive(Serialize)]
struct RegisterBody<'a> {
    #[serde(rename = "publicKey")]
    public_key: String,
    name: &'a str,
    kind: &'static str,
}

/// Tells the account service this device's public key, so that a browser can
/// look it up and show it alongside this device's name.
pub async fn register_public_key(
    http_client: &Arc<dyn HttpClient>,
    api_url: &str,
    bearer: &str,
    name: &str,
    public_key: &[u8; KEY_LEN],
) -> Result<(), DirectoryError> {
    let body = serde_json::to_string(&RegisterBody {
        public_key: encode_public_key(public_key),
        name,
        kind: "ide",
    })
    .map_err(|error| DirectoryError::Unreachable(error.to_string()))?;
    let request = Request::builder()
        .method("PUT")
        .uri(format!("{api_url}/remote/devices/me/key"))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {bearer}"))
        .body(AsyncBody::from(body))
        .map_err(|error| DirectoryError::Unreachable(error.to_string()))?;
    let (status, response_body) = send(http_client, request).await?;
    match status {
        200..=299 => Ok(()),
        403 if response_body.contains("device_revoked") => Err(DirectoryError::Revoked),
        other => Err(DirectoryError::Rejected(other)),
    }
}

/// A device of this account as the account service lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryDevice {
    pub device_id: String,
    pub kind: String,
    pub name: String,
    pub public_key: Option<[u8; KEY_LEN]>,
}

#[derive(Deserialize)]
struct DeviceRow {
    #[serde(rename = "deviceId")]
    device_id: String,
    kind: String,
    name: String,
    #[serde(rename = "publicKey", default)]
    public_key: Option<String>,
}

pub async fn list_devices(
    http_client: &Arc<dyn HttpClient>,
    api_url: &str,
    bearer: &str,
) -> Result<Vec<DirectoryDevice>, DirectoryError> {
    let request = Request::builder()
        .uri(format!("{api_url}/remote/devices"))
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {bearer}"))
        .body(AsyncBody::default())
        .map_err(|error| DirectoryError::Unreachable(error.to_string()))?;
    let (status, body) = send(http_client, request).await?;
    if !(200..=299).contains(&status) {
        return Err(DirectoryError::Rejected(status));
    }
    let rows: Vec<DeviceRow> = serde_json::from_str(&body)
        .map_err(|_| DirectoryError::Unreachable("the device list could not be parsed".into()))?;
    Ok(rows
        .into_iter()
        .map(|row| DirectoryDevice {
            device_id: row.device_id,
            kind: row.kind,
            name: row.name,
            public_key: row.public_key.as_deref().and_then(decode_public_key),
        })
        .collect())
}

async fn send(
    http_client: &Arc<dyn HttpClient>,
    request: Request<AsyncBody>,
) -> Result<(u16, String), DirectoryError> {
    let mut response = http_client
        .send(request)
        .await
        .map_err(|_| DirectoryError::Unreachable("the request could not be sent".into()))?;
    let status = response.status().as_u16();
    let mut body = String::new();
    // Read only so an error slug can be recognised; it is never logged,
    // because an authenticated endpoint's error body can echo a credential.
    if let Err(error) = response.body_mut().read_to_string(&mut body).await
        && (200..=299).contains(&status)
    {
        return Err(DirectoryError::Unreachable(format!(
            "the response could not be read: {}",
            error.kind()
        )));
    }
    Ok((status, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::InMemoryKeychain;
    use gpui::TestAppContext;
    use http_client::{FakeHttpClient, Response};

    #[gpui::test]
    async fn a_key_is_made_once_and_then_read_back(cx: &mut TestAppContext) {
        let keychain = InMemoryKeychain::new();
        let provider: Arc<dyn CredentialsProvider> = keychain.clone();
        let async_cx = cx.to_async();

        let first = load_or_create_keypair(&provider, "user-1", &async_cx)
            .await
            .expect("a key");
        let second = load_or_create_keypair(&provider, "user-1", &async_cx)
            .await
            .expect("the same key");
        assert_eq!(first.public_key(), second.public_key());
        assert_eq!(first.private_key(), second.private_key());
        assert_eq!(
            keychain.write_count(),
            1,
            "a stored key must not be rewritten"
        );

        let other = load_or_create_keypair(&provider, "user-2", &async_cx)
            .await
            .expect("another account's key");
        assert_ne!(first.public_key(), other.public_key());
    }

    #[gpui::test]
    async fn an_unreadable_keychain_does_not_mint_a_replacement(cx: &mut TestAppContext) {
        let keychain = InMemoryKeychain::new();
        keychain.fail_reads();
        let provider: Arc<dyn CredentialsProvider> = keychain.clone();
        let result = load_or_create_keypair(&provider, "user-1", &cx.to_async()).await;
        assert!(result.is_err());
        assert_eq!(keychain.write_count(), 0);
    }

    #[gpui::test]
    async fn a_key_that_cannot_be_saved_is_an_error(cx: &mut TestAppContext) {
        let keychain = InMemoryKeychain::new();
        keychain.fail_writes();
        let provider: Arc<dyn CredentialsProvider> = keychain.clone();
        let result = load_or_create_keypair(&provider, "user-1", &cx.to_async()).await;
        assert!(result.is_err());
    }

    #[gpui::test]
    async fn a_corrupt_entry_is_replaced(cx: &mut TestAppContext) {
        let keychain = InMemoryKeychain::new();
        keychain.seed("zode://remote-device-key/user-1", "user-1", b"too short");
        let provider: Arc<dyn CredentialsProvider> = keychain.clone();
        let keypair = load_or_create_keypair(&provider, "user-1", &cx.to_async())
            .await
            .expect("a replacement key");
        let again = load_or_create_keypair(&provider, "user-1", &cx.to_async())
            .await
            .expect("and it is stable");
        assert_eq!(keypair.public_key(), again.public_key());
    }

    #[gpui::test]
    async fn deleting_removes_the_key(cx: &mut TestAppContext) {
        let keychain = InMemoryKeychain::new();
        let provider: Arc<dyn CredentialsProvider> = keychain.clone();
        let async_cx = cx.to_async();
        let first = load_or_create_keypair(&provider, "user-1", &async_cx)
            .await
            .unwrap();
        delete_keypair(&provider, "user-1", &async_cx)
            .await
            .unwrap();
        let second = load_or_create_keypair(&provider, "user-1", &async_cx)
            .await
            .unwrap();
        assert_ne!(first.public_key(), second.public_key());
    }

    #[test]
    fn public_keys_round_trip_as_unpadded_base64url() {
        let key = [0xfb; KEY_LEN];
        let encoded = encode_public_key(&key);
        assert!(!encoded.contains(['+', '/', '=']));
        assert_eq!(decode_public_key(&encoded), Some(key));
        assert_eq!(decode_public_key("short"), None);
    }

    #[gpui::test]
    async fn registration_sends_the_key_under_the_bearer(_cx: &mut TestAppContext) {
        let seen = Arc::new(parking_lot::Mutex::new(None));
        let client: Arc<dyn HttpClient> = FakeHttpClient::create({
            let seen = seen.clone();
            move |request| {
                let seen = seen.clone();
                async move {
                    let (parts, mut body) = request.into_parts();
                    let mut text = String::new();
                    body.read_to_string(&mut text).await.unwrap();
                    *seen.lock() = Some((
                        parts.method.to_string(),
                        parts.uri.to_string(),
                        parts
                            .headers
                            .get("authorization")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_owned),
                        text,
                    ));
                    Ok(Response::builder()
                        .status(200)
                        .body(AsyncBody::from("{}"))
                        .unwrap())
                }
            }
        });
        register_public_key(
            &client,
            "https://api.test/api",
            "tok",
            "My Mac",
            &[7; KEY_LEN],
        )
        .await
        .expect("registered");
        let (method, uri, authorization, body) = seen.lock().clone().expect("a request");
        assert_eq!(method, "PUT");
        assert_eq!(uri, "https://api.test/api/remote/devices/me/key");
        assert_eq!(authorization.as_deref(), Some("Bearer tok"));
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["kind"], "ide");
        assert_eq!(body["name"], "My Mac");
        assert_eq!(body["publicKey"], encode_public_key(&[7; KEY_LEN]));
    }

    #[gpui::test]
    async fn a_revoked_device_is_told_apart_from_a_failure(_cx: &mut TestAppContext) {
        let revoked: Arc<dyn HttpClient> = FakeHttpClient::create(|_| async {
            Ok(Response::builder()
                .status(403)
                .body(AsyncBody::from(r#"{"error":"device_revoked"}"#))
                .unwrap())
        });
        assert!(matches!(
            register_public_key(&revoked, "https://api.test/api", "t", "n", &[1; KEY_LEN]).await,
            Err(DirectoryError::Revoked)
        ));
        let broken: Arc<dyn HttpClient> = FakeHttpClient::create(|_| async {
            Ok(Response::builder()
                .status(500)
                .body(AsyncBody::from("boom"))
                .unwrap())
        });
        assert!(matches!(
            register_public_key(&broken, "https://api.test/api", "t", "n", &[1; KEY_LEN]).await,
            Err(DirectoryError::Rejected(500))
        ));
    }

    #[gpui::test]
    async fn the_device_list_is_read_with_names_and_keys(_cx: &mut TestAppContext) {
        let client: Arc<dyn HttpClient> = FakeHttpClient::create(|_| async {
            Ok(Response::builder()
                .status(200)
                .body(AsyncBody::from(format!(
                    r#"[{{"deviceId":"d1","kind":"web","name":"Chrome","publicKey":"{}","online":true,"rotatedAt":null}},
                        {{"deviceId":"d2","kind":"ide","name":"Mac","publicKey":"bad","online":false,"rotatedAt":null}}]"#,
                    encode_public_key(&[3; KEY_LEN])
                )))
                .unwrap())
        });
        let devices = list_devices(&client, "https://api.test/api", "t")
            .await
            .expect("a list");
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].name, "Chrome");
        assert_eq!(devices[0].public_key, Some([3; KEY_LEN]));
        assert_eq!(
            devices[1].public_key, None,
            "a key that does not decode is absent"
        );
    }

    #[test]
    fn a_device_name_is_never_empty_or_over_long() {
        let name = local_device_name();
        assert!(!name.is_empty());
        assert!(name.chars().count() <= MAX_DEVICE_NAME_CHARS);
    }
}
