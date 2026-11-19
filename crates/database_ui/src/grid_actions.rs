//! What clicking, keying and right-clicking the result grid does: select rows,
//! and copy them in the format asked for.

use crate::browse_filter_bar::filter_for_cell;
use crate::database_panel::DatabasePanel;
use crate::grid_editing::CellMenu;
use crate::query::{QueryState, cell_text};
use crate::row_serializers::{CopyFormat, offered_formats, to_csv, to_insert, to_json, to_tsv};
use crate::sql_dialect::Dialect;
use crate::tree_menus::icon_entry;
use database::protocol::Cell;
use gpui::{ClipboardItem, Context, Modifiers, Pixels, Point, ScrollStrategy, WeakEntity, Window};
use ui::{ContextMenu, ContextMenuEntry, IconPosition, prelude::*};

/// An entry that calls back into the panel, for the page that was on screen
/// when the menu opened: a menu stays up while another page is read, and its
/// row and column would then name a cell of rows it was never opened over.
fn panel_entry(
    menu: ContextMenu,
    label: impl Into<SharedString>,
    icon: IconName,
    panel: &WeakEntity<DatabasePanel>,
    read: u64,
    action: impl Fn(&mut DatabasePanel, &mut Context<DatabasePanel>) + 'static,
) -> ContextMenu {
    let panel = panel.clone();
    icon_entry(menu, label, icon, move |_window, cx| {
        panel
            .update(cx, |panel, cx| {
                if panel.is_page_read(read) {
                    action(panel, cx)
                }
            })
            .ok();
    })
}

/// What the menu says about editing the cell: always there on a table, greyed
/// out where the cell cannot be edited, so it is clear the table can be.
fn edit_entries(
    menu: ContextMenu,
    edit: &CellMenu,
    row: usize,
    column: usize,
    read: u64,
    panel: &WeakEntity<DatabasePanel>,
) -> ContextMenu {
    let entry = |label: &'static str, icon: IconName, enabled: bool| {
        ContextMenuEntry::new(label)
            .icon(icon)
            .icon_position(IconPosition::Start)
            .icon_color(Color::Muted)
            .disabled(!enabled)
    };
    let menu = menu.item(
        entry("Edit Value", IconName::Pencil, edit.editable).handler({
            let panel = panel.clone();
            move |window, cx| {
                panel
                    .update(cx, |panel, cx| {
                        if panel.is_page_read(read) {
                            panel.begin_edit(row, column, window, cx);
                        }
                    })
                    .ok();
            }
        }),
    );
    let menu = if edit.nullable {
        menu.item(
            entry("Set to NULL", IconName::Eraser, edit.editable).handler({
                let panel = panel.clone();
                move |_window, cx| {
                    panel
                        .update(cx, |panel, cx| {
                            if panel.is_page_read(read) {
                                panel.set_cell_null(row, column, cx)
                            }
                        })
                        .ok();
                }
            }),
        )
    } else {
        menu
    };
    let menu = if edit.staged {
        let panel = panel.clone();
        icon_entry(menu, "Revert Change", IconName::Undo, move |_window, cx| {
            panel
                .update(cx, |panel, cx| {
                    if panel.is_page_read(read) {
                        panel.revert_cell_edit(row, column, cx)
                    }
                })
                .ok();
        })
    } else {
        menu
    };
    menu.separator()
}

impl DatabasePanel {
    /// A click on a row. Plain, it selects that row and shows its detail, as it
    /// always did; with a modifier it only changes the selection, so building a
    /// range never opens and closes the detail view along the way.
    pub(crate) fn click_row(
        &mut self,
        index: usize,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let QueryState::Done(page) = &mut self.query else {
            return;
        };
        if index >= page.result.rows.len() {
            return;
        }
        if modifiers.shift {
            page.selection.extend_to(index);
        } else if modifiers.secondary() {
            page.selection.toggle(index);
        } else {
            page.selection.select_only(index);
            self.toggle_row_detail(index, cx);
        }
        // A click does not move focus on its own, and the arrow keys are bound
        // for whatever holds the grid's focus.
        window.focus(&self.results_focus, cx);
        cx.notify();
    }

    /// Remembers which column a press landed in, for `copy` to tell one cell
    /// from a row.
    pub(crate) fn focus_cell_column(&mut self, column: usize, cx: &mut Context<Self>) {
        let QueryState::Done(page) = &mut self.query else {
            return;
        };
        page.selection.set_column(column);
        page.edits.notice = None;
        cx.notify();
    }

    pub(crate) fn select_all_rows(&mut self, cx: &mut Context<Self>) {
        let QueryState::Done(page) = &mut self.query else {
            return;
        };
        let count = page.result.rows.len();
        page.selection.select_all(count);
        cx.notify();
    }

    /// Up and down: the selection moves one row, the grid scrolls to keep it
    /// in view, and an open detail follows it.
    pub(crate) fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let QueryState::Done(page) = &mut self.query else {
            return;
        };
        let count = page.result.rows.len();
        let Some(row) = page.selection.move_cursor(delta, count) else {
            return;
        };
        if page.selected_row.is_some() {
            page.selected_row = Some(row);
        }
        self.result_table
            .read(cx)
            .scroll_handle
            .scroll_to_item(row, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// What `CopySelection` does: the cell, when one is selected, otherwise
    /// the rows as TSV, which pastes into a spreadsheet as rows and columns.
    pub(crate) fn copy_selection(&mut self, cx: &mut Context<Self>) {
        let QueryState::Done(page) = &self.query else {
            return;
        };
        let text = match page.selection.single_cell() {
            Some((row, column)) => {
                let Some(cell) = page.result.rows.get(row).and_then(|row| row.get(column)) else {
                    return;
                };
                match cell {
                    // Copying the word would paste a value that was never there.
                    Cell::Null => return,
                    cell => cell_text(cell).to_string(),
                }
            }
            None if page.selection.is_empty() => return,
            None => to_tsv(&page.result, &page.selection.rows()),
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// The selected rows in `format`. Nothing for a format the engine cannot
    /// write, which the menu does not offer either.
    pub(crate) fn copy_rows_as(&mut self, format: CopyFormat, cx: &mut Context<Self>) {
        let QueryState::Done(page) = &self.query else {
            return;
        };
        let rows = page.selection.rows();
        if rows.is_empty() {
            return;
        }
        let text = match format {
            CopyFormat::Tsv => Some(to_tsv(&page.result, &rows)),
            CopyFormat::Csv => Some(to_csv(&page.result, &rows)),
            CopyFormat::Json => Some(to_json(&page.result, &rows)),
            CopyFormat::Insert => self.result_dialect().and_then(|dialect| {
                let table = page
                    .browse
                    .as_ref()
                    .map(|browse| (browse.schema.as_str(), browse.table.as_str()));
                to_insert(&dialect, table, &page.result, &rows)
            }),
        };
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn copy_cell(&mut self, row: usize, column: usize, cx: &mut Context<Self>) {
        let QueryState::Done(page) = &self.query else {
            return;
        };
        let Some(cell) = page.result.rows.get(row).and_then(|row| row.get(column)) else {
            return;
        };
        if !matches!(cell, Cell::Null) {
            cx.write_to_clipboard(ClipboardItem::new_string(cell_text(cell).to_string()));
        }
    }

    /// The dialect of the connection the page on screen came from, which is not
    /// necessarily the active one: that may have been clicked since.
    pub(crate) fn result_dialect(&self) -> Option<Dialect> {
        let QueryState::Done(page) = &self.query else {
            return None;
        };
        let index = self.index_of_key(page.connection.as_ref()?)?;
        self.dialect(index)
    }

    pub(crate) fn deploy_cell_menu(
        &mut self,
        row: usize,
        column: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let formats = offered_formats(self.result_dialect().as_ref());
        let can_filter = self.can_filter();
        let edit = self.cell_menu(row, column, cx);
        let QueryState::Done(page) = &mut self.query else {
            return;
        };
        let read = page.origin.read;
        let Some(cell) = page.result.rows.get(row).and_then(|row| row.get(column)) else {
            return;
        };
        let has_value = !matches!(cell, Cell::Null);
        let filterable =
            can_filter && page.browse.is_some() && filter_for_cell(String::new(), cell).is_some();
        let Some(column_name) = page
            .result
            .columns
            .get(column)
            .map(|column| column.name.clone())
        else {
            return;
        };
        // The menu acts on the rows it was opened over: right-clicking one
        // outside the selection selects it, one inside keeps the selection.
        if !page.selection.contains(row) {
            page.selection.select_only(row);
        }
        let label = if page.selection.rows().len() == 1 {
            "Copy Row as"
        } else {
            "Copy Rows as"
        };
        cx.notify();

        let panel = cx.weak_entity();
        self.open_context_menu(position, window, cx, move |menu, _window, _cx| {
            let menu = if edit.offered {
                edit_entries(menu, &edit, row, column, read, &panel)
            } else {
                menu
            };
            let menu = if has_value {
                panel_entry(
                    menu,
                    "Copy Value",
                    IconName::Copy,
                    &panel,
                    read,
                    move |panel, cx| panel.copy_cell(row, column, cx),
                )
            } else {
                menu
            };
            let menu = if filterable {
                let panel = panel.clone();
                icon_entry(
                    menu,
                    "Filter by This Value",
                    IconName::Filter,
                    move |window, cx| {
                        panel
                            .update(cx, |panel, cx| {
                                if panel.is_page_read(read) {
                                    panel.filter_by_value(row, column, window, cx)
                                }
                            })
                            .ok();
                    },
                )
            } else {
                menu
            };
            let menu = menu.submenu_with_icon(label, IconName::Copy, {
                let panel = panel.clone();
                move |mut submenu, _window, _cx| {
                    for format in &formats {
                        let format = *format;
                        submenu = panel_entry(
                            submenu,
                            format.label(),
                            format.icon(),
                            &panel,
                            read,
                            move |panel, cx| panel.copy_rows_as(format, cx),
                        );
                    }
                    submenu
                }
            });
            panel_entry(
                menu,
                "Copy Column Name",
                IconName::TextSnippet,
                &panel,
                read,
                move |_panel, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(column_name.clone()));
                },
            )
        });
    }
}
