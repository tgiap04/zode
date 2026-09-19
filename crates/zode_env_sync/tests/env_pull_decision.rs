//! The six answers a pull can give and the four a push can give, driven
//! against a fake store that honours the same preconditions the real one does.
//!
//! Written against a fake server rather than mocked at the function boundary
//! because the interesting part IS the interaction: which header goes out,
//! what a 409 carries back, and — the two that are new here — what happens
//! when the server answers with a blob from the wrong slot or an older
//! version of the right one.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use http_client::{FakeHttpClient, HttpClient, Response};
use zode_account::ApiCredential;
use zode_env_sync::env_state::EnvState;
use zode_env_sync::env_sync::{PullOutcome, PushOutcome, apply_remote, pull, push};
use zode_env_sync::{EntryId, EnvDek, seal};
use zode_sync::sync::SyncContext;
use zode_sync::{DEK_LEN, to_blob};

const USER: &str = "68b1f0c2a4d3e5f60718293a";
const API: &str = "https://api.example.invalid/api";

fn key() -> EnvDek {
    EnvDek::from_bytes([0x33; DEK_LEN])
}

fn entry() -> EntryId {
    EntryId::parse("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1").expect("a valid fixture id")
}

fn block<T>(future: impl std::future::Future<Output = T>) -> T {
    futures::executor::block_on(future)
}

struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("zode-env-pull-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the sandbox must be creatable");
        Self { dir }
    }

    fn env_path(&self) -> PathBuf {
        self.dir.join("project/.env")
    }

    fn state_path(&self) -> PathBuf {
        self.dir.join("env_state.json")
    }

    fn backups(&self) -> PathBuf {
        self.dir.join("backups")
    }

    fn write_local(&self, content: &str) {
        let path = self.env_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn read_local(&self) -> Option<String> {
        std::fs::read_to_string(self.env_path()).ok()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn header(
    request: &http_client::http::Request<http_client::AsyncBody>,
    name: &str,
) -> Option<String> {
    request
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn document_json(blob: &str, revision: &str, error: Option<&str>) -> String {
    let prefix = error.map_or(String::new(), |slug| format!(r#""error":"{slug}","#));
    format!(
        r#"{{{prefix}"blob":"{blob}","revision":"{revision}","updatedAt":"2026-09-17T00:00:00.000Z","byteLength":1}}"#
    )
}

#[derive(Clone, Default)]
struct FakeStore {
    documents: Arc<Mutex<HashMap<String, (String, String)>>>,
    revisions: Arc<AtomicUsize>,
}

impl FakeStore {
    /// Seeds the store as if another machine had pushed this content.
    fn seed(&self, dek: &EnvDek, seq: u64, plaintext: &str) -> String {
        let resource = seal::entry_resource(&entry()).unwrap();
        let envelope = seal::seal(dek, USER, &resource, seq, plaintext.as_bytes()).unwrap();
        let revision = format!("rev-{}", self.revisions.fetch_add(1, Ordering::SeqCst));
        self.documents.lock().unwrap().insert(
            entry().as_hex(),
            (to_blob(&envelope).unwrap(), revision.clone()),
        );
        revision
    }

    fn client(&self) -> Arc<dyn HttpClient> {
        let documents = self.documents.clone();
        let revisions = self.revisions.clone();

        FakeHttpClient::create(move |mut request| {
            let documents = documents.clone();
            let revisions = revisions.clone();
            let method = request.method().to_string();
            let id = request
                .uri()
                .path()
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_string();
            let if_match = header(&request, "If-Match");
            let if_none_match = header(&request, "If-None-Match");

            async move {
                // Read the body BEFORE taking the lock. Both halves of that
                // order matter: awaiting while holding a `MutexGuard` is a
                // deadlock waiting for a second task, and driving the read
                // with a nested `block_on` panics inside the executor already
                // driving this future.
                let mut body_bytes = Vec::new();
                if method == "PUT" {
                    use futures::AsyncReadExt as _;
                    let _ = request.body_mut().read_to_end(&mut body_bytes).await;
                }

                let mut store = documents.lock().expect("the fake store lock");
                let current = store.get(&id).cloned();

                let (status, body) = match method.as_str() {
                    "GET" => match current {
                        Some((blob, revision)) => (200, document_json(&blob, &revision, None)),
                        None => (404, r#"{"error":"not_found"}"#.to_string()),
                    },
                    "PUT" => {
                        let allowed = match (&if_none_match, &if_match, &current) {
                            (Some(star), _, None) if star == "*" => true,
                            (Some(star), _, Some(_)) if star == "*" => false,
                            (_, Some(claimed), Some((_, revision))) => claimed == revision,
                            _ => false,
                        };

                        if allowed {
                            let parsed: serde_json::Value =
                                serde_json::from_slice(&body_bytes).unwrap_or_default();
                            let blob = parsed["blob"].as_str().unwrap_or_default().to_string();
                            let revision =
                                format!("rev-{}", revisions.fetch_add(1, Ordering::SeqCst));
                            store.insert(id.clone(), (blob, revision.clone()));
                            (
                                200,
                                format!(
                                    r#"{{"revision":"{revision}","updatedAt":"2026-09-17T00:00:00.000Z"}}"#
                                ),
                            )
                        } else if let Some((blob, revision)) = current {
                            (
                                409,
                                document_json(&blob, &revision, Some("revision_mismatch")),
                            )
                        } else {
                            (404, r#"{"error":"not_found"}"#.to_string())
                        }
                    }
                    _ => {
                        store.remove(&id);
                        (200, r#"{"ok":true}"#.to_string())
                    }
                };

                Ok(Response::builder()
                    .status(status)
                    .body(http_client::AsyncBody::from(body))
                    .expect("a well-formed fake response"))
            }
        })
    }

    fn context(&self) -> SyncContext {
        SyncContext {
            http_client: self.client(),
            api_url: API.to_string(),
            credential: ApiCredential {
                user_id: USER.into(),
                access_token: "test-token".to_string(),
            },
        }
    }
}

#[test]
fn an_entry_never_pushed_reports_local_only() {
    let sandbox = Sandbox::new("local-only");
    sandbox.write_local("A=1\n");
    let store = FakeStore::default();

    let outcome = block(pull(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();

    assert!(matches!(outcome, PullOutcome::LocalOnly), "{outcome:?}");
}

#[test]
fn identical_content_reports_up_to_date_and_records_the_version() {
    let sandbox = Sandbox::new("up-to-date");
    sandbox.write_local("A=1\n");
    let store = FakeStore::default();
    store.seed(&key(), 5, "A=1\n");

    let outcome = block(pull(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();

    assert!(matches!(outcome, PullOutcome::UpToDate), "{outcome:?}");
    assert_eq!(
        EnvState::load(&sandbox.state_path()).seen_seq(&entry()),
        Some(5),
        "agreement must record the version, or the rollback check has nothing to compare",
    );
}

#[test]
fn an_untouched_local_file_reports_remote_newer() {
    let sandbox = Sandbox::new("remote-newer");
    sandbox.write_local("A=1\n");
    let store = FakeStore::default();
    store.seed(&key(), 1, "A=1\n");

    // Agree first, so the recorded hash matches what is on disk.
    block(pull(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();

    store.seed(&key(), 2, "A=2\n");
    let outcome = block(pull(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();

    match outcome {
        PullOutcome::RemoteNewer(divergence) => {
            assert_eq!(divergence.remote, "A=2\n");
            assert_eq!(divergence.seq, 2);
        }
        other => panic!("expected RemoteNewer, got {other:?}"),
    }
}

#[test]
fn edits_on_both_sides_report_a_conflict() {
    let sandbox = Sandbox::new("conflict");
    sandbox.write_local("A=1\n");
    let store = FakeStore::default();
    store.seed(&key(), 1, "A=1\n");
    block(pull(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();

    sandbox.write_local("A=local\n");
    store.seed(&key(), 2, "A=remote\n");

    let outcome = block(pull(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();

    assert!(matches!(outcome, PullOutcome::Conflict(_)), "{outcome:?}");
    assert_eq!(
        sandbox.read_local().as_deref(),
        Some("A=local\n"),
        "deciding must not write",
    );
}

#[test]
fn the_wrong_key_never_touches_the_local_file() {
    let sandbox = Sandbox::new("wrong-key");
    sandbox.write_local("A=mine\n");
    let store = FakeStore::default();
    store.seed(&EnvDek::from_bytes([0x99; DEK_LEN]), 1, "A=theirs\n");

    let outcome = block(pull(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();

    assert!(
        matches!(outcome, PullOutcome::KeyMismatch(_)),
        "{outcome:?}"
    );
    assert_eq!(sandbox.read_local().as_deref(), Some("A=mine\n"));
}

#[test]
fn a_replayed_older_version_is_refused_and_writes_nothing() {
    // The attack `seq` exists for: the server hands back a blob that is
    // authentic, decrypts cleanly, and holds a credential the user revoked.
    let sandbox = Sandbox::new("rollback");
    sandbox.write_local("API_KEY=current\n");
    let store = FakeStore::default();

    store.seed(&key(), 9, "API_KEY=current\n");
    block(pull(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();

    store.seed(&key(), 4, "API_KEY=revoked\n");
    let outcome = block(pull(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();

    match outcome {
        PullOutcome::Rollback { seen, got } => assert_eq!((seen, got), (9, 4)),
        other => panic!("expected Rollback, got {other:?}"),
    }
    assert_eq!(sandbox.read_local().as_deref(), Some("API_KEY=current\n"));
}

#[test]
fn applying_the_remote_backs_the_file_up_outside_the_project() {
    let sandbox = Sandbox::new("apply");
    sandbox.write_local("A=old\n");

    apply_remote(
        &entry(),
        &sandbox.env_path(),
        &sandbox.backups(),
        "A=new\n",
        "rev-1".into(),
        3,
        &sandbox.state_path(),
    )
    .unwrap();

    assert_eq!(sandbox.read_local().as_deref(), Some("A=new\n"));

    let project = sandbox.env_path().parent().unwrap().to_path_buf();
    let backups: Vec<PathBuf> = walk(&sandbox.backups());
    assert_eq!(backups.len(), 1, "exactly one copy should have been kept");
    assert!(
        !backups[0].starts_with(&project),
        "{} is inside the project — that is how a .env reaches git",
        backups[0].display()
    );
    assert_eq!(std::fs::read_to_string(&backups[0]).unwrap(), "A=old\n");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(sandbox.env_path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "a pulled .env must not be world-readable"
        );
    }
}

fn walk(root: &std::path::Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}

#[test]
fn a_first_push_creates_and_a_second_replaces() {
    let sandbox = Sandbox::new("push");
    sandbox.write_local("A=1\n");
    let store = FakeStore::default();

    let created = block(push(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();
    match created {
        PushOutcome::Stored { seq, .. } => assert_eq!(seq, 1, "the first version is 1"),
        other => panic!("expected Stored, got {other:?}"),
    }

    sandbox.write_local("A=2\n");
    let replaced = block(push(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();
    match replaced {
        PushOutcome::Stored { seq, .. } => {
            assert_eq!(
                seq, 2,
                "the counter must climb, or a replay is undetectable"
            )
        }
        other => panic!("expected Stored, got {other:?}"),
    }
}

#[test]
fn pushing_the_same_content_twice_does_nothing_the_second_time() {
    let sandbox = Sandbox::new("push-idempotent");
    sandbox.write_local("A=1\n");
    let store = FakeStore::default();

    block(push(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();
    let again = block(push(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();

    assert!(matches!(again, PushOutcome::UpToDate), "{again:?}");
}

#[test]
fn a_push_against_a_moved_revision_conflicts_instead_of_overwriting() {
    let sandbox = Sandbox::new("push-conflict");
    sandbox.write_local("A=1\n");
    let store = FakeStore::default();
    block(push(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();

    // Another machine writes, moving the revision this one believes in.
    store.seed(&key(), 7, "A=theirs\n");

    sandbox.write_local("A=mine\n");
    let outcome = block(push(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();

    match outcome {
        PushOutcome::Conflict(divergence) => assert_eq!(divergence.remote, "A=theirs\n"),
        other => panic!("expected Conflict, got {other:?}"),
    }
}

#[test]
fn pushing_with_no_local_file_does_nothing() {
    let sandbox = Sandbox::new("push-absent");
    let store = FakeStore::default();

    let outcome = block(push(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();

    assert!(matches!(outcome, PushOutcome::NothingToPush), "{outcome:?}");
}

#[test]
fn nothing_about_local_bindings_can_reach_the_network() {
    // `Bindings` maps absolute checkout paths to projects. It is the only
    // structure in the feature that describes how a user's disk is laid out,
    // and it must never be serialised into a request.
    //
    // Held as a source fact rather than a runtime one: `env_sync` is the only
    // module that talks to the server, so if the type is not named there, no
    // code in it can send the type. Weaker than a graph rule and stronger than
    // a comment — the same trade `script/check-account-no-telemetry` makes for
    // `zode_account_ui`.
    const NETWORK_MODULE: &str = include_str!("../src/env_sync.rs");

    for forbidden in ["Bindings", "bindings::", "worktree_root"] {
        assert!(
            !NETWORK_MODULE.contains(forbidden),
            "`{forbidden}` is named in the module that builds requests",
        );
    }
}

#[test]
fn a_request_body_carries_the_blob_and_nothing_else() {
    // The positive half of the assertion above: what a push actually sends.
    let sandbox = Sandbox::new("payload");
    sandbox.write_local("SECRET=hunter2\n");
    let store = FakeStore::default();

    block(push(
        &store.context(),
        &key(),
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    ))
    .unwrap();

    let stored = store.documents.lock().unwrap();
    let (blob, _) = stored
        .get(&entry().as_hex())
        .expect("the push stored something");

    assert!(
        !blob.contains("hunter2") && !blob.contains("SECRET"),
        "plaintext reached the wire",
    );
    assert!(
        !blob.contains(&sandbox.dir.to_string_lossy().to_string()),
        "a local path reached the wire",
    );
}

#[test]
fn what_the_panel_shows_is_what_the_request_carries() {
    // The evidence claim, asserted rather than promised.
    //
    // A panel that re-sealed the file to show it would display a DIFFERENT
    // envelope every time — the nonce is fresh per seal — so "the two agree"
    // could not even be checked. One value, built once, shown and sent.
    use zode_env_sync::env_sync::{prepare_push, send_prepared};

    let sandbox = Sandbox::new("wire");
    sandbox.write_local("STRIPE_SECRET_KEY=sk_live_abcdef\n");
    let store = FakeStore::default();
    let context = store.context();

    let prepared = prepare_push(
        &key(),
        USER,
        &entry(),
        &sandbox.env_path(),
        &sandbox.state_path(),
    )
    .unwrap()
    .expect("there is a file to send");

    // What a user would be shown before pressing send.
    let shown = prepared.wire.clone();
    assert!(!shown.envelope_json.contains("sk_live_abcdef"));
    assert!(!shown.blob_base64.contains("sk_live_abcdef"));

    block(send_prepared(
        &context,
        &key(),
        prepared,
        &sandbox.state_path(),
    ))
    .unwrap();

    let stored = store.documents.lock().unwrap();
    let (sent, _) = stored
        .get(&entry().as_hex())
        .expect("the push stored something");
    assert_eq!(
        *sent, shown.blob_base64,
        "the bytes on the wire are not the bytes the panel showed",
    );
}
