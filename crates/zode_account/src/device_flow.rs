use std::sync::Arc;
use std::time::Duration;

use futures::AsyncReadExt as _;
use gpui::BackgroundExecutor;
use http_client::{AsyncBody, HttpClient, Request, StatusCode};
use serde::Deserialize;

use crate::tokens::StoredTokens;

/// RFC 8628 §3.5 asks a client that is told `slow_down` to widen its interval
/// by five seconds, permanently, for the rest of the flow.
const SLOW_DOWN_INCREMENT: Duration = Duration::from_secs(5);

/// A ceiling on the interval, so a server answering `slow_down` in a loop
/// cannot stretch the wait past the point of usefulness.
const MAX_INTERVAL: Duration = Duration::from_secs(30);

/// What the user is asked to do, and how long they have to do it.
#[derive(Clone, PartialEq, Eq)]
pub struct PendingAuthorization {
    /// Opaque; the client polls with this. Never shown to the user.
    pub device_code: String,
    /// Short and human-typed. This is the one on screen.
    pub user_code: String,
    pub verification_uri: String,
    /// Same page with the code filled in — for a client that can open a browser.
    pub verification_uri_complete: String,
    pub expires_in: Duration,
    pub interval: Duration,
}

impl std::fmt::Debug for PendingAuthorization {
    /// Everything but the device code, which is a bearer credential: whoever
    /// holds it can collect the tokens once the user approves. The user code is
    /// on screen by design and stays.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingAuthorization")
            .field("device_code", &"<redacted>")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("verification_uri_complete", &self.verification_uri_complete)
            .field("expires_in", &self.expires_in)
            .field("interval", &self.interval)
            .finish()
    }
}

/// Why a sign-in stopped.
///
/// Split into "the user or the server ended it" and "something went wrong" so
/// the UI can say which. A denied request is not a failure to report as one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceFlowError {
    /// The user pressed Deny.
    AccessDenied,
    /// Nobody approved it in time.
    Expired,
    /// The endpoint could not be reached, or answered something unusable.
    Unreachable(String),
    /// The server rejected the request in a way that retrying cannot fix.
    Rejected(String),
}

impl std::fmt::Display for DeviceFlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AccessDenied => write!(f, "the sign-in request was denied"),
            Self::Expired => write!(f, "the sign-in request expired"),
            Self::Unreachable(detail) => write!(f, "the account service is unreachable: {detail}"),
            Self::Rejected(detail) => {
                write!(f, "the account service rejected the request: {detail}")
            }
        }
    }
}

/// One poll, classified. Nothing here waits — that is `poll_until_authorized`'s
/// job, which keeps this a straight line a test can drive one answer at a time.
/// Same split, for the same reason, as `agent_usage::claude::Attempt`.
#[derive(Debug, PartialEq, Eq)]
enum PollOutcome {
    Authorized(Box<StoredTokens>),
    /// Ask again after the current interval.
    KeepWaiting,
    /// Ask again, but widen the interval first.
    SlowDown,
    Failed(DeviceFlowError),
}

#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
}

/// Asks the server to start a sign-in, and returns what to show the user.
pub async fn request_authorization(
    http_client: &Arc<dyn HttpClient>,
    api_url: &str,
) -> Result<PendingAuthorization, DeviceFlowError> {
    let body = post_json(http_client, &format!("{api_url}/auth/device/code"), "{}").await?;

    let parsed: DeviceCodeResponse = serde_json::from_str(&body).map_err(|_| {
        DeviceFlowError::Unreachable("the device code response could not be read".into())
    })?;

    Ok(PendingAuthorization {
        device_code: parsed.device_code,
        user_code: parsed.user_code,
        verification_uri: parsed.verification_uri,
        verification_uri_complete: parsed.verification_uri_complete,
        expires_in: Duration::from_secs(parsed.expires_in),
        interval: Duration::from_secs(parsed.interval),
    })
}

/// Polls until the user decides, the request expires, or the flow breaks.
///
/// The deadline is counted in polls against `expires_in` rather than against a
/// wall clock, so the loop terminates under a test executor where time is
/// advanced by hand and no real seconds pass.
pub async fn poll_until_authorized(
    http_client: &Arc<dyn HttpClient>,
    api_url: &str,
    pending: &PendingAuthorization,
    executor: &BackgroundExecutor,
) -> Result<StoredTokens, DeviceFlowError> {
    let mut interval = pending.interval;
    let mut waited = Duration::ZERO;

    loop {
        match poll_once(http_client, api_url, &pending.device_code).await {
            PollOutcome::Authorized(tokens) => return Ok(*tokens),
            PollOutcome::Failed(error) => return Err(error),
            PollOutcome::SlowDown => {
                interval = (interval + SLOW_DOWN_INCREMENT).min(MAX_INTERVAL);
            }
            PollOutcome::KeepWaiting => {}
        }

        if waited >= pending.expires_in {
            return Err(DeviceFlowError::Expired);
        }
        executor.timer(interval).await;
        waited += interval;
    }
}

/// Exchanges a refresh token for a fresh pair.
///
/// `Err(Rejected)` means the server refused this token and nothing else does:
/// the caller signs the user out and deletes the keychain entry on it, so a
/// throttled, timed-out or proxied request must come back as `Unreachable`
/// instead. See [`refusal_of_refresh_token`].
pub async fn refresh(
    http_client: &Arc<dyn HttpClient>,
    api_url: &str,
    refresh_token: &str,
) -> Result<StoredTokens, DeviceFlowError> {
    let request_body = serde_json::json!({ "refresh_token": refresh_token }).to_string();
    let answer = exchange(
        http_client,
        &format!("{api_url}/auth/device/refresh"),
        &request_body,
    )
    .await?;

    if !answer.status.is_success() {
        let Some(slug) = refusal_of_refresh_token(answer.status, &answer.body) else {
            return Err(DeviceFlowError::Unreachable(format!(
                "the server answered {} to the refresh",
                answer.status.as_u16()
            )));
        };
        return Err(DeviceFlowError::Rejected(slug));
    }

    let parsed: TokenResponse = serde_json::from_str(&answer.body).map_err(|_| {
        DeviceFlowError::Unreachable("the refresh response could not be read".into())
    })?;

    Ok(StoredTokens::new(
        parsed.access_token,
        parsed.refresh_token,
        Duration::from_secs(parsed.expires_in),
    ))
}

/// The error slug when this answer is the server refusing the refresh token.
///
/// Two shapes are a refusal. A 401 with the service's JSON error body is what
/// it sends for a token that is unknown, spent, or revoked (its own body names
/// no OAuth slug), and a 400 `invalid_grant` is the RFC 6749 section 5.2 form.
/// Anything else says nothing about the token: 408 and 429 are the network and
/// the rate limiter, a login page or firewall page is not JSON, and a 400 with
/// another slug is a malformed request, which deleting a good session cannot
/// fix. The cost of reading those as a refusal is a user signed out of
/// something that still works, so the default is to keep the session.
fn refusal_of_refresh_token(status: StatusCode, body: &str) -> Option<String> {
    let parsed = serde_json::from_str::<ErrorResponse>(body).ok()?;
    match status.as_u16() {
        401 => Some(parsed.error),
        400 if parsed.error == "invalid_grant" => Some(parsed.error),
        _ => None,
    }
}

/// Tells the server to forget a device session. Best effort by design — the
/// caller clears the keychain regardless of what this returns, because a user
/// who pressed Sign Out must end up signed out even with the network down.
pub async fn revoke(http_client: &Arc<dyn HttpClient>, api_url: &str, refresh_token: &str) {
    let request_body = serde_json::json!({ "refresh_token": refresh_token }).to_string();
    if let Err(error) = post_json(
        http_client,
        &format!("{api_url}/auth/device/logout"),
        &request_body,
    )
    .await
    {
        log::warn!("could not revoke the device session remotely: {error}");
    }
}

/// How this machine names itself in the account's device list.
///
/// Read from the OS rather than asked for: the sign-in flow is already the
/// longest path in the feature, and the machine knows what it is called. The
/// user can rename it afterwards on the web, where the cost of a text field is
/// nothing.
///
/// `sysinfo` rather than shelling out to `hostname`: the repo forbids
/// `std::process::Command` on the grounds that it blocks the calling thread
/// for an unknown duration, and this runs on the poll path where a stall would
/// be felt directly.
///
/// Capped to match the server's own limit, so an unusually long hostname is
/// trimmed here rather than rejected there.
fn device_name() -> Option<String> {
    const MAX_NAME: usize = 64;
    let name = sysinfo::System::host_name()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    Some(name.chars().take(MAX_NAME).collect())
}

fn platform() -> &'static str {
    // The server accepts exactly these three; anything else would be rejected
    // and cost the user their sign-in over a cosmetic field.
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

async fn poll_once(
    http_client: &Arc<dyn HttpClient>,
    api_url: &str,
    device_code: &str,
) -> PollOutcome {
    // Sent on every poll rather than only the last one: the client cannot know
    // which poll will be the one that succeeds, and the server ignores the
    // fields until it issues a session.
    let mut body = serde_json::json!({
        "device_code": device_code,
        "platform": platform(),
    });
    if let Some(name) = device_name() {
        body["device_name"] = name.into();
    }
    let request_body = body.to_string();

    match post_json(
        http_client,
        &format!("{api_url}/auth/device/token"),
        &request_body,
    )
    .await
    {
        Ok(body) => match serde_json::from_str::<TokenResponse>(&body) {
            Ok(parsed) => PollOutcome::Authorized(Box::new(StoredTokens::new(
                parsed.access_token,
                parsed.refresh_token,
                Duration::from_secs(parsed.expires_in),
            ))),
            Err(_) => PollOutcome::Failed(DeviceFlowError::Unreachable(
                "the token response could not be read".into(),
            )),
        },
        Err(DeviceFlowError::Rejected(code)) => match code.as_str() {
            "authorization_pending" => PollOutcome::KeepWaiting,
            "slow_down" => PollOutcome::SlowDown,
            "access_denied" => PollOutcome::Failed(DeviceFlowError::AccessDenied),
            "expired_token" => PollOutcome::Failed(DeviceFlowError::Expired),
            other => PollOutcome::Failed(DeviceFlowError::Rejected(other.to_string())),
        },
        // A transport hiccup mid-poll is not a reason to abandon a sign-in the
        // user may already have approved; the deadline still bounds the loop.
        Err(DeviceFlowError::Unreachable(_)) => PollOutcome::KeepWaiting,
        Err(other) => PollOutcome::Failed(other),
    }
}

/// What came back from a POST that reached a server, before anyone decides
/// what the status means.
struct Answer {
    status: StatusCode,
    body: String,
}

/// One POST. A transport failure is `Unreachable`; any answer at all, whatever
/// its status, is returned for the caller to classify.
async fn exchange(
    http_client: &Arc<dyn HttpClient>,
    url: &str,
    body: &str,
) -> Result<Answer, DeviceFlowError> {
    let request = Request::builder()
        .method("POST")
        .uri(url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .body(AsyncBody::from(body.to_string()))
        .map_err(|_| DeviceFlowError::Unreachable("the request could not be built".into()))?;

    let mut response = http_client
        .send(request)
        .await
        .map_err(|_| DeviceFlowError::Unreachable("the endpoint could not be reached".into()))?;

    let status = response.status();
    let mut response_body = String::new();
    if response
        .body_mut()
        .read_to_string(&mut response_body)
        .await
        .is_err()
    {
        return Err(DeviceFlowError::Unreachable(
            "the response could not be read".into(),
        ));
    }

    Ok(Answer {
        status,
        body: response_body,
    })
}

/// One POST. `Err(Rejected(code))` carries the server's RFC 8628 error slug
/// when it sent one, which is what makes the classification above a lookup
/// rather than a guess at the status code.
async fn post_json(
    http_client: &Arc<dyn HttpClient>,
    url: &str,
    body: &str,
) -> Result<String, DeviceFlowError> {
    let Answer {
        status,
        body: response_body,
    } = exchange(http_client, url, body).await?;

    if status.is_success() {
        return Ok(response_body);
    }

    if status.is_server_error() {
        return Err(DeviceFlowError::Unreachable(format!(
            "the server answered {}",
            status.as_u16()
        )));
    }

    // The error slug, never the whole body: an error body from an
    // authenticated endpoint is exactly the sort of thing that echoes a
    // credential back into a log.
    //
    // A body that is NOT that JSON means we never reached the account service
    // at all — most often a host serving a single-page app for every path, so
    // `/api/...` returns index.html and a POST to it answers 405. Reporting
    // that as `http_405` reads as "the API refused us" and sends whoever is
    // debugging into the request code, when the fault is that nothing is
    // routing `/api` to the backend. So it says which URL, and what to check.
    match serde_json::from_str::<ErrorResponse>(&response_body) {
        Ok(parsed) => Err(DeviceFlowError::Rejected(parsed.error)),
        Err(_) => Err(DeviceFlowError::Rejected(format!(
            "{url} answered {} without a JSON error, so it is not the account \
             service — check that /api is routed to the backend",
            status.as_u16()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use http_client::{FakeHttpClient, Response};
    use std::sync::Mutex;

    /// Answers each request with the next canned response, so a test can lay
    /// out an entire polling conversation as a list.
    /// Captures the body of every request, so a test can assert what was sent
    /// rather than only what came back.
    fn recording(status: u16, body: String) -> (Arc<dyn HttpClient>, Arc<Mutex<Vec<String>>>) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let recorder = sent.clone();

        let client = FakeHttpClient::create(move |request| {
            let recorder = recorder.clone();
            let body = body.clone();
            async move {
                use futures::AsyncReadExt as _;
                let mut raw = String::new();
                let mut request_body = request.into_body();
                let _ = request_body.read_to_string(&mut raw).await;
                recorder.lock().unwrap().push(raw);
                Ok(Response::builder()
                    .status(status)
                    .body(body.into())
                    .unwrap())
            }
        });

        (client as Arc<dyn HttpClient>, sent)
    }

    /// The device list is only useful if the machine says what it is, and the
    /// server accepts exactly three platform values — a fourth would cost the
    /// user their sign-in over a cosmetic field.
    #[gpui::test]
    async fn the_poll_tells_the_server_what_machine_this_is() {
        let (client, sent) = recording(400, r#"{"error":"authorization_pending"}"#.to_string());

        let _ = poll_once(&client, "https://zodekit.site/api", "dc").await;

        let bodies = sent.lock().unwrap().clone();
        let parsed: serde_json::Value = serde_json::from_str(&bodies[0]).unwrap();
        assert_eq!(parsed["device_code"], "dc");
        assert!(
            ["macos", "linux", "windows"].contains(&parsed["platform"].as_str().unwrap()),
            "platform must be one the server accepts, got {:?}",
            parsed["platform"],
        );
    }

    fn scripted(steps: Vec<(u16, String)>) -> (Arc<dyn HttpClient>, Arc<Mutex<usize>>) {
        let calls = Arc::new(Mutex::new(0));
        let counter = calls.clone();
        let steps = Arc::new(steps);

        let client = FakeHttpClient::create(move |_request| {
            let steps = steps.clone();
            let counter = counter.clone();
            async move {
                let index = {
                    let mut guard = counter.lock().unwrap();
                    let current = *guard;
                    *guard += 1;
                    current
                };
                let (status, body) = steps
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| steps.last().cloned().expect("no scripted responses"));
                Ok(Response::builder()
                    .status(status)
                    .body(body.into())
                    .unwrap())
            }
        });

        (client as Arc<dyn HttpClient>, calls)
    }

    fn pending(interval_secs: u64, expires_secs: u64) -> PendingAuthorization {
        PendingAuthorization {
            device_code: "device".into(),
            user_code: "A1B2-C3D4".into(),
            verification_uri: "https://zodekit.site/activate".into(),
            verification_uri_complete: "https://zodekit.site/activate?code=A1B2-C3D4".into(),
            expires_in: Duration::from_secs(expires_secs),
            interval: Duration::from_secs(interval_secs),
        }
    }

    fn error_body(code: &str) -> String {
        serde_json::json!({ "error": code }).to_string()
    }

    fn token_body() -> String {
        serde_json::json!({
            "access_token": "access-1",
            "refresh_token": "refresh-1",
            "expires_in": 900,
            "token_type": "Bearer"
        })
        .to_string()
    }

    #[gpui::test]
    async fn requesting_authorization_reads_what_the_user_must_be_shown(cx: &mut TestAppContext) {
        let body = serde_json::json!({
            "device_code": "dc",
            "user_code": "A1B2-C3D4",
            "verification_uri": "https://zodekit.site/activate",
            "verification_uri_complete": "https://zodekit.site/activate?code=A1B2-C3D4",
            "expires_in": 600,
            "interval": 5
        })
        .to_string();
        let (client, _) = scripted(vec![(201, body)]);

        let result = request_authorization(&client, "https://zodekit.site/api")
            .await
            .unwrap();

        assert_eq!(result.user_code, "A1B2-C3D4");
        assert_eq!(result.interval, Duration::from_secs(5));
        assert_eq!(result.expires_in, Duration::from_secs(600));
        cx.background_executor.run_until_parked();
    }

    #[gpui::test]
    async fn polls_through_pending_until_the_user_approves(cx: &mut TestAppContext) {
        let (client, calls) = scripted(vec![
            (400, error_body("authorization_pending")),
            (400, error_body("authorization_pending")),
            (201, token_body()),
        ]);

        let tokens = poll_until_authorized(
            &client,
            "https://zodekit.site/api",
            &pending(5, 600),
            &cx.background_executor,
        )
        .await
        .unwrap();

        assert_eq!(tokens.access_token, "access-1");
        assert_eq!(*calls.lock().unwrap(), 3);
    }

    #[gpui::test]
    async fn a_denied_request_stops_immediately(cx: &mut TestAppContext) {
        let (client, calls) = scripted(vec![(400, error_body("access_denied"))]);

        let error = poll_until_authorized(
            &client,
            "https://zodekit.site/api",
            &pending(5, 600),
            &cx.background_executor,
        )
        .await
        .unwrap_err();

        assert_eq!(error, DeviceFlowError::AccessDenied);
        // No second ask: the answer will not change.
        assert_eq!(*calls.lock().unwrap(), 1);
    }

    #[gpui::test]
    async fn an_expired_code_stops_immediately(cx: &mut TestAppContext) {
        let (client, _) = scripted(vec![(400, error_body("expired_token"))]);

        let error = poll_until_authorized(
            &client,
            "https://zodekit.site/api",
            &pending(5, 600),
            &cx.background_executor,
        )
        .await
        .unwrap_err();

        assert_eq!(error, DeviceFlowError::Expired);
    }

    #[gpui::test]
    async fn slow_down_widens_the_interval_instead_of_giving_up(cx: &mut TestAppContext) {
        let (client, calls) = scripted(vec![
            (400, error_body("slow_down")),
            (400, error_body("slow_down")),
            (201, token_body()),
        ]);

        let tokens = poll_until_authorized(
            &client,
            "https://zodekit.site/api",
            &pending(5, 600),
            &cx.background_executor,
        )
        .await
        .unwrap();

        assert_eq!(tokens.refresh_token, "refresh-1");
        assert_eq!(*calls.lock().unwrap(), 3);
    }

    #[gpui::test]
    async fn a_transport_hiccup_keeps_waiting_rather_than_abandoning_the_sign_in(
        cx: &mut TestAppContext,
    ) {
        // 500 mid-poll: the user may already have pressed Approve in their
        // browser, so dropping the flow here would lose their consent.
        let (client, calls) = scripted(vec![
            (500, String::new()),
            (400, error_body("authorization_pending")),
            (201, token_body()),
        ]);

        let tokens = poll_until_authorized(
            &client,
            "https://zodekit.site/api",
            &pending(5, 600),
            &cx.background_executor,
        )
        .await
        .unwrap();

        assert_eq!(tokens.access_token, "access-1");
        assert_eq!(*calls.lock().unwrap(), 3);
    }

    #[gpui::test]
    async fn the_loop_terminates_when_the_deadline_passes(cx: &mut TestAppContext) {
        // Always pending, and a deadline shorter than two intervals.
        let (client, _) = scripted(vec![(400, error_body("authorization_pending"))]);

        let error = poll_until_authorized(
            &client,
            "https://zodekit.site/api",
            &pending(5, 5),
            &cx.background_executor,
        )
        .await
        .unwrap_err();

        assert_eq!(error, DeviceFlowError::Expired);
    }

    #[gpui::test]
    async fn refresh_returns_a_fresh_pair(cx: &mut TestAppContext) {
        let (client, _) = scripted(vec![(201, token_body())]);

        let tokens = refresh(&client, "https://zodekit.site/api", "old-refresh")
            .await
            .unwrap();

        assert_eq!(tokens.refresh_token, "refresh-1");
        assert!(!tokens.needs_refresh(std::time::SystemTime::now()));
        cx.background_executor.run_until_parked();
    }

    #[gpui::test]
    async fn a_rejected_refresh_is_reported_as_rejected_not_unreachable(cx: &mut TestAppContext) {
        // The distinction decides whether the user gets signed out or merely
        // marked offline, so it must not blur.
        let (client, _) = scripted(vec![(401, error_body("invalid_grant"))]);

        let error = refresh(&client, "https://zodekit.site/api", "spent")
            .await
            .unwrap_err();

        assert!(matches!(error, DeviceFlowError::Rejected(_)));
        cx.background_executor.run_until_parked();
    }

    /// The exact production failure this message was rewritten for: a host that
    /// serves its single-page app for every path answers a POST to
    /// `/api/auth/device/code` with 405 and an HTML body. The old message said
    /// `http_405`, which reads as "the API refused us" and cost a real
    /// debugging session before anyone looked at the nginx config.
    #[gpui::test]
    async fn a_non_json_4xx_says_the_url_is_not_the_account_service(cx: &mut TestAppContext) {
        let (client, _) = scripted(vec![(405, "<!doctype html><html>...".into())]);

        let error = request_authorization(&client, "https://zodekit.site/api")
            .await
            .unwrap_err();

        let message = error.to_string();
        assert!(
            message.contains("not the account service"),
            "expected a message naming the misconfiguration, got: {message}"
        );
        assert!(
            message.contains("/api"),
            "the message must name the URL: {message}"
        );
        assert!(
            !message.contains("http_405"),
            "the synthetic slug is what sent the last reader the wrong way: {message}"
        );
        cx.background_executor.run_until_parked();
    }

    /// What the account service really sends when it refuses a refresh token:
    /// the framework's own body for an `UnauthorizedException`, which carries no
    /// OAuth slug at all. The device-token route's `invalid_grant` is the other
    /// shape a refusal can take.
    #[gpui::test]
    async fn only_an_explicit_refusal_of_the_token_is_a_rejection(cx: &mut TestAppContext) {
        for (status, body) in [
            (
                401,
                r#"{"statusCode":401,"message":"Refresh token invalid or reused","error":"Unauthorized"}"#
                    .to_string(),
            ),
            (401, error_body("invalid_grant")),
            (400, error_body("invalid_grant")),
        ] {
            let (client, _) = scripted(vec![(status, body.clone())]);

            let error = refresh(&client, "https://zodekit.site/api", "spent")
                .await
                .unwrap_err();

            assert!(
                matches!(error, DeviceFlowError::Rejected(_)),
                "{status} {body} must be a rejection, got {error:?}"
            );
        }
        cx.background_executor.run_until_parked();
    }

    /// Signing someone out wipes their keychain entry, so a rate limit, a
    /// timeout, a captive portal, or a firewall page must never be read as the
    /// server refusing the token.
    #[gpui::test]
    async fn an_answer_that_is_not_a_refusal_leaves_the_session_alone(cx: &mut TestAppContext) {
        let throttled = r#"{"statusCode":429,"message":"ThrottlerException: Too Many Requests"}"#;
        let bad_request = r#"{"statusCode":400,"message":["refresh_token must be a string"],"error":"Bad Request"}"#;
        for (status, body) in [
            (408, String::new()),
            (429, throttled.to_string()),
            (429, error_body("rate_limited")),
            (
                401,
                "<html><body>Sign in to the network</body></html>".to_string(),
            ),
            (401, String::new()),
            (403, "<!doctype html><title>Blocked</title>".to_string()),
            (404, "<!doctype html>".to_string()),
            (405, "<!doctype html>".to_string()),
            (400, bad_request.to_string()),
            (400, error_body("invalid_request")),
            (502, String::new()),
        ] {
            let (client, _) = scripted(vec![(status, body.clone())]);

            let error = refresh(&client, "https://zodekit.site/api", "fine")
                .await
                .unwrap_err();

            assert!(
                matches!(error, DeviceFlowError::Unreachable(_)),
                "{status} {body:?} must not be read as a rejection, got {error:?}"
            );
        }
        cx.background_executor.run_until_parked();
    }

    #[test]
    fn debug_output_never_prints_the_device_code() {
        let authorization = PendingAuthorization {
            device_code: "super-secret-device-code".into(),
            ..pending(5, 600)
        };

        let rendered = format!("{authorization:?}");

        assert!(!rendered.contains("super-secret-device-code"), "{rendered}");
        assert!(
            rendered.contains("A1B2-C3D4"),
            "the code on screen is not secret"
        );
    }

    #[gpui::test]
    async fn a_server_error_on_refresh_is_unreachable_not_rejected(cx: &mut TestAppContext) {
        let (client, _) = scripted(vec![(503, String::new())]);

        let error = refresh(&client, "https://zodekit.site/api", "fine")
            .await
            .unwrap_err();

        assert!(matches!(error, DeviceFlowError::Unreachable(_)));
        cx.background_executor.run_until_parked();
    }
}
