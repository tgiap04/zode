//! A host a test can steer, built from the same pieces a real one is: the
//! responder handshake table, the secure channel and the pairing gate. It lets
//! the initiator be held against the code that answers it in production without
//! a whole Zode behind it.

use std::{collections::HashMap, sync::Arc};

use gpui::{AppContext as _, Context, Entity, Subscription};
use remote_relay_protocol::{
    Control, DeviceKeypair, InnerKind, PairingGate, PairingMessage, RELAY_PROTOCOL_VERSION,
    RPC_PROTOCOL_VERSION, build_prologue, random_nonce,
};

use crate::{
    OpenMode, RelayClient, RelayEvent, RelayRole, RelayTransport,
    secure_session::{ResponderHandshakes, SecureChannel, read_control},
    test_support::StaticCredentials,
};

type Responder = Box<dyn FnMut(&Control) -> Vec<Control>>;

struct HostedSession {
    peer_device_id: String,
    channel: SecureChannel,
}

/// What a test sees of the pairing the host is part of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptedPairing {
    None,
    /// Waiting for the person at the host to compare `digits`.
    Comparing {
        digits: String,
    },
}

pub struct ScriptedHost {
    keypair: Arc<DeviceKeypair>,
    user_id: String,
    device_id: String,
    client: Entity<RelayClient>,
    pins: HashMap<String, [u8; 32]>,
    handshakes: ResponderHandshakes,
    opened: HashMap<u32, (String, OpenMode)>,
    sessions: HashMap<u32, HostedSession>,
    gate: PairingGate,
    pair_session: Option<u32>,
    pair_outcome: Option<([u8; 32], String, String)>,
    responder: Option<Responder>,
    silent_on_hello: bool,
    received_controls: Vec<(u32, Control)>,
    received_data: Vec<(u32, u32, Vec<u8>)>,
    ended_sessions: Vec<u32>,
    _subscription: Subscription,
}

impl ScriptedHost {
    /// A host that connects to `relay` as `host-device` with `keypair`.
    pub fn new(
        transport: Arc<dyn RelayTransport>,
        user_id: &str,
        keypair: Arc<DeviceKeypair>,
        cx: &mut Context<Self>,
    ) -> Self {
        let client = cx.new(|cx| {
            RelayClient::new_as(
                Arc::new(StaticCredentials::new(user_id, "host-device", "host-token")),
                transport,
                "https://api.test/api",
                RelayRole::Host,
                cx,
            )
        });
        let subscription = cx.subscribe(&client, |this, _client, event: &RelayEvent, cx| {
            this.on_event(event.clone(), cx);
        });
        Self {
            keypair,
            user_id: user_id.to_string(),
            device_id: "host-device".to_string(),
            client,
            pins: HashMap::new(),
            handshakes: ResponderHandshakes::default(),
            opened: HashMap::new(),
            sessions: HashMap::new(),
            gate: PairingGate::new(),
            pair_session: None,
            pair_outcome: None,
            responder: None,
            silent_on_hello: false,
            received_controls: Vec::new(),
            received_data: Vec::new(),
            ended_sessions: Vec::new(),
            _subscription: subscription,
        }
    }

    pub fn public_key(&self) -> [u8; 32] {
        *self.keypair.public_key()
    }

    /// Trusts `device_id` as the holder of `public_key`, as a finished
    /// pairing would.
    pub fn trust(&mut self, device_id: &str, public_key: [u8; 32]) {
        self.pins.insert(device_id.to_string(), public_key);
    }

    pub fn trusts(&self, device_id: &str) -> bool {
        self.pins.contains_key(device_id)
    }

    /// Controls the host sends in answer to each control it receives, after
    /// `hello`.
    /// Leaves `hello` unanswered, as a host that is up but wedged does.
    pub fn stay_silent_on_hello(&mut self) {
        self.silent_on_hello = true;
    }

    pub fn respond_with(&mut self, responder: impl FnMut(&Control) -> Vec<Control> + 'static) {
        self.responder = Some(Box::new(responder));
    }

    pub fn pairing(&self) -> ScriptedPairing {
        match &self.pair_outcome {
            Some((_, digits, _)) => ScriptedPairing::Comparing {
                digits: digits.clone(),
            },
            None => ScriptedPairing::None,
        }
    }

    pub fn received_controls(&self) -> &[(u32, Control)] {
        &self.received_controls
    }

    pub fn received_data(&self) -> &[(u32, u32, Vec<u8>)] {
        &self.received_data
    }

    /// Sessions the relay or the peer ended, by id.
    pub fn ended_sessions(&self) -> &[u32] {
        &self.ended_sessions
    }

    pub fn session_ids(&self) -> Vec<u32> {
        self.sessions.keys().copied().collect()
    }

    pub fn client(&self) -> &Entity<RelayClient> {
        &self.client
    }

    /// The person at the host answers the digits on screen.
    pub fn decide_pairing(&mut self, trust: bool, cx: &mut Context<Self>) {
        let Some((peer_key, _, peer_device_id)) = self.pair_outcome.take() else {
            return;
        };
        if trust {
            self.gate.confirm_digits();
            self.pins.insert(peer_device_id, peer_key);
        } else {
            self.gate.reject_digits();
        }
        if let Some(session_id) = self.pair_session.take() {
            self.close_at_relay(session_id, cx);
        }
    }

    fn close_at_relay(&mut self, session_id: u32, cx: &mut Context<Self>) {
        if let Err(error) = self.client.read(cx).close_session(session_id) {
            log::debug!("the scripted host could not close a session: {error}");
        }
        self.forget(session_id);
    }

    fn forget(&mut self, session_id: u32) {
        self.opened.remove(&session_id);
        self.handshakes.cancel(session_id);
        self.sessions.remove(&session_id);
        if self.pair_session == Some(session_id) {
            self.pair_session = None;
            self.pair_outcome = None;
        }
    }

    pub fn send_control(&mut self, session_id: u32, control: &Control, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        match session.channel.seal_control(control) {
            Ok(sealed) => self.write(session_id, &sealed, cx),
            Err(error) => log::error!("the scripted host could not seal a control: {error}"),
        }
    }

    pub fn send_data(
        &mut self,
        session_id: u32,
        stream_id: u32,
        bytes: &[u8],
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        let sealed = if bytes.is_empty() {
            session.channel.seal_end_of_stream(stream_id)
        } else {
            session.channel.seal_data(stream_id, bytes.to_vec())
        };
        match sealed {
            Ok(sealed) => self.write(session_id, &sealed, cx),
            Err(error) => log::error!("the scripted host could not seal data: {error}"),
        }
    }

    fn write(&mut self, session_id: u32, sealed: &[u8], cx: &mut Context<Self>) {
        if let Err(error) = self.client.read(cx).send_frame(session_id, sealed) {
            log::warn!("the scripted host could not write: {error}");
        }
    }

    fn on_event(&mut self, event: RelayEvent, cx: &mut Context<Self>) {
        match event {
            RelayEvent::SessionOpened {
                session_id,
                peer_device_id,
                mode,
            } => self.on_opened(session_id, peer_device_id, mode, cx),
            RelayEvent::SessionClosed { session_id, .. } => {
                self.ended_sessions.push(session_id);
                self.forget(session_id);
            }
            RelayEvent::Pairing {
                session_id,
                message,
            } => self.on_pairing(session_id, message, cx),
            RelayEvent::Frame {
                session_id,
                payload,
            } => self.on_frame(session_id, &payload, cx),
            _ => {}
        }
    }

    fn on_opened(&mut self, session_id: u32, peer: String, mode: OpenMode, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        match mode {
            OpenMode::Pair => {
                self.opened.insert(session_id, (peer, mode));
            }
            OpenMode::Session => {
                let Some(key) = self.pins.get(&peer).copied() else {
                    self.close_at_relay(session_id, cx);
                    return;
                };
                let Ok(prologue) = build_prologue(&self.user_id, &peer, &self.device_id) else {
                    self.close_at_relay(session_id, cx);
                    return;
                };
                if self
                    .handshakes
                    .expect(session_id, key, prologue, now)
                    .is_err()
                {
                    self.close_at_relay(session_id, cx);
                    return;
                }
                self.opened.insert(session_id, (peer, mode));
            }
        }
    }

    fn on_pairing(&mut self, session_id: u32, message: PairingMessage, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let Some((peer, OpenMode::Pair)) = self.opened.get(&session_id).cloned() else {
            return;
        };
        match message {
            PairingMessage::PairRequest {
                public_key,
                commitment,
            } => {
                let Ok(nonce) = random_nonce() else {
                    return;
                };
                match self.gate.begin(
                    *self.keypair.public_key(),
                    nonce,
                    public_key,
                    commitment,
                    now,
                ) {
                    Ok(accept) => {
                        if let Err(error) = self.client.read(cx).send_pairing(session_id, &accept) {
                            log::warn!("the scripted host could not accept: {error}");
                        }
                    }
                    Err(_) => self.close_at_relay(session_id, cx),
                }
            }
            PairingMessage::PairReveal { nonce } => match self.gate.reveal(nonce, now) {
                Ok(outcome) => {
                    self.pair_session = Some(session_id);
                    self.pair_outcome = Some((
                        outcome.peer_public_key,
                        outcome.short_authentication_string,
                        peer,
                    ));
                }
                Err(_) => self.close_at_relay(session_id, cx),
            },
            PairingMessage::PairAccept { .. } => {}
        }
    }

    fn on_frame(&mut self, session_id: u32, payload: &[u8], cx: &mut Context<Self>) {
        if self.sessions.contains_key(&session_id) {
            self.on_ciphertext(session_id, payload, cx);
            return;
        }
        let now = cx.background_executor().now();
        let Some((peer, _)) = self.opened.remove(&session_id) else {
            return;
        };
        match self
            .handshakes
            .accept(session_id, self.keypair.private_key(), payload, now)
        {
            Ok((session, reply)) => {
                self.write(session_id, &reply, cx);
                self.sessions.insert(
                    session_id,
                    HostedSession {
                        peer_device_id: peer,
                        channel: SecureChannel::new(session),
                    },
                );
            }
            Err(_) => self.close_at_relay(session_id, cx),
        }
    }

    fn on_ciphertext(&mut self, session_id: u32, payload: &[u8], cx: &mut Context<Self>) {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        let frame = match session.channel.open(payload) {
            Ok(frame) => frame,
            Err(_) => {
                self.close_at_relay(session_id, cx);
                return;
            }
        };
        if frame.kind == InnerKind::Data {
            self.received_data
                .push((session_id, frame.stream_id, frame.payload));
            return;
        }
        let Ok(control) = read_control(&frame) else {
            return;
        };
        let mut replies = Vec::new();
        match &control {
            Control::Hello { .. } if self.silent_on_hello => {}
            Control::Hello { .. } => {
                replies.push(Control::HelloAck {
                    relay_protocol: RELAY_PROTOCOL_VERSION,
                    app_version: "9.9.9".into(),
                    rpc_protocol: RPC_PROTOCOL_VERSION,
                    capabilities: vec!["terminal".into(), "ide".into()],
                });
                replies.push(Control::AgentList { agents: vec![] });
                replies.push(Control::TerminalList { terminals: vec![] });
            }
            Control::Ping => replies.push(Control::Pong),
            other => {
                if let Some(responder) = self.responder.as_mut() {
                    replies.extend(responder(other));
                }
            }
        }
        self.received_controls.push((session_id, control));
        for reply in replies {
            self.send_control(session_id, &reply, cx);
        }
    }

    /// Who this host believes is on `session_id`.
    pub fn peer_of(&self, session_id: u32) -> Option<&str> {
        self.sessions
            .get(&session_id)
            .map(|session| session.peer_device_id.as_str())
    }
}
