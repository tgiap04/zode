//! The account behind Zode's optional sign-in.
//!
//! Signing in is never required to use the editor, and this crate is built so
//! that staying signed out costs nothing at all: **while signed out it makes no
//! network requests**, at startup or ever. Every request in here is the direct
//! consequence of something the user did.
//!
//! Layers, smallest first:
//!
//! | Module | Holds | Reaches |
//! |---|---|---|
//! | [`tokens`] | expiry arithmetic | nothing |
//! | [`device_flow`] | RFC 8628 conversation | HTTP |
//! | [`storage`] | the keychain entry | OS keychain |
//! | [`account`] | the state machine | both, via the above |
//!
//! `device_flow` and `tokens` take their dependencies as arguments precisely so
//! a test can drive them with `FakeHttpClient` and a fixed clock.

mod account;
mod device_flow;
mod storage;
mod tokens;

use std::sync::LazyLock;

use gpui::{App, AppContext as _, Entity};
use zed_env_vars::{EnvVar, env_var};

pub use account::{Account, AccountStatus, AccountStatusChanged, AccountUser, ApiCredential};
pub use device_flow::DeviceFlowError;
pub use tokens::StoredTokens;

/// Where the account service lives.
///
/// An environment variable rather than a settings key on purpose: pointing the
/// editor at a local backend is a developer action, not a user preference, and
/// a settings key would have to be threaded through the generated
/// `settings_content` schema for something no user should ever set. Same shape
/// as `ZED_DEVELOPMENT_USE_KEYCHAIN` in `zed_credentials_provider`.
static ZODE_API_URL: LazyLock<EnvVar> = env_var!("ZODE_API_URL");

/// Where the browser app lives, when it is not simply the API host without
/// its `api.` prefix. Set this when the two are hosted somewhere unrelated.
static ZODE_WEB_URL: LazyLock<EnvVar> = env_var!("ZODE_WEB_URL");

/// The API's own host, not the marketing site.
///
/// The browser app is served from `zodekit.site` and the API from
/// `api.zodekit.site`; a page can reach the second through CORS, but the IDE
/// is not a browser and has no reason to depend on whatever the site host
/// proxies. Going straight at the API is both simpler and one fewer thing that
/// can be misconfigured out from under a released binary.
const DEFAULT_API_URL: &str = "https://api.zodekit.site/api";

/// The browser app's origin — the marketing site and the account pages.
const DEFAULT_WEB_URL: &str = "https://zodekit.site";

pub fn api_url() -> String {
    ZODE_API_URL
        .value
        .clone()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_API_URL.to_string())
}

/// Where to send someone who wants to look at their account in a browser.
///
/// This used to be derived by stripping `/api` off [`api_url`], which was
/// correct only while the API answered on the site's own host. Pointing the
/// IDE at `api.zodekit.site` quietly turned "Account on the Web" into a link
/// to the API — a host that serves no pages at all.
///
/// The order below matters. When `ZODE_API_URL` is set but `ZODE_WEB_URL` is
/// not, the web URL is DERIVED from it rather than falling back to the
/// production default: a developer who pointed the editor at a local backend
/// must not be sent to the live site, which is a far worse failure than a
/// broken link.
pub fn web_url() -> String {
    if let Some(explicit) = ZODE_WEB_URL
        .value
        .clone()
        .filter(|url| !url.trim().is_empty())
    {
        return explicit;
    }
    match ZODE_API_URL
        .value
        .clone()
        .filter(|url| !url.trim().is_empty())
    {
        Some(api) => derive_web_url(&api),
        None => DEFAULT_WEB_URL.to_string(),
    }
}

/// Turns an API base into the origin that serves pages.
///
/// Two steps, both conventions rather than rules: drop a trailing `/api`
/// path, and drop a leading `api.` from the host. `ZODE_WEB_URL` exists for
/// every arrangement these two guesses do not describe.
fn derive_web_url(api_url: &str) -> String {
    let trimmed = api_url.trim_end_matches('/');
    let base = trimmed.strip_suffix("/api").unwrap_or(trimmed);

    let Some((scheme, rest)) = base.split_once("://") else {
        return base.to_string();
    };
    match rest.strip_prefix("api.") {
        Some(without) => format!("{scheme}://{without}"),
        None => base.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::derive_web_url;

    #[test]
    fn production_drops_both_the_path_and_the_api_subdomain() {
        assert_eq!(
            derive_web_url("https://api.zodekit.site/api"),
            "https://zodekit.site"
        );
    }

    #[test]
    fn a_local_backend_stays_local() {
        // The case the derivation exists to protect: a developer pointed at a
        // local server must never be handed a link to production.
        assert_eq!(
            derive_web_url("http://localhost:5173/api"),
            "http://localhost:5173"
        );
        assert_eq!(
            derive_web_url("http://127.0.0.1:8000/api"),
            "http://127.0.0.1:8000"
        );
    }

    #[test]
    fn a_host_that_merely_starts_with_api_is_left_alone() {
        // `apiary.example.com` is not `api.example.com`. Only the labelled
        // prefix is removed.
        assert_eq!(
            derive_web_url("https://apiary.example.com/api"),
            "https://apiary.example.com",
        );
    }

    #[test]
    fn a_trailing_slash_does_not_change_the_answer() {
        assert_eq!(
            derive_web_url("https://api.zodekit.site/api/"),
            "https://zodekit.site"
        );
    }

    #[test]
    fn a_base_without_the_api_path_still_works() {
        assert_eq!(
            derive_web_url("https://api.zodekit.site"),
            "https://zodekit.site"
        );
    }
}

/// Installs the global [`Account`] and restores a saved session.
///
/// `restore` reads the keychain before it touches the network, so a machine
/// that has never signed in issues no request from this call.
///
/// Returns the entity so `zode_sync::init` can be handed the same one rather
/// than reaching for the global and having to cope with it being absent.
pub fn init(cx: &mut App) -> Entity<Account> {
    let http_client = cx.http_client();
    let credentials = zed_credentials_provider::global(cx);

    let account = cx.new(|cx| {
        let mut account = Account::new(http_client, credentials, api_url());
        account.restore(cx).detach();
        account
    });

    Account::set_global(account.clone(), cx);
    account
}
