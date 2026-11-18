use crate::database_panel::ConnectionKey;
use crate::grid_editing::PageEdits;
use crate::grid_selection::GridSelection;
use crate::safe_mode::StatementKind;
use crate::session::SessionId;
use crate::sql_dialect::{Dialect, Feature, SortKey};
use database::client::DriverError;
use database::protocol::{Cell, ColumnDef, ErrorCode, ResultSet};
use gpui::SharedString;

/// What the result area is showing.
pub(crate) enum QueryState {
    /// Nothing has been run yet on this connection.
    Idle,
    Running {
        /// Names the request so `cancel` has something to point at.
        request_id: String,
        /// Set once the user has asked to stop, so the button can say so rather
        /// than looking like it did nothing.
        cancelling: bool,
    },
    Done(Page),
    Failed(QueryError),
}

pub(crate) struct Page {
    pub(crate) result: ResultSet,
    /// The statement this page came from, kept so paging can re-run it without
    /// depending on what is in the editor now.
    pub(crate) sql: String,
    pub(crate) offset: u64,
    pub(crate) limit: u32,
    /// The row the detail view shows, as an index into `result.rows`. Held by
    /// the page so that paging or running another statement starts with no row
    /// shown, rather than showing whatever now sits at the old index.
    pub(crate) selected_row: Option<usize>,
    /// The rows highlighted for copying, which a click may extend beyond the
    /// one row the detail view shows.
    pub(crate) selection: GridSelection,
    /// Set when the page is a whole table opened from the tree, as opposed to
    /// a statement somebody typed: only then is there a table to sort or
    /// filter by rewriting the read.
    pub(crate) browse: Option<TableBrowse>,
    /// The connection this page was read from, which is where its rows were
    /// written for -- not necessarily the active one by now.
    pub(crate) connection: Option<ConnectionKey>,
    /// Which read produced this page, and over which session.
    pub(crate) origin: PageOrigin,
    /// Whether the statement could change data. Decided once, when it was sent
    /// and confirmed, because turning the page sends it again.
    pub(crate) kind: StatementKind,
    /// Cell edits not yet saved, and the editor being typed into. Held by the
    /// page like the selection, so that reading another page starts clean --
    /// and so that nothing can replace a page that has edits without asking
    /// (see `grid_editing::guard_unsaved`).
    pub(crate) edits: PageEdits,
}

/// What a page was read by, for what must be sure it is still acting on that:
/// a name in the connection list can come to mean another database, and a menu
/// stays open over rows that have since been replaced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PageOrigin {
    /// The session the rows came from. `None` for a page nothing was read for.
    pub(crate) session: Option<SessionId>,
    /// The read's own number, which no other read of this panel shares.
    pub(crate) read: u64,
}

/// The table a browsing page is showing, and how its read is sorted and
/// filtered.
///
/// This is the state a re-run starts from: the statement is rebuilt from it
/// every time rather than patched, so a sort and a filter can never disagree
/// with what the header and the filter bar show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TableBrowse {
    /// Held by identity rather than by place: a sort or a filter is applied
    /// later, and the list may have moved by then.
    pub(crate) connection: ConnectionKey,
    pub(crate) schema: String,
    pub(crate) table: String,
    pub(crate) order: Vec<SortColumn>,
    pub(crate) filters: Vec<Filter>,
    /// The table's primary key, in the order the table lists its columns.
    /// Empty for a table without one, and for one not described yet.
    ///
    /// What a row is named by (see `row_identity`), and the order a page is
    /// read in when nobody chose one.
    pub(crate) key_columns: Vec<String>,
    /// What the table's columns are, as described: the types an edit is held
    /// to. Empty for a table not described yet, which cannot be edited.
    pub(crate) columns: Vec<ColumnDef>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SortColumn {
    pub(crate) column: String,
    pub(crate) descending: bool,
}

/// How a filter's value is written into a statement. Taken from the column's
/// cells rather than from the typed text, because `5` is a number in one
/// column and a name in another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ValueKind {
    Text,
    Number,
    Bool,
}

impl ValueKind {
    /// `None` for a null, which says nothing about its column.
    pub(crate) fn of(cell: &Cell) -> Option<Self> {
        match cell {
            Cell::Null => None,
            Cell::Number { .. } => Some(Self::Number),
            Cell::Bool { .. } => Some(Self::Bool),
            _ => Some(Self::Text),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FilterOp {
    Equals,
    NotEquals,
    Less,
    Greater,
    LessOrEqual,
    GreaterOrEqual,
    Like,
    In,
    IsNull,
    IsNotNull,
}

impl FilterOp {
    pub(crate) const ALL: [FilterOp; 10] = [
        Self::Equals,
        Self::NotEquals,
        Self::Less,
        Self::Greater,
        Self::LessOrEqual,
        Self::GreaterOrEqual,
        Self::Like,
        Self::In,
        Self::IsNull,
        Self::IsNotNull,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Equals => "=",
            Self::NotEquals => "!=",
            Self::Less => "<",
            Self::Greater => ">",
            Self::LessOrEqual => "<=",
            Self::GreaterOrEqual => ">=",
            Self::Like => "LIKE",
            Self::In => "IN",
            Self::IsNull => "IS NULL",
            Self::IsNotNull => "IS NOT NULL",
        }
    }

    pub(crate) fn needs_value(self) -> bool {
        !matches!(self, Self::IsNull | Self::IsNotNull)
    }

    /// `LIKE` is a text operator: against a number or a boolean most engines
    /// refuse it outright, so the filter bar does not offer it there.
    pub(crate) fn applies_to(self, kind: ValueKind) -> bool {
        self != Self::Like || kind == ValueKind::Text
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Filter {
    pub(crate) column: String,
    pub(crate) op: FilterOp,
    /// As typed. For `IN`, the comma-separated list.
    pub(crate) value: String,
    pub(crate) kind: ValueKind,
}

impl TableBrowse {
    pub(crate) fn new(connection: ConnectionKey, schema: &str, table: &str) -> Self {
        Self {
            connection,
            schema: schema.to_string(),
            table: table.to_string(),
            order: Vec::new(),
            filters: Vec::new(),
            key_columns: Vec::new(),
            columns: Vec::new(),
        }
    }

    pub(crate) fn with_columns(mut self, columns: &[ColumnDef]) -> Self {
        self.columns = columns.to_vec();
        self.key_columns = columns
            .iter()
            .filter(|column| column.primary_key)
            .map(|column| column.name.clone())
            .collect();
        self
    }

    /// The statement that reads this table in this order with these filters.
    /// `None` when the engine cannot express them.
    pub(crate) fn statement(&self, dialect: &Dialect) -> Option<String> {
        if !self.order.is_empty() && !dialect.supports(Feature::Sort)
            || !self.filters.is_empty() && !dialect.supports(Feature::Filter)
        {
            return None;
        }
        // Without an order, a server may hand rows back differently from one
        // page to the next, so a page turn repeats rows and skips others. The
        // key is the one order that is always total.
        let order: Vec<SortKey<'_>> = if self.order.is_empty() {
            self.key_columns
                .iter()
                .map(|column| SortKey {
                    column,
                    descending: false,
                })
                .collect()
        } else {
            self.order
                .iter()
                .map(|key| SortKey {
                    column: &key.column,
                    descending: key.descending,
                })
                .collect()
        };
        let filter = dialect.where_clause(&self.filters);
        dialect.select_page(
            &self.schema,
            &self.table,
            None,
            Some(&order),
            filter.as_deref(),
        )
    }

    /// Header click: none, then ascending, then descending, then none again.
    /// A click on another column starts that column ascending.
    pub(crate) fn cycle_sort(&mut self, column: &str) {
        let next = match self.order.first() {
            Some(key) if key.column == column && !key.descending => Some(true),
            Some(key) if key.column == column => None,
            _ => Some(false),
        };
        self.order = next
            .map(|descending| SortColumn {
                column: column.to_string(),
                descending,
            })
            .into_iter()
            .collect();
    }

    /// `Some(descending)` when `column` is the one the read is sorted by.
    pub(crate) fn sort_of(&self, column: &str) -> Option<bool> {
        self.order
            .first()
            .filter(|key| key.column == column)
            .map(|key| key.descending)
    }
}

/// What the pager says instead of "Next Page" over a page it will not turn.
pub(crate) const WRITE_PAGE_NOTE: &str = "This statement changes data, so turning the page would run it again. Run it again from the editor if that is what you want.";

impl Page {
    /// `1–200` / `201–250`, in ordinary counting rather than offsets.
    pub(crate) fn range_label(&self) -> String {
        if self.result.rows.is_empty() {
            return "no rows".into();
        }
        let first = self.offset + 1;
        let last = self.offset + self.result.rows.len() as u64;
        if self.result.truncated {
            format!("{first}–{last} of more")
        } else {
            format!("{first}–{last}")
        }
    }

    /// Whether a page turn may send the statement again. Not for one that
    /// changes data: it was confirmed once, and `UPDATE ... RETURNING` run for
    /// the next page is the same update, run twice.
    pub(crate) fn can_turn(&self) -> bool {
        self.kind == StatementKind::Read
    }

    pub(crate) fn has_previous(&self) -> bool {
        self.can_turn() && self.offset > 0
    }

    pub(crate) fn has_next(&self) -> bool {
        self.can_turn() && self.result.truncated
    }

    /// Shows row `index` in the detail view, or hides the view when that row is
    /// already the one shown. An index past the end of the page is ignored: the
    /// click may have landed on a page that has since been replaced.
    pub(crate) fn toggle_row(&mut self, index: usize) {
        if index >= self.result.rows.len() {
            return;
        }
        self.selected_row = if self.selected_row == Some(index) {
            None
        } else {
            Some(index)
        };
    }

    /// The row the detail view shows, with its index in the page.
    pub(crate) fn selected(&self) -> Option<(usize, &[Cell])> {
        let index = self.selected_row?;
        let row = self.result.rows.get(index)?;
        Some((index, row.as_slice()))
    }
}

pub(crate) struct QueryError {
    pub(crate) message: SharedString,
    /// Kept apart from the message so the UI can say "this column is read-only"
    /// rather than "check your SQL" -- a distinction the user cannot make from
    /// the driver's wording alone.
    pub(crate) read_only: bool,
}

impl QueryError {
    pub(crate) fn from_anyhow(error: &anyhow::Error) -> Self {
        let read_only = error
            .downcast_ref::<DriverError>()
            .is_some_and(|error| error.code() == ErrorCode::ReadOnly);
        Self {
            message: format!("{error:#}").into(),
            read_only,
        }
    }
}

/// One cell, as text.
///
/// Nulls are the only value that is *not* its own text: everything else was
/// already rendered by the driver, which is the only layer that knows what its
/// engine's types mean.
pub(crate) fn cell_text(cell: &Cell) -> SharedString {
    match cell {
        Cell::Null => "NULL".into(),
        Cell::Text { value } | Cell::Number { value } | Cell::Json { value } => {
            SharedString::from(value.clone())
        }
        Cell::Timestamp { value } => SharedString::from(value.clone()),
        Cell::Bool { value } => if *value { "true" } else { "false" }.into(),
        Cell::Binary { byte_len } => format!("<{byte_len} bytes>").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database_panel::NodeOrigin;
    use database::protocol::ResultColumn;

    fn key() -> ConnectionKey {
        ("db".into(), NodeOrigin::Personal)
    }

    fn result(rows: Vec<Vec<Cell>>) -> ResultSet {
        ResultSet {
            columns: vec![
                ResultColumn {
                    name: "id".into(),
                    type_name: "integer".into(),
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

    /// A blob's size, never its bytes -- the grid says the same thing the
    /// protocol does.
    #[test]
    fn a_blob_reads_as_its_size() {
        assert_eq!(
            cell_text(&Cell::Binary { byte_len: 4 }).as_ref(),
            "<4 bytes>"
        );
    }

    #[test]
    fn the_range_label_counts_from_one_and_says_when_there_is_more() {
        let page = |offset: u64, rows: usize, truncated: bool| Page {
            result: ResultSet {
                truncated,
                rows: vec![vec![Cell::Null; 2]; rows],
                ..result(Vec::new())
            },
            sql: String::new(),
            offset,
            limit: 200,
            selected_row: None,
            selection: GridSelection::default(),
            browse: None,
            connection: None,
            kind: StatementKind::Read,
            origin: Default::default(),
            edits: Default::default(),
        };

        assert_eq!(page(0, 200, true).range_label(), "1–200 of more");
        assert_eq!(page(200, 50, false).range_label(), "201–250");
        assert_eq!(page(0, 0, false).range_label(), "no rows");
        assert!(!page(0, 200, true).has_previous());
        assert!(page(200, 50, false).has_previous());
        assert!(page(0, 200, true).has_next());
    }

    /// Turning a page sends its statement again, so a page of one that changes
    /// data has no pages to turn to.
    #[test]
    fn a_page_of_a_write_offers_no_page_turn() {
        let page = |kind: StatementKind, offset: u64| Page {
            result: ResultSet {
                truncated: true,
                ..result(vec![vec![Cell::Null; 2]])
            },
            sql: String::new(),
            offset,
            limit: 1,
            selected_row: None,
            selection: GridSelection::default(),
            browse: None,
            connection: None,
            kind,
            origin: Default::default(),
            edits: Default::default(),
        };
        assert!(page(StatementKind::Read, 1).has_next());
        assert!(page(StatementKind::Read, 1).has_previous());
        assert!(!page(StatementKind::Write, 1).has_next());
        assert!(!page(StatementKind::Write, 1).has_previous());
        assert!(!page(StatementKind::Write, 1).can_turn());
    }

    fn two_row_page() -> Page {
        Page {
            result: ResultSet {
                rows: vec![
                    vec![Cell::Text {
                        value: "first".into(),
                    }],
                    vec![Cell::Null],
                ],
                ..result(Vec::new())
            },
            sql: String::new(),
            offset: 0,
            limit: 200,
            selected_row: None,
            selection: GridSelection::default(),
            browse: None,
            connection: None,
            kind: StatementKind::Read,
            origin: Default::default(),
            edits: Default::default(),
        }
    }

    #[test]
    fn clicking_a_row_shows_it_and_clicking_it_again_hides_it() {
        let mut page = two_row_page();
        assert!(page.selected().is_none(), "a new page shows no row");

        page.toggle_row(1);
        let (index, row) = page.selected().expect("the clicked row is shown");
        assert_eq!(index, 1);
        assert_eq!(row, &[Cell::Null]);

        page.toggle_row(0);
        assert_eq!(page.selected().map(|(index, _)| index), Some(0));

        page.toggle_row(0);
        assert!(page.selected().is_none());
    }

    /// A click can land on a row of the page that was just replaced; it must not
    /// select past the end of the new one.
    #[test]
    fn a_row_past_the_end_of_the_page_is_never_selected() {
        let mut page = two_row_page();
        page.toggle_row(2);
        assert!(page.selected().is_none());
    }

    #[test]
    fn a_header_click_cycles_ascending_descending_unsorted() {
        let mut browse = TableBrowse::new(key(), "s", "t");
        browse.cycle_sort("a");
        assert_eq!(browse.sort_of("a"), Some(false));
        browse.cycle_sort("a");
        assert_eq!(browse.sort_of("a"), Some(true));
        browse.cycle_sort("a");
        assert_eq!(browse.sort_of("a"), None);
        assert!(browse.order.is_empty());

        browse.cycle_sort("a");
        browse.cycle_sort("b");
        assert_eq!(browse.sort_of("a"), None, "one sort column at a time");
        assert_eq!(browse.sort_of("b"), Some(false));
    }

    #[test]
    fn the_statement_is_rebuilt_from_state_with_order_and_filters() {
        use crate::sql_dialect::Dialect;
        let dialect = Dialect::detect("sqlite", &Default::default());
        let mut browse = TableBrowse::new(key(), "main", "t");
        assert_eq!(
            browse.statement(&dialect),
            Some("SELECT *\nFROM \"main\".\"t\"".into())
        );
        browse.cycle_sort("n");
        browse.filters.push(Filter {
            column: "n".into(),
            op: FilterOp::Equals,
            value: "x".into(),
            kind: ValueKind::Text,
        });
        assert_eq!(
            browse.statement(&dialect),
            Some("SELECT *\nFROM \"main\".\"t\"\nWHERE \"n\" = 'x'\nORDER BY \"n\" ASC".into())
        );
    }

    /// A filter on an engine that cannot express it is no read at all, never a
    /// read that quietly ignores the filter.
    #[test]
    fn a_filter_the_engine_cannot_express_makes_no_statement() {
        use crate::sql_dialect::Dialect;
        let documents = Dialect::detect(
            "mongodb",
            &database::protocol::Capabilities {
                read_table_template: Some("{}".into()),
                ..Default::default()
            },
        );
        let mut browse = TableBrowse::new(key(), "d", "c");
        browse.filters.push(Filter {
            column: "a".into(),
            op: FilterOp::IsNull,
            value: String::new(),
            kind: ValueKind::Text,
        });
        assert_eq!(browse.statement(&documents), None);
    }

    #[test]
    fn like_is_a_text_operator_only() {
        assert!(FilterOp::Like.applies_to(ValueKind::Text));
        assert!(!FilterOp::Like.applies_to(ValueKind::Number));
        assert!(!FilterOp::Like.applies_to(ValueKind::Bool));
        assert!(FilterOp::Equals.applies_to(ValueKind::Number));
        assert!(!FilterOp::IsNull.needs_value());
    }
}
