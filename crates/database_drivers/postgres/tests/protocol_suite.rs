//! The shared protocol suite, run against a real PostgreSQL server.
//!
//! Skipped, loudly, when `ZODE_TEST_POSTGRES_URL` names no server. Everything
//! else in this crate tests the pieces a statement is built from; this is the
//! only test that finds out whether a server accepts them.
//!
//! ```sh
//! ZODE_TEST_POSTGRES_URL=postgres://zode@localhost/zode_test \
//!     cargo test -p zode-db-postgres
//! ```

use database::driver_test_suite::{
    Dialect, RowUpdateFixture, row_update_suite, server_url, shared_suite,
};
use std::path::Path;

#[test]
fn the_postgres_driver_answers_the_protocol() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };

    shared_suite(
        Path::new(env!("CARGO_BIN_EXE_zode-db-postgres")),
        &url,
        Dialect {
            write_statement: "CREATE TABLE zode_probe (x integer)",
            // Cast, because an untyped NULL and an untyped literal leave the
            // server unable to say what type it is sending.
            null_and_empty: "SELECT NULL::text AS a, ''::text AS b",
            three_rows: "SELECT 1 AS n UNION ALL SELECT 2 UNION ALL SELECT 3 ORDER BY n",
        },
    );
}

/// The schema this run created, dropped even if the suite panics.
struct Schema {
    url: String,
    name: String,
}

impl Drop for Schema {
    fn drop(&mut self) {
        let dropped =
            postgres::Client::connect(&self.url, postgres::NoTls).and_then(|mut client| {
                client.batch_execute(&format!("DROP SCHEMA IF EXISTS {} CASCADE", self.name))
            });
        if let Err(error) = dropped {
            eprintln!("could not drop {}: {error}", self.name);
        }
    }
}

#[test]
fn the_postgres_driver_updates_rows_as_the_protocol_says() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    // The fixture wants `&'static str`; a test process that leaks one short
    // name is the cheaper way to give each run a schema of its own.
    let name: &'static str =
        Box::leak(format!("zode_suite_{}", std::process::id()).into_boxed_str());
    let _cleanup = Schema {
        url: url.clone(),
        name: name.to_string(),
    };
    postgres::Client::connect(&url, postgres::NoTls)
        .and_then(|mut client| {
            client.batch_execute(&format!(
                "DROP SCHEMA IF EXISTS {name} CASCADE;
                 CREATE SCHEMA {name};
                 CREATE TABLE {name}.row_edits (id integer PRIMARY KEY, age integer);
                 INSERT INTO {name}.row_edits VALUES (1, 30);"
            ))
        })
        .expect("the fixture table");

    row_update_suite(
        Path::new(env!("CARGO_BIN_EXE_zode-db-postgres")),
        &url,
        RowUpdateFixture {
            schema: name,
            table: "row_edits",
            key_column: "id",
            key_value: "1",
            missing_key_value: "999",
            set_column: "age",
            new_value: "31",
            bad_value: "not a number",
            read_back: Box::leak(
                format!("SELECT age FROM {name}.row_edits WHERE id = 1").into_boxed_str(),
            ),
        },
    );
}
