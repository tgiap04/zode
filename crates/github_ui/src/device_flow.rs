//! GitHub's OAuth device flow, run straight from Zode with the zodekit OAuth
//! App's public `client_id`. No secret is involved and the token never passes
//! through a zodekit server.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use futures::future::{Either, select};
use futures::{AsyncReadExt as _, FutureExt as _};
use gpui::BackgroundExecutor;
use http_client::{AsyncBody, HttpClient, RedirectPolicy, Request};
use serde::Deserialize;
use url::Url;

pub const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
pub const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
/// Issues and pull requests (`repo`), organization membership for org
/// repositories (`read:org`), and Projects v2 (`project`).
pub const SCOPES: &str = "repo read:org project";
const DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// GitHub's own minimum; a server asking for less is not believed.
const MIN_INTERVAL: Duration = Duration::from_secs(5);
/// RFC 8628 §3.5: `slow_down` widens the interval by five seconds for the rest
/// of the flow.
const SLOW_DOWN_INCREMENT: Duration = Duration::from_secs(5);
const MAX_INTERVAL: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Both answers are a few hundred bytes; anything past this is not GitHub.
const MAX_ANSWER_BYTES: u64 = 64 * 1024;

/// What the user is shown while the flow waits for them.
#[derive(Clone, PartialEq, Eq)]
pub struct DeviceCode {
    /// A bearer credential until the flow ends: never shown or logged.
    device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: Duration,
    pub interval: Duration,
}

impl fmt::Debug for DeviceCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeviceCode")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("expires_in", &self.expires_in)
            .field("interval", &self.interval)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceFlowError {
    AccessDenied,
    Expired,
    /// The OAuth App does not have device flow switched on.
    Disabled,
    Unreachable(String),
    Rejected(String),
}

impl fmt::Display for DeviceFlowError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccessDenied => write!(formatter, "The sign-in was denied on GitHub."),
            Self::Expired => write!(
                formatter,
                "The code expired before it was entered. Start again."
            ),
            Self::Disabled => write!(
                formatter,
                "Sign-in from Zode is not enabled for this GitHub app. Use the GitHub CLI instead."
            ),
            Self::Unreachable(detail) => write!(formatter, "Could not reach GitHub: {detail}"),
            Self::Rejected(detail) => write!(formatter, "GitHub refused the sign-in: {detail}"),
        }
    }
}

#[derive(Deserialize)]
struct DeviceCodeAnswer {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: Option<u64>,
}

/// GitHub answers a poll with HTTP 200 whether or not a token came back, so
/// both shapes are read from one struct.
#[derive(Deserialize)]
struct TokenAnswer {
    access_token: Option<String>,
    error: Option<String>,
    interval: Option<u64>,
}

#[derive(Debug, PartialEq, Eq)]
enum PollOutcome {
    Token(String),
    KeepWaiting,
    SlowDown { interval: Option<Duration> },
    Failed(DeviceFlowError),
}

pub async fn request_device_code(
    http_client: &Arc<dyn HttpClient>,
    client_id: &str,
    executor: &BackgroundExecutor,
) -> Result<DeviceCode, DeviceFlowError> {
    let body = form(&[("client_id", client_id), ("scope", SCOPES)]);
    let (status, answer) = post_form(http_client, DEVICE_CODE_URL, body, executor).await?;
    if let Some(error) = error_of(&answer) {
        return Err(classify_error(&error).unwrap_or(DeviceFlowError::Rejected(error)));
    }
    if !(200..300).contains(&status) {
        return Err(DeviceFlowError::Unreachable(format!(
            "GitHub answered HTTP {status}"
        )));
    }
    let answer: DeviceCodeAnswer = serde_json::from_str(&answer).map_err(|_| {
        DeviceFlowError::Unreachable("GitHub's answer could not be read".to_string())
    })?;
    // Opened in the browser by a button press, so it must be GitHub's page.
    if !is_github_page(&answer.verification_uri) {
        return Err(DeviceFlowError::Rejected(
            "the verification page is not on github.com".to_string(),
        ));
    }
    Ok(DeviceCode {
        device_code: answer.device_code,
        user_code: answer.user_code,
        verification_uri: answer.verification_uri,
        expires_in: Duration::from_secs(answer.expires_in),
        interval: clamp_interval(answer.interval.map(Duration::from_secs)),
    })
}

/// Waits an interval, polls, and repeats until the user decides or the code
/// expires. Returns the token's secret. Dropping the future cancels the flow.
///
/// The deadline is counted in intervals waited rather than read off a clock,
/// so a test executor that advances time by hand still ends the loop.
pub async fn poll_for_token(
    http_client: &Arc<dyn HttpClient>,
    client_id: &str,
    code: &DeviceCode,
    executor: &BackgroundExecutor,
) -> Result<String, DeviceFlowError> {
    let mut interval = code.interval;
    let mut waited = Duration::ZERO;
    loop {
        if waited >= code.expires_in {
            return Err(DeviceFlowError::Expired);
        }
        executor.timer(interval).await;
        waited += interval;
        match poll_once(http_client, client_id, &code.device_code, executor).await {
            PollOutcome::Token(token) => return Ok(token),
            PollOutcome::Failed(error) => return Err(error),
            PollOutcome::KeepWaiting => {}
            PollOutcome::SlowDown { interval: asked } => {
                let widened = (interval + SLOW_DOWN_INCREMENT).min(MAX_INTERVAL);
                interval = asked.map_or(widened, |asked| clamp_interval(Some(asked)).max(widened));
            }
        }
    }
}

async fn poll_once(
    http_client: &Arc<dyn HttpClient>,
    client_id: &str,
    device_code: &str,
    executor: &BackgroundExecutor,
) -> PollOutcome {
    let body = form(&[
        ("client_id", client_id),
        ("device_code", device_code),
        ("grant_type", DEVICE_GRANT_TYPE),
    ]);
    let answer = match post_form(http_client, ACCESS_TOKEN_URL, body, executor).await {
        Ok((_status, answer)) => answer,
        // A dropped connection mid-wait is no reason to abandon a sign-in the
        // user may already have approved; the deadline still ends the loop.
        Err(DeviceFlowError::Unreachable(_)) => return PollOutcome::KeepWaiting,
        Err(error) => return PollOutcome::Failed(error),
    };
    let Ok(answer) = serde_json::from_str::<TokenAnswer>(&answer) else {
        return PollOutcome::KeepWaiting;
    };
    if let Some(token) = answer.access_token.filter(|token| !token.trim().is_empty()) {
        return PollOutcome::Token(token);
    }
    match answer.error.as_deref() {
        Some("authorization_pending") => PollOutcome::KeepWaiting,
        Some("slow_down") => PollOutcome::SlowDown {
            interval: answer.interval.map(Duration::from_secs),
        },
        Some(error) => PollOutcome::Failed(
            classify_error(error).unwrap_or_else(|| DeviceFlowError::Rejected(error.to_string())),
        ),
        None => PollOutcome::KeepWaiting,
    }
}

fn classify_error(error: &str) -> Option<DeviceFlowError> {
    match error {
        "access_denied" => Some(DeviceFlowError::AccessDenied),
        "expired_token" => Some(DeviceFlowError::Expired),
        "device_flow_disabled" => Some(DeviceFlowError::Disabled),
        _ => None,
    }
}

fn error_of(answer: &str) -> Option<String> {
    serde_json::from_str::<TokenAnswer>(answer).ok()?.error
}

fn clamp_interval(interval: Option<Duration>) -> Duration {
    interval
        .unwrap_or(MIN_INTERVAL)
        .clamp(MIN_INTERVAL, MAX_INTERVAL)
}

fn form(pairs: &[(&str, &str)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (name, value) in pairs {
        serializer.append_pair(name, value);
    }
    serializer.finish()
}

/// Only `https://github.com/...`, compared on the parsed host so
/// `github.com.example` does not pass.
pub fn is_github_page(url: &str) -> bool {
    Url::parse(url).is_ok_and(|url| {
        url.scheme() == "https" && url.host_str() == Some("github.com") && url.port().is_none()
    })
}

/// The answer's status and body. The body is never logged: a token answer
/// holds the token.
async fn post_form(
    http_client: &Arc<dyn HttpClient>,
    url: &str,
    body: String,
    executor: &BackgroundExecutor,
) -> Result<(u16, String), DeviceFlowError> {
    let mut request = Request::builder()
        .method("POST")
        .uri(url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("User-Agent", "Zode")
        .body(AsyncBody::from(body))
        .map_err(|_| DeviceFlowError::Unreachable("the request could not be built".into()))?;
    request.extensions_mut().insert(RedirectPolicy::NoFollow);

    let exchange = async {
        let mut response = http_client.send(request).await?;
        let mut bytes = Vec::new();
        response
            .body_mut()
            .take(MAX_ANSWER_BYTES)
            .read_to_end(&mut bytes)
            .await?;
        anyhow::Ok((
            response.status().as_u16(),
            String::from_utf8_lossy(&bytes).into_owned(),
        ))
    };
    match select(exchange.boxed(), executor.timer(REQUEST_TIMEOUT)).await {
        Either::Left((Ok(answer), _)) => Ok(answer),
        Either::Left((Err(_), _)) => Err(DeviceFlowError::Unreachable(
            "the connection failed".to_string(),
        )),
        Either::Right(_) => Err(DeviceFlowError::Unreachable(format!(
            "no answer within {} seconds",
            REQUEST_TIMEOUT.as_secs()
        ))),
    }
}
