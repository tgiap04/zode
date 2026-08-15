//! What the client must survive, exercised over the real encode/decode path.
//!
//! Every test here runs against [`FakeDriver`], so what is shortened is the
//! pipe -- not the protocol. A payload that would not survive JSON does not
//! survive these either.

use crate::client::{DEFAULT_REQUEST_TIMEOUT, DriverClient, DriverError};
use crate::fake_driver::{FakeBehaviour, FakeDriver};
use crate::protocol::{
    Cancel, CancelParams, Cell, Connect, ConnectParams, ConnectionId, ConnectionRef, DescribeTable,
    DescribeTableParams, Disconnect, ErrorCode, ListSchemas, ListTables, ListTablesParams, Query,
    QueryParams,
};
use gpui::TestAppContext;
use std::sync::Arc;
use std::time::Duration;

fn client(cx: &mut TestAppContext, behaviour: FakeBehaviour) -> DriverClient {
    let driver = Arc::new(FakeDriver::new(behaviour));
    cx.update(|cx| DriverClient::new(driver, DEFAULT_REQUEST_TIMEOUT, &cx.to_async()))
}

fn connection() -> ConnectionRef {
    ConnectionRef {
        connection_id: ConnectionId("fake-1".into()),
    }
}

/// All eight, over one client. A driver missing any of them is a driver whose
/// gap only shows up on the click that needs it.
#[gpui::test]
async fn every_method_makes_the_round_trip(cx: &mut TestAppContext) {
    let client = client(cx, FakeBehaviour::Well);

    let initialize = client.initialize().await.unwrap();
    assert_eq!(initialize.driver_name, "fake");

    let connected = client
        .request::<Connect>(ConnectParams {
            url: "fake://local".into(),
            secret: None,
        })
        .await
        .unwrap();
    assert_eq!(connected.default_schema.as_deref(), Some("public"));

    let schemas = client
        .request::<ListSchemas>(connection())
        .await
        .unwrap()
        .schemas;
    assert!(schemas.iter().any(|schema| schema.is_default));

    let tables = client
        .request::<ListTables>(ListTablesParams {
            connection_id: connected.connection_id.clone(),
            schema: "public".into(),
        })
        .await
        .unwrap()
        .tables;
    assert_eq!(tables.len(), 2, "a table and a view");

    let columns = client
        .request::<DescribeTable>(DescribeTableParams {
            connection_id: connected.connection_id.clone(),
            schema: "public".into(),
            table: "users".into(),
        })
        .await
        .unwrap()
        .columns;
    assert!(
        columns.iter().any(|column| column.primary_key),
        "a primary key must survive the wire, or the grid cannot identify a row"
    );

    client
        .request::<Query>(QueryParams {
            connection_id: connected.connection_id.clone(),
            sql: "select * from users".into(),
            limit: 200,
            offset: 0,
            request_id: "q1".into(),
        })
        .await
        .unwrap();

    client
        .request::<Cancel>(CancelParams {
            connection_id: connected.connection_id.clone(),
            request_id: "q1".into(),
        })
        .await
        .unwrap();

    client.request::<Disconnect>(connection()).await.unwrap();
}

/// The page is what crosses the wire. `truncated` has to be answerable without
/// a `COUNT(*)`, and the second page has to pick up where the first stopped.
#[gpui::test]
async fn paging_says_when_there_is_more(cx: &mut TestAppContext) {
    let client = client(cx, FakeBehaviour::Well);
    let page = async |offset: u64| {
        client
            .request::<Query>(QueryParams {
                connection_id: ConnectionId("fake-1".into()),
                sql: "select * from users".into(),
                limit: 2,
                offset,
                request_id: format!("q{offset}"),
            })
            .await
            .unwrap()
    };

    let first = page(0).await;
    assert_eq!(first.rows.len(), 2);
    assert!(first.truncated, "a third row exists, so say so");

    let second = page(2).await;
    assert_eq!(second.rows.len(), 1);
    assert!(!second.truncated, "and stop saying so at the end");
}

/// A null and an empty string are different answers. This is the path that
/// actually matters -- through JSON, not just through the type.
#[gpui::test]
async fn a_null_arrives_as_a_null(cx: &mut TestAppContext) {
    let client = client(cx, FakeBehaviour::Well);
    let result = client
        .request::<Query>(QueryParams {
            connection_id: ConnectionId("fake-1".into()),
            sql: "select * from users".into(),
            limit: 200,
            offset: 0,
            request_id: "q".into(),
        })
        .await
        .unwrap();

    let nulls = result
        .rows
        .iter()
        .filter(|row| row.get(1) == Some(&Cell::Null))
        .count();
    assert_eq!(
        nulls, 1,
        "the fake holds exactly one null, and it must arrive as one"
    );
}

/// Refused at the handshake rather than worked around: a driver speaking a
/// different shape fails later anyway, somewhere far less obvious.
#[gpui::test]
async fn a_driver_on_the_wrong_version_is_refused_with_both_numbers(cx: &mut TestAppContext) {
    let client = client(cx, FakeBehaviour::WrongVersion);
    let error = client
        .initialize()
        .await
        .expect_err("a version mismatch must not be waved through");
    let message = error.to_string();
    assert!(
        message.contains(&crate::PROTOCOL_VERSION.to_string())
            && message.contains(&crate::PROTOCOL_VERSION.wrapping_add(1).to_string()),
        "the message must name both versions, or nobody can tell which end to fix: {message}"
    );
}

/// The read-only rejection has to be distinguishable from a syntax error
/// without reading English -- the UI says different things for the two.
#[gpui::test]
async fn a_read_only_refusal_keeps_its_code(cx: &mut TestAppContext) {
    let client = client(cx, FakeBehaviour::ReadOnly);
    let error = client
        .request::<Query>(QueryParams {
            connection_id: ConnectionId("fake-1".into()),
            sql: "delete from users".into(),
            limit: 200,
            offset: 0,
            request_id: "q".into(),
        })
        .await
        .expect_err("a write must be refused");

    let driver_error = error
        .downcast_ref::<DriverError>()
        .expect("a driver's own refusal must survive as one, not collapse into a string");
    assert_eq!(driver_error.code(), ErrorCode::ReadOnly);
}

/// A driver that accepts requests and answers none must not hold the caller
/// forever. This is the backstop under `cancel`, not a replacement for it.
#[gpui::test]
async fn a_silent_driver_times_out_rather_than_hanging(cx: &mut TestAppContext) {
    let driver = Arc::new(FakeDriver::new(FakeBehaviour::Silent));
    let timeout = Duration::from_millis(50);
    let client = cx.update(|cx| DriverClient::new(driver, timeout, &cx.to_async()));

    let error = client
        .initialize()
        .await
        .expect_err("a silent driver must not be waited on forever");
    assert!(
        error.to_string().contains("did not answer"),
        "and the reason must say so plainly: {error}"
    );
}

/// Dropping the transport is how a closed column releases its driver. Whatever
/// was still in flight has to fail rather than wait out the full timeout.
#[gpui::test]
async fn callers_are_woken_when_the_driver_goes_away(cx: &mut TestAppContext) {
    let client = client(cx, FakeBehaviour::Dies);
    let error = client
        .initialize()
        .await
        .expect_err("a driver that has gone must not leave callers waiting on it");
    assert!(
        error.to_string().contains("stopped before answering"),
        "and must say the driver stopped, not that it was slow: {error}"
    );
}

/// The tree loads a level at a time. If `list_tables` were fetched eagerly,
/// opening a Postgres with a few thousand tables would stall on the first click.
#[gpui::test]
async fn the_client_asks_for_exactly_what_it_was_told_to(cx: &mut TestAppContext) {
    let driver = Arc::new(FakeDriver::new(FakeBehaviour::Well));
    let calls = driver.calls.clone();
    let client = cx.update(|cx| DriverClient::new(driver, DEFAULT_REQUEST_TIMEOUT, &cx.to_async()));

    client.initialize().await.unwrap();
    client.request::<ListSchemas>(connection()).await.unwrap();

    assert_eq!(
        calls.lock().as_slice(),
        ["initialize", "list_schemas"],
        "the client must make no call of its own accord"
    );
}
