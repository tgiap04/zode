use std::time::{Duration, SystemTime};

/// Every message here is meant to be shown to the user as is. None of them
/// carries the token: the variants hold only what GitHub or the system said.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GithubError {
    #[error(
        "Not signed in to GitHub. Sign in with the GitHub CLI (`gh auth login`) or connect an account."
    )]
    NotSignedIn,
    #[error("GitHub no longer accepts this token. Connect the account again.")]
    TokenRevoked,
    #[error(
        "The GitHub token lacks the `{scope}` scope. Grant it with `gh auth refresh -s {scope}` or connect the account again."
    )]
    MissingScope { scope: String },
    #[error(
        "This organization uses SAML single sign-on. Authorize the token for it on GitHub, then reload."
    )]
    SsoRequired { authorize_url: String },
    #[error(
        "{} restricts which apps can read its data. Ask an owner to approve the app, then reload.",
        org.as_deref().map_or_else(|| "The organization".to_string(), |org| format!("The organization {org}"))
    )]
    OrgRestricted { org: Option<String> },
    #[error(
        "GitHub's rate limit was reached. {}",
        retry_after.map_or_else(
            || "Try again once it resets.".to_string(),
            |wait| format!("GitHub asked to wait {} s.", wait.as_secs())
        )
    )]
    RateLimited {
        reset_at: Option<SystemTime>,
        /// The wait GitHub named in `retry-after`, as sent. The write queue
        /// holds writes back for at most [`crate::MAX_WRITE_PAUSE`].
        retry_after: Option<Duration>,
    },
    #[error("GitHub could not find this, or the token cannot see it.")]
    NotFound,
    #[error("GitHub refused the request: {message}")]
    Forbidden { message: String },
    #[error("GitHub rejected the change: {message}")]
    Rejected { message: String },
    #[error("Could not reach GitHub: {message}")]
    Network { message: String },
    /// A write whose answer never arrived whole: it may have reached GitHub
    /// and been applied. Unlike [`GithubError::Network`], sending it again
    /// can duplicate it.
    #[error(
        "GitHub may already have applied this change ({message}). Reload and check before trying again."
    )]
    OutcomeUnknown { message: String },
    #[error("GitHub sent an answer Zode could not read: {message}")]
    UnexpectedResponse { message: String },
    #[error("The GitHub CLI failed: {message}")]
    GhCli { message: String },
    #[error("Could not read the GitHub token from the system keychain: {message}")]
    Keychain { message: String },
}

pub type GithubResult<T> = Result<T, GithubError>;

/// Longest service message passed on to the user.
const MAX_MESSAGE_CHARACTERS: usize = 300;

/// A message from GitHub, trimmed and cut short so a huge body cannot flood a
/// toast.
pub(crate) fn shorten_message(message: &str) -> String {
    let message = message.trim();
    if message.is_empty() {
        return "no details given".to_string();
    }
    match message.char_indices().nth(MAX_MESSAGE_CHARACTERS) {
        Some((cut, _)) => format!("{}…", &message[..cut]),
        None => message.to_string(),
    }
}

pub(crate) fn unexpected(message: impl std::fmt::Display) -> GithubError {
    GithubError::UnexpectedResponse {
        message: shorten_message(&message.to_string()),
    }
}

/// GitHub explains an organization's OAuth App restriction in prose, naming
/// the organization between backticks.
pub(crate) fn org_restriction(message: &str) -> Option<GithubError> {
    if !message.contains("OAuth App access restrictions") {
        return None;
    }
    let org = message
        .split('`')
        .nth(1)
        .filter(|org| crate::model::is_valid_path_part(org))
        .map(str::to_string);
    Some(GithubError::OrgRestricted { org })
}
