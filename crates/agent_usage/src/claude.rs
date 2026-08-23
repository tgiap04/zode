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
//! never written anywhere.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use futures::AsyncReadExt as _;
use gpui::{BackgroundExecutor, SharedString};
use http_client::{AsyncBody, HttpClient, Request};
use serde::Deserialize;

use crate::{UsageWindow, WindowKind};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";

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

/// Why there is nothing to show, when there is nothing to show.
///
/// Carried rather than collapsed to `None` so the indicator's tooltip can say
/// which of these it is. "No quota shown" has several quite different causes and
/// only one of them is worth acting on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unavailable {
    /// An `ANTHROPIC_*` override is in force.
    RuntimeOverride,
    /// Claude Code has never signed in on this machine.
    NoCredentials,
    /// Signed in, but on a plan with no quota windows to report.
    UnsupportedPlan,
    /// The request or the parse failed. Carries a short reason, never a token.
    Request(SharedString),
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
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
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
                if !output.status.success() {
                    return None;
                }
                serde_json::from_slice::<Credentials>(&output.stdout).ok()
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
            let contents = smol::fs::read(path).await.ok()?;
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
    let oauth = credentials.oauth.ok_or(Unavailable::NoCredentials)?;
    if !plan_is_supported(&oauth) {
        return Err(Unavailable::UnsupportedPlan);
    }
    let token = oauth
        .access_token
        .filter(|token| !token.trim().is_empty())
        .ok_or(Unavailable::NoCredentials)?;

    let request = Request::builder()
        .uri(USAGE_URL)
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .header("anthropic-beta", OAUTH_BETA)
        .body(AsyncBody::default())
        .map_err(|_| Unavailable::Request("the usage request could not be built".into()))?;

    let mut response = http_client
        .send(request)
        .await
        .map_err(|_| Unavailable::Request("the usage endpoint could not be reached".into()))?;

    if !response.status().is_success() {
        // The status, never the body: an error body from an authenticated
        // endpoint is exactly the sort of thing that echoes a credential back.
        return Err(Unavailable::Request(
            format!("the usage endpoint answered {}", response.status().as_u16()).into(),
        ));
    }

    let mut body = String::new();
    response
        .body_mut()
        .read_to_string(&mut body)
        .await
        .map_err(|_| Unavailable::Request("the usage response could not be read".into()))?;

    parse_windows(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            access_token: Some("unused".into()),
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
}
