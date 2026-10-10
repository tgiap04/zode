//! The host and a stand-in browser, talking through an in-process relay.
//!
//! Everything between them is the real thing: the real pairing exchange, the
//! real Noise handshake and transport, the real pty-backed terminal. What is
//! faked is only the socket and the account service.

use std::{sync::Arc, time::Duration};

use collections::HashMap;
use futures::AsyncReadExt as _;
use gpui::{AppContext as _, Entity, TestAppContext, UpdateGlobal as _};
use http_client::{AsyncBody, FakeHttpClient, HttpClient, Response};
use parking_lot::Mutex;
use remote_relay_client::{
    OpenMode, StopReason, TrustStore, decode_public_key, encode_pairing, encode_public_key,
    secure_session::{SecureChannel, read_control},
    test_support::{FakeClient, FakeRelay, InMemoryKeychain, StaticCredentials},
};
use remote_relay_protocol::{
    Control, DeviceKeypair, Handshake, HandshakeParameters, InnerKind, KEY_LEN, PairingMessage,
    PairingRequester, build_prologue, random_nonce,
};
use settings::{Settings as _, SettingsStore};
use task::Shell;
use terminal::{
    Terminal, TerminalBuilder,
    terminal_settings::{AlternateScroll, CursorShape},
};
use util::paths::PathStyle;
use zode_account::{Account, AccountStatus, AccountUser};

use crate::{
    AgentMirror, FileBrowser, HostEnvironment, HostState, RemoteHost, TerminalMirror,
    session_router::UNSUPPORTED,
};

const USER_ID: &str = "user-1";
const HOST_DEVICE: &str = "host-device";

struct Rig {
    relay: FakeRelay,
    host: Entity<RemoteHost>,
    terminals: Entity<TerminalMirror>,
    keychain: Arc<InMemoryKeychain>,
    host_public_key: Arc<Mutex<Option<[u8; KEY_LEN]>>>,
    registrations: Arc<Mutex<usize>>,
}

fn set_remote_control(cx: &mut TestAppContext, enabled: bool, idle_timeout_minutes: Option<u64>) {
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |content| {
                let settings = content.remote_control.get_or_insert_default();
                settings.enabled = Some(enabled);
                if let Some(minutes) = idle_timeout_minutes {
                    settings.idle_timeout_minutes = Some(minutes);
                }
            });
        });
    });
}

fn device_list_body() -> String {
    format!(
        r#"[{{"deviceId":"browser-1","kind":"web","name":"Ada's browser","publicKey":"{key}","online":true,"rotatedAt":null}},
            {{"deviceId":"browser-2","kind":"web","name":"Grace's browser","publicKey":"{key}","online":true,"rotatedAt":null}}]"#,
        key = encode_public_key(&[9; KEY_LEN])
    )
}

async fn rig(cx: &mut TestAppContext, enabled: bool) -> Rig {
    rig_with_status(
        cx,
        enabled,
        AccountStatus::SignedIn(AccountUser {
            id: USER_ID.into(),
            email: "ada@example.com".into(),
            name: None,
            avatar_url: None,
        }),
    )
    .await
}

async fn rig_with_status(cx: &mut TestAppContext, enabled: bool, status: AccountStatus) -> Rig {
    rig_with_files(cx, enabled, status, false).await
}

/// `with_files` gives the host something to answer file requests with, as the
/// running app always does; without it the host does not offer `files`.
async fn rig_with_files(
    cx: &mut TestAppContext,
    enabled: bool,
    status: AccountStatus,
    with_files: bool,
) -> Rig {
    cx.executor().allow_parking();
    cx.update(|cx| {
        let store = SettingsStore::test(cx);
        cx.set_global(store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        cx.set_global(db::AppDatabase::test_new());
    });
    set_remote_control(cx, enabled, None);

    let relay = FakeRelay::new();
    let keychain = InMemoryKeychain::new();
    let host_public_key = Arc::new(Mutex::new(None));
    let registrations = Arc::new(Mutex::new(0));
    let http_client: Arc<dyn HttpClient> = FakeHttpClient::create({
        let host_public_key = host_public_key.clone();
        let registrations = registrations.clone();
        move |request| {
            let host_public_key = host_public_key.clone();
            let registrations = registrations.clone();
            async move {
                let (parts, mut body) = request.into_parts();
                if parts.method == "PUT" {
                    let mut text = String::new();
                    body.read_to_string(&mut text)
                        .await
                        .expect("a request body");
                    let value: serde_json::Value = serde_json::from_str(&text).expect("json");
                    let key = value["publicKey"].as_str().and_then(decode_public_key);
                    *host_public_key.lock() = key;
                    *registrations.lock() += 1;
                    return Ok(Response::builder()
                        .status(200)
                        .body(AsyncBody::from("{}"))
                        .expect("a response"));
                }
                Ok(Response::builder()
                    .status(200)
                    .body(AsyncBody::from(device_list_body()))
                    .expect("a response"))
            }
        }
    });

    let account = cx.update(|cx| cx.new(|_| Account::for_test(status)));
    let environment = HostEnvironment {
        credentials: Arc::new(StaticCredentials::new(USER_ID, HOST_DEVICE, "access-token")),
        keychain: keychain.clone(),
        http_client,
        api_url: "https://api.test/api".to_string(),
        device_name: "Test Mac".to_string(),
    };
    let agents = cx.new(AgentMirror::new);
    let terminals = cx.new(TerminalMirror::new);
    let files = with_files.then(|| cx.new(FileBrowser::new));
    let host = {
        let relay = relay.clone();
        let terminals = terminals.clone();
        cx.new(move |cx| {
            RemoteHost::new(
                account,
                environment,
                relay.transport(),
                agents,
                terminals,
                files,
                "9.9.9".to_string(),
                cx,
            )
        })
    };
    cx.run_until_parked();
    Rig {
        relay,
        host,
        terminals,
        keychain,
        host_public_key,
        registrations,
    }
}

pub(crate) async fn shell_terminal(script: &str, cx: &mut TestAppContext) -> Entity<Terminal> {
    let builder = cx
        .update(|cx| {
            TerminalBuilder::new(
                None,
                None,
                Shell::WithArguments {
                    program: "/bin/sh".to_string(),
                    args: vec!["-c".to_string(), script.to_string()],
                    title_override: None,
                },
                HashMap::default(),
                CursorShape::default(),
                AlternateScroll::On,
                None,
                vec![],
                0,
                false,
                0,
                None,
                cx,
                vec![],
                PathStyle::local(),
            )
        })
        .await
        .expect("a shell must spawn");
    cx.new(|cx| builder.subscribe(cx))
}

/// A browser, as the host meets it: a key, and a way to the relay.
struct Browser {
    id: String,
    keypair: DeviceKeypair,
    client: FakeClient,
}

impl Browser {
    fn new(rig: &Rig, id: &str) -> Self {
        Self {
            id: id.to_string(),
            keypair: DeviceKeypair::generate().expect("a key"),
            client: rig.relay.connect_client(id),
        }
    }
}

/// Pairs `browser` with the host, comparing the codes the way two people would.
fn pair(rig: &Rig, browser: &Browser, cx: &mut TestAppContext) {
    let host_key = rig
        .host_public_key
        .lock()
        .expect("the host registered its key");
    let session_id = browser.client.open(HOST_DEVICE, OpenMode::Pair);
    let nonce = random_nonce().expect("randomness");
    let (requester, request) = PairingRequester::start(*browser.keypair.public_key(), nonce);
    browser
        .client
        .send_text(&encode_pairing(session_id, &request).expect("encodes"));
    cx.run_until_parked();

    let accept = browser
        .client
        .take_pairing()
        .pop()
        .expect("the host accepted");
    let PairingMessage::PairAccept { public_key, nonce } = accept else {
        panic!("expected an accept");
    };
    assert_eq!(public_key, host_key);
    let (reveal, outcome) = requester
        .receive_accept(public_key, nonce)
        .expect("the browser accepts the host's key");
    browser
        .client
        .send_text(&encode_pairing(session_id, &reveal).expect("encodes"));
    cx.run_until_parked();

    let pending = rig
        .host
        .read_with(cx, |host, _| host.pending_pairing().cloned())
        .expect("the host is asking the person");
    assert_eq!(
        pending.code, outcome.short_authentication_string,
        "both screens must show the same six digits"
    );
    assert_eq!(pending.peer_device_id, browser.id);
    rig.host
        .update(cx, |host, cx| host.decide_pairing(session_id, true, cx));
    cx.run_until_parked();
    assert!(
        browser.client.was_told_closed(session_id),
        "pairing ends its session"
    );
}

struct Connected {
    session_id: u32,
    channel: SecureChannel,
}

impl Connected {
    fn send(&mut self, browser: &Browser, control: &Control) {
        let sealed = self.channel.seal_control(control).expect("seals");
        browser.client.send_binary(self.session_id, &sealed);
    }

    fn send_data(&mut self, browser: &Browser, stream_id: u32, bytes: &[u8]) {
        let sealed = self
            .channel
            .seal_data(stream_id, bytes.to_vec())
            .expect("seals");
        browser.client.send_binary(self.session_id, &sealed);
    }

    /// Everything the host has sent since, opened.
    fn receive(&mut self, browser: &Browser) -> Vec<(InnerKind, u32, Vec<u8>)> {
        browser
            .client
            .take_binary()
            .into_iter()
            .map(|(_, payload)| {
                let frame = self.channel.open(&payload).expect("the host's frames open");
                (frame.kind, frame.stream_id, frame.payload)
            })
            .collect()
    }

    fn controls(&mut self, browser: &Browser) -> Vec<Control> {
        self.receive(browser)
            .into_iter()
            .filter(|(kind, _, _)| *kind == InnerKind::Control)
            .map(|(_, _, payload)| {
                read_control(&remote_relay_protocol::InnerFrame {
                    kind: InnerKind::Control,
                    stream_id: 0,
                    payload,
                })
                .expect("the host speaks the protocol")
            })
            .collect()
    }
}

fn hello(relay_protocol: u32) -> Control {
    Control::Hello {
        relay_protocol,
        app_version: "0.1.5".into(),
        rpc_protocol: 1,
        capabilities: vec!["terminal".into()],
    }
}

/// Opens a session as an already-paired browser and completes the handshake.
fn open_session(rig: &Rig, browser: &Browser, cx: &mut TestAppContext) -> Connected {
    let host_key = rig
        .host_public_key
        .lock()
        .expect("the host registered its key");
    let session_id = browser.client.open(HOST_DEVICE, OpenMode::Session);
    let prologue = build_prologue(USER_ID, &browser.id, HOST_DEVICE).expect("a prologue");
    let mut handshake = Handshake::initiator(&HandshakeParameters {
        local_private_key: browser.keypair.private_key(),
        remote_public_key: &host_key,
        prologue: &prologue,
    })
    .expect("a handshake");
    let first = handshake.write_message(&[]).expect("message one");
    browser.client.send_binary(session_id, &first);
    cx.run_until_parked();

    let (_, reply) = browser
        .client
        .take_binary()
        .pop()
        .expect("the host answered the handshake");
    handshake.read_message(&reply).expect("message two");
    Connected {
        session_id,
        channel: SecureChannel::new(handshake.into_session().expect("a session")),
    }
}

async fn paired_and_connected(cx: &mut TestAppContext) -> (Rig, Browser, Connected) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let mut connected = open_session(&rig, &browser, cx);
    connected.send(&browser, &hello(1));
    cx.run_until_parked();
    (rig, browser, connected)
}

#[gpui::test]
async fn while_disabled_no_socket_is_opened_and_no_terminal_is_tapped(cx: &mut TestAppContext) {
    let rig = rig(cx, false).await;
    let terminal = shell_terminal("sleep 5", cx).await;
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(600));
    cx.run_until_parked();

    assert_eq!(
        rig.relay.connection_attempts(),
        0,
        "not one connection may be tried"
    );
    assert!(!rig.host.read_with(cx, |host, _| host.has_relay_client()));
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.state().clone()),
        HostState::Disabled
    );
    assert_eq!(
        *rig.registrations.lock(),
        0,
        "not even the account service is told"
    );
    assert_eq!(
        rig.keychain.write_count(),
        0,
        "no device key is made for a feature that is off"
    );
    assert!(
        !rig.terminals
            .read_with(cx, |terminals, cx| terminals.any_tapped(cx))
    );
    assert!(!terminal.read_with(cx, |terminal, _| terminal.has_remote_tap()));
}

#[gpui::test]
async fn turning_it_on_connects_and_registers_the_key_and_turning_it_off_takes_everything_down(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, false).await;
    set_remote_control(cx, true, None);
    cx.run_until_parked();

    assert_eq!(rig.relay.connection_attempts(), 1);
    assert!(rig.relay.is_host_connected());
    assert_eq!(*rig.registrations.lock(), 1);
    assert_eq!(rig.relay.last_bearer().as_deref(), Some("access-token"));
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.state().clone()),
        HostState::Online
    );
    assert_eq!(
        rig.keychain.write_count(),
        1,
        "the device key is saved once"
    );
    assert!(
        rig.keychain
            .stored_passwords()
            .iter()
            .all(|password| password.len() == 2 * KEY_LEN),
        "only the key pair is stored"
    );

    set_remote_control(cx, false, None);
    cx.run_until_parked();
    assert!(!rig.relay.is_host_connected(), "off closes the socket");
    assert!(!rig.host.read_with(cx, |host, _| host.has_relay_client()));
}

#[gpui::test]
async fn the_device_key_is_never_sent_anywhere(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let stored = rig.keychain.stored_passwords();
    let private_key = &stored[0][..KEY_LEN];
    let public_key = rig.host_public_key.lock().expect("registered");
    assert_ne!(private_key, public_key, "the pair is private then public");
    assert_eq!(
        &stored[0][KEY_LEN..],
        public_key,
        "and the public half is what was sent"
    );
}

#[gpui::test]
async fn a_paired_browser_sees_the_lists_and_can_drive_a_terminal(cx: &mut TestAppContext) {
    let terminal_script = "read line; printf 'got:%s\\\\n' \"$line\"; sleep 30";
    let (rig, browser, mut connected) = paired_and_connected(cx).await;
    let terminal = shell_terminal(terminal_script, cx).await;
    cx.run_until_parked();

    // Hello is answered, then the lists follow; the terminal that appeared
    // afterwards is announced in a fresh list.
    let controls = connected.controls(&browser);
    assert!(
        matches!(
            &controls[0],
            Control::HelloAck { relay_protocol: 1, rpc_protocol: 1, app_version, capabilities }
                if app_version == "9.9.9" && capabilities == &vec!["terminal".to_string()]
        ),
        "{controls:?}"
    );
    assert!(matches!(&controls[1], Control::AgentList { .. }));
    let terminals = controls
        .iter()
        .rev()
        .find_map(|control| match control {
            Control::TerminalList { terminals } => Some(terminals.clone()),
            _ => None,
        })
        .expect("a terminal list");
    assert_eq!(terminals.len(), 1);
    let terminal_id = terminals[0].id.clone();
    assert!(terminals[0].columns > 0 && terminals[0].rows > 0);

    assert!(
        !terminal.read_with(cx, |terminal, _| terminal.has_remote_tap()),
        "listing taps nothing"
    );
    connected.send(
        &browser,
        &Control::TerminalAttach {
            terminal_id: terminal_id.clone(),
            stream_id: 7,
        },
    );
    cx.run_until_parked();
    assert!(
        terminal.read_with(cx, |terminal, _| terminal.has_remote_tap()),
        "attaching taps it"
    );

    let received = connected.receive(&browser);
    let attached = received
        .iter()
        .find(|(kind, _, _)| *kind == InnerKind::Control)
        .expect("an attach reply");
    let Control::TerminalAttached {
        stream_id,
        columns,
        rows,
        ..
    } = read_control(&remote_relay_protocol::InnerFrame {
        kind: InnerKind::Control,
        stream_id: 0,
        payload: attached.2.clone(),
    })
    .expect("parses")
    else {
        panic!("expected terminal_attached");
    };
    assert_eq!(stream_id, 7);
    assert!(columns > 0 && rows > 0);
    let snapshot: Vec<u8> = received
        .iter()
        .filter(|(kind, stream, _)| *kind == InnerKind::Data && *stream == 7)
        .flat_map(|(_, _, payload)| payload.clone())
        .collect();
    assert!(
        snapshot.starts_with(b"\x1bc"),
        "the screen is sent first, as a reset and a redraw"
    );

    // Typing arrives in the terminal, and its answer comes back on the stream.
    connected.send_data(&browser, 7, b"hi\r");
    let mut output = Vec::new();
    for _ in 0..500 {
        cx.background_executor
            .timer(Duration::from_millis(10))
            .await;
        cx.run_until_parked();
        output.extend(
            connected
                .receive(&browser)
                .into_iter()
                .filter(|(kind, stream, _)| *kind == InnerKind::Data && *stream == 7)
                .flat_map(|(_, _, payload)| payload),
        );
        if String::from_utf8_lossy(&output).contains("got:hi") {
            break;
        }
    }
    assert!(
        String::from_utf8_lossy(&output).contains("got:hi"),
        "the terminal's answer reached the browser: {:?}",
        String::from_utf8_lossy(&output)
    );

    // The relay carried every one of those bytes and could read none of them.
    for frame in rig.relay.binary_frames_from_host() {
        assert!(
            !String::from_utf8_lossy(&frame).contains("got:hi"),
            "plaintext crossed the relay"
        );
    }

    connected.send(&browser, &Control::TerminalDetach { terminal_id });
    cx.run_until_parked();
    assert!(
        !terminal.read_with(cx, |terminal, _| terminal.has_remote_tap()),
        "the last device leaving removes the tap"
    );
}

#[gpui::test]
async fn output_that_outruns_the_relay_is_held_and_resent_without_breaking_the_session(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, false).await;
    // Room for a handful of frames, so the host's queue to the relay fills.
    rig.relay.set_outbound_capacity(6);
    set_remote_control(cx, true, None);
    cx.run_until_parked();
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let mut connected = open_session(&rig, &browser, cx);
    connected.send(&browser, &hello(1));
    cx.run_until_parked();
    let terminal = shell_terminal(
        "read line; head -c 3000000 /dev/zero | tr '\\000' 'x'; printf '\\nFLOODEND\\n'; sleep 30",
        cx,
    )
    .await;
    cx.run_until_parked();
    let terminal_id = connected
        .controls(&browser)
        .into_iter()
        .rev()
        .find_map(|control| match control {
            Control::TerminalList { terminals } => terminals.first().map(|t| t.id.clone()),
            _ => None,
        })
        .expect("the terminal is listed");
    connected.send(
        &browser,
        &Control::TerminalAttach {
            terminal_id,
            stream_id: 7,
        },
    );
    cx.run_until_parked();
    connected.receive(&browser);

    rig.relay.pause_delivery();
    connected.send_data(&browser, 7, b"go\r");
    for _ in 0..500 {
        if terminal
            .update(cx, |terminal, _| terminal.get_content())
            .contains("FLOODEND")
        {
            break;
        }
        cx.background_executor
            .timer(Duration::from_millis(10))
            .await;
        cx.run_until_parked();
    }
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.sessions().len()),
        1,
        "a relay that is slow is not a reason to drop the device"
    );

    rig.relay.resume_delivery();
    let mut output = Vec::new();
    for _ in 0..500 {
        cx.background_executor
            .timer(Duration::from_millis(10))
            .await;
        cx.run_until_parked();
        // Every frame must still open: a frame sealed and then not sent would
        // leave the counter one ahead of the peer's, and nothing after it
        // could be read.
        output.extend(
            connected
                .receive(&browser)
                .into_iter()
                .filter(|(kind, stream, _)| *kind == InnerKind::Data && *stream == 7)
                .flat_map(|(_, _, payload)| payload),
        );
        if String::from_utf8_lossy(&output).contains("FLOODEND") {
            break;
        }
    }
    assert!(
        String::from_utf8_lossy(&output).contains("FLOODEND"),
        "the end of the flood reached the browser"
    );
    assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 1);
}

#[gpui::test]
async fn a_device_that_was_never_paired_gets_no_session(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let stranger = Browser::new(&rig, "browser-2");
    let session_id = stranger.client.open(HOST_DEVICE, OpenMode::Session);
    cx.run_until_parked();
    assert!(stranger.client.was_told_closed(session_id));
    assert!(rig.host.read_with(cx, |host, _| host.sessions().is_empty()));
}

#[gpui::test]
async fn a_handshake_from_the_wrong_key_is_dropped_and_shows_nothing(cx: &mut TestAppContext) {
    let (rig, browser, _connected) = paired_and_connected(cx).await;
    // Same device id, a key nobody paired.
    let impostor = Browser {
        id: browser.id,
        keypair: DeviceKeypair::generate().expect("a key"),
        client: rig.relay.connect_client("browser-1"),
    };
    let host_key = rig.host_public_key.lock().expect("registered");
    let session_id = impostor.client.open(HOST_DEVICE, OpenMode::Session);
    let prologue = build_prologue(USER_ID, &impostor.id, HOST_DEVICE).expect("a prologue");
    let mut handshake = Handshake::initiator(&HandshakeParameters {
        local_private_key: impostor.keypair.private_key(),
        remote_public_key: &host_key,
        prologue: &prologue,
    })
    .expect("a handshake");
    let first = handshake.write_message(&[]).expect("message one");
    impostor.client.send_binary(session_id, &first);
    cx.run_until_parked();

    assert!(
        impostor.client.take_binary().is_empty(),
        "no reply is given to an impostor"
    );
    assert!(impostor.client.was_told_closed(session_id));
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.sessions().len()),
        1,
        "the real session is untouched"
    );
}

#[gpui::test]
async fn a_session_does_not_count_as_control_until_the_peer_proves_the_keys(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let _connected = open_session(&rig, &browser, cx);
    assert!(
        rig.host.read_with(cx, |host, _| host.sessions().is_empty()),
        "a recorded first message replayed by the relay would look exactly like this"
    );
    assert!(!rig.host.read_with(cx, |host, _| host.holds_display_awake()));
}

#[gpui::test]
async fn a_different_protocol_version_is_refused_and_the_session_closed(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let mut connected = open_session(&rig, &browser, cx);
    connected.send(&browser, &hello(2));
    cx.run_until_parked();

    let controls = connected.controls(&browser);
    assert!(
        matches!(&controls[0], Control::Error { code, .. } if code == "version"),
        "{controls:?}"
    );
    assert!(browser.client.was_told_closed(connected.session_id));
    assert!(rig.host.read_with(cx, |host, _| host.sessions().is_empty()));
}

#[gpui::test]
async fn nothing_is_acted_on_before_hello(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let terminal = shell_terminal("sleep 30", cx).await;
    let mut connected = open_session(&rig, &browser, cx);
    cx.run_until_parked();
    connected.send(
        &browser,
        &Control::TerminalAttach {
            terminal_id: crate::agent_mirror::terminal_id(terminal.entity_id()),
            stream_id: 3,
        },
    );
    cx.run_until_parked();
    assert!(!terminal.read_with(cx, |terminal, _| terminal.has_remote_tap()));
    assert!(browser.client.was_told_closed(connected.session_id));
}

#[gpui::test]
async fn file_and_ide_requests_are_declined_by_name(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = paired_and_connected(cx).await;
    connected.controls(&browser);
    connected.send(
        &browser,
        &Control::FileRead {
            request_id: 4,
            worktree_id: Some("1".into()),
            path: "/etc/passwd".into(),
        },
    );
    cx.run_until_parked();
    let controls = connected.controls(&browser);
    assert!(
        matches!(
            &controls[0],
            Control::Error { code, request_id: Some(4), .. } if code == UNSUPPORTED
        ),
        "{controls:?}"
    );
    // Declining a request does not end the session.
    connected.send(&browser, &Control::Ping);
    cx.run_until_parked();
    assert_eq!(connected.controls(&browser), vec![Control::Pong]);
}

#[gpui::test]
async fn a_message_that_does_not_authenticate_ends_the_session(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = paired_and_connected(cx).await;
    let mut sealed = connected
        .channel
        .seal_control(&Control::Ping)
        .expect("seals");
    let last = sealed.len() - 1;
    sealed[last] ^= 0x01;
    browser.client.send_binary(connected.session_id, &sealed);
    cx.run_until_parked();
    assert!(browser.client.was_told_closed(connected.session_id));
    assert!(rig.host.read_with(cx, |host, _| host.sessions().is_empty()));
}

#[gpui::test]
async fn the_kill_switch_disconnects_everyone_and_releases_the_display(cx: &mut TestAppContext) {
    let (rig, browser, connected) = paired_and_connected(cx).await;
    assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 1);
    assert_eq!(
        cx.display_wake_reasons().len(),
        1,
        "a device in control keeps the display awake"
    );
    assert!(rig.host.read_with(cx, |host, _| host.holds_display_awake()));
    let sessions = rig.host.read_with(cx, |host, _| host.sessions());
    assert_eq!(sessions[0].device_name, "Ada's browser");
    assert_eq!(sessions[0].device_id, "browser-1");

    rig.host.update(cx, |host, cx| host.disconnect_all(cx));
    cx.run_until_parked();

    assert!(browser.client.was_told_closed(connected.session_id));
    assert!(rig.host.read_with(cx, |host, _| host.sessions().is_empty()));
    assert!(cx.display_wake_reasons().is_empty());
    assert!(
        rig.relay.is_host_connected(),
        "the kill switch ends sessions, not the connection"
    );
}

#[gpui::test]
async fn the_users_own_display_setting_decides_whether_a_session_holds_the_display(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        cx.set_global(db::AppDatabase::test_new());
    });
    let rig_state = rig(cx, true).await;
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |content| content.keep_display_awake = Some(false));
        });
    });
    let browser = Browser::new(&rig_state, "browser-1");
    pair(&rig_state, &browser, cx);
    let mut connected = open_session(&rig_state, &browser, cx);
    connected.send(&browser, &hello(1));
    cx.run_until_parked();
    assert_eq!(
        rig_state
            .host
            .read_with(cx, |host, _| host.sessions().len()),
        1
    );
    assert!(cx.display_wake_reasons().is_empty());

    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |content| content.keep_display_awake = Some(true));
        });
    });
    cx.run_until_parked();
    assert_eq!(
        cx.display_wake_reasons().len(),
        1,
        "switching it on takes effect at once"
    );
}

#[gpui::test]
async fn a_device_that_sends_nothing_is_disconnected_after_the_idle_timeout(
    cx: &mut TestAppContext,
) {
    let (rig, browser, mut connected) = paired_and_connected(cx).await;
    set_remote_control(cx, true, Some(1));
    cx.run_until_parked();

    // Quiet, but alive: pings are not input.
    for _ in 0..4 {
        cx.executor().advance_clock(Duration::from_secs(15));
        connected.send(&browser, &Control::Ping);
        cx.run_until_parked();
    }
    assert!(
        browser.client.was_told_closed(connected.session_id),
        "a minute of pings and nothing else is a minute of silence"
    );
    assert!(rig.host.read_with(cx, |host, _| host.sessions().is_empty()));
}

/// Attaches the session to the only terminal and returns its id.
async fn attach_to_a_terminal(
    rig: &Rig,
    browser: &Browser,
    connected: &mut Connected,
    stream_id: u32,
    cx: &mut TestAppContext,
) -> String {
    let terminal_id = rig.terminals.read_with(cx, |terminals, cx| {
        terminals
            .summaries(cx)
            .first()
            .map(|summary| summary.id.clone())
            .expect("a terminal to attach to")
    });
    connected.send(
        browser,
        &Control::TerminalAttach {
            terminal_id: terminal_id.clone(),
            stream_id,
        },
    );
    cx.run_until_parked();
    connected.receive(browser);
    terminal_id
}

#[gpui::test]
async fn input_keeps_a_session_from_going_idle(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = paired_and_connected(cx).await;
    let _terminal = shell_terminal("sleep 600", cx).await;
    cx.run_until_parked();
    attach_to_a_terminal(&rig, &browser, &mut connected, 7, cx).await;
    set_remote_control(cx, true, Some(1));
    cx.run_until_parked();
    for _ in 0..8 {
        cx.executor().advance_clock(Duration::from_secs(20));
        connected.send_data(&browser, 7, b"x");
        cx.run_until_parked();
    }
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.sessions().len()),
        1,
        "typing is use"
    );
}

#[gpui::test]
async fn traffic_that_reaches_no_terminal_does_not_keep_a_session_alive(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = paired_and_connected(cx).await;
    set_remote_control(cx, true, Some(1));
    cx.run_until_parked();
    for _ in 0..4 {
        cx.executor().advance_clock(Duration::from_secs(20));
        connected.send(
            &browser,
            &Control::TerminalDetach {
                terminal_id: "terminal-nothing".into(),
            },
        );
        connected.send_data(&browser, 9, b"typed into nothing");
        connected.send(&browser, &Control::Ping);
        cx.run_until_parked();
    }
    assert!(
        browser.client.was_told_closed(connected.session_id),
        "none of that reached a terminal, so a minute of it is a minute of silence"
    );
    assert!(rig.host.read_with(cx, |host, _| host.sessions().is_empty()));
}

#[gpui::test]
async fn an_idle_timeout_of_zero_never_disconnects(cx: &mut TestAppContext) {
    let (rig, _browser, _connected) = paired_and_connected(cx).await;
    set_remote_control(cx, true, Some(0));
    cx.run_until_parked();
    cx.executor()
        .advance_clock(Duration::from_secs(60 * 60 * 24));
    cx.run_until_parked();
    assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 1);
}

#[gpui::test]
async fn a_handshake_never_finished_is_dropped_after_ten_seconds(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let session_id = browser.client.open(HOST_DEVICE, OpenMode::Session);
    cx.run_until_parked();
    assert!(!browser.client.was_told_closed(session_id));

    cx.executor().advance_clock(Duration::from_secs(16));
    cx.run_until_parked();
    assert!(browser.client.was_told_closed(session_id));
}

#[gpui::test]
async fn at_most_eight_handshakes_wait_and_the_ninth_is_refused(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let sessions: Vec<u32> = (0..9)
        .map(|_| browser.client.open(HOST_DEVICE, OpenMode::Session))
        .collect();
    cx.run_until_parked();
    let closed: Vec<u32> = sessions
        .iter()
        .copied()
        .filter(|session_id| browser.client.was_told_closed(*session_id))
        .collect();
    assert_eq!(closed, vec![sessions[8]]);
}

/// Starts a handshake for `browser` and returns the session and the handshake,
/// without ever proving the keys.
fn begin_handshake(rig: &Rig, browser: &Browser) -> (u32, Handshake) {
    let session_id = browser.client.open(HOST_DEVICE, OpenMode::Session);
    let host_key = rig.host_public_key.lock().expect("registered");
    let prologue = build_prologue(USER_ID, &browser.id, HOST_DEVICE).expect("a prologue");
    let mut handshake = Handshake::initiator(&HandshakeParameters {
        local_private_key: browser.keypair.private_key(),
        remote_public_key: &host_key,
        prologue: &prologue,
    })
    .expect("a handshake");
    browser.client.send_binary(
        session_id,
        &handshake.write_message(&[]).expect("message one"),
    );
    (session_id, handshake)
}

#[gpui::test]
async fn no_more_than_four_devices_are_in_control_at_once(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    for _ in 0..4 {
        let mut connected = open_session(&rig, &browser, cx);
        connected.send(&browser, &hello(1));
        cx.run_until_parked();
    }
    assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 4);

    let (fifth, _handshake) = begin_handshake(&rig, &browser);
    cx.run_until_parked();
    assert!(
        browser
            .client
            .take_binary()
            .iter()
            .all(|(session_id, _)| *session_id != fifth),
        "no reply to a fifth device"
    );
    assert!(browser.client.was_told_closed(fifth));
    assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 4);
}

#[gpui::test]
async fn peers_that_have_not_proven_the_keys_do_not_use_up_the_places(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    set_remote_control(cx, true, Some(0));
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);

    // A relay replaying first handshake messages: answered, never proven.
    let replays: Vec<u32> = (0..4).map(|_| begin_handshake(&rig, &browser).0).collect();
    cx.run_until_parked();
    assert!(rig.host.read_with(cx, |host, _| host.sessions().is_empty()));
    let (refused, _handshake) = begin_handshake(&rig, &browser);
    cx.run_until_parked();
    assert!(
        browser.client.was_told_closed(refused),
        "only so many unproven peers are answered at once"
    );

    // They are dropped on a deadline of their own, with the idle limit off.
    cx.executor().advance_clock(Duration::from_secs(16));
    cx.run_until_parked();
    for session_id in replays {
        assert!(browser.client.was_told_closed(session_id));
    }

    let mut connected = open_session(&rig, &browser, cx);
    connected.send(&browser, &hello(1));
    cx.run_until_parked();
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.sessions().len()),
        1,
        "a real device is not locked out by the replays"
    );
}

#[gpui::test]
async fn a_session_that_never_proves_the_keys_ends_even_when_the_idle_limit_is_off(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, true).await;
    set_remote_control(cx, true, Some(0));
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let connected = open_session(&rig, &browser, cx);
    cx.executor().advance_clock(Duration::from_secs(16));
    cx.run_until_parked();
    assert!(browser.client.was_told_closed(connected.session_id));
}

#[gpui::test]
async fn only_as_many_peers_as_there_are_places_can_prove_themselves(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let mut confirmed = Vec::new();
    for _ in 0..2 {
        let mut connected = open_session(&rig, &browser, cx);
        connected.send(&browser, &hello(1));
        cx.run_until_parked();
        confirmed.push(connected);
    }
    // Three more are answered while only two places are taken, so all of them
    // are allowed to try.
    let mut waiting: Vec<Connected> = (0..3).map(|_| open_session(&rig, &browser, cx)).collect();
    for connected in &mut waiting {
        connected.send(&browser, &hello(1));
        cx.run_until_parked();
    }
    assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 4);
    assert!(
        browser.client.was_told_closed(waiting[2].session_id),
        "the last to prove itself found every place taken"
    );
}

#[gpui::test]
async fn a_mismatched_commitment_ends_pairing_and_three_of_them_lock_it(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    for attempt in 0..3 {
        let session_id = browser.client.open(HOST_DEVICE, OpenMode::Pair);
        let (_requester, request) = PairingRequester::start(
            *browser.keypair.public_key(),
            random_nonce().expect("randomness"),
        );
        browser
            .client
            .send_text(&encode_pairing(session_id, &request).expect("encodes"));
        cx.run_until_parked();
        assert_eq!(
            browser.client.take_pairing().len(),
            1,
            "attempt {attempt} was answered"
        );
        browser.client.send_text(
            &encode_pairing(
                session_id,
                &PairingMessage::PairReveal { nonce: [0xee; 32] },
            )
            .expect("encodes"),
        );
        cx.run_until_parked();
        assert!(browser.client.was_told_closed(session_id));
        assert!(
            rig.host
                .read_with(cx, |host, _| host.pending_pairing().is_none()),
            "no question is shown"
        );
        // The hourly allowance is not what is being measured: a new exchange
        // needs the gate's two minutes to pass.
        cx.executor().advance_clock(Duration::from_secs(121));
        cx.run_until_parked();
    }
    assert!(rig.host.read_with(cx, |host, _| host.pairing_locked_out()));

    let session_id = browser.client.open(HOST_DEVICE, OpenMode::Pair);
    let (_requester, request) = PairingRequester::start(
        *browser.keypair.public_key(),
        random_nonce().expect("randomness"),
    );
    browser
        .client
        .send_text(&encode_pairing(session_id, &request).expect("encodes"));
    cx.run_until_parked();
    assert!(
        browser.client.take_pairing().is_empty(),
        "a locked host answers nobody"
    );
    assert!(browser.client.was_told_closed(session_id));

    rig.host.update(cx, |host, cx| host.unlock_pairing(cx));
    assert!(!rig.host.read_with(cx, |host, _| host.pairing_locked_out()));
}

#[gpui::test]
async fn rejecting_the_codes_pins_nothing(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    let session_id = browser.client.open(HOST_DEVICE, OpenMode::Pair);
    let nonce = random_nonce().expect("randomness");
    let (requester, request) = PairingRequester::start(*browser.keypair.public_key(), nonce);
    browser
        .client
        .send_text(&encode_pairing(session_id, &request).expect("encodes"));
    cx.run_until_parked();
    let PairingMessage::PairAccept {
        public_key,
        nonce: host_nonce,
    } = browser.client.take_pairing().pop().expect("accepted")
    else {
        panic!("expected an accept");
    };
    let (reveal, _) = requester
        .receive_accept(public_key, host_nonce)
        .expect("accepts");
    browser
        .client
        .send_text(&encode_pairing(session_id, &reveal).expect("encodes"));
    cx.run_until_parked();

    rig.host
        .update(cx, |host, cx| host.decide_pairing(session_id, false, cx));
    cx.run_until_parked();
    assert!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().is_empty())
    );
    assert!(browser.client.was_told_closed(session_id));

    let second = open_session_attempt(&browser, cx);
    assert!(second, "an untrusted device is told no");
}

fn open_session_attempt(browser: &Browser, cx: &mut TestAppContext) -> bool {
    let session_id = browser.client.open(HOST_DEVICE, OpenMode::Session);
    cx.run_until_parked();
    browser.client.was_told_closed(session_id)
}

#[gpui::test]
async fn an_unanswered_pairing_question_expires_after_two_minutes(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    let session_id = browser.client.open(HOST_DEVICE, OpenMode::Pair);
    let (requester, request) = PairingRequester::start(
        *browser.keypair.public_key(),
        random_nonce().expect("randomness"),
    );
    browser
        .client
        .send_text(&encode_pairing(session_id, &request).expect("encodes"));
    cx.run_until_parked();
    let PairingMessage::PairAccept { public_key, nonce } =
        browser.client.take_pairing().pop().expect("accepted")
    else {
        panic!("expected an accept");
    };
    let (reveal, _) = requester
        .receive_accept(public_key, nonce)
        .expect("accepts");
    browser
        .client
        .send_text(&encode_pairing(session_id, &reveal).expect("encodes"));
    cx.run_until_parked();
    assert!(
        rig.host
            .read_with(cx, |host, _| host.pending_pairing().is_some())
    );

    cx.executor().advance_clock(Duration::from_secs(125));
    cx.run_until_parked();
    assert!(
        rig.host
            .read_with(cx, |host, _| host.pending_pairing().is_none())
    );
    assert!(browser.client.was_told_closed(session_id));
}

#[gpui::test]
async fn forgetting_a_device_ends_its_session_and_it_cannot_come_back(cx: &mut TestAppContext) {
    let (rig, browser, connected) = paired_and_connected(cx).await;
    rig.host
        .update(cx, |host, cx| host.forget_device("browser-1", cx));
    cx.run_until_parked();
    assert!(browser.client.was_told_closed(connected.session_id));
    assert!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().is_empty())
    );
    assert!(open_session_attempt(&browser, cx));
}

#[gpui::test]
async fn the_relay_announcing_a_revoked_device_removes_its_pin_and_its_session(
    cx: &mut TestAppContext,
) {
    let (rig, browser, connected) = paired_and_connected(cx).await;
    rig.relay.revoke_device("browser-1");
    cx.run_until_parked();
    assert!(browser.client.was_told_closed(connected.session_id));
    assert!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().is_empty())
    );
    let reloaded = cx
        .update(|cx| TrustStore::load(USER_ID.to_string(), cx))
        .await;
    assert!(
        reloaded.devices().is_empty(),
        "the pin is gone from disk too"
    );
}

#[gpui::test]
async fn revocation_of_this_device_ends_everything_and_never_reconnects(cx: &mut TestAppContext) {
    let (rig, browser, connected) = paired_and_connected(cx).await;
    let attempts = rig.relay.connection_attempts();
    rig.relay.drop_host_connection(Some(4403), "device_revoked");
    cx.run_until_parked();

    assert!(browser.client.was_told_closed(connected.session_id));
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.state().clone()),
        HostState::Halted(StopReason::Revoked)
    );
    assert!(rig.host.read_with(cx, |host, _| host.sessions().is_empty()));
    assert!(!rig.host.read_with(cx, |host, _| host.has_relay_client()));
    assert!(cx.display_wake_reasons().is_empty());

    let reloaded = cx
        .update(|cx| TrustStore::load(USER_ID.to_string(), cx))
        .await;
    assert!(
        reloaded.devices().is_empty(),
        "a revoked device trusts nobody"
    );
    assert!(
        rig.keychain.stored_passwords().is_empty(),
        "and its key is removed from the keychain"
    );

    cx.executor().advance_clock(Duration::from_secs(3600));
    cx.run_until_parked();
    assert_eq!(
        rig.relay.connection_attempts(),
        attempts,
        "it must not knock again"
    );

    // Switching it off and on again is the way back.
    set_remote_control(cx, false, None);
    cx.run_until_parked();
    set_remote_control(cx, true, None);
    cx.run_until_parked();
    assert_eq!(rig.relay.connection_attempts(), attempts + 1);
}

#[gpui::test]
async fn a_dropped_connection_ends_sessions_and_reconnects_with_backoff(cx: &mut TestAppContext) {
    let (rig, _browser, _connected) = paired_and_connected(cx).await;
    rig.relay.drop_host_connection(None, "");
    cx.run_until_parked();
    assert!(rig.host.read_with(cx, |host, _| host.sessions().is_empty()));
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.state().clone()),
        HostState::Reconnecting
    );
    cx.executor().advance_clock(Duration::from_millis(1300));
    cx.run_until_parked();
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.state().clone()),
        HostState::Online
    );
    assert!(
        rig.host.read_with(cx, |host, _| host.has_relay_client()
            && host.sessions().is_empty()),
        "sessions do not survive the socket"
    );
}

#[gpui::test]
async fn an_agent_tab_opening_is_announced_to_connected_devices(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = paired_and_connected(cx).await;
    connected.controls(&browser);
    cx.update(|cx| {
        editor::init(cx);
        project::DisableAiSettings::register(cx);
    });
    let fs = fs::FakeFs::new(cx.executor());
    let project = project::Project::test(fs, [], cx).await;
    let (multi, cx) =
        cx.add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
    let workspace = multi.read_with(cx, |multi, _| multi.workspace().clone());
    workspace.update_in(cx, |workspace, window, cx| {
        agent_ui::AgentView::open(workspace, project::CLAUDE_CODE_AGENT_ID, None, window, cx);
    });
    cx.run_until_parked();

    let updates: Vec<_> = connected
        .controls(&browser)
        .into_iter()
        .filter_map(|control| match control {
            Control::AgentUpdate { agent } => Some(agent),
            _ => None,
        })
        .collect();
    assert_eq!(updates.len(), 1, "{updates:?}");
    assert_eq!(updates[0].name, "Claude Code");
}

/// Drives a pairing as far as the question being asked, and returns its session.
fn begin_pairing_until_asked(rig: &Rig, browser: &Browser, cx: &mut TestAppContext) -> u32 {
    let session_id = browser.client.open(HOST_DEVICE, OpenMode::Pair);
    let (requester, request) = PairingRequester::start(
        *browser.keypair.public_key(),
        random_nonce().expect("randomness"),
    );
    browser
        .client
        .send_text(&encode_pairing(session_id, &request).expect("encodes"));
    cx.run_until_parked();
    let PairingMessage::PairAccept { public_key, nonce } =
        browser.client.take_pairing().pop().expect("accepted")
    else {
        panic!("expected an accept");
    };
    let (reveal, _) = requester
        .receive_accept(public_key, nonce)
        .expect("accepts");
    browser
        .client
        .send_text(&encode_pairing(session_id, &reveal).expect("encodes"));
    cx.run_until_parked();
    assert!(
        rig.host
            .read_with(cx, |host, _| host.pending_pairing().is_some()),
        "the person is being asked"
    );
    session_id
}

fn record_host_events(
    rig: &Rig,
    cx: &mut TestAppContext,
) -> (
    std::rc::Rc<std::cell::RefCell<Vec<crate::RemoteHostEvent>>>,
    gpui::Subscription,
) {
    let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let subscription = cx.update(|cx| {
        let events = events.clone();
        cx.subscribe(&rig.host, move |_, event: &crate::RemoteHostEvent, _| {
            events.borrow_mut().push(event.clone());
        })
    });
    (events, subscription)
}

fn problems(events: &std::cell::RefCell<Vec<crate::RemoteHostEvent>>) -> Vec<(String, bool)> {
    events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            crate::RemoteHostEvent::Problem { message, retry } => Some((message.clone(), *retry)),
            _ => None,
        })
        .collect()
}

#[gpui::test]
async fn a_revoked_sign_in_halts_remote_control_but_keeps_the_key_and_the_pins(
    cx: &mut TestAppContext,
) {
    let (rig, browser, connected) = paired_and_connected(cx).await;
    let (events, _subscription) = record_host_events(&rig, cx);
    let attempts = rig.relay.connection_attempts();
    rig.relay
        .drop_host_connection(Some(4403), "session_revoked");
    cx.run_until_parked();

    assert!(browser.client.was_told_closed(connected.session_id));
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.state().clone()),
        HostState::Halted(StopReason::SessionEnded)
    );
    assert!(!rig.host.read_with(cx, |host, _| host.has_relay_client()));
    let reloaded = cx
        .update(|cx| TrustStore::load(USER_ID.to_string(), cx))
        .await;
    assert_eq!(
        reloaded.devices().len(),
        1,
        "signing in again must find every paired device still trusted"
    );
    assert_eq!(
        rig.keychain.stored_passwords().len(),
        1,
        "the device key is not deleted"
    );
    assert_eq!(problems(&events).len(), 1, "the person is told");
    cx.executor().advance_clock(Duration::from_secs(3600));
    cx.run_until_parked();
    assert_eq!(
        rig.relay.connection_attempts(),
        attempts,
        "it does not knock"
    );
}

#[gpui::test]
async fn stopping_because_of_the_relay_is_said_out_loud(cx: &mut TestAppContext) {
    for (code, reason) in [(4429, "quota_exceeded"), (4409, "replaced")] {
        let rig = rig(cx, true).await;
        let (events, _subscription) = record_host_events(&rig, cx);
        rig.relay.drop_host_connection(Some(code), reason);
        cx.run_until_parked();
        let said = problems(&events);
        assert_eq!(said.len(), 1, "code {code}: {said:?}");
        assert!(!said[0].1, "nothing to retry: it is not a start-up failure");
    }
}

#[gpui::test]
async fn a_device_key_that_cannot_be_read_is_reported_and_can_be_retried(cx: &mut TestAppContext) {
    let rig = rig(cx, false).await;
    rig.keychain.fail_reads();
    let (events, _subscription) = record_host_events(&rig, cx);
    set_remote_control(cx, true, None);
    cx.run_until_parked();
    assert!(matches!(
        rig.host.read_with(cx, |host, _| host.state().clone()),
        HostState::Failed(_)
    ));
    let said = problems(&events);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].1, "trying again can help");

    rig.keychain.recover();
    rig.host.update(cx, |host, cx| host.retry(cx));
    cx.run_until_parked();
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.state().clone()),
        HostState::Online
    );
}

#[gpui::test]
async fn turning_it_on_with_nobody_signed_in_is_said_out_loud_once(cx: &mut TestAppContext) {
    let rig = rig_with_status(cx, false, AccountStatus::SignedOut).await;
    let (events, _subscription) = record_host_events(&rig, cx);
    set_remote_control(cx, true, None);
    cx.run_until_parked();
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.state().clone()),
        HostState::SignedOut
    );
    assert_eq!(problems(&events).len(), 1);

    set_remote_control(cx, true, Some(5));
    cx.run_until_parked();
    assert_eq!(
        problems(&events).len(),
        1,
        "an unrelated settings change does not repeat it"
    );
}

#[gpui::test]
async fn a_reveal_from_a_stranger_does_not_end_the_question_on_screen(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    let stranger = Browser::new(&rig, "browser-2");
    let session_id = begin_pairing_until_asked(&rig, &browser, cx);

    let foreign = stranger.client.open(HOST_DEVICE, OpenMode::Pair);
    stranger.client.send_text(
        &encode_pairing(foreign, &PairingMessage::PairReveal { nonce: [7; 32] }).expect("encodes"),
    );
    cx.run_until_parked();

    let pending = rig
        .host
        .read_with(cx, |host, _| host.pending_pairing().cloned())
        .expect("the person is still being asked");
    assert_eq!(pending.session_id, session_id);
    assert!(
        stranger.client.was_told_closed(foreign),
        "the stranger is turned away"
    );
    assert!(!browser.client.was_told_closed(session_id));
    assert!(!rig.host.read_with(cx, |host, _| host.pairing_locked_out()));

    rig.host
        .update(cx, |host, cx| host.decide_pairing(session_id, true, cx));
    cx.run_until_parked();
    assert_eq!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().len()),
        1,
        "and the exchange could still be completed"
    );
}

#[gpui::test]
async fn a_device_that_sees_pairing_close_can_open_a_session_straight_away(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    let session_id = begin_pairing_until_asked(&rig, &browser, cx);
    rig.host
        .update(cx, |host, cx| host.decide_pairing(session_id, true, cx));
    cx.run_until_parked();
    // The moment the browser sees the pair session close, the new pin must
    // already be usable by the responder: the handshake goes out right after.
    assert!(browser.client.was_told_closed(session_id));
    let mut connected = open_session(&rig, &browser, cx);
    connected.send(&browser, &hello(1));
    cx.run_until_parked();
    assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 1);
}

#[gpui::test]
async fn an_answer_for_another_session_changes_nothing(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    let session_id = begin_pairing_until_asked(&rig, &browser, cx);
    rig.host.update(cx, |host, cx| {
        host.decide_pairing(session_id + 100, true, cx)
    });
    cx.run_until_parked();
    assert!(
        rig.host
            .read_with(cx, |host, _| host.pending_pairing().is_some()),
        "the question is still on screen"
    );
    assert!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().is_empty())
    );
}

#[gpui::test]
async fn announcing_a_pairing_session_a_second_time_does_not_hijack_it(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    let session_id = browser.client.open(HOST_DEVICE, OpenMode::Pair);
    let nonce = random_nonce().expect("randomness");
    let (requester, request) = PairingRequester::start(*browser.keypair.public_key(), nonce);
    browser
        .client
        .send_text(&encode_pairing(session_id, &request).expect("encodes"));
    cx.run_until_parked();
    let PairingMessage::PairAccept { public_key, nonce } =
        browser.client.take_pairing().pop().expect("accepted")
    else {
        panic!("expected an accept");
    };

    // The relay says the same id is a handshake session now.
    browser.client.send_text(
        &serde_json::json!({
            "t": "opened", "sid": session_id, "peer": browser.id,
            "peerKind": "web", "mode": "session"
        })
        .to_string(),
    );
    cx.run_until_parked();

    let (reveal, _) = requester
        .receive_accept(public_key, nonce)
        .expect("accepts");
    browser
        .client
        .send_text(&encode_pairing(session_id, &reveal).expect("encodes"));
    cx.run_until_parked();
    assert!(
        rig.host
            .read_with(cx, |host, _| host.pending_pairing().is_some()),
        "pairing carries on as what it was"
    );
}

#[gpui::test]
async fn a_lockout_survives_the_connection_dropping_and_coming_back(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    for _ in 0..3 {
        let session_id = browser.client.open(HOST_DEVICE, OpenMode::Pair);
        let (_requester, request) = PairingRequester::start(
            *browser.keypair.public_key(),
            random_nonce().expect("randomness"),
        );
        browser
            .client
            .send_text(&encode_pairing(session_id, &request).expect("encodes"));
        cx.run_until_parked();
        browser.client.send_text(
            &encode_pairing(
                session_id,
                &PairingMessage::PairReveal { nonce: [0xee; 32] },
            )
            .expect("encodes"),
        );
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_secs(121));
        cx.run_until_parked();
    }
    assert!(rig.host.read_with(cx, |host, _| host.pairing_locked_out()));

    rig.relay.drop_host_connection(None, "");
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    assert!(rig.host.read_with(cx, |host, _| host.has_relay_client()));
    assert!(
        rig.host.read_with(cx, |host, _| host.pairing_locked_out()),
        "dropping the connection must not be a way to clear a lockout"
    );
}

#[gpui::test]
async fn turning_it_off_from_the_action_disconnects_at_once_and_stays_off(cx: &mut TestAppContext) {
    let (rig, browser, connected) = paired_and_connected(cx).await;
    cx.update(|cx| {
        <dyn fs::Fs>::set_global(fs::FakeFs::new(cx.background_executor().clone()), cx);
        RemoteHost::set_global(rig.host.clone(), cx);
    });
    let (events, _subscription) = record_host_events(&rig, cx);

    cx.update(|cx| crate::disable_remote_control(cx));
    // Nothing has been awaited: the settings file has not been touched yet.
    assert!(
        rig.host.read_with(cx, |host, _| host.sessions().is_empty()),
        "the device is out of control before the setting is written"
    );
    assert!(!rig.host.read_with(cx, |host, _| host.has_relay_client()));
    cx.run_until_parked();
    assert!(browser.client.was_told_closed(connected.session_id));

    cx.run_until_parked();
    assert!(
        !cx.update(|cx| crate::RemoteControlSettings::is_enabled(cx)),
        "and the setting was written"
    );
    drop(events);
}

mod with_a_window {
    use super::*;
    use crate::{indicator::RemoteControlIndicator, pairing_modal::PairingModal};
    use gpui::VisualTestContext;
    use remote_relay_client::{OpenMode, encode_pairing};
    use workspace::Workspace;

    async fn window<'a>(
        cx: &'a mut TestAppContext,
        rig: &Rig,
    ) -> (Entity<Workspace>, &'a mut VisualTestContext) {
        cx.update(|cx| {
            editor::init(cx);
            project::DisableAiSettings::register(cx);
        });
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (multi, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi.read_with(cx, |multi, _| multi.workspace().clone());
        workspace.update_in(cx, |workspace, window, cx| {
            crate::register_workspace(workspace, &rig.host, window, cx);
        });
        (workspace, cx)
    }

    /// Drives a pairing as far as the question being asked.
    fn ask_the_question(rig: &Rig, browser: &Browser, cx: &mut VisualTestContext) -> u32 {
        let session_id = browser.client.open(HOST_DEVICE, OpenMode::Pair);
        let (requester, request) = PairingRequester::start(
            *browser.keypair.public_key(),
            random_nonce().expect("randomness"),
        );
        browser
            .client
            .send_text(&encode_pairing(session_id, &request).expect("encodes"));
        cx.run_until_parked();
        let PairingMessage::PairAccept { public_key, nonce } =
            browser.client.take_pairing().pop().expect("accepted")
        else {
            panic!("expected an accept");
        };
        let (reveal, _) = requester
            .receive_accept(public_key, nonce)
            .expect("accepts");
        browser
            .client
            .send_text(&encode_pairing(session_id, &reveal).expect("encodes"));
        cx.run_until_parked();
        assert!(
            rig.host
                .read_with(cx, |host, _| host.pending_pairing().is_some())
        );
        session_id
    }

    #[gpui::test]
    async fn the_question_is_put_on_screen_once_and_answering_it_closes_it(
        cx: &mut TestAppContext,
    ) {
        let rig = rig(cx, true).await;
        let (workspace, cx) = window(cx, &rig).await;
        let browser = Browser::new(&rig, "browser-1");
        let session_id = ask_the_question(&rig, &browser, cx);

        let modal = workspace
            .read_with(cx, |workspace, cx| {
                workspace.active_modal::<PairingModal>(cx)
            })
            .expect("the person is asked");
        assert!(
            !rig.host
                .update(cx, |host, _| host.claim_pairing_presentation(session_id)),
            "a second window must not ask the same question again"
        );

        rig.host
            .update(cx, |host, cx| host.decide_pairing(session_id, true, cx));
        cx.run_until_parked();
        assert!(
            workspace
                .read_with(cx, |workspace, cx| workspace
                    .active_modal::<PairingModal>(cx))
                .is_none(),
            "answered, so gone"
        );
        assert_eq!(
            rig.host
                .read_with(cx, |host, _| host.trusted_devices().len()),
            1
        );
        drop(modal);
    }

    #[gpui::test]
    async fn closing_the_question_without_answering_is_answering_no(cx: &mut TestAppContext) {
        let rig = rig(cx, true).await;
        let (workspace, cx) = window(cx, &rig).await;
        let browser = Browser::new(&rig, "browser-1");
        let session_id = ask_the_question(&rig, &browser, cx);

        workspace.update_in(cx, |workspace, window, cx| {
            workspace.hide_modal(window, cx);
        });
        cx.run_until_parked();

        assert!(
            rig.host
                .read_with(cx, |host, _| host.pending_pairing().is_none())
        );
        assert!(
            rig.host
                .read_with(cx, |host, _| host.trusted_devices().is_empty())
        );
        assert!(browser.client.was_told_closed(session_id));
    }

    #[gpui::test]
    async fn a_question_that_expires_takes_its_window_with_it(cx: &mut TestAppContext) {
        let rig = rig(cx, true).await;
        let (workspace, cx) = window(cx, &rig).await;
        let browser = Browser::new(&rig, "browser-1");
        ask_the_question(&rig, &browser, cx);
        assert!(
            workspace
                .read_with(cx, |workspace, cx| workspace
                    .active_modal::<PairingModal>(cx))
                .is_some()
        );

        cx.executor().advance_clock(Duration::from_secs(125));
        cx.run_until_parked();
        assert!(
            workspace
                .read_with(cx, |workspace, cx| workspace
                    .active_modal::<PairingModal>(cx))
                .is_none()
        );
    }

    #[gpui::test]
    async fn the_indicator_is_in_the_status_bar_and_draws_with_a_device_in_control(
        cx: &mut TestAppContext,
    ) {
        let rig = rig(cx, true).await;
        let (workspace, cx) = window(cx, &rig).await;
        let indicator = workspace.read_with(cx, |workspace, cx| {
            workspace
                .status_bar()
                .read(cx)
                .item_of_type::<RemoteControlIndicator>()
        });
        assert!(
            indicator.is_some(),
            "registered outside the table of hideable items"
        );

        let browser = Browser::new(&rig, "browser-1");
        let host_key = rig.host_public_key.lock().expect("registered");
        // A device in control: pair, connect, say hello.
        let nonce = random_nonce().expect("randomness");
        let (requester, request) = PairingRequester::start(*browser.keypair.public_key(), nonce);
        let pair_session = browser.client.open(HOST_DEVICE, OpenMode::Pair);
        browser
            .client
            .send_text(&encode_pairing(pair_session, &request).expect("encodes"));
        cx.run_until_parked();
        let PairingMessage::PairAccept { public_key, nonce } =
            browser.client.take_pairing().pop().expect("accepted")
        else {
            panic!("expected an accept");
        };
        let (reveal, _) = requester
            .receive_accept(public_key, nonce)
            .expect("accepts");
        browser
            .client
            .send_text(&encode_pairing(pair_session, &reveal).expect("encodes"));
        cx.run_until_parked();
        rig.host
            .update(cx, |host, cx| host.decide_pairing(pair_session, true, cx));
        cx.run_until_parked();

        let session_id = browser.client.open(HOST_DEVICE, OpenMode::Session);
        let prologue = build_prologue(USER_ID, &browser.id, HOST_DEVICE).expect("a prologue");
        let mut handshake = Handshake::initiator(&HandshakeParameters {
            local_private_key: browser.keypair.private_key(),
            remote_public_key: &host_key,
            prologue: &prologue,
        })
        .expect("a handshake");
        browser.client.send_binary(
            session_id,
            &handshake.write_message(&[]).expect("message one"),
        );
        cx.run_until_parked();
        let (_, reply) = browser.client.take_binary().pop().expect("a reply");
        handshake.read_message(&reply).expect("message two");
        let mut channel = SecureChannel::new(handshake.into_session().expect("a session"));
        browser
            .client
            .send_binary(session_id, &channel.seal_control(&hello(1)).expect("seals"));
        cx.run_until_parked();

        assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 1);
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }
    #[gpui::test]
    async fn the_status_bar_stays_while_a_device_is_in_control_whatever_the_setting_says(
        cx: &mut TestAppContext,
    ) {
        let rig = rig(cx, true).await;
        let (workspace, cx) = window(cx, &rig).await;
        cx.update_global(|store: &mut SettingsStore, cx| {
            store.update_user_settings(cx, |settings| {
                settings.status_bar.get_or_insert_default().show = Some(false);
            });
        });
        assert!(!workspace.read_with(cx, |workspace, cx| workspace.status_bar_visible(cx)));

        let browser = Browser::new(&rig, "browser-1");
        pair(&rig, &browser, cx);
        let mut connected = open_session(&rig, &browser, cx);
        connected.send(&browser, &hello(1));
        cx.run_until_parked();
        assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 1);
        assert!(
            workspace.read_with(cx, |workspace, cx| workspace.status_bar_visible(cx)),
            "a hidden status bar would hide the only sign that someone is in control"
        );

        rig.host.update(cx, |host, cx| host.disconnect_all(cx));
        cx.run_until_parked();
        assert!(
            !workspace.read_with(cx, |workspace, cx| workspace.status_bar_visible(cx)),
            "and it goes back to what the setting says when nobody is"
        );
    }
}

#[gpui::test]
async fn a_host_turned_off_by_hand_stays_off_while_the_setting_still_reads_on(
    cx: &mut TestAppContext,
) {
    let (rig, _browser, _connected) = paired_and_connected(cx).await;
    // The write failed, or another layer of settings says on.
    rig.host.update(cx, |host, cx| host.turn_off(cx));
    set_remote_control(cx, true, Some(15));
    cx.run_until_parked();
    assert!(!rig.host.read_with(cx, |host, _| host.has_relay_client()));

    set_remote_control(cx, false, None);
    cx.run_until_parked();
    set_remote_control(cx, true, None);
    cx.run_until_parked();
    assert!(
        rig.host.read_with(cx, |host, _| host.has_relay_client()),
        "seeing the setting off and then on again is how it comes back"
    );
}

#[cfg(unix)]
mod project_server {
    use super::*;
    use crate::ide_bridge::{IdeLaunch, IdeLaunchOverride};
    use std::{os::unix::fs::PermissionsExt as _, path::Path};

    /// A stand-in for the project server: it records that it ran and how, starts
    /// a long-lived process in the place the real server's daemon would be, and
    /// then does `ending`.
    struct Fixture {
        directory: tempfile::TempDir,
    }

    impl Fixture {
        fn new(ending: &str, cx: &mut TestAppContext) -> Self {
            let directory = tempfile::tempdir().expect("a temp dir");
            let root = directory.path();
            let state = root.join("state");
            std::fs::create_dir_all(&state).expect("a state dir");
            let program = root.join("remote_server");
            let script = format!(
                "#!/bin/sh\n\
                 echo $$ > \"{root}/proxy-$3.pid\"\n\
                 echo \"$@\" > \"{root}/args-$3\"\n\
                 touch \"{root}/started\"\n\
                 mkdir -p \"{state}/$3\"\n\
                 sh -c 'sleep 1000; :' daemon \"{state}/$3/server.pid\" >/dev/null 2>&1 </dev/null &\n\
                 echo $! > \"{state}/$3/server.pid\"\n\
                 {ending}\n",
                root = root.display(),
                state = state.display(),
            );
            std::fs::write(&program, script).expect("a script");
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
                .expect("executable");
            cx.update(|cx| {
                cx.set_global(IdeLaunchOverride(IdeLaunch {
                    program,
                    state_dir: state,
                }));
            });
            Self { directory }
        }

        fn path(&self) -> &Path {
            self.directory.path()
        }

        fn started(&self) -> bool {
            self.path().join("started").exists()
        }

        /// The identifiers the stand-in was started with, one per launch.
        fn identifiers(&self) -> Vec<String> {
            let mut found: Vec<String> = std::fs::read_dir(self.path())
                .expect("the fixture directory")
                .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                .filter_map(|name| name.strip_prefix("args-").map(str::to_string))
                .collect();
            found.sort();
            found
        }

        fn identifier(&self) -> String {
            self.identifiers()
                .into_iter()
                .next()
                .expect("the project server was started")
        }

        fn pid_of(&self, file: &Path) -> Option<String> {
            std::fs::read_to_string(file)
                .ok()
                .map(|text| text.trim().to_string())
        }
    }

    fn is_running(pid: &str) -> bool {
        let output = smol::block_on(
            util::command::new_command("ps")
                .args(["-o", "stat=", "-p", pid])
                .output(),
        )
        .expect("ps runs");
        let state = String::from_utf8_lossy(&output.stdout);
        let state = state.trim();
        // A process that was killed and not yet reaped is dead for our purposes.
        !state.is_empty() && !state.starts_with('Z')
    }

    async fn wait_for(cx: &mut TestAppContext, mut done: impl FnMut() -> bool) -> bool {
        for _ in 0..500 {
            cx.background_executor
                .timer(Duration::from_millis(10))
                .await;
            cx.run_until_parked();
            if done() {
                return true;
            }
        }
        false
    }

    /// A paired, greeted browser, with the stand-in as the project server.
    async fn session(ending: &str, cx: &mut TestAppContext) -> (Rig, Browser, Connected, Fixture) {
        let rig = rig(cx, true).await;
        let fixture = Fixture::new(ending, cx);
        let browser = Browser::new(&rig, "browser-1");
        pair(&rig, &browser, cx);
        let mut connected = open_session(&rig, &browser, cx);
        connected.send(&browser, &hello(1));
        cx.run_until_parked();
        (rig, browser, connected, fixture)
    }

    fn open_request(request_id: u32, stream_id: u32) -> Control {
        Control::IdeOpen {
            request_id,
            path: String::new(),
            line: None,
            stream_id: Some(stream_id),
            app_version: Some("9.9.9".into()),
            proto_version: Some(rpc::PROTOCOL_VERSION),
        }
    }

    fn data_on(
        received: Vec<(InnerKind, u32, Vec<u8>)>,
        stream: u32,
    ) -> Vec<(InnerKind, u32, Vec<u8>)> {
        received
            .into_iter()
            .filter(|(kind, id, _)| *kind == InnerKind::Data && *id == stream)
            .collect()
    }

    /// Opens the project stream and returns the host's answer.
    fn open(
        browser: &Browser,
        connected: &mut Connected,
        request_id: u32,
        stream_id: u32,
        cx: &mut TestAppContext,
    ) -> Control {
        connected.send(browser, &open_request(request_id, stream_id));
        cx.run_until_parked();
        connected
            .controls(browser)
            .into_iter()
            .find(|control| matches!(control, Control::IdeOpened { .. } | Control::Error { .. }))
            .expect("an answer to the request")
    }

    async fn echoed(
        browser: &Browser,
        connected: &mut Connected,
        stream_id: u32,
        sent: &[u8],
        cx: &mut TestAppContext,
    ) -> bool {
        connected.send_data(browser, stream_id, sent);
        let mut seen = Vec::new();
        wait_for(cx, || {
            for (_, _, payload) in data_on(connected.receive(browser), stream_id) {
                seen.extend(payload);
            }
            seen.windows(sent.len()).any(|window| window == sent)
        })
        .await
    }

    #[gpui::test]
    async fn the_ide_capability_is_advertised_when_a_project_server_exists(
        cx: &mut TestAppContext,
    ) {
        let (_rig, browser, mut connected, _fixture) = session("exec cat", cx).await;
        let controls = connected.controls(&browser);
        assert!(
            matches!(
                &controls[0],
                Control::HelloAck { capabilities, .. }
                    if capabilities == &vec!["terminal".to_string(), "ide".to_string()]
            ),
            "{controls:?}"
        );
    }

    #[gpui::test]
    async fn a_project_request_without_a_project_server_is_declined_by_name(
        cx: &mut TestAppContext,
    ) {
        let (_rig, browser, mut connected) = paired_and_connected(cx).await;
        connected.controls(&browser);
        let answer = open(&browser, &mut connected, 8, 5, cx);
        assert!(
            matches!(
                &answer,
                Control::Error { code, request_id: Some(8), .. } if code == UNSUPPORTED
            ),
            "{answer:?}"
        );
    }

    #[gpui::test]
    async fn the_project_stream_carries_bytes_both_ways(cx: &mut TestAppContext) {
        let (_rig, browser, mut connected, fixture) = session("exec cat", cx).await;
        connected.controls(&browser);
        assert!(!fixture.started(), "nothing runs before it is asked for");

        let answer = open(&browser, &mut connected, 3, 5, cx);
        let Control::IdeOpened {
            request_id,
            stream_id,
            path_style,
            shell,
            default_shell,
        } = answer
        else {
            panic!("expected ide_opened, got {answer:?}");
        };
        assert_eq!((request_id, stream_id), (3, Some(5)));
        assert_eq!(path_style.as_deref(), Some("posix"));
        assert!(shell.is_some() && default_shell.is_some());

        assert!(echoed(&browser, &mut connected, 5, b"hello project\n", cx).await);
        let identifier = fixture.identifier();
        assert!(identifier.starts_with("relay-5-"), "{identifier}");
        let arguments = std::fs::read_to_string(fixture.path().join(format!("args-{identifier}")))
            .expect("the server was started with its arguments");
        assert_eq!(arguments.trim(), format!("proxy --identifier {identifier}"));
    }

    #[gpui::test]
    async fn data_sent_before_the_stream_is_opened_goes_nowhere(cx: &mut TestAppContext) {
        let (_rig, browser, mut connected, fixture) = session("exec cat", cx).await;
        connected.controls(&browser);
        connected.send_data(&browser, 5, b"too early\n");
        cx.run_until_parked();
        assert!(!fixture.started());

        open(&browser, &mut connected, 3, 5, cx);
        assert!(echoed(&browser, &mut connected, 5, b"on time\n", cx).await);
        // Everything the host sent on the stream: only what was sent after.
        connected.send_data(&browser, 5, b"again\n");
        let mut seen = Vec::new();
        wait_for(cx, || {
            for (_, _, payload) in data_on(connected.receive(&browser), 5) {
                seen.extend(payload);
            }
            String::from_utf8_lossy(&seen).contains("again")
        })
        .await;
        assert!(!String::from_utf8_lossy(&seen).contains("too early"));
    }

    #[gpui::test]
    async fn a_second_project_stream_on_one_session_is_refused(cx: &mut TestAppContext) {
        let (_rig, browser, mut connected, _fixture) = session("exec cat", cx).await;
        connected.controls(&browser);
        open(&browser, &mut connected, 3, 5, cx);
        let answer = open(&browser, &mut connected, 4, 6, cx);
        assert!(
            matches!(
                &answer,
                Control::Error { code, request_id: Some(4), .. } if code == "too_large"
            ),
            "{answer:?}"
        );
        assert!(
            echoed(&browser, &mut connected, 5, b"still here\n", cx).await,
            "the first stream is untouched"
        );
    }

    #[gpui::test]
    async fn a_build_that_differs_is_refused_with_a_version_error_and_nothing_runs(
        cx: &mut TestAppContext,
    ) {
        let (_rig, browser, mut connected, fixture) = session("exec cat", cx).await;
        connected.controls(&browser);
        connected.send(
            &browser,
            &Control::IdeOpen {
                request_id: 9,
                path: String::new(),
                line: None,
                stream_id: Some(5),
                app_version: Some("0.0.1".into()),
                proto_version: Some(rpc::PROTOCOL_VERSION),
            },
        );
        cx.run_until_parked();
        let controls = connected.controls(&browser);
        assert!(
            controls.iter().any(|control| matches!(
                control,
                Control::Error { code, request_id: Some(9), message }
                    if code == "version" && message.contains("0.0.1") && message.contains("9.9.9")
            )),
            "{controls:?}"
        );
        assert!(!fixture.started());
        connected.send(&browser, &Control::Ping);
        cx.run_until_parked();
        assert_eq!(
            connected.controls(&browser),
            vec![Control::Pong],
            "the mirror keeps working"
        );
    }

    #[gpui::test]
    async fn a_session_that_has_not_proved_itself_cannot_start_a_project_server(
        cx: &mut TestAppContext,
    ) {
        let rig = rig(cx, true).await;
        let fixture = Fixture::new("exec cat", cx);
        let browser = Browser::new(&rig, "browser-1");
        pair(&rig, &browser, cx);
        // The handshake is answered, but no frame under its keys has arrived.
        let _unproven = open_session(&rig, &browser, cx);
        cx.run_until_parked();
        assert!(!fixture.started());
        assert!(rig.host.read_with(cx, |host, _| host.sessions().is_empty()));
    }

    #[gpui::test]
    async fn closing_the_session_stops_the_project_server_and_its_daemon(cx: &mut TestAppContext) {
        let (_rig, browser, mut connected, fixture) = session("exec cat", cx).await;
        connected.controls(&browser);
        open(&browser, &mut connected, 3, 5, cx);
        assert!(echoed(&browser, &mut connected, 5, b"up\n", cx).await);

        let session_id = connected.session_id;
        let identifier = fixture.identifier();
        let proxy = fixture
            .pid_of(&fixture.path().join(format!("proxy-{identifier}.pid")))
            .expect("the proxy recorded its pid");
        let daemon = fixture
            .pid_of(
                &fixture
                    .path()
                    .join(format!("state/{identifier}/server.pid")),
            )
            .expect("the daemon recorded its pid");
        assert!(is_running(&proxy) && is_running(&daemon));

        browser.client.close(session_id);
        assert!(
            wait_for(cx, || !is_running(&proxy) && !is_running(&daemon)).await,
            "both the proxy and the daemon must be gone"
        );
    }

    #[gpui::test]
    async fn revoking_the_device_during_an_open_project_stops_the_project_server(
        cx: &mut TestAppContext,
    ) {
        let (rig, browser, mut connected, fixture) = session("exec cat", cx).await;
        connected.controls(&browser);
        open(&browser, &mut connected, 3, 5, cx);
        assert!(echoed(&browser, &mut connected, 5, b"up\n", cx).await);
        let identifier = fixture.identifier();
        let proxy = fixture
            .pid_of(&fixture.path().join(format!("proxy-{identifier}.pid")))
            .expect("the proxy recorded its pid");
        let daemon = fixture
            .pid_of(
                &fixture
                    .path()
                    .join(format!("state/{identifier}/server.pid")),
            )
            .expect("the daemon recorded its pid");

        rig.relay.revoke_device("browser-1");
        assert!(
            wait_for(cx, || !is_running(&proxy) && !is_running(&daemon)).await,
            "a revoked device keeps no process on this machine"
        );
    }

    #[gpui::test]
    async fn turning_remote_control_off_stops_the_project_server(cx: &mut TestAppContext) {
        let (rig, browser, mut connected, fixture) = session("exec cat", cx).await;
        connected.controls(&browser);
        open(&browser, &mut connected, 3, 5, cx);
        assert!(echoed(&browser, &mut connected, 5, b"up\n", cx).await);
        let proxy = fixture
            .pid_of(
                &fixture
                    .path()
                    .join(format!("proxy-{}.pid", fixture.identifier())),
            )
            .expect("the proxy recorded its pid");

        rig.host.update(cx, |host, cx| host.turn_off(cx));
        assert!(wait_for(cx, || !is_running(&proxy)).await);
    }

    #[gpui::test]
    async fn a_project_server_that_exits_ends_its_stream_but_not_the_session(
        cx: &mut TestAppContext,
    ) {
        let (rig, browser, mut connected, fixture) = session("sleep 1; echo bye", cx).await;
        connected.controls(&browser);
        open(&browser, &mut connected, 3, 5, cx);
        assert!(wait_for(cx, || !fixture.identifiers().is_empty()).await);
        let pid_file = fixture
            .path()
            .join(format!("state/{}/server.pid", fixture.identifier()));
        assert!(wait_for(cx, || pid_file.exists()).await);
        let daemon = fixture.pid_of(&pid_file).expect("the daemon's pid");
        assert!(is_running(&daemon));

        let mut seen: Vec<(InnerKind, u32, Vec<u8>)> = Vec::new();
        let ended = wait_for(cx, || {
            seen.extend(data_on(connected.receive(&browser), 5));
            seen.iter().any(|(_, _, payload)| payload.is_empty())
        })
        .await;
        assert!(ended, "the end of the stream is sent: {seen:?}");
        let text: Vec<u8> = seen
            .into_iter()
            .flat_map(|(_, _, payload)| payload)
            .collect();
        assert_eq!(text, b"bye\n");

        connected.send(&browser, &Control::Ping);
        cx.run_until_parked();
        assert_eq!(connected.controls(&browser), vec![Control::Pong]);
        assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 1);

        assert!(
            wait_for(cx, || !is_running(&daemon)).await,
            "the daemon goes with the stream"
        );
    }

    #[gpui::test]
    async fn two_sessions_get_two_project_servers_with_their_own_identifiers(
        cx: &mut TestAppContext,
    ) {
        let (rig, first, mut first_connected, fixture) = session("exec cat", cx).await;
        first_connected.controls(&first);
        open(&first, &mut first_connected, 3, 5, cx);

        let second = Browser::new(&rig, "browser-2");
        pair(&rig, &second, cx);
        let mut second_connected = open_session(&rig, &second, cx);
        second_connected.send(&second, &hello(1));
        cx.run_until_parked();
        second_connected.controls(&second);
        open(&second, &mut second_connected, 3, 5, cx);

        assert_ne!(first_connected.session_id, second_connected.session_id);
        assert!(echoed(&first, &mut first_connected, 5, b"one\n", cx).await);
        assert!(echoed(&second, &mut second_connected, 5, b"two\n", cx).await);
        assert_eq!(
            fixture.identifiers().len(),
            2,
            "each session has a server of its own"
        );
    }

    #[gpui::test]
    async fn reopening_the_project_stream_never_shares_a_server_with_the_old_one(
        cx: &mut TestAppContext,
    ) {
        let (_rig, browser, mut connected, fixture) = session("exec cat", cx).await;
        connected.controls(&browser);
        open(&browser, &mut connected, 3, 5, cx);
        assert!(echoed(&browser, &mut connected, 5, b"first\n", cx).await);
        let first = fixture.identifier();
        let first_daemon = fixture
            .pid_of(&fixture.path().join(format!("state/{first}/server.pid")))
            .expect("the first daemon's pid");

        connected.send_data(&browser, 5, b"");
        cx.run_until_parked();
        open(&browser, &mut connected, 4, 5, cx);
        assert!(echoed(&browser, &mut connected, 5, b"second\n", cx).await);

        let identifiers = fixture.identifiers();
        assert_eq!(identifiers.len(), 2, "{identifiers:?}");
        let second = identifiers
            .into_iter()
            .find(|identifier| *identifier != first)
            .expect("a second identifier");
        let second_daemon = fixture
            .pid_of(&fixture.path().join(format!("state/{second}/server.pid")))
            .expect("the second daemon's pid");
        assert!(
            wait_for(cx, || !is_running(&first_daemon)).await,
            "the old server was stopped"
        );
        assert!(
            is_running(&second_daemon),
            "stopping the old one left the new one alone"
        );
        assert!(
            fixture.path().join(format!("state/{second}")).exists(),
            "and its state"
        );
    }

    #[gpui::test]
    async fn a_pid_file_naming_some_other_process_does_not_get_that_process_killed(
        cx: &mut TestAppContext,
    ) {
        let (_rig, browser, mut connected, fixture) = session("exec cat", cx).await;
        connected.controls(&browser);
        open(&browser, &mut connected, 3, 5, cx);
        assert!(echoed(&browser, &mut connected, 5, b"up\n", cx).await);
        let identifier = fixture.identifier();
        let state = fixture.path().join(format!("state/{identifier}"));
        let genuine = fixture
            .pid_of(&state.join("server.pid"))
            .expect("the daemon's pid");

        let mut bystander = util::command::new_command("sleep")
            .arg("1000")
            .spawn()
            .expect("a bystander process");
        std::fs::write(state.join("server.pid"), bystander.id().to_string())
            .expect("a pid file that names the bystander");

        connected.send_data(&browser, 5, b"");
        cx.run_until_parked();
        assert!(
            wait_for(cx, || !state.exists()).await,
            "the state is cleaned up"
        );
        let still_there = bystander.try_status().expect("status").is_none();
        bystander.kill().expect("the bystander is stopped");
        smol::block_on(
            util::command::new_command("kill")
                .args(["-9", genuine.as_str()])
                .status(),
        )
        .expect("kill runs");
        assert!(still_there, "the unrelated process was left running");
    }

    /// The Zode that controls another, end to end: the same code that runs
    /// in the app, against the real host through the in-process relay.
    mod controller {
        use super::*;
        use futures::{FutureExt as _, StreamExt as _, channel::mpsc};
        use remote::{MockDelegate, RelayConnectionOptions, RemoteConnectionOptions};
        use remote_relay_client::{
            PairingStep, RelayEnvironment, RelayHost, RelayInitiator, RelaySession,
            send_data_waiting,
        };
        use rpc::proto::Envelope;

        const CLIENT_DEVICE: &str = "client-device";

        struct Controller {
            initiator: Entity<RelayInitiator>,
            host_key: [u8; KEY_LEN],
        }

        fn directory(host_key: [u8; KEY_LEN]) -> Arc<dyn HttpClient> {
            let body = serde_json::json!([{
                "deviceId": HOST_DEVICE,
                "kind": "ide",
                "name": "Studio",
                "publicKey": encode_public_key(&host_key),
            }])
            .to_string();
            FakeHttpClient::create(move |_| {
                let body = body.clone();
                async move {
                    Ok(Response::builder()
                        .status(200)
                        .body(AsyncBody::from(body))
                        .expect("a response"))
                }
            })
        }

        fn controller(rig: &Rig, version: &str, cx: &mut TestAppContext) -> Controller {
            let host_key = rig
                .host_public_key
                .lock()
                .expect("the host registered its key");
            cx.update(|cx| {
                release_channel::init_test(
                    version.parse().expect("a version"),
                    release_channel::ReleaseChannel::Dev,
                    cx,
                )
            });
            let environment = RelayEnvironment {
                credentials: Arc::new(StaticCredentials::new(
                    USER_ID,
                    CLIENT_DEVICE,
                    "client-token",
                )),
                keychain: InMemoryKeychain::new(),
                http_client: directory(host_key),
                api_url: "https://api.test/api".to_string(),
                device_name: "Laptop".to_string(),
            };
            let transport = rig.relay.client_transport(CLIENT_DEVICE);
            let initiator =
                cx.new(|cx| RelayInitiator::new(environment, transport, "9.9.9".into(), cx));
            cx.update(|cx| RelayInitiator::register_global(&initiator, cx));
            cx.run_until_parked();
            Controller {
                initiator,
                host_key,
            }
        }

        async fn host_entry(controller: &Controller, cx: &mut TestAppContext) -> RelayHost {
            let hosts = controller
                .initiator
                .update(cx, |initiator, cx| initiator.hosts(cx))
                .await
                .expect("the host list");
            hosts
                .into_iter()
                .find(|host| host.device_id == HOST_DEVICE)
                .expect("the host is listed")
        }

        /// Pairs the controller with the host the way two people would: both
        /// compare the digits and both say they match.
        async fn pair_with_host(rig: &Rig, controller: &Controller, cx: &mut TestAppContext) {
            let host = host_entry(controller, cx).await;
            let attempt = controller
                .initiator
                .update(cx, |initiator, cx| initiator.pair(host, cx));
            cx.run_until_parked();
            let digits = match attempt.read_with(cx, |attempt, _| attempt.step().clone()) {
                PairingStep::Comparing { digits } => digits,
                other => panic!("expected digits to compare, found {other:?}"),
            };
            let pending = rig
                .host
                .read_with(cx, |host, _| host.pending_pairing().cloned())
                .expect("the host asks its person");
            assert_eq!(pending.code, digits, "both screens show the same digits");
            attempt.update(cx, |attempt, cx| attempt.confirm_digits(cx));
            rig.host.update(cx, |host, cx| {
                host.decide_pairing(pending.session_id, true, cx)
            });
            cx.run_until_parked();
            assert_eq!(
                attempt.read_with(cx, |attempt, _| attempt.step().clone()),
                PairingStep::Done
            );
        }

        async fn connect_project(
            cx: &mut TestAppContext,
        ) -> anyhow::Result<Arc<dyn remote::RemoteConnection>> {
            let options = RemoteConnectionOptions::Relay(RelayConnectionOptions {
                host_device_id: HOST_DEVICE.to_string(),
                host_name: "Studio".to_string(),
            });
            remote::connect(options, Arc::new(MockDelegate), &mut cx.to_async()).await
        }

        fn envelope(id: u32) -> Envelope {
            Envelope {
                id,
                ..Default::default()
            }
        }

        #[gpui::test]
        async fn the_project_stream_opens_and_serves_a_request(cx: &mut TestAppContext) {
            let rig = rig(cx, true).await;
            let fixture = Fixture::new("exec cat", cx);
            let controller = controller(&rig, "9.9.9", cx);
            pair_with_host(&rig, &controller, cx).await;

            let connection = connect_project(cx).await.expect("a connection");
            assert!(connection.terminals_over_rpc());
            assert!(
                connection
                    .build_command(
                        None,
                        &[],
                        &Default::default(),
                        None,
                        None,
                        remote::Interactive::Yes
                    )
                    .is_err()
            );
            assert!(!connection.shell().is_empty());

            let (incoming_tx, mut incoming_rx) = mpsc::unbounded();
            let (outgoing_tx, outgoing_rx) = mpsc::unbounded();
            let (activity_tx, _activity_rx) = mpsc::channel(1);
            let io = connection.start_proxy(
                "unused".into(),
                false,
                incoming_tx,
                outgoing_rx,
                activity_tx,
                Arc::new(MockDelegate),
                &mut cx.to_async(),
            );
            outgoing_tx.unbounded_send(envelope(41)).expect("sent");
            let mut echoed_back = None;
            for _ in 0..500 {
                cx.background_executor
                    .timer(Duration::from_millis(10))
                    .await;
                cx.run_until_parked();
                if let Ok(reply) = incoming_rx.try_recv() {
                    echoed_back = Some(reply);
                    break;
                }
            }
            assert_eq!(echoed_back.map(|reply| reply.id), Some(41));
            assert!(fixture.started());

            // A second proxy on the same connection cannot reuse the stream.
            let (second_tx, _second_rx) = mpsc::unbounded();
            let (_unused_tx, unused_rx) = mpsc::unbounded();
            let (second_activity, _a) = mpsc::channel(1);
            let second = connection.start_proxy(
                "again".into(),
                true,
                second_tx,
                unused_rx,
                second_activity,
                Arc::new(MockDelegate),
                &mut cx.to_async(),
            );
            assert!(second.await.is_err());
            drop(io);
        }

        #[gpui::test]
        async fn a_session_the_host_ends_marks_the_connection_as_ended(cx: &mut TestAppContext) {
            let rig = rig(cx, true).await;
            let _fixture = Fixture::new("exec cat", cx);
            let controller = controller(&rig, "9.9.9", cx);
            pair_with_host(&rig, &controller, cx).await;
            let connection = connect_project(cx).await.expect("a connection");
            assert!(!connection.has_been_killed());

            rig.host.update(cx, |host, cx| host.turn_off(cx));
            assert!(wait_for(cx, || connection.has_been_killed()).await);
        }

        #[gpui::test]
        async fn closing_the_connection_kills_the_project_server_and_its_daemon(
            cx: &mut TestAppContext,
        ) {
            let rig = rig(cx, true).await;
            let fixture = Fixture::new("exec cat", cx);
            let controller = controller(&rig, "9.9.9", cx);
            pair_with_host(&rig, &controller, cx).await;
            let connection = connect_project(cx).await.expect("a connection");
            let (incoming_tx, mut incoming_rx) = mpsc::unbounded();
            let (outgoing_tx, outgoing_rx) = mpsc::unbounded();
            let (activity_tx, _activity_rx) = mpsc::channel(1);
            let _io = connection.start_proxy(
                "unused".into(),
                false,
                incoming_tx,
                outgoing_rx,
                activity_tx,
                Arc::new(MockDelegate),
                &mut cx.to_async(),
            );
            outgoing_tx.unbounded_send(envelope(1)).expect("sent");
            assert!(wait_for(cx, || incoming_rx.try_recv().is_ok()).await);

            rig.host
                .read_with(cx, |host, _| host.sessions().last().map(|s| s.session_id))
                .expect("the project session is in control");
            let identifier = fixture.identifier();
            let proxy = fixture
                .pid_of(&fixture.path().join(format!("proxy-{identifier}.pid")))
                .expect("the proxy recorded its pid");
            let daemon = fixture
                .pid_of(
                    &fixture
                        .path()
                        .join(format!("state/{identifier}/server.pid")),
                )
                .expect("the daemon recorded its pid");
            assert!(is_running(&proxy) && is_running(&daemon));

            connection.kill().await.expect("killed");
            assert!(connection.has_been_killed());
            assert!(
                wait_for(cx, || !is_running(&proxy) && !is_running(&daemon)).await,
                "the proxy and its daemon are gone"
            );
        }

        #[gpui::test]
        async fn another_version_is_refused_with_an_update_message_and_the_mirror_still_works(
            cx: &mut TestAppContext,
        ) {
            let rig = rig(cx, true).await;
            let fixture = Fixture::new("exec cat", cx);
            let terminal =
                shell_terminal("read line; printf 'got:%s\\n' \"$line\"; sleep 30", cx).await;
            let controller = controller(&rig, "8.8.8", cx);
            pair_with_host(&rig, &controller, cx).await;

            let error = connect_project(cx).await.err().expect("refused");
            let message = format!("{error:#}");
            assert!(
                message.contains("Update both Zodes to the same version"),
                "{message}"
            );
            assert!(
                message.contains("8.8.8") && message.contains("9.9.9"),
                "{message}"
            );
            assert!(!fixture.started(), "nothing was started on the host");

            // The terminal and agent mirror is unaffected.
            let session: Entity<RelaySession> = controller
                .initiator
                .update(cx, |initiator, cx| initiator.connect(HOST_DEVICE, cx))
                .await
                .expect("a mirror session");
            cx.run_until_parked();
            let terminal_id = session.read_with(cx, |session, _| {
                session
                    .terminals()
                    .first()
                    .map(|terminal| terminal.id.clone())
                    .expect("the host lists its terminal")
            });
            let (stream_tx, mut stream_rx) = RelaySession::stream_channel();
            let stream_id = session.update(cx, |session, _| {
                let stream_id = session.allocate_stream_id();
                session
                    .register_stream(stream_id, stream_tx)
                    .expect("a stream");
                stream_id
            });
            session
                .update(cx, |session, cx| {
                    session.send_control(
                        &Control::TerminalAttach {
                            terminal_id,
                            stream_id,
                        },
                        cx,
                    )
                })
                .expect("attached");
            cx.run_until_parked();
            let weak = session.downgrade();
            send_data_waiting(&weak, stream_id, b"hello\n", &mut cx.to_async())
                .await
                .expect("typed");
            let mut seen = Vec::new();
            let found = wait_for(cx, || {
                while let Some(Some(chunk)) = stream_rx.next().now_or_never() {
                    seen.extend(chunk);
                }
                String::from_utf8_lossy(&seen).contains("got:hello")
            })
            .await;
            assert!(
                found,
                "typing reached the host terminal: {:?}",
                String::from_utf8_lossy(&seen)
            );
            drop(terminal);
        }

        #[gpui::test]
        async fn a_host_that_has_not_paired_this_zode_is_not_reachable(cx: &mut TestAppContext) {
            let rig = rig(cx, true).await;
            Fixture::new("exec cat", cx);
            let _controller = controller(&rig, "9.9.9", cx);
            let error = connect_project(cx).await.err().expect("refused");
            assert!(
                format!("{error:#}").to_lowercase().contains("pair"),
                "{error:#}"
            );
        }

        #[gpui::test]
        async fn the_hosts_key_is_what_gets_pinned(cx: &mut TestAppContext) {
            let rig = rig(cx, true).await;
            let controller = controller(&rig, "9.9.9", cx);
            pair_with_host(&rig, &controller, cx).await;
            let pinned = controller
                .initiator
                .read_with(cx, |initiator, _| initiator.pinned_hosts());
            assert_eq!(pinned.len(), 1);
            assert_eq!(pinned[0].public_key, controller.host_key);
        }
    }
}

mod file_requests;
