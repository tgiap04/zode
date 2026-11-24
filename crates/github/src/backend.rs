use futures::future::BoxFuture;

use crate::error::GithubResult;
use crate::model::{
    Comment, GithubUser, Issue, IssueFilter, IssueType, IssueUpdate, Label, Milestone, NewIssue,
    Page, ParentIssue, ProjectV2, ProjectV2Item, PullRequest, PullRequestStateFilter, RateLimits,
    RepositoryId, RepositoryPermission, RepositoryVisibility, SubIssuePosition, Viewer,
};
use crate::projects::{ProjectDetails, ProjectFieldInput, ProjectOwners};
use crate::pull_requests::{MergeMethod, PullRequestDetails};

/// A future that owns everything it needs, so it can run on any executor.
pub type GithubFuture<T> = BoxFuture<'static, GithubResult<T>>;

/// Everything the GitHub tab reads from or writes to GitHub, for one token.
///
/// Lists take an opaque cursor from the previous page (`None` for the first).
/// Writes are spaced out by the token's [`crate::WriteQueue`], so a caller can
/// fire them without counting. A write's place in the queue is taken when it
/// is asked for and kept until its future finishes or is dropped: poll it, or
/// drop it, because one kept alive but never polled holds back every later
/// write of the token.
pub trait GithubBackend: Send + Sync {
    /// The account behind the token, and its scopes when GitHub tells them.
    fn viewer(&self) -> GithubFuture<Viewer>;

    /// What GitHub reported on the latest answers, without asking it again.
    fn rate_limits(&self) -> RateLimits;

    fn repository_visibility(
        &self,
        repository: &RepositoryId,
    ) -> GithubFuture<RepositoryVisibility>;

    /// The signed-in account's role in the repository; `None` when GitHub's
    /// answer did not say.
    fn repository_permission(
        &self,
        repository: &RepositoryId,
    ) -> GithubFuture<Option<RepositoryPermission>>;

    /// Issues only: GitHub's issue list also holds pull requests, which are
    /// left out here.
    fn list_issues(
        &self,
        repository: &RepositoryId,
        filter: &IssueFilter,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Issue>>;

    /// GitHub's Search API, which has its own and much smaller rate limit
    /// (30 a minute), so it is only for an explicit search. Kept to
    /// `repository`: whatever in `text` would widen it to other repositories
    /// is dropped. Pull requests are left out.
    fn search_issues(
        &self,
        repository: &RepositoryId,
        filter: &IssueFilter,
        text: &str,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Issue>>;

    fn issue(&self, repository: &RepositoryId, number: u64) -> GithubFuture<Issue>;

    fn create_issue(&self, repository: &RepositoryId, issue: NewIssue) -> GithubFuture<Issue>;

    fn update_issue(
        &self,
        repository: &RepositoryId,
        number: u64,
        update: IssueUpdate,
    ) -> GithubFuture<Issue>;

    fn list_comments(
        &self,
        repository: &RepositoryId,
        number: u64,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Comment>>;

    fn add_comment(
        &self,
        repository: &RepositoryId,
        number: u64,
        body: String,
    ) -> GithubFuture<Comment>;

    fn list_labels(
        &self,
        repository: &RepositoryId,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Label>>;

    /// Adds to the issue's labels, keeping the ones already there. Returns the
    /// issue's labels afterwards.
    fn add_labels(
        &self,
        repository: &RepositoryId,
        number: u64,
        labels: Vec<String>,
    ) -> GithubFuture<Vec<Label>>;

    fn remove_label(
        &self,
        repository: &RepositoryId,
        number: u64,
        label: String,
    ) -> GithubFuture<()>;

    fn add_assignees(
        &self,
        repository: &RepositoryId,
        number: u64,
        logins: Vec<String>,
    ) -> GithubFuture<Issue>;

    fn remove_assignees(
        &self,
        repository: &RepositoryId,
        number: u64,
        logins: Vec<String>,
    ) -> GithubFuture<Issue>;

    /// Up to 20 accounts the issues of `repository` can be assigned to whose
    /// login or name matches `query`; an empty `query` gives the first 20.
    fn assignable_users(
        &self,
        repository: &RepositoryId,
        query: &str,
    ) -> GithubFuture<Vec<GithubUser>>;

    fn list_milestones(
        &self,
        repository: &RepositoryId,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Milestone>>;

    /// `None` clears the milestone.
    fn set_milestone(
        &self,
        repository: &RepositoryId,
        number: u64,
        milestone: Option<u64>,
    ) -> GithubFuture<Issue>;

    fn list_sub_issues(
        &self,
        repository: &RepositoryId,
        number: u64,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Issue>>;

    /// `sub_issue_id` is the numeric [`Issue::id`], not its number.
    fn add_sub_issue(
        &self,
        repository: &RepositoryId,
        parent_number: u64,
        sub_issue_id: u64,
    ) -> GithubFuture<()>;

    fn remove_sub_issue(
        &self,
        repository: &RepositoryId,
        parent_number: u64,
        sub_issue_id: u64,
    ) -> GithubFuture<()>;

    /// Empty when the owner has no issue types.
    fn list_issue_types(&self, repository: &RepositoryId) -> GithubFuture<Vec<IssueType>>;

    /// Both ids are GraphQL node ids; `None` clears the type.
    fn set_issue_type(
        &self,
        issue_node_id: String,
        issue_type_id: Option<String>,
    ) -> GithubFuture<()>;

    /// `None` when the issue has no parent, or none the token can see.
    fn issue_parent(
        &self,
        repository: &RepositoryId,
        number: u64,
    ) -> GithubFuture<Option<ParentIssue>>;

    /// Makes the issue a sub-issue of `parent_number`, taking it from the
    /// parent it has, if any. `sub_issue_id` is the numeric [`Issue::id`].
    fn replace_parent(
        &self,
        repository: &RepositoryId,
        parent_number: u64,
        sub_issue_id: u64,
    ) -> GithubFuture<()>;

    /// Moves a sub-issue among its siblings. All ids are GraphQL node ids.
    fn reprioritize_sub_issue(
        &self,
        parent_node_id: String,
        sub_issue_node_id: String,
        position: SubIssuePosition,
    ) -> GithubFuture<()>;

    /// Projects (v2) linked to the repository.
    fn list_projects(
        &self,
        repository: &RepositoryId,
        cursor: Option<String>,
    ) -> GithubFuture<Page<ProjectV2>>;

    fn list_project_items(
        &self,
        project_id: String,
        cursor: Option<String>,
    ) -> GithubFuture<Page<ProjectV2Item>>;

    /// The project's fields and saved views. `project_id` is the node id.
    fn project_details(&self, project_id: String) -> GithubFuture<ProjectDetails>;

    /// Sets the item's value in a text, number, date, single-select or
    /// iteration field; `None` clears it. All ids are node ids.
    fn set_project_item_field(
        &self,
        project_id: String,
        item_id: String,
        field_id: String,
        value: Option<ProjectFieldInput>,
    ) -> GithubFuture<()>;

    /// Puts the item right after `after_id` in the project's own order, or
    /// first when `None`. The order is the project's, not a column's.
    fn move_project_item(
        &self,
        project_id: String,
        item_id: String,
        after_id: Option<String>,
    ) -> GithubFuture<()>;

    /// Adds an issue or pull request (`content_id`, a node id) to the project
    /// and returns the item's id; content already there answers with its
    /// item. GitHub cannot set a field in the same call.
    fn add_project_item(&self, project_id: String, content_id: String) -> GithubFuture<String>;

    /// Takes the item out of the project; the issue or pull request stays.
    fn delete_project_item(&self, project_id: String, item_id: String) -> GithubFuture<()>;

    /// Node ids a new project linked to the repository needs: the
    /// repository's, the signed-in account's, and the owning organization's
    /// when an organization owns the repository.
    fn project_owners(&self, repository: &RepositoryId) -> GithubFuture<ProjectOwners>;

    /// Creates a project under `owner_id` (a user or organization node id)
    /// linked to the repository `repository_id`. GitHub adds the default
    /// Status field itself.
    fn create_project(
        &self,
        owner_id: String,
        title: String,
        repository_id: String,
    ) -> GithubFuture<ProjectV2>;

    fn list_pull_requests(
        &self,
        repository: &RepositoryId,
        state: PullRequestStateFilter,
        cursor: Option<String>,
    ) -> GithubFuture<Page<PullRequest>>;

    fn pull_request(&self, repository: &RepositoryId, number: u64) -> GithubFuture<PullRequest>;

    /// The pull request with its checks, reviews, first page of changed
    /// files, and what its repository lets a merge do.
    fn pull_request_details(
        &self,
        repository: &RepositoryId,
        number: u64,
    ) -> GithubFuture<PullRequestDetails>;

    /// Merges only while the head is still `expected_head_oid`: GitHub
    /// refuses the merge when someone pushed since it was read.
    fn merge_pull_request(
        &self,
        pull_request_id: String,
        method: MergeMethod,
        expected_head_oid: String,
    ) -> GithubFuture<()>;

    /// Deletes `refs/heads/{branch}` only while it still points at
    /// `expected_oid`. `repository_node_id` is the GraphQL node id.
    fn delete_branch_at(
        &self,
        repository_node_id: String,
        branch: String,
        expected_oid: String,
    ) -> GithubFuture<()>;
}
