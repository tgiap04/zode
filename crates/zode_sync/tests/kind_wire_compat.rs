//! What the three shipped artifacts put on the wire, frozen.
//!
//! `zode_sync::client` is being generalised so env sync can address more than
//! three hard-coded slots. Settings, keymaps and extension lists are already
//! stored on real users' accounts under the current shape, so the
//! generalisation has exactly one acceptance criterion: **not one byte
//! changes** for them.
//!
//! Two things are held here, and they fail for different reasons:
//!
//! - the AAD, held by frozen ciphertexts that an older build produced. Change
//!   how the AAD is assembled and these stop opening — which is precisely what
//!   a user would hit after upgrading.
//! - the request line, held by driving the real client against a recording
//!   fake. Change the URL and the server answers 404 forever.
//!
//! Neither is a round-trip test. A round-trip passes happily while both sides
//! move together, and both sides moving together is the failure.

mod fake_store;

use std::sync::{Arc, Mutex};

use fake_store::{API, block};
use http_client::{FakeHttpClient, HttpClient, Response};
use zode_sync::client::{self, Precondition};
use zode_sync::{Dek, Kind, Resource, decrypt, from_blob};

const USER: &str = "68b1f0c2a4d3e5f60718293a";
const TOKEN: &str = "test-access-token";

/// The key the frozen vectors were sealed under.
fn frozen_key() -> Dek {
    Dek::from_bytes([
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
        0x1e, 0x1f,
    ])
}

fn open_frozen(blob: &str, kind: Kind) -> String {
    let envelope = from_blob(blob.trim()).expect("the frozen vector must still parse");
    let opened = decrypt(&frozen_key(), USER, kind, &envelope)
        .expect("the frozen vector must still decrypt — the AAD changed");
    String::from_utf8(opened).expect("the frozen vector held UTF-8")
}

#[test]
fn a_settings_envelope_from_an_earlier_build_still_opens() {
    assert_eq!(
        open_frozen(include_str!("fixtures/envelope-v1.b64"), Kind::Settings),
        "{\n  \"theme\": \"One Dark\"\n}\n"
    );
}

#[test]
fn a_keymap_envelope_from_an_earlier_build_still_opens() {
    assert_eq!(
        open_frozen(include_str!("fixtures/wire-keymap-v1.b64"), Kind::Keymap),
        "[\n  { \"bindings\": {} }\n]\n"
    );
}

#[test]
fn an_extensions_envelope_from_an_earlier_build_still_opens() {
    assert_eq!(
        open_frozen(
            include_str!("fixtures/wire-extensions-v1.b64"),
            Kind::Extensions
        ),
        "[\"rust-analyzer\",\"toml\"]\n"
    );
}

/// Records what the client asked for, and answers with the least interesting
/// thing that lets each call return.
#[derive(Clone, Default)]
struct Recorder {
    seen: Arc<Mutex<Vec<(String, String)>>>,
}

impl Recorder {
    fn client(&self) -> Arc<dyn HttpClient> {
        let seen = self.seen.clone();
        FakeHttpClient::create(move |request| {
            seen.lock()
                .unwrap()
                .push((request.method().to_string(), request.uri().to_string()));
            async move {
                Ok(Response::builder()
                    .status(404)
                    .body(http_client::AsyncBody::from("{\"error\":\"not_found\"}"))
                    .unwrap())
            }
        })
    }

    fn take(&self) -> Vec<(String, String)> {
        std::mem::take(&mut *self.seen.lock().unwrap())
    }
}

#[test]
fn every_kind_keeps_its_url() {
    for (kind, expected) in [
        (Kind::Settings, "settings"),
        (Kind::Keymap, "keymap"),
        (Kind::Extensions, "extensions"),
    ] {
        let recorder = Recorder::default();
        let http = recorder.client();

        let resource = Resource::sync(kind);
        block(client::fetch(&http, API, TOKEN, &resource)).unwrap();
        block(client::store(
            &http,
            API,
            TOKEN,
            &resource,
            "blob",
            Precondition::Create,
        ))
        .unwrap();
        block(client::forget(&http, API, TOKEN, &resource)).unwrap();

        let seen = recorder.take();
        let url = format!("{API}/sync/{expected}");
        assert_eq!(
            seen,
            vec![
                ("GET".into(), url.clone()),
                ("PUT".into(), url.clone()),
                ("DELETE".into(), url),
            ],
            "the wire path for {expected} moved",
        );
    }
}

#[test]
fn the_kind_column_in_the_url_is_the_kind_name() {
    // `Kind::as_str` is what the server's `:kind` path segment matches on, so
    // it is a wire contract rather than a display detail. Asserted separately
    // from the URL test so a rename shows up as a rename, not as three broken
    // requests.
    assert_eq!(Kind::Settings.as_str(), "settings");
    assert_eq!(Kind::Keymap.as_str(), "keymap");
    assert_eq!(Kind::Extensions.as_str(), "extensions");
    assert_eq!(
        Kind::ALL.len(),
        3,
        "the Sync Settings modal renders one row per kind"
    );
}

#[test]
fn a_resource_segment_cannot_reshape_the_url() {
    for bad in [
        "..",
        "../../admin",
        "a/b",
        "has space",
        "UPPER",
        "trailing/",
        "",
        &"x".repeat(65),
    ] {
        assert!(
            Resource::env_entry(bad).is_err(),
            "{bad:?} was accepted as an entry id",
        );
    }

    let good = Resource::env_entry("0123456789abcdef0123456789abcdef").unwrap();
    assert_eq!(good.path(), "env/0123456789abcdef0123456789abcdef");
}

#[test]
fn an_env_entry_binds_its_own_id() {
    // The hole this closes: with the AAD binding only user and label, the
    // server could serve entry B's blob from entry A's slot and the tag would
    // still verify, because the tag only covers the ciphertext.
    use zode_sync::{SyncCryptoError, decrypt_at, encrypt_at};

    let dek = frozen_key();
    let a = Resource::env_entry("0123456789abcdef0123456789abcdef").unwrap();
    let b = Resource::env_entry("fedcba9876543210fedcba9876543210").unwrap();

    let sealed = encrypt_at(&dek, USER, &a, b"DATABASE_URL=postgres://...").unwrap();

    assert!(decrypt_at(&dek, USER, &a, &sealed).is_ok());
    assert!(
        matches!(
            decrypt_at(&dek, USER, &b, &sealed),
            Err(SyncCryptoError::WrongKey)
        ),
        "a blob opened in the wrong entry's slot",
    );
}

#[test]
fn the_three_kinds_carry_no_discriminator() {
    // This is the whole compatibility argument in one assertion: the extra AAD
    // field exists, and it is absent for everything that shipped before it.
    for kind in Kind::ALL {
        assert_eq!(Resource::sync(kind).aad_discriminator(), None, "{kind}");
    }
    assert_eq!(
        Resource::env_entry("0123456789abcdef0123456789abcdef")
            .unwrap()
            .aad_discriminator(),
        Some("0123456789abcdef0123456789abcdef"),
    );
}
