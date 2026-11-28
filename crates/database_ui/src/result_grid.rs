use crate::database_panel::DatabasePanel;
use crate::grid_cell::{CellColors, grid_cell};
use crate::grid_editing::CellAddress;
use crate::query::{QueryState, TableBrowse, WRITE_PAGE_NOTE, cell_text};
use database::protocol::Cell;
use gpui::{ClickEvent, Context, MouseButton, Stateful, WeakEntity, Window};
use std::rc::Rc;
use ui::{ColumnWidthConfig, Tab, Table, Tooltip, prelude::*};

impl DatabasePanel {
    /// The grid, under the filter bar when the table on screen has one.
    ///
    /// The bar sits outside the grid's own states so that a filter matching
    /// nothing, or failing, still leaves it there to be changed.
    pub(crate) fn render_results(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let body = self.render_result_body(window, cx);
        // The body alone holds the grid's focus and key context, not the bar
        // above it: its text boxes must not be under the grid's arrow keys.
        let grid = v_flex()
            .min_h_0()
            .track_focus(&self.results_focus)
            .key_context("DatabaseResults")
            .child(body);
        let Some(bar) = self.render_filter_bar(cx) else {
            return grid.size_full().into_any_element();
        };
        v_flex()
            .size_full()
            .min_h_0()
            .child(bar)
            .child(grid.flex_1())
            .into_any_element()
    }

    fn render_result_body(&self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match &self.query {
            QueryState::Idle => self.render_nothing_chosen(cx),
            QueryState::Running { cancelling, .. } => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .p_4()
                .gap_1()
                .child(
                    Label::new(if *cancelling {
                        "Cancelling…"
                    } else {
                        "Running…"
                    })
                    .color(Color::Muted),
                )
                .when(!*cancelling, |element| {
                    element.child(div().pt_1().child(
                        Button::new("database-cancel", "Cancel").on_click(cx.listener(
                            |this, _event, _window, cx| {
                                this.cancel_query(cx);
                            },
                        )),
                    ))
                })
                .into_any_element(),
            // Read-only is called out by name: a user who typed `delete` needs
            // to be told the column is read-only, not that their SQL is wrong.
            QueryState::Failed(error) => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .p_4()
                .gap_1()
                .child(
                    Label::new(if error.read_only {
                        "This connection is read-only"
                    } else {
                        "Query failed"
                    })
                    .color(Color::Error),
                )
                .child(
                    div()
                        .id("database-query-error")
                        .debug_selector(|| "database-query-error".into())
                        // A driver's own words can run to a page. Bounded and
                        // scrolling, so the title above it stays where it is
                        // read instead of being centred off the top of the pane.
                        .min_h_0()
                        .max_h(rems(10.))
                        .overflow_y_scroll()
                        .child(
                            Label::new(error.message.clone())
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        ),
                )
                .into_any_element(),
            QueryState::Done(page) if page.result.rows.is_empty() => {
                note("No rows", Color::Muted, cx)
            }
            QueryState::Done(page) => {
                let result = Rc::new(page.result.clone());
                let columns = result.columns.len();
                let row_count = result.rows.len();
                let selection = page.selection.clone();
                let selected_background = cx.theme().colors().element_selected;
                let panel = cx.entity().downgrade();
                let edit_view = self.edit_view();
                let colors = Rc::new(CellColors::of(cx));
                // Offered only for a table opened from the tree: a statement
                // somebody typed is not rewritten to sort it.
                let sortable = self.can_sort().then(|| page.browse.clone()).flatten();

                let grid = div().flex_1().min_w_0().min_h_0().overflow_hidden().child(
                    // Virtualised even though a page is bounded: `page_size`
                    // is a setting, and someone will set it to 10_000.
                    Table::new(columns)
                        .interactable(&self.result_table)
                        // Sized for these columns only once `sync_result_columns`
                        // has seen them; until then, shared equally, since a
                        // width list of another length would not line up.
                        .when(
                            self.result_column_widths.read(cx).cols() == columns,
                            |table| {
                                table.width_config(ColumnWidthConfig::Resizable(
                                    self.result_column_widths.clone(),
                                ))
                            },
                        )
                        .striped()
                        .header_background(cx.theme().colors().tab_bar_background)
                        .header(
                            result
                                .columns
                                .iter()
                                .enumerate()
                                .map(|(index, column)| {
                                    let is_key = page.browse.as_ref().is_some_and(|browse| {
                                        browse.key_columns.contains(&column.name)
                                    });
                                    header_cell(
                                        &panel,
                                        index,
                                        &column.name,
                                        &column.type_name,
                                        is_key,
                                        sortable.as_ref(),
                                    )
                                })
                                .collect::<Vec<_>>(),
                        )
                        .uniform_list("database-results", row_count, {
                            let result = result.clone();
                            let panel = panel.clone();
                            move |range, window, cx| {
                                range
                                    .filter_map(|index| {
                                        result.rows.get(index).map(|row| (index, row))
                                    })
                                    .map(|(index, row)| {
                                        row.iter()
                                            .enumerate()
                                            .map(|(column, cell)| {
                                                grid_cell(
                                                    &panel,
                                                    &edit_view,
                                                    &colors,
                                                    CellAddress { row: index, column },
                                                    cell,
                                                    window,
                                                    cx,
                                                )
                                            })
                                            .collect::<Vec<_>>()
                                    })
                                    .collect()
                            }
                        })
                        .map_row(move |(index, row), _window, _cx| {
                            let panel = panel.clone();
                            row.debug_selector(move || format!("database-result-row-{index}"))
                                .cursor_pointer()
                                .when(selection.contains(index), |row| row.bg(selected_background))
                                .on_click(move |event, window, cx| {
                                    panel
                                        .update(cx, |panel, cx| {
                                            // The second click of a double-click edits
                                            // the cell under the pointer; where it
                                            // cannot, it is a click like any other.
                                            if event.click_count() >= 2
                                                && panel.edit_under_pointer(
                                                    index,
                                                    event.modifiers(),
                                                    window,
                                                    cx,
                                                )
                                            {
                                                return;
                                            }
                                            panel.click_row(index, event.modifiers(), window, cx)
                                        })
                                        .ok();
                                })
                                .into_any_element()
                        }),
                );

                // Beside the rows when the view is wide; under them in the
                // docked column, and in a side-by-side view too narrow to spare
                // the width, where a side pane would leave neither readable.
                let side_by_side = self.row_detail_beside();
                let detail = page.selected().map(|(index, row)| {
                    let detail = self.render_row_detail(page, index, row, cx);
                    let border = cx.theme().colors().border;
                    if side_by_side {
                        div()
                            .flex_none()
                            .w(rems(20.))
                            .h_full()
                            .border_l_1()
                            .border_color(border)
                            .child(detail)
                    } else {
                        div()
                            .flex_none()
                            .w_full()
                            .h(relative(0.5))
                            .border_t_1()
                            .border_color(border)
                            .child(detail)
                    }
                });

                v_flex()
                    .size_full()
                    .min_h_0()
                    .child(
                        div()
                            .flex()
                            .when(!side_by_side, |body| body.flex_col())
                            .flex_1()
                            .min_h_0()
                            .child(grid)
                            .children(detail),
                    )
                    .children(self.render_edit_bar(cx))
                    .child(self.render_pager(cx))
                    .into_any_element()
            }
        }
    }

    /// What stands where the rows go before anything has been asked for.
    ///
    /// A line of grey text is enough in the column, where this region is a few
    /// hundred pixels tall. Laid out sideways it is the largest thing on screen,
    /// and an all-but-empty half-window reads as a failure to load rather than
    /// as a waiting room -- so there it says what to do, and where.
    ///
    /// Keyed on the layout rather than on `full_screen`, which is one of two ways
    /// to reach that layout: a tab and a floating window get there by being wide.
    fn render_nothing_chosen(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.side_by_side() {
            return note("Run a statement to see results here", Color::Muted, cx);
        }

        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .p_4()
            .gap_1()
            .debug_selector(|| "database-no-table-placeholder".into())
            .child(
                Icon::new(IconName::Table)
                    .size(IconSize::XLarge)
                    .color(Color::Muted),
            )
            .child(Label::new("No table selected"))
            .child(
                Label::new("Pick a table on the left to read its rows, or write a statement below and run it.")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .into_any_element()
    }

    fn render_pager(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let QueryState::Done(page) = &self.query else {
            return div();
        };
        let (label, previous, next) = (page.range_label(), page.has_previous(), page.has_next());
        let turnable = page.can_turn();
        let elapsed = page.result.elapsed_ms;
        let can_filter = self.can_filter();
        let filter_shown = self.browse_bar.expanded() || self.has_active_filters();
        let export_label = self.export_label();
        let exporting = matches!(
            self.export.status,
            crate::export_results::ExportStatus::Running { .. }
        );
        let export_over = !exporting && export_label.is_some();
        let edit_notice = page.edits.notice.clone();

        div().child(
            h_flex()
                .flex_none()
                .w_full()
                .h(Tab::container_height(cx))
                .px_2()
                .justify_between()
                .bg(cx.theme().colors().tab_bar_background)
                .border_t_1()
                .border_color(cx.theme().colors().border_variant)
                .child(
                    // Gives way to the buttons on a narrow view: the page
                    // controls are what is used, the counts only read.
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_2()
                        .overflow_hidden()
                        .child(status_chip(None, label.into(), cx))
                        .child(status_chip(
                            Some(IconName::Clock),
                            format!("{elapsed} ms").into(),
                            cx,
                        ))
                        .children(edit_notice.map(|text| {
                            Label::new(text)
                                .size(LabelSize::XSmall)
                                .color(Color::Warning)
                                .truncate()
                        }))
                        .children(export_label.map(|(text, colour)| {
                            Label::new(text)
                                .size(LabelSize::XSmall)
                                .color(colour)
                                .truncate()
                        })),
                )
                .child(
                    h_flex()
                        .flex_none()
                        .gap_0p5()
                        .when(can_filter, |buttons| {
                            buttons.child(
                                IconButton::new("database-toggle-filter", IconName::ListFilter)
                                    .icon_size(IconSize::Small)
                                    .toggle_state(filter_shown)
                                    .tooltip(|_window, cx| {
                                        Tooltip::for_action(
                                            "Filter Rows",
                                            &zode_actions::database::ToggleFilterBar,
                                            cx,
                                        )
                                    })
                                    .on_click(cx.listener(|this, _event, window, cx| {
                                        this.toggle_filter_bar(window, cx);
                                    })),
                            )
                        })
                        .when(exporting, |buttons| {
                            buttons.child(
                                Button::new("database-export-cancel", "Cancel Export")
                                    .label_size(LabelSize::Small)
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.cancel_export(cx);
                                    })),
                            )
                        })
                        .when(export_over, |buttons| {
                            buttons.child(
                                IconButton::new("database-export-dismiss", IconName::Close)
                                    .icon_size(IconSize::Small)
                                    .tooltip(Tooltip::text("Dismiss"))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.dismiss_export_status(cx);
                                    })),
                            )
                        })
                        .child(
                            IconButton::new("database-export", IconName::Download)
                                .icon_size(IconSize::Small)
                                .disabled(exporting)
                                .tooltip(Tooltip::text("Export to a File"))
                                .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                                    this.deploy_export_menu(event.position(), window, cx);
                                })),
                        )
                        .child(
                            IconButton::new("database-copy-csv", IconName::Copy)
                                .icon_size(IconSize::Small)
                                .tooltip(|_window, cx| Tooltip::simple("Copy Page as CSV", cx))
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.copy_page_as_csv(cx);
                                })),
                        )
                        .child(
                            IconButton::new("database-previous-page", IconName::ChevronLeft)
                                .icon_size(IconSize::Small)
                                .tooltip(move |_window, cx| {
                                    if turnable {
                                        Tooltip::for_action(
                                            "Previous Page",
                                            &zode_actions::database::PreviousPage,
                                            cx,
                                        )
                                    } else {
                                        Tooltip::simple(WRITE_PAGE_NOTE, cx)
                                    }
                                })
                                .disabled(!previous)
                                .on_click(cx.listener(|this, _event, window, cx| {
                                    this.page(false, window, cx);
                                })),
                        )
                        .child(
                            IconButton::new("database-next-page", IconName::ChevronRight)
                                .icon_size(IconSize::Small)
                                .tooltip(move |_window, cx| {
                                    if turnable {
                                        Tooltip::for_action(
                                            "Next Page",
                                            &zode_actions::database::NextPage,
                                            cx,
                                        )
                                    } else {
                                        Tooltip::simple(WRITE_PAGE_NOTE, cx)
                                    }
                                })
                                .disabled(!next)
                                .on_click(cx.listener(|this, _event, window, cx| {
                                    this.page(true, window, cx);
                                })),
                        ),
                ),
        )
    }
}

/// A small rounded figure in the status bar: what a page holds, and how long
/// it took. Chips rather than loose grey text, so the two read as two facts.
fn status_chip(icon: Option<IconName>, text: SharedString, cx: &App) -> impl IntoElement {
    h_flex()
        .flex_none()
        .gap_1()
        .px_1p5()
        .py_px()
        .rounded_md()
        .bg(cx.theme().colors().element_background)
        .when_some(icon, |chip, icon| {
            chip.child(Icon::new(icon).size(IconSize::XSmall).color(Color::Muted))
        })
        .child(Label::new(text).size(LabelSize::XSmall).color(Color::Muted))
}

/// A column's name over its type, with a key for the column a table is keyed
/// on. On a table opened from the tree it is also the button that sorts by it,
/// with an arrow once it does.
///
/// The type is the engine's own word for it, muted and small: enough to tell a
/// `text` id from an `integer` one at a glance without widening the column.
fn header_cell(
    panel: &WeakEntity<DatabasePanel>,
    index: usize,
    name: &str,
    type_name: &str,
    is_key: bool,
    sortable: Option<&TableBrowse>,
) -> AnyElement {
    let label = h_flex()
        .min_w_0()
        .gap_1()
        .when(is_key, |label| {
            label.child(
                Icon::new(IconName::Hash)
                    .size(IconSize::XSmall)
                    .color(Color::Warning),
            )
        })
        .child(
            Label::new(name.to_string())
                .size(LabelSize::Small)
                .weight(gpui::FontWeight::SEMIBOLD)
                .truncate(),
        )
        .when(!type_name.is_empty(), |label| {
            label.child(
                Label::new(type_name.to_lowercase())
                    .size(LabelSize::XSmall)
                    .color(Color::Muted)
                    .truncate(),
            )
        });
    let Some(browse) = sortable else {
        return label.into_any_element();
    };
    let (panel, column) = (panel.clone(), name.to_string());
    let selector = format!("database-sort-{name}");
    h_flex()
        .id(("database-sort", index))
        .debug_selector(move || selector)
        .min_w_0()
        .gap_1()
        .cursor_pointer()
        .child(label)
        .when_some(browse.sort_of(name), |header, descending| {
            header.child(
                Icon::new(if descending {
                    IconName::ArrowDown
                } else {
                    IconName::ArrowUp
                })
                .size(IconSize::XSmall)
                .color(Color::Accent),
            )
        })
        .on_click(move |_event, window, cx| {
            panel
                .update(cx, |panel, cx| panel.sort_by(&column, window, cx))
                .ok();
        })
        .into_any_element()
}

/// A cell that knows its place in the grid: pressing in it remembers the
/// column, and right-clicking it opens the copy menu.
///
/// A plain wrapper rather than a handler on the row, which cannot tell which of
/// its cells was under the pointer.
pub(crate) fn cell_target(
    panel: &WeakEntity<DatabasePanel>,
    address: CellAddress,
    name: SharedString,
    content: AnyElement,
) -> Stateful<Div> {
    let (pressed, menu) = (panel.clone(), panel.clone());
    let (row, column) = (address.row, address.column);
    let selector = name.clone();
    div()
        .id(name)
        .debug_selector(move || selector.to_string())
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
            pressed
                .update(cx, |panel, cx| panel.focus_cell_column(column, cx))
                .ok();
        })
        .on_mouse_down(MouseButton::Right, move |event, window, cx| {
            menu.update(cx, |panel, cx| {
                panel.deploy_cell_menu(row, column, event.position, window, cx)
            })
            .ok();
        })
        .child(content)
}

/// A null is drawn differently from every other value, because it is the one
/// value that is not text -- and a grid that renders it like the empty string
/// is lying about the data.
///
/// Every cell is one line. The rows of a `uniform_list` all take the height of
/// the first, so a text value holding a newline would make every row as tall as
/// the first row's longest cell and clip the rest.
pub(crate) fn render_cell(cell: &Cell) -> AnyElement {
    let text = one_line(&cell_text(cell));
    match cell {
        Cell::Null => Label::new(text)
            .size(LabelSize::Small)
            .color(Color::Muted)
            .italic()
            .truncate()
            .into_any_element(),
        _ => Label::new(text)
            .size(LabelSize::Small)
            .truncate()
            .into_any_element(),
    }
}

/// The text with each line break written as `⏎`, which is what
/// `Label::single_line` does and what `truncate` alone does not.
fn one_line(text: &str) -> SharedString {
    if !text.contains(['\n', '\r']) {
        return SharedString::from(text.to_string());
    }
    SharedString::from(text.replace("\r\n", "⏎").replace(['\n', '\r'], "⏎"))
}

fn note(text: &'static str, colour: Color, _cx: &mut Context<DatabasePanel>) -> AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .child(Label::new(text).size(LabelSize::Small).color(colour))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_holding_line_breaks_is_drawn_on_one_line() {
        assert_eq!(one_line("first\nsecond"), "first⏎second");
        assert_eq!(one_line("first\r\nsecond"), "first⏎second");
        assert_eq!(one_line("first\rsecond"), "first⏎second");
        assert_eq!(one_line("a\n\nb"), "a⏎⏎b");
        assert_eq!(one_line("\n"), "⏎");
    }

    #[test]
    fn a_cell_without_line_breaks_is_left_as_it_is() {
        assert_eq!(one_line("plain text"), "plain text");
        assert_eq!(one_line(""), "");
        assert_eq!(one_line("tab\tseparated"), "tab\tseparated");
    }
}
