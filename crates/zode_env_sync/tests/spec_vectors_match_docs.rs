//! Holds `docs/src/env-sync-protocol.md` to the code it describes.
//!
//! The published spec is the whole trust argument for this feature: the server
//! is closed-source, so the only thing a user can check is that the client
//! does what its documentation says. A spec that drifts from the code is worse
//! than no spec — it is a confident, checkable-looking claim that happens to
//! be false.
//!
//! So the document is not prose about the format. It is the source of the
//! vectors, parsed and asserted here. Change a digit in the table and this
//! goes red; change the format without updating the table and this goes red
//! too. That is the same lesson the Crockford alphabet taught in
//! `zode_sync::recovery_key`: two copies of one rule, and neither knowing the
//! other moved.

use std::collections::HashMap;

use sha2::{Digest as _, Sha256};
use zode_env_sync::{EntryId, EnvCryptoError, EnvDek, padding, seal, unwrap_key};
use zode_sync::{DEK_LEN, Dek, from_blob};

const USER: &str = "68b1f0c2a4d3e5f60718293a";
const SPEC: &str = include_str!("../../../docs/src/env-sync-protocol.md");

/// Pulls the ```` ```text zode-env-vectors ```` block out of the spec.
fn published_vectors() -> HashMap<String, String> {
    let mut inside = false;
    let mut vectors = HashMap::new();

    for line in SPEC.lines() {
        if line.trim_start().starts_with("```") {
            if inside {
                break;
            }
            inside = line.contains("zode-env-vectors");
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((name, value)) = line.split_once('=') {
            vectors.insert(name.trim().to_string(), value.trim().to_string());
        }
    }

    assert!(
        !vectors.is_empty(),
        "the spec no longer carries a `zode-env-vectors` block — it is the only \
         thing keeping the published format honest, so it may not simply go away",
    );
    vectors
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn published(vectors: &HashMap<String, String>, name: &str) -> String {
    vectors
        .get(name)
        .unwrap_or_else(|| panic!("the spec no longer publishes `{name}`"))
        .clone()
}

#[test]
fn the_published_fingerprints_are_the_ones_the_code_computes() {
    let vectors = published_vectors();

    assert_eq!(
        published(&vectors, "envdek.kid"),
        hex(&EnvDek::from_bytes([0x33; DEK_LEN]).kid()),
        "the published envDEK fingerprint is not what this build produces",
    );
    assert_eq!(
        published(&vectors, "dek.kid"),
        hex(&Dek::from_bytes([0x11; DEK_LEN]).kid()),
        "the published DEK fingerprint is not what this build produces",
    );
}

#[test]
fn the_published_framing_is_the_one_the_code_writes() {
    let vectors = published_vectors();
    let block = padding::pack(3, b"A=1").expect("the published payload must pack");

    assert_eq!(
        published(&vectors, "pack.block.len"),
        block.len().to_string(),
        "the framed block is no longer the length the spec publishes",
    );
    assert_eq!(
        published(&vectors, "pack.block.head"),
        hex(&block[..16]),
        "the header layout moved away from what the spec describes",
    );
    assert_eq!(
        published(&vectors, "pack.block.sha256"),
        hex(&Sha256::digest(&block)),
        "the framed block differs from the spec somewhere past its first 16 bytes",
    );
}

#[test]
fn the_published_limits_are_the_ones_the_code_enforces() {
    let vectors = published_vectors();

    assert_eq!(
        published(&vectors, "max.payload.bytes"),
        padding::MAX_PAYLOAD_BYTES.to_string(),
    );
    assert_eq!(
        published(&vectors, "pad.block.bytes"),
        padding::PAD_BLOCK.to_string(),
    );
}

#[test]
fn the_spec_still_states_the_two_claims_it_exists_to_make() {
    // Not a format check. These two sentences are the reason the page is
    // published at all, and a tidy-up that removes them removes the argument
    // while leaving the vectors intact and green.
    assert!(
        SPEC.contains("no password and no key derivation anywhere in this design"),
        "the spec stopped saying there is no password in the design",
    );
    assert!(
        SPEC.contains("stamped with the OUTER key"),
        "the spec stopped documenting the `env-key` AAD exception, which is the \
         one thing a second implementer will otherwise get wrong",
    );
}

#[test]
fn an_entry_sealed_by_an_earlier_build_still_opens() {
    let envelope = from_blob(include_str!("fixtures/env-entry-v1.b64").trim())
        .expect("the frozen entry must still parse");
    let entry = EntryId::parse("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1").unwrap();

    let opened = seal::open(
        &EnvDek::from_bytes([0x33; DEK_LEN]),
        USER,
        &seal::entry_resource(&entry).unwrap(),
        None,
        &envelope,
    )
    .expect("the frozen entry must still decrypt — the AAD or the framing changed");

    assert_eq!(opened.seq, 3);
    assert_eq!(opened.payload, b"A=1");
}

#[test]
fn a_key_wrapped_by_an_earlier_build_still_unwraps() {
    let envelope = from_blob(include_str!("fixtures/env-key-v1.b64").trim())
        .expect("the frozen wrapped key must still parse");

    let env_dek = unwrap_key(&Dek::from_bytes([0x11; DEK_LEN]), USER, &envelope)
        .expect("the frozen wrapped key must still unwrap");

    assert_eq!(
        hex(&env_dek.kid()),
        hex(&EnvDek::from_bytes([0x33; DEK_LEN]).kid())
    );
}

#[test]
fn the_frozen_entry_cannot_be_opened_from_another_slot() {
    // The frozen vector doubles as the substitution test: these are real
    // stored bytes, not something this test just produced.
    let envelope = from_blob(include_str!("fixtures/env-entry-v1.b64").trim()).unwrap();
    let other = EntryId::parse("b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2").unwrap();

    assert!(matches!(
        seal::open(
            &EnvDek::from_bytes([0x33; DEK_LEN]),
            USER,
            &seal::entry_resource(&other).unwrap(),
            None,
            &envelope,
        ),
        Err(EnvCryptoError::WrongKey)
    ));
}
