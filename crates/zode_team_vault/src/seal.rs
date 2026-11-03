use zeroize::{Zeroize as _, Zeroizing};
use zode_env_sync::{EntryId, padding};
use zode_sync::{Envelope, Resource, decrypt_at, encrypt_at};

use crate::TeamCryptoError;
use crate::team_key::TeamKey;

/// Bytes of the key version at the head of the framed payload.
const KEY_VERSION_BYTES: usize = 4;

/// The largest entry this format carries: the `.env` limit, less the key
/// version that rides inside the framed payload.
pub const MAX_ENTRY_BYTES: usize = padding::MAX_PAYLOAD_BYTES - KEY_VERSION_BYTES;

/// The slot one vault entry lives in, and the identity its ciphertext is
/// bound to.
pub fn entry_resource(team_id: &str, entry: &EntryId) -> Result<Resource, TeamCryptoError> {
    Ok(Resource::team_entry(team_id, &entry.as_hex())?)
}

/// What came out of an entry that verified and was not a rollback.
pub struct OpenedEntry {
    pub seq: u64,
    pub key_version: u32,
    pub payload: Zeroizing<Vec<u8>>,
}

impl std::fmt::Debug for OpenedEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenedEntry")
            .field("seq", &self.seq)
            .field("key_version", &self.key_version)
            .field("payload_bytes", &self.payload.len())
            .finish()
    }
}

/// Seals one entry under the team key of `key_version`.
///
/// The framing is the `.env` framing (`seq`, length, padding to 4 KiB) with the
/// key version as the first four bytes of the framed payload, so `seq` and the
/// version are both inside the ciphertext where the server cannot change them.
/// The owner field of the AAD is the team: entries are written by many
/// members, and the team is what they belong to.
pub fn seal_entry(
    team_key: &TeamKey,
    team_id: &str,
    entry: &EntryId,
    key_version: u32,
    seq: u64,
    payload: &[u8],
) -> Result<Envelope, TeamCryptoError> {
    if payload.len() > MAX_ENTRY_BYTES {
        return Err(TeamCryptoError::TooLarge {
            bytes: payload.len(),
            limit: MAX_ENTRY_BYTES,
        });
    }
    let resource = entry_resource(team_id, entry)?;
    let mut inner = Zeroizing::new(Vec::with_capacity(KEY_VERSION_BYTES + payload.len()));
    inner.extend_from_slice(&key_version.to_le_bytes());
    inner.extend_from_slice(payload);

    let mut block = padding::pack(seq, &inner)?;
    let sealed = encrypt_at(&team_key.as_dek(), team_id, &resource, &block);
    block.zeroize();
    Ok(sealed?)
}

/// Opens one entry, and refuses an old one.
///
/// `key_version` is the version the listing names, whose key is `team_key`.
/// `seen_seq` is what this machine has already seen for this entry; as with
/// `.env`, there is no path out of this function that hands back the payload
/// of a replayed blob.
pub fn open_entry(
    team_key: &TeamKey,
    team_id: &str,
    entry: &EntryId,
    key_version: u32,
    seen_seq: Option<u64>,
    envelope: &Envelope,
) -> Result<OpenedEntry, TeamCryptoError> {
    let resource = entry_resource(team_id, entry)?;
    let mut block = decrypt_at(&team_key.as_dek(), team_id, &resource, envelope)?;
    let unpacked = padding::unpack(&block);
    block.zeroize();
    let (seq, inner) = unpacked?;
    let inner = Zeroizing::new(inner);

    let sealed_version = inner
        .first_chunk::<KEY_VERSION_BYTES>()
        .map(|bytes| u32::from_le_bytes(*bytes))
        .ok_or_else(|| TeamCryptoError::Malformed("the entry carries no key version".into()))?;
    if sealed_version != key_version {
        return Err(TeamCryptoError::KeyVersionMismatch {
            listed: key_version,
            sealed: sealed_version,
        });
    }

    if let Some(seen) = seen_seq
        && seq < seen
    {
        return Err(TeamCryptoError::Rollback { seen, got: seq });
    }

    Ok(OpenedEntry {
        seq,
        key_version: sealed_version,
        payload: Zeroizing::new(inner[KEY_VERSION_BYTES..].to_vec()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::team_key::TEAM_KEY_LEN;

    const TEAM: &str = "68b1f0c2a4d3e5f607182900";
    const OTHER_TEAM: &str = "68b1f0c2a4d3e5f607182911";

    fn key(byte: u8) -> TeamKey {
        TeamKey::from_bytes([byte; TEAM_KEY_LEN])
    }

    fn entry(byte: u8) -> EntryId {
        EntryId::parse(&format!("{byte:02x}").repeat(16)).unwrap()
    }

    fn sealed(seq: u64) -> Envelope {
        seal_entry(
            &key(0x51),
            TEAM,
            &entry(0xa1),
            2,
            seq,
            b"STRIPE_KEY=sk_live_x",
        )
        .unwrap()
    }

    #[test]
    fn a_sealed_entry_opens_again() {
        let opened = open_entry(&key(0x51), TEAM, &entry(0xa1), 2, Some(3), &sealed(3)).unwrap();
        assert_eq!((opened.seq, opened.key_version), (3, 2));
        assert_eq!(opened.payload.as_slice(), b"STRIPE_KEY=sk_live_x");
    }

    #[test]
    fn an_entry_served_from_another_slot_or_team_fails() {
        let envelope = sealed(1);
        assert_eq!(
            open_entry(&key(0x51), TEAM, &entry(0xb2), 2, None, &envelope).err(),
            Some(TeamCryptoError::WrongKey)
        );
        assert_eq!(
            open_entry(&key(0x51), OTHER_TEAM, &entry(0xa1), 2, None, &envelope).err(),
            Some(TeamCryptoError::WrongKey)
        );
    }

    #[test]
    fn an_older_seq_is_refused_and_yields_nothing() {
        assert_eq!(
            open_entry(&key(0x51), TEAM, &entry(0xa1), 2, Some(9), &sealed(4)).err(),
            Some(TeamCryptoError::Rollback { seen: 9, got: 4 })
        );
    }

    #[test]
    fn the_same_seq_is_not_a_rollback_and_a_fresh_machine_takes_any() {
        assert!(open_entry(&key(0x51), TEAM, &entry(0xa1), 2, Some(5), &sealed(5)).is_ok());
        assert!(open_entry(&key(0x51), TEAM, &entry(0xa1), 2, None, &sealed(42)).is_ok());
    }

    #[test]
    fn another_version_s_key_reports_rotation() {
        assert_eq!(
            open_entry(&key(0x52), TEAM, &entry(0xa1), 3, None, &sealed(1)).err(),
            Some(TeamCryptoError::KeyRotated)
        );
    }

    #[test]
    fn a_listing_that_misnames_the_version_is_refused() {
        // Same key, wrong label: the version inside the ciphertext wins.
        assert_eq!(
            open_entry(&key(0x51), TEAM, &entry(0xa1), 1, None, &sealed(1)).err(),
            Some(TeamCryptoError::KeyVersionMismatch {
                listed: 1,
                sealed: 2
            })
        );
    }

    #[test]
    fn two_seals_never_share_a_nonce() {
        let first = sealed(1);
        let second = sealed(1);
        assert_ne!(first.nonce, second.nonce);
        assert_ne!(first.ct, second.ct);
    }

    #[test]
    fn the_stored_length_does_not_betray_the_entry() {
        let tiny = seal_entry(&key(0x51), TEAM, &entry(0xa1), 1, 1, b"A=1").unwrap();
        let big = seal_entry(&key(0x51), TEAM, &entry(0xa1), 1, 1, &[0x41; 4000]).unwrap();
        assert_eq!(tiny.ct.len(), big.ct.len());
    }

    #[test]
    fn an_oversized_entry_is_refused_before_it_is_sealed() {
        assert!(matches!(
            seal_entry(
                &key(0x51),
                TEAM,
                &entry(0xa1),
                1,
                1,
                &vec![0; MAX_ENTRY_BYTES + 1]
            ),
            Err(TeamCryptoError::TooLarge { .. })
        ));
        assert!(
            seal_entry(
                &key(0x51),
                TEAM,
                &entry(0xa1),
                1,
                1,
                &vec![0; MAX_ENTRY_BYTES]
            )
            .is_ok()
        );
    }

    #[test]
    fn debug_shows_the_size_never_the_secret() {
        let opened = open_entry(&key(0x51), TEAM, &entry(0xa1), 2, None, &sealed(1)).unwrap();
        let rendered = format!("{opened:?}");
        assert!(!rendered.contains("sk_live"), "{rendered}");
        assert!(rendered.contains("payload_bytes"), "{rendered}");
    }
}
