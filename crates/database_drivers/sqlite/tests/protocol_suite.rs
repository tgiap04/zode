//! The shared protocol suite, run against the real binary.
//!
//! This is the one driver that needs no server, so this is the run that keeps
//! the harness itself honest: the PostgreSQL and MySQL suites are skipped
//! wherever their servers are absent, and a harness only ever exercised in
//! those runs could break without anyone noticing.

use database::driver_test_suite::{Dialect, shared_suite};
use std::path::Path;

/// SQLite opens read-only, so the file has to exist and have something in it
/// before the driver ever sees it.
fn database_with_a_table(directory: &Path) -> String {
    let path = directory.join("suite.sqlite");
    let connection = rusqlite::Connection::open(&path).expect("a writable temporary database");
    connection
        .execute_batch("CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT);")
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
