//! The one write this driver offers.
//!
//! `query` runs inside `START TRANSACTION READ ONLY`, and that is not touched
//! here: an edit is a call of its own, with its own transaction, built from
//! structured values and never from SQL text.

use crate::{introspect, values};
use database::protocol::{
    ColumnDef, ColumnValue, EditValue, ErrorCode, ErrorData, ResponseError, RowChange,
    UpdateRowsParams, UpdateRowsResult,
};
use mysql::prelude::Queryable as _;
use mysql::{Conn, TxOpts};

/// How long a Save waits for a row another transaction holds. InnoDB's own
/// default is 50 seconds, with this connection's lock held throughout.
const LOCK_WAIT_SECONDS: u32 = 5;

/// Every change, or none.
///
/// Inherits the session's transaction mode rather than forcing `READ WRITE`: a
/// server an administrator defaulted to read-only meant it, and an explicit
/// `READ WRITE` here would walk straight past that.
pub fn apply(
    conn: &mut Conn,
    params: &UpdateRowsParams,
) -> Result<UpdateRowsResult, ResponseError> {
    let columns = introspect::columns(conn, &params.schema, &params.table)?;
    if columns.is_empty() {
        return Err(ResponseError::new(
            ErrorCode::Syntax,
            format!("`{}` is not a table in `{}`", params.table, params.schema),
        ));
    }
    refuse_without_transactions(conn, &params.schema, &params.table)?;
    // Planned in full before the transaction opens: a bad change anywhere in
    // the batch should not cost a round trip, let alone a lock.
    let statements = params
        .changes
        .iter()
        .enumerate()
        .map(|(index, change)| build(&params.schema, &params.table, &columns, change, index))
        .collect::<Result<Vec<_>, _>>()?;

    with_short_lock_wait(conn, |conn| write(conn, &statements, params))?;
    Ok(UpdateRowsResult {
        updated: statements.len() as u64,
    })
}

fn write(
    conn: &mut Conn,
    statements: &[Statement],
    params: &UpdateRowsParams,
) -> Result<(), ResponseError> {
    let mut transaction = conn
        .start_transaction(TxOpts::default())
        .map_err(values::error)?;
    for (index, (statement, change)) in statements.iter().zip(&params.changes).enumerate() {
        let result = transaction
            .exec_iter(&statement.sql, statement.values.clone())
            .map_err(|error| rejection(error, index, &change.set))?;
        // Matched rows, not changed ones: `CLIENT_FOUND_ROWS` is set at connect,
        // so saving the value a cell already holds is still one row.
        let affected = result.affected_rows();
        let warnings = result.warnings();
        drop(result);
        if warnings > 0 {
            refuse_warnings(&mut transaction, index, &change.set)?;
        }
        if affected != 1 {
            return Err(conflict(index, affected));
        }
    }
    // Dropping the transaction on any early return above is the rollback.
    transaction.commit().map_err(values::error)
}

/// Runs `body` with the session's lock wait cut short, and puts it back.
///
/// A session variable, so it would otherwise outlive the Save and shorten every
/// later wait this connection makes. Put back whether or not `body` worked.
fn with_short_lock_wait<T>(
    conn: &mut Conn,
    body: impl FnOnce(&mut Conn) -> Result<T, ResponseError>,
) -> Result<T, ResponseError> {
    let previous: Option<u64> = conn
        .query_first("SELECT @@session.innodb_lock_wait_timeout")
        .map_err(values::error)?;
    conn.query_drop(format!(
        "SET SESSION innodb_lock_wait_timeout = {LOCK_WAIT_SECONDS}"
    ))
    .map_err(values::error)?;
    let outcome = body(conn);
    if let Some(previous) = previous
        && let Err(error) =
            conn.query_drop(format!("SET SESSION innodb_lock_wait_timeout = {previous}"))
    {
        eprintln!("could not restore innodb_lock_wait_timeout to {previous}: {error}");
    }
    outcome
}

/// A table whose engine cannot roll back is not edited at all.
///
/// The batch is only all-or-nothing where there are transactions: on MyISAM or
/// MEMORY a failure halfway leaves the earlier changes written, which is
/// exactly what this method promises never to do. A view has no engine of its
/// own and is let through.
fn refuse_without_transactions(
    conn: &mut Conn,
    schema: &str,
    table: &str,
) -> Result<(), ResponseError> {
    let row: Option<(Option<String>, Option<String>)> = conn
        .exec_first(
            "SELECT t.engine, e.transactions FROM information_schema.tables t \
             LEFT JOIN information_schema.engines e ON e.engine = t.engine \
             WHERE t.table_schema = ? AND t.table_name = ?",
            (schema, table),
        )
        .map_err(values::error)?;
    let Some((engine, transactions)) = row else {
        return Ok(());
    };
    match without_transactions(engine.as_deref(), transactions.as_deref()) {
        true => Err(ResponseError::new(
            ErrorCode::InvalidValue,
            format!(
                "`{table}` uses the {} engine, which has no transactions: a save that failed \
                 halfway could not be undone, so this table cannot be edited here",
                engine.as_deref().unwrap_or("storage")
            ),
        )),
        false => Ok(()),
    }
}

/// `information_schema.engines.transactions` is `YES`, `NO` or `NULL` (the
/// server does not say). Only a plain `NO` refuses: guessing from the engine's
/// name would turn away every engine this driver has not heard of.
fn without_transactions(engine: Option<&str>, transactions: Option<&str>) -> bool {
    engine.is_some() && transactions == Some("NO")
}

/// A server not in strict mode keeps `0` for `abc` and says so in a warning.
/// An edit that quietly stored something other than what was typed is worse
/// than one that fails, so any warning from an UPDATE is a refusal.
fn refuse_warnings(
    transaction: &mut mysql::Transaction<'_>,
    change: usize,
    set: &[ColumnValue],
) -> Result<(), ResponseError> {
    let warnings: Vec<(String, u32, String)> =
        transaction.query("SHOW WARNINGS").map_err(values::error)?;
    let serious = warnings
        .into_iter()
        .find(|(level, _, _)| level == "Warning" || level == "Error");
    match serious {
        Some((_, _, message)) => Err(value_refused(message, change, set)),
        None => Ok(()),
    }
}

#[derive(Debug)]
pub struct Statement {
    pub sql: String,
    pub values: Vec<mysql::Value>,
}

/// One change as a parameterised UPDATE. Pure, so the quoting and the typing
/// are tested without a server.
pub fn build(
    schema: &str,
    table: &str,
    columns: &[ColumnDef],
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
        let column = known(columns, &set.column, index)?;
        if holds_bytes(&column.type_name) {
            return Err(invalid(
                "binary columns cannot be edited as text",
                index,
                Some(&column.name),
            ));
        }
        // The text as typed: MySQL converts it to the column's type on
        // assignment, and what it will not convert comes back as an error or,
        // in a lenient `sql_mode`, a warning -- both handled by the caller.
        values.push(match &set.value {
            EditValue::Null => mysql::Value::NULL,
            EditValue::Text { value } => mysql::Value::Bytes(value.clone().into_bytes()),
        });
        assignments.push(format!("{} = ?", quote_identifier(&column.name)));
    }

    let mut conditions = Vec::with_capacity(change.key.len());
    for key in &change.key {
        let column = known(columns, &key.column, index)?;
        let EditValue::Text { value } = &key.value else {
            return Err(invalid("a key column is NULL", index, Some(&column.name)));
        };
        values.push(key_value(column, value).map_err(|reason| {
            invalid(
                &format!("key {}: {reason}", column.name),
                index,
                Some(&column.name),
            )
        })?);
        conditions.push(format!("{} = ?", quote_identifier(&column.name)));
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

/// `column_type` of a column that holds bytes. The grid shows only the size of
/// such a cell, so text typed into one is never what was meant.
fn holds_bytes(column_type: &str) -> bool {
    let declared = column_type.to_ascii_lowercase();
    [
        "binary",
        "varbinary",
        "tinyblob",
        "blob",
        "mediumblob",
        "longblob",
    ]
    .iter()
    .any(|kind| declared.starts_with(kind))
}

/// An integer key goes over as an integer. Handed over as text, MySQL compares
/// it with the column as two doubles, which cannot tell apart two ids above
/// 2^53 -- exactly the size a snowflake id is.
fn key_value(column: &ColumnDef, text: &str) -> Result<mysql::Value, String> {
    let declared = column.type_name.to_ascii_lowercase();
    let is_integer = ["tinyint", "smallint", "mediumint", "int", "bigint"]
        .iter()
        .any(|kind| declared.starts_with(kind));
    if !is_integer {
        return Ok(mysql::Value::Bytes(text.as_bytes().to_vec()));
    }
    let text = text.trim();
    if declared.contains("unsigned") {
        text.parse::<u64>().map(mysql::Value::UInt)
    } else {
        text.parse::<i64>().map(mysql::Value::Int)
    }
    .map_err(|_| "expected a whole number".to_string())
}

fn known<'a>(
    columns: &'a [ColumnDef],
    name: &str,
    index: usize,
) -> Result<&'a ColumnDef, ResponseError> {
    columns
        .iter()
        .find(|column| column.name == name)
        .ok_or_else(|| {
            invalid(
                &format!("no column `{name}` in this table"),
                index,
                Some(name),
            )
        })
}

/// Wraps an identifier in backticks, doubling any it already contains.
pub fn quote_identifier(identifier: &str) -> String {
    format!("`{}`", identifier.replace('`', "``"))
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

/// Server errors that mean the *value* was refused, as opposed to the
/// statement, the connection or the permissions: a null in a `NOT NULL`, a
/// duplicate, a foreign key, a check, a conversion or range failure, a value
/// that does not fit, invalid JSON, a generated column, and a trigger's own
/// `SIGNAL` -- the way a schema writes its own validation.
const VALUE_REFUSALS: [u16; 17] = [
    1048, // ER_BAD_NULL_ERROR
    1062, // ER_DUP_ENTRY
    1263, // WARN_DATA_OUT_OF_RANGE
    1264, // ER_WARN_DATA_OUT_OF_RANGE
    1265, // WARN_DATA_TRUNCATED
    1292, // ER_TRUNCATED_WRONG_VALUE
    1366, // ER_TRUNCATED_WRONG_VALUE_FOR_FIELD
    1406, // ER_DATA_TOO_LONG
    1411, // ER_WRONG_VALUE_FOR_TYPE
    1451, // ER_ROW_IS_REFERENCED_2
    1452, // ER_NO_REFERENCED_ROW_2
    1525, // ER_WRONG_VALUE
    1644, // ER_SIGNAL_EXCEPTION
    1690, // ER_DATA_OUT_OF_RANGE
    3105, // ER_NON_DEFAULT_VALUE_FOR_GENERATED_COLUMN
    3140, // ER_INVALID_JSON_TEXT
    3819, // ER_CHECK_CONSTRAINT_VIOLATED
];

/// How a failed write reads to the caller, or `None` to leave it to the
/// ordinary mapping.
///
/// A lock wait that timed out and a deadlock are `internal` with a sentence of
/// their own, never `conflict`: that code tells the grid *the row changed*, and
/// a row someone else is holding says nothing of the kind.
fn write_code(code: u16) -> Option<ErrorCode> {
    match code {
        code if VALUE_REFUSALS.contains(&code) => Some(ErrorCode::InvalidValue),
        // ER_NON_UPDATABLE_TABLE: a view the server cannot update through.
        1288 => Some(ErrorCode::InvalidValue),
        // ER_TABLEACCESS_DENIED_ERROR, ER_COLUMNACCESS_DENIED_ERROR: the same
        // "you may not write here" a read-only session gives.
        1142 | 1143 => Some(ErrorCode::ReadOnly),
        // ER_LOCK_WAIT_TIMEOUT, ER_LOCK_DEADLOCK.
        1205 | 1213 => Some(ErrorCode::Internal),
        _ => None,
    }
}

fn lock_explanation(code: u16) -> Option<String> {
    match code {
        1205 => Some(format!(
            "the row is locked by another transaction (waited {LOCK_WAIT_SECONDS} s), so nothing \
             was saved -- try again when it is done"
        )),
        1213 => Some(
            "deadlock detected: another transaction needed the same rows, so nothing was saved \
             -- try again"
                .to_string(),
        ),
        _ => None,
    }
}

fn rejection(error: mysql::Error, change: usize, set: &[ColumnValue]) -> ResponseError {
    let mysql::Error::MySqlError(server) = &error else {
        return values::error(error);
    };
    match write_code(server.code) {
        Some(ErrorCode::InvalidValue) => value_refused(server.message.clone(), change, set),
        Some(code) => {
            let message = lock_explanation(server.code).unwrap_or_else(|| server.message.clone());
            ResponseError::new(code, message)
                .with_detail(database::protocol::error_chain(&error))
                .with_data(ErrorData {
                    change: Some(change as u64),
                    column: None,
                })
        }
        None => values::error(error),
    }
}

fn value_refused(message: String, change: usize, set: &[ColumnValue]) -> ResponseError {
    let column = column_in(&message, set);
    ResponseError::new(ErrorCode::InvalidValue, message.clone())
        .with_detail(message)
        .with_data(ErrorData {
            change: Some(change as u64),
            column,
        })
}

/// Which column of the change a server message is about.
///
/// MySQL names it as `for column 'age' at row 1`, `Column 'age' cannot be
/// null`, `` for column `db`.`t`.`age` `` or, for a duplicate, `for key
/// 'people.handle'` -- where the index is called after its column unless it was
/// named otherwise. Only a column the change was setting is believed: a unique
/// index can fail on a column nobody touched, and pointing at it would mark the
/// wrong cell.
fn column_in(message: &str, set: &[ColumnValue]) -> Option<String> {
    let named = ["column ", "key "].iter().find_map(|marker| {
        let at = message.to_ascii_lowercase().find(marker)? + marker.len();
        let word = message[at..].split_whitespace().next()?;
        let last = word.rsplit('.').next()?;
        let name = last.trim_matches(|c| c == '\'' || c == '`' || c == '"');
        set.iter()
            .find(|column| column.column == name)
            .map(|column| column.column.clone())
    });
    named.or_else(|| match set {
        [only] => Some(only.column.clone()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, type_name: &str) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            type_name: type_name.into(),
            nullable: true,
            primary_key: false,
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

    fn table() -> Vec<ColumnDef> {
        vec![
            column("id", "bigint"),
            column("size", "int unsigned"),
            column("we`ird", "varchar(20)"),
        ]
    }

    #[test]
    fn identifiers_are_quoted_and_values_are_placeholders() {
        let change = RowChange {
            key: vec![pair("id", text("7"))],
            set: vec![
                pair("we`ird", text("'; DROP TABLE t; --")),
                pair("size", EditValue::Null),
            ],
        };
        let statement = build("ma`in", "t", &table(), &change, 0).expect("builds");
        assert_eq!(
            statement.sql,
            "UPDATE `ma``in`.`t` SET `we``ird` = ?, `size` = ? WHERE `id` = ?"
        );
        assert_eq!(
            statement.values,
            vec![
                mysql::Value::Bytes(b"'; DROP TABLE t; --".to_vec()),
                mysql::Value::NULL,
                mysql::Value::Int(7)
            ]
        );
    }

    #[test]
    fn an_integer_key_is_an_integer_and_anything_else_stays_text() {
        let signed = column("id", "bigint");
        let unsigned = column("n", "int(10) unsigned");
        let word = column("w", "varchar(5)");
        assert_eq!(
            key_value(&signed, " 9007199254740993 "),
            Ok(mysql::Value::Int(9007199254740993))
        );
        assert_eq!(
            key_value(&unsigned, "18446744073709551615"),
            Ok(mysql::Value::UInt(u64::MAX))
        );
        assert!(key_value(&unsigned, "-1").is_err());
        assert!(key_value(&signed, "12 monkeys").is_err());
        assert_eq!(
            key_value(&word, "12"),
            Ok(mysql::Value::Bytes(b"12".to_vec()))
        );
    }

    #[test]
    fn a_name_the_table_does_not_have_a_null_key_or_an_empty_change_is_refused() {
        let refused = |key: Vec<ColumnValue>, set: Vec<ColumnValue>| {
            build("db", "t", &table(), &RowChange { key, set }, 2)
        };
        let error = refused(vec![pair("nope", text("1"))], vec![pair("size", text("1"))])
            .expect_err("unknown key column");
        assert_eq!(error.code, ErrorCode::InvalidValue);
        let data = error.data.expect("where");
        assert_eq!(
            (data.change, data.column.as_deref()),
            (Some(2), Some("nope"))
        );
        assert!(refused(vec![pair("id", text("1"))], vec![pair("nope", text("1"))]).is_err());
        assert!(
            refused(
                vec![pair("id", EditValue::Null)],
                vec![pair("size", text("1"))]
            )
            .is_err()
        );
        assert!(refused(vec![], vec![pair("size", text("1"))]).is_err());
        assert!(refused(vec![pair("id", text("1"))], vec![]).is_err());
        assert!(
            refused(
                vec![pair("id", text("1"))],
                vec![pair("size", text("1")), pair("size", text("2"))]
            )
            .is_err()
        );
    }

    #[test]
    fn a_server_message_points_at_a_column_only_when_it_was_being_set() {
        let both = [pair("age", text("1")), pair("name", text("x"))];
        let named = |message: &str| column_in(message, &both);
        assert_eq!(
            named("Data truncated for column 'age' at row 1").as_deref(),
            Some("age")
        );
        assert_eq!(
            named("Incorrect integer value: 'name' for column `db`.`t`.`age` at row 1").as_deref(),
            Some("age"),
            "a typed value that looks like another column must not fool it"
        );
        assert_eq!(
            named("Column 'name' cannot be null").as_deref(),
            Some("name")
        );
        assert_eq!(
            named("Duplicate entry 'bob' for key 'people.name'").as_deref(),
            Some("name")
        );
        assert_eq!(
            named("Duplicate entry 'bob' for key 'people.handle'"),
            None,
            "an untouched column must not be blamed"
        );
        assert_eq!(
            column_in("Check constraint 'c' is violated.", &both[..1]).as_deref(),
            Some("age"),
            "with one column being set there is only one to blame"
        );
    }

    #[test]
    fn the_refusals_are_the_codes_a_bad_value_produces() {
        assert!(VALUE_REFUSALS.contains(&1048));
        assert!(VALUE_REFUSALS.contains(&3819));
        for other in [1045, 1290, 1792, 1317, 1142, 1054, 1064] {
            assert!(!VALUE_REFUSALS.contains(&other), "{other}");
        }
    }

    #[test]
    fn a_column_that_holds_bytes_is_refused_as_a_set() {
        for binary in [
            "blob",
            "tinyblob",
            "mediumblob",
            "longblob",
            "binary(4)",
            "varbinary(16)",
        ] {
            assert!(holds_bytes(binary), "{binary}");
        }
        for text in [
            "text",
            "mediumtext",
            "varchar(20)",
            "char(4)",
            "int",
            "json",
            "bit(8)",
        ] {
            assert!(!holds_bytes(text), "{text}");
        }
        let columns = vec![column("id", "int"), column("raw", "varbinary(8)")];
        let change = RowChange {
            key: vec![pair("id", text("1"))],
            set: vec![pair("raw", text("x"))],
        };
        let error = build("db", "t", &columns, &change, 3).expect_err("bytes");
        assert_eq!(error.code, ErrorCode::InvalidValue);
        assert_eq!(error.message, "binary columns cannot be edited as text");
        let data = error.data.expect("where");
        assert_eq!(
            (data.change, data.column.as_deref()),
            (Some(3), Some("raw"))
        );
    }

    #[test]
    fn only_an_engine_that_says_no_to_transactions_is_turned_away() {
        assert!(without_transactions(Some("MyISAM"), Some("NO")));
        assert!(without_transactions(Some("MEMORY"), Some("NO")));
        assert!(!without_transactions(Some("InnoDB"), Some("YES")));
        assert!(
            !without_transactions(Some("SomethingNew"), None),
            "unknown is let through"
        );
        assert!(!without_transactions(None, None), "a view has no engine");
    }

    #[test]
    fn a_failed_write_is_sorted_into_the_code_the_caller_reacts_to() {
        let expected = [
            (1062, Some(ErrorCode::InvalidValue)),
            (3819, Some(ErrorCode::InvalidValue)),
            (1288, Some(ErrorCode::InvalidValue)),
            (1142, Some(ErrorCode::ReadOnly)),
            (1143, Some(ErrorCode::ReadOnly)),
            (1205, Some(ErrorCode::Internal)),
            (1213, Some(ErrorCode::Internal)),
            // Left to `values::error`: read-only sessions, credentials, syntax.
            (1290, None),
            (1792, None),
            (1045, None),
            (1064, None),
        ];
        for (code, mapped) in expected {
            assert_eq!(write_code(code), mapped, "{code}");
        }
    }

    #[test]
    fn a_lock_or_a_deadlock_says_nothing_was_saved() {
        for code in [1205, 1213] {
            let message = lock_explanation(code).expect("a sentence of its own");
            assert!(message.contains("nothing"), "{code}: {message}");
        }
        assert!(lock_explanation(1205).is_some_and(|message| message.contains("locked")));
        assert_eq!(lock_explanation(1062), None);
    }
}
