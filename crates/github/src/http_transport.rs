//! Sending one request to api.github.com and reading the answer: headers,
//! timeout, size cap, no redirects, ETag reuse, rate limits, and the write
//! queue for anything that changes data.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures::future::{Either, select};
use futures::{AsyncReadExt as _, FutureExt as _};
use gpui::BackgroundExecutor;
use http_client::http::{HeaderMap, HeaderValue};
use http_client::{AsyncBody, HttpClient, Method, RedirectPolicy, Request};
use serde_json::Value;
use url::Url;

use crate::error::{GithubError, GithubResult, unexpected};
use crate::model::RateLimits;
use crate::rest::{
    self, API_BASE, API_VERSION, CachedResponse, ETAG_CACHE_MAX_BYTES, ETAG_CACHE_MAX_ENTRIES,
    EtagCache,
};
use crate::token::GithubToken;
use crate::write_queue::{RATE_LIMITED_PAUSE, WriteOutcome, WriteQueue, WriteTicket};

/// Past this the request is given up as a network failure.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// A page of 100 issues with long bodies is a few hundred kilobytes; anything
/// far bigger is not an answer worth holding in memory.
pub const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

/// Whether a request only reads. It decides what a lost answer means: a read
/// that got none can simply be asked again, a write may already be applied.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    Read,
    Write,
}

impl Access {
    /// `never_sent` is true when the request provably did not leave this
    /// machine, so a write is safe to try again and is reported as a plain
    /// network failure rather than an unknown outcome.
    fn lost_answer(self, cause: &str, never_sent: bool) -> GithubError {
        let cause = cause.trim_end_matches('.');
        match self {
            Self::Write if !never_sent => GithubError::OutcomeUnknown {
                message: cause.to_string(),
            },
            Self::Read | Self::Write => GithubError::Network {
                message: format!("{cause}."),
            },
        }
    }
}

/// Whether `http_client.send` failed while still making the connection (DNS,
/// refused, unreachable, TLS handshake), before any byte of the request was
/// written. The reqwest client reports this itself; for any other client the
/// `io::Error` kinds in the chain are the only evidence. Whatever is neither
/// is treated as possibly sent: a failure that cannot be placed must not
/// invite a retry of a write that may have been applied.
pub(crate) fn failed_before_the_request_was_sent(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        if let Some(error) = cause.downcast_ref::<reqwest::Error>() {
            return error.is_connect();
        }
        cause.downcast_ref::<std::io::Error>().is_some_and(|error| {
            matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::HostUnreachable
                    | std::io::ErrorKind::NetworkUnreachable
                    | std::io::ErrorKind::AddrNotAvailable
            )
        })
    })
}

/// Why an exchange produced no answer, and whether the request went out.
struct LostExchange {
    message: String,
    never_sent: bool,
}

/// What came back from one exchange with GitHub.
enum Exchange {
    Answered(Answer),
    TooLarge,
}

pub(crate) struct Transport {
    pub http_client: Arc<dyn HttpClient>,
    pub token: GithubToken,
    pub executor: BackgroundExecutor,
    pub write_queue: Arc<WriteQueue>,
    pub etags: Mutex<EtagCache>,
    pub rate_limits: Mutex<RateLimits>,
}

pub(crate) struct Answer {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: String,
}

/// A successful `GET`, from GitHub or from the ETag cache.
pub(crate) struct RestAnswer {
    pub body: String,
    pub headers: HeaderMap,
    pub next_page: Option<String>,
}

pub(crate) fn api_url(segments: &[&str], query: &[(&str, String)]) -> GithubResult<Url> {
    // The url crate drops these instead of encoding them, so removing a label
    // named `..` would otherwise become "remove every label".
    if let Some(segment) = segments
        .iter()
        .find(|segment| matches!(**segment, "." | ".."))
    {
        return Err(GithubError::Rejected {
            message: format!("`{segment}` cannot be used as a name in a GitHub address."),
        });
    }
    let mut url = Url::parse(API_BASE).map_err(unexpected)?;
    url.path_segments_mut()
        .map_err(|()| unexpected("the API address has no path"))?
        .extend(segments);
    if !query.is_empty() {
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(name, value)| (*name, value.as_str())));
    }
    Ok(url)
}

impl Transport {
    pub fn new(
        http_client: Arc<dyn HttpClient>,
        token: GithubToken,
        executor: BackgroundExecutor,
        write_queue: Arc<WriteQueue>,
    ) -> Self {
        Self {
            http_client,
            token,
            executor,
            write_queue,
            etags: Mutex::new(EtagCache::new(ETAG_CACHE_MAX_ENTRIES, ETAG_CACHE_MAX_BYTES)),
            rate_limits: Mutex::new(RateLimits::default()),
        }
    }

    pub fn rate_limits(&self) -> RateLimits {
        self.rate_limits
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Sends the request and reads the body. A redirect is refused rather
    /// than followed, so the token never goes to an address GitHub names.
    async fn send(
        &self,
        access: Access,
        method: Method,
        url: &Url,
        body: Option<&Value>,
        if_none_match: Option<&str>,
    ) -> GithubResult<Answer> {
        let mut authorization = HeaderValue::from_str(&format!("Bearer {}", self.token.secret()))
            .map_err(unexpected)?;
        authorization.set_sensitive(true);
        let mut builder = Request::builder()
            .method(method)
            .uri(url.as_str())
            .header("Authorization", authorization)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .header("User-Agent", "Zode");
        if let Some(etag) = if_none_match {
            builder = builder.header("If-None-Match", etag);
        }
        let body = match body {
            Some(body) => {
                builder = builder.header("Content-Type", "application/json");
                AsyncBody::from(serde_json::to_vec(body).map_err(unexpected)?)
            }
            None => AsyncBody::empty(),
        };
        let mut request = builder.body(body).map_err(unexpected)?;
        request.extensions_mut().insert(RedirectPolicy::NoFollow);

        let exchange = async {
            let mut response =
                self.http_client
                    .send(request)
                    .await
                    .map_err(|error| LostExchange {
                        never_sent: failed_before_the_request_was_sent(&error),
                        message: error.to_string(),
                    })?;
            let mut bytes = Vec::new();
            // One byte past the cap tells an answer of exactly the cap from a
            // longer one, which would otherwise be cut and fail as bad JSON.
            response
                .body_mut()
                .take(MAX_RESPONSE_BYTES + 1)
                .read_to_end(&mut bytes)
                .await
                .map_err(|error| LostExchange {
                    never_sent: false,
                    message: error.to_string(),
                })?;
            if bytes.len() as u64 > MAX_RESPONSE_BYTES {
                return Ok(Exchange::TooLarge);
            }
            Ok::<_, LostExchange>(Exchange::Answered(Answer {
                status: response.status().as_u16(),
                headers: response.headers().clone(),
                body: String::from_utf8_lossy(&bytes).into_owned(),
            }))
        };
        let timer = self.executor.timer(REQUEST_TIMEOUT);
        let exchange = match select(exchange.boxed(), timer).await {
            Either::Left((exchange, _)) => exchange.map_err(|lost| {
                access.lost_answer(
                    &crate::error::shorten_message(&lost.message),
                    lost.never_sent,
                )
            })?,
            Either::Right(_) => {
                return Err(access.lost_answer(
                    &format!(
                        "GitHub did not answer within {} seconds",
                        REQUEST_TIMEOUT.as_secs()
                    ),
                    false,
                ));
            }
        };
        let answer = match exchange {
            Exchange::Answered(answer) => answer,
            Exchange::TooLarge => {
                return Err(match access {
                    Access::Read => unexpected(format!(
                        "GitHub's answer is too large to read (over {} MB).",
                        MAX_RESPONSE_BYTES / (1024 * 1024)
                    )),
                    Access::Write => access.lost_answer("the answer was too large to read", false),
                });
            }
        };
        self.record_rate_limit(&answer.headers);
        if (300..400).contains(&answer.status) && answer.status != 304 {
            return Err(unexpected(format!(
                "GitHub answered with a redirect (HTTP {}), which Zode does not follow.",
                answer.status
            )));
        }
        Ok(answer)
    }

    fn record_rate_limit(&self, headers: &HeaderMap) {
        let Some(limit) = rest::rate_limit(headers) else {
            return;
        };
        let mut rate_limits = self
            .rate_limits
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match rest::rate_limit_resource(headers) {
            Some("graphql") => rate_limits.graphql = Some(limit),
            Some("core") | None => rate_limits.rest = Some(limit),
            // `search` (30 a minute) is a bucket of its own. `RateLimits`
            // has no slot for it, and filing it under `rest` would show the
            // search budget as the REST one.
            Some(_) => {}
        }
    }

    pub async fn get(&self, url: Url, use_cache: bool) -> GithubResult<RestAnswer> {
        let identity = self.token.identity().clone();
        let etag = use_cache
            .then(|| {
                self.etags
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&identity, url.as_str())
                    .map(|cached| cached.etag.clone())
            })
            .flatten();
        let mut answer = self
            .send(Access::Read, Method::GET, &url, None, etag.as_deref())
            .await?;
        if answer.status == 304 {
            let cached = self
                .etags
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&identity, url.as_str())
                .cloned();
            match cached {
                Some(cached) => {
                    return Ok(RestAnswer {
                        body: cached.body,
                        headers: answer.headers,
                        next_page: cached.next_page,
                    });
                }
                // Evicted while the request was out, so there is nothing to
                // answer a 304 with.
                None => {
                    answer = self
                        .send(Access::Read, Method::GET, &url, None, None)
                        .await?;
                }
            }
        }
        if !(200..300).contains(&answer.status) {
            return Err(rest::error_for_response(
                answer.status,
                &answer.headers,
                &answer.body,
            ));
        }
        let next_page = rest::next_page(&answer.headers);
        if use_cache && let Some(etag) = rest::header(&answer.headers, "etag") {
            self.etags
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(
                    identity,
                    url.to_string(),
                    CachedResponse {
                        etag: etag.to_string(),
                        body: answer.body.clone(),
                        next_page: next_page.clone(),
                    },
                );
        }
        Ok(RestAnswer {
            body: answer.body,
            headers: answer.headers,
            next_page,
        })
    }

    /// A REST call that changes data, run in the token's write queue.
    pub async fn write(
        &self,
        ticket: WriteTicket,
        method: Method,
        url: Url,
        body: Option<Value>,
    ) -> GithubResult<String> {
        self.write_queue
            .run(ticket, async {
                let answer = match self
                    .send(Access::Write, method, &url, body.as_ref(), None)
                    .await
                {
                    Ok(answer) => answer,
                    Err(error) => return failed_write(error, None),
                };
                let retry_after = rest::retry_after(&answer.headers);
                if (200..300).contains(&answer.status) {
                    WriteOutcome {
                        result: Ok(answer.body),
                        retry_after,
                    }
                } else {
                    let error =
                        rest::error_for_response(answer.status, &answer.headers, &answer.body);
                    failed_write(error, retry_after)
                }
            })
            .await
    }

    /// A GraphQL read. Items GitHub would not show are left out of the data
    /// rather than failing the whole read.
    pub async fn graphql(&self, document: &str, variables: Value) -> GithubResult<Value> {
        graphql_data(
            &self
                .graphql_answer(Access::Read, document, variables)
                .await?,
            crate::graphql::Partial::KeepNestedNodes,
        )
    }

    /// A GraphQL mutation, run in the token's write queue.
    pub async fn graphql_mutation(
        &self,
        ticket: WriteTicket,
        document: &str,
        variables: Value,
    ) -> GithubResult<Value> {
        self.write_queue
            .run(ticket, async {
                let answer = match self
                    .graphql_answer(Access::Write, document, variables)
                    .await
                {
                    Ok(answer) => answer,
                    Err(error) => return failed_write(error, None),
                };
                let retry_after = rest::retry_after(&answer.headers);
                match graphql_data(&answer, crate::graphql::Partial::Refuse) {
                    Ok(data) => WriteOutcome {
                        result: Ok(data),
                        retry_after,
                    },
                    Err(error) => failed_write(error, retry_after),
                }
            })
            .await
    }

    async fn graphql_answer(
        &self,
        access: Access,
        document: &str,
        variables: Value,
    ) -> GithubResult<Answer> {
        let url = api_url(&["graphql"], &[])?;
        let body = crate::graphql::request_body(document, variables);
        self.send(access, Method::POST, &url, Some(&body), None)
            .await
    }
}

/// GraphQL errors come with HTTP 200, so both the status and `errors[]` are
/// read.
fn graphql_data(answer: &Answer, partial: crate::graphql::Partial) -> GithubResult<Value> {
    if !(200..300).contains(&answer.status) {
        return Err(rest::error_for_response(
            answer.status,
            &answer.headers,
            &answer.body,
        ));
    }
    crate::graphql::data_or_partial_error(&answer.body, partial).map_err(|error| match error {
        // A GraphQL rate limit names no reset time; the headers do.
        GithubError::RateLimited {
            reset_at: None,
            retry_after,
        } => GithubError::RateLimited {
            reset_at: rest::reset_at(&answer.headers),
            retry_after,
        },
        // GraphQL answers an SSO refusal with HTTP 200 and the same header
        // REST sends.
        GithubError::Forbidden { .. } => rest::sso_authorize_url(&answer.headers)
            .map_or(error, |authorize_url| GithubError::SsoRequired {
                authorize_url,
            }),
        error => error,
    })
}

/// A failed write holds the next one back for as long as GitHub asked, or for
/// [`RATE_LIMITED_PAUSE`] when it hit a rate limit without saying.
fn failed_write<T>(error: GithubError, retry_after: Option<Duration>) -> WriteOutcome<T> {
    let retry_after = retry_after
        .or_else(|| matches!(error, GithubError::RateLimited { .. }).then_some(RATE_LIMITED_PAUSE));
    WriteOutcome {
        result: Err(error),
        retry_after,
    }
}
