//! Rows of a result as text, in the formats people paste them into.
//!
//! Pure: a result, the indices of the rows wanted and, for `INSERT`, a
//! dialect. Copying a selection and exporting a file are meant to share these,
//! so nothing here knows about the clipboard.

use crate::query::cell_text;
use crate::sql_dialect::{Dialect, Feature};
use database::protocol::{Cell, ResultSet};
use serde_json::Value;
use std::collections::HashSet;

/// A way to write rows out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CopyFormat {
    Tsv,
    Csv,
    Json,
    Insert,
}

impl CopyFormat {
    pub(crate) fn icon(self) -> ui::IconName {
        match self {
            CopyFormat::Tsv => ui::IconName::Table,
            CopyFormat::Csv => ui::IconName::FileTextOutlined,
            CopyFormat::Json => ui::IconName::Json,
            CopyFormat::Insert => ui::IconName::Code,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            CopyFormat::Tsv => "TSV",
            CopyFormat::Csv => "CSV",
            CopyFormat::Json => "JSON",
            CopyFormat::Insert => "SQL INSERT",
        }
    }
}

/// The formats the copy menu offers for rows from `dialect`'s engine. `INSERT`
/// needs one that writes rows, so it is absent for a document store and when
/// the connection is not known.
pub(crate) fn offered_formats(dialect: Option<&Dialect>) -> Vec<CopyFormat> {
    let mut formats = vec![CopyFormat::Tsv, CopyFormat::Csv, CopyFormat::Json];
    if dialect.is_some_and(|dialect| dialect.supports(Feature::WriteTemplates)) {
        formats.push(CopyFormat::Insert);
    }
    formats
}

/// Every row of the result, in order.
pub(crate) fn every_row(result: &ResultSet) -> Vec<usize> {
    (0..result.rows.len()).collect()
}

/// The rows wanted, skipping any index past the end: a selection can outlive
/// the page it was made on for as long as one frame.
fn wanted<'a>(result: &'a ResultSet, rows: &'a [usize]) -> impl Iterator<Item = &'a [Cell]> {
    rows.iter()
        .filter_map(|&index| result.rows.get(index))
        .map(Vec::as_slice)
}

/// How many of the cells in `rows` are binary. Their bytes never reached this
/// crate, so every writer here puts a note where the value was.
pub(crate) fn binary_cells(result: &ResultSet, rows: &[usize]) -> u64 {
    wanted(result, rows)
        .flatten()
        .filter(|cell| matches!(cell, Cell::Binary { .. }))
        .count() as u64
}

/// Tab-separated, one line per row and no header, so three selected rows paste
/// as three spreadsheet rows.
///
/// A value holding a tab, a line break or a quote is quoted the way a
/// spreadsheet reads it back. A null is an empty field and the empty string is
/// `""`, as in [`to_csv`].
pub(crate) fn to_tsv(result: &ResultSet, rows: &[usize]) -> String {
    let mut text = String::new();
    for row in wanted(result, rows) {
        text.push_str(&fields(row, &['\t', '"', '\n', '\r']).join("\t"));
        text.push('\n');
    }
    text
}

/// A row's cells as the fields of one line.
///
/// A null is an empty field, except where it is the only cell: a line with
/// nothing on it is no row at all to a reader, so the one-column result's null
/// is written `""`, which is also how it writes the empty string. That is the
/// one place a delimited file cannot tell the two apart.
fn fields(row: &[Cell], special: &[char]) -> Vec<String> {
    if let [Cell::Null] = row {
        return vec!["\"\"".to_string()];
    }
    row.iter()
        .map(|cell| match cell {
            Cell::Null => String::new(),
            cell => quote(&cell_text(cell), special),
        })
        .collect()
}

/// The rows as CSV, RFC 4180, with a header line.
///
/// A null becomes an *empty unquoted field*, which is the closest CSV has to
/// "absent" -- an empty string becomes `""`, so the two stay distinguishable to
/// anything that reads quoting properly.
pub(crate) fn to_csv(result: &ResultSet, rows: &[usize]) -> String {
    csv_header(result) + &csv_rows(result, rows)
}

/// The column names as one CSV line.
pub(crate) fn csv_header(result: &ResultSet) -> String {
    let header: Vec<String> = result
        .columns
        .iter()
        .map(|column| quote(&column.name, &[',', '"', '\n', '\r']))
        .collect();
    format!("{}\r\n", header.join(","))
}

/// The rows as CSV lines, with no header.
pub(crate) fn csv_rows(result: &ResultSet, rows: &[usize]) -> String {
    let mut csv = String::new();
    for row in wanted(result, rows) {
        csv.push_str(&fields(row, &[',', '"', '\n', '\r']).join(","));
        csv.push_str("\r\n");
    }
    csv
}

/// Quotes a field only when it has to be, which keeps the common case readable.
///
/// The empty string *has to be*: an unquoted empty field is how these writers
/// spell a null, so leaving `""` bare would collapse the one distinction a
/// delimited file can carry between "absent" and "empty".
fn quote(field: &str, special: &[char]) -> String {
    if field.is_empty() || field.contains(special) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// An array of objects, one per row, keyed by column name.
///
/// Numbers and embedded JSON keep the text the driver gave, so a `numeric`
/// with thirty digits is not rounded through a float on the way out. A value
/// that is not valid JSON of its kind (`NaN`, a document that does not parse) is
/// written as a string rather than breaking the whole array.
pub(crate) fn to_json(result: &ResultSet, rows: &[usize]) -> String {
    let objects = json_objects(result, rows);
    if objects.is_empty() {
        return "[]".into();
    }
    format!("[\n{}\n]", objects.join(",\n"))
}

/// One indented object per row, as [`to_json`] writes them between the
/// brackets.
pub(crate) fn json_objects(result: &ResultSet, rows: &[usize]) -> Vec<String> {
    let keys = unique_keys(result);
    wanted(result, rows)
        .map(|row| {
            let fields: Vec<String> = keys
                .iter()
                .zip(row)
                .map(|(key, cell)| format!("{}: {}", json_string(key), json_value(cell)))
                .collect();
            format!("  {{{}}}", fields.join(", "))
        })
        .collect()
}

/// The column names as object keys. A second `id` (a join's two tables both
/// have one) would be a repeated key, which a reader keeps only one of, so a
/// repeat is suffixed: `id`, `id_2`, `id_3`.
fn unique_keys(result: &ResultSet) -> Vec<String> {
    let mut used: HashSet<String> = HashSet::new();
    result
        .columns
        .iter()
        .map(|column| {
            let mut key = column.name.clone();
            let mut attempt = 1;
            while !used.insert(key.clone()) {
                attempt += 1;
                key = format!("{}_{attempt}", column.name);
            }
            key
        })
        .collect()
}

fn json_string(text: &str) -> String {
    Value::String(text.to_string()).to_string()
}

fn json_value(cell: &Cell) -> String {
    match cell {
        Cell::Null => "null".into(),
        Cell::Bool { value } => value.to_string(),
        Cell::Number { value } if serde_json::from_str::<serde_json::Number>(value).is_ok() => {
            value.clone()
        }
        Cell::Json { value } if serde_json::from_str::<Value>(value).is_ok() => value.clone(),
        cell => json_string(&cell_text(cell)),
    }
}

/// One `INSERT` per row, for the engine `dialect` speaks.
///
/// `table` is `(schema, name)` of the table the rows came from, when they came
/// from one. Without it the statement names `<table>`, a placeholder no engine
/// parses, so running it unedited fails rather than writing into a table it
/// guessed. `None` for a document store, which has no `INSERT`, and for a
/// result with nothing to write.
pub(crate) fn to_insert(
    dialect: &Dialect,
    table: Option<(&str, &str)>,
    result: &ResultSet,
    rows: &[usize],
) -> Option<String> {
    let statements = insert_statements(dialect, table, result, rows)?;
    (!statements.is_empty()).then(|| statements.join("\n"))
}

/// The statements behind [`to_insert`], one per row. `None` where `INSERT`
/// cannot be written at all; `Some` of nothing when there are no rows to write.
pub(crate) fn insert_statements(
    dialect: &Dialect,
    table: Option<(&str, &str)>,
    result: &ResultSet,
    rows: &[usize],
) -> Option<Vec<String>> {
    if !dialect.supports(Feature::WriteTemplates) || result.columns.is_empty() {
        return None;
    }
    let target = table.map_or_else(
        || "<table>".to_string(),
        |(schema, name)| dialect.qualified(schema, name),
    );
    let names = result
        .columns
        .iter()
        .map(|column| dialect.quote_ident(&column.name))
        .collect::<Vec<_>>()
        .join(", ");

    Some(
        wanted(result, rows)
            .map(|row| {
                let values = row
                    .iter()
                    .map(|cell| sql_value(dialect, cell))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("INSERT INTO {target} ({names}) VALUES ({values});")
            })
            .collect(),
    )
}

/// A cell as a SQL literal.
///
/// A number written the way every engine reads one stays unquoted; any other
/// number text (`NaN`, `Infinity`, a driver's own format) is quoted rather than
/// trusted to parse. A blob's bytes never reached this crate, so it is `NULL`
/// with a note: guessing would write a row that looks complete and is not.
fn sql_value(dialect: &Dialect, cell: &Cell) -> String {
    match cell {
        Cell::Null => dialect.literal(None),
        Cell::Bool { value } => value.to_string(),
        Cell::Number { value } if serde_json::from_str::<serde_json::Number>(value).is_ok() => {
            value.clone()
        }
        Cell::Binary { byte_len } => {
            format!(
                "{} /* {byte_len} bytes not copied */",
                dialect.literal(None)
            )
        }
        cell => dialect.literal(Some(&cell_text(cell))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::protocol::{Capabilities, ResultColumn};

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

    fn number(value: &str) -> Cell {
        Cell::Number {
            value: value.into(),
        }
    }

    fn text(value: &str) -> Cell {
        Cell::Text {
            value: value.into(),
        }
    }

    fn sqlite() -> Dialect {
        Dialect::detect("sqlite", &Capabilities::default())
    }

    fn mongodb() -> Dialect {
        Dialect::detect(
            "mongodb",
            &Capabilities {
                read_table_template: Some(r#"{"find": {table}, "$db": {schema}}"#.into()),
                ..Default::default()
            },
        )
    }

    /// A comma, a quote and a newline are exactly what breaks a hand-rolled CSV
    /// writer, and exactly what turns up in a `note` column.
    #[test]
    fn csv_quotes_only_the_fields_that_need_it() {
        let result = result(vec![vec![number("1"), text("a,b \"c\"\nd")]]);
        assert_eq!(
            to_csv(&result, &[0]),
            "id,note\r\n1,\"a,b \"\"c\"\"\nd\"\r\n"
        );
    }

    /// The one distinction CSV can just about carry: an absent value is an
    /// empty field, an empty string is a quoted empty field.
    #[test]
    fn csv_keeps_null_and_the_empty_string_apart() {
        let result = result(vec![
            vec![number("1"), Cell::Null],
            vec![number("2"), text("")],
        ]);
        let csv = to_csv(&result, &every_row(&result));
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[1], "1,");
        assert_eq!(lines[2], "2,\"\"");
    }

    #[test]
    fn csv_carries_only_the_rows_asked_for_in_the_order_given() {
        let result = result(vec![
            vec![number("1"), text("a")],
            vec![number("2"), text("b")],
            vec![number("3"), text("c")],
        ]);
        assert_eq!(to_csv(&result, &[2, 0]), "id,note\r\n3,c\r\n1,a\r\n");
        assert_eq!(to_csv(&result, &[]), "id,note\r\n");
        assert_eq!(
            to_csv(&result, &[1, 9]),
            "id,note\r\n2,b\r\n",
            "an index past the end is skipped, not indexed"
        );
    }

    #[test]
    fn tsv_is_one_line_per_row_with_no_header() {
        let result = result(vec![
            vec![number("1"), text("first")],
            vec![number("2"), text("second")],
            vec![number("3"), text("third")],
        ]);
        assert_eq!(
            to_tsv(&result, &[0, 1, 2]),
            "1\tfirst\n2\tsecond\n3\tthird\n"
        );
    }

    /// An unquoted tab or newline moves the rest of the value into the next
    /// cell or row of the spreadsheet it is pasted into.
    #[test]
    fn tsv_quotes_what_would_break_a_cell_or_a_row() {
        let result = result(vec![
            vec![number("1"), text("a\tb")],
            vec![number("2"), text("line\nbreak")],
            vec![number("3"), text("say \"hi\"")],
            vec![number("4"), text("héllo ✓ 日本")],
        ]);
        assert_eq!(
            to_tsv(&result, &every_row(&result)),
            "1\t\"a\tb\"\n2\t\"line\nbreak\"\n3\t\"say \"\"hi\"\"\"\n4\théllo ✓ 日本\n"
        );
    }

    #[test]
    fn tsv_keeps_null_and_the_empty_string_apart() {
        let result = result(vec![
            vec![number("1"), Cell::Null],
            vec![number("2"), text("")],
        ]);
        assert_eq!(to_tsv(&result, &[0, 1]), "1\t\n2\t\"\"\n");
    }

    #[test]
    fn json_is_an_array_of_objects_that_parses_back() {
        let result = result(vec![
            vec![number("1"), text("a \"quoted\"\n\tvalue \u{1}")],
            vec![number("2"), Cell::Null],
        ]);
        let json = to_json(&result, &[0, 1]);
        let parsed: Value = serde_json::from_str(&json).expect("the output is valid JSON");
        assert_eq!(parsed[0]["id"], 1);
        assert_eq!(parsed[0]["note"], "a \"quoted\"\n\tvalue \u{1}");
        assert_eq!(parsed[1]["note"], Value::Null);
        assert_eq!(parsed.as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn json_keeps_each_kind_of_cell_as_what_it_is() {
        let result = ResultSet {
            columns: ["n", "big", "nan", "flag", "doc", "bad", "blob", "when"]
                .map(|name| ResultColumn {
                    name: name.into(),
                    type_name: String::new(),
                })
                .to_vec(),
            rows: vec![vec![
                number("1.50"),
                number("123456789012345678901234567890"),
                number("NaN"),
                Cell::Bool { value: true },
                Cell::Json {
                    value: r#"{"a": [1, 2]}"#.into(),
                },
                Cell::Json {
                    value: "{not json".into(),
                },
                Cell::Binary { byte_len: 4 },
                Cell::Timestamp {
                    value: "2026-01-02 03:04:05".into(),
                },
            ]],
            truncated: false,
            elapsed_ms: 0,
        };
        let json = to_json(&result, &[0]);
        assert!(json.contains("\"big\": 123456789012345678901234567890"));
        let parsed: Value = serde_json::from_str(&json).expect("valid JSON");
        let row = &parsed[0];
        assert!(
            json.contains("\"n\": 1.50"),
            "the driver's text is kept: {json}"
        );
        assert_eq!(row["nan"], "NaN", "not a JSON number, so a string");
        assert_eq!(row["flag"], true);
        assert_eq!(row["doc"]["a"][1], 2);
        assert_eq!(row["bad"], "{not json");
        assert_eq!(row["blob"], "<4 bytes>");
        assert_eq!(row["when"], "2026-01-02 03:04:05");
    }

    #[test]
    fn json_of_no_rows_is_an_empty_array() {
        assert_eq!(to_json(&result(Vec::new()), &[]), "[]");
    }

    #[test]
    fn insert_writes_one_statement_per_row_for_the_table_the_rows_came_from() {
        let result = result(vec![
            vec![number("1"), text("it's")],
            vec![number("2"), Cell::Null],
        ]);
        assert_eq!(
            to_insert(&sqlite(), Some(("main", "notes")), &result, &[0, 1]),
            Some(
                "INSERT INTO \"main\".\"notes\" (\"id\", \"note\") VALUES (1, 'it''s');\n\
                 INSERT INTO \"main\".\"notes\" (\"id\", \"note\") VALUES (2, NULL);"
                    .to_string()
            )
        );
    }

    #[test]
    fn insert_without_a_table_names_a_placeholder_no_engine_parses() {
        let result = result(vec![vec![number("1"), text("a")]]);
        let sql = to_insert(&sqlite(), None, &result, &[0]).expect("a statement");
        assert!(sql.starts_with("INSERT INTO <table> ("), "{sql}");
    }

    #[test]
    fn insert_does_not_trust_odd_numbers_or_invent_blob_bytes() {
        let result = ResultSet {
            columns: ["a", "b", "c"]
                .map(|name| ResultColumn {
                    name: name.into(),
                    type_name: String::new(),
                })
                .to_vec(),
            rows: vec![vec![
                number("NaN"),
                Cell::Binary { byte_len: 3 },
                Cell::Bool { value: false },
            ]],
            truncated: false,
            elapsed_ms: 0,
        };
        let sql = to_insert(&sqlite(), None, &result, &[0]).expect("a statement");
        assert!(
            sql.contains("VALUES ('NaN', NULL /* 3 bytes not copied */, false);"),
            "{sql}"
        );
    }

    #[test]
    fn the_menu_offers_insert_only_where_the_engine_writes_rows() {
        use CopyFormat::*;
        assert_eq!(offered_formats(Some(&sqlite())), [Tsv, Csv, Json, Insert]);
        assert_eq!(offered_formats(Some(&mongodb())), [Tsv, Csv, Json]);
        assert_eq!(offered_formats(None), [Tsv, Csv, Json]);
    }

    #[test]
    fn insert_is_not_offered_for_a_document_store_or_for_nothing_to_write() {
        let result = result(vec![vec![number("1"), text("a")]]);
        assert_eq!(to_insert(&mongodb(), None, &result, &[0]), None);
        assert_eq!(to_insert(&sqlite(), None, &result, &[]), None);
    }

    fn one_column(rows: Vec<Cell>) -> ResultSet {
        ResultSet {
            columns: vec![ResultColumn {
                name: "note".into(),
                type_name: "text".into(),
            }],
            rows: rows.into_iter().map(|cell| vec![cell]).collect(),
            truncated: false,
            elapsed_ms: 0,
        }
    }

    /// A line with nothing on it is not a row to a CSV reader: it is skipped, and
    /// a one-column result loses every row whose value is null.
    #[test]
    fn a_null_alone_in_its_row_is_not_a_blank_line() {
        let result = one_column(vec![text("a"), Cell::Null, text("c")]);
        assert_eq!(to_csv(&result, &[0, 1, 2]), "note\r\na\r\n\"\"\r\nc\r\n");
        assert_eq!(to_tsv(&result, &[0, 1, 2]), "a\n\"\"\nc\n");
    }

    #[test]
    fn a_null_beside_other_cells_is_still_an_empty_field() {
        let result = result(vec![vec![Cell::Null, Cell::Null]]);
        assert_eq!(to_csv(&result, &[0]), "id,note\r\n,\r\n");
        assert_eq!(to_tsv(&result, &[0]), "\t\n");
    }

    #[test]
    fn duplicate_column_names_do_not_make_duplicate_json_keys() {
        let result = ResultSet {
            columns: ["id", "id", "name", "id", "id_2"]
                .map(|name| ResultColumn {
                    name: name.into(),
                    type_name: String::new(),
                })
                .to_vec(),
            rows: vec![vec![
                number("1"),
                number("2"),
                text("n"),
                number("3"),
                number("4"),
            ]],
            truncated: false,
            elapsed_ms: 0,
        };
        let json = to_json(&result, &[0]);
        let parsed: Value = serde_json::from_str(&json).expect("valid JSON");
        let object = parsed[0].as_object().expect("an object per row");
        assert_eq!(object.len(), 5, "no key was overwritten: {json}");
        assert_eq!(object["id"], 1);
        assert_eq!(object["id_2"], 2, "the first repeat is suffixed");
        assert_eq!(object["name"], "n");
        assert_eq!(object["id_3"], 3);
        assert_eq!(object["id_2_2"], 4, "a real `id_2` does not collide either");
    }

    #[test]
    fn binary_cells_are_counted_among_the_rows_asked_for_only() {
        let result = one_column(vec![
            Cell::Binary { byte_len: 3 },
            text("a"),
            Cell::Binary { byte_len: 9 },
        ]);
        assert_eq!(binary_cells(&result, &[0, 1, 2]), 2);
        assert_eq!(binary_cells(&result, &[1]), 0);
        assert_eq!(binary_cells(&result, &[0, 7]), 1, "an index past the end");
    }
}
