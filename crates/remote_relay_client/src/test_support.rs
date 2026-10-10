//! An in-process relay and an in-memory keychain.
//!
//! The relay does what the real one does and no more: it sets up sessions
//! between a host and its account's clients, routes binary frames on the
//! session id, forwards pairing messages untouched and announces revocations.
//! It never looks inside a frame, which is also what lets a test assert that
//! nothing it carried was readable.

use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use anyhow::{Result, anyhow};
use credentials_provider::CredentialsProvider;
use gpui::{AppContext as _, AsyncApp, Task};
use parking_lot::Mutex;
use remote_relay_protocol::{decode_relay_frame, encode_relay_frame};
use serde_json::{Value, json};
use smol::channel::{self, Receiver, Sender};

use crate::{
    CredentialError, OpenMode, RelayCredential, RelayCredentials, RelayLink, RelayTransport,
    TransportError, WireInbound, WireOutbound, encode_open,
};

type Reply<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + 'a>>;

/// A keychain that lives in a map, and can be told to fail.
#[derive(Default)]
pub struct InMemoryKeychain {
    entries: Mutex<HashMap<String, (String, Vec<u8>)>>,
    writes: Mutex<usize>,
    fail_reads: Mutex<bool>,
    fail_writes: Mutex<bool>,
}

impl InMemoryKeychain {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn seed(&self, url: &str, username: &str, password: &[u8]) {
        self.entries
            .lock()
            .insert(url.to_string(), (username.to_string(), password.to_vec()));
    }

    pub fn write_count(&self) -> usize {
        *self.writes.lock()
    }

    pub fn fail_reads(&self) {
        *self.fail_reads.lock() = true;
    }

    pub fn fail_writes(&self) {
        *self.fail_writes.lock() = true;
    }

    /// Stops failing, as a keychain does once it is unlocked.
    pub fn recover(&self) {
        *self.fail_reads.lock() = false;
        *self.fail_writes.lock() = false;
    }

    /// Every stored password, so a test can assert that a secret did not land
    /// somewhere it should not.
    pub fn stored_passwords(&self) -> Vec<Vec<u8>> {
        self.entries
            .lock()
            .values()
            .map(|(_, password)| password.clone())
            .collect()
    }
}

impl CredentialsProvider for InMemoryKeychain {
    fn read_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a AsyncApp,
    ) -> Reply<'a, Option<(String, Vec<u8>)>> {
        Box::pin(async move {
            if *self.fail_reads.lock() {
                return Err(anyhow!("the keychain is locked"));
            }
            Ok(self.entries.lock().get(url).cloned())
        })
    }

    fn write_credentials<'a>(
        &'a self,
        url: &'a str,
        username: &'a str,
        password: &'a [u8],
        _cx: &'a AsyncApp,
    ) -> Reply<'a, ()> {
        Box::pin(async move {
            if *self.fail_writes.lock() {
                return Err(anyhow!("the keychain refused the write"));
            }
            *self.writes.lock() += 1;
            self.seed(url, username, password);
            Ok(())
        })
    }

    fn delete_credentials<'a>(&'a self, url: &'a str, _cx: &'a AsyncApp) -> Reply<'a, ()> {
        Box::pin(async move {
            self.entries.lock().remove(url);
            Ok(())
        })
    }
}

/// A credential that is the same every time, or an error until told otherwise.
pub struct StaticCredentials {
    credential: Mutex<RelayCredential>,
    failure: Mutex<Option<CredentialError>>,
    invalidations: Mutex<usize>,
}

impl StaticCredentials {
    pub fn new(user_id: &str, device_id: &str, bearer: &str) -> Self {
        Self {
            credential: Mutex::new(RelayCredential {
                bearer: bearer.into(),
                user_id: user_id.into(),
                device_id: device_id.into(),
            }),
            failure: Mutex::new(None),
            invalidations: Mutex::new(0),
        }
    }

    pub fn failing(error: CredentialError) -> Self {
        let credentials = Self::new("user", "host-device", "token");
        *credentials.failure.lock() = Some(error);
        credentials
    }

    /// Signs in as someone else, as an account switch does.
    pub fn switch_account(&self, user_id: &str, device_id: &str, bearer: &str) {
        *self.credential.lock() = RelayCredential {
            bearer: bearer.into(),
            user_id: user_id.into(),
            device_id: device_id.into(),
        };
    }

    pub fn succeed(&self) {
        *self.failure.lock() = None;
    }

    pub fn invalidations(&self) -> usize {
        *self.invalidations.lock()
    }
}

impl RelayCredentials for StaticCredentials {
    fn credential(&self, _cx: &mut AsyncApp) -> Task<Result<RelayCredential, CredentialError>> {
        Task::ready(match self.failure.lock().clone() {
            Some(error) => Err(error),
            None => Ok(self.credential.lock().clone()),
        })
    }

    fn invalidate(&self) {
        *self.invalidations.lock() += 1;
    }
}

impl RelayCredentials for Arc<StaticCredentials> {
    fn credential(&self, cx: &mut AsyncApp) -> Task<Result<RelayCredential, CredentialError>> {
        self.as_ref().credential(cx)
    }

    fn invalidate(&self) {
        self.as_ref().invalidate();
    }
}

struct HostPort {
    device_id: String,
    inbound: Sender<WireInbound>,
    generation: u64,
}

struct ClientPort {
    sender: Sender<ClientFrame>,
    /// The socket of a real client, which can be told to close; a raw
    /// [`FakeClient`] has none.
    socket: Option<ClientSocket>,
}

struct ClientSocket {
    inbound: Sender<WireInbound>,
    generation: u64,
}

struct SessionRecord {
    client_device_id: String,
}

#[derive(Default)]
struct RelayState {
    attempts: usize,
    refuse_with: Option<(u16, Option<String>)>,
    last_url: Option<String>,
    last_bearer: Option<String>,
    host: Option<HostPort>,
    host_generation: u64,
    client_generation: u64,
    clients: HashMap<String, ClientPort>,
    sessions: HashMap<u32, SessionRecord>,
    next_session_id: u32,
    host_binary_frames_seen: Vec<Vec<u8>>,
    delivery_paused: bool,
    client_delivery_paused: bool,
    outbound_capacity: Option<usize>,
    hello_version: Option<u32>,
}

/// What a client gets from the relay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientFrame {
    Text(String),
    Binary(Vec<u8>),
}

#[derive(Clone, Default)]
pub struct FakeRelay {
    state: Arc<Mutex<RelayState>>,
}

impl FakeRelay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn transport(&self) -> Arc<FakeTransport> {
        Arc::new(FakeTransport {
            relay: self.clone(),
            client_device_id: None,
        })
    }

    /// A transport for a Zode that controls another: connecting through it
    /// with a `role=client` address makes `device_id` a client of the relay.
    pub fn client_transport(&self, device_id: &str) -> Arc<FakeTransport> {
        Arc::new(FakeTransport {
            relay: self.clone(),
            client_device_id: Some(device_id.to_string()),
        })
    }

    pub fn connection_attempts(&self) -> usize {
        self.state.lock().attempts
    }

    pub fn last_url(&self) -> Option<String> {
        self.state.lock().last_url.clone()
    }

    pub fn last_bearer(&self) -> Option<String> {
        self.state.lock().last_bearer.clone()
    }

    pub fn is_host_connected(&self) -> bool {
        self.state.lock().host.is_some()
    }

    pub fn refuse_connections(&self, status: u16) {
        self.state.lock().refuse_with = Some((status, None));
    }

    /// Refuses with the relay's error code in the body, as the real one does.
    pub fn refuse_connections_with(&self, status: u16, reason: &str) {
        self.state.lock().refuse_with = Some((status, Some(reason.to_string())));
    }

    /// The protocol version the relay announces in its hello.
    pub fn set_hello_version(&self, version: u32) {
        self.state.lock().hello_version = Some(version);
    }

    pub fn accept_connections(&self) {
        self.state.lock().refuse_with = None;
    }

    /// Stops the relay reading what the host writes, so the host's queue to it
    /// fills the way it does when a real relay cannot keep up.
    pub fn pause_delivery(&self) {
        self.state.lock().delivery_paused = true;
    }

    /// How many frames the host may have waiting to be written, for the next
    /// connection. Small values make the host's queue to the relay fill the way
    /// it does when many devices are attached at once.
    pub fn set_outbound_capacity(&self, capacity: usize) {
        self.state.lock().outbound_capacity = Some(capacity);
    }

    /// Like [`pause_delivery`](Self::pause_delivery), for what clients write.
    pub fn pause_client_delivery(&self) {
        self.state.lock().client_delivery_paused = true;
    }

    pub fn resume_delivery(&self) {
        self.state.lock().delivery_paused = false;
        self.state.lock().client_delivery_paused = false;
    }

    /// Every binary payload the host sent, as the relay saw it, to assert that
    /// plaintext never travels.
    pub fn binary_frames_from_host(&self) -> Vec<Vec<u8>> {
        self.state.lock().host_binary_frames_seen.clone()
    }

    /// Ends the host's socket the way the real relay would: `code` and
    /// `reason` are the WebSocket close code and reason text, `None` a socket
    /// that just died.
    pub fn drop_host_connection(&self, code: Option<u16>, reason: &str) {
        let host = self.state.lock().host.take();
        if let Some(host) = host
            && host
                .inbound
                .try_send(WireInbound::Closed {
                    code,
                    reason: reason.to_string(),
                })
                .is_err()
        {
            log::debug!("the host was already gone");
        }
        self.end_all_sessions("peer_gone");
        self.broadcast_presence();
    }

    /// What the relay does when one end of a session disconnects: it ends the
    /// session and tells the other end.
    fn end_all_sessions(&self, reason: &str) {
        let sessions: Vec<u32> = self.state.lock().sessions.keys().copied().collect();
        for session_id in sessions {
            self.end_session(session_id, reason);
        }
    }

    /// Tells the host that one of the account's devices was revoked.
    pub fn revoke_device(&self, device_id: &str) {
        self.send_to_host_text(json!({"t": "device_revoked", "deviceId": device_id}).to_string());
        self.close_client_socket(device_id, Some(4403), "device_revoked");
        if self.host_device_id().as_deref() == Some(device_id) {
            self.drop_host_connection(Some(4403), "device_revoked");
        }
    }

    /// Ends a client's socket the way the relay would. `None` for a socket
    /// that just died.
    pub fn close_client_socket(&self, device_id: &str, code: Option<u16>, reason: &str) {
        let socket = {
            let mut state = self.state.lock();
            // A raw `FakeClient` has no socket to close and keeps its port.
            let has_socket = state
                .clients
                .get(device_id)
                .is_some_and(|port| port.socket.is_some());
            if has_socket {
                state.clients.remove(device_id).and_then(|port| port.socket)
            } else {
                None
            }
        };
        if let Some(socket) = socket {
            if socket
                .inbound
                .try_send(WireInbound::Closed {
                    code,
                    reason: reason.to_string(),
                })
                .is_err()
            {
                log::debug!("the client was already gone");
            }
            self.end_sessions_of_client(device_id, "peer_gone");
        }
    }

    pub fn is_client_connected(&self, device_id: &str) -> bool {
        self.state
            .lock()
            .clients
            .get(device_id)
            .is_some_and(|port| port.socket.is_some())
    }

    fn end_sessions_of_client(&self, device_id: &str, reason: &str) {
        let sessions: Vec<u32> = self
            .state
            .lock()
            .sessions
            .iter()
            .filter(|(_, session)| session.client_device_id == device_id)
            .map(|(id, _)| *id)
            .collect();
        for session_id in sessions {
            self.end_session(session_id, reason);
        }
    }

    /// What the relay tells a client about the hosts it may open sessions to:
    /// the account's connected hosts, never the client's own device.
    fn presence_for(&self, client_device_id: &str) -> String {
        let hosts: Vec<String> = self
            .state
            .lock()
            .host
            .iter()
            .filter(|host| host.device_id != client_device_id)
            .map(|host| host.device_id.clone())
            .collect();
        json!({"t": "presence", "hosts": hosts}).to_string()
    }

    fn broadcast_presence(&self) {
        let devices: Vec<(String, Sender<WireInbound>)> = self
            .state
            .lock()
            .clients
            .iter()
            .filter_map(|(device_id, port)| {
                port.socket
                    .as_ref()
                    .map(|socket| (device_id.clone(), socket.inbound.clone()))
            })
            .collect();
        for (device_id, inbound) in devices {
            if inbound
                .try_send(WireInbound::Text(self.presence_for(&device_id)))
                .is_err()
            {
                log::debug!("a client's queue is full or gone");
            }
        }
    }

    pub fn connect_client(&self, device_id: &str) -> FakeClient {
        let (sender, receiver) = channel::unbounded();
        self.state.lock().clients.insert(
            device_id.to_string(),
            ClientPort {
                sender,
                socket: None,
            },
        );
        FakeClient {
            relay: self.clone(),
            device_id: device_id.to_string(),
            frames: receiver,
            buffer: Mutex::new(Vec::new()),
        }
    }

    fn send_to_host_text(&self, text: String) {
        let inbound = self
            .state
            .lock()
            .host
            .as_ref()
            .map(|host| host.inbound.clone());
        if let Some(inbound) = inbound
            && inbound.try_send(WireInbound::Text(text)).is_err()
        {
            log::debug!("the host's queue is full or gone");
        }
    }

    fn send_to_host_binary(&self, bytes: Vec<u8>) {
        let inbound = self
            .state
            .lock()
            .host
            .as_ref()
            .map(|host| host.inbound.clone());
        if let Some(inbound) = inbound
            && inbound.try_send(WireInbound::Binary(bytes)).is_err()
        {
            log::debug!("the host's queue is full or gone");
        }
    }

    fn send_to_client(&self, device_id: &str, frame: ClientFrame) {
        let sender = self
            .state
            .lock()
            .clients
            .get(device_id)
            .map(|client| client.sender.clone());
        if let Some(sender) = sender
            && sender.try_send(frame).is_err()
        {
            log::debug!("a client's queue is gone");
        }
    }

    fn client_for_session(&self, session_id: u32) -> Option<String> {
        self.state
            .lock()
            .sessions
            .get(&session_id)
            .map(|session| session.client_device_id.clone())
    }

    /// What the relay does with a frame the host wrote.
    fn route_from_host(&self, message: WireOutbound) {
        match message {
            WireOutbound::Binary(bytes) => {
                self.state
                    .lock()
                    .host_binary_frames_seen
                    .push(bytes.clone());
                let Ok(frame) = decode_relay_frame(&bytes) else {
                    return;
                };
                if let Some(client) = self.client_for_session(frame.session_id) {
                    self.send_to_client(&client, ClientFrame::Binary(bytes));
                }
            }
            WireOutbound::Text(text) => {
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    return;
                };
                let session_id = value
                    .get("sid")
                    .and_then(Value::as_u64)
                    .and_then(|sid| u32::try_from(sid).ok());
                let tag = value.get("t").and_then(Value::as_str).unwrap_or_default();
                let Some(session_id) = session_id else {
                    return;
                };
                if tag == "close" {
                    self.end_session(session_id, "closed");
                } else if tag.starts_with("pair_")
                    && let Some(client) = self.client_for_session(session_id)
                {
                    self.send_to_client(&client, ClientFrame::Text(text));
                }
            }
            WireOutbound::Close => {}
        }
    }

    /// What the relay does with a frame a client socket wrote.
    fn route_from_client(&self, device_id: &str, message: WireOutbound) {
        match message {
            WireOutbound::Binary(bytes) => {
                let Ok(frame) = decode_relay_frame(&bytes) else {
                    return;
                };
                if self.client_for_session(frame.session_id).as_deref() == Some(device_id) {
                    self.send_to_host_binary(bytes);
                }
            }
            WireOutbound::Text(text) => {
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    return;
                };
                let tag = value.get("t").and_then(Value::as_str).unwrap_or_default();
                let session_id = value
                    .get("sid")
                    .and_then(Value::as_u64)
                    .and_then(|sid| u32::try_from(sid).ok());
                match (tag, session_id) {
                    ("open", _) => self.open_for_client(device_id, &value),
                    ("close", Some(session_id)) => {
                        if self.client_for_session(session_id).as_deref() == Some(device_id) {
                            self.end_session(session_id, "closed");
                        } else {
                            self.error_to_client(device_id, "unauthorized");
                        }
                    }
                    (tag, Some(session_id)) if tag.starts_with("pair_") => {
                        if self.client_for_session(session_id).as_deref() == Some(device_id) {
                            self.send_to_host_text(text);
                        } else {
                            self.error_to_client(device_id, "unauthorized");
                        }
                    }
                    _ => self.error_to_client(device_id, "malformed"),
                }
            }
            WireOutbound::Close => {}
        }
    }

    fn error_to_client(&self, device_id: &str, code: &str) {
        self.send_to_client(
            device_id,
            ClientFrame::Text(json!({"t": "error", "code": code}).to_string()),
        );
    }

    fn open_for_client(&self, device_id: &str, message: &Value) {
        let Some(host_device_id) = message.get("host").and_then(Value::as_str) else {
            return self.error_to_client(device_id, "malformed");
        };
        if host_device_id == device_id {
            return self.error_to_client(device_id, "unauthorized");
        }
        let mode = message
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("session")
            .to_string();
        let session_id = {
            let mut state = self.state.lock();
            if state
                .host
                .as_ref()
                .is_none_or(|host| host.device_id != host_device_id)
            {
                drop(state);
                return self.error_to_client(device_id, "not_found");
            }
            state.next_session_id += 1;
            let session_id = state.next_session_id;
            state.sessions.insert(
                session_id,
                SessionRecord {
                    client_device_id: device_id.to_string(),
                },
            );
            session_id
        };
        self.send_to_client(
            device_id,
            ClientFrame::Text(
                json!({"t": "opened", "sid": session_id, "peer": host_device_id, "peerKind": "ide", "mode": mode})
                    .to_string(),
            ),
        );
        self.send_to_host_text(
            json!({"t": "opened", "sid": session_id, "peer": device_id, "peerKind": "ide", "mode": mode})
                .to_string(),
        );
    }

    fn end_session(&self, session_id: u32, reason: &str) {
        let Some(session) = self.state.lock().sessions.remove(&session_id) else {
            return;
        };
        let notice = json!({"t": "close", "sid": session_id, "reason": reason}).to_string();
        self.send_to_client(&session.client_device_id, ClientFrame::Text(notice.clone()));
        self.send_to_host_text(notice);
    }
}

/// Removes a client from the relay when its connection's task is dropped.
struct ClientGuard {
    relay: FakeRelay,
    device_id: String,
    generation: u64,
}

impl Drop for ClientGuard {
    fn drop(&mut self) {
        let ours = {
            let mut state = self.relay.state.lock();
            let ours = state
                .clients
                .get(&self.device_id)
                .and_then(|port| port.socket.as_ref())
                .is_some_and(|socket| socket.generation == self.generation);
            if ours {
                state.clients.remove(&self.device_id);
            }
            ours
        };
        if ours {
            self.relay
                .end_sessions_of_client(&self.device_id, "peer_gone");
        }
    }
}

pub struct FakeTransport {
    relay: FakeRelay,
    /// Who connects through this transport when it is a client's.
    client_device_id: Option<String>,
}

/// Removes the host from the relay when the connection's task is dropped.
struct HostGuard {
    relay: FakeRelay,
    generation: u64,
}

impl Drop for HostGuard {
    fn drop(&mut self) {
        let ended = {
            let mut state = self.relay.state.lock();
            let ours = state
                .host
                .as_ref()
                .is_some_and(|host| host.generation == self.generation);
            if ours {
                state.host = None;
            }
            ours
        };
        if ended {
            self.relay.end_all_sessions("peer_gone");
            self.relay.broadcast_presence();
        }
    }
}

/// The role in a relay address, as the relay reads it: nothing or `host` is a
/// host, `client` a client, and anything else -- empty, another case, given
/// twice -- is refused.
fn role_in(url: &str) -> Option<crate::RelayRole> {
    let Some((_, query)) = url.split_once('?') else {
        return Some(crate::RelayRole::Host);
    };
    let mut roles = query
        .split('&')
        .filter_map(|pair| pair.strip_prefix("role="));
    let role = roles.next();
    if roles.next().is_some() {
        return None;
    }
    match role {
        None => Some(crate::RelayRole::Host),
        Some("host") => Some(crate::RelayRole::Host),
        Some("client") => Some(crate::RelayRole::Client),
        Some(_) => None,
    }
}

impl RelayTransport for FakeTransport {
    fn connect(
        &self,
        url: String,
        bearer: String,
        cx: &AsyncApp,
    ) -> Task<Result<RelayLink, TransportError>> {
        match role_in(&url) {
            Some(crate::RelayRole::Client) => self.connect_as_client(url, bearer, cx),
            Some(crate::RelayRole::Host) => self.connect_as_host(url, bearer, cx),
            None => {
                let mut state = self.relay.state.lock();
                state.attempts += 1;
                state.last_url = Some(url);
                state.last_bearer = Some(bearer);
                Task::ready(Err(TransportError::Refused {
                    status: 400,
                    reason: Some("invalid_role".into()),
                }))
            }
        }
    }
}

impl FakeTransport {
    fn connect_as_client(
        &self,
        url: String,
        bearer: String,
        cx: &AsyncApp,
    ) -> Task<Result<RelayLink, TransportError>> {
        let relay = self.relay.clone();
        let device_id = self
            .client_device_id
            .clone()
            .unwrap_or_else(|| "client-device".to_string());
        let (inbound_sender, inbound) = channel::bounded(crate::INBOUND_QUEUE_CAPACITY);
        let capacity = relay
            .state
            .lock()
            .outbound_capacity
            .unwrap_or(crate::OUTBOUND_QUEUE_CAPACITY);
        let (outbound, outbound_receiver): (Sender<WireOutbound>, Receiver<WireOutbound>) =
            channel::bounded(capacity);
        let (frame_sender, frames) = channel::unbounded();

        let generation = {
            let mut state = relay.state.lock();
            state.attempts += 1;
            state.last_url = Some(url);
            state.last_bearer = Some(bearer);
            if let Some((status, reason)) = state.refuse_with.clone() {
                return Task::ready(Err(TransportError::Refused { status, reason }));
            }
            state.client_generation += 1;
            let generation = state.client_generation;
            let previous = state.clients.insert(
                device_id.clone(),
                ClientPort {
                    sender: frame_sender,
                    socket: Some(ClientSocket {
                        inbound: inbound_sender.clone(),
                        generation,
                    }),
                },
            );
            if let Some(socket) = previous.and_then(|port| port.socket)
                && socket
                    .inbound
                    .try_send(WireInbound::Closed {
                        code: Some(4409),
                        reason: "replaced".into(),
                    })
                    .is_err()
            {
                log::debug!("the replaced client was already gone");
            }
            generation
        };

        let hello_version = relay.state.lock().hello_version.unwrap_or(1);
        let hello = json!({"t": "hello", "relay": hello_version, "role": "client"}).to_string();
        for text in [hello, relay.presence_for(&device_id)] {
            if inbound_sender.try_send(WireInbound::Text(text)).is_err() {
                log::debug!("the client's queue is full or gone");
            }
        }

        let guard = ClientGuard {
            relay: relay.clone(),
            device_id: device_id.clone(),
            generation,
        };
        let forwarding = cx.background_spawn(async move {
            while let Ok(frame) = frames.recv().await {
                let message = match frame {
                    ClientFrame::Text(text) => WireInbound::Text(text),
                    ClientFrame::Binary(bytes) => WireInbound::Binary(bytes),
                };
                if inbound_sender.send(message).await.is_err() {
                    break;
                }
            }
        });
        let cx_executor = cx.background_executor().clone();
        let routing = cx.background_spawn(async move {
            let _guard = guard;
            let _forwarding = forwarding;
            let executor = cx_executor;
            while let Ok(message) = outbound_receiver.recv().await {
                while relay.state.lock().client_delivery_paused {
                    executor.timer(std::time::Duration::from_millis(5)).await;
                }
                relay.route_from_client(&device_id, message);
            }
        });
        Task::ready(Ok(RelayLink {
            inbound,
            outbound,
            _pump: routing,
        }))
    }

    fn connect_as_host(
        &self,
        url: String,
        bearer: String,
        cx: &AsyncApp,
    ) -> Task<Result<RelayLink, TransportError>> {
        let relay = self.relay.clone();
        let (inbound_sender, inbound) = channel::bounded(crate::INBOUND_QUEUE_CAPACITY);
        let capacity = relay
            .state
            .lock()
            .outbound_capacity
            .unwrap_or(crate::OUTBOUND_QUEUE_CAPACITY);
        let (outbound, outbound_receiver): (Sender<WireOutbound>, Receiver<WireOutbound>) =
            channel::bounded(capacity);

        let outcome = {
            let mut state = relay.state.lock();
            state.attempts += 1;
            state.last_url = Some(url);
            state.last_bearer = Some(bearer);
            match state.refuse_with.clone() {
                Some((status, reason)) => Err(TransportError::Refused { status, reason }),
                None => {
                    state.host_generation += 1;
                    let generation = state.host_generation;
                    if let Some(previous) = state.host.take()
                        && previous
                            .inbound
                            .try_send(WireInbound::Closed {
                                code: Some(4409),
                                reason: "replaced".into(),
                            })
                            .is_err()
                    {
                        log::debug!("the replaced host was already gone");
                    }
                    state.host = Some(HostPort {
                        device_id: "host-device".into(),
                        inbound: inbound_sender.clone(),
                        generation,
                    });
                    Ok(generation)
                }
            }
        };
        let generation = match outcome {
            Ok(generation) => generation,
            Err(error) => return Task::ready(Err(error)),
        };

        let guard = HostGuard {
            relay: relay.clone(),
            generation,
        };
        let hello_version = relay.state.lock().hello_version.unwrap_or(1);
        let hello = json!({"t": "hello", "relay": hello_version, "role": "host"}).to_string();
        if inbound_sender.try_send(WireInbound::Text(hello)).is_err() {
            log::debug!("the host's queue is full or gone");
        }
        relay.broadcast_presence();

        let executor = cx.background_executor().clone();
        let routing = cx.background_spawn(async move {
            let _guard = guard;
            while let Ok(message) = outbound_receiver.recv().await {
                while relay.state.lock().delivery_paused {
                    executor.timer(std::time::Duration::from_millis(5)).await;
                }
                relay.route_from_host(message);
            }
        });
        Task::ready(Ok(RelayLink {
            inbound,
            outbound,
            _pump: routing,
        }))
    }
}

/// A browser, as far as the relay is concerned: it opens sessions and sends
/// frames, and receives whatever the host sent it.
pub struct FakeClient {
    relay: FakeRelay,
    device_id: String,
    frames: Receiver<ClientFrame>,
    buffer: Mutex<Vec<ClientFrame>>,
}

impl FakeClient {
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Opens a session to the host, as the relay does: both ends are told.
    pub fn open(&self, host_device_id: &str, mode: OpenMode) -> u32 {
        // Parsed back from what the browser would send, so the open message
        // this crate writes is the one the relay reads.
        let message: Value =
            serde_json::from_str(&encode_open(host_device_id, mode)).unwrap_or(Value::Null);
        let mode_name = message["mode"].as_str().unwrap_or("session").to_string();

        let session_id = {
            let mut state = self.relay.state.lock();
            state.next_session_id += 1;
            let session_id = state.next_session_id;
            state.sessions.insert(
                session_id,
                SessionRecord {
                    client_device_id: self.device_id.clone(),
                },
            );
            session_id
        };
        self.relay.send_to_host_text(
            json!({"t": "opened", "sid": session_id, "peer": self.device_id, "peerKind": "web", "mode": mode_name})
                .to_string(),
        );
        self.relay.send_to_client(
            &self.device_id,
            ClientFrame::Text(
                json!({"t": "opened", "sid": session_id, "peer": host_device_id, "peerKind": "ide", "mode": mode_name})
                    .to_string(),
            ),
        );
        session_id
    }

    pub fn close(&self, session_id: u32) {
        self.relay.end_session(session_id, "closed");
    }

    pub fn send_binary(&self, session_id: u32, payload: &[u8]) {
        match encode_relay_frame(session_id, payload) {
            Ok(frame) => self.relay.send_to_host_binary(frame),
            Err(error) => log::error!("a test sent an unsendable frame: {error}"),
        }
    }

    /// A pairing message, forwarded to the host untouched.
    pub fn send_text(&self, raw: &str) {
        self.relay.send_to_host_text(raw.to_string());
    }

    /// Moves everything that has arrived into the buffer, then removes and
    /// returns the frames `select` claims. A frame nobody has asked about yet
    /// stays put, so asking for binary frames does not eat a pairing message.
    fn take_where<T>(&self, mut select: impl FnMut(&ClientFrame) -> Option<T>) -> Vec<T> {
        let mut buffer = self.buffer.lock();
        buffer.extend(std::iter::from_fn(|| self.frames.try_recv().ok()));
        let mut taken = Vec::new();
        buffer.retain(|frame| match select(frame) {
            Some(item) => {
                taken.push(item);
                false
            }
            None => true,
        });
        taken
    }

    /// Binary payloads received since last asked, with their session ids.
    pub fn take_binary(&self) -> Vec<(u32, Vec<u8>)> {
        self.take_where(|frame| match frame {
            ClientFrame::Binary(bytes) => decode_relay_frame(bytes)
                .ok()
                .map(|frame| (frame.session_id, frame.payload)),
            ClientFrame::Text(_) => None,
        })
    }

    /// Pairing messages received since last asked.
    pub fn take_pairing(&self) -> Vec<remote_relay_protocol::PairingMessage> {
        self.take_where(|frame| match frame {
            ClientFrame::Text(text) => match crate::parse_relay_text(text) {
                Ok(Some(crate::RelayText::Pairing { message, .. })) => Some(message),
                _ => None,
            },
            ClientFrame::Binary(_) => None,
        })
    }

    /// Whether the relay told this client that `session_id` closed.
    pub fn was_told_closed(&self, session_id: u32) -> bool {
        !self
            .take_where(|frame| match frame {
                ClientFrame::Text(text) => match crate::parse_relay_text(text) {
                    Ok(Some(crate::RelayText::Close { sid, .. })) if sid == session_id => Some(()),
                    _ => None,
                },
                ClientFrame::Binary(_) => None,
            })
            .is_empty()
    }
}

impl FakeRelay {
    /// The device id the relay believes the host connected as.
    pub fn host_device_id(&self) -> Option<String> {
        self.state
            .lock()
            .host
            .as_ref()
            .map(|host| host.device_id.clone())
    }
}
