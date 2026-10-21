//! This device's long-term key, and telling the account service about its
//! public half.
//!
//! The private key lives in the OS keychain and nowhere else: not in settings,
//! not in the database, not in a log. Losing it is recoverable — pair again —
//! while leaking it lets anyone who also controls the relay impersonate every
//! browser this device trusts.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

use anyhow::{Context as _, Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use credentials_provider::CredentialsProvider;
use futures::{AsyncReadExt as _, lock::Mutex as AsyncMutex};
use gpui::AsyncApp;
use http_client::{AsyncBody, HttpClient, Request, http::HeaderValue};
use parking_lot::Mutex;
use release_channel::{RELEASE_CHANNEL, ReleaseChannel};
use remote_relay_protocol::{DeviceKeypair, KEY_LEN};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// The account service refuses names longer than this.
const MAX_DEVICE_NAME_CHARS: usize = 64;

/// The account service is no more trusted than the relay, so no answer from it
/// is read past this size.
const MAX_RESPONSE_BYTES: u64 = 256 * 1024;

/// An account holds a few devices, so a list longer than this is not one.
const MAX_DIRECTORY_ROWS: usize = 256;

/// What a device is called when its name has nothing left once cleaned.
const UNNAMED_DEVICE: &str = "A device";

fn keychain_url(user_id: &str, channel: ReleaseChannel) -> String {
    // One entry per account: a key made for one account must not answer to
    // another's pins, and signing out of one account must not delete the
    // other's key.
    //
    // One per release channel as well, because each channel keeps its own
    // pins: a development build that revoked its device would otherwise
    // delete the key of the installed app. Stable keeps the address it has
    // always had, so installed copies keep their key.
    match channel {
        ReleaseChannel::Stable => format!("zode://remote-device-key/{user_id}"),
        other => format!("zode://remote-device-key-{}/{user_id}", other.dev_name()),
    }
}

/// One lock per keychain entry in this process, so that two callers who both
/// find it empty cannot each make a key and leave one of them with a key that
/// is not the stored one. Keyed by the keychain as well as the entry, since
/// two keychains share no entries.
static KEY_LOCKS: LazyLock<Mutex<HashMap<(usize, String), Arc<AsyncMutex<()>>>>> =
    LazyLock::new(Mutex::default);

fn key_lock(provider: &Arc<dyn CredentialsProvider>, url: &str) -> Arc<AsyncMutex<()>> {
    let mut locks = KEY_LOCKS.lock();
    // A lock only the table holds is not in use.
    locks.retain(|_, lock| Arc::strong_count(lock) > 1);
    let keychain = Arc::as_ptr(provider).cast::<()>() as usize;
    locks
        .entry((keychain, url.to_string()))
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

/// Reads this device's key for `user_id`, making and storing one first if the
/// keychain has none.
///
/// An unreadable keychain is an error and not "no key": minting a fresh key
/// every time the keychain hiccups would orphan every pin in silence. The same
/// goes for a key that was made but could not be saved, which would be
/// forgotten at exit, and for an entry this version does not recognise, which
/// a newer build may have written and must not be overwritten. Only an entry
/// that is proven damaged — its public half is not the public key of its
/// private half — is replaced.
pub async fn load_or_create_keypair(
    provider: &Arc<dyn CredentialsProvider>,
    user_id: &str,
    cx: &AsyncApp,
) -> Result<Arc<DeviceKeypair>> {
    let url = keychain_url(user_id, *RELEASE_CHANNEL);
    let lock = key_lock(provider, &url);
    let _held = lock.lock().await;
    load_or_create_at(provider, &url, user_id, cx).await
}

async fn load_or_create_at(
    provider: &Arc<dyn CredentialsProvider>,
    url: &str,
    user_id: &str,
    cx: &AsyncApp,
) -> Result<Arc<DeviceKeypair>> {
    let stored = provider
        .read_credentials(url, cx)
        .await
        .context("the keychain could not be read")?;

    if let Some((_, payload)) = stored {
        // The keychain hands back a plain Vec holding the private key.
        let payload = Zeroizing::new(payload);
        match decode_keypair(&payload).context("the stored device key could not be checked")? {
            StoredKey::Valid(keypair) => return Ok(Arc::new(keypair)),
            StoredKey::UnknownFormat(length) => bail!(
                "the stored device key is {length} bytes, not {}, so it was left as it is; \
                 remove it from the keychain to make a new one",
                KEY_LEN * 2
            ),
            StoredKey::Mismatched => log::warn!(
                "the stored device key does not match its own public key; making a new one, \
                 which means paired devices must pair again"
            ),
        }
    }

    let keypair = DeviceKeypair::generate().context("a device key could not be generated")?;
    let payload = Zeroizing::new(encode_keypair(&keypair));
    provider
        .write_credentials(url, user_id, &payload, cx)
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
    let url = keychain_url(user_id, *RELEASE_CHANNEL);
    let lock = key_lock(provider, &url);
    let _held = lock.lock().await;
    provider
        .delete_credentials(&url, cx)
        .await
        .context("the device key could not be removed from the keychain")
}

fn encode_keypair(keypair: &DeviceKeypair) -> Vec<u8> {
    let mut payload = Vec::with_capacity(KEY_LEN * 2);
    payload.extend_from_slice(keypair.private_key());
    payload.extend_from_slice(keypair.public_key());
    payload
}

enum StoredKey {
    Valid(DeviceKeypair),
    /// Not the size of any key this version writes.
    UnknownFormat(usize),
    /// The right size, but the public half is not the private half's.
    Mismatched,
}

fn decode_keypair(payload: &[u8]) -> Result<StoredKey, remote_relay_protocol::NoiseError> {
    let Some((private_key, public_key)) = payload.split_first_chunk::<KEY_LEN>() else {
        return Ok(StoredKey::UnknownFormat(payload.len()));
    };
    let Ok(public_key) = <[u8; KEY_LEN]>::try_from(public_key) else {
        return Ok(StoredKey::UnknownFormat(payload.len()));
    };
    let keypair = DeviceKeypair::from_private_key(Zeroizing::new(*private_key))?;
    if keypair.public_key() == &public_key {
        Ok(StoredKey::Valid(keypair))
    } else {
        Ok(StoredKey::Mismatched)
    }
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

/// A device's name as it is safe to show: a name is chosen by the device
/// itself, or by whoever runs the account service, so it is not trusted to be
/// short or to be only text. See [`sanitize_display_text`]; the limit is the
/// longest name the account service accepts. Empty when nothing visible is
/// left.
pub fn sanitize_device_name(name: &str) -> String {
    sanitize_display_text(name, MAX_DEVICE_NAME_CHARS)
}

/// Text that someone else chose, made safe to put on screen: control
/// characters, line and paragraph separators, characters that reorder text,
/// and characters that draw nothing at all are dropped, so the text cannot
/// break out of its line, make the words around it read differently, or
/// pass for empty or for another text. What is left is trimmed and cut to at
/// most `max_chars` characters, the last of them `…` when anything was cut.
/// Empty when nothing visible is left.
pub fn sanitize_display_text(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let cleaned: String = text
        .chars()
        .filter(|character| !character.is_control() && !is_invisible_or_reordering(*character))
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.chars().count() <= max_chars {
        return cleaned.to_string();
    }
    cleaned
        .chars()
        .take(max_chars - 1)
        .chain(std::iter::once('\u{2026}'))
        .collect()
}

/// The line and paragraph separators and every character of the Unicode
/// "format" category (Cf), which includes the soft hyphen, the zero-width
/// characters, the joiners, the bidirectional controls and the tag characters.
/// Written out because the standard library has no general-category lookup.
fn is_invisible_or_reordering(character: char) -> bool {
    matches!(
        character,
        '\u{00AD}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
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

/// The bearer as a header value that stays out of any dump of the request.
fn bearer_header(bearer: &str) -> Result<HeaderValue, DirectoryError> {
    let mut value = HeaderValue::from_str(&format!("Bearer {bearer}"))
        .map_err(|_| DirectoryError::Unreachable("the credential cannot be sent".into()))?;
    value.set_sensitive(true);
    Ok(value)
}

/// Whether a 403 body says the device was revoked, as opposed to merely
/// mentioning it: the body is the account service's to write, and a revoked
/// device deletes everything it has paired.
fn says_device_revoked(body: &[u8]) -> bool {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: String,
    }
    serde_json::from_slice::<ErrorBody>(body).is_ok_and(|body| body.error == "device_revoked")
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
        .header("Authorization", bearer_header(bearer)?)
        .body(AsyncBody::from(body))
        .map_err(|error| DirectoryError::Unreachable(error.to_string()))?;
    let (status, response_body) = send(http_client, request).await?;
    match status {
        200..=299 => Ok(()),
        403 if says_device_revoked(&response_body) => Err(DirectoryError::Revoked),
        other => Err(DirectoryError::Rejected(other)),
    }
}

/// A device of this account as the account service lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryDevice {
    pub device_id: String,
    pub kind: String,
    /// Already passed through [`sanitize_device_name`], and never empty.
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
        .header("Authorization", bearer_header(bearer)?)
        .body(AsyncBody::default())
        .map_err(|error| DirectoryError::Unreachable(error.to_string()))?;
    let (status, body) = send(http_client, request).await?;
    if !(200..=299).contains(&status) {
        return Err(DirectoryError::Rejected(status));
    }
    let rows: Vec<DeviceRow> = serde_json::from_slice(&body)
        .map_err(|_| DirectoryError::Unreachable("the device list could not be parsed".into()))?;
    Ok(rows
        .into_iter()
        .take(MAX_DIRECTORY_ROWS)
        .map(|row| {
            let name = sanitize_device_name(&row.name);
            DirectoryDevice {
                device_id: row.device_id,
                kind: row.kind,
                name: if name.is_empty() {
                    UNNAMED_DEVICE.to_string()
                } else {
                    name
                },
                public_key: row.public_key.as_deref().and_then(decode_public_key),
            }
        })
        .collect())
}

/// Sends the request and returns the status with at most
/// [`MAX_RESPONSE_BYTES`] of body.
///
/// The body is read only so an error slug can be recognised or a list parsed;
/// it is never logged, because an authenticated endpoint's error body can
/// echo a credential. A body that is too long or cannot be read is an error
/// for a success status, and an empty body for any other, where the status is
/// all that is acted on.
async fn send(
    http_client: &Arc<dyn HttpClient>,
    request: Request<AsyncBody>,
) -> Result<(u16, Vec<u8>), DirectoryError> {
    let mut response = http_client
        .send(request)
        .await
        .map_err(|_| DirectoryError::Unreachable("the request could not be sent".into()))?;
    let status = response.status().as_u16();
    let succeeded = (200..=299).contains(&status);
    let mut body = Vec::new();
    // One byte past the limit, so a body of exactly the limit is not mistaken
    // for a longer one.
    let read = response
        .body_mut()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut body)
        .await;
    let failure = match read {
        Ok(_) if body.len() as u64 > MAX_RESPONSE_BYTES => {
            Some("the response was too large".to_string())
        }
        Ok(_) => None,
        Err(error) => Some(format!("the response could not be read: {}", error.kind())),
    };
    match failure {
        None => Ok((status, body)),
        Some(failure) if succeeded => Err(DirectoryError::Unreachable(failure)),
        Some(_) => Ok((status, Vec::new())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::InMemoryKeychain;
    use gpui::TestAppContext;
    use http_client::{FakeHttpClient, Response};
    use std::{
        future::Future,
        pin::Pin,
        sync::atomic::{AtomicUsize, Ordering},
    };

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

    fn entry_url(user_id: &str) -> String {
        keychain_url(user_id, *RELEASE_CHANNEL)
    }

    #[gpui::test]
    async fn an_entry_of_an_unknown_length_is_refused_and_left_alone(cx: &mut TestAppContext) {
        for payload in [&b""[..], b"too short", &[7u8; KEY_LEN * 2 + 1]] {
            let keychain = InMemoryKeychain::new();
            keychain.seed(&entry_url("user-1"), "user-1", payload);
            let provider: Arc<dyn CredentialsProvider> = keychain.clone();

            let result = load_or_create_keypair(&provider, "user-1", &cx.to_async()).await;

            assert!(result.is_err(), "{} bytes", payload.len());
            assert_eq!(keychain.write_count(), 0, "{} bytes", payload.len());
            assert_eq!(keychain.stored_passwords(), vec![payload.to_vec()]);
        }
    }

    #[gpui::test]
    async fn an_entry_whose_public_half_does_not_match_its_private_half_is_replaced(
        cx: &mut TestAppContext,
    ) {
        let keychain = InMemoryKeychain::new();
        let mut damaged = vec![1u8; KEY_LEN];
        damaged.extend_from_slice(&[2u8; KEY_LEN]);
        keychain.seed(&entry_url("user-1"), "user-1", &damaged);
        let provider: Arc<dyn CredentialsProvider> = keychain.clone();

        let keypair = load_or_create_keypair(&provider, "user-1", &cx.to_async())
            .await
            .expect("a replacement key");
        assert_ne!(keypair.public_key(), &[2u8; KEY_LEN]);
        let again = load_or_create_keypair(&provider, "user-1", &cx.to_async())
            .await
            .expect("and it is stable");
        assert_eq!(keypair.public_key(), again.public_key());
        assert_eq!(keychain.write_count(), 1);
    }

    #[gpui::test]
    async fn a_stored_pair_that_agrees_with_itself_is_used_as_it_is(cx: &mut TestAppContext) {
        let keychain = InMemoryKeychain::new();
        let original = DeviceKeypair::generate().unwrap();
        keychain.seed(&entry_url("user-1"), "user-1", &encode_keypair(&original));
        let provider: Arc<dyn CredentialsProvider> = keychain.clone();

        let loaded = load_or_create_keypair(&provider, "user-1", &cx.to_async())
            .await
            .expect("the stored key");
        assert_eq!(loaded.private_key(), original.private_key());
        assert_eq!(loaded.public_key(), original.public_key());
        assert_eq!(keychain.write_count(), 0);
    }

    /// A keychain that takes a moment to answer, as one does while it asks
    /// for a password.
    struct SlowKeychain(Arc<InMemoryKeychain>);

    impl CredentialsProvider for SlowKeychain {
        fn read_credentials<'a>(
            &'a self,
            url: &'a str,
            cx: &'a AsyncApp,
        ) -> Pin<Box<dyn Future<Output = Result<Option<(String, Vec<u8>)>>> + 'a>> {
            Box::pin(async move {
                let stored = self.0.read_credentials(url, cx).await;
                smol::future::yield_now().await;
                stored
            })
        }

        fn write_credentials<'a>(
            &'a self,
            url: &'a str,
            username: &'a str,
            password: &'a [u8],
            cx: &'a AsyncApp,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
            Box::pin(async move {
                smol::future::yield_now().await;
                self.0.write_credentials(url, username, password, cx).await
            })
        }

        fn delete_credentials<'a>(
            &'a self,
            url: &'a str,
            cx: &'a AsyncApp,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
            self.0.delete_credentials(url, cx)
        }
    }

    #[gpui::test]
    async fn two_callers_that_both_find_no_key_end_up_with_the_same_one(cx: &mut TestAppContext) {
        let keychain = InMemoryKeychain::new();
        let provider: Arc<dyn CredentialsProvider> = Arc::new(SlowKeychain(keychain.clone()));
        let async_cx = cx.to_async();

        let (first, second) = futures::future::join(
            load_or_create_keypair(&provider, "user-1", &async_cx),
            load_or_create_keypair(&provider, "user-1", &async_cx),
        )
        .await;
        let (first, second) = (first.expect("a key"), second.expect("a key"));

        assert_eq!(first.public_key(), second.public_key());
        assert_eq!(keychain.write_count(), 1, "the key was made twice");
        let stored = load_or_create_keypair(&provider, "user-1", &async_cx)
            .await
            .expect("the stored key");
        assert_eq!(stored.public_key(), first.public_key());
    }

    #[test]
    fn stable_keeps_the_address_installed_copies_already_use() {
        assert_eq!(
            keychain_url("user-1", ReleaseChannel::Stable),
            "zode://remote-device-key/user-1"
        );
    }

    #[test]
    fn no_two_channels_share_a_keychain_entry() {
        let urls: std::collections::HashSet<String> = [
            ReleaseChannel::Stable,
            ReleaseChannel::Preview,
            ReleaseChannel::Nightly,
            ReleaseChannel::Dev,
        ]
        .into_iter()
        .map(|channel| keychain_url("user-1", channel))
        .collect();
        assert_eq!(urls.len(), 4);
    }

    #[gpui::test]
    async fn removing_one_channels_key_leaves_the_other_channels_alone(cx: &mut TestAppContext) {
        let keychain = InMemoryKeychain::new();
        let provider: Arc<dyn CredentialsProvider> = keychain.clone();
        let async_cx = cx.to_async();
        let stable = keychain_url("user-1", ReleaseChannel::Stable);
        let dev = keychain_url("user-1", ReleaseChannel::Dev);

        let installed = load_or_create_at(&provider, &stable, "user-1", &async_cx)
            .await
            .unwrap();
        load_or_create_at(&provider, &dev, "user-1", &async_cx)
            .await
            .unwrap();
        provider.delete_credentials(&dev, &async_cx).await.unwrap();

        let still_there = load_or_create_at(&provider, &stable, "user-1", &async_cx)
            .await
            .unwrap();
        assert_eq!(still_there.public_key(), installed.public_key());
        assert_eq!(
            keychain.write_count(),
            2,
            "the installed key was not remade"
        );
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

    fn answering(
        status: u16,
        body: impl Fn() -> AsyncBody + Send + Sync + 'static,
    ) -> Arc<dyn HttpClient> {
        FakeHttpClient::create(move |_| {
            let response = Response::builder().status(status).body(body()).unwrap();
            async move { Ok(response) }
        })
    }

    #[gpui::test]
    async fn only_a_body_whose_error_is_device_revoked_means_revoked(_cx: &mut TestAppContext) {
        for body in [
            r#"{"error":"device_revoked"}"#,
            r#"{"error":"device_revoked","message":"signed out"}"#,
        ] {
            let client = answering(403, move || AsyncBody::from(body));
            assert!(
                matches!(
                    register_public_key(&client, "https://api.test/api", "t", "n", &[1; KEY_LEN])
                        .await,
                    Err(DirectoryError::Revoked)
                ),
                "{body}"
            );
        }
        for body in [
            "device_revoked",
            "<html>device_revoked</html>",
            r#"{"error":"Forbidden","message":"device_revoked"}"#,
            r#"{"error":["device_revoked"]}"#,
            r#"{"error":"device_revoked_soon"}"#,
            r#"{"message":"device_revoked"}"#,
            "",
        ] {
            let client = answering(403, move || AsyncBody::from(body));
            assert!(
                matches!(
                    register_public_key(&client, "https://api.test/api", "t", "n", &[1; KEY_LEN])
                        .await,
                    Err(DirectoryError::Rejected(403))
                ),
                "{body}"
            );
        }
        let client = answering(500, || AsyncBody::from(r#"{"error":"device_revoked"}"#));
        assert!(matches!(
            register_public_key(&client, "https://api.test/api", "t", "n", &[1; KEY_LEN]).await,
            Err(DirectoryError::Rejected(500))
        ));
    }

    #[gpui::test]
    async fn the_bearer_is_marked_sensitive_on_every_directory_call(_cx: &mut TestAppContext) {
        let sensitive = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let client: Arc<dyn HttpClient> = FakeHttpClient::create({
            let sensitive = sensitive.clone();
            move |request| {
                sensitive.lock().push(
                    request
                        .headers()
                        .get("authorization")
                        .map(HeaderValue::is_sensitive),
                );
                async {
                    Ok(Response::builder()
                        .status(200)
                        .body(AsyncBody::from("[]"))
                        .unwrap())
                }
            }
        });
        register_public_key(&client, "https://api.test/api", "tok", "n", &[1; KEY_LEN])
            .await
            .unwrap();
        list_devices(&client, "https://api.test/api", "tok")
            .await
            .unwrap();
        assert_eq!(*sensitive.lock(), [Some(true), Some(true)]);
    }

    /// A body of `length` bytes that counts how many were asked for.
    struct CountedBody {
        remaining: usize,
        served: Arc<AtomicUsize>,
    }

    impl futures::AsyncRead for CountedBody {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buffer: &mut [u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            let length = buffer.len().min(self.remaining);
            buffer[..length].fill(b' ');
            self.remaining -= length;
            self.served.fetch_add(length, Ordering::SeqCst);
            std::task::Poll::Ready(Ok(length))
        }
    }

    #[gpui::test]
    async fn a_response_is_not_read_past_its_limit(_cx: &mut TestAppContext) {
        let served = Arc::new(AtomicUsize::new(0));
        let client = answering(200, {
            let served = served.clone();
            move || {
                AsyncBody::from_reader(CountedBody {
                    remaining: 64 * 1024 * 1024,
                    served: served.clone(),
                })
            }
        });

        let result = list_devices(&client, "https://api.test/api", "t").await;

        assert!(matches!(result, Err(DirectoryError::Unreachable(_))));
        assert!(
            served.load(Ordering::SeqCst) as u64 <= MAX_RESPONSE_BYTES + 1,
            "{} bytes were read",
            served.load(Ordering::SeqCst)
        );
    }

    #[gpui::test]
    async fn the_limit_itself_is_allowed_and_one_byte_more_is_not(_cx: &mut TestAppContext) {
        let padded = |length: usize| {
            let mut body = String::from("[");
            body.push_str(&" ".repeat(length - 2));
            body.push(']');
            body
        };
        let at_the_limit = padded(MAX_RESPONSE_BYTES as usize);
        let client = answering(200, move || AsyncBody::from(at_the_limit.clone()));
        assert_eq!(
            list_devices(&client, "https://api.test/api", "t")
                .await
                .expect("the limit is allowed"),
            vec![]
        );

        let over = padded(MAX_RESPONSE_BYTES as usize + 1);
        let client = answering(200, move || AsyncBody::from(over.clone()));
        assert!(matches!(
            list_devices(&client, "https://api.test/api", "t").await,
            Err(DirectoryError::Unreachable(_))
        ));
    }

    #[gpui::test]
    async fn an_oversized_error_body_is_just_the_error(_cx: &mut TestAppContext) {
        let client = answering(403, || {
            AsyncBody::from(format!(
                r#"{{"error":"device_revoked","padding":"{}"}}"#,
                "x".repeat(MAX_RESPONSE_BYTES as usize)
            ))
        });
        assert!(
            matches!(
                register_public_key(&client, "https://api.test/api", "t", "n", &[1; KEY_LEN]).await,
                Err(DirectoryError::Rejected(403))
            ),
            "a body that cannot be read in full is not taken as a revocation"
        );
    }

    #[gpui::test]
    async fn a_list_longer_than_any_account_has_is_cut_short(_cx: &mut TestAppContext) {
        let rows: Vec<String> = (0..MAX_DIRECTORY_ROWS + 50)
            .map(|index| format!(r#"{{"deviceId":"d{index}","kind":"web","name":"n{index}"}}"#))
            .collect();
        let body = format!("[{}]", rows.join(","));
        let client = answering(200, move || AsyncBody::from(body.clone()));

        let devices = list_devices(&client, "https://api.test/api", "t")
            .await
            .expect("a list");

        assert_eq!(devices.len(), MAX_DIRECTORY_ROWS);
        assert_eq!(devices[0].device_id, "d0");
    }

    #[gpui::test]
    async fn names_from_the_directory_arrive_clean_and_short(_cx: &mut TestAppContext) {
        let long_name = "m".repeat(500);
        let body = serde_json::json!([
            {"deviceId": "d1", "kind": "web", "name": "Chrome\u{202E}\ngnp.exe\u{0}"},
            {"deviceId": "d2", "kind": "web", "name": long_name},
            {"deviceId": "d3", "kind": "web", "name": "\u{202E}\n\t "},
            {"deviceId": "d4", "kind": "web", "name": "\u{200B}\u{FEFF}"},
        ])
        .to_string();
        let client = answering(200, move || AsyncBody::from(body.clone()));

        let devices = list_devices(&client, "https://api.test/api", "t")
            .await
            .expect("a list");

        assert_eq!(devices[0].name, "Chromegnp.exe");
        assert_eq!(devices[1].name.chars().count(), MAX_DEVICE_NAME_CHARS);
        assert_eq!(devices[2].name, UNNAMED_DEVICE, "nothing left to show");
        assert_eq!(devices[3].name, UNNAMED_DEVICE, "nothing visible left");
    }

    #[test]
    fn a_device_name_loses_what_could_break_out_of_its_line() {
        assert_eq!(
            sanitize_device_name("Giáp's MacBook Pro"),
            "Giáp's MacBook Pro"
        );
        assert_eq!(sanitize_device_name("Máy tính 🖥️"), "Máy tính 🖥️");
        assert_eq!(sanitize_device_name("  padded  "), "padded");
        assert_eq!(
            sanitize_device_name("a\nb\rc\td\u{0}e\u{7f}f\u{85}g"),
            "abcdefg"
        );
        assert_eq!(sanitize_device_name("a\u{2028}b\u{2029}c"), "abc");
        for reorder in [
            '\u{061C}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}',
            '\u{202E}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
        ] {
            assert_eq!(
                sanitize_device_name(&format!("a{reorder}b")),
                "ab",
                "U+{:04X}",
                reorder as u32
            );
        }
    }

    #[test]
    fn characters_that_draw_nothing_are_dropped_and_a_name_of_only_them_is_empty() {
        for invisible in [
            '\u{00AD}',
            '\u{200B}',
            '\u{200C}',
            '\u{200D}',
            '\u{2060}',
            '\u{2061}',
            '\u{2062}',
            '\u{2063}',
            '\u{2064}',
            '\u{FEFF}',
            '\u{180E}',
            '\u{206A}',
            '\u{FFF9}',
            '\u{110BD}',
            '\u{1D173}',
            '\u{E0001}',
            '\u{E0041}',
        ] {
            let code = invisible as u32;
            assert_eq!(
                sanitize_device_name(&format!("a{invisible}b")),
                "ab",
                "U+{code:04X}"
            );
            assert_eq!(
                sanitize_device_name(&format!("{invisible}{invisible} {invisible}")),
                "",
                "U+{code:04X}"
            );
        }
        assert_eq!(sanitize_device_name("\u{200B}\u{FEFF}\u{2060}"), "");
    }

    #[test]
    fn a_device_name_is_cut_by_characters_after_it_is_cleaned() {
        let name = format!("{}{}", "\n".repeat(100), "é".repeat(100));
        let cleaned = sanitize_device_name(&name);
        assert_eq!(cleaned.chars().count(), MAX_DEVICE_NAME_CHARS);
        assert!(cleaned.ends_with('…'));
        assert!(cleaned.chars().take(63).all(|character| character == 'é'));
        assert_eq!(sanitize_device_name(""), "");

        let exactly = "é".repeat(MAX_DEVICE_NAME_CHARS);
        assert_eq!(sanitize_device_name(&exactly), exactly, "nothing was cut");
    }

    #[test]
    fn display_text_is_cleaned_then_cut_to_the_length_asked_for() {
        assert_eq!(sanitize_display_text("short", 40), "short");
        assert_eq!(sanitize_display_text("abcdef", 6), "abcdef");
        assert_eq!(sanitize_display_text("abcdefg", 6), "abcde…");
        assert_eq!(sanitize_display_text("ab\u{200B}c\u{202E}d\ne", 5), "abcde");
        assert_eq!(
            sanitize_display_text("ab\u{200B}c\u{202E}d\nef", 5),
            "abcd…",
            "the characters that were dropped do not count toward the length"
        );
        assert_eq!(sanitize_display_text("ééééé", 3), "éé…");
        assert_eq!(sanitize_display_text("abc", 1), "…");
        assert_eq!(sanitize_display_text("a", 1), "a");
        assert_eq!(sanitize_display_text("abc", 0), "");
        assert_eq!(sanitize_display_text("  \u{200B}  ", 10), "");
    }

    #[test]
    fn a_device_name_is_never_empty_or_over_long() {
        let name = local_device_name();
        assert!(!name.is_empty());
        assert!(name.chars().count() <= MAX_DEVICE_NAME_CHARS);
    }
}
