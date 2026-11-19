//! The one write this driver offers.
//!
//! Browsing keeps its `SQLITE_OPEN_READ_ONLY` handle, so nothing typed into the
//! scratch buffer can write. An edit gets a handle of its own, opened for the
//! length of one call and dropped with it: the read-only guarantee never has to
//! be lifted and put back.

use crate::{introspect, values};
use database::protocol::{
    ColumnDef, ColumnValue, EditValue, ErrorCode, ErrorData, ResponseError, RowChange, TableKind,
    UpdateRowsParams, UpdateRowsResult,
};
use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags, OptionalExtension as _, TransactionBehavior};
use std::time::Duration;

/// Long enough to ride out another process's short write, short enough that a
/// held lock reads as "busy" and not as a frozen editor.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// The columns of `table`, if it is something an UPDATE can address.
///
/// Checked against the catalog because every name in an `update_rows` call
/// reaches the SQL text as an identifier, and an identifier cannot be bound.
pub fn table_columns(
    connection: &Connection,
    schema: &str,
    table: &str,
) -> Result<Vec<ColumnDef>, ResponseError> {
    let is_table = introspect::tables(connection, schema)?
        .iter()
        .any(|candidate| candidate.name == table && candidate.kind == TableKind::Table);
    if !is_table {
        return Err(ResponseError::new(
            ErrorCode::Syntax,
            format!("`{table}` is not a table in `{schema}`"),
        ));
    }
    introspect::columns(connection, schema, table)
}

/// Every change, or none.
pub fn apply(
    path: &str,
    columns: &[ColumnDef],
    params: &UpdateRowsParams,
) -> Result<UpdateRowsResult, ResponseError> {
    // All of it planned before a writer is opened: a bad change anywhere in the
    // batch should cost nothing, not a lock.
    let statements = params
        .changes
        .iter()
        .enumerate()
        .map(|(index, change)| build(&params.schema, &params.table, columns, change, index))
        .collect::<Result<Vec<_>, _>>()?;

    let mut writer = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(write_failure)?;
    writer.busy_timeout(BUSY_TIMEOUT).map_err(write_failure)?;
    // Immediate: the write lock is taken now, so a busy file fails before the
    // first change instead of halfway through the batch.
    let transaction = writer
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(write_failure)?;

    for (index, (statement, change)) in statements.iter().zip(&params.changes).enumerate() {
        if let Some(column) = holds_bytes(&transaction, statement).map_err(write_failure)? {
            return Err(invalid(BINARY_REFUSAL, index, Some(&column)));
        }
        let affected = transaction
            .execute(
                &statement.sql,
                rusqlite::params_from_iter(&statement.values),
            )
            .map_err(|error| rejection(error, index, &change.set))?;
        if affected != 1 {
            return Err(conflict(index, affected));
        }
    }
    // Dropping the transaction on any early return above is the rollback.
    transaction.commit().map_err(write_failure)?;
    Ok(UpdateRowsResult {
        updated: statements.len() as u64,
    })
}

#[derive(Debug)]
pub struct Statement {
    pub sql: String,
    pub values: Vec<Value>,
    /// Reads the storage class of each column about to be set, from the row the
    /// key names. Its parameters are `values` from `set_columns.len()` on.
    pub probe: String,
    pub set_columns: Vec<String>,
}

const BINARY_REFUSAL: &str = "binary columns cannot be edited as text";

/// One change as a parameterised UPDATE. Pure, so the quoting and the typing
/// are tested without a database.
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
        if declares_bytes(&column.type_name) {
            return Err(invalid(BINARY_REFUSAL, index, Some(&column.name)));
        }
        values.push(coerce(&set.value, column).map_err(|reason| {
            invalid(
                &format!("{}: {reason}", column.name),
                index,
                Some(&column.name),
            )
        })?);
        assignments.push(format!(
            "{} = ?",
            introspect::quote_identifier(&column.name)
        ));
    }

    let mut conditions = Vec::with_capacity(change.key.len());
    for key in &change.key {
        let column = known(columns, &key.column, index)?;
        if key.value == EditValue::Null {
            return Err(invalid("a key column is NULL", index, Some(&column.name)));
        }
        values.push(coerce(&key.value, column).map_err(|reason| {
            invalid(
                &format!("key {}: {reason}", column.name),
                index,
                Some(&column.name),
            )
        })?);
        let quoted = introspect::quote_identifier(&column.name);
        // An untyped column keeps whatever storage class it was given, which
        // the grid's text cannot name. Compared as text; the exactly-one-row
        // rule catches the odd case where that matches more than it should.
        conditions.push(if affinity(&column.type_name) == Affinity::None {
            format!("CAST({quoted} AS TEXT) = ?")
        } else {
            format!("{quoted} = ?")
        });
    }

    let set_columns: Vec<String> = change.set.iter().map(|set| set.column.clone()).collect();
    let target = format!(
        "{}.{}",
        introspect::quote_identifier(schema),
        introspect::quote_identifier(table)
    );
    let conditions = conditions.join(" AND ");
    Ok(Statement {
        // `OR ABORT` is not the default being restated. A statement-level
        // clause overrides the column's own `ON CONFLICT`: with `REPLACE` on a
        // unique column, SQLite deletes the *other* row holding the value and
        // reports one change; with `IGNORE` on a NOT NULL it skips the row
        // without an error. Both look like success or like a vanished row, and
        // neither is what was typed.
        sql: format!(
            "UPDATE OR ABORT {target} SET {} WHERE {conditions}",
            assignments.join(", ")
        ),
        probe: format!(
            "SELECT {} FROM {target} WHERE {conditions}",
            set_columns
                .iter()
                .map(|name| format!("typeof({})", introspect::quote_identifier(name)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        set_columns,
        values,
    })
}

/// A declared type that holds bytes: `BLOB`, which SQLite also gives an
/// untyped column's affinity, so a blank declaration is not one.
fn declares_bytes(declared_type: &str) -> bool {
    !declared_type.trim().is_empty() && affinity(declared_type) == Affinity::None
}

/// The first column about to be set whose current value is a blob, if any.
///
/// SQLite lets any column hold any storage class, so a declared type cannot say
/// whether a cell is bytes. The grid shows only the size of one, so text typed
/// over it was never what the person meant to write.
fn holds_bytes(
    transaction: &rusqlite::Transaction<'_>,
    statement: &Statement,
) -> Result<Option<String>, rusqlite::Error> {
    let key = statement
        .values
        .get(statement.set_columns.len()..)
        .unwrap_or_default();
    let classes: Option<Vec<String>> = transaction
        .query_row(&statement.probe, rusqlite::params_from_iter(key), |row| {
            (0..statement.set_columns.len())
                .map(|index| row.get::<_, String>(index))
                .collect()
        })
        .optional()?;
    Ok(classes.and_then(|classes| {
        classes
            .iter()
            .position(|class| class == "blob")
            .and_then(|at| statement.set_columns.get(at).cloned())
    }))
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

#[derive(Debug, PartialEq, Eq)]
enum Affinity {
    Integer,
    Real,
    Numeric,
    Text,
    None,
}

/// SQLite's own rules for turning a declared type into an affinity
/// (datatype3.html, section 3.1), in the order it applies them.
fn affinity(declared_type: &str) -> Affinity {
    let declared = declared_type.to_ascii_uppercase();
    if declared.contains("INT") {
        Affinity::Integer
    } else if ["CHAR", "CLOB", "TEXT"]
        .iter()
        .any(|word| declared.contains(word))
    {
        Affinity::Text
    } else if declared.is_empty() || declared.contains("BLOB") {
        Affinity::None
    } else if ["REAL", "FLOA", "DOUBLE"]
        .iter()
        .any(|word| declared.contains(word))
    {
        Affinity::Real
    } else {
        Affinity::Numeric
    }
}

/// The text the person typed, as the storage class the column would give it.
///
/// SQLite itself would accept anything anywhere; refusing what is not a number
/// for a number column is the driver's job, because that is the error the
/// person wants shown on the cell.
fn coerce(value: &EditValue, column: &ColumnDef) -> Result<Value, String> {
    let EditValue::Text { value } = value else {
        return Ok(Value::Null);
    };
    if values::is_boolean(&column.type_name) {
        return match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => Ok(Value::Integer(1)),
            "false" | "0" => Ok(Value::Integer(0)),
            _ => Err("expected true or false".into()),
        };
    }
    match affinity(&column.type_name) {
        Affinity::Integer => number(value).ok_or_else(|| "expected a number".into()),
        Affinity::Real => number(value)
            .map(|number| match number {
                Value::Integer(integer) => Value::Real(integer as f64),
                other => other,
            })
            .ok_or_else(|| "expected a number".into()),
        // Bound as text: SQLite applies NUMERIC affinity itself, and it is the
        // one affinity that legitimately keeps text it cannot convert.
        Affinity::Numeric | Affinity::Text | Affinity::None => Ok(Value::Text(value.clone())),
    }
}

/// An integer when it is one, else a finite real -- which is what SQLite does
/// with a number it cannot store as an integer.
fn number(text: &str) -> Option<Value> {
    let text = text.trim();
    text.parse::<i64>().map(Value::Integer).ok().or_else(|| {
        text.parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .map(Value::Real)
    })
}

fn invalid(message: &str, change: usize, column: Option<&str>) -> ResponseError {
    ResponseError::new(ErrorCode::InvalidValue, message).with_data(ErrorData {
        change: Some(change as u64),
        column: column.map(str::to_string),
    })
}

fn conflict(change: usize, matched: usize) -> ResponseError {
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

/// What SQLite refused about a value becomes `invalid_value`; anything else
/// keeps its ordinary mapping, except a file another process holds, which
/// `write_failure` names.
fn rejection(error: rusqlite::Error, change: usize, set: &[ColumnValue]) -> ResponseError {
    let value_refused = matches!(
        &error,
        rusqlite::Error::SqliteFailure(failure, _) if matches!(
            failure.code,
            rusqlite::ErrorCode::ConstraintViolation
                | rusqlite::ErrorCode::TypeMismatch
                | rusqlite::ErrorCode::TooBig
        )
    );
    if !value_refused {
        return write_failure(error);
    }
    let detail = database::protocol::error_chain(&error);
    let message = match &error {
        rusqlite::Error::SqliteFailure(_, Some(message)) => message.clone(),
        _ => detail.clone(),
    };
    let column = constraint_column(&message, set);
    ResponseError::new(ErrorCode::InvalidValue, message)
        .with_detail(detail)
        .with_data(ErrorData {
            change: Some(change as u64),
            column,
        })
}

/// A failure of the write that is not about a value.
///
/// A busy or locked file is `internal` with a sentence of its own, not
/// `connection`: the connection is fine, another writer simply has the file,
/// and `conflict` would tell the grid the row changed. The same sorting as the
/// other drivers give a lock timeout.
fn write_failure(error: rusqlite::Error) -> ResponseError {
    let locked = matches!(
        &error,
        rusqlite::Error::SqliteFailure(failure, _) if matches!(
            failure.code,
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
        )
    );
    if !locked {
        return values::error(error);
    }
    ResponseError::new(
        ErrorCode::Internal,
        format!(
            "the database file is locked by another process (waited {} s), so nothing was \
             saved -- try again when it is done",
            BUSY_TIMEOUT.as_secs()
        ),
    )
    .with_detail(database::protocol::error_chain(&error))
}

/// `NOT NULL constraint failed: users.email`, `UNIQUE constraint failed: t.a, t.b`.
///
/// Only a column the change was setting is believed: a unique index can fail
/// on a column nobody touched, and pointing at it would mark the wrong cell.
fn constraint_column(message: &str, set: &[ColumnValue]) -> Option<String> {
    let named = message
        .split_once("constraint failed: ")
        .map(|(_, tail)| tail);
    let from_message = named
        .into_iter()
        .flat_map(|tail| tail.split(", "))
        .find_map(|part| {
            let name = part.rsplit('.').next().unwrap_or(part);
            set.iter()
                .find(|column| column.column == name)
                .map(|column| column.column.clone())
        });
    from_message.or_else(|| match set {
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

    fn pair(name: &str, value: &str) -> ColumnValue {
        ColumnValue {
            column: name.into(),
            value: EditValue::Text {
                value: value.into(),
            },
        }
    }

    fn table() -> Vec<ColumnDef> {
        vec![
            column("id", "INTEGER"),
            column("we\"ird", "TEXT"),
            column("loose", ""),
        ]
    }

    #[test]
    fn identifiers_are_quoted_and_values_are_placeholders() {
        let change = RowChange {
            key: vec![pair("id", "7")],
            set: vec![pair("we\"ird", "'; DROP TABLE t; --")],
        };
        let statement = build("ma\"in", "t", &table(), &change, 0).expect("builds");
        assert_eq!(
            statement.sql,
            r#"UPDATE OR ABORT "ma""in"."t" SET "we""ird" = ? WHERE "id" = ?"#
        );
        assert_eq!(
            statement.values,
            vec![Value::Text("'; DROP TABLE t; --".into()), Value::Integer(7)]
        );
    }

    #[test]
    fn an_untyped_key_column_is_compared_as_text() {
        let change = RowChange {
            key: vec![pair("loose", "7")],
            set: vec![pair("id", "1")],
        };
        let statement = build("main", "t", &table(), &change, 0).expect("builds");
        assert!(
            statement
                .sql
                .ends_with(r#"WHERE CAST("loose" AS TEXT) = ?"#)
        );
    }

    #[test]
    fn affinity_follows_the_order_sqlite_applies_its_rules() {
        assert_eq!(affinity("BIGINT"), Affinity::Integer);
        assert_eq!(affinity("CHARINT"), Affinity::Integer, "INT wins over CHAR");
        assert_eq!(affinity("VARCHAR(20)"), Affinity::Text);
        assert_eq!(affinity(""), Affinity::None);
        assert_eq!(affinity("DOUBLE PRECISION"), Affinity::Real);
        assert_eq!(affinity("DECIMAL(10,2)"), Affinity::Numeric);
        assert_eq!(affinity("BOOLEAN"), Affinity::Numeric);
    }

    #[test]
    fn a_constraint_message_points_at_a_column_only_when_it_was_being_set() {
        let set = [pair("email", "x")];
        let both = [pair("email", "x"), pair("age", "1")];
        assert_eq!(
            constraint_column("NOT NULL constraint failed: users.email", &set).as_deref(),
            Some("email")
        );
        assert_eq!(
            constraint_column("UNIQUE constraint failed: users.a, users.age", &both).as_deref(),
            Some("age")
        );
        assert_eq!(
            constraint_column("UNIQUE constraint failed: users.handle", &both),
            None,
            "an untouched column must not be blamed"
        );
        assert_eq!(
            constraint_column("CHECK constraint failed: positive", &set).as_deref(),
            Some("email"),
            "with one column being set there is only one to blame"
        );
    }

    #[test]
    fn a_change_that_sets_nothing_or_sets_a_column_twice_is_refused() {
        let nothing = RowChange {
            key: vec![pair("id", "1")],
            set: vec![],
        };
        assert!(build("main", "t", &table(), &nothing, 0).is_err());
        let twice = RowChange {
            key: vec![pair("id", "1")],
            set: vec![pair("loose", "a"), pair("loose", "b")],
        };
        assert!(build("main", "t", &table(), &twice, 0).is_err());
    }

    #[test]
    fn every_update_overrides_a_columns_own_conflict_clause() {
        let change = RowChange {
            key: vec![pair("id", "7")],
            set: vec![pair("loose", "x")],
        };
        let statement = build("main", "t", &table(), &change, 0).expect("builds");
        assert!(
            statement.sql.starts_with("UPDATE OR ABORT "),
            "{}",
            statement.sql
        );
        assert_eq!(
            statement.probe,
            r#"SELECT typeof("loose") FROM "main"."t" WHERE "id" = ?"#
        );
    }

    #[test]
    fn a_declared_blob_is_refused_and_an_untyped_column_is_not() {
        let columns = vec![
            column("id", "INTEGER"),
            column("data", "BLOB"),
            column("note", "TEXT"),
            column("loose", ""),
        ];
        let set = |name: &str| RowChange {
            key: vec![pair("id", "1")],
            set: vec![pair(name, "x")],
        };
        let error = build("main", "t", &columns, &set("data"), 2).expect_err("bytes");
        assert_eq!(error.code, ErrorCode::InvalidValue);
        assert_eq!(error.message, BINARY_REFUSAL);
        assert_eq!(
            error.data.and_then(|data| data.column).as_deref(),
            Some("data")
        );
        assert!(build("main", "t", &columns, &set("note"), 0).is_ok());
        assert!(
            build("main", "t", &columns, &set("loose"), 0).is_ok(),
            "an untyped column is judged by what it holds, at write time"
        );
    }

    #[test]
    fn a_busy_or_locked_file_is_internal_and_anything_else_keeps_its_mapping() {
        let failure = |code| {
            rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), Some("busy".into()))
        };
        for code in [rusqlite::ffi::SQLITE_BUSY, rusqlite::ffi::SQLITE_LOCKED] {
            let mapped = write_failure(failure(code));
            assert_eq!(mapped.code, ErrorCode::Internal, "{code}");
            assert!(
                mapped.message.contains("nothing was saved"),
                "{}",
                mapped.message
            );
        }
        assert_eq!(
            write_failure(failure(rusqlite::ffi::SQLITE_READONLY)).code,
            ErrorCode::ReadOnly
        );
        assert_eq!(
            write_failure(failure(rusqlite::ffi::SQLITE_CANTOPEN)).code,
            ErrorCode::Connection
        );
    }
}
