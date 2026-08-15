//! Opening connections and walking their schemas.
//!
//! Split from `database_panel.rs` to keep either side readable: this half is
//! all driver round trips and the state they land in, the other is the panel's
//! own shape and its `Panel` contract.

use crate::connection_store::read_secret;
use crate::database_panel::{DatabasePanel, NodeState, OpenTable, SchemaNode};
use crate::session::Session;
use database::client::DriverError;
use database::protocol::ErrorCode;
use gpui::{Context, Window};
use std::sync::Arc;

/// What opening a connection came to, decided while the node is borrowed and
/// acted on once it is not.
enum Outcome {
    Connected,
    /// The driver said the credentials were rejected, which is the one failure
    /// the user can answer here rather than in settings.
    NeedsPassword,
    Failed,
}

impl Outcome {
    /// Which failure the user can answer here, and which one they cannot.
    ///
    /// Only a rejection of the credentials themselves: an unreachable host or a
    /// driver that will not start is answered by editing settings, and putting
    /// a password box in front of either would be asking for the wrong thing.
    fn for_error(error: &anyhow::Error) -> Self {
        match error.downcast_ref::<DriverError>().map(DriverError::code) {
            Some(ErrorCode::Authentication) => Outcome::NeedsPassword,
            _ => Outcome::Failed,
        }
    }
}

impl DatabasePanel {
    /// Opens a connection, or collapses it if it is already open.
    pub(crate) fn toggle_connection(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self.connections.get(index) else {
            return;
        };
        match &node.state {
            NodeState::Connecting => return,
            NodeState::Connected(_) => {
                if let Some(node) = self.connections.get_mut(index) {
                    node.expanded = !node.expanded;
                }
                cx.notify();
                return;
            }
            NodeState::Idle | NodeState::Failed(_) => {}
        }
        let config = node.config.clone();

        // Looked up before the node is borrowed again: the registry is a
        // separate entity, and reading it needs the same `cx` the node is
        // reached through.
        let Some(descriptor) = self.registry.read(cx).get(&config.driver).cloned() else {
            if let Some(node) = self.connections.get_mut(index) {
                node.state = NodeState::Failed(
                    format!(
                        "no database driver called `{}` -- is the extension that provides it installed?",
                        config.driver
                    )
                    .into(),
                );
            }
            cx.notify();
            return;
        };

        if let Some(node) = self.connections.get_mut(index) {
            node.state = NodeState::Connecting;
        }
        let credentials = zed_credentials_provider::global(cx);
        cx.notify();

        let secret = read_secret(credentials, config.credential_key().to_string(), cx);
        self.tasks.push(cx.spawn_in(window, async move |this, cx| {
            let opened = async {
                let secret = secret.await?;
                Session::open(descriptor, config, secret, cx).await
            }
            .await;

            this.update_in(cx, |this, window, cx| {
                let outcome = {
                    let Some(node) = this.connections.get_mut(index) else {
                        return;
                    };
                    match opened {
                        Ok(session) => {
                            node.collapse_schema_level = false;
                            node.state = NodeState::Connected(Arc::new(session));
                            node.expanded = true;
                            Outcome::Connected
                        }
                        Err(error) => {
                            node.state = NodeState::Failed(format!("{error:#}").into());
                            Outcome::for_error(&error)
                        }
                    }
                };
                // Outside the borrow above: both of these need the panel, and
                // holding a node out of it across either is a second mutable
                // borrow of the same thing.
                match outcome {
                    Outcome::Connected => this.load_schemas(index, cx),
                    Outcome::NeedsPassword => this.prompt_for_password(index, window, cx),
                    Outcome::Failed => {}
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Re-opens a connection from scratch, throwing away whatever it had.
    ///
    /// What the password prompt calls once the keychain has the new secret: the
    /// failed session is of no use, and the point of having typed a password is
    /// to try again with it.
    pub(crate) fn reconnect(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(node) = self.connections.get_mut(index) {
            node.state = NodeState::Idle;
            node.schemas.clear();
            node.expanded = false;
        }
        self.toggle_connection(index, window, cx);
    }

    fn load_schemas(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(NodeState::Connected(session)) =
            self.connections.get(index).map(|node| &node.state)
        else {
            return;
        };
        let session = session.clone();

        self.tasks.push(cx.spawn(async move |this, cx| {
            let schemas = session.schemas().await;
            this.update(cx, |this, cx| {
                let opened_schemas: Vec<usize> = {
                    let Some(node) = this.connections.get_mut(index) else {
                        return;
                    };
                    match schemas {
                        Ok(schemas) => {
                            // One schema, from a driver that says it will never
                            // have more: draw its tables directly rather than
                            // spending a level on a node nobody would collapse.
                            let collapse = schemas.len() == 1;
                            node.collapse_schema_level = collapse;
                            node.schemas = schemas
                                .into_iter()
                                .map(|schema| SchemaNode {
                                    expanded: schema.is_default || collapse,
                                    schema,
                                    tables: None,
                                })
                                .collect();
                            node.schemas
                                .iter()
                                .enumerate()
                                .filter(|(_, schema)| schema.expanded)
                                .map(|(schema_index, _)| schema_index)
                                .collect()
                        }
                        Err(error) => {
                            node.state = NodeState::Failed(format!("{error:#}").into());
                            Vec::new()
                        }
                    }
                };
                // Outside the borrow: see `toggle_connection`.
                for schema_index in opened_schemas {
                    this.load_tables(index, schema_index, cx);
                }
                cx.notify();
            })
            .ok();
        }));
    }

    pub(crate) fn toggle_schema(
        &mut self,
        index: usize,
        schema_index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self.connections.get_mut(index) else {
            return;
        };
        let Some(schema) = node.schemas.get_mut(schema_index) else {
            return;
        };
        schema.expanded = !schema.expanded;
        let needs_tables = schema.expanded && schema.tables.is_none();
        cx.notify();
        if needs_tables {
            self.load_tables(index, schema_index, cx);
        }
    }

    /// Shows a table's columns, or hides them if they are already showing.
    ///
    /// The columns are what tells you whether this is the table you meant --
    /// names alone rarely do.
    pub(crate) fn toggle_table(
        &mut self,
        index: usize,
        schema: &str,
        table: &str,
        cx: &mut Context<Self>,
    ) {
        if self.open_table.as_ref().is_some_and(|open| {
            open.connection == index && open.schema == schema && open.table == table
        }) {
            self.open_table = None;
            cx.notify();
            return;
        }

        self.open_table = Some(OpenTable {
            connection: index,
            schema: schema.to_string(),
            table: table.to_string(),
            columns: None,
        });
        cx.notify();

        let Some(node) = self.connections.get(index) else {
            return;
        };
        let NodeState::Connected(session) = &node.state else {
            return;
        };
        let (session, schema, table) = (session.clone(), schema.to_string(), table.to_string());

        self.tasks.push(cx.spawn(async move |this, cx| {
            let described = session.describe(&schema, &table).await;
            this.update(cx, |this, cx| {
                // Checked against what is open *now*: two quick clicks would
                // otherwise land the first table's columns under the second.
                let Some(open) = this.open_table.as_mut() else {
                    return;
                };
                if open.connection != index || open.schema != schema || open.table != table {
                    return;
                }
                match described {
                    Ok(columns) => open.columns = Some(columns),
                    Err(error) => {
                        this.open_table = None;
                        if let Some(node) = this.connections.get_mut(index) {
                            node.state = NodeState::Failed(format!("{error:#}").into());
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    pub(crate) fn load_tables(
        &mut self,
        index: usize,
        schema_index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self.connections.get(index) else {
            return;
        };
        let NodeState::Connected(session) = &node.state else {
            return;
        };
        let Some(schema) = node.schemas.get(schema_index) else {
            return;
        };
        let (session, name) = (session.clone(), schema.schema.name.clone());

        self.tasks.push(cx.spawn(async move |this, cx| {
            let tables = session.tables(&name).await;
            this.update(cx, |this, cx| {
                let Some(node) = this.connections.get_mut(index) else {
                    return;
                };
                match tables {
                    Ok(tables) => {
                        if let Some(schema) = node.schemas.get_mut(schema_index) {
                            schema.tables = Some(tables);
                        }
                    }
                    Err(error) => node.state = NodeState::Failed(format!("{error:#}").into()),
                }
                cx.notify();
            })
            .ok();
        }));
    }
}

impl DatabasePanel {
    /// Closes a connection and stops its driver.
    ///
    /// Dropping the node's `Arc<Session>` is the whole mechanism: the session
    /// owns the client, the client owns the transport, and the transport kills
    /// the child process. There is nothing to send -- a driver told to go away
    /// over a pipe that is about to close is a message nobody needs.
    ///
    /// The schemas go with it. Keeping a tree drawn from a connection that is
    /// no longer open would be showing a database that may since have changed.
    pub(crate) fn disconnect(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(node) = self.connections.get_mut(index) else {
            return;
        };
        if !matches!(node.state, NodeState::Connected(_)) {
            return;
        }
        node.state = NodeState::Idle;
        node.expanded = false;
        node.schemas.clear();
        node.collapse_schema_level = false;
        self.forget_connection_state(index);
        cx.notify();
    }

    /// Drops whatever one connection was showing, and only that one.
    ///
    /// A grid of rows from a session that no longer exists cannot be paged or
    /// re-run, and the index test is the part worth being careful with: clearing
    /// on the wrong comparison empties a connection nobody touched.
    pub(crate) fn forget_connection_state(&mut self, index: usize) {
        if self
            .open_table
            .as_ref()
            .is_some_and(|open| open.connection == index)
        {
            self.open_table = None;
        }
        if self.active == Some(index) {
            self.active = None;
            self.query = crate::query::QueryState::Idle;
        }
    }

    /// Whether a connection is open, which is what decides the menu entry and
    /// the row's own button.
    pub(crate) fn is_connected(&self, index: usize) -> bool {
        self.connections
            .get(index)
            .is_some_and(|node| matches!(node.state, NodeState::Connected(_)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::protocol::ResponseError;

    fn driver_error(code: ErrorCode) -> anyhow::Error {
        anyhow::Error::new(DriverError(ResponseError::new(code, "refused")))
            // Wrapped, because this is how the error actually arrives: `open`
            // adds context on the way out, and `downcast_ref` has to see
            // through that.
            .context("connecting to `staging`")
    }

    /// The protocol keeps `authentication` apart from every other failure for
    /// exactly this: it is the one the user can answer without leaving the
    /// panel.
    #[test]
    fn only_rejected_credentials_open_the_password_box() {
        assert!(matches!(
            Outcome::for_error(&driver_error(ErrorCode::Authentication)),
            Outcome::NeedsPassword
        ));

        for code in [
            ErrorCode::Connection,
            ErrorCode::Syntax,
            ErrorCode::ReadOnly,
            ErrorCode::Internal,
        ] {
            assert!(
                matches!(Outcome::for_error(&driver_error(code)), Outcome::Failed),
                "{code:?} is not something a password fixes"
            );
        }
    }

    /// A driver that never started, or one that died, produces a plain error
    /// with no code at all. Asking for a password over that would be a guess.
    #[test]
    fn a_failure_with_no_driver_code_asks_for_nothing() {
        assert!(matches!(
            Outcome::for_error(&anyhow::anyhow!("could not start the driver")),
            Outcome::Failed
        ));
    }
}
