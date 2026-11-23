//! Moving a card: setting the item's value in the board's column field, and
//! its place in the project's order. Both show at once and are sent through
//! the token's write queue; a write that fails puts the card back. Field
//! edits go through the same field write.

use github::{
    GithubError, GithubResult, ProjectField, ProjectFieldInput, ProjectFieldKind,
    ProjectFieldValue, ProjectV2Item, ProjectValue,
};
use gpui::{Context, SharedString};

use crate::github_view_session::report_if_revoked;
use crate::projects::project_layout::{
    ColumnKey, board_columns, column_field, item_title, value_of,
};
use crate::projects::projects_section::ProjectsSection;

/// What writing to a project needs; `read:project` only reads.
pub const PROJECT_WRITE_SCOPE: &str = "project";

pub const WRITE_SCOPE_GUIDANCE: &str = "Changing a project needs the `project` scope; this token can only read projects. With the GitHub CLI, run `gh auth refresh -s project`, then check again; or sign in with GitHub, which asks for it.";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FieldWriteKey {
    pub item_id: String,
    pub field_id: String,
}

/// The writes still out for one item's field. Writes run one at a time in
/// the order made, so `confirmed` follows GitHub's value; only the latest
/// write may change what is shown, so an earlier failure cannot undo a later
/// move that went through.
pub struct PendingFieldWrites {
    pub latest: u64,
    pub in_flight: usize,
    pub confirmed: Option<ProjectValue>,
}

/// The column a value puts an item in.
pub fn column_key_of(value: Option<&ProjectValue>) -> ColumnKey {
    match value {
        Some(ProjectValue::SingleSelect { option_id, .. }) => ColumnKey::Option(option_id.clone()),
        Some(ProjectValue::Iteration { iteration_id, .. }) => {
            ColumnKey::Iteration(iteration_id.clone())
        }
        _ => ColumnKey::NoValue,
    }
}

/// What asked for a field write, which decides how its failure is told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FieldWriteOrigin {
    Move,
    Edit,
    Choice,
}

/// What moving to `target` writes and then shows, or `None` when the column
/// is not one of the field's.
pub(super) fn value_for(
    field: &ProjectField,
    target: &ColumnKey,
) -> Option<Option<(ProjectFieldInput, ProjectValue)>> {
    match (&field.kind, target) {
        (_, ColumnKey::NoValue) => Some(None),
        (ProjectFieldKind::SingleSelect(options), ColumnKey::Option(option_id)) => {
            let option = options.iter().find(|option| &option.id == option_id)?;
            Some(Some((
                ProjectFieldInput::SingleSelect {
                    option_id: option.id.clone(),
                },
                ProjectValue::SingleSelect {
                    option_id: option.id.clone(),
                    name: option.name.clone(),
                },
            )))
        }
        (ProjectFieldKind::Iteration(iterations), ColumnKey::Iteration(iteration_id)) => {
            let iteration = iterations
                .iter()
                .find(|iteration| &iteration.id == iteration_id)?;
            Some(Some((
                ProjectFieldInput::Iteration {
                    iteration_id: iteration.id.clone(),
                },
                ProjectValue::Iteration {
                    iteration_id: iteration.id.clone(),
                    title: iteration.title.clone(),
                    start_date: iteration.start_date.clone(),
                },
            )))
        }
        _ => None,
    }
}

fn show_value(item: &mut ProjectV2Item, field_id: &str, value: Option<ProjectValue>) {
    item.field_values.retain(|value| value.field_id != field_id);
    if let Some(value) = value {
        item.field_values.push(ProjectFieldValue {
            field_id: field_id.to_string(),
            value,
        });
    }
}

impl ProjectsSection {
    pub fn move_message(&self) -> Option<&str> {
        self.move_message.as_ref().map(|message| message.as_ref())
    }

    pub fn write_scope_missing(&self) -> bool {
        self.write_scope_missing
    }

    /// GitHub keeps one order for the whole project; under a sort a column
    /// is drawn in the sort's order, so a place in it means nothing.
    pub fn can_reorder(&self) -> bool {
        self.view()
            .is_none_or(|view| view.sort_by_fields.is_empty())
    }

    /// False, with the guidance shown, when the token is known to read only.
    pub(super) fn may_write(&mut self, cx: &mut Context<Self>) -> bool {
        if self.scopes.includes(PROJECT_WRITE_SCOPE) == Some(false) {
            self.write_scope_missing = true;
            cx.notify();
            return false;
        }
        true
    }

    pub(super) fn column_field(&self) -> Option<&ProjectField> {
        column_field(self.view(), self.details()?)
    }

    /// The board column the item is drawn in now.
    pub(super) fn current_column(&self, item_id: &str) -> Option<ColumnKey> {
        let details = self.details()?;
        board_columns(self.view(), details, self.items())
            .into_iter()
            .find(|column| {
                column.items.iter().any(|index| {
                    self.items
                        .get(*index)
                        .is_some_and(|item| item.id == item_id)
                })
            })
            .map(|column| column.key)
    }

    /// Sets the item's value in the board's column field to `target`'s
    /// (clears it for the no-value column).
    pub fn move_item(&mut self, item_id: &str, target: &ColumnKey, cx: &mut Context<Self>) {
        let Some(field) = self.column_field().cloned() else {
            return;
        };
        let Some(write) = value_for(&field, target) else {
            return;
        };
        if self.current_column(item_id).as_ref() == Some(target) || !self.may_write(cx) {
            return;
        }
        self.send_field_write(item_id, &field.id, write, FieldWriteOrigin::Move, cx);
    }

    /// Shows `write` at once and sends it; returns its id. The caller has
    /// checked the scope and that the value changes.
    pub(super) fn send_field_write(
        &mut self,
        item_id: &str,
        field_id: &str,
        write: Option<(ProjectFieldInput, ProjectValue)>,
        origin: FieldWriteOrigin,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        let project_id = self.selected_project.clone()?;
        let item = self.items.iter_mut().find(|item| item.id == item_id)?;
        let (input, shown) = write.unzip();
        let shown_before = value_of(item, field_id).cloned();
        show_value(item, field_id, shown.clone());
        self.move_message = None;
        let key = FieldWriteKey {
            item_id: item_id.to_string(),
            field_id: field_id.to_string(),
        };
        self.next_write += 1;
        let write_id = self.next_write;
        let pending = self
            .field_writes
            .entry(key.clone())
            .or_insert(PendingFieldWrites {
                latest: write_id,
                in_flight: 0,
                confirmed: shown_before,
            });
        pending.latest = write_id;
        pending.in_flight += 1;
        // Asked for now, so its place in the write queue is taken now.
        let write = self.context.backend.set_project_item_field(
            project_id,
            key.item_id.clone(),
            key.field_id.clone(),
            input,
        );
        cx.spawn(async move |this, cx| {
            let result = write.await;
            this.update(cx, |this, cx| {
                this.finish_field_write(key, write_id, shown, origin, result, cx)
            })
            .ok();
        })
        .detach();
        cx.notify();
        Some(write_id)
    }

    fn finish_field_write(
        &mut self,
        key: FieldWriteKey,
        write_id: u64,
        written: Option<ProjectValue>,
        origin: FieldWriteOrigin,
        result: GithubResult<()>,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.field_writes.get_mut(&key) else {
            return;
        };
        pending.in_flight = pending.in_flight.saturating_sub(1);
        let is_latest = pending.latest == write_id;
        let shown = match result {
            Ok(()) => {
                pending.confirmed = written.clone();
                self.field_edit_saved(&key, write_id);
                written
            }
            Err(error) => {
                report_if_revoked(&error, &self.context.session, cx);
                let back_to = pending.confirmed.clone();
                if is_latest {
                    match origin {
                        FieldWriteOrigin::Move => {
                            self.report_failed_move(&key.item_id, back_to.as_ref(), error)
                        }
                        FieldWriteOrigin::Edit | FieldWriteOrigin::Choice => {
                            self.report_failed_field_write(&key, write_id, error)
                        }
                    }
                }
                back_to
            }
        };
        if self
            .field_writes
            .get(&key)
            .is_some_and(|pending| pending.in_flight == 0)
        {
            self.field_writes.remove(&key);
        }
        if is_latest && let Some(item) = self.items.iter_mut().find(|item| item.id == key.item_id) {
            show_value(item, &key.field_id, shown);
        }
        cx.notify();
    }

    fn report_failed_move(
        &mut self,
        item_id: &str,
        back_to: Option<&ProjectValue>,
        error: GithubError,
    ) {
        self.note_write_error(&error);
        let title = self.title_of(item_id);
        let column = self.column_name(&column_key_of(back_to));
        self.move_message = Some(SharedString::from(format!(
            "Could not move “{title}”: {error}. It is back in {column}."
        )));
    }

    pub(super) fn title_of(&self, item_id: &str) -> String {
        self.items
            .iter()
            .find(|item| item.id == item_id)
            .map_or("the card", item_title)
            .to_string()
    }

    pub(super) fn note_write_error(&mut self, error: &GithubError) {
        if let GithubError::MissingScope { scope } = error
            && scope.contains("project")
        {
            self.write_scope_missing = true;
        }
    }

    pub(super) fn column_name(&self, key: &ColumnKey) -> String {
        self.details()
            .map(|details| board_columns(self.view(), details, &[]))
            .into_iter()
            .flatten()
            .find(|column| &column.key == key)
            .map_or_else(|| "its column".to_string(), |column| column.name)
    }
}
