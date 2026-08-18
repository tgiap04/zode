use crate::connection_store::ConnectionConfig;
use anyhow::{Context as _, Result};
use database::client::{DEFAULT_REQUEST_TIMEOUT, DriverClient};
use database::protocol::{
    Cancel, CancelParams, Capabilities, Connect, ConnectParams, ConnectionId, ConnectionRef,
    DescribeTable, DescribeTableParams, ListSchemas, ListTables, ListTablesParams, Query,
    QueryParams, ResultSet, SchemaRef, TableRef,
};
use database::registry::DriverDescriptor;
use database::transport::{StdioTransport, Transport};
use gpui::{App, AsyncApp, Task};
use std::sync::Arc;

/// A driver process with one connection open on it.
///
/// Dropping this drops the `DriverClient`, which drops the transport, which
/// kills the child process -- so closing a connection never leaves a driver
/// holding a database handle nobody is watching.
pub struct Session {
    client: Arc<DriverClient>,
    connection_id: ConnectionId,
    /// What the driver calls itself. Shown on the connection's tooltip, which
    /// is the only place someone can check that the driver named in settings is
    /// the one that answered.
    pub driver_name: String,
    /// What the driver said it can do. Read for `quote_identifier`, which is
    /// how an identifier from the tree becomes SQL an engine will accept --
    /// engines disagree about the quote character, and this crate must not know
    /// which is which.
    pub capabilities: Capabilities,
}

/// Starts a driver, asks what it can do, and stops it again.
///
/// What the add-connection dialog uses to find out which fields to show. A
/// whole process for one question is not free, but it is the only way the UI
/// can stay unable to name an engine -- and it is a question asked once, when
/// someone is already typing.
pub fn driver_capabilities(
    descriptor: DriverDescriptor,
    cx: &App,
) -> Task<Result<database::protocol::Capabilities>> {
    cx.spawn(async move |cx: &mut AsyncApp| {
        let transport: Arc<dyn Transport> = Arc::new(
            StdioTransport::new(&descriptor.binary, None, cx).with_context(|| {
                format!(
                    "could not start the `{}` driver at {} \
                     (in a development build, run `script/build-database-drivers` first)",
                    descriptor.id,
                    descriptor.binary.executable.display()
                )
            })?,
        );
        // Dropped at the end of this scope, which kills the process: the driver
        // has answered the only thing it was started for.
        let client = DriverClient::new(transport, DEFAULT_REQUEST_TIMEOUT, cx);
        Ok(client.initialize().await?.capabilities)
    })
}

impl Session {
    /// Starts a driver and connects it.
    ///
    /// Both halves in one call because a driver process with no connection on
    /// it is of no use to anyone, and leaving that state reachable means every
    /// caller downstream has to handle it.
    pub async fn open(
        descriptor: DriverDescriptor,
        config: ConnectionConfig,
        secret: Option<String>,
        cx: &AsyncApp,
    ) -> Result<Self> {
        let transport: Arc<dyn Transport> = Arc::new(
            StdioTransport::new(&descriptor.binary, None, cx).with_context(|| {
                // Names the path, and says the one thing that fixes it in a
                // development build: the drivers are separate binaries that
                // `cargo run` does not build, so a fresh checkout has none and
                // every connection fails here with nothing to go on.
                format!(
                    "could not start the `{}` driver at {} \
                     (in a development build, run `script/build-database-drivers` first)",
                    config.driver,
                    descriptor.binary.executable.display()
                )
            })?,
        );
        let client = Arc::new(DriverClient::new(transport, DEFAULT_REQUEST_TIMEOUT, cx));

        // Before anything else is asked of it: a driver speaking a different
        // shape fails later anyway, somewhere far less obvious than here.
        let initialize = client.initialize().await?;

        let connected = client
            .request::<Connect>(ConnectParams {
                url: config.url.clone(),
                secret,
            })
            .await
            .with_context(|| format!("connecting to `{}`", config.name))?;

        Ok(Self {
            client,
            connection_id: connected.connection_id,
            driver_name: initialize.driver_name,
            capabilities: initialize.capabilities,
        })
    }

    fn connection(&self) -> ConnectionRef {
        ConnectionRef {
            connection_id: self.connection_id.clone(),
        }
    }

    /// Runs one page of a statement.
    ///
    /// `request_id` is chosen by the caller so [`Self::cancel`] has something to
    /// name -- a query worth cancelling is usually one that has not answered
    /// anything yet, so the driver cannot be the one to hand out the id.
    pub async fn query(
        &self,
        sql: String,
        limit: u32,
        offset: u64,
        request_id: String,
    ) -> Result<ResultSet> {
        self.client
            .request::<Query>(QueryParams {
                connection_id: self.connection_id.clone(),
                sql,
                limit,
                offset,
                request_id,
            })
            .await
    }

    /// Asks the driver to interrupt a query.
    ///
    /// Best effort by design: a driver whose engine cannot interrupt says so in
    /// its capabilities, and one whose query has already finished ignores this.
    pub async fn cancel(&self, request_id: String) -> Result<()> {
        self.client
            .request::<Cancel>(CancelParams {
                connection_id: self.connection_id.clone(),
                request_id,
            })
            .await?;
        Ok(())
    }

    pub async fn schemas(&self) -> Result<Vec<SchemaRef>> {
        Ok(self
            .client
            .request::<ListSchemas>(self.connection())
            .await?
            .schemas)
    }

    pub async fn tables(&self, schema: &str) -> Result<Vec<TableRef>> {
        Ok(self
            .client
            .request::<ListTables>(ListTablesParams {
                connection_id: self.connection_id.clone(),
                schema: schema.to_string(),
            })
            .await?
            .tables)
    }

    pub async fn describe(
        &self,
        schema: &str,
        table: &str,
    ) -> Result<Vec<database::protocol::ColumnDef>> {
        Ok(self
            .client
            .request::<DescribeTable>(DescribeTableParams {
                connection_id: self.connection_id.clone(),
                schema: schema.to_string(),
                table: table.to_string(),
            })
            .await?
            .columns)
    }
}
