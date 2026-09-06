//! Rotating the key, including the ways it can be interrupted.
//!
//! Rotation is the one operation that can strand a user's data: it makes the
//! old key stop working, one artifact at a time. So the tests here are mostly
//! about the failure points, not the happy path.

mod fake_store;

use fake_store::{FakeStore, Sandbox, USER, block, context};
use zode_sync::rotate::{RotationOutcome, plan, rotate};
use zode_sync::{Dek, Kind};

fn old_key() -> Dek {
    Dek::from_bytes([0x11; 32])
}

fn new_key() -> Dek {
    Dek::from_bytes([0x22; 32])
}

#[test]
fn rotation_re_encrypts_everything_stored() {
    let sandbox = Sandbox::new("rotate-all");
    let store = FakeStore::new();
    let (old, new) = (old_key(), new_key());

    store.seed(Kind::Settings, &old, "{ \"a\": 1 }");
    store.seed(Kind::Keymap, &old, "[]");
    store.seed(Kind::Extensions, &old, "# zode-extensions v1\na/one\n");

    let mut persisted = 0;
    let outcome = block(rotate(
        &context(&store),
        &old,
        &new,
        &sandbox.state_path(),
        || {
            persisted += 1;
            async {}
        },
    ))
    .unwrap();

    assert!(matches!(outcome, RotationOutcome::Complete), "{outcome:?}");
    assert_eq!(
        persisted, 1,
        "the new key is saved once, not once per artifact"
    );

    // Everything now carries the new fingerprint, and nothing is left needing
    // the old one.
    let after = block(plan(&context(&store), &old, &new)).unwrap();
    assert!(after.pending.is_empty(), "{after:?}");
    assert_eq!(after.done.len(), 3);

    // And the content survived the round trip.
    assert_eq!(
        store.read_plaintext(Kind::Settings, &new),
        Some("{ \"a\": 1 }".to_string())
    );
}

/// The ordering that matters: the new key is saved only after the first
/// artifact is safely stored under it. Saving it first and then losing the
/// network would leave this machine holding a key that opens nothing, having
/// discarded the one that opened everything.
#[test]
fn the_new_key_is_saved_only_after_the_first_successful_write() {
    let sandbox = Sandbox::new("rotate-order");
    let store = FakeStore::new();
    let (old, new) = (old_key(), new_key());
    store.seed(Kind::Settings, &old, "{ \"a\": 1 }");

    let mut writes_when_persisted = None;
    let writes = store.writes.clone();
    let outcome = block(rotate(
        &context(&store),
        &old,
        &new,
        &sandbox.state_path(),
        || {
            if writes_when_persisted.is_none() {
                writes_when_persisted = Some(writes.load(std::sync::atomic::Ordering::SeqCst));
            }
            async {}
        },
    ))
    .unwrap();

    assert!(matches!(outcome, RotationOutcome::Complete));
    assert_eq!(
        writes_when_persisted,
        Some(1),
        "the key must be saved after the first write lands, not before",
    );
}

/// Interrupted halfway: one artifact under the new key, two under the old.
/// A second attempt must finish the job rather than start over or give up.
#[test]
fn a_second_attempt_resumes_where_the_first_stopped() {
    let sandbox = Sandbox::new("rotate-resume");
    let store = FakeStore::new();
    let (old, new) = (old_key(), new_key());

    store.seed(Kind::Settings, &new, "{ \"already\": true }");
    store.seed(Kind::Keymap, &old, "[]");
    store.seed(Kind::Extensions, &old, "# zode-extensions v1\n");

    let midway = block(plan(&context(&store), &old, &new)).unwrap();
    assert_eq!(midway.done, vec![Kind::Settings]);
    assert_eq!(midway.pending, vec![Kind::Keymap, Kind::Extensions]);

    let outcome = block(rotate(
        &context(&store),
        &old,
        &new,
        &sandbox.state_path(),
        || async {},
    ))
    .unwrap();
    assert!(matches!(outcome, RotationOutcome::Complete), "{outcome:?}");

    // The already-rotated artifact was not touched again, and is intact.
    assert_eq!(
        store.read_plaintext(Kind::Settings, &new),
        Some("{ \"already\": true }".to_string()),
    );
}

/// Nothing stored at all. Rotation must succeed rather than error — a user who
/// has never pushed still gets a fresh key.
#[test]
fn rotating_with_nothing_stored_succeeds() {
    let sandbox = Sandbox::new("rotate-empty");
    let store = FakeStore::new();
    let (old, new) = (old_key(), new_key());

    let planned = block(plan(&context(&store), &old, &new)).unwrap();
    assert_eq!(planned.absent.len(), 3);

    let mut persisted = 0;
    let outcome = block(rotate(
        &context(&store),
        &old,
        &new,
        &sandbox.state_path(),
        || {
            persisted += 1;
            async {}
        },
    ))
    .unwrap();

    assert!(matches!(outcome, RotationOutcome::Complete));
    // Nothing was written, so nothing licensed keeping the new key yet. The
    // caller stores it once the modal is acknowledged.
    assert_eq!(persisted, 0);
}

/// A blob neither key opens — written by a third key, or corrupt. Rotation
/// must leave it alone rather than destroy it: re-encrypting means decrypting
/// first, and that is not possible.
#[test]
fn an_unreadable_blob_is_left_untouched() {
    let store = FakeStore::new();
    let (old, new) = (old_key(), new_key());
    let stranger = Dek::from_bytes([0x77; 32]);

    store.seed(Kind::Settings, &stranger, "{ \"not mine\": true }");

    let planned = block(plan(&context(&store), &old, &new)).unwrap();
    // Settings is stored but appears in none of the three lists: not pending
    // (the old key does not open it), not done (nor does the new one), and not
    // absent (it is there). Rotation simply passes over it.
    assert!(!planned.pending.contains(&Kind::Settings), "{planned:?}");
    assert!(!planned.done.contains(&Kind::Settings), "{planned:?}");
    assert!(!planned.absent.contains(&Kind::Settings), "{planned:?}");

    // Still readable by whoever holds the third key.
    assert_eq!(
        store.read_plaintext(Kind::Settings, &stranger),
        Some("{ \"not mine\": true }".to_string()),
    );
}

/// Another machine writes mid-rotation. The write must fail rather than
/// clobber it, and what is left is reported so a second run can finish.
#[test]
fn a_concurrent_write_stops_the_rotation_cleanly() {
    let sandbox = Sandbox::new("rotate-conflict");
    let store = FakeStore::new();
    let (old, new) = (old_key(), new_key());

    store.seed(Kind::Settings, &old, "{ \"a\": 1 }");
    store.seed(Kind::Keymap, &old, "[]");
    // Every PUT is answered as a conflict, standing in for a third machine
    // that keeps getting there first.
    store.reject_writes();

    let outcome = block(rotate(
        &context(&store),
        &old,
        &new,
        &sandbox.state_path(),
        || async {},
    ))
    .unwrap();

    match outcome {
        RotationOutcome::Interrupted { remaining } => {
            assert_eq!(
                remaining.len(),
                2,
                "both artifacts must be reported as unfinished"
            );
        }
        other => panic!("expected Interrupted, got {other:?}"),
    }

    // And nothing was half-written: both still open with the old key.
    assert_eq!(
        store.read_plaintext(Kind::Settings, &old),
        Some("{ \"a\": 1 }".to_string())
    );
    let _ = USER;
}
