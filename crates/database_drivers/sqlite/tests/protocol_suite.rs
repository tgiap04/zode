//! The shared protocol suite, run against the real binary.
//!
//! This is the one driver that needs no server, so this is the run that keeps
//! the harness itself honest: the PostgreSQL and MySQL suites are skipped
//! wherever their servers are absent, and a harness only ever exercised in
//! those runs could break without anyone noticing.

use database::driver_test_suite::{Dialect, RowUpdateFixture, row_update_suite, shared_suite};
use std::path::Path;

/// SQLite opens read-only, so the file has to exist and have something in it
/// before the driver ever sees it.
fn database_with_a_table(directory: &Path) -> String {
    let path = directory.join("suite.sqlite");
    let connection = rusqlite::Connection::open(&path).expect("a writable temporary database");
    connection
        .execute_batch(
            "CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE row_edits (id INTEGER PRIMARY KEY, age INTEGER);
             INSERT INTO row_edits VALUES (1, 30);",
        )
        .expect("the fixture table");
    path.to_string_lossy().into_owned()
}

#[test]
fn the_sqlite_driver_answers_the_protocol() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let url = database_with_a_table(directory.path());

    shared_suite(
        Path::new(env!("CARGO_BIN_EXE_zode-db-sqlite")),
        &url,
        Dialect {
            write_statement: "CREATE TABLE zode_probe (x INTEGER)",
            null_and_empty: "SELECT NULL AS a, '' AS b",
            three_rows: "SELECT 1 AS n UNION ALL SELECT 2 UNION ALL SELECT 3 ORDER BY n",
        },
    );
}

#[test]
fn the_sqlite_driver_updates_rows_as_the_protocol_says() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let url = database_with_a_table(directory.path());

    row_update_suite(
        Path::new(env!("CARGO_BIN_EXE_zode-db-sqlite")),
        &url,
        RowUpdateFixture {
            schema: "main",
            table: "row_edits",
            key_column: "id",
            key_value: "1",
            missing_key_value: "999",
            set_column: "age",
            new_value: "31",
            bad_value: "not a number",
            read_back: "SELECT age FROM row_edits WHERE id = 1",
        },
    );
}
