//! One device's connection to the relay.
//!
//! Shared by the Zode that gets controlled and, later, the one that controls:
//! both open a WebSocket, are told who else is reachable, and exchange opaque
//! frames per session. What the frames mean is not this crate's business; the
//! encrypted channel and the messages inside it live in
//! `remote_relay_protocol`, and the pieces a responder needs to run them are in
//! [`secure_session`].

mod credentials;
mod device_identity;
mod environment;
mod initiator;
mod initiator_connect;
#[cfg(test)]
pub(crate) mod initiator_tests;
mod pairing_attempt;
#[cfg(test)]
mod pairing_tests;
mod relay_transport;
mod relay_wire;
#[cfg(any(test, feature = "test-support"))]
pub mod scripted_host;
pub mod secure_session;
mod session;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod trust_store;

use std::{sync::Arc, time::Duration};

use gpui::{AsyncApp, Context, EventEmitter, Task, WeakEntity};
use rand::Rng as _;
use remote_relay_protocol::{
    PairingMessage, RELAY_PROTOCOL_VERSION, decode_relay_frame, encode_relay_frame,
};
use smol::channel::{Sender, TrySendError};

pub use credentials::{
    AccountCredentials, CredentialError, RelayCredential, RelayCredentials,
    device_id_from_access_token,
};
pub use device_identity::{
    DirectoryDevice, DirectoryError, decode_public_key, delete_keypair, encode_public_key,
    list_devices, load_or_create_keypair, local_device_name, register_public_key,
};
pub use environment::{RegisteringCredentials, RelayEnvironment};
pub use initiator::{ConnectError, InitiatorState, RelayHost, RelayInitiator, RelayInitiatorEvent};
pub use pairing_attempt::{PairingAttempt, PairingAttemptEvent, PairingFailure, PairingStep};
pub use relay_transport::{
    INBOUND_QUEUE_CAPACITY, OUTBOUND_QUEUE_CAPACITY, RELAY_SUBPROTOCOL, RelayLink, RelayTransport,
    TransportError, WebSocketTransport, WireInbound, WireOutbound,
};
pub use relay_wire::{
    OpenMode, RelayText, WireError, encode_close, encode_open, encode_pairing, parse_relay_text,
};
pub use session::{
    HostFacts, MAX_STREAM_QUEUE_BYTES, RelaySession, RelaySessionEvent, SessionError,
    StreamReceiver, StreamSender, send_data_waiting,
};
pub use trust_store::{MAX_PINNED_DEVICES, PinnedDevice, TrustError, TrustRole, TrustStore};

const BACKOFF_FLOOR: Duration = Duration::from_secs(1);
const BACKOFF_CEILING: Duration = Duration::from_secs(60);
const BACKOFF_JITTER: f64 = 0.2;

/// A connection that ends sooner than this after it was accepted does not
/// count as a working one: a relay that accepts and then closes would
/// otherwise be retried every second forever.
const STABLE_CONNECTION: Duration = Duration::from_secs(30);

/// How long to wait before the next attempt: one second, doubling to a
/// minute, spread by up to 20% either way so that a relay restart does not
/// bring every host back in the same instant.
#[derive(Debug, Default, Clone, Copy)]
pub struct Backoff {
    attempt: u32,
}

impl Backoff {
    pub fn next_delay(&mut self) -> Duration {
        let spread = rand::rng().random_range(-BACKOFF_JITTER..=BACKOFF_JITTER);
        self.next_base_delay().mul_f64(1.0 + spread)
    }

    fn next_base_delay(&mut self) -> Duration {
        let delay = BACKOFF_FLOOR
            .saturating_mul(1u32.checked_shl(self.attempt).unwrap_or(u32::MAX))
            .min(BACKOFF_CEILING);
        self.attempt = self.attempt.saturating_add(1);
        delay
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

/// The WebSocket address of the relay for an API base such as
/// `https://api.zodekit.site/api`.
pub fn relay_url(api_url: &str) -> String {
    let trimmed = api_url.trim_end_matches('/');
    let websocket = if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        trimmed.to_string()
    };
    format!("{websocket}/relay")
}

/// Which door a connection came through. A host waits for devices to open
/// sessions to it; a client opens them to a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RelayRole {
    #[default]
    Host,
    Client,
}

/// The relay address for `role`. A host is the relay's default, so its address
/// carries nothing extra.
pub fn relay_url_for(api_url: &str, role: RelayRole) -> String {
    match role {
        RelayRole::Host => relay_url(api_url),
        RelayRole::Client => format!("{}?role=client", relay_url(api_url)),
    }
}

/// Whether `url` would send the access token over an unencrypted connection
/// to a machine other than this one.
fn sends_token_in_the_clear(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("ws://") else {
        return false;
    };
    let authority = rest.split(['/', '?']).next().unwrap_or_default();
    let host = match authority.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    !matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// Why a client stopped for good rather than reconnecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The account revoked this device (close code 4403 with the reason
    /// `device_revoked`, or the account service said so). The only reason that
    /// justifies forgetting the device's key and pairings.
    Revoked,
    /// The sign-in behind this connection was ended by the account service
    /// (close code 4403 with any other reason, such as a refresh token being
    /// reused). The device is not revoked: signing in again fixes it, so
    /// nothing is forgotten.
    SessionEnded,
    /// The account has used up its relay allowance (close code 4429).
    QuotaExceeded,
    /// Another process connected as this same device (close code 4409).
    /// Reconnecting would only start a fight over the one slot.
    Replaced,
    /// The relay speaks a framing this build does not.
    IncompatibleRelay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Fetching a credential or opening the socket.
    Connecting,
    Connected,
    /// Between attempts.
    Waiting,
    Stopped(StopReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayEvent {
    Connected,
    /// The socket ended. A new attempt follows unless [`RelayEvent::Stopped`]
    /// does.
    Disconnected,
    SessionOpened {
        session_id: u32,
        peer_device_id: String,
        mode: OpenMode,
    },
    SessionClosed {
        session_id: u32,
        reason: String,
    },
    Pairing {
        session_id: u32,
        message: PairingMessage,
    },
    Frame {
        session_id: u32,
        payload: Vec<u8>,
    },
    DeviceRevoked {
        device_id: String,
    },
    /// The account's hosts that are online, sent to a client socket on
    /// connect and whenever it changes.
    Presence {
        hosts: Vec<String>,
    },
    /// The relay refused something this end asked for, such as opening a
    /// session to a host that is not connected. It names no session.
    Error {
        code: String,
    },
    Stopped(StopReason),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SendError {
    #[error("not connected to the relay")]
    NotConnected,
    /// The relay is not keeping up. Hold the data and try again shortly.
    #[error("the relay connection is full")]
    Backpressure,
    #[error("the frame cannot be sent: {0}")]
    Unsendable(String),
}

/// A cloneable handle on the socket's write side. It does not keep the
/// connection alive: when the client reconnects or is dropped the queue it
/// writes to closes and every call answers [`SendError::NotConnected`].
#[derive(Clone)]
pub struct RelayWriter {
    outbound: Sender<WireOutbound>,
}

impl RelayWriter {
    pub fn send_pairing(&self, session_id: u32, message: &PairingMessage) -> Result<(), SendError> {
        let text = encode_pairing(session_id, message)
            .map_err(|error| SendError::Unsendable(error.to_string()))?;
        self.try_send(WireOutbound::Text(text))
    }

    /// Sends one binary frame on `session_id`. `payload` is opaque here.
    pub fn send_frame(&self, session_id: u32, payload: &[u8]) -> Result<(), SendError> {
        let frame = encode_relay_frame(session_id, payload)
            .map_err(|error| SendError::Unsendable(error.to_string()))?;
        self.try_send(WireOutbound::Binary(frame))
    }

    pub fn close_session(&self, session_id: u32) -> Result<(), SendError> {
        self.try_send(WireOutbound::Text(encode_close(session_id)))
    }

    /// Asks the relay to open a session to `host_device_id`. Only a client
    /// socket may; the answer arrives as [`RelayEvent::SessionOpened`] or
    /// [`RelayEvent::Error`].
    pub fn request_open(&self, host_device_id: &str, mode: OpenMode) -> Result<(), SendError> {
        self.try_send(WireOutbound::Text(encode_open(host_device_id, mode)))
    }

    /// Whether `frames` more frames fit in the queue to the socket. Checked
    /// before sealing a frame, because a sealed frame that is then dropped
    /// leaves a hole in the cipher's counter.
    pub fn has_capacity_for(&self, frames: usize) -> bool {
        self.outbound
            .capacity()
            .is_some_and(|capacity| capacity.saturating_sub(self.outbound.len()) >= frames)
    }

    fn try_send(&self, message: WireOutbound) -> Result<(), SendError> {
        self.outbound
            .try_send(message)
            .map_err(|error| match error {
                TrySendError::Full(_) => SendError::Backpressure,
                TrySendError::Closed(_) => SendError::NotConnected,
            })
    }
}

pub struct RelayClient {
    credentials: Arc<dyn RelayCredentials>,
    transport: Arc<dyn RelayTransport>,
    role: RelayRole,
    url: String,
    outbound: Option<Sender<WireOutbound>>,
    credential: Option<RelayCredential>,
    state: ConnectionState,
    _connection: Task<()>,
}

impl EventEmitter<RelayEvent> for RelayClient {}

enum Ended {
    Dropped,
    Stop(StopReason),
}

/// What to do about an upgrade the relay refused with an HTTP status.
enum Refusal {
    Stop(StopReason),
    /// The credential is no good; ask its source to start over, then retry.
    Invalidate,
    Retry,
}

fn refusal_for(status: u16, reason: Option<&str>) -> Refusal {
    match (status, reason) {
        (429, Some("quota_exceeded")) => Refusal::Stop(StopReason::QuotaExceeded),
        // 429 also answers a busy or rate-limited relay, which clears by itself.
        (400 | 426, _) => Refusal::Stop(StopReason::IncompatibleRelay),
        (401 | 403, _) => Refusal::Invalidate,
        _ => Refusal::Retry,
    }
}

/// What the relay's close code and reason mean for the client. Code 4403 is
/// shared by a revoked device and an ended sign-in, so only the explicit
/// `device_revoked` reason may yield the destructive [`StopReason::Revoked`].
fn stop_reason_for_close(code: Option<u16>, reason: &str) -> Option<StopReason> {
    match code {
        Some(4403) if reason == "device_revoked" => Some(StopReason::Revoked),
        Some(4403) => Some(StopReason::SessionEnded),
        Some(4429) => Some(StopReason::QuotaExceeded),
        Some(4409) => Some(StopReason::Replaced),
        _ => None,
    }
}

impl RelayClient {
    /// Connects right away, and keeps reconnecting until stopped or dropped.
    pub fn new(
        credentials: Arc<dyn RelayCredentials>,
        transport: Arc<dyn RelayTransport>,
        api_url: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_as(credentials, transport, api_url, RelayRole::Host, cx)
    }

    pub fn new_as(
        credentials: Arc<dyn RelayCredentials>,
        transport: Arc<dyn RelayTransport>,
        api_url: &str,
        role: RelayRole,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            credentials,
            transport,
            role,
            url: relay_url_for(api_url, role),
            outbound: None,
            credential: None,
            state: ConnectionState::Connecting,
            _connection: cx.spawn(async move |this, cx| Self::run(this, cx).await),
        }
    }

    pub fn state(&self) -> ConnectionState {
        self.state
    }

    pub fn is_connected(&self) -> bool {
        self.state == ConnectionState::Connected
    }

    /// Who this connection is, once a credential has been obtained.
    pub fn credential(&self) -> Option<&RelayCredential> {
        self.credential.as_ref()
    }

    pub fn role(&self) -> RelayRole {
        self.role
    }

    /// The write side of the current connection, if there is one.
    pub fn writer(&self) -> Option<RelayWriter> {
        self.outbound
            .clone()
            .map(|outbound| RelayWriter { outbound })
    }

    fn connected_writer(&self) -> Result<RelayWriter, SendError> {
        self.writer().ok_or(SendError::NotConnected)
    }

    pub fn send_pairing(&self, session_id: u32, message: &PairingMessage) -> Result<(), SendError> {
        self.connected_writer()?.send_pairing(session_id, message)
    }

    /// Sends one binary frame on `session_id`. `payload` is opaque here.
    pub fn send_frame(&self, session_id: u32, payload: &[u8]) -> Result<(), SendError> {
        self.connected_writer()?.send_frame(session_id, payload)
    }

    pub fn close_session(&self, session_id: u32) -> Result<(), SendError> {
        self.connected_writer()?.close_session(session_id)
    }

    pub fn request_open(&self, host_device_id: &str, mode: OpenMode) -> Result<(), SendError> {
        self.connected_writer()?.request_open(host_device_id, mode)
    }

    /// Whether `frames` more frames fit in the queue to the socket. Checked
    /// before sealing a frame, because a sealed frame that is then dropped
    /// leaves a hole in the cipher's counter.
    pub fn has_capacity_for(&self, frames: usize) -> bool {
        self.writer()
            .is_some_and(|writer| writer.has_capacity_for(frames))
    }

    async fn run(this: WeakEntity<Self>, cx: &mut AsyncApp) {
        let mut backoff = Backoff::default();
        loop {
            let Ok((credentials, transport, url)) = this.read_with(cx, |this, _| {
                (
                    this.credentials.clone(),
                    this.transport.clone(),
                    this.url.clone(),
                )
            }) else {
                return;
            };

            if sends_token_in_the_clear(&url) {
                log::error!("refusing to send the access token to {url} without encryption");
                Self::stop(&this, StopReason::IncompatibleRelay, cx);
                return;
            }

            let credential = match credentials.credential(cx).await {
                Ok(credential) => credential,
                Err(CredentialError::Revoked) => {
                    Self::stop(&this, StopReason::Revoked, cx);
                    return;
                }
                Err(CredentialError::Unavailable(reason)) => {
                    log::debug!("no relay credential yet: {reason}");
                    if !Self::wait(&this, &mut backoff, cx).await {
                        return;
                    }
                    continue;
                }
            };
            if this
                .update(cx, |this, _| this.credential = Some(credential.clone()))
                .is_err()
            {
                return;
            }

            let link = match transport.connect(url, credential.bearer.clone(), cx).await {
                Ok(link) => link,
                Err(TransportError::Refused { status, reason }) => {
                    log::warn!(
                        "the relay refused the connection with status {status} ({})",
                        reason.as_deref().unwrap_or("no reason given")
                    );
                    match refusal_for(status, reason.as_deref()) {
                        Refusal::Stop(stop_reason) => {
                            Self::stop(&this, stop_reason, cx);
                            return;
                        }
                        Refusal::Invalidate => credentials.invalidate(),
                        Refusal::Retry => {}
                    }
                    if !Self::wait(&this, &mut backoff, cx).await {
                        return;
                    }
                    continue;
                }
                Err(error) => {
                    log::warn!("could not connect to the relay: {error}");
                    if !Self::wait(&this, &mut backoff, cx).await {
                        return;
                    }
                    continue;
                }
            };

            let connected = this.update(cx, |this, cx| {
                this.outbound = Some(link.outbound.clone());
                this.state = ConnectionState::Connected;
                cx.emit(RelayEvent::Connected);
                cx.notify();
            });
            if connected.is_err() {
                return;
            }
            let connected_at = cx.background_executor().now();

            let ended = Self::read_until_closed(&this, &link, cx).await;

            if cx.background_executor().now().duration_since(connected_at) >= STABLE_CONNECTION {
                backoff.reset();
            }

            let detached = this.update(cx, |this, cx| {
                this.outbound = None;
                this.state = ConnectionState::Waiting;
                cx.emit(RelayEvent::Disconnected);
                cx.notify();
            });
            drop(link);
            if detached.is_err() {
                return;
            }
            if let Ended::Stop(reason) = ended {
                Self::stop(&this, reason, cx);
                return;
            }
            if !Self::wait(&this, &mut backoff, cx).await {
                return;
            }
        }
    }

    /// Sleeps out one backoff step. `false` means the client is gone.
    async fn wait(this: &WeakEntity<Self>, backoff: &mut Backoff, cx: &mut AsyncApp) -> bool {
        let marked = this.update(cx, |this, cx| {
            this.state = ConnectionState::Waiting;
            cx.notify();
        });
        if marked.is_err() {
            return false;
        }
        cx.background_executor().timer(backoff.next_delay()).await;
        this.update(cx, |this, cx| {
            this.state = ConnectionState::Connecting;
            cx.notify();
        })
        .is_ok()
    }

    fn stop(this: &WeakEntity<Self>, reason: StopReason, cx: &mut AsyncApp) {
        let stopped = this.update(cx, |this, cx| {
            this.state = ConnectionState::Stopped(reason);
            cx.emit(RelayEvent::Stopped(reason));
            cx.notify();
        });
        if stopped.is_err() {
            log::debug!("the relay client was dropped before it could report {reason:?}");
        }
    }

    async fn read_until_closed(
        this: &WeakEntity<Self>,
        link: &RelayLink,
        cx: &mut AsyncApp,
    ) -> Ended {
        while let Ok(inbound) = link.inbound.recv().await {
            let event = match inbound {
                WireInbound::Closed { code, reason } => {
                    return stop_reason_for_close(code, &reason)
                        .map_or(Ended::Dropped, Ended::Stop);
                }
                WireInbound::Binary(bytes) => match decode_relay_frame(&bytes) {
                    Ok(frame) => Some(RelayEvent::Frame {
                        session_id: frame.session_id,
                        payload: frame.payload,
                    }),
                    Err(error) => {
                        log::warn!("the relay sent a malformed frame: {error}");
                        None
                    }
                },
                WireInbound::Text(text) => match parse_relay_text(&text) {
                    Ok(Some(RelayText::Hello { relay, .. })) if relay != RELAY_PROTOCOL_VERSION => {
                        log::error!(
                            "the relay speaks version {relay}, not {RELAY_PROTOCOL_VERSION}"
                        );
                        return Ended::Stop(StopReason::IncompatibleRelay);
                    }
                    Ok(Some(text)) => Self::event_for(text),
                    Ok(None) => None,
                    Err(error) => {
                        log::warn!("the relay sent a message this build cannot read: {error}");
                        None
                    }
                },
            };
            if let Some(event) = event
                && this.update(cx, |_, cx| cx.emit(event)).is_err()
            {
                return Ended::Dropped;
            }
        }
        Ended::Dropped
    }

    fn event_for(text: RelayText) -> Option<RelayEvent> {
        match text {
            RelayText::Opened {
                sid, peer, mode, ..
            } => Some(RelayEvent::SessionOpened {
                session_id: sid,
                peer_device_id: peer,
                mode,
            }),
            RelayText::Close { sid, reason } => Some(RelayEvent::SessionClosed {
                session_id: sid,
                reason,
            }),
            RelayText::Pairing { sid, message } => Some(RelayEvent::Pairing {
                session_id: sid,
                message,
            }),
            RelayText::DeviceRevoked { device_id } => Some(RelayEvent::DeviceRevoked { device_id }),
            RelayText::Error { code } => {
                log::warn!("the relay reported an error: {code}");
                Some(RelayEvent::Error { code })
            }
            RelayText::Presence { hosts } => Some(RelayEvent::Presence { hosts }),
            RelayText::Hello { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FakeRelay, StaticCredentials};
    use gpui::{AppContext as _, Entity, TestAppContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn backoff_doubles_from_one_second_to_a_minute_and_resets() {
        let mut backoff = Backoff::default();
        let delays: Vec<u64> = (0..9)
            .map(|_| backoff.next_base_delay().as_secs())
            .collect();
        assert_eq!(delays, vec![1, 2, 4, 8, 16, 32, 60, 60, 60]);
        backoff.reset();
        assert_eq!(backoff.next_base_delay(), Duration::from_secs(1));
        for _ in 0..200 {
            backoff.next_base_delay();
        }
        assert_eq!(
            backoff.next_base_delay(),
            BACKOFF_CEILING,
            "no overflow however long it fails"
        );
    }

    #[test]
    fn the_delay_is_spread_around_the_base_so_hosts_do_not_return_together() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..50 {
            let mut backoff = Backoff::default();
            for _ in 0..3 {
                backoff.next_delay();
            }
            let delay = backoff.next_delay();
            assert!(
                delay >= Duration::from_millis(6400) && delay <= Duration::from_millis(9600),
                "{delay:?} is outside 8 s +/- 20%"
            );
            seen.insert(delay);
        }
        assert!(seen.len() > 1, "every delay was identical");
    }

    #[test]
    fn a_token_is_never_sent_unencrypted_to_another_machine() {
        assert!(sends_token_in_the_clear("ws://relay.example.com/api/relay"));
        assert!(sends_token_in_the_clear("ws://10.0.0.5:8000/api/relay"));
        assert!(!sends_token_in_the_clear(
            "wss://relay.example.com/api/relay"
        ));
        assert!(!sends_token_in_the_clear("ws://localhost:8000/api/relay"));
        assert!(!sends_token_in_the_clear("ws://127.0.0.1:8000/api/relay"));
        assert!(!sends_token_in_the_clear("ws://[::1]:8000/api/relay"));
    }

    #[test]
    fn the_relay_address_follows_the_api_address() {
        assert_eq!(
            relay_url("https://api.zodekit.site/api"),
            "wss://api.zodekit.site/api/relay"
        );
        assert_eq!(
            relay_url("http://localhost:8000/api/"),
            "ws://localhost:8000/api/relay"
        );
    }

    #[test]
    fn only_the_documented_close_codes_stop_the_client() {
        assert_eq!(
            stop_reason_for_close(Some(4403), "device_revoked"),
            Some(StopReason::Revoked)
        );
        assert_eq!(
            stop_reason_for_close(Some(4429), ""),
            Some(StopReason::QuotaExceeded)
        );
        assert_eq!(
            stop_reason_for_close(Some(4409), "replaced"),
            Some(StopReason::Replaced)
        );
        assert_eq!(stop_reason_for_close(Some(4408), ""), None);
        assert_eq!(stop_reason_for_close(Some(1001), ""), None);
        assert_eq!(stop_reason_for_close(None, ""), None);
    }

    #[test]
    fn only_an_explicit_device_revocation_is_destructive() {
        for reason in ["session_revoked", "", "something_new"] {
            assert_eq!(
                stop_reason_for_close(Some(4403), reason),
                Some(StopReason::SessionEnded),
                "reason {reason:?} must not wipe the device"
            );
        }
    }

    #[test]
    fn refusals_map_to_a_stop_a_credential_reset_or_a_retry() {
        use Refusal::*;
        assert!(matches!(
            refusal_for(429, Some("quota_exceeded")),
            Stop(StopReason::QuotaExceeded)
        ));
        assert!(matches!(refusal_for(429, Some("busy")), Retry));
        assert!(matches!(refusal_for(429, None), Retry));
        assert!(matches!(
            refusal_for(400, Some("subprotocol_required")),
            Stop(StopReason::IncompatibleRelay)
        ));
        assert!(matches!(
            refusal_for(426, None),
            Stop(StopReason::IncompatibleRelay)
        ));
        assert!(matches!(
            refusal_for(401, Some("session_ended")),
            Invalidate
        ));
        assert!(matches!(refusal_for(403, None), Invalidate));
        assert!(matches!(refusal_for(503, None), Retry));
    }

    struct Recorded {
        events: Rc<RefCell<Vec<RelayEvent>>>,
        _subscription: gpui::Subscription,
    }

    fn client(relay: &FakeRelay, cx: &mut TestAppContext) -> (Entity<RelayClient>, Recorded) {
        let transport = relay.transport();
        let client = cx.new(|cx| {
            RelayClient::new(
                Arc::new(StaticCredentials::new("user", "host-device", "token")),
                transport,
                "https://api.test/api",
                cx,
            )
        });
        let events = Rc::new(RefCell::new(Vec::new()));
        let subscription = cx.update(|cx| {
            let events = events.clone();
            cx.subscribe(&client, move |_, event: &RelayEvent, _| {
                events.borrow_mut().push(event.clone());
            })
        });
        (
            client,
            Recorded {
                events,
                _subscription: subscription,
            },
        )
    }

    #[gpui::test]
    async fn it_connects_with_the_bearer_and_reports_it(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        let (client, recorded) = client(&relay, cx);
        cx.run_until_parked();

        assert!(client.read_with(cx, |client, _| client.is_connected()));
        assert_eq!(relay.connection_attempts(), 1);
        assert_eq!(
            relay.last_url().as_deref(),
            Some("wss://api.test/api/relay")
        );
        assert_eq!(relay.last_bearer().as_deref(), Some("token"));
        assert_eq!(
            recorded.events.borrow().as_slice(),
            &[RelayEvent::Connected]
        );
        assert_eq!(
            client.read_with(cx, |client, _| client
                .credential()
                .map(|c| c.device_id.clone())),
            Some("host-device".to_string())
        );
    }

    #[gpui::test]
    async fn sessions_pairing_frames_and_revocations_become_events(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        let (_client, recorded) = client(&relay, cx);
        cx.run_until_parked();

        let browser = relay.connect_client("browser-1");
        let session_id = browser.open("host-device", OpenMode::Session);
        browser.send_binary(session_id, b"opaque");
        browser.send_text(
            &encode_pairing(session_id, &PairingMessage::PairReveal { nonce: [4; 32] }).unwrap(),
        );
        relay.revoke_device("browser-9");
        browser.close(session_id);
        cx.run_until_parked();

        assert_eq!(
            recorded.events.borrow().as_slice(),
            &[
                RelayEvent::Connected,
                RelayEvent::SessionOpened {
                    session_id,
                    peer_device_id: "browser-1".into(),
                    mode: OpenMode::Session
                },
                RelayEvent::Frame {
                    session_id,
                    payload: b"opaque".to_vec()
                },
                RelayEvent::Pairing {
                    session_id,
                    message: PairingMessage::PairReveal { nonce: [4; 32] }
                },
                RelayEvent::DeviceRevoked {
                    device_id: "browser-9".into()
                },
                RelayEvent::SessionClosed {
                    session_id,
                    reason: "closed".into()
                },
            ]
        );
    }

    #[gpui::test]
    async fn frames_and_close_requests_reach_the_peer(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        let (client, _recorded) = client(&relay, cx);
        cx.run_until_parked();
        let browser = relay.connect_client("browser-1");
        let session_id = browser.open("host-device", OpenMode::Session);
        cx.run_until_parked();

        client.read_with(cx, |client, _| {
            client.send_frame(session_id, b"sealed").unwrap();
            client
                .send_pairing(session_id, &PairingMessage::PairReveal { nonce: [1; 32] })
                .unwrap();
        });
        cx.run_until_parked();
        assert_eq!(
            browser.take_binary(),
            vec![(session_id, b"sealed".to_vec())]
        );
        assert_eq!(browser.take_pairing().len(), 1);

        client.read_with(cx, |client, _| client.close_session(session_id).unwrap());
        cx.run_until_parked();
        assert!(browser.was_told_closed(session_id));
    }

    #[gpui::test]
    async fn sending_while_disconnected_is_an_error_not_a_panic(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        relay.refuse_connections(503);
        let (client, _recorded) = client(&relay, cx);
        cx.run_until_parked();
        client.read_with(cx, |client, _| {
            assert_eq!(client.send_frame(1, b"x"), Err(SendError::NotConnected));
            assert!(!client.has_capacity_for(1));
        });
    }

    #[gpui::test]
    async fn a_dropped_connection_is_retried_after_the_backoff(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        let (client, recorded) = client(&relay, cx);
        cx.run_until_parked();
        relay.drop_host_connection(None, "");
        cx.run_until_parked();
        assert!(!client.read_with(cx, |client, _| client.is_connected()));
        assert_eq!(
            relay.connection_attempts(),
            1,
            "it waits before trying again"
        );

        cx.executor().advance_clock(Duration::from_millis(1300));
        cx.run_until_parked();
        assert_eq!(relay.connection_attempts(), 2);
        assert!(client.read_with(cx, |client, _| client.is_connected()));
        assert_eq!(
            recorded.events.borrow().as_slice(),
            &[
                RelayEvent::Connected,
                RelayEvent::Disconnected,
                RelayEvent::Connected
            ]
        );
    }

    #[gpui::test]
    async fn failed_attempts_back_off_progressively(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        relay.refuse_connections(503);
        let (_client, _recorded) = client(&relay, cx);
        cx.run_until_parked();
        assert_eq!(relay.connection_attempts(), 1);
        // Each wait is jittered by up to 20%: the first is under 1.2 s, the
        // second between 1.6 s and 2.4 s.
        cx.executor().advance_clock(Duration::from_millis(1300));
        cx.run_until_parked();
        assert_eq!(relay.connection_attempts(), 2);
        cx.executor().advance_clock(Duration::from_millis(1000));
        cx.run_until_parked();
        assert_eq!(
            relay.connection_attempts(),
            2,
            "the second wait is about two seconds"
        );
        cx.executor().advance_clock(Duration::from_millis(2500));
        cx.run_until_parked();
        assert_eq!(relay.connection_attempts(), 3);
    }

    #[gpui::test]
    async fn a_relay_that_accepts_then_closes_is_not_retried_at_the_floor(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        let (_client, _recorded) = client(&relay, cx);
        cx.run_until_parked();
        relay.drop_host_connection(Some(4400), "malformed");
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(1300));
        cx.run_until_parked();
        assert_eq!(relay.connection_attempts(), 2);

        relay.drop_host_connection(Some(4400), "malformed");
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(1300));
        cx.run_until_parked();
        assert_eq!(
            relay.connection_attempts(),
            2,
            "a connection that did not last must not reset the delay"
        );
        cx.executor().advance_clock(Duration::from_millis(1200));
        cx.run_until_parked();
        assert_eq!(relay.connection_attempts(), 3);
    }

    #[gpui::test]
    async fn a_connection_that_lasted_starts_the_delays_over(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        let (_client, _recorded) = client(&relay, cx);
        cx.run_until_parked();
        // Two quick failures push the next delay to about four seconds.
        relay.drop_host_connection(Some(4400), "malformed");
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(1300));
        cx.run_until_parked();
        relay.drop_host_connection(Some(4400), "malformed");
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(2500));
        cx.run_until_parked();
        assert_eq!(relay.connection_attempts(), 3);

        cx.executor().advance_clock(Duration::from_secs(40));
        cx.run_until_parked();
        relay.drop_host_connection(None, "");
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(1300));
        cx.run_until_parked();
        assert_eq!(
            relay.connection_attempts(),
            4,
            "after a long-lived connection the wait is one second again"
        );
    }

    #[gpui::test]
    async fn revocation_stops_the_client_for_good(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        let (client, recorded) = client(&relay, cx);
        cx.run_until_parked();
        relay.drop_host_connection(Some(4403), "device_revoked");
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_secs(120));
        cx.run_until_parked();

        assert_eq!(
            client.read_with(cx, |client, _| client.state()),
            ConnectionState::Stopped(StopReason::Revoked)
        );
        assert_eq!(relay.connection_attempts(), 1, "it must not reconnect");
        assert_eq!(
            recorded.events.borrow().last(),
            Some(&RelayEvent::Stopped(StopReason::Revoked))
        );
    }

    #[gpui::test]
    async fn quota_and_replacement_also_stop_it(cx: &mut TestAppContext) {
        for (code, close_reason, reason) in [
            (4429, "quota_exceeded", StopReason::QuotaExceeded),
            (4409, "replaced", StopReason::Replaced),
        ] {
            let relay = FakeRelay::new();
            let (client, _recorded) = client(&relay, cx);
            cx.run_until_parked();
            relay.drop_host_connection(Some(code), close_reason);
            cx.run_until_parked();
            assert_eq!(
                client.read_with(cx, |client, _| client.state()),
                ConnectionState::Stopped(reason)
            );
        }
    }

    #[gpui::test]
    async fn a_refusal_with_403_asks_the_credential_source_to_start_over(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        relay.refuse_connections(403);
        let credentials = Arc::new(StaticCredentials::new("user", "host-device", "token"));
        let transport = relay.transport();
        let _client = cx
            .new(|cx| RelayClient::new(credentials.clone(), transport, "https://api.test/api", cx));
        cx.run_until_parked();
        assert_eq!(credentials.invalidations(), 1);
    }

    #[gpui::test]
    async fn a_revoked_credential_stops_without_connecting(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        let credentials = Arc::new(StaticCredentials::failing(CredentialError::Revoked));
        let transport = relay.transport();
        let client =
            cx.new(|cx| RelayClient::new(credentials, transport, "https://api.test/api", cx));
        cx.run_until_parked();
        assert_eq!(
            client.read_with(cx, |client, _| client.state()),
            ConnectionState::Stopped(StopReason::Revoked)
        );
        assert_eq!(relay.connection_attempts(), 0);
    }

    #[gpui::test]
    async fn an_unavailable_credential_waits_and_tries_again(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        let credentials = Arc::new(StaticCredentials::failing(CredentialError::Unavailable(
            "signed out".into(),
        )));
        let transport = relay.transport();
        let client = cx
            .new(|cx| RelayClient::new(credentials.clone(), transport, "https://api.test/api", cx));
        cx.run_until_parked();
        assert_eq!(relay.connection_attempts(), 0);

        credentials.succeed();
        cx.executor().advance_clock(Duration::from_millis(1300));
        cx.run_until_parked();
        assert!(client.read_with(cx, |client, _| client.is_connected()));
    }

    #[gpui::test]
    async fn a_revoked_sign_in_stops_the_client_without_calling_the_device_revoked(
        cx: &mut TestAppContext,
    ) {
        let relay = FakeRelay::new();
        let (client, recorded) = client(&relay, cx);
        cx.run_until_parked();
        relay.drop_host_connection(Some(4403), "session_revoked");
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_secs(120));
        cx.run_until_parked();

        assert_eq!(
            client.read_with(cx, |client, _| client.state()),
            ConnectionState::Stopped(StopReason::SessionEnded)
        );
        assert_eq!(relay.connection_attempts(), 1, "it must not reconnect");
        assert_eq!(
            recorded.events.borrow().last(),
            Some(&RelayEvent::Stopped(StopReason::SessionEnded))
        );
    }

    #[gpui::test]
    async fn an_exhausted_quota_at_upgrade_halts_instead_of_retrying(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        relay.refuse_connections_with(429, "quota_exceeded");
        let (client, _recorded) = client(&relay, cx);
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_secs(300));
        cx.run_until_parked();
        assert_eq!(
            client.read_with(cx, |client, _| client.state()),
            ConnectionState::Stopped(StopReason::QuotaExceeded)
        );
        assert_eq!(relay.connection_attempts(), 1);
    }

    #[gpui::test]
    async fn a_busy_relay_is_retried_not_halted(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        relay.refuse_connections_with(429, "busy");
        let (client, _recorded) = client(&relay, cx);
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(1300));
        cx.run_until_parked();
        assert_eq!(relay.connection_attempts(), 2);
        assert!(!matches!(
            client.read_with(cx, |client, _| client.state()),
            ConnectionState::Stopped(_)
        ));
    }

    #[gpui::test]
    async fn a_relay_that_refuses_the_dialect_halts_as_incompatible(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        relay.refuse_connections_with(400, "subprotocol_required");
        let (client, _recorded) = client(&relay, cx);
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_secs(300));
        cx.run_until_parked();
        assert_eq!(
            client.read_with(cx, |client, _| client.state()),
            ConnectionState::Stopped(StopReason::IncompatibleRelay)
        );
        assert_eq!(relay.connection_attempts(), 1);
    }

    #[gpui::test]
    async fn a_rejected_token_asks_the_credential_source_to_start_over(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        relay.refuse_connections_with(401, "session_ended");
        let credentials = Arc::new(StaticCredentials::new("user", "host-device", "token"));
        let transport = relay.transport();
        let _client = cx
            .new(|cx| RelayClient::new(credentials.clone(), transport, "https://api.test/api", cx));
        cx.run_until_parked();
        assert_eq!(credentials.invalidations(), 1);
        cx.executor().advance_clock(Duration::from_millis(1300));
        cx.run_until_parked();
        assert_eq!(
            relay.connection_attempts(),
            2,
            "it keeps trying with backoff"
        );
    }

    #[gpui::test]
    async fn a_hello_from_another_relay_version_halts_as_incompatible(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        relay.set_hello_version(2);
        let (client, _recorded) = client(&relay, cx);
        cx.run_until_parked();
        assert_eq!(
            client.read_with(cx, |client, _| client.state()),
            ConnectionState::Stopped(StopReason::IncompatibleRelay)
        );
    }

    #[gpui::test]
    async fn a_token_is_not_sent_over_cleartext_to_a_remote_relay(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        let transport = relay.transport();
        let client = cx.new(|cx| {
            RelayClient::new(
                Arc::new(StaticCredentials::new("user", "host-device", "token")),
                transport,
                "http://relay.example.com/api",
                cx,
            )
        });
        cx.run_until_parked();
        assert_eq!(relay.connection_attempts(), 0);
        assert_eq!(
            client.read_with(cx, |client, _| client.state()),
            ConnectionState::Stopped(StopReason::IncompatibleRelay)
        );
    }

    #[gpui::test]
    async fn dropping_the_client_closes_the_connection(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        let (client, recorded) = client(&relay, cx);
        cx.run_until_parked();
        assert!(relay.is_host_connected());
        drop(recorded);
        drop(client);
        // Dropped entities are released when effects flush.
        cx.update(|_| {});
        cx.run_until_parked();
        assert!(!relay.is_host_connected());
    }
}
