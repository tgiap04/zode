use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures::AsyncReadExt as _;
use gpui::TestAppContext;
use http_client::{AsyncBody, FakeHttpClient, RedirectPolicy, Response};
use serde_json::{Value, json};

use crate::backend::GithubBackend;
use crate::error::GithubError;
use crate::graphql::documents;
use crate::http_backend::HttpBackend;
use crate::http_backend::SEARCH_TEXT_MAX_CHARACTERS;
use crate::http_transport::{MAX_RESPONSE_BYTES, REQUEST_TIMEOUT};
use crate::model::{
    IssueFilter, IssueState, IssueStateFilter, OAuthScopes, ParentIssue, RateLimit, RepositoryId,
    RepositoryVisibility, SubIssuePosition, SubIssuesSummary,
};
use crate::projects::ProjectFieldInput;
use crate::pull_requests::MergeMethod;
use crate::token::{GithubToken, TokenSource};
use crate::write_queue::{MIN_WRITE_INTERVAL, RATE_LIMITED_PAUSE};

const SECRET: &str = "ghu_secret_token_value";

#[derive(Clone)]
enum Canned {
    Answer {
        status: u16,
        headers: Vec<(&'static str, String)>,
        body: String,
    },
    Hang,
    /// The connection drops after the request was read.
    Fail,
    /// The client cannot reach GitHub at all: DNS, refused, offline.
    FailToConnect(std::io::ErrorKind),
    /// The headers arrive, then the connection breaks while the body is read.
    /// The kind is one that, in the connect phase, would prove nothing was sent.
    BrokenBody(std::io::ErrorKind),
}

/// A reader that fails, to stand in for a body cut off mid-stream.
struct BrokenReader(std::io::ErrorKind);

impl futures::AsyncRead for BrokenReader {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        _buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Ready(Err(std::io::Error::new(self.0, "body cut off")))
    }
}

fn answer(status: u16, headers: &[(&'static str, &str)], body: impl Into<String>) -> Canned {
    Canned::Answer {
        status,
        headers: headers
            .iter()
            .map(|(name, value)| (*name, value.to_string()))
            .collect(),
        body: body.into(),
    }
}

#[derive(Clone, Debug)]
struct Sent {
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
    body: Value,
    redirect_policy: Option<RedirectPolicy>,
    authorization_is_sensitive: bool,
}

impl Sent {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

struct Harness {
    backend: HttpBackend,
    sent: Arc<Mutex<Vec<Sent>>>,
    canned: Arc<Mutex<VecDeque<Canned>>>,
}

impl Harness {
    fn sent(&self) -> Vec<Sent> {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn push(&self, canned: Canned) {
        self.canned
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(canned);
    }
}

fn set_up(source: TokenSource, answers: Vec<Canned>, cx: &mut TestAppContext) -> Harness {
    let sent: Arc<Mutex<Vec<Sent>>> = Arc::default();
    let canned = Arc::new(Mutex::new(VecDeque::from(answers)));
    let client = FakeHttpClient::create({
        let sent = sent.clone();
        let canned = canned.clone();
        move |request| {
            let sent = sent.clone();
            let next = canned
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front();
            async move {
                let redirect_policy = request.extensions().get::<RedirectPolicy>().cloned();
                let authorization_is_sensitive = request
                    .headers()
                    .get("authorization")
                    .is_some_and(|value| value.is_sensitive());
                let (parts, mut body) = request.into_parts();
                let mut bytes = Vec::new();
                body.read_to_end(&mut bytes).await?;
                sent.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(Sent {
                        method: parts.method.to_string(),
                        uri: parts.uri.to_string(),
                        headers: parts
                            .headers
                            .iter()
                            .map(|(name, value)| {
                                (
                                    name.to_string(),
                                    value.to_str().unwrap_or_default().to_string(),
                                )
                            })
                            .collect(),
                        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
                        redirect_policy,
                        authorization_is_sensitive,
                    });
                match next {
                    Some(Canned::Answer {
                        status,
                        headers,
                        body,
                    }) => {
                        let mut builder = Response::builder().status(status);
                        for (name, value) in headers {
                            builder = builder.header(name, value);
                        }
                        Ok(builder.body(AsyncBody::from(body))?)
                    }
                    Some(Canned::Hang) => futures::future::pending().await,
                    Some(Canned::Fail) => Err(anyhow::anyhow!("connection reset by peer")),
                    Some(Canned::FailToConnect(kind)) => Err(anyhow::Error::new(
                        std::io::Error::new(kind, "connect failed"),
                    )
                    .context("error sending request for url (https://api.github.com/)")),
                    Some(Canned::BrokenBody(kind)) => Ok(Response::builder()
                        .status(200)
                        .body(AsyncBody::from_reader(BrokenReader(kind)))?),
                    None => Err(anyhow::anyhow!("no answer was set up")),
                }
            }
        }
    });
    let token = GithubToken::new(SECRET, source);
    let backend = cx.update(|cx| HttpBackend::new(client, token, cx));
    Harness {
        backend,
        sent,
        canned,
    }
}

fn repository() -> RepositoryId {
    RepositoryId::new("octo", "repo").expect("a valid repository")
}

fn issue_json(number: u64, is_pull_request: bool) -> Value {
    let mut issue = json!({
        "id": 1000 + number,
        "node_id": format!("I_{number}"),
        "number": number,
        "title": format!("Issue {number}"),
        "body": "text",
        "state": "open",
        "state_reason": null,
        "user": {"login": "octocat"},
        "labels": [{"name": "bug", "color": "d73a4a", "description": null}],
        "assignees": [],
        "milestone": null,
        "comments": 2,
        "html_url": format!("https://github.com/octo/repo/issues/{number}"),
        "repository_url": "https://api.github.com/repos/octo/repo",
        "created_at": "2026-11-01T00:00:00Z",
        "updated_at": "2026-11-02T00:00:00Z"
    });
    if is_pull_request {
        issue["pull_request"] = json!({"url": "https://api.github.com/repos/octo/repo/pulls/2"});
    }
    issue
}

fn comment_json(id: u64) -> Value {
    json!({
        "id": id,
        "node_id": format!("IC_{id}"),
        "user": {"login": "octocat"},
        "body": "hello",
        "html_url": format!("https://github.com/octo/repo/issues/1#issuecomment-{id}"),
        "created_at": "2026-11-01T00:00:00Z",
        "updated_at": "2026-11-01T00:00:00Z"
    })
}

#[gpui::test]
async fn rest_requests_carry_the_token_and_api_headers(cx: &mut TestAppContext) {
    let harness = set_up(
        TokenSource::GhCli,
        vec![answer(200, &[], json!([issue_json(1, false)]).to_string())],
        cx,
    );
    let filter = IssueFilter {
        state: IssueStateFilter::All,
        labels: vec!["bug".to_string(), "good first issue".to_string()],
        assignee: Some("octocat".to_string()),
        ..IssueFilter::default()
    };
    let page = harness
        .backend
        .list_issues(&repository(), &filter, None)
        .await
        .expect("a page");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].labels[0].name, "bug");
    let sent = harness.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, "GET");
    assert_eq!(
        sent[0].uri,
        "https://api.github.com/repos/octo/repo/issues?per_page=50&page=1&state=all&labels=bug%2Cgood+first+issue&assignee=octocat"
    );
    assert_eq!(
        sent[0].header("authorization"),
        Some(format!("Bearer {SECRET}").as_str())
    );
    assert_eq!(
        sent[0].header("accept"),
        Some("application/vnd.github+json")
    );
    assert_eq!(sent[0].header("x-github-api-version"), Some("2026-03-10"));
    assert_eq!(sent[0].redirect_policy, Some(RedirectPolicy::NoFollow));
}

#[gpui::test]
async fn the_issue_list_leaves_out_pull_requests(cx: &mut TestAppContext) {
    let body = json!([
        issue_json(1, false),
        issue_json(2, true),
        issue_json(3, false)
    ]);
    let harness = set_up(
        TokenSource::GhCli,
        vec![answer(200, &[], body.to_string())],
        cx,
    );
    let page = harness
        .backend
        .list_issues(&repository(), &IssueFilter::default(), None)
        .await
        .expect("a page");
    let numbers = page
        .items
        .iter()
        .map(|issue| issue.number)
        .collect::<Vec<_>>();
    assert_eq!(numbers, vec![1, 3]);
}

#[gpui::test]
async fn an_unchanged_list_is_served_from_the_etag_cache(cx: &mut TestAppContext) {
    let link = "<https://api.github.com/repositories/1/issues?page=2>; rel=\"next\"";
    let harness = set_up(
        TokenSource::Keychain,
        vec![
            answer(
                200,
                &[("etag", "W/\"abc\""), ("link", link)],
                json!([issue_json(1, false)]).to_string(),
            ),
            answer(304, &[("etag", "W/\"abc\"")], ""),
        ],
        cx,
    );
    let first = harness
        .backend
        .list_issues(&repository(), &IssueFilter::default(), None)
        .await
        .expect("a page");
    let second = harness
        .backend
        .list_issues(&repository(), &IssueFilter::default(), None)
        .await
        .expect("the cached page");
    assert_eq!(first, second);
    assert_eq!(second.next_cursor.as_deref(), Some("page=2"));
    let sent = harness.sent();
    assert_eq!(sent[0].header("if-none-match"), None);
    assert_eq!(sent[1].header("if-none-match"), Some("W/\"abc\""));
}

#[gpui::test]
async fn comments_are_paged_by_the_link_header(cx: &mut TestAppContext) {
    let link = "<https://api.github.com/repositories/1/issues/1/comments?page=2>; rel=\"next\"";
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(200, &[("link", link)], json!([comment_json(1)]).to_string()),
            answer(200, &[], json!([comment_json(2)]).to_string()),
        ],
        cx,
    );
    let first = harness
        .backend
        .list_comments(&repository(), 1, None)
        .await
        .expect("a page");
    assert_eq!(first.next_cursor.as_deref(), Some("page=2"));
    let second = harness
        .backend
        .list_comments(&repository(), 1, first.next_cursor)
        .await
        .expect("a page");
    assert_eq!(second.items[0].id, 2);
    assert_eq!(second.next_cursor, None);
    assert_eq!(
        harness.sent()[1].uri,
        "https://api.github.com/repos/octo/repo/issues/1/comments?per_page=50&page=2"
    );
}

#[gpui::test]
async fn the_viewer_reports_scopes_only_when_github_sends_them(cx: &mut TestAppContext) {
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(
                200,
                &[("x-oauth-scopes", "repo, read:org")],
                r#"{"login":"octocat"}"#,
            ),
            answer(200, &[], r#"{"login":"octocat"}"#),
        ],
        cx,
    );
    let classic = harness.backend.viewer().await.expect("a viewer");
    assert_eq!(classic.login, "octocat");
    assert_eq!(classic.scopes.includes("read:org"), Some(true));
    let fine_grained = harness.backend.viewer().await.expect("a viewer");
    assert_eq!(fine_grained.scopes, OAuthScopes::Unknown);
    assert_eq!(harness.sent()[0].uri, "https://api.github.com/user");
}

#[gpui::test]
async fn rest_and_graphql_rate_limits_are_kept_apart(cx: &mut TestAppContext) {
    let limits = |resource: &'static str, remaining: &'static str| {
        [
            ("x-ratelimit-resource", resource),
            ("x-ratelimit-limit", "5000"),
            ("x-ratelimit-remaining", remaining),
            ("x-ratelimit-used", "1"),
            ("x-ratelimit-reset", "1764000000"),
        ]
    };
    let projects = json!({"data": {"repository": {"projectsV2": {
        "nodes": [], "pageInfo": {"hasNextPage": false, "endCursor": null}
    }}}});
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(200, &limits("core", "4999"), r#"{"login":"octocat"}"#),
            answer(200, &limits("graphql", "4900"), projects.to_string()),
        ],
        cx,
    );
    harness.backend.viewer().await.expect("a viewer");
    harness
        .backend
        .list_projects(&repository(), None)
        .await
        .expect("projects");
    let rate_limits = harness.backend.rate_limits();
    assert_eq!(
        rate_limits.rest,
        Some(RateLimit {
            limit: 5000,
            remaining: 4999,
            used: 1,
            reset_at: 1_764_000_000
        })
    );
    assert_eq!(rate_limits.graphql.map(|limit| limit.remaining), Some(4900));
}

#[gpui::test]
async fn a_refused_keychain_token_is_revoked_and_never_leaks(cx: &mut TestAppContext) {
    let harness = set_up(
        TokenSource::Keychain,
        vec![answer(401, &[], r#"{"message":"Bad credentials"}"#)],
        cx,
    );
    let error = harness
        .backend
        .issue(&repository(), 1)
        .await
        .expect_err("refused");
    assert_eq!(error, GithubError::TokenRevoked);
    for text in [
        error.to_string(),
        format!("{error:?}"),
        format!("{:?}", harness.backend),
    ] {
        assert!(!text.contains(SECRET), "{text}");
    }
    assert_eq!(
        harness.sent().len(),
        1,
        "nothing is retried with another token"
    );
}

#[gpui::test]
async fn graphql_posts_the_document_and_reads_a_page(cx: &mut TestAppContext) {
    let projects = json!({"data": {"repository": {"projectsV2": {
        "nodes": [{"id": "PVT_1", "number": 3, "title": "Roadmap", "url": "https://github.com/orgs/octo/projects/3", "closed": false}],
        "pageInfo": {"hasNextPage": true, "endCursor": "Y3Vy"}
    }}}});
    let harness = set_up(
        TokenSource::GhCli,
        vec![answer(200, &[], projects.to_string())],
        cx,
    );
    let page = harness
        .backend
        .list_projects(&repository(), None)
        .await
        .expect("projects");
    assert_eq!(page.items[0].title, "Roadmap");
    assert_eq!(page.next_cursor.as_deref(), Some("Y3Vy"));
    let sent = harness.sent();
    assert_eq!(sent[0].method, "POST");
    assert_eq!(sent[0].uri, "https://api.github.com/graphql");
    assert_eq!(sent[0].body["query"], json!(documents::REPOSITORY_PROJECTS));
    assert_eq!(
        sent[0].body["variables"],
        json!({"owner": "octo", "name": "repo", "after": null})
    );
}

#[gpui::test]
async fn graphql_errors_arrive_readable(cx: &mut TestAppContext) {
    let body = json!({"data": null, "errors": [{"type": "INSUFFICIENT_SCOPES", "message": "The 'projectsV2' field requires one of the following scopes: ['read:project'], but your token has only been granted the: ['repo'] scopes."}]});
    let harness = set_up(
        TokenSource::GhCli,
        vec![answer(200, &[], body.to_string())],
        cx,
    );
    let error = harness
        .backend
        .list_projects(&repository(), None)
        .await
        .expect_err("a scope error");
    assert_eq!(
        error,
        GithubError::MissingScope {
            scope: "read:project".to_string()
        }
    );
}

#[gpui::test]
async fn a_missing_pull_request_is_not_found(cx: &mut TestAppContext) {
    let body = json!({"data": {"repository": {"pullRequest": null}}});
    let harness = set_up(
        TokenSource::GhCli,
        vec![answer(200, &[], body.to_string())],
        cx,
    );
    let error = harness
        .backend
        .pull_request(&repository(), 9)
        .await
        .expect_err("missing");
    assert_eq!(error, GithubError::NotFound);
    assert_eq!(harness.sent()[0].body["variables"]["number"], json!(9));
}

#[gpui::test]
async fn a_redirect_is_not_followed(cx: &mut TestAppContext) {
    let harness = set_up(
        TokenSource::GhCli,
        vec![answer(301, &[("location", "https://evil.example/")], "")],
        cx,
    );
    let error = harness
        .backend
        .issue(&repository(), 1)
        .await
        .expect_err("refused");
    assert!(
        matches!(error, GithubError::UnexpectedResponse { ref message } if message.contains("redirect")),
        "{error:?}"
    );
    assert_eq!(harness.sent().len(), 1);
}

#[gpui::test]
async fn a_label_is_removed_by_its_encoded_name(cx: &mut TestAppContext) {
    let harness = set_up(TokenSource::GhCli, vec![answer(200, &[], "[]")], cx);
    harness
        .backend
        .remove_label(&repository(), 4, "good first issue/ä".to_string())
        .await
        .expect("removed");
    let sent = harness.sent();
    assert_eq!(sent[0].method, "DELETE");
    assert_eq!(
        sent[0].uri,
        "https://api.github.com/repos/octo/repo/issues/4/labels/good%20first%20issue%2F%C3%A4"
    );
}

#[gpui::test]
async fn writes_wait_their_turn(cx: &mut TestAppContext) {
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(201, &[], comment_json(1).to_string()),
            answer(201, &[], comment_json(2).to_string()),
        ],
        cx,
    );
    let first = harness
        .backend
        .add_comment(&repository(), 1, "one".to_string());
    let second = harness
        .backend
        .add_comment(&repository(), 1, "two".to_string());
    let first = cx.background_executor.spawn(first);
    let second = cx.background_executor.spawn(second);
    cx.run_until_parked();
    assert_eq!(harness.sent().len(), 1);
    assert_eq!(harness.sent()[0].body, json!({"body": "one"}));
    cx.executor()
        .advance_clock(MIN_WRITE_INTERVAL - Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(harness.sent().len(), 1);
    cx.executor().advance_clock(Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(harness.sent().len(), 2);
    first.await.expect("first comment");
    second.await.expect("second comment");
}

#[gpui::test]
async fn a_rate_limited_write_holds_the_queue_for_a_minute(cx: &mut TestAppContext) {
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(
                403,
                &[],
                r#"{"message":"You have exceeded a secondary rate limit."}"#,
            ),
            answer(201, &[], comment_json(2).to_string()),
        ],
        cx,
    );
    let first = cx.background_executor.spawn(harness.backend.add_comment(
        &repository(),
        1,
        "one".to_string(),
    ));
    let second = cx.background_executor.spawn(harness.backend.add_comment(
        &repository(),
        1,
        "two".to_string(),
    ));
    cx.run_until_parked();
    cx.executor()
        .advance_clock(RATE_LIMITED_PAUSE - Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(harness.sent().len(), 1);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(harness.sent().len(), 2);
    assert!(matches!(first.await, Err(GithubError::RateLimited { .. })));
    second.await.expect("second comment");
}

#[gpui::test]
async fn a_silent_github_times_out(cx: &mut TestAppContext) {
    let harness = set_up(TokenSource::GhCli, vec![Canned::Hang], cx);
    let request = cx
        .background_executor
        .spawn(harness.backend.issue(&repository(), 1));
    cx.run_until_parked();
    cx.executor().advance_clock(REQUEST_TIMEOUT);
    cx.run_until_parked();
    assert!(matches!(request.await, Err(GithubError::Network { .. })));
    harness.push(answer(200, &[], issue_json(1, false).to_string()));
    let issue = harness
        .backend
        .issue(&repository(), 1)
        .await
        .expect("answered");
    assert_eq!(issue.number, 1);
}

#[gpui::test]
async fn the_repository_visibility_is_read_from_its_rest_answer(cx: &mut TestAppContext) {
    let harness = set_up(
        TokenSource::Keychain,
        vec![
            answer(200, &[], r#"{"private":true,"visibility":"private"}"#),
            answer(200, &[], r#"{"private":false,"visibility":"internal"}"#),
            answer(200, &[], r#"{"private":false}"#),
        ],
        cx,
    );
    let backend = &harness.backend;
    assert_eq!(
        backend.repository_visibility(&repository()).await,
        Ok(RepositoryVisibility::Private)
    );
    assert_eq!(
        backend.repository_visibility(&repository()).await,
        Ok(RepositoryVisibility::Internal)
    );
    assert_eq!(
        backend.repository_visibility(&repository()).await,
        Ok(RepositoryVisibility::Public),
        "an answer without `visibility` falls back to `private`"
    );
    assert_eq!(
        harness.sent()[0].uri,
        "https://api.github.com/repos/octo/repo"
    );
}

#[gpui::test]
async fn the_issue_list_asks_for_the_author_mentions_and_milestone(cx: &mut TestAppContext) {
    let harness = set_up(TokenSource::GhCli, vec![answer(200, &[], "[]")], cx);
    let filter = IssueFilter {
        creator: Some("hubot".to_string()),
        mentioned: Some("octocat".to_string()),
        milestone: Some(7),
        ..IssueFilter::default()
    };
    harness
        .backend
        .list_issues(&repository(), &filter, None)
        .await
        .expect("a page");
    assert_eq!(
        harness.sent()[0].uri,
        "https://api.github.com/repos/octo/repo/issues?per_page=50&page=1&state=open&creator=hubot&mentioned=octocat&milestone=7"
    );
}

#[gpui::test]
async fn the_issue_list_and_search_narrow_to_an_issue_type(cx: &mut TestAppContext) {
    let mut parent = issue_json(1, false);
    parent["sub_issues_summary"] = json!({"total": 3, "completed": 1, "percent_completed": 33});
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(200, &[], json!([parent]).to_string()),
            answer(200, &[], json!({"items": []}).to_string()),
        ],
        cx,
    );
    let filter = IssueFilter {
        issue_type: Some("Bug".to_string()),
        ..IssueFilter::default()
    };
    let page = harness
        .backend
        .list_issues(&repository(), &filter, None)
        .await
        .expect("a page");
    assert_eq!(
        page.items[0].sub_issues_summary,
        SubIssuesSummary {
            total: 3,
            completed: 1
        }
    );
    harness
        .backend
        .search_issues(&repository(), &filter, "crash", None)
        .await
        .expect("a page");
    let sent = harness.sent();
    assert_eq!(
        sent[0].uri,
        "https://api.github.com/repos/octo/repo/issues?per_page=50&page=1&state=open&type=Bug"
    );
    let query = url::Url::parse(&sent[1].uri)
        .expect("a url")
        .query_pairs()
        .find(|(name, _)| name == "q")
        .map(|(_, value)| value.into_owned());
    assert_eq!(
        query.as_deref(),
        Some("repo:octo/repo is:issue is:open type:\"Bug\" (crash)")
    );
}

#[gpui::test]
async fn the_parent_is_read_with_its_document(cx: &mut TestAppContext) {
    let with_parent = json!({"data": {"repository": {"issue": {"parent": {
        "id": "I_7", "databaseId": 1007, "number": 7, "title": "Epic", "state": "OPEN",
        "repository": {"nameWithOwner": "octo/other"}
    }}}}});
    let without = json!({"data": {"repository": {"issue": {"parent": null}}}});
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(200, &[], with_parent.to_string()),
            answer(200, &[], without.to_string()),
        ],
        cx,
    );
    let parent = harness
        .backend
        .issue_parent(&repository(), 3)
        .await
        .expect("read");
    assert_eq!(
        parent,
        Some(ParentIssue {
            id: 1007,
            node_id: "I_7".to_string(),
            number: 7,
            title: "Epic".to_string(),
            state: IssueState::Open,
            repository: RepositoryId::new("octo", "other").expect("valid"),
        })
    );
    let none = harness
        .backend
        .issue_parent(&repository(), 4)
        .await
        .expect("read");
    assert_eq!(none, None);
    let sent = harness.sent();
    assert_eq!(sent[0].body["query"], json!(documents::ISSUE_PARENT));
    assert_eq!(
        sent[0].body["variables"],
        json!({"owner": "octo", "name": "repo", "number": 3})
    );
}

#[gpui::test]
async fn sub_issue_moves_go_through_the_queue_with_their_ids(cx: &mut TestAppContext) {
    let moved = json!({"data": {"reprioritizeSubIssue": {"issue": {"id": "I_1"}}}});
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(201, &[], issue_json(7, false).to_string()),
            answer(200, &[], moved.to_string()),
            answer(200, &[], moved.to_string()),
        ],
        cx,
    );
    let replaced =
        cx.background_executor
            .spawn(harness.backend.replace_parent(&repository(), 7, 1003));
    let after = cx
        .background_executor
        .spawn(harness.backend.reprioritize_sub_issue(
            "I_1".to_string(),
            "I_3".to_string(),
            SubIssuePosition::After("I_2".to_string()),
        ));
    let before = cx
        .background_executor
        .spawn(harness.backend.reprioritize_sub_issue(
            "I_1".to_string(),
            "I_3".to_string(),
            SubIssuePosition::Before("I_2".to_string()),
        ));
    cx.run_until_parked();
    assert_eq!(harness.sent().len(), 1, "the moves wait their turn");
    cx.executor().advance_clock(MIN_WRITE_INTERVAL);
    cx.run_until_parked();
    cx.executor().advance_clock(MIN_WRITE_INTERVAL);
    cx.run_until_parked();
    let sent = harness.sent();
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[0].method, "POST");
    assert_eq!(
        sent[0].uri,
        "https://api.github.com/repos/octo/repo/issues/7/sub_issues"
    );
    assert_eq!(
        sent[0].body,
        json!({"sub_issue_id": 1003, "replace_parent": true})
    );
    assert_eq!(
        sent[1].body["query"],
        json!(documents::REPRIORITIZE_SUB_ISSUE)
    );
    assert_eq!(
        sent[1].body["variables"],
        json!({"issueId": "I_1", "subIssueId": "I_3", "afterId": "I_2", "beforeId": null})
    );
    assert_eq!(
        sent[2].body["variables"],
        json!({"issueId": "I_1", "subIssueId": "I_3", "afterId": null, "beforeId": "I_2"})
    );
    replaced.await.expect("replaced");
    after.await.expect("moved after");
    before.await.expect("moved before");
}

#[gpui::test]
async fn search_stays_in_the_repository_and_leaves_out_pull_requests(cx: &mut TestAppContext) {
    let link = "<https://api.github.com/search/issues?q=x&page=2>; rel=\"next\"";
    let body = json!({
        "total_count": 3,
        "incomplete_results": false,
        "items": [issue_json(1, false), issue_json(2, true), issue_json(3, false)]
    });
    let harness = set_up(
        TokenSource::GhCli,
        vec![answer(
            200,
            &[("link", link), ("x-ratelimit-resource", "search")],
            body.to_string(),
        )],
        cx,
    );
    let filter = IssueFilter {
        state: IssueStateFilter::Closed,
        labels: vec!["good first issue".to_string()],
        assignee: Some("octocat".to_string()),
        ..IssueFilter::default()
    };
    let page = harness
        .backend
        .search_issues(
            &repository(),
            &filter,
            "crash repo:evil/other ORG:evil user:evil on start",
            None,
        )
        .await
        .expect("a page");
    let numbers = page
        .items
        .iter()
        .map(|issue| issue.number)
        .collect::<Vec<_>>();
    assert_eq!(numbers, vec![1, 3]);
    assert_eq!(page.next_cursor.as_deref(), Some("page=2"));
    let sent = harness.sent();
    let url = url::Url::parse(&sent[0].uri).expect("a URL");
    assert_eq!(url.path(), "/search/issues");
    let query = url
        .query_pairs()
        .find(|(name, _)| name == "q")
        .map(|(_, value)| value.into_owned());
    assert_eq!(
        query.as_deref(),
        Some(
            "repo:octo/repo is:issue is:closed label:\"good first issue\" assignee:octocat (crash on start)"
        )
    );
}

#[gpui::test]
async fn project_details_post_their_document_and_read_the_node(cx: &mut TestAppContext) {
    let details = json!({"data": {"node": {
        "fields": {"nodes": [{"__typename": "ProjectV2Field", "id": "F_title", "name": "Title", "dataType": "TITLE"}]},
        "views": {"nodes": []}
    }}});
    let missing = json!({"data": {"node": null}});
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(200, &[], details.to_string()),
            answer(200, &[], missing.to_string()),
        ],
        cx,
    );
    let read = harness
        .backend
        .project_details("PVT_1".to_string())
        .await
        .expect("details");
    assert_eq!(read.fields.len(), 1);
    let sent = harness.sent();
    assert_eq!(sent[0].body["query"], json!(documents::PROJECT_DETAILS));
    assert_eq!(sent[0].body["variables"], json!({"projectId": "PVT_1"}));
    let error = harness
        .backend
        .project_details("PVT_gone".to_string())
        .await
        .expect_err("a project the token cannot see");
    assert_eq!(error, GithubError::NotFound);
}

#[gpui::test]
async fn the_repository_permission_is_read_from_its_rest_answer(cx: &mut TestAppContext) {
    let harness = set_up(
        TokenSource::Keychain,
        vec![
            answer(
                200,
                &[],
                r#"{"private":false,"permissions":{"admin":false,"maintain":false,"push":false,"triage":true,"pull":true}}"#,
            ),
            answer(
                200,
                &[],
                r#"{"private":false,"permissions":{"admin":false,"push":false,"pull":true}}"#,
            ),
            answer(200, &[], r#"{"private":false}"#),
        ],
        cx,
    );
    let backend = &harness.backend;
    assert_eq!(
        backend.repository_permission(&repository()).await,
        Ok(Some(crate::model::RepositoryPermission::Triage))
    );
    assert_eq!(
        backend.repository_permission(&repository()).await,
        Ok(Some(crate::model::RepositoryPermission::Read))
    );
    assert_eq!(
        backend.repository_permission(&repository()).await,
        Ok(None),
        "no `permissions` is not read as read-only"
    );
}

#[gpui::test]
async fn project_field_writes_post_their_mutations_in_queue_order(cx: &mut TestAppContext) {
    let done =
        json!({"data": {"updateProjectV2ItemFieldValue": {"projectV2Item": {"id": "PVTI_1"}}}});
    let cleared =
        json!({"data": {"clearProjectV2ItemFieldValue": {"projectV2Item": {"id": "PVTI_1"}}}});
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(200, &[], done.to_string()),
            answer(200, &[], done.to_string()),
            answer(200, &[], cleared.to_string()),
        ],
        cx,
    );
    let option = cx
        .background_executor
        .spawn(harness.backend.set_project_item_field(
            "PVT_1".to_string(),
            "PVTI_1".to_string(),
            "F_status".to_string(),
            Some(ProjectFieldInput::SingleSelect {
                option_id: "done".to_string(),
            }),
        ));
    let iteration = cx
        .background_executor
        .spawn(harness.backend.set_project_item_field(
            "PVT_1".to_string(),
            "PVTI_2".to_string(),
            "F_sprint".to_string(),
            Some(ProjectFieldInput::Iteration {
                iteration_id: "it_2".to_string(),
            }),
        ));
    let clear = cx
        .background_executor
        .spawn(harness.backend.set_project_item_field(
            "PVT_1".to_string(),
            "PVTI_1".to_string(),
            "F_status".to_string(),
            None,
        ));
    cx.run_until_parked();
    assert_eq!(harness.sent().len(), 1, "the second write waits its turn");
    cx.executor().advance_clock(MIN_WRITE_INTERVAL);
    cx.run_until_parked();
    cx.executor().advance_clock(MIN_WRITE_INTERVAL);
    cx.run_until_parked();
    let sent = harness.sent();
    assert_eq!(sent.len(), 3);
    assert_eq!(
        sent[0].body["query"],
        json!(documents::UPDATE_PROJECT_ITEM_FIELD)
    );
    assert_eq!(
        sent[0].body["variables"],
        json!({"projectId": "PVT_1", "itemId": "PVTI_1", "fieldId": "F_status", "value": {"singleSelectOptionId": "done"}})
    );
    assert_eq!(
        sent[1].body["variables"],
        json!({"projectId": "PVT_1", "itemId": "PVTI_2", "fieldId": "F_sprint", "value": {"iterationId": "it_2"}})
    );
    assert_eq!(
        sent[2].body["query"],
        json!(documents::CLEAR_PROJECT_ITEM_FIELD)
    );
    assert_eq!(
        sent[2].body["variables"],
        json!({"projectId": "PVT_1", "itemId": "PVTI_1", "fieldId": "F_status"})
    );
    option.await.expect("option set");
    iteration.await.expect("iteration set");
    clear.await.expect("cleared");
}

#[gpui::test]
async fn a_project_position_write_names_the_item_before_and_reports_a_scope_error(
    cx: &mut TestAppContext,
) {
    let moved = json!({"data": {"updateProjectV2ItemPosition": {"clientMutationId": null}}});
    let refused = json!({"data": null, "errors": [{"type": "INSUFFICIENT_SCOPES", "message": "Your token has not been granted the required scopes to execute this query. The 'updateProjectV2ItemPosition' field requires one of the following scopes: ['project'], but your token has only been granted the: ['read:project'] scopes."}]});
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(200, &[], moved.to_string()),
            answer(200, &[], refused.to_string()),
        ],
        cx,
    );
    harness
        .backend
        .move_project_item(
            "PVT_1".to_string(),
            "PVTI_3".to_string(),
            Some("PVTI_1".to_string()),
        )
        .await
        .expect("moved");
    let sent = harness.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].body["query"],
        json!(documents::UPDATE_PROJECT_ITEM_POSITION)
    );
    assert_eq!(
        sent[0].body["variables"],
        json!({"projectId": "PVT_1", "itemId": "PVTI_3", "afterId": "PVTI_1"})
    );
    cx.executor().advance_clock(MIN_WRITE_INTERVAL);
    let error = harness
        .backend
        .move_project_item("PVT_1".to_string(), "PVTI_3".to_string(), None)
        .await
        .expect_err("read:project cannot write");
    assert_eq!(
        error,
        GithubError::MissingScope {
            scope: "project".to_string()
        }
    );
    assert_eq!(harness.sent()[1].body["variables"]["afterId"], Value::Null);
}

#[gpui::test]
async fn typed_project_field_values_are_sent_as_text_number_and_date(cx: &mut TestAppContext) {
    let updated =
        json!({"data": {"updateProjectV2ItemFieldValue": {"projectV2Item": {"id": "PVTI_1"}}}});
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(200, &[], updated.to_string()),
            answer(200, &[], updated.to_string()),
            answer(200, &[], updated.to_string()),
        ],
        cx,
    );
    let inputs = [
        (
            "F_notes",
            ProjectFieldInput::Text {
                text: "Ship it".to_string(),
            },
            json!({"text": "Ship it"}),
        ),
        (
            "F_points",
            ProjectFieldInput::Number { number: 2.5 },
            json!({"number": 2.5}),
        ),
        (
            "F_due",
            ProjectFieldInput::Date {
                date: "2026-12-01".to_string(),
            },
            json!({"date": "2026-12-01"}),
        ),
    ];
    for (position, (field_id, input, value)) in inputs.into_iter().enumerate() {
        if position > 0 {
            cx.executor().advance_clock(MIN_WRITE_INTERVAL);
        }
        harness
            .backend
            .set_project_item_field(
                "PVT_1".to_string(),
                "PVTI_1".to_string(),
                field_id.to_string(),
                Some(input),
            )
            .await
            .expect("set");
        let sent = harness.sent();
        let last = sent.last().expect("a request was sent");
        assert_eq!(
            last.body["query"],
            json!(documents::UPDATE_PROJECT_ITEM_FIELD)
        );
        assert_eq!(
            last.body["variables"],
            json!({"projectId": "PVT_1", "itemId": "PVTI_1", "fieldId": field_id, "value": value})
        );
    }
}

#[gpui::test]
async fn adding_and_removing_a_project_item_post_their_mutations(cx: &mut TestAppContext) {
    let added = json!({"data": {"addProjectV2ItemById": {"item": {"id": "PVTI_new"}}}});
    let deleted = json!({"data": {"deleteProjectV2Item": {"deletedItemId": "PVTI_new"}}});
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(200, &[], added.to_string()),
            answer(200, &[], deleted.to_string()),
        ],
        cx,
    );
    let item_id = harness
        .backend
        .add_project_item("PVT_1".to_string(), "I_12".to_string())
        .await
        .expect("added");
    assert_eq!(item_id, "PVTI_new");
    cx.executor().advance_clock(MIN_WRITE_INTERVAL);
    harness
        .backend
        .delete_project_item("PVT_1".to_string(), "PVTI_new".to_string())
        .await
        .expect("removed");
    let sent = harness.sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].body["query"], json!(documents::ADD_PROJECT_ITEM));
    assert_eq!(
        sent[0].body["variables"],
        json!({"projectId": "PVT_1", "contentId": "I_12"})
    );
    assert_eq!(sent[1].body["query"], json!(documents::DELETE_PROJECT_ITEM));
    assert_eq!(
        sent[1].body["variables"],
        json!({"projectId": "PVT_1", "itemId": "PVTI_new"})
    );
}

fn search_query(sent: &Sent) -> String {
    url::Url::parse(&sent.uri)
        .expect("a URL")
        .query_pairs()
        .find(|(name, _)| name == "q")
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default()
}

fn empty_search() -> Canned {
    answer(
        200,
        &[],
        json!({"total_count": 0, "incomplete_results": false, "items": []}).to_string(),
    )
}

#[gpui::test]
async fn a_label_named_dot_or_dot_dot_is_never_sent(cx: &mut TestAppContext) {
    let harness = set_up(
        TokenSource::GhCli,
        vec![answer(200, &[], "[]"), answer(200, &[], "[]")],
        cx,
    );
    for label in ["..", "."] {
        let result = harness
            .backend
            .remove_label(&repository(), 4, label.to_string())
            .await;
        assert!(
            matches!(result, Err(GithubError::Rejected { .. })),
            "{label}: {result:?}"
        );
    }
    assert!(harness.sent().is_empty(), "{:?}", harness.sent());
}

#[gpui::test]
async fn project_items_keep_the_rest_when_one_item_is_forbidden(cx: &mut TestAppContext) {
    let body = json!({
        "data": {"node": {"items": {
            "nodes": [
                {"id": "PVTI_1", "type": "ISSUE", "content": {"__typename": "Issue", "number": 7, "title": "Bug", "repository": {"nameWithOwner": "octo/repo"}}},
                {"id": "PVTI_2", "type": "ISSUE", "content": null}
            ],
            "pageInfo": {"hasNextPage": false, "endCursor": null}
        }}},
        "errors": [{"type": "FORBIDDEN", "path": ["node", "items", "nodes", 1, "content"], "message": "Resource not accessible by integration"}]
    });
    let harness = set_up(
        TokenSource::GhCli,
        vec![answer(200, &[], body.to_string())],
        cx,
    );
    let page = harness
        .backend
        .list_project_items("PVT_1".to_string(), None)
        .await
        .expect("the items GitHub showed");
    let contents = page
        .items
        .into_iter()
        .map(|item| item.content)
        .collect::<Vec<_>>();
    assert_eq!(
        contents,
        vec![
            crate::model::ProjectItemContent::Issue {
                number: 7,
                title: "Bug".to_string(),
                repository: "octo/repo".to_string(),
            },
            crate::model::ProjectItemContent::Redacted,
        ]
    );
}

#[gpui::test]
async fn a_shallow_or_scope_graphql_error_still_fails_the_read(cx: &mut TestAppContext) {
    let shallow = json!({
        "data": {"node": null},
        "errors": [{"type": "FORBIDDEN", "path": ["node"], "message": "Resource not accessible by integration"}]
    });
    let scope = json!({
        "data": {"node": {"items": {"nodes": [null], "pageInfo": {"hasNextPage": false}}}},
        "errors": [{"type": "INSUFFICIENT_SCOPES", "path": ["node", "items", "nodes", 0, "content"], "message": "requires one of the following scopes: ['read:project']"}]
    });
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(200, &[], shallow.to_string()),
            answer(200, &[], scope.to_string()),
        ],
        cx,
    );
    let shallow = harness
        .backend
        .list_project_items("PVT_1".to_string(), None)
        .await;
    assert!(
        matches!(shallow, Err(GithubError::Forbidden { .. })),
        "{shallow:?}"
    );
    let scope = harness
        .backend
        .list_project_items("PVT_1".to_string(), None)
        .await;
    assert_eq!(
        scope.err(),
        Some(GithubError::MissingScope {
            scope: "read:project".to_string()
        })
    );
}

#[gpui::test]
async fn rest_pages_follow_an_after_cursor_and_nothing_else(cx: &mut TestAppContext) {
    let link = "<https://api.github.com/repositories/1/issues?per_page=50&q=evil&page=3&after=Y3Vy%3D>; rel=\"next\"";
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(
                200,
                &[("link", link)],
                json!([issue_json(1, false)]).to_string(),
            ),
            answer(200, &[], json!([issue_json(2, false)]).to_string()),
        ],
        cx,
    );
    let first = harness
        .backend
        .list_issues(&repository(), &IssueFilter::default(), None)
        .await
        .expect("a page");
    assert_eq!(first.next_cursor.as_deref(), Some("after=Y3Vy%3D"));
    harness
        .backend
        .list_issues(&repository(), &IssueFilter::default(), first.next_cursor)
        .await
        .expect("the next page");
    let sent = harness.sent();
    let query = url::Url::parse(&sent[1].uri)
        .expect("a URL")
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    assert!(
        query.contains(&("after".to_string(), "Y3Vy=".to_string())),
        "{query:?}"
    );
    assert!(
        query.iter().all(|(name, _)| name != "page" && name != "q"),
        "{query:?}"
    );
    let refused = harness
        .backend
        .list_issues(
            &repository(),
            &IssueFilter::default(),
            Some("q=evil".to_string()),
        )
        .await;
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(harness.sent().len(), 2);
}

#[gpui::test]
async fn search_text_cannot_widen_the_repository(cx: &mut TestAppContext) {
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            empty_search(),
            empty_search(),
            empty_search(),
            empty_search(),
        ],
        cx,
    );
    let filter = IssueFilter {
        state: IssueStateFilter::All,
        ..IssueFilter::default()
    };
    for text in [
        "(repo:x/y) foo",
        "foo OR repo:x/y",
        "x) OR (stars:>1",
        "OR repo:x/y bar",
    ] {
        harness
            .backend
            .search_issues(&repository(), &filter, text, None)
            .await
            .expect("a page");
    }
    let queries = harness.sent().iter().map(search_query).collect::<Vec<_>>();
    assert_eq!(
        queries,
        vec![
            "repo:octo/repo is:issue (foo)",
            "repo:octo/repo is:issue (foo)",
            "repo:octo/repo is:issue (x OR stars:>1)",
            "repo:octo/repo is:issue (bar)",
        ]
    );
}

/// A dropped qualifier can take one half of a quoted phrase with it; the
/// half left behind must not open a quote the group never closes.
#[gpui::test]
async fn a_quote_stranded_by_a_dropped_qualifier_is_removed(cx: &mut TestAppContext) {
    let harness = set_up(
        TokenSource::GhCli,
        vec![empty_search(), empty_search(), empty_search()],
        cx,
    );
    let filter = IssueFilter {
        state: IssueStateFilter::All,
        ..IssueFilter::default()
    };
    for text in ["\"a repo:x\"", "\"a repo:x\" b", "\"a b\" repo:x"] {
        harness
            .backend
            .search_issues(&repository(), &filter, text, None)
            .await
            .expect("a page");
    }
    let queries = harness.sent().iter().map(search_query).collect::<Vec<_>>();
    assert_eq!(
        queries,
        vec![
            "repo:octo/repo is:issue (a)",
            "repo:octo/repo is:issue (a b)",
            "repo:octo/repo is:issue (\"a b\")",
        ]
    );
}

#[gpui::test]
async fn search_drops_results_from_other_repositories(cx: &mut TestAppContext) {
    let mut foreign = issue_json(5, false);
    foreign["repository_url"] = json!("https://api.github.com/repos/evil/other");
    let mut unnamed = issue_json(6, false);
    unnamed
        .as_object_mut()
        .expect("an object")
        .remove("repository_url");
    let body = json!({
        "total_count": 3,
        "incomplete_results": false,
        "items": [issue_json(1, false), foreign, unnamed]
    });
    let harness = set_up(
        TokenSource::GhCli,
        vec![answer(200, &[], body.to_string())],
        cx,
    );
    let page = harness
        .backend
        .search_issues(&repository(), &IssueFilter::default(), "crash", None)
        .await
        .expect("a page");
    let numbers = page
        .items
        .iter()
        .map(|issue| issue.number)
        .collect::<Vec<_>>();
    assert_eq!(numbers, vec![1]);
}

#[gpui::test]
async fn a_login_that_could_add_a_qualifier_is_refused(cx: &mut TestAppContext) {
    let harness = set_up(TokenSource::GhCli, vec![empty_search()], cx);
    let filter = IssueFilter {
        assignee: Some("octocat repo:evil/other".to_string()),
        ..IssueFilter::default()
    };
    let result = harness
        .backend
        .search_issues(&repository(), &filter, "crash", None)
        .await;
    assert!(
        matches!(result, Err(GithubError::Rejected { .. })),
        "{result:?}"
    );
    assert!(harness.sent().is_empty());
}

#[gpui::test]
async fn a_huge_retry_after_holds_writes_for_five_minutes_at_most(cx: &mut TestAppContext) {
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(
                403,
                &[("retry-after", "86400")],
                r#"{"message":"You have exceeded a secondary rate limit."}"#,
            ),
            answer(201, &[], comment_json(2).to_string()),
        ],
        cx,
    );
    let first = cx.background_executor.spawn(harness.backend.add_comment(
        &repository(),
        1,
        "one".to_string(),
    ));
    let second = cx.background_executor.spawn(harness.backend.add_comment(
        &repository(),
        1,
        "two".to_string(),
    ));
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(299));
    cx.run_until_parked();
    assert_eq!(harness.sent().len(), 1);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(harness.sent().len(), 2);
    let error = first.await.expect_err("rate limited");
    assert!(
        error.to_string().contains("GitHub asked to wait 86400 s"),
        "{error}"
    );
    second.await.expect("second comment");
}

#[gpui::test]
async fn a_rate_limited_graphql_answer_reads_its_reset_from_the_headers(cx: &mut TestAppContext) {
    let body = json!({"errors": [{"type": "RATE_LIMITED", "message": "API rate limit exceeded for user ID 1."}]});
    let harness = set_up(
        TokenSource::GhCli,
        vec![answer(
            200,
            &[
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "1764000000"),
            ],
            body.to_string(),
        )],
        cx,
    );
    let error = harness
        .backend
        .list_projects(&repository(), None)
        .await
        .expect_err("rate limited");
    let expected = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_764_000_000);
    assert!(
        matches!(error, GithubError::RateLimited { reset_at: Some(at), .. } if at == expected),
        "{error:?}"
    );
}

#[gpui::test]
async fn project_owners_offer_the_organization_only_when_it_owns_the_repository(
    cx: &mut TestAppContext,
) {
    let owned_by = |typename: &str, login: &str| {
        json!({"data": {
            "viewer": {"id": "U_me", "login": "octocat", "avatarUrl": "https://avatars.githubusercontent.com/u/1?s=40"},
            "repository": {"id": "R_1", "owner": {"__typename": typename, "id": format!("O_{login}"), "login": login, "avatarUrl": "https://avatars.githubusercontent.com/u/2?s=40"}},
        }})
        .to_string()
    };
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(200, &[], owned_by("Organization", "octo")),
            answer(200, &[], owned_by("User", "someone")),
        ],
        cx,
    );
    let viewer = crate::projects::ProjectOwner {
        id: "U_me".to_string(),
        login: "octocat".to_string(),
        avatar_url: Some("https://avatars.githubusercontent.com/u/1?s=40".to_string()),
    };
    let organization_repository = harness.backend.project_owners(&repository()).await;
    assert_eq!(
        organization_repository,
        Ok(crate::projects::ProjectOwners {
            repository_id: "R_1".to_string(),
            viewer: viewer.clone(),
            organization: Some(crate::projects::ProjectOwner {
                id: "O_octo".to_string(),
                login: "octo".to_string(),
                avatar_url: Some("https://avatars.githubusercontent.com/u/2?s=40".to_string()),
            }),
        })
    );
    let personal_repository = harness.backend.project_owners(&repository()).await;
    assert_eq!(
        personal_repository,
        Ok(crate::projects::ProjectOwners {
            repository_id: "R_1".to_string(),
            viewer,
            organization: None,
        })
    );
    let sent = harness.sent();
    assert_eq!(sent[0].body["query"], json!(documents::PROJECT_OWNERS));
    assert_eq!(
        sent[0].body["variables"],
        json!({"owner": "octo", "name": "repo"})
    );
}

#[gpui::test]
async fn creating_a_project_waits_its_turn_and_sends_owner_title_and_repository(
    cx: &mut TestAppContext,
) {
    let deleted = json!({"data": {"deleteProjectV2Item": {"deletedItemId": "PVTI_1"}}});
    let created = json!({"data": {"createProjectV2": {"projectV2": {
        "id": "PVT_new", "number": 7, "title": "Launch", "url": "https://github.com/orgs/octo/projects/7", "closed": false,
    }}}});
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(200, &[], deleted.to_string()),
            answer(200, &[], created.to_string()),
        ],
        cx,
    );
    let earlier = cx.background_executor.spawn(
        harness
            .backend
            .delete_project_item("PVT_1".to_string(), "PVTI_1".to_string()),
    );
    let create = cx.background_executor.spawn(harness.backend.create_project(
        "O_octo".to_string(),
        "Launch".to_string(),
        "R_1".to_string(),
    ));
    cx.run_until_parked();
    assert_eq!(harness.sent().len(), 1, "the create waits its turn");
    cx.executor().advance_clock(MIN_WRITE_INTERVAL);
    cx.run_until_parked();
    let sent = harness.sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].body["query"], json!(documents::CREATE_PROJECT));
    assert_eq!(
        sent[1].body["variables"],
        json!({"ownerId": "O_octo", "title": "Launch", "repositoryId": "R_1"})
    );
    earlier.await.expect("removed");
    assert_eq!(
        create.await,
        Ok(crate::model::ProjectV2 {
            id: "PVT_new".to_string(),
            number: 7,
            title: "Launch".to_string(),
            url: "https://github.com/orgs/octo/projects/7".to_string(),
            closed: false,
        })
    );
}

#[gpui::test]
async fn a_refused_project_create_keeps_githubs_message(cx: &mut TestAppContext) {
    let refused = json!({"errors": [{
        "type": "FORBIDDEN",
        "message": "Members of this organization cannot create projects.",
    }]});
    let harness = set_up(
        TokenSource::GhCli,
        vec![answer(200, &[], refused.to_string())],
        cx,
    );
    let error = harness
        .backend
        .create_project(
            "O_octo".to_string(),
            "Launch".to_string(),
            "R_1".to_string(),
        )
        .await;
    assert_eq!(
        error,
        Err(GithubError::Forbidden {
            message: "Members of this organization cannot create projects.".to_string(),
        })
    );
}

#[gpui::test]
async fn assignable_users_post_their_document_and_read_name_and_avatar(cx: &mut TestAppContext) {
    let users = json!({"data": {"repository": {"assignableUsers": {"nodes": [
        {"login": "octocat", "name": "The Octocat",
         "avatarUrl": "https://avatars.githubusercontent.com/u/583231?s=40&v=4"},
        null,
        {"login": "hubot", "name": null, "avatarUrl": "https://avatars.githubusercontent.com/u/480938?s=40"}
    ]}}}});
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(200, &[], users.to_string()),
            answer(200, &[], users.to_string()),
        ],
        cx,
    );
    let found = harness
        .backend
        .assignable_users(&repository(), "oct")
        .await
        .expect("read");
    assert_eq!(
        found,
        vec![
            crate::model::GithubUser {
                login: "octocat".to_string(),
                name: Some("The Octocat".to_string()),
                avatar_url: Some(
                    "https://avatars.githubusercontent.com/u/583231?s=40&v=4".to_string()
                ),
            },
            crate::model::GithubUser {
                login: "hubot".to_string(),
                name: None,
                avatar_url: Some("https://avatars.githubusercontent.com/u/480938?s=40".to_string()),
            },
        ]
    );
    harness
        .backend
        .assignable_users(&repository(), "  ")
        .await
        .expect("read");
    let sent = harness.sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(
        sent[0].body["query"],
        json!(documents::REPOSITORY_ASSIGNABLE_USERS)
    );
    assert_eq!(
        sent[0].body["variables"],
        json!({"owner": "octo", "name": "repo", "query": "oct"})
    );
    assert_eq!(
        sent[1].body["variables"],
        json!({"owner": "octo", "name": "repo", "query": null}),
        "an empty query asks for the first assignable users"
    );
}

#[gpui::test]
async fn an_issue_keeps_its_assignees_avatar_urls(cx: &mut TestAppContext) {
    let mut issue = issue_json(1, false);
    issue["assignees"] = json!([
        {"login": "octocat", "avatar_url": "https://avatars.githubusercontent.com/u/583231?v=4"},
        {"login": "ghost"}
    ]);
    let harness = set_up(
        TokenSource::GhCli,
        vec![answer(200, &[], json!([issue]).to_string())],
        cx,
    );
    let page = harness
        .backend
        .list_issues(&repository(), &IssueFilter::default(), None)
        .await
        .expect("a page");
    let issue = &page.items[0];
    assert_eq!(issue.assignees, vec!["octocat", "ghost"]);
    assert_eq!(
        issue.assignee_avatars.get("octocat").map(String::as_str),
        Some("https://avatars.githubusercontent.com/u/583231?v=4")
    );
    assert_eq!(issue.assignee_avatars.get("ghost"), None);
}

#[gpui::test]
async fn the_authorization_header_is_marked_sensitive(cx: &mut TestAppContext) {
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(200, &[], r#"{"login":"octocat"}"#),
            answer(201, &[], comment_json(1).to_string()),
        ],
        cx,
    );
    harness.backend.viewer().await.expect("a viewer");
    harness
        .backend
        .add_comment(&repository(), 1, "hello".to_string())
        .await
        .expect("a comment");
    let sent = harness.sent();
    assert!(
        sent.iter()
            .all(|request| request.authorization_is_sensitive)
    );
}

#[gpui::test]
async fn a_write_that_gets_no_answer_may_have_been_applied(cx: &mut TestAppContext) {
    let harness = set_up(TokenSource::GhCli, vec![Canned::Hang, Canned::Hang], cx);
    let comment = cx.background_executor.spawn(harness.backend.add_comment(
        &repository(),
        1,
        "hello".to_string(),
    ));
    cx.run_until_parked();
    cx.executor().advance_clock(REQUEST_TIMEOUT);
    cx.run_until_parked();
    let error = comment.await.expect_err("timed out");
    assert!(
        matches!(error, GithubError::OutcomeUnknown { ref message } if message.contains("30 seconds")),
        "{error:?}"
    );
    assert!(error.to_string().contains("Reload and check"));

    cx.executor().advance_clock(MIN_WRITE_INTERVAL);
    let merge = cx
        .background_executor
        .spawn(harness.backend.merge_pull_request(
            "PR_1".to_string(),
            MergeMethod::Squash,
            "abc".to_string(),
        ));
    cx.run_until_parked();
    cx.executor().advance_clock(REQUEST_TIMEOUT);
    cx.run_until_parked();
    assert!(matches!(
        merge.await,
        Err(GithubError::OutcomeUnknown { .. })
    ));
}

#[gpui::test]
async fn a_write_whose_connection_drops_may_have_been_applied(cx: &mut TestAppContext) {
    let harness = set_up(TokenSource::GhCli, vec![Canned::Fail, Canned::Fail], cx);
    let error = harness
        .backend
        .add_comment(&repository(), 1, "hello".to_string())
        .await
        .expect_err("dropped");
    assert!(
        matches!(error, GithubError::OutcomeUnknown { ref message } if message.contains("connection reset")),
        "{error:?}"
    );
    cx.executor().advance_clock(MIN_WRITE_INTERVAL);
    let error = harness
        .backend
        .merge_pull_request("PR_1".to_string(), MergeMethod::Squash, "abc".to_string())
        .await
        .expect_err("dropped");
    assert!(
        matches!(error, GithubError::OutcomeUnknown { .. }),
        "{error:?}"
    );
}

#[gpui::test]
async fn a_read_whose_connection_drops_stays_a_network_error(cx: &mut TestAppContext) {
    let harness = set_up(TokenSource::GhCli, vec![Canned::Fail, Canned::Fail], cx);
    let error = harness
        .backend
        .issue(&repository(), 1)
        .await
        .expect_err("dropped");
    assert!(matches!(error, GithubError::Network { .. }), "{error:?}");
    let error = harness
        .backend
        .list_projects(&repository(), None)
        .await
        .expect_err("dropped");
    assert!(
        matches!(error, GithubError::Network { .. }),
        "a GraphQL query is a POST but reads nothing it changes: {error:?}"
    );
}

#[gpui::test]
async fn a_write_that_got_an_error_answer_is_not_unknown(cx: &mut TestAppContext) {
    let harness = set_up(
        TokenSource::GhCli,
        vec![answer(500, &[], r#"{"message":"Server Error"}"#)],
        cx,
    );
    let error = harness
        .backend
        .add_comment(&repository(), 1, "hello".to_string())
        .await
        .expect_err("refused");
    assert!(
        matches!(error, GithubError::UnexpectedResponse { .. }),
        "{error:?}"
    );
}

fn viewer_body_of_size(size: usize) -> String {
    let frame = r#"{"login":"octocat","padding":""}"#.len();
    let body = format!(
        r#"{{"login":"octocat","padding":"{}"}}"#,
        "x".repeat(size - frame)
    );
    assert_eq!(body.len(), size);
    body
}

#[gpui::test]
async fn an_answer_of_exactly_the_size_cap_is_read(cx: &mut TestAppContext) {
    let body = viewer_body_of_size(MAX_RESPONSE_BYTES as usize);
    let harness = set_up(TokenSource::GhCli, vec![answer(200, &[], body)], cx);
    let viewer = harness.backend.viewer().await.expect("a viewer");
    assert_eq!(viewer.login, "octocat");
}

#[gpui::test]
async fn an_answer_over_the_size_cap_is_refused_rather_than_cut(cx: &mut TestAppContext) {
    let body = viewer_body_of_size(MAX_RESPONSE_BYTES as usize + 1);
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(200, &[], body.clone()),
            answer(201, &[], body.replace("octocat", "octocaz")),
        ],
        cx,
    );
    let error = harness.backend.viewer().await.expect_err("too large");
    assert!(
        matches!(error, GithubError::UnexpectedResponse { ref message } if message.contains("too large")),
        "{error:?}"
    );
    let error = harness
        .backend
        .add_comment(&repository(), 1, "hello".to_string())
        .await
        .expect_err("too large");
    assert!(
        matches!(error, GithubError::OutcomeUnknown { ref message } if message.contains("too large")),
        "a write that was applied but cannot be read back is unknown: {error:?}"
    );
}

#[gpui::test]
async fn a_graphql_forbidden_with_sso_headers_asks_to_authorize_the_token(cx: &mut TestAppContext) {
    let forbidden = json!({"data": null, "errors": [{
        "type": "FORBIDDEN",
        "message": "Resource protected by organization SAML enforcement."
    }]})
    .to_string();
    let sso = (
        "x-github-sso",
        "required; url=https://github.com/orgs/acme/sso?authorization_request=abc",
    );
    let harness = set_up(
        TokenSource::GhCli,
        vec![
            answer(200, &[sso], forbidden.clone()),
            answer(200, &[sso], forbidden.clone()),
            answer(200, &[], forbidden),
        ],
        cx,
    );
    let expected = GithubError::SsoRequired {
        authorize_url: "https://github.com/orgs/acme/sso?authorization_request=abc".to_string(),
    };
    let error = harness
        .backend
        .list_projects(&repository(), None)
        .await
        .expect_err("sso");
    assert_eq!(error, expected, "a query");
    cx.executor().advance_clock(MIN_WRITE_INTERVAL);
    let error = harness
        .backend
        .merge_pull_request("PR_1".to_string(), MergeMethod::Squash, "abc".to_string())
        .await
        .expect_err("sso");
    assert_eq!(error, expected, "a mutation");
    cx.executor().advance_clock(MIN_WRITE_INTERVAL);
    let error = harness
        .backend
        .list_projects(&repository(), None)
        .await
        .expect_err("forbidden");
    assert!(
        matches!(error, GithubError::Forbidden { .. }),
        "without the header it stays a plain refusal: {error:?}"
    );
}

#[gpui::test]
async fn search_text_is_cut_to_a_bounded_length(cx: &mut TestAppContext) {
    let harness = set_up(TokenSource::GhCli, vec![empty_search(), empty_search()], cx);
    let filter = IssueFilter {
        state: IssueStateFilter::All,
        ..IssueFilter::default()
    };
    let long_text = "word ".repeat(2000);
    let exact_text = format!("{} end", "a".repeat(SEARCH_TEXT_MAX_CHARACTERS - 4));
    for text in [long_text.as_str(), exact_text.as_str()] {
        harness
            .backend
            .search_issues(&repository(), &filter, text, None)
            .await
            .expect("a page");
    }
    let queries = harness.sent().iter().map(search_query).collect::<Vec<_>>();
    let group = |query: &str| {
        query
            .strip_prefix("repo:octo/repo is:issue (")
            .and_then(|rest| rest.strip_suffix(')'))
            .map(str::to_string)
            .expect("a text group")
    };
    let long_group = group(&queries[0]);
    assert!(long_group.starts_with("word word"));
    assert!(
        long_group.chars().count() <= SEARCH_TEXT_MAX_CHARACTERS,
        "{} characters",
        long_group.chars().count()
    );
    assert_eq!(
        group(&queries[1]),
        exact_text,
        "text of exactly the cap is kept whole"
    );
}

#[gpui::test]
async fn a_label_with_a_comma_cannot_be_listed_by_the_rest_filter(cx: &mut TestAppContext) {
    let harness = set_up(TokenSource::GhCli, vec![answer(200, &[], "[]")], cx);
    let filter = IssueFilter {
        labels: vec!["bug".to_string(), "a,b".to_string()],
        ..IssueFilter::default()
    };
    let error = harness
        .backend
        .list_issues(&repository(), &filter, None)
        .await
        .expect_err("refused");
    assert!(
        matches!(error, GithubError::Rejected { ref message } if message.contains("a,b")),
        "{error:?}"
    );
    assert!(
        harness.sent().is_empty(),
        "nothing is sent that GitHub would read as two labels"
    );
}

#[gpui::test]
async fn a_write_that_never_connected_is_a_plain_network_error(cx: &mut TestAppContext) {
    let kinds = [
        std::io::ErrorKind::ConnectionRefused,
        std::io::ErrorKind::HostUnreachable,
        std::io::ErrorKind::NetworkUnreachable,
        std::io::ErrorKind::AddrNotAvailable,
    ];
    let harness = set_up(
        TokenSource::GhCli,
        kinds
            .iter()
            .map(|kind| Canned::FailToConnect(*kind))
            .collect(),
        cx,
    );
    for kind in kinds {
        cx.executor().advance_clock(MIN_WRITE_INTERVAL);
        let error = harness
            .backend
            .add_comment(&repository(), 1, "hello".to_string())
            .await
            .expect_err("unreachable");
        assert!(
            matches!(error, GithubError::Network { .. }),
            "{kind:?}: {error:?}"
        );
        assert!(
            !error.to_string().contains("may already have applied"),
            "a safe retry must not be discouraged: {error}"
        );
    }
}

#[gpui::test]
async fn a_write_cut_off_while_reading_the_answer_stays_unknown(cx: &mut TestAppContext) {
    let harness = set_up(
        TokenSource::GhCli,
        vec![Canned::BrokenBody(std::io::ErrorKind::ConnectionRefused)],
        cx,
    );
    let error = harness
        .backend
        .add_comment(&repository(), 1, "hello".to_string())
        .await
        .expect_err("cut off");
    assert!(
        matches!(error, GithubError::OutcomeUnknown { .. }),
        "the request was sent, so an io kind alone proves nothing: {error:?}"
    );
}

#[test]
fn a_real_refused_connection_is_recognised_as_never_sent() {
    use http_client::HttpClient as _;

    let address = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("bind a local port");
    let client = reqwest_client::ReqwestClient::new();
    let request = http_client::Request::builder()
        .uri(format!("http://{address}/"))
        .body(AsyncBody::empty())
        .expect("request");
    let error = futures::executor::block_on(client.send(request))
        .err()
        .expect("nothing listens");
    assert!(
        crate::http_transport::failed_before_the_request_was_sent(&error),
        "{error:?}"
    );
}
