use ring::rand::{SecureRandom as _, SystemRandom};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroize as _;
use zode_sync::{DEK_LEN, Dek, Envelope, KID_LEN, Resource, decrypt_at, encrypt_at};

use crate::{ENV_KEY_RESOURCE, EnvCryptoError};

/// The key every `.env` blob is sealed under: 32 random bytes, and nothing
/// derived from anything.
///
/// Separate from [`zode_sync::Dek`] on purpose, and the separation buys three
/// things that one shared key does not:
///
/// - rotating env access after losing a laptop does not force every machine to
///   re-enter a recovery key for settings as well;
/// - the user still writes down exactly one recovery key, because this one is
///   wrapped under that one rather than shown;
/// - sharing a project with a teammate later means wrapping 32 bytes to them,
///   not re-encrypting every file.
///
/// `Debug` is written by hand and `Drop` zeroes the bytes, for the same reason
/// as `Dek`: a log line and a core dump are how a key like this actually
/// escapes.
pub struct EnvDek {
    bytes: [u8; DEK_LEN],
}

impl EnvDek {
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

    /// The fingerprint written into every env envelope.
    ///
    /// Identical construction to `Dek::kid`, which is what makes it the same
    /// value `encrypt_at` stamps on the envelope — so "you have the wrong key"
    /// and "this was rotated elsewhere" stay distinguishable.
    pub fn kid(&self) -> [u8; KID_LEN] {
        let digest = Sha256::digest(self.bytes);
        let mut kid = [0u8; KID_LEN];
        kid.copy_from_slice(&digest[..KID_LEN]);
        kid
    }

    /// The AEAD key, as `zode_sync` wants it.
    ///
    /// Builds a short-lived `Dek` rather than reaching into `zode_sync`'s
    /// private bytes. The copy is zeroed when it drops, and the alternative —
    /// a public accessor on `Dek` — would weaken the one type the whole design
    /// depends on keeping closed.
    pub(crate) fn as_dek(&self) -> Dek {
        Dek::from_bytes(self.bytes)
    }

    pub(crate) fn expose(&self) -> &[u8; DEK_LEN] {
        &self.bytes
    }
}

impl Drop for EnvDek {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

impl std::fmt::Debug for EnvDek {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EnvDek(<redacted>)")
    }
}

/// The resource the wrapped env key is stored under.
pub fn key_resource() -> Result<Resource, EnvCryptoError> {
    Ok(Resource::env_singleton(ENV_KEY_RESOURCE)?)
}

/// Seals the env key under the recovery key.
///
/// The AAD carries `kid(DEK)` rather than `kid(envDEK)` — this is the one
/// envelope in the system whose identity is the OUTER key, because it is the
/// envelope that hands over the inner one. A reader who expects `kid(envDEK)`
/// here has found the exception, not a bug.
pub fn wrap_key(dek: &Dek, user_id: &str, env_dek: &EnvDek) -> Result<Envelope, EnvCryptoError> {
    Ok(encrypt_at(
        dek,
        user_id,
        &key_resource()?,
        env_dek.expose(),
    )?)
}

/// Opens the env key with the recovery key.
pub fn unwrap_key(dek: &Dek, user_id: &str, envelope: &Envelope) -> Result<EnvDek, EnvCryptoError> {
    let mut bytes = decrypt_at(dek, user_id, &key_resource()?, envelope)?;
    let result = <[u8; DEK_LEN]>::try_from(bytes.as_slice())
        .map(EnvDek::from_bytes)
        .map_err(|_| {
            EnvCryptoError::Malformed(format!(
                "the wrapped env key is {} bytes, not {DEK_LEN}",
                bytes.len()
            ))
        });
    // The decrypted copy must not outlive this function whether it parsed or
    // not — a 32-byte Vec left on the heap is the key.
    bytes.zeroize();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_reveals_the_key() {
        let rendered = format!("{:?}", EnvDek::from_bytes([0xAB; DEK_LEN]));
        assert_eq!(rendered, "EnvDek(<redacted>)");
        assert!(!rendered.contains("ab"));
        assert!(!rendered.contains("171"));
    }

    #[test]
    fn generate_produces_distinct_keys() {
        let a = EnvDek::generate().unwrap();
        let b = EnvDek::generate().unwrap();
        assert_ne!(a.expose(), b.expose());
    }

    #[test]
    fn the_key_round_trips_through_its_wrapper() {
        let dek = Dek::from_bytes([0x11; DEK_LEN]);
        let env_dek = EnvDek::from_bytes([0x22; DEK_LEN]);

        let wrapped = wrap_key(&dek, "user-a", &env_dek).unwrap();
        let opened = unwrap_key(&dek, "user-a", &wrapped).unwrap();

        assert_eq!(opened.expose(), env_dek.expose());
    }

    #[test]
    fn another_recovery_key_does_not_open_it() {
        let env_dek = EnvDek::from_bytes([0x22; DEK_LEN]);
        let wrapped = wrap_key(&Dek::from_bytes([0x11; DEK_LEN]), "user-a", &env_dek).unwrap();

        assert!(matches!(
            unwrap_key(&Dek::from_bytes([0x99; DEK_LEN]), "user-a", &wrapped),
            Err(EnvCryptoError::KeyRotated { .. })
        ));
    }

    #[test]
    fn another_user_does_not_open_it() {
        let env_dek = EnvDek::from_bytes([0x22; DEK_LEN]);
        let dek = Dek::from_bytes([0x11; DEK_LEN]);
        let wrapped = wrap_key(&dek, "user-a", &env_dek).unwrap();

        assert!(matches!(
            unwrap_key(&dek, "user-b", &wrapped),
            Err(EnvCryptoError::WrongKey)
        ));
    }

    #[test]
    fn the_wrapper_is_stamped_with_the_outer_key() {
        // The documented exception, asserted so it cannot be "fixed" into
        // consistency with everything else.
        use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

        let dek = Dek::from_bytes([0x11; DEK_LEN]);
        let env_dek = EnvDek::from_bytes([0x22; DEK_LEN]);
        let wrapped = wrap_key(&dek, "user-a", &env_dek).unwrap();

        assert_eq!(BASE64.decode(&wrapped.kid).unwrap(), dek.kid());
        assert_ne!(BASE64.decode(&wrapped.kid).unwrap(), env_dek.kid());
    }
}
