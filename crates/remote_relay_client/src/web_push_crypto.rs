//! Encrypts a Web Push message for one browser: RFC 8291 key agreement over
//! the RFC 8188 `aes128gcm` content coding, as one record.
//!
//! Pure: no clock, no network, no GPUI. The push service that carries the
//! result cannot read it; only the browser holding the private half of
//! `p256dh` and the `auth` secret can.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    aead::{AES_128_GCM, Aad, LessSafeKey, Nonce, UnboundKey},
    agreement::{self, ECDH_P256, EphemeralPrivateKey, UnparsedPublicKey},
    hkdf::{HKDF_SHA256, KeyType, Prk, Salt},
    rand::{SecureRandom, SystemRandom},
};

/// An uncompressed P-256 point: `0x04 || x || y`.
pub const P256_PUBLIC_KEY_LEN: usize = 65;
pub const AUTH_SECRET_LEN: usize = 16;
const SALT_LEN: usize = 16;
const TAG_LEN: usize = 16;
/// The record size announced in the header. One record is all that is ever
/// sent, so it only has to be larger than the record.
const RECORD_SIZE: u32 = 4096;
const HEADER_LEN: usize = SALT_LEN + 4 + 1 + P256_PUBLIC_KEY_LEN;
/// What the push services accept as a whole body.
pub const MAX_PUSH_BODY_LEN: usize = 4096;
/// The longest plaintext that still fits [`MAX_PUSH_BODY_LEN`] once the
/// header, the padding delimiter and the tag are added.
pub const MAX_PLAINTEXT_LEN: usize = MAX_PUSH_BODY_LEN - HEADER_LEN - 1 - TAG_LEN;

/// The keys of one browser's push subscription, as `PushSubscription.getKey()`
/// gives them.
#[derive(Clone, PartialEq, Eq)]
pub struct PushSubscriptionKeys {
    pub p256dh: [u8; P256_PUBLIC_KEY_LEN],
    pub auth: [u8; AUTH_SECRET_LEN],
}

impl std::fmt::Debug for PushSubscriptionKeys {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PushSubscriptionKeys")
            .field("p256dh", &"<redacted>")
            .field("auth", &"<redacted>")
            .finish()
    }
}

impl PushSubscriptionKeys {
    /// Reads the two keys from base64url, padded or not. `None` unless the
    /// public key is an uncompressed P-256 point on the curve and the secret is
    /// 16 bytes.
    pub fn decode(p256dh: &str, auth: &str) -> Option<Self> {
        let p256dh = decode_base64url(p256dh)?;
        let auth = decode_base64url(auth)?;
        let p256dh: [u8; P256_PUBLIC_KEY_LEN] = p256dh.try_into().ok()?;
        let auth: [u8; AUTH_SECRET_LEN] = auth.try_into().ok()?;
        if p256dh[0] != 0x04 || !is_on_the_curve(&p256dh) {
            return None;
        }
        Some(Self { p256dh, auth })
    }

    pub fn encode(&self) -> (String, String) {
        (
            URL_SAFE_NO_PAD.encode(self.p256dh),
            URL_SAFE_NO_PAD.encode(self.auth),
        )
    }
}

/// Whether `point` is one ECDH can be done with, found by doing one with a
/// throwaway key: `ring` checks the point before it agrees on anything.
fn is_on_the_curve(point: &[u8; P256_PUBLIC_KEY_LEN]) -> bool {
    let Ok(throwaway) = EphemeralPrivateKey::generate(&ECDH_P256, &SystemRandom::new()) else {
        return false;
    };
    agreement::agree_ephemeral(
        throwaway,
        &UnparsedPublicKey::new(&ECDH_P256, point),
        |_| (),
    )
    .is_ok()
}

/// Keys whose public half is a real point, made from `seed` so that a test can
/// make the same ones twice.
#[cfg(any(test, feature = "test-support"))]
#[allow(deprecated)]
pub fn test_keys(seed: u8) -> PushSubscriptionKeys {
    let scalar = [seed.max(1); 32];
    let random = ring::test::rand::FixedSliceRandom { bytes: &scalar };
    let mut p256dh = [0; P256_PUBLIC_KEY_LEN];
    if let Ok(public) = EphemeralPrivateKey::generate(&ECDH_P256, &random)
        .and_then(|private| private.compute_public_key())
    {
        p256dh.copy_from_slice(public.as_ref());
    }
    PushSubscriptionKeys {
        p256dh,
        auth: [seed; AUTH_SECRET_LEN],
    }
}

fn decode_base64url(encoded: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(encoded.trim_end_matches('=')).ok()
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WebPushError {
    #[error("the message is too long to push")]
    TooLong,
    #[error("the browser's push key is not a usable P-256 key")]
    BadKey,
    #[error("the message could not be encrypted")]
    Crypto,
}

impl From<ring::error::Unspecified> for WebPushError {
    fn from(_: ring::error::Unspecified) -> Self {
        Self::Crypto
    }
}

/// Encrypts `plaintext` for the browser that holds `keys`, with a fresh key
/// pair and salt for this one message.
pub fn encrypt(plaintext: &[u8], keys: &PushSubscriptionKeys) -> Result<Vec<u8>, WebPushError> {
    let random = SystemRandom::new();
    let mut salt = [0; SALT_LEN];
    random.fill(&mut salt)?;
    encrypt_with(plaintext, keys, &random, salt)
}

/// [`encrypt`] with the ephemeral key drawn from `random` and the salt given,
/// so that a known answer can be checked.
fn encrypt_with(
    plaintext: &[u8],
    keys: &PushSubscriptionKeys,
    random: &dyn SecureRandom,
    salt: [u8; SALT_LEN],
) -> Result<Vec<u8>, WebPushError> {
    if plaintext.len() > MAX_PLAINTEXT_LEN {
        return Err(WebPushError::TooLong);
    }
    let sender_private = EphemeralPrivateKey::generate(&ECDH_P256, random)?;
    let sender_public = sender_private.compute_public_key()?;
    let receiver_public = UnparsedPublicKey::new(&ECDH_P256, keys.p256dh);
    let ecdh_secret: Vec<u8> =
        agreement::agree_ephemeral(sender_private, &receiver_public, |secret| secret.to_vec())
            .map_err(|_| WebPushError::BadKey)?;

    // RFC 8291 section 3.4: the auth secret salts the shared secret, bound to
    // both public keys, into the keying material RFC 8188 then salts again.
    let mut key_info = Vec::with_capacity(14 + 2 * P256_PUBLIC_KEY_LEN);
    key_info.extend_from_slice(b"WebPush: info\0");
    key_info.extend_from_slice(&keys.p256dh);
    key_info.extend_from_slice(sender_public.as_ref());
    let auth_prk = Salt::new(HKDF_SHA256, &keys.auth).extract(&ecdh_secret);
    let input_keying_material: [u8; 32] = expand(&auth_prk, &key_info)?;

    let prk = Salt::new(HKDF_SHA256, &salt).extract(&input_keying_material);
    let content_key: [u8; 16] = expand(&prk, b"Content-Encoding: aes128gcm\0")?;
    let nonce: [u8; 12] = expand(&prk, b"Content-Encoding: nonce\0")?;

    let mut body = Vec::with_capacity(HEADER_LEN + plaintext.len() + 1 + TAG_LEN);
    body.extend_from_slice(&salt);
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(P256_PUBLIC_KEY_LEN as u8);
    body.extend_from_slice(sender_public.as_ref());

    let mut record = Vec::with_capacity(plaintext.len() + 1 + TAG_LEN);
    record.extend_from_slice(plaintext);
    // The delimiter of the last (and only) record, with no padding after it.
    record.push(0x02);
    let key = LessSafeKey::new(UnboundKey::new(&AES_128_GCM, &content_key)?);
    // The nonce is fresh for every message: it is derived from a fresh salt
    // and a fresh key pair.
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::empty(),
        &mut record,
    )?;
    body.extend_from_slice(&record);
    Ok(body)
}

struct OutputLength(usize);

impl KeyType for OutputLength {
    fn len(&self) -> usize {
        self.0
    }
}

fn expand<const LEN: usize>(prk: &Prk, info: &[u8]) -> Result<[u8; LEN], WebPushError> {
    let mut output = [0; LEN];
    prk.expand(&[info], OutputLength(LEN))?.fill(&mut output)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(encoded: &str) -> Vec<u8> {
        decode_base64url(encoded).expect("base64url")
    }

    fn rfc_keys() -> PushSubscriptionKeys {
        PushSubscriptionKeys::decode(
            concat!(
                // Split so the spell checker does not read a word into the key.
                "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkN",
                "toIAiw4"
            ),
            "BTBZMqHH6r4Tts7J_aSIgg",
        )
        .expect("the RFC's keys decode")
    }

    /// RFC 8291, section 5 and appendix A.
    #[test]
    #[allow(deprecated)]
    fn the_rfc_8291_example_encrypts_byte_for_byte() {
        let sender_private = decode("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw");
        let random = ring::test::rand::FixedSliceRandom {
            bytes: &sender_private,
        };
        let salt: [u8; SALT_LEN] = decode("DGv6ra1nlYgDCS1FRnbzlw")
            .try_into()
            .expect("16 bytes");
        let body = encrypt_with(
            b"When I grow up, I want to be a watermelon",
            &rfc_keys(),
            &random,
            salt,
        )
        .expect("encrypted");
        assert_eq!(
            URL_SAFE_NO_PAD.encode(&body),
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27ml\
             mlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPT\
             pK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
        );
    }

    #[test]
    fn every_message_gets_its_own_key_and_salt() {
        let first = encrypt(b"same", &rfc_keys()).expect("encrypted");
        let second = encrypt(b"same", &rfc_keys()).expect("encrypted");
        assert_eq!(first.len(), HEADER_LEN + 4 + 1 + TAG_LEN);
        assert_ne!(first[..HEADER_LEN], second[..HEADER_LEN]);
    }

    #[test]
    fn a_message_too_long_for_one_push_is_refused() {
        let longest = vec![b'a'; MAX_PLAINTEXT_LEN];
        let body = encrypt(&longest, &rfc_keys()).expect("the longest fits");
        assert_eq!(body.len(), MAX_PUSH_BODY_LEN);
        let too_long = vec![b'a'; MAX_PLAINTEXT_LEN + 1];
        assert_eq!(encrypt(&too_long, &rfc_keys()), Err(WebPushError::TooLong));
    }

    #[test]
    fn keys_are_read_only_when_they_have_the_right_shape() {
        let keys = rfc_keys();
        let (p256dh, auth) = keys.encode();
        assert_eq!(PushSubscriptionKeys::decode(&p256dh, &auth), Some(keys));
        assert!(
            PushSubscriptionKeys::decode(&format!("{p256dh}="), &format!("{auth}==")).is_some(),
            "padding is tolerated"
        );
        assert_eq!(PushSubscriptionKeys::decode("BCVx", &auth), None);
        assert_eq!(PushSubscriptionKeys::decode(&p256dh, "BTBZ"), None);
        assert_eq!(PushSubscriptionKeys::decode("not base64!", &auth), None);
        let mut compressed = rfc_keys().p256dh;
        compressed[0] = 0x02;
        assert_eq!(
            PushSubscriptionKeys::decode(&URL_SAFE_NO_PAD.encode(compressed), &auth),
            None
        );
    }

    #[test]
    fn a_point_off_the_curve_is_not_read_as_a_key() {
        let mut off_curve = rfc_keys().p256dh;
        off_curve[1..].fill(0);
        let (_, auth) = rfc_keys().encode();
        assert_eq!(
            PushSubscriptionKeys::decode(&URL_SAFE_NO_PAD.encode(off_curve), &auth),
            None
        );
        let mut nudged = rfc_keys().p256dh;
        nudged[64] ^= 1;
        assert_eq!(
            PushSubscriptionKeys::decode(&URL_SAFE_NO_PAD.encode(nudged), &auth),
            None
        );
    }

    #[test]
    fn a_point_off_the_curve_is_refused() {
        let mut keys = rfc_keys();
        keys.p256dh[1..].fill(0);
        assert_eq!(encrypt(b"x", &keys), Err(WebPushError::BadKey));
    }

    #[test]
    fn keys_never_reach_a_log() {
        assert!(!format!("{:?}", rfc_keys()).contains("BCVx"));
        assert!(format!("{:?}", rfc_keys()).contains("redacted"));
    }
}
