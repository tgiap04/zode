use serde_json::json;

use crate::graphql::parse_project_item;
use crate::graphql_projects::parse_project_details;
use crate::model::Label;
use crate::projects::{
    ProjectField, ProjectFieldKind, ProjectFieldOption, ProjectFieldValue, ProjectIteration,
    ProjectSort, ProjectValue, ProjectView, ProjectViewLayout,
};

fn id_node(id: &str) -> serde_json::Value {
    json!({ "id": id })
}

#[test]
fn fields_and_views_are_read() {
    let project = json!({
        "fields": {"nodes": [
            {"__typename": "ProjectV2Field", "id": "F_title", "name": "Title", "dataType": "TITLE"},
            {"__typename": "ProjectV2Field", "id": "F_assignees", "name": "Assignees", "dataType": "ASSIGNEES"},
            {"__typename": "ProjectV2SingleSelectField", "id": "F_status", "name": "Status", "dataType": "SINGLE_SELECT",
             "options": [{"id": "o_todo", "name": "Todo", "color": "GRAY"}, {"id": "o_done", "name": "Done", "color": "GREEN"}]},
            {"__typename": "ProjectV2IterationField", "id": "F_sprint", "name": "Sprint", "dataType": "ITERATION",
             "configuration": {
                 "iterations": [{"id": "it_2", "title": "Sprint 2", "startDate": "2026-11-16"}],
                 "completedIterations": [{"id": "it_1", "title": "Sprint 1", "startDate": "2026-11-02"}]
             }},
            {"__typename": "ProjectV2Field", "id": "F_points", "name": "Points", "dataType": "NUMBER"},
            {"__typename": "ProjectV2Field", "id": "F_reviewers", "name": "Reviewers", "dataType": "REVIEWERS"},
            {}
        ]},
        "views": {"nodes": [
            {"id": "V_1", "number": 1, "name": "Board", "layout": "BOARD_LAYOUT", "filter": "is:open",
             "groupByFields": {"nodes": [id_node("F_status")]},
             "verticalGroupByFields": {"nodes": [id_node("F_sprint")]},
             "sortByFields": {"nodes": [{"direction": "DESC", "field": id_node("F_points")}]},
             "visibleFields": {"nodes": [id_node("F_title"), id_node("F_assignees")]}},
            {"id": "V_2", "number": 2, "name": "Plan", "layout": "ROADMAP_LAYOUT", "filter": null,
             "groupByFields": {"nodes": []},
             "verticalGroupByFields": {"nodes": []},
             "sortByFields": {"nodes": []},
             "visibleFields": {"nodes": []}}
        ]}
    });
    let details = parse_project_details(&project).expect("details");
    assert_eq!(
        details.fields,
        vec![
            ProjectField {
                id: "F_title".into(),
                name: "Title".into(),
                kind: ProjectFieldKind::Title,
            },
            ProjectField {
                id: "F_assignees".into(),
                name: "Assignees".into(),
                kind: ProjectFieldKind::Assignees,
            },
            ProjectField {
                id: "F_status".into(),
                name: "Status".into(),
                kind: ProjectFieldKind::SingleSelect(vec![
                    ProjectFieldOption {
                        id: "o_todo".into(),
                        name: "Todo".into(),
                        color: Some("GRAY".into()),
                    },
                    ProjectFieldOption {
                        id: "o_done".into(),
                        name: "Done".into(),
                        color: Some("GREEN".into()),
                    },
                ]),
            },
            ProjectField {
                id: "F_sprint".into(),
                name: "Sprint".into(),
                kind: ProjectFieldKind::Iteration(vec![
                    ProjectIteration {
                        id: "it_2".into(),
                        title: "Sprint 2".into(),
                        start_date: "2026-11-16".into(),
                        completed: false,
                    },
                    ProjectIteration {
                        id: "it_1".into(),
                        title: "Sprint 1".into(),
                        start_date: "2026-11-02".into(),
                        completed: true,
                    },
                ]),
            },
            ProjectField {
                id: "F_points".into(),
                name: "Points".into(),
                kind: ProjectFieldKind::Number,
            },
            ProjectField {
                id: "F_reviewers".into(),
                name: "Reviewers".into(),
                kind: ProjectFieldKind::Other,
            },
        ],
        "a field of a kind the query does not ask about is skipped"
    );
    assert_eq!(
        details.views,
        vec![
            ProjectView {
                id: "V_1".into(),
                number: 1,
                name: "Board".into(),
                layout: ProjectViewLayout::Board,
                filter: Some("is:open".into()),
                group_by_fields: vec!["F_status".into()],
                vertical_group_by_fields: vec!["F_sprint".into()],
                sort_by_fields: vec![ProjectSort {
                    field_id: "F_points".into(),
                    descending: true,
                }],
                visible_fields: vec!["F_title".into(), "F_assignees".into()],
            },
            ProjectView {
                id: "V_2".into(),
                number: 2,
                name: "Plan".into(),
                layout: ProjectViewLayout::Roadmap,
                filter: None,
                group_by_fields: Vec::new(),
                vertical_group_by_fields: Vec::new(),
                sort_by_fields: Vec::new(),
                visible_fields: Vec::new(),
            },
        ]
    );
}

#[test]
fn an_item_carries_its_people_labels_and_field_values() {
    let item = json!({
        "id": "PVTI_1", "type": "ISSUE",
        "content": {
            "__typename": "Issue", "number": 7, "title": "Bug",
            "repository": {"nameWithOwner": "octo/repo"},
            "assignees": {"nodes": [{"login": "octocat"}, {"login": "hubot"}]},
            "labels": {"nodes": [{"name": "bug", "color": "d73a4a", "description": null}]}
        },
        "fieldValues": {"nodes": [
            {"__typename": "ProjectV2ItemFieldTextValue", "text": "Bug", "field": id_node("F_title")},
            {"__typename": "ProjectV2ItemFieldSingleSelectValue", "optionId": "o_todo", "name": "Todo", "field": id_node("F_status")},
            {"__typename": "ProjectV2ItemFieldIterationValue", "iterationId": "it_2", "title": "Sprint 2", "startDate": "2026-11-16", "field": id_node("F_sprint")},
            {"__typename": "ProjectV2ItemFieldNumberValue", "number": 3.5, "field": id_node("F_points")},
            {"__typename": "ProjectV2ItemFieldDateValue", "date": "2026-12-01", "field": id_node("F_due")},
            {"__typename": "ProjectV2ItemFieldMilestoneValue", "milestone": {"title": "v1"}, "field": id_node("F_milestone")},
            {"__typename": "ProjectV2ItemFieldUserValue"},
            {"__typename": "ProjectV2ItemFieldSingleSelectValue", "optionId": "o_x", "name": "X"}
        ]}
    });
    let item = parse_project_item(&item).expect("an item");
    assert_eq!(item.assignees, vec!["octocat", "hubot"]);
    assert_eq!(
        item.labels,
        vec![Label {
            name: "bug".into(),
            color: "d73a4a".into(),
            description: None,
        }]
    );
    let value = |field_id: &str, value: ProjectValue| ProjectFieldValue {
        field_id: field_id.into(),
        value,
    };
    assert_eq!(
        item.field_values,
        vec![
            value("F_title", ProjectValue::Text("Bug".into())),
            value(
                "F_status",
                ProjectValue::SingleSelect {
                    option_id: "o_todo".into(),
                    name: "Todo".into(),
                },
            ),
            value(
                "F_sprint",
                ProjectValue::Iteration {
                    iteration_id: "it_2".into(),
                    title: "Sprint 2".into(),
                    start_date: "2026-11-16".into(),
                },
            ),
            value("F_points", ProjectValue::Number(3.5)),
            value("F_due", ProjectValue::Date("2026-12-01".into())),
            value("F_milestone", ProjectValue::Milestone("v1".into())),
        ],
        "values of kinds not drawn, or without a field, are left out"
    );
}

#[test]
fn a_hidden_item_keeps_nothing_of_its_content() {
    let item = json!({
        "id": "PVTI_9", "type": "REDACTED", "content": null,
        "fieldValues": {"nodes": [
            {"__typename": "ProjectV2ItemFieldSingleSelectValue", "optionId": "o_done", "name": "Done", "field": id_node("F_status")}
        ]}
    });
    let item = parse_project_item(&item).expect("an item");
    assert!(item.assignees.is_empty() && item.labels.is_empty());
    assert_eq!(item.field_values.len(), 1, "its column is still known");
}
