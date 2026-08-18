//! A driver that never leaves the process.
//!
//! Exists so the client, and later the panel, can be tested without a binary on
//! disk or a database anywhere. It answers the same eight methods over the same
//! `Transport`, so what it exercises is the real encode/decode path -- only the
//! pipe is shortened.

use crate::protocol::{Capabilities, DescribeTableResult, Empty, PROTOCOL_VERSION};
use crate::protocol::{
    Cell, ColumnDef, ConnectResult, ConnectionId, ErrorCode, InitializeResult, ListSchemasResult,
    ListTablesResult, Request, Response, ResponseError, ResultColumn, ResultSet, SchemaRef,
    TableKind, TableRef,
};
use crate::transport::Transport;
use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use parking_lot::Mutex;
use serde_json::json;
use smol::channel;
use std::pin::Pin;
use std::sync::Arc;

/// How the fake should misbehave, so the unhappy paths get tested too.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FakeBehaviour {
    #[default]
    Well,
    /// Answers `initialize` with a version this build cannot speak.
    WrongVersion,
    /// Accepts every request and answers none, to exercise the timeout.
    Silent,
    /// Closes its answer pipe on the first request, the way a child process
    /// that crashes mid-call does.
    Dies,
    /// Refuses writes the way a read-only connection does.
    ReadOnly,
}

pub struct FakeDriver {
    inbound: channel::Receiver<String>,
    outbound: channel::Sender<String>,
    behaviour: FakeBehaviour,
    /// Every method the fake was asked for, in order, so a test can assert that
    /// a lazily-loading tree really did stop at one level.
    pub calls: Arc<Mutex<Vec<String>>>,
}

impl FakeDriver {
    pub fn new(behaviour: FakeBehaviour) -> Self {
        let (outbound, inbound) = channel::unbounded();
        Self {
            inbound,
            outbound,
            behaviour,
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn answer(&self, request: &Request) -> Response {
        let id = request.id.clone();
        let read_only_write = matches!(self.behaviour, FakeBehaviour::ReadOnly)
            && request.method == "query"
            && request
                .params
                .get("sql")
                .and_then(|sql| sql.as_str())
                .is_some_and(is_write_statement);
        if read_only_write {
            return Response::err(
                id,
                ResponseError::new(ErrorCode::ReadOnly, "this connection is read-only"),
            );
        }

        let result = match request.method.as_str() {
            "initialize" => json!(InitializeResult {
                protocol_version: if self.behaviour == FakeBehaviour::WrongVersion {
                    PROTOCOL_VERSION.wrapping_add(1)
                } else {
                    PROTOCOL_VERSION
                },
                driver_name: "fake".into(),
                capabilities: Capabilities {
                    multiple_schemas: true,
                    cancellation: true,
                    ..Default::default()
                },
            }),
            "connect" => json!(ConnectResult {
                connection_id: ConnectionId("fake-1".into()),
                default_schema: Some("public".into()),
            }),
            "disconnect" | "cancel" => json!(Empty {}),
            "list_schemas" => json!(ListSchemasResult {
                schemas: vec![SchemaRef {
                    name: "public".into(),
                    is_default: true,
                }],
            }),
            "list_tables" => json!(ListTablesResult {
                tables: vec![
                    TableRef {
                        name: "users".into(),
                        kind: TableKind::Table,
                    },
                    TableRef {
                        name: "active_users".into(),
                        kind: TableKind::View,
                    },
                ],
            }),
            "describe_table" => json!(DescribeTableResult {
                columns: vec![
                    ColumnDef {
                        name: "id".into(),
                        type_name: "integer".into(),
                        nullable: false,
                        primary_key: true,
                    },
                    ColumnDef {
                        name: "email".into(),
                        type_name: "text".into(),
                        nullable: true,
                        primary_key: false,
                    },
                ],
            }),
            "query" => json!(fake_result_set(request)),
            unknown => {
                return Response::err(
                    id,
                    ResponseError::new(
                        ErrorCode::Unsupported,
                        format!("the fake driver has no `{unknown}`"),
                    ),
                );
            }
        };
        Response::ok(id, result)
    }
}

fn is_write_statement(sql: &str) -> bool {
    let head = sql.trim_start().split_whitespace().next().unwrap_or("");
    [
        "insert", "update", "delete", "drop", "create", "alter", "truncate",
    ]
    .contains(&head.to_ascii_lowercase().as_str())
}

/// Two rows and a null, paged the way a real driver pages: the fake owns three
/// rows in total, so a `limit` of 2 is enough to see `truncated` work.
fn fake_result_set(request: &Request) -> ResultSet {
    const TOTAL: u64 = 3;
    let limit = request
        .params
        .get("limit")
        .and_then(|limit| limit.as_u64())
        .unwrap_or(TOTAL);
    let offset = request
        .params
        .get("offset")
        .and_then(|offset| offset.as_u64())
        .unwrap_or(0);

    let rows = (offset..TOTAL.min(offset.saturating_add(limit)))
        .map(|row| {
            vec![
                Cell::Number {
                    value: (row + 1).to_string(),
                },
                if row == 1 {
                    Cell::Null
                } else {
                    Cell::Text {
                        value: format!("user{row}@example.com"),
                    }
                },
            ]
        })
        .collect::<Vec<_>>();

    ResultSet {
        columns: vec![
            ResultColumn {
                name: "id".into(),
                type_name: "integer".into(),
            },
            ResultColumn {
                name: "email".into(),
                type_name: "text".into(),
            },
        ],
        truncated: offset.saturating_add(rows.len() as u64) < TOTAL,
        rows,
        elapsed_ms: 0,
    }
}

#[async_trait]
impl Transport for FakeDriver {
    async fn send(&self, message: String) -> Result<()> {
        let request: Request = serde_json::from_str(&message)?;
        self.calls.lock().push(request.method.clone());
        if self.behaviour == FakeBehaviour::Silent {
            return Ok(());
        }
        if self.behaviour == FakeBehaviour::Dies {
            // Ends the receive stream, which is all a caller ever sees of a
            // child process exiting.
            self.outbound.close();
            return Ok(());
        }
        let response = serde_json::to_string(&self.answer(&request))?;
        self.outbound.send(response).await?;
        Ok(())
    }

    fn receive(&self) -> Pin<Box<dyn Stream<Item = String> + Send>> {
        Box::pin(self.inbound.clone())
    }

    fn receive_err(&self) -> Pin<Box<dyn Stream<Item = String> + Send>> {
        let (_sender, receiver) = channel::unbounded();
        Box::pin(receiver)
    }
}
