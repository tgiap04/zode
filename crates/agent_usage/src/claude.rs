//! Claude's plan quota, read the way Claude Code reads it.
//!
//! `GET https://api.anthropic.com/api/oauth/usage`, authorised with the OAuth
//! access token Claude Code already holds. **This is not a documented public
//! API** — the path and the `anthropic-beta` header are what the CLI itself uses,
//! and the response already carries codenamed fields for things that are not
//! public. Everything here therefore degrades to "nothing to show" rather than
//! surfacing an error: a status bar is not the place to report that an
//! undocumented endpoint moved.
//!
//! The token is read fresh for each fetch and never stored, never logged, and
//! never written anywhere. The buffers this module reads it into, and the
//! copies it makes, are wiped when the fetch lets go of them; what the OS, the
//! JSON parser and the HTTP stack copy along the way is beyond its reach.
//!
//! For a project on a remote host the token is the one Claude Code holds on
//! that host, read through the project's [`CommandHost`]: the agents there
//! spend that account, so this machine's would be the wrong one to show.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::AsyncReadExt as _;
use gpui::{BackgroundExecutor, SharedString};
use http_client::{AsyncBody, HttpClient, Request};
use serde::Deserialize;
use util::command::{CommandHost, Stdio};
use zeroize::Zeroizing;

use crate::{UsageWindow, WindowKind};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";

/// How many times to ask, in total, before giving up on a transient answer.
const MAX_ATTEMPTS: usize = 3;
/// Waits between attempts, used when the server does not say how long to wait.
const BACKOFF: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(4)];
/// Ceiling on any single wait, including one the server asked for.
///
/// A `Retry-After` of ten minutes is a perfectly legal answer, and honouring it
/// literally would hold a background task open long past the point where the
/// 60-second poll makes the whole attempt moot. Clamping keeps the retry chain
/// shorter than `POLL_INTERVAL`, so a retry never overlaps the next poll and
/// doubles the load this exists to reduce.
const MAX_WAIT: Duration = Duration::from_secs(5);
/// Client-side statuses that mean "later", not "no": a request timeout and an
/// early-data rejection.
///
/// 429 is deliberately absent. This endpoint limits on *burst* rather than on
/// volume -- ten requests back to back are refused where the same ten spread out
/// are not -- so asking again a second later is the burst, not the cure. One
/// rejected request became three, which kept the limit engaged and made the next
/// read likelier to fail than if nothing had been retried at all. A 429 is now
/// final for that read; the next poll comes round on its own.
const RETRYABLE_STATUSES: [u16; 2] = [408, 425];

/// The environment variables that mean "I am not talking to my subscription".
///
/// Any of these set redirects the CLI at a different endpoint or a different
/// credential, so whatever the subscription's quota says has nothing to do with
/// what the user is spending. Showing it anyway would be showing a number about
/// the wrong account.
const RUNTIME_OVERRIDES: [&str; 3] = [
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
];

/// Prints Claude Code's credential file on a remote host, where there is no
/// keychain and the file is the only copy.
///
/// Constant, with nothing spliced in: the config directory reaches it as a
/// variable. A wrapper that runs no login shell (`wsl.exe --exec`) does not
/// load the user's `CLAUDE_CONFIG_DIR`, so the value the project's environment
/// was loaded with is passed along explicitly.
const HOST_CREDENTIALS_SCRIPT: &str = "file=\"${CLAUDE_CONFIG_DIR:-$HOME/.claude}/.credentials.json\"; \
     [ -r \"$file\" ] || exit 3; exec cat -- \"$file\"";

/// The script's answer for "there is no credential file", told apart from a
/// host that could not run it at all.
const NO_CREDENTIALS_FILE_EXIT: i32 = 3;

/// More than any credential file holds. Reading stops here rather than
/// buffering whatever a host sends back.
const MAX_CREDENTIALS_BYTES: usize = 64 * 1024;

/// Room for the one byte past the limit that tells a file too large from one
/// that fits, plus the 32 bytes `read_to_end` reserves whenever the buffer is
/// full. Without them a file at the limit grows the buffer, and the
/// allocation it leaves holds the file unwiped.
const CREDENTIAL_BUFFER_CAPACITY: usize = MAX_CREDENTIALS_BYTES + 1 + 32;

/// How long a remote host gets to print its credential file. Covers starting
/// a stopped WSL distro, which takes seconds.
const HOST_READ_TIMEOUT: Duration = Duration::from_secs(15);

/// Why there is nothing to show, when there is nothing to show.
///
/// Carried rather than collapsed to `None` so the indicator's tooltip can say
/// which of these it is. "No quota shown" has several quite different causes and
/// only one of them is worth acting on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unavailable {
    /// An `ANTHROPIC_*` override is in force.
    RuntimeOverride,
    /// Claude Code has never signed in on the machine that was asked.
    NoCredentials,
    /// The remote host a project lives on could not be asked: it is not
    /// connected, or the command reaching it failed.
    HostUnavailable,
    /// The remote host printed something that is not a credential file:
    /// output from the user's shell startup files, or more than one could
    /// hold. Not proof of being signed out, so it is not said as such.
    CredentialsUnreadable,
    /// Signed in, but on a plan with no quota windows to report.
    UnsupportedPlan,
    /// The request or the parse failed. Carries a short reason, never a token.
    Request(SharedString),
    /// The endpoint refused for tempo rather than for cause, and kept refusing
    /// across every attempt.
    ///
    /// Separate from [`Self::Request`] because it means something different to
    /// the person reading the tooltip: nothing is misconfigured and nothing is
    /// broken, the account is simply being asked too often — most likely by this
    /// editor and the Claude Code CLI at once, since they share one undocumented
    /// endpoint and one token.
    RateLimited,
}

/// The parts of Claude Code's credential file this needs.
///
/// Deliberately not `Debug`: a derived `Debug` on a struct holding an access
/// token is one `{:?}` away from putting it in a log.
#[derive(Deserialize)]
struct Credentials {
    #[serde(rename = "claudeAiOauth")]
    oauth: Option<OauthCredentials>,
}

#[derive(Deserialize)]
struct OauthCredentials {
    /// Wrapped as it is parsed, so a file that fails to parse further on
    /// still wipes the token it got through.
    #[serde(rename = "accessToken")]
    access_token: Option<Zeroizing<String>>,
    #[serde(rename = "subscriptionType", default)]
    subscription_type: Option<String>,
    #[serde(rename = "rateLimitTier", default)]
    rate_limit_tier: Option<String>,
}

/// The response, read through `limits[]` and nothing else.
///
/// The payload also carries `five_hour` / `seven_day` (the same two windows in an
/// older shape), a row of codenamed fields for unreleased features, and `spend` /
/// `extra_usage` — money. Only `limits` is read: it is self-describing, it is
/// where the model-scoped window lives, and its `percent` is an integer where
/// `utilization` is a float of ambiguous scale.
#[derive(Deserialize)]
struct UsageResponse {
    #[serde(default)]
    limits: Vec<Limit>,
}

#[derive(Deserialize)]
struct Limit {
    /// What the endpoint calls this window: `session`, `weekly_all`,
    /// `weekly_scoped`. Read as a free string rather than an enum so a kind this
    /// build has never seen deserialises instead of failing the whole payload.
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    percent: Option<u8>,
    #[serde(default)]
    resets_at: Option<String>,
    #[serde(default)]
    scope: Option<Scope>,
}

impl Limit {
    /// The kind, as this crate models it.
    ///
    /// Every weekly variant folds into one `Weekly`: `weekly_all` and
    /// `weekly_scoped` differ in what they *cover*, and the cover is already said
    /// by `scope.model.display_name` — repeating it in the kind would give two
    /// sources for one fact.
    fn window_kind(&self) -> WindowKind {
        match self.kind.as_deref() {
            Some("session") => WindowKind::Session,
            Some(kind) if kind.starts_with("weekly") => WindowKind::Weekly,
            _ => WindowKind::Unknown,
        }
    }
}

#[derive(Deserialize)]
struct Scope {
    #[serde(default)]
    model: Option<ScopedModel>,
}

#[derive(Deserialize)]
struct ScopedModel {
    #[serde(default)]
    display_name: Option<String>,
}

/// Whether any override is in force, given a way to read the environment.
///
/// Takes a lookup rather than reading `std::env` so this is testable without
/// mutating the process — a test that sets a real environment variable changes it
/// for every other test in the binary.
pub fn runtime_override_present(var: impl Fn(&str) -> Option<String>) -> bool {
    RUNTIME_OVERRIDES.iter().any(|key| {
        var(key)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    })
}

/// Whether this plan reports quota windows at all.
///
/// Free and unauthenticated plans have none, and asking for them yields an empty
/// list that would render as an icon with nothing beside it.
fn plan_is_supported(oauth: &OauthCredentials) -> bool {
    let subscription = oauth
        .subscription_type
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if !subscription.is_empty() && subscription != "free" && subscription != "none" {
        return true;
    }

    let tier = oauth
        .rate_limit_tier
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    ["claude", "max", "pro", "team", "enterprise"]
        .iter()
        .any(|known| tier.contains(known))
}

/// Turns the response body into windows.
///
/// Every field is optional and a row missing its `percent` is skipped rather than
/// failing the whole parse: this endpoint is undocumented and already carries
/// fields for unreleased features, so one unfamiliar row must not cost the rows
/// beside it.
///
/// **`is_active` is deliberately not consulted.** It marks which window is
/// currently binding, not which to display — on a real account only the session
/// window is active while all three are shown. Filtering on it would render one
/// window where three belong, and a test written from the same misreading would
/// agree.
pub fn parse_windows(body: &str) -> Result<Vec<UsageWindow>, Unavailable> {
    let response: UsageResponse = serde_json::from_str(body)
        .map_err(|_| Unavailable::Request("the usage response could not be read".into()))?;

    Ok(response
        .limits
        .into_iter()
        .filter_map(|limit| {
            let percent = limit.percent?.min(100);
            let kind = limit.window_kind();
            let resets_at = limit
                .resets_at
                .as_deref()
                .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
                .map(|parsed| parsed.with_timezone(&Utc));
            let label = limit
                .scope
                .and_then(|scope| scope.model)
                .and_then(|model| model.display_name)
                .filter(|name| !name.trim().is_empty())
                .map(SharedString::from);

            Some(UsageWindow {
                percent,
                resets_at,
                label,
                kind,
            })
        })
        .collect())
}

/// Reads Claude Code's credentials, without holding on to them.
///
/// macOS keeps them in the keychain as a *generic* password, which the editor's
/// own `read_credentials` cannot reach — that reads internet passwords keyed by
/// server URL, a different keychain class. So this shells out to `security`, the
/// same way Claude Code's own tooling does. Every other platform, and macOS when
/// the keychain has nothing, falls back to the file.
async fn read_credentials(executor: &BackgroundExecutor) -> Option<Credentials> {
    let from_keychain = if cfg!(target_os = "macos") {
        executor
            .spawn(async {
                let output = util::command::new_command("security")
                    .args([
                        "find-generic-password",
                        "-s",
                        "Claude Code-credentials",
                        "-w",
                    ])
                    .output()
                    .await
                    .ok()?;
                let stdout = Zeroizing::new(output.stdout);
                if !output.status.success() {
                    return None;
                }
                serde_json::from_slice::<Credentials>(&stdout).ok()
            })
            .await
    } else {
        None
    };

    if from_keychain.is_some() {
        return from_keychain;
    }

    let path = util::paths::home_dir()
        .join(".claude")
        .join(".credentials.json");
    executor
        .spawn(async move {
            let contents = Zeroizing::new(smol::fs::read(path).await.ok()?);
            serde_json::from_slice::<Credentials>(&contents).ok()
        })
        .await
}

/// The whole path: eligibility, credentials, request, parse.
///
/// Ordered so the cheapest disqualification runs first — an override means the
/// credentials are never read at all, which is the right outcome for a user who
/// has pointed their CLI somewhere else.
pub async fn fetch(
    http_client: Arc<dyn HttpClient>,
    executor: BackgroundExecutor,
) -> Result<Vec<UsageWindow>, Unavailable> {
    if runtime_override_present(|key| std::env::var(key).ok()) {
        return Err(Unavailable::RuntimeOverride);
    }

    let credentials = read_credentials(&executor)
        .await
        .ok_or(Unavailable::NoCredentials)?;
    let token = access_token(credentials)?;

    fetch_with_token(http_client, executor, &token).await
}

/// [`fetch`] for the account Claude Code is signed into on `host`.
///
/// Never falls back to this machine's keychain or file: a remote host that
/// cannot be read shows nothing, because this machine's numbers would be
/// another account's.
pub async fn fetch_on_host(
    http_client: Arc<dyn HttpClient>,
    executor: BackgroundExecutor,
    host: Arc<dyn CommandHost>,
) -> Result<Vec<UsageWindow>, Unavailable> {
    if runtime_override_present(|key| host.environment_variable(key)) {
        return Err(Unavailable::RuntimeOverride);
    }

    let deadline = async {
        executor.timer(HOST_READ_TIMEOUT).await;
        log::warn!("claude usage: the project's host did not print its credentials in time");
        Err(Unavailable::HostUnavailable)
    };
    let credentials = smol::future::or(read_credentials_on_host(host.as_ref()), deadline).await?;
    let token = access_token(credentials)?;

    fetch_with_token(http_client, executor, &token).await
}

/// The token in `credentials`, once the plan is known to have quota windows.
fn access_token(credentials: Credentials) -> Result<Zeroizing<String>, Unavailable> {
    let oauth = credentials.oauth.ok_or(Unavailable::NoCredentials)?;
    if !plan_is_supported(&oauth) {
        return Err(Unavailable::UnsupportedPlan);
    }
    oauth
        .access_token
        .filter(|token| !token.trim().is_empty())
        .ok_or(Unavailable::NoCredentials)
}

/// Claude Code's credential file on `host`, printed by
/// [`HOST_CREDENTIALS_SCRIPT`] and held in memory only.
async fn read_credentials_on_host(host: &dyn CommandHost) -> Result<Credentials, Unavailable> {
    let host_name = host
        .remote_name()
        .unwrap_or_else(|| "this machine".to_string());
    let config_dir = host
        .environment_variable("CLAUDE_CONFIG_DIR")
        .filter(|directory| !directory.trim().is_empty());
    let env: Vec<(&str, &str)> = config_dir
        .as_deref()
        .map(|directory| ("CLAUDE_CONFIG_DIR", directory))
        .into_iter()
        .collect();

    let unavailable = |what: &str, error: &dyn std::fmt::Display| {
        log::warn!("claude usage: {what} on {host_name}: {error:#}");
        Unavailable::HostUnavailable
    };
    let mut command = host
        .command("sh", &["-c", HOST_CREDENTIALS_SCRIPT], &env, None)
        .map_err(|error| unavailable("the credentials could not be read", &error))?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| unavailable("the credential read would not start", &error))?;

    let mut contents = credential_buffer();
    let read = match child.stdout.take() {
        Some(stdout) => read_bounded(stdout, &mut contents).await,
        None => Ok(0),
    };
    let too_large = contents.len() > MAX_CREDENTIALS_BYTES;
    if read.is_err() || too_large {
        if let Err(error) = child.kill() {
            log::debug!("claude usage: could not stop the credential read on {host_name}: {error}");
        }
    }
    let status = child
        .status()
        .await
        .map_err(|error| unavailable("the credential read could not be awaited", &error))?;

    if let Err(error) = read {
        return Err(unavailable("the credentials could not be read", &error));
    }
    if too_large {
        log::warn!("claude usage: the credential file on {host_name} is too large to be one");
        return Err(Unavailable::CredentialsUnreadable);
    }
    match status.code() {
        Some(0) => {}
        Some(NO_CREDENTIALS_FILE_EXIT) => return Err(Unavailable::NoCredentials),
        _ => return Err(unavailable("the credential read failed", &status)),
    }
    // The parser's error is dropped unread: it quotes the value it choked on,
    // which can be the token.
    serde_json::from_slice::<Credentials>(&contents).map_err(|_| {
        log::warn!("claude usage: the credential file on {host_name} could not be parsed");
        Unavailable::CredentialsUnreadable
    })
}

/// What a remote host's credential file is read into.
fn credential_buffer() -> Zeroizing<Vec<u8>> {
    Zeroizing::new(Vec::with_capacity(CREDENTIAL_BUFFER_CAPACITY))
}

/// Reads at most one byte more than [`MAX_CREDENTIALS_BYTES`], so a caller can
/// tell a file that fits from one that does not.
async fn read_bounded(
    reader: impl futures::AsyncRead + Unpin,
    contents: &mut Vec<u8>,
) -> std::io::Result<usize> {
    futures::AsyncReadExt::take(reader, MAX_CREDENTIALS_BYTES as u64 + 1)
        .read_to_end(contents)
        .await
}

/// The request-and-retry half of [`fetch`], with the credential already in hand.
///
/// Split out so a test can drive the retry behaviour: `fetch` reads the OS
/// keychain and the user's home directory, neither of which a test can stub, and
/// the retry policy is the part worth testing.
async fn fetch_with_token(
    http_client: Arc<dyn HttpClient>,
    executor: BackgroundExecutor,
    token: &str,
) -> Result<Vec<UsageWindow>, Unavailable> {
    // Attempt, wait, attempt -- but only for the answers that clear up on their
    // own. A 429 leaves on the first one: see `RETRYABLE_STATUSES`.
    let mut last_retryable = None;
    for attempt in 0..MAX_ATTEMPTS {
        match request_once(&http_client, token).await {
            Attempt::Done(windows) => return Ok(windows),
            Attempt::Fatal(reason) => return Err(give_up(reason, attempt + 1)),
            Attempt::Retry { after, reason } => {
                last_retryable = Some(reason);
                let Some(backoff) = BACKOFF.get(attempt) else {
                    break;
                };
                let wait = after.unwrap_or(*backoff).min(MAX_WAIT);
                // Debug rather than warn: a timeout absorbed by a retry is the
                // routine case this chain exists for, and a warning per
                // occurrence would bury the one that actually reached the user.
                log::debug!(
                    "claude usage: attempt {} of {MAX_ATTEMPTS} was told to wait; retrying in {:?}",
                    attempt + 1,
                    wait
                );
                executor.timer(wait).await;
            }
        }
    }

    Err(give_up(
        last_retryable.unwrap_or(Unavailable::RateLimited),
        MAX_ATTEMPTS,
    ))
}

/// Records a read that came back with nothing, and hands the reason on.
///
/// Every failing exit goes through here, including the one-attempt exits: a 429
/// no longer reaches the end of the retry loop, and it is the failure a user is
/// most likely to be asking about. Before this existed the status bar said
/// "rate limited" and the log said nothing at all, so there was no way to tell a
/// steady drip from a storm.
fn give_up(reason: Unavailable, attempts: usize) -> Unavailable {
    log::warn!("claude usage: no read after {attempts} attempt(s) ({reason:?})");
    reason
}

/// One request, classified. Nothing here waits or retries — that is the caller's
/// job, so this stays a straight line that a test can drive one answer at a time.
enum Attempt {
    Done(Vec<UsageWindow>),
    /// Worth asking again. `after` is what the server asked for, when it said.
    Retry {
        after: Option<Duration>,
        reason: Unavailable,
    },
    /// Asking again would not help, and for 401/403 would make it worse.
    Fatal(Unavailable),
}

async fn request_once(http_client: &Arc<dyn HttpClient>, token: &str) -> Attempt {
    let authorization = Zeroizing::new(format!("Bearer {token}"));
    let Ok(mut authorization) = http_client::http::HeaderValue::from_str(&authorization) else {
        return Attempt::Fatal(Unavailable::Request(
            "the access token is not a valid header".into(),
        ));
    };
    // Keeps the token out of any header dump the HTTP layer logs.
    authorization.set_sensitive(true);
    let request = match Request::builder()
        .uri(USAGE_URL)
        .header("Accept", "application/json")
        .header("Authorization", authorization)
        .header("anthropic-beta", OAUTH_BETA)
        .body(AsyncBody::default())
    {
        Ok(request) => request,
        Err(_) => {
            return Attempt::Fatal(Unavailable::Request(
                "the usage request could not be built".into(),
            ));
        }
    };

    let mut response = match http_client.send(request).await {
        Ok(response) => response,
        // A transport failure is the other thing that clears up on its own.
        Err(_) => {
            return Attempt::Retry {
                after: None,
                reason: Unavailable::Request("the usage endpoint could not be reached".into()),
            };
        }
    };

    let status = response.status();
    if !status.is_success() {
        // The status, never the body: an error body from an authenticated
        // endpoint is exactly the sort of thing that echoes a credential back.
        let reason = if status.as_u16() == 429 {
            Unavailable::RateLimited
        } else {
            Unavailable::Request(format!("the usage endpoint answered {}", status.as_u16()).into())
        };
        // 429 is a tempo problem, 5xx is the server's problem, and 408 and 425 are
        // transient by definition — all four are worth asking again. Any other 4xx
        // is about this request: retrying a 401 just spends the rate limit on an
        // answer that will not change.
        return if RETRYABLE_STATUSES.contains(&status.as_u16()) || status.is_server_error() {
            Attempt::Retry {
                after: retry_after(&response),
                reason,
            }
        } else {
            Attempt::Fatal(reason)
        };
    }

    let mut body = String::new();
    if response.body_mut().read_to_string(&mut body).await.is_err() {
        return Attempt::Retry {
            after: None,
            reason: Unavailable::Request("the usage response could not be read".into()),
        };
    }

    match parse_windows(&body) {
        Ok(windows) => Attempt::Done(windows),
        // A body that arrived but did not parse is a shape change, not a hiccup.
        Err(reason) => Attempt::Fatal(reason),
    }
}

/// `Retry-After` in seconds, when the server sent one that reads as seconds.
///
/// The header also allows an HTTP-date. That form is not parsed: pulling in a
/// date parser for a hint that the backoff already covers is a poor trade, and
/// an unparsed header falls back to the backoff rather than to zero.
fn retry_after<T>(response: &http_client::Response<T>) -> Option<Duration> {
    response
        .headers()
        .get("retry-after")?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    use http_client::{FakeHttpClient, Response};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A client that answers a scripted list of statuses in order, then repeats
    /// the last one, and counts how many times it was asked.
    fn scripted(
        statuses: Vec<u16>,
        retry_after: Option<&'static str>,
    ) -> (Arc<dyn HttpClient>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let client = FakeHttpClient::create(move |_| {
            let index = seen.fetch_add(1, Ordering::SeqCst);
            let status = *statuses.get(index).unwrap_or(statuses.last().unwrap());
            async move {
                let mut builder = Response::builder().status(status);
                if let Some(value) = retry_after {
                    builder = builder.header("retry-after", value);
                }
                let body = if status == 200 {
                    // Well-formed and empty: zero windows is a valid answer, so
                    // this proves the success path without pinning the payload.
                    r#"{"limits": []}"#.to_string()
                } else {
                    String::new()
                };
                Ok(builder.body(body.into()).unwrap())
            }
        });
        (client as Arc<dyn HttpClient>, calls)
    }

    /// The bug this closes: one transient 429 used to leave the indicator blank
    /// until the next poll, a minute later.
    #[gpui::test]
    async fn a_429_is_not_retried_even_when_the_next_ask_would_have_worked(
        cx: &mut gpui::TestAppContext,
    ) {
        // Scripted to succeed on a second ask, to prove the second ask is not
        // made. This endpoint refuses bursts rather than volume -- ten requests
        // back to back are refused where the same ten spread out are not -- so a
        // retry a second later is the burst, and it kept the limit engaged
        // instead of riding it out.
        let (client, calls) = scripted(vec![429, 200], None);
        assert_eq!(
            fetch_with_token(client, cx.executor(), "t").await,
            Err(Unavailable::RateLimited),
            "the first refusal is the answer"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "one refusal must cost one request, not three"
        );
    }

    #[gpui::test]
    async fn a_429_that_never_clears_reports_rate_limiting_and_stops(
        cx: &mut gpui::TestAppContext,
    ) {
        let (client, calls) = scripted(vec![429], None);
        assert_eq!(
            fetch_with_token(client, cx.executor(), "t").await,
            Err(Unavailable::RateLimited),
            "the reason must say it is tempo, not a bare status code"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "asking again is what spends the limit it is waiting on"
        );
    }

    /// A 5xx is the server's problem and clears up; a 401 is this request's
    /// problem and will not. Retrying the second one spends the shared rate limit
    /// on an answer that cannot change.
    #[gpui::test]
    async fn a_server_error_retries_and_an_auth_error_does_not(cx: &mut gpui::TestAppContext) {
        let (client, calls) = scripted(vec![503, 200], None);
        assert!(fetch_with_token(client, cx.executor(), "t").await.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let (client, calls) = scripted(vec![401], None);
        assert!(matches!(
            fetch_with_token(client, cx.executor(), "t").await,
            Err(Unavailable::Request(_))
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1, "401 must not be retried");
    }

    /// 408 and 425 read as client errors but mean "later", not "no". Lumping them
    /// in with 401 would turn a timeout into a permanent blank.
    #[gpui::test]
    async fn a_timeout_or_early_data_rejection_is_retried(cx: &mut gpui::TestAppContext) {
        for status in RETRYABLE_STATUSES {
            let (client, calls) = scripted(vec![status, 200], None);
            assert!(
                fetch_with_token(client, cx.executor(), "t").await.is_ok(),
                "{status} should have been retried"
            );
            assert_eq!(calls.load(Ordering::SeqCst), 2, "{status}");
        }

        // The neighbours that must not be retried, so this cannot quietly widen.
        for status in [400u16, 401, 403, 404] {
            let (client, calls) = scripted(vec![status], None);
            assert!(fetch_with_token(client, cx.executor(), "t").await.is_err());
            assert_eq!(
                calls.load(Ordering::SeqCst),
                1,
                "{status} must be asked once"
            );
        }
    }

    #[gpui::test]
    async fn an_absurd_retry_after_does_not_hold_the_task_open(cx: &mut gpui::TestAppContext) {
        // 600s is a legal answer. Honouring it literally would outlive the
        // 60-second poll that makes the whole attempt moot. Driven with a 408
        // rather than a 429: a 429 no longer reaches the wait at all.
        let (client, calls) = scripted(vec![408, 200], Some("600"));
        assert!(fetch_with_token(client, cx.executor(), "t").await.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn retry_after_reads_seconds_and_ignores_anything_else() {
        let with = |value: &str| {
            let response = Response::builder()
                .status(429)
                .header("retry-after", value)
                .body(())
                .unwrap();
            retry_after(&response)
        };
        assert_eq!(with("2"), Some(Duration::from_secs(2)));
        assert_eq!(with("  3 "), Some(Duration::from_secs(3)));
        assert_eq!(
            with("Wed, 21 Oct 2026 07:28:00 GMT"),
            None,
            "the HTTP-date form falls back to the backoff rather than to zero"
        );
        assert_eq!(with("-1"), None);
        assert_eq!(
            retry_after(&Response::builder().status(429).body(()).unwrap()),
            None
        );
    }

    /// The real response, recorded from a live call on 2026-08-21.
    ///
    /// Trimmed to what this module reads plus the things it must ignore: the older
    /// `five_hour`/`seven_day` pair, a codenamed field, and `spend`. Keeping those
    /// in the fixture is the point — a parser that trips over them fails here
    /// rather than in front of a user.
    const REAL_RESPONSE: &str = r#"{
      "five_hour": {"utilization": 53.0, "resets_at": "2026-08-21T12:29:59.983789+00:00"},
      "seven_day": {"utilization": 10.0, "resets_at": "2026-08-27T23:59:59.983816+00:00"},
      "seven_day_opus": null,
      "nimbus_quill": {"utilization": 0.0, "resets_at": null},
      "amber_ladder": null,
      "spend": {"percent": 4, "enabled": true},
      "limits": [
        {"kind": "session", "group": "session", "percent": 53, "severity": "normal",
         "resets_at": "2026-08-21T12:29:59.983789+00:00", "scope": null, "is_active": true},
        {"kind": "weekly_all", "group": "weekly", "percent": 10, "severity": "normal",
         "resets_at": "2026-08-27T23:59:59.983816+00:00", "scope": null, "is_active": false},
        {"kind": "weekly_scoped", "group": "weekly", "percent": 0, "severity": "normal",
         "resets_at": null,
         "scope": {"model": {"id": null, "display_name": "Fable"}, "surface": null},
         "is_active": false}
      ],
      "member_dashboard_available": true
    }"#;

    #[test]
    fn the_real_response_yields_all_three_windows() {
        let windows = parse_windows(REAL_RESPONSE).expect("the recorded response must parse");

        assert_eq!(
            windows.len(),
            3,
            "the reference display shows three windows; anything fewer means a row was dropped"
        );
        assert_eq!(windows[0].percent, 53);
        assert_eq!(windows[1].percent, 10);
        assert_eq!(windows[2].percent, 0);
    }

    /// Two of the three rows are `is_active: false`, and all three are displayed.
    ///
    /// This is the misreading the whole module is built to avoid: `is_active`
    /// marks the window that is currently binding, not the windows worth showing.
    #[test]
    fn inactive_windows_are_still_shown() {
        let windows = parse_windows(REAL_RESPONSE).unwrap();
        assert_eq!(
            windows.len(),
            3,
            "filtering on `is_active` would leave only the session window"
        );
    }

    /// The model-scoped row has no reset instant and carries a name instead —
    /// which is where the word "Fable" in the reference display comes from.
    #[test]
    fn a_model_scoped_window_carries_a_name_instead_of_a_countdown() {
        let windows = parse_windows(REAL_RESPONSE).unwrap();
        let scoped = &windows[2];

        assert_eq!(
            scoped.resets_at, None,
            "this row genuinely has no reset time"
        );
        assert_eq!(
            scoped.label,
            Some("Fable".into()),
            "the name stands in for the countdown"
        );
    }

    /// Each row carries the kind the endpoint named, not the kind its position
    /// implies.
    ///
    /// This is what the panel labels its rows from. Reading the kind off the index
    /// instead would pass here today and lie the first time the endpoint reorders
    /// `limits[]` — which it is free to do, being undocumented.
    #[test]
    fn every_row_carries_the_kind_the_endpoint_named() {
        let windows = parse_windows(REAL_RESPONSE).unwrap();

        assert_eq!(windows[0].kind, WindowKind::Session, "`kind: \"session\"`");
        assert_eq!(
            windows[1].kind,
            WindowKind::Weekly,
            "`kind: \"weekly_all\"`"
        );
        assert_eq!(
            windows[2].kind,
            WindowKind::Weekly,
            "`weekly_scoped` is still a weekly window; what it is scoped *to* is \
             said by the label, and saying it twice would give two sources for one \
             fact"
        );

        assert_eq!(windows[0].kind.short_tag(), "5h");
        assert_eq!(windows[1].kind.short_tag(), "wk");
    }

    /// A kind this build has never seen costs the label and never the row.
    ///
    /// The percentage is still true, so dropping the window would under-report the
    /// quota — and silently, which is the worst way to be wrong about a number.
    #[test]
    fn an_unrecognised_kind_costs_the_label_not_the_row() {
        let windows = parse_windows(
            r#"{"limits": [{"kind": "cinder_cove", "percent": 61, "resets_at": null}]}"#,
        )
        .expect("an unfamiliar kind is not a parse failure");

        assert_eq!(windows.len(), 1, "the row survives");
        assert_eq!(windows[0].percent, 61, "and its number is untouched");
        assert_eq!(windows[0].kind, WindowKind::Unknown);
        assert_eq!(
            windows[0].kind.short_tag(),
            "",
            "no tag rather than a guessed one"
        );
    }

    /// The two time-boxed rows keep their reset instants, parsed from RFC 3339
    /// with an offset.
    #[test]
    fn time_boxed_windows_keep_their_reset_instants() {
        let windows = parse_windows(REAL_RESPONSE).unwrap();

        let session = windows[0].resets_at.expect("the session window resets");
        assert_eq!(session.to_rfc3339(), "2026-08-21T12:29:59.983789+00:00");
        assert!(windows[0].label.is_none(), "a countdown needs no name");

        assert!(
            windows[1].resets_at.is_some(),
            "the weekly window resets too"
        );
    }

    /// An unfamiliar row must not cost the rows beside it.
    ///
    /// This endpoint is undocumented and already ships fields for unreleased
    /// features, so a row this build does not understand is expected traffic
    /// rather than an error.
    #[test]
    fn an_unreadable_row_is_skipped_not_fatal() {
        let windows = parse_windows(
            r#"{"limits": [
                {"kind": "session", "percent": 42, "resets_at": null},
                {"kind": "something_new", "resets_at": null},
                {"kind": "weekly_all", "percent": 7, "resets_at": null}
            ]}"#,
        )
        .expect("one strange row must not fail the parse");

        assert_eq!(
            windows.iter().map(|w| w.percent).collect::<Vec<_>>(),
            vec![42, 7],
            "the row with no percent is dropped; its neighbours survive"
        );
    }

    #[test]
    fn a_body_that_is_not_the_expected_shape_is_an_error_not_a_panic() {
        assert!(matches!(
            parse_windows("not json at all"),
            Err(Unavailable::Request(_))
        ));
        assert_eq!(
            parse_windows(r#"{"unrelated": true}"#).unwrap(),
            Vec::new(),
            "a well-formed body with no limits is zero windows, not an error"
        );
    }

    /// A percentage above 100 is clamped rather than trusted.
    #[test]
    fn a_percentage_is_clamped_to_a_hundred() {
        let windows =
            parse_windows(r#"{"limits": [{"percent": 250, "resets_at": null}]}"#).unwrap();
        assert_eq!(windows[0].percent, 100);
    }

    #[test]
    fn any_anthropic_override_disqualifies_the_display() {
        for key in RUNTIME_OVERRIDES {
            assert!(
                runtime_override_present(|asked| (asked == key).then(|| "set".to_string())),
                "{key} points the CLI elsewhere, so subscription quota says nothing"
            );
        }
        assert!(
            !runtime_override_present(|_| None),
            "with none of them set the display is eligible"
        );
        assert!(
            !runtime_override_present(|_| Some("   ".to_string())),
            "a blank value is not an override"
        );
    }

    #[test]
    fn a_plan_with_no_quota_is_recognised() {
        let plan = |subscription: &str, tier: &str| OauthCredentials {
            access_token: Some(Zeroizing::new("unused".to_string())),
            subscription_type: Some(subscription.into()),
            rate_limit_tier: Some(tier.into()),
        };

        assert!(plan_is_supported(&plan("team", "default_claude_max_5x")));
        assert!(plan_is_supported(&plan("max", "")));
        assert!(
            plan_is_supported(&plan("", "default_claude_max_5x")),
            "the tier alone is enough when the subscription is blank"
        );
        assert!(!plan_is_supported(&plan("free", "")));
        assert!(!plan_is_supported(&plan("none", "")));
        assert!(!plan_is_supported(&plan("", "")));
    }

    #[cfg(unix)]
    const REMOTE_TOKEN: &str = "sk-ant-oat01-remote-account-secret";

    #[cfg(unix)]
    fn credential_file(token: &str) -> String {
        format!(
            r#"{{"claudeAiOauth":{{"accessToken":"{token}","subscriptionType":"max","rateLimitTier":"default_claude_max_5x"}}}}"#
        )
    }

    /// A host that prints `contents` the way the credential script would.
    #[cfg(unix)]
    fn host_printing(contents: &str) -> crate::test_support::RecordingHost {
        crate::test_support::RecordingHost::running(&format!("printf '%s' '{contents}'"))
    }

    /// A client that records the `Authorization` header of every request and
    /// answers each with `status`.
    fn recording_client(status: u16) -> (Arc<dyn HttpClient>, Arc<std::sync::Mutex<Vec<String>>>) {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = seen.clone();
        let client = FakeHttpClient::create(move |request| {
            let authorization = request
                .headers()
                .get("Authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            record.lock().unwrap().push(authorization);
            async move {
                let body = if status == 200 {
                    r#"{"limits": [{"kind": "session", "percent": 12, "resets_at": null}]}"#
                } else {
                    ""
                };
                Ok(Response::builder()
                    .status(status)
                    .body(body.to_string().into())
                    .unwrap())
            }
        });
        (client as Arc<dyn HttpClient>, seen)
    }

    #[cfg(unix)]
    #[test]
    fn a_remote_project_s_credentials_are_read_through_its_host() {
        let host = host_printing(&credential_file(REMOTE_TOKEN));

        let credentials = smol::block_on(read_credentials_on_host(&host));
        let token = credentials.and_then(access_token);

        assert_eq!(
            token.as_ref().map(|token| token.as_str()),
            Ok(REMOTE_TOKEN),
            "the token is the one the host printed"
        );
        let asked = host.asked();
        assert_eq!(asked.len(), 1, "one command per read: {asked:?}");
        assert_eq!(asked[0].program, "sh");
        assert_eq!(asked[0].args, vec!["-c", HOST_CREDENTIALS_SCRIPT]);
        assert!(
            asked[0].env.is_empty(),
            "nothing to pass when the host's shell sets no config directory"
        );
    }

    /// The wrapper runs no login shell, so the directory the user set in their
    /// shell has to be handed to the script, or it reads the default one.
    #[cfg(unix)]
    #[test]
    fn the_host_s_claude_config_directory_reaches_the_script() {
        let host = crate::test_support::RecordingHost::running(
            "[ \"$CLAUDE_CONFIG_DIR\" = /home/me/.claude-work ] || exit 9; printf '{}'",
        )
        .with_environment("CLAUDE_CONFIG_DIR", "/home/me/.claude-work");

        let read = smol::block_on(read_credentials_on_host(&host));

        assert!(read.is_ok(), "the script saw the directory");
        assert_eq!(
            host.asked()[0].env,
            vec![(
                "CLAUDE_CONFIG_DIR".to_string(),
                "/home/me/.claude-work".to_string()
            )]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_host_with_no_sign_in_is_not_signed_in() {
        for (script, what) in [
            (
                format!("exit {NO_CREDENTIALS_FILE_EXIT}"),
                "no credential file",
            ),
            ("printf '{}'".to_string(), "a file with no sign-in in it"),
        ] {
            let host = crate::test_support::RecordingHost::running(&script);
            let token = smol::block_on(read_credentials_on_host(&host)).and_then(access_token);
            assert_eq!(
                token.err(),
                Some(Unavailable::NoCredentials),
                "{what} means not signed in"
            );
        }
    }

    /// Prints the start of a credential file holding `token`, then pads it
    /// past what one can hold.
    #[cfg(unix)]
    fn oversized_credential_file_script(token: &str) -> String {
        format!(
            r#"printf '%s' '{{"claudeAiOauth":{{"accessToken":"{token}'; head -c {} /dev/zero | tr '\0' a; printf '"}}}}'"#,
            MAX_CREDENTIALS_BYTES + 16
        )
    }

    /// What `rc` files print, a file that half parses, or one far too large
    /// for a credential file: none of it says the user is signed out, and
    /// telling them so would send them to sign in again for nothing.
    #[cfg(unix)]
    #[test]
    fn a_credential_file_that_cannot_be_read_as_one_is_unreadable_not_signed_out() {
        for (script, what) in [
            (
                format!("printf '%s' '{{\"claudeAiOauth\":{{\"accessToken\":\"{REMOTE_TOKEN}\"'"),
                "a truncated file",
            ),
            ("printf 'not json'".to_string(), "a file that is not JSON"),
            (
                format!(
                    "printf 'Welcome to Ubuntu\\n%s' '{}'",
                    credential_file(REMOTE_TOKEN)
                ),
                "a greeting from the shell's startup files before the file",
            ),
            (
                format!("printf '%s' '{{\"claudeAiOauth\":\"{REMOTE_TOKEN}\"}}'"),
                "a sign-in of the wrong shape",
            ),
            (
                oversized_credential_file_script(REMOTE_TOKEN),
                "more than a credential file holds",
            ),
        ] {
            let host = crate::test_support::RecordingHost::running(&script);
            let token = smol::block_on(read_credentials_on_host(&host)).and_then(access_token);
            assert_eq!(
                token.err(),
                Some(Unavailable::CredentialsUnreadable),
                "{what} is unreadable, not signed out"
            );
        }
    }

    /// A file at the limit fills the buffer to its last byte, which is where
    /// `read_to_end` grows it, leaving the old allocation unwiped.
    #[test]
    fn reading_a_full_credential_file_never_moves_the_buffer() {
        let mut contents = credential_buffer();
        let capacity = contents.capacity();
        let oversized = vec![b'a'; MAX_CREDENTIALS_BYTES * 2];

        let read = smol::block_on(read_bounded(
            futures::io::Cursor::new(oversized),
            &mut contents,
        ));

        assert_eq!(read.ok(), Some(MAX_CREDENTIALS_BYTES + 1));
        assert_eq!(
            contents.capacity(),
            capacity,
            "the buffer was reallocated during the read"
        );
    }

    /// The `Authorization` header is marked sensitive, so no layer that dumps
    /// headers prints it.
    #[test]
    fn the_authorization_header_is_marked_sensitive() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = seen.clone();
        let client: Arc<dyn HttpClient> = FakeHttpClient::create(move |request| {
            record.lock().unwrap().push(
                request
                    .headers()
                    .get("Authorization")
                    .map(|value| value.is_sensitive()),
            );
            async move {
                Ok(Response::builder()
                    .status(401)
                    .body(String::new().into())
                    .unwrap())
            }
        });

        smol::block_on(request_once(&client, REMOTE_TOKEN_FOR_EVERY_PLATFORM));

        assert_eq!(*seen.lock().unwrap(), vec![Some(true)]);
    }

    const REMOTE_TOKEN_FOR_EVERY_PLATFORM: &str = "sk-ant-oat01-header-secret";

    #[cfg(unix)]
    #[test]
    fn a_host_that_cannot_run_the_read_is_unavailable_not_signed_out() {
        for host in [
            crate::test_support::RecordingHost::disconnected(),
            crate::test_support::RecordingHost::running("exit 255"),
        ] {
            assert_eq!(
                smol::block_on(read_credentials_on_host(&host)).err(),
                Some(Unavailable::HostUnavailable)
            );
        }
    }

    /// The bug this guards: a remote read that fails reaching for this
    /// machine's keychain, and showing this machine's account as the remote's.
    #[gpui::test]
    async fn a_remote_read_that_fails_never_falls_back_to_this_machine(
        cx: &mut gpui::TestAppContext,
    ) {
        let (client, seen) = recording_client(200);
        let host = Arc::new(crate::test_support::RecordingHost::disconnected());

        let result = fetch_on_host(client, cx.executor(), host.clone()).await;

        assert_eq!(result, Err(Unavailable::HostUnavailable));
        assert!(
            seen.lock().unwrap().is_empty(),
            "no request may go out with any other account's token"
        );
        assert_eq!(host.asked().len(), 1, "the host was asked, once");
    }

    /// The override check reads the host's environment, where the CLI that
    /// spends the quota runs, and stops before any credential is read.
    #[gpui::test]
    async fn an_override_on_the_host_disqualifies_its_display(cx: &mut gpui::TestAppContext) {
        let (client, seen) = recording_client(200);
        let host = Arc::new(
            crate::test_support::RecordingHost::running("exit 0")
                .with_environment("ANTHROPIC_API_KEY", "set"),
        );

        let result = fetch_on_host(client, cx.executor(), host.clone()).await;

        assert_eq!(result, Err(Unavailable::RuntimeOverride));
        assert!(host.asked().is_empty(), "the credentials were never read");
        assert!(seen.lock().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[gpui::test]
    async fn the_remote_account_s_token_is_the_one_the_endpoint_sees(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.executor().allow_parking();
        let (client, seen) = recording_client(200);
        let host = Arc::new(host_printing(&credential_file(REMOTE_TOKEN)));

        let windows = fetch_on_host(client, cx.executor(), host).await;

        assert_eq!(windows.map(|windows| windows.len()), Ok(1));
        assert_eq!(
            *seen.lock().unwrap(),
            vec![format!("Bearer {REMOTE_TOKEN}")]
        );
    }

    /// The error paths a token is likeliest to leak on: the endpoint refusing
    /// it, a credential file that half parses or holds the token where an
    /// object belongs, one too large, and a read that exits with an error.
    #[cfg(unix)]
    #[gpui::test]
    async fn the_remote_token_never_reaches_a_log_or_an_error(cx: &mut gpui::TestAppContext) {
        crate::test_support::install_log_capture();
        cx.executor().allow_parking();
        let (client, _) = recording_client(401);
        let host = Arc::new(host_printing(&credential_file(REMOTE_TOKEN)));

        let refused = fetch_on_host(client, cx.executor(), host).await;
        let mut errors = vec![format!("{:?}", refused)];
        for (script, expected) in [
            (
                format!("printf '%s' '{{\"claudeAiOauth\":{{\"accessToken\":\"{REMOTE_TOKEN}\"'"),
                Unavailable::CredentialsUnreadable,
            ),
            // serde_json quotes the value of the wrong type in its error.
            (
                format!("printf '%s' '{{\"claudeAiOauth\":\"{REMOTE_TOKEN}\"}}'"),
                Unavailable::CredentialsUnreadable,
            ),
            (
                oversized_credential_file_script(REMOTE_TOKEN),
                Unavailable::CredentialsUnreadable,
            ),
            (
                format!("printf '%s' '{}'; exit 7", credential_file(REMOTE_TOKEN)),
                Unavailable::HostUnavailable,
            ),
        ] {
            let host = crate::test_support::RecordingHost::running(&script);
            let read = smol::block_on(read_credentials_on_host(&host)).err();
            assert_eq!(read, Some(expected), "{script:?} took another path");
            errors.push(format!("{read:?}"));
        }
        let log = crate::test_support::captured_log();
        assert!(
            log.iter()
                .any(|line| line.contains("claude usage") && line.contains("401")),
            "the refusal was logged, so the capture saw this run: {log:?}"
        );
        assert!(
            log.iter()
                .any(|line| line.contains("claude usage") && line.contains("exit status: 7")),
            "the failed read was logged: {log:?}"
        );
        for text in errors.iter().chain(log.iter()) {
            assert!(
                !text.contains(REMOTE_TOKEN) && !text.contains("remote-account-secret"),
                "the token leaked into {text:?}"
            );
        }
    }
}
