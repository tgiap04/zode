//! How a view's items are laid out: the board's columns and the table's
//! columns and row order. Pure, so it can be checked without a window.
//!
//! Items are named by their index into the list they were laid out from, so
//! a column or a row is a list of indices, kept in the order drawn.

use std::cmp::Ordering;

use github::{
    ProjectDetails, ProjectField, ProjectFieldKind, ProjectItemContent, ProjectV2Item,
    ProjectValue, ProjectView, ProjectViewLayout,
};

/// Which group of the column field a board column holds.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ColumnKey {
    /// Items with no value in the column field, or a value whose option or
    /// iteration no longer exists.
    NoValue,
    Option(String),
    Iteration(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardColumn {
    pub key: ColumnKey,
    pub name: String,
    /// Indices into the items, in the order drawn.
    pub items: Vec<usize>,
}

/// Board for a board view; table for the rest (a roadmap is shown as a
/// table, and so is a project without views).
pub fn shows_board(view: Option<&ProjectView>) -> bool {
    view.is_some_and(|view| view.layout == ProjectViewLayout::Board)
}

/// The field a board's columns come from: the view's first group-by field
/// that is a single select or an iteration, else the project's single-select
/// `Status`, which is what GitHub's boards fall back to.
pub fn column_field<'a>(
    view: Option<&ProjectView>,
    details: &'a ProjectDetails,
) -> Option<&'a ProjectField> {
    let groups = |field: &&ProjectField| {
        matches!(
            field.kind,
            ProjectFieldKind::SingleSelect(_) | ProjectFieldKind::Iteration(_)
        )
    };
    view.into_iter()
        .flat_map(|view| view.group_by_fields.iter())
        .filter_map(|field_id| details.field(field_id))
        .find(groups)
        .or_else(|| {
            details.fields.iter().find(|field| {
                field.name.eq_ignore_ascii_case("Status")
                    && matches!(field.kind, ProjectFieldKind::SingleSelect(_))
            })
        })
}

pub fn board_columns(
    view: Option<&ProjectView>,
    details: &ProjectDetails,
    items: &[ProjectV2Item],
) -> Vec<BoardColumn> {
    let ordered = sorted_items(view, details, items);
    let Some(field) = column_field(view, details) else {
        return vec![BoardColumn {
            key: ColumnKey::NoValue,
            name: "No value".to_string(),
            items: ordered,
        }];
    };
    let mut columns = vec![BoardColumn {
        key: ColumnKey::NoValue,
        name: format!("No {}", field.name),
        items: Vec::new(),
    }];
    match &field.kind {
        ProjectFieldKind::SingleSelect(options) => {
            columns.extend(options.iter().map(|option| BoardColumn {
                key: ColumnKey::Option(option.id.clone()),
                name: option.name.clone(),
                items: Vec::new(),
            }))
        }
        ProjectFieldKind::Iteration(iterations) => {
            columns.extend(iterations.iter().map(|iteration| BoardColumn {
                key: ColumnKey::Iteration(iteration.id.clone()),
                name: iteration.title.clone(),
                items: Vec::new(),
            }))
        }
        _ => {}
    }
    for index in ordered {
        let key = items
            .get(index)
            .and_then(|item| value_of(item, &field.id))
            .and_then(|value| match value {
                ProjectValue::SingleSelect { option_id, .. } => {
                    Some(ColumnKey::Option(option_id.clone()))
                }
                ProjectValue::Iteration { iteration_id, .. } => {
                    Some(ColumnKey::Iteration(iteration_id.clone()))
                }
                _ => None,
            });
        let position = key
            .and_then(|key| columns.iter().position(|column| column.key == key))
            .unwrap_or(0);
        if let Some(column) = columns.get_mut(position) {
            column.items.push(index);
        }
    }
    // A finished iteration is a column only while it still holds items.
    if let ProjectFieldKind::Iteration(iterations) = &field.kind {
        columns.retain(|column| match &column.key {
            ColumnKey::Iteration(id) => {
                !column.items.is_empty()
                    || iterations
                        .iter()
                        .any(|iteration| &iteration.id == id && !iteration.completed)
            }
            _ => true,
        });
    }
    columns
}

/// The view's visible fields, in its order; the title alone when the view
/// names none the project still has.
pub fn table_fields<'a>(
    view: Option<&ProjectView>,
    details: &'a ProjectDetails,
) -> Vec<&'a ProjectField> {
    let visible = view
        .into_iter()
        .flat_map(|view| view.visible_fields.iter())
        .filter_map(|field_id| details.field(field_id))
        .collect::<Vec<_>>();
    if !visible.is_empty() {
        return visible;
    }
    details
        .fields
        .iter()
        .filter(|field| field.kind == ProjectFieldKind::Title)
        .collect()
}

/// Item indices in the view's sort order, applied here because GitHub does
/// not sort the items it returns. Ties, and views without a sort, keep the
/// project's own order; empty values go last either way.
pub fn sorted_items(
    view: Option<&ProjectView>,
    details: &ProjectDetails,
    items: &[ProjectV2Item],
) -> Vec<usize> {
    let mut order = (0..items.len()).collect::<Vec<_>>();
    let sorts = view
        .into_iter()
        .flat_map(|view| view.sort_by_fields.iter())
        .filter_map(|sort| Some((details.field(&sort.field_id)?, sort.descending)))
        .collect::<Vec<_>>();
    if sorts.is_empty() {
        return order;
    }
    let keys = items
        .iter()
        .map(|item| {
            sorts
                .iter()
                .map(|(field, _)| sort_key(field, item))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    order.sort_by(|left, right| {
        for (position, (_, descending)) in sorts.iter().enumerate() {
            let left_key = keys.get(*left).and_then(|keys| keys.get(position));
            let right_key = keys.get(*right).and_then(|keys| keys.get(position));
            let ordering = match (left_key.cloned().flatten(), right_key.cloned().flatten()) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) if *descending => right.compare(&left),
                (Some(left), Some(right)) => left.compare(&right),
            };
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        Ordering::Equal
    });
    order
}

#[derive(Clone, Debug)]
enum SortKey {
    Text(String),
    Number(f64),
    Position(usize),
}

impl SortKey {
    fn compare(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Text(left), Self::Text(right)) => left.cmp(right),
            (Self::Number(left), Self::Number(right)) => left.total_cmp(right),
            (Self::Position(left), Self::Position(right)) => left.cmp(right),
            _ => Ordering::Equal,
        }
    }
}

fn sort_key(field: &ProjectField, item: &ProjectV2Item) -> Option<SortKey> {
    match (&field.kind, value_of(item, &field.id)) {
        (ProjectFieldKind::Number, Some(ProjectValue::Number(number))) => {
            Some(SortKey::Number(*number))
        }
        (
            ProjectFieldKind::SingleSelect(options),
            Some(ProjectValue::SingleSelect { option_id, .. }),
        ) => options
            .iter()
            .position(|option| &option.id == option_id)
            .map(SortKey::Position),
        (ProjectFieldKind::Iteration(_), Some(ProjectValue::Iteration { start_date, .. })) => {
            Some(SortKey::Text(start_date.clone()))
        }
        _ => {
            let text = cell_text(field, item);
            (!text.is_empty()).then(|| SortKey::Text(text.to_lowercase()))
        }
    }
}

pub fn value_of<'a>(item: &'a ProjectV2Item, field_id: &str) -> Option<&'a ProjectValue> {
    item.field_values
        .iter()
        .find(|value| value.field_id == field_id)
        .map(|value| &value.value)
}

/// A field value a board card shows under its title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardField {
    pub name: String,
    pub value: String,
    /// GitHub's colour name (`RED`, `BLUE`, …) of a single-select option.
    pub color: Option<String>,
    pub icon: Option<CardIcon>,
}

/// Drawn before a count, which means nothing without it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardIcon {
    PullRequest,
    SubIssues,
}

/// The view's visible fields that hold a value on `item`, in the view's
/// order, less what the card draws already: the title, the people, the
/// labels, the repository and the field the columns come from.
pub fn card_fields(
    view: Option<&ProjectView>,
    details: &ProjectDetails,
    item: &ProjectV2Item,
) -> Vec<CardField> {
    let Some(view) = view else {
        return Vec::new();
    };
    if item.content == ProjectItemContent::Redacted {
        return Vec::new();
    }
    let column = column_field(Some(view), details).map(|field| field.id.as_str());
    view.visible_fields
        .iter()
        .filter_map(|field_id| details.field(field_id))
        .filter(|field| Some(field.id.as_str()) != column)
        .filter(|field| {
            !matches!(
                field.kind,
                ProjectFieldKind::Title
                    | ProjectFieldKind::Assignees
                    | ProjectFieldKind::Labels
                    | ProjectFieldKind::Repository
                    | ProjectFieldKind::Other
            )
        })
        .filter_map(|field| {
            let value = cell_text(field, item);
            if value.is_empty() {
                return None;
            }
            let color = match (&field.kind, value_of(item, &field.id)) {
                (
                    ProjectFieldKind::SingleSelect(options),
                    Some(ProjectValue::SingleSelect { option_id, .. }),
                ) => options
                    .iter()
                    .find(|option| &option.id == option_id)
                    .and_then(|option| option.color.clone()),
                _ => None,
            };
            let (value, icon) = match (&field.kind, value_of(item, &field.id)) {
                (
                    ProjectFieldKind::LinkedPullRequests,
                    Some(ProjectValue::PullRequests { total, .. }),
                ) => (total.to_string(), Some(CardIcon::PullRequest)),
                (ProjectFieldKind::SubIssuesProgress, _) => (value, Some(CardIcon::SubIssues)),
                _ => (value, None),
            };
            Some(CardField {
                name: field.name.clone(),
                value,
                color,
                icon,
            })
        })
        .collect()
}

/// What an item shows as its title. A hidden item (from a repository the
/// token cannot read) shows nothing of itself.
pub fn item_title(item: &ProjectV2Item) -> &str {
    match &item.content {
        ProjectItemContent::Issue { title, .. }
        | ProjectItemContent::PullRequest { title, .. }
        | ProjectItemContent::DraftIssue { title } => title,
        ProjectItemContent::Redacted => "Hidden item",
    }
}

/// A table cell as plain text; empty when the item has no value there.
pub fn cell_text(field: &ProjectField, item: &ProjectV2Item) -> String {
    match &field.kind {
        ProjectFieldKind::Title => item_title(item).to_string(),
        ProjectFieldKind::Assignees => item.assignees.join(", "),
        ProjectFieldKind::Labels => item
            .labels
            .iter()
            .map(|label| label.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        ProjectFieldKind::SubIssuesProgress => item
            .sub_issues
            .as_ref()
            .filter(|summary| summary.total > 0)
            .map(|summary| format!("{}/{}", summary.completed, summary.total))
            .unwrap_or_default(),
        ProjectFieldKind::Repository => match &item.content {
            ProjectItemContent::Issue { repository, .. }
            | ProjectItemContent::PullRequest { repository, .. } => repository.clone(),
            _ => String::new(),
        },
        _ => match value_of(item, &field.id) {
            Some(ProjectValue::Text(text) | ProjectValue::Date(text)) => text.clone(),
            Some(ProjectValue::Number(number)) => number.to_string(),
            Some(ProjectValue::SingleSelect { name, .. }) => name.clone(),
            Some(ProjectValue::Iteration { title, .. }) => title.clone(),
            Some(ProjectValue::Milestone(title)) => title.clone(),
            Some(ProjectValue::PullRequests { numbers, total }) => {
                let mut text = numbers
                    .iter()
                    .map(|number| format!("#{number}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let unlisted = total.saturating_sub(numbers.len() as u64);
                if unlisted > 0 {
                    text.push_str(&format!(" +{unlisted}"));
                }
                text
            }
            None => String::new(),
        },
    }
}
