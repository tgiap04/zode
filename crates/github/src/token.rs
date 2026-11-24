use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use credentials_provider::CredentialsProvider;
use futures::future::{BoxFuture, Either, select};
use gpui::{App, AsyncApp, BackgroundExecutor, Task};
use sha2::{Digest as _, Sha256};

use crate::error::{GithubError, GithubResult, shorten_message};

/// The keychain entry for a token from Zode's own sign-in (device flow).
/// Never written with a token that came from the GitHub CLI.
pub const KEYCHAIN_URL: &str = "zode-github://token";

/// Shown in the keychain's own UI next to the entry; never read back.
const KEYCHAIN_USERNAME: &str = "github-token";

/// `gh` may wait on a locked Secret Service on Linux; past this the tab says
/// so instead of hanging.
pub const GH_TIMEOUT: Duration = Duration::from_secs(8);

pub const GH_ARGUMENTS: [&str; 4] = ["auth", "token", "--hostname", "github.com"];

/// Where a token came from, shown in the tab so the user knows which account
/// acts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenSource {
    Keychain,
    GhCli,
    /// `gh` answered with a variable from Zode's environment rather than its
    /// own login.
    GhEnvironment(TokenVariable),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenVariable {
    GhToken,
    GithubToken,
}

impl TokenVariable {
    /// In the order `gh` reads them.
    pub const ALL: [Self; 2] = [Self::GhToken, Self::GithubToken];

    pub fn name(self) -> &'static str {
        match self {
            Self::GhToken => "GH_TOKEN",
            Self::GithubToken => "GITHUB_TOKEN",
        }
    }
}

/// Stands for a token in maps and logs without holding it: a short hash.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TokenIdentity(String);

impl TokenIdentity {
    fn of(secret: &str) -> Self {
        let digest = Sha256::digest(format!("zode-github-token:{secret}").as_bytes());
        let hex = digest
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Self(hex)
    }
}

/// A GitHub token. Its `Debug` never shows the secret, and the secret is only
/// readable inside this crate.
#[derive(Clone)]
pub struct GithubToken {
    secret: Arc<str>,
    source: TokenSource,
    identity: TokenIdentity,
}

impl GithubToken {
    pub fn new(secret: &str, source: TokenSource) -> Self {
        let secret = secret.trim();
        Self {
            identity: TokenIdentity::of(secret),
            secret: secret.into(),
            source,
        }
    }

    pub fn source(&self) -> TokenSource {
        self.source
    }

    pub fn identity(&self) -> &TokenIdentity {
        &self.identity
    }

    pub(crate) fn secret(&self) -> &str {
        &self.secret
    }
}

impl fmt::Debug for GithubToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GithubToken")
            .field("source", &self.source)
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

/// What one run of `gh` printed. No `Debug`: stdout is the token.
pub struct GhOutput {
    pub success: bool,
    pub stdout: String,
}

/// The machine Zode runs on, as far as finding and running `gh` goes.
pub trait GhHost: Send + Sync {
    /// From Zode's own environment (the login shell's), never a project's.
    fn environment_variable(&self, name: &str) -> Option<String>;
    /// Searches Zode's own `PATH`.
    fn find_on_path(&self, program: &str) -> Option<PathBuf>;
    /// Runs `program` with stdin closed. Dropping the future must stop it.
    fn run(&self, program: PathBuf, arguments: Vec<String>) -> BoxFuture<'static, GhRunResult>;
}

pub type GhRunResult = Result<GhOutput, String>;

/// This machine.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalGhHost;

impl GhHost for LocalGhHost {
    fn environment_variable(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|value| !value.is_empty())
    }

    fn find_on_path(&self, program: &str) -> Option<PathBuf> {
        which::which(program).ok()
    }

    fn run(&self, program: PathBuf, arguments: Vec<String>) -> BoxFuture<'static, GhRunResult> {
        Box::pin(async move {
            let mut command = util::command::new_command(&program);
            command
                .args(&arguments)
                .stdin(util::command::Stdio::null())
                .stdout(util::command::Stdio::piped())
                .stderr(util::command::Stdio::piped())
                .kill_on_drop(true);
            let output = command
                .output()
                .await
                .map_err(|error| format!("could not run {}: {error}", program.display()))?;
            Ok(GhOutput {
                success: output.status.success(),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            })
        })
    }
}

/// The token to use: the keychain's when there is one, otherwise the GitHub
/// CLI's. A keychain token GitHub later refuses is not swapped for the CLI's
/// here or anywhere else, since that would quietly act as another account.
pub async fn resolve_token(
    credentials: &dyn CredentialsProvider,
    gh_host: &dyn GhHost,
    cx: &AsyncApp,
) -> GithubResult<GithubToken> {
    if let Some(token) = read_keychain_token(credentials, cx).await? {
        return Ok(token);
    }
    token_from_gh(gh_host, cx.background_executor()).await
}

pub async fn read_keychain_token(
    credentials: &dyn CredentialsProvider,
    cx: &AsyncApp,
) -> GithubResult<Option<GithubToken>> {
    let Some((_username, password)) = credentials
        .read_credentials(KEYCHAIN_URL, cx)
        .await
        .map_err(keychain_error)?
    else {
        return Ok(None);
    };
    let secret = String::from_utf8(password).map_err(|error| keychain_error(error.into()))?;
    // Falling back to `gh` here would quietly switch accounts.
    if !is_plausible_token(secret.trim()) {
        return Err(GithubError::Keychain {
            message: "the stored token is unreadable".to_string(),
        });
    }
    Ok(Some(GithubToken::new(&secret, TokenSource::Keychain)))
}

/// Runs `gh auth token` found on Zode's own `PATH`, by absolute path, with
/// stdin closed and a timeout. The token stays in memory only.
pub async fn token_from_gh(
    gh_host: &dyn GhHost,
    executor: &BackgroundExecutor,
) -> GithubResult<GithubToken> {
    let Some(program) = gh_host
        .find_on_path("gh")
        .filter(|program| program.is_absolute())
    else {
        return Err(GithubError::NotSignedIn);
    };
    let arguments = GH_ARGUMENTS
        .iter()
        .map(|argument| argument.to_string())
        .collect();
    let output = with_timeout(gh_host.run(program, arguments), executor)
        .await
        .ok_or_else(|| GithubError::GhCli {
            message: format!(
                "`gh auth token` did not answer within {} seconds.",
                GH_TIMEOUT.as_secs()
            ),
        })?
        .map_err(|message| GithubError::GhCli {
            message: shorten_message(&message),
        })?;
    let secret = output.stdout.trim();
    if !output.success || !is_plausible_token(secret) {
        return Err(GithubError::NotSignedIn);
    }
    let source = TokenVariable::ALL
        .into_iter()
        .find(|variable| {
            gh_host
                .environment_variable(variable.name())
                .is_some_and(|value| value.trim() == secret)
        })
        .map_or(TokenSource::GhCli, TokenSource::GhEnvironment);
    Ok(GithubToken::new(secret, source))
}

/// One line of visible ASCII: what every kind of GitHub token looks like, and
/// never a message `gh` printed instead.
fn is_plausible_token(secret: &str) -> bool {
    !secret.is_empty() && secret.chars().all(|character| character.is_ascii_graphic())
}

/// Stores a token from Zode's own sign-in.
pub fn write_keychain_token(
    credentials: Arc<dyn CredentialsProvider>,
    token: String,
    cx: &App,
) -> Task<anyhow::Result<()>> {
    let token = token.trim().to_string();
    if token.is_empty() {
        return Task::ready(Err(anyhow!("The GitHub token is empty.")));
    }
    cx.spawn(async move |cx: &mut AsyncApp| {
        credentials
            .write_credentials(KEYCHAIN_URL, KEYCHAIN_USERNAME, token.as_bytes(), cx)
            .await
    })
}

pub fn delete_keychain_token(
    credentials: Arc<dyn CredentialsProvider>,
    cx: &App,
) -> Task<anyhow::Result<()>> {
    cx.spawn(async move |cx: &mut AsyncApp| credentials.delete_credentials(KEYCHAIN_URL, cx).await)
}

fn keychain_error(error: anyhow::Error) -> GithubError {
    GithubError::Keychain {
        message: shorten_message(&error.to_string()),
    }
}

async fn with_timeout<T>(
    work: impl Future<Output = T>,
    executor: &BackgroundExecutor,
) -> Option<T> {
    let work = std::pin::pin!(work);
    match select(work, executor.timer(GH_TIMEOUT)).await {
        Either::Left((value, _)) => Some(value),
        Either::Right(_) => None,
    }
}
