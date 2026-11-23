use std::sync::Arc;

use futures::future::{Either, select};
use gpui::{App, AsyncApp, Global, Task};
use settings::Settings as _;

use crate::credentials::read_api_key_in;
use crate::{
    DeeplProvider, GoogleFreeProvider, GoogleProvider, OpenAiCompatibleProvider, ProviderKind,
    TranslateError, TranslateRequest, TranslateSettings, TranslationProvider,
    normalize_language_code, provider_display_name,
};

/// Holds what [`translate`] cannot read from settings. Today that is only a
/// provider that replaces the configured one, for tests.
#[derive(Default)]
pub struct TranslateService {
    provider_override: Option<Arc<dyn TranslationProvider>>,
}

impl Global for TranslateService {}

impl TranslateService {
    pub fn with_provider(provider: Arc<dyn TranslationProvider>) -> Self {
        Self {
            provider_override: Some(provider),
        }
    }
}

/// Translates with the provider the settings name.
///
/// Blank text, a text over `max_characters` and a malformed language code are
/// refused before the keychain or the network is touched. Dropping the task
/// cancels the request.
pub fn translate(request: TranslateRequest, cx: &App) -> Task<Result<String, TranslateError>> {
    let settings = TranslateSettings::get_global(cx).clone();
    let request = match prepare(request, settings.max_characters) {
        Ok(request) => request,
        Err(error) => return Task::ready(Err(error)),
    };
    let provider_override = cx
        .try_global::<TranslateService>()
        .and_then(|service| service.provider_override.clone());
    let credentials = zode_credentials_provider::global(cx);
    let http_client = cx.http_client();
    cx.spawn(async move |cx: &mut AsyncApp| {
        let provider = match provider_override {
            Some(provider) => provider,
            None => {
                let api_key = read_api_key_in(credentials.as_ref(), settings.provider, cx)
                    .await
                    .map_err(|error| TranslateError::Keychain {
                        provider: provider_display_name(settings.provider),
                        message: error.to_string(),
                    })?;
                configured_provider(&settings, api_key)?
            }
        };
        let translation = provider.translate(&request, http_client);
        let timer = cx.background_executor().timer(settings.timeout);
        match select(translation, timer).await {
            Either::Left((result, _)) => result,
            Either::Right(_) => Err(TranslateError::Timeout {
                provider: provider.name(),
                seconds: settings.timeout.as_secs(),
            }),
        }
    })
}

fn prepare(
    request: TranslateRequest,
    max_characters: usize,
) -> Result<TranslateRequest, TranslateError> {
    if request.text.trim().is_empty() {
        return Err(TranslateError::EmptyText);
    }
    let length = request.text.chars().count();
    if length > max_characters {
        return Err(TranslateError::TooLong {
            length,
            limit: max_characters,
        });
    }
    Ok(TranslateRequest {
        from: normalize_language_code(&request.from)?,
        to: normalize_language_code(&request.to)?,
        text: request.text,
    })
}

fn configured_provider(
    settings: &TranslateSettings,
    api_key: Option<String>,
) -> Result<Arc<dyn TranslationProvider>, TranslateError> {
    let missing_key = || TranslateError::MissingKey {
        provider: provider_display_name(settings.provider),
    };
    Ok(match settings.provider {
        ProviderKind::Deepl => Arc::new(DeeplProvider::new(api_key.ok_or_else(missing_key)?)),
        ProviderKind::Google => Arc::new(GoogleProvider::new(api_key.ok_or_else(missing_key)?)),
        ProviderKind::OpenaiCompatible => Arc::new(OpenAiCompatibleProvider::new(
            &settings.openai_compatible_base_url,
            &settings.openai_compatible_model,
            api_key,
        )?),
        ProviderKind::GoogleFree => Arc::new(GoogleFreeProvider),
    })
}
