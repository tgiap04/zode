use zeroize::Zeroize as _;
use zode_sync::{Envelope, Resource, decrypt_at, encrypt_at};

use crate::env_dek::EnvDek;
use crate::ids::EntryId;
use crate::{ENV_MANIFEST_RESOURCE, EnvCryptoError, padding};

/// The slot one env file lives in.
pub fn entry_resource(entry: &EntryId) -> Result<Resource, EnvCryptoError> {
    Ok(Resource::env_entry(&entry.as_hex())?)
}

/// The slot the encrypted manifest lives in.
pub fn manifest_resource() -> Result<Resource, EnvCryptoError> {
    Ok(Resource::env_singleton(ENV_MANIFEST_RESOURCE)?)
}

/// What came out of a blob that verified.
#[derive(Debug)]
pub struct Opened {
    pub seq: u64,
    pub payload: Vec<u8>,
}

/// Seals one payload into the envelope the server stores.
///
/// `seq` is written inside the ciphertext rather than beside it. The server
/// cannot forge a higher one, because forging one would mean encrypting — and
/// it has no key.
pub fn seal(
    env_dek: &EnvDek,
    user_id: &str,
    resource: &Resource,
    seq: u64,
    payload: &[u8],
) -> Result<Envelope, EnvCryptoError> {
    let mut block = padding::pack(seq, payload)?;
    let sealed = encrypt_at(&env_dek.as_dek(), user_id, resource, &block);
    // The framed plaintext is the `.env` with a header on it. Do not leave it
    // on the heap for the allocator to hand to whatever asks next.
    block.zeroize();
    Ok(sealed?)
}

/// Opens one blob, and refuses an old one.
///
/// `seen_seq` is what this machine has already applied for this resource.
/// Passing it in — rather than returning the sequence and trusting the caller
/// to check — is what makes the rollback refusal structural: there is no path
/// out of this function that hands back the payload of a replayed blob.
pub fn open(
    env_dek: &EnvDek,
    user_id: &str,
    resource: &Resource,
    seen_seq: Option<u64>,
    envelope: &Envelope,
) -> Result<Opened, EnvCryptoError> {
    let mut block = decrypt_at(&env_dek.as_dek(), user_id, resource, envelope)?;
    let unpacked = padding::unpack(&block);
    block.zeroize();

    let (seq, mut payload) = unpacked?;

    if let Some(seen) = seen_seq
        && seq < seen
    {
        payload.zeroize();
        return Err(EnvCryptoError::Rollback { seen, got: seq });
    }

    Ok(Opened { seq, payload })
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_sync::DEK_LEN;

    const USER: &str = "68b1f0c2a4d3e5f60718293a";

    fn key() -> EnvDek {
        EnvDek::from_bytes([0x33; DEK_LEN])
    }

    fn entry(byte: u8) -> EntryId {
        EntryId::parse(&format!("{byte:02x}").repeat(16)).unwrap()
    }

    #[test]
    fn a_sealed_file_opens_again() {
        let resource = entry_resource(&entry(0xa1)).unwrap();
        let sealed = seal(&key(), USER, &resource, 3, b"STRIPE_KEY=sk_live_x").unwrap();
        let opened = open(&key(), USER, &resource, Some(3), &sealed).unwrap();

        assert_eq!(opened.seq, 3);
        assert_eq!(opened.payload, b"STRIPE_KEY=sk_live_x");
    }

    #[test]
    fn the_server_cannot_serve_one_entry_from_another_slot() {
        // The hole that `entry_id` in the AAD closes. Same user, same key,
        // same bytes — only the slot differs.
        let sealed = seal(
            &key(),
            USER,
            &entry_resource(&entry(0xa1)).unwrap(),
            1,
            b"A=1",
        )
        .unwrap();

        assert!(matches!(
            open(
                &key(),
                USER,
                &entry_resource(&entry(0xb2)).unwrap(),
                None,
                &sealed
            ),
            Err(EnvCryptoError::WrongKey)
        ));
    }

    #[test]
    fn the_manifest_cannot_be_served_as_an_entry() {
        let sealed = seal(&key(), USER, &manifest_resource().unwrap(), 1, b"{}").unwrap();

        assert!(matches!(
            open(
                &key(),
                USER,
                &entry_resource(&entry(0xa1)).unwrap(),
                None,
                &sealed
            ),
            Err(EnvCryptoError::WrongKey)
        ));
    }

    #[test]
    fn an_older_version_is_refused_and_yields_nothing() {
        let resource = entry_resource(&entry(0xa1)).unwrap();
        let old = seal(&key(), USER, &resource, 4, b"API_KEY=revoked").unwrap();

        match open(&key(), USER, &resource, Some(9), &old) {
            Err(EnvCryptoError::Rollback { seen, got }) => {
                assert_eq!((seen, got), (9, 4));
            }
            other => panic!("expected Rollback, got {other:?}"),
        }
    }

    #[test]
    fn the_same_version_is_not_a_rollback() {
        // Re-applying what is already applied is a no-op, not an attack.
        let resource = entry_resource(&entry(0xa1)).unwrap();
        let sealed = seal(&key(), USER, &resource, 5, b"A=1").unwrap();
        assert_eq!(
            open(&key(), USER, &resource, Some(5), &sealed).unwrap().seq,
            5
        );
    }

    #[test]
    fn a_machine_that_has_seen_nothing_accepts_any_version() {
        let resource = entry_resource(&entry(0xa1)).unwrap();
        let sealed = seal(&key(), USER, &resource, 42, b"A=1").unwrap();
        assert_eq!(
            open(&key(), USER, &resource, None, &sealed).unwrap().seq,
            42
        );
    }

    #[test]
    fn another_user_does_not_open_it() {
        let resource = entry_resource(&entry(0xa1)).unwrap();
        let sealed = seal(&key(), USER, &resource, 1, b"A=1").unwrap();

        assert!(matches!(
            open(&key(), "68b1f0c2a4d3e5f60718293b", &resource, None, &sealed),
            Err(EnvCryptoError::WrongKey)
        ));
    }

    #[test]
    fn another_key_reports_rotation_rather_than_a_mistake() {
        let resource = entry_resource(&entry(0xa1)).unwrap();
        let sealed = seal(&key(), USER, &resource, 1, b"A=1").unwrap();
        let other = EnvDek::from_bytes([0x44; DEK_LEN]);

        assert!(matches!(
            open(&other, USER, &resource, None, &sealed),
            Err(EnvCryptoError::KeyRotated { .. })
        ));
    }

    #[test]
    fn the_stored_length_does_not_betray_the_file() {
        use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

        let resource = entry_resource(&entry(0xa1)).unwrap();
        let tiny = seal(&key(), USER, &resource, 1, b"A=1").unwrap();
        let big = seal(&key(), USER, &resource, 1, &vec![0x41; 4000]).unwrap();

        assert_eq!(
            BASE64.decode(&tiny.ct).unwrap().len(),
            BASE64.decode(&big.ct).unwrap().len(),
        );
    }
}
