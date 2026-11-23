use std::sync::Arc;

use futures::future::BoxFuture;
use http_client::{AsyncBody, HttpClient, Method, Request, StatusCode};
use url::form_urlencoded;

use crate::google::google_code;
use crate::provider::{send, service_message, unexpected_response};
use crate::{TranslateError, TranslateRequest, TranslationProvider};

const NAME: &str = "Google Translate (free, no key)";
const ENDPOINT: &str = "https://translate.googleapis.com/translate_a/single";

/// The unofficial endpoint behind Google's web widgets. It needs no key, so
/// Google may throttle or close it at any time; it is an opt-in provider and
/// never the default.
///
/// The text goes in a form body rather than the query, so a long selection
/// does not run into URL length limits and never shows up in a logged URL.
pub struct GoogleFreeProvider;

impl GoogleFreeProvider {
    fn build_request(request: &TranslateRequest) -> Result<Request<AsyncBody>, TranslateError> {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("client", "gtx")
            .append_pair("sl", google_code(&request.from))
            .append_pair("tl", google_code(&request.to))
            .append_pair("dt", "t")
            .finish();
        let body = form_urlencoded::Serializer::new(String::new())
            .append_pair("q", &request.text)
            .finish();
        Request::builder()
            .method(Method::POST)
            .uri(format!("{ENDPOINT}?{query}"))
            .header(
                "Content-Type",
                "application/x-www-form-urlencoded;charset=utf-8",
            )
            .body(AsyncBody::from(body))
            .map_err(|error| TranslateError::InvalidConfiguration(error.to_string()))
    }
}

impl TranslationProvider for GoogleFreeProvider {
    fn name(&self) -> &'static str {
        NAME
    }

    fn translate(
        &self,
        request: &TranslateRequest,
        http_client: Arc<dyn HttpClient>,
    ) -> BoxFuture<'static, Result<String, TranslateError>> {
        let http_request = Self::build_request(request);
        Box::pin(async move {
            let (status, body) = send(NAME, http_client, http_request?).await?;
            if !status.is_success() {
                return Err(error_for_status(status, &body));
            }
            parse_translation(&body)
        })
    }
}

/// There is no key to blame, so a refusal is either throttling or a plain
/// service error. Google answers throttling with an HTML "unusual traffic"
/// page, which is no use to show.
fn error_for_status(status: StatusCode, body: &str) -> TranslateError {
    let status = status.as_u16();
    if matches!(status, 429 | 503) {
        return TranslateError::FreeEndpointThrottled { status };
    }
    let message = if body.trim_start().starts_with('<') {
        "no details given".to_string()
    } else {
        service_message(body)
    };
    TranslateError::Service {
        provider: NAME,
        status,
        message,
    }
}

/// The answer is a nested array whose first element holds one
/// `[translation, original, ...]` row per sentence. A row whose translation
/// is `null` carries something else (such as a transliteration) and is
/// skipped; any other shape means the endpoint changed.
fn parse_translation(body: &str) -> Result<String, TranslateError> {
    let unreadable = || unexpected_response(NAME, "the answer is not in the expected shape");
    let parsed: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| unexpected_response(NAME, &error.to_string()))?;
    let rows = parsed
        .get(0)
        .and_then(|rows| rows.as_array())
        .ok_or_else(unreadable)?;
    let mut translation = String::new();
    let mut found = false;
    for row in rows {
        match row.as_array().and_then(|row| row.first()) {
            Some(serde_json::Value::String(segment)) => {
                translation.push_str(segment);
                found = true;
            }
            Some(serde_json::Value::Null) => {}
            _ => return Err(unreadable()),
        }
    }
    if !found {
        return Err(unexpected_response(NAME, "the answer holds no translation"));
    }
    Ok(translation)
}
