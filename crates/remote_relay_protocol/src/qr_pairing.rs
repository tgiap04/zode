//! The out-of-band half of pairing by QR code.
//!
//! The six digits exist so two people can check that nobody sits between two
//! screens. A QR code shown on this screen and read by the phone's camera is a
//! channel nobody can sit in: it tells the phone which key is ours, and tells us
//! the phone saw the screen, because only a device that read the code knows the
//! secret in it. The phone proves that in `pair_reveal`, and the digits are
//! skipped.
//!
//! The proof binds both public keys, both nonces and the phone's device id, so
//! it cannot be replayed into another exchange or carried to another device's
//! name, and the secret is spent on the first valid proof.

use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    hmac,
    rand::{SecureRandom as _, SystemRandom},
};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

use crate::{KEY_LEN, NONCE_LEN, PairingError};

pub const QR_SECRET_LEN: usize = 32;
pub const QR_PROOF_LEN: usize = 32;

/// How long a code may be scanned for after it is shown.
pub const QR_SESSION_TTL: Duration = Duration::from_secs(120);

const QR_PROOF_LABEL: &[u8] = b"zode-qr-pair-v1";
const QR_URI_VERSION: u32 = 1;
const QR_CONFIRM_LABEL: &[u8] = b"zode-qr-confirm-v1";
/// Crockford base32: no I, L, O or U, so a code read aloud or off a screen is
/// not confused.
const CONFIRM_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CONFIRM_CODE_LEN: usize = 4;

/// What the QR code carries that only the person at this screen can have seen.
/// Wiped when dropped and never printed.
pub struct QrSecret(Zeroizing<[u8; QR_SECRET_LEN]>);

impl QrSecret {
    pub fn random() -> Result<Self, PairingError> {
        let mut bytes = Zeroizing::new([0u8; QR_SECRET_LEN]);
        SystemRandom::new()
            .fill(bytes.as_mut_slice())
            .map_err(|_| PairingError::NoRandomness)?;
        Ok(Self(bytes))
    }

    pub fn from_bytes(bytes: [u8; QR_SECRET_LEN]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    fn mac_key(&self) -> hmac::Key {
        hmac::Key::new(hmac::HMAC_SHA256, self.0.as_slice())
    }
}

impl std::fmt::Debug for QrSecret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("QrSecret(<redacted>)")
    }
}

fn proof_message(
    requester_public_key: &[u8; KEY_LEN],
    host_public_key: &[u8; KEY_LEN],
    requester_nonce: &[u8; NONCE_LEN],
    host_nonce: &[u8; NONCE_LEN],
    requester_device_id: &str,
) -> Option<Vec<u8>> {
    let device_id = requester_device_id.as_bytes();
    let device_id_len = u16::try_from(device_id.len()).ok()?;
    let mut message = Vec::with_capacity(
        QR_PROOF_LABEL.len() + 2 * KEY_LEN + 2 * NONCE_LEN + 2 + device_id.len(),
    );
    message.extend_from_slice(QR_PROOF_LABEL);
    message.extend_from_slice(requester_public_key);
    message.extend_from_slice(host_public_key);
    message.extend_from_slice(requester_nonce);
    message.extend_from_slice(host_nonce);
    message.extend_from_slice(&device_id_len.to_be_bytes());
    message.extend_from_slice(device_id);
    Some(message)
}

/// `HMAC-SHA256(secret, "zode-qr-pair-v1" ‖ requester key ‖ host key ‖
/// requester nonce ‖ host nonce ‖ u16be(len) ‖ requester device id)`. The phone
/// computes this; it is here so the phone's answer can be held against the same
/// bytes. The device id is the one the relay reports for the phone: without it
/// in the proof, a relay could hand the phone's proven key to the host under
/// another device's name.
///
/// `None` when the device id is too long for its two-byte length.
pub fn qr_proof(
    secret: &QrSecret,
    requester_public_key: &[u8; KEY_LEN],
    host_public_key: &[u8; KEY_LEN],
    requester_nonce: &[u8; NONCE_LEN],
    host_nonce: &[u8; NONCE_LEN],
    requester_device_id: &str,
) -> Option<[u8; QR_PROOF_LEN]> {
    let message = proof_message(
        requester_public_key,
        host_public_key,
        requester_nonce,
        host_nonce,
        requester_device_id,
    )?;
    let tag = hmac::sign(&secret.mac_key(), &message);
    let mut proof = [0u8; QR_PROOF_LEN];
    for (slot, byte) in proof.iter_mut().zip(tag.as_ref()) {
        *slot = *byte;
    }
    Some(proof)
}

/// Whether `proof` is the one `secret` makes for this exchange and this
/// requester device. Compared in constant time: the proof is a secret-keyed
/// value a guesser could otherwise learn a byte at a time.
pub fn verify_qr_proof(
    secret: &QrSecret,
    requester_public_key: &[u8; KEY_LEN],
    host_public_key: &[u8; KEY_LEN],
    requester_nonce: &[u8; NONCE_LEN],
    host_nonce: &[u8; NONCE_LEN],
    requester_device_id: &str,
    proof: &[u8; QR_PROOF_LEN],
) -> bool {
    let Some(message) = proof_message(
        requester_public_key,
        host_public_key,
        requester_nonce,
        host_nonce,
        requester_device_id,
    ) else {
        return false;
    };
    hmac::verify(&secret.mac_key(), &message, proof).is_ok()
}

/// `SHA-256` of our static public key, so the phone can check the QR names the
/// key the account directory lists for us without the code carrying the key.
pub fn host_key_fingerprint(host_public_key: &[u8; KEY_LEN]) -> [u8; 32] {
    Sha256::digest(host_public_key).into()
}

/// The text the QR code holds: `zode://pair?v=1&h=…&k=…&s=…&e=…`. A phone's
/// camera opens it in the Zode app through the `zode` scheme.
///
/// `expires_at_unix` is whole seconds since the epoch. Only `h`, a device id
/// the account service chose, needs percent-encoding; the rest is base64url or
/// digits.
pub fn qr_pairing_uri(
    host_device_id: &str,
    host_public_key: &[u8; KEY_LEN],
    secret: &QrSecret,
    expires_at_unix: u64,
) -> String {
    format!(
        "zode://pair?v={QR_URI_VERSION}&h={}&k={}&s={}&e={expires_at_unix}",
        percent_encode(host_device_id),
        URL_SAFE_NO_PAD.encode(host_key_fingerprint(host_public_key)),
        URL_SAFE_NO_PAD.encode(secret.0.as_slice()),
    )
}

/// Four characters that show on the computer next to the QR code, made from
/// the code's secret and our key's fingerprint. A phone that opened the link
/// without scanning (a link anybody can send) shows its own copy for the person
/// to compare with the one on this screen: a fake link chooses its own key, but
/// cannot know the secret this screen shows, so cannot make the code match.
pub fn qr_confirm_code(secret: &QrSecret, host_public_key: &[u8; KEY_LEN]) -> String {
    let mut message = Vec::with_capacity(QR_CONFIRM_LABEL.len() + 32);
    message.extend_from_slice(QR_CONFIRM_LABEL);
    message.extend_from_slice(&host_key_fingerprint(host_public_key));
    let tag = hmac::sign(&secret.mac_key(), &message);
    let bits = tag
        .as_ref()
        .iter()
        .take(3)
        .fold(0u32, |bits, byte| (bits << 8) | u32::from(*byte))
        >> 4;
    (0..CONFIRM_CODE_LEN)
        .map(|position| {
            let shift = 5 * (CONFIRM_CODE_LEN - 1 - position);
            char::from(CONFIRM_ALPHABET[((bits >> shift) & 0x1f) as usize])
        })
        .collect()
}

fn percent_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;

    // The vectors shared with the phone app (plan 261124-0900-qr-pairing,
    // reports/qr-proof-vectors.json). Both repos must reproduce them.
    const SECRET_B64URL: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
    const REQUESTER_KEY_B64: &str = "ERERERERERERERERERERERERERERERERERERERERERE=";
    const HOST_KEY_B64: &str = "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiI=";
    const REQUESTER_NONCE_B64: &str = "MzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzM=";
    const HOST_NONCE_B64: &str = "REREREREREREREREREREREREREREREREREREREREREQ=";
    const REQUESTER_DEVICE_ID: &str = "dev_phone_1";
    const PROOF_B64: &str = "GOj+v2xONqNOVNRVlDFUyppqfYJn2vDW6JzQCGmDEO4=";
    const FINGERPRINT_B64URL: &str = "n3LqDPSVNuPGbHh_cFGG35pDeAg3U66VNtZbOtf83cQ";
    const URI: &str = "zode://pair?v=1&h=dev_abc123&k=n3LqDPSVNuPGbHh_cFGG35pDeAg3U66VNtZbOtf83cQ&s=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8&e=1795000000";

    fn bytes<const LEN: usize>(encoded: &str, engine: &impl base64::Engine) -> [u8; LEN] {
        let decoded = engine.decode(encoded).expect("valid base64");
        decoded.try_into().expect("the length the vector states")
    }

    fn secret() -> QrSecret {
        QrSecret::from_bytes(bytes(SECRET_B64URL, &URL_SAFE_NO_PAD))
    }

    struct Exchange {
        requester_key: [u8; KEY_LEN],
        host_key: [u8; KEY_LEN],
        requester_nonce: [u8; NONCE_LEN],
        host_nonce: [u8; NONCE_LEN],
        requester_device_id: &'static str,
        proof: [u8; QR_PROOF_LEN],
    }

    fn vector() -> Exchange {
        Exchange {
            requester_key: bytes(REQUESTER_KEY_B64, &STANDARD),
            host_key: bytes(HOST_KEY_B64, &STANDARD),
            requester_nonce: bytes(REQUESTER_NONCE_B64, &STANDARD),
            host_nonce: bytes(HOST_NONCE_B64, &STANDARD),
            requester_device_id: REQUESTER_DEVICE_ID,
            proof: bytes(PROOF_B64, &STANDARD),
        }
    }

    fn verify(secret: &QrSecret, exchange: &Exchange, proof: &[u8; QR_PROOF_LEN]) -> bool {
        verify_qr_proof(
            secret,
            &exchange.requester_key,
            &exchange.host_key,
            &exchange.requester_nonce,
            &exchange.host_nonce,
            exchange.requester_device_id,
            proof,
        )
    }

    #[test]
    fn the_proof_reproduces_the_shared_vector() {
        let exchange = vector();
        let proof = qr_proof(
            &secret(),
            &exchange.requester_key,
            &exchange.host_key,
            &exchange.requester_nonce,
            &exchange.host_nonce,
            exchange.requester_device_id,
        );
        assert_eq!(proof, Some(exchange.proof));
        assert!(verify(&secret(), &exchange, &exchange.proof));
    }

    #[test]
    fn the_fingerprint_and_uri_reproduce_the_shared_vectors() {
        let exchange = vector();
        assert_eq!(
            URL_SAFE_NO_PAD.encode(host_key_fingerprint(&exchange.host_key)),
            FINGERPRINT_B64URL
        );
        assert_eq!(
            qr_pairing_uri("dev_abc123", &exchange.host_key, &secret(), 1_795_000_000),
            URI
        );
    }

    #[test]
    fn the_confirm_code_reproduces_the_shared_vector() {
        let exchange = vector();
        assert_eq!(qr_confirm_code(&secret(), &exchange.host_key), "GWHJ");
    }

    #[test]
    fn the_confirm_code_depends_on_the_secret_and_the_key_and_is_four_crockford_characters() {
        let exchange = vector();
        let other_secret = qr_confirm_code(
            &QrSecret::from_bytes([9; QR_SECRET_LEN]),
            &exchange.host_key,
        );
        let other_key = qr_confirm_code(&secret(), &[7; KEY_LEN]);
        for code in [&other_secret, &other_key] {
            assert_eq!(code.len(), 4);
            assert!(
                code.bytes().all(|byte| CONFIRM_ALPHABET.contains(&byte)),
                "{code}"
            );
            assert_ne!(code, "GWHJ");
        }
    }

    #[test]
    fn a_proof_for_any_other_input_is_refused() {
        let exchange = vector();
        let mut wrong_proof = exchange.proof;
        wrong_proof[31] ^= 1;
        assert!(!verify(&secret(), &exchange, &wrong_proof));
        assert!(!verify(
            &QrSecret::from_bytes([9; QR_SECRET_LEN]),
            &exchange,
            &exchange.proof
        ));
        for field in 0..5 {
            let mut other = vector();
            match field {
                0 => other.requester_key[0] ^= 1,
                1 => other.host_key[0] ^= 1,
                2 => other.requester_nonce[0] ^= 1,
                3 => other.host_nonce[0] ^= 1,
                _ => other.requester_device_id = "dev_phone_2",
            }
            assert!(
                !verify(&secret(), &other, &exchange.proof),
                "the proof must bind input {field}"
            );
        }
    }

    #[test]
    fn a_device_id_too_long_for_its_length_has_no_proof() {
        let exchange = vector();
        let too_long = "d".repeat(usize::from(u16::MAX) + 1);
        let proof = |device_id: &str| {
            qr_proof(
                &secret(),
                &exchange.requester_key,
                &exchange.host_key,
                &exchange.requester_nonce,
                &exchange.host_nonce,
                device_id,
            )
        };
        assert_eq!(proof(&too_long), None);
        assert!(proof(&"d".repeat(usize::from(u16::MAX))).is_some());
        assert!(!verify_qr_proof(
            &secret(),
            &exchange.requester_key,
            &exchange.host_key,
            &exchange.requester_nonce,
            &exchange.host_nonce,
            &too_long,
            &exchange.proof,
        ));
    }

    #[test]
    fn the_device_id_is_percent_encoded_and_nothing_else() {
        let uri = qr_pairing_uri("a b&c=d/é", &[1; KEY_LEN], &secret(), 7);
        assert!(uri.contains("&h=a%20b%26c%3Dd%2F%C3%A9&k="), "{uri}");
        assert!(uri.starts_with("zode://pair?v=1&h="));
        assert!(uri.ends_with("&e=7"));
    }

    #[test]
    fn a_random_secret_is_32_bytes_and_never_printed() {
        let first = QrSecret::random().expect("randomness");
        let second = QrSecret::random().expect("randomness");
        assert_ne!(first.0.as_slice(), second.0.as_slice());
        assert_eq!(first.0.len(), QR_SECRET_LEN);
        assert_eq!(format!("{first:?}"), "QrSecret(<redacted>)");
    }
}
