use std::sync::Arc;

use futures::AsyncReadExt as _;
use http_client::{AsyncBody, HttpClient, Request};
use serde::{Deserialize, Serialize};

use crate::envelope::Kind;

/// One stored artifact as the server describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteDocument {
    /// Base64 of the envelope. Still encrypted at this layer.
    pub blob: String,
    pub revision: String,
}

/// What went wrong talking to the sync store.
///
/// `Unreachable` and `Rejected` are kept apart for the same reason
/// `DeviceFlowError` keeps them apart: one means try again later, the other
/// means something is actually wrong, and a UI that conflates them tells a
/// user on a train that their account is broken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientError {
    Unreachable(String),
    /// The session was refused. The caller should stop rather than retry.
    Unauthorized,
    Rejected(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(detail) => write!(f, "the sync service is unreachable: {detail}"),
            Self::Unauthorized => write!(f, "the sync service refused this session"),
            Self::Rejected(detail) => write!(f, "the sync service rejected the request: {detail}"),
        }
    }
}

/// What a conditional write can come back as.
pub enum WriteOutcome {
    Stored {
        revision: String,
    },
    /// Someone else wrote first. The current document travels with the answer
    /// so a diff can be built without a second request — and without a second
    /// window in which a third write could land.
    Conflict(RemoteDocument),
    /// `If-Match` named a document the server no longer has.
    Gone,
}

/// The precondition the caller asserts, mirroring the two HTTP headers.
pub enum Precondition<'a> {
    Create,
    Replace(&'a str),
}

#[derive(Serialize)]
struct PutBody<'a> {
    blob: &'a str,
}

#[derive(Deserialize)]
struct DocumentResponse {
    blob: String,
    revision: String,
}

#[derive(Deserialize)]
struct StoredResponse {
    revision: String,
}

/// Fetches one artifact. `Ok(None)` means the user has never pushed this kind.
pub async fn fetch(
    http_client: &Arc<dyn HttpClient>,
    api_url: &str,
    access_token: &str,
    kind: Kind,
) -> Result<Option<RemoteDocument>, ClientError> {
    let request = build(api_url, access_token, kind, "GET")?
        .body(AsyncBody::default())
        .map_err(|_| ClientError::Unreachable("the request could not be built".into()))?;

    let (status, body) = send(http_client, request).await?;

    if status == 404 {
        return Ok(None);
    }
    classify(status, &body)?;

    let parsed: DocumentResponse = serde_json::from_str(&body).map_err(|_| {
        ClientError::Rejected("the sync service answered with unusable JSON".into())
    })?;
    Ok(Some(RemoteDocument {
        blob: parsed.blob,
        revision: parsed.revision,
    }))
}

/// Stores one artifact, conditional on what the caller believes is there.
///
/// There is no unconditional variant, and there must never be one: the server
/// cannot merge two versions because it cannot read them, so an unconditional
/// write is just a silent way to destroy the other machine's work.
pub async fn store(
    http_client: &Arc<dyn HttpClient>,
    api_url: &str,
    access_token: &str,
    kind: Kind,
    blob: &str,
    precondition: Precondition<'_>,
) -> Result<WriteOutcome, ClientError> {
    let payload = serde_json::to_string(&PutBody { blob })
        .map_err(|_| ClientError::Unreachable("the request could not be built".into()))?;

    let builder =
        build(api_url, access_token, kind, "PUT")?.header("Content-Type", "application/json");
    let builder = match precondition {
        Precondition::Create => builder.header("If-None-Match", "*"),
        Precondition::Replace(revision) => builder.header("If-Match", revision),
    };

    let request = builder
        .body(AsyncBody::from(payload))
        .map_err(|_| ClientError::Unreachable("the request could not be built".into()))?;

    let (status, body) = send(http_client, request).await?;

    if status == 409 {
        let parsed: DocumentResponse = serde_json::from_str(&body).map_err(|_| {
            ClientError::Rejected("the sync service answered a conflict without a document".into())
        })?;
        return Ok(WriteOutcome::Conflict(RemoteDocument {
            blob: parsed.blob,
            revision: parsed.revision,
        }));
    }
    if status == 404 {
        return Ok(WriteOutcome::Gone);
    }
    classify(status, &body)?;

    let parsed: StoredResponse = serde_json::from_str(&body).map_err(|_| {
        ClientError::Rejected("the sync service answered with unusable JSON".into())
    })?;
    Ok(WriteOutcome::Stored {
        revision: parsed.revision,
    })
}

/// Forgets one artifact server-side. Idempotent, like the endpoint.
pub async fn forget(
    http_client: &Arc<dyn HttpClient>,
    api_url: &str,
    access_token: &str,
    kind: Kind,
) -> Result<(), ClientError> {
    let request = build(api_url, access_token, kind, "DELETE")?
        .body(AsyncBody::default())
        .map_err(|_| ClientError::Unreachable("the request could not be built".into()))?;

    let (status, body) = send(http_client, request).await?;
    if status == 404 {
        return Ok(());
    }
    classify(status, &body)
}

fn build(
    api_url: &str,
    access_token: &str,
    kind: Kind,
    method: &str,
) -> Result<http_client::http::request::Builder, ClientError> {
    Ok(Request::builder()
        .method(method)
        .uri(format!("{api_url}/sync/{kind}"))
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {access_token}")))
}

async fn send(
    http_client: &Arc<dyn HttpClient>,
    request: http_client::http::Request<AsyncBody>,
) -> Result<(u16, String), ClientError> {
    let mut response = http_client
        .send(request)
        .await
        .map_err(|_| ClientError::Unreachable("the endpoint could not be reached".into()))?;

    let status = response.status().as_u16();
    let mut body = String::new();
    if response.body_mut().read_to_string(&mut body).await.is_err() {
        return Err(ClientError::Unreachable(
            "the response could not be read".into(),
        ));
    }
    Ok((status, body))
}

fn classify(status: u16, body: &str) -> Result<(), ClientError> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    if status == 401 || status == 403 {
        return Err(ClientError::Unauthorized);
    }
    if status >= 500 {
        return Err(ClientError::Unreachable(format!(
            "the server answered {status}"
        )));
    }

    // The error slug only, never the whole body: a body from an authenticated
    // endpoint is exactly the sort of thing that echoes a credential into a
    // log. Same rule as `device_flow::post_json`.
    #[derive(Deserialize)]
    struct ErrorBody {
        error: String,
    }
    let slug = serde_json::from_str::<ErrorBody>(body)
        .map(|parsed| parsed.error)
        .unwrap_or_else(|_| format!("http_{status}"));
    Err(ClientError::Rejected(slug))
}
