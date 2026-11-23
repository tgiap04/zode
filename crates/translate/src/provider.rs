use std::sync::Arc;

use futures::{AsyncReadExt as _, future::BoxFuture};
use http_client::{AsyncBody, HttpClient, RedirectPolicy, Request, StatusCode};

use crate::TranslateRequest;

/// A service that turns a [`TranslateRequest`] into translated text.
///
/// Providers are built per request with the key already read, so the trait
/// has no access to the keychain or settings and a test can supply its own.
pub trait TranslationProvider: Send + Sync {
    fn name(&self) -> &'static str;

    fn translate(
        &self,
        request: &TranslateRequest,
        http_client: Arc<dyn HttpClient>,
    ) -> BoxFuture<'static, Result<String, TranslateError>>;
}

/// Every message here is meant to be shown to the user as is.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TranslateError {
    #[error("Nothing is selected to translate.")]
    EmptyText,
    #[error(
        "The selection is {length} characters long and the limit is {limit}. Select less text or raise the limit in Settings → Translate."
    )]
    TooLong { length: usize, limit: usize },
    #[error("\"{code}\" is not a language code. Use a code such as \"en\", \"vi\" or \"pt-BR\".")]
    InvalidLanguage { code: String },
    #[error("{provider} cannot translate from \"{from}\" to \"{to}\".")]
    UnsupportedPair {
        provider: &'static str,
        from: String,
        to: String,
    },
    #[error("No {provider} API key is set. Add one in Settings → Translate.")]
    MissingKey { provider: &'static str },
    #[error(
        "{provider} rejected the API key (HTTP {status}). Check the key in Settings → Translate."
    )]
    InvalidKey { provider: &'static str, status: u16 },
    #[error(
        "{provider} refused the request because a quota or rate limit was reached (HTTP {status}). Try again later or check your plan."
    )]
    QuotaExceeded { provider: &'static str, status: u16 },
    #[error(
        "Google is limiting free requests right now (HTTP {status}); try again later or use a provider with a key in Settings → Translate."
    )]
    FreeEndpointThrottled { status: u16 },
    #[error("{0}")]
    InvalidConfiguration(String),
    #[error("{provider} refused the request (HTTP {status}): {message}")]
    Forbidden {
        provider: &'static str,
        status: u16,
        message: String,
    },
    #[error(
        "{provider} answered with a redirect (HTTP {status}). Zode does not follow it, so the text is not sent to another address. Check the URL in Settings → Translate."
    )]
    Redirected { provider: &'static str, status: u16 },
    #[error("{provider} did not answer within {seconds} seconds.")]
    Timeout {
        provider: &'static str,
        seconds: u64,
    },
    #[error("Could not reach {provider}: {message}")]
    Network {
        provider: &'static str,
        message: String,
    },
    #[error("{provider} answered with HTTP {status}: {message}")]
    Service {
        provider: &'static str,
        status: u16,
        message: String,
    },
    #[error("{provider} sent an answer Zode could not read: {message}")]
    UnexpectedResponse {
        provider: &'static str,
        message: String,
    },
    #[error(
        "{provider} sent an answer larger than {limit_mebibytes} MiB, which is too large to be a translation. Zode did not use it."
    )]
    AnswerTooLarge {
        provider: &'static str,
        limit_mebibytes: u64,
    },
    #[error(
        "The {provider} API key was saved for {saved_for}, but the base URL now points to {configured}. Zode does not send a key to a different server: enter the key again in Settings → Translate."
    )]
    KeyForOtherServer {
        provider: &'static str,
        saved_for: String,
        configured: String,
    },
    #[error("Could not read the {provider} API key from the system keychain: {message}")]
    Keychain {
        provider: &'static str,
        message: String,
    },
}

/// A translation is a few kilobytes at most; anything far bigger is not an
/// answer worth holding in memory.
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

/// Longest service message passed on to the user.
const MAX_MESSAGE_CHARACTERS: usize = 300;

/// Sends the request and reads the whole body. A redirect is refused rather
/// than followed: the client would re-send the selection to wherever the
/// server points, possibly over plain http, and keep headers such as
/// `X-Goog-Api-Key` on the way.
pub(crate) async fn send(
    provider: &'static str,
    http_client: Arc<dyn HttpClient>,
    mut request: Request<AsyncBody>,
) -> Result<(StatusCode, String), TranslateError> {
    request.extensions_mut().insert(RedirectPolicy::NoFollow);
    let network_error = |error: &dyn std::fmt::Display| TranslateError::Network {
        provider,
        message: error.to_string(),
    };
    let mut response = http_client
        .send(request)
        .await
        .map_err(|error| network_error(&error))?;
    if response.status().is_redirection() {
        return Err(TranslateError::Redirected {
            provider,
            status: response.status().as_u16(),
        });
    }
    let mut bytes = Vec::new();
    // One byte past the cap tells a body that is exactly the cap from a longer one.
    response
        .body_mut()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| network_error(&error))?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        // A cut translation would be shown and written over the user's text
        // as if whole. A cut error page is still good for its first lines.
        if response.status().is_success() {
            return Err(TranslateError::AnswerTooLarge {
                provider,
                limit_mebibytes: MAX_RESPONSE_BYTES / (1024 * 1024),
            });
        }
        bytes.truncate(MAX_RESPONSE_BYTES as usize);
    }
    Ok((
        response.status(),
        String::from_utf8_lossy(&bytes).into_owned(),
    ))
}

/// The error for a failed status that no provider gives a special meaning.
pub(crate) fn error_for_status(
    provider: &'static str,
    status: StatusCode,
    body: &str,
) -> TranslateError {
    let status = status.as_u16();
    match status {
        401 | 403 => TranslateError::InvalidKey { provider, status },
        429 | 456 => TranslateError::QuotaExceeded { provider, status },
        _ => TranslateError::Service {
            provider,
            status,
            message: service_message(body),
        },
    }
}

/// The human part of an error body: DeepL puts it in `message`, Google and
/// OpenAI in `error.message`. Anything else is passed on trimmed and cut short.
pub(crate) fn service_message(body: &str) -> String {
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok();
    let message = parsed.as_ref().and_then(|value| {
        value
            .pointer("/error/message")
            .or_else(|| value.get("message"))
            .and_then(|message| message.as_str())
    });
    let message = message.unwrap_or(body).trim();
    if message.is_empty() {
        return "no details given".to_string();
    }
    match message.char_indices().nth(MAX_MESSAGE_CHARACTERS) {
        Some((cut, _)) => format!("{}…", &message[..cut]),
        None => message.to_string(),
    }
}

pub(crate) fn json_body(
    provider: &'static str,
    body: &serde_json::Value,
) -> Result<AsyncBody, TranslateError> {
    serde_json::to_vec(body)
        .map(AsyncBody::from)
        .map_err(|error| TranslateError::UnexpectedResponse {
            provider,
            message: error.to_string(),
        })
}

pub(crate) fn unexpected_response(provider: &'static str, what: &str) -> TranslateError {
    TranslateError::UnexpectedResponse {
        provider,
        message: what.to_string(),
    }
}
