use std::sync::Arc;

use futures::future::BoxFuture;
use http_client::{HttpClient, Method, Request};
use serde_json::json;

use crate::language::{deepl_source_code, deepl_target_code};
use crate::provider::{error_for_status, json_body, send, service_message, unexpected_response};
use crate::{TranslateError, TranslateRequest, TranslationProvider};

const NAME: &str = "DeepL";
const FREE_ENDPOINT: &str = "https://api-free.deepl.com/v2/translate";
const PRO_ENDPOINT: &str = "https://api.deepl.com/v2/translate";

pub struct DeeplProvider {
    api_key: String,
}

impl DeeplProvider {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    /// DeepL marks keys of the free plan with a `:fx` suffix, and those keys
    /// only work against the free host.
    pub fn endpoint(&self) -> &'static str {
        if self.api_key.trim().ends_with(":fx") {
            FREE_ENDPOINT
        } else {
            PRO_ENDPOINT
        }
    }

    fn build_request(
        &self,
        request: &TranslateRequest,
    ) -> Result<Request<http_client::AsyncBody>, TranslateError> {
        let unsupported = || TranslateError::UnsupportedPair {
            provider: NAME,
            from: request.from.clone(),
            to: request.to.clone(),
        };
        let source = deepl_source_code(&request.from).ok_or_else(unsupported)?;
        let target = deepl_target_code(&request.to).ok_or_else(unsupported)?;
        let body = json!({
            "text": [request.text],
            "source_lang": source,
            "target_lang": target,
            "preserve_formatting": true,
        });
        Request::builder()
            .method(Method::POST)
            .uri(self.endpoint())
            .header(
                "Authorization",
                format!("DeepL-Auth-Key {}", self.api_key.trim()),
            )
            .header("Content-Type", "application/json")
            .body(json_body(NAME, &body)?)
            .map_err(|error| TranslateError::InvalidConfiguration(error.to_string()))
    }
}

impl TranslationProvider for DeeplProvider {
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
                let message = service_message(&body);
                // DeepL answers 400 when a language it lists is not offered
                // for this account or model, e.g. "Value for 'target_lang'
                // not supported."
                if status.as_u16() == 400 && message.contains("_lang") {
                    return Err(TranslateError::UnsupportedPair {
                        provider: NAME,
                        from,
                        to,
                    });
                }
                return Err(error_for_status(NAME, status, &body));
            }
            let parsed: serde_json::Value = serde_json::from_str(&body)
                .map_err(|error| unexpected_response(NAME, &error.to_string()))?;
            parsed
                .pointer("/translations/0/text")
                .and_then(|text| text.as_str())
                .map(str::to_string)
                .ok_or_else(|| unexpected_response(NAME, "the answer holds no translation"))
        })
    }
}
