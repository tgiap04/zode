//! Everything here runs against a real SQLite file and no server at all, so it
//! is the same in CI as on a laptop.

use super::*;
use database::protocol::{Cell, TableKind};
use serde_json::json;
use std::io::Cursor;
use std::path::PathBuf;

/// A database with the shapes that break drivers: a table with a primary key,
/// one without, a view, every storage class, and a null.
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("a temp dir");
    let path = directory.path().join("fixture.sqlite");
    let writable = Connection::open(&path).expect("creating the fixture");
    writable
        .execute_batch(
            "CREATE TABLE users (
                 id INTEGER PRIMARY KEY,
                 email TEXT,
                 score REAL,
                 avatar BLOB,
                 active BOOLEAN,
                 seen_at TIMESTAMP
             );
             INSERT INTO users VALUES (1, 'a@b.com', 0.1, x'00010203', 1, '2026-08-15 10:00:00');
             INSERT INTO users VALUES (2, NULL, 2.5, NULL, 0, NULL);
             CREATE TABLE keyless (note TEXT);
             INSERT INTO keyless VALUES ('no primary key here');
             CREATE VIEW active_users AS SELECT id FROM users WHERE active = 1;",
        )
        .expect("seeding the fixture");
    (directory, path)
}

fn connected(path: &PathBuf) -> (SqliteDriver, ConnectionId) {
    let driver = SqliteDriver::default();
    let result = driver
        .connect(json!({ "url": path.display().to_string() }))
        .expect("connecting to the fixture");
    let result: ConnectResult = serde_json::from_value(result).unwrap();
    (driver, result.connection_id)
}

fn query(driver: &SqliteDriver, id: &ConnectionId, sql: &str) -> Result<ResultSet, ResponseError> {
    let value = driver.query(json!({
        "connection_id": id,
        "sql": sql,
        "limit": 200,
        "offset": 0,
        "request_id": "t",
    }))?;
    Ok(serde_json::from_value(value).unwrap())
}

#[test]
fn all_eight_methods_answer_against_a_real_file() {
    let (_dir, path) = fixture();
    let (driver, id) = connected(&path);

    let initialize: InitializeResult =
        serde_json::from_value(driver.initialize(json!({})).unwrap()).unwrap();
    assert_eq!(initialize.protocol_version, PROTOCOL_VERSION);

    let schemas: ListSchemasResult =
        serde_json::from_value(driver.list_schemas(json!({ "connection_id": id })).unwrap())
            .unwrap();
    assert_eq!(
        schemas.schemas.len(),
        1,
        "`temp` exists on every connection and holds nothing the user put there"
    );
    assert!(schemas.schemas[0].is_default);

    let tables: ListTablesResult = serde_json::from_value(
        driver
            .list_tables(json!({ "connection_id": id, "schema": "main" }))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(tables.tables.len(), 3, "two tables and a view");
    assert!(
        tables
            .tables
            .iter()
            .any(|table| table.kind == TableKind::View),
        "a view must not be reported as a table"
    );

    let described: DescribeTableResult = serde_json::from_value(
        driver
            .describe_table(json!({ "connection_id": id, "schema": "main", "table": "users" }))
            .unwrap(),
    )
    .unwrap();
    assert!(described.columns.iter().any(|column| column.primary_key));

    query(&driver, &id, "SELECT 1").unwrap();
    driver
        .cancel(json!({ "connection_id": id, "request_id": "t" }))
        .unwrap();
    driver.disconnect(json!({ "connection_id": id })).unwrap();
}

/// The whole read-only guarantee, exercised through the statements a user can
/// actually type. The last case is the one that matters: `PRAGMA query_only`
/// can be switched off from SQL, so if that were the enforcement there would be
/// no enforcement.
#[test]
fn the_database_itself_refuses_every_write() {
    let (_dir, path) = fixture();
    let (driver, id) = connected(&path);

    for sql in [
        "DELETE FROM users",
        "UPDATE users SET email = 'x'",
        "INSERT INTO users (id) VALUES (99)",
        "DROP TABLE users",
        "CREATE TABLE sneaky (a INTEGER)",
        "ALTER TABLE users ADD COLUMN extra TEXT",
    ] {
        let Err(error) = query(&driver, &id, sql) else {
            panic!("`{sql}` must be refused, but it succeeded");
        };
        assert_eq!(
            error.code,
            ErrorCode::ReadOnly,
            "`{sql}` must be refused as read-only, not as bad SQL: {error:?}"
        );
    }

    // Turning the pragma off is allowed to succeed -- it is only a setting.
    query(&driver, &id, "PRAGMA query_only = OFF").ok();
    let error = query(&driver, &id, "DELETE FROM users")
        .expect_err("the handle is read-only, whatever the pragma says");
    assert_eq!(
        error.code,
        ErrorCode::ReadOnly,
        "a read-only mode the user can switch off by typing is not one"
    );

    // And the file is genuinely untouched.
    let rows = query(&driver, &id, "SELECT count(*) FROM users").unwrap();
    assert_eq!(rows.rows[0][0], Cell::Number { value: "2".into() });
}

#[test]
fn a_page_says_when_there_is_more_and_stops_when_there_is_not() {
    let (_dir, path) = fixture();
    let (driver, id) = connected(&path);

    let page = |limit: u32, offset: u64| -> ResultSet {
        let value = driver
            .query(json!({
                "connection_id": id,
                "sql": "SELECT id FROM users ORDER BY id",
                "limit": limit,
                "offset": offset,
                "request_id": "p",
            }))
            .unwrap();
        serde_json::from_value(value).unwrap()
    };

    let first = page(1, 0);
    assert_eq!(first.rows.len(), 1);
    assert!(first.truncated, "a second row exists, so say so");

    let second = page(1, 1);
    assert_eq!(second.rows.len(), 1);
    assert!(!second.truncated, "and stop saying so at the end");
    assert_ne!(
        first.rows[0][0], second.rows[0][0],
        "the second page must not repeat the first"
    );

    assert!(
        page(200, 99).rows.is_empty(),
        "past the end is empty, not an error"
    );
}

/// A blob crossing as base64 through a line-delimited JSON pipe is how a driver
/// stalls the editor reading it.
#[test]
fn a_blob_crosses_as_a_size_and_a_null_stays_a_null() {
    let (_dir, path) = fixture();
    let (driver, id) = connected(&path);
    let result = query(&driver, &id, "SELECT avatar, email FROM users ORDER BY id").unwrap();

    assert_eq!(result.rows[0][0], Cell::Binary { byte_len: 4 });
    assert_eq!(result.rows[1][0], Cell::Null);
    assert_eq!(
        result.rows[1][1],
        Cell::Null,
        "and a null text is still null"
    );

    let encoded = serde_json::to_string(&result).expect("the page must survive JSON");
    assert!(
        !encoded.contains("\u{0}"),
        "raw bytes must not reach the pipe"
    );
}

/// Bad SQL and a refused write need different codes, because the UI says
/// different things for the two.
#[test]
fn a_syntax_error_is_not_reported_as_a_read_only_refusal() {
    let (_dir, path) = fixture();
    let (driver, id) = connected(&path);
    let error = query(&driver, &id, "SELECT FROM WHERE").expect_err("that is not SQL");
    assert_eq!(error.code, ErrorCode::Syntax);
}

/// Plenty of real tables have no primary key. Reporting that honestly beats
/// erroring, and beats inventing one.
#[test]
fn a_table_without_a_primary_key_describes_itself_without_complaint() {
    let (_dir, path) = fixture();
    let (driver, id) = connected(&path);
    let described: DescribeTableResult = serde_json::from_value(
        driver
            .describe_table(json!({ "connection_id": id, "schema": "main", "table": "keyless" }))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(described.columns.len(), 1);
    assert!(!described.columns[0].primary_key);
}

#[test]
fn a_missing_file_is_a_connection_error_not_an_empty_database() {
    let driver = SqliteDriver::default();
    let error = driver
        .connect(json!({ "url": "/nonexistent/definitely/not/here.sqlite" }))
        .expect_err("a typo in a path must not silently open an empty database");
    assert_eq!(error.code, ErrorCode::Connection);
}

#[test]
fn an_unknown_connection_is_named_as_such() {
    let driver = SqliteDriver::default();
    let error = driver
        .list_schemas(json!({ "connection_id": "never-opened" }))
        .expect_err("a connection that was never opened cannot be listed");
    assert_eq!(error.code, ErrorCode::UnknownConnection);
}

/// The framing, over the same `serve` loop the binary runs. Everything above
/// calls the driver directly; this is the one that proves a request written as
/// a line comes back as a line.
#[test]
fn the_serve_loop_answers_a_line_with_a_line() {
    let (_dir, path) = fixture();
    let driver = Arc::new(SqliteDriver::default());

    let requests = format!(
        "{}\n{}\n",
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "connect",
                "params": { "url": path.display().to_string() } }),
    );

    let output = SharedBuffer::default();
    serve(driver, Cursor::new(requests), output.clone()).unwrap();

    let written = output.contents();
    let lines: Vec<&str> = written.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines.len(), 2, "one answer per request, one line each");
    for line in lines {
        let response: serde_json::Value = serde_json::from_str(line).expect("each line is JSON");
        assert_eq!(response["jsonrpc"], "2.0");
        assert!(
            response.get("error").is_none(),
            "neither call should have failed: {response}"
        );
    }
}

/// An undecodable line must not stop the loop: the next request still has to be
/// answered, or one bad byte ends the session.
#[test]
fn rubbish_on_the_pipe_does_not_end_the_session() {
    let driver = Arc::new(SqliteDriver::default());
    let requests = format!(
        "not json at all\n{}\n",
        json!({ "jsonrpc": "2.0", "id": 9, "method": "initialize", "params": {} })
    );

    let output = SharedBuffer::default();
    serve(driver, Cursor::new(requests), output.clone()).unwrap();

    let written = output.contents();
    let lines: Vec<&str> = written.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines.len(),
        1,
        "the bad line gets no reply -- it carried no id to reply to -- but the good one does"
    );
    assert!(lines[0].contains("\"id\":9"));
}

#[derive(Clone, Default)]
struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl SharedBuffer {
    fn contents(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).expect("the driver writes UTF-8")
    }
}

impl std::io::Write for SharedBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
