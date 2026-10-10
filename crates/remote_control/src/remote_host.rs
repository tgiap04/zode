//! The host: switches remote control on and off, and answers for it.
//!
//! One [`RemoteHost`] exists per process. While remote control is off, or
//! nobody is signed in, it holds nothing: no relay client, so no socket, and
//! mirrors that are asleep, so no tap on any terminal. Turning it on builds a
//! [`Hosting`] and turning it off drops it, which is the whole of switching
//! off -- there is no second place to forget to clean up.

use std::sync::Arc;

use gpui::{
    App, AppContext as _, Context, DisplayWakeLock, Entity, EventEmitter, Global, Subscription,
    Task,
};
use keep_awake::KeepDisplayAwakeSetting;
use remote_relay_client::{
    ConnectionState, PinnedDevice, RelayClient, RelayEvent, RelayTransport, StopReason, TrustStore,
    delete_keypair, load_or_create_keypair,
};
use remote_relay_protocol::Control;
use settings::{Settings as _, SettingsStore};
use zode_account::{Account, AccountStatusChanged};

use crate::{
    agent_mirror::{AgentMirror, AgentMirrorEvent},
    file_browse::{BrowseError, FileBrowser, FileReply},
    host_environment::{HostEnvironment, RegisteringCredentials},
    hosting::{HOUSEKEEPING_INTERVAL, Hosting, HostingParts},
    pairing_flow::PendingDecision,
    remote_control_settings::RemoteControlSettings,
    terminal_mirror::{TerminalMirror, TerminalMirrorEvent},
};

/// A device that is in control of this Zode right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub session_id: u32,
    pub device_id: String,
    pub device_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostState {
    /// Remote control is switched off.
    Disabled,
    /// It is on, but nobody is signed in.
    SignedOut,
    Starting,
    Online,
    /// Connected before; trying again.
    Reconnecting,
    /// Stopped and will not try again until switched off and on, or the
    /// account changes.
    Halted(StopReason),
    /// Could not start: the device key could not be read or saved.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteHostEvent {
    /// Something the indicator or a modal shows has changed.
    Changed,
    /// A device has just taken control. Raised once per session.
    SessionStarted { device_name: String },
    /// Remote control stopped or could not do something, and the person at
    /// this screen should hear about it. `retry` says trying again may help.
    Problem { message: String, retry: bool },
}

/// What to tell the person when the host ends up in `state`, if it is a state
/// they did not ask for.
fn problem_message(state: &HostState) -> Option<String> {
    let message = match state {
        HostState::SignedOut => {
            "Remote control is on, but no one is signed in to a Zode account. Sign in to use it."
        }
        HostState::Halted(StopReason::Revoked) => {
            "This device was removed from your Zode account, so remote control stopped and the \
             devices it trusted were forgotten."
        }
        HostState::Halted(StopReason::SessionEnded) => {
            "Your Zode sign-in ended, so remote control stopped. Sign in again to use it."
        }
        HostState::Halted(StopReason::QuotaExceeded) => {
            "Remote control stopped: your account has used up its relay allowance."
        }
        HostState::Halted(StopReason::Replaced) => {
            "Remote control stopped: another Zode is connected to the relay as this device."
        }
        HostState::Halted(StopReason::IncompatibleRelay) => {
            "Remote control stopped: the relay does not speak a version this Zode understands. \
             Update Zode."
        }
        HostState::Disabled
        | HostState::Starting
        | HostState::Online
        | HostState::Reconnecting
        | HostState::Failed(_) => return None,
    };
    Some(message.to_string())
}

struct GlobalRemoteHost(Entity<RemoteHost>);
impl Global for GlobalRemoteHost {}

pub struct RemoteHost {
    account: Entity<Account>,
    environment: Arc<HostEnvironment>,
    transport: Arc<dyn RelayTransport>,
    agents: Entity<AgentMirror>,
    terminals: Entity<TerminalMirror>,
    /// Answers file requests; without it the host does not offer `files`.
    files: Option<Entity<FileBrowser>>,
    app_version: String,
    running: Option<Hosting>,
    state: HostState,
    /// The account this host is running for, or has halted for. Starting is
    /// not attempted again for the same account until the setting is switched
    /// off, so a device that was revoked does not keep knocking.
    serving: Option<String>,
    startup: Option<Task<()>>,
    /// Removing a revoked device's key. A new start waits for it, so a quick
    /// off and on cannot read a key that is about to be deleted.
    key_deletion: Option<Task<()>>,
    /// Whether the setting was on at the last look, to tell "just switched on
    /// while nobody is signed in" from "starting up before sign-in finished".
    was_enabled: bool,
    /// Turned off by hand while the setting still reads on, which is what a
    /// failed settings write or a setting overridden elsewhere looks like. The
    /// host stays down until the setting is seen off.
    switched_off_by_hand: bool,
    display_lock: Option<DisplayWakeLock>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RemoteHostEvent> for RemoteHost {}

impl RemoteHost {
    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalRemoteHost>()
            .map(|global| global.0.clone())
    }

    pub fn set_global(host: Entity<Self>, cx: &mut App) {
        cx.set_global(GlobalRemoteHost(host));
    }

    pub fn new(
        account: Entity<Account>,
        environment: HostEnvironment,
        transport: Arc<dyn RelayTransport>,
        agents: Entity<AgentMirror>,
        terminals: Entity<TerminalMirror>,
        files: Option<Entity<FileBrowser>>,
        app_version: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            cx.observe_global::<SettingsStore>(|this, cx| {
                this.sync(cx);
                // The user's `keep_display_awake` switch is read here too.
                this.update_display_lock(cx);
            }),
            cx.subscribe(&account, |this, _account, _: &AccountStatusChanged, cx| {
                this.sync(cx);
            }),
        ];
        let mut host = Self {
            account,
            environment: Arc::new(environment),
            transport,
            agents,
            terminals,
            files,
            app_version,
            running: None,
            state: HostState::Disabled,
            serving: None,
            startup: None,
            key_deletion: None,
            was_enabled: RemoteControlSettings::is_enabled(cx),
            switched_off_by_hand: false,
            display_lock: None,
            _subscriptions: subscriptions,
        };
        host.sync(cx);
        host
    }

    pub fn state(&self) -> &HostState {
        &self.state
    }

    /// The devices in control right now: ones that have proven they hold the
    /// session keys, and no others.
    pub fn sessions(&self) -> Vec<SessionInfo> {
        let Some(running) = &self.running else {
            return Vec::new();
        };
        let mut sessions: Vec<SessionInfo> = running
            .confirmed_sessions()
            .map(|(session_id, session)| SessionInfo {
                session_id,
                device_id: session.peer_device_id.clone(),
                device_name: session.peer_name.clone(),
            })
            .collect();
        sessions.sort_by_key(|session| session.session_id);
        sessions
    }

    pub fn trusted_devices(&self) -> Vec<PinnedDevice> {
        self.running
            .as_ref()
            .map(|running| running.trust.devices().to_vec())
            .unwrap_or_default()
    }

    pub fn pending_pairing(&self) -> Option<&PendingDecision> {
        self.running.as_ref()?.pairing.pending_decision()
    }

    pub fn pairing_locked_out(&self) -> bool {
        self.running
            .as_ref()
            .is_some_and(|running| running.pairing.is_locked_out())
    }

    /// Whether the question for `session_id` has been put on screen, marking it
    /// so that a second window does not put it there again.
    pub fn claim_pairing_presentation(&mut self, session_id: u32) -> bool {
        let Some(running) = self.running.as_mut() else {
            return false;
        };
        if running.pairing_presented == Some(session_id) {
            return false;
        }
        running.pairing_presented = Some(session_id);
        true
    }

    /// Answers the question for `session_id`; an answer for any other session
    /// is ignored.
    pub fn decide_pairing(&mut self, session_id: u32, trust: bool, cx: &mut Context<Self>) {
        if let Some(running) = self.running.as_mut() {
            running.decide_pairing(session_id, trust, cx);
        }
        self.refresh(cx);
    }

    pub(crate) fn present_pairing(
        &mut self,
        session_id: u32,
        peer_name: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(running) = self.running.as_mut() {
            running.present_pairing(session_id, peer_name, cx);
        }
        self.refresh(cx);
    }

    pub(crate) fn flush_all(&mut self, cx: &mut Context<Self>) {
        if let Some(running) = self.running.as_mut() {
            running.flush_all(cx);
        }
    }

    /// The question on screen was dismissed without an answer. A question that
    /// is not answered is answered no.
    pub fn reject_pairing_if_pending(&mut self, session_id: u32, cx: &mut Context<Self>) {
        if self
            .pending_pairing()
            .is_some_and(|pending| pending.session_id == session_id)
        {
            self.decide_pairing(session_id, false, cx);
        }
    }

    /// The kill switch.
    pub fn disconnect_all(&mut self, cx: &mut Context<Self>) {
        if let Some(running) = self.running.as_mut() {
            running.end_all_sessions(cx);
        }
        self.refresh(cx);
    }

    pub fn forget_device(&mut self, device_id: &str, cx: &mut Context<Self>) {
        if let Some(running) = self.running.as_mut() {
            running.end_sessions_of(device_id, cx);
            running.trust.forget(device_id, cx);
        }
        self.refresh(cx);
    }

    /// Takes everything down now, without waiting for the setting to change.
    /// Writing the setting is asynchronous and can fail, and a device that is
    /// in control must not stay in control until it succeeds.
    pub fn turn_off(&mut self, cx: &mut Context<Self>) {
        self.switched_off_by_hand = true;
        self.stop(HostState::Disabled, cx);
    }

    /// Tells the person about something that went wrong outside the host.
    pub fn report_problem(&mut self, message: String, cx: &mut Context<Self>) {
        log::error!("{message}");
        cx.emit(RemoteHostEvent::Problem {
            message,
            retry: false,
        });
    }

    /// Starts again after a failure to start. Nothing else is retried on its
    /// own: reading the keychain can raise a prompt, so it is the person's call.
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        if matches!(self.state, HostState::Failed(_)) {
            self.serving = None;
            self.sync(cx);
        }
    }

    /// The only way out of a pairing lockout.
    pub fn unlock_pairing(&mut self, cx: &mut Context<Self>) {
        if let Some(running) = self.running.as_mut()
            && let Some(session_id) = running.pairing.unlock()
        {
            running.end_session(session_id, cx);
        }
        self.refresh(cx);
    }

    /// Brings the host in line with the setting and the account. Safe to call
    /// at any time and as often as asked.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let user_id = self
            .account
            .read(cx)
            .status()
            .user()
            .map(|user| user.id.to_string());

        let enabled = RemoteControlSettings::is_enabled(cx);
        let just_enabled = enabled && !self.was_enabled;
        self.was_enabled = enabled;
        if !enabled {
            self.switched_off_by_hand = false;
            self.stop(HostState::Disabled, cx);
            return;
        }
        if self.switched_off_by_hand {
            return;
        }
        let Some(user_id) = user_id else {
            let was_serving = self.serving.is_some();
            let changed = self.stop(HostState::SignedOut, cx);
            // Said when the person asked for remote control, or lost their
            // sign-in while it was running; not while startup is still waiting
            // for the account to come back from the keychain.
            if changed
                && (just_enabled || was_serving)
                && let Some(message) = problem_message(&self.state)
            {
                cx.emit(RemoteHostEvent::Problem {
                    message,
                    retry: false,
                });
            }
            return;
        };
        if self.serving.as_deref() == Some(user_id.as_str()) {
            return;
        }
        // Another account signed in: nothing of the last one carries over.
        self.stop(HostState::Starting, cx);
        self.serving = Some(user_id.clone());
        self.start(user_id, cx);
    }

    fn start(&mut self, user_id: String, cx: &mut Context<Self>) {
        self.state = HostState::Starting;
        let keychain = self.environment.keychain.clone();
        let pending_key_deletion = self.key_deletion.take();
        self.startup = Some(cx.spawn(async move |this, cx| {
            if let Some(deletion) = pending_key_deletion {
                deletion.await;
            }
            let keypair = match load_or_create_keypair(&keychain, &user_id, cx).await {
                Ok(keypair) => keypair,
                Err(error) => {
                    log::error!("remote control cannot start: {error:#}");
                    this.update(cx, |this, cx| {
                        this.startup = None;
                        this.state = HostState::Failed(format!("{error:#}"));
                        cx.emit(RemoteHostEvent::Problem {
                            message: format!("Remote control could not start: {error:#}"),
                            retry: true,
                        });
                        cx.emit(RemoteHostEvent::Changed);
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            let trust = cx.update(|cx| TrustStore::load(user_id.clone(), cx)).await;
            this.update(cx, |this, cx| {
                this.startup = None;
                this.begin(user_id, keypair, trust, cx);
            })
            .ok();
        }));
    }

    fn begin(
        &mut self,
        user_id: String,
        keypair: Arc<remote_relay_protocol::DeviceKeypair>,
        trust: TrustStore,
        cx: &mut Context<Self>,
    ) {
        // The setting or the account may have changed while the key loaded.
        if self.serving.as_deref() != Some(user_id.as_str()) {
            return;
        }

        let credentials = Arc::new(RegisteringCredentials::new(
            &self.environment,
            *keypair.public_key(),
        ));
        let transport = self.transport.clone();
        let api_url = self.environment.api_url.clone();
        let client = cx.new({
            let credentials = credentials.clone();
            move |cx| RelayClient::new(credentials, transport, &api_url, cx)
        });

        self.agents.update(cx, |agents, cx| agents.activate(cx));
        self.terminals
            .update(cx, |terminals, cx| terminals.activate(cx));

        let subscriptions = vec![
            cx.subscribe(&client, |this, _client, event: &RelayEvent, cx| {
                this.on_relay_event(event.clone(), cx);
            }),
            cx.subscribe(
                &self.terminals,
                |this, _terminals, event: &TerminalMirrorEvent, cx| {
                    this.on_terminal_event(event, cx);
                },
            ),
            cx.subscribe(
                &self.agents,
                |this, _agents, event: &AgentMirrorEvent, cx| {
                    this.on_agent_event(event, cx);
                },
            ),
        ];
        let housekeeping = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(HOUSEKEEPING_INTERVAL).await;
                let alive = this.update(cx, |this, cx| {
                    let Some(running) = this.running.as_mut() else {
                        return;
                    };
                    let before = running.sessions.len();
                    running.housekeeping(cx);
                    if running.sessions.len() != before {
                        this.refresh(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });

        self.running = Some(Hosting::new(HostingParts {
            user_id,
            keypair,
            trust,
            client,
            credentials,
            http_client: self.environment.http_client.clone(),
            api_url: self.environment.api_url.clone(),
            agents: self.agents.clone(),
            terminals: self.terminals.clone(),
            files: self.files.clone(),
            app_version: self.app_version.clone(),
            housekeeping,
            subscriptions,
        }));
        self.state = HostState::Starting;
        self.refresh(cx);
    }

    /// Puts everything back as if remote control had never been on.
    /// `false` if there was nothing to put back.
    fn stop(&mut self, state: HostState, cx: &mut Context<Self>) -> bool {
        let was_active = self.running.take().is_some()
            | self.startup.take().is_some()
            | self.serving.take().is_some();
        if !was_active && self.state == state {
            return false;
        }
        // Dropping the hosting dropped the relay client, which closed the
        // socket; the mirrors are put to sleep and untap every terminal.
        self.agents.update(cx, |agents, _| agents.deactivate());
        self.terminals
            .update(cx, |terminals, cx| terminals.deactivate(cx));
        self.display_lock = None;
        self.state = state;
        cx.emit(RemoteHostEvent::Changed);
        cx.notify();
        true
    }

    fn on_relay_event(&mut self, event: RelayEvent, cx: &mut Context<Self>) {
        match event {
            RelayEvent::Stopped(reason) => {
                self.on_relay_stopped(reason, cx);
                return;
            }
            RelayEvent::Connected => {}
            RelayEvent::Disconnected => {
                if let Some(running) = self.running.as_mut() {
                    running.forget_all_sessions(cx);
                }
            }
            other => {
                if let Some(running) = self.running.as_mut() {
                    match other {
                        RelayEvent::SessionOpened {
                            session_id,
                            peer_device_id,
                            mode,
                        } => running.on_session_opened(session_id, peer_device_id, mode, cx),
                        RelayEvent::SessionClosed { session_id, .. } => {
                            running.on_session_closed(session_id, cx);
                        }
                        RelayEvent::Pairing {
                            session_id,
                            message,
                        } => running.on_pairing(session_id, message, cx),
                        RelayEvent::Frame {
                            session_id,
                            payload,
                        } => running.on_frame(session_id, payload, cx),
                        RelayEvent::DeviceRevoked { device_id } => {
                            running.end_sessions_of(&device_id, cx);
                            running.trust.forget(&device_id, cx);
                        }
                        // A host is never sent presence, and a relay error names no
                        // session it could end; both are logged by the client.
                        RelayEvent::Connected
                        | RelayEvent::Disconnected
                        | RelayEvent::Presence { .. }
                        | RelayEvent::Error { .. }
                        | RelayEvent::Stopped(_) => {}
                    }
                }
            }
        }
        self.refresh(cx);
    }

    #[cfg(test)]
    pub(crate) fn file_requests_in_flight(&self, session_id: u32) -> usize {
        self.running
            .as_ref()
            .map_or(0, |running| running.file_requests_in_flight(session_id))
    }

    #[cfg(test)]
    pub(crate) fn queue_waiting_bytes(&mut self, session_id: u32, total: usize) {
        if let Some(running) = self.running.as_mut() {
            running.queue_waiting_bytes(session_id, total);
        }
    }

    #[cfg(test)]
    pub(crate) fn set_next_host_stream(&mut self, session_id: u32, stream_id: u32) {
        if let Some(running) = self.running.as_mut() {
            running.set_next_host_stream(session_id, stream_id);
        }
    }

    pub(crate) fn finish_file_request(
        &mut self,
        session_id: u32,
        ticket: u64,
        request_id: u32,
        reply: Result<FileReply, BrowseError>,
        cx: &mut Context<Self>,
    ) {
        if let Some(running) = self.running.as_mut() {
            running.finish_file_request(session_id, ticket, request_id, reply, cx);
        }
    }

    fn on_relay_stopped(&mut self, reason: StopReason, cx: &mut Context<Self>) {
        let wipe = reason == StopReason::Revoked;
        let identity = self
            .running
            .as_ref()
            .map(|running| (running.user_id.clone(), running.client.clone()));
        if let Some(running) = self.running.as_mut() {
            running.end_all_sessions(cx);
            if wipe {
                // This device is no longer the account's. Nothing it trusted
                // was trusted by anyone but the account, and its key is of no
                // further use.
                running.trust.forget_all(cx);
            }
        }
        if wipe && let Some((user_id, _client)) = identity {
            let keychain = self.environment.keychain.clone();
            self.key_deletion = Some(cx.spawn(async move |_this, cx| {
                if let Err(error) = delete_keypair(&keychain, &user_id, cx).await {
                    log::error!("{error:#}");
                }
            }));
        }
        let serving = self.serving.clone();
        self.stop(HostState::Halted(reason), cx);
        if let Some(message) = problem_message(&self.state) {
            cx.emit(RemoteHostEvent::Problem {
                message,
                retry: false,
            });
        }
        // Remembered, so the same account is not started again until remote
        // control is switched off or the account changes.
        self.serving = serving;
    }

    fn on_terminal_event(&mut self, event: &TerminalMirrorEvent, cx: &mut Context<Self>) {
        let Some(running) = self.running.as_mut() else {
            return;
        };
        match event {
            TerminalMirrorEvent::ListChanged => {
                let terminals = running.terminals.read(cx).summaries(cx);
                running.broadcast(&Control::TerminalList { terminals }, cx);
            }
            TerminalMirrorEvent::OutputReady { session_id } => running.flush(*session_id, cx),
            TerminalMirrorEvent::Resized {
                terminal_id,
                columns,
                rows,
                sessions,
            } => {
                let control = Control::TerminalResized {
                    terminal_id: terminal_id.clone(),
                    columns: *columns,
                    rows: *rows,
                };
                running.send_to(sessions, &control, cx);
            }
            TerminalMirrorEvent::Closed {
                terminal_id,
                sessions,
            } => {
                let control = Control::TerminalClosed {
                    terminal_id: terminal_id.clone(),
                    exit_code: None,
                };
                running.send_to(sessions, &control, cx);
                running.forget_streams_of(terminal_id, sessions);
            }
        }
        self.refresh(cx);
    }

    fn on_agent_event(&mut self, event: &AgentMirrorEvent, cx: &mut Context<Self>) {
        let Some(running) = self.running.as_mut() else {
            return;
        };
        match event {
            AgentMirrorEvent::Changed(agent) => {
                running.broadcast(
                    &Control::AgentUpdate {
                        agent: agent.clone(),
                    },
                    cx,
                );
            }
            AgentMirrorEvent::Removed { .. } => {
                let agents = running.agents.read(cx).summaries(cx);
                running.broadcast(&Control::AgentList { agents }, cx);
            }
        }
        self.refresh(cx);
    }

    /// Recomputes what depends on the sessions and tells whoever is watching.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Some(running) = &self.running {
            self.state = match running.client.read(cx).state() {
                ConnectionState::Connected => HostState::Online,
                ConnectionState::Connecting | ConnectionState::Waiting => {
                    if matches!(self.state, HostState::Online | HostState::Reconnecting) {
                        HostState::Reconnecting
                    } else {
                        HostState::Starting
                    }
                }
                ConnectionState::Stopped(reason) => HostState::Halted(reason),
            };
        }
        self.update_display_lock(cx);
        cx.emit(RemoteHostEvent::Changed);
        cx.notify();
    }

    /// Holds the display awake for as long as a device is in control, on the
    /// same terms as an agent doing work: the user's own `keep_display_awake`
    /// switch decides, and a platform that cannot hold it is left alone.
    fn update_display_lock(&mut self, cx: &mut Context<Self>) {
        let wanted = !self.sessions().is_empty()
            && KeepDisplayAwakeSetting::try_get(cx).is_none_or(|setting| setting.0)
            && cx.can_keep_display_awake();
        match (wanted, self.display_lock.is_some()) {
            (true, false) => {
                self.display_lock = cx.keep_display_awake("Zode is being controlled remotely");
            }
            (false, true) => self.display_lock = None,
            _ => {}
        }
    }

    /// Whether the display is being held awake. For tests and the indicator.
    pub fn holds_display_awake(&self) -> bool {
        self.display_lock.is_some()
    }

    /// Whether a relay client exists at all. The check that switching off
    /// really did drop the connection.
    pub fn has_relay_client(&self) -> bool {
        self.running.is_some()
    }
}
