//! The relay connection against a scripted host, through the same pool the
//! app connects with: what a window gets from it, and what ends when.

use std::{collections::HashMap, sync::Arc};

use futures::{StreamExt as _, channel::mpsc};
use gpui::{AppContext as _, Entity, TestAppContext};
use http_client::{AsyncBody, FakeHttpClient, HttpClient, Response};
use remote_relay_client::{
    PairingStep, RelayEnvironment, RelayInitiator, encode_public_key,
    scripted_host::ScriptedHost,
    test_support::{FakeRelay, InMemoryKeychain, StaticCredentials},
};
use remote_relay_protocol::{Control, DeviceKeypair};
use rpc::proto::Envelope;

use super::{RelayConnectionOptions, RelayRemoteConnection};
use crate::{
    MockDelegate, RemoteConnection, RemoteConnectionOptions,
    proxy::ProxyLaunchError,
    transport::relay_stream::{MessageDecoder, encode_message},
};

const USER: &str = "user";
const CLIENT_DEVICE: &str = "client-device";
const HOST_DEVICE: &str = "host-device";

struct Rig {
    relay: FakeRelay,
    host: Entity<ScriptedHost>,
    /// Held because the app holds the initiator it hands out only weakly.
    _initiator: Entity<RelayInitiator>,
}

struct Proxy {
    outgoing: mpsc::UnboundedSender<Envelope>,
    incoming: mpsc::UnboundedReceiver<Envelope>,
    io: gpui::Task<anyhow::Result<i32>>,
}

fn directory(host_key: [u8; 32]) -> Arc<dyn HttpClient> {
    let devices = serde_json::json!([{
        "deviceId": HOST_DEVICE,
        "kind": "ide",
        "name": "Studio",
        "publicKey": encode_public_key(&host_key),
    }])
    .to_string();
    FakeHttpClient::create(move |request| {
        let body = if request.method() == "PUT" {
            "{}".to_string()
        } else {
            devices.clone()
        };
        async move {
            Ok(Response::builder()
                .status(200)
                .body(AsyncBody::from(body))
                .expect("a response"))
        }
    })
}

fn options() -> RemoteConnectionOptions {
    RemoteConnectionOptions::Relay(relay_options())
}

fn relay_options() -> RelayConnectionOptions {
    RelayConnectionOptions {
        host_device_id: HOST_DEVICE.to_string(),
        host_name: "Studio".to_string(),
    }
}

/// A host that serves project streams, an initiator paired with it, and the
/// initiator registered as the one the app uses.
async fn rig(cx: &mut TestAppContext) -> Rig {
    cx.update(|cx| {
        cx.set_global(db::AppDatabase::test_new());
        release_channel::init_test(
            "9.9.9".parse().expect("a version"),
            release_channel::ReleaseChannel::Dev,
            cx,
        );
    });
    let relay = FakeRelay::new();
    let host_keypair = Arc::new(DeviceKeypair::generate().expect("a key"));
    let host_key = *host_keypair.public_key();
    let host = cx.new(|cx| ScriptedHost::new(relay.transport(), USER, host_keypair, cx));
    host.update(cx, |host, _| {
        host.respond_with(|control| match control {
            Control::IdeOpen {
                request_id,
                stream_id,
                ..
            } => vec![Control::IdeOpened {
                request_id: *request_id,
                stream_id: *stream_id,
                path_style: Some("posix".into()),
                shell: None,
                default_shell: None,
            }],
            _ => Vec::new(),
        });
    });
    let environment = RelayEnvironment {
        credentials: Arc::new(StaticCredentials::new(USER, CLIENT_DEVICE, "client-token")),
        keychain: InMemoryKeychain::new(),
        http_client: directory(host_key),
        api_url: "https://api.test/api".to_string(),
        device_name: "Laptop".to_string(),
    };
    let transport = relay.client_transport(CLIENT_DEVICE);
    let initiator = cx.new(|cx| RelayInitiator::new(environment, transport, "9.9.9".into(), cx));
    cx.update(|cx| RelayInitiator::register_global(&initiator, cx));
    cx.run_until_parked();

    let listed = initiator
        .update(cx, |initiator, cx| initiator.hosts(cx))
        .await
        .expect("the host list");
    let entry = listed
        .into_iter()
        .find(|entry| entry.device_id == HOST_DEVICE)
        .expect("the host is listed");
    let attempt = initiator.update(cx, |initiator, cx| initiator.pair(entry, cx));
    cx.run_until_parked();
    attempt.update(cx, |attempt, cx| attempt.confirm_digits(cx));
    host.update(cx, |host, cx| host.decide_pairing(true, cx));
    cx.run_until_parked();
    assert_eq!(
        attempt.read_with(cx, |attempt, _| attempt.step().clone()),
        PairingStep::Done
    );
    Rig {
        relay,
        host,
        _initiator: initiator,
    }
}

async fn connect(cx: &mut TestAppContext) -> Arc<dyn RemoteConnection> {
    crate::connect(options(), Arc::new(MockDelegate), &mut cx.to_async())
        .await
        .expect("a connection")
}

fn envelope(id: u32) -> Envelope {
    Envelope {
        id,
        ..Default::default()
    }
}

fn start_proxy(
    connection: &Arc<dyn RemoteConnection>,
    reconnect: bool,
    cx: &mut TestAppContext,
) -> Proxy {
    let (incoming_tx, incoming) = mpsc::unbounded();
    let (outgoing, outgoing_rx) = mpsc::unbounded();
    let (activity_tx, _activity_rx) = mpsc::channel(1);
    let io = connection.start_proxy(
        "workspace-1".into(),
        reconnect,
        incoming_tx,
        outgoing_rx,
        activity_tx,
        Arc::new(MockDelegate),
        &mut cx.to_async(),
    );
    Proxy {
        outgoing,
        incoming,
        io,
    }
}

impl Rig {
    /// The envelope ids the host has read on each of its sessions' streams.
    fn ids_read(&self, cx: &TestAppContext) -> HashMap<(u32, u32), Vec<u32>> {
        let mut decoders: HashMap<(u32, u32), MessageDecoder> = HashMap::new();
        let mut ids: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
        for (session_id, stream_id, bytes) in self
            .host
            .read_with(cx, |host, _| host.received_data().to_vec())
        {
            let decoder = decoders.entry((session_id, stream_id)).or_default();
            for message in decoder.push(&bytes).expect("a message") {
                ids.entry((session_id, stream_id))
                    .or_default()
                    .push(message.id);
            }
        }
        ids
    }

    /// The host answers on `session_id`'s `stream_id` with an envelope.
    fn answer(&self, session_id: u32, stream_id: u32, id: u32, cx: &mut TestAppContext) {
        let bytes = smol::block_on(encode_message(envelope(id))).expect("encoded");
        self.host.update(cx, |host, cx| {
            host.send_data(session_id, stream_id, &bytes, cx)
        });
    }

    fn sessions(&self, cx: &TestAppContext) -> usize {
        self.host.read_with(cx, |host, _| host.session_ids().len())
    }

    fn ended(&self, cx: &TestAppContext) -> Vec<u32> {
        self.host
            .read_with(cx, |host, _| host.ended_sessions().to_vec())
    }
}

#[gpui::test]
async fn two_windows_on_one_host_each_get_a_project_stream_of_their_own(cx: &mut TestAppContext) {
    let rig = rig(cx).await;
    let first_window = connect(cx).await;
    let second_window = connect(cx).await;
    assert_eq!(
        Arc::as_ptr(&first_window) as *const (),
        Arc::as_ptr(&second_window) as *const (),
        "the pool shares one connection between windows on one host"
    );
    assert_eq!(
        rig.sessions(cx),
        1,
        "nothing is opened before a window asks"
    );

    let mut first = start_proxy(&first_window, false, cx);
    let mut second = start_proxy(&second_window, false, cx);
    first.outgoing.unbounded_send(envelope(41)).expect("sent");
    second.outgoing.unbounded_send(envelope(42)).expect("sent");
    cx.run_until_parked();

    assert_eq!(rig.sessions(cx), 2, "each window has a session of its own");
    let read = rig.ids_read(cx);
    assert_eq!(read.len(), 2, "{read:?}");
    for ((session_id, stream_id), ids) in &read {
        assert_eq!(ids.len(), 1, "each window's messages stay on its stream");
        rig.answer(*session_id, *stream_id, ids[0] + 100, cx);
    }
    cx.run_until_parked();
    assert_eq!(first.incoming.next().await.map(|reply| reply.id), Some(141));
    assert_eq!(
        second.incoming.next().await.map(|reply| reply.id),
        Some(142)
    );
    drop((first.io, second.io));
}

#[gpui::test]
async fn closing_one_window_ends_its_session_and_leaves_the_other_working(cx: &mut TestAppContext) {
    let rig = rig(cx).await;
    let connection = connect(cx).await;
    let first = start_proxy(&connection, false, cx);
    let mut second = start_proxy(&connection, false, cx);
    second.outgoing.unbounded_send(envelope(7)).expect("sent");
    first.outgoing.unbounded_send(envelope(1)).expect("sent");
    cx.run_until_parked();
    let read = rig.ids_read(cx);
    let ((first_session, _), _) = read
        .iter()
        .find(|(_, ids)| ids.contains(&1))
        .expect("the first window's stream");
    let ((second_session, second_stream), _) = read
        .iter()
        .find(|(_, ids)| ids.contains(&7))
        .expect("the second window's stream");

    drop(first.io);
    cx.run_until_parked();
    assert!(
        rig.ended(cx).contains(first_session),
        "the closed window's session stayed open, and its project server with it"
    );
    assert!(!rig.ended(cx).contains(second_session));
    assert!(
        !connection.has_been_killed(),
        "the other window still uses the connection"
    );

    rig.answer(*second_session, *second_stream, 8, cx);
    cx.run_until_parked();
    assert_eq!(second.incoming.next().await.map(|reply| reply.id), Some(8));
}

#[gpui::test]
async fn killing_a_connection_that_windows_still_use_ends_nothing(cx: &mut TestAppContext) {
    let rig = rig(cx).await;
    let connection = connect(cx).await;
    let _first = start_proxy(&connection, false, cx);
    let _second = start_proxy(&connection, false, cx);
    cx.run_until_parked();
    let sessions = rig.host.read_with(cx, |host, _| host.session_ids());
    assert_eq!(sessions.len(), 2);

    connection.kill().await.expect("killed");
    cx.run_until_parked();
    assert!(!connection.has_been_killed());
    assert_eq!(rig.sessions(cx), 2);
    let ended = rig.ended(cx);
    assert!(
        !sessions.iter().any(|session_id| ended.contains(session_id)),
        "a window's project server was taken down by someone else's kill: {ended:?}"
    );
}

#[gpui::test]
async fn a_window_that_reconnects_leaves_the_other_windows_session_alone(cx: &mut TestAppContext) {
    let rig = rig(cx).await;
    let connection = connect(cx).await;
    let first = start_proxy(&connection, false, cx);
    let mut second = start_proxy(&connection, false, cx);
    second.outgoing.unbounded_send(envelope(7)).expect("sent");
    cx.run_until_parked();
    let read = rig.ids_read(cx);
    let (second_session, second_stream) = *read
        .iter()
        .find(|(_, ids)| ids.contains(&7))
        .map(|(key, _)| key)
        .expect("the second window's stream");

    // What the client of the first window does to reconnect: it lets go of its
    // proxy, kills the connection, and asks the pool for one again.
    drop(first.io);
    connection.kill().await.expect("killed");
    cx.run_until_parked();
    assert!(!connection.has_been_killed());
    assert!(
        !rig.ended(cx).contains(&second_session),
        "the second window's session ended with the first window's reconnect"
    );
    let again = connect(cx).await;
    assert_eq!(
        Arc::as_ptr(&again) as *const (),
        Arc::as_ptr(&connection) as *const (),
        "the pool built a connection, and a session and project server on the host, only to refuse it"
    );
    let reconnect = start_proxy(&again, true, cx);
    let exit_code = reconnect.io.await.expect("an exit code");
    assert!(matches!(
        ProxyLaunchError::from_exit_code(exit_code),
        Some(ProxyLaunchError::ServerNotRunning)
    ));
    cx.run_until_parked();
    assert_eq!(
        rig.sessions(cx),
        1,
        "only the second window's session is left, and none was opened for the reconnect"
    );

    rig.answer(second_session, second_stream, 8, cx);
    cx.run_until_parked();
    assert_eq!(second.incoming.next().await.map(|reply| reply.id), Some(8));
}

#[gpui::test]
async fn a_connection_nobody_uses_is_killed_for_good(cx: &mut TestAppContext) {
    let rig = rig(cx).await;
    let connection = connect(cx).await;
    let window = start_proxy(&connection, false, cx);
    cx.run_until_parked();
    drop(window.io);
    cx.run_until_parked();

    connection.kill().await.expect("killed");
    assert!(connection.has_been_killed());
    let late = start_proxy(&connection, false, cx);
    assert!(
        late.io.await.is_err(),
        "a killed connection opens nothing new"
    );
    cx.run_until_parked();
    assert_eq!(rig.sessions(cx), 0);
}

#[gpui::test]
async fn letting_go_of_the_connection_ends_every_windows_session(cx: &mut TestAppContext) {
    let rig = rig(cx).await;
    let connection = connect(cx).await;
    let _first = start_proxy(&connection, false, cx);
    let _second = start_proxy(&connection, false, cx);
    cx.run_until_parked();
    let sessions = rig.host.read_with(cx, |host, _| host.session_ids());
    assert_eq!(sessions.len(), 2);

    drop(connection);
    cx.run_until_parked();
    let ended = rig.ended(cx);
    assert!(
        sessions.iter().all(|session_id| ended.contains(session_id)),
        "sessions {sessions:?} were open, only {ended:?} ended"
    );
}

#[gpui::test]
async fn a_window_opening_its_session_is_not_cut_off_by_another_window_closing(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx).await;
    let connection = connect(cx).await;
    let first = start_proxy(&connection, false, cx);
    cx.run_until_parked();
    let mut second = start_proxy(&connection, false, cx);
    // The first window is gone while the second is still setting up.
    drop(first.io);
    connection.kill().await.expect("killed");
    cx.run_until_parked();

    second.outgoing.unbounded_send(envelope(5)).expect("sent");
    cx.run_until_parked();
    let read = rig.ids_read(cx);
    let (session_id, stream_id) = *read
        .iter()
        .find(|(_, ids)| ids.contains(&5))
        .map(|(key, _)| key)
        .expect("the second window's messages reach the host");
    rig.answer(session_id, stream_id, 6, cx);
    cx.run_until_parked();
    assert_eq!(second.incoming.next().await.map(|reply| reply.id), Some(6));
    assert!(!rig.ended(cx).contains(&session_id));
}

#[gpui::test]
async fn a_kill_leaves_the_connection_alone_while_a_window_is_still_opening_its_session(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx).await;
    let connection = connect(cx).await;
    let first = start_proxy(&connection, false, cx);
    cx.run_until_parked();
    // The host's answers are held up, so the second window's session is still
    // being set up while the first window goes.
    rig.relay.pause_delivery();
    let mut second = start_proxy(&connection, false, cx);
    cx.run_until_parked();
    drop(first.io);
    cx.run_until_parked();
    assert_eq!(rig.sessions(cx), 1, "only the session being set up is left");

    connection.kill().await.expect("killed");
    rig.relay.resume_delivery();
    for _ in 0..10 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(10));
        cx.run_until_parked();
    }
    second.outgoing.unbounded_send(envelope(9)).expect("sent");
    cx.run_until_parked();
    let read = rig.ids_read(cx);
    let (session_id, stream_id) = *read
        .iter()
        .find(|(_, ids)| ids.contains(&9))
        .map(|(key, _)| key)
        .expect("the second window's messages reach the host");
    rig.answer(session_id, stream_id, 10, cx);
    cx.run_until_parked();
    assert_eq!(second.incoming.next().await.map(|reply| reply.id), Some(10));
}

#[gpui::test]
async fn a_reconnect_reports_that_the_project_server_is_gone(cx: &mut TestAppContext) {
    let rig = rig(cx).await;
    let connection = connect(cx).await;
    let reconnect = start_proxy(&connection, true, cx);
    let exit_code = reconnect.io.await.expect("an exit code");
    assert!(
        matches!(
            ProxyLaunchError::from_exit_code(exit_code),
            Some(ProxyLaunchError::ServerNotRunning)
        ),
        "resuming into a fresh project server would leave the client talking to one that knows nothing it holds (exit code {exit_code})"
    );
    cx.run_until_parked();
    assert_eq!(rig.sessions(cx), 1, "no new session was opened for it");
}

#[gpui::test]
async fn a_session_that_ended_is_not_kept_alive_by_the_connection(cx: &mut TestAppContext) {
    let rig = rig(cx).await;
    let connection =
        RelayRemoteConnection::new(relay_options(), Arc::new(MockDelegate), &mut cx.to_async())
            .await
            .expect("a connection");
    let session = connection
        .first_stream
        .lock()
        .as_ref()
        .map(|stream| stream.session.clone())
        .expect("a session");
    assert!(session.upgrade().is_some());
    assert!(!connection.has_been_killed());

    rig.relay.drop_host_connection(None, "");
    cx.run_until_parked();
    cx.update(|_| {});
    cx.run_until_parked();

    assert!(connection.has_been_killed());
    assert!(
        session.upgrade().is_none(),
        "an ended session, and the relay connection it holds, stayed alive"
    );
}
