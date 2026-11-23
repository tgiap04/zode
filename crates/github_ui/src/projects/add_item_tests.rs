use std::sync::Arc;

use github::{
    FakeWrite, GithubError, ProjectFieldInput, ProjectItemContent, fake_issue, fake_pull_request,
};
use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::issues::issues_test_support::repository;
use crate::projects::field_editor_tests::{rich_backend, writable};
use crate::projects::projects_section_tests::{PROJECT, projects_tab, section};
use crate::projects::{ColumnKey, ProjectsSection, board_columns};

fn candidates(section: &Entity<ProjectsSection>, cx: &mut VisualTestContext) -> Vec<u64> {
    section.read_with(cx, |section, _| {
        section
            .add_item_picker()
            .map(|picker| picker.candidates.iter().map(|found| found.number).collect())
            .unwrap_or_default()
    })
}

fn column_of(
    section: &Entity<ProjectsSection>,
    item_id: &str,
    cx: &mut VisualTestContext,
) -> ColumnKey {
    section.read_with(cx, |section, _| {
        let details = section.details().expect("details");
        board_columns(section.view(), details, section.items())
            .into_iter()
            .find(|column| {
                column
                    .items
                    .iter()
                    .any(|index| section.items()[*index].id == item_id)
            })
            .map(|column| column.key)
            .expect("on the board")
    })
}

fn has_item(section: &Entity<ProjectsSection>, item_id: &str, cx: &mut VisualTestContext) -> bool {
    section.read_with(cx, |section, _| {
        section.items().iter().any(|item| item.id == item_id)
    })
}

#[gpui::test]
async fn adding_an_issue_with_a_status_sends_the_add_then_the_status(cx: &mut TestAppContext) {
    let backend = Arc::new(
        rich_backend()
            .with_issue(&repository(), fake_issue(12, "Login fails"))
            .with_pull_request(&repository(), fake_pull_request(13, "Login page")),
    );
    let (_workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);

    section.update(cx, |section, cx| {
        section.open_add_item(cx);
        section.search_add_item("login".into(), cx);
    });
    cx.run_until_parked();
    assert_eq!(
        candidates(&section, cx),
        vec![12, 13],
        "issues and pull requests"
    );

    section.update(cx, |section, cx| section.search_add_item("#12".into(), cx));
    cx.run_until_parked();
    assert_eq!(candidates(&section, cx), vec![12]);
    let found = section.read_with(cx, |section, _| {
        section.add_item_picker().expect("picker").candidates[0].clone()
    });
    section.update(cx, |section, cx| {
        section.set_add_item_status(Some("done".into()), cx);
        section.add_item(found, cx);
    });
    cx.run_until_parked();

    assert_eq!(
        backend.writes(),
        vec![
            FakeWrite::AddProjectItem(PROJECT.into(), "I_12".into()),
            FakeWrite::SetProjectItemField(
                PROJECT.into(),
                "PVTI_I_12".into(),
                "F_status".into(),
                Some(ProjectFieldInput::SingleSelect {
                    option_id: "done".into()
                }),
            ),
        ],
        "added first, then its status: GitHub cannot do both in one call"
    );
    assert_eq!(
        column_of(&section, "PVTI_I_12", cx),
        ColumnKey::Option("done".into())
    );
    let content = section.read_with(cx, |section, _| {
        section
            .items()
            .iter()
            .find(|item| item.id == "PVTI_I_12")
            .map(|item| item.content.clone())
    });
    assert_eq!(
        content,
        Some(ProjectItemContent::Issue {
            number: 12,
            title: "Login fails".into(),
            repository: "octo/repo".into(),
        })
    );

    // A pull request, with no status: one write.
    section.update(cx, |section, cx| {
        section.open_add_item(cx);
        section.search_add_item("13".into(), cx);
    });
    cx.run_until_parked();
    let found = section.read_with(cx, |section, _| {
        section.add_item_picker().expect("picker").candidates[0].clone()
    });
    assert!(found.pull_request);
    section.update(cx, |section, cx| section.add_item(found, cx));
    cx.run_until_parked();
    assert_eq!(
        backend.writes().last(),
        Some(&FakeWrite::AddProjectItem(PROJECT.into(), "PR_13".into()))
    );
    assert_eq!(backend.writes().len(), 3);
    assert_eq!(column_of(&section, "PVTI_PR_13", cx), ColumnKey::NoValue);
}

#[gpui::test]
async fn items_already_in_the_project_are_not_offered(cx: &mut TestAppContext) {
    let backend = Arc::new(rich_backend().with_issue(&repository(), fake_issue(7, "Seven")));
    let (_workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);
    section.update(cx, |section, cx| {
        section.open_add_item(cx);
        section.search_add_item("#7".into(), cx);
    });
    cx.run_until_parked();
    assert_eq!(candidates(&section, cx), Vec::<u64>::new());
    let message = section.read_with(cx, |section, _| {
        section.add_item_message().map(str::to_string)
    });
    assert!(
        message
            .as_deref()
            .is_some_and(|message| message.contains("already")),
        "{message:?}"
    );
}

#[gpui::test]
async fn removing_asks_first_and_cancelling_sends_nothing(cx: &mut TestAppContext) {
    let backend = Arc::new(rich_backend());
    let (_workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);

    section.update(cx, |section, cx| section.request_remove_item("PVTI_7", cx));
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert_eq!(
        section.read_with(cx, |section, _| section
            .pending_removal()
            .map(str::to_string)),
        Some("PVTI_7".to_string())
    );
    let question = section
        .read_with(cx, |section, _| section.removal_question())
        .expect("a question");
    assert!(question.contains("Issue 7"), "{question}");
    assert!(question.contains("not deleted"), "{question}");
    assert!(cx.debug_bounds("github-project-remove-confirm").is_some());
    assert_eq!(backend.writes(), Vec::new(), "asking sends nothing");

    section.update(cx, |section, cx| section.cancel_remove_item(cx));
    cx.run_until_parked();
    assert!(section.read_with(cx, |section, _| section.pending_removal().is_none()));
    assert_eq!(backend.writes(), Vec::new(), "cancel sends nothing");
    assert!(has_item(&section, "PVTI_7", cx));

    section.update(cx, |section, cx| {
        section.request_remove_item("PVTI_7", cx);
        section.confirm_remove_item(cx);
    });
    cx.run_until_parked();
    assert_eq!(
        backend.writes(),
        vec![FakeWrite::DeleteProjectItem(
            PROJECT.into(),
            "PVTI_7".into()
        )]
    );
    assert!(!has_item(&section, "PVTI_7", cx));
}

#[gpui::test]
async fn a_failed_removal_puts_the_item_back_and_says_why(cx: &mut TestAppContext) {
    let backend = Arc::new(rich_backend());
    let (_workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);
    backend.fail_next_project_write(GithubError::NotFound);
    section.update(cx, |section, cx| {
        section.request_remove_item("PVTI_7", cx);
        section.confirm_remove_item(cx);
    });
    cx.run_until_parked();
    assert!(has_item(&section, "PVTI_7", cx));
    let order = section.read_with(cx, |section, _| {
        section
            .items()
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(order, vec!["PVTI_7", "PVTI_9"], "back in its place");
    let message = section.read_with(cx, |section, _| section.move_message().map(str::to_string));
    assert!(
        message
            .as_deref()
            .is_some_and(|message| message.contains("Could not remove")),
        "{message:?}"
    );
}

const OTHER_PROJECT: &str = "PVT_2";

/// [`rich_backend`] plus a second project holding one issue, and issue 12
/// that can be added.
fn two_projects() -> Arc<github::FakeGithubBackend> {
    let mut other = crate::projects::projects_section_tests::project();
    other.id = OTHER_PROJECT.into();
    other.number = 2;
    other.title = "Other".into();
    Arc::new(
        rich_backend()
            .with_issue(&repository(), fake_issue(12, "Login fails"))
            .with_project(&repository(), other)
            .with_project_details(
                OTHER_PROJECT,
                crate::projects::field_editor_tests::rich_details(),
            )
            .with_project_item(
                OTHER_PROJECT,
                crate::projects::projects_section_tests::item(
                    "PVTI_20",
                    crate::projects::projects_section_tests::issue(20),
                    None,
                ),
            ),
    )
}

fn item_ids(section: &Entity<ProjectsSection>, cx: &mut VisualTestContext) -> Vec<String> {
    section.read_with(cx, |section, _| {
        section.items().iter().map(|item| item.id.clone()).collect()
    })
}

#[gpui::test]
async fn an_add_answered_after_another_project_was_picked_leaves_it_alone(cx: &mut TestAppContext) {
    let backend = two_projects();
    let (_workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);
    section.update(cx, |section, cx| {
        section.open_add_item(cx);
        section.search_add_item("#12".into(), cx);
    });
    cx.run_until_parked();
    let found = section.read_with(cx, |section, _| {
        section.add_item_picker().expect("picker").candidates[0].clone()
    });
    let release = backend.hold_next_project_write();
    section.update(cx, |section, cx| {
        section.set_add_item_status(Some("done".into()), cx);
        section.add_item(found, cx);
        section.select_project(OTHER_PROJECT.into(), cx);
    });
    cx.run_until_parked();
    assert_eq!(item_ids(&section, cx), vec!["PVTI_20"]);
    release.send(()).ok();
    cx.run_until_parked();
    assert_eq!(
        item_ids(&section, cx),
        vec!["PVTI_20"],
        "the first project's new item is not shown in the second"
    );
    assert_eq!(
        backend.writes(),
        vec![FakeWrite::AddProjectItem(PROJECT.into(), "I_12".into())],
        "no status is set on the item from the other project"
    );
}

#[gpui::test]
async fn a_failed_removal_answered_after_another_project_was_picked_is_not_put_back(
    cx: &mut TestAppContext,
) {
    let backend = two_projects();
    let (_workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);
    backend.fail_next_project_write(GithubError::NotFound);
    let release = backend.hold_next_project_write();
    section.update(cx, |section, cx| {
        section.request_remove_item("PVTI_7", cx);
        section.confirm_remove_item(cx);
        section.select_project(OTHER_PROJECT.into(), cx);
    });
    cx.run_until_parked();
    release.send(()).ok();
    cx.run_until_parked();
    assert_eq!(item_ids(&section, cx), vec!["PVTI_20"]);
}

#[gpui::test]
async fn a_failed_removal_does_not_double_an_item_read_again_meanwhile(cx: &mut TestAppContext) {
    let backend = Arc::new(rich_backend());
    let (_workspace, view, cx) = projects_tab(backend.clone(), writable(), cx).await;
    let section = section(&view, cx);
    backend.fail_next_project_write(GithubError::NotFound);
    let release = backend.hold_next_project_write();
    section.update(cx, |section, cx| {
        section.request_remove_item("PVTI_7", cx);
        section.confirm_remove_item(cx);
        section.reread_project(cx);
    });
    cx.run_until_parked();
    assert_eq!(item_ids(&section, cx), vec!["PVTI_7", "PVTI_9"]);
    release.send(()).ok();
    cx.run_until_parked();
    assert_eq!(item_ids(&section, cx), vec!["PVTI_7", "PVTI_9"]);
}
