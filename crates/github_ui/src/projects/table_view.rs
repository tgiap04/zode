use std::ops::Range;
use std::rc::Rc;

use github::{ProjectField, ProjectFieldKind, ProjectV2Item, ProjectValue};
use gpui::{App, Context, IntoElement, MouseButton, MouseDownEvent, WeakEntity, px};
use ui::{Table, Tooltip, prelude::*};

use crate::avatar::render_people;
use crate::projects::board_card_render::{card_icon, item_people};
use crate::projects::field_editor::{FieldEditing, field_editing};
use crate::projects::field_editor_render::FieldEditorParts;
use crate::projects::project_layout::{CardIcon, cell_text, sorted_items, table_fields, value_of};
use crate::projects::projects_section::ProjectsSection;
use crate::untrusted_text::reveal_hidden_characters;

/// How a cell is drawn besides its text.
enum CellDisplay {
    Text,
    /// `(login, avatar URL)` of the assignees.
    People(Vec<(String, Option<String>)>),
    /// An icon and a count, the cell's text on hover.
    Count(CardIcon, String),
}

struct Cell {
    text: SharedString,
    display: CellDisplay,
    field_id: String,
    /// Clicking it edits it here; other cells open the item.
    editable: bool,
}

impl ProjectsSection {
    /// The view's visible fields as columns, rows in the view's sort order.
    /// Text, number, date, single-select and iteration cells are edited in
    /// place; a right click on a row opens its menu.
    pub(crate) fn render_table(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(details) = self.details() else {
            return div().into_any_element();
        };
        let fields = table_fields(self.view(), details);
        if fields.is_empty() {
            return Label::new("This view shows no fields.")
                .color(Color::Muted)
                .into_any_element();
        }
        let header = fields
            .iter()
            .map(|field| SharedString::from(reveal_hidden_characters(&field.name).into_owned()))
            .collect::<Vec<_>>();
        let order = sorted_items(self.view(), details, self.items());
        let row_items = Rc::new(
            order
                .iter()
                .filter_map(|index| Some(self.items().get(*index)?.id.clone()))
                .collect::<Vec<_>>(),
        );
        let editing = self.field_edit().map(|edit| {
            (
                edit.item_id.clone(),
                edit.field_id.clone(),
                FieldEditorParts::of(edit),
            )
        });
        let handle = cx.entity().downgrade();
        let count = row_items.len();
        let row_handle = handle.clone();
        let rows_order = Rc::new(order.clone());
        div()
            .size_full()
            .child(
                Table::new(fields.len())
                    .striped()
                    .interactable(&self.table_state)
                    .map_row(move |(position, row), _window, _cx| {
                        let (Some(index), Some(item_id)) = (
                            order.get(position).copied(),
                            row_items.get(position).cloned(),
                        ) else {
                            return row.into_any_element();
                        };
                        let opener = row_handle.clone();
                        let menu = row_handle.clone();
                        row.debug_selector(move || format!("github-project-row-{position}"))
                            .cursor_pointer()
                            .on_click(move |_, window, cx| {
                                opener
                                    .update(cx, |section, cx| section.open_item(index, window, cx))
                                    .ok();
                            })
                            .on_mouse_down(
                                MouseButton::Right,
                                move |event: &MouseDownEvent, window, cx| {
                                    let position = event.position;
                                    menu.update(cx, |section, cx| {
                                        section.select_card(item_id.clone(), cx);
                                        section.deploy_card_menu(
                                            item_id.clone(),
                                            Some(position),
                                            window,
                                            cx,
                                        );
                                    })
                                    .ok();
                                    cx.stop_propagation();
                                },
                            )
                            .into_any_element()
                    })
                    .header(header)
                    // Cells are worked out for the rows on screen only, not
                    // for every item on each frame.
                    .uniform_list("github-project-table", count, move |range, _window, cx| {
                        let rows = visible_rows(&handle, &rows_order, range, cx);
                        rows.iter()
                            .map(|(position, item_id, cells)| {
                                let (position, item_id) = (*position, item_id.as_str());
                                cells
                                    .iter()
                                    .enumerate()
                                    .map(|(column, cell)| {
                                        if let Some((_, _, parts)) = editing.as_ref().filter(
                                            |(edited_item, edited_field, _)| {
                                                edited_item == item_id
                                                    && *edited_field == cell.field_id
                                            },
                                        ) {
                                            return parts.render(handle.clone(), cx);
                                        }
                                        render_cell(
                                            cell,
                                            item_id,
                                            position,
                                            column,
                                            handle.clone(),
                                            cx,
                                        )
                                    })
                                    .collect()
                            })
                            .collect()
                    }),
            )
            .into_any_element()
    }
}

/// `(position, item id, cells)` for the rows of `range`, read from the
/// section as it is now.
fn visible_rows(
    section: &WeakEntity<ProjectsSection>,
    order: &[usize],
    range: Range<usize>,
    cx: &App,
) -> Vec<(usize, String, Vec<Cell>)> {
    let Some(section) = section.upgrade() else {
        return Vec::new();
    };
    let section = section.read(cx);
    let Some(details) = section.details() else {
        return Vec::new();
    };
    let fields = table_fields(section.view(), details);
    range
        .filter_map(|position| {
            let item = section.items().get(*order.get(position)?)?;
            let hidden = item.content == github::ProjectItemContent::Redacted;
            let cells = fields
                .iter()
                .map(|field| Cell {
                    text: reveal_hidden_characters(&cell_text(field, item))
                        .into_owned()
                        .into(),
                    display: cell_display(field, item),
                    field_id: field.id.clone(),
                    editable: !hidden
                        && matches!(
                            field_editing(field),
                            FieldEditing::Typed | FieldEditing::Choice
                        ),
                })
                .collect();
            Some((position, item.id.clone(), cells))
        })
        .collect()
}

fn cell_display(field: &ProjectField, item: &ProjectV2Item) -> CellDisplay {
    match (&field.kind, value_of(item, &field.id)) {
        (ProjectFieldKind::Assignees, _) if !item.assignees.is_empty() => {
            CellDisplay::People(item_people(item))
        }
        (ProjectFieldKind::LinkedPullRequests, Some(ProjectValue::PullRequests { total, .. })) => {
            CellDisplay::Count(CardIcon::PullRequest, total.to_string())
        }
        (ProjectFieldKind::SubIssuesProgress, _) if item.sub_issues.is_some() => {
            match cell_text(field, item) {
                text if text.is_empty() => CellDisplay::Text,
                text => CellDisplay::Count(CardIcon::SubIssues, text),
            }
        }
        _ => CellDisplay::Text,
    }
}

fn render_cell(
    cell: &Cell,
    item_id: &str,
    position: usize,
    column: usize,
    handle: gpui::WeakEntity<ProjectsSection>,
    cx: &App,
) -> AnyElement {
    let cell_id = ("github-project-cell", position * 1000 + column);
    match &cell.display {
        CellDisplay::People(people) => {
            return render_people(cell_id, people, px(200.), cx)
                .unwrap_or_else(|| div().into_any_element());
        }
        CellDisplay::Count(icon, count) => {
            let (icon, color) = card_icon(*icon, count);
            return h_flex()
                .id(cell_id)
                .gap_1()
                .tooltip(Tooltip::text(cell.text.clone()))
                .child(Icon::new(icon).size(IconSize::Small).color(color))
                .child(
                    Label::new(count.clone())
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .into_any_element();
        }
        CellDisplay::Text => {}
    }
    let label = Label::new(cell.text.clone())
        .size(LabelSize::Small)
        .truncate();
    if !cell.editable {
        return label.into_any_element();
    }
    let item_id = item_id.to_string();
    let field_id = cell.field_id.clone();
    div()
        .id(cell_id)
        .debug_selector(move || format!("github-project-cell-{position}-{column}"))
        .w_full()
        .min_h_4()
        .child(label)
        .on_click(move |event, window, cx| {
            cx.stop_propagation();
            let position = event.position();
            handle
                .update(cx, |section, cx| {
                    section.edit_field_at(&item_id, &field_id, Some(position), window, cx)
                })
                .ok();
        })
        .into_any_element()
}
