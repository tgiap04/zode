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

/// A slot on the sync service, and the identity the ciphertext is bound to.
///
/// Exists because the server grew a second shape. `/api/sync/:kind` holds one
/// document per kind and there are exactly three of them; env sync holds one
/// per file and there is no bound. Addressing both through [`Kind`] would mean
/// either a `Kind` variant per env file — impossible, it is an enum — or a
/// second client that drifts from this one.
///
/// The AAD fields are carried here rather than derived at the call site so
/// there is one place where "what this ciphertext is bound to" is decided. A
/// resource that forgets to bind its identity is a resource the server can
/// swap for another, and the tag still verifies because the tag only covers
/// the ciphertext.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resource {
    /// Path under the API root, no leading slash: `sync/settings`, `env/<id>`.
    path: String,
    /// First AAD field after the user id.
    aad_label: &'static str,
    /// Trailing AAD field, for resources where the label alone does not say
    /// which one this is. `None` for the three original kinds, and it must
    /// stay `None` for them — an extra field would change bytes already
    /// sitting on users' accounts.
    discriminator: Option<String>,
}

impl Resource {
    /// One of the three original artifacts.
    pub fn sync(kind: Kind) -> Self {
        Self {
            path: format!("sync/{kind}"),
            aad_label: kind.as_str(),
            discriminator: None,
        }
    }

    /// One env file.
    ///
    /// Takes the id as a string rather than a typed `EntryId` because that type
    /// lives in `zode_env_sync`, which depends on this crate. The validation
    /// that matters happens here either way.
    pub fn env_entry(entry_id: &str) -> Result<Self, SyncCryptoError> {
        let entry_id = validated_segment(entry_id)?;
        Ok(Self {
            path: format!("env/{entry_id}"),
            aad_label: "env",
            discriminator: Some(entry_id),
        })
    }

    /// One of the env singletons: `env-key`, `env-manifest`.
    pub fn env_singleton(name: &'static str) -> Result<Self, SyncCryptoError> {
        validated_segment(name)?;
        Ok(Self {
            path: format!("sync/{name}"),
            aad_label: name,
            discriminator: None,
        })
    }

    /// The collection of env entries, for the listing endpoint.
    ///
    /// Addressable but never sealed — nothing is encrypted under it, so its
    /// AAD label is never used. It exists so the listing request goes through
    /// the same validated-path type as everything else rather than round a
    /// side door.
    pub fn env_listing() -> Self {
        Self {
            path: "env".to_string(),
            aad_label: "env",
            discriminator: None,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn aad_label(&self) -> &str {
        self.aad_label
    }

    pub fn aad_discriminator(&self) -> Option<&str> {
        self.discriminator.as_deref()
    }
}

impl From<Kind> for Resource {
    fn from(kind: Kind) -> Self {
        Self::sync(kind)
    }
}

impl std::fmt::Display for Resource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.path)
    }
}

/// Accepts one path segment, rejects anything that could reshape a URL.
///
/// Rejected rather than sanitised: stripping the bad characters out of
/// `../../admin` leaves a different, valid-looking id, and a caller that
/// handed us one is a caller with a bug worth surfacing.
fn validated_segment(raw: &str) -> Result<String, SyncCryptoError> {
    if raw.is_empty() || raw.len() > 64 {
        return Err(SyncCryptoError::Malformed(format!(
            "resource segment length {}",
            raw.len()
        )));
    }
    if !raw
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(SyncCryptoError::Malformed(
            "a resource segment is lowercase letters, digits and dashes".into(),
        ));
    }
    Ok(raw.to_string())
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
fn aad(user_id: &str, resource: &Resource, version: u32, kid: &[u8; KID_LEN]) -> Vec<u8> {
    let label = resource.aad_label();
    let discriminator = resource.aad_discriminator();
    let mut bytes = Vec::with_capacity(
        user_id.len() + label.len() + KID_LEN + 16 + discriminator.map_or(0, |d| d.len() + 1),
    );
    bytes.extend_from_slice(user_id.as_bytes());
    bytes.push(SEPARATOR);
    bytes.extend_from_slice(label.as_bytes());
    bytes.push(SEPARATOR);
    bytes.extend_from_slice(version.to_string().as_bytes());
    bytes.push(SEPARATOR);
    bytes.extend_from_slice(kid);

    // Appended only when the resource has one, which is what keeps the three
    // original kinds byte-identical to what earlier builds wrote.
    if let Some(discriminator) = discriminator {
        bytes.push(SEPARATOR);
        bytes.extend_from_slice(discriminator.as_bytes());
    }
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
    encrypt_at(dek, user_id, &Resource::sync(kind), plaintext)
}

/// The same, for any resource.
///
/// [`encrypt`] is kept as the `Kind` front door rather than replaced, so every
/// call site and every frozen test vector for the three shipped artifacts goes
/// on compiling and passing untouched. That is the evidence that this
/// generalisation changed nothing for them.
pub fn encrypt_at(
    dek: &Dek,
    user_id: &str,
    resource: &Resource,
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
            Aad::from(aad(user_id, resource, ENVELOPE_VERSION, &kid)),
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
    decrypt_at(dek, user_id, &Resource::sync(kind), envelope)
}

/// The same, for any resource.
pub fn decrypt_at(
    dek: &Dek,
    user_id: &str,
    resource: &Resource,
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
            Aad::from(aad(user_id, resource, envelope.v, &ours)),
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
