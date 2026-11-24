use std::time::Duration;

use futures::FutureExt as _;
use github::{RateLimit, RateLimits};
use gpui::{Task, TestAppContext};
use release_channel::ReleaseChannel;
use serde_json::json;

use crate::device_flow::{
    ACCESS_TOKEN_URL, DEVICE_CODE_URL, DeviceCode, DeviceFlowError, is_github_page, poll_for_token,
    request_device_code,
};
use crate::github_view_render::rate_limit_summary;
use crate::sign_in::TokenStorage;
use crate::test_support::ScriptedHttp;

#[gpui::test]
async fn a_verification_page_off_github_is_refused(cx: &mut TestAppContext) {
    let http = ScriptedHttp::default();
    http.answer(
        DEVICE_CODE_URL,
        200,
        json!({
            "device_code": "d",
            "user_code": "AAAA-BBBB",
            "verification_uri": "https://github.com.evil.example/login/device",
            "expires_in": 900,
            "interval": 5
        })
        .to_string(),
    );
    let result = request_device_code(&http.client(), "Iv1.x", &cx.background_executor).await;
    assert!(
        matches!(result, Err(DeviceFlowError::Rejected(_))),
        "{result:?}"
    );
}

#[gpui::test]
async fn an_app_without_device_flow_says_so(cx: &mut TestAppContext) {
    let http = ScriptedHttp::default();
    http.answer(
        DEVICE_CODE_URL,
        400,
        json!({ "error": "device_flow_disabled" }).to_string(),
    );
    let result = request_device_code(&http.client(), "Iv1.x", &cx.background_executor).await;
    assert_eq!(result, Err(DeviceFlowError::Disabled));
}

#[gpui::test]
async fn the_code_and_interval_are_read_and_a_short_interval_is_raised(cx: &mut TestAppContext) {
    let http = ScriptedHttp::default();
    http.answer(
        DEVICE_CODE_URL,
        200,
        json!({
            "device_code": "d",
            "user_code": "AAAA-BBBB",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 1
        })
        .to_string(),
    );
    let code = request_device_code(&http.client(), "Iv1.x", &cx.background_executor)
        .await
        .expect("a code");
    assert_eq!(code.user_code, "AAAA-BBBB");
    assert_eq!(code.interval, Duration::from_secs(5));
    assert!(
        !format!("{code:?}").contains("\"d\""),
        "the device code never shows in Debug"
    );
}

#[test]
fn only_https_github_com_pages_are_opened() {
    assert!(is_github_page("https://github.com/login/device"));
    assert!(!is_github_page("http://github.com/login/device"));
    assert!(!is_github_page("https://github.com.evil.example/"));
    assert!(!is_github_page("https://evil.example/?github.com"));
    assert!(!is_github_page("https://github.com:8443/login/device"));
}

#[test]
fn a_verification_page_with_userinfo_is_not_opened() {
    for page in [
        "https://user@github.com/login/device",
        "https://user:secret@github.com/login/device",
        "https://:secret@github.com/login/device",
        "https://github.com@evil.example/login/device",
    ] {
        assert!(!is_github_page(page), "{page}");
    }
}

const CLIENT_ID: &str = "Iv1.x";

async fn requested_code(http: &ScriptedHttp, expires_in: u64, cx: &TestAppContext) -> DeviceCode {
    http.answer(
        DEVICE_CODE_URL,
        200,
        json!({
            "device_code": "d",
            "user_code": "AAAA-BBBB",
            "verification_uri": "https://github.com/login/device",
            "expires_in": expires_in,
            "interval": 5
        })
        .to_string(),
    );
    request_device_code(&http.client(), CLIENT_ID, &cx.background_executor)
        .await
        .expect("a code")
}

fn start_polling(
    http: &ScriptedHttp,
    code: DeviceCode,
    cx: &TestAppContext,
) -> Task<Result<String, DeviceFlowError>> {
    let client = http.client();
    let executor = cx.background_executor.clone();
    cx.background_executor
        .spawn(async move { poll_for_token(&client, CLIENT_ID, &code, &executor).await })
}

fn advance(seconds: u64, cx: &TestAppContext) {
    cx.executor().advance_clock(Duration::from_secs(seconds));
    cx.run_until_parked();
}

fn polls(http: &ScriptedHttp) -> usize {
    http.sent()
        .iter()
        .filter(|request| request.url == ACCESS_TOKEN_URL)
        .count()
}

#[gpui::test]
async fn answers_that_are_not_ok_end_the_wait_after_three(cx: &mut TestAppContext) {
    for (status, body) in [
        (401, "<html>Unauthorized</html>"),
        (403, "{\"message\":\"Forbidden\"}"),
        (500, "<html>oops</html>"),
        (200, "<html>not json</html>"),
    ] {
        let http = ScriptedHttp::default();
        let code = requested_code(&http, 900, cx).await;
        http.answer(ACCESS_TOKEN_URL, status, body);
        let task = start_polling(&http, code, cx);
        for _ in 0..12 {
            advance(5, cx);
        }
        let result = task.now_or_never();
        assert!(
            matches!(result, Some(Err(_))),
            "HTTP {status}: gave up, got {result:?}"
        );
        assert_eq!(polls(&http), 3, "HTTP {status}");
    }
}

#[gpui::test]
async fn a_refused_answer_is_rejected_and_a_busy_one_is_unreachable(cx: &mut TestAppContext) {
    for (status, expect_rejected) in [(403, true), (502, false)] {
        let http = ScriptedHttp::default();
        let code = requested_code(&http, 900, cx).await;
        http.answer(ACCESS_TOKEN_URL, status, "<html></html>");
        let task = start_polling(&http, code, cx);
        for _ in 0..24 {
            advance(5, cx);
        }
        match task.now_or_never() {
            Some(Err(DeviceFlowError::Rejected(detail))) if expect_rejected => {
                assert!(detail.contains(&status.to_string()), "{detail}");
            }
            Some(Err(DeviceFlowError::Unreachable(detail))) if !expect_rejected => {
                assert!(detail.contains(&status.to_string()), "{detail}");
            }
            other => panic!("HTTP {status}: {other:?}"),
        }
    }
}

#[gpui::test]
async fn too_many_requests_and_server_errors_widen_the_interval(cx: &mut TestAppContext) {
    for status in [429, 503] {
        let http = ScriptedHttp::default();
        let code = requested_code(&http, 900, cx).await;
        http.answer(ACCESS_TOKEN_URL, status, "<html></html>");
        http.answer(
            ACCESS_TOKEN_URL,
            200,
            json!({ "error": "authorization_pending" }).to_string(),
        );
        let _task = start_polling(&http, code, cx);
        advance(5, cx);
        assert_eq!(polls(&http), 1, "HTTP {status}: first poll");
        advance(5, cx);
        assert_eq!(polls(&http), 1, "HTTP {status}: the interval grew");
        advance(5, cx);
        assert_eq!(polls(&http), 2, "HTTP {status}: second poll");
    }
}

#[gpui::test]
async fn a_good_answer_between_bad_ones_resets_the_count(cx: &mut TestAppContext) {
    let http = ScriptedHttp::default();
    let code = requested_code(&http, 900, cx).await;
    let pending = json!({ "error": "authorization_pending" }).to_string();
    let token = json!({ "access_token": "gho_x" }).to_string();
    for (status, body) in [
        (500, "<html></html>"),
        (500, "<html></html>"),
        (200, pending.as_str()),
        (500, "<html></html>"),
        (500, "<html></html>"),
        (200, token.as_str()),
    ] {
        http.answer(ACCESS_TOKEN_URL, status, body);
    }
    let task = start_polling(&http, code, cx);
    for _ in 0..40 {
        advance(5, cx);
    }
    assert!(
        matches!(task.now_or_never(), Some(Ok(token)) if token == "gho_x"),
        "four bad answers in all, never three in a row"
    );
}

#[gpui::test]
async fn the_code_lifetime_is_capped(cx: &mut TestAppContext) {
    let http = ScriptedHttp::default();
    let code = requested_code(&http, 7 * 24 * 60 * 60, cx).await;
    assert_eq!(code.expires_in, Duration::from_secs(30 * 60));
}

#[gpui::test]
async fn time_spent_waiting_for_an_answer_counts_toward_the_deadline(cx: &mut TestAppContext) {
    let http = ScriptedHttp::default();
    let code = requested_code(&http, 900, cx).await;
    http.hang(ACCESS_TOKEN_URL);
    let task = start_polling(&http, code, cx);
    // Each round is a five second wait plus a thirty second timeout, so only
    // about twenty-six rounds fit in the code's lifetime; counting the waits
    // alone would allow a hundred and eighty.
    for _ in 0..200 {
        advance(5, cx);
    }
    assert!(
        matches!(task.now_or_never(), Some(Err(DeviceFlowError::Expired))),
        "the code expired while requests were timing out"
    );
    assert!(polls(&http) <= 900 / 35 + 1, "{} polls", polls(&http));
}

#[test]
fn only_dev_builds_keep_the_token_in_memory() {
    assert_eq!(
        TokenStorage::for_build(Some(ReleaseChannel::Dev), false),
        TokenStorage::MemoryOnly
    );
    assert_eq!(
        TokenStorage::for_build(Some(ReleaseChannel::Dev), true),
        TokenStorage::Keychain
    );
    for channel in [
        ReleaseChannel::Nightly,
        ReleaseChannel::Preview,
        ReleaseChannel::Stable,
    ] {
        assert_eq!(
            TokenStorage::for_build(Some(channel), false),
            TokenStorage::Keychain
        );
    }
}

#[test]
fn the_rate_limit_shows_both_buckets_and_the_soonest_reset() {
    assert_eq!(rate_limit_summary(&RateLimits::default(), 0), None);
    let rate_limits = RateLimits {
        rest: Some(RateLimit {
            limit: 5000,
            remaining: 4980,
            used: 20,
            reset_at: 1_000 + 12 * 60,
        }),
        graphql: Some(RateLimit {
            limit: 5000,
            remaining: 4990,
            used: 10,
            reset_at: 1_000 + 30 * 60,
        }),
    };
    assert_eq!(
        rate_limit_summary(&rate_limits, 1_000).as_deref(),
        Some("REST 4980/5000 · GraphQL 4990/5000, resets in 12 min")
    );
}
