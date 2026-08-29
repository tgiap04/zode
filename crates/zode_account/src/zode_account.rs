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

use gpui::{App, AppContext as _};
use zed_env_vars::{EnvVar, env_var};

pub use account::{Account, AccountStatus, AccountStatusChanged, AccountUser};
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

const DEFAULT_API_URL: &str = "https://zodekit.site/api";

pub fn api_url() -> String {
    ZODE_API_URL
        .value
        .clone()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_API_URL.to_string())
}

/// Installs the global [`Account`] and restores a saved session.
///
/// `restore` reads the keychain before it touches the network, so a machine
/// that has never signed in issues no request from this call.
pub fn init(cx: &mut App) {
    let http_client = cx.http_client();
    let credentials = zed_credentials_provider::global(cx);

    let account = cx.new(|cx| {
        let mut account = Account::new(http_client, credentials, api_url());
        account.restore(cx).detach();
        account
    });

    Account::set_global(account, cx);
}
