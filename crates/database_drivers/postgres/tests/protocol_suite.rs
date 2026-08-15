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

use database::driver_test_suite::{Dialect, server_url, shared_suite};
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
