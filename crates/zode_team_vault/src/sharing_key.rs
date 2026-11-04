use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use remote_relay_protocol::{DeviceKeypair, KEY_LEN, is_low_order_public_key};
use sha2::{Digest as _, Sha256};
use zeroize::{Zeroize as _, Zeroizing};
use zode_sync::{Dek, Envelope, Resource, decrypt_at, encrypt_at};

use crate::TeamCryptoError;

/// Bytes of SHA-256 kept as a key's fingerprint. The server computes the same
/// value (phase 06), so the two can be compared without trusting either.
pub const FINGERPRINT_LEN: usize = 16;

/// The first 16 bytes of `SHA-256(public key)`.
///
/// What a person compares out loud, and what a pin remembers. Shown as eight
/// groups of four hex digits; sent as 32 hex digits.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint([u8; FINGERPRINT_LEN]);

impl Fingerprint {
    pub fn of(public_key: &[u8; KEY_LEN]) -> Self {
        let digest = Sha256::digest(public_key);
        let mut bytes = [0u8; FINGERPRINT_LEN];
        bytes.copy_from_slice(&digest[..FINGERPRINT_LEN]);
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; FINGERPRINT_LEN] {
        &self.0
    }

    /// The wire form: 32 lowercase hex digits.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// Reads the wire form back, strictly: lowercase, exactly 32 digits.
    pub fn from_hex(raw: &str) -> Option<Self> {
        if raw.len() != FINGERPRINT_LEN * 2 {
            return None;
        }
        let mut bytes = [0u8; FINGERPRINT_LEN];
        for (index, pair) in raw.as_bytes().chunks_exact(2).enumerate() {
            let high = hex_value(pair[0])?;
            let low = hex_value(pair[1])?;
            bytes[index] = (high << 4) | low;
        }
        Some(Self(bytes))
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

impl std::fmt::Display for Fingerprint {
    /// Eight groups of four, for reading aloud: `a1b2 c3d4 …`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, pair) in self.0.chunks_exact(2).enumerate() {
            if index > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{:02x}{:02x}", pair[0], pair[1])?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fingerprint({})", self.to_hex())
    }
}

/// The user's X25519 sharing key pair.
///
/// One per user, not per machine: the private half is sealed under the
/// recovery key and synced, so every machine of the user opens the same team
/// envelopes. The private half is wiped on drop (by `DeviceKeypair`).
pub struct SharingKeypair(DeviceKeypair);

impl SharingKeypair {
    pub fn generate() -> Result<Self, TeamCryptoError> {
        DeviceKeypair::generate()
            .map(Self)
            .map_err(|error| TeamCryptoError::Malformed(error.to_string()))
    }

    /// Rebuilds the pair from its private half, deriving the public half.
    pub fn from_private_key(
        private_key: Zeroizing<[u8; KEY_LEN]>,
    ) -> Result<Self, TeamCryptoError> {
        let keypair = DeviceKeypair::from_private_key(private_key)
            .map_err(|error| TeamCryptoError::Malformed(error.to_string()))?;
        if is_low_order_public_key(keypair.public_key()) {
            return Err(TeamCryptoError::LowOrderKey);
        }
        Ok(Self(keypair))
    }

    pub fn public_key(&self) -> &[u8; KEY_LEN] {
        self.0.public_key()
    }

    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::of(self.public_key())
    }

    pub(crate) fn private_key(&self) -> &[u8; KEY_LEN] {
        self.0.private_key()
    }
}

impl std::fmt::Debug for SharingKeypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SharingKeypair({})", self.fingerprint())
    }
}

/// The wire form of a public key: base64url, no padding, as the server expects.
pub fn encode_public_key(public_key: &[u8; KEY_LEN]) -> String {
    URL_SAFE_NO_PAD.encode(public_key)
}

/// Reads a public key the server sent. A low-order point is refused here, at
/// the door, so no later step can seal a team key to one.
pub fn decode_public_key(encoded: &str) -> Result<[u8; KEY_LEN], TeamCryptoError> {
    let public_key: [u8; KEY_LEN] = URL_SAFE_NO_PAD
        .decode(encoded)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| {
            TeamCryptoError::Malformed("a public key is 32 bytes of base64url".into())
        })?;
    if is_low_order_public_key(&public_key) {
        return Err(TeamCryptoError::LowOrderKey);
    }
    Ok(public_key)
}

/// Seals the private half under the recovery key, for the `sharing-key` slot.
///
/// Stamped with `kid(DEK)`, like `env-key`: this envelope hands a key over, so
/// its identity is the outer key.
pub fn seal_private_key(
    dek: &Dek,
    user_id: &str,
    keypair: &SharingKeypair,
) -> Result<Envelope, TeamCryptoError> {
    Ok(encrypt_at(
        dek,
        user_id,
        &Resource::sharing_key(),
        keypair.private_key(),
    )?)
}

/// Opens the private half with the recovery key.
pub fn open_private_key(
    dek: &Dek,
    user_id: &str,
    envelope: &Envelope,
) -> Result<SharingKeypair, TeamCryptoError> {
    let mut bytes = decrypt_at(dek, user_id, &Resource::sharing_key(), envelope)?;
    let parsed = <[u8; KEY_LEN]>::try_from(bytes.as_slice()).map_err(|_| {
        TeamCryptoError::Malformed(format!(
            "the sealed sharing key is {} bytes, not {KEY_LEN}",
            bytes.len()
        ))
    });
    // The decrypted copy must not outlive this function, parsed or not.
    bytes.zeroize();
    SharingKeypair::from_private_key(Zeroizing::new(parsed?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_sync::DEK_LEN;

    const USER: &str = "68b1f0c2a4d3e5f60718293a";

    fn fixed() -> SharingKeypair {
        SharingKeypair::from_private_key(Zeroizing::new([0x41; KEY_LEN])).unwrap()
    }

    #[test]
    fn the_fingerprint_reads_as_eight_groups_and_round_trips_its_wire_form() {
        let fingerprint = fixed().fingerprint();
        let shown = fingerprint.to_string();
        assert_eq!(shown.split(' ').count(), 8, "{shown}");
        assert!(shown.split(' ').all(|group| group.len() == 4), "{shown}");
        assert_eq!(shown.replace(' ', ""), fingerprint.to_hex());
        assert_eq!(
            Fingerprint::from_hex(&fingerprint.to_hex()),
            Some(fingerprint)
        );
    }

    #[test]
    fn a_fingerprint_in_another_spelling_is_refused() {
        let hex = fixed().fingerprint().to_hex();
        assert_eq!(Fingerprint::from_hex(&hex.to_uppercase()), None);
        assert_eq!(Fingerprint::from_hex(&hex[..30]), None);
        assert_eq!(Fingerprint::from_hex(&format!("{hex}00")), None);
    }

    #[test]
    fn debug_never_reveals_the_private_key() {
        let keypair = fixed();
        let rendered = format!("{keypair:?}");
        assert!(rendered.starts_with("SharingKeypair("), "{rendered}");
        assert!(!rendered.contains("4141"), "{rendered}");
        assert!(!rendered.contains("65, 65"), "{rendered}");
    }

    #[test]
    fn the_private_key_round_trips_under_the_recovery_key() {
        let dek = Dek::from_bytes([0x11; DEK_LEN]);
        let sealed = seal_private_key(&dek, USER, &fixed()).unwrap();
        let opened = open_private_key(&dek, USER, &sealed).unwrap();
        assert_eq!(opened.public_key(), fixed().public_key());
    }

    #[test]
    fn another_recovery_key_or_another_user_does_not_open_it() {
        let dek = Dek::from_bytes([0x11; DEK_LEN]);
        let sealed = seal_private_key(&dek, USER, &fixed()).unwrap();
        assert_eq!(
            open_private_key(&Dek::from_bytes([0x99; DEK_LEN]), USER, &sealed).err(),
            Some(TeamCryptoError::KeyRotated)
        );
        assert_eq!(
            open_private_key(&dek, "68b1f0c2a4d3e5f60718293b", &sealed).err(),
            Some(TeamCryptoError::WrongKey)
        );
    }

    #[test]
    fn the_sealed_private_key_is_not_in_the_envelope() {
        let dek = Dek::from_bytes([0x11; DEK_LEN]);
        let sealed = seal_private_key(&dek, USER, &fixed()).unwrap();
        let json = serde_json::to_string(&sealed).unwrap();
        assert!(!json.contains(&URL_SAFE_NO_PAD.encode([0x41u8; KEY_LEN])));
        assert!(
            !json.contains(&base64::engine::general_purpose::STANDARD.encode([0x41u8; KEY_LEN]))
        );
    }

    #[test]
    fn a_public_key_round_trips_and_a_low_order_one_is_refused() {
        let public_key = *fixed().public_key();
        assert_eq!(
            decode_public_key(&encode_public_key(&public_key)),
            Ok(public_key)
        );
        assert_eq!(
            decode_public_key(&encode_public_key(&[0u8; KEY_LEN])),
            Err(TeamCryptoError::LowOrderKey)
        );
        let mut one = [0u8; KEY_LEN];
        one[0] = 1;
        assert_eq!(
            decode_public_key(&encode_public_key(&one)),
            Err(TeamCryptoError::LowOrderKey)
        );
        assert!(matches!(
            decode_public_key("not base64!"),
            Err(TeamCryptoError::Malformed(_))
        ));
        assert!(matches!(
            decode_public_key(&encode_public_key(&public_key)[..20]),
            Err(TeamCryptoError::Malformed(_))
        ));
    }

    #[test]
    fn generated_pairs_are_distinct() {
        let first = SharingKeypair::generate().unwrap();
        let second = SharingKeypair::generate().unwrap();
        assert_ne!(first.public_key(), second.public_key());
    }
}
