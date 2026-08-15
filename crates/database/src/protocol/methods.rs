use super::types::{Capabilities, ColumnDef, ResultSet, SchemaRef, TableRef};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// One call a driver answers.
///
/// The name is what crosses the wire, so it is the part that must never change
/// without a version bump. Params and results are ordinary structs, which keeps
/// the client typed without the driver needing to know Rust at all.
pub trait Method {
    const NAME: &'static str;
    type Params: Serialize + DeserializeOwned + Send + 'static;
    type Result: Serialize + DeserializeOwned + Send + 'static;
}

/// A live connection, handed out by `connect` and meaningless to anyone else.
///
/// Opaque on purpose: a driver may key it on a pool slot, a socket, or a file
/// handle, and nothing above this layer should be able to write one down.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConnectionId(pub String);

macro_rules! method {
    ($name:ident, $wire:literal, $params:ty, $result:ty) => {
        pub struct $name;
        impl Method for $name {
            const NAME: &'static str = $wire;
            type Params = $params;
            type Result = $result;
        }
    };
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NoParams {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InitializeResult {
    /// The driver's own idea of which protocol it speaks. Compared against
    /// `PROTOCOL_VERSION` before anything else is asked of it.
    pub protocol_version: u32,
    pub driver_name: String,
    #[serde(default)]
    pub capabilities: Capabilities,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectParams {
    /// Everything but the secret. Drivers parse their own URLs -- a Postgres
    /// DSN and a SQLite path have nothing in common worth a shared parser.
    pub url: String,
    /// The password, fetched from the OS keychain by the caller. It is passed
    /// per connect rather than stored anywhere in this crate, so nothing here
    /// ever holds a secret longer than one call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectResult {
    pub connection_id: ConnectionId,
    /// Which schema the connection landed on, so a tree can open it without a
    /// second round trip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_schema: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionRef {
    pub connection_id: ConnectionId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Empty {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListSchemasResult {
    pub schemas: Vec<SchemaRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListTablesParams {
    pub connection_id: ConnectionId,
    pub schema: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListTablesResult {
    pub tables: Vec<TableRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DescribeTableParams {
    pub connection_id: ConnectionId,
    pub schema: String,
    pub table: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DescribeTableResult {
    pub columns: Vec<ColumnDef>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryParams {
    pub connection_id: ConnectionId,
    pub sql: String,
    /// Always set. A page is what crosses the wire -- never a whole table.
    pub limit: u32,
    pub offset: u64,
    /// Names this query so `cancel` has something to point at. Chosen by the
    /// caller rather than returned by the driver: a query that needs cancelling
    /// is usually one that has not answered anything yet.
    pub request_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CancelParams {
    pub connection_id: ConnectionId,
    pub request_id: String,
}

method!(Initialize, "initialize", NoParams, InitializeResult);
method!(Connect, "connect", ConnectParams, ConnectResult);
method!(Disconnect, "disconnect", ConnectionRef, Empty);
method!(
    ListSchemas,
    "list_schemas",
    ConnectionRef,
    ListSchemasResult
);
method!(
    ListTables,
    "list_tables",
    ListTablesParams,
    ListTablesResult
);
method!(
    DescribeTable,
    "describe_table",
    DescribeTableParams,
    DescribeTableResult
);
method!(Query, "query", QueryParams, ResultSet);
method!(Cancel, "cancel", CancelParams, Empty);

/// Every method name that crosses the wire.
///
/// Exists so a driver can be checked against the full set at build time rather
/// than discovering a missing arm on the first click that needs it.
pub const METHOD_NAMES: [&str; 8] = [
    Initialize::NAME,
    Connect::NAME,
    Disconnect::NAME,
    ListSchemas::NAME,
    ListTables::NAME,
    DescribeTable::NAME,
    Query::NAME,
    Cancel::NAME,
];
