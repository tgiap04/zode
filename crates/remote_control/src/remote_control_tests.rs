//! The host and a stand-in browser, talking through an in-process relay.
//!
//! Everything between them is the real thing: the real pairing exchange, the
//! real Noise handshake and transport, the real pty-backed terminal. What is
//! faked is only the socket and the account service.

use std::{sync::Arc, time::Duration};

#[cfg(unix)]
use collections::HashMap;
use futures::AsyncReadExt as _;
use gpui::{AppContext as _, Entity, TestAppContext, UpdateGlobal as _};
use http_client::{AsyncBody, FakeHttpClient, HttpClient, Response};
use parking_lot::Mutex;
use remote_relay_client::{
    OpenMode, PinnedDevice, StopReason, TrustStore, decode_public_key, encode_pairing,
    encode_public_key,
    secure_session::{SecureChannel, read_control},
    test_support::{FakeClient, FakeRelay, InMemoryKeychain, StaticCredentials},
};
use remote_relay_protocol::{
    AgentStatus, AgentSummary, Control, DeviceKeypair, Handshake, HandshakeParameters, InnerKind,
    KEY_LEN, MAX_INNER_PAYLOAD_LEN, PairingMessage, PairingRequester, build_prologue, random_nonce,
};
use settings::{Settings as _, SettingsStore};
#[cfg(unix)]
use task::Shell;
#[cfg(unix)]
use terminal::{
    Terminal, TerminalBuilder,
    terminal_settings::{AlternateScroll, CursorShape},
};
#[cfg(unix)]
use util::paths::PathStyle;
use zode_account::{Account, AccountStatus, AccountUser};

use crate::{
    AgentMirror, FileBrowser, HostEnvironment, HostState, RemoteHost, TerminalMirror,
    remote_host::TrustedDevices, session_router::UNSUPPORTED,
};

const USER_ID: &str = "user-1";
const HOST_DEVICE: &str = "host-device";

struct Rig {
    relay: FakeRelay,
    host: Entity<RemoteHost>,
    account: Entity<Account>,
    #[cfg_attr(
        not(unix),
        allow(dead_code, reason = "only the shell-driven tests read it")
    )]
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
    build_rig(cx, enabled, status, with_files, false).await
}

/// A rig whose host is set up the way `init` sets up the app's: switched off
/// when Zode quits, and given to every window.
async fn installed_rig(cx: &mut TestAppContext) -> Rig {
    build_rig(
        cx,
        true,
        AccountStatus::SignedIn(AccountUser {
            id: USER_ID.into(),
            email: "ada@example.com".into(),
            name: None,
            avatar_url: None,
        }),
        false,
        true,
    )
    .await
}

async fn build_rig(
    cx: &mut TestAppContext,
    enabled: bool,
    status: AccountStatus,
    with_files: bool,
    installed: bool,
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
    let account_handle = account.clone();
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
        if installed {
            cx.update(|cx| {
                crate::install(
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
        } else {
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
        }
    };
    cx.run_until_parked();
    Rig {
        relay,
        host,
        account: account_handle,
        terminals,
        keychain,
        host_public_key,
        registrations,
    }
}

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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
#[cfg(unix)]
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

#[cfg(unix)]
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

    // A fifth is answered like any other and, once it has proven it holds the
    // keys, told in words why there is no place for it.
    let mut fifth = open_session(&rig, &browser, cx);
    assert!(!browser.client.was_told_closed(fifth.session_id));
    fifth.send(&browser, &hello(1));
    cx.run_until_parked();
    let told = fifth.controls(&browser);
    assert!(
        matches!(
            told.as_slice(),
            [Control::Error { code, message, .. }]
                if code == "too_large" && message.contains("already has 4 devices connected")
        ),
        "{told:?}"
    );
    assert!(browser.client.was_told_closed(fifth.session_id));
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
    let (events, _subscription) = record_host_events(&rig, cx);
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
    let told = problems(&events);
    assert_eq!(
        told.len(),
        1,
        "the person hears of the lockout once, not once per request: {told:?}"
    );
    assert!(told[0].0.contains("locked"), "{told:?}");

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
async fn a_lockout_survives_turning_remote_control_off_and_on(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    for _ in 0..3 {
        let session_id = send_pairing_request(&browser, cx);
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

    set_remote_control(cx, false, None);
    cx.run_until_parked();
    set_remote_control(cx, true, None);
    cx.run_until_parked();

    assert!(rig.host.read_with(cx, |host, _| host.has_relay_client()));
    assert!(
        rig.host.read_with(cx, |host, _| host.pairing_locked_out()),
        "turning remote control off and on must not be a way to clear a lockout"
    );
    rig.host.update(cx, |host, cx| host.unlock_pairing(cx));
    assert!(!rig.host.read_with(cx, |host, _| host.pairing_locked_out()));
}

#[gpui::test]
async fn a_spent_answer_allowance_survives_turning_remote_control_off_and_on(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, true).await;
    let (events, _subscription) = record_host_events(&rig, cx);
    let browser = Browser::new(&rig, "browser-1");
    for _ in 0..3 {
        let session_id = send_pairing_request(&browser, cx);
        assert_eq!(browser.client.take_pairing().len(), 1);
        browser.client.close(session_id);
        cx.run_until_parked();
    }

    set_remote_control(cx, false, None);
    cx.run_until_parked();
    set_remote_control(cx, true, None);
    cx.run_until_parked();
    assert!(rig.host.read_with(cx, |host, _| host.has_relay_client()));

    let session_id = send_pairing_request(&browser, cx);
    assert!(
        browser.client.take_pairing().is_empty(),
        "the allowance was spent before the restart"
    );
    assert!(browser.client.was_told_closed(session_id));
    let told = problems(&events);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].0.contains("about 60 minutes"), "{told:?}");
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

#[gpui::test]
async fn turning_it_on_from_the_action_writes_the_setting_and_connects(cx: &mut TestAppContext) {
    let rig = rig(cx, false).await;
    cx.update(|cx| {
        <dyn fs::Fs>::set_global(fs::FakeFs::new(cx.background_executor().clone()), cx);
        RemoteHost::set_global(rig.host.clone(), cx);
    });

    cx.update(|cx| crate::enable_remote_control(cx));
    cx.run_until_parked();

    assert!(cx.update(|cx| crate::RemoteControlSettings::is_enabled(cx)));
    assert!(
        rig.relay.is_host_connected(),
        "the host follows the setting"
    );
}

mod with_a_window {
    use super::*;
    use crate::{
        indicator::RemoteControlIndicator, pairing_modal::PairingModal,
        trusted_devices_modal::TrustedDevicesModal,
    };
    use gpui::{Modifiers, ParentElement as _, VisualTestContext};
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

    /// Clicks the element a test probe names, as a person would, and lets
    /// everything the click set going settle.
    fn click(selector: &'static str, cx: &mut VisualTestContext) {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("`{selector}` must be drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn pairing_modal_is_open(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> bool {
        workspace
            .read_with(cx, |workspace, cx| {
                workspace.active_modal::<PairingModal>(cx)
            })
            .is_some()
    }

    #[gpui::test]
    async fn the_reject_button_refuses_the_device_and_closes_the_question(cx: &mut TestAppContext) {
        let rig = rig(cx, true).await;
        let (workspace, cx) = window(cx, &rig).await;
        let browser = Browser::new(&rig, "browser-1");
        let session_id = ask_the_question(&rig, &browser, cx);
        assert!(pairing_modal_is_open(&workspace, cx));

        click("remote-pairing-reject", cx);

        assert!(!pairing_modal_is_open(&workspace, cx), "answered, so gone");
        assert!(
            rig.host
                .read_with(cx, |host, _| host.pending_pairing().is_none()),
            "the host is no longer asking"
        );
        assert!(
            rig.host
                .read_with(cx, |host, _| host.trusted_devices().is_empty()),
            "Reject must trust nothing"
        );
        assert!(
            browser.client.was_told_closed(session_id),
            "the other end must be told the pairing is over"
        );
    }

    #[gpui::test]
    async fn the_trust_button_trusts_the_device_and_closes_the_question(cx: &mut TestAppContext) {
        let rig = rig(cx, true).await;
        let (workspace, cx) = window(cx, &rig).await;
        let browser = Browser::new(&rig, "browser-1");
        let session_id = ask_the_question(&rig, &browser, cx);
        assert!(pairing_modal_is_open(&workspace, cx));

        click("remote-pairing-trust", cx);

        assert!(!pairing_modal_is_open(&workspace, cx), "answered, so gone");
        let trusted = rig.host.read_with(cx, |host, _| host.trusted_devices());
        assert_eq!(
            trusted
                .iter()
                .map(|device| device.device_id.as_str())
                .collect::<Vec<_>>(),
            vec!["browser-1"],
            "Trust It must trust exactly the device that asked"
        );
        assert!(
            browser.client.was_told_closed(session_id),
            "the other end must be told the pairing is over"
        );
    }

    /// Enough devices that their rows cannot fit the card, so the list either
    /// scrolls inside its bound or pushes the footer out of the modal.
    #[gpui::test]
    async fn a_long_list_of_trusted_devices_scrolls_instead_of_growing(cx: &mut TestAppContext) {
        // Pinned straight into the store rather than paired one by one: a host
        // answers three pairing requests an hour, which is fewer than a list needs
        // to overflow.
        let rig = rig(cx, false).await;
        let mut store = cx
            .update(|cx| TrustStore::load(USER_ID.to_string(), cx))
            .await;
        for index in 0..16u8 {
            cx.update(|cx| {
                store.pin(
                    PinnedDevice {
                        device_id: format!("browser-{index}"),
                        public_key: [index; KEY_LEN],
                        name: format!("Browser {index}"),
                        paired_at: 1,
                    },
                    cx,
                )
            })
            .expect("room for another device");
        }
        store.flush().await;
        cx.update(|cx| {
            <dyn fs::Fs>::set_global(fs::FakeFs::new(cx.background_executor().clone()), cx);
            RemoteHost::set_global(rig.host.clone(), cx);
            crate::enable_remote_control(cx);
        });
        cx.run_until_parked();
        let (workspace, cx) = window(cx, &rig).await;
        assert_eq!(
            rig.host
                .read_with(cx, |host, _| host.trusted_devices().len()),
            16
        );

        let host = rig.host.clone();
        workspace.update_in(cx, |workspace, window, cx| {
            workspace.toggle_modal(window, cx, move |window, cx| {
                TrustedDevicesModal::new(host, window, cx)
            });
        });
        cx.run_until_parked();

        let list = cx
            .debug_bounds("remote-trusted-devices-list")
            .expect("the list must be drawn");
        let bound = cx.update(|window, _| gpui::rems(22.).to_pixels(window.rem_size()));
        assert!(
            (list.size.height - bound).abs() < gpui::px(1.),
            "sixteen rows must fill the list's bound of {bound:?} and scroll past it, \
             got a list {:?} tall",
            list.size.height
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

    fn posted_notification_ids(cx: &VisualTestContext) -> Vec<String> {
        cx.posted_notifications()
            .iter()
            .map(|notification| notification.id.to_string())
            .collect()
    }

    fn bring_to_front(cx: &mut VisualTestContext) {
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
    }

    #[gpui::test]
    async fn a_pairing_request_while_zode_is_in_the_background_posts_one_notification(
        cx: &mut TestAppContext,
    ) {
        let rig = rig(cx, true).await;
        cx.set_notifications_supported(true);
        let (workspace, cx) = window(cx, &rig).await;
        cx.deactivate_window();
        let browser = Browser::new(&rig, "browser-1");
        let session_id = ask_the_question(&rig, &browser, cx);

        assert!(
            !pairing_modal_is_open(&workspace, cx),
            "no window is in front, so nothing is put on screen yet"
        );
        let posted = cx.posted_notifications();
        assert_eq!(posted.len(), 1, "the person is told once");
        assert_eq!(
            posted[0].id.to_string(),
            format!("remote-control-pairing-{session_id}")
        );
        assert_eq!(
            posted[0].title.to_string(),
            "Ada's browser wants to control this Zode"
        );
        assert_eq!(
            posted[0].body.to_string(),
            "Switch to Zode to compare the six digits."
        );

        rig.host.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(
            posted_notification_ids(cx).len(),
            1,
            "hearing about the same request again must not tell the person again"
        );

        bring_to_front(cx);
        assert!(
            pairing_modal_is_open(&workspace, cx),
            "coming to the front is what puts the question on screen"
        );
        assert_eq!(
            posted_notification_ids(cx).len(),
            1,
            "and it does not tell the person a second time"
        );
    }

    #[gpui::test]
    async fn two_windows_in_the_background_post_one_notification_between_them(
        cx: &mut TestAppContext,
    ) {
        let rig = rig(cx, true).await;
        cx.set_notifications_supported(true);
        let (_first, cx) = window(cx, &rig).await;
        let (second, cx) = window(cx, &rig).await;
        cx.deactivate_window();
        let browser = Browser::new(&rig, "browser-1");
        let session_id = ask_the_question(&rig, &browser, cx);

        assert_eq!(
            posted_notification_ids(cx),
            vec![format!("remote-control-pairing-{session_id}")]
        );

        bring_to_front(cx);
        assert!(pairing_modal_is_open(&second, cx));
    }

    #[gpui::test]
    async fn an_active_window_shows_the_question_and_posts_no_notification(
        cx: &mut TestAppContext,
    ) {
        let rig = rig(cx, true).await;
        cx.set_notifications_supported(true);
        let (workspace, cx) = window(cx, &rig).await;
        let browser = Browser::new(&rig, "browser-1");
        ask_the_question(&rig, &browser, cx);

        assert!(pairing_modal_is_open(&workspace, cx));
        assert!(
            posted_notification_ids(cx).is_empty(),
            "somebody is looking at Zode, so there is nobody to call back"
        );
    }

    #[gpui::test]
    async fn a_window_in_front_keeps_the_notification_back_from_a_window_behind_it(
        cx: &mut TestAppContext,
    ) {
        let rig = rig(cx, true).await;
        cx.set_notifications_supported(true);
        let (first, cx) = window(cx, &rig).await;
        let (second, cx) = window(cx, &rig).await;
        let browser = Browser::new(&rig, "browser-1");
        ask_the_question(&rig, &browser, cx);

        assert!(pairing_modal_is_open(&second, cx), "the front window asks");
        assert!(
            !pairing_modal_is_open(&first, cx),
            "and the one behind it does not"
        );
        assert!(
            posted_notification_ids(cx).is_empty(),
            "a window behind another of Zode's must not call the person back"
        );
    }

    #[gpui::test]
    async fn leaving_zode_after_the_question_was_shown_does_not_post_a_notification(
        cx: &mut TestAppContext,
    ) {
        let rig = rig(cx, true).await;
        cx.set_notifications_supported(true);
        let (workspace, cx) = window(cx, &rig).await;
        let browser = Browser::new(&rig, "browser-1");
        ask_the_question(&rig, &browser, cx);
        assert!(pairing_modal_is_open(&workspace, cx));

        cx.deactivate_window();

        assert!(
            posted_notification_ids(cx).is_empty(),
            "the question was already put in front of the person"
        );
    }

    #[gpui::test]
    async fn a_platform_that_cannot_post_notifications_still_asks_when_zode_comes_back(
        cx: &mut TestAppContext,
    ) {
        let rig = rig(cx, true).await;
        let (workspace, cx) = window(cx, &rig).await;
        cx.deactivate_window();
        let browser = Browser::new(&rig, "browser-1");
        ask_the_question(&rig, &browser, cx);

        assert!(posted_notification_ids(cx).is_empty());
        assert!(!pairing_modal_is_open(&workspace, cx));

        bring_to_front(cx);
        assert!(pairing_modal_is_open(&workspace, cx));
    }

    #[gpui::test]
    async fn the_call_back_is_forgotten_with_its_question(cx: &mut TestAppContext) {
        let rig = rig(cx, true).await;
        cx.set_notifications_supported(true);
        let (_workspace, cx) = window(cx, &rig).await;
        cx.deactivate_window();
        let browser = Browser::new(&rig, "browser-1");
        let session_id = ask_the_question(&rig, &browser, cx);
        assert_eq!(posted_notification_ids(cx).len(), 1);
        assert!(
            !rig.host
                .update(cx, |host, _| host.claim_pairing_notification(session_id)),
            "the person has been called back already"
        );

        rig.host
            .update(cx, |host, cx| host.decide_pairing(session_id, false, cx));
        cx.run_until_parked();

        assert!(
            rig.host
                .update(cx, |host, _| host.claim_pairing_notification(session_id)),
            "nothing is remembered about a session that is over, so a reused id is announced"
        );
        assert!(
            !rig.host
                .update(cx, |host, _| host.claim_pairing_notification(session_id)),
            "and then only once"
        );
    }

    #[gpui::test]
    async fn each_new_request_is_announced_on_its_own(cx: &mut TestAppContext) {
        let rig = rig(cx, true).await;
        cx.set_notifications_supported(true);
        let (_workspace, cx) = window(cx, &rig).await;
        cx.deactivate_window();
        let first_browser = Browser::new(&rig, "browser-1");
        let first = ask_the_question(&rig, &first_browser, cx);
        rig.host
            .update(cx, |host, cx| host.decide_pairing(first, false, cx));
        cx.run_until_parked();

        let second_browser = Browser::new(&rig, "browser-2");
        let second = ask_the_question(&rig, &second_browser, cx);

        assert_eq!(
            posted_notification_ids(cx),
            vec![
                format!("remote-control-pairing-{first}"),
                format!("remote-control-pairing-{second}"),
            ]
        );
    }

    /// A modal that stays up until it is allowed to close, as a dialog that
    /// declines to be dismissed does.
    struct StubbornModal {
        focus_handle: gpui::FocusHandle,
        may_close: std::rc::Rc<std::cell::Cell<bool>>,
    }

    impl gpui::EventEmitter<gpui::DismissEvent> for StubbornModal {}

    impl gpui::Focusable for StubbornModal {
        fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
            self.focus_handle.clone()
        }
    }

    impl workspace::ModalView for StubbornModal {
        fn on_before_dismiss(
            &mut self,
            _window: &mut gpui::Window,
            _cx: &mut gpui::Context<Self>,
        ) -> workspace::DismissDecision {
            workspace::DismissDecision::Dismiss(self.may_close.get())
        }
    }

    impl gpui::Render for StubbornModal {
        fn render(
            &mut self,
            _window: &mut gpui::Window,
            _cx: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            gpui::div().child("busy")
        }
    }

    #[gpui::test]
    async fn the_trusted_devices_stay_listed_when_the_host_starts_fails_or_is_stopped_under_them(
        cx: &mut TestAppContext,
    ) {
        let rig = rig(cx, false).await;
        pin_in_storage(&[pinned("phone", 1)], cx).await;
        let (workspace, cx) = window(cx, &rig).await;
        let host = rig.host.clone();
        workspace.update_in(cx, |workspace, window, cx| {
            workspace.toggle_modal(window, cx, move |window, cx| {
                TrustedDevicesModal::new(host, window, cx)
            });
        });
        let listed = |cx: &mut VisualTestContext| {
            rig.host
                .read_with(cx, |host, cx| match host.trusted_devices_listing(cx) {
                    TrustedDevices::Listed { devices, .. } => Some(
                        devices
                            .into_iter()
                            .map(|device| device.device_id)
                            .collect::<Vec<_>>(),
                    ),
                    TrustedDevices::SignedOut | TrustedDevices::Loading => None,
                })
        };
        cx.run_until_parked();
        assert_eq!(listed(cx), Some(vec!["phone".to_string()]));

        // It tries to start and cannot read its key.
        rig.keychain.fail_reads();
        set_remote_control_in_window(true, cx);
        assert!(matches!(
            rig.host.read_with(cx, |host, _| host.state().clone()),
            HostState::Failed(_)
        ));
        assert_eq!(
            listed(cx),
            Some(vec!["phone".to_string()]),
            "a start that failed does not take the list with it"
        );

        // It is started and stopped again before it got anywhere.
        rig.keychain.recover();
        set_remote_control_in_window(false, cx);
        cx.update(|_, cx| {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |content| {
                    content.remote_control.get_or_insert_default().enabled = Some(true);
                });
            });
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |content| {
                    content.remote_control.get_or_insert_default().enabled = Some(false);
                });
            });
        });
        cx.run_until_parked();
        assert_eq!(
            listed(cx),
            Some(vec!["phone".to_string()]),
            "and the modal asks for the list again rather than staying on Loading"
        );
    }

    fn set_remote_control_in_window(enabled: bool, cx: &mut VisualTestContext) {
        cx.update(|_, cx| {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |content| {
                    content.remote_control.get_or_insert_default().enabled = Some(enabled);
                });
            });
        });
        cx.run_until_parked();
    }

    #[gpui::test]
    async fn a_question_that_could_not_be_put_on_screen_is_asked_again_once_it_can(
        cx: &mut TestAppContext,
    ) {
        let rig = rig(cx, true).await;
        let (workspace, cx) = window(cx, &rig).await;
        let may_close = std::rc::Rc::new(std::cell::Cell::new(false));
        workspace.update_in(cx, |workspace, window, cx| {
            let may_close = may_close.clone();
            workspace.toggle_modal(window, cx, move |_, cx| StubbornModal {
                focus_handle: cx.focus_handle(),
                may_close,
            });
        });
        let browser = Browser::new(&rig, "browser-1");
        let session_id = ask_the_question(&rig, &browser, cx);
        assert!(
            !pairing_modal_is_open(&workspace, cx),
            "the modal that would not close kept its place"
        );
        assert!(
            !rig.host
                .read_with(cx, |host, _| host.pairing_is_presented(session_id)),
            "a question that is not on screen is not counted as shown"
        );

        may_close.set(true);
        workspace.update_in(cx, |workspace, window, cx| {
            workspace.hide_modal(window, cx);
        });
        rig.host.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert!(pairing_modal_is_open(&workspace, cx));
        assert!(
            rig.host
                .read_with(cx, |host, _| host.pairing_is_presented(session_id))
        );
    }

    #[gpui::test]
    async fn a_question_replaced_by_another_modal_does_not_count_toward_the_lockout(
        cx: &mut TestAppContext,
    ) {
        let rig = rig(cx, true).await;
        let (workspace, cx) = window(cx, &rig).await;
        let browser = Browser::new(&rig, "browser-1");
        for _ in 0..3 {
            let session_id = ask_the_question(&rig, &browser, cx);
            assert!(pairing_modal_is_open(&workspace, cx));

            let host = rig.host.clone();
            workspace.update_in(cx, |workspace, window, cx| {
                workspace.toggle_modal(window, cx, move |window, cx| {
                    TrustedDevicesModal::new(host, window, cx)
                });
            });
            cx.run_until_parked();
            assert!(!pairing_modal_is_open(&workspace, cx));
            assert!(
                rig.host
                    .read_with(cx, |host, _| host.pending_pairing().is_none())
            );
            assert!(browser.client.was_told_closed(session_id));
            workspace.update_in(cx, |workspace, window, cx| {
                workspace.hide_modal(window, cx);
            });
        }
        assert!(
            !rig.host.read_with(cx, |host, _| host.pairing_locked_out()),
            "being covered by another modal is not a mismatch"
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

/// Sends a pairing request on a new session and returns the session.
fn send_pairing_request(browser: &Browser, cx: &mut TestAppContext) -> u32 {
    let session_id = browser.client.open(HOST_DEVICE, OpenMode::Pair);
    let (_requester, request) = PairingRequester::start(
        *browser.keypair.public_key(),
        random_nonce().expect("randomness"),
    );
    browser
        .client
        .send_text(&encode_pairing(session_id, &request).expect("encodes"));
    cx.run_until_parked();
    session_id
}

fn pinned(device_id: &str, key_byte: u8) -> PinnedDevice {
    PinnedDevice {
        device_id: device_id.to_string(),
        public_key: [key_byte; KEY_LEN],
        name: format!("Browser {device_id}"),
        paired_at: 1_700_000_000,
    }
}

/// The ids Manage Trusted Devices would list, or `None` while it has nothing
/// to list yet.
fn listed_device_ids(rig: &Rig, cx: &mut TestAppContext) -> Option<Vec<String>> {
    rig.host
        .read_with(cx, |host, cx| match host.trusted_devices_listing(cx) {
            TrustedDevices::Listed { devices, .. } => {
                Some(devices.into_iter().map(|device| device.device_id).collect())
            }
            TrustedDevices::SignedOut | TrustedDevices::Loading => None,
        })
}

async fn pin_in_storage(devices: &[PinnedDevice], cx: &mut TestAppContext) {
    let mut store = cx
        .update(|cx| TrustStore::load(USER_ID.to_string(), cx))
        .await;
    for device in devices {
        cx.update(|cx| store.pin(device.clone(), cx))
            .expect("room for another device");
    }
    store.flush().await;
}

#[gpui::test]
async fn re_pairing_a_device_with_a_new_key_ends_the_sessions_made_with_the_old_one(
    cx: &mut TestAppContext,
) {
    let (rig, browser, mut connected) = paired_and_connected(cx).await;
    connected.controls(&browser);
    let old_session = connected.session_id;
    assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 1);

    // The same device id and a key nobody has trusted yet: what re-pairing a
    // browser whose key may have been taken looks like.
    let rekeyed = Browser {
        id: browser.id,
        keypair: DeviceKeypair::generate().expect("a key"),
        client: browser.client,
    };
    let session_id = begin_pairing_until_asked(&rig, &rekeyed, cx);
    let pending = rig
        .host
        .read_with(cx, |host, _| host.pending_pairing().cloned())
        .expect("the person is asked");
    assert_eq!(
        pending.replaces.as_deref(),
        Some("Ada's browser"),
        "the question says it replaces a device that is already trusted"
    );
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.sessions().len()),
        1,
        "nothing changes until the person says yes"
    );

    rig.host
        .update(cx, |host, cx| host.decide_pairing(session_id, true, cx));
    cx.run_until_parked();
    assert!(
        rig.host.read_with(cx, |host, _| host.sessions().is_empty()),
        "a session built from the old key must not outlive it"
    );
    assert!(rekeyed.client.was_told_closed(old_session));
    let trusted = rig.host.read_with(cx, |host, _| host.trusted_devices());
    assert_eq!(trusted.len(), 1);
    assert_eq!(trusted[0].public_key, *rekeyed.keypair.public_key());
}

#[gpui::test]
async fn re_pairing_with_the_same_key_leaves_the_session_alone(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = paired_and_connected(cx).await;
    connected.controls(&browser);
    let session_id = begin_pairing_until_asked(&rig, &browser, cx);
    let pending = rig
        .host
        .read_with(cx, |host, _| host.pending_pairing().cloned())
        .expect("the person is asked");
    assert_eq!(
        pending.replaces, None,
        "the key that is saved is the key that asks: nothing is replaced"
    );
    rig.host
        .update(cx, |host, cx| host.decide_pairing(session_id, true, cx));
    cx.run_until_parked();

    assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 1);
    connected.send(&browser, &Control::Ping);
    cx.run_until_parked();
    assert_eq!(connected.controls(&browser), vec![Control::Pong]);
}

#[gpui::test]
async fn a_device_that_is_not_trusted_yet_is_not_said_to_replace_anything(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    begin_pairing_until_asked(&rig, &browser, cx);
    let pending = rig
        .host
        .read_with(cx, |host, _| host.pending_pairing().cloned())
        .expect("the person is asked");
    assert_eq!(pending.replaces, None);
}

#[gpui::test]
async fn a_request_abandoned_before_anyone_was_asked_neither_holds_pairing_nor_spends_a_question(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    for attempt in 0..3 {
        let session_id = send_pairing_request(&browser, cx);
        assert_eq!(
            browser.client.take_pairing().len(),
            1,
            "request {attempt} is answered at once: the ones before it left"
        );
        browser.client.close(session_id);
        cx.run_until_parked();
    }
    // Another device is not held up by them, and the hour's three questions
    // are all still there to be asked.
    let other = Browser::new(&rig, "browser-2");
    for _ in 0..3 {
        pair(&rig, &other, cx);
    }
}

#[gpui::test]
async fn a_requester_that_leaves_when_it_does_not_like_the_digits_is_cut_off_after_three_tries(
    cx: &mut TestAppContext,
) {
    // The requester learns the host's nonce from the accept, can work out the
    // digits before it reveals, and closes if they are not what it wants: nobody
    // is asked and nothing counts against it, so only the number of answers it
    // gets bounds how often it may try.
    let rig = rig(cx, true).await;
    let (events, _subscription) = record_host_events(&rig, cx);
    let browser = Browser::new(&rig, "browser-1");
    for attempt in 0..3 {
        let session_id = send_pairing_request(&browser, cx);
        assert_eq!(
            browser.client.take_pairing().len(),
            1,
            "attempt {attempt} is answered"
        );
        browser.client.close(session_id);
        cx.run_until_parked();
    }
    assert!(problems(&events).is_empty());

    for attempt in 3..8 {
        let session_id = send_pairing_request(&browser, cx);
        assert!(
            browser.client.take_pairing().is_empty(),
            "attempt {attempt} is not answered"
        );
        assert!(browser.client.was_told_closed(session_id));
    }
    assert!(
        rig.host
            .read_with(cx, |host, _| host.pending_pairing().is_none()),
        "nobody was ever asked"
    );
    let told = problems(&events);
    assert_eq!(told.len(), 1, "said once: {told:?}");
    assert!(told[0].0.contains("too many times"), "{told:?}");
}

#[gpui::test]
async fn a_relay_inventing_device_ids_is_cut_off_after_twelve_answers_an_hour(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, true).await;
    let mut answered = 0;
    for index in 0..16 {
        let invented = Browser::new(&rig, &format!("invented-{index}"));
        let session_id = send_pairing_request(&invented, cx);
        if !invented.client.take_pairing().is_empty() {
            answered += 1;
            invented.client.close(session_id);
            cx.run_until_parked();
        }
    }
    assert_eq!(answered, 12, "however many names it makes up");

    cx.executor()
        .advance_clock(Duration::from_secs(60 * 60 + 1));
    cx.run_until_parked();
    let again = Browser::new(&rig, "invented-again");
    send_pairing_request(&again, cx);
    assert_eq!(
        again.client.take_pairing().len(),
        1,
        "the hour passes and it may try again"
    );
}

#[gpui::test]
async fn once_the_allowance_is_spent_the_person_is_told_why_nothing_was_asked(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, true).await;
    let (events, _subscription) = record_host_events(&rig, cx);
    let browser = Browser::new(&rig, "browser-1");
    for _ in 0..3 {
        pair(&rig, &browser, cx);
    }
    assert!(problems(&events).is_empty());

    for _ in 0..2 {
        let session_id = send_pairing_request(&browser, cx);
        assert!(browser.client.take_pairing().is_empty());
        assert!(browser.client.was_told_closed(session_id));
    }
    let told = problems(&events);
    assert_eq!(
        told.len(),
        1,
        "said once, however often it is asked: {told:?}"
    );
    assert!(told[0].0.contains("3 times this hour"), "{told:?}");
}

#[gpui::test]
async fn a_full_trust_store_is_said_before_the_person_compares_digits(cx: &mut TestAppContext) {
    let rig = rig(cx, false).await;
    let devices: Vec<PinnedDevice> = (0..remote_relay_client::MAX_PINNED_DEVICES as u8)
        .map(|index| pinned(&format!("full-{index}"), index))
        .collect();
    pin_in_storage(&devices, cx).await;
    set_remote_control(cx, true, None);
    cx.run_until_parked();
    let (events, _subscription) = record_host_events(&rig, cx);

    let browser = Browser::new(&rig, "browser-1");
    let session_id = send_pairing_request(&browser, cx);
    assert!(
        browser.client.take_pairing().is_empty(),
        "no ceremony is run for a pin that cannot be written"
    );
    assert!(browser.client.was_told_closed(session_id));
    let told = problems(&events);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].0.contains("too many devices"), "{told:?}");
}

#[gpui::test]
async fn closing_the_question_unanswered_never_counts_toward_the_lockout(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    for _ in 0..3 {
        let session_id = begin_pairing_until_asked(&rig, &browser, cx);
        rig.host.update(cx, |host, cx| {
            host.dismiss_pairing_if_pending(session_id, cx)
        });
        cx.run_until_parked();
        assert!(browser.client.was_told_closed(session_id));
        assert!(
            rig.host
                .read_with(cx, |host, _| host.pending_pairing().is_none())
        );
    }
    assert!(
        !rig.host.read_with(cx, |host, _| host.pairing_locked_out()),
        "digits that were never compared did not fail to match"
    );
    assert!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().is_empty())
    );
}

#[gpui::test]
async fn a_trust_click_after_the_deadline_says_the_request_expired(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let (events, _subscription) = record_host_events(&rig, cx);
    let browser = Browser::new(&rig, "browser-1");
    // Housekeeping looks every five seconds, from when the host started. The
    // question is asked off that rhythm, so there is a moment after its
    // deadline that housekeeping has not yet seen.
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    let session_id = begin_pairing_until_asked(&rig, &browser, cx);

    // Past the deadline and before housekeeping has looked: the question is
    // still on screen, which is when the click comes.
    cx.executor().advance_clock(Duration::from_secs(121));
    assert!(
        rig.host
            .read_with(cx, |host, _| host.pending_pairing().is_some())
    );
    rig.host
        .update(cx, |host, cx| host.decide_pairing(session_id, true, cx));
    cx.run_until_parked();

    assert!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().is_empty())
    );
    let told = problems(&events);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].0.contains("ran out of time"), "{told:?}");
}

#[gpui::test]
async fn trusted_devices_are_listed_and_forgotten_while_remote_control_is_off(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, false).await;
    pin_in_storage(&[pinned("lost-laptop", 1), pinned("phone", 2)], cx).await;
    assert_eq!(
        listed_device_ids(&rig, cx),
        None,
        "nothing is known until the saved devices have been read"
    );

    rig.host
        .update(cx, |host, cx| host.load_trusted_devices(cx));
    cx.run_until_parked();
    assert_eq!(
        listed_device_ids(&rig, cx),
        Some(vec!["lost-laptop".to_string(), "phone".to_string()])
    );

    rig.host
        .update(cx, |host, cx| host.forget_device("lost-laptop", cx));
    cx.run_until_parked();
    assert_eq!(listed_device_ids(&rig, cx), Some(vec!["phone".to_string()]));
    let reloaded = cx
        .update(|cx| TrustStore::load(USER_ID.to_string(), cx))
        .await;
    assert_eq!(
        reloaded
            .devices()
            .iter()
            .map(|device| device.device_id.as_str())
            .collect::<Vec<_>>(),
        vec!["phone"],
        "forgetting while off is saved"
    );

    set_remote_control(cx, true, None);
    cx.run_until_parked();
    assert!(rig.host.read_with(cx, |host, _| host.has_relay_client()));
    assert_eq!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().len()),
        1,
        "the forgotten device is still forgotten when the host starts"
    );
}

#[gpui::test]
async fn a_forget_that_cannot_be_saved_says_so_instead_of_looking_done(cx: &mut TestAppContext) {
    let rig = rig(cx, false).await;
    let kvp = cx.update(|cx| db::kvp::KeyValueStore::global(cx));
    kvp.write_kvp(format!("remote_control_trust:{USER_ID}"), "not json".into())
        .await
        .expect("saved");
    set_remote_control(cx, true, None);
    cx.run_until_parked();
    let (events, _subscription) = record_host_events(&rig, cx);

    rig.host
        .update(cx, |host, cx| host.forget_device("browser-1", cx));
    cx.run_until_parked();

    let told = problems(&events);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].0.contains("could not be read"), "{told:?}");
}

#[gpui::test]
async fn trusted_devices_are_still_listed_when_the_host_has_halted(cx: &mut TestAppContext) {
    let (rig, _browser, _connected) = paired_and_connected(cx).await;
    rig.relay
        .drop_host_connection(Some(4403), "session_revoked");
    cx.run_until_parked();
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.state().clone()),
        HostState::Halted(StopReason::SessionEnded)
    );

    assert_eq!(
        listed_device_ids(&rig, cx),
        Some(vec!["browser-1".to_string()])
    );
    rig.host
        .update(cx, |host, cx| host.forget_device("browser-1", cx));
    cx.run_until_parked();
    let reloaded = cx
        .update(|cx| TrustStore::load(USER_ID.to_string(), cx))
        .await;
    assert!(reloaded.devices().is_empty(), "the forget was saved");
}

#[gpui::test]
async fn without_an_account_there_are_no_trusted_devices_to_show(cx: &mut TestAppContext) {
    let rig = rig_with_status(cx, true, AccountStatus::SignedOut).await;
    rig.host
        .update(cx, |host, cx| host.load_trusted_devices(cx));
    cx.run_until_parked();
    assert!(rig.host.read_with(cx, |host, cx| matches!(
        host.trusted_devices_listing(cx),
        TrustedDevices::SignedOut
    )));
}

#[gpui::test]
async fn turning_remote_control_on_works_after_the_write_that_turned_it_off_failed(
    cx: &mut TestAppContext,
) {
    let (rig, _browser, _connected) = paired_and_connected(cx).await;
    cx.update(|cx| {
        <dyn fs::Fs>::set_global(fs::FakeFs::new(cx.background_executor().clone()), cx);
        RemoteHost::set_global(rig.host.clone(), cx);
    });
    // Turned off, but the setting still reads on: the write did not go through.
    rig.host.update(cx, |host, cx| host.turn_off(cx));
    cx.run_until_parked();
    assert!(!rig.host.read_with(cx, |host, _| host.has_relay_client()));

    cx.update(|cx| crate::enable_remote_control(cx));
    cx.run_until_parked();
    assert!(
        rig.host.read_with(cx, |host, _| host.has_relay_client()),
        "asking for it again must bring it back"
    );
    assert!(rig.relay.is_host_connected());
}

#[gpui::test]
async fn quitting_zode_turns_remote_control_off(cx: &mut TestAppContext) {
    let rig = installed_rig(cx).await;
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let mut connected = open_session(&rig, &browser, cx);
    connected.send(&browser, &hello(1));
    cx.run_until_parked();
    assert!(rig.host.read_with(cx, |host, _| host.has_relay_client()));

    cx.update(|cx| cx.shutdown());

    assert!(
        !rig.host.read_with(cx, |host, _| host.has_relay_client()),
        "the relay client goes before the process does"
    );
    assert!(rig.host.read_with(cx, |host, _| host.sessions().is_empty()));
    cx.run_until_parked();
    assert!(browser.client.was_told_closed(connected.session_id));
}

#[gpui::test]
async fn frames_that_change_nothing_do_not_redraw_what_watches_the_host(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = paired_and_connected(cx).await;
    connected.controls(&browser);
    let notified = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let _observation = cx.update(|cx| {
        let notified = notified.clone();
        cx.observe(&rig.host, move |_, _| notified.set(notified.get() + 1))
    });

    for _ in 0..20 {
        connected.send(&browser, &Control::Ping);
    }
    cx.run_until_parked();
    assert_eq!(
        connected.controls(&browser).len(),
        20,
        "every ping is answered"
    );
    assert_eq!(
        notified.get(),
        0,
        "twenty pings changed nothing the status bar shows"
    );

    browser.client.close(connected.session_id);
    cx.run_until_parked();
    assert!(
        notified.get() > 0,
        "a device leaving is something the status bar shows"
    );
}

/// A greeted browser, with `files` offered, the way the running app is.
async fn connected_with_files(cx: &mut TestAppContext) -> (Rig, Browser, Connected) {
    let rig = rig_with_files(
        cx,
        true,
        AccountStatus::SignedIn(AccountUser {
            id: USER_ID.into(),
            email: "ada@example.com".into(),
            name: None,
            avatar_url: None,
        }),
        true,
    )
    .await;
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let mut connected = open_session(&rig, &browser, cx);
    connected.send(&browser, &hello(1));
    cx.run_until_parked();
    (rig, browser, connected)
}

#[gpui::test]
async fn browsing_files_keeps_a_session_from_going_idle(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = connected_with_files(cx).await;
    set_remote_control(cx, true, Some(1));
    cx.run_until_parked();
    for request_id in 0..8 {
        cx.executor().advance_clock(Duration::from_secs(20));
        connected.send(
            &browser,
            &Control::FilesList {
                request_id,
                worktree_id: None,
                path: String::new(),
            },
        );
        cx.run_until_parked();
    }
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.sessions().len()),
        1,
        "asking to see files is use, as the setting says"
    );

    cx.executor().advance_clock(Duration::from_secs(61));
    cx.run_until_parked();
    assert!(
        rig.host.read_with(cx, |host, _| host.sessions().is_empty()),
        "and a minute without asking is a minute of silence"
    );
}

#[cfg(unix)]
#[gpui::test]
async fn control_messages_sent_while_the_relay_is_full_wait_instead_of_ending_sessions(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, false).await;
    rig.relay.set_outbound_capacity(6);
    set_remote_control(cx, true, None);
    cx.run_until_parked();
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let mut connected = open_session(&rig, &browser, cx);
    connected.send(&browser, &hello(1));
    cx.run_until_parked();
    let flooding = shell_terminal(
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
        if flooding
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

    // The relay's queue is now full of output, and the device pings and the
    // host announces a new terminal to every device.
    assert!(
        !rig.host.read_with(cx, |host, cx| host.relay_has_room(cx)),
        "the connection to the relay is full when the ping arrives"
    );
    connected.send(&browser, &Control::Ping);
    let _another = shell_terminal("sleep 30", cx).await;
    cx.run_until_parked();
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.sessions().len()),
        1,
        "a full relay connection is not a reason to end a session over a reply"
    );

    rig.relay.resume_delivery();
    let mut received = Vec::new();
    for _ in 0..500 {
        cx.background_executor
            .timer(Duration::from_millis(10))
            .await;
        cx.run_until_parked();
        received.extend(connected.receive(&browser));
        let flood: Vec<u8> = received
            .iter()
            .filter(|(kind, stream, _)| *kind == InnerKind::Data && *stream == 7)
            .flat_map(|(_, _, payload)| payload.clone())
            .collect();
        if String::from_utf8_lossy(&flood).contains("FLOODEND") {
            break;
        }
    }
    let controls: Vec<Control> = received
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
        .collect();
    assert!(
        controls.contains(&Control::Pong),
        "the reply went out once there was room: {controls:?}"
    );
    assert!(
        controls.iter().any(|control| matches!(
            control,
            Control::TerminalList { terminals } if terminals.len() == 2
        )),
        "and so did the announcement: {controls:?}"
    );
    assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 1);
}

/// A host whose connection to the relay holds a handful of frames, with a device
/// that has said hello.
async fn connected_over_a_narrow_relay(cx: &mut TestAppContext) -> (Rig, Browser, Connected) {
    let rig = rig(cx, false).await;
    rig.relay.set_outbound_capacity(6);
    set_remote_control(cx, true, None);
    cx.run_until_parked();
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let mut connected = open_session(&rig, &browser, cx);
    connected.send(&browser, &hello(1));
    cx.run_until_parked();
    connected.controls(&browser);
    (rig, browser, connected)
}

/// Lets the relay go again and gathers everything the host sends until `done`.
async fn collect_until(
    browser: &Browser,
    connected: &mut Connected,
    cx: &mut TestAppContext,
    mut done: impl FnMut(&[(InnerKind, u32, Vec<u8>)]) -> bool,
) -> Vec<(InnerKind, u32, Vec<u8>)> {
    let mut received = Vec::new();
    for _ in 0..1000 {
        cx.background_executor
            .timer(Duration::from_millis(10))
            .await;
        cx.run_until_parked();
        received.extend(connected.receive(browser));
        if done(&received) {
            break;
        }
    }
    received
}

fn controls_of(received: &[(InnerKind, u32, Vec<u8>)]) -> Vec<Control> {
    received
        .iter()
        .filter(|(kind, _, _)| *kind == InnerKind::Control)
        .map(|(_, _, payload)| {
            read_control(&remote_relay_protocol::InnerFrame {
                kind: InnerKind::Control,
                stream_id: 0,
                payload: payload.clone(),
            })
            .expect("the host speaks the protocol")
        })
        .collect()
}

fn data_frames(received: &[(InnerKind, u32, Vec<u8>)]) -> usize {
    received
        .iter()
        .filter(|(kind, _, payload)| *kind == InnerKind::Data && !payload.is_empty())
        .count()
}

#[gpui::test]
async fn a_reply_does_not_wait_behind_a_file_the_device_is_slow_to_take(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = connected_over_a_narrow_relay(cx).await;
    let session_id = connected.session_id;
    let total: usize = 1_000_000;
    let frames = total.div_ceil(MAX_INNER_PAYLOAD_LEN);

    rig.relay.pause_delivery();
    rig.host
        .update(cx, |host, _| host.queue_waiting_bytes(session_id, total));
    rig.host.update(cx, |host, cx| host.flush_all(cx));
    assert!(!rig.host.read_with(cx, |host, cx| host.relay_has_room(cx)));
    connected.send(&browser, &Control::Ping);
    cx.run_until_parked();

    rig.relay.resume_delivery();
    let received = collect_until(&browser, &mut connected, cx, |received| {
        data_frames(received) == frames && controls_of(received).contains(&Control::Pong)
    })
    .await;
    assert_eq!(data_frames(&received), frames, "all of the file arrives");
    let pong_at = received
        .iter()
        .position(|(kind, _, _)| *kind == InnerKind::Control)
        .expect("the reply arrives");
    let frames_before = received[..pong_at]
        .iter()
        .filter(|(kind, _, _)| *kind == InnerKind::Data)
        .count();
    assert!(
        frames_before < frames / 2,
        "the reply went out after the {frames_before} frames already on their way, \
         not after all {frames}"
    );
    assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 1);
}

#[gpui::test]
async fn what_the_host_pushes_to_a_device_slow_to_read_is_replaced_and_never_ends_it(
    cx: &mut TestAppContext,
) {
    let (rig, browser, mut connected) = connected_over_a_narrow_relay(cx).await;
    let session_id = connected.session_id;
    // More waiting for the device than it is allowed to ask for.
    let total: usize = 8 * 1024 * 1024 + 100 * 1024;
    let frames = total.div_ceil(MAX_INNER_PAYLOAD_LEN);

    rig.relay.pause_delivery();
    rig.host
        .update(cx, |host, _| host.queue_waiting_bytes(session_id, total));
    rig.host.update(cx, |host, cx| host.flush_all(cx));
    assert!(!rig.host.read_with(cx, |host, cx| host.relay_has_room(cx)));

    let agent = |status| Control::AgentUpdate {
        agent: AgentSummary {
            id: "agent-1".to_string(),
            name: "Claude Code".to_string(),
            status,
            title: None,
            session_id: None,
            agent: None,
        },
    };
    for round in 0..200 {
        let status = if round % 2 == 0 {
            AgentStatus::Working
        } else {
            AgentStatus::Idle
        };
        rig.host
            .update(cx, |host, cx| host.push_to_all(&agent(status), cx));
    }
    rig.host.update(cx, |host, cx| {
        host.push_to_all(&agent(AgentStatus::Finished), cx)
    });
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.sessions().len()),
        1,
        "the device asked for none of it, so it is not ended for it"
    );

    rig.relay.resume_delivery();
    let received = collect_until(&browser, &mut connected, cx, |received| {
        data_frames(received) == frames
    })
    .await;
    let updates: Vec<Control> = controls_of(&received)
        .into_iter()
        .filter(|control| matches!(control, Control::AgentUpdate { .. }))
        .collect();
    assert_eq!(
        updates,
        vec![agent(AgentStatus::Finished)],
        "told the latest, once"
    );
    assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 1);
}

#[gpui::test]
async fn a_device_that_asks_for_more_than_it_reads_is_still_ended(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = connected_over_a_narrow_relay(cx).await;
    let session_id = connected.session_id;
    rig.relay.pause_delivery();
    rig.host.update(cx, |host, _| {
        host.queue_waiting_bytes(session_id, 8 * 1024 * 1024 + 64 * 1024)
    });

    connected.send(&browser, &Control::Ping);
    cx.run_until_parked();
    assert!(
        rig.host.read_with(cx, |host, _| host.sessions().is_empty()),
        "what it asked for, and did not read, is its doing"
    );
}

#[gpui::test]
async fn signing_out_is_told_to_whatever_shows_the_trusted_devices(cx: &mut TestAppContext) {
    let rig = rig(cx, false).await;
    pin_in_storage(&[pinned("phone", 1)], cx).await;
    rig.host
        .update(cx, |host, cx| host.load_trusted_devices(cx));
    cx.run_until_parked();
    assert_eq!(listed_device_ids(&rig, cx), Some(vec!["phone".to_string()]));
    let notified = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let _observation = cx.update(|cx| {
        let notified = notified.clone();
        cx.observe(&rig.host, move |_, _| notified.set(notified.get() + 1))
    });

    rig.account
        .update(cx, |account, cx| account.sign_out(cx).detach());
    cx.run_until_parked();

    assert!(notified.get() > 0);
    assert!(rig.host.read_with(cx, |host, cx| matches!(
        host.trusted_devices_listing(cx),
        TrustedDevices::SignedOut
    )));
}

#[cfg(unix)]
mod project_server {
    use super::*;
    use crate::ide_bridge::{IdeLaunch, IdeLaunchOverride};
    use std::{os::unix::fs::PermissionsExt as _, path::Path};

    /// How long a test gives, in real time, to something outside the test's own
    /// process. A budget counted in scheduler polls is not a duration: what a
    /// poll costs depends on what the test does in it and on how busy the
    /// machine is.
    const REAL_TIME_LIMIT: Duration = Duration::from_secs(20);

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
                 sh -c 'echo $$ > \"$1\"; sleep 1000; :' daemon \"{state}/$3/server.pid\" >/dev/null 2>&1 </dev/null &\n\
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

        /// The pid the stand-in's proxy recorded for itself.
        fn proxy_pid(&self, identifier: &str) -> String {
            wait_for_pid(&self.path().join(format!("proxy-{identifier}.pid")))
        }

        /// The pid the stand-in's daemon recorded for itself.
        ///
        /// The daemon writes it from the process that already holds its final
        /// command line, as the real server does, and the host stops only a
        /// process that looks like the server. Stopping it any sooner -- the
        /// shell's `$!` is a process that has forked and not yet exec'd -- asks
        /// the host to stop something it rightly declines to touch, and the
        /// daemon then outlives the test.
        fn daemon_pid(&self, identifier: &str) -> String {
            wait_for_pid(&self.path().join(format!("state/{identifier}/server.pid")))
        }
    }

    /// Waits off the scheduler: the process that writes the file is not driven
    /// by it.
    fn wait_for_pid(file: &Path) -> String {
        let give_up = std::time::Instant::now() + REAL_TIME_LIMIT;
        loop {
            // A file that exists and is still empty is being written.
            let written = std::fs::read_to_string(file)
                .ok()
                .map(|text| text.trim().to_string())
                .filter(|pid| !pid.is_empty());
            if let Some(pid) = written {
                return pid;
            }
            assert!(
                std::time::Instant::now() < give_up,
                "{} was never written",
                file.display()
            );
            std::thread::sleep(Duration::from_millis(10));
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
        let give_up = std::time::Instant::now() + REAL_TIME_LIMIT;
        loop {
            cx.background_executor
                .timer(Duration::from_millis(10))
                .await;
            cx.run_until_parked();
            if done() {
                return true;
            }
            if std::time::Instant::now() >= give_up {
                return false;
            }
        }
    }

    /// A paired, greeted browser, with the stand-in as the project server.
    async fn session(ending: &str, cx: &mut TestAppContext) -> (Rig, Browser, Connected, Fixture) {
        let rig = rig(cx, true).await;
        session_on(rig, ending, cx).await
    }

    async fn session_on(
        rig: Rig,
        ending: &str,
        cx: &mut TestAppContext,
    ) -> (Rig, Browser, Connected, Fixture) {
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
        let proxy = fixture.proxy_pid(&identifier);
        let daemon = fixture.daemon_pid(&identifier);
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
        let proxy = fixture.proxy_pid(&identifier);
        let daemon = fixture.daemon_pid(&identifier);

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
        let proxy = fixture.proxy_pid(&fixture.identifier());

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
        let daemon = fixture.daemon_pid(&fixture.identifier());
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

    fn framed(envelope: &rpc::proto::Envelope) -> Vec<u8> {
        use rpc::proto::Message as _;
        let mut bytes = u32::try_from(envelope.encoded_len())
            .expect("a short message")
            .to_le_bytes()
            .to_vec();
        envelope.encode(&mut bytes).expect("encodes");
        bytes
    }

    /// What the controlling Zode's project client sends every few seconds.
    fn heartbeat(id: u32) -> Vec<u8> {
        framed(&rpc::proto::Envelope {
            id,
            payload: Some(rpc::proto::envelope::Payload::Ping(rpc::proto::Ping {})),
            ..Default::default()
        })
    }

    /// Anything else a project client says.
    fn project_request(id: u32) -> Vec<u8> {
        framed(&rpc::proto::Envelope {
            id,
            responding_to: Some(1),
            payload: Some(rpc::proto::envelope::Payload::Ack(rpc::proto::Ack {})),
            ..Default::default()
        })
    }

    #[gpui::test]
    async fn the_project_clients_heartbeat_does_not_keep_a_session_from_going_idle(
        cx: &mut TestAppContext,
    ) {
        let (rig, browser, mut connected, _fixture) = session("exec cat", cx).await;
        connected.controls(&browser);
        open(&browser, &mut connected, 3, 5, cx);
        set_remote_control(cx, true, Some(1));
        cx.run_until_parked();
        for id in 1..=30 {
            cx.executor().advance_clock(Duration::from_secs(5));
            connected.send_data(&browser, 5, &heartbeat(id));
            cx.run_until_parked();
        }
        assert!(
            rig.host.read_with(cx, |host, _| host.sessions().is_empty()),
            "two and a half minutes of pings and nothing else is silence"
        );
        assert!(browser.client.was_told_closed(connected.session_id));
    }

    #[gpui::test]
    async fn requests_on_the_project_stream_keep_a_session_from_going_idle(
        cx: &mut TestAppContext,
    ) {
        let (rig, browser, mut connected, _fixture) = session("exec cat", cx).await;
        connected.controls(&browser);
        open(&browser, &mut connected, 3, 5, cx);
        set_remote_control(cx, true, Some(1));
        cx.run_until_parked();
        for id in 1..=8 {
            cx.executor().advance_clock(Duration::from_secs(20));
            let mut bytes = heartbeat(id * 2);
            bytes.extend(project_request(id * 2 + 1));
            connected.send_data(&browser, 5, &bytes);
            cx.run_until_parked();
        }
        assert_eq!(
            rig.host.read_with(cx, |host, _| host.sessions().len()),
            1,
            "somebody is working in the project"
        );
    }

    #[gpui::test]
    async fn quitting_zode_stops_the_project_server_and_its_daemon(cx: &mut TestAppContext) {
        let rig = installed_rig(cx).await;
        let (_rig, browser, mut connected, fixture) = session_on(rig, "exec cat", cx).await;
        connected.controls(&browser);
        open(&browser, &mut connected, 3, 5, cx);
        assert!(echoed(&browser, &mut connected, 5, b"up\n", cx).await);
        let identifier = fixture.identifier();
        let proxy = fixture.proxy_pid(&identifier);
        let daemon = fixture.daemon_pid(&identifier);
        assert!(is_running(&proxy) && is_running(&daemon));

        cx.update(|cx| cx.shutdown());
        assert!(
            wait_for(cx, || !is_running(&proxy) && !is_running(&daemon)).await,
            "nothing a device started may outlive Zode"
        );
    }

    #[gpui::test]
    async fn the_end_of_a_project_stream_is_not_lost_when_the_relay_is_full(
        cx: &mut TestAppContext,
    ) {
        let rig = rig(cx, false).await;
        rig.relay.set_outbound_capacity(6);
        set_remote_control(cx, true, None);
        cx.run_until_parked();
        let (rig, browser, mut connected, _fixture) = session_on(rig, "exec cat", cx).await;
        connected.controls(&browser);
        open(&browser, &mut connected, 3, 5, cx);
        assert!(echoed(&browser, &mut connected, 5, b"up\n", cx).await);

        rig.relay.pause_delivery();
        rig.host.update(cx, |host, _| {
            host.queue_waiting_bytes(connected.session_id, 1_000_000)
        });
        rig.host.update(cx, |host, cx| host.flush_all(cx));
        assert!(!rig.host.read_with(cx, |host, cx| host.relay_has_room(cx)));
        // The device is done with the project server.
        connected.send_data(&browser, 5, &[]);
        cx.run_until_parked();

        rig.relay.resume_delivery();
        let received = collect_until(&browser, &mut connected, cx, |received| {
            received.iter().any(|(kind, stream, payload)| {
                *kind == InnerKind::Data && *stream == 5 && payload.is_empty()
            })
        })
        .await;
        assert!(
            received
                .iter()
                .any(|(kind, stream, payload)| *kind == InnerKind::Data
                    && *stream == 5
                    && payload.is_empty()),
            "the stream's end was sent once there was room"
        );
        assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 1);
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
        let first_daemon = fixture.daemon_pid(&first);

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
        let second_daemon = fixture.daemon_pid(&second);
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
        let genuine = fixture.daemon_pid(&identifier);

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
            wait_for(cx, || {
                echoed_back = incoming_rx.try_recv().ok();
                echoed_back.is_some()
            })
            .await;
            assert_eq!(echoed_back.map(|reply| reply.id), Some(41));
            assert!(fixture.started());

            // A reconnect cannot reuse the stream, and a new one would know
            // nothing of what the client holds: the client is told the project
            // server is gone, which is how it reports that to the person.
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
            assert_eq!(
                second.await.expect("a reconnect is answered, not failed"),
                remote::proxy::ProxyLaunchError::ServerNotRunning.to_exit_code()
            );
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
            let io = connection.start_proxy(
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
            let proxy = fixture.proxy_pid(&identifier);
            let daemon = fixture.daemon_pid(&identifier);
            assert!(is_running(&proxy) && is_running(&daemon));

            // A window that gives a connection up has let go of its own proxy
            // by then; a connection another window still uses is not killed.
            drop(io);
            cx.run_until_parked();
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
mod session_requests;
