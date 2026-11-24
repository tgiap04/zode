//! What the GitHub tab last found about its account, and whether "Sign in
//! with GitHub" is offered, kept apart from the tab so the settings page can
//! read both without reaching the tab or GitHub.

use gpui::{App, Global, SharedString};

use crate::token::TokenSource;

/// The zodekit OAuth App's `client_id`, which has device flow switched on. It
/// is public: device flow needs no client secret.
pub const BUILT_IN_CLIENT_ID: &str = "Ov23li05bNA03JOhTAvk";
/// Read only by development builds: a release build would otherwise let
/// whoever sets Zode's environment pick the OAuth App users consent to.
pub const CLIENT_ID_VARIABLE: &str = "ZODE_GITHUB_CLIENT_ID";

/// Kept only when it could be an OAuth `client_id`, since it ends up in a
/// URL path. The variable wins over the built-in value.
pub fn client_id(built_in: &str, variable: Option<String>) -> Option<String> {
    let variable = variable.map(|value| value.trim().to_string());
    [variable.as_deref(), Some(built_in.trim())]
        .into_iter()
        .flatten()
        .find(|id| !id.is_empty())
        .filter(|id| {
            id.len() <= 64
                && id
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
        })
        .map(str::to_string)
}

/// The `client_id` a build uses: a release build ignores the variable.
pub fn client_id_for_build(
    built_in: &str,
    variable: Option<String>,
    development_build: bool,
) -> Option<String> {
    client_id(built_in, variable.filter(|_| development_build))
}

/// The `client_id` in use, installed at start-up (or by a test). `None`
/// hides "Sign in with GitHub".
pub struct GithubClientId(pub Option<String>);

impl Global for GithubClientId {}

/// The installed `client_id`, or the built-in one when none was installed.
pub fn configured_client_id(cx: &App) -> Option<String> {
    match cx.try_global::<GithubClientId>() {
        Some(installed) => installed.0.clone(),
        None => client_id(BUILT_IN_CLIENT_ID, None),
    }
}

/// Whether "Sign in with GitHub" is offered: only with a `client_id`.
pub fn sign_in_with_github_available(cx: &App) -> bool {
    configured_client_id(cx).is_some()
}

/// GitHub's page for revoking the app's access, where a token Zode cannot
/// revoke itself (that needs the app's secret) is really ended.
pub fn revoke_page(cx: &App) -> Option<String> {
    configured_client_id(cx)
        .map(|id| format!("https://github.com/settings/connections/applications/{id}"))
}

pub fn token_source_label(source: TokenSource) -> String {
    match source {
        TokenSource::Keychain => "Zode".to_string(),
        TokenSource::GhCli => "GitHub CLI".to_string(),
        TokenSource::GhEnvironment(variable) => format!("{} variable", variable.name()),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SignInStatus {
    /// No GitHub tab has looked for a token since Zode started.
    #[default]
    NotChecked,
    Checking,
    SigningIn,
    SignedOut,
    SignedIn {
        login: SharedString,
        source: TokenSource,
    },
    Revoked,
    Failed,
}

impl SignInStatus {
    pub fn describe(&self) -> String {
        match self {
            Self::NotChecked => {
                "Not checked yet. Zode looks for a token when you open the GitHub tab.".to_string()
            }
            Self::Checking => "Checking GitHub…".to_string(),
            Self::SigningIn => "Signing in: finish in the GitHub tab.".to_string(),
            Self::SignedOut => "Not signed in.".to_string(),
            Self::SignedIn { login, source } => {
                format!("Signed in as @{login} via {}.", token_source_label(*source))
            }
            Self::Revoked => {
                "GitHub no longer accepts the token. Open the GitHub tab to sign in again."
                    .to_string()
            }
            Self::Failed => {
                "The last check could not reach GitHub. The GitHub tab says why.".to_string()
            }
        }
    }
}

/// The global behind [`sign_in_status`]; observe it to redraw on a change.
#[derive(Default)]
pub struct GithubSignInStatus(SignInStatus);

impl Global for GithubSignInStatus {}

pub fn sign_in_status(cx: &App) -> SignInStatus {
    cx.try_global::<GithubSignInStatus>()
        .map(|status| status.0.clone())
        .unwrap_or_default()
}

/// Setting a global wakes its observers, so an unchanged status is skipped.
pub fn publish_sign_in_status(status: SignInStatus, cx: &mut App) {
    if sign_in_status(cx) != status {
        cx.set_global(GithubSignInStatus(status));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_client_id_variable_wins_and_an_empty_one_hides_sign_in() {
        assert_eq!(client_id("", None), None);
        assert_eq!(client_id("", Some("  ".into())), None);
        assert_eq!(client_id("Iv1.built", None).as_deref(), Some("Iv1.built"));
        assert_eq!(
            client_id("Iv1.built", Some("Ov23liOverride".into())).as_deref(),
            Some("Ov23liOverride")
        );
        assert_eq!(client_id("", Some("x/../y".into())), None, "never a path");
    }

    #[test]
    fn only_a_development_build_reads_the_client_id_variable() {
        let variable = || Some("Ov23liOverride".to_string());
        assert_eq!(
            client_id_for_build("Iv1.built", variable(), true).as_deref(),
            Some("Ov23liOverride")
        );
        assert_eq!(
            client_id_for_build("Iv1.built", variable(), false).as_deref(),
            Some("Iv1.built"),
            "a release build keeps the built-in app"
        );
        assert_eq!(
            client_id_for_build("", variable(), false),
            None,
            "and offers no device flow without one"
        );
    }

    #[test]
    fn a_release_build_offers_sign_in_with_the_zodekit_app() {
        assert_eq!(
            client_id_for_build(BUILT_IN_CLIENT_ID, None, false).as_deref(),
            Some("Ov23li05bNA03JOhTAvk")
        );
    }

    #[gpui::test]
    fn without_an_installed_client_id_only_the_built_in_one_counts(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            assert_eq!(
                configured_client_id(cx),
                client_id(BUILT_IN_CLIENT_ID, None)
            );
            cx.set_global(GithubClientId(Some("Iv1.installed".into())));
            assert!(sign_in_with_github_available(cx));
            assert_eq!(
                revoke_page(cx).as_deref(),
                Some("https://github.com/settings/connections/applications/Iv1.installed")
            );
            cx.set_global(GithubClientId(None));
            assert!(!sign_in_with_github_available(cx));
        });
    }
}
