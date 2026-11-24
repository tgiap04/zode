//! How a pull request's states read, in the list and in its tab.

use github::{
    CheckState, ChecksState, FileChange, PullRequest, PullRequestState, ReviewDecision, ReviewState,
};
use ui::Color;

use crate::untrusted_text::reveal_hidden_characters;

/// ✓, ✗ or "running" for the combined checks.
pub fn checks_label(checks: ChecksState) -> (&'static str, Color) {
    match checks {
        ChecksState::Passing => ("✓", Color::Success),
        ChecksState::Failing => ("✗", Color::Error),
        ChecksState::Running => ("running", Color::Warning),
    }
}

pub fn check_label(state: CheckState) -> (&'static str, Color) {
    match state {
        CheckState::Passed => ("✓", Color::Success),
        CheckState::Failed => ("✗", Color::Error),
        CheckState::Running => ("running", Color::Warning),
        CheckState::Skipped => ("skipped", Color::Muted),
    }
}

pub fn review_decision_label(decision: ReviewDecision) -> (&'static str, Color) {
    match decision {
        ReviewDecision::Approved => ("Approved", Color::Success),
        ReviewDecision::ChangesRequested => ("Changes requested", Color::Error),
        ReviewDecision::ReviewRequired => ("Review required", Color::Warning),
    }
}

pub fn review_state_label(state: ReviewState) -> (&'static str, Color) {
    match state {
        ReviewState::Approved => ("approved", Color::Success),
        ReviewState::ChangesRequested => ("requested changes", Color::Error),
        ReviewState::Commented => ("commented", Color::Muted),
        ReviewState::Dismissed => ("dismissed", Color::Muted),
        ReviewState::Pending => ("pending", Color::Muted),
    }
}

pub fn state_label(pull_request: &PullRequest) -> (&'static str, Color) {
    match pull_request.state {
        PullRequestState::Open if pull_request.is_draft => ("Draft", Color::Muted),
        PullRequestState::Open => ("Open", Color::Success),
        PullRequestState::Closed => ("Closed", Color::Error),
        PullRequestState::Merged => ("Merged", Color::Accent),
    }
}

pub fn file_change_label(change: FileChange) -> &'static str {
    match change {
        FileChange::Added => "added",
        FileChange::Changed => "changed",
        FileChange::Copied => "copied",
        FileChange::Deleted => "deleted",
        FileChange::Modified => "modified",
        FileChange::Renamed => "renamed",
    }
}

/// `head → base`, with hidden characters shown: both names are chosen by
/// whoever opened the pull request.
pub fn branches_label(pull_request: &PullRequest) -> String {
    format!(
        "{} → {}",
        reveal_hidden_characters(&pull_request.head_ref_name),
        reveal_hidden_characters(&pull_request.base_ref_name)
    )
}

pub fn login_label(login: Option<&str>) -> String {
    login.map_or("a deleted account".to_string(), |login| {
        format!("@{}", reveal_hidden_characters(login))
    })
}
