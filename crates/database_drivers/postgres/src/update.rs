//! The one write this driver offers.
//!
//! `query` runs inside `BEGIN TRANSACTION READ ONLY`, and that is not touched
//! here: an edit is a call of its own, with its own transaction, built from
//! structured values and never from SQL text.

use crate::values;
use bytes::{BufMut as _, BytesMut};
use database::protocol::{
    ColumnValue, EditValue, ErrorCode, ErrorData, ResponseError, RowChange, UpdateRowsParams,
    UpdateRowsResult,
};
use postgres::Client;
use postgres::types::{Format, IsNull, ToSql, Type, to_sql_checked};
use std::error::Error;

/// How long a Save waits for a row another session holds before giving up.
const LOCK_TIMEOUT: &str = "5s";
/// The longest one UPDATE may run. A row addressed by its key is a moment's
/// work; anything past this is a scan of a table nobody indexed for it, or a
/// stuck server, and either way the connection must not stay held.
const STATEMENT_TIMEOUT: &str = "30s";

/// A column as an UPDATE needs to know it.
pub struct Target {
    pub name: String,
    /// Holds bytes, directly or through a domain. The grid shows only the size
    /// of such a cell, so text typed into one is never what was meant.
    pub binary: bool,
}

/// A value that crosses as the text it is and lets the server decide what type
/// that text is.
///
/// Neither an explicit `CAST` nor a Rust type will do. `format_type` names
/// `char(5)` as `character`, and `CAST(x AS character)` is `char(1)`: a cell
/// edit stored its first letter, and a key `'US'` compared as `'U'` matched the
/// wrong row with the exactly-one-row check none the wiser. Sent as an untyped
/// parameter the server instead applies *assignment* coercion to the column's
/// own type -- modifier, domain and all -- which raises "value too long" where a
/// cast would truncate, and compares a key with the column's own operator.
///
/// Text format on the wire for the same reason: the server parses it with the
/// type's input function, which is the one place that knows what `bit(3)` or
/// `timestamptz` accepts.
#[derive(Debug)]
struct TextParam<'a>(Option<&'a str>);

impl ToSql for TextParam<'_> {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        match self.0 {
            Some(text) => {
                out.put_slice(text.as_bytes());
                Ok(IsNull::No)
            }
            None => Ok(IsNull::Yes),
        }
    }

    fn accepts(_: &Type) -> bool {
        true
    }

    fn encode_format(&self, _: &Type) -> Format {
        Format::Text
    }

    to_sql_checked!();
}

/// The columns of `table`, from the catalog.
///
/// Every name in an `update_rows` call becomes an identifier in the statement,
/// so nothing the UI sent is trusted to be one. A materialised view is left out
/// on purpose: PostgreSQL cannot update one, and saying so up front beats a
/// failed statement.
pub fn table_columns(
    client: &mut Client,
    schema: &str,
    table: &str,
) -> Result<Vec<Target>, ResponseError> {
    let rows = client
        .query(
            "SELECT a.attname, COALESCE(( \
                 WITH RECURSIVE chain(oid, typtype, basetype) AS ( \
                     SELECT t.oid, t.typtype, t.typbasetype FROM pg_type t \
                     WHERE t.oid = a.atttypid \
                     UNION ALL \
                     SELECT t.oid, t.typtype, t.typbasetype FROM pg_type t \
                     JOIN chain ON chain.typtype = 'd' AND t.oid = chain.basetype \
                 ) \
                 SELECT oid FROM chain WHERE typtype <> 'd' \
             ) = 'bytea'::regtype::oid, false) \
             FROM pg_attribute a \
             JOIN pg_class c ON c.oid = a.attrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 \
               AND c.relkind IN ('r', 'p', 'v', 'f') \
               AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
            &[&schema, &table],
        )
        .map_err(values::error)?;
    if rows.is_empty() {
        return Err(ResponseError::new(
            ErrorCode::Syntax,
            format!("`{table}` is not an updatable table in `{schema}`"),
        ));
    }
    Ok(rows
        .into_iter()
        .map(|row| Target {
            name: row.get(0),
            binary: row.get(1),
        })
        .collect())
}

/// Every change, or none.
///
/// Inherits the session's transaction mode rather than forcing `READ WRITE`: a
/// role or database a DBA set to `default_transaction_read_only` meant it, and
/// an explicit `READ WRITE` here would walk straight past that. The refusal
/// that results arrives as `read_only`, which is the right thing to tell
/// someone whose Save was turned down by their own server.
pub fn apply(
    client: &mut Client,
    params: &UpdateRowsParams,
) -> Result<UpdateRowsResult, ResponseError> {
    let targets = table_columns(client, &params.schema, &params.table)?;
    // Planned in full before the transaction opens: a bad change anywhere in
    // the batch should not cost a round trip, let alone a lock.
    let statements = params
        .changes
        .iter()
        .enumerate()
        .map(|(index, change)| build(&params.schema, &params.table, &targets, change, index))
        .collect::<Result<Vec<_>, _>>()?;

    let mut transaction = client.transaction().map_err(values::error)?;
    // Local to this transaction, so the user's own queries keep the server's
    // defaults. The connection's lock is held for as long as this runs: waiting
    // on a row another session holds, with no limit, would freeze browsing too.
    transaction
        .batch_execute(&format!(
            "SET LOCAL lock_timeout = '{LOCK_TIMEOUT}'; \
             SET LOCAL statement_timeout = '{STATEMENT_TIMEOUT}'"
        ))
        .map_err(values::error)?;
    for (index, (statement, change)) in statements.iter().zip(&params.changes).enumerate() {
        // Prepared with no types, so the server infers each one from the column
        // it meets -- see `TextParam` for why nothing here names a type.
        let prepared = transaction
            .prepare(&statement.sql)
            .map_err(|error| failure(error, index, &change.set))?;
        let arguments: Vec<TextParam<'_>> = statement
            .values
            .iter()
            .map(|value| TextParam(value.as_deref()))
            .collect();
        let affected = transaction
            .execute(
                &prepared,
                &arguments
                    .iter()
                    .map(|argument| argument as &(dyn ToSql + Sync))
                    .collect::<Vec<_>>(),
            )
            .map_err(|error| failure(error, index, &change.set))?;
        if affected != 1 {
            return Err(conflict(index, affected));
        }
    }
    // Dropping the transaction on any early return above is the rollback.
    transaction.commit().map_err(values::error)?;
    Ok(UpdateRowsResult {
        updated: statements.len() as u64,
    })
}

#[derive(Debug)]
pub struct Statement {
    pub sql: String,
    /// One per `$n`, `None` for NULL.
    pub values: Vec<Option<String>>,
}

/// One change as a parameterised UPDATE. Pure, so the quoting is tested without
/// a server.
pub fn build(
    schema: &str,
    table: &str,
    targets: &[Target],
    change: &RowChange,
    index: usize,
) -> Result<Statement, ResponseError> {
    if change.set.is_empty() {
        return Err(invalid("a change sets nothing", index, None));
    }
    if change.key.is_empty() {
        return Err(invalid(
            "a change with no key would match every row",
            index,
            None,
        ));
    }

    let mut values = Vec::with_capacity(change.set.len() + change.key.len());
    let mut assignments = Vec::with_capacity(change.set.len());
    for (position, set) in change.set.iter().enumerate() {
        if change.set[..position]
            .iter()
            .any(|seen| seen.column == set.column)
        {
            return Err(invalid("a column is set twice", index, Some(&set.column)));
        }
        let target = known(targets, &set.column, index)?;
        if target.binary {
            return Err(invalid(
                "binary columns cannot be edited as text",
                index,
                Some(&target.name),
            ));
        }
        values.push(text_of(&set.value));
        assignments.push(format!(
            "{} = ${}",
            quote_identifier(&target.name),
            values.len()
        ));
    }

    let mut conditions = Vec::with_capacity(change.key.len());
    for key in &change.key {
        let target = known(targets, &key.column, index)?;
        if key.value == EditValue::Null {
            return Err(invalid("a key column is NULL", index, Some(&target.name)));
        }
        values.push(text_of(&key.value));
        conditions.push(format!(
            "{} = ${}",
            quote_identifier(&target.name),
            values.len()
        ));
    }

    Ok(Statement {
        sql: format!(
            "UPDATE {}.{} SET {} WHERE {}",
            quote_identifier(schema),
            quote_identifier(table),
            assignments.join(", "),
            conditions.join(" AND ")
        ),
        values,
    })
}

fn text_of(value: &EditValue) -> Option<String> {
    match value {
        EditValue::Null => None,
        EditValue::Text { value } => Some(value.clone()),
    }
}

fn known<'a>(targets: &'a [Target], name: &str, index: usize) -> Result<&'a Target, ResponseError> {
    targets
        .iter()
        .find(|target| target.name == name)
        .ok_or_else(|| {
            invalid(
                &format!("no column `{name}` in this table"),
                index,
                Some(name),
            )
        })
}

/// Wraps an identifier in double quotes, doubling any it already contains.
pub fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn invalid(message: &str, change: usize, column: Option<&str>) -> ResponseError {
    ResponseError::new(ErrorCode::InvalidValue, message).with_data(ErrorData {
        change: Some(change as u64),
        column: column.map(str::to_string),
    })
}

fn conflict(change: usize, matched: u64) -> ResponseError {
    ResponseError::new(
        ErrorCode::Conflict,
        "a row changed or was deleted since it was read",
    )
    .with_detail(format!("row {change} matched {matched} rows"))
    .with_data(ErrorData {
        change: Some(change as u64),
        column: None,
    })
}

/// Whether a SQLSTATE says the *value* was refused, as opposed to the
/// statement, the connection or the permissions.
///
/// Class 22 is a data exception (a failed cast, an out-of-range number, a
/// string too long), class 23 an integrity violation (NOT NULL, unique, check,
/// foreign key). `42804` is a datatype mismatch, `428C9` an attempt to set a
/// generated column, and `P0001` is what a trigger's `RAISE EXCEPTION` carries
/// -- the way a schema writes its own validation.
fn is_value_rejection(sqlstate: &str) -> bool {
    sqlstate.starts_with("22")
        || sqlstate.starts_with("23")
        || matches!(sqlstate, "42804" | "428C9" | "P0001")
}

/// How a failed write reads to the caller, or `None` to leave it to the
/// ordinary mapping (a lost connection, bad credentials, a read-only session).
///
/// Four of these are not about the value at all, and none of them is a
/// `conflict`: that code tells the grid *the row changed*, and a row someone
/// else is holding, or a deadlock, says nothing of the kind. They are `internal`
/// with a sentence that names what happened and that nothing was saved.
fn write_code(sqlstate: &str) -> Option<ErrorCode> {
    match sqlstate {
        state if is_value_rejection(state) => Some(ErrorCode::InvalidValue),
        // object_not_in_prerequisite_state: what an UPDATE of a view that is not
        // auto-updatable answers. The request asked for something this object
        // cannot do, which is closer to a refused value than to a broken query.
        "55000" => Some(ErrorCode::InvalidValue),
        // insufficient_privilege. The same answer a read-only session gives, and
        // the UI treats both as "you may not write here".
        "42501" => Some(ErrorCode::ReadOnly),
        // deadlock_detected, lock_not_available (our `lock_timeout`),
        // query_canceled (our `statement_timeout`), undefined_function (a key
        // column whose type has no `=`, such as `json`).
        "40P01" | "55P03" | "57014" | "42883" => Some(ErrorCode::Internal),
        _ => None,
    }
}

/// The sentence for the states whose own wording does not say what to do next.
fn explanation(sqlstate: &str, server_said: &str) -> String {
    match sqlstate {
        "40P01" => "deadlock detected: another transaction needed the same rows, so nothing \
                    was saved -- try again"
            .to_string(),
        "55P03" => format!(
            "the row is locked by another transaction (waited {LOCK_TIMEOUT}), so nothing was \
             saved -- try again when it is done"
        ),
        "57014" => format!(
            "the save was stopped before it finished (it may run for at most \
             {STATEMENT_TIMEOUT}), so nothing was saved"
        ),
        "42883" => {
            format!("a row cannot be addressed through a column of this type: {server_said}")
        }
        _ => server_said.to_string(),
    }
}

/// What the server refused about one change, as the caller will read it.
fn failure(error: postgres::Error, change: usize, set: &[ColumnValue]) -> ResponseError {
    let Some(database_error) = error.as_db_error() else {
        return values::error(error);
    };
    let sqlstate = database_error.code().code();
    let Some(code) = write_code(sqlstate) else {
        return values::error(error);
    };
    // Believed only when the change was setting it: a foreign key can name a
    // column nobody touched, and marking that cell would point at the wrong one.
    let column = (code == ErrorCode::InvalidValue)
        .then(|| {
            database_error
                .column()
                .filter(|named| set.iter().any(|set| set.column == *named))
                .map(str::to_string)
                .or_else(|| match set {
                    [only] => Some(only.column.clone()),
                    _ => None,
                })
        })
        .flatten();
    ResponseError::new(code, explanation(sqlstate, database_error.message()))
        .with_detail(database::protocol::error_chain(&error))
        .with_data(ErrorData {
            change: Some(change as u64),
            column,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(name: &str, binary: bool) -> Target {
        Target {
            name: name.into(),
            binary,
        }
    }

    fn pair(name: &str, value: EditValue) -> ColumnValue {
        ColumnValue {
            column: name.into(),
            value,
        }
    }

    fn text(value: &str) -> EditValue {
        EditValue::Text {
            value: value.into(),
        }
    }

    fn table() -> Vec<Target> {
        vec![
            target("id", false),
            target("we\"ird", false),
            target("tags", false),
            target("raw", true),
        ]
    }

    #[test]
    fn identifiers_are_quoted_and_values_are_placeholders() {
        let change = RowChange {
            key: vec![pair("id", text("7"))],
            set: vec![
                pair("we\"ird", text("'; DROP TABLE t; --")),
                pair("tags", EditValue::Null),
            ],
        };
        let statement = build("ma\"in", "t", &table(), &change, 0).expect("builds");
        assert_eq!(
            statement.sql,
            "UPDATE \"ma\"\"in\".\"t\" SET \"we\"\"ird\" = $1, \"tags\" = $2 WHERE \"id\" = $3"
        );
        assert_eq!(
            statement.values,
            vec![
                Some("'; DROP TABLE t; --".to_string()),
                None,
                Some("7".into())
            ]
        );
    }

    #[test]
    fn a_composite_key_is_one_condition_per_column() {
        let change = RowChange {
            key: vec![pair("id", text("7")), pair("we\"ird", text("a"))],
            set: vec![pair("tags", text("{1,2}"))],
        };
        let statement = build("public", "t", &table(), &change, 0).expect("builds");
        assert!(
            statement
                .sql
                .ends_with("WHERE \"id\" = $2 AND \"we\"\"ird\" = $3")
        );
    }

    #[test]
    fn a_name_the_table_does_not_have_is_refused_and_named() {
        for (key, set, named) in [("id", "nope", "nope"), ("nope", "tags", "nope")] {
            let change = RowChange {
                key: vec![pair(key, text("1"))],
                set: vec![pair(set, text("x"))],
            };
            let error = build("public", "t", &table(), &change, 3).expect_err("unknown column");
            assert_eq!(error.code, ErrorCode::InvalidValue);
            let data = error.data.expect("where");
            assert_eq!(data.change, Some(3));
            assert_eq!(data.column.as_deref(), Some(named));
        }
    }

    #[test]
    fn a_null_key_an_empty_key_or_an_empty_set_is_refused() {
        let refused = |key: Vec<ColumnValue>, set: Vec<ColumnValue>| {
            build("public", "t", &table(), &RowChange { key, set }, 0).is_err()
        };
        assert!(refused(
            vec![pair("id", EditValue::Null)],
            vec![pair("tags", text("x"))]
        ));
        assert!(refused(vec![], vec![pair("tags", text("x"))]));
        assert!(refused(vec![pair("id", text("1"))], vec![]));
        assert!(refused(
            vec![pair("id", text("1"))],
            vec![pair("tags", text("a")), pair("tags", text("b"))]
        ));
    }

    #[test]
    fn only_a_refused_value_is_an_invalid_value() {
        for refused in [
            "22P02", "22001", "23502", "23505", "23514", "42804", "428C9", "P0001",
        ] {
            assert!(is_value_rejection(refused), "{refused}");
        }
        for other in [
            "25006", "42P01", "42703", "42501", "08006", "57014", "40001",
        ] {
            assert!(!is_value_rejection(other), "{other}");
        }
    }

    /// No type name is ever written into a statement: the server decides what
    /// the text is from the column it meets.
    #[test]
    fn no_statement_names_a_type() {
        let change = RowChange {
            key: vec![pair("id", text("7"))],
            set: vec![pair("tags", text("{1}"))],
        };
        let statement = build("public", "t", &table(), &change, 0).expect("builds");
        assert!(!statement.sql.to_ascii_uppercase().contains("CAST"));
    }

    #[test]
    fn a_binary_column_is_refused_as_a_set_and_named() {
        let change = RowChange {
            key: vec![pair("id", text("7"))],
            set: vec![pair("raw", text("abc"))],
        };
        let error = build("public", "t", &table(), &change, 4).expect_err("binary");
        assert_eq!(error.code, ErrorCode::InvalidValue);
        assert_eq!(error.message, "binary columns cannot be edited as text");
        let data = error.data.expect("where");
        assert_eq!(
            (data.change, data.column.as_deref()),
            (Some(4), Some("raw"))
        );
    }

    #[test]
    fn a_failed_write_is_sorted_into_the_code_the_caller_reacts_to() {
        let expected = [
            ("22001", Some(ErrorCode::InvalidValue)),
            ("23505", Some(ErrorCode::InvalidValue)),
            ("55000", Some(ErrorCode::InvalidValue)),
            ("42501", Some(ErrorCode::ReadOnly)),
            ("40P01", Some(ErrorCode::Internal)),
            ("55P03", Some(ErrorCode::Internal)),
            ("57014", Some(ErrorCode::Internal)),
            ("42883", Some(ErrorCode::Internal)),
            // Left to `values::error`: read-only sessions, credentials, the wire.
            ("25006", None),
            ("28P01", None),
            ("08006", None),
            ("42P01", None),
        ];
        for (sqlstate, code) in expected {
            assert_eq!(write_code(sqlstate), code, "{sqlstate}");
        }
    }

    #[test]
    fn a_lock_or_a_deadlock_says_nothing_was_saved_and_is_not_a_conflict() {
        for sqlstate in ["40P01", "55P03", "57014"] {
            let message = explanation(sqlstate, "the server's words");
            assert!(message.contains("nothing was"), "{sqlstate}: {message}");
            assert_ne!(write_code(sqlstate), Some(ErrorCode::Conflict));
        }
        assert!(explanation("55P03", "").contains("locked"));
        assert_eq!(explanation("22001", "too long"), "too long");
    }

    #[test]
    fn a_parameter_is_text_on_the_wire_whatever_type_the_server_infers() {
        let mut out = BytesMut::new();
        for kind in [
            Type::INT4,
            Type::BPCHAR,
            Type::BIT,
            Type::TIMESTAMPTZ,
            Type::UUID,
        ] {
            assert!(TextParam::accepts(&kind), "{kind}");
            assert!(matches!(
                TextParam(Some("x")).encode_format(&kind),
                Format::Text
            ));
        }
        assert!(matches!(
            TextParam(Some("xyz12")).to_sql(&Type::BPCHAR, &mut out),
            Ok(IsNull::No)
        ));
        assert_eq!(&out[..], b"xyz12", "the whole text, not a prefix of it");
        assert!(matches!(
            TextParam(None).to_sql(&Type::INT4, &mut out),
            Ok(IsNull::Yes)
        ));
    }
}
