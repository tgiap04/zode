//! Codex's plan quota, read over the app-server's JSON-RPC.
//!
//! `codex app-server` speaks newline-delimited JSON-RPC 2.0 on stdio, and
//! `account/rateLimits/read` answers with the windows this module turns into
//! [`UsageWindow`]s. Unlike the Claude path, **nothing here touches a
//! credential**: the subprocess already holds its own session and refreshes it
//! itself, which is the reason this route was chosen over the undocumented
//! `chatgpt.com/backend-api` endpoint that would have required the editor to
//! carry the user's OAuth token.
//!
//! # Verified against a live `codex app-server`
//!
//! The shape below was recorded from `codex-cli 0.149.0` and cross-checked against
//! the protocol schema the CLI itself emits (`codex app-server
//! generate-json-schema`), where `GetAccountRateLimitsResponse` requires
//! `rateLimits` and `RateLimitWindow` requires `usedPercent`.
//!
//! The probe also caught something no documentation mentioned: **the server
//! answers `-32600 "Not initialized"` to anything sent before an `initialize`
//! request.** Without that handshake this method never returns data at all.
//!
//! The schema is still moving, so [`Unavailable::Unreadable`] remains, and it logs
//! the JSON *key names* it could not make sense of — never their values, since
//! this payload comes from a process holding an authenticated session.

use std::time::Duration;

use chrono::{TimeZone as _, Utc};
use futures::{AsyncBufReadExt as _, AsyncWriteExt as _, StreamExt as _, io::BufReader};
use gpui::{BackgroundExecutor, SharedString};
use serde::Deserialize;
use util::command::{Stdio, new_command};

use crate::{UsageWindow, WindowKind};

/// How long to wait for the app-server to answer before giving up.
///
/// It has to cover process start plus a round trip. Generous, because the cost of
/// waiting is a status bar that fills in a moment later, while the cost of being
/// too eager is a working setup that reports itself broken.
const RPC_TIMEOUT: Duration = Duration::from_secs(10);

const RATE_LIMITS_METHOD: &str = "account/rateLimits/read";

/// The app-server refuses every method until this one has been answered.
///
/// Found by probing, not by reading: nothing in the documentation says so, and the
/// failure is a flat `-32600 "Not initialized"` that looks like a refused request
/// rather than a missing step.
const INITIALIZE_METHOD: &str = "initialize";

/// The id the rate-limit request carries, so its reply can be told from the
/// initialize reply and from the notifications that arrive between them.
const RATE_LIMITS_ID: u32 = 2;

/// Why there is nothing to show for Codex.
///
/// The three variants exist to be told apart. "Nothing shown" for a missing CLI,
/// a signed-out session, and a response this build could not read are three
/// different problems with three different fixes, and collapsing them into one
/// blank space is what makes an unverified schema unfixable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unavailable {
    /// No `codex` on `PATH`.
    NotInstalled,
    /// The server answered, but this build could not find the windows in it.
    /// The payload is logged when this is produced.
    Unreadable(SharedString),
    /// The process could not be started, or did not answer in time.
    Failed(SharedString),
}

#[derive(Deserialize)]
struct RpcEnvelope {
    #[serde(default)]
    result: Option<RateLimitsResult>,
    #[serde(default)]
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
struct RateLimitsResult {
    #[serde(rename = "rateLimits", default)]
    rate_limits: Option<RateLimits>,
}

#[derive(Deserialize)]
struct RateLimits {
    #[serde(default)]
    primary: Option<RateLimitWindow>,
    /// Absent on plans with a single window — observed `null` on a free plan.
    #[serde(default)]
    secondary: Option<RateLimitWindow>,
    /// A name for what is being metered, when the server offers one. `null` on the
    /// plan this was recorded from, so it is read but never relied on.
    #[serde(rename = "limitName", default)]
    limit_name: Option<String>,
}

#[derive(Deserialize)]
struct RateLimitWindow {
    /// The schema types this as a required `int32`. Read as a float anyway: a JSON
    /// integer parses into one, so this accepts today's shape and tomorrow's if it
    /// gains a decimal, which costs nothing and cannot be wrong.
    #[serde(rename = "usedPercent", default)]
    used_percent: Option<f64>,
    /// Unix seconds. The Claude endpoint uses RFC 3339 for the same idea; this one
    /// does not, which is why the conversion lives here rather than being shared.
    #[serde(rename = "resetsAt", default)]
    resets_at: Option<i64>,
    /// How long this window is, in minutes.
    ///
    /// The only thing Codex says about *which* window this is — it names no kind
    /// at all. The recorded payload reads 43 200, which is thirty days, so the
    /// name has to be derived from this rather than assumed to be one of the two
    /// lengths Claude uses.
    #[serde(rename = "windowDurationMins", default)]
    window_duration_mins: Option<u64>,
}

impl RateLimitWindow {
    fn into_usage_window(self, label: Option<SharedString>) -> Option<UsageWindow> {
        let percent = self.used_percent?;
        if !percent.is_finite() {
            return None;
        }
        let resets_at = self
            .resets_at
            .filter(|seconds| *seconds > 0)
            .and_then(|seconds| Utc.timestamp_opt(seconds, 0).single());
        let kind = self
            .window_duration_mins
            .filter(|minutes| *minutes > 0)
            .map(|minutes| WindowKind::Span(Duration::from_secs(minutes * 60)))
            .unwrap_or(WindowKind::Unknown);

        Some(UsageWindow {
            percent: percent.round().clamp(0.0, 100.0) as u8,
            resets_at,
            // Only ever shown when there is no countdown to show instead, which
            // for Codex means a window with no `resetsAt`.
            label,
            kind,
        })
    }
}

/// Whether this line is the JSON-RPC reply to `id`.
///
/// Parsed rather than string-matched: `"id":2` can appear inside a payload that is
/// not the reply, and a notification carries no id at all.
fn is_reply_to(line: &str, id: u32) -> bool {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|value| value.get("id").and_then(|id| id.as_u64()))
        .is_some_and(|found| found == u64::from(id))
}

/// The key paths in a JSON document, without any of its values.
///
/// This is what the diagnostic log needs and the most it should have. The whole
/// point of logging anything here is to say *which names came back* when the
/// expected ones did not — values add nothing to that, and a payload from a
/// process holding an authenticated session is not something to copy into a log
/// file on the off-chance it is interesting.
fn key_paths(line: &str) -> Vec<String> {
    fn walk(value: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    let path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    walk(child, &path, out);
                    if !out.iter().any(|seen| seen == &path) {
                        out.push(path);
                    }
                }
            }
            // An array's shape is its first element's shape; the rest repeat it.
            serde_json::Value::Array(items) => {
                if let Some(first) = items.first() {
                    walk(first, &format!("{prefix}[]"), out);
                }
            }
            _ => {}
        }
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return vec!["<not json>".to_string()];
    };
    let mut out = Vec::new();
    walk(&value, "", &mut out);
    out.sort();
    out
}

/// Turns one JSON-RPC response line into windows.
///
/// Separated from the process so the shape can be asserted without a `codex`
/// binary — which matters more here than anywhere else in this crate, because
/// these field names are the part nobody has verified against a live server.
pub fn parse_windows(line: &str) -> Result<Vec<UsageWindow>, Unavailable> {
    let envelope: RpcEnvelope = serde_json::from_str(line).map_err(|_| {
        log::warn!(
            "codex app-server sent a line that is not JSON ({} bytes)",
            line.len()
        );
        Unavailable::Unreadable("the response was not JSON-RPC".into())
    })?;

    if let Some(error) = envelope.error {
        let message = error.message.unwrap_or_else(|| "no message".into());
        return Err(Unavailable::Failed(
            format!("codex refused the request: {message}").into(),
        ));
    }

    let windows: Vec<UsageWindow> = envelope
        .result
        .and_then(|result| result.rate_limits)
        .map(|limits| {
            let label = limits
                .limit_name
                .filter(|name| !name.trim().is_empty())
                .map(SharedString::from);
            [limits.primary, limits.secondary]
                .into_iter()
                .flatten()
                .filter_map(|window| window.into_usage_window(label.clone()))
                .collect()
        })
        .unwrap_or_default();

    if windows.is_empty() {
        // The one place a payload is logged, and the reason this variant exists:
        // these field names were taken from documentation rather than from an
        // observed response, so a mismatch has to be visible in one line rather
        // than looking like a signed-out session.
        log::warn!(
            "codex app-server answered {RATE_LIMITS_METHOD} but no rate-limit \
             windows could be read from it. Expected `result.rateLimits.primary` \
             and `.secondary`, each with `usedPercent` and `resetsAt`. Got these \
             keys instead: {}",
            key_paths(line).join(", ")
        );
        return Err(Unavailable::Unreadable(
            "codex answered, but this build could not read its rate limits".into(),
        ));
    }

    Ok(windows)
}

/// Asks a fresh `codex app-server` for the current windows.
///
/// A short-lived process per read rather than one kept alive: a long-lived
/// app-server would be a second session running beside the one the agent panel
/// already starts, and OpenAI's own issue tracker warns that two processes can
/// race on refreshing `auth.json`. A process per minute is the cheaper mistake to
/// have made.
pub async fn fetch(executor: BackgroundExecutor) -> Result<Vec<UsageWindow>, Unavailable> {
    let binary = executor
        .spawn(async { which::which("codex").ok() })
        .await
        .ok_or(Unavailable::NotInstalled)?;

    let timeout = async {
        executor.timer(RPC_TIMEOUT).await;
        Err(Unavailable::Failed(
            "codex app-server did not answer in time".into(),
        ))
    };

    smol::future::or(read_windows(binary), timeout).await
}

/// The read itself, with no deadline of its own.
///
/// Split from [`fetch`] because a timeout is a policy and this is the work. It also
/// makes the work reachable from a test: under gpui's test executor the timer runs
/// on a virtual clock and fires immediately, so anything racing one can never
/// observe the other side win.
async fn read_windows(binary: std::path::PathBuf) -> Result<Vec<UsageWindow>, Unavailable> {
    let mut child = new_command(&binary)
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            Unavailable::Failed(format!("codex app-server would not start: {error}").into())
        })?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| Unavailable::Failed("codex app-server has no stdin".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Unavailable::Failed("codex app-server has no stdout".into()))?;

    // The handshake first: everything else is met with `-32600 "Not initialized"`
    // until it has been answered. Nothing in the documentation says so -- it was
    // found by probing a live server.
    let initialize = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"{INITIALIZE_METHOD}\",\
         \"params\":{{\"clientInfo\":{{\"name\":\"zode\",\"version\":\"{}\"}}}}}}\n",
        env!("CARGO_PKG_VERSION")
    );
    let request = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{RATE_LIMITS_ID},\"method\":\"{RATE_LIMITS_METHOD}\"}}\n"
    );
    for message in [initialize, request] {
        stdin.write_all(message.as_bytes()).await.map_err(|error| {
            Unavailable::Failed(format!("could not talk to codex: {error}").into())
        })?;
    }
    stdin.flush().await.ok();

    // Both requests go out before either reply is read, so the one that matters has
    // to be found by its id. Between them the server also emits unsolicited
    // notifications -- `remoteControl/status/changed` was observed -- which carry
    // no id at all.
    let mut lines = BufReader::new(stdout).lines();
    let mut reply = None;
    while let Some(line) = lines.next().await {
        let line = line.map_err(|error| {
            Unavailable::Failed(format!("could not read codex's reply: {error}").into())
        })?;
        if !is_reply_to(&line, RATE_LIMITS_ID) {
            continue;
        }
        reply = Some(line);
        break;
    }

    let reply = reply
        .ok_or_else(|| Unavailable::Failed("codex app-server closed without answering".into()))?;

    // The child is short-lived by design; nothing is gained by waiting on it.
    let _ = child.kill();

    parse_windows(&reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole path against the real `codex` on this machine.
    ///
    /// `#[ignore]` on purpose and permanently: it spawns a process and depends on
    /// whoever is signed in, which is precisely the kind of I/O the rest of this
    /// crate goes out of its way to keep out of the suite. It exists because the
    /// schema behind `parse_windows` is still moving, and a fixture cannot notice
    /// that. Run it deliberately when the CLI is upgraded:
    ///
    /// ```text
    /// cargo test -p agent_usage -- --ignored --nocapture reading_a_live_codex
    /// ```
    /// A plain `#[test]` on its own executor, not a `#[gpui::test]`: gpui's test
    /// scheduler forbids parking, which is exactly what awaiting a real subprocess
    /// does. That is the right rule for the suite and the reason this one test has
    /// to step outside it.
    #[test]
    #[ignore = "spawns the real codex CLI; run by hand after a CLI upgrade"]
    fn reading_a_live_codex_app_server() {
        let Ok(binary) = which::which("codex") else {
            eprintln!("skipped: no codex on PATH");
            return;
        };
        match smol::block_on(read_windows(binary)) {
            Ok(windows) => {
                assert!(
                    !windows.is_empty(),
                    "a signed-in codex must report at least one window"
                );
                for window in &windows {
                    eprintln!(
                        "codex: {}% used, resets_at {:?}, label {:?}",
                        window.percent, window.resets_at, window.label
                    );
                    assert!(window.percent <= 100);
                }
            }
            Err(Unavailable::NotInstalled) => {
                eprintln!("skipped: no codex on PATH");
            }
            Err(other) => {
                panic!("codex is installed, so this should have produced windows: {other:?}")
            }
        }
    }

    /// Recorded from `codex-cli 0.149.0` on a live, signed-in machine.
    ///
    /// Kept whole rather than trimmed: `credits`, `planType`, `individualLimit`
    /// and `rateLimitsByLimitId` are all things this module must walk past without
    /// tripping, and a fixture that omits them would not prove that.
    const REAL_RESPONSE: &str = r#"{"id":2,"result":{
        "rateLimits":{
            "limitId":"codex","limitName":null,
            "primary":{"usedPercent":0,"windowDurationMins":43200,"resetsAt":1789913696},
            "secondary":null,
            "credits":{"hasCredits":false,"unlimited":false,"balance":null},
            "individualLimit":null,"spendControlReached":false,
            "planType":"free","rateLimitReachedType":null
        },
        "rateLimitsByLimitId":{"codex":{
            "limitId":"codex","limitName":null,
            "primary":{"usedPercent":0,"windowDurationMins":43200,"resetsAt":1789913696},
            "secondary":null
        }},
        "rateLimitResetCredits":{"availableCount":0}
    }}"#;

    #[test]
    fn the_recorded_response_yields_its_one_window() {
        let windows = parse_windows(REAL_RESPONSE).expect("the recorded response must parse");

        assert_eq!(
            windows.len(),
            1,
            "this account has one metered window; `secondary` is null and must not \
             become a phantom second row"
        );
        assert_eq!(windows[0].percent, 0);
        assert_eq!(
            windows[0].resets_at.map(|at| at.timestamp()),
            Some(1789913696),
            "unix seconds become an instant"
        );
        assert!(
            windows[0].label.is_none(),
            "`limitName` is null on this plan, so nothing stands in for the countdown"
        );
    }

    /// Codex names no kind, so the kind comes from the window's length — and the
    /// length in the recorded payload is neither of Claude's two.
    ///
    /// `windowDurationMins: 43200` is thirty days. A display that assumed the two
    /// familiar lengths would have labelled this `5h` with total confidence.
    #[test]
    fn the_window_kind_is_derived_from_its_length() {
        let windows = parse_windows(REAL_RESPONSE).unwrap();

        assert_eq!(
            windows[0].kind,
            WindowKind::Span(Duration::from_secs(43_200 * 60)),
            "the recorded account meters a thirty-day window"
        );
        assert_eq!(windows[0].kind.short_tag(), "30d");
        assert_eq!(windows[0].kind.long_name(), "30d window");
    }

    /// A window that reports no length is `Unknown` rather than a fabricated span.
    #[test]
    fn a_window_with_no_stated_length_claims_no_kind() {
        let windows = parse_windows(
            r#"{"id":2,"result":{"rateLimits":{
                "primary": {"usedPercent": 12, "resetsAt": 1789913696}
            }}}"#,
        )
        .unwrap();

        assert_eq!(windows[0].kind, WindowKind::Unknown);
        assert_eq!(windows[0].percent, 12, "the number is still read");
    }

    /// A shorter window reads in hours, so the tag is not always days.
    #[test]
    fn a_span_shorter_than_a_day_reads_in_hours() {
        let windows = parse_windows(
            r#"{"id":2,"result":{"rateLimits":{
                "primary": {"usedPercent": 5, "windowDurationMins": 300, "resetsAt": 1789913696}
            }}}"#,
        )
        .unwrap();

        assert_eq!(windows[0].kind.short_tag(), "5h");
    }

    /// A plan reporting both windows yields both.
    #[test]
    fn both_windows_are_read_when_both_are_present() {
        let two = r#"{"id":2,"result":{"rateLimits":{
            "primary":   {"usedPercent": 42, "resetsAt": 1789913696},
            "secondary": {"usedPercent": 8,  "resetsAt": 1790913696}
        }}}"#;

        let windows = parse_windows(two).unwrap();
        assert_eq!(
            windows.iter().map(|w| w.percent).collect::<Vec<_>>(),
            vec![42, 8]
        );
    }

    /// `usedPercent` is typed as an integer, but a float must not break the parse.
    #[test]
    fn a_fractional_percentage_is_rounded_rather_than_refused() {
        let fractional = r#"{"result":{"rateLimits":{"primary":{"usedPercent":7.5}}}}"#;
        let windows = parse_windows(fractional).expect("a float is still a number");
        assert_eq!(windows[0].percent, 8);
    }

    /// `limitName`, when the server sends one, stands in for a missing countdown —
    /// the same role the model name plays on the Claude side.
    #[test]
    fn a_named_limit_with_no_reset_shows_its_name() {
        let named = r#"{"result":{"rateLimits":{
            "limitName":"Codex Cloud",
            "primary":{"usedPercent":12}
        }}}"#;

        let windows = parse_windows(named).unwrap();
        assert_eq!(windows[0].resets_at, None);
        assert_eq!(windows[0].label, Some("Codex Cloud".into()));
    }

    /// The reply is found by its id, not by arrival order.
    ///
    /// Two requests are in flight and the server interleaves notifications between
    /// their replies — a real one was observed doing exactly that — so matching
    /// loosely would parse the handshake's reply and report the schema unreadable.
    #[test]
    fn only_the_reply_with_the_right_id_is_taken() {
        let initialize_reply = r#"{"id":1,"result":{"userAgent":"zode/0.149.0","codexHome":"/x"}}"#;
        let notification =
            r#"{"method":"remoteControl/status/changed","params":{"status":"disabled"}}"#;

        assert!(
            !is_reply_to(initialize_reply, RATE_LIMITS_ID),
            "the handshake's own reply must not be mistaken for the answer"
        );
        assert!(
            !is_reply_to(notification, RATE_LIMITS_ID),
            "a notification carries no id at all"
        );
        assert!(is_reply_to(REAL_RESPONSE, RATE_LIMITS_ID));
    }

    /// Sending anything before `initialize` earns this, and it is a refusal rather
    /// than an unreadable payload — the distinction the usage panel depends on.
    #[test]
    fn the_not_initialized_refusal_is_reported_as_one() {
        let refused = r#"{"error":{"code":-32600,"message":"Not initialized"},"id":1}"#;

        match parse_windows(refused) {
            Err(Unavailable::Failed(reason)) => assert!(
                reason.contains("Not initialized"),
                "the server's own words are the most useful thing to show: {reason}"
            ),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// A response that arrives but cannot be read is its own outcome.
    ///
    /// This is the variant that exists because the field names above came from
    /// documentation rather than a live server: if they are wrong, this fires and
    /// the log says which names were expected.
    #[test]
    fn a_response_with_unexpected_field_names_is_unreadable_not_empty() {
        let renamed = r#"{"jsonrpc":"2.0","id":1,"result":{"rate_limits":{
            "primary": {"used_percent": 42.0, "resets_at": 1787000000}
        }}}"#;

        assert_eq!(
            parse_windows(renamed),
            Err(Unavailable::Unreadable(
                "codex answered, but this build could not read its rate limits".into()
            )),
            "a schema this build does not recognise must be reported, not silently blank"
        );
    }

    /// An explicit JSON-RPC error is a refusal, not an unreadable payload — a
    /// signed-out session lands here, and it needs a different sentence.
    #[test]
    fn a_json_rpc_error_is_reported_as_a_refusal() {
        let refused =
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"not signed in"}}"#;

        match parse_windows(refused) {
            Err(Unavailable::Failed(reason)) => {
                assert!(
                    reason.contains("not signed in"),
                    "the server's own words are the most useful thing to show: {reason}"
                );
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// The diagnostic log carries key names and no values.
    ///
    /// That is the whole reason it exists — to say which names came back when the
    /// expected ones did not — and it is also the reason it must not carry values:
    /// this payload comes from a process holding an authenticated session.
    #[test]
    fn the_diagnostic_reports_key_names_and_never_values() {
        let renamed = r#"{"jsonrpc":"2.0","id":1,"result":{"rate_limits":{
            "primary": {"used_percent": 42.0, "secret_ish": "sk-do-not-log-me"}
        }}}"#;

        let paths = key_paths(renamed);

        assert!(
            paths.iter().any(|path| path == "result.rate_limits"),
            "the actual names are what makes the mismatch fixable: {paths:?}"
        );
        assert!(
            paths
                .iter()
                .any(|path| path == "result.rate_limits.primary.used_percent"),
            "including the leaf names: {paths:?}"
        );
        assert!(
            !paths.iter().any(|path| path.contains("sk-do-not-log-me")),
            "a value must never appear, whatever it is: {paths:?}"
        );
        assert!(
            !paths.iter().any(|path| path.contains("42")),
            "not even harmless-looking ones: {paths:?}"
        );
    }

    #[test]
    fn a_line_that_is_not_json_is_unreadable() {
        assert!(matches!(
            parse_windows("codex: command not found"),
            Err(Unavailable::Unreadable(_))
        ));
    }

    /// A window with no percentage is dropped; the one beside it survives.
    #[test]
    fn a_window_without_a_percentage_is_skipped() {
        let partial = r#"{"result":{"rateLimits":{
            "primary":   {"resetsAt": 1787000000},
            "secondary": {"usedPercent": 7.0, "resetsAt": 1787500000}
        }}}"#;

        let windows = parse_windows(partial).expect("one usable window is enough");
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].percent, 7);
    }

    /// A percentage outside 0..=100 is clamped rather than trusted.
    #[test]
    fn a_percentage_is_clamped() {
        let odd = r#"{"result":{"rateLimits":{"primary":{"usedPercent":250.0,"resetsAt":0}}}}"#;
        let windows = parse_windows(odd).unwrap();
        assert_eq!(windows[0].percent, 100);
        assert_eq!(
            windows[0].resets_at, None,
            "a zero timestamp is absent data, not 1970"
        );
    }
}
