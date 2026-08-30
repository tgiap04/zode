//! The MongoDB driver.
//!
//! Read-only is enforced by the statement whitelist in [`query`], not by the
//! connection. That is a weaker place to stand than the other two drivers get
//! -- SQLite opens the file `SQLITE_OPEN_READ_ONLY` and PostgreSQL runs every
//! statement inside a read-only transaction, so in both the *engine* refuses
//! the write. MongoDB offers no equivalent: a connection is as privileged as
//! its user, and the only server-side answer is a read-only role, which is the
//! user's to grant and not this driver's to assume. So the guarantee here is
//! that nothing but `find` and `aggregate` is ever sent, and that an
//! aggregation carrying `$out` or `$merge` is refused before it leaves.
//!
//! Mapping onto the protocol, which has no MongoDB in it and should not:
//! a schema is a database, a table is a collection, and a statement is a JSON
//! command document rather than SQL. Nothing above this layer learns any of
//! that.

mod introspect;
mod query;
mod values;

use database::protocol::{
    CancelParams, Capabilities, ConnectParams, ConnectResult, ConnectionField, ConnectionForm,
    ConnectionId, ConnectionRef, DescribeTableParams, DescribeTableResult, Empty, ErrorCode,
    InitializeResult, ListSchemasResult, ListTablesParams, ListTablesResult, NoParams,
    PROTOCOL_VERSION, QueryParams, ResponseError, ResultColumn, ResultSet,
};
use database::server::{Driver, serve, typed};
use mongodb::bson::Document;
use mongodb::options::ClientOptions;
use mongodb::sync::Client;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

struct Live {
    client: Client,
    /// The database the connection URL pointed at, so the tree can open it
    /// without guessing which of the server's databases was meant.
    default_database: String,
}

#[derive(Default)]
struct MongoDriver {
    connections: Mutex<HashMap<String, Arc<Live>>>,
    next_id: AtomicU64,
}

impl MongoDriver {
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

/// Where the options say they were going to connect, without the credentials.
///
/// Read back off the parsed options rather than off the URL, for the reason the
/// PostgreSQL driver names its address the same way: a failure that names no
/// server leaves the reader to work out which connection it was, and a URL
/// carries a password.
fn server_address(options: &ClientOptions) -> String {
    options
        .hosts
        .iter()
        .map(|host| host.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

impl Driver for MongoDriver {
    fn initialize(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |_: NoParams| {
            Ok(InitializeResult {
                protocol_version: PROTOCOL_VERSION,
                driver_name: "MongoDB".into(),
                capabilities: Capabilities {
                    multiple_schemas: true,
                    // Said plainly rather than offered and broken: the blocking
                    // client hands out no handle that reaches a running
                    // operation, so a cancel button here would be one that lies.
                    cancellation: false,
                    identifier_quote: values::IDENTIFIER_QUOTE,
                    connection_form: Some(connection_form()),
                },
            })
        })
    }

    fn connect(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: ConnectParams| {
            let mut options = ClientOptions::parse(&params.url)
                .run()
                .map_err(values::error)?;
            if let Some(secret) = params.secret.as_deref()
                && let Some(credential) = options.credential.as_mut()
            {
                credential.password = Some(secret.to_string());
            }
            let address = server_address(&options);
            let default_database = options
                .default_database
                .clone()
                .unwrap_or_else(|| "test".to_string());

            let client = Client::with_options(options).map_err(values::error)?;
            // `Client::with_options` connects lazily, so without this a
            // connection to a server that is not there would be reported as a
            // success and fail later at whatever the user clicked first.
            client
                .database(&default_database)
                .run_command(mongodb::bson::doc! { "ping": 1 })
                .run()
                .map_err(|error| {
                    let mut error = values::error(error);
                    if error.code == ErrorCode::Connection {
                        error.message = format!("could not reach {address}");
                    }
                    error
                })?;

            let live = Arc::new(Live {
                client,
                default_database: default_database.clone(),
            });
            let id = format!("mongodb-{}", self.next_id.fetch_add(1, Ordering::SeqCst));
            self.connections
                .lock()
                .map_err(poisoned)?
                .insert(id.clone(), live);

            Ok(ConnectResult {
                connection_id: ConnectionId(id),
                default_schema: Some(default_database),
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
            Ok(ListSchemasResult {
                schemas: introspect::databases(&live.client, &live.default_database)?,
            })
        })
    }

    fn list_tables(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: ListTablesParams| {
            let live = self.live(&params.connection_id)?;
            Ok(ListTablesResult {
                tables: introspect::collections(&live.client, &params.schema)?,
            })
        })
    }

    fn describe_table(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: DescribeTableParams| {
            let live = self.live(&params.connection_id)?;
            Ok(DescribeTableResult {
                columns: introspect::fields(&live.client, &params.schema, &params.table)?,
            })
        })
    }

    fn query(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |params: QueryParams| {
            let live = self.live(&params.connection_id)?;
            run_page(&live, &params)
        })
    }

    fn cancel(&self, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        typed(params, |_: CancelParams| {
            // `initialize` reports `cancellation: false`, so nothing should ask.
            // Answering rather than erroring keeps a caller that asks anyway
            // from seeing a failure it can do nothing about.
            Ok(Empty {})
        })
    }
}

/// One page of a statement's results.
///
/// Paged in the server with `skip`/`limit` rather than by reading and dropping,
/// and asking for one document more than the page so `truncated` costs no
/// second round trip -- the same bargain every other driver here makes.
fn run_page(live: &Live, params: &QueryParams) -> Result<ResultSet, ResponseError> {
    let started = Instant::now();
    let command = query::parse(&params.sql)?;
    let collection = live
        .client
        // The database the statement runs against is the one the connection
        // opened on. A command document naming its own database would be a
        // second way to say where a query goes, and the tree's selection would
        // then disagree with the buffer.
        .database(&live.default_database)
        .collection::<Document>(command.collection());

    // One past the page. `limit` is `u32` and this is `i64`, so no clamp is
    // needed for it -- but the sum is taken in `i64` all the same, because the
    // offset is a `u64` the caller chooses.
    let wanted = params.limit as i64;
    let skip = params.offset.min(i64::MAX as u64);

    let documents: Vec<Document> = match &command {
        query::Command::Find {
            filter,
            sort,
            projection,
            ..
        } => {
            let mut find = collection
                .find(filter.clone())
                .skip(skip)
                .limit(wanted.saturating_add(1));
            if let Some(sort) = sort {
                find = find.sort(sort.clone());
            }
            if let Some(projection) = projection {
                find = find.projection(projection.clone());
            }
            find.run()
                .map_err(values::error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(values::error)?
        }
        query::Command::Aggregate { pipeline, .. } => {
            let mut stages = pipeline.clone();
            stages.push(mongodb::bson::doc! { "$skip": skip as i64 });
            stages.push(mongodb::bson::doc! { "$limit": wanted.saturating_add(1) });
            collection
                .aggregate(stages)
                .run()
                .map_err(values::error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(values::error)?
        }
    };

    let truncated = documents.len() as i64 > wanted;
    let page = &documents[..documents.len().min(wanted.max(0) as usize)];

    let names = values::columns_across(page);
    let columns: Vec<ResultColumn> = names
        .iter()
        .map(|name| ResultColumn {
            name: name.clone(),
            // The type of the first document that carries this field. A
            // collection promises nothing about the rest, and a heading that
            // claimed otherwise would be inventing a schema.
            type_name: page
                .iter()
                .find_map(|document| document.get(name))
                .map(values::type_name)
                .unwrap_or("")
                .to_string(),
        })
        .collect();

    let rows = page
        .iter()
        .map(|document| {
            names
                .iter()
                .map(|name| match document.get(name) {
                    Some(value) => values::cell(value),
                    // Absent, not null -- but the grid has one way to draw
                    // "nothing here" and inventing a second would be a column
                    // of its own for a distinction nobody asked for.
                    None => database::protocol::Cell::Null,
                })
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

/// What a MongoDB URI actually needs.
///
/// A replica set is written as several hosts in one field rather than as
/// several fields: how many there are is the user's business and not a shape a
/// form can fix in advance.
///
/// `authSource` earns a field of its own because leaving it out is the single
/// most common way a MongoDB connection fails while the same credentials work
/// everywhere else. MongoDB authenticates against the database named in the
/// URI unless told otherwise, and most deployments create their users in
/// `admin` -- so a form without this asks for a user and a password and then
/// looks for them in the wrong place, and the server answers only
/// "Authentication failed".
fn connection_form() -> ConnectionForm {
    ConnectionForm {
        fields: vec![
            ConnectionField {
                key: "host".into(),
                label: "Host".into(),
                group: Some("Connection".into()),
                placeholder: Some("localhost:27017".into()),
                default: Some("localhost:27017".into()),
                ..Default::default()
            },
            ConnectionField {
                key: "database".into(),
                label: "Database".into(),
                group: Some("Connection".into()),
                url_encoded: true,
                ..Default::default()
            },
            ConnectionField {
                key: "user".into(),
                label: "User".into(),
                group: Some("Authentication".into()),
                url_encoded: true,
                ..Default::default()
            },
            ConnectionField {
                key: "password".into(),
                label: "Password".into(),
                group: Some("Authentication".into()),
                secret: true,
                ..Default::default()
            },
            ConnectionField {
                key: "auth_source".into(),
                label: "Auth database".into(),
                group: Some("Authentication".into()),
                // `admin` because that is where a deployment with
                // authentication turned on almost always keeps its users.
                // Anyone whose user lives in the database itself types that
                // name here -- which is a thing the form could not say at all
                // before.
                default: Some("admin".into()),
                url_encoded: true,
                ..Default::default()
            },
        ],
        url_template: "mongodb://{user}@{host}/{database}?authSource={auth_source}".into(),
    }
}

fn main() -> std::io::Result<()> {
    let driver = Arc::new(MongoDriver::default());
    serve(driver, std::io::stdin().lock(), std::io::stdout())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The address must come off the parsed options, never the URL, so no
    /// password can ride along into a message the UI shows and a log keeps.
    #[test]
    fn the_address_is_named_without_the_password() {
        let options = ClientOptions::parse("mongodb://someone:hunter2@db.example:27018/app")
            .run()
            .expect("a well-formed URI");
        let address = server_address(&options);
        assert_eq!(address, "db.example:27018");
        assert!(!address.contains("hunter2"), "{address}");
    }

    /// A replica set is several hosts, and a failure naming only the first
    /// would send the reader looking at the wrong machine.
    #[test]
    fn every_host_of_a_replica_set_is_named() {
        let options = ClientOptions::parse("mongodb://a.example:27017,b.example:27017/app")
            .run()
            .expect("a well-formed URI");
        assert_eq!(server_address(&options), "a.example:27017, b.example:27017");
    }
}
