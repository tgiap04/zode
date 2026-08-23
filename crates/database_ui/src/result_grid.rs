use crate::database_panel::DatabasePanel;
use crate::query::{QueryState, cell_text};
use database::protocol::Cell;
use gpui::{Context, Window};
use std::rc::Rc;
use ui::{Table, Tooltip, prelude::*};

impl DatabasePanel {
    pub(crate) fn render_results(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match &self.query {
            QueryState::Idle => self.render_nothing_chosen(cx),
            QueryState::Running { cancelling, .. } => {
                v_flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(
                        Label::new(if *cancelling {
                            "Cancelling…"
                        } else {
                            "Running…"
                        })
                        .color(Color::Muted),
                    )
                    .when(!*cancelling, |element| {
                        element.child(Button::new("database-cancel", "Cancel").on_click(
                            cx.listener(|this, _event, _window, cx| {
                                this.cancel_query(cx);
                            }),
                        ))
                    })
                    .into_any_element()
            }
            // Read-only is called out by name: a user who typed `delete` needs
            // to be told the column is read-only, not that their SQL is wrong.
            QueryState::Failed(error) => v_flex()
                .size_full()
                .p_3()
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
                    Label::new(error.message.clone())
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .into_any_element(),
            QueryState::Done(page) if page.result.rows.is_empty() => {
                note("No rows", Color::Muted, cx)
            }
            QueryState::Done(page) => {
                let result = Rc::new(page.result.clone());
                let columns = result.columns.len();
                let row_count = result.rows.len();

                v_flex()
                    .size_full()
                    .min_h_0()
                    .child(
                        // Virtualised even though a page is bounded: `page_size`
                        // is a setting, and someone will set it to 10_000.
                        div().flex_1().min_h_0().overflow_hidden().child(
                            Table::new(columns)
                                .striped()
                                .header(
                                    result
                                        .columns
                                        .iter()
                                        .map(|column| column.name.clone())
                                        .collect::<Vec<_>>(),
                                )
                                .uniform_list("database-results", row_count, {
                                    let result = result.clone();
                                    move |range, _window, _cx| {
                                        range
                                            .filter_map(|index| result.rows.get(index))
                                            .map(|row| {
                                                row.iter().map(render_cell).collect::<Vec<_>>()
                                            })
                                            .collect()
                                    }
                                }),
                        ),
                    )
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
        let elapsed = page.result.elapsed_ms;

        div().child(
            h_flex()
                .flex_none()
                .w_full()
                .px_2()
                .py_1()
                .gap_1()
                .justify_between()
                .border_t_1()
                .border_color(cx.theme().colors().border)
                .child(
                    h_flex()
                        .gap_2()
                        .child(Label::new(label).size(LabelSize::Small).color(Color::Muted))
                        .child(
                            Label::new(format!("{elapsed} ms"))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        ),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            IconButton::new("database-copy-csv", IconName::Copy)
                                .icon_size(IconSize::Small)
                                .tooltip(|_window, cx| Tooltip::simple("Copy page as CSV", cx))
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.copy_page_as_csv(cx);
                                })),
                        )
                        .child(
                            IconButton::new("database-previous-page", IconName::ChevronLeft)
                                .icon_size(IconSize::Small)
                                .disabled(!previous)
                                .on_click(cx.listener(|this, _event, window, cx| {
                                    this.page(false, window, cx);
                                })),
                        )
                        .child(
                            IconButton::new("database-next-page", IconName::ChevronRight)
                                .icon_size(IconSize::Small)
                                .disabled(!next)
                                .on_click(cx.listener(|this, _event, window, cx| {
                                    this.page(true, window, cx);
                                })),
                        ),
                ),
        )
    }
}

/// A null is drawn differently from every other value, because it is the one
/// value that is not text -- and a grid that renders it like the empty string
/// is lying about the data.
fn render_cell(cell: &Cell) -> AnyElement {
    let text = cell_text(cell);
    match cell {
        Cell::Null => Label::new(text)
            .size(LabelSize::Small)
            .color(Color::Muted)
            .italic()
            .into_any_element(),
        _ => Label::new(text)
            .size(LabelSize::Small)
            .single_line()
            .into_any_element(),
    }
}

fn note(text: &'static str, colour: Color, _cx: &mut Context<DatabasePanel>) -> AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .child(Label::new(text).size(LabelSize::Small).color(colour))
        .into_any_element()
}
