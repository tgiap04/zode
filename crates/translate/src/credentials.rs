use std::sync::Arc;

use anyhow::{Result, anyhow};
use credentials_provider::CredentialsProvider;
use gpui::{App, AsyncApp, Task};

use settings::Settings as _;

use crate::openai_compatible::key_origin;
use crate::{ProviderKind, TranslateError, TranslateSettings, provider_display_name};

/// Shown in the keychain's own UI next to the entry. DeepL and Google keys
/// are not tied to a server; the OpenAI-compatible key records the server it
/// was saved for in this field instead (see [`write_api_key`]).
const USERNAME: &str = "api-key";

/// The keychain entry for a provider's key. One entry per provider, so
/// switching providers keeps every key. `None` for a provider that takes no
/// key at all, which never touches the keychain.
pub fn credentials_url(kind: ProviderKind) -> Option<&'static str> {
    match kind {
        ProviderKind::Deepl => Some("zode-translate://deepl"),
        ProviderKind::Google => Some("zode-translate://google"),
        ProviderKind::OpenaiCompatible => Some("zode-translate://openai-compatible"),
        ProviderKind::GoogleFree => None,
    }
}

pub fn read_api_key(
    credentials: Arc<dyn CredentialsProvider>,
    kind: ProviderKind,
    cx: &App,
) -> Task<Result<Option<String>>> {
    cx.spawn(async move |cx: &mut AsyncApp| read_api_key_in(credentials.as_ref(), kind, cx).await)
}

/// Stores a key with surrounding whitespace removed, since keys are pasted
/// and a trailing newline would make every request fail as unauthorized.
///
/// The OpenAI-compatible key is saved together with the server the base URL
/// names right now, and is only ever sent there: editing the URL later to
/// another host would otherwise hand the key to that host.
pub fn write_api_key(
    credentials: Arc<dyn CredentialsProvider>,
    kind: ProviderKind,
    api_key: String,
    cx: &App,
) -> Task<Result<()>> {
    let Some(url) = credentials_url(kind) else {
        return Task::ready(Err(no_key_needed(kind)));
    };
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        return Task::ready(Err(anyhow!("The API key is empty.")));
    }
    let username = match kind {
        ProviderKind::OpenaiCompatible => {
            let base_url = &TranslateSettings::get_global(cx).openai_compatible_base_url;
            match key_origin(base_url) {
                Some(origin) => origin,
                None => {
                    return Task::ready(Err(anyhow!(
                        "Set a valid base URL for the OpenAI-compatible API before saving its key."
                    )));
                }
            }
        }
        _ => USERNAME.to_string(),
    };
    cx.spawn(async move |cx: &mut AsyncApp| {
        credentials
            .write_credentials(url, &username, api_key.as_bytes(), cx)
            .await
    })
}

pub fn delete_api_key(
    credentials: Arc<dyn CredentialsProvider>,
    kind: ProviderKind,
    cx: &App,
) -> Task<Result<()>> {
    let Some(url) = credentials_url(kind) else {
        return Task::ready(Ok(()));
    };
    cx.spawn(async move |cx: &mut AsyncApp| credentials.delete_credentials(url, cx).await)
}

/// The server name and the key stored for a provider.
async fn read_entry_in(
    credentials: &dyn CredentialsProvider,
    kind: ProviderKind,
    cx: &AsyncApp,
) -> Result<Option<(String, String)>> {
    let Some(url) = credentials_url(kind) else {
        return Ok(None);
    };
    let Some((username, password)) = credentials.read_credentials(url, cx).await? else {
        return Ok(None);
    };
    let api_key = String::from_utf8(password)?;
    let api_key = api_key.trim();
    Ok((!api_key.is_empty()).then(|| (username, api_key.to_string())))
}

pub(crate) async fn read_api_key_in(
    credentials: &dyn CredentialsProvider,
    kind: ProviderKind,
    cx: &AsyncApp,
) -> Result<Option<String>> {
    Ok(read_entry_in(credentials, kind, cx)
        .await?
        .map(|(_, api_key)| api_key))
}

/// The key to send with a request made under `settings`. An OpenAI-compatible
/// key is only handed out when it was saved for the server the base URL names
/// now; an entry that records no server (written by an older version) counts
/// as a different one.
pub(crate) async fn read_api_key_for_request_in(
    credentials: &dyn CredentialsProvider,
    settings: &TranslateSettings,
    cx: &AsyncApp,
) -> Result<Option<String>, TranslateError> {
    let provider = provider_display_name(settings.provider);
    let entry = read_entry_in(credentials, settings.provider, cx)
        .await
        .map_err(|error| TranslateError::Keychain {
            provider,
            message: error.to_string(),
        })?;
    let Some((saved_for, api_key)) = entry else {
        return Ok(None);
    };
    if settings.provider != ProviderKind::OpenaiCompatible {
        return Ok(Some(api_key));
    }
    // An unusable base URL fails the request before anything is sent.
    let Some(configured) = key_origin(&settings.openai_compatible_base_url) else {
        return Ok(None);
    };
    if saved_for == configured {
        return Ok(Some(api_key));
    }
    Err(TranslateError::KeyForOtherServer {
        provider,
        saved_for: if saved_for == USERNAME {
            "a server that was not recorded".to_string()
        } else {
            saved_for
        },
        configured,
    })
}

fn no_key_needed(kind: ProviderKind) -> anyhow::Error {
    anyhow!("{} needs no API key.", crate::provider_display_name(kind))
}
