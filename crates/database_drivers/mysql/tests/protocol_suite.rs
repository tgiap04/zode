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

use database::driver_test_suite::{
    Dialect, RowUpdateFixture, row_update_suite, server_url, shared_suite,
};
use mysql::prelude::Queryable as _;
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

/// The database this run created, dropped even if the suite panics.
struct Database {
    url: String,
    name: &'static str,
}

impl Database {
    fn connection(url: &str) -> mysql::Conn {
        mysql::Conn::new(mysql::Opts::from_url(url).expect("a URL")).expect("a connection")
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        let dropped = Self::connection(&self.url)
            .query_drop(format!("DROP DATABASE IF EXISTS `{}`", self.name));
        if let Err(error) = dropped {
            eprintln!("could not drop {}: {error}", self.name);
        }
    }
}

#[test]
fn the_mysql_driver_updates_rows_as_the_protocol_says() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
        return;
    };
    // The fixture wants `&'static str`; a test process that leaks one short
    // name is the cheaper way to give each run a database of its own.
    let name: &'static str =
        Box::leak(format!("zode_suite_{}", std::process::id()).into_boxed_str());
    let database = Database {
        url: url.clone(),
        name,
    };
    let mut setup = Database::connection(&url);
    for statement in [
        format!("DROP DATABASE IF EXISTS `{name}`"),
        format!("CREATE DATABASE `{name}`"),
        format!("CREATE TABLE `{name}`.row_edits (id INT PRIMARY KEY, age INT)"),
        format!("INSERT INTO `{name}`.row_edits VALUES (1, 30)"),
    ] {
        setup.query_drop(statement).expect("the fixture table");
    }

    row_update_suite(
        Path::new(env!("CARGO_BIN_EXE_zode-db-mysql")),
        &url,
        RowUpdateFixture {
            schema: database.name,
            table: "row_edits",
            key_column: "id",
            key_value: "1",
            missing_key_value: "999",
            set_column: "age",
            new_value: "31",
            bad_value: "not a number",
            read_back: Box::leak(
                format!("SELECT age FROM `{name}`.row_edits WHERE id = 1").into_boxed_str(),
            ),
        },
    );
}
