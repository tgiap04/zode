//! The Zode that controls another: it holds a client connection to the relay,
//! opens sessions to hosts it has paired with, and runs the encrypted channel
//! from the initiator's end.
//!
//! Nothing here connects until a [`RelayInitiator`] is created, and one is
//! created only when the person opens the relay devices list or a project on
//! another Zode. Dropping the last holder of it closes the socket.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context as _, Result, anyhow};
use futures::{FutureExt as _, channel::oneshot, future::Shared};
use gpui::{
    App, AppContext as _, AsyncApp, Context, Entity, EventEmitter, Global, Subscription, Task,
    WeakEntity,
};
use remote_relay_protocol::{DeviceKeypair, KEY_LEN, PairingMessage};
use smol::channel::{Receiver, Sender, TrySendError};
use zode_account::Account;

use crate::{
    ConnectionState, OpenMode, PinnedDevice, RegisteringCredentials, RelayClient, RelayEnvironment,
    RelayEvent, RelayRole, RelaySession, RelayTransport, RelayWriter, StopReason, TrustRole,
    TrustStore, WebSocketTransport, delete_keypair, list_devices, load_or_create_keypair,
    session::RetiredSessions,
};

/// How long to wait for the relay to connect before giving up on an action.
pub(crate) const CONNECT_WAIT: Duration = Duration::from_secs(15);

/// How long the relay has to answer an `open`.
pub(crate) const OPEN_WAIT: Duration = Duration::from_secs(10);

/// Frames and notices that may wait for a handshake that has not read them
/// yet. A relay that floods a session before it is established is cut off.
pub(crate) const AWAITING_QUEUE: usize = 64;

pub(crate) const CAPABILITIES: [&str; 2] = ["terminal", "ide"];

/// Why a session to a host could not be established.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ConnectError {
    #[error("{0}")]
    Unavailable(String),
    #[error("the relay connection ended: {0}")]
    Relay(String),
    #[error("that Zode is not online")]
    HostOffline,
    #[error("the relay is busy right now; try again in a moment")]
    RateLimited,
    #[error("the relay does not let this device reach that Zode")]
    Unauthorized,
    #[error("that Zode does not trust this one. Pair them again")]
    NotPaired,
    #[error("that Zode did not answer in time")]
    TimedOut,
    #[error("the secure channel could not be set up: {0}")]
    Handshake(String),
    #[error("that Zode sent something unexpected: {0}")]
    Protocol(String),
    /// The host answered `hello` with an error of its own.
    #[error("{message}")]
    Refused { code: String, message: String },
}

/// A Zode of the account that this one may control, as the directory lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayHost {
    pub device_id: String,
    pub name: String,
    /// The key the directory holds for the device. Pairing refuses a host
    /// whose key differs, and cannot start without one.
    pub public_key: Option<[u8; KEY_LEN]>,
    pub online: bool,
    pub paired: bool,
    /// Paired, but the directory now lists another key for the device.
    pub key_changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayInitiatorEvent {
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitiatorState {
    /// Reading the device key and the pinned hosts.
    Starting,
    Failed(String),
    Relay(ConnectionState),
}

/// What an established-or-establishing session receives from the relay.
pub(crate) enum SidEvent {
    Frame(Vec<u8>),
    Pairing(PairingMessage),
    /// The relay ended this session.
    Closed(String),
    /// The connection to the relay itself was lost, which ends every session.
    LinkLost(String),
}

pub(crate) enum Route {
    /// A handshake or a pairing is reading the session itself.
    Awaiting(Sender<SidEvent>),
    Session(WeakEntity<RelaySession>),
}

pub(crate) struct PendingOpen {
    pub(crate) host_device_id: String,
    pub(crate) mode: OpenMode,
    pub(crate) respond: oneshot::Sender<Result<(u32, Receiver<SidEvent>), ConnectError>>,
}

pub(crate) struct Identity {
    pub(crate) keypair: Arc<DeviceKeypair>,
    pub(crate) user_id: String,
    pub(crate) device_id: String,
}

struct GlobalRelayInitiator(WeakEntity<RelayInitiator>);
impl Global for GlobalRelayInitiator {}

pub struct RelayInitiator {
    pub(crate) environment: Arc<RelayEnvironment>,
    pub(crate) app_version: String,
    transport: Arc<dyn RelayTransport>,
    pub(crate) identity: Option<Identity>,
    pub(crate) client: Option<Entity<RelayClient>>,
    pub(crate) hosts_trust: Option<TrustStore>,
    online: HashSet<String>,
    pub(crate) routes: HashMap<u32, Route>,
    pub(crate) retired: RetiredSessions,
    pub(crate) active_pairing: Option<WeakEntity<crate::PairingAttempt>>,
    pub(crate) pending_open: Option<PendingOpen>,
    pub(crate) connect_waiters: Vec<oneshot::Sender<()>>,
    /// One `open` is outstanding at a time, because the relay's answer names
    /// the host and nothing else that would tell two of them apart.
    pub(crate) open_lock: Arc<smol::lock::Mutex<()>>,
    pub(crate) startup: Shared<Task<Result<(), Arc<str>>>>,
    failed: Option<String>,
    pub(crate) stopped: Option<StopReason>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RelayInitiatorEvent> for RelayInitiator {}

pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

impl RelayInitiator {
    /// The initiator some window already holds open, if any. It is held weakly
    /// here, so it lives exactly as long as something in the UI or a project
    /// on another Zode holds it. One that stopped or failed to start is not
    /// returned: it would never work again, and a new one can.
    pub fn global(cx: &App) -> Option<Entity<Self>> {
        let initiator = cx.try_global::<GlobalRelayInitiator>()?.0.upgrade()?;
        let usable = {
            let state = initiator.read(cx);
            state.stopped.is_none() && state.failed.is_none()
        };
        usable.then_some(initiator)
    }

    pub fn register_global(initiator: &Entity<Self>, cx: &mut App) {
        cx.set_global(GlobalRelayInitiator(initiator.downgrade()));
    }

    /// The running initiator, or a new one for the signed-in account. Creating
    /// it is what opens the connection to the relay.
    pub fn get_or_create(cx: &mut App) -> Result<Entity<Self>> {
        if let Some(existing) = Self::global(cx) {
            return Ok(existing);
        }
        let account = Account::global(cx).ok_or_else(|| anyhow!("accounts are not set up"))?;
        if account.read(cx).status().user().is_none() {
            return Err(anyhow!(
                "Sign in to your Zode account to reach your other Zodes"
            ));
        }
        let environment = RelayEnvironment::from_account(&account, cx);
        let app_version = release_channel::AppVersion::global(cx).to_string();
        let initiator =
            cx.new(|cx| Self::new(environment, Arc::new(WebSocketTransport), app_version, cx));
        Self::register_global(&initiator, cx);
        Ok(initiator)
    }

    pub fn new(
        environment: RelayEnvironment,
        transport: Arc<dyn RelayTransport>,
        app_version: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let environment = Arc::new(environment);
        let startup = cx
            .spawn({
                let environment = environment.clone();
                async move |this, cx| Self::start(this, environment, cx).await
            })
            .shared();
        Self {
            environment,
            app_version,
            transport,
            identity: None,
            client: None,
            hosts_trust: None,
            online: HashSet::new(),
            routes: HashMap::new(),
            retired: RetiredSessions::default(),
            active_pairing: None,
            pending_open: None,
            connect_waiters: Vec::new(),
            open_lock: Arc::new(smol::lock::Mutex::new(())),
            startup,
            failed: None,
            stopped: None,
            _subscriptions: Vec::new(),
        }
    }

    async fn start(
        this: WeakEntity<Self>,
        environment: Arc<RelayEnvironment>,
        cx: &mut AsyncApp,
    ) -> Result<(), Arc<str>> {
        let outcome = Self::load_identity(&environment, cx).await;
        let (identity, trust) = match outcome {
            Ok(loaded) => loaded,
            Err(error) => {
                let message: Arc<str> = format!("{error:#}").into();
                this.update(cx, |this, cx| {
                    this.failed = Some(message.to_string());
                    cx.emit(RelayInitiatorEvent::Changed);
                    cx.notify();
                })
                .ok();
                return Err(message);
            }
        };
        let credentials = Arc::new(RegisteringCredentials::new(
            &environment,
            *identity.keypair.public_key(),
        ));
        this.update(cx, |this, cx| {
            let client = cx.new({
                let transport = this.transport.clone();
                let api_url = environment.api_url.clone();
                move |cx| {
                    RelayClient::new_as(credentials, transport, &api_url, RelayRole::Client, cx)
                }
            });
            this._subscriptions.push(cx.subscribe(
                &client,
                |this, _client, event: &RelayEvent, cx| {
                    this.on_relay_event(event.clone(), cx);
                },
            ));
            this.client = Some(client);
            this.identity = Some(identity);
            this.hosts_trust = Some(trust);
            cx.emit(RelayInitiatorEvent::Changed);
            cx.notify();
        })
        .map_err(|_| Arc::<str>::from("the relay connection was closed"))
    }

    async fn load_identity(
        environment: &RelayEnvironment,
        cx: &mut AsyncApp,
    ) -> Result<(Identity, TrustStore)> {
        let credential = environment
            .credentials
            .credential(cx)
            .await
            .map_err(|error| anyhow!("{error}"))?;
        let keypair = load_or_create_keypair(&environment.keychain, &credential.user_id, cx)
            .await
            .context("this device's key could not be read")?;
        let trust = cx
            .update(|cx| TrustStore::load_for(credential.user_id.clone(), TrustRole::Hosts, cx))
            .await;
        Ok((
            Identity {
                keypair,
                user_id: credential.user_id,
                device_id: credential.device_id,
            },
            trust,
        ))
    }

    pub fn state(&self, cx: &App) -> InitiatorState {
        if let Some(failure) = &self.failed {
            return InitiatorState::Failed(failure.clone());
        }
        match &self.client {
            None => InitiatorState::Starting,
            Some(client) => InitiatorState::Relay(client.read(cx).state()),
        }
    }

    /// Hosts that have been paired, by device id.
    pub fn is_paired(&self, device_id: &str) -> bool {
        self.hosts_trust
            .as_ref()
            .is_some_and(|trust| trust.get(device_id).is_some())
    }

    pub fn pinned_hosts(&self) -> Vec<PinnedDevice> {
        self.hosts_trust
            .as_ref()
            .map(|trust| trust.devices().to_vec())
            .unwrap_or_default()
    }

    pub fn forget_host(&mut self, device_id: &str, cx: &mut Context<Self>) {
        if let Some(trust) = self.hosts_trust.as_mut()
            && trust.forget(device_id, cx)
        {
            cx.emit(RelayInitiatorEvent::Changed);
            cx.notify();
        }
    }

    /// Resolves once any pin written just before has reached the database.
    pub fn flush_pins(&mut self) -> Task<()> {
        self.hosts_trust
            .as_mut()
            .map_or_else(|| Task::ready(()), TrustStore::flush)
    }

    /// The account's Zodes this one may reach, with who is online and who is
    /// paired.
    pub fn hosts(&self, cx: &mut Context<Self>) -> Task<Result<Vec<RelayHost>>> {
        let startup = self.startup.clone();
        let environment = self.environment.clone();
        cx.spawn(async move |this, cx| {
            startup.await.map_err(|error| anyhow!("{error}"))?;
            let credential = environment
                .credentials
                .credential(cx)
                .await
                .map_err(|error| anyhow!("{error}"))?;
            let same_account = this
                .update(cx, |this, cx| this.check_account(&credential, cx))
                .map_err(|_| anyhow!("the relay connection was closed"))?;
            if !same_account {
                return Err(anyhow!("the signed-in account changed; reopen this list"));
            }
            let devices = list_devices(
                &environment.http_client,
                &environment.api_url,
                &credential.bearer,
            )
            .await
            .map_err(|error| anyhow!("{error}"))?;
            this.read_with(cx, |this, _| {
                devices
                    .into_iter()
                    .filter(|device| {
                        device.kind == "ide" && device.device_id != credential.device_id
                    })
                    .map(|device| {
                        let pinned = this
                            .hosts_trust
                            .as_ref()
                            .and_then(|trust| trust.get(&device.device_id));
                        RelayHost {
                            online: this.online.contains(&device.device_id),
                            paired: pinned.is_some(),
                            key_changed: pinned.is_some_and(|pinned| {
                                device
                                    .public_key
                                    .is_some_and(|key| key != pinned.public_key)
                            }),
                            name: device.name,
                            public_key: device.public_key,
                            device_id: device.device_id,
                        }
                    })
                    .collect()
            })
            .map_err(|_| anyhow!("the relay connection was closed"))
        })
    }

    /// Whether `credential` belongs to the account and device this initiator
    /// started as. Its keys, pins and handshake identity are all those of the
    /// first account, so after a switch it stops rather than mix the two.
    pub(crate) fn check_account(
        &mut self,
        credential: &crate::RelayCredential,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(identity) = &self.identity else {
            return true;
        };
        if identity.user_id == credential.user_id && identity.device_id == credential.device_id {
            return true;
        }
        log::warn!("the signed-in account changed; stopping the relay connection");
        self.stopped = Some(StopReason::SessionEnded);
        self.client = None;
        self._subscriptions.clear();
        self.online.clear();
        self.end_all_routes("the signed-in account changed", cx);
        for waiter in self.connect_waiters.drain(..) {
            drop(waiter);
        }
        cx.emit(RelayInitiatorEvent::Changed);
        cx.notify();
        false
    }

    fn forget_retired_routes(&mut self) {
        let retired = std::mem::take(&mut *self.retired.lock());
        for session_id in retired {
            self.routes.remove(&session_id);
        }
    }

    fn on_relay_event(&mut self, event: RelayEvent, cx: &mut Context<Self>) {
        self.forget_retired_routes();
        match event {
            RelayEvent::Connected => {
                for waiter in self.connect_waiters.drain(..) {
                    if waiter.send(()).is_err() {
                        log::debug!("nobody was waiting for the relay to connect");
                    }
                }
            }
            RelayEvent::Disconnected => {
                self.online.clear();
                self.end_all_routes("the connection to the relay dropped", cx);
            }
            RelayEvent::Presence { hosts } => self.online = hosts.into_iter().collect(),
            RelayEvent::SessionOpened {
                session_id,
                peer_device_id,
                mode,
            } => self.on_opened(session_id, peer_device_id, mode, cx),
            RelayEvent::SessionClosed { session_id, reason } => {
                self.end_route(session_id, reason, false, cx);
            }
            RelayEvent::Pairing {
                session_id,
                message,
            } => self.deliver(session_id, SidEvent::Pairing(message), cx),
            RelayEvent::Frame {
                session_id,
                payload,
            } => self.deliver(session_id, SidEvent::Frame(payload), cx),
            // Told to hosts only.
            RelayEvent::DeviceRevoked { .. } => {}
            RelayEvent::Error { code } => self.fail_pending_open(match code.as_str() {
                "not_found" => ConnectError::HostOffline,
                "rate_limited" => ConnectError::RateLimited,
                "unauthorized" => ConnectError::Unauthorized,
                other => ConnectError::Relay(other.to_string()),
            }),
            RelayEvent::Stopped(reason) => self.on_stopped(reason, cx),
        }
        cx.emit(RelayInitiatorEvent::Changed);
        cx.notify();
    }

    fn on_stopped(&mut self, reason: StopReason, cx: &mut Context<Self>) {
        self.stopped = Some(reason);
        self.online.clear();
        self.end_all_routes("the relay connection stopped", cx);
        self.fail_pending_open(ConnectError::Relay(format!("{reason:?}")));
        for waiter in self.connect_waiters.drain(..) {
            drop(waiter);
        }
        // Only an explicit revocation of this device forgets anything: an
        // ended sign-in is fixed by signing in again.
        if reason != StopReason::Revoked {
            return;
        }
        let Some(identity) = &self.identity else {
            return;
        };
        let user_id = identity.user_id.clone();
        if let Some(trust) = self.hosts_trust.as_mut() {
            trust.forget_all(cx);
        }
        let keychain = self.environment.keychain.clone();
        cx.spawn(async move |_this, cx| {
            if let Err(error) = delete_keypair(&keychain, &user_id, cx).await {
                log::error!("{error:#}");
            }
        })
        .detach();
    }

    pub(crate) fn fail_pending_open(&mut self, error: ConnectError) {
        if let Some(pending) = self.pending_open.take()
            && pending.respond.send(Err(error)).is_err()
        {
            log::debug!("nobody was waiting for the open");
        }
    }

    fn on_opened(&mut self, session_id: u32, peer_device_id: String, mode: OpenMode, cx: &App) {
        let wanted = self.pending_open.as_ref().is_some_and(|pending| {
            pending.host_device_id == peer_device_id && pending.mode == mode
        });
        if !wanted {
            log::debug!("closing a session nobody asked for");
            self.close_at_relay(session_id, cx);
            return;
        }
        let Some(pending) = self.pending_open.take() else {
            return;
        };
        let (sender, receiver) = smol::channel::bounded(AWAITING_QUEUE);
        self.routes.insert(session_id, Route::Awaiting(sender));
        if pending.respond.send(Ok((session_id, receiver))).is_err() {
            self.routes.remove(&session_id);
            self.close_at_relay(session_id, cx);
        }
    }

    pub(crate) fn writer(&self, cx: &App) -> Option<RelayWriter> {
        self.client.as_ref()?.read(cx).writer()
    }

    fn deliver(&mut self, session_id: u32, event: SidEvent, cx: &mut Context<Self>) {
        match self.routes.get(&session_id) {
            Some(Route::Awaiting(sender)) => match sender.try_send(event) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) | Err(TrySendError::Closed(_)) => {
                    self.routes.remove(&session_id);
                    self.close_at_relay(session_id, cx);
                }
            },
            Some(Route::Session(session)) => {
                let SidEvent::Frame(payload) = event else {
                    return;
                };
                let Some(session) = session.upgrade() else {
                    self.routes.remove(&session_id);
                    self.close_at_relay(session_id, cx);
                    return;
                };
                session.update(cx, |session, cx| session.on_frame(&payload, cx));
            }
            None => log::debug!("a frame for session {session_id}, which is not held"),
        }
    }

    pub(crate) fn close_at_relay(&self, session_id: u32, cx: &App) {
        if let Some(writer) = self.writer(cx)
            && let Err(error) = writer.close_session(session_id)
        {
            log::debug!("could not tell the relay to close session {session_id}: {error}");
        }
    }

    fn end_route(&mut self, session_id: u32, reason: String, lost: bool, cx: &mut Context<Self>) {
        match self.routes.remove(&session_id) {
            Some(Route::Awaiting(sender)) => {
                let event = if lost {
                    SidEvent::LinkLost(reason)
                } else {
                    SidEvent::Closed(reason)
                };
                if sender.try_send(event).is_err() {
                    log::debug!("the session ended while nobody was reading it");
                }
            }
            Some(Route::Session(session)) => {
                if let Some(session) = session.upgrade() {
                    session.update(cx, |session, cx| session.finish(reason, cx));
                }
            }
            None => {}
        }
    }

    fn end_all_routes(&mut self, reason: &str, cx: &mut Context<Self>) {
        let ids: Vec<u32> = self.routes.keys().copied().collect();
        for session_id in ids {
            self.end_route(session_id, reason.to_string(), true, cx);
        }
        self.fail_pending_open(ConnectError::Relay(reason.to_string()));
    }
}
