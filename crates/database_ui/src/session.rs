use crate::connection_store::ConnectionConfig;
use crate::driver_registry::DriverHost;
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

/// What to say when a driver is simply not on the machine.
///
/// One sentence, in one place, because it is said from two: the capability
/// probe the dialog runs and the connection the tree opens. It names the
/// remedy for each of the two ways to be here -- an install that has not
/// downloaded this driver yet, and a checkout that has not built one.
pub fn not_installed(driver: &str) -> String {
    format!(
        "the `{driver}` driver is not installed -- download it from the connection dialog, \
         or in a development build run `script/build-database-drivers`"
    )
}

/// The same, for a driver that is not installed on a remote host.
pub fn not_installed_on(driver: &str, host: &str) -> String {
    format!(
        "the `{driver}` driver is not installed on {host} -- download it from the connection dialog"
    )
}

/// Starts `descriptor`'s driver on `host`.
///
/// On a remote host the driver runs through the command that reaches it,
/// never as a process of this machine, and the path is the host's own.
pub(crate) async fn start_driver(
    host: &DriverHost,
    descriptor: &DriverDescriptor,
    cx: &AsyncApp,
) -> Result<Arc<dyn Transport>> {
    let binary = descriptor
        .binary()
        .with_context(|| match host.host_name() {
            Some(host_name) => not_installed_on(&descriptor.id, &host_name),
            None => not_installed(&descriptor.id),
        })?;
    let transport = match host {
        DriverHost::Local => StdioTransport::new(binary, None, cx),
        DriverHost::Remote(remote) => {
            let command_host = cx.update(|cx| remote.command_host(cx)).await?;
            // A local command host would run the host's path on this machine.
            anyhow::ensure!(
                command_host.remote_name().is_some(),
                "the command host for {} does not reach it: it runs commands on this machine",
                remote.host_name()
            );
            let program = binary.executable.to_string_lossy();
            let args: Vec<&str> = binary.args.iter().map(String::as_str).collect();
            let env: Vec<(&str, &str)> = binary
                .env
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect();
            command_host
                .command(&program, &args, &env, None)
                .and_then(|command| StdioTransport::from_command(command, cx))
        }
    }
    // Names the path it actually tried. The driver being absent altogether is
    // a different sentence, said above -- conflating the two is what left
    // "never installed" looking like the driver's own error.
    .with_context(|| {
        format!(
            "could not start the `{}` driver at {}{}",
            descriptor.id,
            binary.executable.display(),
            host.host_name()
                .map(|host_name| format!(" on {host_name}"))
                .unwrap_or_default()
        )
    })?;
    Ok(Arc::new(transport))
}

/// Starts a driver, asks what it can do, and stops it again.
///
/// What the add-connection dialog uses to find out which fields to show. A
/// whole process for one question is not free, but it is the only way the UI
/// can stay unable to name an engine -- and it is a question asked once, when
/// someone is already typing.
pub fn driver_capabilities(
    host: DriverHost,
    descriptor: DriverDescriptor,
    cx: &App,
) -> Task<Result<database::protocol::Capabilities>> {
    cx.spawn(async move |cx: &mut AsyncApp| {
        let transport = start_driver(&host, &descriptor, cx).await?;
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
        host: DriverHost,
        descriptor: DriverDescriptor,
        config: ConnectionConfig,
        secret: Option<String>,
        cx: &AsyncApp,
    ) -> Result<Self> {
        let transport = start_driver(&host, &descriptor, cx).await?;
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
