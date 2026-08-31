//! The shared protocol suite, run against a real MongoDB server.
//!
//! Skipped, loudly, when `ZODE_TEST_MONGODB_URL` names no server -- the pattern
//! every other driver here follows. Everything else in this crate tests the
//! pieces a statement is built from; this is the only test that finds out
//! whether a server accepts them.
//!
//! ```sh
//! ZODE_TEST_MONGODB_URL=mongodb://localhost:27017/zode_test \
//!     cargo test -p zode-db-mongodb
//! ```

use database::driver_test_suite::{Dialect, server_url, shared_suite};
use std::path::Path;

#[test]
fn the_mongodb_driver_answers_the_protocol() {
    let Some(url) = server_url("ZODE_TEST_MONGODB_URL") else {
        return;
    };

    shared_suite(
        Path::new(env!("CARGO_BIN_EXE_zode-db-mongodb")),
        &url,
        Dialect {
            // Every one of these is a JSON command document rather than SQL --
            // which is the point: the suite asks the protocol's questions and
            // never learns that this engine has no SQL in it.
            write_statement: r#"{"insert": "zode_probe", "documents": [{"x": 1}]}"#,
            null_and_empty: r#"{"aggregate": "zode_probe", "pipeline": [{"$limit": 1}, {"$project": {"_id": 0, "a": {"$literal": null}, "b": {"$literal": ""}}}]}"#,
            three_rows: r#"{"aggregate": "zode_probe", "pipeline": [{"$limit": 1}, {"$project": {"n": [1, 2, 3]}}, {"$unwind": "$n"}, {"$project": {"_id": 0, "n": 1}}]}"#,
        },
    );
}
