//! Holds `docs/src/team-sharing-protocol.md` to the code it describes.
//!
//! The vectors are parsed out of the published page and asserted here, the
//! same discipline as `zode_env_sync/tests/spec_vectors_match_docs.rs`: change a
//! digit in the page or a byte in the format and this goes red.

use std::collections::HashMap;

use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;
use zode_env_sync::{EntryId, padding};
use zode_sync::{Dek, from_blob};
use zode_team_vault::sharing_key::open_private_key;
use zode_team_vault::team_key::{prologue, wrap_with_fixed_ephemeral_for_test};
use zode_team_vault::{
    EnvelopeIds, Fingerprint, SharingKeypair, TeamCryptoError, TeamKey, open_entry,
    seal::MAX_ENTRY_BYTES, unwrap,
};

const SPEC: &str = include_str!("../../../docs/src/team-sharing-protocol.md");

fn published() -> HashMap<String, String> {
    let mut inside = false;
    let mut vectors = HashMap::new();
    for line in SPEC.lines() {
        if line.trim_start().starts_with("```") {
            if inside {
                break;
            }
            inside = line.contains("zode-team-vectors");
            continue;
        }
        if inside && let Some((name, value)) = line.split_once('=') {
            vectors.insert(name.trim().to_string(), value.trim().to_string());
        }
    }
    assert!(
        !vectors.is_empty(),
        "the spec no longer carries a `zode-team-vectors` block, and it may not simply go away"
    );
    vectors
}

fn vector(vectors: &HashMap<String, String>, name: &str) -> String {
    vectors
        .get(name)
        .unwrap_or_else(|| panic!("the spec no longer publishes `{name}`"))
        .clone()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
        .collect()
}

fn keypair(byte: u8) -> SharingKeypair {
    SharingKeypair::from_private_key(Zeroizing::new([byte; 32])).unwrap()
}

fn ids(vectors: &HashMap<String, String>) -> (String, String, u32) {
    (
        vector(vectors, "team.id"),
        vector(vectors, "recipient.user"),
        vector(vectors, "key.version").parse().unwrap(),
    )
}

#[test]
fn the_published_keys_and_fingerprints_are_the_ones_the_code_computes() {
    let vectors = published();
    let (wrapper, recipient) = (keypair(0x41), keypair(0x42));
    assert_eq!(
        vector(&vectors, "wrapper.public"),
        hex(wrapper.public_key())
    );
    assert_eq!(
        vector(&vectors, "wrapper.fingerprint"),
        wrapper.fingerprint().to_hex()
    );
    assert_eq!(
        vector(&vectors, "wrapper.fingerprint.shown"),
        wrapper.fingerprint().to_string()
    );
    assert_eq!(
        vector(&vectors, "recipient.public"),
        hex(recipient.public_key())
    );
    assert_eq!(
        vector(&vectors, "recipient.fingerprint"),
        recipient.fingerprint().to_hex()
    );
    assert_eq!(
        vector(&vectors, "teamkey.kid"),
        hex(&TeamKey::from_bytes([0x77; 32]).kid())
    );
}

#[test]
fn the_published_prologue_and_envelope_are_the_ones_the_code_writes() {
    let vectors = published();
    let (team_id, user, key_version) = ids(&vectors);
    let ids = EnvelopeIds {
        team_id: &team_id,
        recipient_user_id: &user,
        key_version,
    };
    assert_eq!(vector(&vectors, "prologue"), hex(&prologue(&ids).unwrap()));

    let wrapped = wrap_with_fixed_ephemeral_for_test(
        &keypair(0x41),
        keypair(0x42).public_key(),
        &ids,
        &TeamKey::from_bytes([0x77; 32]),
        &[0x55; 32],
    )
    .unwrap();
    assert_eq!(vector(&vectors, "wrap.len"), wrapped.len().to_string());
    assert_eq!(
        vector(&vectors, "wrap"),
        hex(&wrapped),
        "the envelope bytes moved"
    );
}

#[test]
fn the_published_envelope_opens_to_the_published_key_and_nowhere_else() {
    let vectors = published();
    let (team_id, user, key_version) = ids(&vectors);
    let wrapped = unhex(&vector(&vectors, "wrap"));
    let wrapper = Fingerprint::from_hex(&vector(&vectors, "wrapper.fingerprint")).unwrap();
    let ids = EnvelopeIds {
        team_id: &team_id,
        recipient_user_id: &user,
        key_version,
    };

    let opened = unwrap(&keypair(0x42), &wrapped, &ids, &wrapper).unwrap();
    assert_eq!(hex(&opened.kid()), vector(&vectors, "teamkey.kid"));

    let next_version = EnvelopeIds {
        key_version: key_version + 1,
        ..ids
    };
    assert_eq!(
        unwrap(&keypair(0x42), &wrapped, &next_version, &wrapper).err(),
        Some(TeamCryptoError::WrongKey)
    );
    assert!(matches!(
        unwrap(&keypair(0x42), &wrapped, &ids, &keypair(0x43).fingerprint()),
        Err(TeamCryptoError::WrapperMismatch { .. })
    ));
}

#[test]
fn the_published_framing_is_the_one_the_code_writes() {
    let vectors = published();
    let mut inner = vector(&vectors, "frame.key.version")
        .parse::<u32>()
        .unwrap()
        .to_le_bytes()
        .to_vec();
    inner.extend_from_slice(vector(&vectors, "frame.entry.utf8").as_bytes());
    let block = padding::pack(vector(&vectors, "frame.seq").parse().unwrap(), &inner).unwrap();
    assert_eq!(vector(&vectors, "frame.head"), hex(&block[..20]));
    assert_eq!(
        vector(&vectors, "frame.sha256"),
        hex(&Sha256::digest(&block))
    );
    assert_eq!(
        vector(&vectors, "max.entry.bytes"),
        MAX_ENTRY_BYTES.to_string()
    );
}

#[test]
fn an_entry_sealed_by_an_earlier_build_still_opens_and_only_in_its_slot() {
    let vectors = published();
    let team_id = vector(&vectors, "team.id");
    let envelope = from_blob(include_str!("fixtures/team-entry-v1.b64").trim()).unwrap();
    let key = TeamKey::from_bytes([0x77; 32]);
    let entry = EntryId::parse(&"a1".repeat(16)).unwrap();

    let opened = open_entry(&key, &team_id, &entry, 1, None, &envelope)
        .expect("the frozen entry must still open — the AAD or the framing changed");
    assert_eq!(
        (opened.seq, opened.key_version, opened.payload.as_slice()),
        (3, 1, &b"A=1"[..])
    );

    let other = EntryId::parse(&"b2".repeat(16)).unwrap();
    assert_eq!(
        open_entry(&key, &team_id, &other, 1, None, &envelope).err(),
        Some(TeamCryptoError::WrongKey)
    );
    assert_eq!(
        open_entry(&key, "68b1f0c2a4d3e5f607182911", &entry, 1, None, &envelope).err(),
        Some(TeamCryptoError::WrongKey)
    );
    assert_eq!(
        open_entry(&key, &team_id, &entry, 1, Some(4), &envelope).err(),
        Some(TeamCryptoError::Rollback { seen: 4, got: 3 })
    );
}

#[test]
fn a_sharing_key_sealed_by_an_earlier_build_still_opens() {
    let vectors = published();
    let envelope = from_blob(include_str!("fixtures/sharing-key-v1.b64").trim()).unwrap();
    let opened = open_private_key(
        &Dek::from_bytes([0x11; 32]),
        &vector(&vectors, "recipient.user"),
        &envelope,
    )
    .expect("the frozen sharing key must still open");
    assert_eq!(
        hex(opened.public_key()),
        vector(&vectors, "recipient.public")
    );
}

#[test]
fn the_spec_still_states_what_it_exists_to_state() {
    for claim in [
        "no password and no key derivation anywhere in this design",
        "**substituting** a member's public key",
        "the **pinned** fingerprint",
        "refuses a server that names a lower one",
        "two for one version are both refused",
        "never opened this team",
        "The only state anything is sealed to",
    ] {
        assert!(SPEC.contains(claim), "the spec stopped saying: {claim}");
    }
}
