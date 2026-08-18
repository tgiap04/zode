//! Running a real driver binary and holding it to `PROTOCOL.md`.
//!
//! Every other test in this workspace stops at the edge of the process: the
//! client's tests speak to a fake, and each driver's own tests check the pieces
//! it builds a statement from. Nothing was left that started the binary, spoke
//! the protocol to it and read what a real engine answered -- so a driver could
//! satisfy every test in the tree and still be unusable.
//!
//! Deliberately std-only, with no gpui and no async: this is a test harness for
//! a child process, and giving it a runtime would mean the thing under test ran
//! differently here than it does in Zode.
//!
//! The shipped drivers all run [`shared_suite`], SQLite included. That last one
//! matters most: the servers are not always there, so without an engine that
//! needs no server this harness would be skipped in every run and could rot
//! unnoticed.

use crate::protocol::{Cell, ErrorCode, PROTOCOL_VERSION, ResultSet};
use serde_json::{Value, json};
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// A driver binary, running, with the pipe to talk to it.
pub struct DriverProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl DriverProcess {
    /// The blocking `std::process::Command` is banned across this workspace
    /// because it stalls whichever thread calls it. That is exactly what is
    /// wanted here: a `#[test]` has no GPUI executor to hand the work to, and
    /// the test thread has nothing else to do but wait for the driver.
    #[allow(
        clippy::disallowed_methods,
        reason = "a test harness for a child process, with no executor to spawn onto"
    )]
    pub fn start(executable: &Path) -> Self {
        let mut child = Command::new(executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited rather than piped: a driver logs to stderr, and a pipe
            // nobody reads would eventually block the process being tested.
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap_or_else(|error| panic!("could not start {}: {error}", executable.display()));

        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = BufReader::new(child.stdout.take().expect("stdout was piped"));
        Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    /// One request, and the driver's answer -- result or error.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, (ErrorCode, String)> {
        let id = self.next_id;
        self.next_id += 1;

        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{request}").expect("the driver's stdin is open");
        self.stdin.flush().expect("the driver's stdin is open");

        let mut line = String::new();
        let read = self
            .stdout
            .read_line(&mut line)
            .expect("the driver's stdout is readable");
        assert!(
            read > 0,
            "the driver closed its stdout answering `{method}`"
        );

        let response: Value =
            serde_json::from_str(&line).unwrap_or_else(|error| panic!("`{line}`: {error}"));
        assert_eq!(
            response["id"], id,
            "answers may arrive out of order, but this harness sends one at a time"
        );

        if let Some(error) = response.get("error").filter(|error| !error.is_null()) {
            let code: ErrorCode =
                serde_json::from_value(error["code"].clone()).unwrap_or_else(|_| {
                    panic!("`{}` is not an error code this protocol has", error["code"])
                });
            return Err((
                code,
                error["message"].as_str().unwrap_or_default().to_string(),
            ));
        }
        Ok(response["result"].clone())
    }

    /// The same, for the calls a test expects to succeed.
    pub fn expect(&mut self, method: &str, params: Value) -> Value {
        self.call(method, params)
            .unwrap_or_else(|(code, message)| panic!("`{method}` failed as {code:?}: {message}"))
    }
}

impl Drop for DriverProcess {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

/// The few statements the shared suite cannot write for itself.
///
/// Small on purpose. Anything larger would be a second place where each
/// engine's differences live, and the point of the protocol is that there is
/// only one -- inside its driver.
pub struct Dialect {
    /// Something that writes, to check the connection refuses it.
    pub write_statement: &'static str,
    /// Two columns, in this order: a null and an empty string.
    pub null_and_empty: &'static str,
    /// At least three rows, in a stable order, with one column.
    pub three_rows: &'static str,
}

/// What every driver must do, whatever it is talking to.
///
/// Not a substitute for a driver's own tests -- it makes no claim about types,
/// schema introspection or paging strategy. It claims the driver starts, speaks
/// the framing, refuses to write, pages, and tells a null from an empty string.
pub fn shared_suite(executable: &Path, url: &str, dialect: Dialect) {
    let mut driver = DriverProcess::start(executable);

    let initialized = driver.expect("initialize", json!({}));
    assert_eq!(
        initialized["protocol_version"], PROTOCOL_VERSION,
        "a driver whose version differs is refused by the client, so it must not differ here"
    );
    assert!(
        !initialized["driver_name"]
            .as_str()
            .unwrap_or_default()
            .is_empty(),
        "the name is the only place anyone can check which driver answered"
    );

    let connected = driver.expect("connect", json!({"url": url}));
    let connection_id = connected["connection_id"].clone();
    assert!(
        connection_id.is_string(),
        "connect must name the connection"
    );

    let schemas = driver.expect("list_schemas", json!({"connection_id": connection_id}));
    assert!(
        !schemas["schemas"]
            .as_array()
            .is_none_or(|schemas| schemas.is_empty()),
        "a connected driver has at least the schema it is pointed at"
    );

    // The guarantee the whole design rests on: the engine refuses, not Zode.
    let refused = driver.call(
        "query",
        json!({
            "connection_id": connection_id,
            "sql": dialect.write_statement,
            "limit": 10,
            "offset": 0,
            "request_id": "write",
        }),
    );
    match refused {
        Err((ErrorCode::ReadOnly, _)) => {}
        Err((code, message)) => panic!(
            "a write must be refused as `read_only`, not as {code:?}: {message} -- \
             the user is told the connection is read-only, not that their SQL is wrong"
        ),
        Ok(_) => panic!(
            "`{}` was allowed on a read-only connection",
            dialect.write_statement
        ),
    }

    let page = query(&mut driver, &connection_id, dialect.null_and_empty, 10, 0);
    let row = page.rows.first().expect("one row");
    assert_eq!(row[0], Cell::Null, "a null must cross as a null");
    assert_ne!(
        row[1],
        Cell::Null,
        "an empty string is not a null, and a database client that confuses them is broken"
    );

    let first = query(&mut driver, &connection_id, dialect.three_rows, 2, 0);
    assert_eq!(first.rows.len(), 2, "a page holds what it was asked for");
    assert!(
        first.truncated,
        "and says when there is more, without a COUNT(*)"
    );

    let second = query(&mut driver, &connection_id, dialect.three_rows, 2, 2);
    assert_eq!(second.rows.len(), 1, "the rest of the rows, and no more");
    assert_ne!(
        first.rows[0], second.rows[0],
        "an offset that returns the first page again is paging that does not page"
    );

    // Named after nothing that exists: a driver that answers this at all would
    // be reaching into whichever connection it happened to have.
    let stray = driver.call(
        "query",
        json!({
            "connection_id": "no-such-connection",
            "sql": dialect.three_rows,
            "limit": 1,
            "offset": 0,
            "request_id": "stray",
        }),
    );
    assert!(
        matches!(stray, Err((ErrorCode::UnknownConnection, _))),
        "a call naming an unknown connection must say so, not fall through to another"
    );

    driver.expect("disconnect", json!({"connection_id": connection_id}));
}

fn query(
    driver: &mut DriverProcess,
    connection_id: &Value,
    sql: &str,
    limit: u32,
    offset: u64,
) -> ResultSet {
    let result = driver.expect(
        "query",
        json!({
            "connection_id": connection_id,
            "sql": sql,
            "limit": limit,
            "offset": offset,
            "request_id": format!("page-{offset}"),
        }),
    );
    serde_json::from_value(result).expect("a query answers with a result set")
}

/// Reads a server URL from the environment, or explains why the test did not
/// run.
///
/// Printed rather than silent: a test that quietly does nothing reads exactly
/// like a test that passed, and this one needs a server that is often absent.
pub fn server_url(variable: &str) -> Option<String> {
    match std::env::var(variable) {
        Ok(url) if !url.is_empty() => Some(url),
        _ => {
            println!("skipped: set {variable} to a server to run this against one");
            None
        }
    }
}
