use github::{
    IssueState, Label, Milestone, NewIssue, RepositoryPermission, RepositoryVisibility,
    StateReason, fake_issue,
};

use crate::issues::issue_actions::{
    IssueMenuEntry, ignored_fields, ignored_notice, row_menu_entries, target_line,
};
use crate::issues::issues_test_support::repository;

const BASIC: [IssueMenuEntry; 3] = [
    IssueMenuEntry::OpenInTab,
    IssueMenuEntry::OpenOnGithub,
    IssueMenuEntry::CopyLink,
];

fn closes() -> Vec<IssueMenuEntry> {
    [
        StateReason::Completed,
        StateReason::NotPlanned,
        StateReason::Duplicate,
    ]
    .map(IssueMenuEntry::Close)
    .to_vec()
}

#[test]
fn the_row_menu_is_a_function_of_the_role_and_the_author() {
    let mut theirs = fake_issue(1, "Theirs");
    theirs.author = Some("hubot".to_string());
    let reader = Some(RepositoryPermission::Read);
    assert_eq!(
        row_menu_entries(&theirs, reader, "octocat"),
        BASIC.to_vec(),
        "a reader cannot close someone else's issue"
    );

    let mut expected = BASIC.to_vec();
    expected.extend(closes());
    assert_eq!(
        row_menu_entries(&theirs, Some(RepositoryPermission::Triage), "octocat"),
        expected
    );
    assert_eq!(
        row_menu_entries(&theirs, None, "octocat"),
        expected,
        "an unknown role is left to GitHub"
    );
    let mine = fake_issue(2, "Mine");
    assert_eq!(
        row_menu_entries(&mine, reader, "octocat"),
        expected,
        "the author closes their own issue"
    );

    let mut closed = fake_issue(3, "Closed");
    closed.state = IssueState::Closed;
    let mut reopen = BASIC.to_vec();
    reopen.push(IssueMenuEntry::Reopen);
    assert_eq!(
        row_menu_entries(&closed, Some(RepositoryPermission::Write), "hubot"),
        reopen
    );
}

#[test]
fn the_target_line_names_the_account_the_repository_and_its_visibility() {
    assert_eq!(
        target_line(
            "octocat",
            &repository(),
            Some(RepositoryVisibility::Private)
        ),
        "@octocat → octo/repo (private)"
    );
    assert_eq!(
        target_line("octocat", &repository(), Some(RepositoryVisibility::Public)),
        "@octocat → octo/repo (public)"
    );
    assert_eq!(
        target_line("octocat", &repository(), None),
        "@octocat → octo/repo (checking visibility…)"
    );
    assert_eq!(
        target_line("octo\u{202e}cat", &repository(), None),
        "@octo<U+202E>cat → octo/repo (checking visibility…)",
        "a hidden character in the login is shown"
    );
}

#[test]
fn what_github_dropped_from_a_new_issue_is_named() {
    let requested = NewIssue {
        title: "New".to_string(),
        labels: vec!["bug".to_string()],
        assignees: vec!["Octocat".to_string()],
        milestone: Some(7),
        ..NewIssue::default()
    };
    let dropped = fake_issue(4, "New");
    assert_eq!(
        ignored_fields(&requested, &dropped),
        vec!["labels", "assignees", "milestone"]
    );
    let mut applied = fake_issue(4, "New");
    applied.labels = vec![Label {
        name: "bug".to_string(),
        color: "d73a4a".to_string(),
        description: None,
    }];
    applied.assignees = vec!["octocat".to_string()];
    applied.milestone = Some(Milestone {
        number: 7,
        title: "v7".to_string(),
        is_open: true,
        due_on: None,
    });
    assert!(ignored_fields(&requested, &applied).is_empty());
    assert_eq!(
        ignored_notice(&["labels", "milestone"], "octocat", &repository()).as_deref(),
        Some(
            "GitHub did not apply the labels, milestone. @octocat may not have push access to octo/repo."
        )
    );
    assert_eq!(ignored_notice(&[], "octocat", &repository()), None);
}
