//! What a cell of the grid looks like while it has an edit: as it was read, as
//! it is to be saved, marked where the value is wrong, or as the box being typed
//! into.

use crate::database_panel::DatabasePanel;
use crate::grid_editing::CellAddress;
use crate::query::QueryState;
use crate::result_grid::{cell_target, render_cell};
use database::protocol::Cell;
use gpui::{App, Entity, Hsla, MouseButton, SharedString, WeakEntity, Window};
use std::collections::BTreeMap;
use ui::{Tooltip, prelude::*};
use ui_input::InputField;
use zode_actions::database as actions;

impl DatabasePanel {
    /// What the grid draws over the cells it has edits for, taken as a copy so
    /// that the virtualised list can hold it.
    pub(crate) fn edit_view(&self) -> std::rc::Rc<EditView> {
        let QueryState::Done(page) = &self.query else {
            return Default::default();
        };
        let staged = page
            .edits
            .staged
            .iter()
            .map(|(address, staged)| {
                let problem = staged
                    .error
                    .clone()
                    .map(SharedString::from)
                    .or_else(|| page.edits.row_errors.get(&address.row).cloned());
                (*address, (staged.value.clone(), problem))
            })
            .collect();
        let editing = page
            .edits
            .editor
            .as_ref()
            .map(|editor| (editor.address, editor.input.clone()));
        std::rc::Rc::new(EditView { staged, editing })
    }
}

/// The edits as the grid draws them: for each staged cell its value and what is
/// wrong with it, and the box being typed into.
#[derive(Default)]
pub(crate) struct EditView {
    staged: BTreeMap<CellAddress, (Option<String>, Option<SharedString>)>,
    editing: Option<(CellAddress, Entity<InputField>)>,
}

pub(crate) struct CellColors {
    pub(crate) edited: Hsla,
    pub(crate) invalid: Hsla,
    pub(crate) invalid_border: Hsla,
    pub(crate) focus_border: Hsla,
    pub(crate) editor_background: Hsla,
}

impl CellColors {
    pub(crate) fn of(cx: &App) -> Self {
        let status = cx.theme().status();
        let colors = cx.theme().colors();
        Self {
            edited: status.modified_background.opacity(0.35),
            invalid: status.error_background.opacity(0.35),
            invalid_border: status.error_border,
            focus_border: colors.border_focused,
            editor_background: colors.editor_background,
        }
    }
}

/// One cell of the grid: as it was read, as it is to be saved, or as a box
/// being typed into.
pub(crate) fn grid_cell(
    panel: &WeakEntity<DatabasePanel>,
    view: &EditView,
    colors: &CellColors,
    address: CellAddress,
    cell: &Cell,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let name = SharedString::from(format!("database-cell-{}-{}", address.row, address.column));
    if let Some((editing, input)) = &view.editing
        && *editing == address
    {
        return editor_cell(panel, input, colors, name, window, cx);
    }
    let Some((value, problem)) = view.staged.get(&address) else {
        return cell_target(panel, address, name, render_cell(cell)).into_any_element();
    };
    let shown = match value {
        Some(text) => Cell::Text {
            value: text.clone(),
        },
        None => Cell::Null,
    };
    cell_target(panel, address, name, render_cell(&shown))
        .bg(if problem.is_some() {
            colors.invalid
        } else {
            colors.edited
        })
        .when(problem.is_some(), |cell| {
            cell.border_1().border_color(colors.invalid_border)
        })
        .when_some(problem.clone(), |cell, problem| {
            cell.tooltip(Tooltip::text(problem))
        })
        .into_any_element()
}

fn editor_cell(
    panel: &WeakEntity<DatabasePanel>,
    input: &Entity<InputField>,
    colors: &CellColors,
    name: SharedString,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let editor = input.read(cx).editor().clone().render(window, cx);
    let selector = name.clone();
    let keys = panel.clone();
    let (confirm, cancel, next, previous) =
        (panel.clone(), panel.clone(), panel.clone(), panel.clone());
    div()
        .id(name)
        .debug_selector(move || selector.to_string())
        .key_context("DatabaseCellEditor")
        .w_full()
        .min_w_0()
        .px_1()
        .border_1()
        .border_color(colors.focus_border)
        .bg(colors.editor_background)
        // The pointer is in the box, not on the row behind it: a click that
        // reached the row would select it and take the focus away.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _, cx| cx.stop_propagation())
        .on_action(move |_: &menu::Confirm, window, cx| {
            confirm
                .update(cx, |panel, cx| panel.confirm_editor(window, cx))
                .ok();
        })
        .on_action(move |_: &menu::Cancel, window, cx| {
            cancel
                .update(cx, |panel, cx| panel.cancel_editor(window, cx))
                .ok();
        })
        .on_action(move |_: &actions::EditNextCell, window, cx| {
            next.update(cx, |panel, cx| panel.edit_neighbour(true, window, cx))
                .ok();
        })
        .on_action(move |_: &actions::EditPreviousCell, window, cx| {
            previous
                .update(cx, |panel, cx| panel.edit_neighbour(false, window, cx))
                .ok();
        })
        // The arrows are the grid's; a one-line box has no use for them, and
        // moving the selection out from under it would strand the box.
        .on_action(|_: &menu::SelectNext, _, cx| cx.stop_propagation())
        .on_action(|_: &menu::SelectPrevious, _, cx| cx.stop_propagation())
        .on_action(move |_: &actions::SaveChanges, window, cx| {
            keys.update(cx, |panel, cx| panel.request_save(window, cx))
                .ok();
        })
        .child(editor)
        .into_any_element()
}
