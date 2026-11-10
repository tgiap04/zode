use crate::database_panel::DatabasePanel;
use crate::query::{Page, QueryState, cell_text};
use database::protocol::{Cell, ResultColumn};
use gpui::{App, ClipboardItem, Context};
use ui::{Tab, Tooltip, prelude::*};

impl DatabasePanel {
    /// Shows row `index` of the current page in the detail view, or closes the
    /// view when that row is already the one shown.
    pub(crate) fn toggle_row_detail(&mut self, index: usize, cx: &mut Context<Self>) {
        if let QueryState::Done(page) = &mut self.query {
            page.toggle_row(index);
            cx.notify();
        }
    }

    /// Closes the detail view. Answers whether there was one to close, so a
    /// key that closes it can be left to others when there was not.
    pub(crate) fn close_row_detail(&mut self, cx: &mut Context<Self>) -> bool {
        let QueryState::Done(page) = &mut self.query else {
            return false;
        };
        if page.selected_row.take().is_none() {
            return false;
        }
        cx.notify();
        true
    }

    /// Every field of the selected row in full: a grid cell is one truncated
    /// line, so a long or multi-line value is only readable here.
    pub(crate) fn render_row_detail(
        &self,
        page: &Page,
        index: usize,
        row: &[Cell],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let position = page.offset + index as u64 + 1;
        let mut fields = Vec::with_capacity(row.len());
        for (field, (column, cell)) in page.result.columns.iter().zip(row).enumerate() {
            fields.push(render_field(field, column, cell, cx));
        }
        let colors = cx.theme().colors();

        v_flex()
            .id("database-row-detail")
            .debug_selector(|| "database-row-detail".into())
            .size_full()
            .min_h_0()
            .bg(colors.panel_background)
            .child(
                h_flex()
                    .flex_none()
                    .h(Tab::container_height(cx))
                    .px_2()
                    .justify_between()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(Label::new(format!("Row {position}")).size(LabelSize::Small))
                    .child(
                        IconButton::new("database-row-detail-close", IconName::Close)
                            .icon_size(IconSize::Small)
                            .tooltip(|_window, cx| Tooltip::simple("Close Row Detail", cx))
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.close_row_detail(cx);
                            })),
                    ),
            )
            .child(
                v_flex()
                    .id("database-row-detail-fields")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(fields),
            )
            .into_any_element()
    }
}

/// One column of the row: its name and type, then the whole value. A null is
/// set apart from the text `NULL`, as it is in the grid.
fn render_field(position: usize, column: &ResultColumn, cell: &Cell, cx: &App) -> AnyElement {
    let text = cell_text(cell);
    let value = match cell {
        Cell::Null => Label::new(text.clone())
            .size(LabelSize::Small)
            .color(Color::Muted)
            .italic()
            .into_any_element(),
        _ => div()
            .text_ui_sm(cx)
            .text_color(cx.theme().colors().text)
            .child(text.clone())
            .into_any_element(),
    };

    v_flex()
        .debug_selector(move || format!("database-row-detail-field-{position}"))
        .px_2()
        .py_1()
        .gap_0p5()
        .border_b_1()
        .border_color(cx.theme().colors().border_variant)
        .child(
            h_flex()
                .justify_between()
                .gap_1()
                .child(
                    h_flex()
                        .min_w_0()
                        .gap_1()
                        .child(
                            Label::new(column.name.clone())
                                .size(LabelSize::Small)
                                .color(Color::Muted)
                                .truncate(),
                        )
                        .child(
                            Label::new(column.type_name.clone())
                                .size(LabelSize::XSmall)
                                .color(Color::Disabled)
                                .truncate(),
                        ),
                )
                .child(
                    IconButton::new(("database-row-detail-copy", position), IconName::Copy)
                        .icon_size(IconSize::XSmall)
                        .tooltip(|_window, cx| Tooltip::simple("Copy Value", cx))
                        // A null has no text to copy; copying the word would paste
                        // a value that was never there.
                        .disabled(matches!(cell, Cell::Null))
                        .on_click(move |_event, _window, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
                        }),
                ),
        )
        .child(value)
        .into_any_element()
}
