use std::collections::{HashMap, VecDeque};
use std::time::{Duration, SystemTime};

use http_client::http::HeaderMap;

use crate::error::{GithubError, org_restriction, shorten_message, unexpected};
use crate::model::{OAuthScopes, RateLimit};
use crate::token::TokenIdentity;

pub(crate) const API_BASE: &str = "https://api.github.com";
pub(crate) const API_VERSION: &str = "2026-03-10";
pub(crate) const PER_PAGE: u32 = 50;

pub(crate) fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
}

fn number(headers: &HeaderMap, name: &str) -> Option<u64> {
    header(headers, name)?.parse().ok()
}

pub(crate) fn rate_limit(headers: &HeaderMap) -> Option<RateLimit> {
    let limit = number(headers, "x-ratelimit-limit")?;
    let remaining = number(headers, "x-ratelimit-remaining")?;
    Some(RateLimit {
        limit,
        remaining,
        used: number(headers, "x-ratelimit-used").unwrap_or(limit.saturating_sub(remaining)),
        reset_at: number(headers, "x-ratelimit-reset")?,
    })
}

/// `core`, `graphql`, `search`… as named by `X-RateLimit-Resource`.
pub(crate) fn rate_limit_resource(headers: &HeaderMap) -> Option<&str> {
    header(headers, "x-ratelimit-resource")
}

/// An absent header means "unknown", not "no scopes".
pub(crate) fn oauth_scopes(headers: &HeaderMap) -> OAuthScopes {
    match header(headers, "x-oauth-scopes") {
        None => OAuthScopes::Unknown,
        Some(scopes) => OAuthScopes::Granted(scope_list(scopes)),
    }
}

fn scope_list(scopes: &str) -> Vec<String> {
    scopes
        .split(',')
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
        .map(str::to_string)
        .collect()
}

/// The page to open to authorize the token for an SSO organization, from
/// `X-GitHub-SSO: required; url=…`. Only an https github.com address is
/// passed on, since the tab opens it.
pub(crate) fn sso_authorize_url(headers: &HeaderMap) -> Option<String> {
    let mut parts = header(headers, "x-github-sso")?.split(';').map(str::trim);
    if parts.next()? != "required" {
        return None;
    }
    let url = parts.find_map(|part| part.strip_prefix("url="))?;
    let url = url::Url::parse(url).ok()?;
    (url.scheme() == "https" && url.host_str() == Some("github.com")).then(|| url.to_string())
}

pub(crate) fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    number(headers, "retry-after").map(Duration::from_secs)
}

/// When the rate limit resets, from `x-ratelimit-reset`.
pub(crate) fn reset_at(headers: &HeaderMap) -> Option<SystemTime> {
    number(headers, "x-ratelimit-reset")
        .and_then(|seconds| SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
}

/// The query parameters that move through a REST list, best first: some
/// lists page by cursor (`after`/`before`), the rest by number.
const CURSOR_PARAMETERS: [&str; 3] = ["after", "before", "page"];

/// The one paging parameter to send, from the pairs of a `next` link or of a
/// cursor handed back.
fn paging_parameter<'a>(
    pairs: impl Iterator<Item = (&'a str, &'a str)> + Clone,
) -> Option<(&'static str, &'a str)> {
    CURSOR_PARAMETERS.into_iter().find_map(|wanted| {
        pairs.clone().find_map(|(name, value)| {
            let valid = match wanted {
                "page" => value.parse::<u32>().is_ok(),
                _ => !value.is_empty(),
            };
            (name == wanted && valid).then_some((wanted, value))
        })
    })
}

/// The `(target, parameters)` of each entry of a `Link` header. A target may
/// hold a comma (`?labels=a,b`), so entries are told apart by the `<` that
/// opens each one, not by splitting on commas.
fn link_entries(value: &str) -> impl Iterator<Item = (&str, &str)> {
    value.split('<').skip(1).filter_map(|entry| {
        let (target, parameters) = entry.split_once('>')?;
        Some((target, parameters.trim().trim_end_matches(',')))
    })
}

/// The cursor of the next page from the `Link` header: the link's paging
/// parameter alone, as `name=value`. Everything else in the link is dropped,
/// so the next request is built here rather than sent where GitHub named.
pub(crate) fn next_page(headers: &HeaderMap) -> Option<String> {
    link_entries(header(headers, "link")?).find_map(|(target, parameters)| {
        if !parameters
            .split(';')
            .any(|parameter| parameter.trim() == "rel=\"next\"")
        {
            return None;
        }
        let url = url::Url::parse(target).ok()?;
        let pairs = url.query_pairs().collect::<Vec<_>>();
        let (name, value) =
            paging_parameter(pairs.iter().map(|(name, value)| (&**name, &**value)))?;
        Some(
            url::form_urlencoded::Serializer::new(String::new())
                .append_pair(name, value)
                .finish(),
        )
    })
}

/// The paging parameter of a cursor from [`next_page`]. A cursor naming any
/// other parameter is refused, so a cursor cannot change what is asked for.
pub(crate) fn cursor_parameter(cursor: &str) -> Option<(&'static str, String)> {
    let pairs = url::form_urlencoded::parse(cursor.as_bytes()).collect::<Vec<_>>();
    if pairs.is_empty()
        || pairs
            .iter()
            .any(|(name, _)| !CURSOR_PARAMETERS.contains(&&**name))
    {
        return None;
    }
    paging_parameter(pairs.iter().map(|(name, value)| (&**name, &**value)))
        .map(|(name, value)| (name, value.to_string()))
}

/// The error for a REST answer that is not a success.
pub(crate) fn error_for_response(status: u16, headers: &HeaderMap, body: &str) -> GithubError {
    let message = service_message(body);
    if matches!(status, 403 | 404)
        && let Some(authorize_url) = sso_authorize_url(headers)
    {
        return GithubError::SsoRequired { authorize_url };
    }
    match status {
        401 => GithubError::TokenRevoked,
        403 | 429 => {
            if let Some(error) = rate_limited(status, headers, &message) {
                error
            } else if let Some(error) = org_restriction(&message) {
                error
            } else if let Some(scope) = missing_scope(headers) {
                GithubError::MissingScope { scope }
            } else {
                GithubError::Forbidden {
                    message: shorten_message(&message),
                }
            }
        }
        404 | 410 => missing_scope(headers)
            .map(|scope| GithubError::MissingScope { scope })
            .unwrap_or(GithubError::NotFound),
        422 => GithubError::Rejected {
            message: shorten_message(&message),
        },
        _ => unexpected(format!("HTTP {status}: {message}")),
    }
}

fn rate_limited(status: u16, headers: &HeaderMap, message: &str) -> Option<GithubError> {
    let wait = retry_after(headers);
    if header(headers, "x-ratelimit-remaining") == Some("0") {
        return Some(GithubError::RateLimited {
            reset_at: reset_at(headers),
            retry_after: wait,
        });
    }
    if let Some(wait) = wait {
        return Some(GithubError::RateLimited {
            reset_at: SystemTime::now().checked_add(wait),
            retry_after: Some(wait),
        });
    }
    (status == 429 || message.to_lowercase().contains("rate limit")).then_some(
        GithubError::RateLimited {
            reset_at: None,
            retry_after: None,
        },
    )
}

/// The scope to ask for, when GitHub said which ones the call accepts and the
/// token holds none of them. Unknown scopes never count as missing.
fn missing_scope(headers: &HeaderMap) -> Option<String> {
    let granted = oauth_scopes(headers);
    let accepted = scope_list(header(headers, "x-accepted-oauth-scopes")?);
    let holds_one = accepted
        .iter()
        .any(|scope| granted.includes(scope) == Some(true));
    match granted {
        OAuthScopes::Unknown => None,
        OAuthScopes::Granted(_) if holds_one => None,
        OAuthScopes::Granted(_) => accepted.into_iter().next(),
    }
}

/// The human part of an error body (`message`), or the body itself.
fn service_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("message")?.as_str().map(str::to_string))
        .unwrap_or_else(|| body.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CachedResponse {
    pub etag: String,
    pub body: String,
    pub next_page: Option<String>,
}

/// Bodies of earlier `GET`s with their ETag, so an unchanged list costs a
/// free `304`. Keyed by token as well as URL: another account may see other
/// things at the same address. Oldest entries go first once either bound is
/// reached.
pub(crate) struct EtagCache {
    entries: HashMap<(TokenIdentity, String), CachedResponse>,
    order: VecDeque<(TokenIdentity, String)>,
    max_entries: usize,
    max_bytes: usize,
    bytes: usize,
}

pub(crate) const ETAG_CACHE_MAX_ENTRIES: usize = 200;
pub(crate) const ETAG_CACHE_MAX_BYTES: usize = 8 * 1024 * 1024;

impl EtagCache {
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            max_entries,
            max_bytes,
            bytes: 0,
        }
    }

    pub fn get(&self, identity: &TokenIdentity, url: &str) -> Option<&CachedResponse> {
        self.entries.get(&(identity.clone(), url.to_string()))
    }

    pub fn insert(&mut self, identity: TokenIdentity, url: String, response: CachedResponse) {
        let key = (identity, url);
        self.remove(&key);
        let size = size_of(&response);
        if size > self.max_bytes {
            return;
        }
        self.bytes += size;
        self.order.push_back(key.clone());
        self.entries.insert(key, response);
        while self.entries.len() > self.max_entries || self.bytes > self.max_bytes {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(evicted) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(size_of(&evicted));
            }
        }
    }

    fn remove(&mut self, key: &(TokenIdentity, String)) {
        if let Some(removed) = self.entries.remove(key) {
            self.bytes = self.bytes.saturating_sub(size_of(&removed));
            self.order.retain(|existing| existing != key);
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

fn size_of(response: &CachedResponse) -> usize {
    response.etag.len() + response.body.len()
}
