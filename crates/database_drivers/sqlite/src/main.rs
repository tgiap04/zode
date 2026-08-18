//! The SQLite driver.
//!
//! Read-only is not a convention here: the file is opened with
//! `SQLITE_OPEN_READ_ONLY`, so the handle itself cannot write. That is stronger
//! than `PRAGMA query_only`, which any statement the user types can turn back
//! off -- and a read-only mode the user can switch off by typing is not one.

mod introspect;
mod values;

use database::protocol::{
    CancelParams, Capabilities, ConnectParams, ConnectResult, ConnectionField, ConnectionForm,
    ConnectionId, ConnectionRef, DescribeTableParams, DescribeTableResult, Empty, ErrorCode,
    InitializeResult, ListSchemasResult, ListTablesParams, ListTablesResult, NoParams,
    PROTOCOL_VERSION, QueryParams, ResponseError, ResultColumn, ResultSet,
};
use database::server::{Driver, serve, typed};
use rusqlite::{Connection, InterruptHandle, OpenFlags};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

struct Live {
    connection: Mutex<Connection>,
    /// Reachable without the connection lock -- which is the whole point:
    /// `cancel` arrives while `query` holds that lock.
    interrupt: InterruptHandle,
    /// Which request the connection is busy with, so a stale `cancel` cannot
    /// interrupt the query that came after the one it named.
    running: Mutex<Option<String>>,
}

#[derive(Default)]
struct SqliteDriver {
    connections: Mutex<HashMap<String, Arc<Live>>>,
    next_id: AtomicU64,
}

impl SqliteDriver {
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

/// `sqlite://path`, `sqlite:path` or a bare path -- all three turn up in
/// settings written by hand, and rejecting two of them teaches nothing.
fn path_from_url(url: &str) -> &str {
    url.strip_prefix("sqlite://")
        .or_else(|| url.strip_prefix("sqlite:"))
        .unwrap_or(url)
}

impl Driver for SqliteDriver {
    fn initialize(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |_: NoParams| {
            Ok(InitializeResult {
                protocol_version: PROTOCOL_VERSION,
                driver_name: "SQLite".into(),
                capabilities: Capabilities {
                    // One database per file. `temp` is filtered out and ATTACH
                    // needs a write handle, so `main` is all there ever is --
                    // and a tree that draws a lone schema node wastes a level.
                    multiple_schemas: false,
                    cancellation: true,
                    identifier_quote: None,
                    connection_form: Some(connection_form()),
                },
            })
        })
    }

    fn connect(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: ConnectParams| {
            let path = path_from_url(&params.url);
            // The read-only guarantee, and the only one. Note it also refuses
            // to create a missing file, which is what we want: a typo in a path
            // should say "no such database", not open an empty one.
            let connection = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            )
            .map_err(values::error)?;

            let live = Arc::new(Live {
                interrupt: connection.get_interrupt_handle(),
                connection: Mutex::new(connection),
                running: Mutex::new(None),
            });
            let id = format!("sqlite-{}", self.next_id.fetch_add(1, Ordering::SeqCst));
            self.connections
                .lock()
                .map_err(poisoned)?
                .insert(id.clone(), live);

            Ok(ConnectResult {
                connection_id: ConnectionId(id),
                default_schema: Some("main".into()),
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
            let connection = live.connection.lock().map_err(poisoned)?;
            Ok(ListSchemasResult {
                schemas: introspect::schemas(&connection)?,
            })
        })
    }

    fn list_tables(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: ListTablesParams| {
            let live = self.live(&params.connection_id)?;
            let connection = live.connection.lock().map_err(poisoned)?;
            Ok(ListTablesResult {
                tables: introspect::tables(&connection, &params.schema)?,
            })
        })
    }

    fn describe_table(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: DescribeTableParams| {
            let live = self.live(&params.connection_id)?;
            let connection = live.connection.lock().map_err(poisoned)?;
            Ok(DescribeTableResult {
                columns: introspect::columns(&connection, &params.schema, &params.table)?,
            })
        })
    }

    fn query(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: QueryParams| {
            let live = self.live(&params.connection_id)?;
            let connection = live.connection.lock().map_err(poisoned)?;
            // Claimed only once the connection is ours. Set any earlier and a
            // `cancel` naming this request could interrupt whichever query is
            // actually running while this one is still waiting for the lock.
            *live.running.lock().map_err(poisoned)? = Some(params.request_id.clone());
            let result = run_page(&connection, &params);
            *live.running.lock().map_err(poisoned)? = None;
            result
        })
    }

    fn cancel(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: CancelParams| {
            let live = self.live(&params.connection_id)?;
            let running = live.running.lock().map_err(poisoned)?;
            // A `cancel` that names a query which already finished must not
            // reach through and kill the next one.
            if running.as_deref() == Some(params.request_id.as_str()) {
                live.interrupt.interrupt();
            }
            Ok(Empty {})
        })
    }
}

/// One page of a statement's results.
///
/// Stepped and skipped rather than wrapped in `SELECT * FROM (...) LIMIT ?`:
/// wrapping only works for statements SQLite will accept as a subquery, which
/// rules out `PRAGMA` and `EXPLAIN`, and those are exactly what someone reaches
/// for when a query is behaving strangely.
fn run_page(connection: &Connection, params: &QueryParams) -> Result<ResultSet, ResponseError> {
    let started = Instant::now();
    let mut statement = connection.prepare(&params.sql).map_err(values::error)?;

    // Collected into owned values before `raw_query` takes the statement
    // mutably, and because a declared type is absent for an expression column
    // (`SELECT id + 1`) -- left blank rather than guessed at.
    let columns: Vec<ResultColumn> = statement
        .columns()
        .iter()
        .map(|column| ResultColumn {
            name: column.name().to_string(),
            type_name: column.decl_type().unwrap_or_default().to_string(),
        })
        .collect();
    let declared: Vec<String> = columns
        .iter()
        .map(|column| column.type_name.clone())
        .collect();

    let mut rows = statement.raw_query();
    for _ in 0..params.offset {
        if rows.next().map_err(values::error)?.is_none() {
            break;
        }
    }

    // One past the page, so `truncated` is answered without a `COUNT(*)`.
    let wanted = params.limit as usize;
    let mut page = Vec::with_capacity(wanted.min(1024));
    let mut truncated = false;
    while let Some(row) = rows.next().map_err(values::error)? {
        if page.len() == wanted {
            truncated = true;
            break;
        }
        page.push(
            (0..columns.len())
                .map(|index| {
                    let value = row.get_ref(index).map_err(values::error)?;
                    Ok(values::cell(value, &declared[index]))
                })
                .collect::<Result<Vec<_>, ResponseError>>()?,
        );
    }

    Ok(ResultSet {
        columns,
        rows: page,
        truncated,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

/// One file, and nothing else to ask for.
///
/// `url_encoded` stays off: the template *is* the path, and encoding it would
/// turn every separator into `%2F`.
fn connection_form() -> ConnectionForm {
    ConnectionForm {
        fields: vec![ConnectionField {
            key: "path".into(),
            label: "Database file".into(),
            group: Some("Connection".into()),
            placeholder: Some("/path/to/app.sqlite".into()),
            ..Default::default()
        }],
        url_template: "{path}".into(),
    }
}

fn main() -> std::io::Result<()> {
    let driver = Arc::new(SqliteDriver::default());
    serve(driver, std::io::stdin().lock(), std::io::stdout())
}

#[cfg(test)]
mod tests;
