use std::sync::Arc;

use anyhow::{Result, anyhow};
use credentials_provider::CredentialsProvider;
use gpui::{App, AsyncApp, Task};

use crate::ProviderKind;

/// Shown in the keychain's own UI next to the entry; never read back.
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
    cx.spawn(async move |cx: &mut AsyncApp| {
        credentials
            .write_credentials(url, USERNAME, api_key.as_bytes(), cx)
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

pub(crate) async fn read_api_key_in(
    credentials: &dyn CredentialsProvider,
    kind: ProviderKind,
    cx: &AsyncApp,
) -> Result<Option<String>> {
    let Some(url) = credentials_url(kind) else {
        return Ok(None);
    };
    let Some((_username, password)) = credentials.read_credentials(url, cx).await? else {
        return Ok(None);
    };
    let api_key = String::from_utf8(password)?;
    let api_key = api_key.trim();
    Ok((!api_key.is_empty()).then(|| api_key.to_string()))
}

fn no_key_needed(kind: ProviderKind) -> anyhow::Error {
    anyhow!("{} needs no API key.", crate::provider_display_name(kind))
}
