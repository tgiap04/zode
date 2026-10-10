//! The initiator held against a scripted host that answers with the same
//! handshake table, channel and pairing gate a real host does.

use std::{sync::Arc, time::Duration};

use futures::StreamExt as _;
use gpui::{AppContext as _, Entity, TestAppContext};
use http_client::{AsyncBody, FakeHttpClient, HttpClient, Response};
use remote_relay_protocol::{Control, DeviceKeypair};
use serde_json::json;

use crate::{
    ConnectError, PinnedDevice, RelayEnvironment, RelayInitiator, RelaySession, RelaySessionEvent,
    SessionError, encode_public_key, load_or_create_keypair,
    scripted_host::ScriptedHost,
    test_support::{FakeRelay, InMemoryKeychain, StaticCredentials},
};

pub(crate) const USER: &str = "user";
pub(crate) const CLIENT_DEVICE: &str = "client-device";
pub(crate) const HOST_DEVICE: &str = "host-device";

pub(crate) struct Rig {
    pub relay: FakeRelay,
    pub host: Entity<ScriptedHost>,
    pub host_key: [u8; 32],
    pub initiator: Entity<RelayInitiator>,
    pub keychain: Arc<InMemoryKeychain>,
    pub credentials: Arc<StaticCredentials>,
}

pub(crate) fn directory(devices: Vec<serde_json::Value>) -> Arc<dyn HttpClient> {
    FakeHttpClient::create(move |request| {
        let devices = devices.clone();
        async move {
            let body = if request.method() == "PUT" {
                "{}".to_string()
            } else {
                serde_json::Value::Array(devices).to_string()
            };
            Ok(Response::builder()
                .status(200)
                .body(AsyncBody::from(body))
                .expect("a response"))
        }
    })
}

pub(crate) fn host_row(public_key: Option<[u8; 32]>) -> serde_json::Value {
    json!({
        "deviceId": HOST_DEVICE,
        "kind": "ide",
        "name": "Studio",
        "publicKey": public_key.map(|key| encode_public_key(&key)),
    })
}

pub(crate) fn init_db(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(db::AppDatabase::test_new()));
}

pub(crate) fn rig_with(
    cx: &mut TestAppContext,
    relay: FakeRelay,
    devices: Vec<serde_json::Value>,
) -> Rig {
    let host_keypair = Arc::new(DeviceKeypair::generate().expect("a key"));
    rig_with_host_key(cx, relay, devices, host_keypair)
}

pub(crate) fn rig_with_host_key(
    cx: &mut TestAppContext,
    relay: FakeRelay,
    devices: Vec<serde_json::Value>,
    host_keypair: Arc<DeviceKeypair>,
) -> Rig {
    init_db(cx);
    let host_key = *host_keypair.public_key();
    let host = cx.new(|cx| ScriptedHost::new(relay.transport(), USER, host_keypair, cx));
    let keychain = InMemoryKeychain::new();
    let credentials = Arc::new(StaticCredentials::new(USER, CLIENT_DEVICE, "client-token"));
    let environment = RelayEnvironment {
        credentials: credentials.clone(),
        keychain: keychain.clone(),
        http_client: directory(devices),
        api_url: "https://api.test/api".into(),
        device_name: "Laptop".into(),
    };
    let transport = relay.client_transport(CLIENT_DEVICE);
    let initiator = cx.new(|cx| RelayInitiator::new(environment, transport, "9.9.9".into(), cx));
    cx.run_until_parked();
    Rig {
        relay,
        host,
        host_key,
        initiator,
        keychain,
        credentials,
    }
}

pub(crate) fn rig(cx: &mut TestAppContext) -> Rig {
    let relay = FakeRelay::new();
    let probe = DeviceKeypair::generate().expect("a key");
    rig_with(cx, relay, vec![host_row(Some(*probe.public_key()))])
}

impl Rig {
    pub async fn initiator_public_key(&self, cx: &mut TestAppContext) -> [u8; 32] {
        let provider: Arc<dyn credentials_provider::CredentialsProvider> = self.keychain.clone();
        *load_or_create_keypair(&provider, USER, &cx.to_async())
            .await
            .expect("the initiator's key")
            .public_key()
    }

    /// Both ends trust each other, as a finished pairing leaves them.
    pub async fn pair_both(&self, cx: &mut TestAppContext) {
        let initiator_key = self.initiator_public_key(cx).await;
        self.host
            .update(cx, |host, _| host.trust(CLIENT_DEVICE, initiator_key));
        let host_key = self.host_key;
        self.initiator.update(cx, |initiator, cx| {
            if let Some(trust) = initiator.hosts_trust.as_mut() {
                trust
                    .pin(
                        PinnedDevice {
                            device_id: HOST_DEVICE.into(),
                            public_key: host_key,
                            name: "Studio".into(),
                            paired_at: 1,
                        },
                        cx,
                    )
                    .expect("a pin");
            }
        });
    }

    pub async fn connect(
        &self,
        cx: &mut TestAppContext,
    ) -> Result<Entity<RelaySession>, ConnectError> {
        let task = self
            .initiator
            .update(cx, |initiator, cx| initiator.connect(HOST_DEVICE, cx));
        task.await
    }
}

#[gpui::test]
async fn it_connects_as_a_client_and_learns_who_is_online(cx: &mut TestAppContext) {
    let relay = FakeRelay::new();
    let rig = rig_with(cx, relay, vec![host_row(None)]);
    assert_eq!(
        rig.relay.last_url().as_deref(),
        Some("wss://api.test/api/relay?role=client")
    );
    assert!(rig.relay.is_client_connected(CLIENT_DEVICE));
    let hosts = rig
        .initiator
        .update(cx, |initiator, cx| initiator.hosts(cx))
        .await
        .expect("a host list");
    assert_eq!(hosts.len(), 1);
    assert!(hosts[0].online);
    assert!(!hosts[0].paired);
    assert_eq!(hosts[0].public_key, None);
}

#[gpui::test]
async fn the_host_list_leaves_out_this_device_and_browsers(cx: &mut TestAppContext) {
    let relay = FakeRelay::new();
    let devices = vec![
        host_row(None),
        json!({"deviceId": CLIENT_DEVICE, "kind": "ide", "name": "Laptop"}),
        json!({"deviceId": "browser", "kind": "web", "name": "Firefox"}),
        json!({"deviceId": "sleeping", "kind": "ide", "name": "Desk"}),
    ];
    let rig = rig_with(cx, relay, devices);
    let hosts = rig
        .initiator
        .update(cx, |initiator, cx| initiator.hosts(cx))
        .await
        .expect("a host list");
    let ids: Vec<&str> = hosts.iter().map(|host| host.device_id.as_str()).collect();
    assert_eq!(ids, vec![HOST_DEVICE, "sleeping"]);
    assert!(hosts[0].online);
    assert!(!hosts[1].online);
}

#[gpui::test]
async fn a_pinned_host_that_trusts_us_yields_a_confirmed_session(cx: &mut TestAppContext) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    let session = rig.connect(cx).await.expect("a session");
    session.read_with(cx, |session, _| {
        assert_eq!(session.host().app_version, "9.9.9");
        assert!(session.host().can("ide"));
        assert_eq!(session.host_name(), "Studio");
        assert!(!session.is_closed());
    });
    cx.run_until_parked();
    let seen = rig.relay.binary_frames_from_host();
    assert!(!seen.is_empty());
    for frame in seen {
        assert!(
            !String::from_utf8_lossy(&frame).contains("app_version"),
            "a frame carried plaintext"
        );
    }
}

#[gpui::test]
async fn a_host_that_does_not_trust_this_zode_closes_the_session(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let host_key = rig.host_key;
    rig.initiator.update(cx, |initiator, cx| {
        if let Some(trust) = initiator.hosts_trust.as_mut() {
            trust
                .pin(
                    PinnedDevice {
                        device_id: HOST_DEVICE.into(),
                        public_key: host_key,
                        name: "Studio".into(),
                        paired_at: 1,
                    },
                    cx,
                )
                .expect("a pin");
        }
    });
    assert_eq!(rig.connect(cx).await.err(), Some(ConnectError::NotPaired));
}

#[gpui::test]
async fn an_unpaired_host_is_not_connected_to(cx: &mut TestAppContext) {
    let rig = rig(cx);
    assert_eq!(rig.connect(cx).await.err(), Some(ConnectError::NotPaired));
    assert_eq!(rig.relay.connection_attempts(), 2, "only the two sockets");
}

#[gpui::test]
async fn a_host_that_is_offline_is_reported_as_such(cx: &mut TestAppContext) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    rig.relay.drop_host_connection(None, "");
    cx.run_until_parked();
    assert_eq!(rig.connect(cx).await.err(), Some(ConnectError::HostOffline));
}

#[gpui::test]
async fn requests_are_matched_to_their_replies(cx: &mut TestAppContext) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    rig.host.update(cx, |host, _| {
        host.respond_with(|control| match control {
            Control::IdeOpen {
                request_id,
                stream_id: Some(7),
                ..
            } => vec![Control::IdeOpened {
                request_id: *request_id,
                stream_id: Some(7),
                path_style: Some("posix".into()),
                shell: None,
                default_shell: None,
            }],
            Control::IdeOpen { request_id, .. } => vec![Control::Error {
                code: "version".into(),
                message: "update".into(),
                request_id: Some(*request_id),
            }],
            _ => Vec::new(),
        });
    });
    let session = rig.connect(cx).await.expect("a session");
    let mut open = |stream_id: u32| {
        session.update(cx, |session, cx| {
            session.request(
                |request_id| Control::IdeOpen {
                    request_id,
                    path: String::new(),
                    line: None,
                    stream_id: Some(stream_id),
                    app_version: Some("9.9.9".into()),
                    proto_version: Some(1),
                },
                cx,
            )
        })
    };
    let (accepted, refused) = (open(7), open(8));
    let accepted = accepted.await.expect("an answer").expect("accepted");
    assert!(matches!(
        accepted,
        Control::IdeOpened {
            stream_id: Some(7),
            ..
        }
    ));
    match refused.await.expect("an answer") {
        Err(SessionError::Remote { code, .. }) => assert_eq!(code, "version"),
        other => panic!("expected the host's error, got {other:?}"),
    }
    assert!(
        !session.read_with(cx, |session, _| session.is_closed()),
        "an error about one request is not an error about the session"
    );
}

#[gpui::test]
async fn a_request_the_host_never_answers_times_out(cx: &mut TestAppContext) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    let session = rig.connect(cx).await.expect("a session");
    let answer = session.update(cx, |session, cx| {
        session.request(
            |request_id| Control::FilesList {
                request_id,
                worktree_id: None,
                path: "/".into(),
            },
            cx,
        )
    });
    cx.executor().advance_clock(Duration::from_secs(31));
    cx.run_until_parked();
    assert_eq!(
        answer.await.expect("an answer").err(),
        Some(SessionError::TimedOut)
    );
}

#[gpui::test]
async fn stream_bytes_arrive_in_order_and_the_end_closes_the_stream(cx: &mut TestAppContext) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    let session = rig.connect(cx).await.expect("a session");
    let (sender, mut receiver) = RelaySession::stream_channel();
    let stream_id = session.update(cx, |session, _| {
        let stream_id = session.allocate_stream_id();
        session
            .register_stream(stream_id, sender)
            .expect("registered");
        stream_id
    });
    let session_id = session.read_with(cx, |session, _| session.session_id());
    rig.host.update(cx, |host, cx| {
        host.send_data(session_id, stream_id, b"one", cx);
        host.send_data(session_id, stream_id, b"two", cx);
        host.send_data(session_id, stream_id, b"", cx);
    });
    cx.run_until_parked();
    assert_eq!(receiver.next().await, Some(b"one".to_vec()));
    assert_eq!(receiver.next().await, Some(b"two".to_vec()));
    assert_eq!(receiver.next().await, None, "the end of the stream");
}

#[gpui::test]
async fn data_sent_by_the_initiator_reaches_the_host_in_order(cx: &mut TestAppContext) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    let session = rig.connect(cx).await.expect("a session");
    session.update(cx, |session, cx| {
        for index in 0..20u8 {
            session
                .try_send_data(3, &[index], cx)
                .expect("room for one frame");
        }
        session.send_end_of_stream(3, cx).expect("room");
    });
    cx.run_until_parked();
    let received = rig
        .host
        .read_with(cx, |host, _| host.received_data().to_vec());
    let bytes: Vec<u8> = received
        .iter()
        .filter(|(_, stream, payload)| *stream == 3 && !payload.is_empty())
        .map(|(_, _, payload)| payload[0])
        .collect();
    assert_eq!(bytes, (0..20u8).collect::<Vec<_>>());
    assert!(
        received
            .last()
            .is_some_and(|(_, _, payload)| payload.is_empty())
    );
}

#[gpui::test]
async fn a_write_with_no_room_seals_nothing_and_succeeds_later_without_a_gap(
    cx: &mut TestAppContext,
) {
    let relay = FakeRelay::new();
    relay.set_outbound_capacity(4);
    let probe = DeviceKeypair::generate().expect("a key");
    let rig = rig_with(cx, relay, vec![host_row(Some(*probe.public_key()))]);
    rig.pair_both(cx).await;
    let session = rig.connect(cx).await.expect("a session");
    cx.run_until_parked();

    let big = vec![7u8; remote_relay_protocol::MAX_INNER_PAYLOAD_LEN * 3];
    let outcome = session.update(cx, |session, cx| {
        session.try_send_data(5, b"a", cx).expect("fits");
        session.try_send_data(5, b"b", cx).expect("fits");
        session.try_send_data(5, &big, cx)
    });
    assert_eq!(outcome, Err(SessionError::Backpressure));

    cx.run_until_parked();
    session.update(cx, |session, cx| {
        session.try_send_data(5, &big, cx).expect("room now");
    });
    cx.run_until_parked();
    let received = rig
        .host
        .read_with(cx, |host, _| host.received_data().to_vec());
    assert_eq!(received.len(), 5, "a, b and three frames: none was lost");
    assert!(
        rig.host
            .read_with(cx, |host, _| host.session_ids().len() == 1),
        "the host could read every frame, so the counter had no gap"
    );
}

#[gpui::test]
async fn a_version_error_from_the_host_ends_the_session(cx: &mut TestAppContext) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    let session = rig.connect(cx).await.expect("a session");
    let closed = Arc::new(parking_lot::Mutex::new(None));
    let _subscription = cx.update(|cx| {
        let closed = closed.clone();
        cx.subscribe(&session, move |_, event: &RelaySessionEvent, _| {
            if let RelaySessionEvent::Closed(reason) = event {
                *closed.lock() = Some(reason.clone());
            }
        })
    });
    let session_id = session.read_with(cx, |session, _| session.session_id());
    rig.host.update(cx, |host, cx| {
        host.send_control(
            session_id,
            &Control::Error {
                code: "version".into(),
                message: "update".into(),
                request_id: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    assert!(closed.lock().is_some());
    assert!(session.read_with(cx, |session, _| session.is_closed()));
    assert_eq!(
        session.update(cx, |session, cx| session.send_control(&Control::Ping, cx)),
        Err(SessionError::Closed(
            "the other Zode runs a different version".into()
        ))
    );
}

#[gpui::test]
async fn pings_are_answered_and_lists_are_remembered(cx: &mut TestAppContext) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    let session = rig.connect(cx).await.expect("a session");
    let session_id = session.read_with(cx, |session, _| session.session_id());
    rig.host.update(cx, |host, cx| {
        host.send_control(session_id, &Control::Ping, cx);
        host.send_control(
            session_id,
            &Control::AgentUpdate {
                agent: remote_relay_protocol::AgentSummary {
                    id: "a1".into(),
                    name: "Agent".into(),
                    status: remote_relay_protocol::AgentStatus::Working,
                    title: None,
                },
            },
            cx,
        );
    });
    cx.run_until_parked();
    assert!(rig.host.read_with(cx, |host, _| {
        host.received_controls()
            .iter()
            .any(|(_, control)| *control == Control::Pong)
    }));
    session.read_with(cx, |session, _| {
        assert_eq!(session.agents().len(), 1);
        assert_eq!(session.agents()[0].id, "a1");
    });
}

#[gpui::test]
async fn closing_a_session_tells_the_relay_and_the_host(cx: &mut TestAppContext) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    let session = rig.connect(cx).await.expect("a session");
    let session_id = session.read_with(cx, |session, _| session.session_id());
    session.update(cx, |session, cx| session.close(cx));
    cx.run_until_parked();
    assert!(session.read_with(cx, |session, _| session.is_closed()));
    assert!(
        rig.host
            .read_with(cx, |host, _| host.ended_sessions().contains(&session_id))
    );
}

#[gpui::test]
async fn only_an_explicit_device_revocation_wipes_the_pins_and_key(cx: &mut TestAppContext) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    rig.relay
        .close_client_socket(CLIENT_DEVICE, Some(4403), "session_revoked");
    cx.run_until_parked();
    assert!(
        rig.initiator
            .read_with(cx, |initiator, _| initiator.is_paired(HOST_DEVICE))
    );
    assert!(!rig.keychain.stored_passwords().is_empty());

    let rig = self::rig(cx);
    rig.pair_both(cx).await;
    rig.relay.revoke_device(CLIENT_DEVICE);
    cx.run_until_parked();
    assert!(
        !rig.initiator
            .read_with(cx, |initiator, _| initiator.is_paired(HOST_DEVICE))
    );
    assert!(
        rig.keychain.stored_passwords().is_empty(),
        "the key is gone"
    );
}

#[gpui::test]
async fn a_dropped_connection_ends_the_session_and_the_client_reconnects(cx: &mut TestAppContext) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    let session = rig.connect(cx).await.expect("a session");
    rig.relay.close_client_socket(CLIENT_DEVICE, None, "");
    cx.run_until_parked();
    assert!(session.read_with(cx, |session, _| session.is_closed()));
    cx.executor().advance_clock(Duration::from_millis(1300));
    cx.run_until_parked();
    assert!(rig.relay.is_client_connected(CLIENT_DEVICE));
}

#[gpui::test]
async fn the_relay_refuses_a_role_it_does_not_know(cx: &mut TestAppContext) {
    use crate::RelayTransport as _;
    let relay = FakeRelay::new();
    let transport = relay.client_transport("someone");
    for url in [
        "wss://api.test/api/relay?role=Client",
        "wss://api.test/api/relay?role=",
        "wss://api.test/api/relay?role=client&role=client",
        "wss://api.test/api/relay?role=admin",
    ] {
        let outcome = transport
            .connect(url.into(), "token".into(), &cx.to_async())
            .await;
        assert!(
            matches!(
                &outcome,
                Err(crate::TransportError::Refused { status: 400, reason: Some(reason) })
                    if reason == "invalid_role"
            ),
            "{url}"
        );
    }
}

#[gpui::test]
async fn a_client_cannot_open_a_session_to_itself(cx: &mut TestAppContext) {
    let relay = FakeRelay::new();
    let rig = rig_with(cx, relay, vec![]);
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let client = rig
        .initiator
        .read_with(cx, |initiator, _| initiator.client.clone())
        .expect("a client");
    let _subscription = cx.update(|cx| {
        let events = events.clone();
        cx.subscribe(&client, move |_, event: &crate::RelayEvent, _| {
            events.lock().push(event.clone());
        })
    });
    cx.update(|cx| {
        client
            .read(cx)
            .writer()
            .expect("connected")
            .request_open(CLIENT_DEVICE, crate::OpenMode::Session)
            .expect("queued");
    });
    cx.run_until_parked();
    assert!(events.lock().iter().any(|event| matches!(
        event,
        crate::RelayEvent::Error { code } if code == "unauthorized"
    )));
}

fn routes_held(rig: &Rig, cx: &TestAppContext) -> usize {
    rig.initiator
        .read_with(cx, |initiator, _| initiator.routes.len())
}

#[gpui::test]
async fn an_account_switch_stops_the_initiator_instead_of_mixing_identities(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    cx.update(|cx| RelayInitiator::register_global(&rig.initiator, cx));
    assert!(cx.update(|cx| RelayInitiator::global(cx)).is_some());

    rig.credentials
        .switch_account("someone-else", "their-device", "their-token");
    let listed = rig
        .initiator
        .update(cx, |initiator, cx| initiator.hosts(cx))
        .await;
    assert!(
        listed.is_err(),
        "the directory must not be read as the new account"
    );
    assert!(
        rig.initiator
            .read_with(cx, |initiator, _| initiator.stopped.is_some())
    );
    assert!(matches!(
        rig.connect(cx).await,
        Err(ConnectError::Unavailable(_)) | Err(ConnectError::Relay(_))
    ));
    assert!(
        cx.update(|cx| RelayInitiator::global(cx)).is_none(),
        "a new initiator must be created for the new account"
    );
    assert!(
        !rig.initiator
            .read_with(cx, |initiator, _| initiator.pinned_hosts().is_empty()),
        "the first account's pins are left alone"
    );
}

#[gpui::test]
async fn a_stopped_initiator_is_not_handed_out_as_the_global_one(cx: &mut TestAppContext) {
    let rig = rig(cx);
    cx.update(|cx| RelayInitiator::register_global(&rig.initiator, cx));
    assert!(cx.update(|cx| RelayInitiator::global(cx)).is_some());
    rig.relay
        .close_client_socket(CLIENT_DEVICE, Some(4403), "session_ended");
    cx.run_until_parked();
    assert!(
        rig.initiator
            .read_with(cx, |initiator, _| initiator.stopped.is_some())
    );
    assert!(cx.update(|cx| RelayInitiator::global(cx)).is_none());
}

#[gpui::test]
async fn dropping_a_session_without_closing_it_ends_it_at_the_relay_and_drops_its_route(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    let session = rig.connect(cx).await.expect("a session");
    let session_id = session.read_with(cx, |session, _| session.session_id());
    assert_eq!(routes_held(&rig, cx), 1);
    drop(session);
    cx.update(|_| {});
    cx.run_until_parked();
    assert!(
        rig.host
            .read_with(cx, |host, _| host.ended_sessions().contains(&session_id)),
        "the relay was not told"
    );
    assert_eq!(routes_held(&rig, cx), 0, "the route was kept");
}

#[gpui::test]
async fn closing_a_session_drops_its_route(cx: &mut TestAppContext) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    let session = rig.connect(cx).await.expect("a session");
    session.update(cx, |session, cx| session.close(cx));
    cx.run_until_parked();
    assert_eq!(routes_held(&rig, cx), 0);
}

#[gpui::test]
async fn an_opened_session_that_is_dropped_unused_is_closed_at_the_relay(cx: &mut TestAppContext) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    let weak = rig.initiator.downgrade();
    let opened = RelayInitiator::open_awaiting(
        &weak,
        HOST_DEVICE,
        crate::OpenMode::Session,
        &mut cx.to_async(),
    )
    .await
    .expect("opened");
    let session_id = opened.session_id;
    assert_eq!(routes_held(&rig, cx), 1);
    drop(opened);
    cx.run_until_parked();
    assert!(
        rig.host
            .read_with(cx, |host, _| host.ended_sessions().contains(&session_id))
    );
    assert_eq!(routes_held(&rig, cx), 0);
}

#[gpui::test]
async fn a_reader_that_falls_too_far_behind_has_its_stream_ended(cx: &mut TestAppContext) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    let session = rig.connect(cx).await.expect("a session");
    let (sender, mut receiver) = RelaySession::stream_channel();
    let stream_id = session.update(cx, |session, _| {
        let stream_id = session.allocate_stream_id();
        session
            .register_stream(stream_id, sender)
            .expect("registered");
        stream_id
    });
    let session_id = session.read_with(cx, |session, _| session.session_id());
    let chunk = vec![1u8; remote_relay_protocol::MAX_INNER_PAYLOAD_LEN];
    let chunks = crate::MAX_STREAM_QUEUE_BYTES * 3 / chunk.len();
    for _ in 0..chunks {
        rig.host.update(cx, |host, cx| {
            host.send_data(session_id, stream_id, &chunk, cx)
        });
        cx.run_until_parked();
    }
    let mut received = 0;
    while let Some(bytes) = receiver.next().await {
        received += bytes.len();
    }
    assert!(
        received <= crate::MAX_STREAM_QUEUE_BYTES,
        "{received} bytes were queued for a reader that never read"
    );
    assert!(
        rig.host
            .read_with(cx, |host, _| host.received_data().iter().any(
                |(_, stream, payload)| *stream == stream_id && payload.is_empty()
            )),
        "the host was not told to stop"
    );
}

#[gpui::test]
async fn a_reader_that_keeps_up_is_never_cut_off(cx: &mut TestAppContext) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    let session = rig.connect(cx).await.expect("a session");
    let (sender, mut receiver) = RelaySession::stream_channel();
    let stream_id = session.update(cx, |session, _| {
        let stream_id = session.allocate_stream_id();
        session
            .register_stream(stream_id, sender)
            .expect("registered");
        stream_id
    });
    let session_id = session.read_with(cx, |session, _| session.session_id());
    let chunk = vec![1u8; remote_relay_protocol::MAX_INNER_PAYLOAD_LEN];
    let chunks = crate::MAX_STREAM_QUEUE_BYTES * 3 / chunk.len();
    for _ in 0..chunks {
        rig.host.update(cx, |host, cx| {
            host.send_data(session_id, stream_id, &chunk, cx)
        });
        cx.run_until_parked();
        assert_eq!(
            receiver.next().await.map(|bytes| bytes.len()),
            Some(chunk.len())
        );
    }
}

#[gpui::test]
async fn a_write_that_never_gets_room_gives_up_with_backpressure(cx: &mut TestAppContext) {
    let relay = FakeRelay::new();
    relay.set_outbound_capacity(2);
    let probe = DeviceKeypair::generate().expect("a key");
    let rig = rig_with(cx, relay, vec![host_row(Some(*probe.public_key()))]);
    rig.pair_both(cx).await;
    let session = rig.connect(cx).await.expect("a session");
    cx.run_until_parked();
    rig.relay.pause_client_delivery();
    let big = vec![7u8; remote_relay_protocol::MAX_INNER_PAYLOAD_LEN * 8];
    let weak = session.downgrade();
    let write =
        cx.spawn(async move |mut cx| crate::send_data_waiting(&weak, 5, &big, &mut cx).await);
    cx.run_until_parked();
    cx.executor()
        .advance_clock(crate::session::WRITE_DEADLINE + Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(write.await, Err(SessionError::Backpressure));
}

#[gpui::test]
async fn a_host_that_keeps_talking_without_answering_hello_still_times_out(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx);
    rig.pair_both(cx).await;
    rig.host.update(cx, |host, _| host.stay_silent_on_hello());
    let task = rig
        .initiator
        .update(cx, |initiator, cx| initiator.connect(HOST_DEVICE, cx));
    for _ in 0..6 {
        cx.executor().advance_clock(Duration::from_secs(4));
        cx.run_until_parked();
        let session_ids = rig.host.read_with(cx, |host, _| host.session_ids());
        for session_id in session_ids {
            rig.host.update(cx, |host, cx| {
                host.send_control(session_id, &Control::Ping, cx)
            });
        }
    }
    cx.run_until_parked();
    assert_eq!(task.await.err(), Some(ConnectError::TimedOut));
}
