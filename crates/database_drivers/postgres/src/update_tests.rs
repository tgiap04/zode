//! `update_rows` against a real server: what a type accepts, what it refuses,
//! and that a refusal leaves the table as it was.
//!
//! Skipped, loudly, when `ZODE_TEST_POSTGRES_URL` names no server. Each test
//! builds a schema of its own and drops it afterwards, so they share a server
//! without sharing rows.

use super::*;
use database::driver_test_suite::server_url;
use database::protocol::{Cell, UpdateRowsResult};
use serde_json::{Value, json};

/// A throwaway schema, dropped when the test ends however it ends.
struct Sandbox {
    url: String,
    schema: String,
}

impl Sandbox {
    fn new(url: &str, label: &str) -> Self {
        let schema = format!("zode_edit_{label}_{}", std::process::id());
        let sandbox = Self {
            url: url.to_string(),
            schema: schema.clone(),
        };
        let mut setup = sandbox.admin();
        setup
            .batch_execute(&format!(
                "DROP SCHEMA IF EXISTS {schema} CASCADE;
                 CREATE SCHEMA {schema};
                 CREATE TYPE {schema}.mood AS ENUM ('sad', 'ok', 'happy');
                 CREATE TABLE {schema}.people (
                     id integer PRIMARY KEY,
                     name varchar(5) NOT NULL,
                     age integer CHECK (age >= 0),
                     score numeric(5, 2),
                     born date,
                     seen timestamptz,
                     active boolean,
                     mood {schema}.mood,
                     uid uuid,
                     tags integer[],
                     doc jsonb,
                     handle text UNIQUE
                 );
                 INSERT INTO {schema}.people VALUES
                   (1, 'ann', 30, 1.50, '2000-01-02', '2026-01-02 03:04:05+00', true, 'ok',
                    '11111111-1111-1111-1111-111111111111', '{{1,2}}', '{{\"a\": 1}}', 'ann'),
                   (2, 'bob', 40, 2.50, '2001-02-03', '2026-02-03 04:05:06+00', false, 'sad',
                    '22222222-2222-2222-2222-222222222222', '{{3}}', '{{}}', 'bob');
                 CREATE TABLE {schema}.enrolment (
                     student integer, course text, grade text,
                     PRIMARY KEY (student, course)
                 );
                 INSERT INTO {schema}.enrolment VALUES (1, 'math', 'A'), (1, 'art', 'B');
                 CREATE FUNCTION {schema}.wipe() RETURNS bigint LANGUAGE sql AS $$
                     WITH gone AS (DELETE FROM {schema}.people RETURNING 1)
                     SELECT count(*) FROM gone $$;
                 CREATE VIEW {schema}.adults AS SELECT id, name FROM {schema}.people WHERE age >= 18;
                 CREATE MATERIALIZED VIEW {schema}.frozen AS SELECT id FROM {schema}.people;
                 CREATE VIEW {schema}.joined AS
                     SELECT p.id, e.course FROM {schema}.people p
                     JOIN {schema}.enrolment e ON e.student = p.id;
                 CREATE TABLE {schema}.countries (code char(2) PRIMARY KEY, label text);
                 INSERT INTO {schema}.countries VALUES ('U', 'one letter'), ('US', 'two letters');
                 CREATE DOMAIN {schema}.short_text AS varchar(5);
                 CREATE DOMAIN {schema}.blobby AS bytea;
                 CREATE TABLE {schema}.shapes (
                     id integer PRIMARY KEY,
                     fixed char(5),
                     flags bit(3),
                     capped {schema}.short_text,
                     raw bytea,
                     wrapped {schema}.blobby
                 );
                 INSERT INTO {schema}.shapes VALUES (1, 'abc', B'101', 'abc', '\\x0102', '\\x0304');"
            ))
            .expect("building the sandbox schema");
        sandbox
    }

    fn admin(&self) -> postgres::Client {
        postgres::Client::connect(&self.url, postgres::NoTls).expect("an admin connection")
    }

    fn driver(&self) -> (PostgresDriver, ConnectionId) {
        connect(&self.url)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let dropped = self
            .admin()
            .batch_execute(&format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema));
        if let Err(error) = dropped {
            eprintln!("could not drop {}: {error}", self.schema);
        }
    }
}

fn connect(url: &str) -> (PostgresDriver, ConnectionId) {
    tls::install_crypto_provider();
    let driver = PostgresDriver::default();
    let result = driver
        .connect(json!({ "url": url }))
        .expect("connecting to the server");
    let result: ConnectResult = serde_json::from_value(result).unwrap();
    (driver, result.connection_id)
}

fn text(value: &str) -> Value {
    json!({ "kind": "text", "value": value })
}

fn null() -> Value {
    json!({ "kind": "null" })
}

fn column(name: &str, value: Value) -> Value {
    json!({ "column": name, "value": value })
}

fn change(key: &[(&str, Value)], set: &[(&str, Value)]) -> Value {
    json!({
        "key": key.iter().map(|(name, value)| column(name, value.clone())).collect::<Vec<_>>(),
        "set": set.iter().map(|(name, value)| column(name, value.clone())).collect::<Vec<_>>(),
    })
}

fn update(
    sandbox: &Sandbox,
    driver: &PostgresDriver,
    id: &ConnectionId,
    table: &str,
    changes: Vec<Value>,
) -> Result<UpdateRowsResult, ResponseError> {
    driver
        .update_rows(json!({
            "connection_id": id, "schema": sandbox.schema, "table": table, "changes": changes,
        }))
        .map(|value| serde_json::from_value(value).expect("an answer that decodes"))
}

fn read(driver: &PostgresDriver, id: &ConnectionId, sql: &str) -> Cell {
    let value = driver
        .query(json!({
            "connection_id": id, "sql": sql, "limit": 10, "offset": 0, "request_id": "t",
        }))
        .expect("the read-back");
    let page: ResultSet = serde_json::from_value(value).unwrap();
    assert_eq!(page.rows.len(), 1, "{sql} must find one row");
    page.rows[0][0].clone()
}

fn person(sandbox: &Sandbox, driver: &PostgresDriver, id: &ConnectionId, field: &str) -> Cell {
    read(
        driver,
        id,
        &format!(
            "SELECT {field}::text FROM {}.people WHERE id = 1",
            sandbox.schema
        ),
    )
}

fn plain(value: &str) -> Cell {
    Cell::Text {
        value: value.into(),
    }
}

#[test]
fn an_edited_cell_reaches_the_row_and_null_is_not_the_empty_string() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "basic");
    let (driver, id) = sandbox.driver();
    let set = |value: Value| {
        update(
            &sandbox,
            &driver,
            &id,
            "people",
            vec![change(&[("id", text("1"))], &[("handle", value)])],
        )
        .expect("a valid edit")
    };
    assert_eq!(set(text("zed")).updated, 1);
    assert_eq!(person(&sandbox, &driver, &id, "handle"), plain("zed"));
    set(text(""));
    assert_eq!(person(&sandbox, &driver, &id, "handle"), plain(""));
    set(null());
    assert_eq!(person(&sandbox, &driver, &id, "handle"), Cell::Null);
    assert_eq!(
        read(
            &driver,
            &id,
            &format!("SELECT handle FROM {}.people WHERE id = 2", sandbox.schema)
        ),
        plain("bob"),
        "no other row moves"
    );
}

#[test]
fn every_type_takes_what_it_can_hold_and_refuses_the_rest_on_its_own_column() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "types");
    let (driver, id) = sandbox.driver();
    // (column, accepted, refused). `name` is varchar(5): the refused value is
    // one character too long, and must be an error -- not stored cut short.
    let cases = [
        ("age", "31", "abc"),
        ("age", "5", "-1"),
        ("age", "7", "99999999999"),
        ("score", "9.99", "1000"),
        ("born", "2001-02-03", "2001-13-45"),
        ("seen", "2026-05-05 10:00:00+00", "not a time"),
        ("active", "false", "maybe"),
        ("mood", "happy", "angry"),
        ("uid", "33333333-3333-3333-3333-333333333333", "not-a-uuid"),
        ("tags", "{3,4}", "{a}"),
        ("doc", "{\"b\": 2}", "{bad"),
        ("name", "abcde", "abcdef"),
    ];
    for (name, accepted, refused) in cases {
        let before = person(&sandbox, &driver, &id, name);
        let error = update(
            &sandbox,
            &driver,
            &id,
            "people",
            vec![change(&[("id", text("1"))], &[(name, text(refused))])],
        )
        .expect_err(&format!("{refused:?} must not fit {name}"));
        assert_eq!(error.code, ErrorCode::InvalidValue, "{name} = {refused:?}");
        let data = error.data.expect("where it went wrong");
        assert_eq!(data.column.as_deref(), Some(name), "{name} = {refused:?}");
        assert_eq!(data.change, Some(0));
        assert_eq!(person(&sandbox, &driver, &id, name), before, "{name}");

        let applied = update(
            &sandbox,
            &driver,
            &id,
            "people",
            vec![change(&[("id", text("1"))], &[(name, text(accepted))])],
        )
        .unwrap_or_else(|error| panic!("{accepted:?} must fit {name}: {error:?}"));
        assert_eq!(applied.updated, 1);
    }
    assert_eq!(person(&sandbox, &driver, &id, "name"), plain("abcde"));
}

#[test]
fn a_not_null_column_refuses_null_and_a_unique_one_refuses_a_duplicate() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "constraints");
    let (driver, id) = sandbox.driver();
    for (name, value) in [("name", null()), ("handle", text("bob"))] {
        let error = update(
            &sandbox,
            &driver,
            &id,
            "people",
            vec![
                change(&[("id", text("1"))], &[("age", text("31"))]),
                change(&[("id", text("1"))], &[(name, value)]),
            ],
        )
        .expect_err("the constraint refuses");
        assert_eq!(error.code, ErrorCode::InvalidValue, "{name}");
        let data = error.data.expect("where");
        assert_eq!(data.change, Some(1));
        assert_eq!(data.column.as_deref(), Some(name));
        assert_eq!(
            person(&sandbox, &driver, &id, "age"),
            plain("30"),
            "the first change is rolled back with the second"
        );
    }
}

#[test]
fn a_value_is_data_and_never_part_of_the_statement() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "inject");
    let (driver, id) = sandbox.driver();
    let hostile = "x'); DROP TABLE people; --";
    update(
        &sandbox,
        &driver,
        &id,
        "people",
        vec![change(&[("id", text("1"))], &[("handle", text(hostile))])],
    )
    .expect("any text is a value");
    assert_eq!(person(&sandbox, &driver, &id, "handle"), plain(hostile));

    let error = update(
        &sandbox,
        &driver,
        &id,
        "people",
        vec![change(
            &[("id", text("1"))],
            &[("handle\" = 'x', \"age", text("1"))],
        )],
    )
    .expect_err("not a column");
    assert_eq!(error.code, ErrorCode::InvalidValue);
    assert_eq!(
        read(
            &driver,
            &id,
            &format!("SELECT count(*) FROM {}.people", sandbox.schema)
        ),
        Cell::Number { value: "2".into() }
    );
}

#[test]
fn a_missing_row_is_a_conflict_that_rolls_the_whole_batch_back() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "conflict");
    let (driver, id) = sandbox.driver();
    let error = update(
        &sandbox,
        &driver,
        &id,
        "people",
        vec![
            change(&[("id", text("1"))], &[("age", text("31"))]),
            change(&[("id", text("99"))], &[("age", text("32"))]),
        ],
    )
    .expect_err("the second change matches nothing");
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(error.data.and_then(|data| data.change), Some(1));
    assert_eq!(person(&sandbox, &driver, &id, "age"), plain("30"));

    // A key that is not the whole key matches two rows, which is also a conflict.
    let partial = update(
        &sandbox,
        &driver,
        &id,
        "enrolment",
        vec![change(&[("student", text("1"))], &[("grade", text("F"))])],
    )
    .expect_err("student 1 has two rows");
    assert_eq!(partial.code, ErrorCode::Conflict);
    assert_eq!(
        read(
            &driver,
            &id,
            &format!(
                "SELECT string_agg(grade, ',' ORDER BY course) FROM {}.enrolment",
                sandbox.schema
            )
        ),
        plain("B,A")
    );
    let whole = update(
        &sandbox,
        &driver,
        &id,
        "enrolment",
        vec![change(
            &[("student", text("1")), ("course", text("art"))],
            &[("grade", text("C"))],
        )],
    )
    .expect("the whole key names one row");
    assert_eq!(whole.updated, 1);
}

/// The grid sends back what it displayed. Each of these is a value PostgreSQL
/// rendered itself, and each has to find its row again.
#[test]
fn a_key_shown_in_the_grid_finds_its_row_again() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "keys");
    let (driver, id) = sandbox.driver();
    let shown = |field: &str| match read(
        &driver,
        &id,
        &format!("SELECT {field} FROM {}.people WHERE id = 1", sandbox.schema),
    ) {
        Cell::Text { value }
        | Cell::Number { value }
        | Cell::Timestamp { value }
        | Cell::Json { value } => value,
        Cell::Bool { value } => value.to_string(),
        other => panic!("{field} showed {other:?}"),
    };
    for field in ["uid", "score", "born", "seen", "mood", "tags", "active"] {
        let key = shown(field);
        let applied = update(
            &sandbox,
            &driver,
            &id,
            "people",
            vec![change(&[(field, text(&key))], &[("age", text("33"))])],
        )
        .unwrap_or_else(|error| panic!("{field} = {key:?}: {error:?}"));
        assert_eq!(applied.updated, 1, "{field}");
        assert_eq!(person(&sandbox, &driver, &id, "age"), plain("33"));
        update(
            &sandbox,
            &driver,
            &id,
            "people",
            vec![change(&[("id", text("1"))], &[("age", text("30"))])],
        )
        .expect("putting it back");
    }
}

#[test]
fn a_simple_view_can_be_edited_and_a_materialised_one_cannot() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "views");
    let (driver, id) = sandbox.driver();
    update(
        &sandbox,
        &driver,
        &id,
        "adults",
        vec![change(&[("id", text("1"))], &[("name", text("anne"))])],
    )
    .expect("a simple view is auto-updatable");
    assert_eq!(person(&sandbox, &driver, &id, "name"), plain("anne"));

    for table in ["frozen", "missing"] {
        let error = update(
            &sandbox,
            &driver,
            &id,
            table,
            vec![change(&[("id", text("1"))], &[("id", text("9"))])],
        )
        .expect_err("not an updatable table");
        assert_eq!(error.code, ErrorCode::Syntax, "{table}");
    }
}

#[test]
fn browsing_stays_read_only_and_a_failed_query_does_not_break_editing() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "browse");
    let (driver, id) = sandbox.driver();
    let query = |sql: &str| {
        driver.query(json!({
            "connection_id": id, "sql": sql, "limit": 10, "offset": 0, "request_id": "t",
        }))
    };
    assert!(query("NOT SQL AT ALL").is_err(), "a typo is an error");
    assert!(query("SELECT 1 / 0").is_err(), "a runtime error is one too");
    assert!(query("SELECT 1").is_ok(), "browsing survives it");
    update(
        &sandbox,
        &driver,
        &id,
        "people",
        vec![change(&[("id", text("1"))], &[("age", text("31"))])],
    )
    .expect("editing right after a query that failed");
    // A statement PostgreSQL cannot declare a cursor over is a syntax error
    // before it is ever run; one that writes from inside a SELECT is what
    // reaches the read-only transaction.
    let refused = query(&format!("SELECT {}.wipe()", sandbox.schema))
        .expect_err("still read-only after an edit");
    assert_eq!(refused.code, ErrorCode::ReadOnly, "{refused:?}");
}

/// A DBA's `default_transaction_read_only` is a statement of intent. Forcing
/// `READ WRITE` would walk past it; inheriting it turns the Save down in the
/// words the user already knows.
#[test]
fn a_server_that_defaults_to_read_only_turns_the_save_down() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "policy");
    let separator = if url.contains('?') { '&' } else { '?' };
    let (driver, id) = connect(&format!(
        "{url}{separator}options=-c%20default_transaction_read_only%3Don"
    ));
    let error = update(
        &sandbox,
        &driver,
        &id,
        "people",
        vec![change(&[("id", text("1"))], &[("age", text("31"))])],
    )
    .expect_err("the session is read-only by policy");
    assert_eq!(error.code, ErrorCode::ReadOnly);
}

#[test]
fn an_empty_batch_opens_nothing_and_an_unknown_connection_is_refused() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "empty");
    let (driver, id) = sandbox.driver();
    assert_eq!(
        update(&sandbox, &driver, &id, "people", vec![])
            .unwrap()
            .updated,
        0
    );
    let error = update(
        &sandbox,
        &driver,
        &ConnectionId("nope".into()),
        "people",
        vec![],
    )
    .expect_err("no such connection");
    assert_eq!(error.code, ErrorCode::UnknownConnection);
}

fn shape(sandbox: &Sandbox, driver: &PostgresDriver, id: &ConnectionId, field: &str) -> Cell {
    read(
        driver,
        id,
        &format!(
            "SELECT {field}::text FROM {}.shapes WHERE id = 1",
            sandbox.schema
        ),
    )
}

fn set_shape(
    sandbox: &Sandbox,
    driver: &PostgresDriver,
    id: &ConnectionId,
    field: &str,
    value: &str,
) -> Result<UpdateRowsResult, ResponseError> {
    update(
        sandbox,
        driver,
        id,
        "shapes",
        vec![change(&[("id", text("1"))], &[(field, text(value))])],
    )
}

/// `format_type(oid, NULL)` names `char(5)` as `character`, and an explicit
/// `CAST(x AS character)` is `char(1)`: the edit stored its first letter.
#[test]
fn a_fixed_width_column_keeps_everything_that_fits_it_and_refuses_the_rest() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "fixed");
    let (driver, id) = sandbox.driver();
    // (column, accepted, as stored, refused)
    for (field, accepted, stored, refused) in [
        ("fixed", "xyz12", "xyz12", "xyz123"),
        ("flags", "110", "110", "1101"),
        ("capped", "abcde", "abcde", "abcdefgh"),
    ] {
        let before = shape(&sandbox, &driver, &id, field);
        let error = set_shape(&sandbox, &driver, &id, field, refused)
            .expect_err(&format!("{refused:?} must not fit {field}"));
        assert_eq!(error.code, ErrorCode::InvalidValue, "{field}: {error:?}");
        assert_eq!(
            error.data.and_then(|data| data.column).as_deref(),
            Some(field)
        );
        assert_eq!(
            shape(&sandbox, &driver, &id, field),
            before,
            "{field}: an over-long value must not be stored cut short"
        );

        set_shape(&sandbox, &driver, &id, field, accepted)
            .unwrap_or_else(|error| panic!("{accepted:?} must fit {field}: {error:?}"));
        assert_eq!(
            shape(&sandbox, &driver, &id, field),
            plain(stored),
            "{field}"
        );
    }
}

/// `code char(2)` holding `'U '` and `'US'`: addressing the second used to be
/// compared as `char(1)`, which matched the first -- one row, so the
/// exactly-one check passed and the wrong row was written.
#[test]
fn a_fixed_width_key_finds_its_own_row_and_not_a_shorter_neighbour() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "fixedkey");
    let (driver, id) = sandbox.driver();
    let label = |code: &str| {
        read(
            &driver,
            &id,
            &format!(
                "SELECT label FROM {}.countries WHERE code = '{code}'",
                sandbox.schema
            ),
        )
    };
    for (key, edited, other, other_label) in [
        ("US", "US", "U", "one letter"),
        // As the grid shows a char(2) holding one letter: padded.
        ("U ", "U", "US", "two letters"),
        ("U", "U", "US", "two letters"),
    ] {
        update(
            &sandbox,
            &driver,
            &id,
            "countries",
            vec![change(&[("code", text(key))], &[("label", text("edited"))])],
        )
        .unwrap_or_else(|error| panic!("{key:?}: {error:?}"));
        assert_eq!(label(edited), plain("edited"), "{key:?} edits its own row");
        assert_eq!(label(other), plain(other_label), "{key:?} leaves the other");
        update(
            &sandbox,
            &driver,
            &id,
            "countries",
            vec![change(&[("code", text(key))], &[("label", text("reset"))])],
        )
        .expect("putting it back");
        let restored = if edited == "US" {
            "two letters"
        } else {
            "one letter"
        };
        update(
            &sandbox,
            &driver,
            &id,
            "countries",
            vec![change(
                &[("code", text(edited))],
                &[("label", text(restored))],
            )],
        )
        .expect("putting it back");
    }
}

#[test]
fn a_binary_column_is_not_edited_as_text_even_behind_a_domain() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "binary");
    let (driver, id) = sandbox.driver();
    for field in ["raw", "wrapped"] {
        let before = shape(&sandbox, &driver, &id, field);
        let error = set_shape(&sandbox, &driver, &id, field, "text").expect_err("binary");
        assert_eq!(error.code, ErrorCode::InvalidValue, "{field}");
        assert!(
            error
                .message
                .contains("binary columns cannot be edited as text"),
            "{}",
            error.message
        );
        assert_eq!(
            error.data.and_then(|data| data.column).as_deref(),
            Some(field)
        );
        assert_eq!(shape(&sandbox, &driver, &id, field), before);
    }
}

/// `SELECT 1; COMMIT; UPDATE ...` handed to a script of its own used to end the
/// read-only transaction and write. One statement per `query`, over a protocol
/// that cannot carry two.
#[test]
fn a_query_that_smuggles_a_second_statement_is_refused_and_writes_nothing() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "escape");
    let (driver, id) = sandbox.driver();
    let schema = &sandbox.schema;
    let attempts = [
        format!(
            "SELECT 1; COMMIT; UPDATE {schema}.people SET age = 99 WHERE id = 1; COMMIT; \
             DECLARE zode_page NO SCROLL CURSOR FOR SELECT 1"
        ),
        format!("SELECT 1; COMMIT; UPDATE {schema}.people SET age = 99 WHERE id = 1"),
        format!("SELECT 1; UPDATE {schema}.people SET age = 99 WHERE id = 1"),
        format!("SELECT 1; ROLLBACK; BEGIN READ WRITE; UPDATE {schema}.people SET age = 99"),
    ];
    for sql in attempts {
        let refused = driver
            .query(json!({
                "connection_id": id, "sql": sql, "limit": 10, "offset": 0, "request_id": "t",
            }))
            .expect_err(&format!("`{sql}` must be refused"));
        assert!(
            matches!(refused.code, ErrorCode::ReadOnly | ErrorCode::Syntax),
            "{sql}: {refused:?}"
        );
        assert_eq!(
            person(&sandbox, &driver, &id, "age"),
            plain("30"),
            "`{sql}` must not have written"
        );
    }
    // The same connection still pages, and still refuses a write.
    for sql in [
        "SELECT 1;",
        "SELECT 1 -- a trailing comment",
        "VALUES (1), (2)",
    ] {
        read_page(&driver, &id, sql);
    }
}

fn read_page(driver: &PostgresDriver, id: &ConnectionId, sql: &str) {
    driver
        .query(json!({
            "connection_id": id, "sql": sql, "limit": 10, "offset": 0, "request_id": "t",
        }))
        .unwrap_or_else(|error| panic!("`{sql}` must page: {error:?}"));
}

/// A row another session holds locked made the Save wait for as long as that
/// session lived -- with the connection's lock held, so browsing froze too.
#[test]
fn a_row_locked_by_another_session_is_refused_in_bounded_time() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "locked");
    let (driver, id) = sandbox.driver();
    let mut holder = sandbox.admin();
    holder
        .batch_execute(&format!(
            "BEGIN; UPDATE {}.people SET age = age WHERE id = 1",
            sandbox.schema
        ))
        .expect("taking the row lock");

    let driver = std::sync::Arc::new(driver);
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = {
        let driver = driver.clone();
        let id = id.clone();
        let schema = sandbox.schema.clone();
        std::thread::spawn(move || {
            let outcome = driver.update_rows(json!({
                "connection_id": id, "schema": schema, "table": "people",
                "changes": [change(&[("id", text("1"))], &[("age", text("31"))])],
            }));
            sender.send(outcome).ok();
        })
    };
    let outcome = receiver.recv_timeout(std::time::Duration::from_secs(20));
    // Released whatever happens, so a failing run does not leave the worker
    // waiting on a lock nobody will ever let go of.
    holder
        .batch_execute("ROLLBACK")
        .expect("releasing the lock");
    let outcome = outcome.expect("the Save must not wait on a lock for ever");
    worker.join().expect("the worker finishes");

    let error = outcome.expect_err("the row is locked");
    assert_eq!(error.code, ErrorCode::Internal, "{error:?}");
    assert!(error.message.contains("locked"), "{}", error.message);
    assert_eq!(person(&sandbox, &driver, &id, "age"), plain("30"));
    update(
        &sandbox,
        &driver,
        &id,
        "people",
        vec![change(&[("id", text("1"))], &[("age", text("32"))])],
    )
    .expect("the connection is usable once the lock is gone");
}

#[test]
fn a_view_postgres_cannot_update_is_an_invalid_value_that_says_why() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "joined");
    let (driver, id) = sandbox.driver();
    let error = update(
        &sandbox,
        &driver,
        &id,
        "joined",
        vec![change(&[("id", text("1"))], &[("course", text("x"))])],
    )
    .expect_err("a join view is not auto-updatable");
    assert_eq!(error.code, ErrorCode::InvalidValue, "{error:?}");
    assert!(error.message.contains("view"), "{}", error.message);
}

/// A role that may read the table but not write it.
#[test]
fn a_user_without_the_privilege_is_read_only_and_not_a_syntax_error() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "privilege");
    let role = format!("zode_fx_reader_{}", std::process::id());
    let mut admin = sandbox.admin();
    let created = admin.batch_execute(&format!(
        "DROP ROLE IF EXISTS {role};
         CREATE ROLE {role} LOGIN PASSWORD 'reader';
         GRANT USAGE ON SCHEMA {schema} TO {role};
         GRANT SELECT ON ALL TABLES IN SCHEMA {schema} TO {role};",
        schema = sandbox.schema
    ));
    if let Err(error) = created {
        println!("skipped: this account cannot create a role ({error})");
        return;
    }
    let (_, rest) = url.split_once("://").expect("a URL");
    let rest = rest.rsplit_once('@').map_or(rest, |(_, tail)| tail);
    let (driver, id) = connect(&format!("postgres://{role}:reader@{rest}"));
    let outcome = update(
        &sandbox,
        &driver,
        &id,
        "people",
        vec![change(&[("id", text("1"))], &[("age", text("31"))])],
    );
    let cleanup = admin.batch_execute(&format!("DROP OWNED BY {role}; DROP ROLE {role};"));
    if let Err(error) = cleanup {
        eprintln!("could not drop {role}: {error}");
    }
    let error = outcome.expect_err("no UPDATE privilege");
    assert_eq!(error.code, ErrorCode::ReadOnly, "{error:?}");
}

/// Paging survives being split into a prepared `DECLARE` and a script that
/// holds only numbers: the offset is skipped and the extra row answers
/// `truncated`.
#[test]
fn a_page_is_still_a_page_when_the_cursor_is_declared_on_its_own() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "paging");
    let (driver, id) = sandbox.driver();
    let page = |limit: u32, offset: u64| -> ResultSet {
        let value = driver
            .query(json!({
                "connection_id": id, "limit": limit, "offset": offset, "request_id": "t",
                "sql": "SELECT n, NULL::text AS nothing, ''::text AS empty \
                        FROM generate_series(1, 5) AS n",
            }))
            .expect("a page");
        serde_json::from_value(value).unwrap()
    };
    let numbers = |page: &ResultSet| -> Vec<String> {
        page.rows
            .iter()
            .map(|row| match &row[0] {
                Cell::Number { value } => value.clone(),
                other => panic!("{other:?}"),
            })
            .collect()
    };
    let first = page(2, 0);
    assert_eq!(numbers(&first), ["1", "2"]);
    assert!(first.truncated);
    assert_eq!(first.rows[0][1], Cell::Null);
    assert_eq!(first.rows[0][2], plain(""));
    let middle = page(2, 2);
    assert_eq!(numbers(&middle), ["3", "4"]);
    assert!(middle.truncated);
    let last = page(2, 4);
    assert_eq!(numbers(&last), ["5"]);
    assert!(!last.truncated);
    assert_eq!(
        page(2, 0)
            .columns
            .iter()
            .map(|column| column.type_name.as_str())
            .collect::<Vec<_>>(),
        ["int4", "text", "text"]
    );
}

#[test]
fn a_statement_that_needs_parameters_is_a_syntax_error_and_not_a_lost_connection() {
    let Some(url) = server_url("ZODE_TEST_POSTGRES_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "params");
    let (driver, id) = sandbox.driver();
    for sql in ["SELECT $1::int", "SELECT $1"] {
        let error = driver
            .query(json!({
                "connection_id": id, "sql": sql, "limit": 10, "offset": 0, "request_id": "t",
            }))
            .expect_err("nothing can fill $1");
        assert_eq!(error.code, ErrorCode::Syntax, "{sql}: {error:?}");
    }
    read_page(&driver, &id, "SELECT 1");
}
