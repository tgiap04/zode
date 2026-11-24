//! Changing an item's value in a project field: typed into a text, number or
//! date cell, or picked from a single select's options or an iteration
//! field's iterations. Assignees, labels, milestone, title and repository
//! are the issue's own and are changed on the issue, not in the project.

use editor::Editor;
use github::{
    GithubError, ProjectField, ProjectFieldInput, ProjectFieldKind, ProjectItemContent,
    ProjectValue,
};
use gpui::{
    AppContext as _, Context, Entity, Focusable as _, Pixels, Point, SharedString, Subscription,
    Window,
};
use time::{Date, Month};
use ui::ContextMenu;

use crate::projects::move_item::{FieldWriteKey, FieldWriteOrigin, column_key_of, value_for};
use crate::projects::project_layout::{ColumnKey, cell_text, value_of};
use crate::projects::projects_section::ProjectsSection;

/// A text, number or date cell being typed into. It stays open while its
/// value is being saved, so a failure leaves the typed text to retry.
pub struct FieldEdit {
    pub item_id: String,
    pub field_id: String,
    pub editor: Entity<Editor>,
    pub error: Option<SharedString>,
    /// The write saving it, while GitHub has not answered.
    pub saving: Option<u64>,
}

/// The options or iterations offered for a table cell.
pub struct FieldMenu {
    pub menu: Entity<ContextMenu>,
    pub position: Option<Point<Pixels>>,
    pub _dismissed: Subscription,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldEditing {
    Typed,
    Choice,
    /// Its value comes from the issue or pull request itself.
    OnTheIssue,
    /// Kinds this panel does not change (linked pull requests and the like).
    NotEditable,
}

pub fn field_editing(field: &ProjectField) -> FieldEditing {
    match field.kind {
        ProjectFieldKind::Text | ProjectFieldKind::Number | ProjectFieldKind::Date => {
            FieldEditing::Typed
        }
        ProjectFieldKind::SingleSelect(_) | ProjectFieldKind::Iteration(_) => FieldEditing::Choice,
        ProjectFieldKind::Title
        | ProjectFieldKind::Assignees
        | ProjectFieldKind::Labels
        | ProjectFieldKind::Milestone
        | ProjectFieldKind::Repository => FieldEditing::OnTheIssue,
        ProjectFieldKind::LinkedPullRequests
        | ProjectFieldKind::SubIssuesProgress
        | ProjectFieldKind::Other => FieldEditing::NotEditable,
    }
}

/// What typing `text` into the field writes and then shows; `None` (an
/// empty cell) clears the value. Refused before anything is sent when it is
/// not a number or a real `YYYY-MM-DD` date.
pub fn parse_typed_value(
    field: &ProjectField,
    text: &str,
) -> Result<Option<(ProjectFieldInput, ProjectValue)>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    match field.kind {
        ProjectFieldKind::Text => Ok(Some((
            ProjectFieldInput::Text {
                text: text.to_string(),
            },
            ProjectValue::Text(text.to_string()),
        ))),
        ProjectFieldKind::Number => match text.parse::<f64>() {
            Ok(number) if number.is_finite() => Ok(Some((
                ProjectFieldInput::Number { number },
                ProjectValue::Number(number),
            ))),
            _ => Err(format!("“{text}” is not a number.")),
        },
        ProjectFieldKind::Date if is_calendar_date(text) => Ok(Some((
            ProjectFieldInput::Date {
                date: text.to_string(),
            },
            ProjectValue::Date(text.to_string()),
        ))),
        ProjectFieldKind::Date => Err(format!("“{text}” is not a date; write it as YYYY-MM-DD.")),
        _ => Err(format!("{} is not typed into.", field.name)),
    }
}

fn is_calendar_date(text: &str) -> bool {
    let parts = text.split('-').collect::<Vec<_>>();
    let [year, month, day] = parts.as_slice() else {
        return false;
    };
    let digits = |part: &str, length: usize| {
        part.len() == length && part.bytes().all(|byte| byte.is_ascii_digit())
    };
    if !(digits(year, 4) && digits(month, 2) && digits(day, 2)) {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (year.parse::<i32>(), month.parse::<u8>(), day.parse())
    else {
        return false;
    };
    Month::try_from(month)
        .and_then(|month| Date::from_calendar_date(year, month, day))
        .is_ok()
}

impl ProjectsSection {
    pub fn field_edit(&self) -> Option<&FieldEdit> {
        self.field_edit.as_ref()
    }

    pub fn field_message(&self) -> Option<&str> {
        self.field_message.as_ref().map(|message| message.as_ref())
    }

    pub(super) fn editable_field(&mut self, item_id: &str, field_id: &str) -> Option<ProjectField> {
        let field = self.details()?.field(field_id)?.clone();
        let item = self.items.iter().find(|item| item.id == item_id)?;
        if item.content == ProjectItemContent::Redacted {
            return None;
        }
        self.field_message = match field_editing(&field) {
            FieldEditing::Typed | FieldEditing::Choice => None,
            FieldEditing::OnTheIssue => Some(SharedString::from(format!(
                "{} belongs to the issue or pull request itself, not the project; edit it on the issue.",
                field.name
            ))),
            FieldEditing::NotEditable => Some(SharedString::from(format!(
                "{} cannot be changed here.",
                field.name
            ))),
        };
        self.field_message.is_none().then_some(field)
    }

    pub fn start_field_edit(
        &mut self,
        item_id: &str,
        field_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit_field_at(item_id, field_id, None, window, cx);
    }

    /// A typed field opens its editor; a choice opens its menu at
    /// `position` (a table cell's click).
    pub(super) fn edit_field_at(
        &mut self,
        item_id: &str,
        field_id: &str,
        position: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let field = self.editable_field(item_id, field_id);
        cx.notify();
        let Some(field) = field else {
            return;
        };
        if !self.may_write(cx) {
            return;
        }
        self.field_menu = None;
        if field_editing(&field) == FieldEditing::Choice {
            self.deploy_field_menu(item_id, &field, position, window, cx);
            return;
        }
        let Some(item) = self.items.iter().find(|item| item.id == item_id) else {
            return;
        };
        let text = cell_text(&field, item);
        let editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text(&field.name, window, cx);
            editor.set_text(text, window, cx);
            editor
        });
        window.focus(&editor.focus_handle(cx), cx);
        self.field_edit = Some(FieldEdit {
            item_id: item_id.to_string(),
            field_id: field.id,
            editor,
            error: None,
            saving: None,
        });
    }

    /// Checks what was typed and sends it; a value GitHub already holds
    /// closes the editor without a write.
    pub fn save_field_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = &self.field_edit else {
            return;
        };
        if edit.saving.is_some() {
            return;
        }
        let (item_id, field_id) = (edit.item_id.clone(), edit.field_id.clone());
        let text = edit.editor.read(cx).text(cx);
        let field = self
            .details()
            .and_then(|details| details.field(&field_id))
            .cloned();
        let item_present = self.items.iter().any(|item| item.id == item_id);
        let (Some(field), true) = (field, item_present) else {
            if let Some(edit) = &mut self.field_edit {
                edit.error = Some(
                    "This item or field is no longer in the project, so nothing was saved; \
                     press Escape to close."
                        .into(),
                );
            }
            cx.notify();
            return;
        };
        let write = match parse_typed_value(&field, &text) {
            Ok(write) => write,
            Err(error) => {
                if let Some(edit) = &mut self.field_edit {
                    edit.error = Some(error.into());
                }
                cx.notify();
                return;
            }
        };
        if !self.may_write(cx) {
            return;
        }
        let current = self
            .items
            .iter()
            .find(|item| item.id == item_id)
            .and_then(|item| value_of(item, &field_id));
        if current == write.as_ref().map(|(_, shown)| shown) {
            self.field_edit = None;
            cx.notify();
            return;
        }
        let write_id =
            self.send_field_write(&item_id, &field_id, write, FieldWriteOrigin::Edit, cx);
        if let Some(edit) = &mut self.field_edit {
            edit.saving = write_id;
            edit.error = None;
        }
        cx.notify();
    }

    /// Closes the editor; nothing is sent.
    pub fn cancel_field_edit(&mut self, cx: &mut Context<Self>) {
        self.field_edit = None;
        cx.notify();
    }

    /// Sets a single select to an option, or an iteration field to one of
    /// its iterations; `None` clears it.
    pub fn set_field_choice(
        &mut self,
        item_id: &str,
        field_id: &str,
        choice: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.field_menu = None;
        let field = self.editable_field(item_id, field_id);
        cx.notify();
        let Some(field) = field.filter(|field| field_editing(field) == FieldEditing::Choice) else {
            return;
        };
        let target = match (&field.kind, choice) {
            (ProjectFieldKind::SingleSelect(_), Some(option_id)) => ColumnKey::Option(option_id),
            (ProjectFieldKind::Iteration(_), Some(iteration_id)) => {
                ColumnKey::Iteration(iteration_id)
            }
            _ => ColumnKey::NoValue,
        };
        let Some(write) = value_for(&field, &target) else {
            return;
        };
        let current = self
            .items
            .iter()
            .find(|item| item.id == item_id)
            .map(|item| column_key_of(value_of(item, field_id)));
        if current.as_ref() == Some(&target) || !self.may_write(cx) {
            return;
        }
        self.send_field_write(item_id, field_id, write, FieldWriteOrigin::Choice, cx);
    }

    /// A save that went through closes its editor.
    pub(super) fn field_edit_saved(&mut self, key: &FieldWriteKey, write_id: u64) {
        if self.field_edit.as_ref().is_some_and(|edit| {
            edit.saving == Some(write_id)
                && edit.item_id == key.item_id
                && edit.field_id == key.field_id
        }) {
            self.field_edit = None;
        }
    }

    /// The editor that sent it keeps its text and shows why; otherwise the
    /// failure is told above the board.
    pub(super) fn report_failed_field_write(
        &mut self,
        key: &FieldWriteKey,
        write_id: u64,
        error: GithubError,
    ) {
        self.note_write_error(&error);
        if let Some(edit) = self
            .field_edit
            .as_mut()
            .filter(|edit| edit.item_id == key.item_id && edit.field_id == key.field_id)
        {
            if edit.saving == Some(write_id) {
                edit.saving = None;
            }
            edit.error = Some(SharedString::from(format!("Not saved: {error}.")));
            return;
        }
        let field = self
            .details()
            .and_then(|details| details.field(&key.field_id))
            .map_or_else(|| "the field".to_string(), |field| field.name.clone());
        let title = self.title_of(&key.item_id);
        self.move_message = Some(SharedString::from(format!(
            "Could not change {field} of “{title}”: {error}. It shows GitHub's value again."
        )));
    }
}
