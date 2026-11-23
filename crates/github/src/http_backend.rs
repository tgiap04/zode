use std::fmt;
use std::sync::Arc;

use futures::FutureExt as _;
use gpui::App;
use http_client::{HttpClient, Method};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use url::Url;

use crate::backend::{GithubBackend, GithubFuture};
use crate::error::{GithubError, GithubResult, shorten_message, unexpected};
use crate::graphql::{self, documents};
use crate::graphql_projects;
use crate::http_transport::{RestAnswer, Transport, api_url};
use crate::model::{
    Comment, GithubUser, Issue, IssueFilter, IssueState, IssueStateFilter, IssueType, IssueUpdate,
    Label, Milestone, NewIssue, Page, ParentIssue, ProjectV2, ProjectV2Item, PullRequest,
    PullRequestStateFilter, RateLimits, RepositoryId, RepositoryPermission, RepositoryVisibility,
    SubIssuePosition, Viewer,
};
use crate::projects::{ProjectDetails, ProjectFieldInput, ProjectOwners};
use crate::pull_requests::{MergeMethod, PullRequestDetails};
use crate::rest::{self, PER_PAGE};
use crate::rest_payloads::{
    CommentPayload, IssuePayload, LabelPayload, MilestonePayload, RepositoryPayload,
    SearchIssuesPayload, UserPayload, state_reason_name,
};
use crate::token::{GithubToken, TokenSource};
use crate::write_queue::{WriteQueues, WriteTicket};

/// Talks to api.github.com with one token. There is no way to build one
/// without a token, so nothing is sent while signed out.
#[derive(Clone)]
pub struct HttpBackend {
    transport: Arc<Transport>,
}

impl HttpBackend {
    pub fn new(http_client: Arc<dyn HttpClient>, token: GithubToken, cx: &mut App) -> Self {
        let write_queue = WriteQueues::queue_for(token.identity(), cx);
        let executor = cx.background_executor().clone();
        Self {
            transport: Arc::new(Transport::new(http_client, token, executor, write_queue)),
        }
    }

    pub fn token_source(&self) -> TokenSource {
        self.transport.token.source()
    }

    /// Every call runs on its own handle to the transport, so the returned
    /// future owns all it needs.
    fn run<T, Work>(&self, work: impl FnOnce(Arc<Transport>) -> Work) -> GithubFuture<T>
    where
        Work: Future<Output = GithubResult<T>> + Send + 'static,
    {
        work(self.transport.clone()).boxed()
    }

    /// Like [`Self::run`] for a write: its place in the token's queue is
    /// taken now, so writes run in the order they were asked for. The place
    /// is held until the future finishes or is dropped, so one that is kept
    /// alive but never polled holds back every later write of the token.
    fn run_write<T, Work>(
        &self,
        work: impl FnOnce(Arc<Transport>, WriteTicket) -> Work,
    ) -> GithubFuture<T>
    where
        Work: Future<Output = GithubResult<T>> + Send + 'static,
    {
        let ticket = self.transport.write_queue.ticket();
        work(self.transport.clone(), ticket).boxed()
    }
}

impl fmt::Debug for HttpBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpBackend")
            .field("token", &self.transport.token)
            .finish_non_exhaustive()
    }
}

fn repository_segments<'a>(repository: &'a RepositoryId, rest: &[&'a str]) -> Vec<&'a str> {
    let mut segments = vec!["repos", repository.owner(), repository.name()];
    segments.extend_from_slice(rest);
    segments
}

fn page_query(cursor: Option<&str>) -> GithubResult<Vec<(&'static str, String)>> {
    let paging = match cursor {
        None => ("page", "1".to_string()),
        Some(cursor) => rest::cursor_parameter(cursor)
            .ok_or_else(|| unexpected(format!("`{cursor}` is not a page cursor")))?,
    };
    Ok(vec![("per_page", PER_PAGE.to_string()), paging])
}

fn parse<T: DeserializeOwned>(body: &str) -> GithubResult<T> {
    serde_json::from_str(body).map_err(unexpected)
}

fn rest_page<P: DeserializeOwned, T: From<P>>(answer: RestAnswer) -> GithubResult<Page<T>> {
    let payloads: Vec<P> = parse(&answer.body)?;
    Ok(Page {
        items: payloads.into_iter().map(T::from).collect(),
        next_cursor: answer.next_page,
    })
}

fn issue_url(repository: &RepositoryId, number: u64, rest: &[&str]) -> GithubResult<Url> {
    let number = number.to_string();
    let mut segments = repository_segments(repository, &["issues", &number]);
    segments.extend_from_slice(rest);
    api_url(&segments, &[])
}

/// Search qualifiers that pick which repositories are searched. GitHub reads
/// several `repo:` as "any of", so one typed by the user would pull in issues
/// of another repository under this one's name.
const SCOPE_QUALIFIERS: [&str; 4] = ["repo:", "org:", "user:", "owner:"];

/// A login as GitHub allows it, which cannot carry a second qualifier.
fn checked_login(login: &str) -> GithubResult<&str> {
    let is_login = !login.is_empty()
        && login
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-');
    if is_login {
        Ok(login)
    } else {
        Err(GithubError::Rejected {
            message: format!("`{}` is not a GitHub login.", shorten_message(login)),
        })
    }
}

/// The most of the typed words a search sends. GitHub ignores a query past
/// 256 characters, so a longer one would only be a bigger request.
pub(crate) const SEARCH_TEXT_MAX_CHARACTERS: usize = 256;

/// The words typed, as one group that cannot reach past this repository:
/// scope qualifiers are dropped wherever they stand, and parentheses and an
/// unpaired quote are removed so the group cannot be closed early.
fn search_text_group(text: &str) -> Option<String> {
    let text = text
        .chars()
        .take(SEARCH_TEXT_MAX_CHARACTERS)
        .map(|character| {
            if matches!(character, '(' | ')') {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let mut words = text
        .split_whitespace()
        .filter(|word| {
            let word = word.to_ascii_lowercase();
            !SCOPE_QUALIFIERS
                .iter()
                .any(|qualifier| word.contains(qualifier))
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    // Counted after the qualifiers go: a dropped word can take one half of
    // a quoted phrase with it.
    let quotes = words
        .iter()
        .map(|word| word.matches('"').count())
        .sum::<usize>();
    if quotes % 2 == 1 {
        words = words
            .into_iter()
            .map(|word| word.replace('"', ""))
            .filter(|word| !word.is_empty())
            .collect();
    }
    // Dropping a qualifier can strand an operator (`foo OR repo:x/y`), which
    // GitHub answers with 422.
    let is_operator = |word: &String| matches!(word.as_str(), "AND" | "OR" | "NOT");
    let first = words.iter().position(|word| !is_operator(word))?;
    let last = words.iter().rposition(|word| !is_operator(word))?;
    let words = &words[first..=last];
    Some(format!("({})", words.join(" ")))
}

/// The `q` of a search: this repository's issues, the filter as qualifiers,
/// then the words typed. The milestone is left to the caller: search names a
/// milestone by its title, which the filter does not hold.
fn issue_search_query(
    repository: &RepositoryId,
    filter: &IssueFilter,
    text: &str,
) -> GithubResult<String> {
    let mut terms = vec![format!("repo:{repository}"), "is:issue".to_string()];
    match filter.state {
        IssueStateFilter::Open => terms.push("is:open".to_string()),
        IssueStateFilter::Closed => terms.push("is:closed".to_string()),
        IssueStateFilter::All => {}
    }
    for label in &filter.labels {
        terms.push(format!("label:\"{}\"", label.replace('"', "")));
    }
    let people = [
        ("assignee", &filter.assignee),
        ("author", &filter.creator),
        ("mentions", &filter.mentioned),
    ];
    for (qualifier, login) in people {
        if let Some(login) = login {
            terms.push(format!("{qualifier}:{}", checked_login(login)?));
        }
    }
    if let Some(issue_type) = &filter.issue_type {
        terms.push(format!("type:\"{}\"", issue_type.replace('"', "")));
    }
    terms.extend(search_text_group(text));
    Ok(terms.join(" "))
}

fn repository_variables(repository: &RepositoryId) -> Value {
    json!({ "owner": repository.owner(), "name": repository.name() })
}

/// The repository object of a GraphQL answer; `null` means the token cannot
/// see it.
fn repository_of(data: &Value) -> GithubResult<&Value> {
    data.get("repository")
        .filter(|repository| !repository.is_null())
        .ok_or(GithubError::NotFound)
}

fn graphql_page<T>(
    connection: Option<&Value>,
    parse_node: impl Fn(&Value) -> GithubResult<T>,
) -> GithubResult<Page<T>> {
    let connection = connection.ok_or_else(|| unexpected("the list is missing"))?;
    Ok(Page {
        items: graphql::nodes(connection)
            .map(parse_node)
            .collect::<GithubResult<_>>()?,
        next_cursor: graphql::next_cursor(connection),
    })
}

impl GithubBackend for HttpBackend {
    fn viewer(&self) -> GithubFuture<Viewer> {
        self.run(move |transport| async move {
            let answer = transport.get(api_url(&["user"], &[])?, false).await?;
            let user: UserPayload = parse(&answer.body)?;
            Ok(Viewer {
                login: user.login,
                scopes: rest::oauth_scopes(&answer.headers),
            })
        })
    }

    fn rate_limits(&self) -> RateLimits {
        self.transport.rate_limits()
    }

    fn repository_visibility(
        &self,
        repository: &RepositoryId,
    ) -> GithubFuture<RepositoryVisibility> {
        let repository = repository.clone();
        self.run(move |transport| async move {
            let url = api_url(&repository_segments(&repository, &[]), &[])?;
            let answer = transport.get(url, true).await?;
            Ok(parse::<RepositoryPayload>(&answer.body)?.into())
        })
    }

    fn repository_permission(
        &self,
        repository: &RepositoryId,
    ) -> GithubFuture<Option<RepositoryPermission>> {
        let repository = repository.clone();
        self.run(move |transport| async move {
            let url = api_url(&repository_segments(&repository, &[]), &[])?;
            let answer = transport.get(url, true).await?;
            Ok(parse::<RepositoryPayload>(&answer.body)?.permission())
        })
    }

    fn list_issues(
        &self,
        repository: &RepositoryId,
        filter: &IssueFilter,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Issue>> {
        let repository = repository.clone();
        let filter = filter.clone();
        self.run(move |transport| async move {
            let mut query = page_query(cursor.as_deref())?;
            let state = match filter.state {
                IssueStateFilter::Open => "open",
                IssueStateFilter::Closed => "closed",
                IssueStateFilter::All => "all",
            };
            query.push(("state", state.to_string()));
            if let Some(label) = filter.labels.iter().find(|label| label.contains(',')) {
                // `labels=` is a comma-separated list with no way to escape
                // a comma, so GitHub would read this label as two.
                return Err(GithubError::Rejected {
                    message: format!(
                        "The label `{}` has a comma in its name, which the issue list cannot filter by.",
                        shorten_message(label)
                    ),
                });
            }
            if !filter.labels.is_empty() {
                query.push(("labels", filter.labels.join(",")));
            }
            if let Some(assignee) = filter.assignee {
                query.push(("assignee", assignee));
            }
            if let Some(creator) = filter.creator {
                query.push(("creator", creator));
            }
            if let Some(mentioned) = filter.mentioned {
                query.push(("mentioned", mentioned));
            }
            if let Some(milestone) = filter.milestone {
                query.push(("milestone", milestone.to_string()));
            }
            if let Some(issue_type) = filter.issue_type {
                query.push(("type", issue_type));
            }
            let url = api_url(&repository_segments(&repository, &["issues"]), &query)?;
            let answer = transport.get(url, true).await?;
            let payloads: Vec<IssuePayload> = parse(&answer.body)?;
            Ok(Page {
                items: payloads
                    .into_iter()
                    .filter(|payload| !payload.is_pull_request())
                    .map(Issue::from)
                    .collect(),
                next_cursor: answer.next_page,
            })
        })
    }

    fn search_issues(
        &self,
        repository: &RepositoryId,
        filter: &IssueFilter,
        text: &str,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Issue>> {
        let repository = repository.clone();
        let query = issue_search_query(&repository, filter, text);
        self.run(move |transport| async move {
            let mut parameters = page_query(cursor.as_deref())?;
            parameters.push(("q", query?));
            let url = api_url(&["search", "issues"], &parameters)?;
            let answer = transport.get(url, true).await?;
            let payload: SearchIssuesPayload = parse(&answer.body)?;
            Ok(Page {
                // Search spans all of GitHub; a result from elsewhere would
                // be acted on by its number in this repository.
                items: payload
                    .items
                    .into_iter()
                    .filter(|payload| payload.belongs_to(&repository))
                    .filter(|payload| !payload.is_pull_request())
                    .map(Issue::from)
                    .collect(),
                next_cursor: answer.next_page,
            })
        })
    }

    fn issue(&self, repository: &RepositoryId, number: u64) -> GithubFuture<Issue> {
        let repository = repository.clone();
        self.run(move |transport| async move {
            let answer = transport
                .get(issue_url(&repository, number, &[])?, true)
                .await?;
            Ok(Issue::from(parse::<IssuePayload>(&answer.body)?))
        })
    }

    fn create_issue(&self, repository: &RepositoryId, issue: NewIssue) -> GithubFuture<Issue> {
        let repository = repository.clone();
        self.run_write(move |transport, ticket| async move {
            let url = api_url(&repository_segments(&repository, &["issues"]), &[])?;
            let mut body = json!({ "title": issue.title });
            if let Some(text) = issue.body {
                body["body"] = json!(text);
            }
            if !issue.labels.is_empty() {
                body["labels"] = json!(issue.labels);
            }
            if !issue.assignees.is_empty() {
                body["assignees"] = json!(issue.assignees);
            }
            if let Some(milestone) = issue.milestone {
                body["milestone"] = json!(milestone);
            }
            let answer = transport
                .write(ticket, Method::POST, url, Some(body))
                .await?;
            Ok(Issue::from(parse::<IssuePayload>(&answer)?))
        })
    }

    fn update_issue(
        &self,
        repository: &RepositoryId,
        number: u64,
        update: IssueUpdate,
    ) -> GithubFuture<Issue> {
        let repository = repository.clone();
        self.run_write(move |transport, ticket| async move {
            let mut body = json!({});
            if let Some(title) = update.title {
                body["title"] = json!(title);
            }
            if let Some(text) = update.body {
                body["body"] = json!(text);
            }
            if let Some(state) = update.state {
                body["state"] = json!(match state {
                    IssueState::Open => "open",
                    IssueState::Closed => "closed",
                });
            }
            if let Some(reason) = update.state_reason {
                body["state_reason"] = json!(state_reason_name(reason));
            }
            let url = issue_url(&repository, number, &[])?;
            let answer = transport
                .write(ticket, Method::PATCH, url, Some(body))
                .await?;
            Ok(Issue::from(parse::<IssuePayload>(&answer)?))
        })
    }

    fn list_comments(
        &self,
        repository: &RepositoryId,
        number: u64,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Comment>> {
        let repository = repository.clone();
        self.run(move |transport| async move {
            let mut url = issue_url(&repository, number, &["comments"])?;
            url.query_pairs_mut()
                .extend_pairs(page_query(cursor.as_deref())?);
            rest_page::<CommentPayload, Comment>(transport.get(url, true).await?)
        })
    }

    fn add_comment(
        &self,
        repository: &RepositoryId,
        number: u64,
        body: String,
    ) -> GithubFuture<Comment> {
        let repository = repository.clone();
        self.run_write(move |transport, ticket| async move {
            let url = issue_url(&repository, number, &["comments"])?;
            let answer = transport
                .write(ticket, Method::POST, url, Some(json!({ "body": body })))
                .await?;
            Ok(Comment::from(parse::<CommentPayload>(&answer)?))
        })
    }

    fn list_labels(
        &self,
        repository: &RepositoryId,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Label>> {
        let repository = repository.clone();
        self.run(move |transport| async move {
            let query = page_query(cursor.as_deref())?;
            let url = api_url(&repository_segments(&repository, &["labels"]), &query)?;
            rest_page::<LabelPayload, Label>(transport.get(url, true).await?)
        })
    }

    fn add_labels(
        &self,
        repository: &RepositoryId,
        number: u64,
        labels: Vec<String>,
    ) -> GithubFuture<Vec<Label>> {
        let repository = repository.clone();
        self.run_write(move |transport, ticket| async move {
            let url = issue_url(&repository, number, &["labels"])?;
            let answer = transport
                .write(ticket, Method::POST, url, Some(json!({ "labels": labels })))
                .await?;
            let labels: Vec<LabelPayload> = parse(&answer)?;
            Ok(labels.into_iter().map(Label::from).collect())
        })
    }

    fn remove_label(
        &self,
        repository: &RepositoryId,
        number: u64,
        label: String,
    ) -> GithubFuture<()> {
        let repository = repository.clone();
        self.run_write(move |transport, ticket| async move {
            let url = issue_url(&repository, number, &["labels", &label])?;
            transport.write(ticket, Method::DELETE, url, None).await?;
            Ok(())
        })
    }

    fn add_assignees(
        &self,
        repository: &RepositoryId,
        number: u64,
        logins: Vec<String>,
    ) -> GithubFuture<Issue> {
        let repository = repository.clone();
        self.run_write(move |transport, ticket| async move {
            let url = issue_url(&repository, number, &["assignees"])?;
            let answer = transport
                .write(
                    ticket,
                    Method::POST,
                    url,
                    Some(json!({ "assignees": logins })),
                )
                .await?;
            Ok(Issue::from(parse::<IssuePayload>(&answer)?))
        })
    }

    fn remove_assignees(
        &self,
        repository: &RepositoryId,
        number: u64,
        logins: Vec<String>,
    ) -> GithubFuture<Issue> {
        let repository = repository.clone();
        self.run_write(move |transport, ticket| async move {
            let url = issue_url(&repository, number, &["assignees"])?;
            let answer = transport
                .write(
                    ticket,
                    Method::DELETE,
                    url,
                    Some(json!({ "assignees": logins })),
                )
                .await?;
            Ok(Issue::from(parse::<IssuePayload>(&answer)?))
        })
    }

    fn assignable_users(
        &self,
        repository: &RepositoryId,
        query: &str,
    ) -> GithubFuture<Vec<GithubUser>> {
        let mut variables = repository_variables(repository);
        let query = query.trim();
        variables["query"] = if query.is_empty() {
            Value::Null
        } else {
            json!(query)
        };
        self.run(move |transport| async move {
            let data = transport
                .graphql(documents::REPOSITORY_ASSIGNABLE_USERS, variables)
                .await?;
            let Some(connection) = repository_of(&data)?.get("assignableUsers") else {
                return Err(unexpected("the list is missing"));
            };
            graphql::nodes(connection)
                .map(graphql::parse_github_user)
                .collect()
        })
    }

    fn list_milestones(
        &self,
        repository: &RepositoryId,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Milestone>> {
        let repository = repository.clone();
        self.run(move |transport| async move {
            let mut query = page_query(cursor.as_deref())?;
            query.push(("state", "open".to_string()));
            let url = api_url(&repository_segments(&repository, &["milestones"]), &query)?;
            rest_page::<MilestonePayload, Milestone>(transport.get(url, true).await?)
        })
    }

    fn set_milestone(
        &self,
        repository: &RepositoryId,
        number: u64,
        milestone: Option<u64>,
    ) -> GithubFuture<Issue> {
        let repository = repository.clone();
        self.run_write(move |transport, ticket| async move {
            let url = issue_url(&repository, number, &[])?;
            let answer = transport
                .write(
                    ticket,
                    Method::PATCH,
                    url,
                    Some(json!({ "milestone": milestone })),
                )
                .await?;
            Ok(Issue::from(parse::<IssuePayload>(&answer)?))
        })
    }

    fn list_sub_issues(
        &self,
        repository: &RepositoryId,
        number: u64,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Issue>> {
        let repository = repository.clone();
        self.run(move |transport| async move {
            let mut url = issue_url(&repository, number, &["sub_issues"])?;
            url.query_pairs_mut()
                .extend_pairs(page_query(cursor.as_deref())?);
            rest_page::<IssuePayload, Issue>(transport.get(url, true).await?)
        })
    }

    fn add_sub_issue(
        &self,
        repository: &RepositoryId,
        parent_number: u64,
        sub_issue_id: u64,
    ) -> GithubFuture<()> {
        let repository = repository.clone();
        self.run_write(move |transport, ticket| async move {
            let url = issue_url(&repository, parent_number, &["sub_issues"])?;
            let body = json!({ "sub_issue_id": sub_issue_id });
            transport
                .write(ticket, Method::POST, url, Some(body))
                .await?;
            Ok(())
        })
    }

    fn remove_sub_issue(
        &self,
        repository: &RepositoryId,
        parent_number: u64,
        sub_issue_id: u64,
    ) -> GithubFuture<()> {
        let repository = repository.clone();
        self.run_write(move |transport, ticket| async move {
            let url = issue_url(&repository, parent_number, &["sub_issue"])?;
            let body = json!({ "sub_issue_id": sub_issue_id });
            transport
                .write(ticket, Method::DELETE, url, Some(body))
                .await?;
            Ok(())
        })
    }

    fn list_issue_types(&self, repository: &RepositoryId) -> GithubFuture<Vec<IssueType>> {
        let variables = repository_variables(repository);
        self.run(move |transport| async move {
            let data = transport
                .graphql(documents::REPOSITORY_ISSUE_TYPES, variables)
                .await?;
            let Some(connection) = repository_of(&data)?
                .get("issueTypes")
                .filter(|types| !types.is_null())
            else {
                return Ok(Vec::new());
            };
            graphql::nodes(connection)
                .map(graphql::parse_issue_type)
                .collect()
        })
    }

    fn set_issue_type(
        &self,
        issue_node_id: String,
        issue_type_id: Option<String>,
    ) -> GithubFuture<()> {
        self.run_write(move |transport, ticket| async move {
            let variables = json!({ "issueId": issue_node_id, "issueTypeId": issue_type_id });
            transport
                .graphql_mutation(ticket, documents::UPDATE_ISSUE_ISSUE_TYPE, variables)
                .await?;
            Ok(())
        })
    }

    fn issue_parent(
        &self,
        repository: &RepositoryId,
        number: u64,
    ) -> GithubFuture<Option<ParentIssue>> {
        let mut variables = repository_variables(repository);
        variables["number"] = json!(number);
        self.run(move |transport| async move {
            let data = transport
                .graphql(documents::ISSUE_PARENT, variables)
                .await?;
            let issue = repository_of(&data)?
                .get("issue")
                .filter(|issue| !issue.is_null())
                .ok_or(GithubError::NotFound)?;
            match issue.get("parent").filter(|parent| !parent.is_null()) {
                Some(parent) => graphql::parse_parent_issue(parent).map(Some),
                None => Ok(None),
            }
        })
    }

    fn replace_parent(
        &self,
        repository: &RepositoryId,
        parent_number: u64,
        sub_issue_id: u64,
    ) -> GithubFuture<()> {
        let repository = repository.clone();
        self.run_write(move |transport, ticket| async move {
            let url = issue_url(&repository, parent_number, &["sub_issues"])?;
            let body = json!({ "sub_issue_id": sub_issue_id, "replace_parent": true });
            transport
                .write(ticket, Method::POST, url, Some(body))
                .await?;
            Ok(())
        })
    }

    fn reprioritize_sub_issue(
        &self,
        parent_node_id: String,
        sub_issue_node_id: String,
        position: SubIssuePosition,
    ) -> GithubFuture<()> {
        let (after_id, before_id) = match position {
            SubIssuePosition::After(id) => (Some(id), None),
            SubIssuePosition::Before(id) => (None, Some(id)),
        };
        let variables = json!({
            "issueId": parent_node_id,
            "subIssueId": sub_issue_node_id,
            "afterId": after_id,
            "beforeId": before_id,
        });
        self.run_write(move |transport, ticket| async move {
            transport
                .graphql_mutation(ticket, documents::REPRIORITIZE_SUB_ISSUE, variables)
                .await?;
            Ok(())
        })
    }

    fn list_projects(
        &self,
        repository: &RepositoryId,
        cursor: Option<String>,
    ) -> GithubFuture<Page<ProjectV2>> {
        let mut variables = repository_variables(repository);
        variables["after"] = json!(cursor);
        self.run(move |transport| async move {
            let data = transport
                .graphql(documents::REPOSITORY_PROJECTS, variables)
                .await?;
            graphql_page(
                repository_of(&data)?.get("projectsV2"),
                graphql::parse_project,
            )
        })
    }

    fn list_project_items(
        &self,
        project_id: String,
        cursor: Option<String>,
    ) -> GithubFuture<Page<ProjectV2Item>> {
        self.run(move |transport| async move {
            let variables = json!({ "projectId": project_id, "after": cursor });
            let data = transport
                .graphql(documents::PROJECT_ITEMS, variables)
                .await?;
            let project = data
                .get("node")
                .filter(|node| !node.is_null())
                .ok_or(GithubError::NotFound)?;
            graphql_page(project.get("items"), graphql::parse_project_item)
        })
    }

    fn project_details(&self, project_id: String) -> GithubFuture<ProjectDetails> {
        self.run(move |transport| async move {
            let variables = json!({ "projectId": project_id });
            let data = transport
                .graphql(documents::PROJECT_DETAILS, variables)
                .await?;
            let project = data
                .get("node")
                .filter(|node| !node.is_null())
                .ok_or(GithubError::NotFound)?;
            graphql_projects::parse_project_details(project)
        })
    }

    fn set_project_item_field(
        &self,
        project_id: String,
        item_id: String,
        field_id: String,
        value: Option<ProjectFieldInput>,
    ) -> GithubFuture<()> {
        self.run_write(move |transport, ticket| async move {
            let mut variables =
                json!({ "projectId": project_id, "itemId": item_id, "fieldId": field_id });
            let document = match value {
                Some(ProjectFieldInput::Text { text }) => {
                    variables["value"] = json!({ "text": text });
                    documents::UPDATE_PROJECT_ITEM_FIELD
                }
                Some(ProjectFieldInput::Number { number }) => {
                    variables["value"] = json!({ "number": number });
                    documents::UPDATE_PROJECT_ITEM_FIELD
                }
                Some(ProjectFieldInput::Date { date }) => {
                    variables["value"] = json!({ "date": date });
                    documents::UPDATE_PROJECT_ITEM_FIELD
                }
                Some(ProjectFieldInput::SingleSelect { option_id }) => {
                    variables["value"] = json!({ "singleSelectOptionId": option_id });
                    documents::UPDATE_PROJECT_ITEM_FIELD
                }
                Some(ProjectFieldInput::Iteration { iteration_id }) => {
                    variables["value"] = json!({ "iterationId": iteration_id });
                    documents::UPDATE_PROJECT_ITEM_FIELD
                }
                None => documents::CLEAR_PROJECT_ITEM_FIELD,
            };
            transport
                .graphql_mutation(ticket, document, variables)
                .await?;
            Ok(())
        })
    }

    fn move_project_item(
        &self,
        project_id: String,
        item_id: String,
        after_id: Option<String>,
    ) -> GithubFuture<()> {
        self.run_write(move |transport, ticket| async move {
            let variables =
                json!({ "projectId": project_id, "itemId": item_id, "afterId": after_id });
            transport
                .graphql_mutation(ticket, documents::UPDATE_PROJECT_ITEM_POSITION, variables)
                .await?;
            Ok(())
        })
    }

    fn add_project_item(&self, project_id: String, content_id: String) -> GithubFuture<String> {
        self.run_write(move |transport, ticket| async move {
            let variables = json!({ "projectId": project_id, "contentId": content_id });
            let data = transport
                .graphql_mutation(ticket, documents::ADD_PROJECT_ITEM, variables)
                .await?;
            data.get("addProjectV2ItemById")
                .and_then(|payload| payload.get("item"))
                .filter(|item| !item.is_null())
                .ok_or(GithubError::NotFound)
                .and_then(|item| graphql::string_at(item, "id"))
        })
    }

    fn delete_project_item(&self, project_id: String, item_id: String) -> GithubFuture<()> {
        self.run_write(move |transport, ticket| async move {
            let variables = json!({ "projectId": project_id, "itemId": item_id });
            transport
                .graphql_mutation(ticket, documents::DELETE_PROJECT_ITEM, variables)
                .await?;
            Ok(())
        })
    }

    fn project_owners(&self, repository: &RepositoryId) -> GithubFuture<ProjectOwners> {
        let variables = repository_variables(repository);
        self.run(move |transport| async move {
            let data = transport
                .graphql(documents::PROJECT_OWNERS, variables)
                .await?;
            graphql_projects::parse_project_owners(&data, repository_of(&data)?)
        })
    }

    fn create_project(
        &self,
        owner_id: String,
        title: String,
        repository_id: String,
    ) -> GithubFuture<ProjectV2> {
        self.run_write(move |transport, ticket| async move {
            let variables =
                json!({ "ownerId": owner_id, "title": title, "repositoryId": repository_id });
            let data = transport
                .graphql_mutation(ticket, documents::CREATE_PROJECT, variables)
                .await?;
            data.get("createProjectV2")
                .and_then(|payload| payload.get("projectV2"))
                .filter(|project| !project.is_null())
                .ok_or_else(|| unexpected("the new project is missing"))
                .and_then(graphql::parse_project)
        })
    }

    fn list_pull_requests(
        &self,
        repository: &RepositoryId,
        state: PullRequestStateFilter,
        cursor: Option<String>,
    ) -> GithubFuture<Page<PullRequest>> {
        let mut variables = repository_variables(repository);
        variables["after"] = json!(cursor);
        variables["states"] = match state {
            PullRequestStateFilter::Open => json!(["OPEN"]),
            PullRequestStateFilter::Closed => json!(["CLOSED"]),
            PullRequestStateFilter::Merged => json!(["MERGED"]),
            PullRequestStateFilter::All => Value::Null,
        };
        self.run(move |transport| async move {
            let data = transport
                .graphql(documents::REPOSITORY_PULL_REQUESTS, variables)
                .await?;
            graphql_page(
                repository_of(&data)?.get("pullRequests"),
                graphql::parse_pull_request,
            )
        })
    }

    fn pull_request(&self, repository: &RepositoryId, number: u64) -> GithubFuture<PullRequest> {
        let mut variables = repository_variables(repository);
        variables["number"] = json!(number);
        self.run(move |transport| async move {
            let data = transport
                .graphql(documents::REPOSITORY_PULL_REQUEST, variables)
                .await?;
            let pull_request = repository_of(&data)?
                .get("pullRequest")
                .filter(|pull_request| !pull_request.is_null())
                .ok_or(GithubError::NotFound)?;
            graphql::parse_pull_request(pull_request)
        })
    }

    fn pull_request_details(
        &self,
        repository: &RepositoryId,
        number: u64,
    ) -> GithubFuture<PullRequestDetails> {
        let mut variables = repository_variables(repository);
        variables["number"] = json!(number);
        self.run(move |transport| async move {
            let data = transport
                .graphql(documents::PULL_REQUEST_DETAILS, variables)
                .await?;
            graphql::parse_pull_request_details(repository_of(&data)?)
        })
    }

    fn merge_pull_request(
        &self,
        pull_request_id: String,
        method: MergeMethod,
        expected_head_oid: String,
    ) -> GithubFuture<()> {
        let variables = json!({
            "pullRequestId": pull_request_id,
            "mergeMethod": method.graphql_name(),
            "expectedHeadOid": expected_head_oid,
        });
        self.run_write(move |transport, ticket| async move {
            transport
                .graphql_mutation(ticket, documents::MERGE_PULL_REQUEST, variables)
                .await?;
            Ok(())
        })
    }

    fn delete_branch_at(
        &self,
        repository_node_id: String,
        branch: String,
        expected_oid: String,
    ) -> GithubFuture<()> {
        let variables = json!({
            "repositoryId": repository_node_id,
            "refName": format!("refs/heads/{branch}"),
            "expectedOid": expected_oid,
            "deletedOid": DELETED_OID,
        });
        self.run_write(move |transport, ticket| async move {
            transport
                .graphql_mutation(ticket, documents::DELETE_BRANCH_AT, variables)
                .await?;
            Ok(())
        })
    }
}

/// What `updateRefs` reads as "delete the ref".
const DELETED_OID: &str = "0000000000000000000000000000000000000000";
