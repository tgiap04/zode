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

use database::driver_test_suite::{
    Dialect, RowUpdateFixture, row_update_suite, server_url, shared_suite,
};
use mongodb::bson::{Document, doc, oid::ObjectId};
use mongodb::options::ClientOptions;
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

/// The row-update suite needs a document to edit, and the driver cannot write
/// one -- so this test seeds it through the client directly, and removes it
/// afterwards even when the suite fails.
///
/// `count` is an int on purpose: a string field accepts anything typed, so only
/// a typed field can show the driver rejecting a value.
#[test]
fn the_mongodb_driver_updates_rows_atomically() {
    let Some(url) = server_url("ZODE_TEST_MONGODB_URL") else {
        return;
    };

    let options = ClientOptions::parse(&url).run().expect("a well-formed URL");
    let database_name = options
        .default_database
        .clone()
        .unwrap_or_else(|| "test".to_string());
    let client = mongodb::sync::Client::with_options(options).expect("a client");
    let collection = client
        .database(&database_name)
        .collection::<Document>("zode_edit_probe");
    let present = ObjectId::new();
    let missing = ObjectId::new();
    collection
        .insert_one(doc! { "_id": present, "count": 7, "name": "probe" })
        .run()
        .expect("seed the fixture document");

    // The fixture wants `'static` text; a test process is short-lived, so
    // leaking a few strings is the plainest way to give it that.
    let leak = |text: String| -> &'static str { Box::leak(text.into_boxed_str()) };
    let schema = leak(database_name.clone());
    let present_hex = leak(present.to_hex());
    let missing_hex = leak(missing.to_hex());
    let read_back = leak(format!(
        r#"{{"find": "zode_edit_probe", "$db": "{database_name}", "filter": {{"_id": {{"$oid": "{present_hex}"}}}}, "projection": {{"_id": 0, "count": 1}}}}"#,
    ));

    // `catch_unwind` so a failed assertion still reaches the cleanup below.
    let outcome = std::panic::catch_unwind(|| {
        row_update_suite(
            Path::new(env!("CARGO_BIN_EXE_zode-db-mongodb")),
            &url,
            RowUpdateFixture {
                schema,
                table: "zode_edit_probe",
                key_column: "_id",
                key_value: present_hex,
                missing_key_value: missing_hex,
                set_column: "count",
                new_value: "42",
                bad_value: "not a number",
                read_back,
            },
        );
    });
    collection
        .delete_one(doc! { "_id": present })
        .run()
        .expect("remove the fixture document");
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
