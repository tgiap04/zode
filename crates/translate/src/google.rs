use std::sync::Arc;

use futures::future::BoxFuture;
use http_client::{AsyncBody, HttpClient, Method, Request};
use serde_json::json;

use crate::provider::{error_for_status, json_body, send, service_message, unexpected_response};
use crate::{TranslateError, TranslateRequest, TranslationProvider};

const NAME: &str = "Google Cloud Translation";
const ENDPOINT: &str = "https://translation.googleapis.com/language/translate/v2";

/// Google Cloud Translation v2 with an API key. The key travels in the
/// `X-Goog-Api-Key` header rather than the `key` query parameter, so it never
/// shows up in a URL that something might print.
pub struct GoogleProvider {
    api_key: String,
}

impl GoogleProvider {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    fn build_request(
        &self,
        request: &TranslateRequest,
    ) -> Result<Request<AsyncBody>, TranslateError> {
        let body = json!({
            "q": [request.text],
            "source": google_code(&request.from),
            "target": google_code(&request.to),
            "format": "text",
        });
        Request::builder()
            .method(Method::POST)
            .uri(ENDPOINT)
            .header("X-Goog-Api-Key", self.api_key.trim())
            .header("Content-Type", "application/json")
            .body(json_body(NAME, &body)?)
            .map_err(|error| TranslateError::InvalidConfiguration(error.to_string()))
    }
}

/// Google v2 names Chinese scripts by region.
pub(crate) fn google_code(code: &str) -> &str {
    match code {
        "zh-Hans" => "zh-CN",
        "zh-Hant" => "zh-TW",
        other => other,
    }
}

impl TranslationProvider for GoogleProvider {
    fn name(&self) -> &'static str {
        NAME
    }

    fn translate(
        &self,
        request: &TranslateRequest,
        http_client: Arc<dyn HttpClient>,
    ) -> BoxFuture<'static, Result<String, TranslateError>> {
        let http_request = self.build_request(request);
        let from = request.from.clone();
        let to = request.to.clone();
        Box::pin(async move {
            let (status, body) = send(NAME, http_client, http_request?).await?;
            if !status.is_success() {
                // Google answers a bad key with 400, not 401, and says so only
                // in the body.
                if status.as_u16() == 400 {
                    if body.contains("API_KEY_INVALID") {
                        return Err(TranslateError::InvalidKey {
                            provider: NAME,
                            status: 400,
                        });
                    }
                    let message = service_message(&body).to_ascii_lowercase();
                    if message.contains("language") {
                        return Err(TranslateError::UnsupportedPair {
                            provider: NAME,
                            from,
                            to,
                        });
                    }
                }
                // 403 usually means the key is fine but its project has the
                // API switched off or no billing account; only the body says
                // which.
                if status.as_u16() == 403 {
                    return Err(TranslateError::Forbidden {
                        provider: NAME,
                        status: 403,
                        message: service_message(&body),
                    });
                }
                return Err(error_for_status(NAME, status, &body));
            }
            let parsed: serde_json::Value = serde_json::from_str(&body)
                .map_err(|error| unexpected_response(NAME, &error.to_string()))?;
            parsed
                .pointer("/data/translations/0/translatedText")
                .and_then(|text| text.as_str())
                .map(str::to_string)
                .ok_or_else(|| unexpected_response(NAME, "the answer holds no translation"))
        })
    }
}
