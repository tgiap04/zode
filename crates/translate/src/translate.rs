//! Translating a piece of text through a service the user picked: DeepL,
//! Google Cloud Translation, any OpenAI-compatible chat completions API, or
//! Google's unofficial free endpoint.
//!
//! No UI lives here. Callers build a [`TranslateRequest`] and hand it to
//! [`translate`], which reads the settings and the API key, refuses what is too
//! long before touching the network, and gives up after the configured timeout.

mod credentials;
mod deepl;
mod google;
mod google_free;
mod language;
mod openai_compatible;
mod provider;
mod service;

#[cfg(test)]
mod provider_tests;
#[cfg(test)]
mod service_tests;

use std::time::Duration;

use gpui::App;
use settings::{RegisterSetting, Settings, SettingsContent};

pub use credentials::{credentials_url, delete_api_key, read_api_key, write_api_key};
pub use deepl::DeeplProvider;
pub use google::GoogleProvider;
pub use google_free::GoogleFreeProvider;
pub use language::{known_languages, language_name, normalize_language_code};
pub use openai_compatible::{OpenAiCompatibleProvider, validate_base_url};
pub use provider::{TranslateError, TranslationProvider};
pub use service::{TranslateService, translate};
pub use settings::TranslateProviderContent as ProviderKind;

pub fn init(cx: &mut App) {
    cx.set_global(TranslateService::default());
}

/// One piece of text to translate in a fixed direction. Language codes are
/// BCP 47 style (`en`, `vi`, `pt-BR`, `zh-Hans`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranslateRequest {
    pub text: String,
    pub from: String,
    pub to: String,
}

/// Google Cloud Translation takes at most 30,000 code points per request, the
/// smallest limit among the providers.
const MAX_CHARACTERS_LIMIT: usize = 30_000;
/// Zero would fail every request at once, and a huge value overflows the
/// platform timers.
const MIN_TIMEOUT_SECONDS: u64 = 1;
const MAX_TIMEOUT_SECONDS: u64 = 120;

#[derive(Clone, Debug, PartialEq, RegisterSetting)]
pub struct TranslateSettings {
    pub provider: ProviderKind,
    pub openai_compatible_base_url: String,
    pub openai_compatible_model: String,
    pub max_characters: usize,
    pub timeout: Duration,
}

impl Settings for TranslateSettings {
    fn from_settings(content: &SettingsContent) -> Self {
        // `unwrap()` is the documented contract of `Settings::from_settings`:
        // `default.json` carries every field, so a missing one means the
        // settings ritual (settings_content.rs + default.json together) was
        // not followed, and this is meant to panic until it is fixed.
        let content = content.translate.as_ref().unwrap();
        let openai_compatible = content.openai_compatible.as_ref().unwrap();
        Self {
            provider: content.provider.unwrap(),
            openai_compatible_base_url: openai_compatible.base_url.clone().unwrap(),
            openai_compatible_model: openai_compatible.model.clone().unwrap(),
            max_characters: content
                .max_characters
                .unwrap()
                .clamp(1, MAX_CHARACTERS_LIMIT),
            timeout: Duration::from_secs(
                content
                    .timeout_seconds
                    .unwrap()
                    .clamp(MIN_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS),
            ),
        }
    }
}

/// The name shown to the user for a provider.
pub fn provider_display_name(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Deepl => "DeepL",
        ProviderKind::Google => "Google Cloud Translation",
        ProviderKind::OpenaiCompatible => "OpenAI-compatible API",
        ProviderKind::GoogleFree => "Google Translate (free, no key)",
    }
}

/// Whether the provider keeps a key in the keychain at all.
pub fn provider_uses_api_key(kind: ProviderKind) -> bool {
    credentials_url(kind).is_some()
}

/// Whether the provider refuses to work without an API key. A local
/// OpenAI-compatible server (Ollama, LM Studio) usually needs none, and the
/// free Google endpoint takes none at all.
pub fn provider_requires_api_key(kind: ProviderKind) -> bool {
    match kind {
        ProviderKind::Deepl | ProviderKind::Google => true,
        ProviderKind::OpenaiCompatible | ProviderKind::GoogleFree => false,
    }
}
