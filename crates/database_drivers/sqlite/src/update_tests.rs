//! `update_rows` against a real SQLite file: what an edit must do, and every
//! way it must refuse to.

use super::*;
use crate::tests::{connected, fixture, query};
use database::protocol::{Cell, UpdateRowsResult};
use serde_json::{Value, json};
use std::path::PathBuf;

fn text(value: &str) -> Value {
    json!({ "kind": "text", "value": value })
}

fn null() -> Value {
    json!({ "kind": "null" })
}

fn column(name: &str, value: Value) -> Value {
    json!({ "column": name, "value": value })
}

/// One change: the key columns, then the columns to set.
fn change(key: &[(&str, Value)], set: &[(&str, Value)]) -> Value {
    json!({
        "key": key.iter().map(|(name, value)| column(name, value.clone())).collect::<Vec<_>>(),
        "set": set.iter().map(|(name, value)| column(name, value.clone())).collect::<Vec<_>>(),
    })
}

fn update(
    driver: &SqliteDriver,
    id: &ConnectionId,
    table: &str,
    changes: Vec<Value>,
) -> Result<UpdateRowsResult, ResponseError> {
    driver
        .update_rows(json!({
            "connection_id": id, "schema": "main", "table": table, "changes": changes,
        }))
        .map(|value| serde_json::from_value(value).expect("an answer that decodes"))
}

fn read(driver: &SqliteDriver, id: &ConnectionId, sql: &str) -> Cell {
    let page = query(driver, id, sql).expect("the read-back");
    assert_eq!(page.rows.len(), 1, "{sql} must find one row");
    page.rows[0][0].clone()
}

fn user_email(driver: &SqliteDriver, id: &ConnectionId, user: u32) -> Cell {
    read(
        driver,
        id,
        &format!("SELECT email FROM users WHERE id = {user}"),
    )
}

/// The shared fixture plus tables that can refuse a value, written before the
/// driver opens the file read-only.
fn constrained() -> (tempfile::TempDir, PathBuf, SqliteDriver, ConnectionId) {
    let (directory, path) = fixture();
    Connection::open(&path)
        .expect("reopening the fixture")
        .execute_batch(
            "CREATE TABLE accounts (
                 id INTEGER PRIMARY KEY,
                 handle TEXT NOT NULL UNIQUE,
                 balance INTEGER,
                 ratio REAL,
                 price DECIMAL(10, 2),
                 loose
             );
             INSERT INTO accounts VALUES (1, 'ann', 10, 0.5, 1.10, 'x');
             INSERT INTO accounts VALUES (2, 'bob', 20, 1.5, 2.20, 'y');
             INSERT INTO enrolment VALUES (1, 'math', 'A');
             INSERT INTO enrolment VALUES (1, 'art', 'B');
             CREATE TABLE replacing (id INTEGER PRIMARY KEY, a TEXT UNIQUE ON CONFLICT REPLACE);
             INSERT INTO replacing VALUES (1, 'x'), (2, 'y');
             CREATE TABLE ignoring (
                 id INTEGER PRIMARY KEY, a TEXT NOT NULL ON CONFLICT IGNORE, b TEXT
             );
             INSERT INTO ignoring VALUES (1, 'p', NULL);
             CREATE TABLE blobs (id INTEGER PRIMARY KEY, data BLOB, note TEXT, loose);
             INSERT INTO blobs VALUES (1, x'0102', 'text', x'0304');
             INSERT INTO blobs VALUES (2, NULL, x'05', 'text');",
        )
        .expect("seeding the constrained tables");
    let (driver, id) = connected(&path);
    (directory, path, driver, id)
}

#[test]
fn an_edited_cell_reaches_the_row() {
    let (_dir, _path, driver, id) = constrained();
    let applied = update(
        &driver,
        &id,
        "users",
        vec![change(&[("id", text("1"))], &[("email", text("z@b.com"))])],
    )
    .expect("a valid edit");
    assert_eq!(applied.updated, 1);
    assert_eq!(
        user_email(&driver, &id, 1),
        Cell::Text {
            value: "z@b.com".into()
        }
    );
    assert_eq!(
        user_email(&driver, &id, 2),
        Cell::Null,
        "no other row moves"
    );
}

#[test]
fn null_and_the_empty_string_are_different_edits() {
    let (_dir, _path, driver, id) = constrained();
    let set = |value: Value| {
        update(
            &driver,
            &id,
            "users",
            vec![change(&[("id", text("1"))], &[("email", value)])],
        )
        .expect("a valid edit");
    };
    set(text(""));
    assert_eq!(user_email(&driver, &id, 1), Cell::Text { value: "".into() });
    set(null());
    assert_eq!(user_email(&driver, &id, 1), Cell::Null);
}

#[test]
fn a_value_is_stored_as_text_and_never_run_as_sql() {
    let (_dir, _path, driver, id) = constrained();
    let hostile = "x'); DROP TABLE users; --";
    update(
        &driver,
        &id,
        "users",
        vec![change(&[("id", text("1"))], &[("email", text(hostile))])],
    )
    .expect("any text is a value");
    assert_eq!(
        user_email(&driver, &id, 1),
        Cell::Text {
            value: hostile.into()
        }
    );
    assert_eq!(
        read(&driver, &id, "SELECT count(*) FROM users"),
        Cell::Number { value: "2".into() }
    );
}

#[test]
fn an_identifier_that_is_not_in_the_table_is_refused_and_named() {
    let (_dir, _path, driver, id) = constrained();
    for (key, set, named) in [
        ("id", "email\" = 'x', \"id", "email\" = 'x', \"id"),
        ("id", "nope", "nope"),
        ("nope", "email", "nope"),
    ] {
        let error = update(
            &driver,
            &id,
            "users",
            vec![change(&[(key, text("1"))], &[(set, text("q"))])],
        )
        .expect_err("a column the table does not have");
        assert_eq!(error.code, ErrorCode::InvalidValue);
        assert_eq!(
            error.data.and_then(|data| data.column).as_deref(),
            Some(named)
        );
    }
    assert_eq!(
        user_email(&driver, &id, 1),
        Cell::Text {
            value: "a@b.com".into()
        }
    );
}

#[test]
fn a_table_that_is_not_there_or_is_a_view_is_not_written_to() {
    let (_dir, _path, driver, id) = constrained();
    for table in ["active_users", "missing", "sqlite_master"] {
        let error = update(
            &driver,
            &id,
            table,
            vec![change(&[("id", text("1"))], &[("id", text("9"))])],
        )
        .expect_err("not an ordinary table");
        assert_eq!(error.code, ErrorCode::Syntax, "{table}");
    }
}

#[test]
fn a_change_with_no_key_or_a_null_key_is_refused() {
    let (_dir, _path, driver, id) = constrained();
    let keyless = update(
        &driver,
        &id,
        "users",
        vec![change(&[], &[("email", text("q"))])],
    )
    .expect_err("no key would address every row");
    assert_eq!(keyless.code, ErrorCode::InvalidValue);

    let null_key = update(
        &driver,
        &id,
        "users",
        vec![change(&[("id", null())], &[("email", text("q"))])],
    )
    .expect_err("NULL matches nothing");
    assert_eq!(null_key.code, ErrorCode::InvalidValue);
    assert_eq!(
        null_key.data.and_then(|data| data.column).as_deref(),
        Some("id")
    );
}

#[test]
fn a_row_that_is_gone_is_a_conflict_that_names_the_change() {
    let (_dir, _path, driver, id) = constrained();
    let error = update(
        &driver,
        &id,
        "users",
        vec![
            change(&[("id", text("1"))], &[("email", text("first"))]),
            change(&[("id", text("99"))], &[("email", text("second"))]),
        ],
    )
    .expect_err("the second change matches nothing");
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(error.data.and_then(|data| data.change), Some(1));
    assert_eq!(
        user_email(&driver, &id, 1),
        Cell::Text {
            value: "a@b.com".into()
        },
        "the batch is atomic: the good change is rolled back too"
    );
}

#[test]
fn a_key_that_is_not_the_whole_key_is_a_conflict_and_changes_nothing() {
    let (_dir, _path, driver, id) = constrained();
    let error = update(
        &driver,
        &id,
        "enrolment",
        vec![change(&[("student", text("1"))], &[("grade", text("F"))])],
    )
    .expect_err("student 1 has two rows");
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(
        read(
            &driver,
            &id,
            "SELECT group_concat(grade) FROM (SELECT grade FROM enrolment WHERE student = 1 ORDER BY course)"
        ),
        Cell::Text {
            value: "B,A".into()
        }
    );

    let applied = update(
        &driver,
        &id,
        "enrolment",
        vec![change(
            &[("student", text("1")), ("course", text("art"))],
            &[("grade", text("C"))],
        )],
    )
    .expect("the whole key names one row");
    assert_eq!(applied.updated, 1);
    assert_eq!(
        read(
            &driver,
            &id,
            "SELECT grade FROM enrolment WHERE course = 'art'"
        ),
        Cell::Text { value: "C".into() }
    );
}

#[test]
fn a_constraint_rejection_rolls_the_batch_back_and_names_the_column() {
    let (_dir, _path, driver, id) = constrained();
    for (set, value) in [("handle", null()), ("handle", text("bob"))] {
        let error = update(
            &driver,
            &id,
            "accounts",
            vec![
                change(&[("id", text("1"))], &[("balance", text("11"))]),
                change(&[("id", text("1"))], &[(set, value)]),
            ],
        )
        .expect_err("NOT NULL and UNIQUE both refuse");
        assert_eq!(error.code, ErrorCode::InvalidValue);
        let data = error.data.expect("where it went wrong");
        assert_eq!(data.change, Some(1));
        assert_eq!(data.column.as_deref(), Some("handle"));
        assert_eq!(
            read(&driver, &id, "SELECT balance FROM accounts WHERE id = 1"),
            Cell::Number { value: "10".into() },
            "the first change is rolled back with the second"
        );
    }
}

#[test]
fn text_that_is_not_a_number_is_refused_for_a_number_column() {
    let (_dir, _path, driver, id) = constrained();
    for (set, bad) in [
        ("balance", "12abc"),
        ("ratio", "abc"),
        ("balance", ""),
        ("ratio", "NaN"),
    ] {
        let error = update(
            &driver,
            &id,
            "accounts",
            vec![change(&[("id", text("1"))], &[(set, text(bad))])],
        )
        .expect_err("not a number");
        assert_eq!(error.code, ErrorCode::InvalidValue, "{set} = {bad:?}");
        assert_eq!(
            error.data.and_then(|data| data.column).as_deref(),
            Some(set)
        );
    }
    let error = update(
        &driver,
        &id,
        "accounts",
        vec![change(&[("id", text("one"))], &[("balance", text("1"))])],
    )
    .expect_err("a key is typed too");
    assert_eq!(error.code, ErrorCode::InvalidValue);
    assert_eq!(
        read(&driver, &id, "SELECT balance FROM accounts WHERE id = 1"),
        Cell::Number { value: "10".into() }
    );
}

#[test]
fn numbers_are_stored_as_numbers_and_other_columns_keep_their_text() {
    let (_dir, _path, driver, id) = constrained();
    update(
        &driver,
        &id,
        "accounts",
        vec![change(
            &[("id", text("1"))],
            &[
                ("balance", text(" 42 ")),
                ("ratio", text("2")),
                ("price", text("3.30")),
                ("loose", text("7")),
            ],
        )],
    )
    .expect("valid edits");
    let types = read(
        &driver,
        &id,
        "SELECT typeof(balance) || typeof(ratio) || typeof(price) || typeof(loose) \
         FROM accounts WHERE id = 1",
    );
    assert_eq!(
        types,
        Cell::Text {
            value: "integerrealrealtext".into()
        },
        "INTEGER/REAL/NUMERIC affinity applies; an untyped column keeps what it was given"
    );
}

#[test]
fn a_key_is_found_whatever_storage_class_its_column_holds() {
    let (_dir, _path, driver, id) = constrained();
    // `loose` has no declared type and holds text; a key shown as text must
    // still find it.
    let applied = update(
        &driver,
        &id,
        "accounts",
        vec![change(&[("loose", text("y"))], &[("balance", text("21"))])],
    )
    .expect("an untyped key column");
    assert_eq!(applied.updated, 1);
    assert_eq!(
        read(&driver, &id, "SELECT balance FROM accounts WHERE id = 2"),
        Cell::Number { value: "21".into() }
    );
    // And the REAL and DECIMAL keys, which are displayed as digits.
    let applied = update(
        &driver,
        &id,
        "accounts",
        vec![
            change(&[("ratio", text("0.5"))], &[("balance", text("1"))]),
            change(&[("price", text("2.2"))], &[("balance", text("2"))]),
        ],
    )
    .expect("numeric keys");
    assert_eq!(applied.updated, 2);
}

#[test]
fn a_boolean_is_written_the_way_the_grid_shows_it() {
    let (_dir, _path, driver, id) = constrained();
    update(
        &driver,
        &id,
        "users",
        vec![change(&[("id", text("1"))], &[("active", text("false"))])],
    )
    .expect("`false` into a BOOLEAN column");
    assert_eq!(
        read(&driver, &id, "SELECT active FROM users WHERE id = 1"),
        Cell::Bool { value: false }
    );
    let error = update(
        &driver,
        &id,
        "users",
        vec![change(&[("id", text("1"))], &[("active", text("maybe"))])],
    )
    .expect_err("not a boolean");
    assert_eq!(error.code, ErrorCode::InvalidValue);
}

#[test]
fn writing_the_value_a_cell_already_holds_is_not_a_conflict() {
    let (_dir, _path, driver, id) = constrained();
    let applied = update(
        &driver,
        &id,
        "users",
        vec![change(&[("id", text("1"))], &[("email", text("a@b.com"))])],
    )
    .expect("the same value again");
    assert_eq!(applied.updated, 1);
}

#[test]
fn an_empty_batch_writes_nothing_and_an_unknown_connection_is_refused() {
    let (_dir, _path, driver, id) = constrained();
    assert_eq!(update(&driver, &id, "users", vec![]).unwrap().updated, 0);
    let error = update(&driver, &ConnectionId("nope".into()), "users", vec![])
        .expect_err("no such connection");
    assert_eq!(error.code, ErrorCode::UnknownConnection);
}

/// The browsing handle is the one the user types into. Writing through a
/// separate handle must leave it exactly as read-only as before.
#[test]
fn browsing_is_still_read_only_after_an_edit() {
    let (_dir, _path, driver, id) = constrained();
    update(
        &driver,
        &id,
        "users",
        vec![change(&[("id", text("1"))], &[("email", text("z"))])],
    )
    .expect("an edit");
    let refused = query(&driver, &id, "DELETE FROM users").expect_err("still read-only");
    assert_eq!(refused.code, ErrorCode::ReadOnly);
    assert_eq!(
        read(&driver, &id, "SELECT count(*) FROM users"),
        Cell::Number { value: "2".into() }
    );
}

#[cfg(unix)]
#[test]
fn a_file_the_system_will_not_let_us_write_is_read_only_not_a_syntax_error() {
    use std::os::unix::fs::PermissionsExt as _;
    let (_dir, path, driver, id) = constrained();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).expect("chmod");
    let error = update(
        &driver,
        &id,
        "users",
        vec![change(&[("id", text("1"))], &[("email", text("z"))])],
    )
    .expect_err("the file refuses writes");
    assert_eq!(error.code, ErrorCode::ReadOnly);
}

/// A column's own `ON CONFLICT REPLACE` deletes the *other* row that holds the
/// value and then writes this one -- `changes()` says 1, and a row somebody
/// never touched is gone.
#[test]
fn a_column_that_replaces_on_conflict_refuses_instead_of_deleting_another_row() {
    let (_dir, _path, driver, id) = constrained();
    let error = update(
        &driver,
        &id,
        "replacing",
        vec![change(&[("id", text("2"))], &[("a", text("x"))])],
    )
    .expect_err("'x' belongs to row 1");
    assert_eq!(error.code, ErrorCode::InvalidValue, "{error:?}");
    assert_eq!(
        error.data.and_then(|data| data.column).as_deref(),
        Some("a")
    );
    assert_eq!(
        read(
            &driver,
            &id,
            "SELECT group_concat(a) FROM (SELECT a FROM replacing ORDER BY id)"
        ),
        Cell::Text {
            value: "x,y".into()
        },
        "both rows are as they were"
    );
}

/// `NOT NULL ON CONFLICT IGNORE` makes the UPDATE skip the row without an
/// error, which read as a conflict -- the row "changed since it was read" when
/// the value was simply refused.
#[test]
fn a_not_null_column_that_ignores_on_conflict_is_still_an_invalid_value() {
    let (_dir, _path, driver, id) = constrained();
    let error = update(
        &driver,
        &id,
        "ignoring",
        vec![change(&[("id", text("1"))], &[("a", null())])],
    )
    .expect_err("NULL is not allowed");
    assert_eq!(error.code, ErrorCode::InvalidValue, "{error:?}");
    assert_eq!(
        error.data.and_then(|data| data.column).as_deref(),
        Some("a")
    );
    assert_eq!(
        read(&driver, &id, "SELECT a FROM ignoring WHERE id = 1"),
        Cell::Text { value: "p".into() }
    );
}

#[test]
fn bytes_are_never_overwritten_with_text() {
    let (_dir, _path, driver, id) = constrained();
    // (row, column): declared BLOB; an untyped column holding a blob; a TEXT
    // column that happens to hold one.
    for (row, column) in [("1", "data"), ("1", "loose"), ("2", "note")] {
        let error = update(
            &driver,
            &id,
            "blobs",
            vec![change(&[("id", text(row))], &[(column, text("words"))])],
        )
        .expect_err("a blob");
        assert_eq!(error.code, ErrorCode::InvalidValue, "{column}: {error:?}");
        assert!(
            error
                .message
                .contains("binary columns cannot be edited as text"),
            "{}",
            error.message
        );
        assert_eq!(
            error.data.and_then(|data| data.column).as_deref(),
            Some(column)
        );
    }
    assert_eq!(
        read(
            &driver,
            &id,
            "SELECT typeof(data) || typeof(loose) FROM blobs WHERE id = 1"
        ),
        Cell::Text {
            value: "blobblob".into()
        }
    );
    // Text over text is untouched by all this.
    update(
        &driver,
        &id,
        "blobs",
        vec![change(&[("id", text("1"))], &[("note", text("fine"))])],
    )
    .expect("a text cell is editable");
}

/// Another process holding the write lock past the busy timeout.
#[test]
fn a_file_another_writer_holds_is_a_clear_refusal_and_not_a_lost_connection() {
    let (_dir, path, driver, id) = constrained();
    let holder = Connection::open(&path).expect("a second writer");
    holder
        .execute_batch("BEGIN IMMEDIATE")
        .expect("taking the write lock");
    let error = update(
        &driver,
        &id,
        "users",
        vec![change(&[("id", text("1"))], &[("email", text("z"))])],
    )
    .expect_err("the file is locked");
    holder.execute_batch("ROLLBACK").expect("releasing it");
    assert_eq!(error.code, ErrorCode::Internal, "{error:?}");
    assert!(error.message.contains("locked"), "{}", error.message);
    assert!(
        error.message.contains("nothing was saved"),
        "{}",
        error.message
    );
}
