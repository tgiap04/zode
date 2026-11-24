//! A backend that never leaves the process.
//!
//! Lets the GitHub tab be tested without reaching GitHub: it answers the same
//! trait from what the builder put in, applies writes to that state, and
//! records each write so a test can prove one was, or was not, sent.

use std::collections::VecDeque;

use futures::FutureExt as _;
use futures::channel::oneshot;
use parking_lot::Mutex;

use crate::backend::{GithubBackend, GithubFuture};
use crate::error::{GithubError, GithubResult};
use crate::model::{
    Comment, GithubUser, Issue, IssueFilter, IssueState, IssueStateFilter, IssueType, IssueUpdate,
    Label, Milestone, NewIssue, OAuthScopes, Page, ParentIssue, ProjectItemContent, ProjectV2,
    ProjectV2Item, PullRequest, PullRequestState, PullRequestStateFilter, RateLimits, RepositoryId,
    RepositoryPermission, RepositoryVisibility, SubIssuePosition, SubIssuesSummary, Viewer,
};
use crate::projects::{
    ProjectDetails, ProjectFieldInput, ProjectFieldKind, ProjectFieldValue, ProjectOwner,
    ProjectOwners, ProjectValue,
};
use crate::pull_requests::{MergeMethod, PullRequestDetails};

/// A write that reached the fake, with its values as given.
#[derive(Clone, Debug, PartialEq)]
pub enum FakeWrite {
    CreateIssue(RepositoryId, NewIssue),
    UpdateIssue(RepositoryId, u64, IssueUpdate),
    AddComment(RepositoryId, u64, String),
    AddLabels(RepositoryId, u64, Vec<String>),
    RemoveLabel(RepositoryId, u64, String),
    AddAssignees(RepositoryId, u64, Vec<String>),
    RemoveAssignees(RepositoryId, u64, Vec<String>),
    SetMilestone(RepositoryId, u64, Option<u64>),
    AddSubIssue(RepositoryId, u64, u64),
    RemoveSubIssue(RepositoryId, u64, u64),
    SetIssueType(String, Option<String>),
    ReplaceParent(RepositoryId, u64, u64),
    ReprioritizeSubIssue(String, String, SubIssuePosition),
    /// Project, item, field, and the value (`None` clears it).
    SetProjectItemField(String, String, String, Option<ProjectFieldInput>),
    /// Project, item, and the item it now follows.
    MoveProjectItem(String, String, Option<String>),
    /// Project, and the node id of the issue or pull request added.
    AddProjectItem(String, String),
    /// Project and item.
    DeleteProjectItem(String, String),
    /// Owner node id, title, and the repository node id it is linked to.
    CreateProject(String, String, String),
    /// Pull request node id, method, and the head it was guarded by.
    MergePullRequest(String, MergeMethod, String),
    /// Repository node id, branch, and the commit it had to point at.
    DeleteBranchAt(String, String, String),
}

#[derive(Default)]
struct FakeState {
    viewer: Option<Viewer>,
    rate_limits: RateLimits,
    failure: Option<GithubError>,
    issues: Vec<(RepositoryId, Issue)>,
    comments: Vec<(RepositoryId, u64, Comment)>,
    labels: Vec<(RepositoryId, Label)>,
    milestones: Vec<(RepositoryId, Milestone)>,
    sub_issues: Vec<(RepositoryId, u64, Issue)>,
    issue_types: Vec<(RepositoryId, IssueType)>,
    projects: Vec<(RepositoryId, ProjectV2)>,
    project_items: Vec<(String, ProjectV2Item)>,
    project_details: Vec<(String, ProjectDetails)>,
    project_owners: Vec<(RepositoryId, ProjectOwners)>,
    /// Projects, project details and project items asked for.
    project_requests: usize,
    /// Failures the next project writes answer with, one each, in order.
    project_write_failures: VecDeque<GithubError>,
    /// Failures the next project field writes answer with after they took
    /// effect, as a timed-out request that did reach GitHub.
    applied_project_field_failures: VecDeque<(GithubError, oneshot::Receiver<()>)>,
    pull_requests: Vec<(RepositoryId, PullRequest)>,
    pull_request_details: Vec<(RepositoryId, PullRequestDetails)>,
    visibilities: Vec<(RepositoryId, RepositoryVisibility)>,
    permissions: Vec<(RepositoryId, RepositoryPermission)>,
    writes: Vec<FakeWrite>,
    searches: Vec<String>,
    held_issue_lists: VecDeque<oneshot::Receiver<()>>,
    /// Holds for the next project adds and removals, in order.
    held_project_writes: VecDeque<oneshot::Receiver<()>>,
    /// Holds for the next project item pages, in order.
    held_project_item_lists: VecDeque<oneshot::Receiver<()>>,
    /// Item pages still to answer with a cursor that does not move on.
    stuck_project_item_pages: usize,
    /// Item pages still to answer empty, under a cursor that moves on.
    empty_project_item_pages: usize,
    /// The cursors the next item pages claim more under, one each, in
    /// order.
    scripted_project_item_cursors: VecDeque<String>,
    held_issue_reads: VecDeque<oneshot::Receiver<()>>,
    held_comment_lists: VecDeque<oneshot::Receiver<()>>,
    held_pull_request_details: VecDeque<oneshot::Receiver<()>>,
    held_merges: VecDeque<oneshot::Receiver<()>>,
    assignable_users: Vec<(RepositoryId, GithubUser)>,
    /// Every assignable-user query asked for, in order.
    assignable_user_queries: Vec<String>,
    held_assignable_users: VecDeque<oneshot::Receiver<()>>,
}

pub struct FakeGithubBackend {
    state: Mutex<FakeState>,
    page_size: usize,
}

impl Default for FakeGithubBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// An open issue with only what a list needs filled in.
pub fn fake_issue(number: u64, title: &str) -> Issue {
    Issue {
        id: 1000 + number,
        node_id: format!("I_{number}"),
        number,
        title: title.to_string(),
        body: None,
        state: IssueState::Open,
        state_reason: None,
        author: Some("octocat".to_string()),
        labels: Vec::new(),
        assignees: Vec::new(),
        assignee_avatars: Default::default(),
        milestone: None,
        issue_type: None,
        sub_issues_summary: SubIssuesSummary::default(),
        comment_count: 0,
        html_url: format!("https://github.com/octo/repo/issues/{number}"),
        created_at: "2026-11-01T00:00:00Z".to_string(),
        updated_at: "2026-11-01T00:00:00Z".to_string(),
    }
}

pub use crate::fake_pull_requests::fake_pull_request_details;

/// An open, same-repository pull request.
pub fn fake_pull_request(number: u64, title: &str) -> PullRequest {
    PullRequest {
        id: format!("PR_{number}"),
        number,
        title: title.to_string(),
        state: PullRequestState::Open,
        is_draft: false,
        author: Some("octocat".to_string()),
        head_ref_name: format!("feature-{number}"),
        head_ref_oid: format!("{number:040x}"),
        base_ref_name: "main".to_string(),
        is_cross_repository: false,
        url: format!("https://github.com/octo/repo/pull/{number}"),
        updated_at: "2026-11-01T00:00:00Z".to_string(),
        checks: None,
        review_decision: None,
        requested_reviewers: Vec::new(),
    }
}

impl FakeGithubBackend {
    /// Signed in as `octocat` with `repo` and `read:org`, holding nothing.
    pub fn new() -> Self {
        Self {
            state: Mutex::new(FakeState {
                viewer: Some(Viewer {
                    login: "octocat".to_string(),
                    scopes: OAuthScopes::Granted(vec!["repo".to_string(), "read:org".to_string()]),
                }),
                ..FakeState::default()
            }),
            page_size: 50,
        }
    }

    fn with_state(self, change: impl FnOnce(&mut FakeState)) -> Self {
        change(&mut self.state.lock());
        self
    }

    pub fn with_viewer(self, viewer: Viewer) -> Self {
        self.with_state(|state| state.viewer = Some(viewer))
    }

    pub fn with_rate_limits(self, rate_limits: RateLimits) -> Self {
        self.with_state(|state| state.rate_limits = rate_limits)
    }

    pub fn with_page_size(mut self, page_size: usize) -> Self {
        self.page_size = page_size.max(1);
        self
    }

    /// Every call fails with `error` until [`Self::set_failure`] clears it.
    pub fn failing_with(self, error: GithubError) -> Self {
        self.with_state(|state| state.failure = Some(error))
    }

    pub fn with_issue(self, repository: &RepositoryId, issue: Issue) -> Self {
        self.with_state(|state| state.issues.push((repository.clone(), issue)))
    }

    pub fn with_comment(self, repository: &RepositoryId, number: u64, comment: Comment) -> Self {
        self.with_state(|state| state.comments.push((repository.clone(), number, comment)))
    }

    pub fn with_label(self, repository: &RepositoryId, label: Label) -> Self {
        self.with_state(|state| state.labels.push((repository.clone(), label)))
    }

    pub fn with_milestone(self, repository: &RepositoryId, milestone: Milestone) -> Self {
        self.with_state(|state| state.milestones.push((repository.clone(), milestone)))
    }

    pub fn with_sub_issue(self, repository: &RepositoryId, parent: u64, sub_issue: Issue) -> Self {
        self.with_state(|state| {
            state
                .sub_issues
                .push((repository.clone(), parent, sub_issue))
        })
    }

    pub fn with_assignable_user(self, repository: &RepositoryId, user: GithubUser) -> Self {
        self.with_state(|state| state.assignable_users.push((repository.clone(), user)))
    }

    /// Every assignable-user query asked for, in order.
    pub fn assignable_user_queries(&self) -> Vec<String> {
        self.state.lock().assignable_user_queries.clone()
    }

    /// The next assignable-user query is recorded when asked for but answers
    /// only once the returned sender fires, so a test can make an older
    /// answer land late.
    pub fn hold_next_assignable_users(&self) -> oneshot::Sender<()> {
        let (release, held) = oneshot::channel();
        self.state.lock().held_assignable_users.push_back(held);
        release
    }

    pub fn with_issue_type(self, repository: &RepositoryId, issue_type: IssueType) -> Self {
        self.with_state(|state| state.issue_types.push((repository.clone(), issue_type)))
    }

    pub fn with_project(self, repository: &RepositoryId, project: ProjectV2) -> Self {
        self.with_state(|state| state.projects.push((repository.clone(), project)))
    }

    pub fn with_project_item(self, project_id: &str, item: ProjectV2Item) -> Self {
        self.with_state(|state| state.project_items.push((project_id.to_string(), item)))
    }

    pub fn with_project_details(self, project_id: &str, details: ProjectDetails) -> Self {
        self.with_state(|state| {
            state
                .project_details
                .push((project_id.to_string(), details))
        })
    }

    /// How many project reads (projects, details, items) reached the fake.
    /// The next project write (a field value or a position) is recorded but
    /// fails with `error`; the ones after it go through.
    pub fn fail_next_project_write(&self, error: GithubError) {
        self.state.lock().project_write_failures.push_back(error);
    }

    /// Who a project linked to `repository` may be created under. Without
    /// it the viewer alone is offered, with ids made from the names.
    pub fn with_project_owners(self, repository: &RepositoryId, owners: ProjectOwners) -> Self {
        self.with_state(|state| state.project_owners.push((repository.clone(), owners)))
    }

    pub fn project_requests(&self) -> usize {
        self.state.lock().project_requests
    }

    pub fn with_pull_request(self, repository: &RepositoryId, pull_request: PullRequest) -> Self {
        self.with_state(|state| state.pull_requests.push((repository.clone(), pull_request)))
    }

    /// Also lists the pull request. One without details answers with
    /// [`fake_pull_request_details`].
    pub fn with_pull_request_details(
        self,
        repository: &RepositoryId,
        details: PullRequestDetails,
    ) -> Self {
        self.with_state(|state| {
            state
                .pull_requests
                .push((repository.clone(), details.pull_request.clone()));
            state
                .pull_request_details
                .push((repository.clone(), details));
        })
    }

    /// Someone pushing to the pull request's head: applied without being
    /// recorded as a write.
    pub fn push_to_pull_request(&self, repository: &RepositoryId, number: u64, oid: &str) {
        let mut state = self.state.lock();
        for (owner, pull_request) in &mut state.pull_requests {
            if owner == repository && pull_request.number == number {
                pull_request.head_ref_oid = oid.to_string();
            }
        }
        for (owner, details) in &mut state.pull_request_details {
            if owner == repository && details.pull_request.number == number {
                details.pull_request.head_ref_oid = oid.to_string();
                if let Some(head) = &mut details.head_branch {
                    head.oid = Some(oid.to_string());
                }
            }
        }
    }

    /// Repositories not given one read as public.
    pub fn with_repository_visibility(
        self,
        repository: &RepositoryId,
        visibility: RepositoryVisibility,
    ) -> Self {
        self.with_state(|state| state.visibilities.push((repository.clone(), visibility)))
    }

    /// Repositories not given one read as [`RepositoryPermission::Write`].
    /// Below [`RepositoryPermission::Triage`] the fake does what GitHub does:
    /// labels, assignees and milestones are dropped without an error.
    pub fn with_repository_permission(
        self,
        repository: &RepositoryId,
        permission: RepositoryPermission,
    ) -> Self {
        self.with_state(|state| state.permissions.push((repository.clone(), permission)))
    }

    /// Someone else changing the issue on GitHub: applied without being
    /// recorded as a write.
    pub fn edit_issue(
        &self,
        repository: &RepositoryId,
        number: u64,
        change: impl FnOnce(&mut Issue),
    ) {
        if let Ok(issue) = issue_mut(&mut self.state.lock(), repository, number) {
            change(issue);
        }
    }

    pub fn set_failure(&self, failure: Option<GithubError>) {
        self.state.lock().failure = failure;
    }

    /// Every search text asked for, in order.
    pub fn searches(&self) -> Vec<String> {
        self.state.lock().searches.clone()
    }

    /// The next issue list is read when asked for but answers only once the
    /// returned sender fires, so a test can make an older answer land late.
    pub fn hold_next_issue_list(&self) -> oneshot::Sender<()> {
        let (release, held) = oneshot::channel();
        self.state.lock().held_issue_lists.push_back(held);
        release
    }

    /// The next single-issue read answers only once the returned sender
    /// fires, so a test can sign out while a save reads the issue again.
    pub fn hold_next_issue_read(&self) -> oneshot::Sender<()> {
        let (release, held) = oneshot::channel();
        self.state.lock().held_issue_reads.push_back(held);
        release
    }

    /// The next comments page is read when asked for but answers only once
    /// the returned sender fires, so a test can send a comment meanwhile.
    pub fn hold_next_comment_list(&self) -> oneshot::Sender<()> {
        let (release, held) = oneshot::channel();
        self.state.lock().held_comment_lists.push_back(held);
        release
    }

    /// The next pull request details read answers only once the returned
    /// sender fires.
    pub fn hold_next_pull_request_details(&self) -> oneshot::Sender<()> {
        let (release, held) = oneshot::channel();
        self.state.lock().held_pull_request_details.push_back(held);
        release
    }

    /// The next merge reaches the fake when asked for but answers only once
    /// the returned sender fires.
    pub fn hold_next_merge(&self) -> oneshot::Sender<()> {
        let (release, held) = oneshot::channel();
        self.state.lock().held_merges.push_back(held);
        release
    }

    /// The next item pages claim more under `cursors`, one each, in order,
    /// whatever they were asked with.
    pub fn with_project_item_cursors(self, cursors: &[&str]) -> Self {
        let cursors = cursors.iter().map(|cursor| cursor.to_string()).collect();
        self.with_state(|state| state.scripted_project_item_cursors = cursors)
    }

    /// The next project add or removal reaches the fake when asked for but
    /// answers only once the returned sender fires, so a test can change
    /// what is shown before the answer lands.
    pub fn hold_next_project_write(&self) -> oneshot::Sender<()> {
        let (release, held) = oneshot::channel();
        self.state.lock().held_project_writes.push_back(held);
        release
    }

    /// The next project field write takes effect at once, then answers with
    /// `error` once the returned sender fires: the outcome of a request that
    /// timed out after reaching GitHub.
    pub fn fail_next_project_field_write_after_applying(
        &self,
        error: GithubError,
    ) -> oneshot::Sender<()> {
        let (release, held) = oneshot::channel();
        self.state
            .lock()
            .applied_project_field_failures
            .push_back((error, held));
        release
    }

    /// The next project item page is read when asked for but answers only
    /// once the returned sender fires, so a test can add or remove an item
    /// meanwhile.
    pub fn hold_next_project_item_list(&self) -> oneshot::Sender<()> {
        let (release, held) = oneshot::channel();
        self.state.lock().held_project_item_lists.push_back(held);
        release
    }

    /// The next `pages` item pages claim more after them under a cursor
    /// that never moves on: the first answers `0`, and `0` answers `0`
    /// again, holding no items when the project has none.
    pub fn with_stuck_project_item_cursor(self, pages: usize) -> Self {
        self.with_state(|state| state.stuck_project_item_pages = pages)
    }

    /// The first `pages` item pages hold nothing but lead on, as a page
    /// whose items are all hidden does.
    pub fn with_empty_project_item_pages(self, pages: usize) -> Self {
        self.with_state(|state| state.empty_project_item_pages = pages)
    }

    /// Someone else changing the pull request on GitHub (retargeting it,
    /// a review that blocks it): applied without being recorded as a write.
    pub fn edit_pull_request_details(
        &self,
        repository: &RepositoryId,
        number: u64,
        change: impl Fn(&mut PullRequestDetails),
    ) {
        let mut state = self.state.lock();
        for (owner, details) in &mut state.pull_request_details {
            if owner == repository && details.pull_request.number == number {
                change(details);
            }
        }
    }

    fn held_project_write<T: Send + 'static>(&self, answer: GithubFuture<T>) -> GithubFuture<T> {
        let hold = self.state.lock().held_project_writes.pop_front();
        held_until(hold, answer)
    }

    pub fn writes(&self) -> Vec<FakeWrite> {
        self.state.lock().writes.clone()
    }

    fn answer<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut FakeState) -> GithubResult<T>,
    ) -> GithubFuture<T> {
        let mut state = self.state.lock();
        let result = match &state.failure {
            Some(failure) => Err(failure.clone()),
            None => work(&mut state),
        };
        futures::future::ready(result).boxed()
    }

    fn write<T: Send + 'static>(
        &self,
        write: FakeWrite,
        work: impl FnOnce(&mut FakeState) -> GithubResult<T>,
    ) -> GithubFuture<T> {
        self.answer(|state| {
            state.writes.push(write);
            work(state)
        })
    }

    fn page<T: Clone>(&self, items: Vec<T>, cursor: Option<String>) -> GithubResult<Page<T>> {
        let start = match cursor {
            None => 0,
            Some(cursor) => page_offset(&cursor).parse::<usize>().map_err(|_| {
                crate::error::unexpected(format!("`{cursor}` is not a page cursor"))
            })?,
        };
        let end = start.saturating_add(self.page_size).min(items.len());
        Ok(Page {
            items: items.get(start..end).unwrap_or_default().to_vec(),
            next_cursor: (end < items.len()).then(|| end.to_string()),
        })
    }
}

/// `answer`, given only once `hold` fires when there is one.
fn held_until<T: Send + 'static>(
    hold: Option<oneshot::Receiver<()>>,
    answer: GithubFuture<T>,
) -> GithubFuture<T> {
    match hold {
        None => answer,
        Some(release) => async move {
            release.await.ok();
            answer.await
        }
        .boxed(),
    }
}

/// A cursor is an item offset, with `~n` after it when an empty page made
/// it; the suffix only makes each such cursor new.
fn page_offset(cursor: &str) -> &str {
    cursor.split_once('~').map_or(cursor, |(offset, _)| offset)
}

/// The value an option or iteration id stands for, as a read would show it.
fn fake_field_value(
    state: &FakeState,
    project_id: &str,
    field_id: &str,
    input: ProjectFieldInput,
) -> GithubResult<ProjectValue> {
    let field = state
        .project_details
        .iter()
        .find(|(project, _)| project == project_id)
        .and_then(|(_, details)| details.field(field_id))
        .ok_or(GithubError::NotFound)?;
    match (&field.kind, input) {
        (ProjectFieldKind::Text, ProjectFieldInput::Text { text }) => Ok(ProjectValue::Text(text)),
        (ProjectFieldKind::Number, ProjectFieldInput::Number { number }) => {
            Ok(ProjectValue::Number(number))
        }
        (ProjectFieldKind::Date, ProjectFieldInput::Date { date }) => Ok(ProjectValue::Date(date)),
        (
            ProjectFieldKind::SingleSelect(options),
            ProjectFieldInput::SingleSelect { option_id },
        ) => {
            let option = options
                .iter()
                .find(|option| option.id == option_id)
                .ok_or(GithubError::NotFound)?;
            Ok(ProjectValue::SingleSelect {
                option_id,
                name: option.name.clone(),
            })
        }
        (
            ProjectFieldKind::Iteration(iterations),
            ProjectFieldInput::Iteration { iteration_id },
        ) => {
            let iteration = iterations
                .iter()
                .find(|iteration| iteration.id == iteration_id)
                .ok_or(GithubError::NotFound)?;
            Ok(ProjectValue::Iteration {
                iteration_id,
                title: iteration.title.clone(),
                start_date: iteration.start_date.clone(),
            })
        }
        _ => Err(crate::error::unexpected("the value does not fit the field")),
    }
}

/// What a project item made from `content_id` shows: an issue or pull
/// request the fake holds, in any repository.
fn fake_item_content(state: &FakeState, content_id: &str) -> GithubResult<ProjectItemContent> {
    if let Some((repository, issue)) = state
        .issues
        .iter()
        .find(|(_, issue)| issue.node_id == content_id)
    {
        return Ok(ProjectItemContent::Issue {
            number: issue.number,
            title: issue.title.clone(),
            repository: repository.to_string(),
        });
    }
    state
        .pull_requests
        .iter()
        .find(|(_, pull_request)| pull_request.id == content_id)
        .map(
            |(repository, pull_request)| ProjectItemContent::PullRequest {
                number: pull_request.number,
                title: pull_request.title.clone(),
                repository: repository.to_string(),
            },
        )
        .ok_or(GithubError::NotFound)
}

/// Where the project's first item sits among every project's items.
fn position_of_first(items: &[(String, ProjectV2Item)], project_id: &str) -> usize {
    items
        .iter()
        .position(|(project, _)| project == project_id)
        .unwrap_or(items.len())
}

fn issue_mut<'a>(
    state: &'a mut FakeState,
    repository: &RepositoryId,
    number: u64,
) -> GithubResult<&'a mut Issue> {
    state
        .issues
        .iter_mut()
        .find(|(owner, issue)| owner == repository && issue.number == number)
        .map(|(_, issue)| issue)
        .ok_or(GithubError::NotFound)
}

/// The issues `filter` lets through. Mentions are not tracked, so a
/// `mentioned` filter lets through the issues whose body names the login.
fn filtered_issues(
    state: &FakeState,
    repository: &RepositoryId,
    filter: &IssueFilter,
) -> Vec<Issue> {
    of_repository(&state.issues, repository)
        .into_iter()
        .filter(|issue| match filter.state {
            IssueStateFilter::Open => issue.state == IssueState::Open,
            IssueStateFilter::Closed => issue.state == IssueState::Closed,
            IssueStateFilter::All => true,
        })
        .filter(|issue| {
            filter
                .labels
                .iter()
                .all(|wanted| issue.labels.iter().any(|label| &label.name == wanted))
        })
        .filter(|issue| {
            filter
                .assignee
                .as_ref()
                .is_none_or(|login| issue.assignees.contains(login))
        })
        .filter(|issue| {
            filter
                .creator
                .as_ref()
                .is_none_or(|login| issue.author.as_ref() == Some(login))
        })
        .filter(|issue| {
            filter.mentioned.as_ref().is_none_or(|login| {
                issue
                    .body
                    .as_deref()
                    .is_some_and(|body| body.contains(&format!("@{login}")))
            })
        })
        .filter(|issue| {
            filter.milestone.is_none_or(|wanted| {
                issue
                    .milestone
                    .as_ref()
                    .is_some_and(|milestone| milestone.number == wanted)
            })
        })
        .filter(|issue| match filter.issue_type.as_deref() {
            None => true,
            Some("none") => issue.issue_type.is_none(),
            Some("*") => issue.issue_type.is_some(),
            Some(wanted) => issue.issue_type.as_deref() == Some(wanted),
        })
        .map(|issue| summarized(state, repository, issue))
        .collect()
}

/// The issue with its sub-issue counts taken from what the fake holds, as
/// GitHub keeps them.
fn summarized(state: &FakeState, repository: &RepositoryId, mut issue: Issue) -> Issue {
    let mut summary = SubIssuesSummary::default();
    for (_, _, child) in state
        .sub_issues
        .iter()
        .filter(|(owner, parent, _)| owner == repository && *parent == issue.number)
    {
        summary.total += 1;
        if child.state == IssueState::Closed {
            summary.completed += 1;
        }
    }
    if summary.total > 0 {
        issue.sub_issues_summary = summary;
    }
    issue
}

/// Adds the issue numbered by `sub_issue_id` under `parent_number`, with
/// GitHub's checks: one parent per issue, 100 sub-issues per parent.
fn link_sub_issue(
    state: &mut FakeState,
    repository: &RepositoryId,
    parent_number: u64,
    sub_issue_id: u64,
) -> GithubResult<()> {
    let sub_issue = state
        .issues
        .iter()
        .find(|(_, issue)| issue.id == sub_issue_id)
        .map(|(_, issue)| issue.clone())
        .ok_or(GithubError::NotFound)?;
    if state
        .sub_issues
        .iter()
        .any(|(_, _, issue)| issue.id == sub_issue_id)
    {
        return Err(GithubError::Rejected {
            message: "Issue may not contain duplicate sub-issues and can only have one parent"
                .to_string(),
        });
    }
    let siblings = state
        .sub_issues
        .iter()
        .filter(|(owner, parent, _)| owner == repository && *parent == parent_number)
        .count() as u64;
    if siblings >= crate::model::MAX_SUB_ISSUES_PER_PARENT {
        return Err(GithubError::Rejected {
            message: "Parent cannot have more than 100 sub-issues".to_string(),
        });
    }
    state
        .sub_issues
        .push((repository.clone(), parent_number, sub_issue));
    Ok(())
}

fn permission_of(state: &FakeState, repository: &RepositoryId) -> RepositoryPermission {
    of_repository(&state.permissions, repository)
        .into_iter()
        .next()
        .unwrap_or(RepositoryPermission::Write)
}

fn can_triage(state: &FakeState, repository: &RepositoryId) -> bool {
    permission_of(state, repository).can_triage()
}

fn known_label(known: &[Label], name: String) -> Label {
    known
        .iter()
        .find(|label| label.name == name)
        .cloned()
        .unwrap_or(Label {
            name,
            color: "ededed".to_string(),
            description: None,
        })
}

fn of_repository<T: Clone>(items: &[(RepositoryId, T)], repository: &RepositoryId) -> Vec<T> {
    items
        .iter()
        .filter(|(owner, _)| owner == repository)
        .map(|(_, item)| item.clone())
        .collect()
}

impl GithubBackend for FakeGithubBackend {
    fn viewer(&self) -> GithubFuture<Viewer> {
        self.answer(|state| state.viewer.clone().ok_or(GithubError::NotSignedIn))
    }

    fn rate_limits(&self) -> RateLimits {
        self.state.lock().rate_limits.clone()
    }

    fn repository_visibility(
        &self,
        repository: &RepositoryId,
    ) -> GithubFuture<RepositoryVisibility> {
        self.answer(|state| {
            Ok(of_repository(&state.visibilities, repository)
                .into_iter()
                .next()
                .unwrap_or(RepositoryVisibility::Public))
        })
    }

    fn repository_permission(
        &self,
        repository: &RepositoryId,
    ) -> GithubFuture<Option<RepositoryPermission>> {
        self.answer(|state| Ok(Some(permission_of(state, repository))))
    }

    fn list_issues(
        &self,
        repository: &RepositoryId,
        filter: &IssueFilter,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Issue>> {
        let (issues, hold) = {
            let mut state = self.state.lock();
            let hold = state.held_issue_lists.pop_front();
            (filtered_issues(&state, repository, filter), hold)
        };
        let page = self.page(issues, cursor);
        held_until(hold, self.answer(|_| page))
    }

    fn search_issues(
        &self,
        repository: &RepositoryId,
        filter: &IssueFilter,
        text: &str,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Issue>> {
        let words = text
            .split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        let issues = {
            let mut state = self.state.lock();
            state.searches.push(text.to_string());
            filtered_issues(&state, repository, filter)
                .into_iter()
                .filter(|issue| {
                    let haystack = format!(
                        "{}\n{}",
                        issue.title,
                        issue.body.as_deref().unwrap_or_default()
                    )
                    .to_lowercase();
                    words.iter().all(|word| haystack.contains(word.as_str()))
                })
                .collect::<Vec<_>>()
        };
        let page = self.page(issues, cursor);
        self.answer(|_| page)
    }

    fn issue(&self, repository: &RepositoryId, number: u64) -> GithubFuture<Issue> {
        let hold = self.state.lock().held_issue_reads.pop_front();
        held_until(
            hold,
            self.answer(|state| {
                let issue = issue_mut(state, repository, number)?.clone();
                Ok(summarized(state, repository, issue))
            }),
        )
    }

    fn create_issue(&self, repository: &RepositoryId, issue: NewIssue) -> GithubFuture<Issue> {
        let write = FakeWrite::CreateIssue(repository.clone(), issue.clone());
        self.write(write, |state| {
            let number = state
                .issues
                .iter()
                .map(|(_, issue)| issue.number)
                .max()
                .unwrap_or(0)
                + 1;
            let mut created = fake_issue(number, &issue.title);
            created.body = issue.body;
            if can_triage(state, repository) {
                let known = of_repository(&state.labels, repository);
                created.labels = issue
                    .labels
                    .into_iter()
                    .map(|name| known_label(&known, name))
                    .collect();
                created.assignees = issue.assignees;
                created.milestone = issue.milestone.and_then(|wanted| {
                    of_repository(&state.milestones, repository)
                        .into_iter()
                        .find(|milestone| milestone.number == wanted)
                });
            }
            state.issues.push((repository.clone(), created.clone()));
            Ok(created)
        })
    }

    fn update_issue(
        &self,
        repository: &RepositoryId,
        number: u64,
        update: IssueUpdate,
    ) -> GithubFuture<Issue> {
        let write = FakeWrite::UpdateIssue(repository.clone(), number, update.clone());
        self.write(write, |state| {
            let issue = issue_mut(state, repository, number)?;
            if let Some(title) = update.title {
                issue.title = title;
            }
            if let Some(body) = update.body {
                issue.body = Some(body);
            }
            if let Some(issue_state) = update.state {
                issue.state = issue_state;
            }
            if update.state_reason.is_some() {
                issue.state_reason = update.state_reason;
            }
            Ok(issue.clone())
        })
    }

    fn list_comments(
        &self,
        repository: &RepositoryId,
        number: u64,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Comment>> {
        let (comments, hold) = {
            let mut state = self.state.lock();
            let hold = state.held_comment_lists.pop_front();
            let comments = state
                .comments
                .iter()
                .filter(|(owner, issue, _)| owner == repository && *issue == number)
                .map(|(_, _, comment)| comment.clone())
                .collect();
            (comments, hold)
        };
        let page = self.page(comments, cursor);
        held_until(hold, self.answer(|_| page))
    }

    fn add_comment(
        &self,
        repository: &RepositoryId,
        number: u64,
        body: String,
    ) -> GithubFuture<Comment> {
        let write = FakeWrite::AddComment(repository.clone(), number, body.clone());
        self.write(write, |state| {
            // Pull requests take comments through the issue endpoint too.
            let is_pull_request = of_repository(&state.pull_requests, repository)
                .iter()
                .any(|pull_request| pull_request.number == number);
            if !is_pull_request {
                issue_mut(state, repository, number)?.comment_count += 1;
            }
            let id = state.comments.len() as u64 + 1;
            let comment = Comment {
                id,
                node_id: format!("IC_{id}"),
                author: state.viewer.as_ref().map(|viewer| viewer.login.clone()),
                body,
                html_url: format!(
                    "https://github.com/{repository}/issues/{number}#issuecomment-{id}"
                ),
                created_at: "2026-11-01T00:00:00Z".to_string(),
                updated_at: "2026-11-01T00:00:00Z".to_string(),
            };
            state
                .comments
                .push((repository.clone(), number, comment.clone()));
            Ok(comment)
        })
    }

    fn list_labels(
        &self,
        repository: &RepositoryId,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Label>> {
        let labels = of_repository(&self.state.lock().labels, repository);
        let page = self.page(labels, cursor);
        self.answer(|_| page)
    }

    fn add_labels(
        &self,
        repository: &RepositoryId,
        number: u64,
        labels: Vec<String>,
    ) -> GithubFuture<Vec<Label>> {
        let write = FakeWrite::AddLabels(repository.clone(), number, labels.clone());
        self.write(write, |state| {
            let known = of_repository(&state.labels, repository);
            let allowed = can_triage(state, repository);
            let issue = issue_mut(state, repository, number)?;
            for name in labels {
                if !allowed || issue.labels.iter().any(|label| label.name == name) {
                    continue;
                }
                issue.labels.push(known_label(&known, name));
            }
            Ok(issue.labels.clone())
        })
    }

    fn remove_label(
        &self,
        repository: &RepositoryId,
        number: u64,
        label: String,
    ) -> GithubFuture<()> {
        let write = FakeWrite::RemoveLabel(repository.clone(), number, label.clone());
        self.write(write, |state| {
            let issue = issue_mut(state, repository, number)?;
            let before = issue.labels.len();
            issue.labels.retain(|existing| existing.name != label);
            if issue.labels.len() == before {
                return Err(GithubError::NotFound);
            }
            Ok(())
        })
    }

    fn add_assignees(
        &self,
        repository: &RepositoryId,
        number: u64,
        logins: Vec<String>,
    ) -> GithubFuture<Issue> {
        let write = FakeWrite::AddAssignees(repository.clone(), number, logins.clone());
        self.write(write, |state| {
            let allowed = can_triage(state, repository);
            let known = of_repository(&state.assignable_users, repository);
            let issue = issue_mut(state, repository, number)?;
            for login in logins {
                if allowed && !issue.assignees.contains(&login) {
                    let avatar = known
                        .iter()
                        .find(|user| user.login == login)
                        .and_then(|user| user.avatar_url.clone());
                    if let Some(avatar) = avatar {
                        issue.assignee_avatars.insert(login.clone(), avatar);
                    }
                    issue.assignees.push(login);
                }
            }
            Ok(issue.clone())
        })
    }

    fn remove_assignees(
        &self,
        repository: &RepositoryId,
        number: u64,
        logins: Vec<String>,
    ) -> GithubFuture<Issue> {
        let write = FakeWrite::RemoveAssignees(repository.clone(), number, logins.clone());
        self.write(write, |state| {
            let issue = issue_mut(state, repository, number)?;
            issue.assignees.retain(|login| !logins.contains(login));
            issue
                .assignee_avatars
                .retain(|login, _| !logins.contains(login));
            Ok(issue.clone())
        })
    }

    fn assignable_users(
        &self,
        repository: &RepositoryId,
        query: &str,
    ) -> GithubFuture<Vec<GithubUser>> {
        let hold = {
            let mut state = self.state.lock();
            state.assignable_user_queries.push(query.to_string());
            state.held_assignable_users.pop_front()
        };
        let wanted = query.to_lowercase();
        let answer = self.answer(|state| {
            Ok(of_repository(&state.assignable_users, repository)
                .into_iter()
                .filter(|user| {
                    user.login.to_lowercase().contains(&wanted)
                        || user
                            .name
                            .as_ref()
                            .is_some_and(|name| name.to_lowercase().contains(&wanted))
                })
                .take(20)
                .collect())
        });
        held_until(hold, answer)
    }

    fn list_milestones(
        &self,
        repository: &RepositoryId,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Milestone>> {
        let milestones = of_repository(&self.state.lock().milestones, repository);
        let page = self.page(milestones, cursor);
        self.answer(|_| page)
    }

    fn set_milestone(
        &self,
        repository: &RepositoryId,
        number: u64,
        milestone: Option<u64>,
    ) -> GithubFuture<Issue> {
        let write = FakeWrite::SetMilestone(repository.clone(), number, milestone);
        self.write(write, |state| {
            let found = match milestone {
                None => None,
                Some(wanted) => Some(
                    of_repository(&state.milestones, repository)
                        .into_iter()
                        .find(|milestone| milestone.number == wanted)
                        .ok_or_else(|| GithubError::Rejected {
                            message: format!("milestone {wanted} does not exist"),
                        })?,
                ),
            };
            let allowed = can_triage(state, repository);
            let issue = issue_mut(state, repository, number)?;
            if allowed {
                issue.milestone = found;
            }
            Ok(issue.clone())
        })
    }

    fn list_sub_issues(
        &self,
        repository: &RepositoryId,
        number: u64,
        cursor: Option<String>,
    ) -> GithubFuture<Page<Issue>> {
        let sub_issues = {
            let state = self.state.lock();
            state
                .sub_issues
                .iter()
                .filter(|(owner, parent, _)| owner == repository && *parent == number)
                .map(|(_, _, issue)| summarized(&state, repository, issue.clone()))
                .collect()
        };
        let page = self.page(sub_issues, cursor);
        self.answer(|_| page)
    }

    fn add_sub_issue(
        &self,
        repository: &RepositoryId,
        parent_number: u64,
        sub_issue_id: u64,
    ) -> GithubFuture<()> {
        let write = FakeWrite::AddSubIssue(repository.clone(), parent_number, sub_issue_id);
        self.write(write, |state| {
            link_sub_issue(state, repository, parent_number, sub_issue_id)
        })
    }

    fn remove_sub_issue(
        &self,
        repository: &RepositoryId,
        parent_number: u64,
        sub_issue_id: u64,
    ) -> GithubFuture<()> {
        let write = FakeWrite::RemoveSubIssue(repository.clone(), parent_number, sub_issue_id);
        self.write(write, |state| {
            state.sub_issues.retain(|(owner, parent, issue)| {
                !(owner == repository && *parent == parent_number && issue.id == sub_issue_id)
            });
            Ok(())
        })
    }

    fn list_issue_types(&self, repository: &RepositoryId) -> GithubFuture<Vec<IssueType>> {
        self.answer(|state| Ok(of_repository(&state.issue_types, repository)))
    }

    fn set_issue_type(
        &self,
        issue_node_id: String,
        issue_type_id: Option<String>,
    ) -> GithubFuture<()> {
        let write = FakeWrite::SetIssueType(issue_node_id.clone(), issue_type_id.clone());
        self.write(write, |state| {
            let name = match &issue_type_id {
                None => None,
                Some(wanted) => Some(
                    state
                        .issue_types
                        .iter()
                        .find(|(_, issue_type)| &issue_type.id == wanted)
                        .map(|(_, issue_type)| issue_type.name.clone())
                        .ok_or_else(|| GithubError::Rejected {
                            message: format!("issue type {wanted} does not exist"),
                        })?,
                ),
            };
            let issue = state
                .issues
                .iter_mut()
                .find(|(_, issue)| issue.node_id == issue_node_id)
                .map(|(_, issue)| issue)
                .ok_or(GithubError::NotFound)?;
            issue.issue_type = name;
            Ok(())
        })
    }

    fn issue_parent(
        &self,
        repository: &RepositoryId,
        number: u64,
    ) -> GithubFuture<Option<ParentIssue>> {
        self.answer(|state| {
            let Some(parent_number) = state
                .sub_issues
                .iter()
                .find(|(owner, _, issue)| owner == repository && issue.number == number)
                .map(|(_, parent, _)| *parent)
            else {
                return Ok(None);
            };
            let parent = issue_mut(state, repository, parent_number)?;
            Ok(Some(ParentIssue {
                id: parent.id,
                node_id: parent.node_id.clone(),
                number: parent.number,
                title: parent.title.clone(),
                state: parent.state,
                repository: repository.clone(),
            }))
        })
    }

    fn replace_parent(
        &self,
        repository: &RepositoryId,
        parent_number: u64,
        sub_issue_id: u64,
    ) -> GithubFuture<()> {
        let write = FakeWrite::ReplaceParent(repository.clone(), parent_number, sub_issue_id);
        self.write(write, |state| {
            let before = state.sub_issues.clone();
            state
                .sub_issues
                .retain(|(_, _, issue)| issue.id != sub_issue_id);
            let linked = link_sub_issue(state, repository, parent_number, sub_issue_id);
            if linked.is_err() {
                state.sub_issues = before;
            }
            linked
        })
    }

    fn reprioritize_sub_issue(
        &self,
        parent_node_id: String,
        sub_issue_node_id: String,
        position: SubIssuePosition,
    ) -> GithubFuture<()> {
        let write = FakeWrite::ReprioritizeSubIssue(
            parent_node_id.clone(),
            sub_issue_node_id.clone(),
            position.clone(),
        );
        self.write(write, |state| {
            let (repository, parent_number) = state
                .issues
                .iter()
                .find(|(_, issue)| issue.node_id == parent_node_id)
                .map(|(repository, issue)| (repository.clone(), issue.number))
                .ok_or(GithubError::NotFound)?;
            let index_of = |state: &FakeState, node_id: &str| {
                state.sub_issues.iter().position(|(owner, parent, issue)| {
                    *owner == repository && *parent == parent_number && issue.node_id == node_id
                })
            };
            let moved = index_of(state, &sub_issue_node_id).ok_or(GithubError::NotFound)?;
            let entry = state.sub_issues.remove(moved);
            let (anchor, offset) = match &position {
                SubIssuePosition::After(anchor) => (anchor, 1),
                SubIssuePosition::Before(anchor) => (anchor, 0),
            };
            match index_of(state, anchor) {
                Some(anchor) => {
                    state.sub_issues.insert(anchor + offset, entry);
                    Ok(())
                }
                None => {
                    state.sub_issues.insert(moved, entry);
                    Err(GithubError::NotFound)
                }
            }
        })
    }

    fn list_projects(
        &self,
        repository: &RepositoryId,
        cursor: Option<String>,
    ) -> GithubFuture<Page<ProjectV2>> {
        let projects = {
            let mut state = self.state.lock();
            state.project_requests += 1;
            of_repository(&state.projects, repository)
        };
        let page = self.page(projects, cursor);
        self.answer(|_| page)
    }

    fn list_project_items(
        &self,
        project_id: String,
        cursor: Option<String>,
    ) -> GithubFuture<Page<ProjectV2Item>> {
        let (items, stuck, empty, scripted, hold) = {
            let mut state = self.state.lock();
            state.project_requests += 1;
            let hold = state.held_project_item_lists.pop_front();
            let scripted = state.scripted_project_item_cursors.pop_front();
            let empty = (state.empty_project_item_pages > 0).then(|| {
                state.empty_project_item_pages -= 1;
                let offset = cursor.as_deref().map_or("0", page_offset);
                format!("{offset}~{}", state.empty_project_item_pages)
            });
            let stuck = empty.is_none() && state.stuck_project_item_pages > 0;
            if stuck {
                state.stuck_project_item_pages -= 1;
            }
            let items = state
                .project_items
                .iter()
                .filter(|(project, _)| *project == project_id)
                .map(|(_, item)| item.clone())
                .collect();
            (items, stuck, empty, scripted, hold)
        };
        if let Some(next_cursor) = scripted {
            return held_until(
                hold,
                self.answer(move |_| {
                    Ok(Page {
                        items: Vec::new(),
                        next_cursor: Some(next_cursor),
                    })
                }),
            );
        }
        if let Some(next_cursor) = empty {
            return held_until(
                hold,
                self.answer(move |_| {
                    Ok(Page {
                        items: Vec::new(),
                        next_cursor: Some(next_cursor),
                    })
                }),
            );
        }
        let mut page = self.page(items, cursor.clone());
        if stuck && let Ok(page) = &mut page {
            page.next_cursor = Some(cursor.unwrap_or_else(|| "0".to_string()));
        }
        held_until(hold, self.answer(|_| page))
    }

    fn project_details(&self, project_id: String) -> GithubFuture<ProjectDetails> {
        self.state.lock().project_requests += 1;
        self.answer(|state| {
            state
                .project_details
                .iter()
                .find(|(project, _)| *project == project_id)
                .map(|(_, details)| details.clone())
                .ok_or(GithubError::NotFound)
        })
    }

    fn set_project_item_field(
        &self,
        project_id: String,
        item_id: String,
        field_id: String,
        value: Option<ProjectFieldInput>,
    ) -> GithubFuture<()> {
        let write = FakeWrite::SetProjectItemField(
            project_id.clone(),
            item_id.clone(),
            field_id.clone(),
            value.clone(),
        );
        let applied_failure = self.state.lock().applied_project_field_failures.pop_front();
        let (applied_failure, hold) = match applied_failure {
            Some((failure, hold)) => (Some(failure), Some(hold)),
            None => (None, None),
        };
        let answer = self.write(write, |state| {
            if let Some(failure) = state.project_write_failures.pop_front() {
                return Err(failure);
            }
            let new_value = match value {
                None => None,
                Some(input) => Some(fake_field_value(state, &project_id, &field_id, input)?),
            };
            let item = state
                .project_items
                .iter_mut()
                .find(|(project, item)| *project == project_id && item.id == item_id)
                .map(|(_, item)| item)
                .ok_or(GithubError::NotFound)?;
            item.field_values.retain(|value| value.field_id != field_id);
            if let Some(value) = new_value {
                item.field_values
                    .push(ProjectFieldValue { field_id, value });
            }
            applied_failure.map_or(Ok(()), Err)
        });
        held_until(hold, answer)
    }

    fn move_project_item(
        &self,
        project_id: String,
        item_id: String,
        after_id: Option<String>,
    ) -> GithubFuture<()> {
        let write =
            FakeWrite::MoveProjectItem(project_id.clone(), item_id.clone(), after_id.clone());
        self.write(write, |state| {
            if let Some(failure) = state.project_write_failures.pop_front() {
                return Err(failure);
            }
            let items = &mut state.project_items;
            let position = |items: &[(String, ProjectV2Item)], id: &str| {
                items
                    .iter()
                    .position(|(project, item)| *project == project_id && item.id == id)
                    .ok_or(GithubError::NotFound)
            };
            let moved = items.remove(position(items, &item_id)?);
            let at = match &after_id {
                Some(after_id) => position(items, after_id)? + 1,
                None => position_of_first(items, &project_id),
            };
            items.insert(at, moved);
            Ok(())
        })
    }

    /// Adding content already in the project answers with its item, as
    /// GitHub does.
    fn add_project_item(&self, project_id: String, content_id: String) -> GithubFuture<String> {
        let write = FakeWrite::AddProjectItem(project_id.clone(), content_id.clone());
        let answer = self.write(write, |state| {
            if let Some(failure) = state.project_write_failures.pop_front() {
                return Err(failure);
            }
            let content = fake_item_content(state, &content_id)?;
            if let Some((_, item)) = state
                .project_items
                .iter()
                .find(|(project, item)| *project == project_id && item.content == content)
            {
                return Ok(item.id.clone());
            }
            let item_id = format!("PVTI_{content_id}");
            state.project_items.push((
                project_id,
                ProjectV2Item {
                    id: item_id.clone(),
                    content,
                    assignees: Vec::new(),
                    assignee_avatars: Default::default(),
                    state: None,
                    sub_issues: None,
                    labels: Vec::new(),
                    field_values: Vec::new(),
                },
            ));
            Ok(item_id)
        });
        self.held_project_write(answer)
    }

    /// Without owners set up, the viewer alone, with ids made from the names;
    /// remembered so a create can find the repository again.
    fn project_owners(&self, repository: &RepositoryId) -> GithubFuture<ProjectOwners> {
        self.answer(|state| {
            if let Some(owners) = of_repository(&state.project_owners, repository)
                .into_iter()
                .next()
            {
                return Ok(owners);
            }
            let login = state
                .viewer
                .as_ref()
                .map(|viewer| viewer.login.clone())
                .ok_or(GithubError::NotSignedIn)?;
            let owners = ProjectOwners {
                repository_id: format!("R_{}_{}", repository.owner(), repository.name()),
                viewer: ProjectOwner {
                    id: format!("U_{login}"),
                    login,
                    avatar_url: None,
                },
                organization: None,
            };
            state
                .project_owners
                .push((repository.clone(), owners.clone()));
            Ok(owners)
        })
    }

    /// The project is listed for the repository whose owners were asked
    /// for under `repository_id`.
    fn create_project(
        &self,
        owner_id: String,
        title: String,
        repository_id: String,
    ) -> GithubFuture<ProjectV2> {
        let write =
            FakeWrite::CreateProject(owner_id.clone(), title.clone(), repository_id.clone());
        self.write(write, |state| {
            if let Some(failure) = state.project_write_failures.pop_front() {
                return Err(failure);
            }
            let (repository, owners) = state
                .project_owners
                .iter()
                .find(|(_, owners)| owners.repository_id == repository_id)
                .cloned()
                .ok_or(GithubError::NotFound)?;
            let owner = std::iter::once(&owners.viewer)
                .chain(owners.organization.as_ref())
                .find(|owner| owner.id == owner_id)
                .ok_or(GithubError::NotFound)?;
            let owner_path = if owners.organization.as_ref() == Some(owner) {
                "orgs"
            } else {
                "users"
            };
            let number = state.projects.len() as u64 + 1;
            let project = ProjectV2 {
                id: format!("PVT_new_{number}"),
                number,
                title,
                url: format!(
                    "https://github.com/{owner_path}/{}/projects/{number}",
                    owner.login
                ),
                closed: false,
            };
            state.projects.push((repository, project.clone()));
            Ok(project)
        })
    }

    fn delete_project_item(&self, project_id: String, item_id: String) -> GithubFuture<()> {
        let write = FakeWrite::DeleteProjectItem(project_id.clone(), item_id.clone());
        let answer = self.write(write, |state| {
            if let Some(failure) = state.project_write_failures.pop_front() {
                return Err(failure);
            }
            let position = state
                .project_items
                .iter()
                .position(|(project, item)| *project == project_id && item.id == item_id)
                .ok_or(GithubError::NotFound)?;
            state.project_items.remove(position);
            Ok(())
        });
        self.held_project_write(answer)
    }

    fn list_pull_requests(
        &self,
        repository: &RepositoryId,
        state_filter: PullRequestStateFilter,
        cursor: Option<String>,
    ) -> GithubFuture<Page<PullRequest>> {
        let pull_requests = of_repository(&self.state.lock().pull_requests, repository)
            .into_iter()
            .filter(|pull_request| match state_filter {
                PullRequestStateFilter::Open => pull_request.state == PullRequestState::Open,
                PullRequestStateFilter::Closed => pull_request.state == PullRequestState::Closed,
                PullRequestStateFilter::Merged => pull_request.state == PullRequestState::Merged,
                PullRequestStateFilter::All => true,
            })
            .collect();
        let page = self.page(pull_requests, cursor);
        self.answer(|_| page)
    }

    fn pull_request(&self, repository: &RepositoryId, number: u64) -> GithubFuture<PullRequest> {
        self.answer(|state| {
            of_repository(&state.pull_requests, repository)
                .into_iter()
                .find(|pull_request| pull_request.number == number)
                .ok_or(GithubError::NotFound)
        })
    }

    fn pull_request_details(
        &self,
        repository: &RepositoryId,
        number: u64,
    ) -> GithubFuture<PullRequestDetails> {
        let hold = self.state.lock().held_pull_request_details.pop_front();
        let answer = self.answer(|state| {
            let details = of_repository(&state.pull_request_details, repository)
                .into_iter()
                .find(|details| details.pull_request.number == number);
            if let Some(details) = details {
                return Ok(details);
            }
            of_repository(&state.pull_requests, repository)
                .into_iter()
                .find(|pull_request| pull_request.number == number)
                .map(fake_pull_request_details)
                .ok_or(GithubError::NotFound)
        });
        held_until(hold, answer)
    }

    fn merge_pull_request(
        &self,
        pull_request_id: String,
        method: MergeMethod,
        expected_head_oid: String,
    ) -> GithubFuture<()> {
        let write =
            FakeWrite::MergePullRequest(pull_request_id.clone(), method, expected_head_oid.clone());
        let hold = self.state.lock().held_merges.pop_front();
        let answer = self.write(write, |state| {
            merge_fake_pull_request(state, &pull_request_id, &expected_head_oid)
        });
        held_until(hold, answer)
    }

    fn delete_branch_at(
        &self,
        repository_node_id: String,
        branch: String,
        expected_oid: String,
    ) -> GithubFuture<()> {
        let write = FakeWrite::DeleteBranchAt(
            repository_node_id.clone(),
            branch.clone(),
            expected_oid.clone(),
        );
        self.write(write, |state| {
            delete_fake_branch(state, &repository_node_id, &branch, &expected_oid)
        })
    }
}

fn merge_fake_pull_request(
    state: &mut FakeState,
    id: &str,
    expected_head_oid: &str,
) -> GithubResult<()> {
    let head = state
        .pull_requests
        .iter()
        .find(|(_, pull_request)| pull_request.id == id)
        .map(|(_, pull_request)| pull_request.head_ref_oid.clone())
        .ok_or(GithubError::NotFound)?;
    if head != expected_head_oid {
        return Err(GithubError::Rejected {
            message: "Head branch was modified. Review and try the merge again.".to_string(),
        });
    }
    for (_, pull_request) in &mut state.pull_requests {
        if pull_request.id == id {
            pull_request.state = PullRequestState::Merged;
        }
    }
    for (_, details) in &mut state.pull_request_details {
        if details.pull_request.id == id {
            details.pull_request.state = PullRequestState::Merged;
        }
    }
    Ok(())
}

fn delete_fake_branch(
    state: &mut FakeState,
    repository_node_id: &str,
    branch: &str,
    expected_oid: &str,
) -> GithubResult<()> {
    for (_, details) in &mut state.pull_request_details {
        if details.settings.repository_node_id != repository_node_id {
            continue;
        }
        let Some(head) = &details.head_branch else {
            continue;
        };
        if head.name != branch {
            continue;
        }
        if head.oid.as_deref() != Some(expected_oid) {
            return Err(GithubError::Rejected {
                message: "the branch moved".to_string(),
            });
        }
        details.head_branch = None;
    }
    Ok(())
}
