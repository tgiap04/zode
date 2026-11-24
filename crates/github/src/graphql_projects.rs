//! Reads the Projects (v2) answers: a project's fields and views, and the
//! people, labels and field values of an item.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::error::{GithubResult, unexpected};
use crate::graphql::{nodes, string_at};
use crate::model::{Label, ProjectItemState, SubIssuesSummary};
use crate::projects::{
    ProjectDetails, ProjectField, ProjectFieldKind, ProjectFieldOption, ProjectFieldValue,
    ProjectIteration, ProjectOwner, ProjectOwners, ProjectSort, ProjectValue, ProjectView,
    ProjectViewLayout,
};

fn text_at(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_string)
}

fn connection_nodes<'a>(value: &'a Value, field: &str) -> impl Iterator<Item = &'a Value> {
    value
        .get(field)
        .into_iter()
        .flat_map(|connection| nodes(connection))
}

/// The id a `... on ProjectV2FieldCommon { id }` selection put in `node`.
fn field_id(node: &Value) -> Option<String> {
    text_at(node, "id")
}

pub(crate) fn parse_project_details(project: &Value) -> GithubResult<ProjectDetails> {
    Ok(ProjectDetails {
        // A field of a kind the query has no fragment for comes back as `{}`.
        fields: connection_nodes(project, "fields")
            .filter(|node| node.get("id").is_some())
            .map(parse_field)
            .collect::<GithubResult<_>>()?,
        views: connection_nodes(project, "views")
            .map(parse_view)
            .collect::<GithubResult<_>>()?,
    })
}

fn parse_field(node: &Value) -> GithubResult<ProjectField> {
    let kind = match node.get("dataType").and_then(Value::as_str) {
        Some("TITLE") => ProjectFieldKind::Title,
        Some("ASSIGNEES") => ProjectFieldKind::Assignees,
        Some("LABELS") => ProjectFieldKind::Labels,
        Some("REPOSITORY") => ProjectFieldKind::Repository,
        Some("MILESTONE") => ProjectFieldKind::Milestone,
        Some("LINKED_PULL_REQUESTS") => ProjectFieldKind::LinkedPullRequests,
        Some("SUB_ISSUES_PROGRESS") => ProjectFieldKind::SubIssuesProgress,
        Some("TEXT") => ProjectFieldKind::Text,
        Some("NUMBER") => ProjectFieldKind::Number,
        Some("DATE") => ProjectFieldKind::Date,
        Some("SINGLE_SELECT") => ProjectFieldKind::SingleSelect(
            node.get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|option| {
                    Ok(ProjectFieldOption {
                        id: string_at(option, "id")?,
                        name: string_at(option, "name")?,
                        color: text_at(option, "color"),
                    })
                })
                .collect::<GithubResult<_>>()?,
        ),
        Some("ITERATION") => {
            let configuration = node.get("configuration").unwrap_or(&Value::Null);
            let iterations = |list: &str, completed: bool| {
                configuration
                    .get(list)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(move |iteration| {
                        Ok(ProjectIteration {
                            id: string_at(iteration, "id")?,
                            title: string_at(iteration, "title")?,
                            start_date: string_at(iteration, "startDate")?,
                            completed,
                        })
                    })
            };
            ProjectFieldKind::Iteration(
                iterations("iterations", false)
                    .chain(iterations("completedIterations", true))
                    .collect::<GithubResult<_>>()?,
            )
        }
        _ => ProjectFieldKind::Other,
    };
    Ok(ProjectField {
        id: string_at(node, "id")?,
        name: string_at(node, "name")?,
        kind,
    })
}

fn parse_view(node: &Value) -> GithubResult<ProjectView> {
    let layout = match node.get("layout").and_then(Value::as_str) {
        Some("BOARD_LAYOUT") => ProjectViewLayout::Board,
        Some("TABLE_LAYOUT") => ProjectViewLayout::Table,
        Some("ROADMAP_LAYOUT") => ProjectViewLayout::Roadmap,
        other => return Err(unexpected(format!("unknown project view layout {other:?}"))),
    };
    let field_ids =
        |list: &str| -> Vec<String> { connection_nodes(node, list).filter_map(field_id).collect() };
    Ok(ProjectView {
        id: string_at(node, "id")?,
        number: node
            .get("number")
            .and_then(Value::as_u64)
            .ok_or_else(|| unexpected("`number` is missing"))?,
        name: string_at(node, "name")?,
        layout,
        filter: text_at(node, "filter").filter(|filter| !filter.trim().is_empty()),
        group_by_fields: field_ids("groupByFields"),
        vertical_group_by_fields: field_ids("verticalGroupByFields"),
        sort_by_fields: connection_nodes(node, "sortByFields")
            .filter_map(|sort| {
                Some(ProjectSort {
                    field_id: field_id(sort.get("field")?)?,
                    descending: sort.get("direction").and_then(Value::as_str) == Some("DESC"),
                })
            })
            .collect(),
        visible_fields: field_ids("visibleFields"),
    })
}

pub(crate) fn parse_assignees(content: &Value) -> Vec<String> {
    connection_nodes(content, "assignees")
        .filter_map(|assignee| text_at(assignee, "login"))
        .collect()
}

pub(crate) fn parse_assignee_avatars(content: &Value) -> BTreeMap<String, String> {
    connection_nodes(content, "assignees")
        .filter_map(|assignee| Some((text_at(assignee, "login")?, text_at(assignee, "avatarUrl")?)))
        .collect()
}

pub(crate) fn parse_item_state(content: &Value) -> Option<ProjectItemState> {
    let state = content.get("state").and_then(Value::as_str)?;
    match content.get("__typename").and_then(Value::as_str)? {
        "Issue" => Some(
            match (state, content.get("stateReason").and_then(Value::as_str)) {
                ("OPEN", _) => ProjectItemState::Open,
                (_, Some("NOT_PLANNED" | "DUPLICATE")) => ProjectItemState::NotPlanned,
                _ => ProjectItemState::Closed,
            },
        ),
        "PullRequest" => Some(match state {
            "MERGED" => ProjectItemState::Merged,
            "CLOSED" => ProjectItemState::Closed,
            _ if content.get("isDraft").and_then(Value::as_bool) == Some(true) => {
                ProjectItemState::Draft
            }
            _ => ProjectItemState::Open,
        }),
        _ => None,
    }
}

/// Only an issue has sub-issues.
pub(crate) fn parse_sub_issues(content: &Value) -> Option<SubIssuesSummary> {
    let summary = content.get("subIssuesSummary")?;
    Some(SubIssuesSummary {
        total: summary.get("total")?.as_u64()?,
        completed: summary.get("completed")?.as_u64()?,
    })
}

pub(crate) fn parse_labels(content: &Value) -> Vec<Label> {
    connection_nodes(content, "labels")
        .filter_map(|label| {
            Some(Label {
                name: text_at(label, "name")?,
                color: text_at(label, "color").unwrap_or_default(),
                description: text_at(label, "description"),
            })
        })
        .collect()
}

/// Values of kinds a board or table does not draw, and values whose field
/// did not come back, are left out.
pub(crate) fn parse_field_values(item: &Value) -> Vec<ProjectFieldValue> {
    connection_nodes(item, "fieldValues")
        .filter_map(|node| {
            let field_id = field_id(node.get("field")?)?;
            let value = match node.get("__typename").and_then(Value::as_str)? {
                "ProjectV2ItemFieldTextValue" => ProjectValue::Text(text_at(node, "text")?),
                "ProjectV2ItemFieldNumberValue" => {
                    ProjectValue::Number(node.get("number").and_then(Value::as_f64)?)
                }
                "ProjectV2ItemFieldDateValue" => ProjectValue::Date(text_at(node, "date")?),
                "ProjectV2ItemFieldSingleSelectValue" => ProjectValue::SingleSelect {
                    option_id: text_at(node, "optionId")?,
                    name: text_at(node, "name").unwrap_or_default(),
                },
                "ProjectV2ItemFieldIterationValue" => ProjectValue::Iteration {
                    iteration_id: text_at(node, "iterationId")?,
                    title: text_at(node, "title").unwrap_or_default(),
                    start_date: text_at(node, "startDate").unwrap_or_default(),
                },
                "ProjectV2ItemFieldPullRequestValue" => {
                    let pull_requests = node.get("pullRequests")?;
                    let numbers = connection_nodes(node, "pullRequests")
                        .filter_map(|pull_request| pull_request.get("number")?.as_u64())
                        .collect::<Vec<_>>();
                    ProjectValue::PullRequests {
                        total: pull_requests
                            .get("totalCount")
                            .and_then(Value::as_u64)
                            .unwrap_or(numbers.len() as u64),
                        numbers,
                    }
                }
                "ProjectV2ItemFieldMilestoneValue" => {
                    ProjectValue::Milestone(text_at(node.get("milestone")?, "title")?)
                }
                _ => return None,
            };
            Some(ProjectFieldValue { field_id, value })
        })
        .collect()
}

fn parse_owner(node: &Value) -> GithubResult<ProjectOwner> {
    Ok(ProjectOwner {
        id: string_at(node, "id")?,
        login: string_at(node, "login")?,
        avatar_url: text_at(node, "avatarUrl"),
    })
}

/// `repository` is the answer's repository object, already checked to be
/// there. Its owner is offered only when GitHub says it is an organization.
pub(crate) fn parse_project_owners(
    data: &Value,
    repository: &Value,
) -> GithubResult<ProjectOwners> {
    let viewer = data
        .get("viewer")
        .ok_or_else(|| unexpected("`viewer` is missing"))?;
    let owner = repository
        .get("owner")
        .ok_or_else(|| unexpected("`owner` is missing"))?;
    let organization = match owner.get("__typename").and_then(Value::as_str) {
        Some("Organization") => Some(parse_owner(owner)?),
        _ => None,
    };
    Ok(ProjectOwners {
        repository_id: string_at(repository, "id")?,
        viewer: parse_owner(viewer)?,
        organization,
    })
}
