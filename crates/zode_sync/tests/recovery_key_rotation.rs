//! Replacing the recovery key through `SyncSession`, where the keychain is
//! part of the story: which key this machine keeps, and what a second attempt
//! continues with.
//!
//! The account here holds only the two key slots (`sharing-key`, `env-key`):
//! they are what a stranded rotation costs most, and they keep the test away
//! from the local sync record the artifacts write to.

mod fake_store;

use std::sync::Arc;

use fake_store::{FakeStore, MemoryKeychain, USER};
use gpui::{AppContext as _, Entity, TestAppContext};
use zode_account::{Account, AccountStatus, AccountUser};
use zode_sync::{Dek, KeyRotation, Resource, SyncSession};

const OLD: [u8; 32] = [0x11; 32];
const SYNC_KEY: &str = "zode://sync-key";
const ROTATION: &str = "zode://sync-key-rotation";

fn sharing_secret() -> Vec<u8> {
    (0..32u8).map(|index| 0x80 | index).collect()
}

fn env_secret() -> Vec<u8> {
    vec![0xFE; 32]
}

fn env_slot() -> Resource {
    Resource::env_singleton("env-key").unwrap()
}

fn seed_key_slots(store: &FakeStore) {
    let old = Dek::from_bytes(OLD);
    store.seed_at(&Resource::sharing_key(), &old, &sharing_secret());
    store.seed_at(&env_slot(), &old, &env_secret());
}

/// Both key slots open under `dek`, with their content intact.
fn both_open_under(store: &FakeStore, dek: &Dek) -> bool {
    store
        .open_at(&Resource::sharing_key(), dek)
        .and_then(Result::ok)
        == Some(sharing_secret())
        && store.open_at(&env_slot(), dek).and_then(Result::ok) == Some(env_secret())
}

fn stored_kid(keychain: &MemoryKeychain) -> Option<[u8; zode_sync::KID_LEN]> {
    let bytes: [u8; 32] = keychain.get(SYNC_KEY)?.try_into().ok()?;
    Some(Dek::from_bytes(bytes).kid())
}

fn session(
    cx: &mut TestAppContext,
    store: &FakeStore,
    keychain: Arc<MemoryKeychain>,
) -> Entity<SyncSession> {
    let http_client = store.client();
    let account = cx.new(|_| {
        let mut account = Account::for_test_with(
            AccountStatus::SignedIn(AccountUser {
                id: USER.into(),
                email: "ada@example.com".into(),
                name: None,
                avatar_url: None,
            }),
            http_client,
            keychain,
        );
        account.set_tokens_for_test();
        account
    });
    let session = cx.new(|_| SyncSession::new(account));
    let loaded = session.update(cx, |session, cx| session.load_key(cx));
    cx.foreground_executor().block_test(loaded);
    session
}

fn rotate(cx: &mut TestAppContext, session: &Entity<SyncSession>) -> KeyRotation {
    let task = session.update(cx, |session, cx| session.rotate_key(cx));
    let result = cx.foreground_executor().block_test(task);
    cx.run_until_parked();
    result.expect("a rotation that started answers with an outcome")
}

/// The server refuses `env-key` with 402 after `sharing-key` already moved.
/// Nothing may be left needing a key this machine does not keep.
#[gpui::test]
fn a_plan_refusal_midway_keeps_the_old_key_and_everything_readable(cx: &mut TestAppContext) {
    let store = FakeStore::new();
    seed_key_slots(&store);
    store.refuse_by_plan_on("env-key");
    let keychain = MemoryKeychain::with_recovery_key(OLD);
    let session = session(cx, &store, keychain.clone());

    let outcome = rotate(cx, &session);
    assert!(
        matches!(
            &outcome,
            KeyRotation::Interrupted { adopted: false, reason, .. } if !reason.is_empty()
        ),
        "a 402 must stop the rotation before the new key is adopted: {outcome:?}"
    );
    assert_eq!(
        session.read_with(cx, |s, _| s.reveal_recovery_key()),
        Some(zode_sync::recovery_key::encode(&Dek::from_bytes(OLD))),
        "this machine stopped using the old key"
    );
    assert!(
        keychain.get(ROTATION).is_some(),
        "the pair is kept for a retry"
    );
    assert_eq!(
        keychain.get(SYNC_KEY),
        Some(OLD.to_vec()),
        "the old key was replaced while a key slot still needed it"
    );
    assert!(
        both_open_under(&store, &Dek::from_bytes(OLD)),
        "a key slot is no longer under the key this machine keeps"
    );
}

/// The connection drops after the first key slot moved. A second attempt
/// continues with the same new key, and ends with every slot under it.
#[gpui::test]
fn a_retry_after_an_interruption_completes_and_strands_no_slot(cx: &mut TestAppContext) {
    let store = FakeStore::new();
    seed_key_slots(&store);
    store.fail_writes_after(1);
    let keychain = MemoryKeychain::with_recovery_key(OLD);
    let session = session(cx, &store, keychain.clone());

    let first = rotate(cx, &session);
    let KeyRotation::Interrupted {
        phrase: first_phrase,
        ..
    } = first
    else {
        panic!("the cut must interrupt: {first:?}");
    };
    store.heal();
    let second = rotate(cx, &session);
    let KeyRotation::Complete { phrase } = second else {
        panic!("the retry must complete: {second:?}");
    };
    assert_eq!(
        phrase, first_phrase,
        "the retry made another key instead of continuing"
    );

    let new = zode_sync::recovery_key::decode(&phrase).unwrap();
    assert!(
        both_open_under(&store, &new),
        "a key slot was stranded under a key nobody keeps"
    );
    assert_eq!(
        stored_kid(&keychain),
        Some(new.kid()),
        "the new key is kept"
    );
    assert_eq!(
        keychain.get(ROTATION),
        None,
        "a finished rotation is forgotten"
    );
}
