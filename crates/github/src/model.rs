use std::collections::BTreeMap;
use std::fmt;

/// `owner/name` of a repository on github.com, both parts checked so they can
/// be put in a URL path as they are.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RepositoryId {
    owner: String,
    name: String,
}

impl RepositoryId {
    /// `None` unless both parts are made of `[A-Za-z0-9._-]` and neither is
    /// `.` or `..`, which would walk the URL path.
    pub fn new(owner: &str, name: &str) -> Option<Self> {
        (is_valid_path_part(owner) && is_valid_path_part(name)).then(|| Self {
            owner: owner.to_string(),
            name: name.to_string(),
        })
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

impl fmt::Display for RepositoryId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.owner, self.name)
    }
}

const MAX_PATH_PART_LENGTH: usize = 100;

pub(crate) fn is_valid_path_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= MAX_PATH_PART_LENGTH
        && part != "."
        && part != ".."
        && part
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
}

/// The scopes a classic token was granted. Fine-grained tokens and GitHub App
/// tokens send no `X-OAuth-Scopes` header, so their scopes are not known and
/// must not be read as "none".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum OAuthScopes {
    #[default]
    Unknown,
    Granted(Vec<String>),
}

impl OAuthScopes {
    /// `None` when the scopes are unknown.
    pub fn includes(&self, scope: &str) -> Option<bool> {
        match self {
            Self::Unknown => None,
            Self::Granted(granted) => Some(
                granted
                    .iter()
                    .any(|granted| granted == scope || implied_by(granted).contains(&scope)),
            ),
        }
    }
}

/// Scopes a broader scope grants without listing them.
fn implied_by(scope: &str) -> &'static [&'static str] {
    match scope {
        "repo" => &[
            "public_repo",
            "repo:status",
            "repo_deployment",
            "repo:invite",
        ],
        "project" => &["read:project"],
        "admin:org" => &["write:org", "read:org"],
        "write:org" => &["read:org"],
        "user" => &["read:user", "user:email", "user:follow"],
        _ => &[],
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Viewer {
    pub login: String,
    pub scopes: OAuthScopes,
}

/// One rate-limit bucket as GitHub last reported it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateLimit {
    pub limit: u64,
    pub remaining: u64,
    pub used: u64,
    /// Unix time in seconds.
    pub reset_at: u64,
}

/// Who can see a repository. `Internal` is an enterprise repository visible
/// to the enterprise's members.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepositoryVisibility {
    Public,
    Private,
    Internal,
}

/// The signed-in account's role in a repository, lowest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RepositoryPermission {
    Read,
    Triage,
    Write,
    Maintain,
    Admin,
}

impl RepositoryPermission {
    /// Labels, assignees, milestones, and closing or reopening anyone's
    /// issue. Below this GitHub drops those fields without an error.
    pub fn can_triage(self) -> bool {
        self >= Self::Triage
    }
}

/// REST and GraphQL are separate buckets. The token may be shared with other
/// tools (the GitHub CLI's is), so these can drop without Zode doing anything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RateLimits {
    pub rest: Option<RateLimit>,
    pub graphql: Option<RateLimit>,
}

/// One page of a list. `next_cursor` is opaque; pass it back to get the next
/// page, `None` means this was the last one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

impl<T> Default for Page<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            next_cursor: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueState {
    Open,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateReason {
    Completed,
    NotPlanned,
    Duplicate,
    Reopened,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub name: String,
    pub color: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Milestone {
    pub number: u64,
    pub title: String,
    pub is_open: bool,
    pub due_on: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueType {
    /// GraphQL node id.
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub is_enabled: bool,
}

/// GitHub's limits on sub-issues.
pub const MAX_SUB_ISSUES_PER_PARENT: u64 = 100;
pub const MAX_SUB_ISSUE_DEPTH: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SubIssuesSummary {
    pub total: u64,
    pub completed: u64,
}

/// An issue's parent, which may live in another repository of the same
/// owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParentIssue {
    /// Numeric id, which the REST sub-issue endpoints take.
    pub id: u64,
    pub node_id: String,
    pub number: u64,
    pub title: String,
    pub state: IssueState,
    pub repository: RepositoryId,
}

/// Where a sub-issue goes among its siblings; each names a sibling's node id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubIssuePosition {
    After(String),
    Before(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    /// Numeric id, which the REST sub-issue endpoints take.
    pub id: u64,
    pub node_id: String,
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: IssueState,
    pub state_reason: Option<StateReason>,
    pub author: Option<String>,
    pub labels: Vec<Label>,
    pub assignees: Vec<String>,
    /// Each assignee's avatar URL as GitHub sent it, by login. The UI decides
    /// whether one is safe to load.
    pub assignee_avatars: BTreeMap<String, String>,
    pub milestone: Option<Milestone>,
    pub issue_type: Option<String>,
    /// All zero when the issue has no sub-issues.
    pub sub_issues_summary: SubIssuesSummary,
    pub comment_count: u64,
    pub html_url: String,
    pub created_at: String,
    pub updated_at: String,
}

/// An account an issue can be assigned to, as the assignee picker shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubUser {
    pub login: String,
    /// The profile name; `None` when the account has not set one.
    pub name: Option<String>,
    /// As GitHub sent it. The UI decides whether it is safe to load.
    pub avatar_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub id: u64,
    pub node_id: String,
    pub author: Option<String>,
    pub body: String,
    pub html_url: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IssueStateFilter {
    #[default]
    Open,
    Closed,
    All,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IssueFilter {
    pub state: IssueStateFilter,
    pub labels: Vec<String>,
    /// A login, `none` or `*`.
    pub assignee: Option<String>,
    /// The login that opened the issue.
    pub creator: Option<String>,
    /// A login mentioned in the issue or its comments.
    pub mentioned: Option<String>,
    /// A milestone number.
    pub milestone: Option<u64>,
    /// An issue type's name, `none` or `*`.
    pub issue_type: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewIssue {
    pub title: String,
    pub body: Option<String>,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub milestone: Option<u64>,
}

/// Only the fields that are `Some` are sent. Labels and assignees are not
/// here on purpose: the REST update replaces the whole set, which would drop
/// whatever someone else added meanwhile, so they have their own add/remove
/// calls.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IssueUpdate {
    pub title: Option<String>,
    pub body: Option<String>,
    pub state: Option<IssueState>,
    pub state_reason: Option<StateReason>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectV2 {
    pub id: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub closed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectItemContent {
    Issue {
        number: u64,
        title: String,
        repository: String,
    },
    PullRequest {
        number: u64,
        title: String,
        repository: String,
    },
    DraftIssue {
        title: String,
    },
    /// An item from a repository the token cannot read.
    Redacted,
}

/// Where an issue or a pull request on a project stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectItemState {
    Open,
    /// An issue closed as completed, or a pull request closed unmerged.
    Closed,
    /// An issue closed as not planned or as a duplicate.
    NotPlanned,
    /// An open draft pull request.
    Draft,
    Merged,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectV2Item {
    pub id: String,
    pub content: ProjectItemContent,
    /// Logins; empty for a hidden item.
    pub assignees: Vec<String>,
    /// Each assignee's avatar URL as GitHub sent it, by login. The UI decides
    /// whether one is safe to load.
    pub assignee_avatars: BTreeMap<String, String>,
    /// `None` for a draft issue and a hidden item.
    pub state: Option<ProjectItemState>,
    /// An issue's sub-issues; `None` for anything else.
    pub sub_issues: Option<SubIssuesSummary>,
    pub labels: Vec<Label>,
    /// Only the kinds a board or table draws; the rest are left out.
    pub field_values: Vec<crate::projects::ProjectFieldValue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullRequestState {
    Open,
    Closed,
    Merged,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PullRequestStateFilter {
    #[default]
    Open,
    Closed,
    Merged,
    All,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequest {
    pub id: String,
    pub number: u64,
    pub title: String,
    pub state: PullRequestState,
    pub is_draft: bool,
    pub author: Option<String>,
    pub head_ref_name: String,
    /// The head commit when this was read; a merge is guarded by it.
    pub head_ref_oid: String,
    pub base_ref_name: String,
    pub is_cross_repository: bool,
    pub url: String,
    pub updated_at: String,
    /// `None` when the head commit has no checks or statuses.
    pub checks: Option<crate::pull_requests::ChecksState>,
    /// `None` when the base branch requires no review.
    pub review_decision: Option<crate::pull_requests::ReviewDecision>,
    /// Logins asked to review; teams are left out.
    pub requested_reviewers: Vec<String>,
}
