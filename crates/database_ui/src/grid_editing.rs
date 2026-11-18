//! Editing the cells of a table opened from the tree: what may be edited, the
//! edits waiting to be saved, and the box a cell turns into while it is typed
//! into.
//!
//! Edits live on the page they were made on (`Page::edits`), so reading another
//! page starts clean, and `guard_unsaved` is what stands between an edit and the
//! reads that would replace it. Saving is `grid_edit_save`; the bar that offers
//! it is `grid_edit_bar`.

use crate::cell_validation::{editability, tidy, validate};
use crate::database_panel::{DatabasePanel, NodeState};
use crate::query::{Page, QueryError, QueryState, cell_text};
use crate::safe_mode::SafetyLevel;
use crate::session::Session;
use crate::sql_dialect::Dialect;
use database::protocol::{Cell, ColumnDef};
use gpui::{
    App, Context, Entity, Focusable as _, Modifiers, PromptLevel, SharedString, Subscription,
    Window,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use ui::prelude::*;
use ui_input::{ErasedEditorEvent, InputField};

/// A cell of the page on screen, by place. Stable for as long as the page is:
/// a page is never edited in place, only replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CellAddress {
    pub(crate) row: usize,
    pub(crate) column: usize,
}

/// What a cell is to be set to, and what is wrong with that.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StagedCell {
    /// `None` is NULL.
    pub(crate) value: Option<String>,
    /// A sentence about what the column expects, from `cell_validation` or from
    /// the database's refusal; shown on the cell.
    pub(crate) error: Option<String>,
    /// The error is the database's word on this value, not this crate's check
    /// of it: asking again would only get the same answer, so it stands until
    /// the cell is edited.
    pub(crate) refused: bool,
}

pub(crate) struct CellEditor {
    pub(crate) address: CellAddress,
    pub(crate) input: Entity<InputField>,
    /// The cell started out NULL, so that nothing typed reads as NULL and not
    /// as the empty string.
    started_null: bool,
    /// What the box held once it was filled, which is the cell's text as the
    /// box reads it -- every line break a `\n` -- and not as the cell has it.
    /// A box that still holds this was not typed in.
    opened_with: String,
    /// Dropped with the editor: the blur that follows a cancel must not stage
    /// what the user just threw away.
    _blurred: Subscription,
}

#[derive(Default)]
pub(crate) struct PageEdits {
    /// In reading order, which is also the order they are saved in.
    pub(crate) staged: BTreeMap<CellAddress, StagedCell>,
    pub(crate) editor: Option<CellEditor>,
    /// Rows the database said changed or went away since they were read.
    pub(crate) row_errors: BTreeMap<usize, SharedString>,
    /// Why the last save did not happen, said in the bar.
    pub(crate) failure: Option<SharedString>,
    /// Why a cell would not open, or what the last save did.
    pub(crate) notice: Option<SharedString>,
    /// The save that is out, by the number it was sent under: an answer is for
    /// the save it answers, whichever page is saving by the time it arrives.
    pub(crate) saving: Option<u64>,
    /// A save went through that was not read back, so the grid still shows the
    /// values from before it and cannot be edited against them.
    pub(crate) reload_needed: bool,
}

impl PageEdits {
    pub(crate) fn new(notice: Option<SharedString>) -> Self {
        Self {
            notice,
            ..Self::default()
        }
    }

    /// After a save that went through and could not be read back.
    pub(crate) fn saved_unread(notice: SharedString) -> Self {
        Self {
            reload_needed: true,
            ..Self::new(Some(notice))
        }
    }

    pub(crate) fn invalid_count(&self) -> usize {
        self.staged
            .values()
            .filter(|staged| staged.error.is_some())
            .count()
    }
}

/// What decides whether a cell may be edited that is not on the page: the
/// connection it came from.
pub(crate) struct EditEnvironment {
    pub(crate) dialect: Dialect,
    pub(crate) level: SafetyLevel,
    pub(crate) row_updates: bool,
}

/// What typing nothing into a cell that was NULL means: that it stays NULL.
/// The one value this cannot say is the empty string for a NULL cell; "Set to
/// NULL" is the explicit way the other direction.
fn is_unchanged(cell: &Cell, value: &Option<String>) -> bool {
    match (cell, value) {
        (Cell::Null, None) => true,
        (Cell::Null, Some(_)) | (_, None) => false,
        (cell, Some(text)) => as_the_box_holds_it(&cell_text(cell)) == *text,
    }
}

/// Text the way the box keeps it: a buffer turns `\r\n` and a lone `\r` into
/// `\n`, so a cell holding either would otherwise differ from itself.
fn as_the_box_holds_it(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

impl Page {
    fn column_def(&self, column: usize) -> Option<&ColumnDef> {
        let name = &self.result.columns.get(column)?.name;
        self.browse
            .as_ref()?
            .columns
            .iter()
            .find(|definition| definition.name == *name)
    }

    pub(crate) fn column_name(&self, column: usize) -> &str {
        self.result
            .columns
            .get(column)
            .map_or("", |column| column.name.as_str())
    }

    fn cell(&self, address: CellAddress) -> Option<&Cell> {
        self.result.rows.get(address.row)?.get(address.column)
    }

    /// Why this cell cannot be edited, in words for the pager. `None` when it
    /// can.
    pub(crate) fn why_not_editable(
        &self,
        environment: &EditEnvironment,
        address: CellAddress,
    ) -> Option<String> {
        if environment.level == SafetyLevel::ReadOnly {
            return Some("This connection is Read Only, so values cannot be edited".into());
        }
        if self.browse.is_none() {
            return Some("Only a table opened with Open Data can be edited".into());
        }
        if !environment.row_updates {
            return Some("This driver cannot update rows".into());
        }
        if self.edits.saving.is_some() {
            return Some("A save is in progress".into());
        }
        if self.edits.reload_needed {
            return Some("Reload the page to see what was saved before editing it again".into());
        }
        if let Err(reason) = self.row_identity(address.row) {
            return Some(reason.message());
        }
        let (Some(definition), Some(cell)) = (self.column_def(address.column), self.cell(address))
        else {
            return Some("The column types of this table are not known".into());
        };
        editability(&environment.dialect, definition, cell)
            .err()
            .map(|reason| format!("{}: {}", definition.name, reason.reason()))
    }

    /// Records that `address` is to hold `value`, checked against its column.
    ///
    /// A value equal to what the cell holds is no edit at all, and is never
    /// validated: a cell the user did not change must not be able to block a
    /// save over a value the database already took.
    pub(crate) fn stage_cell(
        &mut self,
        dialect: &Dialect,
        address: CellAddress,
        value: Option<String>,
    ) {
        let Some(cell) = self.cell(address) else {
            return;
        };
        let value = match (self.column_def(address.column), value) {
            (Some(definition), Some(text)) => Some(tidy(dialect, definition, cell, text)),
            (_, value) => value,
        };
        let unchanged = is_unchanged(cell, &value);
        let error = if unchanged {
            None
        } else {
            match self.column_def(address.column) {
                Some(definition) => validate(dialect, definition, cell, value.as_deref()).err(),
                None => Some("column type unknown".into()),
            }
        };
        self.edits.failure = None;
        if unchanged {
            self.revert_cell(address);
            return;
        }
        self.edits.staged.insert(
            address,
            StagedCell {
                value,
                error,
                refused: false,
            },
        );
    }

    /// Records what was typed when there is no connection to judge it by: kept
    /// rather than thrown away, and marked as unchecked, which stands until
    /// `revalidate` has a connection to check it against.
    pub(crate) fn stage_unchecked(&mut self, address: CellAddress, value: Option<String>) {
        let Some(cell) = self.cell(address) else {
            return;
        };
        if is_unchanged(cell, &value) {
            self.revert_cell(address);
            return;
        }
        self.edits.failure = None;
        self.edits.staged.insert(
            address,
            StagedCell {
                value,
                error: Some("Not connected — reconnect to check this value".into()),
                refused: false,
            },
        );
    }

    pub(crate) fn revert_cell(&mut self, address: CellAddress) {
        self.edits.staged.remove(&address);
        if !self
            .edits
            .staged
            .keys()
            .any(|staged| staged.row == address.row)
        {
            self.edits.row_errors.remove(&address.row);
        }
    }

    /// Checks every staged value again, replacing what an earlier check said.
    /// A refusal from the database is left alone: it knows what this crate does
    /// not, and the same value would only be refused again. How many are still
    /// wrong.
    pub(crate) fn revalidate(&mut self, dialect: &Dialect) -> usize {
        let addresses: Vec<CellAddress> = self
            .edits
            .staged
            .iter()
            .filter(|(_, staged)| !staged.refused)
            .map(|(address, _)| *address)
            .collect();
        for address in addresses {
            let Some(value) = self
                .edits
                .staged
                .get(&address)
                .map(|staged| staged.value.clone())
            else {
                continue;
            };
            let error = match (self.column_def(address.column), self.cell(address)) {
                (Some(definition), Some(cell)) => {
                    validate(dialect, definition, cell, value.as_deref()).err()
                }
                _ => Some("column type unknown".into()),
            };
            if let Some(staged) = self.edits.staged.get_mut(&address) {
                staged.error = error;
            }
        }
        self.edits.invalid_count()
    }

    /// The first thing wrong with the edits, named by row and column, for the
    /// bar to say aloud.
    pub(crate) fn first_problem(&self) -> Option<String> {
        if let Some((address, staged)) = self
            .edits
            .staged
            .iter()
            .find(|(_, staged)| staged.error.is_some())
        {
            return Some(format!(
                "row {}, {}: {}",
                self.offset + address.row as u64 + 1,
                self.column_name(address.column),
                staged.error.as_deref().unwrap_or_default()
            ));
        }
        self.edits
            .row_errors
            .iter()
            .next()
            .map(|(row, message)| format!("row {}: {message}", self.offset + *row as u64 + 1))
    }

    /// What the editor starts with: the staged value if there is one, otherwise
    /// the cell's own text. `None` for NULL.
    fn text_to_edit(&self, address: CellAddress) -> Option<String> {
        match self.edits.staged.get(&address) {
            Some(staged) => staged.value.clone(),
            None => match self.cell(address)? {
                Cell::Null => None,
                cell => Some(cell_text(cell).to_string()),
            },
        }
    }
}

/// What a cell's menu offers.
pub(crate) struct CellMenu {
    pub(crate) offered: bool,
    pub(crate) editable: bool,
    pub(crate) nullable: bool,
    pub(crate) staged: bool,
}

/// Where the reason an edit cannot go on is said: in the pager, for a cell that
/// would not open, or in the bar, for a save that did not happen.
#[derive(Clone, Copy)]
pub(crate) enum Report {
    Pager,
    Bar,
}

impl DatabasePanel {
    /// The connection the page on screen was read from, and the session open on
    /// it -- which must be the very session the page came from. A name in the
    /// list is not enough: the same name can come to mean another database
    /// after its url is edited, and an edit made against one must not be
    /// written to the other.
    pub(crate) fn page_session(&self) -> Result<(usize, Arc<Session>), String> {
        let QueryState::Done(page) = &self.query else {
            return Err("There is no page to edit".into());
        };
        let key = page
            .connection
            .as_ref()
            .ok_or_else(|| "This page did not come from a connection".to_string())?;
        let name = &key.0;
        let index = self.index_of_key(key).ok_or_else(|| {
            format!("{name} is no longer in the connection list, so nothing can be saved to it")
        })?;
        let NodeState::Connected(session) = &self.connections[index].state else {
            return Err(format!(
                "Not connected — reconnect {name} and reload the page"
            ));
        };
        if page.origin.session != Some(session.id) {
            return Err(format!(
                "{name}: the connection changed since this page was read — reload the page"
            ));
        }
        Ok((index, session.clone()))
    }

    pub(crate) fn edit_environment(&self, cx: &App) -> Result<EditEnvironment, String> {
        let (index, session) = self.page_session()?;
        Ok(EditEnvironment {
            dialect: self
                .dialect(index)
                .ok_or_else(|| "Not connected — reconnect and reload the page".to_string())?,
            level: self.safety_of(index, cx),
            row_updates: session.capabilities.row_updates,
        })
    }

    /// The environment, or `None` having said why there is none where `report`
    /// puts it: a refusal nobody can read is a click that did nothing.
    pub(crate) fn environment_or_say(
        &mut self,
        report: Report,
        cx: &mut Context<Self>,
    ) -> Option<EditEnvironment> {
        match self.edit_environment(cx) {
            Ok(environment) => Some(environment),
            Err(message) => {
                if let QueryState::Done(page) = &mut self.query {
                    match report {
                        Report::Pager => page.edits.notice = Some(message.into()),
                        Report::Bar => page.edits.failure = Some(message.into()),
                    }
                }
                cx.notify();
                None
            }
        }
    }

    /// Says `error` in the result area where the page was -- unless the page
    /// has edits waiting, which nothing but the user may throw away: then it
    /// is said in the bar over them, and the page stays.
    pub(crate) fn show_failure(&mut self, error: QueryError, cx: &mut Context<Self>) {
        if self.has_unsaved_edits(cx) {
            if let QueryState::Done(page) = &mut self.query {
                page.edits.failure = Some(error.message);
            }
        } else {
            self.query = QueryState::Failed(error);
        }
        cx.notify();
    }

    /// Puts the result area away because the connection `key` is gone from it,
    /// except where it shows another connection's page, or one with edits
    /// waiting that the user has not been asked about.
    pub(crate) fn clear_result_of(&mut self, key: &crate::database_panel::ConnectionKey) {
        let keep = match &self.query {
            QueryState::Done(page) => {
                page.connection.as_ref().is_some_and(|shown| shown != key)
                    || !page.edits.staged.is_empty()
            }
            _ => false,
        };
        if !keep {
            self.query = QueryState::Idle;
        }
    }

    /// Whether the page on screen is the one `read` produced.
    pub(crate) fn is_page_read(&self, read: u64) -> bool {
        matches!(&self.query, QueryState::Done(page) if page.origin.read == read)
    }

    pub(crate) fn cell_menu(&self, row: usize, column: usize, cx: &App) -> CellMenu {
        let QueryState::Done(page) = &self.query else {
            return CellMenu::none();
        };
        let (Ok(environment), true) = (self.edit_environment(cx), page.browse.is_some()) else {
            return CellMenu::none();
        };
        let address = CellAddress { row, column };
        CellMenu {
            offered: true,
            editable: page.why_not_editable(&environment, address).is_none(),
            nullable: page
                .column_def(column)
                .is_some_and(|definition| definition.nullable),
            staged: page.edits.staged.contains_key(&address),
        }
    }

    /// Edits are waiting to be saved, or one is half typed.
    pub(crate) fn has_unsaved_edits(&self, cx: &App) -> bool {
        let QueryState::Done(page) = &self.query else {
            return false;
        };
        !page.edits.staged.is_empty()
            || page
                .edits
                .editor
                .as_ref()
                .is_some_and(|editor| editor.input.read(cx).text(cx) != editor.opened_with)
    }

    fn staged_count(&self) -> usize {
        match &self.query {
            QueryState::Done(page) => page.edits.staged.len(),
            _ => 0,
        }
    }

    /// Whether a save is out, said in the pager when it is: the write is on its
    /// way whatever is thrown away or replaced meanwhile.
    fn refuse_while_saving(&mut self, cx: &mut Context<Self>) -> bool {
        let QueryState::Done(page) = &mut self.query else {
            return false;
        };
        if page.edits.saving.is_none() {
            return false;
        }
        page.edits.notice = Some("A save is in progress — wait for it to finish".into());
        cx.notify();
        true
    }

    /// Throws the edits away, unless a save is out.
    pub(crate) fn drop_edits(&mut self, cx: &mut Context<Self>) -> bool {
        if self.refuse_while_saving(cx) {
            return false;
        }
        if let QueryState::Done(page) = &mut self.query {
            page.edits = Default::default();
        }
        cx.notify();
        true
    }

    /// Runs `proceed` -- a read that would replace the page -- once the edits on
    /// it are no longer in the way: at once when there are none, after a yes to
    /// "Discard?" when there are, never on a no.
    pub(crate) fn guard_unsaved(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        proceed: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        // Neither asked nor answered while a save is out: a yes could not
        // unsend it, and what replaced the page would show values from before.
        if self.refuse_while_saving(cx) {
            return;
        }
        self.commit_editor(cx);
        let count = self.staged_count();
        if count == 0 {
            proceed(self, window, cx);
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Discard {}?", changes_label(count)),
            Some("They have not been saved, and reading another page would lose them."),
            // Cancel first, so that the key that confirms a dialog -- Return --
            // is not the one that throws work away.
            &["Cancel", "Discard"],
            cx,
        );
        let task = cx.spawn_in(window, async move |this, cx| {
            if answer.await.ok() != Some(1) {
                return;
            }
            this.update_in(cx, |this, window, cx| {
                // The question was asked before a save may have gone out.
                if this.drop_edits(cx) {
                    proceed(this, window, cx);
                }
            })
            .ok();
        });
        self.keep_task(task);
    }

    /// Escape on the grid: asks before throwing away what is staged.
    pub(crate) fn confirm_discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_editor(cx);
        if self.staged_count() == 0 {
            return;
        }
        self.guard_unsaved(window, cx, |this, window, cx| {
            window.focus(&this.results_focus, cx);
        });
    }

    /// The Discard button: what was staged is gone, the page is as it was read.
    pub(crate) fn discard_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.drop_edits(cx) {
            window.focus(&self.results_focus, cx);
        }
    }

    /// A second click of a double-click: opens the cell the pointer is in.
    /// `false` when it cannot be opened, which leaves the click to do what it
    /// always did.
    pub(crate) fn edit_under_pointer(
        &mut self,
        row: usize,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if modifiers.shift || modifiers.secondary() {
            return false;
        }
        let QueryState::Done(page) = &self.query else {
            return false;
        };
        let Some((selected, column)) = page.selection.single_cell() else {
            return false;
        };
        selected == row && self.begin_edit(row, column, window, cx)
    }

    /// The selected cell, for the keyboard and the menu entries that act on it.
    pub(crate) fn edit_selected_cell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let QueryState::Done(page) = &self.query else {
            return;
        };
        if let Some((row, column)) = page.selection.single_cell() {
            self.begin_edit(row, column, window, cx);
        }
    }

    pub(crate) fn set_selected_cell_null(&mut self, cx: &mut Context<Self>) {
        let QueryState::Done(page) = &self.query else {
            return;
        };
        if let Some((row, column)) = page.selection.single_cell() {
            self.set_cell_null(row, column, cx);
        }
    }

    /// Turns a cell into a text box holding its value. `false`, with the reason
    /// in the pager, when the cell cannot be edited.
    pub(crate) fn begin_edit(
        &mut self,
        row: usize,
        column: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let address = CellAddress { row, column };
        let Some(environment) = self.environment_or_say(Report::Pager, cx) else {
            return false;
        };
        let refusal = match &self.query {
            QueryState::Done(page) => page.why_not_editable(&environment, address),
            _ => return false,
        };
        // Whatever was being typed elsewhere is kept before the new box opens.
        self.commit_editor(cx);
        let QueryState::Done(page) = &mut self.query else {
            return false;
        };
        if let Some(reason) = refusal {
            page.edits.notice = Some(reason.into());
            cx.notify();
            return false;
        }
        let text = page.text_to_edit(address);
        page.edits.notice = None;

        let started_null = text.is_none();
        let input =
            cx.new(|cx| InputField::new(window, cx, if started_null { "NULL" } else { "" }));
        let editor = input.read(cx).editor().clone();
        editor.set_text(text.as_deref().unwrap_or_default(), window, cx);
        let opened_with = input.read(cx).text(cx);
        editor.move_selection_to_end(window, cx);
        window.focus(&input.focus_handle(cx), cx);

        let panel = cx.weak_entity();
        let blurred = editor.subscribe(
            Box::new(move |event, _window, cx| {
                if event != ErasedEditorEvent::Blurred {
                    return;
                }
                let panel = panel.clone();
                // Deferred: a blur is reported from inside whatever moved the
                // focus, which may be an update of the panel itself.
                cx.defer(move |cx| {
                    panel
                        .update(cx, |panel, cx| panel.commit_editor_at(address, cx))
                        .ok();
                });
            }),
            window,
            cx,
        );
        if let QueryState::Done(page) = &mut self.query {
            page.edits.editor = Some(CellEditor {
                address,
                input,
                started_null,
                opened_with,
                _blurred: blurred,
            });
        }
        cx.notify();
        true
    }

    fn commit_editor_at(&mut self, address: CellAddress, cx: &mut Context<Self>) {
        let is_open = matches!(&self.query, QueryState::Done(page)
            if page.edits.editor.as_ref().is_some_and(|editor| editor.address == address));
        if is_open {
            self.commit_editor(cx);
        }
    }

    /// Closes the box and stages what was typed in it, checked against the
    /// column.
    pub(crate) fn commit_editor(&mut self, cx: &mut Context<Self>) {
        let dialect = self.result_dialect();
        let QueryState::Done(page) = &mut self.query else {
            return;
        };
        let Some(editor) = page.edits.editor.take() else {
            return;
        };
        cx.notify();
        let text = editor.input.read(cx).text(cx);
        // A box nobody typed in stages nothing: what it holds is the cell as
        // the box reads it, which need not be how the cell is written.
        if text == editor.opened_with {
            return;
        }
        let value = if editor.started_null && text.is_empty() {
            None
        } else {
            Some(text)
        };
        match dialect {
            Some(dialect) => page.stage_cell(&dialect, editor.address, value),
            None => page.stage_unchecked(editor.address, value),
        }
    }

    /// Enter: keeps what was typed and gives the keys back to the grid.
    pub(crate) fn confirm_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_editor(cx);
        window.focus(&self.results_focus, cx);
    }

    /// Escape: closes the box and keeps nothing of what was typed in it.
    pub(crate) fn cancel_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let QueryState::Done(page) = &mut self.query {
            page.edits.editor = None;
        }
        window.focus(&self.results_focus, cx);
        cx.notify();
    }

    /// Tab: keeps what was typed and opens the next cell that can be edited.
    pub(crate) fn edit_neighbour(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let from = match &self.query {
            QueryState::Done(page) => page.edits.editor.as_ref().map(|editor| editor.address),
            _ => None,
        };
        let Some(from) = from else {
            return;
        };
        self.commit_editor(cx);
        let next = self
            .edit_environment(cx)
            .ok()
            .zip(match &self.query {
                QueryState::Done(page) => Some(page),
                _ => None,
            })
            .and_then(|(environment, page)| next_editable(page, &environment, from, forward));
        match next {
            Some(next) => {
                self.begin_edit(next.row, next.column, window, cx);
            }
            None => window.focus(&self.results_focus, cx),
        }
    }

    pub(crate) fn set_cell_null(&mut self, row: usize, column: usize, cx: &mut Context<Self>) {
        let address = CellAddress { row, column };
        let Some(environment) = self.environment_or_say(Report::Pager, cx) else {
            return;
        };
        self.commit_editor(cx);
        let QueryState::Done(page) = &mut self.query else {
            return;
        };
        if let Some(reason) = page.why_not_editable(&environment, address) {
            page.edits.notice = Some(reason.into());
        } else {
            page.stage_cell(&environment.dialect, address, None);
        }
        cx.notify();
    }

    pub(crate) fn revert_cell_edit(&mut self, row: usize, column: usize, cx: &mut Context<Self>) {
        if let QueryState::Done(page) = &mut self.query {
            page.revert_cell(CellAddress { row, column });
            page.edits.failure = None;
        }
        cx.notify();
    }
}

impl CellMenu {
    fn none() -> Self {
        Self {
            offered: false,
            editable: false,
            nullable: false,
            staged: false,
        }
    }
}

fn next_editable(
    page: &Page,
    environment: &EditEnvironment,
    from: CellAddress,
    forward: bool,
) -> Option<CellAddress> {
    let columns = page.result.columns.len();
    let total = page.result.rows.len() * columns;
    if columns == 0 {
        return None;
    }
    let start = from.row * columns + from.column;
    let at = |position: usize| CellAddress {
        row: position / columns,
        column: position % columns,
    };
    let editable = |position: &usize| page.why_not_editable(environment, at(*position)).is_none();
    if forward {
        (start + 1..total).find(editable).map(at)
    } else {
        (0..start).rev().find(editable).map(at)
    }
}

pub(crate) fn changes_label(count: usize) -> String {
    match count {
        1 => "1 unsaved change".into(),
        count => format!("{count} unsaved changes"),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::database_panel::NodeOrigin;
    use crate::query::TableBrowse;
    use crate::safe_mode::StatementKind;
    use database::protocol::{ResultColumn, ResultSet};

    pub(crate) fn definition(name: &str, type_name: &str, nullable: bool, key: bool) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            type_name: type_name.into(),
            nullable,
            primary_key: key,
        }
    }

    fn text(value: &str) -> Cell {
        Cell::Text {
            value: value.into(),
        }
    }

    fn number(value: &str) -> Cell {
        Cell::Number {
            value: value.into(),
        }
    }

    /// `id` (the key), `name` (text, never NULL), `score` (integer, nullable)
    /// and `photo` (a blob), over three rows, starting at row 40 of the table.
    pub(crate) fn page() -> Page {
        let columns = vec![
            definition("id", "integer", false, true),
            definition("name", "text", false, false),
            definition("score", "integer", true, false),
            definition("photo", "blob", true, false),
        ];
        let result_columns = columns
            .iter()
            .map(|column| ResultColumn {
                name: column.name.clone(),
                type_name: column.type_name.clone(),
            })
            .collect();
        let row = |id: &str, name: &str, score: Cell| {
            vec![number(id), text(name), score, Cell::Binary { byte_len: 4 }]
        };
        Page {
            result: ResultSet {
                columns: result_columns,
                rows: vec![
                    row("1", "ada", number("10")),
                    row("2", "grace", Cell::Null),
                    row("3", "linus", number("30")),
                ],
                truncated: false,
                elapsed_ms: 0,
            },
            sql: String::new(),
            offset: 40,
            limit: 200,
            selected_row: None,
            selection: Default::default(),
            browse: Some(
                TableBrowse::new(("db".into(), NodeOrigin::Personal), "public", "people")
                    .with_columns(&columns),
            ),
            connection: Some(("db".into(), NodeOrigin::Personal)),
            kind: StatementKind::Read,
            origin: Default::default(),
            edits: Default::default(),
        }
    }

    pub(crate) fn dialect(driver: &str) -> Dialect {
        Dialect::detect(driver, &Default::default())
    }

    fn environment() -> EditEnvironment {
        EditEnvironment {
            dialect: dialect("postgres"),
            level: SafetyLevel::ConfirmWrites,
            row_updates: true,
        }
    }

    fn at(row: usize, column: usize) -> CellAddress {
        CellAddress { row, column }
    }

    fn why(page: &Page, row: usize, column: usize) -> Option<String> {
        page.why_not_editable(&environment(), at(row, column))
    }

    #[test]
    fn an_ordinary_cell_of_a_keyed_table_can_be_edited() {
        let page = page();
        assert_eq!(why(&page, 0, 1), None);
        assert_eq!(why(&page, 1, 2), None, "a NULL cell too");
    }

    #[test]
    fn the_key_and_a_blob_cannot_be_edited_and_the_reason_names_the_column() {
        let page = page();
        assert_eq!(why(&page, 0, 0), Some("id: part of the primary key".into()));
        assert_eq!(
            why(&page, 0, 3),
            Some("photo: binary values cannot be edited here".into())
        );
    }

    #[test]
    fn what_the_connection_forbids_is_said_before_anything_else() {
        let page = page();
        let read_only = EditEnvironment {
            level: SafetyLevel::ReadOnly,
            ..environment()
        };
        assert!(
            page.why_not_editable(&read_only, at(0, 1))
                .is_some_and(|reason| reason.contains("Read Only"))
        );
        let old_driver = EditEnvironment {
            row_updates: false,
            ..environment()
        };
        assert!(
            page.why_not_editable(&old_driver, at(0, 1))
                .is_some_and(|reason| reason.contains("cannot update rows"))
        );
    }

    #[test]
    fn a_typed_statement_a_missing_key_and_unknown_columns_each_refuse() {
        let mut typed = page();
        typed.browse = None;
        assert!(why(&typed, 0, 1).is_some_and(|reason| reason.contains("Open Data")));

        let mut keyless = page();
        if let Some(browse) = keyless.browse.as_mut() {
            browse.key_columns.clear();
        }
        assert_eq!(
            why(&keyless, 0, 1),
            Some("read-only: no primary key".into())
        );

        let mut undescribed = page();
        if let Some(browse) = undescribed.browse.as_mut() {
            browse.columns.clear();
        }
        assert!(why(&undescribed, 0, 1).is_some_and(|reason| reason.contains("column types")));
    }

    #[test]
    fn a_page_being_saved_refuses_new_edits() {
        let mut page = page();
        page.edits.saving = Some(1);
        assert!(why(&page, 0, 1).is_some_and(|reason| reason.contains("in progress")));
    }

    #[test]
    fn a_page_whose_save_was_not_read_back_cannot_be_edited_until_it_is_reloaded() {
        let mut page = page();
        page.edits = PageEdits::saved_unread("Saved 1 change in 1 row".into());
        assert!(why(&page, 0, 1).is_some_and(|reason| reason.contains("Reload the page")));
        assert_eq!(
            page.edits.notice.as_ref().map(ToString::to_string),
            Some("Saved 1 change in 1 row".to_string()),
            "the note is what the pager says"
        );
    }

    #[test]
    fn a_null_cell_left_empty_is_unchanged_and_every_other_cell_compares_by_its_text() {
        assert!(is_unchanged(&Cell::Null, &None));
        assert!(!is_unchanged(&Cell::Null, &Some(String::new())));
        assert!(!is_unchanged(&text("a"), &None));
        assert!(is_unchanged(&text("a"), &Some("a".into())));
        assert!(!is_unchanged(&text("a"), &Some("A".into())));
        assert!(is_unchanged(&number("10"), &Some("10".into())));
        assert!(is_unchanged(
            &Cell::Bool { value: true },
            &Some("true".into())
        ));
    }

    /// The box normalises every line break to `\n`, so a cell is compared with
    /// what the box would hold for it.
    #[test]
    fn a_cell_is_unchanged_when_the_box_holding_it_would_read_the_same() {
        assert!(is_unchanged(&text("a\r\nb"), &Some("a\nb".into())));
        assert!(is_unchanged(&text("a\rb"), &Some("a\nb".into())));
        assert!(is_unchanged(&text("a\r\nb\r\n"), &Some("a\nb\n".into())));
        assert!(!is_unchanged(&text("a\r\nb"), &Some("a\nc".into())));
        assert!(!is_unchanged(&text("a\r\nb"), &Some("a b".into())));
    }

    #[test]
    fn a_cell_with_windows_line_breaks_put_back_as_the_box_reads_it_is_no_edit() {
        let mut page = page();
        page.result.rows[0][1] = text("a\r\nb");
        page.stage_cell(&dialect("postgres"), at(0, 1), Some("a\nb".into()));
        assert!(page.edits.staged.is_empty());
    }

    /// Whitespace around a number, a date or an id is read past by the check
    /// and so must not be sent: the server may not read past it.
    #[test]
    fn what_is_checked_after_trimming_is_staged_and_sent_trimmed() {
        let mut page = page();
        let dialect = dialect("postgres");
        page.stage_cell(&dialect, at(0, 2), Some("  31 ".into()));
        assert_eq!(page.edits.staged[&at(0, 2)].value.as_deref(), Some("31"));
        assert_eq!(page.edits.staged[&at(0, 2)].error, None);

        // Text is what was typed, spaces included.
        page.stage_cell(&dialect, at(0, 1), Some("  ada l. ".into()));
        assert_eq!(
            page.edits.staged[&at(0, 1)].value.as_deref(),
            Some("  ada l. ")
        );

        // Trimmed back to what the cell holds, it is no edit at all.
        page.stage_cell(&dialect, at(0, 2), Some(" 10 ".into()));
        assert!(!page.edits.staged.contains_key(&at(0, 2)));
    }

    #[test]
    fn a_changed_value_is_staged_and_a_value_put_back_is_not() {
        let mut page = page();
        let dialect = dialect("postgres");
        page.stage_cell(&dialect, at(0, 1), Some("ada l.".into()));
        assert_eq!(
            page.edits.staged.get(&at(0, 1)),
            Some(&StagedCell {
                value: Some("ada l.".into()),
                error: None,
                refused: false,
            })
        );
        page.stage_cell(&dialect, at(0, 1), Some("ada".into()));
        assert!(page.edits.staged.is_empty());
    }

    /// The same mistake, a number typed where text goes and text where a
    /// number goes, reads differently for each column.
    #[test]
    fn each_column_judges_the_value_by_its_own_type() {
        let mut page = page();
        let dialect = dialect("postgres");
        page.stage_cell(&dialect, at(0, 2), Some("many".into()));
        page.stage_cell(&dialect, at(0, 1), None);
        let score = page.edits.staged[&at(0, 2)].error.clone();
        let name = page.edits.staged[&at(0, 1)].error.clone();
        assert!(score.is_some(), "text in an integer column");
        assert_eq!(name.as_deref(), Some("cannot be NULL"));
        assert_ne!(score, name);

        page.stage_cell(&dialect, at(2, 2), None);
        assert_eq!(
            page.edits.staged[&at(2, 2)].error,
            None,
            "score may be NULL"
        );
        assert_eq!(page.edits.invalid_count(), 2);
    }

    /// A value the database took before cannot block a save by being wrong
    /// for its column now.
    #[test]
    fn a_cell_that_was_not_changed_is_never_held_to_its_type() {
        let mut page = page();
        page.result.rows[0][2] = text("not a number");
        page.stage_cell(&dialect("postgres"), at(0, 2), Some("not a number".into()));
        assert!(page.edits.staged.is_empty());
        assert_eq!(page.revalidate(&dialect("postgres")), 0);
    }

    #[test]
    fn revalidating_replaces_what_an_earlier_check_said_with_what_the_check_says() {
        let mut page = page();
        let dialect = dialect("postgres");
        page.stage_cell(&dialect, at(0, 2), Some("11".into()));
        page.edits.staged.get_mut(&at(0, 2)).unwrap().error = Some("the engine said so".into());
        assert_eq!(page.revalidate(&dialect), 0);
        assert_eq!(page.edits.staged[&at(0, 2)].error, None);

        page.stage_cell(&dialect, at(0, 2), Some("eleven".into()));
        assert_eq!(page.revalidate(&dialect), 1);
    }

    #[test]
    fn the_first_problem_counts_rows_from_the_start_of_the_table() {
        let mut page = page();
        assert_eq!(page.first_problem(), None);
        page.stage_cell(&dialect("postgres"), at(2, 2), Some("many".into()));
        let problem = page.first_problem().expect("a problem");
        assert!(problem.starts_with("row 43, score: "), "{problem}");

        let mut conflicted = page_with_row_error();
        assert!(conflicted.first_problem().unwrap().starts_with("row 41: "));
        conflicted.edits.row_errors.clear();
        assert_eq!(conflicted.first_problem(), None);
    }

    fn page_with_row_error() -> Page {
        let mut page = page();
        page.edits.row_errors.insert(0, "gone".into());
        page
    }

    #[test]
    fn reverting_the_last_edit_of_a_row_clears_the_row_s_error_too() {
        let mut page = page();
        let dialect = dialect("postgres");
        page.stage_cell(&dialect, at(0, 1), Some("a".into()));
        page.stage_cell(&dialect, at(0, 2), Some("1".into()));
        page.edits.row_errors.insert(0, "gone".into());
        page.revert_cell(at(0, 1));
        assert!(!page.edits.row_errors.is_empty(), "an edit is still there");
        page.revert_cell(at(0, 2));
        assert!(page.edits.row_errors.is_empty());
    }

    #[test]
    fn tab_skips_what_cannot_be_edited_and_stops_at_the_ends() {
        let page = page();
        let environment = environment();
        // Row 0 name -> score; then the blob is skipped, then the next row's key.
        assert_eq!(
            next_editable(&page, &environment, at(0, 1), true),
            Some(at(0, 2))
        );
        assert_eq!(
            next_editable(&page, &environment, at(0, 2), true),
            Some(at(1, 1))
        );
        assert_eq!(
            next_editable(&page, &environment, at(1, 1), false),
            Some(at(0, 2))
        );
        assert_eq!(next_editable(&page, &environment, at(2, 2), true), None);
        assert_eq!(next_editable(&page, &environment, at(0, 1), false), None);
    }

    #[test]
    fn a_count_of_edits_reads_as_a_sentence() {
        assert_eq!(changes_label(1), "1 unsaved change");
        assert_eq!(changes_label(3), "3 unsaved changes");
    }
}
