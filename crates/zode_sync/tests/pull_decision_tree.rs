//! The five answers a pull can give, and the four a push can give, driven
//! against a fake sync store that enforces the same preconditions the real one
//! does.
//!
//! Written against a fake server rather than mocked at the function boundary
//! because the interesting part IS the interaction: which header goes out,
//! what a 409 carries back, and what the client does with it.

mod fake_store;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use fake_store::{API, FakeStore, Sandbox, USER, block, context, key};
use http_client::{FakeHttpClient, HttpClient};
use zode_account::ApiCredential;
use zode_sync::state::SyncState;
use zode_sync::sync::{SyncContext, apply_remote, overwrite_remote, pull, push};
use zode_sync::{Kind, PullOutcome, PushOutcome, SyncCryptoError};

#[test]
fn a_kind_never_pushed_reports_local_only() {
    let sandbox = Sandbox::new("local-only");
    let store = FakeStore::new();
    sandbox.write_local(Kind::Settings, "{ \"a\": 1 }");

    let outcome = block(pull(
        &context(&store),
        &key(),
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ))
    .unwrap();

    assert!(matches!(outcome, PullOutcome::LocalOnly), "{outcome:?}");
}

#[test]
fn identical_content_reports_up_to_date_and_records_the_revision() {
    let sandbox = Sandbox::new("up-to-date");
    let store = FakeStore::new();
    let dek = key();
    sandbox.write_local(Kind::Settings, "{ \"same\": true }");
    let revision = store.seed(Kind::Settings, &dek, "{ \"same\": true }");

    let outcome = block(pull(
        &context(&store),
        &dek,
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ))
    .unwrap();

    assert!(matches!(outcome, PullOutcome::UpToDate), "{outcome:?}");
    // Agreement must be recorded, or the next push has no precondition to use.
    let state = SyncState::load(&sandbox.state_path());
    assert_eq!(state.get(Kind::Settings).unwrap().revision, revision);
}

#[test]
fn an_untouched_local_file_reports_remote_newer() {
    let sandbox = Sandbox::new("remote-newer");
    let store = FakeStore::new();
    let dek = key();

    // First sync: both sides agree, and that agreement is recorded.
    sandbox.write_local(Kind::Settings, "{ \"v\": 1 }");
    store.seed(Kind::Settings, &dek, "{ \"v\": 1 }");
    block(pull(
        &context(&store),
        &dek,
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ))
    .unwrap();

    // Another machine pushes. This one has not been edited.
    store.seed(Kind::Settings, &dek, "{ \"v\": 2 }");

    let outcome = block(pull(
        &context(&store),
        &dek,
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ))
    .unwrap();

    match outcome {
        PullOutcome::RemoteNewer(divergence) => {
            assert!(
                divergence.diff.unified.contains("+{ \"v\": 2 }"),
                "{}",
                divergence.diff.unified
            );
            assert_eq!(divergence.remote, "{ \"v\": 2 }");
        }
        other => panic!("expected RemoteNewer, got {other:?}"),
    }
}

#[test]
fn edits_on_both_sides_report_a_conflict() {
    let sandbox = Sandbox::new("conflict");
    let store = FakeStore::new();
    let dek = key();

    sandbox.write_local(Kind::Settings, "{ \"v\": 1 }");
    store.seed(Kind::Settings, &dek, "{ \"v\": 1 }");
    block(pull(
        &context(&store),
        &dek,
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ))
    .unwrap();

    // Both sides move on.
    sandbox.write_local(Kind::Settings, "{ \"v\": \"local\" }");
    store.seed(Kind::Settings, &dek, "{ \"v\": \"remote\" }");

    let outcome = block(pull(
        &context(&store),
        &dek,
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ))
    .unwrap();

    assert!(matches!(outcome, PullOutcome::Conflict(_)), "{outcome:?}");
}

/// Invariant 5, measured on the file rather than read from the code.
#[test]
fn a_wrong_key_never_touches_the_local_file() {
    let sandbox = Sandbox::new("wrong-key");
    let store = FakeStore::new();
    sandbox.write_local(Kind::Settings, "{ \"mine\": true }");
    store.seed_with_other_key(Kind::Settings, "{ \"theirs\": true }");

    let before = sandbox.read_local(Kind::Settings).unwrap();

    let outcome = block(pull(
        &context(&store),
        &key(),
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ))
    .unwrap();

    match outcome {
        PullOutcome::KeyMismatch(SyncCryptoError::KeyRotated { .. }) => {}
        other => panic!("expected KeyMismatch/KeyRotated, got {other:?}"),
    }

    assert_eq!(
        sandbox.read_local(Kind::Settings).unwrap(),
        before,
        "the local file must be byte-identical after a failed decryption",
    );
    assert_eq!(
        sandbox.read_backup(Kind::Settings),
        None,
        "nothing should have been backed up"
    );
}

#[test]
fn applying_the_remote_backs_up_the_old_file_first() {
    let sandbox = Sandbox::new("backup");
    sandbox.write_local(Kind::Settings, "{ \"old\": true }");

    apply_remote(
        &sandbox.artifact(Kind::Settings),
        "{ \"new\": true }",
        "rev-9".into(),
        &sandbox.state_path(),
    )
    .unwrap();

    assert_eq!(
        sandbox.read_local(Kind::Settings).unwrap(),
        "{ \"new\": true }"
    );
    // Invariant 8: the copy holds what was replaced, not what replaced it.
    assert_eq!(
        sandbox.read_backup(Kind::Settings).unwrap(),
        "{ \"old\": true }"
    );
    assert_eq!(
        SyncState::load(&sandbox.state_path())
            .get(Kind::Settings)
            .unwrap()
            .revision,
        "rev-9",
    );
}

#[test]
fn a_first_push_creates_and_a_second_replaces() {
    let sandbox = Sandbox::new("push");
    let store = FakeStore::new();
    let dek = key();
    sandbox.write_local(Kind::Settings, "{ \"v\": 1 }");

    let first = block(push(
        &context(&store),
        &dek,
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ))
    .unwrap();
    assert!(matches!(first, PushOutcome::Stored { .. }), "{first:?}");

    sandbox.write_local(Kind::Settings, "{ \"v\": 2 }");
    let second = block(push(
        &context(&store),
        &dek,
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ))
    .unwrap();
    assert!(matches!(second, PushOutcome::Stored { .. }), "{second:?}");
}

#[test]
fn a_push_against_a_moved_revision_conflicts_instead_of_overwriting() {
    let sandbox = Sandbox::new("push-conflict");
    let store = FakeStore::new();
    let dek = key();

    sandbox.write_local(Kind::Settings, "{ \"v\": 1 }");
    block(push(
        &context(&store),
        &dek,
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ))
    .unwrap();

    // Another machine writes, so this machine's recorded revision is stale.
    store.seed(
        Kind::Settings,
        &dek,
        "{ \"v\": \"from the other machine\" }",
    );

    sandbox.write_local(Kind::Settings, "{ \"v\": \"mine\" }");
    let outcome = block(push(
        &context(&store),
        &dek,
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ))
    .unwrap();

    match outcome {
        PushOutcome::Conflict(divergence) => {
            assert_eq!(divergence.remote, "{ \"v\": \"from the other machine\" }");
        }
        other => panic!("expected Conflict, got {other:?}"),
    }
}

#[test]
fn resolving_a_conflict_towards_local_uses_the_conflicting_revision() {
    let sandbox = Sandbox::new("resolve-local");
    let store = FakeStore::new();
    let dek = key();

    sandbox.write_local(Kind::Settings, "{ \"v\": 1 }");
    block(push(
        &context(&store),
        &dek,
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ))
    .unwrap();
    store.seed(Kind::Settings, &dek, "{ \"v\": \"theirs\" }");
    sandbox.write_local(Kind::Settings, "{ \"v\": \"mine\" }");

    let PushOutcome::Conflict(divergence) = block(push(
        &context(&store),
        &dek,
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ))
    .unwrap() else {
        panic!("expected a conflict to resolve");
    };

    let resolved = block(overwrite_remote(
        &context(&store),
        &dek,
        &sandbox.artifact(Kind::Settings),
        &divergence.revision,
        &sandbox.state_path(),
    ))
    .unwrap();
    assert!(
        matches!(resolved, PushOutcome::Stored { .. }),
        "{resolved:?}"
    );

    // And the server now holds the local content.
    let confirmed = block(pull(
        &context(&store),
        &dek,
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ))
    .unwrap();
    assert!(matches!(confirmed, PullOutcome::UpToDate), "{confirmed:?}");
}

#[test]
fn pushing_with_no_local_file_does_nothing() {
    let sandbox = Sandbox::new("nothing");
    let store = FakeStore::new();

    let outcome = block(push(
        &context(&store),
        &key(),
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ))
    .unwrap();
    assert!(matches!(outcome, PushOutcome::NothingToPush), "{outcome:?}");
    assert_eq!(
        store.requests.load(Ordering::SeqCst),
        0,
        "nothing to push means nothing to send"
    );
}

#[test]
fn an_unreachable_service_fails_without_writing_anything() {
    let sandbox = Sandbox::new("offline");
    sandbox.write_local(Kind::Settings, "{ \"mine\": true }");

    let dead: Arc<dyn HttpClient> =
        FakeHttpClient::create(|_| async { Err(anyhow::anyhow!("the network is down")) });
    let context = SyncContext {
        http_client: dead,
        api_url: API.into(),
        credential: ApiCredential {
            access_token: "at-1".into(),
            user_id: USER.into(),
        },
    };

    let result = block(pull(
        &context,
        &key(),
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ));
    assert!(
        result.is_err(),
        "an unreachable service must surface as an error"
    );
    assert_eq!(
        sandbox.read_local(Kind::Settings).unwrap(),
        "{ \"mine\": true }"
    );
    assert_eq!(sandbox.read_backup(Kind::Settings), None);
}

#[test]
fn a_corrupt_sync_state_does_not_stop_a_pull() {
    let sandbox = Sandbox::new("corrupt-state");
    let store = FakeStore::new();
    let dek = key();
    sandbox.write_local(Kind::Settings, "{ \"v\": \"local\" }");
    store.seed(Kind::Settings, &dek, "{ \"v\": \"remote\" }");
    std::fs::write(sandbox.state_path(), "{ not json").unwrap();

    let outcome = block(pull(
        &context(&store),
        &dek,
        &sandbox.artifact(Kind::Settings),
        &sandbox.state_path(),
    ))
    .unwrap();

    // Unreadable state means "never synced", which makes this a conflict — the
    // cautious answer, since it asks rather than overwrites.
    assert!(matches!(outcome, PullOutcome::Conflict(_)), "{outcome:?}");
}

/// Phase-11: `keymap` reuses the whole path, so what is worth asserting is the
/// property that makes syncing whole files worthwhile — comments and
/// formatting survive, because nothing between here and the server ever parses
/// the JSON.
#[test]
fn a_keymap_round_trips_with_its_comments_intact() {
    let sandbox = Sandbox::new("keymap");
    let store = FakeStore::new();
    let dek = key();

    let original = "// my bindings\n[\n  {\n    // reformat\n    \"bindings\": { \"cmd-b\": \"editor::Format\" }\n  }\n]\n";
    sandbox.write_local(Kind::Keymap, original);

    block(push(
        &context(&store),
        &dek,
        &sandbox.artifact(Kind::Keymap),
        &sandbox.state_path(),
    ))
    .unwrap();

    // Wipe the local copy and take it back from the server.
    sandbox.write_local(Kind::Keymap, "[]\n");
    let outcome = block(pull(
        &context(&store),
        &dek,
        &sandbox.artifact(Kind::Keymap),
        &sandbox.state_path(),
    ))
    .unwrap();

    match outcome {
        PullOutcome::RemoteNewer(divergence) | PullOutcome::Conflict(divergence) => {
            assert_eq!(
                divergence.remote, original,
                "every byte must survive, comments included",
            );
        }
        other => panic!("expected a divergence, got {other:?}"),
    }
}

/// The extension list travels through the same crypto and the same
/// preconditions, without ever touching a file.
#[test]
fn an_extension_list_syncs_without_a_file() {
    use zode_sync::extensions;
    use zode_sync::sync::{pull_content, push_content};

    let sandbox = Sandbox::new("extensions");
    let store = FakeStore::new();
    let dek = key();

    let installed = vec!["a/one".to_string(), "b/two".to_string()];
    let rendered = extensions::render(installed);

    let pushed = block(push_content(
        &context(&store),
        &dek,
        Kind::Extensions,
        &rendered,
        &sandbox.state_path(),
    ))
    .unwrap();
    assert!(matches!(pushed, PushOutcome::Stored { .. }), "{pushed:?}");

    // A second machine with one of the two installed.
    let elsewhere = extensions::render(vec!["a/one".to_string()]);
    let outcome = block(pull_content(
        &context(&store),
        &dek,
        Kind::Extensions,
        &elsewhere,
        &sandbox.state_path(),
    ))
    .unwrap();

    match outcome {
        PullOutcome::RemoteNewer(d) | PullOutcome::Conflict(d) => {
            let stored = extensions::parse(&d.remote).expect("the stored list must parse");
            let comparison = extensions::compare(&["a/one".to_string()], &stored);
            assert_eq!(comparison.missing, vec!["b/two".to_string()]);
            assert!(comparison.extra.is_empty());
        }
        other => panic!("expected a divergence, got {other:?}"),
    }

    // No file was created for it — the list is derived, not stored on disk.
    assert!(!sandbox.dir.join("extensions.json").exists());
}
