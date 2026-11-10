//! The phase 06 routes, one function each.
//!
//! The one place this crate touches the network. Every answer a function does
//! not interpret itself goes through `zode_sync::client::classify`, so a 401,
//! a 402 or a 5xx means here exactly what it means for settings and `.env`.
//! Keys and envelopes cross this boundary as bytes the server cannot open.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use remote_relay_protocol::KEY_LEN;
use serde::{Deserialize, Serialize};
use zode_env_sync::EntryId;
use zode_sync::client::{self as sync_client, ClientError};
use zode_sync::sync::SyncContext;
use zode_sync::{Resource, TeamRoute};

use crate::sharing_key::{Fingerprint, decode_public_key, encode_public_key};
use crate::{TeamCryptoError, TeamVaultError};

/// A public sharing key as the server holds it. The fingerprint is recomputed
/// here and compared, never taken on the server's word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisteredKey {
    pub public_key: [u8; KEY_LEN],
    pub fingerprint: Fingerprint,
}

pub enum KeyRegistration<'a> {
    /// `If-None-Match: *`: this account has no key on the server yet.
    Create,
    /// `If-Match: <fingerprint>`: replace this one. The server then deletes
    /// the caller's envelopes in every team, so they must be granted again.
    Rotate(&'a Fingerprint),
}

#[derive(Debug, PartialEq, Eq)]
pub enum RegisterOutcome {
    Registered,
    /// Another key is registered than the precondition named.
    Conflict,
}

/// One sealed team key, as `GET /teams/:id/key` lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealedEnvelope {
    pub key_version: u32,
    pub wrapped_key: Vec<u8>,
    pub wrapped_by_user_id: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum KeyAnswer {
    Envelopes {
        current_version: u32,
        envelopes: Vec<SealedEnvelope>,
    },
    /// A member, but nobody has sealed the current key to them yet.
    KeyPending,
    /// Not (or no longer) a member.
    NotMember,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberKey {
    pub user_id: String,
    pub email: String,
    /// `None` until the member has opened team sharing once.
    pub key: Option<RegisteredKey>,
    pub has_current_envelope: bool,
}

/// `GET /teams/:id/members/keys`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberKeys {
    pub current_version: u32,
    /// Set by the server when a member was removed since the last rotation.
    pub rotation_recommended: bool,
    pub members: Vec<MemberKey>,
}

/// One envelope to store: the recipient's fingerprint is echoed so the server
/// can refuse a wrap aimed at a key it no longer holds for them.
pub struct OutgoingEnvelope {
    pub user_id: String,
    pub recipient_fingerprint: Fingerprint,
    pub wrapped_key: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum EnvelopeWrite {
    Stored,
    /// The team has moved past this key version.
    KeyVersionStale,
    /// A recipient's key on the server is not the one this wrap was made for.
    RecipientKeyChanged,
    NotMember,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RotateWrite {
    Rotated,
    /// The compare-and-swap lost: another rotation moved the version first.
    KeyVersionStale,
    /// A recipient's key on the server is not the one the wrap was made for.
    RecipientKeyChanged {
        user_id: Option<String>,
    },
    NotMember,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultListing {
    pub entry_id: EntryId,
    pub revision: String,
    pub key_version: u32,
    pub byte_length: u64,
    pub updated_at: String,
    pub updated_by: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultDocument {
    /// Base64 of the envelope. Still encrypted at this layer.
    pub blob: String,
    pub revision: String,
    pub key_version: u32,
}

pub enum VaultPrecondition<'a> {
    Create,
    Replace(&'a str),
}

#[derive(Debug, PartialEq, Eq)]
pub enum VaultWrite {
    Stored {
        revision: String,
    },
    /// Someone else wrote first; the current document travels with it.
    Conflict(VaultDocument),
    KeyVersionStale,
    /// `If-Match` named an entry the server no longer has.
    Gone,
}

pub async fn fetch_registered_key(
    context: &SyncContext,
) -> Result<Option<RegisteredKey>, TeamVaultError> {
    let (status, body) = call(context, &Resource::account_sharing_key(), "GET", None, &[]).await?;
    if status == 404 {
        return Ok(None);
    }
    sync_client::classify(status, &body)?;
    let wire: WireKey = parse(&body)?;
    Ok(Some(registered_key(&wire.public_key, &wire.fingerprint)?))
}

pub async fn register_sharing_key(
    context: &SyncContext,
    public_key: &[u8; KEY_LEN],
    registration: KeyRegistration<'_>,
) -> Result<RegisterOutcome, TeamVaultError> {
    let body = to_json(&serde_json::json!({ "publicKey": encode_public_key(public_key) }))?;
    let expected;
    let header = match registration {
        KeyRegistration::Create => ("If-None-Match", "*"),
        KeyRegistration::Rotate(fingerprint) => {
            expected = fingerprint.to_hex();
            ("If-Match", expected.as_str())
        }
    };
    let (status, body) = call(
        context,
        &Resource::account_sharing_key(),
        "PUT",
        Some(&body),
        &[header],
    )
    .await?;
    // 409: another key is registered than the precondition named. 404: the
    // `If-Match` named a key that is no longer there.
    if status == 409 || status == 404 {
        return Ok(RegisterOutcome::Conflict);
    }
    sync_client::classify(status, &body)?;
    Ok(RegisterOutcome::Registered)
}

pub async fn fetch_team_key(
    context: &SyncContext,
    team_id: &str,
) -> Result<KeyAnswer, TeamVaultError> {
    let resource = Resource::team_route(team_id, TeamRoute::Key).map_err(TeamCryptoError::from)?;
    let (status, body) = call(context, &resource, "GET", None, &[]).await?;
    if status == 404 {
        return Ok(match error_slug(&body).as_deref() {
            Some("key_pending") => KeyAnswer::KeyPending,
            _ => KeyAnswer::NotMember,
        });
    }
    sync_client::classify(status, &body)?;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Wire {
        current_version: u32,
        envelopes: Vec<WireEnvelope>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WireEnvelope {
        key_version: u32,
        wrapped_key: String,
        wrapped_by_user_id: String,
    }

    let wire: Wire = parse(&body)?;
    let envelopes = wire
        .envelopes
        .into_iter()
        .map(|envelope| {
            Ok(SealedEnvelope {
                key_version: envelope.key_version,
                wrapped_key: BASE64.decode(&envelope.wrapped_key).map_err(|_| {
                    TeamCryptoError::Malformed("a wrapped key is not base64".into())
                })?,
                wrapped_by_user_id: envelope.wrapped_by_user_id,
            })
        })
        .collect::<Result<Vec<_>, TeamCryptoError>>()?;
    Ok(KeyAnswer::Envelopes {
        current_version: wire.current_version,
        envelopes,
    })
}

/// `None` when the caller is not a member.
pub async fn fetch_member_keys(
    context: &SyncContext,
    team_id: &str,
) -> Result<Option<MemberKeys>, TeamVaultError> {
    let resource =
        Resource::team_route(team_id, TeamRoute::MemberKeys).map_err(TeamCryptoError::from)?;
    let (status, body) = call(context, &resource, "GET", None, &[]).await?;
    if status == 404 {
        return Ok(None);
    }
    sync_client::classify(status, &body)?;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Wire {
        current_version: u32,
        #[serde(default)]
        rotation_recommended: bool,
        members: Vec<WireMember>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WireMember {
        user_id: String,
        email: String,
        public_key: Option<String>,
        fingerprint: Option<String>,
        #[serde(default)]
        has_current_envelope: bool,
    }

    let wire: Wire = parse(&body)?;
    let members = wire
        .members
        .into_iter()
        .map(|member| {
            let key = match (member.public_key, member.fingerprint) {
                (Some(public_key), Some(fingerprint)) => {
                    Some(registered_key(&public_key, &fingerprint)?)
                }
                (None, None) => None,
                _ => {
                    return Err(TeamCryptoError::Malformed(
                        "a member key without its fingerprint".into(),
                    ));
                }
            };
            Ok(MemberKey {
                user_id: member.user_id,
                email: member.email,
                key,
                has_current_envelope: member.has_current_envelope,
            })
        })
        .collect::<Result<Vec<_>, TeamCryptoError>>()?;
    Ok(Some(MemberKeys {
        current_version: wire.current_version,
        rotation_recommended: wire.rotation_recommended,
        members,
    }))
}

pub async fn put_envelopes(
    context: &SyncContext,
    team_id: &str,
    key_version: u32,
    envelopes: &[OutgoingEnvelope],
) -> Result<EnvelopeWrite, TeamVaultError> {
    let resource =
        Resource::team_route(team_id, TeamRoute::KeyEnvelopes).map_err(TeamCryptoError::from)?;
    let body = to_json(&serde_json::json!({
        "keyVersion": key_version,
        "envelopes": wire_envelopes(envelopes),
    }))?;
    let (status, body) = call(context, &resource, "PUT", Some(&body), &[]).await?;
    match (status, error_slug(&body).as_deref()) {
        (404, _) => Ok(EnvelopeWrite::NotMember),
        (409, Some("key_version_stale")) => Ok(EnvelopeWrite::KeyVersionStale),
        (409, Some("recipient_key_changed")) => Ok(EnvelopeWrite::RecipientKeyChanged),
        _ => {
            sync_client::classify(status, &body)?;
            Ok(EnvelopeWrite::Stored)
        }
    }
}

pub async fn rotate_key(
    context: &SyncContext,
    team_id: &str,
    from_version: u32,
    envelopes: &[OutgoingEnvelope],
) -> Result<RotateWrite, TeamVaultError> {
    let resource =
        Resource::team_route(team_id, TeamRoute::KeyRotate).map_err(TeamCryptoError::from)?;
    let body = to_json(&serde_json::json!({
        "fromVersion": from_version,
        "envelopes": wire_envelopes(envelopes),
    }))?;
    let (status, body) = call(context, &resource, "POST", Some(&body), &[]).await?;
    match (status, error_slug(&body).as_deref()) {
        (404, _) => Ok(RotateWrite::NotMember),
        (409, Some("key_version_stale")) => Ok(RotateWrite::KeyVersionStale),
        (409, Some("recipient_key_changed")) => Ok(RotateWrite::RecipientKeyChanged {
            user_id: error_field(&body, "userId"),
        }),
        // `envelopes_incomplete`, `admin_required` and the rest are refusals
        // with their slug, through the shared classification.
        _ => {
            sync_client::classify(status, &body)?;
            Ok(RotateWrite::Rotated)
        }
    }
}

/// `None` when the caller is not a member.
pub async fn list_vault(
    context: &SyncContext,
    team_id: &str,
) -> Result<Option<Vec<VaultListing>>, TeamVaultError> {
    let resource = Resource::team_listing(team_id).map_err(TeamCryptoError::from)?;
    let (status, body) = call(context, &resource, "GET", None, &[]).await?;
    if status == 404 {
        return Ok(None);
    }
    sync_client::classify(status, &body)?;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Wire {
        entries: Vec<WireListing>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WireListing {
        entry_id: String,
        revision: String,
        key_version: u32,
        byte_length: u64,
        updated_at: String,
        updated_by: String,
    }

    let wire: Wire = parse(&body)?;
    let entries = wire
        .entries
        .into_iter()
        .map(|entry| {
            Ok(VaultListing {
                entry_id: EntryId::parse(&entry.entry_id)?,
                revision: entry.revision,
                key_version: entry.key_version,
                byte_length: entry.byte_length,
                updated_at: entry.updated_at,
                updated_by: entry.updated_by,
            })
        })
        .collect::<Result<Vec<_>, TeamCryptoError>>()?;
    Ok(Some(entries))
}

/// `None` when the entry is not there (or the caller is not a member: the
/// server answers both with the same 404, on purpose).
pub async fn fetch_entry(
    context: &SyncContext,
    team_id: &str,
    entry: &EntryId,
) -> Result<Option<VaultDocument>, TeamVaultError> {
    let resource = Resource::team_entry(team_id, &entry.as_hex()).map_err(TeamCryptoError::from)?;
    let (status, body) = call(context, &resource, "GET", None, &[]).await?;
    if status == 404 {
        return Ok(None);
    }
    sync_client::classify(status, &body)?;
    Ok(Some(parse_document(&body)?))
}

pub async fn store_entry(
    context: &SyncContext,
    team_id: &str,
    entry: &EntryId,
    blob: &str,
    key_version: u32,
    precondition: VaultPrecondition<'_>,
) -> Result<VaultWrite, TeamVaultError> {
    let resource = Resource::team_entry(team_id, &entry.as_hex()).map_err(TeamCryptoError::from)?;
    let body = to_json(&serde_json::json!({ "blob": blob, "keyVersion": key_version }))?;
    let header = match precondition {
        VaultPrecondition::Create => ("If-None-Match", "*"),
        VaultPrecondition::Replace(revision) => ("If-Match", revision),
    };
    let (status, body) = call(context, &resource, "PUT", Some(&body), &[header]).await?;
    match (status, error_slug(&body).as_deref()) {
        (409, Some("key_version_stale")) => Ok(VaultWrite::KeyVersionStale),
        (409, _) => Ok(VaultWrite::Conflict(parse_document(&body)?)),
        (404, _) => Ok(VaultWrite::Gone),
        _ => {
            sync_client::classify(status, &body)?;
            #[derive(Deserialize)]
            struct Stored {
                revision: String,
            }
            let stored: Stored = parse(&body)?;
            Ok(VaultWrite::Stored {
                revision: stored.revision,
            })
        }
    }
}

/// Idempotent, like the route.
pub async fn delete_entry(
    context: &SyncContext,
    team_id: &str,
    entry: &EntryId,
) -> Result<(), TeamVaultError> {
    let resource = Resource::team_entry(team_id, &entry.as_hex()).map_err(TeamCryptoError::from)?;
    let (status, body) = call(context, &resource, "DELETE", None, &[]).await?;
    if status == 404 {
        return Ok(());
    }
    Ok(sync_client::classify(status, &body)?)
}

async fn call(
    context: &SyncContext,
    resource: &Resource,
    method: &str,
    body: Option<&str>,
    headers: &[(&str, &str)],
) -> Result<(u16, String), ClientError> {
    let (status, body) = sync_client::request(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        resource,
        method,
        body,
        headers,
    )
    .await?;
    // Every team route answers 404 `billing_disabled` while billing is off.
    // That is not "not a member" and not "no such entry", and no caller may
    // read it as either.
    if status == 404 && error_slug(&body).as_deref() == Some("billing_disabled") {
        return Err(ClientError::Rejected("billing_disabled".into()));
    }
    Ok((status, body))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireKey {
    public_key: String,
    fingerprint: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireOutgoing {
    user_id: String,
    recipient_fingerprint: String,
    wrapped_key: String,
}

fn wire_envelopes(envelopes: &[OutgoingEnvelope]) -> Vec<WireOutgoing> {
    envelopes
        .iter()
        .map(|envelope| WireOutgoing {
            user_id: envelope.user_id.clone(),
            recipient_fingerprint: envelope.recipient_fingerprint.to_hex(),
            wrapped_key: BASE64.encode(&envelope.wrapped_key),
        })
        .collect()
}

fn registered_key(public_key: &str, fingerprint: &str) -> Result<RegisteredKey, TeamCryptoError> {
    let public_key = decode_public_key(public_key)?;
    let computed = Fingerprint::of(&public_key);
    // The server's fingerprint is a claim; the key is the fact. A mismatch
    // means the server is confused or lying, and either way nothing built on
    // this answer can be trusted.
    if Fingerprint::from_hex(fingerprint) != Some(computed) {
        return Err(TeamCryptoError::Malformed(
            "a key whose fingerprint does not match it".into(),
        ));
    }
    Ok(RegisteredKey {
        public_key,
        fingerprint: computed,
    })
}

fn parse_document(body: &str) -> Result<VaultDocument, TeamCryptoError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Wire {
        blob: String,
        revision: String,
        key_version: u32,
    }
    let wire: Wire = parse(body)?;
    Ok(VaultDocument {
        blob: wire.blob,
        revision: wire.revision,
        key_version: wire.key_version,
    })
}

fn parse<T: for<'de> Deserialize<'de>>(body: &str) -> Result<T, TeamCryptoError> {
    // Never the body itself in the error: it may echo something sensitive.
    serde_json::from_str(body).map_err(|_| {
        TeamCryptoError::Malformed("the team service answered with unusable JSON".into())
    })
}

fn to_json(value: &serde_json::Value) -> Result<String, TeamVaultError> {
    serde_json::to_string(value)
        .map_err(|_| ClientError::Unreachable("the request could not be built".into()).into())
}

fn error_field(body: &str, field: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .get(field)?
        .as_str()
        .map(str::to_string)
}

fn error_slug(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: String,
    }
    serde_json::from_str::<ErrorBody>(body)
        .ok()
        .map(|parsed| parsed.error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use http_client::{FakeHttpClient, HttpClient, Response};
    use zode_account::{ApiCredential, PlanLimitReason};

    type Seen = Arc<Mutex<Vec<(String, String, Vec<(String, String)>, String)>>>;

    fn answering(status: u16, body: serde_json::Value) -> (SyncContext, Seen) {
        let seen: Seen = Arc::default();
        let recorded = seen.clone();
        let http_client: Arc<dyn HttpClient> = FakeHttpClient::create(move |request| {
            let recorded = recorded.clone();
            let body = body.clone();
            async move {
                use futures::AsyncReadExt as _;
                let method = request.method().to_string();
                let uri = request.uri().to_string();
                let headers = request
                    .headers()
                    .iter()
                    .filter(|(name, _)| name.as_str().starts_with("if-"))
                    .map(|(name, value)| {
                        (name.to_string(), value.to_str().unwrap_or("").to_string())
                    })
                    .collect();
                let mut sent = String::new();
                request.into_body().read_to_string(&mut sent).await.ok();
                recorded.lock().unwrap().push((method, uri, headers, sent));
                Ok(Response::builder()
                    .status(status)
                    .body(body.to_string().into())
                    .unwrap())
            }
        });
        (
            SyncContext {
                http_client,
                api_url: "https://api.example.invalid/api".into(),
                credential: ApiCredential {
                    access_token: "token".into(),
                    user_id: "68b1f0c2a4d3e5f60718293a".into(),
                },
            },
            seen,
        )
    }

    const TEAM: &str = "68b1f0c2a4d3e5f607182900";

    fn block<T>(future: impl std::future::Future<Output = T>) -> T {
        futures::executor::block_on(future)
    }

    fn some_key() -> [u8; KEY_LEN] {
        *crate::SharingKeypair::from_private_key(zeroize::Zeroizing::new([0x41; KEY_LEN]))
            .unwrap()
            .public_key()
    }

    #[test]
    fn a_key_pending_404_is_not_a_removal() {
        let (context, _) = answering(404, serde_json::json!({ "error": "key_pending" }));
        assert_eq!(
            block(fetch_team_key(&context, TEAM)).unwrap(),
            KeyAnswer::KeyPending
        );
        let (context, _) = answering(404, serde_json::json!({ "error": "not_found" }));
        assert_eq!(
            block(fetch_team_key(&context, TEAM)).unwrap(),
            KeyAnswer::NotMember
        );
    }

    #[test]
    fn a_registered_key_whose_fingerprint_lies_is_refused() {
        let public_key = some_key();
        let (context, _) = answering(
            200,
            serde_json::json!({
                "publicKey": encode_public_key(&public_key),
                "fingerprint": Fingerprint::of(&[9u8; KEY_LEN]).to_hex(),
            }),
        );
        assert!(matches!(
            block(fetch_registered_key(&context)),
            Err(TeamVaultError::Crypto(TeamCryptoError::Malformed(_)))
        ));

        let (context, _) = answering(
            200,
            serde_json::json!({
                "publicKey": encode_public_key(&public_key),
                "fingerprint": Fingerprint::of(&public_key).to_hex(),
            }),
        );
        assert_eq!(
            block(fetch_registered_key(&context)).unwrap(),
            Some(RegisteredKey {
                public_key,
                fingerprint: Fingerprint::of(&public_key)
            })
        );
    }

    #[test]
    fn registration_sends_its_precondition() {
        let public_key = some_key();
        let (context, seen) = answering(200, serde_json::json!({}));
        block(register_sharing_key(
            &context,
            &public_key,
            KeyRegistration::Create,
        ))
        .unwrap();
        let fingerprint = Fingerprint::of(&public_key);
        block(register_sharing_key(
            &context,
            &public_key,
            KeyRegistration::Rotate(&fingerprint),
        ))
        .unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].0, "PUT");
        assert!(seen[0].1.ends_with("/account/sharing-key"));
        assert_eq!(seen[0].2, vec![("if-none-match".into(), "*".into())]);
        assert_eq!(seen[1].2, vec![("if-match".into(), fingerprint.to_hex())]);
    }

    #[test]
    fn a_vault_write_tells_a_stale_version_from_a_conflict() {
        let entry = EntryId::parse(&"a1".repeat(16)).unwrap();
        let (context, _) = answering(409, serde_json::json!({ "error": "key_version_stale" }));
        assert_eq!(
            block(store_entry(
                &context,
                TEAM,
                &entry,
                "b",
                1,
                VaultPrecondition::Create
            ))
            .unwrap(),
            VaultWrite::KeyVersionStale
        );
        let (context, seen) = answering(
            409,
            serde_json::json!({ "error": "revision_mismatch", "blob": "x", "revision": "r9", "keyVersion": 2 }),
        );
        assert_eq!(
            block(store_entry(
                &context,
                TEAM,
                &entry,
                "b",
                2,
                VaultPrecondition::Replace("r1")
            ))
            .unwrap(),
            VaultWrite::Conflict(VaultDocument {
                blob: "x".into(),
                revision: "r9".into(),
                key_version: 2
            })
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].2, vec![("if-match".into(), "r1".into())]);
        let sent: serde_json::Value = serde_json::from_str(&seen[0].3).unwrap();
        assert_eq!(sent, serde_json::json!({ "blob": "b", "keyVersion": 2 }));
    }

    #[test]
    fn a_plan_refusal_on_a_team_route_is_the_plan_refusal() {
        let entry = EntryId::parse(&"a1".repeat(16)).unwrap();
        let (context, _) = answering(402, serde_json::json!({ "error": "team_inactive" }));
        match block(store_entry(
            &context,
            TEAM,
            &entry,
            "b",
            1,
            VaultPrecondition::Create,
        )) {
            Err(TeamVaultError::Client(ClientError::PlanLimit(limit))) => {
                assert_eq!(limit.reason, PlanLimitReason::TeamInactive)
            }
            other => panic!("expected a plan refusal, got {other:?}"),
        }
        let (context, _) = answering(401, serde_json::json!({}));
        assert_eq!(
            block(fetch_team_key(&context, TEAM)).err(),
            Some(TeamVaultError::Client(ClientError::Unauthorized))
        );
    }

    #[test]
    fn billing_off_is_neither_a_removal_nor_a_missing_entry() {
        let entry = EntryId::parse(&"a1".repeat(16)).unwrap();
        let (context, _) = answering(404, serde_json::json!({ "error": "billing_disabled" }));
        let refused = Some(TeamVaultError::Client(ClientError::Rejected(
            "billing_disabled".into(),
        )));
        assert_eq!(block(fetch_team_key(&context, TEAM)).err(), refused);
        assert_eq!(block(fetch_entry(&context, TEAM, &entry)).err(), refused);
        assert_eq!(block(fetch_registered_key(&context)).err(), refused);
        assert_eq!(block(list_vault(&context, TEAM)).err(), refused);
    }

    #[test]
    fn the_member_listing_carries_the_version_and_the_rotation_flag() {
        let public_key = some_key();
        let (context, _) = answering(
            200,
            serde_json::json!({
                "currentVersion": 3,
                "rotationRecommended": true,
                "members": [
                    { "userId": "u1", "email": "a@example.com", "publicKey": encode_public_key(&public_key),
                      "fingerprint": Fingerprint::of(&public_key).to_hex(), "hasCurrentEnvelope": true },
                    { "userId": "u2", "email": "b@example.com", "publicKey": null, "fingerprint": null,
                      "hasCurrentEnvelope": false },
                ]
            }),
        );
        let listed = block(fetch_member_keys(&context, TEAM)).unwrap().unwrap();
        assert_eq!(
            (listed.current_version, listed.rotation_recommended),
            (3, true)
        );
        assert_eq!(listed.members.len(), 2);
        assert!(listed.members[0].key.is_some() && listed.members[1].key.is_none());
    }

    #[test]
    fn only_a_stale_version_is_a_lost_rotation_race() {
        let (context, _) = answering(
            409,
            serde_json::json!({ "error": "key_version_stale", "currentVersion": 4 }),
        );
        assert_eq!(
            block(rotate_key(&context, TEAM, 3, &[])).unwrap(),
            RotateWrite::KeyVersionStale
        );
        let (context, _) = answering(
            409,
            serde_json::json!({ "error": "recipient_key_changed", "userId": "u2" }),
        );
        assert_eq!(
            block(rotate_key(&context, TEAM, 3, &[])).unwrap(),
            RotateWrite::RecipientKeyChanged {
                user_id: Some("u2".into())
            }
        );
        for slug in ["envelopes_incomplete", "admin_required"] {
            let (context, _) =
                answering(409, serde_json::json!({ "error": slug, "missing": ["u3"] }));
            assert_eq!(
                block(rotate_key(&context, TEAM, 3, &[])).err(),
                Some(TeamVaultError::Client(ClientError::Rejected(slug.into()))),
            );
        }
    }

    #[test]
    fn a_role_refusal_is_a_refusal_not_a_sign_out() {
        let (context, _) = answering(409, serde_json::json!({ "error": "admin_required" }));
        assert_eq!(
            block(put_envelopes(&context, TEAM, 1, &[])).err(),
            Some(TeamVaultError::Client(ClientError::Rejected(
                "admin_required".into()
            )))
        );
    }

    #[test]
    fn a_public_key_is_sent_in_its_one_canonical_spelling() {
        let encoded = encode_public_key(&some_key());
        assert_eq!(encoded.len(), 43);
        assert!(!encoded.contains('=') && !encoded.contains('+') && !encoded.contains('/'));
    }

    #[test]
    fn envelopes_travel_as_base64_with_the_echoed_fingerprint() {
        let (context, seen) = answering(200, serde_json::json!({}));
        let fingerprint = Fingerprint::of(&some_key());
        block(put_envelopes(
            &context,
            TEAM,
            3,
            &[OutgoingEnvelope {
                user_id: "u1".into(),
                recipient_fingerprint: fingerprint,
                wrapped_key: vec![0xff, 0x00, 0x80],
            }],
        ))
        .unwrap();
        let seen = seen.lock().unwrap();
        assert!(seen[0].1.ends_with(&format!("/teams/{TEAM}/key/envelopes")));
        let sent: serde_json::Value = serde_json::from_str(&seen[0].3).unwrap();
        assert_eq!(
            sent,
            serde_json::json!({
                "keyVersion": 3,
                "envelopes": [{ "userId": "u1", "recipientFingerprint": fingerprint.to_hex(), "wrappedKey": "/wCA" }]
            })
        );
    }
}
