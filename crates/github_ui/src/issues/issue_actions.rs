//! What an issue's menus offer and what a write's answer means, as plain
//! functions: "a reader cannot close someone else's issue" and "GitHub
//! dropped the labels" are facts a test states without opening a window.

use github::{
    Issue, IssueState, NewIssue, RepositoryId, RepositoryPermission, RepositoryVisibility,
    StateReason,
};

use crate::untrusted_text::reveal_hidden_characters;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IssueMenuEntry {
    OpenInTab,
    OpenOnGithub,
    CopyLink,
    Close(StateReason),
    Reopen,
}

/// What can be done to several selected issues at once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BulkIssueAction {
    Close(StateReason),
    AddLabel(String),
}

pub const CLOSE_REASONS: [StateReason; 3] = [
    StateReason::Completed,
    StateReason::NotPlanned,
    StateReason::Duplicate,
];

pub fn close_reason_label(reason: StateReason) -> &'static str {
    match reason {
        StateReason::Completed => "Close as completed",
        StateReason::NotPlanned => "Close as not planned",
        StateReason::Duplicate => "Close as duplicate",
        StateReason::Reopened => "Reopen",
    }
}

/// Labels, assignees, milestones, and closing anyone's issue. When GitHub
/// did not say what the account may do, everything is offered: GitHub
/// decides, and the answer is read back to tell the user what it ignored.
pub fn may_triage(permission: Option<RepositoryPermission>) -> bool {
    permission.is_none_or(RepositoryPermission::can_triage)
}

/// The author may close and reopen their own issue whatever their role.
pub fn may_change_state(
    issue: &Issue,
    permission: Option<RepositoryPermission>,
    login: &str,
) -> bool {
    may_triage(permission) || issue.author.as_deref() == Some(login)
}

pub fn row_menu_entries(
    issue: &Issue,
    permission: Option<RepositoryPermission>,
    login: &str,
) -> Vec<IssueMenuEntry> {
    let mut entries = vec![
        IssueMenuEntry::OpenInTab,
        IssueMenuEntry::OpenOnGithub,
        IssueMenuEntry::CopyLink,
    ];
    if may_change_state(issue, permission, login) {
        match issue.state {
            IssueState::Open => entries.extend(CLOSE_REASONS.map(IssueMenuEntry::Close)),
            IssueState::Closed => entries.push(IssueMenuEntry::Reopen),
        }
    }
    entries
}

/// `@login → owner/repo (private)`: who writes, and where, shown on every
/// dialog that writes so a fork is never mistaken for its upstream.
pub fn target_line(
    login: &str,
    repository: &RepositoryId,
    visibility: Option<RepositoryVisibility>,
) -> String {
    let visibility = match visibility {
        Some(RepositoryVisibility::Public) => "public",
        Some(RepositoryVisibility::Private) => "private",
        Some(RepositoryVisibility::Internal) => "internal",
        None => "checking visibility…",
    };
    format!(
        "@{} → {repository} ({visibility})",
        reveal_hidden_characters(login)
    )
}

/// The parts of `requested` that the created issue does not carry. GitHub
/// drops labels, assignees and the milestone without an error when the
/// account may not set them.
pub fn ignored_fields(requested: &NewIssue, created: &Issue) -> Vec<&'static str> {
    let mut ignored = Vec::new();
    let labels_applied = requested
        .labels
        .iter()
        .all(|wanted| created.labels.iter().any(|label| &label.name == wanted));
    if !labels_applied {
        ignored.push("labels");
    }
    let assignees_applied = requested.assignees.iter().all(|wanted| {
        created
            .assignees
            .iter()
            .any(|login| login.eq_ignore_ascii_case(wanted))
    });
    if !assignees_applied {
        ignored.push("assignees");
    }
    if requested.milestone.is_some()
        && created.milestone.as_ref().map(|milestone| milestone.number) != requested.milestone
    {
        ignored.push("milestone");
    }
    ignored
}

pub fn ignored_notice(ignored: &[&str], login: &str, repository: &RepositoryId) -> Option<String> {
    if ignored.is_empty() {
        return None;
    }
    Some(format!(
        "GitHub did not apply the {}. @{} may not have push access to {repository}.",
        ignored.join(", "),
        reveal_hidden_characters(login)
    ))
}
