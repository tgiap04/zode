use ring::rand::{SecureRandom as _, SystemRandom};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroize as _;

/// AES-256 key length.
pub const DEK_LEN: usize = 32;

/// Length of the key fingerprint carried in an envelope.
pub const KID_LEN: usize = 8;

/// Domain separator so the recovery key's check character and the envelope's
/// `kid` are derived from different hashes of the same key. Without it, one
/// leaks information about the other for no reason at all.
const CHECK_DOMAIN: &[u8] = b"zode-recovery-check";

/// The data encryption key: 32 random bytes, and the entire secret.
///
/// It never leaves the machine except as the recovery key the user writes
/// down. There is no key derivation step and no server-held wrapping key —
/// both would put something crackable in reach of the server that already
/// holds the ciphertext.
///
/// `Debug` is written by hand and `Drop` zeroes the bytes, because the two
/// ways a key like this actually escapes are a log line and a core dump.
pub struct Dek {
    bytes: [u8; DEK_LEN],
}

impl Dek {
    /// A fresh key from the OS CSPRNG.
    pub fn generate() -> anyhow::Result<Self> {
        let mut bytes = [0u8; DEK_LEN];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| anyhow::anyhow!("the system random number generator refused"))?;
        Ok(Self { bytes })
    }

    pub fn from_bytes(bytes: [u8; DEK_LEN]) -> Self {
        Self { bytes }
    }

    pub(crate) fn bytes(&self) -> &[u8; DEK_LEN] {
        &self.bytes
    }

    /// The key fingerprint written into every envelope.
    ///
    /// Its whole job is telling "you typed the wrong recovery key" apart from
    /// "this blob was written with a key that has since been rotated". Without
    /// it both surface as a GCM tag failure and the user reads the wrong
    /// message.
    pub fn kid(&self) -> [u8; KID_LEN] {
        let digest = Sha256::digest(self.bytes);
        let mut kid = [0u8; KID_LEN];
        kid.copy_from_slice(&digest[..KID_LEN]);
        kid
    }

    /// Five bits of checksum for the recovery key, catching transcription
    /// slips before they turn into an indistinguishable decryption failure.
    pub(crate) fn check_bits(&self) -> u8 {
        let mut hasher = Sha256::new();
        hasher.update(CHECK_DOMAIN);
        hasher.update(self.bytes);
        hasher.finalize()[0] & 0b0001_1111
    }
}

impl Drop for Dek {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

impl std::fmt::Debug for Dek {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the bytes. A derived `Debug` here would put the key into any
        // log line, panic message, or error chain that happens to include a
        // struct holding one.
        write!(f, "Dek(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_reveals_the_key() {
        let dek = Dek::from_bytes([0xAB; DEK_LEN]);
        let rendered = format!("{dek:?}");
        assert_eq!(rendered, "Dek(<redacted>)");
        assert!(
            !rendered.contains("ab"),
            "the key must not appear in Debug output"
        );
        assert!(
            !rendered.contains("171"),
            "the key must not appear in Debug output"
        );
    }

    #[test]
    fn generate_produces_distinct_keys() {
        let a = Dek::generate().unwrap();
        let b = Dek::generate().unwrap();
        assert_ne!(a.bytes(), b.bytes(), "two generated keys must not collide");
    }

    #[test]
    fn kid_is_stable_and_key_specific() {
        let dek = Dek::from_bytes([7u8; DEK_LEN]);
        assert_eq!(dek.kid(), dek.kid());
        assert_ne!(dek.kid(), Dek::from_bytes([8u8; DEK_LEN]).kid());
    }

    #[test]
    fn the_check_bits_are_not_the_kid() {
        // Domain separation, asserted rather than assumed.
        let dek = Dek::from_bytes([3u8; DEK_LEN]);
        assert_ne!(dek.check_bits(), dek.kid()[0] & 0b0001_1111);
    }
}
