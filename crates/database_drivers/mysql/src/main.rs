//! The MySQL driver.
//!
//! Written against `crates/database/PROTOCOL.md` rather than against the two
//! drivers that came before it -- that was the point of writing it last. Where
//! it disagrees with them (backtick quoting, no cursor to page with, no real
//! boolean) it says so where the disagreement lives, and the protocol itself
//! did not have to move.

mod introspect;
mod paging;
mod values;

use database::protocol::{
    CancelParams, Capabilities, ConnectParams, ConnectResult, ConnectionField, ConnectionForm,
    ConnectionId, ConnectionRef, DescribeTableParams, DescribeTableResult, Empty, ErrorCode,
    InitializeResult, ListSchemasResult, ListTablesParams, ListTablesResult, NoParams,
    PROTOCOL_VERSION, QueryParams, ResponseError, ResultColumn, ResultSet,
};
use database::server::{Driver, serve, typed};
use mysql::prelude::Queryable as _;
use mysql::{Conn, Opts, OptsBuilder};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

struct Live {
    conn: Mutex<Conn>,
    /// MySQL cancels by `KILL QUERY <id>` from *another* connection, so both
    /// the id and the means to open that second connection have to be reachable
    /// without the lock the running query holds.
    connection_id: u32,
    opts: Opts,
    running: Mutex<Option<String>>,
}

#[derive(Default)]
struct MysqlDriver {
    connections: Mutex<HashMap<String, Arc<Live>>>,
    next_id: AtomicU64,
}

impl MysqlDriver {
    fn live(&self, id: &ConnectionId) -> Result<Arc<Live>, ResponseError> {
        self.connections
            .lock()
            .map_err(poisoned)?
            .get(&id.0)
            .cloned()
            .ok_or_else(|| {
                ResponseError::new(ErrorCode::UnknownConnection, "no such connection")
                    .with_detail(id.0.clone())
            })
    }
}

fn poisoned<T>(_: T) -> ResponseError {
    ResponseError::new(
        ErrorCode::Internal,
        "the driver's connection table was left locked by a panicking thread",
    )
}

impl Driver for MysqlDriver {
    fn initialize(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |_: NoParams| {
            Ok(InitializeResult {
                protocol_version: PROTOCOL_VERSION,
                driver_name: "MySQL".into(),
                capabilities: Capabilities {
                    // MySQL's databases are what the protocol calls schemas, and
                    // a server holds many -- so the tree keeps that level.
                    multiple_schemas: true,
                    cancellation: true,
                    identifier_quote: Some(values::IDENTIFIER_QUOTE.to_string()),
                    connection_form: Some(connection_form()),
                },
            })
        })
    }

    fn connect(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: ConnectParams| {
            let opts = Opts::from_url(&params.url).map_err(|error| {
                ResponseError::new(ErrorCode::Connection, "could not read the connection URL")
                    .with_detail(error.to_string())
            })?;
            let opts = match params.secret.as_deref() {
                Some(secret) => Opts::from(OptsBuilder::from_opts(opts).pass(Some(secret))),
                None => opts,
            };

            let mut conn = Conn::new(opts.clone()).map_err(values::error)?;
            let connection_id = conn.connection_id();
            let current = introspect::current_schema(&mut conn)?;

            let live = Arc::new(Live {
                conn: Mutex::new(conn),
                connection_id,
                opts,
                running: Mutex::new(None),
            });
            let id = format!("mysql-{}", self.next_id.fetch_add(1, Ordering::SeqCst));
            self.connections
                .lock()
                .map_err(poisoned)?
                .insert(id.clone(), live);

            Ok(ConnectResult {
                connection_id: ConnectionId(id),
                default_schema: current,
            })
        })
    }

    fn disconnect(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: ConnectionRef| {
            self.connections
                .lock()
                .map_err(poisoned)?
                .remove(&params.connection_id.0);
            Ok(Empty {})
        })
    }

    fn list_schemas(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: ConnectionRef| {
            let live = self.live(&params.connection_id)?;
            let mut conn = live.conn.lock().map_err(poisoned)?;
            let current = introspect::current_schema(&mut conn)?;
            Ok(ListSchemasResult {
                schemas: introspect::schemas(&mut conn, current.as_deref())?,
            })
        })
    }

    fn list_tables(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: ListTablesParams| {
            let live = self.live(&params.connection_id)?;
            let mut conn = live.conn.lock().map_err(poisoned)?;
            Ok(ListTablesResult {
                tables: introspect::tables(&mut conn, &params.schema)?,
            })
        })
    }

    fn describe_table(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: DescribeTableParams| {
            let live = self.live(&params.connection_id)?;
            let mut conn = live.conn.lock().map_err(poisoned)?;
            Ok(DescribeTableResult {
                columns: introspect::columns(&mut conn, &params.schema, &params.table)?,
            })
        })
    }

    fn query(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: QueryParams| {
            let live = self.live(&params.connection_id)?;
            let mut conn = live.conn.lock().map_err(poisoned)?;
            // Claimed only once the connection is ours. Set any earlier and a
            // `cancel` naming this request could kill whichever query is
            // actually running while this one still waits for the lock.
            *live.running.lock().map_err(poisoned)? = Some(params.request_id.clone());
            let result = run_page(&mut conn, &params);
            *live.running.lock().map_err(poisoned)? = None;
            result
        })
    }

    fn cancel(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: CancelParams| {
            let live = self.live(&params.connection_id)?;
            let running = live.running.lock().map_err(poisoned)?;
            // A `cancel` naming a query that already finished must not reach
            // through and kill the next one.
            if running.as_deref() == Some(params.request_id.as_str()) {
                // Best effort, and on its own connection: MySQL will not take a
                // KILL on the socket it is busy on.
                if let Ok(mut killer) = Conn::new(live.opts.clone()) {
                    killer
                        .query_drop(format!("KILL QUERY {}", live.connection_id))
                        .ok();
                }
            }
            Ok(Empty {})
        })
    }
}

/// One page, inside a read-only transaction.
///
/// Unlike the PostgreSQL driver there is no cursor to page with -- MySQL has
/// none a client can `FETCH` from -- so a wrappable statement is limited by a
/// derived table and everything else is run whole. See `paging`.
fn run_page(conn: &mut Conn, params: &QueryParams) -> Result<ResultSet, ResponseError> {
    let started = Instant::now();

    // The read-only guarantee.. Nothing inside a transaction can lift its
    // read-only mode, which `SET SESSION` cannot claim.
    conn.query_drop(paging::BEGIN_READ_ONLY)
        .map_err(values::error)?;
    let page = read_page(conn, params);
    // Rolled back whether or not the statement worked: the transaction did
    // nothing to keep, and leaving one open would hold locks on the server.
    conn.query_drop(paging::END_READ_ONLY).ok();
    page.map(|mut page| {
        // Measured around the whole transaction, not just the statement: the
        // BEGIN and the ROLLBACK are round trips the user waited for too.
        page.elapsed_ms = started.elapsed().as_millis() as u64;
        page
    })
}

fn read_page(conn: &mut Conn, params: &QueryParams) -> Result<ResultSet, ResponseError> {
    let sql = paging::paged(&params.sql, params.limit, params.offset);
    let mut result = conn.query_iter(&sql).map_err(values::error)?;

    let columns = result.columns();
    let kinds: Vec<_> = columns
        .as_ref()
        .iter()
        .map(|column| column.column_type())
        .collect();
    let names: Vec<ResultColumn> = columns
        .as_ref()
        .iter()
        .map(|column| ResultColumn {
            name: column.name_str().into_owned(),
            type_name: format!("{:?}", column.column_type())
                .trim_start_matches("MYSQL_TYPE_")
                .to_ascii_lowercase(),
        })
        .collect();

    let wanted = params.limit as usize;
    let mut rows = Vec::new();
    let mut truncated = false;
    for row in result.by_ref() {
        let row = row.map_err(values::error)?;
        if rows.len() == wanted {
            // One row past the page answers `truncated` without a `COUNT(*)`.
            // For a statement that could not be wrapped this is a client-side
            // stop rather than a server-side one -- see `paging`.
            truncated = true;
            break;
        }
        rows.push(
            (0..names.len())
                .map(|index| {
                    values::cell(
                        row.as_ref(index).and_then(|value| match value {
                            mysql::Value::NULL => None,
                            mysql::Value::Bytes(bytes) => Some(bytes.as_slice()),
                            _ => None,
                        }),
                        kinds
                            .get(index)
                            .copied()
                            .unwrap_or(mysql::consts::ColumnType::MYSQL_TYPE_VAR_STRING),
                    )
                })
                .collect(),
        );
    }

    Ok(ResultSet {
        columns: names,
        rows,
        truncated,
        // Overwritten by `run_page`, which owns the timing.
        elapsed_ms: 0,
    })
}

/// Host, port, user, database -- and a password that goes to the keychain, not
/// into the URL this builds.
fn connection_form() -> ConnectionForm {
    let field = |key: &str, label: &str, default: &str, group: &str| ConnectionField {
        key: key.into(),
        label: label.into(),
        group: Some(group.to_string()),
        default: (!default.is_empty()).then(|| default.to_string()),
        // Everything here sits inside the URL's authority, where an unescaped
        // `@` or `/` in a name would change where the host begins.
        url_encoded: true,
        ..Default::default()
    };

    ConnectionForm {
        fields: vec![
            field("host", "Host", "localhost", "Connection"),
            field("port", "Port", "3306", "Connection"),
            field("database", "Database", "", "Connection"),
            field("user", "Username", "root", "Authentication"),
            ConnectionField {
                key: "password".into(),
                label: "Password".into(),
                group: Some("Authentication".into()),
                secret: true,
                ..Default::default()
            },
        ],
        url_template: "mysql://{user}@{host}:{port}/{database}".into(),
    }
}

/// Picks the cryptography rustls will use, before anything asks it to.
///
/// The `mysql` crate's own documentation says to do this, and the reason is the
/// same one the PostgreSQL driver hit: rustls infers its provider from crate
/// features and panics when they name two. Cargo unifies features across
/// whatever packages share one `cargo build`, and the bundle scripts build this
/// driver alongside the editor -- which brings `ring` in through `hyper-rustls`
/// while this driver's rustls asks for `aws-lc-rs`. The result was a driver
/// that worked when built alone and died at the first TLS handshake when built
/// the way it ships.
fn install_crypto_provider() {
    // The error only means another thread installed one first, and every
    // caller here installs the same one.
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .ok();
}

fn main() -> std::io::Result<()> {
    // Before anything can ask rustls for a handshake, and not lazily: the
    // failure it prevents is a panic on a background thread, which surfaces as
    // a connection that simply never answers.
    install_crypto_provider();

    let driver = Arc::new(MysqlDriver::default());
    serve(driver, std::io::stdin().lock(), std::io::stdout())
}

#[cfg(test)]
mod crypto_tests {
    /// Asked the way the connection path asks it. Before the provider was
    /// installed outright this panicked, and only in builds where another
    /// package had pulled in a second provider -- so a driver could pass every
    /// test and still die at the first handshake once it was packaged.
    #[test]
    fn rustls_knows_which_cryptography_to_use() {
        super::install_crypto_provider();
        assert!(
            rustls::crypto::CryptoProvider::get_default().is_some(),
            "a TLS connection would panic on a background thread and simply never answer"
        );
    }
}
