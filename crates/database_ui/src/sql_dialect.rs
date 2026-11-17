//! The one place the panel turns an intent ("read this table, sorted by that
//! column") into a statement an engine accepts.
//!
//! Every statement the UI builds goes through here, so quoting and escaping
//! are written once and tested against the names that break them. A feature an
//! engine cannot express is answered with `None` -- and asked about beforehand
//! with [`Dialect::supports`] -- so a menu entry is hidden rather than shown
//! and failing.
//!
//! This is the one source file besides the driver registry allowed to name
//! engines. The quote character still comes from the driver's capabilities, so
//! MySQL running with `ANSI_QUOTES` or a driver that quotes differently is not
//! decided here.

use crate::query::{Filter, FilterOp, ValueKind};
use crate::statement_split::SplitRules;
use database::protocol::Capabilities;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Engine {
    Postgres,
    MySql,
    Sqlite,
    Mongo,
}

const MYSQL_USUAL: SplitRules = SplitRules {
    backslash_escapes: true,
    double_quoted_strings: true,
    hash_comments: true,
    dash_comment_needs_space: true,
    ..SplitRules::STANDARD
};

/// How much a column's declared type says about what it may hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TypeDiscipline {
    /// The server refuses a value that does not fit the declared type.
    Declared,
    /// The declared type is a hint: any column takes any value, so only the
    /// hints that name a number are worth holding a value to.
    Affinity,
    /// No column types at all; the kind of the value being replaced is the
    /// only thing there is to go by.
    Schemaless,
}

/// Something a menu or a button may want to offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Feature {
    Count,
    Sort,
    Filter,
    /// `INSERT` / `UPDATE` / `DELETE` templates.
    WriteTemplates,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SortKey<'a> {
    pub(crate) column: &'a str,
    pub(crate) descending: bool,
}

/// The predicate of a `WHERE`, written by [`Dialect::where_clause`] from typed
/// filters and by nothing else: no text anyone typed or pasted can be made into
/// one, so none reaches a statement as SQL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WhereClause(String);

impl WhereClause {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Dialect {
    engine: Engine,
    quote: String,
}

impl Dialect {
    /// Reads the engine from the driver's id, and the quote from what the
    /// driver said about itself.
    ///
    /// A driver this build has not heard of is handled as standard SQL, unless
    /// it says it does not read tables with a `SELECT` -- then it is treated
    /// as a document store, which is the one thing that claim means.
    pub(crate) fn detect(driver: &str, capabilities: &Capabilities) -> Self {
        let engine = match driver {
            "postgres" => Engine::Postgres,
            "mysql" => Engine::MySql,
            "sqlite" => Engine::Sqlite,
            "mongodb" => Engine::Mongo,
            _ if capabilities.read_table_template.is_some() => Engine::Mongo,
            _ => Engine::Postgres,
        };
        // An empty quote would make `replace` interleave it between every
        // character of a name.
        let quote = capabilities
            .identifier_quote
            .as_deref()
            .filter(|quote| !quote.is_empty())
            .unwrap_or("\"")
            .to_string();
        Self { engine, quote }
    }

    pub(crate) fn engine(&self) -> Engine {
        self.engine
    }

    pub(crate) fn supports(&self, feature: Feature) -> bool {
        match feature {
            // A `find` takes a `sort` document; its `filter` document is not
            // offered because JSON cannot say what a column holds -- an
            // ObjectId or a date shown as text would be compared as a string
            // and match nothing.
            Feature::Count | Feature::Sort => true,
            Feature::Filter | Feature::WriteTemplates => self.engine != Engine::Mongo,
        }
    }

    pub(crate) fn type_discipline(&self) -> TypeDiscipline {
        match self.engine {
            Engine::Postgres | Engine::MySql => TypeDiscipline::Declared,
            Engine::Sqlite => TypeDiscipline::Affinity,
            Engine::Mongo => TypeDiscipline::Schemaless,
        }
    }

    /// Whether the engine reads a word where a number or a date goes: `NaN` and
    /// `Infinity` for a float, `now` or `infinity` for a date, `yes` or `off`
    /// for a boolean. One that does not refuses them, so a check that took
    /// them would let through what the server then turns down.
    pub(crate) fn reads_word_values(&self) -> bool {
        self.engine == Engine::Postgres
    }

    /// How a buffer of this engine's statements is cut at `;`, `None` for a
    /// document store, whose whole buffer is one JSON document.
    ///
    /// The engine's usual reading, for finding the statement under a cursor.
    pub(crate) fn split_rules(&self) -> Option<SplitRules> {
        self.classification_rules().first().copied()
    }

    /// Every reading of its quotes the engine's server may be using, the usual
    /// one first. Empty for a document store.
    ///
    /// For a guard that must not be fooled by any of them. A setting the client
    /// cannot see moves where a string ends and so what is code: a backslash
    /// escapes in a MySQL string unless `NO_BACKSLASH_ESCAPES`, and `"` is a
    /// name under `ANSI_QUOTES`; a PostgreSQL string takes a backslash only
    /// when `standard_conforming_strings` is off. Its block comments nest
    /// whatever the settings, so every reading it is given nests them.
    pub(crate) fn classification_rules(&self) -> &'static [SplitRules] {
        match self.engine {
            Engine::Mongo => &[],
            Engine::Postgres => &[
                SplitRules {
                    dollar_quotes: true,
                    e_strings: true,
                    nested_comments: true,
                    ..SplitRules::STANDARD
                },
                SplitRules {
                    dollar_quotes: true,
                    e_strings: true,
                    backslash_escapes: true,
                    nested_comments: true,
                    ..SplitRules::STANDARD
                },
            ],
            // The same reading of a backslash that `literal` writes for.
            Engine::MySql => &[
                MYSQL_USUAL,
                SplitRules {
                    backslash_escapes: false,
                    ..MYSQL_USUAL
                },
                SplitRules {
                    double_quoted_strings: false,
                    ..MYSQL_USUAL
                },
            ],
            Engine::Sqlite => &[SplitRules {
                bracket_names: true,
                ..SplitRules::STANDARD
            }],
        }
    }

    /// A name as this engine wants it written, with any quote inside it
    /// doubled so a table called `"; drop table users; --` stays one name.
    pub(crate) fn quote_ident(&self, name: &str) -> String {
        if self.engine == Engine::Mongo {
            return json_string(name);
        }
        let quote = &self.quote;
        format!("{quote}{}{quote}", name.replace(quote, &quote.repeat(2)))
    }

    /// `schema.table`, each part quoted. Not a thing a document store has:
    /// its statements carry the two names themselves.
    pub(crate) fn qualified(&self, schema: &str, table: &str) -> String {
        format!("{}.{}", self.quote_ident(schema), self.quote_ident(table))
    }

    /// A table's full name as someone would paste it: quoted for a SQL engine,
    /// `database.collection` for a document store, which has no quoting to
    /// paste into.
    pub(crate) fn qualified_name(&self, schema: &str, table: &str) -> String {
        if self.engine == Engine::Mongo {
            format!("{schema}.{table}")
        } else {
            self.qualified(schema, table)
        }
    }

    /// A value as a literal, `None` being SQL `NULL`.
    ///
    /// For templates and filters built from cell values only: a value is never
    /// to be concatenated into a statement without passing through here.
    pub(crate) fn literal(&self, value: Option<&str>) -> String {
        let Some(value) = value else {
            return if self.engine == Engine::Mongo {
                "null".into()
            } else {
                "NULL".into()
            };
        };
        match self.engine {
            Engine::Mongo => json_string(value),
            // MySQL reads a backslash as an escape unless told not to, so a
            // value ending in one would swallow its own closing quote.
            Engine::MySql => format!(
                "'{}'",
                value
                    .replace('\\', "\\\\")
                    .replace('\0', "\\0")
                    .replace('\'', "''")
            ),
            // A plain string takes a backslash as an escape when
            // standard_conforming_strings is off, an `E` string always: a value
            // with one is written as an `E` string with the backslash doubled,
            // which is the same value either way.
            Engine::Postgres if value.contains('\\') => {
                format!("E'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
            }
            // Standard strings: a backslash is a backslash.
            Engine::Postgres | Engine::Sqlite => format!("'{}'", value.replace('\'', "''")),
        }
    }

    /// One read of a table, without a `LIMIT`: paging stays on the protocol.
    ///
    /// `filter` is a clause [`Self::where_clause`] wrote, so the only SQL in it
    /// is SQL this module generated from typed filters.
    ///
    /// `None` when the engine cannot express what was asked: a document store
    /// takes a sort but neither a projection nor a predicate.
    pub(crate) fn select_page(
        &self,
        schema: &str,
        table: &str,
        columns: Option<&[&str]>,
        order: Option<&[SortKey<'_>]>,
        filter: Option<&WhereClause>,
    ) -> Option<String> {
        let columns = columns.filter(|columns| !columns.is_empty());
        let order = order.filter(|order| !order.is_empty());

        if self.engine == Engine::Mongo {
            if columns.is_some() || filter.is_some() {
                return None;
            }
            let sort = order.map_or(String::new(), |order| {
                let keys = order
                    .iter()
                    .map(|key| {
                        let direction = if key.descending { -1 } else { 1 };
                        format!("{}: {direction}", self.quote_ident(key.column))
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(r#""sort": {{{keys}}}, "#)
            });
            return Some(format!(
                r#"{{"find": {}, {sort}"$db": {}}}"#,
                self.quote_ident(table),
                self.quote_ident(schema)
            ));
        }

        let projection = match columns {
            Some(columns) => columns
                .iter()
                .map(|column| self.quote_ident(column))
                .collect::<Vec<_>>()
                .join(", "),
            None => "*".to_string(),
        };
        let mut sql = format!(
            "SELECT {projection}\nFROM {}",
            self.qualified(schema, table)
        );
        if let Some(filter) = filter {
            sql.push_str("\nWHERE ");
            sql.push_str(filter.as_str());
        }
        if let Some(order) = order {
            let keys = order
                .iter()
                .map(|key| {
                    let direction = if key.descending { "DESC" } else { "ASC" };
                    format!("{} {direction}", self.quote_ident(key.column))
                })
                .collect::<Vec<_>>()
                .join(", ");
            sql.push_str("\nORDER BY ");
            sql.push_str(&keys);
        }
        Some(sql)
    }

    /// The predicates of `filters` joined with `AND`, or `None` when there is
    /// nothing to filter by or the engine cannot express it.
    ///
    /// Boxed so that `Option::as_deref` hands the clause to [`Self::select_page`]
    /// and [`Self::count`] as it is, which a caller holding the `Option` can
    /// then do without naming the type.
    pub(crate) fn where_clause(&self, filters: &[Filter]) -> Option<Box<WhereClause>> {
        if !self.supports(Feature::Filter) {
            return None;
        }
        let predicates: Vec<String> = filters
            .iter()
            .filter_map(|filter| self.predicate(filter))
            .collect();
        (!predicates.is_empty()).then(|| Box::new(WhereClause(predicates.join(" AND "))))
    }

    fn predicate(&self, filter: &Filter) -> Option<String> {
        let column = self.quote_ident(&filter.column);
        let operator = match filter.op {
            FilterOp::IsNull => return Some(format!("{column} IS NULL")),
            FilterOp::IsNotNull => return Some(format!("{column} IS NOT NULL")),
            FilterOp::In => {
                let items: Vec<String> = filter
                    .value
                    .split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(|item| self.typed_literal(filter.kind, item))
                    .collect();
                if items.is_empty() {
                    return None;
                }
                return Some(format!("{column} IN ({})", items.join(", ")));
            }
            FilterOp::Equals => "=",
            FilterOp::NotEquals => "<>",
            FilterOp::Less => "<",
            FilterOp::Greater => ">",
            FilterOp::LessOrEqual => "<=",
            FilterOp::GreaterOrEqual => ">=",
            FilterOp::Like => {
                return Some(format!(
                    "{column} LIKE {}",
                    self.typed_literal(ValueKind::Text, &filter.value)
                ));
            }
        };
        Some(format!(
            "{column} {operator} {}",
            self.typed_literal(filter.kind, &filter.value)
        ))
    }

    /// A value as the column it is compared with holds it. Bare only when it
    /// is exactly a number (or `true`/`false`) *and* the column is one: any
    /// other text, whatever it looks like, goes through [`Self::literal`].
    fn typed_literal(&self, kind: ValueKind, value: &str) -> String {
        match kind {
            ValueKind::Number if is_plain_number(value.trim()) => value.trim().to_string(),
            ValueKind::Bool => match value.trim().to_ascii_lowercase().as_str() {
                "true" => "TRUE".into(),
                "false" => "FALSE".into(),
                _ => self.literal(Some(value)),
            },
            _ => self.literal(Some(value)),
        }
    }

    /// How many rows a table has, or how many match `filter`.
    pub(crate) fn count(
        &self,
        schema: &str,
        table: &str,
        filter: Option<&WhereClause>,
    ) -> Option<String> {
        if self.engine == Engine::Mongo {
            if filter.is_some() {
                return None;
            }
            return Some(format!(
                r#"{{"aggregate": {}, "pipeline": [{{"$count": "count"}}], "$db": {}}}"#,
                self.quote_ident(table),
                self.quote_ident(schema)
            ));
        }
        let mut sql = format!("SELECT COUNT(*)\nFROM {}", self.qualified(schema, table));
        if let Some(filter) = filter {
            sql.push_str("\nWHERE ");
            sql.push_str(filter.as_str());
        }
        Some(sql)
    }

    /// A statement to fill in, not to run: the values are `<column>`
    /// placeholders, which no engine parses, so running one unedited fails
    /// instead of writing a row of nothing.
    pub(crate) fn insert_template(
        &self,
        schema: &str,
        table: &str,
        columns: &[&str],
    ) -> Option<String> {
        if !self.supports(Feature::WriteTemplates) || columns.is_empty() {
            return None;
        }
        let names = columns
            .iter()
            .map(|column| self.quote_ident(column))
            .collect::<Vec<_>>()
            .join(", ");
        let values = columns
            .iter()
            .map(|column| placeholder(column))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!(
            "INSERT INTO {} ({names})\nVALUES ({values});",
            self.qualified(schema, table)
        ))
    }

    /// `None` without key columns: an `UPDATE` with no `WHERE` rewrites the
    /// whole table, and a template is no place to offer that.
    pub(crate) fn update_template(
        &self,
        schema: &str,
        table: &str,
        columns: &[&str],
        key_columns: &[&str],
    ) -> Option<String> {
        if key_columns.is_empty() {
            return None;
        }
        self.update_with(
            schema,
            table,
            columns,
            format!("WHERE {};", self.key_predicate(key_columns)),
        )
    }

    /// A value as the save preview writes it: bare when it is a number going
    /// into a numeric cell, a quoted literal otherwise, `None` being `NULL`.
    ///
    /// For text a person reads and nothing else -- an edit is sent as a
    /// parameter, and no statement written here is ever run.
    pub(crate) fn preview_literal(&self, value: Option<&str>, numeric: bool) -> String {
        match value {
            Some(text) if numeric && is_plain_number(text) => text.to_string(),
            other => self.literal(other),
        }
    }

    /// What one edited row would be written as, display only: an `UPDATE` for
    /// a SQL engine, an `updateOne` document for a document store. The values
    /// are already literals, from [`Self::preview_literal`].
    pub(crate) fn update_preview(
        &self,
        schema: &str,
        table: &str,
        set: &[(&str, String)],
        key: &[(&str, String)],
    ) -> String {
        if self.engine == Engine::Mongo {
            let object = |pairs: &[(&str, String)]| {
                let fields = pairs
                    .iter()
                    .map(|(name, value)| format!("{}: {value}", json_string(name)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{{fields}}}")
            };
            return format!(
                "{{\"updateOne\": {{\"database\": {}, \"collection\": {}, \"filter\": {}, \"update\": {{\"$set\": {}}}}}}}",
                json_string(schema),
                json_string(table),
                object(key),
                object(set),
            );
        }
        let join = |pairs: &[(&str, String)], separator: &str| {
            pairs
                .iter()
                .map(|(name, value)| format!("{} = {value}", self.quote_ident(name)))
                .collect::<Vec<_>>()
                .join(separator)
        };
        format!(
            "UPDATE {}\nSET {}\nWHERE {};",
            self.qualified(schema, table),
            join(set, ", "),
            join(key, " AND "),
        )
    }

    /// `None` without key columns, for the same reason as an update.
    pub(crate) fn delete_template(
        &self,
        schema: &str,
        table: &str,
        key_columns: &[&str],
    ) -> Option<String> {
        if key_columns.is_empty() {
            return None;
        }
        self.delete_with(
            schema,
            table,
            format!("WHERE {};", self.key_predicate(key_columns)),
        )
    }

    /// An `UPDATE` for a table with no primary key. The `WHERE` is a comment
    /// to be written, never absent: with it gone the statement would rewrite
    /// every row the moment the placeholders were filled in.
    pub(crate) fn update_template_without_key(
        &self,
        schema: &str,
        table: &str,
        columns: &[&str],
    ) -> Option<String> {
        self.update_with(schema, table, columns, UNKEYED_WHERE.into())
    }

    /// See [`Self::update_template_without_key`].
    pub(crate) fn delete_template_without_key(&self, schema: &str, table: &str) -> Option<String> {
        self.delete_with(schema, table, UNKEYED_WHERE.into())
    }

    fn update_with(
        &self,
        schema: &str,
        table: &str,
        columns: &[&str],
        where_clause: String,
    ) -> Option<String> {
        if !self.supports(Feature::WriteTemplates) || columns.is_empty() {
            return None;
        }
        let assignments = columns
            .iter()
            .map(|column| format!("{} = {}", self.quote_ident(column), placeholder(column)))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!(
            "UPDATE {}\nSET {assignments}\n{where_clause}",
            self.qualified(schema, table)
        ))
    }

    fn delete_with(&self, schema: &str, table: &str, where_clause: String) -> Option<String> {
        if !self.supports(Feature::WriteTemplates) {
            return None;
        }
        Some(format!(
            "DELETE FROM {}\n{where_clause}",
            self.qualified(schema, table)
        ))
    }

    fn key_predicate(&self, key_columns: &[&str]) -> String {
        key_columns
            .iter()
            .map(|column| format!("{} = {}", self.quote_ident(column), placeholder(column)))
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

const UNKEYED_WHERE: &str = "-- WHERE <condition>;";

fn placeholder(column: &str) -> String {
    format!("<{column}>")
}

/// A JSON number: an optional minus, an integer part without leading zeros,
/// then an optional fraction and exponent. The strictest common ground of
/// every engine's numeric literal, which is what lets it be written bare.
fn is_plain_number(text: &str) -> bool {
    let bytes = text.as_bytes();
    let digits = |from: usize| {
        bytes
            .get(from..)
            .unwrap_or_default()
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count()
    };
    let mut at = usize::from(bytes.first() == Some(&b'-'));
    match digits(at) {
        0 => return false,
        1 => {}
        _ if bytes.get(at) == Some(&b'0') => return false,
        _ => {}
    }
    at += digits(at);
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        let fraction = digits(at);
        if fraction == 0 {
            return false;
        }
        at += fraction;
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(bytes.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let exponent = digits(at);
        if exponent == 0 {
            return false;
        }
        at += exponent;
    }
    at == bytes.len()
}

/// A JSON string, which is what a MongoDB command document quotes names and
/// values with.
fn json_string(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() + 2);
    encoded.push('"');
    for character in value.chars() {
        match character {
            '"' => encoded.push_str("\\\""),
            '\\' => encoded.push_str("\\\\"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                encoded.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => encoded.push(other),
        }
    }
    encoded.push('"');
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialect(driver: &str) -> Dialect {
        let capabilities = match driver {
            "mysql" => Capabilities {
                identifier_quote: Some("`".into()),
                ..Default::default()
            },
            "mongodb" => Capabilities {
                read_table_template: Some(r#"{"find": {table}, "$db": {schema}}"#.into()),
                ..Default::default()
            },
            _ => Capabilities::default(),
        };
        Dialect::detect(driver, &capabilities)
    }

    const SQL_DRIVERS: [&str; 3] = ["postgres", "mysql", "sqlite"];

    #[test]
    fn the_engine_comes_from_the_driver_id() {
        assert_eq!(dialect("postgres").engine(), Engine::Postgres);
        assert_eq!(dialect("mysql").engine(), Engine::MySql);
        assert_eq!(dialect("sqlite").engine(), Engine::Sqlite);
        assert_eq!(dialect("mongodb").engine(), Engine::Mongo);
    }

    #[test]
    fn an_unknown_driver_is_standard_sql_unless_it_reads_without_select() {
        let plain = Dialect::detect("duckdb", &Capabilities::default());
        assert_eq!(plain.engine(), Engine::Postgres);

        let documents = Dialect::detect(
            "couch",
            &Capabilities {
                read_table_template: Some("{}".into()),
                ..Default::default()
            },
        );
        assert_eq!(documents.engine(), Engine::Mongo);
    }

    #[test]
    fn only_the_engines_that_have_them_read_dollar_quotes_and_backslash_escapes() {
        let postgres = dialect("postgres")
            .split_rules()
            .expect("a SQL engine splits");
        assert!(postgres.dollar_quotes && !postgres.backslash_escapes);
        let mysql = dialect("mysql").split_rules().expect("a SQL engine splits");
        assert!(!mysql.dollar_quotes && mysql.backslash_escapes);
        assert_eq!(
            dialect("sqlite").split_rules(),
            Some(SplitRules {
                bracket_names: true,
                ..SplitRules::STANDARD
            })
        );
    }

    #[test]
    fn only_the_engine_that_nests_block_comments_reads_them_nested() {
        for reading in dialect("postgres").classification_rules() {
            assert!(reading.nested_comments, "{reading:?}");
        }
        for driver in ["mysql", "sqlite"] {
            for reading in dialect(driver).classification_rules() {
                assert!(!reading.nested_comments, "{driver}: {reading:?}");
            }
        }
    }

    /// The server nests the comment, so the quote after the first `*/` is inside
    /// it and the `DELETE` after the `;` is run. A reader that closes the comment
    /// at the first `*/` takes the rest for a string and sees a read.
    #[test]
    fn a_statement_hidden_behind_a_nested_comment_is_a_write() {
        use crate::safe_mode::{StatementKind, classify};
        let hidden = "SELECT 1 /* /* */ ' */ ; DELETE FROM t; --'";
        assert_eq!(classify(&dialect("postgres"), hidden), StatementKind::Write);
        // These engines end the comment at the first `*/`, which leaves the rest
        // of the line a string: there is no second statement.
        for driver in ["mysql", "sqlite"] {
            assert_eq!(
                classify(&dialect(driver), hidden),
                StatementKind::Read,
                "{driver}"
            );
        }
        // A nested comment is still a comment when nothing follows it.
        assert_eq!(
            classify(&dialect("postgres"), "SELECT 1 /* a /* b */ c */"),
            StatementKind::Read
        );
    }

    /// A dollar-quote tag may hold the bytes above ASCII a name may hold.
    #[test]
    fn a_statement_hidden_behind_a_tag_with_a_non_ascii_letter_is_a_write() {
        use crate::safe_mode::{StatementKind, classify};
        assert_eq!(
            classify(&dialect("postgres"), "SELECT $é$ ' $é$; DELETE FROM t; --'"),
            StatementKind::Write
        );
        assert_eq!(
            classify(&dialect("postgres"), "SELECT $é$ ; DELETE FROM t; $é$"),
            StatementKind::Read,
            "inside its own tag the DELETE is text"
        );
    }

    #[test]
    fn a_document_store_does_not_split() {
        let documents = Dialect::detect(
            "an-extension",
            &Capabilities {
                read_table_template: Some("{}".into()),
                ..Default::default()
            },
        );
        assert_eq!(documents.split_rules(), None);
    }

    #[test]
    fn a_quote_inside_a_name_is_doubled_whatever_the_quote_is() {
        assert_eq!(dialect("postgres").quote_ident(r#"a"b"#), r#""a""b""#);
        assert_eq!(dialect("sqlite").quote_ident(r#"a"b"#), r#""a""b""#);
        assert_eq!(dialect("mysql").quote_ident("a`b"), "`a``b`");
        // The other engine's quote is just a character here.
        assert_eq!(dialect("mysql").quote_ident(r#"a"b"#), "`a\"b`");
    }

    #[test]
    fn spaces_unicode_and_injection_attempts_stay_inside_the_quotes() {
        let postgres = dialect("postgres");
        assert_eq!(postgres.quote_ident("order items"), r#""order items""#);
        assert_eq!(postgres.quote_ident("tên_bảng"), r#""tên_bảng""#);
        assert_eq!(
            postgres.quote_ident(r#""; drop table users; --"#),
            r#""""; drop table users; --""#
        );
        assert_eq!(dialect("mysql").quote_ident("`; drop"), "```; drop`");
    }

    #[test]
    fn the_quote_is_whatever_the_driver_says_not_what_the_engine_usually_is() {
        let ansi = Dialect::detect(
            "mysql",
            &Capabilities {
                identifier_quote: Some("\"".into()),
                ..Default::default()
            },
        );
        assert_eq!(ansi.quote_ident("t"), r#""t""#);

        let empty = Dialect::detect(
            "mysql",
            &Capabilities {
                identifier_quote: Some(String::new()),
                ..Default::default()
            },
        );
        assert_eq!(
            empty.quote_ident("ab"),
            r#""ab""#,
            "an empty quote is ignored"
        );
    }

    #[test]
    fn literals_escape_quotes_and_say_null() {
        for driver in SQL_DRIVERS {
            let dialect = dialect(driver);
            assert_eq!(dialect.literal(None), "NULL");
            assert_eq!(dialect.literal(Some("o'brien")), "'o''brien'");
            assert_eq!(dialect.literal(Some("")), "''");
            assert_eq!(
                dialect.literal(Some("'; drop table t; --")),
                "'''; drop table t; --'"
            );
        }
    }

    #[test]
    fn mysql_reads_a_backslash_as_an_escape_and_the_others_do_not() {
        assert_eq!(dialect("mysql").literal(Some(r"a\")), r"'a\\'");
        assert_eq!(dialect("mysql").literal(Some("a\0b")), r"'a\0b'");
        assert_eq!(dialect("sqlite").literal(Some(r"a\")), r"'a\'");
    }

    /// PostgreSQL reads a backslash as an escape in a plain string when
    /// standard_conforming_strings is off, and in an `E` string always: a value
    /// with one is written as an `E` string with the backslash doubled, which
    /// is the same value under both settings.
    #[test]
    fn a_postgres_value_with_a_backslash_is_an_escape_string_with_it_doubled() {
        let postgres = dialect("postgres");
        assert_eq!(postgres.literal(Some(r"a\")), r"E'a\\'");
        assert_eq!(
            postgres.literal(Some(r"\' OR 1=1 -- ")),
            r"E'\\'' OR 1=1 -- '"
        );
        assert_eq!(postgres.literal(Some(r"a\b")), r"E'a\\b'");
        assert_eq!(
            postgres.literal(Some("no backslash")),
            "'no backslash'",
            "a value without one stays a plain string"
        );
        assert_eq!(dialect("sqlite").literal(Some(r"a\")), r"'a\'");
    }

    /// With standard_conforming_strings off the old `"a" = 'x\'` swallowed its
    /// quote and ran the next filter's value as SQL.
    #[test]
    fn two_filters_cannot_turn_one_backslash_into_a_statement() {
        assert_eq!(
            where_of(
                "postgres",
                &[
                    filter("a", FilterOp::Equals, r"x\", ValueKind::Text),
                    filter("b", FilterOp::Equals, "; DELETE FROM t --", ValueKind::Text),
                ]
            ),
            Some(r#""a" = E'x\\' AND "b" = '; DELETE FROM t --'"#.into())
        );
    }

    #[test]
    fn a_mongo_literal_is_json() {
        let mongo = dialect("mongodb");
        assert_eq!(mongo.literal(None), "null");
        assert_eq!(mongo.literal(Some("a\"b\\c\n")), r#""a\"b\\c\n""#);
        assert_eq!(mongo.literal(Some("\u{1}")), r#""\u0001""#);
    }

    #[test]
    fn a_plain_read_is_a_select_star_on_the_qualified_name() {
        assert_eq!(
            dialect("postgres").select_page("public", "users", None, None, None),
            Some("SELECT *\nFROM \"public\".\"users\"".into())
        );
        assert_eq!(
            dialect("mysql").select_page("app", "users", None, None, None),
            Some("SELECT *\nFROM `app`.`users`".into())
        );
        assert_eq!(
            dialect("sqlite").select_page("main", "t", Some(&[]), Some(&[]), None),
            Some("SELECT *\nFROM \"main\".\"t\"".into()),
            "empty lists ask for nothing"
        );
        assert!(
            clause_of(
                "sqlite",
                &[filter("a", FilterOp::In, " , ", ValueKind::Text)]
            )
            .is_none(),
            "a filter that says nothing makes no clause to pass"
        );
    }

    #[test]
    fn columns_filter_and_order_land_in_that_order() {
        let order = [
            SortKey {
                column: "name",
                descending: false,
            },
            SortKey {
                column: "created at",
                descending: true,
            },
        ];
        let filters = [
            filter("age", FilterOp::Greater, "3", ValueKind::Number),
            filter("note", FilterOp::Like, "%-- adults", ValueKind::Text),
        ];
        let clause = clause_of("postgres", &filters);
        assert_eq!(
            dialect("postgres").select_page(
                "public",
                "users",
                Some(&["id", "name"]),
                Some(&order),
                clause.as_deref(),
            ),
            Some(
                "SELECT \"id\", \"name\"\nFROM \"public\".\"users\"\n\
                 WHERE \"age\" > 3 AND \"note\" LIKE '%-- adults'\n\
                 ORDER BY \"name\" ASC, \"created at\" DESC"
                    .into()
            ),
            "the clauses sit on lines of their own, in this order"
        );
    }

    #[test]
    fn counting_is_a_count_star_with_the_same_filter() {
        assert_eq!(
            dialect("mysql").count("app", "users", None),
            Some("SELECT COUNT(*)\nFROM `app`.`users`".into())
        );
        let clause = clause_of(
            "sqlite",
            &[filter("active", FilterOp::Equals, "1", ValueKind::Number)],
        );
        assert_eq!(
            dialect("sqlite").count("main", "users", clause.as_deref()),
            Some("SELECT COUNT(*)\nFROM \"main\".\"users\"\nWHERE \"active\" = 1".into())
        );
    }

    #[test]
    fn a_collection_is_read_with_the_command_the_driver_itself_declares() {
        let capabilities = Capabilities {
            read_table_template: Some(r#"{"find": {table}, "$db": {schema}}"#.into()),
            ..Default::default()
        };
        let mongo = Dialect::detect("mongodb", &capabilities);
        let statement = mongo
            .select_page("app", "users", None, None, None)
            .expect("a whole-collection read");
        assert_eq!(statement, capabilities.read_table_statement("app", "users"));
        assert_eq!(statement, r#"{"find": "users", "$db": "app"}"#);

        assert_eq!(
            mongo.count("app", "users", None),
            Some(
                r#"{"aggregate": "users", "pipeline": [{"$count": "count"}], "$db": "app"}"#.into()
            )
        );
    }

    #[test]
    fn a_collection_name_with_a_quote_stays_one_json_string() {
        assert_eq!(
            dialect("mongodb").select_page("a\"b", "c\\d", None, None, None),
            Some(r#"{"find": "c\\d", "$db": "a\"b"}"#.into())
        );
    }

    #[test]
    fn what_a_document_store_cannot_express_is_none_and_unsupported() {
        let mongo = dialect("mongodb");
        let order = [SortKey {
            column: "a",
            descending: false,
        }];
        assert_eq!(mongo.select_page("d", "c", Some(&["a"]), None, None), None);
        assert_eq!(
            mongo.select_page("d", "c", None, Some(&order), None),
            Some(r#"{"find": "c", "sort": {"a": 1}, "$db": "d"}"#.into()),
            "a sort is a find option"
        );
        // No clause can come from a document store, but one from a SQL engine
        // is still refused by it.
        let clause = clause_of(
            "sqlite",
            &[filter("a", FilterOp::Equals, "1", ValueKind::Number)],
        );
        assert_eq!(
            mongo.select_page("d", "c", None, None, clause.as_deref()),
            None
        );
        assert_eq!(mongo.count("d", "c", clause.as_deref()), None);
        assert_eq!(mongo.insert_template("d", "c", &["a"]), None);
        assert_eq!(mongo.update_template("d", "c", &["a"], &["id"]), None);
        assert_eq!(mongo.delete_template("d", "c", &["id"]), None);

        assert!(mongo.supports(Feature::Count));
        assert!(mongo.supports(Feature::Sort));
        for feature in [Feature::Filter, Feature::WriteTemplates] {
            assert!(!mongo.supports(feature));
        }
        for driver in SQL_DRIVERS {
            for feature in [Feature::Sort, Feature::Filter, Feature::WriteTemplates] {
                assert!(dialect(driver).supports(feature));
            }
        }
    }

    fn filter(column: &str, op: FilterOp, value: &str, kind: ValueKind) -> Filter {
        Filter {
            column: column.into(),
            op,
            value: value.into(),
            kind,
        }
    }

    fn where_of(driver: &str, filters: &[Filter]) -> Option<String> {
        dialect(driver)
            .where_clause(filters)
            .map(|clause| clause.as_str().to_string())
    }

    /// The clause the filters make, for a statement that takes one.
    fn clause_of(driver: &str, filters: &[Filter]) -> Option<Box<WhereClause>> {
        dialect(driver).where_clause(filters)
    }

    #[test]
    fn every_comparison_is_written_with_a_quoted_column_and_a_literal() {
        let cases = [
            (FilterOp::Equals, "="),
            (FilterOp::NotEquals, "<>"),
            (FilterOp::Less, "<"),
            (FilterOp::Greater, ">"),
            (FilterOp::LessOrEqual, "<="),
            (FilterOp::GreaterOrEqual, ">="),
            (FilterOp::Like, "LIKE"),
        ];
        for (op, sql) in cases {
            assert_eq!(
                where_of("postgres", &[filter("name", op, "ann", ValueKind::Text)]),
                Some(format!("\"name\" {sql} 'ann'")),
                "{op:?}"
            );
        }
        assert_eq!(
            where_of(
                "mysql",
                &[filter("name", FilterOp::Equals, "ann", ValueKind::Text)]
            ),
            Some("`name` = 'ann'".into())
        );
    }

    #[test]
    fn null_checks_take_no_value_and_ignore_the_one_typed() {
        assert_eq!(
            where_of(
                "sqlite",
                &[filter("a", FilterOp::IsNull, "stale", ValueKind::Text)]
            ),
            Some("\"a\" IS NULL".into())
        );
        assert_eq!(
            where_of(
                "sqlite",
                &[filter("a", FilterOp::IsNotNull, "", ValueKind::Text)]
            ),
            Some("\"a\" IS NOT NULL".into())
        );
    }

    #[test]
    fn a_value_is_bare_only_when_its_column_is_numeric_and_it_is_a_plain_number() {
        let number = |value: &str| {
            where_of(
                "postgres",
                &[filter("age", FilterOp::Greater, value, ValueKind::Number)],
            )
        };
        assert_eq!(number("42"), Some("\"age\" > 42".into()));
        assert_eq!(number(" -3.5e2 "), Some("\"age\" > -3.5e2".into()));
        assert_eq!(number("0"), Some("\"age\" > 0".into()));
        for not_a_number in [
            "4 OR 1=1",
            "1; drop table t",
            "0x1F",
            "007",
            "+5",
            ".5",
            "5.",
            "inf",
            "NaN",
            "",
            "1e",
            "--1",
        ] {
            assert_eq!(
                number(not_a_number),
                Some(format!("\"age\" > '{}'", not_a_number.replace('\'', "''"))),
                "{not_a_number:?} is quoted, never bare"
            );
        }
        assert_eq!(
            where_of(
                "postgres",
                &[filter("code", FilterOp::Equals, "42", ValueKind::Text)]
            ),
            Some("\"code\" = '42'".into()),
            "a text column keeps its digits quoted"
        );
    }

    #[test]
    fn a_boolean_column_takes_true_and_false_and_quotes_anything_else() {
        let boolean = |value: &str| {
            where_of(
                "mysql",
                &[filter("on", FilterOp::Equals, value, ValueKind::Bool)],
            )
        };
        assert_eq!(boolean("true"), Some("`on` = TRUE".into()));
        assert_eq!(boolean(" FALSE "), Some("`on` = FALSE".into()));
        assert_eq!(boolean("1 or 1"), Some("`on` = '1 or 1'".into()));
    }

    #[test]
    fn in_lists_split_on_commas_and_type_each_item() {
        assert_eq!(
            where_of(
                "postgres",
                &[filter("id", FilterOp::In, "1, 2 ,,3", ValueKind::Number)]
            ),
            Some("\"id\" IN (1, 2, 3)".into())
        );
        assert_eq!(
            where_of(
                "sqlite",
                &[filter("n", FilterOp::In, "a, o'b", ValueKind::Text)]
            ),
            Some("\"n\" IN ('a', 'o''b')".into())
        );
        assert_eq!(
            where_of(
                "sqlite",
                &[filter("n", FilterOp::In, " , ", ValueKind::Text)]
            ),
            None,
            "an empty list filters nothing rather than writing IN ()"
        );
    }

    #[test]
    fn filters_are_anded_in_order() {
        assert_eq!(
            where_of(
                "postgres",
                &[
                    filter("a", FilterOp::Equals, "x", ValueKind::Text),
                    filter("b", FilterOp::IsNull, "", ValueKind::Text),
                ]
            ),
            Some("\"a\" = 'x' AND \"b\" IS NULL".into())
        );
        assert_eq!(where_of("postgres", &[]), None);
    }

    #[test]
    fn an_empty_string_is_a_value_to_filter_by_not_a_missing_one() {
        assert_eq!(
            where_of(
                "postgres",
                &[filter("a", FilterOp::Equals, "", ValueKind::Text)]
            ),
            Some("\"a\" = ''".into())
        );
    }

    #[test]
    fn a_quote_in_a_filtered_value_or_column_cannot_leave_its_quotes() {
        for driver in ["postgres", "sqlite"] {
            assert_eq!(
                where_of(
                    driver,
                    &[filter(
                        "we\"ird",
                        FilterOp::Equals,
                        "x' OR '1'='1",
                        ValueKind::Text
                    )]
                ),
                Some("\"we\"\"ird\" = 'x'' OR ''1''=''1'".into()),
                "{driver}"
            );
        }
    }

    /// MySQL reads `\` as an escape inside a string, so `\'` would close the
    /// literal early and let the rest run as SQL. The backslash is doubled
    /// first, which is what keeps the quote after it a quote *inside* the
    /// string.
    #[test]
    fn mysql_filter_values_double_the_backslash_before_the_quote() {
        assert_eq!(
            where_of(
                "mysql",
                &[filter("n", FilterOp::Equals, r"x\", ValueKind::Text)]
            ),
            Some(r"`n` = 'x\\'".into())
        );
        assert_eq!(
            where_of(
                "mysql",
                &[filter(
                    "n",
                    FilterOp::Equals,
                    r"\' OR 1=1 -- ",
                    ValueKind::Text
                )]
            ),
            Some(r"`n` = '\\'' OR 1=1 -- '".into())
        );
        assert_eq!(
            where_of(
                "postgres",
                &[filter("n", FilterOp::Equals, r"x\", ValueKind::Text)]
            ),
            Some(r#""n" = E'x\\'"#.into()),
            "an escape string, which reads the backslash the same under both settings"
        );
        assert_eq!(
            where_of(
                "mysql",
                &[filter("n", FilterOp::In, r"a\,b", ValueKind::Text)]
            ),
            Some(r"`n` IN ('a\\', 'b')".into())
        );
    }

    #[test]
    fn a_document_store_has_no_where_clause_and_filtering_is_unsupported() {
        assert_eq!(
            where_of(
                "mongodb",
                &[filter("a", FilterOp::Equals, "1", ValueKind::Text)]
            ),
            None
        );
        assert!(!dialect("mongodb").supports(Feature::Filter));
    }

    #[test]
    fn a_document_store_sorts_with_a_find_sort_document_in_key_order() {
        let order = [
            SortKey {
                column: "name",
                descending: false,
            },
            SortKey {
                column: "a\"b",
                descending: true,
            },
        ];
        assert_eq!(
            dialect("mongodb").select_page("app", "users", None, Some(&order), None),
            Some(r#"{"find": "users", "sort": {"name": 1, "a\"b": -1}, "$db": "app"}"#.into())
        );
        assert!(dialect("mongodb").supports(Feature::Sort));
    }

    #[test]
    fn templates_quote_names_and_leave_placeholders_that_do_not_parse() {
        let postgres = dialect("postgres");
        assert_eq!(
            postgres.insert_template("public", "users", &["id", "e-mail"]),
            Some(
                "INSERT INTO \"public\".\"users\" (\"id\", \"e-mail\")\nVALUES (<id>, <e-mail>);"
                    .into()
            )
        );
        assert_eq!(
            postgres.update_template("public", "users", &["name"], &["id", "tenant"]),
            Some(
                "UPDATE \"public\".\"users\"\nSET \"name\" = <name>\n\
                 WHERE \"id\" = <id> AND \"tenant\" = <tenant>;"
                    .into()
            )
        );
        assert_eq!(
            dialect("mysql").delete_template("app", "users", &["id"]),
            Some("DELETE FROM `app`.`users`\nWHERE `id` = <id>;".into())
        );
    }

    #[test]
    fn a_write_with_nothing_to_pin_it_down_is_not_offered() {
        for driver in SQL_DRIVERS {
            let dialect = dialect(driver);
            assert_eq!(dialect.insert_template("s", "t", &[]), None);
            assert_eq!(dialect.update_template("s", "t", &[], &["id"]), None);
            assert_eq!(
                dialect.update_template("s", "t", &["a"], &[]),
                None,
                "no key means every row"
            );
            assert_eq!(dialect.delete_template("s", "t", &[]), None);
        }
    }

    /// The table without a key gets a `WHERE` to write, as a comment, so the
    /// statement can never be run as "every row".
    #[test]
    fn a_table_with_no_key_gets_a_comment_where_not_an_absent_one() {
        let sql = dialect("postgres");
        assert_eq!(
            sql.update_template_without_key("public", "log", &["note"]),
            Some("UPDATE \"public\".\"log\"\nSET \"note\" = <note>\n-- WHERE <condition>;".into())
        );
        assert_eq!(
            sql.delete_template_without_key("public", "log"),
            Some("DELETE FROM \"public\".\"log\"\n-- WHERE <condition>;".into())
        );
        assert_eq!(sql.update_template_without_key("s", "t", &[]), None);
        let mongo = dialect("mongodb");
        assert_eq!(mongo.update_template_without_key("d", "c", &["a"]), None);
        assert_eq!(mongo.delete_template_without_key("d", "c"), None);
    }

    #[test]
    fn a_qualified_name_is_pasteable_for_the_engine_it_belongs_to() {
        assert_eq!(
            dialect("mysql").qualified_name("app", "users"),
            "`app`.`users`"
        );
        assert_eq!(
            dialect("mongodb").qualified_name("app", "users"),
            "app.users"
        );
    }

    /// What the confirmation shows is the statement the edit stands for, with
    /// each value written the way its engine would read it.
    #[test]
    fn an_edit_previews_as_an_update_with_values_written_for_the_engine() {
        let postgres = dialect("postgres");
        let set = [
            ("name", postgres.preview_literal(Some("O'Brien"), false)),
            ("score", postgres.preview_literal(Some("12.5"), true)),
            ("note", postgres.preview_literal(None, false)),
        ];
        let key = [("id", postgres.preview_literal(Some("7"), true))];
        assert_eq!(
            postgres.update_preview("public", "users", &set, &key),
            "UPDATE \"public\".\"users\"\nSET \"name\" = 'O''Brien', \"score\" = 12.5, \"note\" = NULL\nWHERE \"id\" = 7;"
        );
    }

    /// A number typed into a text column stays text: the preview must not
    /// suggest the column becomes a number.
    #[test]
    fn a_numeric_looking_value_is_bare_only_for_a_numeric_cell() {
        let sql = dialect("sqlite");
        assert_eq!(sql.preview_literal(Some("42"), false), "'42'");
        assert_eq!(sql.preview_literal(Some("42"), true), "42");
        assert_eq!(sql.preview_literal(Some("4 2"), true), "'4 2'");
        assert_eq!(sql.preview_literal(Some("1; drop"), true), "'1; drop'");
    }

    #[test]
    fn a_document_edit_previews_as_an_update_one() {
        let mongo = dialect("mongodb");
        let set = [("name", mongo.preview_literal(Some("a\"b"), false))];
        let key = [("_id", mongo.preview_literal(Some("abc"), false))];
        assert_eq!(
            mongo.update_preview("app", "users", &set, &key),
            "{\"updateOne\": {\"database\": \"app\", \"collection\": \"users\", \"filter\": {\"_id\": \"abc\"}, \"update\": {\"$set\": {\"name\": \"a\\\"b\"}}}}"
        );
    }
}
