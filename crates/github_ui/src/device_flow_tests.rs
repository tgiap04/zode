use std::time::Duration;

use github::{RateLimit, RateLimits};
use gpui::TestAppContext;
use release_channel::ReleaseChannel;
use serde_json::json;

use crate::device_flow::{DEVICE_CODE_URL, DeviceFlowError, is_github_page, request_device_code};
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
