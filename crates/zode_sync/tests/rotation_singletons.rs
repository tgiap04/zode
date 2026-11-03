//! Rotating the recovery key must carry the keys sealed under it.
//!
//! `sharing-key` (the user's X25519 private key, `zode_team_vault`) and
//! `env-key` (the `.env` key, `zode_env_sync`) are sealed directly under the
//! recovery key. A rotation that skipped them would leave both unreadable, and
//! the only way back would be a new sharing key — which costs the user every
//! team envelope.
//!
//! The resources are built here exactly as their owning crates build them, so
//! these tests also pin that the rotation re-seals under the same AAD those
//! crates open with.

mod fake_store;

use std::sync::atomic::Ordering;

use fake_store::{FakeStore, Sandbox, block, context};
use zode_sync::rotate::{RotationOutcome, Slot, plan, rotate};
use zode_sync::{Dek, Kind, Resource, SyncCryptoError};

fn old_key() -> Dek {
    Dek::from_bytes([0x11; 32])
}

fn new_key() -> Dek {
    Dek::from_bytes([0x22; 32])
}

/// Key material is binary, not text: 0x80.. is never valid UTF-8 on its own.
fn sharing_secret() -> Vec<u8> {
    (0..32u8).map(|index| 0x80 | index).collect()
}

fn env_secret() -> Vec<u8> {
    vec![0xFE; 32]
}

fn sharing_slot() -> Resource {
    Resource::sharing_key()
}

/// As `zode_env_sync::env_dek::key_resource` builds it.
fn env_slot() -> Resource {
    Resource::env_singleton("env-key").unwrap()
}

/// Settings and keymap as text, both key slots as bytes, all under `dek`.
fn seed_everything(store: &FakeStore, dek: &Dek) {
    store.seed(Kind::Settings, dek, "{ \"a\": 1 }");
    store.seed(Kind::Keymap, dek, "[]");
    store.seed_at(&sharing_slot(), dek, &sharing_secret());
    store.seed_at(&env_slot(), dek, &env_secret());
}

fn opens(store: &FakeStore, resource: &Resource, dek: &Dek) -> Option<Vec<u8>> {
    store.open_at(resource, dek)?.ok()
}

/// Every stored slot opens under exactly one of the two keys, with its
/// original content. Returns how many are already under the new key.
fn assert_each_slot_opens_under_one_key(store: &FakeStore, old: &Dek, new: &Dek) -> usize {
    let expected: [(Resource, Vec<u8>); 4] = [
        (Resource::sync(Kind::Settings), b"{ \"a\": 1 }".to_vec()),
        (Resource::sync(Kind::Keymap), b"[]".to_vec()),
        (sharing_slot(), sharing_secret()),
        (env_slot(), env_secret()),
    ];
    let mut under_new = 0;
    for (resource, plaintext) in expected {
        match (opens(store, &resource, old), opens(store, &resource, new)) {
            (Some(bytes), None) => assert_eq!(bytes, plaintext, "{resource} under the old key"),
            (None, Some(bytes)) => {
                assert_eq!(bytes, plaintext, "{resource} under the new key");
                under_new += 1;
            }
            (old_opens, new_opens) => panic!(
                "{resource} must open under exactly one key: old {:?}, new {:?}",
                old_opens.is_some(),
                new_opens.is_some()
            ),
        }
    }
    under_new
}

#[test]
fn rotation_re_wraps_both_key_slots_byte_for_byte() {
    let sandbox = Sandbox::new("rotate-singletons-all");
    let store = FakeStore::new();
    let (old, new) = (old_key(), new_key());
    seed_everything(&store, &old);

    let outcome = block(rotate(
        &context(&store),
        &old,
        &new,
        &sandbox.state_path(),
        || async {},
    ))
    .unwrap();
    assert!(matches!(outcome, RotationOutcome::Complete), "{outcome:?}");

    assert_eq!(opens(&store, &sharing_slot(), &new), Some(sharing_secret()));
    assert_eq!(opens(&store, &env_slot(), &new), Some(env_secret()));

    // The old key no longer opens them: the fingerprint is the new one.
    for resource in [sharing_slot(), env_slot()] {
        assert!(
            matches!(
                store.open_at(&resource, &old),
                Some(Err(SyncCryptoError::KeyRotated { .. }))
            ),
            "{resource} must no longer open under the old key"
        );
    }

    let after = block(plan(&context(&store), &old, &new)).unwrap();
    assert!(after.pending.is_empty(), "{after:?}");
    assert!(after.done.contains(&Slot::SharingKey), "{after:?}");
    assert!(after.done.contains(&Slot::EnvKey), "{after:?}");
    assert_eq!(assert_each_slot_opens_under_one_key(&store, &old, &new), 4);
}

/// An account whose only sealed data is a key slot: the new key must still be
/// saved, or this machine would discard the only key that opens it.
#[test]
fn a_key_slot_alone_is_enough_to_save_the_new_key() {
    let sandbox = Sandbox::new("rotate-singletons-persist");
    let store = FakeStore::new();
    let (old, new) = (old_key(), new_key());
    store.seed_at(&sharing_slot(), &old, &sharing_secret());

    let mut writes_when_persisted = Vec::new();
    let writes = store.writes.clone();
    let outcome = block(rotate(
        &context(&store),
        &old,
        &new,
        &sandbox.state_path(),
        || {
            writes_when_persisted.push(writes.load(Ordering::SeqCst));
            async {}
        },
    ))
    .unwrap();

    assert!(matches!(outcome, RotationOutcome::Complete), "{outcome:?}");
    assert_eq!(
        writes_when_persisted,
        vec![1],
        "saved once, right after the key slot landed under the new key"
    );
    assert_eq!(opens(&store, &sharing_slot(), &new), Some(sharing_secret()));
}

#[test]
fn a_missing_key_slot_is_skipped() {
    let sandbox = Sandbox::new("rotate-singletons-missing");
    let store = FakeStore::new();
    let (old, new) = (old_key(), new_key());
    store.seed(Kind::Settings, &old, "{ \"a\": 1 }");
    store.seed_at(&env_slot(), &old, &env_secret());

    let planned = block(plan(&context(&store), &old, &new)).unwrap();
    assert!(planned.absent.contains(&Slot::SharingKey), "{planned:?}");
    assert!(planned.pending.contains(&Slot::EnvKey), "{planned:?}");

    let outcome = block(rotate(
        &context(&store),
        &old,
        &new,
        &sandbox.state_path(),
        || async {},
    ))
    .unwrap();

    assert!(matches!(outcome, RotationOutcome::Complete), "{outcome:?}");
    assert_eq!(store.writes.load(Ordering::SeqCst), 2);
    assert_eq!(
        store.revision_of(&sharing_slot()),
        None,
        "a missing slot is not created"
    );
    assert_eq!(opens(&store, &env_slot(), &new), Some(env_secret()));
}

/// Sealed under a third key: rotation cannot open it, so it must not touch it.
#[test]
fn a_key_slot_neither_key_opens_is_left_untouched() {
    let sandbox = Sandbox::new("rotate-singletons-stranger");
    let store = FakeStore::new();
    let (old, new) = (old_key(), new_key());
    let stranger = Dek::from_bytes([0x77; 32]);
    let revision = store.seed_at(&sharing_slot(), &stranger, &sharing_secret());

    let planned = block(plan(&context(&store), &old, &new)).unwrap();
    assert!(!planned.pending.contains(&Slot::SharingKey), "{planned:?}");
    assert!(!planned.done.contains(&Slot::SharingKey), "{planned:?}");
    assert!(!planned.absent.contains(&Slot::SharingKey), "{planned:?}");

    let outcome = block(rotate(
        &context(&store),
        &old,
        &new,
        &sandbox.state_path(),
        || async {},
    ))
    .unwrap();

    assert!(matches!(outcome, RotationOutcome::Complete), "{outcome:?}");
    assert_eq!(store.revision_of(&sharing_slot()), Some(revision));
    assert_eq!(
        opens(&store, &sharing_slot(), &stranger),
        Some(sharing_secret())
    );
}

/// The connection drops after each possible number of successful writes. At
/// every cut point, each slot opens under the old key or the new one with its
/// content intact, and a second run with the same two keys finishes the job.
#[test]
fn a_rotation_cut_off_at_any_point_strands_nothing_and_resumes() {
    for cut_after in 0..4 {
        let sandbox = Sandbox::new(&format!("rotate-singletons-cut-{cut_after}"));
        let store = FakeStore::new();
        let (old, new) = (old_key(), new_key());
        seed_everything(&store, &old);
        store.fail_writes_after(cut_after);

        let mut persisted = 0;
        let first = block(rotate(
            &context(&store),
            &old,
            &new,
            &sandbox.state_path(),
            || {
                persisted += 1;
                async {}
            },
        ));
        assert!(first.is_err(), "cut after {cut_after}: {first:?}");
        assert_eq!(
            assert_each_slot_opens_under_one_key(&store, &old, &new),
            cut_after,
            "cut after {cut_after}"
        );
        // The two key slots go first; the new key is kept only once both are
        // under it. A cut between them leaves the first under the new key
        // (putting it back fails too) for the retry, with the same pair, to
        // count as done.
        assert_eq!(
            persisted,
            usize::from(cut_after >= 2),
            "cut after {cut_after}: the new key is saved once every key slot is under it"
        );

        store.heal();
        let second = block(rotate(
            &context(&store),
            &old,
            &new,
            &sandbox.state_path(),
            || async {},
        ))
        .unwrap();
        assert!(
            matches!(second, RotationOutcome::Complete),
            "cut after {cut_after}: {second:?}"
        );
        assert_eq!(
            assert_each_slot_opens_under_one_key(&store, &old, &new),
            4,
            "cut after {cut_after}"
        );
    }
}

/// Another machine writes the key slot mid-rotation: the write is refused
/// rather than clobbering it, the slot stays under the old key, and since key
/// slots move all or none before any artifact, nothing else moves either.
#[test]
fn a_conflict_on_a_key_slot_is_reported_and_leaves_it_under_the_old_key() {
    let sandbox = Sandbox::new("rotate-singletons-conflict");
    let store = FakeStore::new();
    let (old, new) = (old_key(), new_key());
    seed_everything(&store, &old);
    let revision = store.revision_of(&sharing_slot());
    store.conflict_on(zode_sync::SHARING_KEY_RESOURCE);

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
                remaining,
                vec![
                    Slot::SharingKey,
                    Slot::EnvKey,
                    Slot::Artifact(Kind::Settings),
                    Slot::Artifact(Kind::Keymap),
                ]
            );
        }
        other => panic!("expected Interrupted, got {other:?}"),
    }
    assert_eq!(store.revision_of(&sharing_slot()), revision);
    assert_eq!(opens(&store, &sharing_slot(), &old), Some(sharing_secret()));
    assert_eq!(opens(&store, &env_slot(), &old), Some(env_secret()));
    assert_eq!(assert_each_slot_opens_under_one_key(&store, &old, &new), 0);

    store.heal();
    let second = block(rotate(
        &context(&store),
        &old,
        &new,
        &sandbox.state_path(),
        || async {},
    ))
    .unwrap();
    assert!(matches!(second, RotationOutcome::Complete), "{second:?}");
    assert_eq!(opens(&store, &sharing_slot(), &new), Some(sharing_secret()));
}

#[test]
fn every_slot_addresses_the_resource_its_owner_seals_under() {
    assert_eq!(
        Slot::SharingKey.resource().unwrap(),
        Resource::sharing_key()
    );
    assert_eq!(Slot::EnvKey.resource().unwrap(), env_slot());
    assert_eq!(
        Slot::Artifact(Kind::Keymap).resource().unwrap(),
        Resource::sync(Kind::Keymap)
    );
    assert_eq!(Slot::ALL.len(), 5);
}

/// The server answers 402 to `env-key` (a Free account, before the backend
/// let a re-wrap through). `sharing-key` had already moved: it is put back,
/// the new key is never kept, and every slot still opens under the old one.
#[test]
fn a_plan_refusal_on_a_key_slot_leaves_every_slot_under_the_old_key() {
    let sandbox = Sandbox::new("rotate-singletons-402");
    let store = FakeStore::new();
    let (old, new) = (old_key(), new_key());
    seed_everything(&store, &old);
    store.refuse_by_plan_on("env-key");

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
    ));
    assert!(
        matches!(
            outcome,
            Err(zode_sync::SyncError::Client(
                zode_sync::client::ClientError::PlanLimit(_)
            ))
        ),
        "a 402 is never a success: {outcome:?}"
    );
    assert_eq!(
        persisted, 0,
        "the new key was kept with a key slot not under it"
    );
    assert_eq!(assert_each_slot_opens_under_one_key(&store, &old, &new), 0);
}

/// Key slots go first, and the new key is kept only once every key slot is
/// under it: a conflict on one leaves no artifact moved and no key kept.
#[test]
fn the_key_slots_move_before_any_artifact() {
    let sandbox = Sandbox::new("rotate-singletons-order");
    let store = FakeStore::new();
    let (old, new) = (old_key(), new_key());
    seed_everything(&store, &old);
    store.conflict_on("env-key");

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
    assert!(
        matches!(outcome, RotationOutcome::Interrupted { .. }),
        "{outcome:?}"
    );
    assert_eq!(persisted, 0);
    assert_eq!(
        store.read_plaintext(Kind::Settings, &old),
        Some("{ \"a\": 1 }".to_string()),
        "an artifact moved before the key slots did"
    );
    assert_eq!(assert_each_slot_opens_under_one_key(&store, &old, &new), 0);
}
