use std::time::{Duration, SystemTime};

use http_client::http::{HeaderMap, HeaderName, HeaderValue};

use crate::error::GithubError;
use crate::model::{OAuthScopes, RateLimit};
use crate::rest::{
    CachedResponse, EtagCache, error_for_response, next_page, oauth_scopes, rate_limit,
    retry_after, sso_authorize_url,
};
use crate::token::{GithubToken, TokenSource};

fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_str(value).expect("a valid header value"),
        );
    }
    headers
}

#[test]
fn rate_limit_headers_are_read() {
    let headers = headers(&[
        ("x-ratelimit-limit", "5000"),
        ("x-ratelimit-remaining", "4990"),
        ("x-ratelimit-used", "10"),
        ("x-ratelimit-reset", "1764000000"),
    ]);
    assert_eq!(
        rate_limit(&headers),
        Some(RateLimit {
            limit: 5000,
            remaining: 4990,
            used: 10,
            reset_at: 1_764_000_000,
        })
    );
    assert_eq!(rate_limit(&HeaderMap::new()), None);
}

#[test]
fn an_absent_scopes_header_means_unknown_not_none() {
    assert_eq!(oauth_scopes(&HeaderMap::new()), OAuthScopes::Unknown);
    assert_eq!(
        oauth_scopes(&headers(&[("x-oauth-scopes", "")])),
        OAuthScopes::Granted(Vec::new())
    );
    let granted = oauth_scopes(&headers(&[("x-oauth-scopes", "repo, read:org,gist")]));
    assert_eq!(
        granted,
        OAuthScopes::Granted(vec![
            "repo".to_string(),
            "read:org".to_string(),
            "gist".to_string()
        ])
    );
    assert_eq!(granted.includes("public_repo"), Some(true));
    assert_eq!(granted.includes("project"), Some(false));
    assert_eq!(OAuthScopes::Unknown.includes("repo"), None);
}

#[test]
fn the_sso_header_gives_an_authorize_address_on_github_only() {
    let required = headers(&[(
        "x-github-sso",
        "required; url=https://github.com/orgs/acme/sso?authorization_request=abc",
    )]);
    assert_eq!(
        sso_authorize_url(&required).as_deref(),
        Some("https://github.com/orgs/acme/sso?authorization_request=abc")
    );
    let elsewhere = headers(&[("x-github-sso", "required; url=https://evil.example/sso")]);
    assert_eq!(sso_authorize_url(&elsewhere), None);
    let partial = headers(&[("x-github-sso", "partial-results; organizations=21955855")]);
    assert_eq!(sso_authorize_url(&partial), None);
}

#[test]
fn retry_after_is_read_in_seconds() {
    assert_eq!(
        retry_after(&headers(&[("retry-after", "30")])),
        Some(Duration::from_secs(30))
    );
    assert_eq!(retry_after(&headers(&[("retry-after", "soon")])), None);
    assert_eq!(retry_after(&HeaderMap::new()), None);
}

#[test]
fn the_next_page_number_comes_from_the_link_header() {
    let link = headers(&[(
        "link",
        "<https://api.github.com/repositories/1/issues?per_page=50&page=2>; rel=\"next\", \
         <https://api.github.com/repositories/1/issues?per_page=50&page=9>; rel=\"last\"",
    )]);
    assert_eq!(next_page(&link).as_deref(), Some("page=2"));
    let last_page = headers(&[(
        "link",
        "<https://api.github.com/repositories/1/issues?page=1>; rel=\"prev\"",
    )]);
    assert_eq!(next_page(&last_page), None);
    assert_eq!(next_page(&HeaderMap::new()), None);
}

#[test]
fn the_next_page_cursor_keeps_only_a_paging_parameter() {
    let cursor = headers(&[(
        "link",
        "<https://api.github.com/search/issues?q=repo%3Aevil%2Fother&per_page=50&page=2&after=Y3Vy%3D>; rel=\"next\"",
    )]);
    assert_eq!(next_page(&cursor).as_deref(), Some("after=Y3Vy%3D"));
    let other_only = headers(&[(
        "link",
        "<https://api.github.com/search/issues?q=repo%3Aevil%2Fother>; rel=\"next\"",
    )]);
    assert_eq!(next_page(&other_only), None);
}

#[test]
fn statuses_map_to_readable_errors() {
    let none = HeaderMap::new();
    assert_eq!(
        error_for_response(401, &none, r#"{"message":"Bad credentials"}"#),
        GithubError::TokenRevoked
    );
    assert_eq!(
        error_for_response(404, &none, r#"{"message":"Not Found"}"#),
        GithubError::NotFound
    );
    assert_eq!(
        error_for_response(410, &none, r#"{"message":"Gone"}"#),
        GithubError::NotFound
    );
    assert_eq!(
        error_for_response(422, &none, r#"{"message":"Validation Failed"}"#),
        GithubError::Rejected {
            message: "Validation Failed".to_string()
        }
    );
    assert_eq!(
        error_for_response(403, &none, r#"{"message":"Resource not accessible"}"#),
        GithubError::Forbidden {
            message: "Resource not accessible".to_string()
        }
    );
    assert!(matches!(
        error_for_response(502, &none, "<html>bad gateway</html>"),
        GithubError::UnexpectedResponse { .. }
    ));
}

#[test]
fn an_exhausted_rate_limit_carries_its_reset_time() {
    let exhausted = headers(&[
        ("x-ratelimit-remaining", "0"),
        ("x-ratelimit-reset", "1764000000"),
    ]);
    assert_eq!(
        error_for_response(403, &exhausted, r#"{"message":"API rate limit exceeded"}"#),
        GithubError::RateLimited {
            reset_at: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_764_000_000)),
            retry_after: None,
        }
    );
    let secondary = headers(&[("retry-after", "60")]);
    assert!(matches!(
        error_for_response(429, &secondary, r#"{"message":"secondary rate limit"}"#),
        GithubError::RateLimited {
            reset_at: Some(_),
            retry_after: Some(_)
        }
    ));
    assert!(matches!(
        error_for_response(
            403,
            &HeaderMap::new(),
            r#"{"message":"You have exceeded a secondary rate limit."}"#
        ),
        GithubError::RateLimited { .. }
    ));
}

#[test]
fn an_org_restriction_names_the_organization() {
    let body = r#"{"message":"Although you appear to have the correct authorization credentials, the `acme` organization has enabled OAuth App access restrictions, meaning that data access to third-parties is limited."}"#;
    assert_eq!(
        error_for_response(403, &HeaderMap::new(), body),
        GithubError::OrgRestricted {
            org: Some("acme".to_string())
        }
    );
}

#[test]
fn sso_comes_before_any_other_refusal() {
    let sso = headers(&[(
        "x-github-sso",
        "required; url=https://github.com/orgs/acme/sso?authorization_request=abc",
    )]);
    assert_eq!(
        error_for_response(403, &sso, r#"{"message":"Resource protected by SAML"}"#),
        GithubError::SsoRequired {
            authorize_url: "https://github.com/orgs/acme/sso?authorization_request=abc".to_string()
        }
    );
}

#[test]
fn a_missing_scope_is_named_only_when_the_scopes_are_known() {
    let known = headers(&[
        ("x-oauth-scopes", "repo, read:org"),
        (
            "x-accepted-oauth-scopes",
            "admin:org, write:org, read:org, project",
        ),
    ]);
    // read:org is granted, so this is not a scope problem.
    assert!(matches!(
        error_for_response(403, &known, r#"{"message":"Must have admin rights"}"#),
        GithubError::Forbidden { .. }
    ));
    let lacking = headers(&[
        ("x-oauth-scopes", "repo"),
        ("x-accepted-oauth-scopes", "project, read:project"),
    ]);
    assert_eq!(
        error_for_response(404, &lacking, r#"{"message":"Not Found"}"#),
        GithubError::MissingScope {
            scope: "project".to_string()
        }
    );
    let unknown = headers(&[("x-accepted-oauth-scopes", "project")]);
    assert!(matches!(
        error_for_response(403, &unknown, r#"{"message":"Forbidden"}"#),
        GithubError::Forbidden { .. }
    ));
}

#[test]
fn a_long_service_message_is_cut_short() {
    let long = format!(r#"{{"message":"{}"}}"#, "x".repeat(2000));
    let GithubError::Forbidden { message } = error_for_response(403, &HeaderMap::new(), &long)
    else {
        panic!("expected a refusal");
    };
    assert!(message.chars().count() <= 301, "{}", message.len());
}

fn cached(etag: &str, body: &str) -> CachedResponse {
    CachedResponse {
        etag: etag.to_string(),
        body: body.to_string(),
        next_page: None,
    }
}

#[test]
fn the_etag_cache_is_keyed_by_token_and_url() {
    let first = GithubToken::new("first-token", TokenSource::Keychain);
    let second = GithubToken::new("second-token", TokenSource::GhCli);
    let mut cache = EtagCache::new(10, 1024);
    cache.insert(
        first.identity().clone(),
        "https://a".to_string(),
        cached("\"1\"", "body"),
    );
    assert_eq!(
        cache.get(first.identity(), "https://a"),
        Some(&cached("\"1\"", "body"))
    );
    assert_eq!(cache.get(second.identity(), "https://a"), None);
    assert_eq!(cache.get(first.identity(), "https://b"), None);
}

#[test]
fn the_etag_cache_drops_its_oldest_entries_past_either_bound() {
    let token = GithubToken::new("token", TokenSource::Keychain);
    let identity = token.identity().clone();
    let mut cache = EtagCache::new(2, 1024);
    for url in ["https://1", "https://2", "https://3"] {
        cache.insert(identity.clone(), url.to_string(), cached("e", "body"));
    }
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.get(&identity, "https://1"), None);
    assert!(cache.get(&identity, "https://3").is_some());

    let mut small = EtagCache::new(10, 10);
    small.insert(
        identity.clone(),
        "https://a".to_string(),
        cached("e", "123456"),
    );
    small.insert(
        identity.clone(),
        "https://b".to_string(),
        cached("e", "123456"),
    );
    assert_eq!(
        small.len(),
        1,
        "two six-byte bodies do not fit in ten bytes"
    );
    assert!(small.get(&identity, "https://b").is_some());

    small.insert(
        identity.clone(),
        "https://huge".to_string(),
        cached("e", &"x".repeat(50)),
    );
    assert_eq!(
        small.get(&identity, "https://huge"),
        None,
        "a body over the whole budget is not kept"
    );
}

#[test]
fn replacing_an_entry_keeps_one_copy() {
    let token = GithubToken::new("token", TokenSource::Keychain);
    let identity = token.identity().clone();
    let mut cache = EtagCache::new(10, 100);
    cache.insert(
        identity.clone(),
        "https://a".to_string(),
        cached("1", "old"),
    );
    cache.insert(
        identity.clone(),
        "https://a".to_string(),
        cached("2", "new"),
    );
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.get(&identity, "https://a"), Some(&cached("2", "new")));
}
