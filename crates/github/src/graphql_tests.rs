use serde_json::json;

use crate::error::GithubError;
use crate::graphql::{
    Partial, data_or_error, data_or_partial_error, next_cursor, parse_project_item,
};
use crate::graphql_projects::parse_project_details;
use crate::model::{ProjectItemContent, ProjectItemState, ProjectV2Item, SubIssuesSummary};
use crate::projects::{ProjectFieldKind, ProjectFieldValue, ProjectValue};

#[test]
fn data_without_errors_is_returned() {
    let data = data_or_error(r#"{"data":{"viewer":{"login":"octocat"}}}"#);
    assert_eq!(data, Ok(json!({"viewer": {"login": "octocat"}})));
}

#[test]
fn an_insufficient_scopes_error_names_the_scope() {
    let body = json!({
        "data": null,
        "errors": [{
            "type": "INSUFFICIENT_SCOPES",
            "message": "Your token has not been granted the required scopes to execute this query. The 'projectsV2' field requires one of the following scopes: ['read:project'], but your token has only been granted the: ['gist', 'read:org', 'repo'] scopes. Please modify your token's scopes at: https://github.com/settings/tokens."
        }]
    });
    assert_eq!(
        data_or_error(&body.to_string()),
        Err(GithubError::MissingScope {
            scope: "read:project".to_string()
        })
    );
}

#[test]
fn not_found_and_forbidden_errors_are_mapped() {
    let not_found = json!({
        "data": {"repository": null},
        "errors": [{"type": "NOT_FOUND", "message": "Could not resolve to a Repository with the name 'o/r'."}]
    });
    assert_eq!(
        data_or_error(&not_found.to_string()),
        Err(GithubError::NotFound)
    );
    let forbidden = json!({
        "errors": [{"type": "FORBIDDEN", "message": "Resource not accessible by integration"}]
    });
    assert_eq!(
        data_or_error(&forbidden.to_string()),
        Err(GithubError::Forbidden {
            message: "Resource not accessible by integration".to_string()
        })
    );
    let restricted = json!({
        "errors": [{"type": "FORBIDDEN", "message": "Although you appear to have the correct authorization credentials, the `acme` organization has enabled OAuth App access restrictions, meaning that data access to third-parties is limited."}]
    });
    assert_eq!(
        data_or_error(&restricted.to_string()),
        Err(GithubError::OrgRestricted {
            org: Some("acme".to_string())
        })
    );
    let limited =
        json!({"errors": [{"type": "RATE_LIMITED", "message": "API rate limit exceeded"}]});
    assert_eq!(
        data_or_error(&limited.to_string()),
        Err(GithubError::RateLimited {
            reset_at: None,
            retry_after: None
        })
    );
}

#[test]
fn an_unknown_error_keeps_its_message() {
    let body = json!({"errors": [{"message": "Something went wrong while executing your query."}]});
    assert_eq!(
        data_or_error(&body.to_string()),
        Err(GithubError::UnexpectedResponse {
            message: "Something went wrong while executing your query.".to_string()
        })
    );
}

#[test]
fn a_body_that_is_not_json_is_unexpected() {
    assert!(matches!(
        data_or_error("<html>"),
        Err(GithubError::UnexpectedResponse { .. })
    ));
    assert!(matches!(
        data_or_error(r#"{"data":null}"#),
        Err(GithubError::UnexpectedResponse { .. })
    ));
}

#[test]
fn the_cursor_is_kept_only_while_there_is_a_next_page() {
    let more = json!({"pageInfo": {"hasNextPage": true, "endCursor": "Y3Vyc29y"}});
    assert_eq!(next_cursor(&more).as_deref(), Some("Y3Vyc29y"));
    let last = json!({"pageInfo": {"hasNextPage": false, "endCursor": "Y3Vyc29y"}});
    assert_eq!(next_cursor(&last), None);
    assert_eq!(next_cursor(&json!({})), None);
}

#[test]
fn project_items_of_every_kind_are_read() {
    let issue = json!({
        "id": "PVTI_1", "type": "ISSUE",
        "content": {"__typename": "Issue", "number": 7, "title": "Bug", "repository": {"nameWithOwner": "octo/repo"}}
    });
    assert_eq!(
        parse_project_item(&issue),
        Ok(ProjectV2Item {
            id: "PVTI_1".to_string(),
            content: ProjectItemContent::Issue {
                number: 7,
                title: "Bug".to_string(),
                repository: "octo/repo".to_string(),
            },
            assignees: Vec::new(),
            assignee_avatars: Default::default(),
            state: None,
            sub_issues: None,
            labels: Vec::new(),
            field_values: Vec::new(),
        })
    );
    let pull_request = json!({
        "id": "PVTI_2", "type": "PULL_REQUEST",
        "content": {"__typename": "PullRequest", "number": 8, "title": "Fix", "repository": {"nameWithOwner": "octo/repo"}}
    });
    assert!(matches!(
        parse_project_item(&pull_request).map(|item| item.content),
        Ok(ProjectItemContent::PullRequest { number: 8, .. })
    ));
    let draft = json!({
        "id": "PVTI_3", "type": "DRAFT_ISSUE",
        "content": {"__typename": "DraftIssue", "title": "Idea"}
    });
    assert_eq!(
        parse_project_item(&draft).map(|item| item.content),
        Ok(ProjectItemContent::DraftIssue {
            title: "Idea".to_string()
        })
    );
}

#[test]
fn a_project_item_carries_its_state_and_its_assignees_faces() {
    let item = |content: serde_json::Value| {
        parse_project_item(&json!({"id": "PVTI_9", "type": "ISSUE", "content": content}))
    };
    let issue = |state: &str, reason: serde_json::Value| {
        item(json!({
            "__typename": "Issue", "number": 7, "title": "Bug",
            "repository": {"nameWithOwner": "octo/repo"},
            "state": state, "stateReason": reason,
            "assignees": {"nodes": [
                {"login": "octocat", "avatarUrl": "https://avatars.githubusercontent.com/u/1"},
                {"login": "ghost", "avatarUrl": null}
            ]}
        }))
    };
    let open = issue("OPEN", json!(null)).expect("an open issue");
    assert_eq!(open.state, Some(ProjectItemState::Open));
    assert_eq!(
        open.assignees,
        vec!["octocat".to_string(), "ghost".to_string()]
    );
    assert_eq!(
        open.assignee_avatars.get("octocat").map(String::as_str),
        Some("https://avatars.githubusercontent.com/u/1")
    );
    assert_eq!(open.assignee_avatars.get("ghost"), None);
    assert_eq!(
        issue("CLOSED", json!("COMPLETED")).map(|item| item.state),
        Ok(Some(ProjectItemState::Closed))
    );
    assert_eq!(
        issue("CLOSED", json!("NOT_PLANNED")).map(|item| item.state),
        Ok(Some(ProjectItemState::NotPlanned))
    );
    assert_eq!(
        issue("CLOSED", json!("DUPLICATE")).map(|item| item.state),
        Ok(Some(ProjectItemState::NotPlanned))
    );
    let pull_request = |state: &str, draft: bool| {
        item(json!({
            "__typename": "PullRequest", "number": 8, "title": "Fix",
            "repository": {"nameWithOwner": "octo/repo"},
            "state": state, "isDraft": draft
        }))
        .map(|item| item.state)
    };
    assert_eq!(
        pull_request("OPEN", false),
        Ok(Some(ProjectItemState::Open))
    );
    assert_eq!(
        pull_request("OPEN", true),
        Ok(Some(ProjectItemState::Draft))
    );
    assert_eq!(
        pull_request("MERGED", false),
        Ok(Some(ProjectItemState::Merged))
    );
    assert_eq!(
        pull_request("CLOSED", false),
        Ok(Some(ProjectItemState::Closed))
    );
    assert_eq!(
        item(json!({"__typename": "DraftIssue", "title": "Idea"})).map(|item| item.state),
        Ok(None)
    );
}

#[test]
fn a_redacted_item_is_kept_as_redacted() {
    let redacted = json!({"id": "PVTI_4", "type": "REDACTED", "content": null});
    assert_eq!(
        parse_project_item(&redacted),
        Ok(ProjectV2Item {
            id: "PVTI_4".to_string(),
            content: ProjectItemContent::Redacted,
            assignees: Vec::new(),
            assignee_avatars: Default::default(),
            state: None,
            sub_issues: None,
            labels: Vec::new(),
            field_values: Vec::new(),
        })
    );
}

#[test]
fn only_reads_keep_data_around_an_error_inside_one_list_item() {
    let nested = json!({
        "data": {"node": {"items": {"nodes": [null]}}},
        "errors": [{"type": "FORBIDDEN", "path": ["node", "items", "nodes", 0, "content"], "message": "hidden"}]
    })
    .to_string();
    assert_eq!(
        data_or_partial_error(&nested, Partial::KeepNestedNodes),
        Ok(json!({"node": {"items": {"nodes": [null]}}}))
    );
    assert!(
        data_or_partial_error(&nested, Partial::Refuse).is_err(),
        "a mutation stays strict"
    );
    let path_less = json!({
        "data": {"node": {"items": {"nodes": []}}},
        "errors": [{"type": "FORBIDDEN", "message": "hidden"}]
    })
    .to_string();
    assert!(data_or_partial_error(&path_less, Partial::KeepNestedNodes).is_err());
    let one_bad_of_two = json!({
        "data": {"node": {"items": {"nodes": [null]}}},
        "errors": [
            {"type": "FORBIDDEN", "path": ["node", "items", "nodes", 0], "message": "hidden"},
            {"type": "FORBIDDEN", "path": ["node", "items"], "message": "hidden"}
        ]
    })
    .to_string();
    assert!(data_or_partial_error(&one_bad_of_two, Partial::KeepNestedNodes).is_err());
}

#[test]
fn linked_pull_requests_and_sub_issue_progress_are_read() {
    let item = parse_project_item(&json!({
        "id": "PVTI_5", "type": "ISSUE",
        "content": {
            "__typename": "Issue", "number": 7, "title": "Epic",
            "repository": {"nameWithOwner": "octo/repo"},
            "state": "OPEN",
            "subIssuesSummary": {"total": 3, "completed": 1}
        },
        "fieldValues": {"nodes": [{
            "__typename": "ProjectV2ItemFieldPullRequestValue",
            "field": {"id": "F_prs"},
            "pullRequests": {"totalCount": 12, "nodes": [{"number": 12}, {"number": 13}]}
        }]}
    }))
    .expect("an issue with linked pull requests");
    assert_eq!(
        item.sub_issues,
        Some(SubIssuesSummary {
            total: 3,
            completed: 1
        })
    );
    assert_eq!(
        item.field_values,
        vec![ProjectFieldValue {
            field_id: "F_prs".into(),
            value: ProjectValue::PullRequests {
                numbers: vec![12, 13],
                total: 12,
            },
        }]
    );
    let details = parse_project_details(&json!({
        "fields": {"nodes": [
            {"id": "F_prs", "name": "Linked pull requests", "dataType": "LINKED_PULL_REQUESTS"},
            {"id": "F_subs", "name": "Sub-issues progress", "dataType": "SUB_ISSUES_PROGRESS"}
        ]},
        "views": {"nodes": []}
    }))
    .expect("project details");
    assert_eq!(
        details
            .fields
            .iter()
            .map(|field| field.kind.clone())
            .collect::<Vec<_>>(),
        vec![
            ProjectFieldKind::LinkedPullRequests,
            ProjectFieldKind::SubIssuesProgress
        ]
    );
}
