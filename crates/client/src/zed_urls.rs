//! Contains a helper for constructing the URL to Zed's ACP registry blog post,
//! the one upstream Zed.dev link this fork still surfaces (see the extension
//! upsell banner in `extensions_ui`). The URL adapts to the configured
//! `server_url` setting rather than hardcoding zed.dev.

use gpui::App;
use settings::Settings;

use crate::ClientSettings;

fn server_url(cx: &App) -> &str {
    &ClientSettings::get_global(cx).server_url
}

/// Returns the URL to Zed's ACP registry blog post.
pub fn acp_registry_blog(cx: &App) -> String {
    format!(
        "{server_url}/blog/acp-registry",
        server_url = server_url(cx)
    )
}
