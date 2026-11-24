use client::ZODE_URL_SCHEME;
use gpui::{AsyncApp, actions};

actions!(
    cli,
    [
        /// Registers the zode:// URL scheme handler.
        RegisterZodeScheme
    ]
);

pub async fn register_zode_scheme(cx: &AsyncApp) -> anyhow::Result<()> {
    cx.update(|cx| cx.register_url_scheme(ZODE_URL_SCHEME))
        .await
}
