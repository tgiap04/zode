//! The filter bar over a table opened with Open Data, and the sort and filter
//! actions that re-read the table.
//!
//! Sorting and filtering happen on the server because the grid shows one page
//! of a larger table: ordering the page on screen would look right and be
//! wrong. The state they change lives in `TableBrowse`; the statement is
//! rebuilt from it each time rather than edited.

use crate::connection_store::DatabaseSettings;
use crate::database_panel::DatabasePanel;
use crate::query::{Filter, FilterOp, QueryState, TableBrowse, ValueKind};
use crate::sql_dialect::{Dialect, Feature};
use database::protocol::{Cell, ResultSet};
use gpui::{ClickEvent, Context, Entity, Pixels, Point, WeakEntity, Window};
use settings::Settings as _;
use ui::{ContextMenu, Tooltip, prelude::*};
use ui_input::InputField;

/// One row of the bar: the filter being composed, applied or not.
pub(crate) struct DraftRow {
    column: String,
    op: FilterOp,
    kind: ValueKind,
    input: Entity<InputField>,
}

#[derive(Default)]
pub(crate) struct BrowseFilterBar {
    expanded: bool,
    rows: Vec<DraftRow>,
    /// The browse the latest run asked for, kept for a run that failed or is
    /// still out: the bar has to stay so a bad filter can be fixed.
    last: Option<TableBrowse>,
    /// The columns the filter can name, and what each holds. Remembered from
    /// the last browse page, because a filter that matches nothing leaves a
    /// page with no rows to tell numbers from text by.
    columns: Vec<(String, ValueKind)>,
}

impl BrowseFilterBar {
    /// A table opened fresh starts with no filter, collapsed.
    pub(crate) fn reset(&mut self) {
        self.expanded = false;
        self.rows.clear();
        self.columns.clear();
    }

    pub(crate) fn note_run(&mut self, browse: Option<&TableBrowse>) {
        let same_table = match (&self.last, browse) {
            (Some(last), Some(next)) => {
                (&last.connection, &last.schema, &last.table)
                    == (&next.connection, &next.schema, &next.table)
            }
            _ => false,
        };
        if !same_table {
            self.columns.clear();
        }
        self.last = browse.cloned();
    }

    pub(crate) fn note_columns(&mut self, result: &ResultSet) {
        let previous = std::mem::take(&mut self.columns);
        self.columns = result
            .columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                let sampled = result
                    .rows
                    .iter()
                    .find_map(|row| row.get(index).and_then(ValueKind::of));
                let remembered = previous
                    .iter()
                    .find(|(name, _)| *name == column.name)
                    .map(|(_, kind)| *kind);
                (
                    column.name.clone(),
                    sampled.or(remembered).unwrap_or(ValueKind::Text),
                )
            })
            .collect();
    }

    pub(crate) fn expanded(&self) -> bool {
        self.expanded
    }

    #[cfg(test)]
    pub(crate) fn row_count(&self) -> usize {
        self.rows.len()
    }

    #[cfg(test)]
    pub(crate) fn type_into_row(
        &self,
        index: usize,
        text: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(row) = self.rows.get(index) {
            let editor = row.input.read(cx).editor().clone();
            editor.set_text(text, window, cx);
        }
    }

    fn rebuild(&mut self, filters: &[Filter], window: &mut Window, cx: &mut App) {
        self.rows = filters
            .iter()
            .map(|filter| {
                let input = new_input(window, cx);
                let editor = input.read(cx).editor().clone();
                editor.set_text(&filter.value, window, cx);
                DraftRow {
                    column: filter.column.clone(),
                    op: filter.op,
                    kind: filter.kind,
                    input,
                }
            })
            .collect();
    }

    /// The rows that say something: a comparison with nothing typed to compare
    /// against is a row somebody added and has not filled in yet.
    fn filters(&self, cx: &App) -> Vec<Filter> {
        self.rows
            .iter()
            .filter_map(|row| {
                let text = row.input.read(cx).text(cx);
                if row.op.needs_value() && text.trim().is_empty() {
                    return None;
                }
                Some(Filter {
                    column: row.column.clone(),
                    op: row.op,
                    value: if row.op.needs_value() {
                        text
                    } else {
                        String::new()
                    },
                    kind: row.kind,
                })
            })
            .collect()
    }
}

fn new_input(window: &mut Window, cx: &mut App) -> Entity<InputField> {
    cx.new(|cx| InputField::new(window, cx, "Value"))
}

impl DatabasePanel {
    /// The table the grid is, or was last asked to be, reading.
    pub(crate) fn current_browse(&self) -> Option<&TableBrowse> {
        match &self.query {
            QueryState::Done(page) => page.browse.as_ref(),
            _ => self.browse_bar.last.as_ref(),
        }
    }

    pub(crate) fn browse_dialect(&self) -> Option<Dialect> {
        let index = self.index_of_key(&self.current_browse()?.connection)?;
        self.dialect(index)
    }

    /// Whether the header offers sorting: only for a table opened from the
    /// tree, whose read can be rewritten, on an engine that can sort it.
    pub(crate) fn can_sort(&self) -> bool {
        self.browse_dialect()
            .is_some_and(|dialect| dialect.supports(Feature::Sort))
    }

    pub(crate) fn can_filter(&self) -> bool {
        self.browse_dialect()
            .is_some_and(|dialect| dialect.supports(Feature::Filter))
    }

    pub(crate) fn has_active_filters(&self) -> bool {
        self.current_browse()
            .is_some_and(|browse| !browse.filters.is_empty())
    }

    /// Reads the table again from its first row, in the order and with the
    /// filters `browse` carries.
    pub(crate) fn run_browse(
        &mut self,
        browse: TableBrowse,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Looked up now, not when the table was opened: the list may have moved
        // since, and a statement run at the old place is run on another database.
        let Some(index) = self.index_of_key(&browse.connection) else {
            return;
        };
        self.set_active(index, window, cx);
        let Some(sql) = self
            .dialect(index)
            .and_then(|dialect| browse.statement(&dialect))
        else {
            return;
        };
        let limit = DatabaseSettings::get_global(cx).page_size;
        self.run_page(sql, 0, limit, Some(browse), window, cx);
    }

    /// A click on a header: ascending, then descending, then unsorted.
    pub(crate) fn sort_by(&mut self, column: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_sort() {
            return;
        }
        let Some(mut browse) = self.current_browse().cloned() else {
            return;
        };
        browse.cycle_sort(column);
        self.run_browse(browse, window, cx);
    }

    pub(crate) fn toggle_filter_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_filter() {
            return;
        }
        self.note_filter_columns();
        let bar = &mut self.browse_bar;
        bar.expanded = !bar.expanded;
        if bar.expanded && bar.rows.is_empty() {
            self.add_filter_row(window, cx);
        }
        cx.notify();
    }

    /// Keeps the bar's columns in step with a page that did not come through
    /// `run_page`.
    fn note_filter_columns(&mut self) {
        if let QueryState::Done(page) = &self.query
            && page.browse.is_some()
        {
            self.browse_bar.note_columns(&page.result);
        }
    }

    pub(crate) fn add_filter_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.note_filter_columns();
        let Some((column, kind)) = self.browse_bar.columns.first().cloned() else {
            return;
        };
        let input = new_input(window, cx);
        self.browse_bar.rows.push(DraftRow {
            column,
            op: FilterOp::Equals,
            kind,
            input,
        });
        cx.notify();
    }

    fn remove_filter_row(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.browse_bar.rows.len() {
            self.browse_bar.rows.remove(index);
            cx.notify();
        }
    }

    pub(crate) fn set_filter_column(&mut self, index: usize, column: &str, cx: &mut Context<Self>) {
        let kind = self
            .browse_bar
            .columns
            .iter()
            .find(|(name, _)| name == column)
            .map(|(_, kind)| *kind);
        let (Some(kind), Some(row)) = (kind, self.browse_bar.rows.get_mut(index)) else {
            return;
        };
        row.column = column.to_string();
        row.kind = kind;
        if !row.op.applies_to(kind) {
            row.op = FilterOp::Equals;
        }
        cx.notify();
    }

    pub(crate) fn set_filter_op(&mut self, index: usize, op: FilterOp, cx: &mut Context<Self>) {
        if let Some(row) = self.browse_bar.rows.get_mut(index) {
            row.op = op;
            cx.notify();
        }
    }

    /// Reads the table again with the filters in the bar.
    pub(crate) fn apply_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut browse) = self.current_browse().cloned() else {
            return;
        };
        browse.filters = self.browse_bar.filters(cx);
        self.run_browse(browse, window, cx);
    }

    pub(crate) fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The bar is emptied below, before any read is sent: asking after that
        // would leave a bar saying "no filter" over a page that still has one.
        if self.has_unsaved_edits(cx) {
            self.guard_unsaved(window, cx, |this, window, cx| {
                this.clear_filters(window, cx)
            });
            return;
        }
        let Some(mut browse) = self.current_browse().cloned() else {
            return;
        };
        browse.filters.clear();
        self.browse_bar.rows.clear();
        self.browse_bar.expanded = false;
        self.run_browse(browse, window, cx);
    }

    /// `column = value` from a cell, or `IS NULL` from a null, added to what
    /// the bar already holds.
    pub(crate) fn filter_by_value(
        &mut self,
        row: usize,
        column: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.has_unsaved_edits(cx) {
            self.guard_unsaved(window, cx, move |this, window, cx| {
                this.filter_by_value(row, column, window, cx)
            });
            return;
        }
        let QueryState::Done(page) = &self.query else {
            return;
        };
        let Some(mut browse) = page.browse.clone() else {
            return;
        };
        let Some(name) = page.result.columns.get(column).map(|c| c.name.clone()) else {
            return;
        };
        let Some(cell) = page.result.rows.get(row).and_then(|row| row.get(column)) else {
            return;
        };
        let Some(filter) = filter_for_cell(name, cell) else {
            return;
        };
        self.note_filter_columns();

        let mut filters = self.browse_bar.filters(cx);
        if !filters.contains(&filter) {
            filters.push(filter);
        }
        self.browse_bar.rebuild(&filters, window, cx);
        self.browse_bar.expanded = true;
        browse.filters = filters;
        self.run_browse(browse, window, cx);
    }

    fn open_filter_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
        build: impl FnOnce(ContextMenu, &WeakEntity<DatabasePanel>) -> ContextMenu + 'static,
    ) {
        self.note_filter_columns();
        let panel = cx.weak_entity();
        self.open_context_menu(position, window, cx, move |menu, _window, _cx| {
            build(menu, &panel)
        });
    }

    pub(crate) fn render_filter_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.can_filter() || !(self.browse_bar.expanded || self.has_active_filters()) {
            return None;
        }
        let mut rows = Vec::with_capacity(self.browse_bar.rows.len());
        for (index, row) in self.browse_bar.rows.iter().enumerate() {
            rows.push(self.render_filter_row(index, row, cx));
        }

        Some(
            v_flex()
                .flex_none()
                .w_full()
                .gap_1()
                .p_1()
                .border_b_1()
                .border_color(cx.theme().colors().border)
                .debug_selector(|| "database-filter-bar".into())
                .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| {
                    this.apply_filters(window, cx);
                }))
                .children(rows)
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("database-filter-add", "Add Filter")
                                .size(ButtonSize::Compact)
                                .on_click(cx.listener(|this, _event, window, cx| {
                                    this.add_filter_row(window, cx);
                                })),
                        )
                        .child(div().flex_1())
                        .child(
                            Button::new("database-filter-clear", "Clear")
                                .size(ButtonSize::Compact)
                                .on_click(cx.listener(|this, _event, window, cx| {
                                    this.clear_filters(window, cx);
                                })),
                        )
                        .child(
                            Button::new("database-filter-apply", "Apply")
                                .size(ButtonSize::Compact)
                                .style(ButtonStyle::Filled)
                                .tooltip(|_window, cx| Tooltip::simple("Apply (Enter)", cx))
                                .on_click(cx.listener(|this, _event, window, cx| {
                                    this.apply_filters(window, cx);
                                })),
                        ),
                )
                .into_any_element(),
        )
    }

    fn render_filter_row(
        &self,
        index: usize,
        row: &DraftRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let op = row.op;
        h_flex()
            .gap_1()
            .child(
                Button::new(("database-filter-column", index), row.column.clone())
                    .size(ButtonSize::Compact)
                    .truncate(true)
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        let names: Vec<String> = this
                            .browse_bar
                            .columns
                            .iter()
                            .map(|(name, _)| name.clone())
                            .collect();
                        this.open_filter_menu(event.position(), window, cx, move |menu, panel| {
                            names.into_iter().fold(menu, |menu, name| {
                                let panel = panel.clone();
                                let label = name.clone();
                                menu.entry(label, None, move |_window, cx| {
                                    panel
                                        .update(cx, |panel, cx| {
                                            panel.set_filter_column(index, &name, cx)
                                        })
                                        .ok();
                                })
                            })
                        });
                    })),
            )
            .child(
                Button::new(("database-filter-op", index), op.label())
                    .size(ButtonSize::Compact)
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        let kind = this.browse_bar.rows.get(index).map(|row| row.kind);
                        this.open_filter_menu(event.position(), window, cx, move |menu, panel| {
                            FilterOp::ALL
                                .into_iter()
                                .filter(|op| kind.is_none_or(|kind| op.applies_to(kind)))
                                .fold(menu, |menu, op| {
                                    let panel = panel.clone();
                                    menu.entry(op.label(), None, move |_window, cx| {
                                        panel
                                            .update(cx, |panel, cx| {
                                                panel.set_filter_op(index, op, cx)
                                            })
                                            .ok();
                                    })
                                })
                        });
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .when(op.needs_value(), |value| value.child(row.input.clone())),
            )
            .child(
                IconButton::new(("database-filter-remove", index), IconName::Close)
                    .icon_size(IconSize::Small)
                    .tooltip(|_window, cx| Tooltip::simple("Remove Filter", cx))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.remove_filter_row(index, cx);
                    })),
            )
            .into_any_element()
    }
}

/// What "filter by this value" asks of a cell. A blank text is not offered:
/// the bar skips rows with nothing typed, so the filter could not survive the
/// next time it is applied. A blob has no value to compare.
pub(crate) fn filter_for_cell(column: String, cell: &Cell) -> Option<Filter> {
    let kind = ValueKind::of(cell);
    let (op, value) = match cell {
        Cell::Null => (FilterOp::IsNull, String::new()),
        Cell::Binary { .. } => return None,
        cell => {
            let value = crate::query::cell_text(cell).to_string();
            if value.trim().is_empty() {
                return None;
            }
            (FilterOp::Equals, value)
        }
    };
    Some(Filter {
        column,
        op,
        value,
        kind: kind.unwrap_or(ValueKind::Text),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::protocol::ResultColumn;

    fn result(rows: Vec<Vec<Cell>>) -> ResultSet {
        ResultSet {
            columns: vec![
                ResultColumn {
                    name: "id".into(),
                    type_name: "int".into(),
                },
                ResultColumn {
                    name: "note".into(),
                    type_name: "text".into(),
                },
            ],
            rows,
            truncated: false,
            elapsed_ms: 0,
        }
    }

    #[test]
    fn a_column_is_typed_by_its_first_non_null_cell() {
        let mut bar = BrowseFilterBar::default();
        bar.note_columns(&result(vec![
            vec![Cell::Null, Cell::Text { value: "a".into() }],
            vec![Cell::Number { value: "1".into() }, Cell::Null],
        ]));
        assert_eq!(
            bar.columns,
            vec![
                ("id".to_string(), ValueKind::Number),
                ("note".to_string(), ValueKind::Text)
            ]
        );
    }

    /// A filter that matches nothing leaves a page with no rows; the columns
    /// must not forget they were numbers.
    #[test]
    fn an_empty_page_keeps_what_the_columns_were_known_to_hold() {
        let mut bar = BrowseFilterBar::default();
        bar.note_columns(&result(vec![vec![
            Cell::Number { value: "1".into() },
            Cell::Bool { value: true },
        ]]));
        bar.note_columns(&result(Vec::new()));
        assert_eq!(bar.columns[0].1, ValueKind::Number);
        assert_eq!(bar.columns[1].1, ValueKind::Bool);
    }

    #[test]
    fn a_cell_becomes_an_equals_filter_typed_by_the_cell_and_a_null_a_null_check() {
        let number = filter_for_cell("id".into(), &Cell::Number { value: "7".into() });
        assert_eq!(
            number,
            Some(Filter {
                column: "id".into(),
                op: FilterOp::Equals,
                value: "7".into(),
                kind: ValueKind::Number,
            })
        );
        assert_eq!(
            filter_for_cell("note".into(), &Cell::Null).map(|filter| (filter.op, filter.value)),
            Some((FilterOp::IsNull, String::new()))
        );
        assert_eq!(
            filter_for_cell("b".into(), &Cell::Binary { byte_len: 3 }),
            None
        );
        assert_eq!(
            filter_for_cell("note".into(), &Cell::Text { value: "  ".into() }),
            None
        );
    }
}
