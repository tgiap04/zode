//! The PostgreSQL driver.
//!
//! Read-only is enforced per statement, not per session. `SET SESSION
//! CHARACTERISTICS AS TRANSACTION READ ONLY` is undone by one
//! `SET SESSION CHARACTERISTICS AS TRANSACTION READ WRITE` typed into the
//! scratch buffer -- so every user statement runs inside its own
//! `BEGIN TRANSACTION READ ONLY`, which nothing inside that transaction can
//! lift. The lesson is the SQLite driver's: a read-only mode the user can
//! switch off by typing is not one.

mod introspect;
mod update;
mod values;

use database::protocol::{
    CancelParams, Capabilities, ConnectParams, ConnectResult, ConnectionField, ConnectionForm,
    ConnectionId, ConnectionRef, DescribeTableParams, DescribeTableResult, Empty, ErrorCode,
    InitializeResult, ListSchemasResult, ListTablesParams, ListTablesResult, NoParams,
    PROTOCOL_VERSION, QueryParams, ResponseError, ResultColumn, ResultSet, UpdateRowsParams,
    UpdateRowsResult,
};
use database::server::{Driver, serve, typed};
use postgres::{Client, Config};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

mod paging;
mod tls;

struct Live {
    client: Mutex<Client>,
    /// Reachable without the client lock, which is the whole point: PostgreSQL
    /// cannot cancel from the connection that is busy, so this opens a second
    /// one to ask.
    cancel: postgres::CancelToken,
    running: Mutex<Option<String>>,
}

#[derive(Default)]
struct PostgresDriver {
    connections: Mutex<HashMap<String, Arc<Live>>>,
    next_id: AtomicU64,
}

impl PostgresDriver {
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

impl Driver for PostgresDriver {
    fn initialize(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |_: NoParams| {
            Ok(InitializeResult {
                protocol_version: PROTOCOL_VERSION,
                driver_name: "PostgreSQL".into(),
                capabilities: Capabilities {
                    multiple_schemas: true,
                    cancellation: true,
                    identifier_quote: None,
                    connection_form: Some(connection_form()),
                    // Absent: `SELECT * FROM {schema}.{table}` is exactly what
                    // this engine wants, and the default says so.
                    read_table_template: None,
                    row_updates: true,
                },
            })
        })
    }

    fn connect(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: ConnectParams| {
            let mut config = Config::from_str(&params.url).map_err(values::error)?;
            if let Some(secret) = params.secret.as_deref() {
                config.password(secret);
            }

            // Named here rather than left to the engine's own words: a
            // `tokio_postgres` connect failure knows nothing about which server
            // it meant, so a person with several connections configured was told
            // one had failed and not which one.
            let address = values::server_address(&config);
            let mut client = config.connect(tls::connector()).map_err(|error| {
                let mut error = values::error(error);
                // Only when reaching the server is what failed. Credentials
                // being refused is a different sentence, and the engine's own is
                // the better one.
                if error.code == ErrorCode::Connection {
                    error.message = format!("could not reach {address}");
                }
                error
            })?;
            let schema = introspect::current_schema(&mut client)?;

            let live = Arc::new(Live {
                cancel: client.cancel_token(),
                client: Mutex::new(client),
                running: Mutex::new(None),
            });
            let id = format!("postgres-{}", self.next_id.fetch_add(1, Ordering::SeqCst));
            self.connections
                .lock()
                .map_err(poisoned)?
                .insert(id.clone(), live);

            Ok(ConnectResult {
                connection_id: ConnectionId(id),
                default_schema: Some(schema),
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
            let mut client = live.client.lock().map_err(poisoned)?;
            let current = introspect::current_schema(&mut client)?;
            Ok(ListSchemasResult {
                schemas: introspect::schemas(&mut client, &current)?,
            })
        })
    }

    fn list_tables(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: ListTablesParams| {
            let live = self.live(&params.connection_id)?;
            let mut client = live.client.lock().map_err(poisoned)?;
            Ok(ListTablesResult {
                tables: introspect::tables(&mut client, &params.schema)?,
            })
        })
    }

    fn describe_table(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: DescribeTableParams| {
            let live = self.live(&params.connection_id)?;
            let mut client = live.client.lock().map_err(poisoned)?;
            Ok(DescribeTableResult {
                columns: introspect::columns(&mut client, &params.schema, &params.table)?,
            })
        })
    }

    fn query(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: QueryParams| {
            let live = self.live(&params.connection_id)?;
            let mut client = live.client.lock().map_err(poisoned)?;
            // Claimed only once the connection is ours. Set any earlier and a
            // `cancel` naming this request could interrupt whichever query is
            // actually running while this one is still waiting for the lock.
            *live.running.lock().map_err(poisoned)? = Some(params.request_id.clone());
            let result = run_page(&mut client, &params);
            *live.running.lock().map_err(poisoned)? = None;
            result
        })
    }

    fn update_rows(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: UpdateRowsParams| {
            let live = self.live(&params.connection_id)?;
            if params.changes.is_empty() {
                return Ok(UpdateRowsResult { updated: 0 });
            }
            let mut client = live.client.lock().map_err(poisoned)?;
            update::apply(&mut client, &params)
        })
    }

    fn cancel(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: CancelParams| {
            let live = self.live(&params.connection_id)?;
            let running = live.running.lock().map_err(poisoned)?;
            // A `cancel` naming a query that already finished must not reach
            // through and kill the next one.
            if running.as_deref() == Some(params.request_id.as_str()) {
                // Opens its own connection to ask, which is the only way:
                // PostgreSQL will not take a cancel on the socket it is busy on.
                live.cancel.cancel_query(tls::connector()).ok();
            }
            Ok(Empty {})
        })
    }
}

/// One page, through a cursor inside a read-only transaction.
///
/// A cursor rather than wrapping the statement in `SELECT * FROM (…) LIMIT`:
/// wrapping only works for statements PostgreSQL accepts as a subquery, which
/// rules out `EXPLAIN` and `SHOW` -- and those are exactly what someone reaches
/// for when a query is behaving strangely. `MOVE`/`FETCH` page any statement
/// that produces rows, entirely server-side.
fn run_page(client: &mut Client, params: &QueryParams) -> Result<ResultSet, ResponseError> {
    let started = Instant::now();
    client
        .batch_execute(paging::BEGIN_READ_ONLY)
        .map_err(values::error)?;
    declare_cursor(client, &params.sql)?;

    // One `simple_query` for the rest, which holds nothing the user typed:
    // values come back as the server's own text, which is what keeps `numeric`
    // exact.
    let messages = match client.simple_query(&paging::page_script(params.limit, params.offset)) {
        Ok(messages) => messages,
        Err(error) => {
            abandon_transaction(client);
            return Err(values::error(error));
        }
    };
    let (columns, rows, truncated) = paging::collect(&messages, params.limit);

    // Types come from a separate `prepare`, which is the only way to learn them
    // without decoding values in Rust. Best effort: a statement PostgreSQL will
    // not prepare (a multi-statement string, some utility commands) still shows
    // its rows, just without per-column kinds.
    let kinds = client
        .prepare(&params.sql)
        .ok()
        .map(|statement| {
            statement
                .columns()
                .iter()
                .map(|column| column.type_().clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let columns: Vec<ResultColumn> = columns
        .into_iter()
        .enumerate()
        .map(|(index, name)| ResultColumn {
            name,
            type_name: kinds
                .get(index)
                .map(|kind| kind.name().to_string())
                .unwrap_or_default(),
        })
        .collect();

    let rows = rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .enumerate()
                .map(|(index, value)| values::cell(value.as_deref(), kinds.get(index)))
                .collect()
        })
        .collect();

    Ok(ResultSet {
        columns,
        rows,
        truncated,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

/// Declares the page's cursor over the user's statement, which must be exactly
/// one.
///
/// Prepared before it is run so the server's own parser decides: a string with
/// a second statement in it is refused with "cannot insert multiple commands
/// into a prepared statement", which is the whole of the read-only guarantee
/// for what the user types. See `paging::declare_cursor`.
fn declare_cursor(client: &mut Client, sql: &str) -> Result<(), ResponseError> {
    let statement = match client.prepare(&paging::declare_cursor(sql)) {
        Ok(statement) => statement,
        Err(error) => {
            abandon_transaction(client);
            return Err(values::error(error));
        }
    };
    // Nothing here can supply a value for `$1`, and running it anyway would fail
    // on the client with an error that reads as a lost connection.
    if !statement.params().is_empty() {
        abandon_transaction(client);
        return Err(ResponseError::new(
            ErrorCode::Syntax,
            "a query cannot take parameters ($1, $2, ...)",
        ));
    }
    if let Err(error) = client.execute(&statement, &[]) {
        abandon_transaction(client);
        return Err(values::error(error));
    }
    Ok(())
}

/// Ends whatever transaction the paging script left open when it failed.
///
/// The script's own `ROLLBACK` is its last statement, and PostgreSQL stops a
/// multi-statement string at the first error -- so a runtime error (`1/0`, a
/// cancelled query) leaves the session in an aborted transaction block, where
/// every later call, browsing and editing alike, fails with "current transaction
/// is aborted". Nothing is lifted by this: it only closes the transaction that
/// was already refusing writes.
fn abandon_transaction(client: &mut Client) {
    if let Err(error) = client.batch_execute("ROLLBACK") {
        eprintln!("could not roll back after a failed query: {error}");
    }
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
            field("port", "Port", "5432", "Connection"),
            field("database", "Database", "postgres", "Connection"),
            field("user", "Username", "postgres", "Authentication"),
            ConnectionField {
                key: "password".into(),
                label: "Password".into(),
                group: Some("Authentication".into()),
                secret: true,
                ..Default::default()
            },
        ],
        url_template: "postgres://{user}@{host}:{port}/{database}".into(),
    }
}

fn main() -> std::io::Result<()> {
    // Before anything can ask rustls for a handshake, and not lazily: the
    // failure it prevents is a panic on a background thread, which surfaces as
    // a connection that simply never answers.
    tls::install_crypto_provider();

    let driver = Arc::new(PostgresDriver::default());
    serve(driver, std::io::stdin().lock(), std::io::stdout())
}

#[cfg(test)]
mod update_tests;
