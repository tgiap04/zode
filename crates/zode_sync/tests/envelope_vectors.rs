//! Fixed-input tests for the crypto layer.
//!
//! These exist because `zode_sync`'s crypto has no I/O: everything below is a
//! pure function of its arguments, which is the whole reason the layering is
//! drawn where it is. A crypto module entangled with the network can only be
//! tested against a live server, and then it is not really tested at all.

use zode_sync::{Dek, Kind, SyncCryptoError, decrypt, encrypt, from_blob, to_blob};

const USER: &str = "68b1f0c2a4d3e5f60718293a";

fn key() -> Dek {
    Dek::from_bytes([
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
        0x1e, 0x1f,
    ])
}

#[test]
fn a_sealed_artifact_opens_again() {
    let dek = key();
    let plaintext = br#"{ "theme": "One Dark" } // a comment survives"#;

    let envelope = encrypt(&dek, USER, Kind::Settings, plaintext).unwrap();
    let opened = decrypt(&dek, USER, Kind::Settings, &envelope).unwrap();

    assert_eq!(opened, plaintext);
}

#[test]
fn every_sealing_uses_a_fresh_nonce() {
    // AES-GCM loses all confidentiality guarantees if a nonce repeats under
    // one key. The API makes it impossible for a caller to supply one; this
    // asserts the generator behind it actually varies.
    let dek = key();
    let first = encrypt(&dek, USER, Kind::Settings, b"same input").unwrap();
    let second = encrypt(&dek, USER, Kind::Settings, b"same input").unwrap();

    assert_ne!(first.nonce, second.nonce);
    assert_ne!(first.ct, second.ct);
}

#[test]
fn the_server_cannot_move_a_blob_between_kinds() {
    let dek = key();
    let envelope = encrypt(&dek, USER, Kind::Keymap, b"the keymap").unwrap();

    // Same user, same key, same bytes — only the slot it was served from
    // differs. The AAD is what makes this fail.
    let swapped = decrypt(&dek, USER, Kind::Settings, &envelope);
    assert!(
        matches!(swapped, Err(SyncCryptoError::WrongKey)),
        "got {swapped:?}"
    );
}

#[test]
fn the_server_cannot_move_a_blob_between_users() {
    let dek = key();
    let envelope = encrypt(&dek, USER, Kind::Settings, b"ada's settings").unwrap();

    let swapped = decrypt(&dek, "68b1f0c2a4d3e5f60718293b", Kind::Settings, &envelope);
    assert!(
        matches!(swapped, Err(SyncCryptoError::WrongKey)),
        "got {swapped:?}"
    );
}

#[test]
fn a_different_key_reports_rotation_not_a_typo() {
    let envelope = encrypt(&key(), USER, Kind::Settings, b"sealed under one key").unwrap();
    let other = Dek::from_bytes([0x99; 32]);

    match decrypt(&other, USER, Kind::Settings, &envelope) {
        Err(SyncCryptoError::KeyRotated { theirs, ours }) => {
            assert_eq!(theirs, key().kid());
            assert_eq!(ours, other.kid());
        }
        other => panic!("expected KeyRotated, got {other:?}"),
    }
}

#[test]
fn a_flipped_ciphertext_bit_is_refused() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let dek = key();
    let mut envelope = encrypt(&dek, USER, Kind::Settings, b"authentic").unwrap();

    let mut bytes = BASE64.decode(&envelope.ct).unwrap();
    bytes[0] ^= 0b0000_0001;
    envelope.ct = BASE64.encode(&bytes);

    assert!(matches!(
        decrypt(&dek, USER, Kind::Settings, &envelope),
        Err(SyncCryptoError::WrongKey),
    ));
}

#[test]
fn a_tampered_key_fingerprint_is_refused() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let dek = key();
    let mut envelope = encrypt(&dek, USER, Kind::Settings, b"authentic").unwrap();

    // Claim a fingerprint that is not this key's. The `kid` is inside the AAD,
    // so even matching it back up would not help an attacker.
    envelope.kid = BASE64.encode([0u8; 8]);
    assert!(matches!(
        decrypt(&dek, USER, Kind::Settings, &envelope),
        Err(SyncCryptoError::KeyRotated { .. }),
    ));
}

#[test]
fn an_unknown_version_is_refused_rather_than_guessed() {
    let dek = key();
    let mut envelope = encrypt(&dek, USER, Kind::Settings, b"from the future").unwrap();
    envelope.v = 2;

    assert!(matches!(
        decrypt(&dek, USER, Kind::Settings, &envelope),
        Err(SyncCryptoError::UnsupportedVersion(2)),
    ));
}

#[test]
fn an_unknown_algorithm_is_refused() {
    let dek = key();
    let mut envelope = encrypt(&dek, USER, Kind::Settings, b"x").unwrap();
    envelope.alg = "AES-128-CBC".into();

    assert!(matches!(
        decrypt(&dek, USER, Kind::Settings, &envelope),
        Err(SyncCryptoError::Malformed(_)),
    ));
}

#[test]
fn the_blob_round_trips_through_its_wire_form() {
    let dek = key();
    let envelope = encrypt(&dek, USER, Kind::Extensions, b"[\"rust-analyzer\"]").unwrap();

    let blob = to_blob(&envelope).unwrap();
    let parsed = from_blob(&blob).unwrap();

    assert_eq!(
        decrypt(&dek, USER, Kind::Extensions, &parsed).unwrap(),
        b"[\"rust-analyzer\"]"
    );
}

#[test]
fn garbage_is_not_mistaken_for_an_envelope() {
    assert!(matches!(
        from_blob("not base64 !!"),
        Err(SyncCryptoError::Malformed(_))
    ));
    assert!(matches!(
        from_blob(&{
            use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
            BASE64.encode(b"{\"nope\": true}")
        }),
        Err(SyncCryptoError::Malformed(_)),
    ));
}

/// A frozen envelope, produced once by this code and committed.
///
/// Round-trip tests pass even if the format changes on both sides at the same
/// time. This one does not: if the wire shape, the AAD construction, or the
/// key fingerprint ever changes, an envelope written by an older Zode stops
/// opening — which is exactly the failure a user would hit after upgrading.
#[test]
fn an_envelope_written_by_an_earlier_build_still_opens() {
    const FROZEN: &str = include_str!("fixtures/envelope-v1.b64");

    let envelope = from_blob(FROZEN.trim()).expect("the frozen vector must still parse");
    let opened = decrypt(&key(), USER, Kind::Settings, &envelope)
        .expect("the frozen vector must still decrypt");

    assert_eq!(
        String::from_utf8(opened).unwrap(),
        "{\n  \"theme\": \"One Dark\"\n}\n"
    );
}
