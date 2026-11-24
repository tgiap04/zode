use futures::executor::block_on;

use crate::backend::GithubBackend;
use crate::error::GithubError;
use crate::fake_backend::{FakeGithubBackend, FakeWrite, fake_issue};
use crate::model::{
    IssueFilter, IssueState, IssueType, IssueUpdate, Milestone, RepositoryId, SubIssuePosition,
    SubIssuesSummary,
};

fn repository() -> RepositoryId {
    RepositoryId::new("octo", "repo").expect("a valid repository")
}

#[test]
fn the_fake_pages_what_the_builder_put_in() {
    let backend = FakeGithubBackend::new()
        .with_page_size(2)
        .with_issue(&repository(), fake_issue(1, "One"))
        .with_issue(&repository(), fake_issue(2, "Two"))
        .with_issue(&repository(), fake_issue(3, "Three"));
    let first = block_on(backend.list_issues(&repository(), &IssueFilter::default(), None))
        .expect("a page");
    assert_eq!(first.items.len(), 2);
    let second =
        block_on(backend.list_issues(&repository(), &IssueFilter::default(), first.next_cursor))
            .expect("a page");
    assert_eq!(second.items[0].title, "Three");
    assert_eq!(second.next_cursor, None);
}

#[test]
fn the_fake_applies_and_records_writes() {
    let backend = FakeGithubBackend::new().with_issue(&repository(), fake_issue(1, "One"));
    let update = IssueUpdate {
        state: Some(IssueState::Closed),
        ..IssueUpdate::default()
    };
    let closed = block_on(backend.update_issue(&repository(), 1, update.clone())).expect("closed");
    assert_eq!(closed.state, IssueState::Closed);
    block_on(backend.add_labels(&repository(), 1, vec!["bug".to_string()])).expect("labelled");
    assert_eq!(
        backend.writes(),
        vec![
            FakeWrite::UpdateIssue(repository(), 1, update),
            FakeWrite::AddLabels(repository(), 1, vec!["bug".to_string()]),
        ]
    );
}

#[test]
fn the_fake_can_fail_every_call() {
    let backend = FakeGithubBackend::new().failing_with(GithubError::TokenRevoked);
    assert_eq!(block_on(backend.viewer()), Err(GithubError::TokenRevoked));
    backend.set_failure(None);
    assert_eq!(
        block_on(backend.viewer()).map(|viewer| viewer.login),
        Ok("octocat".to_string())
    );
}

#[test]
fn the_fake_filters_by_assignee_author_and_milestone() {
    let mut assigned = fake_issue(1, "Assigned");
    assigned.assignees = vec!["octocat".to_string()];
    let mut by_hubot = fake_issue(2, "By hubot");
    by_hubot.author = Some("hubot".to_string());
    let mut planned = fake_issue(3, "Planned");
    planned.milestone = Some(Milestone {
        number: 7,
        title: "v1".to_string(),
        is_open: true,
        due_on: None,
    });
    let backend = FakeGithubBackend::new()
        .with_issue(&repository(), assigned)
        .with_issue(&repository(), by_hubot)
        .with_issue(&repository(), planned);
    let numbers = |filter: IssueFilter| {
        block_on(backend.list_issues(&repository(), &filter, None))
            .expect("a page")
            .items
            .iter()
            .map(|issue| issue.number)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        numbers(IssueFilter {
            assignee: Some("octocat".to_string()),
            ..IssueFilter::default()
        }),
        vec![1]
    );
    assert_eq!(
        numbers(IssueFilter {
            creator: Some("hubot".to_string()),
            ..IssueFilter::default()
        }),
        vec![2]
    );
    assert_eq!(
        numbers(IssueFilter {
            milestone: Some(7),
            ..IssueFilter::default()
        }),
        vec![3]
    );
}

#[test]
fn the_fake_searches_titles_and_bodies() {
    let mut with_body = fake_issue(2, "Other");
    with_body.body = Some("It crashes on start".to_string());
    let backend = FakeGithubBackend::new()
        .with_issue(&repository(), fake_issue(1, "Crash when saving"))
        .with_issue(&repository(), with_body)
        .with_issue(&repository(), fake_issue(3, "Unrelated"));
    let page =
        block_on(backend.search_issues(&repository(), &IssueFilter::default(), "crash", None))
            .expect("a page");
    let numbers = page
        .items
        .iter()
        .map(|issue| issue.number)
        .collect::<Vec<_>>();
    assert_eq!(numbers, vec![1, 2]);
    assert_eq!(backend.searches(), vec!["crash".to_string()]);
}

#[test]
fn a_held_issue_list_answers_only_when_released() {
    let backend = FakeGithubBackend::new().with_issue(&repository(), fake_issue(1, "One"));
    let release = backend.hold_next_issue_list();
    let mut held = backend.list_issues(&repository(), &IssueFilter::default(), None);
    assert!(
        futures::FutureExt::now_or_never(&mut held).is_none(),
        "a held list waits"
    );
    release.send(()).expect("the list is still waiting");
    let page = block_on(held).expect("a page");
    assert_eq!(page.items.len(), 1);
}

#[test]
fn the_fake_counts_project_reads() {
    let backend = FakeGithubBackend::new()
        .with_project_details("PVT_1", crate::projects::ProjectDetails::default());
    assert_eq!(backend.project_requests(), 0);
    block_on(backend.list_projects(&repository(), None)).expect("projects");
    block_on(backend.project_details("PVT_1".to_string())).expect("details");
    block_on(backend.list_project_items("PVT_1".to_string(), None)).expect("items");
    assert_eq!(backend.project_requests(), 3);
    assert_eq!(
        block_on(backend.project_details("PVT_2".to_string())),
        Err(GithubError::NotFound)
    );
}

#[test]
fn without_triage_the_fake_drops_labels_assignees_and_milestones_silently() {
    use crate::model::{NewIssue, RepositoryPermission};
    let milestone = Milestone {
        number: 7,
        title: "v7".to_string(),
        is_open: true,
        due_on: None,
    };
    let backend = FakeGithubBackend::new()
        .with_issue(&repository(), fake_issue(1, "One"))
        .with_milestone(&repository(), milestone)
        .with_repository_permission(&repository(), RepositoryPermission::Read);
    let created = block_on(backend.create_issue(
        &repository(),
        NewIssue {
            title: "New".to_string(),
            labels: vec!["bug".to_string()],
            assignees: vec!["octocat".to_string()],
            milestone: Some(7),
            ..NewIssue::default()
        },
    ))
    .expect("created");
    assert!(created.labels.is_empty());
    assert!(created.assignees.is_empty());
    assert_eq!(created.milestone, None);
    let labels = block_on(backend.add_labels(&repository(), 1, vec!["bug".to_string()]))
        .expect("no error, as on GitHub");
    assert!(labels.is_empty());
    assert_eq!(
        block_on(backend.repository_permission(&repository())),
        Ok(Some(RepositoryPermission::Read))
    );
}

#[test]
fn the_fake_keeps_sub_issues_their_parent_order_and_summary() {
    let mut closed = fake_issue(3, "Done child");
    closed.state = IssueState::Closed;
    let backend = FakeGithubBackend::new()
        .with_issue(&repository(), fake_issue(1, "Epic"))
        .with_issue(&repository(), fake_issue(2, "Child"))
        .with_issue(&repository(), closed.clone())
        .with_sub_issue(&repository(), 1, fake_issue(2, "Child"))
        .with_sub_issue(&repository(), 1, closed);
    let epic = block_on(backend.issue(&repository(), 1)).expect("epic");
    assert_eq!(
        epic.sub_issues_summary,
        SubIssuesSummary {
            total: 2,
            completed: 1
        }
    );
    let parent = block_on(backend.issue_parent(&repository(), 2)).expect("read");
    assert_eq!(parent.map(|parent| parent.number), Some(1));
    assert_eq!(
        block_on(backend.issue_parent(&repository(), 1)).expect("read"),
        None
    );

    let position = SubIssuePosition::Before("I_2".to_string());
    block_on(backend.reprioritize_sub_issue(
        "I_1".to_string(),
        "I_3".to_string(),
        position.clone(),
    ))
    .expect("moved");
    let order = block_on(backend.list_sub_issues(&repository(), 1, None))
        .expect("children")
        .items
        .iter()
        .map(|issue| issue.number)
        .collect::<Vec<_>>();
    assert_eq!(order, vec![3, 2]);
    assert_eq!(
        backend.writes(),
        vec![FakeWrite::ReprioritizeSubIssue(
            "I_1".to_string(),
            "I_3".to_string(),
            position
        )]
    );
}

#[test]
fn the_fake_refuses_a_hundred_and_first_sub_issue_and_moves_parents() {
    let backend = (2..=101).fold(
        FakeGithubBackend::new()
            .with_issue(&repository(), fake_issue(1, "Full"))
            .with_issue(&repository(), fake_issue(200, "Other parent"))
            .with_issue(&repository(), fake_issue(201, "Late")),
        |backend, number| {
            backend
                .with_issue(&repository(), fake_issue(number, "Child"))
                .with_sub_issue(&repository(), 1, fake_issue(number, "Child"))
        },
    );
    let error = block_on(backend.add_sub_issue(&repository(), 1, 1201)).expect_err("full");
    assert!(
        matches!(&error, GithubError::Rejected { message } if message.contains("100")),
        "{error:?}"
    );
    block_on(backend.replace_parent(&repository(), 200, 1002)).expect("moved");
    let parent = block_on(backend.issue_parent(&repository(), 2)).expect("read");
    assert_eq!(parent.map(|parent| parent.number), Some(200));
    assert_eq!(
        block_on(backend.issue(&repository(), 1))
            .expect("read")
            .sub_issues_summary
            .total,
        99
    );
}

#[test]
fn the_fake_sets_and_filters_by_issue_type() {
    let backend = FakeGithubBackend::new()
        .with_issue(&repository(), fake_issue(1, "One"))
        .with_issue(&repository(), fake_issue(2, "Two"))
        .with_issue_type(
            &repository(),
            IssueType {
                id: "IT_bug".to_string(),
                name: "Bug".to_string(),
                description: None,
                color: Some("RED".to_string()),
                is_enabled: true,
            },
        );
    block_on(backend.set_issue_type("I_2".to_string(), Some("IT_bug".to_string()))).expect("set");
    let filter = IssueFilter {
        issue_type: Some("Bug".to_string()),
        ..IssueFilter::default()
    };
    let bugs = block_on(backend.list_issues(&repository(), &filter, None)).expect("page");
    assert_eq!(
        bugs.items
            .iter()
            .map(|issue| issue.number)
            .collect::<Vec<_>>(),
        vec![2]
    );
    block_on(backend.set_issue_type("I_2".to_string(), None)).expect("cleared");
    assert_eq!(
        block_on(backend.issue(&repository(), 2))
            .expect("read")
            .issue_type,
        None
    );
}
