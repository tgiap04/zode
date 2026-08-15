//! The shared protocol suite, run against a real MySQL server.
//!
//! Skipped, loudly, when `ZODE_TEST_MYSQL_URL` names no server.
//!
//! ```sh
//! ZODE_TEST_MYSQL_URL=mysql://zode@localhost:3306/zode_test \
//!     cargo test -p zode-db-mysql
//! ```
//!
//! The paging assertions are the ones worth having here: this is the driver
//! that has no cursor and wraps the statement instead, so whether a page really
//! pages is a question only a server can answer.

use database::driver_test_suite::{Dialect, server_url, shared_suite};
use std::path::Path;

#[test]
fn the_mysql_driver_answers_the_protocol() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
        return;
    };

    shared_suite(
        Path::new(env!("CARGO_BIN_EXE_zode-db-mysql")),
        &url,
        Dialect {
            write_statement: "CREATE TABLE zode_probe (x INT)",
            null_and_empty: "SELECT NULL AS a, '' AS b",
            three_rows: "SELECT 1 AS n UNION ALL SELECT 2 UNION ALL SELECT 3 ORDER BY n",
        },
    );
}
