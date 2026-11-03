//! A stand-in for the phase 06 routes, plus the two it leans on
//! (`/account/plan`, `/sync/sharing-key`), shared by one in-memory state so
//! several accounts can talk to "the same server".
//!
//! It holds the contract as phase 06 states it, including the parts that
//! refuse: the fingerprint echo on envelopes, the key version compare-and-swap
//! on rotation, `keyVersion` on vault writes, conditional writes with the
//! current document on 409, and 404 for anyone who is not an active member.
//! Testing against something looser would prove nothing about the code that
//! has to survive it. Like the real server it never opens a blob or a key.
//!
//! The `*_as_server` helpers are the server misbehaving on purpose.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use futures::AsyncReadExt as _;
use gpui::AsyncApp;
use http_client::{FakeHttpClient, HttpClient, Response};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

pub const API: &str = "http://test.invalid/api";

#[derive(Clone)]
struct Member {
    email: String,
    role: String,
    active: bool,
}

#[derive(Clone)]
pub struct Stored {
    pub blob: String,
    pub revision: String,
}

#[derive(Clone)]
pub struct StoredEnvelope {
    pub user_id: String,
    pub key_version: u32,
    pub wrapped_key: String,
    pub wrapped_by: String,
}

#[derive(Clone)]
pub struct VaultDoc {
    pub blob: String,
    pub revision: String,
    pub key_version: u32,
    pub updated_by: String,
}

#[derive(Default)]
struct State {
    team_id: String,
    team_name: String,
    key_version: u32,
    rotation_recommended: bool,
    race_next_vault_write: Option<String>,
    plan_unavailable: bool,
    failing_vault_writes: usize,
    members: BTreeMap<String, Member>,
    /// The public key each account registered, base64url.
    registered: HashMap<String, String>,
    sync_documents: HashMap<(String, String), Stored>,
    envelopes: Vec<StoredEnvelope>,
    vault: BTreeMap<String, VaultDoc>,
    revisions: usize,
    requests: Vec<(String, String, String)>,
}

#[derive(Clone)]
pub struct FakeTeamServer {
    state: Arc<Mutex<State>>,
}

pub fn fingerprint_hex(public_key_b64url: &str) -> String {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    let bytes = URL_SAFE_NO_PAD
        .decode(public_key_b64url)
        .unwrap_or_default();
    Sha256::digest(&bytes)[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl FakeTeamServer {
    pub fn new(team_id: &str, team_name: &str) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                team_id: team_id.into(),
                team_name: team_name.into(),
                ..State::default()
            })),
        }
    }

    pub fn add_member(&self, user_id: &str, email: &str, role: &str) {
        self.state.lock().unwrap().members.insert(
            user_id.into(),
            Member {
                email: email.into(),
                role: role.into(),
                active: true,
            },
        );
    }

    /// Phase 05 removal: the member is out and their envelopes are deleted.
    pub fn remove_member(&self, user_id: &str) {
        let mut state = self.state.lock().unwrap();
        if let Some(member) = state.members.get_mut(user_id) {
            member.active = false;
        }
        state
            .envelopes
            .retain(|envelope| envelope.user_id != user_id);
        if state.key_version > 0 {
            state.rotation_recommended = true;
        }
    }

    pub fn key_version(&self) -> u32 {
        self.state.lock().unwrap().key_version
    }

    pub fn envelope_count(&self) -> usize {
        self.state.lock().unwrap().envelopes.len()
    }

    pub fn envelopes_for(&self, user_id: &str) -> Vec<StoredEnvelope> {
        self.state
            .lock()
            .unwrap()
            .envelopes
            .iter()
            .filter(|envelope| envelope.user_id == user_id)
            .cloned()
            .collect()
    }

    pub fn vault_doc(&self, entry_id: &str) -> Option<VaultDoc> {
        self.state.lock().unwrap().vault.get(entry_id).cloned()
    }

    pub fn vault_versions(&self) -> Vec<u32> {
        self.state
            .lock()
            .unwrap()
            .vault
            .values()
            .map(|doc| doc.key_version)
            .collect()
    }

    pub fn registered_key(&self, user_id: &str) -> Option<String> {
        self.state.lock().unwrap().registered.get(user_id).cloned()
    }

    pub fn request_count(&self, user_id: &str) -> usize {
        self.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|(user, _, _)| user == user_id)
            .count()
    }

    pub fn requests_to(&self, user_id: &str, suffix: &str) -> usize {
        self.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|(user, _, path)| user == user_id && path.ends_with(suffix))
            .count()
    }

    /// The server swaps a member's registered key for one it holds, and
    /// drops their envelopes so they show as waiting for a grant — exactly
    /// what a genuine sharing-key rotation looks like from an admin's side.
    pub fn substitute_key_as_server(&self, user_id: &str, public_key_b64url: &str) {
        let mut state = self.state.lock().unwrap();
        state
            .registered
            .insert(user_id.into(), public_key_b64url.into());
        state
            .envelopes
            .retain(|envelope| envelope.user_id != user_id);
    }

    /// The next `count` vault writes fail as if the server were down.
    pub fn fail_next_vault_writes(&self, count: usize) {
        self.state.lock().unwrap().failing_vault_writes = count;
    }

    /// The next vault write loses a race to another member, who writes
    /// `their_blob` just before it lands.
    pub fn lose_next_vault_write(&self, their_blob: &str) {
        self.state.lock().unwrap().race_next_vault_write = Some(their_blob.into());
    }

    /// The server plants an envelope of its own making.
    pub fn inject_envelope_as_server(&self, envelope: StoredEnvelope) {
        let mut state = self.state.lock().unwrap();
        state.envelopes.retain(|existing| {
            !(existing.user_id == envelope.user_id && existing.key_version == envelope.key_version)
        });
        state.envelopes.push(envelope);
    }

    /// `/account/plan` answers 503 until told otherwise.
    pub fn fail_plan_reads(&self, failing: bool) {
        self.state.lock().unwrap().plan_unavailable = failing;
    }

    /// The server plants an envelope beside whatever is already there, so a
    /// member is handed two for one key version.
    pub fn add_envelope_as_server(&self, envelope: StoredEnvelope) {
        self.state.lock().unwrap().envelopes.push(envelope);
    }

    /// The server registers a public key of its choosing for an account.
    pub fn register_key_as_server(&self, user_id: &str, public_key_b64url: &str) {
        self.state
            .lock()
            .unwrap()
            .registered
            .insert(user_id.into(), public_key_b64url.into());
    }

    /// The server claims an older key version is current.
    pub fn roll_back_key_version_as_server(&self, version: u32) {
        self.state.lock().unwrap().key_version = version;
    }

    /// The server moves the team to a version no admin made.
    pub fn set_key_version_as_server(&self, version: u32) {
        self.state.lock().unwrap().key_version = version;
    }

    /// The server serves an older copy of an entry.
    pub fn replay_entry_as_server(&self, entry_id: &str, document: VaultDoc) {
        self.state
            .lock()
            .unwrap()
            .vault
            .insert(entry_id.into(), document);
    }

    /// An HTTP client acting as `user_id`, as if that account's bearer token
    /// had been presented.
    pub fn client_for(&self, user_id: &str) -> Arc<dyn HttpClient> {
        let state = self.state.clone();
        let user_id = user_id.to_string();
        FakeHttpClient::create(move |request| {
            let state = state.clone();
            let user_id = user_id.clone();
            async move {
                let method = request.method().to_string();
                let path = request.uri().path().to_string();
                let if_match = header(&request, "if-match");
                let if_none_match = header(&request, "if-none-match");
                let mut raw = String::new();
                request.into_body().read_to_string(&mut raw).await.ok();
                let body: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);

                let mut state = state.lock().unwrap();
                state
                    .requests
                    .push((user_id.clone(), method.clone(), path.clone()));
                let (status, reply) = route(
                    &mut state,
                    &user_id,
                    &method,
                    &path,
                    &body,
                    if_match.as_deref(),
                    if_none_match.as_deref(),
                );
                Ok(Response::builder()
                    .status(status)
                    .body(reply.to_string().into())
                    .unwrap())
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

fn not_found() -> (u16, Value) {
    (404, json!({ "error": "not_found" }))
}

fn next_revision(state: &mut State) -> String {
    state.revisions += 1;
    format!("rev-{}", state.revisions)
}

fn route(
    state: &mut State,
    user_id: &str,
    method: &str,
    path: &str,
    body: &Value,
    if_match: Option<&str>,
    if_none_match: Option<&str>,
) -> (u16, Value) {
    let Some(path) = path.strip_prefix("/api/") else {
        return not_found();
    };
    let segments: Vec<&str> = path.split('/').collect();
    match segments.as_slice() {
        ["account", "plan"] if state.plan_unavailable => (503, json!({ "error": "unavailable" })),
        ["account", "plan"] => plan(state, user_id),
        ["sync", kind] => {
            sync_document(state, user_id, kind, method, body, if_match, if_none_match)
        }
        ["account", "sharing-key"] => {
            sharing_key(state, user_id, method, body, if_match, if_none_match)
        }
        ["teams", team_id, rest @ ..] => {
            let active = state
                .members
                .get(user_id)
                .is_some_and(|member| member.active);
            if *team_id != state.team_id || !active {
                return not_found();
            }
            let is_admin = state
                .members
                .get(user_id)
                .is_some_and(|member| member.role == "owner" || member.role == "admin");
            match (method, rest) {
                ("GET", ["key"]) => team_key(state, user_id),
                ("GET", ["members", "keys"]) => member_keys(state, user_id),
                ("PUT", ["key", "envelopes"]) | ("POST", ["key", "rotate"]) if !is_admin => {
                    (409, json!({ "error": "admin_required" }))
                }
                ("PUT", ["key", "envelopes"]) => put_envelopes(state, user_id, body),
                ("POST", ["key", "rotate"]) => rotate(state, user_id, body),
                ("GET", ["vault"]) => listing(state),
                ("GET", ["vault", entry]) => match state.vault.get(*entry) {
                    Some(doc) => (200, document_json(doc)),
                    None => not_found(),
                },
                ("PUT", ["vault", entry]) => {
                    put_entry(state, user_id, entry, body, if_match, if_none_match)
                }
                ("DELETE", ["vault", _]) if !is_admin => {
                    (409, json!({ "error": "admin_required" }))
                }
                ("DELETE", ["vault", entry]) => {
                    state.vault.remove(*entry);
                    (200, json!({ "ok": true }))
                }
                _ => not_found(),
            }
        }
        _ => not_found(),
    }
}

fn plan(state: &State, user_id: &str) -> (u16, Value) {
    let team = state
        .members
        .get(user_id)
        .filter(|member| member.active)
        .map(|member| {
            json!({
                "id": state.team_id,
                "name": state.team_name,
                "role": member.role,
                "keyVersion": state.key_version,
            })
        });
    (
        200,
        json!({
            "tier": if team.is_some() { "team" } else { "free" },
            "source": if team.is_some() { "team_seat" } else { "free" },
            "status": "active",
            "renewsAt": null,
            "endsAt": null,
            "limits": {
                "syncDevices": { "used": 1, "max": null },
                "envSync": true,
                "relayDailyBytes": 1,
                "teamVault": team.is_some(),
            },
            "team": team,
            "grandfather": null,
        }),
    )
}

fn sync_document(
    state: &mut State,
    user_id: &str,
    kind: &str,
    method: &str,
    body: &Value,
    if_match: Option<&str>,
    if_none_match: Option<&str>,
) -> (u16, Value) {
    let key = (user_id.to_string(), kind.to_string());
    match method {
        "GET" => match state.sync_documents.get(&key) {
            Some(stored) => (
                200,
                json!({ "blob": stored.blob, "revision": stored.revision }),
            ),
            None => not_found(),
        },
        "PUT" => {
            let current = state.sync_documents.get(&key).cloned();
            let allowed = match (if_none_match, if_match) {
                (Some("*"), _) => current.is_none(),
                (_, Some(expected)) => current
                    .as_ref()
                    .is_some_and(|stored| stored.revision == expected),
                _ => return (412, json!({ "error": "precondition_required" })),
            };
            if !allowed {
                return match current {
                    Some(stored) => (
                        409,
                        json!({ "error": "revision_mismatch", "blob": stored.blob, "revision": stored.revision }),
                    ),
                    None => not_found(),
                };
            }
            let revision = next_revision(state);
            state.sync_documents.insert(
                key,
                Stored {
                    blob: body["blob"].as_str().unwrap_or("").into(),
                    revision: revision.clone(),
                },
            );
            (200, json!({ "revision": revision }))
        }
        _ => (405, json!({ "error": "method" })),
    }
}

fn sharing_key(
    state: &mut State,
    user_id: &str,
    method: &str,
    body: &Value,
    if_match: Option<&str>,
    if_none_match: Option<&str>,
) -> (u16, Value) {
    let current = state.registered.get(user_id).cloned();
    match method {
        "GET" => match current {
            Some(public_key) => (
                200,
                json!({ "publicKey": public_key, "fingerprint": fingerprint_hex(&public_key) }),
            ),
            None => not_found(),
        },
        "PUT" => {
            let public_key = body["publicKey"].as_str().unwrap_or("").to_string();
            if public_key.len() != 43 || public_key.contains('=') {
                return (400, json!({ "error": "invalid_public_key" }));
            }
            let conflict = |existing: &String| {
                (
                    409,
                    json!({ "error": "revision_mismatch", "publicKey": existing,
                            "fingerprint": fingerprint_hex(existing) }),
                )
            };
            match (if_none_match, if_match, &current) {
                (Some("*"), _, None) => {}
                (Some("*"), _, Some(existing)) => return conflict(existing),
                (_, Some(expected), Some(existing)) if fingerprint_hex(existing) == expected => {
                    // Rotation: every envelope sealed to the old key goes.
                    state
                        .envelopes
                        .retain(|envelope| envelope.user_id != user_id);
                }
                (_, Some(_), Some(existing)) => return conflict(existing),
                (_, Some(_), None) => return not_found(),
                _ => return (412, json!({ "error": "precondition_required" })),
            }
            state.registered.insert(user_id.into(), public_key.clone());
            (
                200,
                json!({ "publicKey": public_key, "fingerprint": fingerprint_hex(&public_key) }),
            )
        }
        _ => (405, json!({ "error": "method" })),
    }
}

fn team_key(state: &State, user_id: &str) -> (u16, Value) {
    let mine: Vec<&StoredEnvelope> = state
        .envelopes
        .iter()
        .filter(|envelope| envelope.user_id == user_id)
        .collect();
    if !mine
        .iter()
        .any(|envelope| envelope.key_version == state.key_version)
    {
        return (404, json!({ "error": "key_pending" }));
    }
    (
        200,
        json!({
            "currentVersion": state.key_version,
            "envelopes": mine.iter().map(|envelope| json!({
                "keyVersion": envelope.key_version,
                "wrappedKey": envelope.wrapped_key,
                "wrappedByUserId": envelope.wrapped_by,
            })).collect::<Vec<_>>(),
        }),
    )
}

fn member_keys(state: &State, _user_id: &str) -> (u16, Value) {
    let members: Vec<Value> = state
        .members
        .iter()
        .filter(|(_, member)| member.active)
        .map(|(member_id, member)| {
            let public_key = state.registered.get(member_id);
            json!({
                "userId": member_id,
                "email": member.email,
                "publicKey": public_key,
                "fingerprint": public_key.map(|key| fingerprint_hex(key)),
                "hasCurrentEnvelope": state.envelopes.iter().any(|envelope| {
                    &envelope.user_id == member_id && envelope.key_version == state.key_version
                }),
            })
        })
        .collect();
    (
        200,
        json!({
            "currentVersion": state.key_version,
            "rotationRecommended": state.rotation_recommended,
            "members": members,
        }),
    )
}

/// Shared by grant and rotate: every recipient is an active member whose
/// registered key is the one the wrapper echoed.
fn check_recipients(state: &State, envelopes: &[Value]) -> Result<(), (u16, Value)> {
    for envelope in envelopes {
        let recipient = envelope["userId"].as_str().unwrap_or("");
        let active = state
            .members
            .get(recipient)
            .is_some_and(|member| member.active);
        let registered = state
            .registered
            .get(recipient)
            .map(|key| fingerprint_hex(key));
        if !active {
            return Err((409, json!({ "error": "recipient_not_member" })));
        }
        if registered.as_deref() != envelope["recipientFingerprint"].as_str() {
            return Err((
                409,
                json!({ "error": "recipient_key_changed", "userId": recipient }),
            ));
        }
    }
    Ok(())
}

fn put_envelopes(state: &mut State, user_id: &str, body: &Value) -> (u16, Value) {
    let key_version = body["keyVersion"].as_u64().unwrap_or(0) as u32;
    let envelopes = body["envelopes"].as_array().cloned().unwrap_or_default();
    let initialising = state.key_version == 0 && key_version == 1;
    if key_version != state.key_version && !initialising {
        return (
            409,
            json!({ "error": "key_version_stale", "currentVersion": state.key_version }),
        );
    }
    let mut recipients: Vec<&str> = envelopes
        .iter()
        .filter_map(|e| e["userId"].as_str())
        .collect();
    recipients.sort_unstable();
    if recipients.windows(2).any(|pair| pair[0] == pair[1]) {
        return (400, json!({ "error": "duplicate_recipient" }));
    }
    if initialising && !recipients.contains(&user_id) {
        return (400, json!({ "error": "self_envelope_required" }));
    }
    let holds_current = state
        .envelopes
        .iter()
        .any(|envelope| envelope.user_id == user_id && envelope.key_version == state.key_version);
    if !initialising && !holds_current {
        return (409, json!({ "error": "key_pending" }));
    }
    if let Err(refusal) = check_recipients(state, &envelopes) {
        return refusal;
    }
    if initialising {
        state.key_version = 1;
    }
    let mut granted = 0;
    for envelope in envelopes {
        let recipient = envelope["userId"].as_str().unwrap_or("").to_string();
        // No overwrite: an existing envelope stays as it is.
        if state
            .envelopes
            .iter()
            .any(|existing| existing.user_id == recipient && existing.key_version == key_version)
        {
            continue;
        }
        granted += 1;
        state.envelopes.push(StoredEnvelope {
            user_id: recipient,
            key_version,
            wrapped_key: envelope["wrappedKey"].as_str().unwrap_or("").into(),
            wrapped_by: user_id.into(),
        });
    }
    (
        200,
        json!({ "keyVersion": key_version, "granted": granted }),
    )
}

fn rotate(state: &mut State, user_id: &str, body: &Value) -> (u16, Value) {
    let from_version = body["fromVersion"].as_u64().unwrap_or(0) as u32;
    let envelopes = body["envelopes"].as_array().cloned().unwrap_or_default();
    if from_version != state.key_version {
        return (
            409,
            json!({ "error": "key_version_stale", "currentVersion": state.key_version }),
        );
    }
    if let Err(refusal) = check_recipients(state, &envelopes) {
        return refusal;
    }
    let covered = state
        .members
        .iter()
        .filter(|(member_id, member)| member.active && state.registered.contains_key(*member_id))
        .all(|(member_id, _)| {
            envelopes
                .iter()
                .any(|envelope| envelope["userId"].as_str() == Some(member_id))
        });
    if !covered {
        return (
            409,
            json!({ "error": "envelopes_incomplete", "missing": [] }),
        );
    }
    let key_version = from_version + 1;
    for envelope in envelopes {
        state.envelopes.push(StoredEnvelope {
            user_id: envelope["userId"].as_str().unwrap_or("").into(),
            key_version,
            wrapped_key: envelope["wrappedKey"].as_str().unwrap_or("").into(),
            wrapped_by: user_id.into(),
        });
    }
    state.key_version = key_version;
    state.rotation_recommended = false;
    (200, json!({ "keyVersion": key_version }))
}

fn document_json(doc: &VaultDoc) -> Value {
    json!({
        "blob": doc.blob,
        "revision": doc.revision,
        "keyVersion": doc.key_version,
        "byteLength": doc.blob.len(),
        "updatedAt": "2026-11-03T00:00:00.000Z",
        "updatedBy": doc.updated_by,
    })
}

fn listing(state: &State) -> (u16, Value) {
    let entries: Vec<Value> = state
        .vault
        .iter()
        .map(|(entry_id, doc)| {
            json!({
                "entryId": entry_id,
                "revision": doc.revision,
                "keyVersion": doc.key_version,
                "byteLength": doc.blob.len(),
                "updatedAt": "2026-11-03T00:00:00.000Z",
                "updatedBy": doc.updated_by,
            })
        })
        .collect();
    (200, json!({ "entries": entries }))
}

fn put_entry(
    state: &mut State,
    user_id: &str,
    entry: &str,
    body: &Value,
    if_match: Option<&str>,
    if_none_match: Option<&str>,
) -> (u16, Value) {
    if state.failing_vault_writes > 0 {
        state.failing_vault_writes -= 1;
        return (503, json!({ "error": "unavailable" }));
    }
    if let Some(their_blob) = state.race_next_vault_write.take() {
        let revision = next_revision(state);
        let key_version = state.key_version;
        state.vault.insert(
            entry.into(),
            VaultDoc {
                blob: their_blob,
                revision,
                key_version,
                updated_by: "someone-else".into(),
            },
        );
    }
    let current = state.vault.get(entry).cloned();
    let allowed = match (if_none_match, if_match) {
        (Some("*"), _) => current.is_none(),
        (_, Some(expected)) => current.as_ref().is_some_and(|doc| doc.revision == expected),
        _ => return (412, json!({ "error": "precondition_required" })),
    };
    if !allowed {
        return match current {
            Some(doc) => {
                let mut reply = document_json(&doc);
                reply["error"] = "revision_mismatch".into();
                (409, reply)
            }
            None => not_found(),
        };
    }
    let key_version = body["keyVersion"].as_u64().unwrap_or(0) as u32;
    if key_version != state.key_version {
        return (409, json!({ "error": "key_version_stale" }));
    }
    let revision = next_revision(state);
    state.vault.insert(
        entry.into(),
        VaultDoc {
            blob: body["blob"].as_str().unwrap_or("").into(),
            revision: revision.clone(),
            key_version,
            updated_by: user_id.into(),
        },
    );
    (
        200,
        json!({ "revision": revision, "updatedAt": "2026-11-03T00:00:00.000Z" }),
    )
}

/// One account's keychain, holding the recovery key (`zode://sync-key`).
pub struct MemoryKeychain {
    entries: Mutex<HashMap<String, (String, Vec<u8>)>>,
}

impl MemoryKeychain {
    pub fn with_recovery_key(user_id: &str, dek: [u8; 32]) -> Arc<Self> {
        let mut entries = HashMap::new();
        entries.insert(
            "zode://sync-key".to_string(),
            (user_id.to_string(), dek.to_vec()),
        );
        Arc::new(Self {
            entries: Mutex::new(entries),
        })
    }

    pub fn empty() -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(HashMap::new()),
        })
    }
}

impl credentials_provider::CredentialsProvider for MemoryKeychain {
    fn read_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Option<(String, Vec<u8>)>>> + 'a>> {
        let found = self.entries.lock().unwrap().get(url).cloned();
        Box::pin(async move { Ok(found) })
    }

    fn write_credentials<'a>(
        &'a self,
        url: &'a str,
        username: &'a str,
        password: &'a [u8],
        _cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + 'a>> {
        self.entries
            .lock()
            .unwrap()
            .insert(url.into(), (username.into(), password.to_vec()));
        Box::pin(async { Ok(()) })
    }

    fn delete_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + 'a>> {
        self.entries.lock().unwrap().remove(url);
        Box::pin(async { Ok(()) })
    }
}
