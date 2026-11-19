//! `update_rows` against a real server: what a type accepts, what it refuses,
//! and that a refusal leaves the table as it was.
//!
//! Skipped, loudly, when `ZODE_TEST_MYSQL_URL` names no server (the account
//! needs `CREATE`/`DROP` on a database of its own). Each test builds a
//! database of its own and drops it afterwards.

use super::*;
use database::driver_test_suite::server_url;
use database::protocol::{Cell, UpdateRowsResult};
use serde_json::{Value, json};

/// A throwaway database, dropped when the test ends however it ends.
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
        for statement in [
            format!("DROP DATABASE IF EXISTS `{schema}`"),
            format!("CREATE DATABASE `{schema}`"),
            format!(
                "CREATE TABLE `{schema}`.people (
                     id int PRIMARY KEY,
                     name varchar(5) NOT NULL,
                     age int unsigned,
                     score decimal(5, 2),
                     born date,
                     seen datetime,
                     active tinyint(1),
                     mood enum('sad', 'ok', 'happy'),
                     doc json,
                     handle varchar(40) UNIQUE,
                     CONSTRAINT age_is_plausible CHECK (age < 150)
                 )"
            ),
            format!(
                "INSERT INTO `{schema}`.people VALUES
                   (1, 'ann', 30, 1.50, '2000-01-02', '2026-01-02 03:04:05', 1, 'ok',
                    '{{\"a\": 1}}', 'ann'),
                   (2, 'bob', 40, 2.50, '2001-02-03', '2026-02-03 04:05:06', 0, 'sad',
                    '{{}}', 'bob')"
            ),
            format!(
                "CREATE TABLE `{schema}`.enrolment (
                     student int, course varchar(20), grade varchar(5),
                     PRIMARY KEY (student, course)
                 )"
            ),
            format!("INSERT INTO `{schema}`.enrolment VALUES (1, 'math', 'A'), (1, 'art', 'B')"),
            // Two ids a double cannot tell apart.
            format!("CREATE TABLE `{schema}`.ids (id bigint PRIMARY KEY, note varchar(10))"),
            format!(
                "INSERT INTO `{schema}`.ids VALUES (9007199254740992, 'low'), (9007199254740993, 'high')"
            ),
            format!(
                "CREATE VIEW `{schema}`.adults AS SELECT id, name FROM `{schema}`.people WHERE age >= 18"
            ),
            format!(
                "CREATE TABLE `{schema}`.blobs (
                     id int PRIMARY KEY, raw varbinary(8), fixed binary(4), big blob,
                     note varchar(10)
                 )"
            ),
            format!("INSERT INTO `{schema}`.blobs VALUES (1, 0x0102, 0x01020304, 0x05, 'text')"),
            format!(
                "CREATE TABLE `{schema}`.legacy (id int PRIMARY KEY, v varchar(10)) ENGINE=MyISAM"
            ),
            format!("INSERT INTO `{schema}`.legacy VALUES (1, 'old')"),
        ] {
            setup.query_drop(&statement).expect("building the sandbox");
        }
        sandbox
    }

    fn admin(&self) -> mysql::Conn {
        mysql::Conn::new(Opts::from_url(&self.url).expect("a URL")).expect("an admin connection")
    }

    fn driver(&self) -> (MysqlDriver, ConnectionId) {
        connect(&self.url)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let dropped = self
            .admin()
            .query_drop(format!("DROP DATABASE IF EXISTS `{}`", self.schema));
        if let Err(error) = dropped {
            eprintln!("could not drop {}: {error}", self.schema);
        }
    }
}

fn connect(url: &str) -> (MysqlDriver, ConnectionId) {
    install_crypto_provider();
    let driver = MysqlDriver::default();
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
    driver: &MysqlDriver,
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

fn query(driver: &MysqlDriver, id: &ConnectionId, sql: &str) -> Result<ResultSet, ResponseError> {
    let value = driver.query(json!({
        "connection_id": id, "sql": sql, "limit": 10, "offset": 0, "request_id": "t",
    }))?;
    Ok(serde_json::from_value(value).unwrap())
}

fn read(driver: &MysqlDriver, id: &ConnectionId, sql: &str) -> Cell {
    let page = query(driver, id, sql).expect("the read-back");
    assert_eq!(page.rows.len(), 1, "{sql} must find one row");
    page.rows[0][0].clone()
}

fn person(sandbox: &Sandbox, driver: &MysqlDriver, id: &ConnectionId, field: &str) -> Cell {
    read(
        driver,
        id,
        &format!(
            "SELECT CAST({field} AS CHAR) FROM `{}`.people WHERE id = 1",
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
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
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
            &format!(
                "SELECT handle FROM `{}`.people WHERE id = 2",
                sandbox.schema
            )
        ),
        plain("bob"),
        "no other row moves"
    );
}

/// MySQL only counts a row as affected when it *changed*, unless the
/// connection asks for rows it matched. Saving the value a cell already holds
/// must not look like a row that vanished.
#[test]
fn writing_the_value_a_cell_already_holds_is_not_a_conflict() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "same");
    let (driver, id) = sandbox.driver();
    let applied = update(
        &sandbox,
        &driver,
        &id,
        "people",
        vec![change(&[("id", text("1"))], &[("age", text("30"))])],
    )
    .expect("the same value again");
    assert_eq!(applied.updated, 1);
}

/// Strict or not, a value the column cannot hold is refused on that column --
/// `sql_mode` is the server's to choose, and the answer must not depend on it.
fn every_type_refuses_what_it_cannot_hold(sandbox: &Sandbox) {
    let (driver, id) = sandbox.driver();
    // (column, accepted, refused). `name` is varchar(5): one character too long
    // must be an error, not stored cut short.
    let cases = [
        ("age", "31", "abc"),
        ("age", "5", "-1"),
        ("age", "7", "99999999999"),
        ("age", "8", "200"),
        ("score", "9.99", "1000"),
        ("born", "2001-02-03", "2001-13-45"),
        ("seen", "2026-05-05 10:00:00", "not a time"),
        ("active", "0", "maybe"),
        ("mood", "happy", "angry"),
        ("doc", "{\"b\": 2}", "{bad"),
        ("name", "abcde", "abcdef"),
    ];
    for (name, accepted, refused) in cases {
        let before = person(sandbox, &driver, &id, name);
        let error = update(
            sandbox,
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
        assert_eq!(person(sandbox, &driver, &id, name), before, "{name}");

        let applied = update(
            sandbox,
            &driver,
            &id,
            "people",
            vec![change(&[("id", text("1"))], &[(name, text(accepted))])],
        )
        .unwrap_or_else(|error| panic!("{accepted:?} must fit {name}: {error:?}"));
        assert_eq!(applied.updated, 1);
    }
    assert_eq!(person(sandbox, &driver, &id, "name"), plain("abcde"));
}

#[test]
fn every_type_takes_what_it_can_hold_and_refuses_the_rest_on_its_own_column() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
        return;
    };
    every_type_refuses_what_it_cannot_hold(&Sandbox::new(&url, "types"));
}

/// Puts the server back to what it was, even if the test panics.
struct SqlMode {
    url: String,
    original: String,
}

impl Drop for SqlMode {
    fn drop(&mut self) {
        let restored =
            mysql::Conn::new(Opts::from_url(&self.url).expect("a URL")).and_then(|mut conn| {
                conn.exec_drop("SET GLOBAL sql_mode = ?", (self.original.clone(),))
            });
        if let Err(error) = restored {
            eprintln!("could not restore sql_mode: {error}");
        }
    }
}

/// The same refusals with strict mode off, where MySQL stores a mangled value
/// and raises a warning instead of an error. A save that quietly keeps `0` for
/// `abc` is the failure this exists to catch.
#[test]
fn a_server_that_only_warns_still_has_its_values_refused() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "lenient");
    let original: String = sandbox
        .admin()
        .query_first("SELECT @@GLOBAL.sql_mode")
        .expect("reading sql_mode")
        .expect("a value");
    let _restore = SqlMode { url, original };
    sandbox
        .admin()
        .query_drop("SET GLOBAL sql_mode = ''")
        .expect("turning strict mode off");
    every_type_refuses_what_it_cannot_hold(&sandbox);
}

#[test]
fn a_not_null_column_refuses_null_and_a_unique_one_refuses_a_duplicate() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
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
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
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
            &[("handle` = 'x', `age", text("1"))],
        )],
    )
    .expect_err("not a column");
    assert_eq!(error.code, ErrorCode::InvalidValue);
    assert_eq!(
        read(
            &driver,
            &id,
            &format!("SELECT count(*) FROM `{}`.people", sandbox.schema)
        ),
        Cell::Number { value: "2".into() }
    );
}

#[test]
fn a_missing_row_is_a_conflict_that_rolls_the_whole_batch_back() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
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
                "SELECT CAST(group_concat(grade ORDER BY course) AS CHAR(20)) FROM `{}`.enrolment",
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

/// Compared as text MySQL would turn both of these into the same double, and
/// the edit would land on whichever row it found -- or refuse to land on
/// either.
#[test]
fn an_integer_key_is_matched_exactly_not_through_a_double() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "bigint");
    let (driver, id) = sandbox.driver();
    let applied = update(
        &sandbox,
        &driver,
        &id,
        "ids",
        vec![change(
            &[("id", text("9007199254740993"))],
            &[("note", text("edited"))],
        )],
    )
    .expect("exactly one row has this id");
    assert_eq!(applied.updated, 1);
    assert_eq!(
        read(
            &driver,
            &id,
            &format!(
                "SELECT note FROM `{}`.ids WHERE id = 9007199254740992",
                sandbox.schema
            )
        ),
        plain("low")
    );

    let error = update(
        &sandbox,
        &driver,
        &id,
        "ids",
        vec![change(
            &[("id", text("12 monkeys"))],
            &[("note", text("x"))],
        )],
    )
    .expect_err("not an integer");
    assert_eq!(error.code, ErrorCode::InvalidValue);
    assert_eq!(
        error.data.and_then(|data| data.column).as_deref(),
        Some("id")
    );
}

/// The grid sends back what it displayed; each of these has to find its row.
#[test]
fn a_key_shown_in_the_grid_finds_its_row_again() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "keys");
    let (driver, id) = sandbox.driver();
    let shown = |field: &str| match read(
        &driver,
        &id,
        &format!(
            "SELECT {field} FROM `{}`.people WHERE id = 1",
            sandbox.schema
        ),
    ) {
        Cell::Text { value }
        | Cell::Number { value }
        | Cell::Timestamp { value }
        | Cell::Json { value } => value,
        other => panic!("{field} showed {other:?}"),
    };
    for field in ["score", "born", "seen", "mood", "active", "handle", "age"] {
        let key = shown(field);
        let applied = update(
            &sandbox,
            &driver,
            &id,
            "people",
            vec![change(&[(field, text(&key))], &[("name", text("zed"))])],
        )
        .unwrap_or_else(|error| panic!("{field} = {key:?}: {error:?}"));
        assert_eq!(applied.updated, 1, "{field}");
        assert_eq!(person(&sandbox, &driver, &id, "name"), plain("zed"));
        update(
            &sandbox,
            &driver,
            &id,
            "people",
            vec![change(&[("id", text("1"))], &[("name", text("ann"))])],
        )
        .expect("putting it back");
    }
}

#[test]
fn a_simple_view_can_be_edited_and_a_missing_table_cannot() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
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
    .expect("a simple view is updatable");
    assert_eq!(person(&sandbox, &driver, &id, "name"), plain("anne"));

    let error = update(
        &sandbox,
        &driver,
        &id,
        "missing",
        vec![change(&[("id", text("1"))], &[("id", text("9"))])],
    )
    .expect_err("no such table");
    assert_eq!(error.code, ErrorCode::Syntax);
}

#[test]
fn browsing_stays_read_only_after_an_edit() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "browse");
    let (driver, id) = sandbox.driver();
    update(
        &sandbox,
        &driver,
        &id,
        "people",
        vec![change(&[("id", text("1"))], &[("age", text("31"))])],
    )
    .expect("an edit");
    let refused = query(
        &driver,
        &id,
        // `WITH` is one of the statements the driver does not wrap in a derived
        // table, which is what lets a write get as far as the transaction.
        &format!(
            "WITH gone AS (SELECT 1 AS n) DELETE FROM `{}`.people WHERE id IN (SELECT n FROM gone)",
            sandbox.schema
        ),
    )
    .expect_err("still read-only after an edit");
    assert_eq!(refused.code, ErrorCode::ReadOnly, "{refused:?}");
    assert_eq!(
        read(
            &driver,
            &id,
            &format!("SELECT count(*) FROM `{}`.people", sandbox.schema)
        ),
        Cell::Number { value: "2".into() }
    );
}

#[test]
fn an_empty_batch_opens_nothing_and_an_unknown_connection_is_refused() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
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

fn age(sandbox: &Sandbox, driver: &MysqlDriver, id: &ConnectionId) -> Cell {
    person(sandbox, driver, id, "age")
}

/// The mysql crate always asks for `CLIENT_MULTI_STATEMENTS`, and a statement
/// that is not wrapped (or one built to close the wrapper) reaches the server as
/// text -- so `COMMIT; UPDATE ...` ended the read-only transaction and wrote.
#[test]
fn a_query_that_smuggles_a_second_statement_is_refused_and_writes_nothing() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "escape");
    let (driver, id) = sandbox.driver();
    let schema = &sandbox.schema;
    let write = format!("UPDATE `{schema}`.people SET age = 99 WHERE id = 1");
    let attempts = [
        // Not wrapped, so run as typed.
        format!("SHOW DATABASES; COMMIT; {write}"),
        format!("WITH t AS (SELECT 1) SELECT 1; COMMIT; {write};"),
        format!("EXPLAIN SELECT 1; {write}"),
        // Wrapped, with the wrapper closed and reopened around the write.
        format!("SELECT 1) AS x; COMMIT; {write}; SELECT * FROM (SELECT 1"),
        format!("SELECT 1; COMMIT; {write}"),
    ];
    for sql in attempts {
        let refused = query(&driver, &id, &sql).expect_err(&format!("`{sql}` must be refused"));
        assert!(
            matches!(refused.code, ErrorCode::ReadOnly | ErrorCode::Syntax),
            "{sql}: {refused:?}"
        );
        assert_eq!(
            age(&sandbox, &driver, &id),
            plain("30"),
            "`{sql}` must not have written"
        );
    }
    // A single statement is not harmed by the check: wrapped, unwrapped, and one
    // the server will not prepare at all.
    for sql in [
        "SELECT 1 -- note",
        "SHOW DATABASES",
        "EXPLAIN SELECT 1",
        "WITH t AS (SELECT 1 AS n) SELECT n FROM t",
        "HELP 'contents'",
    ] {
        query(&driver, &id, sql).unwrap_or_else(|error| panic!("`{sql}` must page: {error:?}"));
    }
    let still_read_only = query(&driver, &id, &write).expect_err("a plain write");
    assert!(matches!(
        still_read_only.code,
        ErrorCode::ReadOnly | ErrorCode::Syntax
    ));
    assert_eq!(age(&sandbox, &driver, &id), plain("30"));
}

#[test]
fn a_table_without_transactions_is_not_edited_because_a_failure_could_not_be_undone() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "myisam");
    let (driver, id) = sandbox.driver();
    let error = update(
        &sandbox,
        &driver,
        &id,
        "legacy",
        vec![change(&[("id", text("1"))], &[("v", text("new"))])],
    )
    .expect_err("MyISAM cannot roll back");
    assert_eq!(error.code, ErrorCode::InvalidValue, "{error:?}");
    assert!(error.message.contains("transactions"), "{}", error.message);
    assert!(error.message.contains("MyISAM"), "{}", error.message);
    assert_eq!(
        read(
            &driver,
            &id,
            &format!("SELECT v FROM `{}`.legacy WHERE id = 1", sandbox.schema)
        ),
        plain("old")
    );
    // A transactional table next to it is unaffected.
    update(
        &sandbox,
        &driver,
        &id,
        "people",
        vec![change(&[("id", text("1"))], &[("age", text("31"))])],
    )
    .expect("InnoDB takes edits");
}

#[test]
fn a_binary_column_is_not_edited_as_text() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "binary");
    let (driver, id) = sandbox.driver();
    for field in ["raw", "fixed", "big"] {
        let error = update(
            &sandbox,
            &driver,
            &id,
            "blobs",
            vec![change(&[("id", text("1"))], &[(field, text("words"))])],
        )
        .expect_err("bytes");
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
    }
    update(
        &sandbox,
        &driver,
        &id,
        "blobs",
        vec![change(&[("id", text("1"))], &[("note", text("fine"))])],
    )
    .expect("a text column is editable");
}

/// InnoDB waits 50 seconds for a row another transaction holds, with the
/// connection's lock held the whole time.
#[test]
fn a_row_locked_by_another_session_is_refused_in_bounded_time() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "locked");
    let (driver, id) = sandbox.driver();
    let mut holder = sandbox.admin();
    holder
        .query_drop(format!(
            "START TRANSACTION; SELECT * FROM `{}`.people WHERE id = 1 FOR UPDATE",
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
    holder.query_drop("ROLLBACK").expect("releasing the lock");
    let outcome = outcome.expect("the Save must not wait on a lock for ever");
    worker.join().expect("the worker finishes");

    let error = outcome.expect_err("the row is locked");
    assert_eq!(error.code, ErrorCode::Internal, "{error:?}");
    assert!(error.message.contains("locked"), "{}", error.message);
    assert_eq!(age(&sandbox, &driver, &id), plain("30"));
    update(
        &sandbox,
        &driver,
        &id,
        "people",
        vec![change(&[("id", text("1"))], &[("age", text("32"))])],
    )
    .expect("the connection is usable once the lock is gone");
}

/// The lock wait is shortened for the Save and put back for everything else.
#[test]
fn the_shorter_lock_wait_does_not_outlive_the_save() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "restore");
    let (driver, id) = sandbox.driver();
    let wait = |driver: &MysqlDriver, id: &ConnectionId| {
        read(
            driver,
            id,
            "SELECT CAST(@@session.innodb_lock_wait_timeout AS CHAR)",
        )
    };
    let before = wait(&driver, &id);
    update(
        &sandbox,
        &driver,
        &id,
        "people",
        vec![change(&[("id", text("1"))], &[("age", text("31"))])],
    )
    .expect("an edit");
    assert_eq!(wait(&driver, &id), before);
    update(
        &sandbox,
        &driver,
        &id,
        "people",
        vec![change(&[("id", text("99"))], &[("age", text("31"))])],
    )
    .expect_err("a conflict");
    assert_eq!(wait(&driver, &id), before, "also after a refused save");
}

/// An account that may read the table but not write it.
#[test]
fn a_user_without_the_privilege_is_read_only_and_not_a_syntax_error() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "privilege");
    let account = format!("zode_fx_reader_{}", std::process::id());
    let mut admin = sandbox.admin();
    let created = admin.query_drop(format!(
        "CREATE USER '{account}'@'%' IDENTIFIED BY 'reader'; \
         GRANT SELECT ON `{}`.* TO '{account}'@'%'",
        sandbox.schema
    ));
    if let Err(error) = created {
        println!("skipped: this account cannot create users ({error})");
        return;
    }
    let (_, rest) = url.split_once("://").expect("a URL");
    let rest = rest.rsplit_once('@').map_or(rest, |(_, tail)| tail);
    let (driver, id) = connect(&format!("mysql://{account}:reader@{rest}"));
    let outcome = update(
        &sandbox,
        &driver,
        &id,
        "people",
        vec![change(&[("id", text("1"))], &[("age", text("31"))])],
    );
    if let Err(error) = admin.query_drop(format!("DROP USER '{account}'@'%'")) {
        eprintln!("could not drop {account}: {error}");
    }
    let error = outcome.expect_err("no UPDATE privilege");
    assert_eq!(error.code, ErrorCode::ReadOnly, "{error:?}");
}

/// Preparing the statement first must not change what a page is.
#[test]
fn a_page_is_still_a_page_behind_the_single_statement_check() {
    let Some(url) = server_url("ZODE_TEST_MYSQL_URL") else {
        return;
    };
    let sandbox = Sandbox::new(&url, "paging");
    let (driver, id) = sandbox.driver();
    let page = |limit: u32, offset: u64| -> ResultSet {
        let value = driver
            .query(json!({
                "connection_id": id, "limit": limit, "offset": offset, "request_id": "t",
                "sql": "SELECT 1 AS n, NULL AS nothing, '' AS blank UNION ALL SELECT 2, NULL, '' \
                        UNION ALL SELECT 3, NULL, '' UNION ALL SELECT 4, NULL, '' \
                        UNION ALL SELECT 5, NULL, ''",
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
    assert_eq!(numbers(&page(2, 2)), ["3", "4"]);
    let last = page(2, 4);
    assert_eq!(numbers(&last), ["5"]);
    assert!(!last.truncated);
    // The connection is still in a state to take the next one.
    read(&driver, &id, "SELECT 1");
}
