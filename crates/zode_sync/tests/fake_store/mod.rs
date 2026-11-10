//! A stand-in for `/api/sync/:kind` plus a scratch directory, shared by the
//! sync integration tests.
//!
//! The fake honours `If-Match` and `If-None-Match` exactly as the real
//! controller does, including returning the current document with a 409 —
//! testing against something looser would prove nothing about the code that
//! has to survive two machines writing at once.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use http_client::{FakeHttpClient, HttpClient, Response};
use zode_account::ApiCredential;
use zode_sync::artifact::Artifact;
use zode_sync::sync::SyncContext;
use zode_sync::{Dek, Kind, Resource};

pub const USER: &str = "68b1f0c2a4d3e5f60718293a";
pub const API: &str = "https://api.example.invalid/api";

pub fn key() -> Dek {
    Dek::from_bytes([0x21; 32])
}

/// A temporary directory that cleans itself up.
pub struct Sandbox {
    pub dir: PathBuf,
}

impl Sandbox {
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("zode-sync-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    pub fn artifact(&self, kind: Kind) -> Artifact {
        Artifact::rooted_at(kind, &self.dir)
    }

    pub fn state_path(&self) -> PathBuf {
        self.dir.join("sync_state.json")
    }

    pub fn write_local(&self, kind: Kind, content: &str) {
        std::fs::write(self.artifact(kind).path, content).unwrap();
    }

    pub fn read_local(&self, kind: Kind) -> Option<String> {
        std::fs::read_to_string(self.artifact(kind).path).ok()
    }

    pub fn read_backup(&self, kind: Kind) -> Option<String> {
        std::fs::read_to_string(self.artifact(kind).backup_path.unwrap()).ok()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[derive(Clone, Default)]
pub struct Stored {
    pub blob: String,
    pub revision: String,
}

/// A stand-in for `/api/sync/:kind` that honours `If-Match` and
/// `If-None-Match` the way the real controller does — including returning the
/// current document with a 409.
#[derive(Clone)]
pub struct FakeStore {
    pub documents: Arc<Mutex<HashMap<String, Stored>>>,
    pub revisions: Arc<AtomicUsize>,
    pub requests: Arc<AtomicUsize>,
    pub writes: Arc<AtomicUsize>,
    pub reject: Arc<std::sync::atomic::AtomicBool>,
    /// Successful writes allowed before every later write answers 503.
    pub fail_after: Arc<Mutex<Option<usize>>>,
    /// Slots whose writes answer 409, as if another machine got there first.
    pub conflicts: Arc<Mutex<Vec<String>>>,
    /// Slots whose writes answer 402, as a plan that does not cover them.
    pub plan_refusals: Arc<Mutex<Vec<String>>>,
}

impl FakeStore {
    pub fn new() -> Self {
        Self {
            documents: Arc::new(Mutex::new(HashMap::new())),
            revisions: Arc::new(AtomicUsize::new(0)),
            requests: Arc::new(AtomicUsize::new(0)),
            writes: Arc::new(AtomicUsize::new(0)),
            reject: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            fail_after: Arc::new(Mutex::new(None)),
            conflicts: Arc::new(Mutex::new(Vec::new())),
            plan_refusals: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Answers writes to this one slot with 402 `env_sync_not_in_plan`.
    pub fn refuse_by_plan_on(&self, slot: &str) {
        self.plan_refusals.lock().unwrap().push(slot.to_string());
    }

    /// Lets `count` writes land, then answers every later one with a 503:
    /// the connection dropping halfway through.
    pub fn fail_writes_after(&self, count: usize) {
        *self.fail_after.lock().unwrap() = Some(count);
    }

    pub fn heal(&self) {
        *self.fail_after.lock().unwrap() = None;
        self.conflicts.lock().unwrap().clear();
        self.plan_refusals.lock().unwrap().clear();
        self.reject.store(false, Ordering::SeqCst);
    }

    /// Answers writes to this one slot with a conflict.
    pub fn conflict_on(&self, slot: &str) {
        self.conflicts.lock().unwrap().push(slot.to_string());
    }

    fn slot_name(resource: &Resource) -> String {
        resource.path().rsplit('/').next().unwrap_or("").to_string()
    }

    /// Seeds any resource with raw bytes, sealed the way its owning crate seals it.
    pub fn seed_at(&self, resource: &Resource, dek: &Dek, plaintext: &[u8]) -> String {
        let envelope = zode_sync::encrypt_at(dek, USER, resource, plaintext).unwrap();
        let blob = zode_sync::to_blob(&envelope).unwrap();
        let revision = format!("rev-{}", self.revisions.fetch_add(1, Ordering::SeqCst));
        self.documents.lock().unwrap().insert(
            Self::slot_name(resource),
            Stored {
                blob,
                revision: revision.clone(),
            },
        );
        revision
    }

    /// Opens a stored resource as raw bytes.
    pub fn open_at(
        &self,
        resource: &Resource,
        dek: &Dek,
    ) -> Option<Result<Vec<u8>, zode_sync::SyncCryptoError>> {
        let stored = self
            .documents
            .lock()
            .unwrap()
            .get(&Self::slot_name(resource))
            .cloned()?;
        let envelope = zode_sync::from_blob(&stored.blob).ok()?;
        Some(zode_sync::decrypt_at(dek, USER, resource, &envelope))
    }

    pub fn revision_of(&self, resource: &Resource) -> Option<String> {
        self.documents
            .lock()
            .unwrap()
            .get(&Self::slot_name(resource))
            .map(|stored| stored.revision.clone())
    }

    /// Answers every subsequent write with a conflict, standing in for another
    /// machine that keeps getting there first.
    pub fn reject_writes(&self) {
        self.reject.store(true, Ordering::SeqCst);
    }

    /// Opens a stored blob, for asserting on content rather than ciphertext.
    pub fn read_plaintext(&self, kind: Kind, dek: &Dek) -> Option<String> {
        let stored = self.documents.lock().unwrap().get(kind.as_str()).cloned()?;
        let envelope = zode_sync::from_blob(&stored.blob).ok()?;
        let bytes = zode_sync::decrypt(dek, USER, kind, &envelope).ok()?;
        String::from_utf8(bytes).ok()
    }

    /// Seeds the store as if another machine had pushed this content.
    pub fn seed(&self, kind: Kind, dek: &Dek, plaintext: &str) -> String {
        let envelope = zode_sync::encrypt(dek, USER, kind, plaintext.as_bytes()).unwrap();
        let blob = zode_sync::to_blob(&envelope).unwrap();
        let revision = format!("rev-{}", self.revisions.fetch_add(1, Ordering::SeqCst));
        self.documents.lock().unwrap().insert(
            kind.as_str().into(),
            Stored {
                blob,
                revision: revision.clone(),
            },
        );
        revision
    }

    /// Seeds a blob that this key cannot open.
    pub fn seed_with_other_key(&self, kind: Kind, plaintext: &str) {
        self.seed(kind, &Dek::from_bytes([0x99; 32]), plaintext);
    }

    pub fn client(&self) -> Arc<dyn HttpClient> {
        let documents = self.documents.clone();
        let revisions = self.revisions.clone();
        let requests = self.requests.clone();
        let writes = self.writes.clone();
        let reject = self.reject.clone();
        let fail_after = self.fail_after.clone();
        let conflicts = self.conflicts.clone();
        let plan_refusals = self.plan_refusals.clone();

        FakeHttpClient::create(move |request| {
            let documents = documents.clone();
            let revisions = revisions.clone();
            let requests = requests.clone();
            let writes = writes.clone();
            let reject = reject.clone();
            let fail_after = fail_after.clone();
            let conflicts = conflicts.clone();
            let plan_refusals = plan_refusals.clone();
            async move {
                requests.fetch_add(1, Ordering::SeqCst);

                let kind = request
                    .uri()
                    .path()
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .to_string();
                let method = request.method().as_str().to_string();
                let if_match = request_header(&request, "if-match");
                let if_none_match = request_header(&request, "if-none-match");

                let body = {
                    use futures::AsyncReadExt as _;
                    let mut raw = String::new();
                    let mut body = request.into_body();
                    let _ = body.read_to_string(&mut raw).await;
                    raw
                };

                let mut documents = documents.lock().unwrap();

                let reply = |status: u16, json: serde_json::Value| {
                    Ok(Response::builder()
                        .status(status)
                        .body(json.to_string().into())
                        .unwrap())
                };
                let document_json = |stored: &Stored| {
                    serde_json::json!({
                        "blob": stored.blob,
                        "revision": stored.revision,
                        "updatedAt": "2026-08-30T00:00:00.000Z",
                        "byteLength": stored.blob.len(),
                    })
                };

                match method.as_str() {
                    "GET" => match documents.get(&kind) {
                        Some(stored) => reply(200, document_json(stored)),
                        None => reply(404, serde_json::json!({ "error": "not_found" })),
                    },
                    "PUT" => {
                        let incoming: serde_json::Value =
                            serde_json::from_str(&body).unwrap_or_default();
                        let blob = incoming["blob"].as_str().unwrap_or("").to_string();
                        let current = documents.get(&kind).cloned();

                        if fail_after
                            .lock()
                            .unwrap()
                            .is_some_and(|limit| writes.load(Ordering::SeqCst) >= limit)
                        {
                            return reply(503, serde_json::json!({ "error": "unavailable" }));
                        }
                        if plan_refusals.lock().unwrap().contains(&kind) {
                            return reply(
                                402,
                                serde_json::json!({ "error": "env_sync_not_in_plan" }),
                            );
                        }

                        let allowed = if reject.load(Ordering::SeqCst)
                            || conflicts.lock().unwrap().contains(&kind)
                        {
                            false
                        } else {
                            match (if_none_match.as_deref(), if_match.as_deref()) {
                                (Some("*"), _) => current.is_none(),
                                (_, Some(expected)) => {
                                    current.as_ref().is_some_and(|s| s.revision == expected)
                                }
                                _ => {
                                    return reply(
                                        412,
                                        serde_json::json!({ "error": "precondition_required" }),
                                    );
                                }
                            }
                        };

                        if !allowed {
                            return match current {
                                Some(stored) => {
                                    let mut json = document_json(&stored);
                                    json["error"] = "revision_mismatch".into();
                                    reply(409, json)
                                }
                                // If-Match against nothing.
                                None => reply(404, serde_json::json!({ "error": "not_found" })),
                            };
                        }

                        writes.fetch_add(1, Ordering::SeqCst);
                        let revision = format!("rev-{}", revisions.fetch_add(1, Ordering::SeqCst));
                        documents.insert(
                            kind,
                            Stored {
                                blob,
                                revision: revision.clone(),
                            },
                        );
                        reply(
                            200,
                            serde_json::json!({
                                "revision": revision,
                                "updatedAt": "2026-08-30T00:00:00.000Z"
                            }),
                        )
                    }
                    "DELETE" => {
                        documents.remove(&kind);
                        reply(200, serde_json::json!({ "ok": true }))
                    }
                    _ => reply(405, serde_json::json!({ "error": "method" })),
                }
            }
        }) as Arc<dyn HttpClient>
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

pub fn context(store: &FakeStore) -> SyncContext {
    SyncContext {
        http_client: store.client(),
        api_url: API.into(),
        credential: ApiCredential {
            access_token: "at-1".into(),
            user_id: USER.into(),
        },
    }
}

pub fn block<T>(future: impl std::future::Future<Output = T>) -> T {
    futures::executor::block_on(future)
}

fn request_header(
    request: &http_client::http::Request<http_client::AsyncBody>,
    name: &str,
) -> Option<String> {
    request
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

/// A keychain holding entries by URL, as the real one does: the recovery key
/// and anything else this crate stores beside it.
#[derive(Default)]
pub struct MemoryKeychain {
    pub entries: Mutex<HashMap<String, (String, Vec<u8>)>>,
}

impl MemoryKeychain {
    pub fn with_recovery_key(dek: [u8; 32]) -> Arc<Self> {
        let keychain = Self::default();
        keychain
            .entries
            .lock()
            .unwrap()
            .insert("zode://sync-key".into(), (USER.into(), dek.to_vec()));
        Arc::new(keychain)
    }

    pub fn get(&self, url: &str) -> Option<Vec<u8>> {
        self.entries
            .lock()
            .unwrap()
            .get(url)
            .map(|(_, bytes)| bytes.clone())
    }
}

impl credentials_provider::CredentialsProvider for MemoryKeychain {
    fn read_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a gpui::AsyncApp,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<Option<(String, Vec<u8>)>>> + 'a>,
    > {
        let found = self.entries.lock().unwrap().get(url).cloned();
        Box::pin(async move { Ok(found) })
    }

    fn write_credentials<'a>(
        &'a self,
        url: &'a str,
        username: &'a str,
        password: &'a [u8],
        _cx: &'a gpui::AsyncApp,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + 'a>> {
        self.entries
            .lock()
            .unwrap()
            .insert(url.into(), (username.into(), password.to_vec()));
        Box::pin(async { Ok(()) })
    }

    fn delete_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a gpui::AsyncApp,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + 'a>> {
        self.entries.lock().unwrap().remove(url);
        Box::pin(async { Ok(()) })
    }
}
