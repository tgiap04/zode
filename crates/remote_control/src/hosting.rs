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
    SendError, TrustStore, list_devices, sanitize_device_name,
    secure_session::{ResponderHandshakes, SecureChannel},
};
use remote_relay_protocol::{
    Control, DeviceKeypair, InnerFrame, InnerKind, MAX_INNER_PAYLOAD_LEN, PAIRING_EXPIRY,
    PairingMessage, SessionCursor, build_prologue, decode_control, encode_control, error_code,
    random_nonce,
};

use crate::{
    agent_mirror::AgentMirror,
    file_browse::{self, BrowseError, FileBrowser, FileReply, FileRequest},
    ide_bridge::{self, IdeBridge, IdeLaunch, IdeOutput},
    ide_heartbeat::RequestDetector,
    pairing_flow::{MAX_PAIRING_REQUESTS_PER_HOUR, PairingFlow, PairingHistory, PairingRefusal},
    remote_control_settings::RemoteControlSettings,
    remote_host::{RemoteHost, RemoteHostEvent},
    session_history::{self, SessionRequest},
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

/// How long after a session was opened for a device it must wait before it may
/// have another opened. The tab a resume opens is added a moment after the
/// answer, and an agent counts as running only once its process is, so a device
/// asking again straight away would be answered by a second process on the same
/// transcript, and a device asking over and over by as many processes as it
/// likes. `ActiveSession::resume` keeps it to one at a time while one is being
/// opened; this is what keeps the next from following at once.
pub(crate) const RESUME_COOLDOWN: Duration = Duration::from_secs(3);

/// Bytes of answers one device may have waiting for the relay. A request is
/// let in only if the answers already waiting, plus the largest answer each
/// request being worked on may yet produce, fit under this; a device that asks
/// faster than it reads is told to slow down instead of being buffered.
const MAX_QUEUED_REPLY_BYTES: usize = 8 * 1024 * 1024;

/// The most one answer can weigh: a diff is the largest thing sent, and a
/// listing or a file is smaller.
const MAX_REPLY_BYTES: usize = file_browse::MAX_DIFF_BYTES;

/// What a control message that carries no list counts for against a device's
/// allowance, and what one that could not be measured counts for.
const CONTROL_WEIGHT_FALLBACK: usize = 1024;

/// What a device that has used up its queue may still have refused, in small
/// messages, before it is judged to be ignoring what it is told and is ended.
const REFUSAL_ALLOWANCE_BYTES: usize = 64 * 1024;

const FALLBACK_DEVICE_NAME: &str = "A browser";

/// Messages the host pushed unasked -- an agent's status, the list of terminals
/// -- that may wait for a device that is not reading. A newer push makes an older
/// one about the same thing stale, so what waits is bounded by how many things
/// there are to say something about; this bounds the rest.
const MAX_QUEUED_PUSHES: usize = 256;

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
    /// Tells the project stream's pings from what the device asks for.
    ide_requests: RequestDetector,
    /// What waits for the relay to have room for it.
    outbound: OutboundQueue,
    /// Work answering file requests and session listings, by ticket. Ending
    /// the session drops it, which cancels the work and its deadline.
    file_tasks: HashMap<u64, Task<()>>,
    /// The session being opened for this device, at most one. Dropped with the
    /// session, so a tab is never opened for a device that has gone.
    resume: Option<Task<()>>,
    next_host_stream: u32,
}

/// What a finished request had been holding, so that it can be given back.
pub(crate) enum RequestSlot {
    /// One of the requests in `file_tasks`.
    Ticket(u64),
    /// The session being opened.
    Resume,
}

/// A control message waiting for the relay, sealed only when it has room.
struct QueuedControl {
    body: ControlBody,
    /// What it counts for against the device's allowance. Nothing for a push.
    weight: usize,
    /// What it says something about, if the host pushed it unasked.
    push: Option<PushKey>,
}

enum ControlBody {
    Plain(Control),
    /// A message that carries a list, encoded once to be weighed and kept encoded
    /// so that sealing does not serialise it again.
    Encoded(Vec<u8>),
}

impl QueuedControl {
    /// Only the messages that carry a list vary in size, so only those are
    /// encoded up front to be weighed; the rest are small and counted flat.
    fn new(control: Control, push: Option<PushKey>) -> Self {
        let (body, measured) = match control {
            Control::AgentList { .. }
            | Control::TerminalList { .. }
            | Control::FilesListReply { .. }
            | Control::SessionsListReply { .. } => match encode_control(&control) {
                Ok(encoded) => {
                    let weight = encoded.len();
                    (ControlBody::Encoded(encoded), weight)
                }
                // Kept as is, so sealing reports why it cannot be sent.
                Err(_) => (ControlBody::Plain(control), CONTROL_WEIGHT_FALLBACK),
            },
            control => (ControlBody::Plain(control), CONTROL_WEIGHT_FALLBACK),
        };
        Self {
            body,
            weight: if push.is_some() { 0 } else { measured },
            push,
        }
    }
}

/// A frame of a file or a diff, or the end of a stream.
enum Bulk {
    Data { stream_id: u32, bytes: Vec<u8> },
    EndOfStream { stream_id: u32 },
}

impl Bulk {
    fn weight(&self) -> usize {
        match self {
            Bulk::Data { bytes, .. } => bytes.len(),
            Bulk::EndOfStream { .. } => 0,
        }
    }
}

enum Outbound {
    Control(QueuedControl),
    Bulk(Bulk),
}

/// What a push is about. A newer push makes an older one about the same thing,
/// or about less, stale.
#[derive(Clone, PartialEq, Eq)]
enum PushKey {
    TerminalList,
    AgentList,
    Agent(String),
    TerminalResized(String),
    TerminalClosed(String),
    Other,
}

fn push_key(control: &Control) -> PushKey {
    match control {
        Control::TerminalList { .. } => PushKey::TerminalList,
        Control::AgentList { .. } => PushKey::AgentList,
        Control::AgentUpdate { agent } => PushKey::Agent(agent.id.clone()),
        Control::TerminalResized { terminal_id, .. } => {
            PushKey::TerminalResized(terminal_id.clone())
        }
        Control::TerminalClosed { terminal_id, .. } => PushKey::TerminalClosed(terminal_id.clone()),
        _ => PushKey::Other,
    }
}

/// Whether `newer`, sent after `older`, leaves nothing worth saying in `older`.
fn supersedes(newer: &PushKey, older: &PushKey) -> bool {
    match (newer, older) {
        (PushKey::Other, _) | (_, PushKey::Other) => false,
        // A whole list says everything an update to one of its members did.
        (PushKey::AgentList, PushKey::Agent(_)) => true,
        // A terminal that closed has no size to report.
        (PushKey::TerminalClosed(closed), PushKey::TerminalResized(resized)) => closed == resized,
        (PushKey::TerminalClosed(_), PushKey::TerminalClosed(_)) => false,
        (newer, older) => newer == older,
    }
}

/// What waits for the relay to have room for it, for one device.
#[derive(Default)]
struct OutboundQueue {
    /// Small messages, in the order the device must see them. They go ahead of
    /// `bulk`: a reply should not wait behind a file.
    controls: VecDeque<QueuedControl>,
    /// How many of `controls` were pushed unasked.
    queued_pushes: usize,
    /// The frames of files and diffs, and the ends of streams, in order.
    bulk: VecDeque<Bulk>,
    /// What the device's answers waiting here count for against its allowance:
    /// only what it asked for, so that what the host pushes cannot be the reason
    /// a device is ended.
    bytes: usize,
}

impl OutboundQueue {
    fn has_waiting(&self) -> bool {
        !self.controls.is_empty() || !self.bulk.is_empty()
    }

    /// Queues an answer to something the device asked for, which counts against
    /// its allowance.
    fn enqueue_reply(&mut self, control: Control) {
        let queued = QueuedControl::new(control, None);
        self.bytes += queued.weight;
        self.controls.push_back(queued);
    }

    /// Queues something the host says unasked. It replaces what it makes stale,
    /// so a device that is slow to read is told the latest and not everything,
    /// and it never counts against the device.
    fn enqueue_push(&mut self, control: Control) {
        let key = push_key(&control);
        if self.queued_pushes > 0 {
            let before = self.controls.len();
            self.controls.retain(|queued| {
                !queued
                    .push
                    .as_ref()
                    .is_some_and(|older| supersedes(&key, older))
            });
            self.queued_pushes -= before - self.controls.len();
        }
        if self.queued_pushes >= MAX_QUEUED_PUSHES
            && let Some(oldest) = self
                .controls
                .iter()
                .position(|queued| queued.push.is_some())
        {
            self.controls.remove(oldest);
            self.queued_pushes -= 1;
        }
        self.controls
            .push_back(QueuedControl::new(control, Some(key)));
        self.queued_pushes += 1;
    }

    fn enqueue_bulk(&mut self, item: Bulk) {
        self.bytes += item.weight();
        self.bulk.push_back(item);
    }

    /// The next frame to send: a control message if one is waiting, since none
    /// is large and a reply should not wait for a file, and otherwise the next
    /// frame of a file. Control messages keep their order among themselves, and
    /// a file's frames never overtake the message that names their stream: it
    /// was queued first and every control message goes before any frame.
    fn next(&mut self) -> Option<Outbound> {
        if let Some(queued) = self.controls.pop_front() {
            self.bytes = self.bytes.saturating_sub(queued.weight);
            if queued.push.is_some() {
                self.queued_pushes = self.queued_pushes.saturating_sub(1);
            }
            return Some(Outbound::Control(queued));
        }
        let item = self.bulk.pop_front()?;
        self.bytes = self.bytes.saturating_sub(item.weight());
        Some(Outbound::Bulk(item))
    }
}

impl ActiveSession {
    /// Whether one more request that may be answered with [`MAX_REPLY_BYTES`]
    /// fits this device's allowance: how many it may have being worked on, and
    /// how much it may have waiting for the relay on top of what the ones
    /// already running may yet add.
    fn has_room_for_a_request(&self) -> bool {
        let running = self.file_tasks.len();
        let reserved = (running + 1) * MAX_REPLY_BYTES;
        running < MAX_FILE_REQUESTS_PER_SESSION
            && self.outbound.bytes + reserved <= MAX_QUEUED_REPLY_BYTES
    }

    /// Queues `reply`, which names a new stream, then `bytes` on that stream
    /// cut into frames, then the end of the stream.
    fn enqueue_stream(&mut self, bytes: Vec<u8>, reply: impl FnOnce(u32) -> Control) {
        let stream_id = self.next_host_stream;
        self.next_host_stream = match stream_id.checked_add(1) {
            Some(next) => next,
            None => HOST_STREAM_BASE,
        };
        self.outbound.enqueue_reply(reply(stream_id));
        for chunk in bytes.chunks(MAX_INNER_PAYLOAD_LEN) {
            self.outbound.enqueue_bulk(Bulk::Data {
                stream_id,
                bytes: chunk.to_vec(),
            });
        }
        self.outbound.enqueue_bulk(Bulk::EndOfStream { stream_id });
    }
}

/// Why a pairing request got no question put to the person.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RefusalNotice {
    CannotPin,
    RateLimited,
    TooManyAnswered,
    LockedOut,
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
    /// Whether this host answers for its agent session history.
    pub(crate) sessions_available: bool,
    /// When each device may next have a session opened for it, by device id.
    /// Bounded by the number of paired devices, because a session only opens for
    /// a paired one. An entry that has run out is not removed on its own: a
    /// lookup compares it with the clock, and the next insert drops what has
    /// passed.
    resume_cooldowns: HashMap<String, Instant>,
    /// Numbers the file requests of every session, so that an answer can tell
    /// whether the request it answers is still waiting.
    next_file_ticket: u64,
    pub(crate) pairing: PairingFlow,
    /// The project server this host can start, once it has been looked for.
    ide_launch: Option<Option<IdeLaunch>>,
    /// The last refusal the person was told about, so a device that keeps
    /// asking is said once and not once per request.
    reported_refusal: Option<RefusalNotice>,
    handshakes: ResponderHandshakes,
    opened: HashMap<u32, Opened>,
    pub(crate) sessions: HashMap<u32, ActiveSession>,
    app_version: String,
    /// The one question on screen has been put in front of somebody.
    pub(crate) pairing_presented: Option<u32>,
    /// The OS has been asked to call the person back to Zode about this
    /// question, because no window was in front to put it on.
    pub(crate) pairing_notified: Option<u32>,
    /// Something the indicator or a modal shows has changed since the host last
    /// looked, which is what tells it a frame or a burst of output is worth a
    /// redraw and most are not.
    changed: bool,
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
    pub(crate) sessions_available: bool,
    pub(crate) pairing_history: PairingHistory,
    pub(crate) app_version: String,
    pub(crate) housekeeping: Task<()>,
    pub(crate) subscriptions: Vec<Subscription>,
}

/// "in about 12 minutes", rounded up so the wait is never understated.
fn wait_phrase(wait: Duration) -> String {
    let minutes = wait.as_secs().div_ceil(60).max(1);
    if minutes == 1 {
        "in about a minute".to_string()
    } else {
        format!("in about {minutes} minutes")
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// A device name fit to show, and never empty: it comes from the account
/// service, which any device on the account can write to, and ends up in the
/// question, the notification, the indicator and the saved pin.
fn displayable_device_name(raw: &str) -> String {
    let name = sanitize_device_name(raw);
    if name.is_empty() {
        FALLBACK_DEVICE_NAME.to_string()
    } else {
        name
    }
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
            sessions_available: parts.sessions_available,
            resume_cooldowns: HashMap::default(),
            next_file_ticket: 0,
            pairing: PairingFlow::resume(parts.pairing_history),
            ide_launch: None,
            reported_refusal: None,
            handshakes: ResponderHandshakes::default(),
            opened: HashMap::default(),
            sessions: HashMap::default(),
            app_version: parts.app_version,
            pairing_presented: None,
            pairing_notified: None,
            changed: false,
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

    /// Where the project server is. Looking is a stat of the filesystem, so it is
    /// done the first time a device asks and not again while this host runs.
    fn project_server(&mut self, cx: &gpui::App) -> Option<IdeLaunch> {
        self.ide_launch
            .get_or_insert_with(|| ide_bridge::locate(cx))
            .clone()
    }

    fn mark_changed(&mut self, cx: &mut Context<RemoteHost>) {
        self.changed = true;
        cx.emit(RemoteHostEvent::Changed);
    }

    /// Whether anything shown has changed since this was last asked.
    pub(crate) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
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
        if self.pairing_notified == Some(session_id) {
            self.pairing_notified = None;
        }
        self.mark_changed(cx);
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
                // Before the person is shown digits for a device that cannot be
                // saved when they press Trust.
                if let Err(error) = self.trust.check_can_pin(&peer_device_id) {
                    log::info!("refused a pairing request: {error}");
                    self.report_refusal(
                        RefusalNotice::CannotPin,
                        format!("A device asked to pair, but Zode did not ask you: {error}."),
                        cx,
                    );
                    self.end_session(session_id, cx);
                    return;
                }
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
                        match refusal {
                            PairingRefusal::RateLimited { retry_after } => self.report_refusal(
                                RefusalNotice::RateLimited,
                                format!(
                                    "A device asked to pair, but Zode has already asked you \
                                     {MAX_PAIRING_REQUESTS_PER_HOUR} times this hour, so it did \
                                     not ask again. Try again {}.",
                                    wait_phrase(retry_after)
                                ),
                                cx,
                            ),
                            PairingRefusal::TooManyAnswered { retry_after } => self.report_refusal(
                                RefusalNotice::TooManyAnswered,
                                format!(
                                    "Devices have started pairing too many times this hour, so \
                                     Zode stopped answering them. If you did not do this, someone \
                                     may be trying to guess a pairing code; try again {}.",
                                    wait_phrase(retry_after)
                                ),
                                cx,
                            ),
                            PairingRefusal::LockedOut => self.report_lockout(cx),
                            _ => {}
                        }
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
                        if self.pairing.is_locked_out() {
                            self.report_lockout(cx);
                        }
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
        // The name is whatever the account service holds, and it ends up in the
        // question, the notification, the indicator and the saved pin.
        let peer_name = displayable_device_name(&peer_name);
        let trust = &self.trust;
        let asked = self.pairing.present(
            session_id,
            peer_name,
            |device_id, key| {
                trust
                    .get(device_id)
                    .filter(|pinned| pinned.public_key != *key)
                    .map(|pinned| pinned.name.clone())
            },
            now,
        );
        if asked {
            self.reported_refusal = None;
            self.mark_changed(cx);
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
            if trust && expired == session_id {
                cx.emit(RemoteHostEvent::Problem {
                    message: "The request to pair ran out of time before it was answered. \
                              Ask the other device to try again."
                        .to_string(),
                    retry: false,
                });
            }
        }
        let Some(decision) = self.pairing.decide(session_id, trust, now) else {
            return;
        };
        self.pairing_presented = None;
        let mut replaced_key_of = None;
        if trust {
            let replaces_key = self
                .trust
                .get(&decision.peer_device_id)
                .is_some_and(|existing| existing.public_key != decision.peer_public_key);
            let device = PinnedDevice {
                device_id: decision.peer_device_id.clone(),
                public_key: decision.peer_public_key,
                name: decision.peer_name.clone(),
                paired_at: unix_now(),
            };
            match self.trust.pin(device, cx) {
                Ok(()) => {
                    replaced_key_of = replaces_key.then(|| decision.peer_device_id.clone());
                }
                Err(error) => {
                    log::error!("could not trust {}: {error}", decision.peer_name);
                    cx.emit(RemoteHostEvent::Problem {
                        message: format!("Could not trust {}: {error}", decision.peer_name),
                        retry: false,
                    });
                }
            }
        } else if self.pairing.is_locked_out() {
            self.report_lockout(cx);
        }
        self.end_session(decision.session_id, cx);
        // Sessions made under the key that was replaced hold it still: their
        // channels were built from it, and nothing checks the pin again.
        if let Some(device_id) = replaced_key_of {
            self.end_sessions_of(&device_id, cx);
        }
    }

    /// The question was closed without an answer. The device is turned away,
    /// but nothing is held against it: digits that were not compared did not
    /// fail to match.
    pub(crate) fn dismiss_pairing(&mut self, session_id: u32, cx: &mut Context<RemoteHost>) {
        let now = Self::now(cx);
        if let Some(expired) = self.pairing.expire(now) {
            self.end_session(expired, cx);
        }
        let Some(decision) = self.pairing.dismiss(session_id, now) else {
            return;
        };
        self.pairing_presented = None;
        self.end_session(decision.session_id, cx);
    }

    /// The only way out of a pairing lockout.
    pub(crate) fn unlock_pairing(&mut self, cx: &mut Context<RemoteHost>) {
        if let Some(session_id) = self.pairing.unlock() {
            self.end_session(session_id, cx);
        }
        self.reported_refusal = None;
    }

    fn report_lockout(&mut self, cx: &mut Context<RemoteHost>) {
        self.report_refusal(
            RefusalNotice::LockedOut,
            "Pairing is locked after repeated mismatches, so devices cannot be added. \
             Unlock it in Manage Trusted Devices."
                .to_string(),
            cx,
        );
    }

    /// Tells the person that a device asked to pair and was turned away, once
    /// for as long as the reason stands.
    fn report_refusal(
        &mut self,
        notice: RefusalNotice,
        message: String,
        cx: &mut Context<RemoteHost>,
    ) {
        if self.reported_refusal == Some(notice) {
            return;
        }
        self.reported_refusal = Some(notice);
        cx.emit(RemoteHostEvent::Problem {
            message,
            retry: false,
        });
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
        // A device that finds every place taken is answered all the same: only
        // once it has proven it holds the keys can it be told, in words, that
        // there is no place for it. How many may be waiting to prove it is
        // bounded below.
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
            .map(|pinned| displayable_device_name(&pinned.name))
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
                ide_requests: RequestDetector::new(),
                outbound: OutboundQueue::default(),
                file_tasks: HashMap::default(),
                resume: None,
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
                self.queue_control(
                    session_id,
                    error_control(
                        error_code::TOO_LARGE,
                        &format!(
                            "this Zode already has {MAX_ACTIVE_SESSIONS} devices connected; \
                             disconnect one to connect another"
                        ),
                    ),
                    cx,
                );
                self.end_session(session_id, cx);
                return;
            }
            session.confirmed = true;
            session.last_activity = now;
            let device_name = session.peer_name.clone();
            cx.emit(RemoteHostEvent::SessionStarted { device_name });
            self.mark_changed(cx);
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
                self.queue_control(
                    session_id,
                    error_control(
                        error_code::MALFORMED,
                        "that is not a message this host reads",
                    ),
                    cx,
                );
                return;
            }
        };
        // Only these two messages read whether there is a project server.
        let ide_available = matches!(control, Control::Hello { .. } | Control::IdeOpen { .. })
            && self.project_server(cx).is_some();
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        let was_greeted = session.router.hello_done();
        let facts = HostFacts {
            app_version: &self.app_version,
            ide_available,
            files_available: self.files.is_some(),
            sessions_available: self.sessions_available,
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
        if self.queue_control(session_id, Control::AgentList { agents }, cx) {
            self.queue_control(session_id, Control::TerminalList { terminals }, cx);
        }
    }

    fn apply(&mut self, session_id: u32, action: RouterAction, cx: &mut Context<RemoteHost>) {
        match action {
            RouterAction::Reply(control) => {
                self.queue_control(session_id, control, cx);
            }
            RouterAction::Refuse(control) => {
                // Sent now if the relay has room; a device being turned away is
                // not waited for.
                self.queue_control(session_id, control, cx);
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
            } => {
                self.note_activity(session_id, cx);
                self.serve_files(session_id, request_id, request, cx);
            }
            RouterAction::Sessions {
                request_id,
                request,
            } => {
                self.note_activity(session_id, cx);
                self.serve_sessions(session_id, request_id, request, cx);
            }
            RouterAction::ReportSize { terminal_id } => {
                let size = self.terminals.read(cx).dimensions_of(&terminal_id, cx);
                let control = match size {
                    Some((columns, rows)) => Control::TerminalResized {
                        terminal_id,
                        columns,
                        rows,
                    },
                    None => error_control(error_code::NOT_FOUND, "there is no such terminal"),
                };
                self.queue_control(session_id, control, cx);
            }
        }
    }

    /// Somebody is at the other end: it asked for something. A ping is not
    /// asking for anything.
    fn note_activity(&mut self, session_id: u32, cx: &gpui::App) {
        let now = Self::now(cx);
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.last_activity = now;
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
            self.queue_control(
                session_id,
                error_control(error_code::MALFORMED, "that stream is already in use"),
                cx,
            );
            return;
        }
        let replaces_own = session
            .streams
            .get(&stream_id)
            .is_some_and(|attached| *attached == terminal);
        if session.streams.contains_key(&stream_id) && !replaces_own {
            self.queue_control(
                session_id,
                error_control(error_code::MALFORMED, "that stream is already in use"),
                cx,
            );
            return;
        }
        let already_attached = session
            .streams
            .values()
            .any(|attached| *attached == terminal);
        if !already_attached && session.streams.len() >= MAX_ATTACHMENTS_PER_SESSION {
            self.queue_control(
                session_id,
                error_control(error_code::TOO_LARGE, "too many terminals are attached"),
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
                self.queue_control(session_id, attached, cx);
            }
            Err(AttachError::NotFound) => {
                self.queue_control(
                    session_id,
                    error_control(error_code::NOT_FOUND, "there is no such terminal"),
                    cx,
                );
            }
            Err(AttachError::Inactive) => {
                self.queue_control(
                    session_id,
                    error_control(error_code::INTERNAL, "terminals are not being shared"),
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
            self.queue_control(session_id, refusal, cx);
            return;
        }
        if session.streams.contains_key(&stream_id) {
            let refusal = refusal(error_code::MALFORMED, "that stream is already in use");
            self.queue_control(session_id, refusal, cx);
            return;
        }
        let Some(launch) = self.project_server(cx) else {
            let refusal = refusal(
                crate::session_router::UNSUPPORTED,
                "this Zode was installed without its project server",
            );
            self.queue_control(session_id, refusal, cx);
            return;
        };
        let bridge = match IdeBridge::start(&launch, stream_id, cx) {
            Ok(bridge) => bridge,
            Err(error) if ide_bridge::is_too_many_stopping(&error) => {
                log::info!("not starting a project server: {error}");
                let refusal = refusal(
                    error_code::RATE_LIMITED,
                    "earlier project servers are still being stopped; try again shortly",
                );
                self.queue_control(session_id, refusal, cx);
                return;
            }
            Err(error) => {
                log::warn!("could not start the project server: {error}");
                let refusal = refusal(error_code::INTERNAL, "the project server did not start");
                self.queue_control(session_id, refusal, cx);
                return;
            }
        };
        let now = Self::now(cx);
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.ide = Some(bridge);
            session.ide_requests = RequestDetector::new();
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
        self.queue_control(session_id, opened, cx);
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
            self.queue_end_of_stream(session_id, stream_id, cx);
            return;
        }
        let Some(ide) = session.ide.as_ref() else {
            return;
        };
        // Read before the bytes are handed on. The controlling Zode pings its
        // project server every few seconds, so traffic alone says nothing about
        // anyone being there.
        let asked_for_something = session.ide_requests.sees_a_request(&frame.payload);
        if ide.send_input(frame.payload).is_err() {
            log::warn!("ending session {session_id}: it sent more than the project server reads");
            self.end_session(session_id, cx);
            return;
        }
        if asked_for_something {
            session.last_activity = now;
        }
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
        if !session.has_room_for_a_request() {
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

    fn serve_sessions(
        &mut self,
        session_id: u32,
        request_id: u32,
        request: SessionRequest,
        cx: &mut Context<RemoteHost>,
    ) {
        match request {
            SessionRequest::List { before, limit } => {
                self.serve_session_list(session_id, request_id, before, limit, cx);
            }
            SessionRequest::Resume {
                session_id: wanted,
                fork,
            } => {
                self.serve_session_resume(session_id, request_id, wanted, fork, cx);
            }
        }
    }

    fn rate_limited(request_id: u32) -> Control {
        Control::Error {
            code: error_code::RATE_LIMITED.to_string(),
            message: "too many requests are waiting; ask again when they are answered".to_string(),
            request_id: Some(request_id),
        }
    }

    fn resume_cooling_down(request_id: u32) -> Control {
        Control::Error {
            code: error_code::RATE_LIMITED.to_string(),
            message: "a session was just opened; ask again in a few seconds".to_string(),
            request_id: Some(request_id),
        }
    }

    /// A listing draws on the same allowance as a file request: it is work for
    /// the host and a reply that has to wait for the relay.
    fn serve_session_list(
        &mut self,
        session_id: u32,
        request_id: u32,
        before: Option<SessionCursor>,
        limit: usize,
        cx: &mut Context<RemoteHost>,
    ) {
        let Some(session) = self.sessions.get(&session_id) else {
            return;
        };
        if !session.has_room_for_a_request() {
            self.queue_control(session_id, Self::rate_limited(request_id), cx);
            return;
        }
        let ticket = self.next_file_ticket;
        self.next_file_ticket += 1;
        let agents = self.agents.clone();
        let task = cx.spawn(async move |this: WeakEntity<RemoteHost>, cx| {
            let deadline = cx
                .background_executor()
                .timer(file_browse::REQUEST_DEADLINE);
            let reply = file_browse::answer_within(
                deadline,
                session_history::list(request_id, before, limit, agents, cx),
            )
            .await;
            this.update(cx, |host, cx| {
                host.finish_session_request(
                    session_id,
                    RequestSlot::Ticket(ticket),
                    request_id,
                    reply,
                    cx,
                );
            })
            .ok();
        });
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.file_tasks.insert(ticket, task);
        }
    }

    /// One session at a time is opened for a device -- across all the
    /// connections it has, and with [`RESUME_COOLDOWN`] between one and the
    /// next -- because each is a process on this machine that the person at it
    /// did not start.
    fn serve_session_resume(
        &mut self,
        session_id: u32,
        request_id: u32,
        wanted: String,
        fork: bool,
        cx: &mut Context<RemoteHost>,
    ) {
        let Some(session) = self.sessions.get(&session_id) else {
            return;
        };
        let device_id = &session.peer_device_id;
        let device_name = session.peer_name.clone();
        let one_is_opening = self
            .sessions
            .values()
            .any(|other| other.peer_device_id == *device_id && other.resume.is_some());
        if one_is_opening || session.outbound.bytes >= MAX_QUEUED_REPLY_BYTES {
            self.queue_control(session_id, Self::rate_limited(request_id), cx);
            return;
        }
        let now = Self::now(cx);
        if self
            .resume_cooldowns
            .get(device_id)
            .is_some_and(|until| now < *until)
        {
            self.queue_control(session_id, Self::resume_cooling_down(request_id), cx);
            return;
        }
        let agents = self.agents.clone();
        let task = cx.spawn(async move |this: WeakEntity<RemoteHost>, cx| {
            let deadline = cx
                .background_executor()
                .timer(file_browse::REQUEST_DEADLINE);
            let reply = file_browse::answer_within(
                deadline,
                session_history::resume(request_id, wanted, fork, device_name, agents, cx),
            )
            .await;
            this.update(cx, |host, cx| {
                host.finish_session_request(session_id, RequestSlot::Resume, request_id, reply, cx);
            })
            .ok();
        });
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.resume = Some(task);
        }
    }

    /// Queues the answer to a session request, if the session that asked for
    /// it is still the one that did.
    pub(crate) fn finish_session_request(
        &mut self,
        session_id: u32,
        slot: RequestSlot,
        request_id: u32,
        reply: Result<Vec<Control>, BrowseError>,
        cx: &mut Context<RemoteHost>,
    ) {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        let was_waiting = match slot {
            RequestSlot::Ticket(ticket) => session.file_tasks.remove(&ticket).is_some(),
            RequestSlot::Resume => session.resume.take().is_some(),
        };
        if !was_waiting {
            return;
        }
        if matches!(slot, RequestSlot::Resume)
            && reply
                .as_ref()
                .is_ok_and(|controls| session_history::opened_a_tab(controls))
        {
            let now = Self::now(cx);
            self.resume_cooldowns.retain(|_, until| *until > now);
            self.resume_cooldowns
                .insert(session.peer_device_id.clone(), now + RESUME_COOLDOWN);
        }
        match reply {
            Err(error) => {
                let control = Control::Error {
                    code: error.code.to_string(),
                    message: error.message,
                    request_id: Some(request_id),
                };
                self.queue_control(session_id, control, cx);
            }
            Ok(controls) => {
                for control in controls {
                    session.outbound.enqueue_reply(control);
                }
                self.flush(session_id, cx);
            }
        }
    }

    /// Queues the answer to something the device asked for behind the other
    /// control messages waiting for it, so it arrives in order and a full relay
    /// connection delays it instead of ending the session. The relay connection
    /// is shared, so one device's output filling it is no reason to end
    /// anybody's session. A device whose own queue is this far over its
    /// allowance is ignoring what it is told, and is ended. `false` if the
    /// session is gone, or ended here.
    pub(crate) fn queue_control(
        &mut self,
        session_id: u32,
        control: Control,
        cx: &mut Context<RemoteHost>,
    ) -> bool {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return false;
        };
        if session.outbound.bytes >= MAX_QUEUED_REPLY_BYTES + REFUSAL_ALLOWANCE_BYTES {
            log::warn!("ending session {session_id}: it asks for more than it reads");
            self.end_session(session_id, cx);
            return false;
        }
        session.outbound.enqueue_reply(control);
        self.flush(session_id, cx);
        self.sessions.contains_key(&session_id)
    }

    /// Queues something the host says on its own account. Unlike an answer it
    /// is not the device's doing, so a device that is slow to read is never
    /// ended for it: what waits is replaced by newer news about the same thing,
    /// and bounded.
    pub(crate) fn push_control(
        &mut self,
        session_id: u32,
        control: Control,
        cx: &mut Context<RemoteHost>,
    ) {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        session.outbound.enqueue_push(control);
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
                    session.outbound.enqueue_reply(control);
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
            if !session.outbound.has_waiting() {
                return;
            }
            let client = self.client.read(cx);
            // Checked before sealing: a sealed frame that is not sent leaves a
            // gap the peer can never read past.
            if !client.has_capacity_for(1) {
                self.schedule_flush_retry(cx);
                return;
            }
            let Some(item) = session.outbound.next() else {
                return;
            };
            let sealed = match item {
                Outbound::Control(QueuedControl {
                    body: ControlBody::Plain(control),
                    ..
                }) => session.channel.seal_control(&control),
                Outbound::Control(QueuedControl {
                    body: ControlBody::Encoded(encoded),
                    ..
                }) => session.channel.seal_encoded_control(encoded),
                Outbound::Bulk(Bulk::Data { stream_id, bytes }) => {
                    session.channel.seal_data(stream_id, bytes)
                }
                Outbound::Bulk(Bulk::EndOfStream { stream_id }) => {
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

    /// Tells the device a stream has ended, behind what is already waiting for
    /// it, and when the relay has room.
    fn queue_end_of_stream(
        &mut self,
        session_id: u32,
        stream_id: u32,
        cx: &mut Context<RemoteHost>,
    ) {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };
        session
            .outbound
            .enqueue_bulk(Bulk::EndOfStream { stream_id });
        self.flush_outbound(session_id, cx);
    }

    /// Pushes a control message to every device that has said hello.
    pub(crate) fn broadcast(&mut self, control: &Control, cx: &mut Context<RemoteHost>) {
        let greeted: Vec<u32> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.router.hello_done())
            .map(|(id, _)| *id)
            .collect();
        for session_id in greeted {
            self.push_control(session_id, control.clone(), cx);
        }
    }

    /// Pushes a control message to each of `sessions`.
    pub(crate) fn send_to(
        &mut self,
        sessions: &[u32],
        control: &Control,
        cx: &mut Context<RemoteHost>,
    ) {
        for session_id in sessions {
            self.push_control(*session_id, control.clone(), cx);
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
            // What is still queued stopped for want of room, and output must not
            // pass it if a place has come free since: the device would see a
            // terminal's output before the message that tells it which stream
            // that is. The retry that `flush_outbound` set up sends it all in
            // order.
            if session.outbound.has_waiting() || !session.router.hello_done() {
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
                    self.queue_end_of_stream(session_id, stream_id, cx);
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

    /// Sets whether this host answers for its agent session history, for the
    /// connection that is up and for any that follows.
    pub(crate) fn set_sessions_available(&mut self, available: bool) {
        self.sessions_available = available;
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
            session.outbound.enqueue_bulk(Bulk::Data {
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

#[cfg(test)]
mod tests {
    use super::*;
    use remote_relay_protocol::{AgentStatus, AgentSummary, TerminalSummary};

    #[test]
    fn a_device_name_is_shown_as_it_was_written() {
        assert_eq!(displayable_device_name("Ada's laptop"), "Ada's laptop");
        assert_eq!(displayable_device_name("  Ada  "), "Ada");
    }

    #[test]
    fn a_device_name_with_nothing_left_is_called_a_browser() {
        for raw in ["", "   ", "\u{202e}\u{0}\n"] {
            assert_eq!(
                displayable_device_name(raw),
                FALLBACK_DEVICE_NAME,
                "{raw:?}"
            );
        }
    }

    #[test]
    fn a_device_name_is_cleaned_before_it_is_shown() {
        assert_eq!(displayable_device_name("Ada\n\u{202e}laptop"), "Adalaptop");
        assert!(displayable_device_name(&"x".repeat(5000)).chars().count() <= 64);
    }

    fn agent(id: &str, status: AgentStatus) -> AgentSummary {
        AgentSummary {
            id: id.to_string(),
            name: "Claude Code".to_string(),
            status,
            title: None,
            session_id: None,
            agent: None,
        }
    }

    fn update(id: &str, status: AgentStatus) -> Control {
        Control::AgentUpdate {
            agent: agent(id, status),
        }
    }

    fn resized(terminal_id: &str, columns: u16) -> Control {
        Control::TerminalResized {
            terminal_id: terminal_id.to_string(),
            columns,
            rows: 24,
        }
    }

    fn closed(terminal_id: &str) -> Control {
        Control::TerminalClosed {
            terminal_id: terminal_id.to_string(),
            exit_code: None,
        }
    }

    fn terminal_list(count: usize) -> Control {
        Control::TerminalList {
            terminals: (0..count)
                .map(|index| TerminalSummary {
                    id: format!("terminal-{index}"),
                    title: "a title that is not short ".repeat(4),
                    columns: 80,
                    rows: 24,
                })
                .collect(),
        }
    }

    /// Everything waiting, in the order it would be sent, as what it is.
    fn drain(queue: &mut OutboundQueue) -> Vec<String> {
        let mut sent = Vec::new();
        while let Some(item) = queue.next() {
            sent.push(match item {
                Outbound::Control(QueuedControl {
                    body: ControlBody::Plain(control),
                    ..
                }) => format!("{control:?}"),
                Outbound::Control(QueuedControl {
                    body: ControlBody::Encoded(bytes),
                    ..
                }) => match decode_control(&bytes) {
                    Ok(Control::TerminalList { terminals }) => {
                        format!("terminal list of {}", terminals.len())
                    }
                    Ok(Control::AgentList { agents }) => format!("agent list of {}", agents.len()),
                    other => format!("{other:?}"),
                },
                Outbound::Bulk(Bulk::Data { bytes, .. }) => format!("data {}", bytes.len()),
                Outbound::Bulk(Bulk::EndOfStream { stream_id }) => format!("end {stream_id}"),
            });
        }
        sent
    }

    #[test]
    fn a_control_without_a_list_weighs_a_flat_amount() {
        for control in [
            Control::Ping,
            Control::Pong,
            error_control(error_code::MALFORMED, "no"),
        ] {
            assert_eq!(
                QueuedControl::new(control, None).weight,
                CONTROL_WEIGHT_FALLBACK
            );
        }
    }

    #[test]
    fn a_control_with_a_list_weighs_what_it_encodes_to() {
        let control = terminal_list(200);
        let encoded = serde_json::to_vec(&control).expect("encodes");
        assert!(encoded.len() > CONTROL_WEIGHT_FALLBACK);
        let queued = QueuedControl::new(control, None);
        assert_eq!(queued.weight, encoded.len());
        // Encoded once, here, and sealed from these bytes.
        assert!(matches!(queued.body, ControlBody::Encoded(bytes) if bytes == encoded));
    }

    #[test]
    fn what_the_host_pushes_never_counts_against_the_device() {
        let mut queue = OutboundQueue::default();
        for index in 0..1000 {
            queue.enqueue_push(closed(&format!("terminal-{index}")));
            queue.enqueue_push(terminal_list(50));
        }
        assert_eq!(queue.bytes, 0, "the device asked for none of it");

        queue.enqueue_reply(Control::Pong);
        assert_eq!(queue.bytes, CONTROL_WEIGHT_FALLBACK, "an answer does");
        drain(&mut queue);
        assert_eq!(queue.bytes, 0);
        assert!(!queue.has_waiting());
    }

    #[test]
    fn a_reply_goes_ahead_of_bulk_data_and_replies_keep_their_order() {
        let mut queue = OutboundQueue::default();
        let data = |size: usize| Bulk::Data {
            stream_id: 7,
            bytes: vec![0; size],
        };
        queue.enqueue_reply(Control::DiffReply {
            request_id: 1,
            stream_id: 7,
            truncated: false,
        });
        queue.enqueue_bulk(data(10));
        queue.enqueue_bulk(data(20));
        queue.enqueue_bulk(Bulk::EndOfStream { stream_id: 7 });
        queue.enqueue_reply(Control::Pong);
        queue.enqueue_push(closed("terminal-1"));
        queue.enqueue_reply(Control::Ping);

        let sent = drain(&mut queue);
        assert_eq!(
            sent.iter().map(String::as_str).collect::<Vec<_>>(),
            [
                "DiffReply { request_id: 1, stream_id: 7, truncated: false }",
                "Pong",
                "TerminalClosed { terminal_id: \"terminal-1\", exit_code: None }",
                "Ping",
                "data 10",
                "data 20",
                "end 7"
            ],
            "controls first and in order, then the frames, which follow the reply that names them"
        );
    }

    #[test]
    fn a_newer_push_replaces_an_older_one_about_the_same_thing() {
        let mut queue = OutboundQueue::default();
        queue.enqueue_push(terminal_list(1));
        queue.enqueue_push(update("agent-1", AgentStatus::Working));
        queue.enqueue_push(update("agent-2", AgentStatus::Working));
        queue.enqueue_push(terminal_list(2));
        queue.enqueue_push(update("agent-1", AgentStatus::Finished));
        queue.enqueue_push(resized("terminal-1", 80));
        queue.enqueue_push(resized("terminal-1", 100));
        queue.enqueue_push(resized("terminal-2", 90));

        let sent = drain(&mut queue);
        assert_eq!(
            sent.iter().map(String::as_str).collect::<Vec<_>>(),
            [
                "AgentUpdate { agent: AgentSummary { id: \"agent-2\", name: \"Claude Code\", status: Working, title: None, session_id: None, agent: None } }",
                "terminal list of 2",
                "AgentUpdate { agent: AgentSummary { id: \"agent-1\", name: \"Claude Code\", status: Finished, title: None, session_id: None, agent: None } }",
                "TerminalResized { terminal_id: \"terminal-1\", columns: 100, rows: 24 }",
                "TerminalResized { terminal_id: \"terminal-2\", columns: 90, rows: 24 }",
            ],
            "only the latest about each thing is told, after what it replaced"
        );
    }

    #[test]
    fn a_whole_list_replaces_the_updates_it_includes_and_a_close_replaces_a_resize() {
        let mut queue = OutboundQueue::default();
        queue.enqueue_push(update("agent-1", AgentStatus::Working));
        queue.enqueue_push(update("agent-2", AgentStatus::Idle));
        queue.enqueue_push(resized("terminal-1", 80));
        queue.enqueue_push(Control::AgentList {
            agents: vec![agent("agent-2", AgentStatus::Idle)],
        });
        queue.enqueue_push(closed("terminal-1"));
        queue.enqueue_push(closed("terminal-2"));

        let sent = drain(&mut queue);
        assert_eq!(
            sent.iter().map(String::as_str).collect::<Vec<_>>(),
            [
                "agent list of 1",
                "TerminalClosed { terminal_id: \"terminal-1\", exit_code: None }",
                "TerminalClosed { terminal_id: \"terminal-2\", exit_code: None }",
            ]
        );
    }

    #[test]
    fn an_answer_is_never_dropped_for_a_push() {
        let mut queue = OutboundQueue::default();
        queue.enqueue_reply(Control::AgentList {
            agents: vec![agent("agent-1", AgentStatus::Idle)],
        });
        queue.enqueue_reply(terminal_list(1));
        queue.enqueue_push(Control::AgentList { agents: Vec::new() });
        queue.enqueue_push(terminal_list(3));

        let sent = drain(&mut queue);
        assert_eq!(
            sent.iter().map(String::as_str).collect::<Vec<_>>(),
            [
                "agent list of 1",
                "terminal list of 1",
                "agent list of 0",
                "terminal list of 3"
            ]
        );
    }

    #[test]
    fn pushes_that_cannot_be_replaced_are_bounded() {
        let mut queue = OutboundQueue::default();
        for index in 0..MAX_QUEUED_PUSHES + 10 {
            queue.enqueue_push(closed(&format!("terminal-{index}")));
        }
        assert_eq!(queue.queued_pushes, MAX_QUEUED_PUSHES);
        let sent = drain(&mut queue);
        assert_eq!(sent.len(), MAX_QUEUED_PUSHES);
        assert!(
            sent[0].contains("terminal-10\""),
            "the oldest were let go: {}",
            sent[0]
        );
        assert_eq!(queue.queued_pushes, 0);
    }
}
