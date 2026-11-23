use serde_json::{Value, json};

use crate::error::{GithubError, GithubResult, org_restriction, shorten_message, unexpected};
use crate::graphql_projects;
pub(crate) use crate::graphql_pull_requests::{parse_pull_request, parse_pull_request_details};
use crate::model::{
    GithubUser, IssueState, IssueType, ParentIssue, ProjectItemContent, ProjectV2, ProjectV2Item,
    RepositoryId,
};

/// The queries and mutations sent to `POST /graphql`. Each file is checked
/// against GitHub's public schema by `graphql_schema_tests.rs`.
pub(crate) mod documents {
    pub const REPOSITORY_ISSUE_TYPES: &str =
        include_str!("graphql_documents/repository_issue_types.graphql");
    pub const UPDATE_ISSUE_ISSUE_TYPE: &str =
        include_str!("graphql_documents/update_issue_issue_type.graphql");
    pub const ISSUE_PARENT: &str = include_str!("graphql_documents/issue_parent.graphql");
    pub const REPRIORITIZE_SUB_ISSUE: &str =
        include_str!("graphql_documents/reprioritize_sub_issue.graphql");
    pub const REPOSITORY_PROJECTS: &str =
        include_str!("graphql_documents/repository_projects.graphql");
    pub const PROJECT_DETAILS: &str = include_str!("graphql_documents/project_details.graphql");
    pub const PROJECT_ITEMS: &str = include_str!("graphql_documents/project_items.graphql");
    pub const UPDATE_PROJECT_ITEM_FIELD: &str =
        include_str!("graphql_documents/update_project_item_field.graphql");
    pub const CLEAR_PROJECT_ITEM_FIELD: &str =
        include_str!("graphql_documents/clear_project_item_field.graphql");
    pub const UPDATE_PROJECT_ITEM_POSITION: &str =
        include_str!("graphql_documents/update_project_item_position.graphql");
    pub const ADD_PROJECT_ITEM: &str = include_str!("graphql_documents/add_project_item.graphql");
    pub const DELETE_PROJECT_ITEM: &str =
        include_str!("graphql_documents/delete_project_item.graphql");
    pub const PROJECT_OWNERS: &str = include_str!("graphql_documents/project_owners.graphql");
    pub const CREATE_PROJECT: &str = include_str!("graphql_documents/create_project.graphql");
    pub const REPOSITORY_PULL_REQUESTS: &str =
        include_str!("graphql_documents/repository_pull_requests.graphql");
    pub const REPOSITORY_PULL_REQUEST: &str =
        include_str!("graphql_documents/repository_pull_request.graphql");
    pub const PULL_REQUEST_DETAILS: &str =
        include_str!("graphql_documents/pull_request_details.graphql");
    pub const MERGE_PULL_REQUEST: &str =
        include_str!("graphql_documents/merge_pull_request.graphql");
    pub const DELETE_BRANCH_AT: &str = include_str!("graphql_documents/delete_branch_at.graphql");
    pub const REPOSITORY_ASSIGNABLE_USERS: &str =
        include_str!("graphql_documents/repository_assignable_users.graphql");
}

pub(crate) fn request_body(document: &str, variables: Value) -> Value {
    json!({ "query": document, "variables": variables })
}

/// Whether an answer that carries both `data` and `errors[]` is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Partial {
    /// Any error fails the call. Mutations use this: a half-applied change
    /// must not read as done.
    Refuse,
    /// A read keeps its data when every error sits inside one item of a
    /// list (`…/nodes/<i>/…`), such as an issue in a project that the token
    /// cannot see. That item arrives `null` and the rest is still shown.
    KeepNestedNodes,
}

/// The `data` of an answer, or the first of its `errors[]` as a readable
/// error. GitHub answers GraphQL errors with HTTP 200, so the status alone
/// says nothing.
#[cfg(test)]
pub(crate) fn data_or_error(body: &str) -> GithubResult<Value> {
    data_or_partial_error(body, Partial::Refuse)
}

pub(crate) fn data_or_partial_error(body: &str, partial: Partial) -> GithubResult<Value> {
    let mut answer: Value = serde_json::from_str(body).map_err(unexpected)?;
    let errors = match answer.get_mut("errors").map(Value::take) {
        Some(Value::Array(errors)) => errors,
        _ => Vec::new(),
    };
    let data = answer.get_mut("data").map(Value::take);
    if let Some(error) = errors.first() {
        let keep = partial == Partial::KeepNestedNodes
            && data.as_ref().is_some_and(Value::is_object)
            && errors.iter().all(is_inside_a_list_item);
        if !keep {
            return Err(error_from_graphql(error));
        }
        // The messages may quote content, so only the count is logged.
        log::warn!(
            "GitHub left {} list item(s) out of a GraphQL answer",
            errors.len()
        );
    }
    match data {
        Some(data) if data.is_object() => Ok(data),
        _ => Err(unexpected("the answer has no data")),
    }
}

/// An error whose `path` runs through `nodes/<index>` at least three steps
/// deep. Scope and rate errors are never partial: they apply to the whole
/// call.
fn is_inside_a_list_item(error: &Value) -> bool {
    if matches!(
        error.get("type").and_then(Value::as_str),
        Some("INSUFFICIENT_SCOPES" | "RATE_LIMITED")
    ) {
        return false;
    }
    let Some(path) = error.get("path").and_then(Value::as_array) else {
        return false;
    };
    path.len() >= 3
        && path
            .windows(2)
            .any(|step| step[0].as_str() == Some("nodes") && step[1].is_u64())
}

/// The cursor of the next page of a connection, `None` on the last page.
pub(crate) fn next_cursor(connection: &Value) -> Option<String> {
    let page_info = connection.get("pageInfo")?;
    if page_info.get("hasNextPage").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    optional_string_at(page_info, "endCursor")
}

pub(crate) fn nodes(connection: &Value) -> impl Iterator<Item = &Value> {
    connection
        .get("nodes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|node| !node.is_null())
}

pub(crate) fn string_at(value: &Value, field: &str) -> GithubResult<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| unexpected(format!("`{field}` is missing")))
}

pub(crate) fn optional_string_at(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_string)
}

pub(crate) fn number_at(value: &Value, field: &str) -> GithubResult<u64> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| unexpected(format!("`{field}` is missing")))
}

pub(crate) fn bool_at(value: &Value, field: &str) -> bool {
    value.get(field).and_then(Value::as_bool).unwrap_or(false)
}

pub(crate) fn parse_issue_type(node: &Value) -> GithubResult<IssueType> {
    Ok(IssueType {
        id: string_at(node, "id")?,
        name: string_at(node, "name")?,
        description: optional_string_at(node, "description"),
        color: optional_string_at(node, "color"),
        is_enabled: bool_at(node, "isEnabled"),
    })
}

pub(crate) fn parse_github_user(node: &Value) -> GithubResult<GithubUser> {
    Ok(GithubUser {
        login: string_at(node, "login")?,
        name: optional_string_at(node, "name").filter(|name| !name.trim().is_empty()),
        avatar_url: optional_string_at(node, "avatarUrl"),
    })
}

pub(crate) fn parse_parent_issue(node: &Value) -> GithubResult<ParentIssue> {
    let name_with_owner = node
        .get("repository")
        .map(|repository| string_at(repository, "nameWithOwner"))
        .unwrap_or_else(|| Err(unexpected("`repository` is missing")))?;
    let repository = name_with_owner
        .split_once('/')
        .and_then(|(owner, name)| RepositoryId::new(owner, name))
        .ok_or_else(|| unexpected(format!("`{name_with_owner}` is not a repository")))?;
    let state = match node.get("state").and_then(Value::as_str) {
        Some("OPEN") => IssueState::Open,
        Some("CLOSED") => IssueState::Closed,
        other => return Err(unexpected(format!("unknown issue state {other:?}"))),
    };
    Ok(ParentIssue {
        id: number_at(node, "databaseId")?,
        node_id: string_at(node, "id")?,
        number: number_at(node, "number")?,
        title: string_at(node, "title")?,
        state,
        repository,
    })
}

pub(crate) fn parse_project(node: &Value) -> GithubResult<ProjectV2> {
    Ok(ProjectV2 {
        id: string_at(node, "id")?,
        number: number_at(node, "number")?,
        title: string_at(node, "title")?,
        url: string_at(node, "url")?,
        closed: bool_at(node, "closed"),
    })
}

pub(crate) fn parse_project_item(node: &Value) -> GithubResult<ProjectV2Item> {
    let content = match (
        node.get("type").and_then(Value::as_str),
        node.get("content").filter(|content| !content.is_null()),
    ) {
        (Some("REDACTED"), _) | (_, None) => ProjectItemContent::Redacted,
        (_, Some(content)) => parse_item_content(content)?,
    };
    let visible_content = node
        .get("content")
        .filter(|_| content != ProjectItemContent::Redacted);
    Ok(ProjectV2Item {
        id: string_at(node, "id")?,
        assignees: visible_content.map_or_else(Vec::new, graphql_projects::parse_assignees),
        assignee_avatars: visible_content
            .map_or_else(Default::default, graphql_projects::parse_assignee_avatars),
        state: visible_content.and_then(graphql_projects::parse_item_state),
        sub_issues: visible_content.and_then(graphql_projects::parse_sub_issues),
        labels: visible_content.map_or_else(Vec::new, graphql_projects::parse_labels),
        field_values: graphql_projects::parse_field_values(node),
        content,
    })
}

fn parse_item_content(content: &Value) -> GithubResult<ProjectItemContent> {
    let repository = || {
        content
            .get("repository")
            .map(|repository| string_at(repository, "nameWithOwner"))
            .unwrap_or_else(|| Err(unexpected("`repository` is missing")))
    };
    match content.get("__typename").and_then(Value::as_str) {
        Some("Issue") => Ok(ProjectItemContent::Issue {
            number: number_at(content, "number")?,
            title: string_at(content, "title")?,
            repository: repository()?,
        }),
        Some("PullRequest") => Ok(ProjectItemContent::PullRequest {
            number: number_at(content, "number")?,
            title: string_at(content, "title")?,
            repository: repository()?,
        }),
        Some("DraftIssue") => Ok(ProjectItemContent::DraftIssue {
            title: string_at(content, "title")?,
        }),
        other => Err(unexpected(format!("unknown project item {other:?}"))),
    }
}

fn error_from_graphql(error: &Value) -> GithubError {
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match error.get("type").and_then(Value::as_str) {
        Some("INSUFFICIENT_SCOPES") => match required_scope(message) {
            Some(scope) => GithubError::MissingScope { scope },
            None => GithubError::Forbidden {
                message: shorten_message(message),
            },
        },
        Some("NOT_FOUND") => GithubError::NotFound,
        Some("RATE_LIMITED") => GithubError::RateLimited {
            reset_at: None,
            retry_after: None,
        },
        // A refused change, such as a merge whose head moved meanwhile.
        Some("UNPROCESSABLE") => GithubError::Rejected {
            message: shorten_message(message),
        },
        Some("FORBIDDEN") => org_restriction(message).unwrap_or(GithubError::Forbidden {
            message: shorten_message(message),
        }),
        _ => unexpected(message),
    }
}

/// The first scope in "requires one of the following scopes: ['read:project']".
fn required_scope(message: &str) -> Option<String> {
    let listed = message.split("following scopes:").nth(1)?;
    let scope = listed.split('\'').nth(1)?;
    let is_scope = !scope.is_empty()
        && scope
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ":_-".contains(character));
    is_scope.then(|| scope.to_string())
}
