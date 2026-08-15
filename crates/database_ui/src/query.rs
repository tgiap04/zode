use database::client::DriverError;
use database::protocol::{Cell, ErrorCode, ResultSet};
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
}

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

    pub(crate) fn has_previous(&self) -> bool {
        self.offset > 0
    }

    pub(crate) fn has_next(&self) -> bool {
        self.result.truncated
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

/// The page as CSV, RFC 4180.
///
/// A null becomes an *empty unquoted field*, which is the closest CSV has to
/// "absent" -- an empty string becomes `""`, so the two stay distinguishable to
/// anything that reads quoting properly.
pub(crate) fn to_csv(result: &ResultSet) -> String {
    let mut csv = String::new();
    let mut row_text = Vec::with_capacity(result.columns.len());

    row_text.extend(result.columns.iter().map(|column| quote(&column.name)));
    csv.push_str(&row_text.join(","));
    csv.push_str("\r\n");

    for row in &result.rows {
        row_text.clear();
        row_text.extend(row.iter().map(|cell| match cell {
            Cell::Null => String::new(),
            cell => quote(&cell_text(cell)),
        }));
        csv.push_str(&row_text.join(","));
        csv.push_str("\r\n");
    }
    csv
}

/// Quotes a field only when it has to be, which keeps the common case readable.
///
/// The empty string *has to be*: an unquoted empty field is how this writer
/// spells a null, so leaving `""` bare would collapse the one distinction CSV
/// can carry between "absent" and "empty".
fn quote(field: &str) -> String {
    if field.is_empty() || field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
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

    /// A comma, a quote and a newline are exactly what breaks a hand-rolled CSV
    /// writer, and exactly what turns up in a `note` column.
    #[test]
    fn csv_quotes_only_the_fields_that_need_it() {
        let csv = to_csv(&result(vec![vec![
            Cell::Number { value: "1".into() },
            Cell::Text {
                value: "a,b \"c\"\nd".into(),
            },
        ]]));
        assert_eq!(csv, "id,note\r\n1,\"a,b \"\"c\"\"\nd\"\r\n");
    }

    /// The one distinction CSV can just about carry: an absent value is an
    /// empty field, an empty string is a quoted empty field.
    #[test]
    fn csv_keeps_null_and_the_empty_string_apart() {
        let csv = to_csv(&result(vec![
            vec![Cell::Number { value: "1".into() }, Cell::Null],
            vec![
                Cell::Number { value: "2".into() },
                Cell::Text {
                    value: String::new(),
                },
            ],
        ]));
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[1], "1,");
        assert_eq!(lines[2], "2,\"\"");
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
        };

        assert_eq!(page(0, 200, true).range_label(), "1–200 of more");
        assert_eq!(page(200, 50, false).range_label(), "201–250");
        assert_eq!(page(0, 0, false).range_label(), "no rows");
        assert!(!page(0, 200, true).has_previous());
        assert!(page(200, 50, false).has_previous());
        assert!(page(0, 200, true).has_next());
    }
}
