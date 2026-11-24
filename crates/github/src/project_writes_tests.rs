use futures::executor::block_on;

use crate::backend::GithubBackend;
use crate::error::GithubError;
use crate::fake_backend::{FakeGithubBackend, FakeWrite};
use crate::model::{ProjectItemContent, ProjectV2Item};
use crate::projects::{
    ProjectDetails, ProjectField, ProjectFieldInput, ProjectFieldKind, ProjectFieldOption,
    ProjectFieldValue, ProjectValue,
};

fn status_details() -> ProjectDetails {
    ProjectDetails {
        fields: vec![ProjectField {
            id: "F_status".to_string(),
            name: "Status".to_string(),
            kind: ProjectFieldKind::SingleSelect(vec![ProjectFieldOption {
                id: "done".to_string(),
                name: "Done".to_string(),
                color: None,
            }]),
        }],
        views: Vec::new(),
    }
}

fn draft(id: &str) -> ProjectV2Item {
    ProjectV2Item {
        id: id.to_string(),
        content: ProjectItemContent::DraftIssue {
            title: id.to_string(),
        },
        assignees: Vec::new(),
        assignee_avatars: Default::default(),
        state: None,
        sub_issues: None,
        labels: Vec::new(),
        field_values: Vec::new(),
    }
}

fn read_items(backend: &FakeGithubBackend) -> Vec<ProjectV2Item> {
    block_on(backend.list_project_items("PVT_1".to_string(), None))
        .expect("items")
        .items
}

#[test]
fn the_fake_applies_field_writes_and_fails_only_the_write_it_was_told_to() {
    let backend = FakeGithubBackend::new()
        .with_project_details("PVT_1", status_details())
        .with_project_item("PVT_1", draft("PVTI_1"));
    let done = Some(ProjectFieldInput::SingleSelect {
        option_id: "done".to_string(),
    });
    backend.fail_next_project_write(GithubError::NotFound);
    let failed = block_on(backend.set_project_item_field(
        "PVT_1".to_string(),
        "PVTI_1".to_string(),
        "F_status".to_string(),
        done.clone(),
    ));
    assert_eq!(failed, Err(GithubError::NotFound));
    assert!(read_items(&backend)[0].field_values.is_empty());

    block_on(backend.set_project_item_field(
        "PVT_1".to_string(),
        "PVTI_1".to_string(),
        "F_status".to_string(),
        done.clone(),
    ))
    .expect("set");
    assert_eq!(
        read_items(&backend)[0].field_values,
        vec![ProjectFieldValue {
            field_id: "F_status".to_string(),
            value: ProjectValue::SingleSelect {
                option_id: "done".to_string(),
                name: "Done".to_string(),
            },
        }]
    );
    block_on(backend.set_project_item_field(
        "PVT_1".to_string(),
        "PVTI_1".to_string(),
        "F_status".to_string(),
        None,
    ))
    .expect("cleared");
    assert!(read_items(&backend)[0].field_values.is_empty());
    let write = |value| {
        FakeWrite::SetProjectItemField(
            "PVT_1".to_string(),
            "PVTI_1".to_string(),
            "F_status".to_string(),
            value,
        )
    };
    assert_eq!(
        backend.writes(),
        vec![write(done.clone()), write(done), write(None)]
    );
}

#[test]
fn the_fake_reorders_items_after_the_one_named() {
    let backend = FakeGithubBackend::new()
        .with_project_item("PVT_1", draft("PVTI_1"))
        .with_project_item("PVT_1", draft("PVTI_2"))
        .with_project_item("PVT_1", draft("PVTI_3"));
    block_on(backend.move_project_item(
        "PVT_1".to_string(),
        "PVTI_3".to_string(),
        Some("PVTI_1".to_string()),
    ))
    .expect("moved");
    block_on(backend.move_project_item("PVT_1".to_string(), "PVTI_2".to_string(), None))
        .expect("moved to the top");
    let order = read_items(&backend)
        .into_iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    assert_eq!(order, vec!["PVTI_2", "PVTI_1", "PVTI_3"]);
    assert_eq!(
        backend.writes(),
        vec![
            FakeWrite::MoveProjectItem(
                "PVT_1".to_string(),
                "PVTI_3".to_string(),
                Some("PVTI_1".to_string())
            ),
            FakeWrite::MoveProjectItem("PVT_1".to_string(), "PVTI_2".to_string(), None),
        ]
    );
}

#[test]
fn the_fake_stores_typed_values_and_adds_and_removes_items() {
    let details = ProjectDetails {
        fields: vec![
            ProjectField {
                id: "F_notes".to_string(),
                name: "Notes".to_string(),
                kind: ProjectFieldKind::Text,
            },
            ProjectField {
                id: "F_points".to_string(),
                name: "Points".to_string(),
                kind: ProjectFieldKind::Number,
            },
        ],
        views: Vec::new(),
    };
    let repository = crate::model::RepositoryId::new("octo", "repo").expect("valid");
    let backend = FakeGithubBackend::new()
        .with_project_details("PVT_1", details)
        .with_issue(&repository, crate::fake_backend::fake_issue(12, "Login"))
        .with_project_item("PVT_1", draft("PVTI_1"));
    block_on(backend.set_project_item_field(
        "PVT_1".to_string(),
        "PVTI_1".to_string(),
        "F_points".to_string(),
        Some(ProjectFieldInput::Number { number: 3.0 }),
    ))
    .expect("set");
    assert_eq!(
        read_items(&backend)[0].field_values,
        vec![ProjectFieldValue {
            field_id: "F_points".to_string(),
            value: ProjectValue::Number(3.0),
        }]
    );
    let added =
        block_on(backend.add_project_item("PVT_1".to_string(), "I_12".to_string())).expect("added");
    let again = block_on(backend.add_project_item("PVT_1".to_string(), "I_12".to_string()))
        .expect("already there");
    assert_eq!(added, again, "adding twice answers with the same item");
    assert_eq!(
        read_items(&backend)[1].content,
        ProjectItemContent::Issue {
            number: 12,
            title: "Login".to_string(),
            repository: "octo/repo".to_string(),
        }
    );
    block_on(backend.delete_project_item("PVT_1".to_string(), added)).expect("removed");
    assert_eq!(read_items(&backend).len(), 1);
}

#[test]
fn the_fake_offers_the_viewer_alone_unless_told_of_an_organization() {
    let repository = crate::model::RepositoryId::new("octo", "repo").expect("a repository");
    let personal = FakeGithubBackend::new();
    let owners = block_on(personal.project_owners(&repository)).expect("owners");
    assert_eq!(owners.viewer.login, "octocat");
    assert_eq!(owners.organization, None);

    let organization = crate::projects::ProjectOwner {
        id: "O_octo".to_string(),
        login: "octo".to_string(),
        avatar_url: None,
    };
    let told = crate::projects::ProjectOwners {
        repository_id: "R_1".to_string(),
        viewer: owners.viewer,
        organization: Some(organization),
    };
    let backend = FakeGithubBackend::new().with_project_owners(&repository, told.clone());
    assert_eq!(block_on(backend.project_owners(&repository)), Ok(told));
}

#[test]
fn the_fake_creates_a_linked_project_and_fails_only_the_create_it_was_told_to() {
    let repository = crate::model::RepositoryId::new("octo", "repo").expect("a repository");
    let backend = FakeGithubBackend::new();
    let owners = block_on(backend.project_owners(&repository)).expect("owners");
    backend.fail_next_project_write(GithubError::Forbidden {
        message: "no".to_string(),
    });
    let refused = block_on(backend.create_project(
        owners.viewer.id.clone(),
        "Launch".to_string(),
        owners.repository_id.clone(),
    ));
    assert_eq!(
        refused,
        Err(GithubError::Forbidden {
            message: "no".to_string()
        })
    );
    let created = block_on(backend.create_project(
        owners.viewer.id.clone(),
        "Launch".to_string(),
        owners.repository_id.clone(),
    ))
    .expect("created");
    assert_eq!(created.title, "Launch");
    let listed = block_on(backend.list_projects(&repository, None))
        .expect("listed")
        .items;
    assert_eq!(listed, vec![created]);
    let write =
        FakeWrite::CreateProject(owners.viewer.id, "Launch".to_string(), owners.repository_id);
    assert_eq!(backend.writes(), vec![write.clone(), write]);
}
