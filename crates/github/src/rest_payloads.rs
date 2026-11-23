//! The JSON shapes of the REST answers Zode reads, and their conversion to the
//! crate's model.

use serde::Deserialize;

use crate::model::{
    Comment, Issue, IssueState, Label, Milestone, RepositoryId, RepositoryPermission,
    RepositoryVisibility, StateReason, SubIssuesSummary,
};

#[derive(Deserialize)]
pub(crate) struct UserPayload {
    pub login: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct RepositoryPayload {
    pub private: bool,
    pub visibility: Option<String>,
    /// Only sent to a signed-in account.
    #[serde(default)]
    pub permissions: Option<PermissionsPayload>,
}

#[derive(Deserialize)]
pub(crate) struct PermissionsPayload {
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub maintain: bool,
    #[serde(default)]
    pub push: bool,
    #[serde(default)]
    pub triage: bool,
}

impl RepositoryPayload {
    pub(crate) fn permission(&self) -> Option<RepositoryPermission> {
        let permissions = self.permissions.as_ref()?;
        Some(if permissions.admin {
            RepositoryPermission::Admin
        } else if permissions.maintain {
            RepositoryPermission::Maintain
        } else if permissions.push {
            RepositoryPermission::Write
        } else if permissions.triage {
            RepositoryPermission::Triage
        } else {
            RepositoryPermission::Read
        })
    }
}

impl From<RepositoryPayload> for RepositoryVisibility {
    fn from(payload: RepositoryPayload) -> Self {
        match payload.visibility.as_deref() {
            Some("internal") => Self::Internal,
            Some("private") => Self::Private,
            Some("public") => Self::Public,
            _ if payload.private => Self::Private,
            _ => Self::Public,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct LabelPayload {
    pub name: String,
    #[serde(default)]
    pub color: String,
    pub description: Option<String>,
}

impl From<LabelPayload> for Label {
    fn from(payload: LabelPayload) -> Self {
        Self {
            name: payload.name,
            color: payload.color,
            description: payload.description,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct MilestonePayload {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub due_on: Option<String>,
}

impl From<MilestonePayload> for Milestone {
    fn from(payload: MilestonePayload) -> Self {
        Self {
            number: payload.number,
            title: payload.title,
            is_open: payload.state == "open",
            due_on: payload.due_on,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct IssueTypePayload {
    pub name: String,
}

#[derive(Deserialize)]
pub(crate) struct SubIssuesSummaryPayload {
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub completed: u64,
}

#[derive(Deserialize)]
pub(crate) struct IssuePayload {
    pub id: u64,
    pub node_id: String,
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub state_reason: Option<String>,
    pub user: Option<UserPayload>,
    #[serde(default)]
    pub labels: Vec<LabelPayload>,
    #[serde(default)]
    pub assignees: Vec<UserPayload>,
    pub milestone: Option<MilestonePayload>,
    #[serde(rename = "type")]
    pub issue_type: Option<IssueTypePayload>,
    /// Missing on answers from before sub-issues existed.
    #[serde(default)]
    pub sub_issues_summary: Option<SubIssuesSummaryPayload>,
    #[serde(default)]
    pub comments: u64,
    pub html_url: String,
    pub created_at: String,
    pub updated_at: String,
    /// Present only on pull requests, which the issue list also returns.
    pub pull_request: Option<serde_json::Value>,
    /// `https://api.github.com/repos/<owner>/<name>`.
    #[serde(default)]
    pub repository_url: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct SearchIssuesPayload {
    #[serde(default)]
    pub items: Vec<IssuePayload>,
}

impl IssuePayload {
    pub fn is_pull_request(&self) -> bool {
        self.pull_request.is_some()
    }

    /// Whether GitHub named this repository as the issue's. An issue that
    /// names none is not taken to be in it.
    pub fn belongs_to(&self, repository: &RepositoryId) -> bool {
        self.repository_url.as_deref().is_some_and(|url| {
            url.eq_ignore_ascii_case(&format!(
                "{}/repos/{}/{}",
                crate::rest::API_BASE,
                repository.owner(),
                repository.name()
            ))
        })
    }
}

impl From<IssuePayload> for Issue {
    fn from(payload: IssuePayload) -> Self {
        Self {
            id: payload.id,
            node_id: payload.node_id,
            number: payload.number,
            title: payload.title,
            body: payload.body,
            state: if payload.state == "closed" {
                IssueState::Closed
            } else {
                IssueState::Open
            },
            state_reason: payload.state_reason.as_deref().and_then(parse_state_reason),
            author: payload.user.map(|user| user.login),
            labels: payload.labels.into_iter().map(Label::from).collect(),
            assignee_avatars: payload
                .assignees
                .iter()
                .filter_map(|user| Some((user.login.clone(), user.avatar_url.clone()?)))
                .collect(),
            assignees: payload
                .assignees
                .into_iter()
                .map(|user| user.login)
                .collect(),
            milestone: payload.milestone.map(Milestone::from),
            issue_type: payload.issue_type.map(|issue_type| issue_type.name),
            sub_issues_summary: payload
                .sub_issues_summary
                .map(|summary| SubIssuesSummary {
                    total: summary.total,
                    completed: summary.completed,
                })
                .unwrap_or_default(),
            comment_count: payload.comments,
            html_url: payload.html_url,
            created_at: payload.created_at,
            updated_at: payload.updated_at,
        }
    }
}

fn parse_state_reason(reason: &str) -> Option<StateReason> {
    match reason {
        "completed" => Some(StateReason::Completed),
        "not_planned" => Some(StateReason::NotPlanned),
        "duplicate" => Some(StateReason::Duplicate),
        "reopened" => Some(StateReason::Reopened),
        _ => None,
    }
}

pub(crate) fn state_reason_name(reason: StateReason) -> &'static str {
    match reason {
        StateReason::Completed => "completed",
        StateReason::NotPlanned => "not_planned",
        StateReason::Duplicate => "duplicate",
        StateReason::Reopened => "reopened",
    }
}

#[derive(Deserialize)]
pub(crate) struct CommentPayload {
    pub id: u64,
    pub node_id: String,
    pub user: Option<UserPayload>,
    #[serde(default)]
    pub body: String,
    pub html_url: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<CommentPayload> for Comment {
    fn from(payload: CommentPayload) -> Self {
        Self {
            id: payload.id,
            node_id: payload.node_id,
            author: payload.user.map(|user| user.login),
            body: payload.body,
            html_url: payload.html_url,
            created_at: payload.created_at,
            updated_at: payload.updated_at,
        }
    }
}
