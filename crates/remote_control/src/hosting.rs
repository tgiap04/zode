//! Everything that exists only while the host is connected to the relay: the
//! sessions the relay has opened, which of them are paired and which are
//! encrypted, and what each is allowed to do.
//!
//! Dropping a [`Hosting`] is how remote control is switched off. The relay
//! client goes with it, which closes the socket, and so does everything
//! hanging off the mirrors -- which is why nothing here may be kept anywhere
//! else.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures::{FutureExt as _, select_biased};
use gpui::{Context, Entity, Subscription, Task, WeakEntity};
use remote_relay_client::{
    CredentialError, OpenMode, PinnedDevice, RelayClient, RelayCredential, RelayCredentials,
    SendError, TrustStore, list_devices,
    secure_session::{ResponderHandshakes, SecureChannel},
};
use remote_relay_protocol::{
    Control, DeviceKeypair, InnerFrame, InnerKind, MAX_INNER_PAYLOAD_LEN, PAIRING_EXPIRY,
    PairingMessage, build_prologue, decode_control, encode_control, error_code, random_nonce,
};

use crate::{
    agent_mirror::AgentMirror,
    file_browse::{self, BrowseError, FileBrowser, FileReply, FileRequest},
    ide_bridge::{self, IdeBridge, IdeOutput},
    pairing_flow::PairingFlow,
    remote_control_settings::RemoteControlSettings,
    remote_host::{RemoteHost, RemoteHostEvent},
    session_router::{
        HOST_STREAM_BASE, HostFacts, MAX_ATTACHMENTS_PER_SESSION, RouterAction, RouterState,
        route_control,
    },
    terminal_mirror::{AttachError, TerminalMirror},
};

/// How often waiting handshakes, abandoned pairings and quiet sessions are
/// looked at.
pub(crate) const HOUSEKEEPING_INTERVAL: Duration = Duration::from_secs(5);

/// How long to wait before trying to send queued output again once the relay
/// connection had no room for it.
const FLUSH_RETRY: Duration = Duration::from_millis(25);

/// How long to wait for the account service to name a device before asking
/// the person to compare codes without one.
const NAME_LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

/// Devices that may be in control at once: the per-host allowance the relay
/// enforces for an account. Only sessions whose peer has proven it holds the
/// keys count, so a relay replaying handshake messages cannot use up the
/// places real devices need.
const MAX_ACTIVE_SESSIONS: usize = 4;

/// Handshakes answered whose peer has not yet proven itself. Capped on their
/// own, and short-lived: see [`CONFIRMATION_DEADLINE`].
const MAX_UNCONFIRMED_SESSIONS: usize = 4;

/// How long a peer has, after the host answers its handshake, to prove it holds
/// the keys. Independent of the idle setting, which may be off.
const CONFIRMATION_DEADLINE: Duration = Duration::from_secs(10);

/// Sessions the relay has opened that have not finished becoming anything,
/// across both kinds. The relay can open them for free, so they are counted.
const MAX_UNFINISHED_SESSIONS: usize = 16;

/// File requests one device may have being worked on at once.
const MAX_FILE_REQUESTS_PER_SESSION: usize = 4;

/// Bytes of answers one device may have waiting for the relay. A request is
/// let in only if the answers already waiting, plus the largest answer each
/// request being worked on may yet produce, fit under this; a device that asks
/// faster than it reads is told to slow down instead of being buffered.
const MAX_QUEUED_REPLY_BYTES: usize = 8 * 1024 * 1024;

/// The most one answer can weigh: a diff is the largest thing sent, and a
/// listing or a file is smaller.
const MAX_REPLY_BYTES: usize = file_browse::MAX_DIFF_BYTES;

/// Stands in for the size of a control message that could not be measured.
const CONTROL_WEIGHT_FALLBACK: usize = 1024;

/// What a device that has used up its queue may still have refused, in small
/// messages, before it is judged to be ignoring what it is told and is ended.
const REFUSAL_ALLOWANCE_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum OpenedKind {
    Pair,
    Handshake,
}

struct Opened {
    peer_device_id: String,
    kind: OpenedKind,
    opened_at: Instant,
}

pub(crate) struct ActiveSession {
    pub(crate) peer_device_id: String,
    pub(crate) peer_name: String,
    channel: SecureChannel,
    router: RouterState,
    pub(crate) last_activity: Instant,
    established_at: Instant,
    /// Client-chosen stream id to the terminal it is attached to.
    streams: HashMap<u32, String>,
    /// Set once the peer has proven it holds the session keys, which is when
    /// the session starts to count as a device being in control.
    pub(crate) confirmed: bool,
    /// The project server this device asked for, at most one. Dropping it
    /// stops the server, so ending the session needs nothing more.
    ide: Option<IdeBridge>,
    /// Answers to file requests, in the order the device must see them.
    outbound: VecDeque<Outbound>,
    outbound_bytes: usize,
    /// Work answering file requests, by ticket. Ending the session drops it,
    /// which cancels the work and its deadline.
    file_tasks: HashMap<u64, Task<()>>,
    next_host_stream: u32,
}

/// One frame of an answer, sealed only when the relay has room for it.
enum Outbound {
    Control(Control, usize),
    Data { stream_id: u32, bytes: Vec<u8> },
    EndOfStream { stream_id: u32 },
}

impl Outbound {
    /// What it counts for against the queue's allowance.
    fn weight(&self) -> usize {
        match self {
            Outbound::Control(_, weight) => *weight,
            Outbound::Data { bytes, .. } => bytes.len(),
            Outbound::EndOfStream { .. } => 0,
        }
    }

    fn control(control: Control) -> Self {
        let weight = encode_control(&control).map_or(CONTROL_WEIGHT_FALLBACK, |bytes| bytes.len());
        Outbound::Control(control, weight)
    }
}

impl ActiveSession {
    fn enqueue(&mut self, item: Outbound) {
        self.outbound_bytes += item.weight();
        self.outbound.push_back(item);
    }

    /// Queues `reply`, which names a new stream, then `bytes` on that stream
    /// cut into frames, then the end of the stream.
    fn enqueue_stream(&mut self, bytes: Vec<u8>, reply: impl FnOnce(u32) -> Control) {
        let stream_id = self.next_host_stream;
        self.next_host_stream = match stream_id.checked_add(1) {
            Some(next) => next,
            None => HOST_STREAM_BASE,
        };
        self.enqueue(Outbound::control(reply(stream_id)));
        for chunk in bytes.chunks(MAX_INNER_PAYLOAD_LEN) {
            self.enqueue(Outbound::Data {
                stream_id,
                bytes: chunk.to_vec(),
            });
        }
        self.enqueue(Outbound::EndOfStream { stream_id });
    }
}

pub(crate) struct Hosting {
    pub(crate) user_id: String,
    pub(crate) keypair: Arc<DeviceKeypair>,
    pub(crate) trust: TrustStore,
    pub(crate) client: Entity<RelayClient>,
    pub(crate) credentials: Arc<dyn RelayCredentials>,
    http_client: Arc<dyn http_client::HttpClient>,
    api_url: String,
    pub(crate) agents: Entity<AgentMirror>,
    pub(crate) terminals: Entity<TerminalMirror>,
    files: Option<Entity<FileBrowser>>,
    /// Numbers the file requests of every session, so that an answer can tell
    /// whether the request it answers is still waiting.
    next_file_ticket: u64,
    pub(crate) pairing: PairingFlow,
    handshakes: ResponderHandshakes,
    opened: HashMap<u32, Opened>,
    pub(crate) sessions: HashMap<u32, ActiveSession>,
    app_version: String,
    /// The one question on screen has been put in front of somebody.
    pub(crate) pairing_presented: Option<u32>,
    flush_retry: Option<Task<()>>,
    name_lookup: Option<Task<()>>,
    pub(crate) _housekeeping: Task<()>,
    pub(crate) _subscriptions: Vec<Subscription>,
}

pub(crate) struct HostingParts {
    pub(crate) user_id: String,
    pub(crate) keypair: Arc<DeviceKeypair>,
    pub(crate) trust: TrustStore,
    pub(crate) client: Entity<RelayClient>,
    pub(crate) credentials: Arc<dyn RelayCredentials>,
    pub(crate) http_client: Arc<dyn http_client::HttpClient>,
    pub(crate) api_url: String,
    pub(crate) agents: Entity<AgentMirror>,
    pub(crate) terminals: Entity<TerminalMirror>,
    pub(crate) files: Option<Entity<FileBrowser>>,
    pub(crate) app_version: String,
    pub(crate) housekeeping: Task<()>,
    pub(crate) subscriptions: Vec<Subscription>,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn error_control(code: &str, message: &str) -> Control {
    Control::Error {
        code: code.to_string(),
        message: message.to_string(),
        request_id: None,
    }
}

impl Hosting {
    pub(crate) fn new(parts: HostingParts) -> Self {
        Self {
            user_id: parts.user_id,
            keypair: parts.keypair,
            trust: parts.trust,
            client: parts.client,
            credentials: parts.credentials,
            http_client: parts.http_client,
            api_url: parts.api_url,
            agents: parts.agents,
            terminals: parts.terminals,
            files: parts.files,
            next_file_ticket: 0,
            pairing: PairingFlow::default(),
            handshakes: ResponderHandshakes::default(),
            opened: HashMap::default(),
            sessions: HashMap::default(),
            app_version: parts.app_version,
            pairing_presented: None,
            flush_retry: None,
            name_lookup: None,
            _housekeeping: parts.housekeeping,
            _subscriptions: parts.subscriptions,
        }
    }

    fn now(cx: &gpui::App) -> Instant {
        // The scheduler's own clock, so a test can move it. In the running app
        // it is `Instant::now()`.
        cx.background_executor().now()
    }

    fn local_device_id(&self, cx: &gpui::App) -> Option<String> {
        self.client
            .read(cx)
            .credential()
            .map(|credential| credential.device_id.clone())
    }

    fn close_at_relay(&self, session_id: u32, cx: &gpui::App) {
        if let Err(error) = self.client.read(cx).close_session(session_id) {
            // The relay still routes a session the host has forgotten until its
            // own timeouts end it; nothing arriving on it is acted on.
            log::warn!("could not tell the relay to close session {session_id}: {error}");
        }
    }

    fn confirmed_count(&self) -> usize {
        self.sessions
            .values()
            .filter(|session| session.confirmed)
            .count()
    }

    /// Forgets everything this side holds about a session. Does not tell the
    /// relay. `true` if it was an established session.
    pub(crate) fn forget_session(&mut self, session_id: u32, cx: &mut Context<RemoteHost>) -> bool {
        self.opened.remove(&session_id);
        self.handshakes.cancel(session_id);
        let was_active = self.sessions.remove(&session_id).is_some();
        self.terminals
            .update(cx, |terminals, cx| terminals.detach_session(session_id, cx));
        self.pairing.session_gone(session_id);
        if self.pairing_presented == Some(session_id) {
            self.pairing_presented = None;
        }
        cx.emit(RemoteHostEvent::Changed);
        was_active
    }

    /// Ends a session on both sides.
    pub(crate) fn end_session(&mut self, session_id: u32, cx: &mut Context<RemoteHost>) {
        self.close_at_relay(session_id, cx);
        self.forget_session(session_id, cx);
    }

    /// The kill switch: every session of every kind.
    pub(crate) fn end_all_sessions(&mut self, cx: &mut Context<RemoteHost>) {
        let mut ids: HashSet<u32> = self.sessions.keys().copied().collect();
        ids.extend(self.opened.keys().copied());
        if let Some(pairing_session) = self.pairing.active_session() {
            ids.insert(pairing_session);
        }
        for session_id in ids {
            self.end_session(session_id, cx);
        }
    }

    /// The socket is gone, and the relay ended every session with it: forget
    /// them all without trying to say goodbye on a connection that is not there.
    pub(crate) fn forget_all_sessions(&mut self, cx: &mut Context<RemoteHost>) {
        let mut ids: HashSet<u32> = self.sessions.keys().copied().collect();
        ids.extend(self.opened.keys().copied());
        if let Some(pairing_session) = self.pairing.active_session() {
            ids.insert(pairing_session);
        }
        for session_id in ids {
            self.forget_session(session_id, cx);
        }
    }

    pub(crate) fn end_sessions_of(&mut self, device_id: &str, cx: &mut Context<RemoteHost>) {
        let ids: Vec<u32> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.peer_device_id == device_id)
            .map(|(id, _)| *id)
            .chain(
                self.opened
                    .iter()
                    .filter(|(_, opened)| opened.peer_device_id == device_id)
                    .map(|(id, _)| *id),
            )
            .collect();
        for session_id in ids {
            self.end_session(session_id, cx);
        }
    }

    /// The relay has opened a session between this host and `peer`.
    pub(crate) fn on_session_opened(
        &mut self,
        session_id: u32,
        peer_device_id: String,
        mode: OpenMode,
        cx: &mut Context<RemoteHost>,
    ) {
        if self.opened.contains_key(&session_id) || self.sessions.contains_key(&session_id) {
            // Ending it would end the session that legitimately has this id.
            log::warn!("ignoring a second announcement of session {session_id}");
            return;
        }
        if self.opened.len() >= MAX_UNFINISHED_SESSIONS {
            log::warn!("refusing session {session_id}: too many are waiting");
            self.close_at_relay(session_id, cx);
            return;
        }
        let now = Self::now(cx);
        match mode {
            OpenMode::Pair => {
                self.opened.insert(
                    session_id,
                    Opened {
                        peer_device_id,
                        kind: OpenedKind::Pair,
                        opened_at: now,
                    },
                );
            }
            OpenMode::Session => {
                let Some(remote_public_key) = self
                    .trust
                    .get(&peer_device_id)
                    .map(|pinned| pinned.public_key)
                else {
                    // Nothing is asked and nothing is shown: a device that was
                    // never paired does not get to put a question on screen.
                    log::info!("refusing a session from a device that was never paired");
                    self.close_at_relay(session_id, cx);
                    return;
                };
                let Some(local_device_id) = self.local_device_id(cx) else {
                    self.close_at_relay(session_id, cx);
                    return;
                };
                let prologue =
                    match build_prologue(&self.user_id, &peer_device_id, &local_device_id) {
                        Ok(prologue) => prologue,
                        Err(error) => {
                            log::warn!(
                                "cannot build a handshake for session {session_id}: {error}"
                            );
                            self.close_at_relay(session_id, cx);
                            return;
                        }
                    };
                if let Err(error) =
                    self.handshakes
                        .expect(session_id, remote_public_key, prologue, now)
                {
                    log::warn!("refusing session {session_id}: {error}");
                    self.close_at_relay(session_id, cx);
                    return;
                }
                self.opened.insert(
                    session_id,
                    Opened {
                        peer_device_id,
                        kind: OpenedKind::Handshake,
                        opened_at: now,
                    },
                );
            }
        }
    }

    pub(crate) fn on_session_closed(&mut self, session_id: u32, cx: &mut Context<RemoteHost>) {
        self.forget_session(session_id, cx);
    }

    pub(crate) fn on_pairing(
        &mut self,
        session_id: u32,
        message: PairingMessage,
        cx: &mut Context<RemoteHost>,
    ) {
        let Some(opened) = self
            .opened
            .get(&session_id)
            .filter(|opened| opened.kind == OpenedKind::Pair)
        else {
            log::debug!("ignoring a pairing message on session {session_id}, which is not pairing");
            return;
        };
        let peer_device_id = opened.peer_device_id.clone();
        let now = Self::now(cx);
        match message {
            PairingMessage::PairRequest {
                public_key,
                commitment,
            } => {
                let nonce = match random_nonce() {
                    Ok(nonce) => nonce,
                    Err(error) => {
                        log::error!("no randomness for pairing: {error}");
                        self.end_session(session_id, cx);
                        return;
                    }
                };
                // An exchange that ran out of time is over, and its session is
                // closed now rather than whenever housekeeping next looks.
                if let Some(expired) = self.pairing.expire(now) {
                    self.end_session(expired, cx);
                }
                match self.pairing.request(
                    session_id,
                    &peer_device_id,
                    *self.keypair.public_key(),
                    nonce,
                    public_key,
                    commitment,
                    now,
                ) {
                    Ok(accept) => {
                        if let Err(error) = self.client.read(cx).send_pairing(session_id, &accept) {
                            log::warn!("could not answer a pairing request: {error}");
                            self.end_session(session_id, cx);
                        }
                    }
                    Err(refusal) => {
                        log::info!("refused a pairing request: {refusal:?}");
                        self.end_session(session_id, cx);
                    }
                }
            }
            PairingMessage::PairReveal { nonce } => {
                match self.pairing.reveal(session_id, nonce, now) {
                    Ok(revealed) => {
                        self.look_up_name_then_ask(session_id, revealed.peer_device_id, cx);
                    }
                    Err(refusal) => {
                        log::info!("pairing failed: {refusal:?}");
                        self.end_session(session_id, cx);
                    }
                }
            }
            // This side is the one that accepts; an accept is the other
            // side's message and means the peer is confused.
            PairingMessage::PairAccept { .. } => {}
        }
    }

    /// The code is known. The person is shown it with the device's name, which
    /// the account service holds, so the name is fetched first -- briefly: a
    /// name is a convenience and the code is the point.
    fn look_up_name_then_ask(
        &mut self,
        session_id: u32,
        peer_device_id: String,
        cx: &mut Context<RemoteHost>,
    ) {
        let credentials = self.credentials.clone();
        let http_client = self.http_client.clone();
        let api_url = self.api_url.clone();
        self.name_lookup = Some(cx.spawn(async move |this, cx| {
            let credential = credentials.credential(cx);
            let timeout = cx.background_executor().timer(NAME_LOOKUP_TIMEOUT).fuse();
            let lookup = Self::fetch_name(credential, http_client, api_url, peer_device_id).fuse();
            futures::pin_mut!(lookup, timeout);
            let name = select_biased! {
                name = lookup => name,
                _ = timeout => None,
            };
            this.update(cx, |this, cx| {
                let name = name.unwrap_or_else(|| "A browser".to_string());
                this.present_pairing(session_id, name, cx);
            })
            .ok();
        }));
    }

    async fn fetch_name(
        credential: Task<Result<RelayCredential, CredentialError>>,
        http_client: Arc<dyn http_client::HttpClient>,
        api_url: String,
        device_id: String,
    ) -> Option<String> {
        let credential = credential.await.ok()?;
        let devices = list_devices(&http_client, &api_url, &credential.bearer)
            .await
            .map_err(|error| log::debug!("could not look up the device's name: {error}"))
            .ok()?;
        devices
            .into_iter()
            .find(|device| device.device_id == device_id)
            .map(|device| device.name)
    }

    /// Puts the question on screen, if the exchange is still alive.
    pub(crate) fn present_pairing(
        &mut self,
        session_id: u32,
        peer_name: String,
        cx: &mut Context<RemoteHost>,
    ) {
        let now = Self::now(cx);
        if self.pairing.present(session_id, peer_name, now) {
            cx.emit(RemoteHostEvent::Changed);
        } else {
            self.end_session(session_id, cx);
        }
    }

    /// The person answered the question for `session_id` on screen.
    pub(crate) fn decide_pairing(
        &mut self,
        session_id: u32,
        trust: bool,
        cx: &mut Context<RemoteHost>,
    ) {
        let now = Self::now(cx);
        if let Some(expired) = self.pairing.expire(now) {
            self.end_session(expired, cx);
        }
        let Some(decision) = self.pairing.decide(session_id, trust, now) else {
            return;
        };
        self.pairing_presented = None;
        if trust {
            let device = PinnedDevice {
                device_id: decision.peer_device_id.clone(),
                public_key: decision.peer_public_key,
                name: decision.peer_name.clone(),
                paired_at: unix_now(),
            };
            if let Err(error) = self.trust.pin(device, cx) {
                log::error!("could not trust {}: {error}", decision.peer_name);
                cx.emit(RemoteHostEvent::Problem {
                    message: format!("Could not trust {}: {error}", decision.peer_name),
                    retry: false,
                });
            }
        }
        self.end_session(decision.session_id, cx);
    }

    /// A binary frame arrived on `session_id`.
    pub(crate) fn on_frame(
        &mut self,
        session_id: u32,
        payload: Vec<u8>,
        cx: &mut Context<RemoteHost>,
    ) {
        if self.sessions.contains_key(&session_id) {
            self.on_ciphertext(session_id, &payload, cx);
            return;
        }
        let waiting_for_handshake = self
            .opened
            .get(&session_id)
            .is_some_and(|opened| opened.kind == OpenedKind::Handshake);
        if !waiting_for_handshake {
            log::debug!("closing session {session_id}: it sent data it was never meant to");
            self.end_session(session_id, cx);
            return;
        }
        self.accept_handshake(session_id, &payload, cx);
    }

    fn accept_handshake(&mut self, session_id: u32, message: &[u8], cx: &mut Context<RemoteHost>) {
        let now = Self::now(cx);
        let Some(opened) = self.opened.remove(&session_id) else {
            return;
        };
        if self.confirmed_count() >= MAX_ACTIVE_SESSIONS {
            log::warn!("refusing a handshake: {MAX_ACTIVE_SESSIONS} devices are already connected");
            self.end_session(session_id, cx);
            return;
        }
        if self.sessions.len() - self.confirmed_count() >= MAX_UNCONFIRMED_SESSIONS {
            log::warn!("refusing a handshake: too many peers have yet to prove themselves");
            self.end_session(session_id, cx);
            return;
        }
        let (session, reply) =
            match self
                .handshakes
                .accept(session_id, self.keypair.private_key(), message, now)
            {
                Ok(accepted) => accepted,
                Err(error) => {
                    log::warn!("a handshake was refused: {error}");
                    self.end_session(session_id, cx);
                    return;
                }
            };
        let peer_name = self
            .trust
            .get(&opened.peer_device_id)
            .map(|pinned| pinned.name.clone())
            .unwrap_or_default();
        if let Err(error) = self.client.read(cx).send_frame(session_id, &reply) {
            log::warn!("could not answer a handshake: {error}");
            self.end_session(session_id, cx);
            return;
        }
        self.sessions.insert(
            session_id,
            ActiveSession {
                peer_device_id: opened.peer_device_id,
                peer_name,
                channel: SecureChannel::new(session),
                router: RouterState::default(),
                last_activity: now,
                established_at: now,
                streams: HashMap::default(),
                confirmed: false,
                ide: None,
                outbound: VecDeque::new(),
                outbound_bytes: 0,
                file_tasks: HashMap::default(),
                next_host_stream: HOST_STREAM_BASE,
            },
        );
    }

    fn on_ciphertext(&mut self, session_id: u32, payload: &[u8], cx: &mut Context<RemoteHost>) {
        let now = Self::now(cx);
        let confirmed_before = self.confirmed_count();
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        let frame = match session.channel.open(payload) {
            Ok(frame) => frame,
            Err(error) => {
                // A frame that does not authenticate ends the session for good.
                log::warn!("ending session {session_id}: {error}");
                self.end_session(session_id, cx);
                return;
            }
        };

        if !session.confirmed && session.channel.is_confirmed() {
            if confirmed_before >= MAX_ACTIVE_SESSIONS {
                // Several peers can be waiting to prove themselves at once;
                // only as many as there are places may succeed.
                log::warn!(
                    "ending session {session_id}: {MAX_ACTIVE_SESSIONS} devices are already connected"
                );
                self.end_session(session_id, cx);
                return;
            }
            session.confirmed = true;
            session.last_activity = now;
            let device_name = session.peer_name.clone();
            cx.emit(RemoteHostEvent::SessionStarted { device_name });
            cx.emit(RemoteHostEvent::Changed);
        }

        match frame.kind {
            InnerKind::Control => self.on_control(session_id, frame, cx),
            InnerKind::Data => self.on_data(session_id, frame, now, cx),
        }
    }

    fn on_control(&mut self, session_id: u32, frame: InnerFrame, cx: &mut Context<RemoteHost>) {
        let control = match decode_control(&frame.payload) {
            Ok(control) => control,
            Err(error) => {
                log::debug!("a device sent a control message that does not parse: {error}");
                self.send_control(
                    session_id,
                    &error_control(
                        error_code::MALFORMED,
                        "that is not a message this host reads",
                    ),
                    cx,
                );
                return;
            }
        };
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        let was_greeted = session.router.hello_done();
        let facts = HostFacts {
            app_version: &self.app_version,
            ide_available: ide_bridge::locate(cx).is_some(),
            files_available: self.files.is_some(),
            proto_version: rpc::PROTOCOL_VERSION,
        };
        let actions = route_control(&mut session.router, control, &facts);
        let greeted_now = !was_greeted && session.router.hello_done();

        for action in actions {
            if !self.sessions.contains_key(&session_id) {
                return;
            }
            self.apply(session_id, action, cx);
        }
        if greeted_now && self.sessions.contains_key(&session_id) {
            self.send_lists(session_id, cx);
        }
    }

    fn send_lists(&mut self, session_id: u32, cx: &mut Context<RemoteHost>) {
        let agents = self.agents.read(cx).summaries(cx);
        let terminals = self.terminals.read(cx).summaries(cx);
        if self.send_control(session_id, &Control::AgentList { agents }, cx) {
            self.send_control(session_id, &Control::TerminalList { terminals }, cx);
        }
    }

    fn apply(&mut self, session_id: u32, action: RouterAction, cx: &mut Context<RemoteHost>) {
        match action {
            RouterAction::Reply(control) => {
                self.send_control(session_id, &control, cx);
            }
            RouterAction::Refuse(control) => {
                self.send_control(session_id, &control, cx);
                self.end_session(session_id, cx);
            }
            RouterAction::Attach {
                terminal_id,
                stream_id,
            } => self.attach(session_id, terminal_id, stream_id, cx),
            RouterAction::Detach { terminal_id } => {
                self.terminals.update(cx, |terminals, cx| {
                    terminals.detach(session_id, &terminal_id, cx)
                });
                if let Some(session) = self.sessions.get_mut(&session_id) {
                    session
                        .streams
                        .retain(|_, attached| *attached != terminal_id);
                }
            }
            RouterAction::OpenIde {
                request_id,
                stream_id,
            } => self.open_ide(session_id, request_id, stream_id, cx),
            RouterAction::Files {
                request_id,
                request,
            } => self.serve_files(session_id, request_id, request, cx),
            RouterAction::ReportSize { terminal_id } => {
                let size = self
                    .terminals
                    .read(cx)
                    .summaries(cx)
                    .into_iter()
                    .find(|summary| summary.id == terminal_id);
                let control = match size {
                    Some(summary) => Control::TerminalResized {
                        terminal_id,
                        columns: summary.columns,
                        rows: summary.rows,
                    },
                    None => error_control(error_code::NOT_FOUND, "there is no such terminal"),
                };
                self.send_control(session_id, &control, cx);
            }
        }
    }

    fn attach(
        &mut self,
        session_id: u32,
        terminal: String,
        stream_id: u32,
        cx: &mut Context<RemoteHost>,
    ) {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        if session
            .ide
            .as_ref()
            .is_some_and(|ide| ide.stream_id == stream_id)
        {
            self.send_control(
                session_id,
                &error_control(error_code::MALFORMED, "that stream is already in use"),
                cx,
            );
            return;
        }
        let replaces_own = session
            .streams
            .get(&stream_id)
            .is_some_and(|attached| *attached == terminal);
        if session.streams.contains_key(&stream_id) && !replaces_own {
            self.send_control(
                session_id,
                &error_control(error_code::MALFORMED, "that stream is already in use"),
                cx,
            );
            return;
        }
        let already_attached = session
            .streams
            .values()
            .any(|attached| *attached == terminal);
        if !already_attached && session.streams.len() >= MAX_ATTACHMENTS_PER_SESSION {
            self.send_control(
                session_id,
                &error_control(error_code::TOO_LARGE, "too many terminals are attached"),
                cx,
            );
            return;
        }
        let result = self.terminals.update(cx, |terminals, cx| {
            terminals.attach(session_id, &terminal, stream_id, cx)
        });
        match result {
            Ok((columns, rows)) => {
                let now = Self::now(cx);
                if let Some(session) = self.sessions.get_mut(&session_id) {
                    session.last_activity = now;
                    // Attaching again moves the terminal to the new stream.
                    session.streams.retain(|_, attached| *attached != terminal);
                    session.streams.insert(stream_id, terminal.clone());
                }
                let attached = Control::TerminalAttached {
                    terminal_id: terminal,
                    stream_id,
                    columns,
                    rows,
                };
                if self.send_control(session_id, &attached, cx) {
                    self.flush(session_id, cx);
                }
            }
            Err(AttachError::NotFound) => {
                self.send_control(
                    session_id,
                    &error_control(error_code::NOT_FOUND, "there is no such terminal"),
                    cx,
                );
            }
            Err(AttachError::Inactive) => {
                self.send_control(
                    session_id,
                    &error_control(error_code::INTERNAL, "terminals are not being shared"),
                    cx,
                );
            }
        }
    }

    fn on_data(
        &mut self,
        session_id: u32,
        frame: InnerFrame,
        now: Instant,
        cx: &mut Context<RemoteHost>,
    ) {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        if session
            .ide
            .as_ref()
            .is_some_and(|ide| ide.stream_id == frame.stream_id)
        {
            self.on_ide_data(session_id, frame, now, cx);
            return;
        }
        let Some(terminal) = session.streams.get(&frame.stream_id).cloned() else {
            log::debug!("input on a stream that is not attached");
            return;
        };
        if frame.payload.is_empty() {
            // The end-of-stream frame: the device is done with this terminal.
            session.streams.remove(&frame.stream_id);
            self.terminals.update(cx, |terminals, cx| {
                terminals.detach(session_id, &terminal, cx)
            });
            return;
        }
        // Only a session that has proven it holds the keys gets this far, and
        // what arrives is typed into the terminal exactly as received.
        let typed = self.terminals.update(cx, |terminals, cx| {
            terminals.input(session_id, &terminal, frame.payload, cx)
        });
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        // Only input that reached a terminal shows somebody is at the other
        // end; pings, lists and detaches are traffic, not use.
        if typed {
            session.last_activity = now;
        } else {
            session.streams.remove(&frame.stream_id);
        }
    }

    fn open_ide(
        &mut self,
        session_id: u32,
        request_id: u32,
        stream_id: u32,
        cx: &mut Context<RemoteHost>,
    ) {
        let refusal = |code: &str, message: &str| Control::Error {
            code: code.to_string(),
            message: message.to_string(),
            request_id: Some(request_id),
        };
        let Some(session) = self.sessions.get(&session_id) else {
            return;
        };
        if session.ide.is_some() {
            let refusal = refusal(
                error_code::TOO_LARGE,
                "this device already has a project connection open",
            );
            self.send_control(session_id, &refusal, cx);
            return;
        }
        if session.streams.contains_key(&stream_id) {
            let refusal = refusal(error_code::MALFORMED, "that stream is already in use");
            self.send_control(session_id, &refusal, cx);
            return;
        }
        let Some(launch) = ide_bridge::locate(cx) else {
            let refusal = refusal(
                crate::session_router::UNSUPPORTED,
                "this Zode was installed without its project server",
            );
            self.send_control(session_id, &refusal, cx);
            return;
        };
        let bridge = match IdeBridge::start(&launch, stream_id, cx) {
            Ok(bridge) => bridge,
            Err(error) => {
                log::warn!("could not start the project server: {error}");
                let refusal = refusal(error_code::INTERNAL, "the project server did not start");
                self.send_control(session_id, &refusal, cx);
                return;
            }
        };
        let now = Self::now(cx);
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.ide = Some(bridge);
            session.last_activity = now;
        }
        let path_style = if util::paths::PathStyle::local().is_windows() {
            "windows"
        } else {
            "posix"
        };
        let opened = Control::IdeOpened {
            request_id,
            stream_id: Some(stream_id),
            path_style: Some(path_style.to_string()),
            shell: Some(util::shell::get_system_shell()),
            default_shell: Some(util::shell::get_default_system_shell()),
        };
        self.send_control(session_id, &opened, cx);
    }

    fn on_ide_data(
        &mut self,
        session_id: u32,
        frame: InnerFrame,
        now: Instant,
        cx: &mut Context<RemoteHost>,
    ) {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        let stream_id = frame.stream_id;
        if frame.payload.is_empty() {
            // The device is done with the project server. The session stays:
            // it still mirrors.
            session.ide = None;
            self.send_end_of_stream(session_id, stream_id, cx);
            return;
        }
        let Some(ide) = session.ide.as_ref() else {
            return;
        };
        if ide.send_input(frame.payload).is_err() {
            log::warn!("ending session {session_id}: it sent more than the project server reads");
            self.end_session(session_id, cx);
            return;
        }
        session.last_activity = now;
    }

    fn serve_files(
        &mut self,
        session_id: u32,
        request_id: u32,
        request: FileRequest,
        cx: &mut Context<RemoteHost>,
    ) {
        let Some(files) = self.files.clone() else {
            let error = Control::Error {
                code: error_code::INTERNAL.to_string(),
                message: "this host cannot serve files".to_string(),
                request_id: Some(request_id),
            };
            self.queue_control(session_id, error, cx);
            return;
        };
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        let running = session.file_tasks.len();
        let reserved = (running + 1) * MAX_REPLY_BYTES;
        if running >= MAX_FILE_REQUESTS_PER_SESSION
            || session.outbound_bytes + reserved > MAX_QUEUED_REPLY_BYTES
        {
            let refusal = Control::Error {
                code: error_code::RATE_LIMITED.to_string(),
                message: "too many file requests are waiting; ask again when they are answered"
                    .to_string(),
                request_id: Some(request_id),
            };
            self.queue_control(session_id, refusal, cx);
            return;
        }
        let ticket = self.next_file_ticket;
        self.next_file_ticket += 1;
        let reply = files.update(cx, |files, cx| files.handle(request, cx));
        let task = cx.spawn(async move |this: WeakEntity<RemoteHost>, cx| {
            let reply = file_browse::answer_within(
                cx.background_executor()
                    .timer(file_browse::REQUEST_DEADLINE),
                reply,
            )
            .await;
            this.update(cx, |host, cx| {
                host.finish_file_request(session_id, ticket, request_id, reply, cx);
            })
            .ok();
        });
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.file_tasks.insert(ticket, task);
        }
    }

    /// Queues a small message behind whatever is already waiting for the
    /// device, so it arrives in order and a full relay connection delays it
    /// instead of ending the session. A device whose queue is this far over its
    /// allowance is ignoring what it is told, and is ended.
    fn queue_control(&mut self, session_id: u32, control: Control, cx: &mut Context<RemoteHost>) {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        if session.outbound_bytes >= MAX_QUEUED_REPLY_BYTES + REFUSAL_ALLOWANCE_BYTES {
            log::warn!("ending session {session_id}: it asks for more than it reads");
            self.end_session(session_id, cx);
            return;
        }
        session.enqueue(Outbound::control(control));
        self.flush(session_id, cx);
    }

    /// Queues the answer to a file request, if the session that asked for it
    /// is still the one that did.
    pub(crate) fn finish_file_request(
        &mut self,
        session_id: u32,
        ticket: u64,
        request_id: u32,
        reply: Result<FileReply, BrowseError>,
        cx: &mut Context<RemoteHost>,
    ) {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        if session.file_tasks.remove(&ticket).is_none() {
            return;
        }
        match reply {
            Err(error) => {
                let control = Control::Error {
                    code: error.code.to_string(),
                    message: error.message,
                    request_id: Some(request_id),
                };
                self.queue_control(session_id, control, cx);
                return;
            }
            Ok(FileReply::Listing(listing)) => {
                for control in file_browse::list_replies(request_id, listing) {
                    session.enqueue(Outbound::control(control));
                }
            }
            Ok(FileReply::File(bytes)) => {
                let size = bytes.len() as u64;
                session.enqueue_stream(bytes, |stream_id| Control::FileReadReply {
                    request_id,
                    size,
                    stream_id,
                });
            }
            Ok(FileReply::Diff { text, truncated }) => {
                session.enqueue_stream(text, |stream_id| Control::DiffReply {
                    request_id,
                    stream_id,
                    truncated,
                });
            }
        }
        self.flush(session_id, cx);
    }

    /// Sends the answers queued for `session_id`, as far as the relay
    /// connection has room for.
    fn flush_outbound(&mut self, session_id: u32, cx: &mut Context<RemoteHost>) {
        loop {
            let Some(session) = self.sessions.get_mut(&session_id) else {
                return;
            };
            if session.outbound.is_empty() {
                return;
            }
            let client = self.client.read(cx);
            // Checked before sealing: a sealed frame that is not sent leaves a
            // gap the peer can never read past.
            if !client.has_capacity_for(1) {
                self.schedule_flush_retry(cx);
                return;
            }
            let Some(item) = session.outbound.pop_front() else {
                return;
            };
            session.outbound_bytes = session.outbound_bytes.saturating_sub(item.weight());
            let sealed = match item {
                Outbound::Control(control, _) => session.channel.seal_control(&control),
                Outbound::Data { stream_id, bytes } => session.channel.seal_data(stream_id, bytes),
                Outbound::EndOfStream { stream_id } => {
                    session.channel.seal_end_of_stream(stream_id)
                }
            };
            let sent = sealed
                .map_err(|error| error.to_string())
                .and_then(|sealed| {
                    client
                        .send_frame(session_id, &sealed)
                        .map_err(|error: SendError| error.to_string())
                });
            if let Err(reason) = sent {
                log::warn!("ending session {session_id}: {reason}");
                self.end_session(session_id, cx);
                return;
            }
        }
    }

    fn send_end_of_stream(&mut self, session_id: u32, stream_id: u32, cx: &gpui::App) {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        let client = self.client.read(cx);
        // Checked before sealing, as everywhere: a sealed frame that is not
        // sent leaves a gap the peer can never read past. A stream that cannot
        // be told it ended is not worth more than a log line; the device learns
        // from the session or the relay ending.
        if !client.has_capacity_for(1) {
            log::debug!("could not end stream {stream_id}: the relay connection is full");
            return;
        }
        match session.channel.seal_end_of_stream(stream_id) {
            Ok(sealed) => {
                if let Err(error) = client.send_frame(session_id, &sealed) {
                    log::debug!("could not end stream {stream_id}: {error}");
                }
            }
            Err(error) => log::debug!("could not end stream {stream_id}: {error}"),
        }
    }

    /// Seals and sends a control message. A session that cannot be written to
    /// is ended: it is either gone or too slow to be worth holding.
    pub(crate) fn send_control(
        &mut self,
        session_id: u32,
        control: &Control,
        cx: &mut Context<RemoteHost>,
    ) -> bool {
        let sent = self.try_send_control(session_id, control, cx);
        if let Err(reason) = sent {
            log::warn!("ending session {session_id}: {reason}");
            self.end_session(session_id, cx);
            return false;
        }
        true
    }

    fn try_send_control(
        &mut self,
        session_id: u32,
        control: &Control,
        cx: &gpui::App,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| "it is already gone".to_string())?;
        let client = self.client.read(cx);
        // Checked before sealing: a sealed frame that is then not sent leaves a
        // gap in the cipher's counter, and the peer could never read on.
        if !client.has_capacity_for(1) {
            return Err("the connection to the relay is full".to_string());
        }
        let sealed = session
            .channel
            .seal_control(control)
            .map_err(|error| error.to_string())?;
        client
            .send_frame(session_id, &sealed)
            .map_err(|error: SendError| error.to_string())
    }

    /// Sends a control message to every device that has said hello.
    pub(crate) fn broadcast(&mut self, control: &Control, cx: &mut Context<RemoteHost>) {
        let greeted: Vec<u32> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.router.hello_done())
            .map(|(id, _)| *id)
            .collect();
        for session_id in greeted {
            self.send_control(session_id, control, cx);
        }
    }

    pub(crate) fn send_to(
        &mut self,
        sessions: &[u32],
        control: &Control,
        cx: &mut Context<RemoteHost>,
    ) {
        for session_id in sessions {
            if self.sessions.contains_key(session_id) {
                self.send_control(*session_id, control, cx);
            }
        }
    }

    pub(crate) fn forget_streams_of(&mut self, terminal: &str, sessions: &[u32]) {
        for session_id in sessions {
            if let Some(session) = self.sessions.get_mut(session_id) {
                session.streams.retain(|_, attached| attached != terminal);
            }
        }
    }

    /// Sends what is queued for `session_id`, as far as the relay connection
    /// has room for.
    pub(crate) fn flush(&mut self, session_id: u32, cx: &mut Context<RemoteHost>) {
        self.flush_outbound(session_id, cx);
        loop {
            let Some(session) = self.sessions.get(&session_id) else {
                return;
            };
            if !session.router.hello_done() {
                return;
            }
            if !self.client.read(cx).has_capacity_for(1) {
                self.schedule_flush_retry(cx);
                return;
            }
            let Some((stream_id, bytes)) = self
                .terminals
                .update(cx, |terminals, cx| terminals.pop_output(session_id, cx))
            else {
                break;
            };
            let sent = self.try_send_data(session_id, stream_id, bytes, cx);
            if let Err(reason) = sent {
                log::warn!("ending session {session_id}: {reason}");
                self.end_session(session_id, cx);
                return;
            }
        }
        self.flush_ide(session_id, cx);
    }

    /// Sends what the project server has written, as far as the relay
    /// connection has room for.
    fn flush_ide(&mut self, session_id: u32, cx: &mut Context<RemoteHost>) {
        loop {
            let Some(ide) = self
                .sessions
                .get(&session_id)
                .and_then(|session| session.ide.as_ref())
            else {
                return;
            };
            if ide.output.is_empty() {
                return;
            }
            if !self.client.read(cx).has_capacity_for(1) {
                self.schedule_flush_retry(cx);
                return;
            }
            let stream_id = ide.stream_id;
            let Ok(output) = ide.output.try_recv() else {
                return;
            };
            match output {
                IdeOutput::Data(bytes) => {
                    if let Err(reason) = self.try_send_data(session_id, stream_id, bytes, cx) {
                        log::warn!("ending session {session_id}: {reason}");
                        self.end_session(session_id, cx);
                        return;
                    }
                }
                IdeOutput::Ended => {
                    // The server is gone; the session is not, and still mirrors.
                    if let Some(session) = self.sessions.get_mut(&session_id) {
                        session.ide = None;
                    }
                    self.send_end_of_stream(session_id, stream_id, cx);
                    return;
                }
            }
        }
    }

    fn try_send_data(
        &mut self,
        session_id: u32,
        stream_id: u32,
        bytes: Vec<u8>,
        cx: &gpui::App,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| "it is already gone".to_string())?;
        let sealed = session
            .channel
            .seal_data(stream_id, bytes)
            .map_err(|error| error.to_string())?;
        self.client
            .read(cx)
            .send_frame(session_id, &sealed)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn flush_all(&mut self, cx: &mut Context<RemoteHost>) {
        self.flush_retry = None;
        let sessions: Vec<u32> = self.sessions.keys().copied().collect();
        for session_id in sessions {
            self.flush(session_id, cx);
        }
    }

    fn schedule_flush_retry(&mut self, cx: &mut Context<RemoteHost>) {
        if self.flush_retry.is_some() {
            return;
        }
        self.flush_retry = Some(cx.spawn(async move |this: WeakEntity<RemoteHost>, cx| {
            cx.background_executor().timer(FLUSH_RETRY).await;
            this.update(cx, |this, cx| this.flush_all(cx)).ok();
        }));
    }

    /// Ends what has run out of time: handshakes that never finished, pairings
    /// nobody answered, and devices that have been silent too long.
    pub(crate) fn housekeeping(&mut self, cx: &mut Context<RemoteHost>) {
        let now = Self::now(cx);

        for session_id in self.handshakes.expire(now) {
            log::debug!("session {session_id} never started its handshake");
            self.end_session(session_id, cx);
        }

        let abandoned: Vec<u32> = self
            .opened
            .iter()
            .filter(|(_, opened)| {
                opened.kind == OpenedKind::Pair
                    && now.saturating_duration_since(opened.opened_at) >= PAIRING_EXPIRY
            })
            .map(|(id, _)| *id)
            .collect();
        for session_id in abandoned {
            self.end_session(session_id, cx);
        }

        if let Some(session_id) = self.pairing.expire(now) {
            self.end_session(session_id, cx);
        }

        let unproven: Vec<u32> = self
            .sessions
            .iter()
            .filter(|(_, session)| {
                !session.confirmed
                    && now.saturating_duration_since(session.established_at)
                        >= CONFIRMATION_DEADLINE
            })
            .map(|(id, _)| *id)
            .collect();
        for session_id in unproven {
            log::debug!("session {session_id} never proved it holds the keys");
            self.end_session(session_id, cx);
        }

        if let Some(limit) = RemoteControlSettings::idle_timeout(cx) {
            let quiet: Vec<u32> = self
                .sessions
                .iter()
                .filter(|(_, session)| {
                    session.confirmed
                        && now.saturating_duration_since(session.last_activity) >= limit
                })
                .map(|(id, _)| *id)
                .collect();
            for session_id in quiet {
                log::info!("disconnecting a device that has been quiet for {limit:?}");
                self.end_session(session_id, cx);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn file_requests_in_flight(&self, session_id: u32) -> usize {
        self.sessions
            .get(&session_id)
            .map_or(0, |session| session.file_tasks.len())
    }

    /// Queues `total` bytes of answers nobody asked for, as if a slow device
    /// had left them waiting.
    #[cfg(test)]
    pub(crate) fn queue_waiting_bytes(&mut self, session_id: u32, total: usize) {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        let mut remaining = total;
        while remaining > 0 {
            let size = remaining.min(MAX_INNER_PAYLOAD_LEN);
            session.enqueue(Outbound::Data {
                stream_id: HOST_STREAM_BASE,
                bytes: vec![0; size],
            });
            remaining -= size;
        }
    }

    #[cfg(test)]
    pub(crate) fn set_next_host_stream(&mut self, session_id: u32, stream_id: u32) {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.next_host_stream = stream_id;
        }
    }

    pub(crate) fn confirmed_sessions(&self) -> impl Iterator<Item = (u32, &ActiveSession)> {
        self.sessions
            .iter()
            .filter(|(_, session)| session.confirmed)
            .map(|(id, session)| (*id, session))
    }
}
