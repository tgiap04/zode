use github::{
    ProjectDetails, ProjectField, ProjectFieldKind, ProjectFieldOption, ProjectFieldValue,
    ProjectItemContent, ProjectIteration, ProjectSort, ProjectV2Item, ProjectValue, ProjectView,
    ProjectViewLayout,
};

use crate::projects::project_layout::{CardField, CardIcon, card_fields};
use crate::projects::{
    BoardColumn, ColumnKey, board_columns, cell_text, item_title, shows_board, sorted_items,
    table_fields,
};

fn option(id: &str, name: &str) -> ProjectFieldOption {
    ProjectFieldOption {
        id: id.into(),
        name: name.into(),
        color: None,
    }
}

fn field(id: &str, name: &str, kind: ProjectFieldKind) -> ProjectField {
    ProjectField {
        id: id.into(),
        name: name.into(),
        kind,
    }
}

/// Title, Status (Todo, Doing, Done), Priority (P1, P2), Sprint (one
/// finished, one current) and Points.
fn details() -> ProjectDetails {
    ProjectDetails {
        fields: vec![
            field("F_title", "Title", ProjectFieldKind::Title),
            field(
                "F_status",
                "Status",
                ProjectFieldKind::SingleSelect(vec![
                    option("todo", "Todo"),
                    option("doing", "Doing"),
                    option("done", "Done"),
                ]),
            ),
            field(
                "F_priority",
                "Priority",
                ProjectFieldKind::SingleSelect(vec![option("p1", "P1"), option("p2", "P2")]),
            ),
            field(
                "F_sprint",
                "Sprint",
                ProjectFieldKind::Iteration(vec![
                    ProjectIteration {
                        id: "s2".into(),
                        title: "Sprint 2".into(),
                        start_date: "2026-11-16".into(),
                        completed: false,
                    },
                    ProjectIteration {
                        id: "s1".into(),
                        title: "Sprint 1".into(),
                        start_date: "2026-11-02".into(),
                        completed: true,
                    },
                ]),
            ),
            field("F_points", "Points", ProjectFieldKind::Number),
            field("F_assignees", "Assignees", ProjectFieldKind::Assignees),
        ],
        views: Vec::new(),
    }
}

fn view(layout: ProjectViewLayout) -> ProjectView {
    ProjectView {
        id: "V".into(),
        number: 1,
        name: "View".into(),
        layout,
        filter: None,
        group_by_fields: Vec::new(),
        vertical_group_by_fields: Vec::new(),
        sort_by_fields: Vec::new(),
        visible_fields: Vec::new(),
    }
}

fn select(field_id: &str, option_id: &str) -> ProjectFieldValue {
    ProjectFieldValue {
        field_id: field_id.into(),
        value: ProjectValue::SingleSelect {
            option_id: option_id.into(),
            name: option_id.into(),
        },
    }
}

fn issue(number: u64, values: Vec<ProjectFieldValue>) -> ProjectV2Item {
    ProjectV2Item {
        id: format!("PVTI_{number}"),
        content: ProjectItemContent::Issue {
            number,
            title: format!("Issue {number}"),
            repository: "octo/repo".into(),
        },
        assignees: Vec::new(),
        assignee_avatars: Default::default(),
        state: None,
        sub_issues: None,
        labels: Vec::new(),
        field_values: values,
    }
}

fn hidden(values: Vec<ProjectFieldValue>) -> ProjectV2Item {
    ProjectV2Item {
        id: "PVTI_hidden".into(),
        content: ProjectItemContent::Redacted,
        assignees: Vec::new(),
        assignee_avatars: Default::default(),
        state: None,
        sub_issues: None,
        labels: Vec::new(),
        field_values: values,
    }
}

fn named(columns: &[BoardColumn]) -> Vec<(String, Vec<usize>)> {
    columns
        .iter()
        .map(|column| (column.name.clone(), column.items.clone()))
        .collect()
}

#[test]
fn the_board_groups_by_the_views_field_with_a_no_value_column() {
    let mut board = view(ProjectViewLayout::Board);
    board.group_by_fields = vec!["F_priority".into()];
    let items = vec![
        issue(
            1,
            vec![select("F_priority", "p2"), select("F_status", "done")],
        ),
        issue(2, vec![select("F_status", "todo")]),
        issue(3, vec![select("F_priority", "p1")]),
        issue(4, vec![select("F_priority", "gone")]),
    ];
    let columns = board_columns(Some(&board), &details(), &items);
    assert_eq!(
        named(&columns),
        vec![
            ("No Priority".to_string(), vec![1, 3]),
            ("P1".to_string(), vec![2]),
            ("P2".to_string(), vec![0]),
        ],
        "Priority, not Status; a deleted option counts as no value"
    );
    assert_eq!(columns[1].key, ColumnKey::Option("p1".into()));
    assert_eq!(columns[0].key, ColumnKey::NoValue);
}

#[test]
fn without_a_group_by_field_the_board_uses_status() {
    let items = vec![
        issue(1, vec![select("F_status", "doing")]),
        issue(2, Vec::new()),
    ];
    let columns = board_columns(Some(&view(ProjectViewLayout::Board)), &details(), &items);
    assert_eq!(
        named(&columns),
        vec![
            ("No Status".to_string(), vec![1]),
            ("Todo".to_string(), vec![]),
            ("Doing".to_string(), vec![0]),
            ("Done".to_string(), vec![]),
        ]
    );
}

#[test]
fn an_iteration_board_hides_finished_iterations_that_are_empty() {
    let mut board = view(ProjectViewLayout::Board);
    board.group_by_fields = vec!["F_sprint".into()];
    let in_sprint = |id: &str| ProjectFieldValue {
        field_id: "F_sprint".into(),
        value: ProjectValue::Iteration {
            iteration_id: id.into(),
            title: id.into(),
            start_date: String::new(),
        },
    };
    let current_only = vec![issue(1, vec![in_sprint("s2")])];
    assert_eq!(
        named(&board_columns(Some(&board), &details(), &current_only)),
        vec![
            ("No Sprint".to_string(), vec![]),
            ("Sprint 2".to_string(), vec![0]),
        ]
    );
    let with_finished = vec![issue(1, vec![in_sprint("s1")])];
    let columns = board_columns(Some(&board), &details(), &with_finished);
    assert_eq!(columns[2].key, ColumnKey::Iteration("s1".into()));
    assert_eq!(columns[2].items, vec![0]);
}

#[test]
fn a_hidden_item_stays_in_its_column_as_a_hidden_item() {
    let items = vec![hidden(vec![select("F_status", "done")])];
    let columns = board_columns(Some(&view(ProjectViewLayout::Board)), &details(), &items);
    assert_eq!(columns[3].items, vec![0]);
    assert_eq!(item_title(&items[0]), "Hidden item");
    assert_eq!(cell_text(&details().fields[0], &items[0]), "Hidden item");
}

#[test]
fn the_table_sorts_by_the_views_fields_with_empty_values_last() {
    let points = |value: f64| ProjectFieldValue {
        field_id: "F_points".into(),
        value: ProjectValue::Number(value),
    };
    let items = vec![
        issue(1, vec![points(2.0), select("F_status", "done")]),
        issue(2, vec![select("F_status", "todo")]),
        issue(3, vec![points(10.0), select("F_status", "done")]),
        issue(4, vec![points(2.0), select("F_status", "todo")]),
    ];
    let mut table = view(ProjectViewLayout::Table);
    table.sort_by_fields = vec![ProjectSort {
        field_id: "F_points".into(),
        descending: true,
    }];
    assert_eq!(
        sorted_items(Some(&table), &details(), &items),
        vec![2, 0, 3, 1],
        "10 before 2 (not as text), ties in project order, no points last"
    );
    table.sort_by_fields = vec![
        ProjectSort {
            field_id: "F_status".into(),
            descending: false,
        },
        ProjectSort {
            field_id: "F_points".into(),
            descending: false,
        },
    ];
    assert_eq!(
        sorted_items(Some(&table), &details(), &items),
        vec![3, 1, 0, 2],
        "options by their order on GitHub (Todo before Done), then points"
    );
    assert_eq!(
        sorted_items(Some(&view(ProjectViewLayout::Table)), &details(), &items),
        vec![0, 1, 2, 3]
    );
}

#[test]
fn the_table_shows_the_views_visible_fields_and_a_roadmap_is_a_table() {
    let mut table = view(ProjectViewLayout::Roadmap);
    table.visible_fields = vec!["F_title".into(), "F_points".into(), "F_gone".into()];
    let details = details();
    let names = table_fields(Some(&table), &details)
        .iter()
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["Title", "Points"]);
    assert!(!shows_board(Some(&table)));
    assert!(shows_board(Some(&view(ProjectViewLayout::Board))));
    let mut item = issue(5, Vec::new());
    item.assignees = vec!["octocat".into(), "hubot".into()];
    assert_eq!(cell_text(&details.fields[5], &item), "octocat, hubot");
}

#[test]
fn a_card_shows_the_views_other_fields_that_have_a_value() {
    let mut details = details();
    if let Some(ProjectFieldKind::SingleSelect(options)) = details
        .fields
        .iter_mut()
        .find(|field| field.id == "F_priority")
        .map(|field| &mut field.kind)
    {
        options[0].color = Some("RED".into());
    }
    let mut board = view(ProjectViewLayout::Board);
    board.group_by_fields = vec!["F_status".into()];
    board.visible_fields = vec![
        "F_title".into(),
        "F_status".into(),
        "F_points".into(),
        "F_priority".into(),
        "F_sprint".into(),
        "F_assignees".into(),
        "F_gone".into(),
    ];
    let points = ProjectFieldValue {
        field_id: "F_points".into(),
        value: ProjectValue::Number(3.),
    };
    let item = issue(
        1,
        vec![
            select("F_status", "doing"),
            select("F_priority", "p1"),
            points,
        ],
    );
    assert_eq!(
        card_fields(Some(&board), &details, &item),
        vec![
            CardField {
                name: "Points".into(),
                value: "3".into(),
                color: None,
                icon: None,
            },
            CardField {
                name: "Priority".into(),
                value: "p1".into(),
                color: Some("RED".into()),
                icon: None,
            },
        ],
        "in the view's order, without the column field, the title, the people or empty values"
    );
    assert_eq!(
        card_fields(
            Some(&board),
            &details,
            &hidden(vec![select("F_priority", "p1")])
        ),
        Vec::new(),
        "a hidden item shows nothing of itself"
    );
    assert_eq!(card_fields(None, &details, &item), Vec::new());
}

#[test]
fn linked_pull_requests_and_sub_issue_progress_show_as_counts() {
    let mut details = details();
    details.fields.push(field(
        "F_prs",
        "Linked pull requests",
        ProjectFieldKind::LinkedPullRequests,
    ));
    details.fields.push(field(
        "F_subs",
        "Sub-issues progress",
        ProjectFieldKind::SubIssuesProgress,
    ));
    let mut item = issue(
        1,
        vec![ProjectFieldValue {
            field_id: "F_prs".into(),
            value: ProjectValue::PullRequests {
                numbers: vec![12, 13],
                total: 2,
            },
        }],
    );
    item.sub_issues = Some(github::SubIssuesSummary {
        total: 3,
        completed: 1,
    });
    let prs = details.field("F_prs").expect("the field");
    let subs = details.field("F_subs").expect("the field");
    assert_eq!(cell_text(prs, &item), "#12, #13");
    assert_eq!(cell_text(subs, &item), "1/3");

    let mut board = view(ProjectViewLayout::Board);
    board.visible_fields = vec!["F_prs".into(), "F_subs".into()];
    assert_eq!(
        card_fields(Some(&board), &details, &item),
        vec![
            CardField {
                name: "Linked pull requests".into(),
                value: "2".into(),
                color: None,
                icon: Some(CardIcon::PullRequest),
            },
            CardField {
                name: "Sub-issues progress".into(),
                value: "1/3".into(),
                color: None,
                icon: Some(CardIcon::SubIssues),
            },
        ]
    );

    let plain = issue(2, Vec::new());
    assert_eq!(cell_text(prs, &plain), "");
    assert_eq!(
        cell_text(subs, &plain),
        "",
        "no sub-issues, nothing to show"
    );
    assert_eq!(card_fields(Some(&board), &details, &plain), Vec::new());
}
