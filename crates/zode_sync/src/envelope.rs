use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom as _, SystemRandom};
use serde::{Deserialize, Serialize};

use crate::dek::{Dek, KID_LEN};

/// Envelope format version. A reader that meets a version it does not know
/// refuses rather than guessing — a guess here means writing the wrong bytes
/// over a user's settings.
pub const ENVELOPE_VERSION: u32 = 1;

const ALGORITHM: &str = "AES-256-GCM";

/// Field separator for the additional authenticated data.
///
/// `0x1F` (unit separator) rather than `:` because no `user_id`, kind name, or
/// version string can contain it — so no two distinct inputs can build the
/// same AAD.
const SEPARATOR: u8 = 0x1F;

/// The three artifacts, matching the server's `kind` path segment exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Settings,
    Keymap,
    Extensions,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Settings, Kind::Keymap, Kind::Extensions];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Settings => "settings",
            Kind::Keymap => "keymap",
            Kind::Extensions => "extensions",
        }
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug)]
pub enum SyncCryptoError {
    /// The tag did not verify under a key with the right fingerprint. Either
    /// the wrong key or tampered ciphertext.
    WrongKey,
    /// The blob was written under a different key — almost always because
    /// another machine rotated it.
    KeyRotated {
        theirs: [u8; KID_LEN],
        ours: [u8; KID_LEN],
    },
    /// A version this build does not understand.
    UnsupportedVersion(u32),
    /// Structurally not an envelope.
    Malformed(String),
}

impl std::fmt::Display for SyncCryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongKey => write!(f, "that recovery key does not open this data"),
            Self::KeyRotated { .. } => write!(
                f,
                "this data was encrypted with a different recovery key — it was probably rotated on another machine",
            ),
            Self::UnsupportedVersion(v) => {
                write!(
                    f,
                    "this data uses envelope format {v}, which this version of Zode cannot read"
                )
            }
            Self::Malformed(reason) => write!(f, "this data is not a Zode envelope: {reason}"),
        }
    }
}

impl std::error::Error for SyncCryptoError {}

/// The wire form. Serialised to JSON, base64'd, and handed to the server,
/// which stores it without ever looking inside.
#[derive(Debug, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u32,
    pub alg: String,
    pub kid: String,
    pub nonce: String,
    pub ct: String,
}

/// Binds the ciphertext to who it belongs to, what it is, how it is framed,
/// and which key wrote it.
///
/// Without this the server could move a blob between kinds or between users
/// and the client would decrypt it happily — the tag would still verify,
/// because the tag only covers the ciphertext.
fn aad(user_id: &str, kind: Kind, version: u32, kid: &[u8; KID_LEN]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(user_id.len() + kind.as_str().len() + KID_LEN + 16);
    bytes.extend_from_slice(user_id.as_bytes());
    bytes.push(SEPARATOR);
    bytes.extend_from_slice(kind.as_str().as_bytes());
    bytes.push(SEPARATOR);
    bytes.extend_from_slice(version.to_string().as_bytes());
    bytes.push(SEPARATOR);
    bytes.extend_from_slice(kid);
    bytes
}

fn sealing_key(dek: &Dek) -> Result<LessSafeKey, SyncCryptoError> {
    let unbound = UnboundKey::new(&AES_256_GCM, dek.bytes())
        .map_err(|_| SyncCryptoError::Malformed("key length".into()))?;
    Ok(LessSafeKey::new(unbound))
}

/// Seals one artifact.
///
/// The nonce is generated in here and is NOT a parameter. AES-GCM fails
/// catastrophically when a nonce repeats under one key, and the surest way to
/// stop that is to make it impossible for a caller to supply one.
pub fn encrypt(
    dek: &Dek,
    user_id: &str,
    kind: Kind,
    plaintext: &[u8],
) -> Result<Envelope, SyncCryptoError> {
    let kid = dek.kid();
    let mut nonce_bytes = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce_bytes)
        .map_err(|_| SyncCryptoError::Malformed("no randomness available".into()))?;

    let mut buffer = plaintext.to_vec();
    sealing_key(dek)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce_bytes),
            Aad::from(aad(user_id, kind, ENVELOPE_VERSION, &kid)),
            &mut buffer,
        )
        .map_err(|_| SyncCryptoError::Malformed("sealing failed".into()))?;

    Ok(Envelope {
        v: ENVELOPE_VERSION,
        alg: ALGORITHM.to_string(),
        kid: BASE64.encode(kid),
        nonce: BASE64.encode(nonce_bytes),
        ct: BASE64.encode(&buffer),
    })
}

/// Opens one artifact, or explains precisely why it could not.
pub fn decrypt(
    dek: &Dek,
    user_id: &str,
    kind: Kind,
    envelope: &Envelope,
) -> Result<Vec<u8>, SyncCryptoError> {
    if envelope.v != ENVELOPE_VERSION {
        return Err(SyncCryptoError::UnsupportedVersion(envelope.v));
    }
    if envelope.alg != ALGORITHM {
        return Err(SyncCryptoError::Malformed(format!(
            "algorithm {}",
            envelope.alg
        )));
    }

    let theirs: [u8; KID_LEN] = BASE64
        .decode(&envelope.kid)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| SyncCryptoError::Malformed("key fingerprint".into()))?;

    let ours = dek.kid();
    if theirs != ours {
        // Answered before attempting to open, so the user is told the key was
        // rotated rather than that they mistyped it.
        return Err(SyncCryptoError::KeyRotated { theirs, ours });
    }

    let nonce_bytes: [u8; NONCE_LEN] = BASE64
        .decode(&envelope.nonce)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| SyncCryptoError::Malformed("nonce".into()))?;

    let mut buffer = BASE64
        .decode(&envelope.ct)
        .map_err(|_| SyncCryptoError::Malformed("ciphertext".into()))?;

    let opened = sealing_key(dek)?
        .open_in_place(
            Nonce::assume_unique_for_key(nonce_bytes),
            Aad::from(aad(user_id, kind, envelope.v, &ours)),
            &mut buffer,
        )
        .map_err(|_| SyncCryptoError::WrongKey)?;

    Ok(opened.to_vec())
}

/// The envelope as it travels: JSON, then base64, which is what the `blob`
/// field of the sync API carries.
pub fn to_blob(envelope: &Envelope) -> Result<String, SyncCryptoError> {
    let json = serde_json::to_vec(envelope)
        .map_err(|error| SyncCryptoError::Malformed(error.to_string()))?;
    Ok(BASE64.encode(json))
}

pub fn from_blob(blob: &str) -> Result<Envelope, SyncCryptoError> {
    let json = BASE64
        .decode(blob)
        .map_err(|_| SyncCryptoError::Malformed("blob is not base64".into()))?;
    serde_json::from_slice(&json).map_err(|error| SyncCryptoError::Malformed(error.to_string()))
}
