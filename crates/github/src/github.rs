//! GitHub for the GitHub tab: a REST + GraphQL client over `http_client`, the
//! token it runs with (Zode's keychain or the GitHub CLI), the github.com
//! repositories a project points at, and one write queue per token.
//!
//! No UI lives here. Nothing is sent without a token: an [`HttpBackend`]
//! cannot be built without one.

mod backend;
mod error;
#[cfg(any(test, feature = "test-support"))]
mod fake_backend;
#[cfg(any(test, feature = "test-support"))]
mod fake_pull_requests;
mod graphql;
mod graphql_projects;
mod graphql_pull_requests;
mod http_backend;
mod http_transport;
mod model;
mod projects;
mod pull_requests;
mod repo_detection;
mod rest;
mod rest_payloads;
mod sign_in_status;
mod token;
mod write_queue;

pub use backend::{GithubBackend, GithubFuture};
pub use error::{GithubError, GithubResult};
#[cfg(any(test, feature = "test-support"))]
pub use fake_backend::{
    FakeGithubBackend, FakeWrite, fake_issue, fake_pull_request, fake_pull_request_details,
};
pub use http_backend::HttpBackend;
pub use http_transport::{MAX_RESPONSE_BYTES, REQUEST_TIMEOUT};
pub use model::{
    Comment, GithubUser, Issue, IssueFilter, IssueState, IssueStateFilter, IssueType, IssueUpdate,
    Label, MAX_SUB_ISSUE_DEPTH, MAX_SUB_ISSUES_PER_PARENT, Milestone, NewIssue, OAuthScopes, Page,
    ParentIssue, ProjectItemContent, ProjectItemState, ProjectV2, ProjectV2Item, PullRequest,
    PullRequestState, PullRequestStateFilter, RateLimit, RateLimits, RepositoryId,
    RepositoryPermission, RepositoryVisibility, StateReason, SubIssuePosition, SubIssuesSummary,
    Viewer,
};
pub use projects::{
    ProjectDetails, ProjectField, ProjectFieldInput, ProjectFieldKind, ProjectFieldOption,
    ProjectFieldValue, ProjectIteration, ProjectOwner, ProjectOwners, ProjectSort, ProjectValue,
    ProjectView, ProjectViewLayout,
};
pub use pull_requests::{
    ChangedFile, CheckState, ChecksState, FileChange, HeadBranch, MergeMethod, MergeSettings,
    MergeStateStatus, Mergeable, PullRequestCheck, PullRequestDetails, PullRequestReview,
    ReviewDecision, ReviewState,
};
pub use repo_detection::{
    DetectedRepository, RemoteName, RepositoryRemotes, detect_repositories, parse_github_remote,
};
pub use sign_in_status::{
    BUILT_IN_CLIENT_ID, CLIENT_ID_VARIABLE, GithubClientId, GithubSignInStatus, SignInStatus,
    client_id, client_id_for_build, configured_client_id, publish_sign_in_status, revoke_page,
    sign_in_status, sign_in_with_github_available, token_source_label,
};
pub use token::{
    GH_ARGUMENTS, GH_TIMEOUT, GhHost, GhOutput, GhRunResult, GithubToken, KEYCHAIN_URL,
    LocalGhHost, TokenIdentity, TokenSource, TokenVariable, delete_keychain_token,
    read_keychain_token, resolve_token, token_from_gh, write_keychain_token,
};
pub use write_queue::{
    MAX_WRITE_PAUSE, MIN_WRITE_INTERVAL, RATE_LIMITED_PAUSE, WriteOutcome, WriteQueue, WriteQueues,
};

#[cfg(test)]
mod fake_backend_tests;
#[cfg(test)]
mod graphql_projects_tests;
#[cfg(test)]
mod graphql_schema_check;
#[cfg(test)]
mod graphql_schema_tests;
#[cfg(test)]
mod graphql_tests;
#[cfg(test)]
mod http_backend_tests;
#[cfg(test)]
mod project_writes_tests;
#[cfg(test)]
mod pull_request_tests;
#[cfg(test)]
mod repo_detection_tests;
#[cfg(test)]
mod rest_tests;
#[cfg(test)]
mod token_tests;
#[cfg(test)]
mod write_queue_tests;
