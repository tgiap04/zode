//! The host: switches remote control on and off, and answers for it.
//!
//! One [`RemoteHost`] exists per process. While remote control is off, or
//! nobody is signed in, it holds nothing: no relay client, so no socket, and
//! mirrors that are asleep, so no tap on any terminal. Turning it on builds a
//! [`Hosting`] and turning it off drops it, which is the whole of switching
//! off -- there is no second place to forget to clean up.

use std::sync::Arc;

use futures::{FutureExt as _, future::LocalBoxFuture};
use gpui::{
    App, AppContext as _, Context, DisplayWakeLock, Entity, EventEmitter, Global, Subscription,
    Task,
};
use keep_awake::KeepDisplayAwakeSetting;
use remote_relay_client::{
    ConnectionState, PinnedDevice, RelayClient, RelayEvent, RelayTransport, StopReason, TrustError,
    TrustStore, delete_keypair, load_or_create_keypair,
};
use remote_relay_protocol::Control;
use settings::{Settings as _, SettingsStore};
use zode_account::{Account, AccountStatusChanged};

use crate::{
    agent_mirror::{AgentMirror, AgentMirrorEvent},
    file_browse::{BrowseError, FileBrowser, FileReply},
    host_environment::{HostEnvironment, RegisteringCredentials},
    hosting::{HOUSEKEEPING_INTERVAL, Hosting, HostingParts, RequestSlot},
    pairing_flow::{PairingHistory, PendingDecision},
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
pub(crate) fn problem_message(state: &HostState) -> Option<String> {
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

/// What the list of trusted devices has to say right now.
pub(crate) enum TrustedDevices {
    /// Nobody is signed in, so there is no account whose devices to list.
    SignedOut,
    Loading,
    Listed {
        devices: Vec<PinnedDevice>,
        /// Some saved devices could not be read, so more may be trusted than
        /// are listed.
        unreadable: bool,
    },
}

/// The pins of the signed-in account while the host is not running. They are
/// the same pins the running host uses, so the devices can be listed and
/// forgotten whether or not remote control is on.
enum RestingTrust {
    Loading { user_id: String, _load: Task<()> },
    Loaded { user_id: String, store: TrustStore },
}

impl RestingTrust {
    fn user_id(&self) -> &str {
        match self {
            Self::Loading { user_id, .. } | Self::Loaded { user_id, .. } => user_id,
        }
    }
}

/// What stopping trust in a device came to.
struct Forgotten {
    /// What to call it in a message.
    what: String,
    /// `Err` if nothing was changed because the saved devices cannot be
    /// written to. Otherwise the write that saves the change, if there was one.
    result: Result<Option<LocalBoxFuture<'static, Result<(), TrustError>>>, TrustError>,
}

/// Stops trusting `device_id`. A forget that looks as if it worked and did not
/// is worse than one that says so, so both ways it can fail are reported by
/// [`RemoteHost::finish_forgetting`].
fn forget_pin(store: &mut TrustStore, device_id: &str, cx: &App) -> Forgotten {
    let what = store
        .get(device_id)
        .map_or_else(|| "the device".to_string(), |pinned| pinned.name.clone());
    let result = store
        .try_forget(device_id, cx)
        .map(|removed| removed.then(|| store.flush_checked().boxed_local()));
    Forgotten { what, result }
}

fn forget_all_pins(store: &mut TrustStore, cx: &App) -> Forgotten {
    let result = store
        .try_forget_all(cx)
        .map(|()| Some(store.flush_checked().boxed_local()));
    Forgotten {
        what: "the devices this Zode trusted".to_string(),
        result,
    }
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
    /// Whether the host offers its agent session history. Off until whoever
    /// builds the host says it should be.
    sessions_available: bool,
    app_version: String,
    running: Option<Hosting>,
    resting_trust: Option<RestingTrust>,
    /// The pairing lockout and hourly allowances of the account that was last
    /// hosted, kept while the host is stopped so turning remote control off and
    /// on does not clear them. Another account starts fresh.
    resting_pairing: Option<(String, PairingHistory)>,
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
    /// Who was signed in at the last look, so that a change is told to whatever
    /// shows the trusted devices of the account that is.
    seen_user: Option<String>,
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
            sessions_available: false,
            app_version,
            running: None,
            resting_trust: None,
            resting_pairing: None,
            state: HostState::Disabled,
            serving: None,
            startup: None,
            key_deletion: None,
            was_enabled: RemoteControlSettings::is_enabled(cx),
            seen_user: None,
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

    /// Offers, or stops offering, the agent session history to devices. Takes
    /// effect for a connection that is up as well as for the next one.
    pub(crate) fn set_sessions_available(&mut self, available: bool) {
        self.sessions_available = available;
        if let Some(running) = self.running.as_mut() {
            running.set_sessions_available(available);
        }
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

    /// The devices trusted to control this Zode, whether or not remote control
    /// is running: empty until they have been read.
    pub fn trusted_devices(&self) -> Vec<PinnedDevice> {
        self.trust_store()
            .map(|store| store.devices().to_vec())
            .unwrap_or_default()
    }

    fn trust_store(&self) -> Option<&TrustStore> {
        match (&self.running, &self.resting_trust) {
            (Some(running), _) => Some(&running.trust),
            (None, Some(RestingTrust::Loaded { store, .. })) => Some(store),
            _ => None,
        }
    }

    fn signed_in_user(&self, cx: &App) -> Option<String> {
        self.account
            .read(cx)
            .status()
            .user()
            .map(|user| user.id.to_string())
    }

    pub(crate) fn trusted_devices_listing(&self, cx: &App) -> TrustedDevices {
        if self.running.is_none() && self.startup.is_none() && self.signed_in_user(cx).is_none() {
            return TrustedDevices::SignedOut;
        }
        match self.trust_store() {
            Some(store) => TrustedDevices::Listed {
                devices: store.devices().to_vec(),
                unreadable: store.is_unreadable(),
            },
            None => TrustedDevices::Loading,
        }
    }

    /// Reads the saved devices of the signed-in account when the host is not
    /// running, so they can be listed and forgotten without turning remote
    /// control on. A running host, and one about to start, already holds them.
    pub(crate) fn load_trusted_devices(&mut self, cx: &mut Context<Self>) {
        if self.running.is_some() || self.startup.is_some() {
            return;
        }
        let Some(user_id) = self.signed_in_user(cx) else {
            return;
        };
        if self
            .resting_trust
            .as_ref()
            .is_some_and(|resting| resting.user_id() == user_id)
        {
            return;
        }
        let load = TrustStore::load(user_id.clone(), cx);
        let loading_for = user_id.clone();
        let task = cx.spawn(async move |this, cx| {
            let store = load.await;
            this.update(cx, |this, cx| {
                this.finish_loading_trust(loading_for, store, cx);
            })
            .ok();
        });
        self.resting_trust = Some(RestingTrust::Loading {
            user_id,
            _load: task,
        });
    }

    fn finish_loading_trust(&mut self, user_id: String, store: TrustStore, cx: &mut Context<Self>) {
        let still_wanted = matches!(
            &self.resting_trust,
            Some(RestingTrust::Loading { user_id: loading, .. }) if *loading == user_id
        );
        if still_wanted {
            self.resting_trust = Some(RestingTrust::Loaded { user_id, store });
            cx.notify();
        }
    }

    /// Keeps the pins of a host that is stopping, so what was forgotten or
    /// trusted a moment ago is not read back from storage before it has been
    /// written.
    fn keep_trust(&mut self, hosting: Hosting, cx: &App) {
        let Hosting {
            user_id,
            trust,
            pairing,
            ..
        } = hosting;
        self.resting_pairing = Some((user_id.clone(), pairing.into_history()));
        self.rest_pins(user_id, trust, cx);
    }

    /// Holds `store` as the pins of the host that is not running, if `user_id`
    /// is still the account that is signed in.
    fn rest_pins(&mut self, user_id: String, store: TrustStore, cx: &App) {
        if self.signed_in_user(cx).as_deref() == Some(user_id.as_str()) {
            self.resting_trust = Some(RestingTrust::Loaded { user_id, store });
        }
    }

    pub fn pending_pairing(&self) -> Option<&PendingDecision> {
        self.running.as_ref()?.pairing.pending_decision()
    }

    pub fn pairing_locked_out(&self) -> bool {
        self.running
            .as_ref()
            .is_some_and(|running| running.pairing.is_locked_out())
    }

    /// Whether the question for `session_id` is already on somebody's screen.
    pub fn pairing_is_presented(&self, session_id: u32) -> bool {
        self.running
            .as_ref()
            .is_some_and(|running| running.pairing_presented == Some(session_id))
    }

    /// Marks the question for `session_id` as put on screen, so that a second
    /// window does not put it there again. `false` if it already was.
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

    /// Whether the OS should be asked to call the person back to Zode about the
    /// question for `session_id`, marking it so that no other window asks too.
    /// Never once the question has been put on screen: that person has seen it.
    pub fn claim_pairing_notification(&mut self, session_id: u32) -> bool {
        let Some(running) = self.running.as_mut() else {
            return false;
        };
        if running.pairing_presented == Some(session_id)
            || running.pairing_notified == Some(session_id)
        {
            return false;
        }
        running.pairing_notified = Some(session_id);
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

    /// The question on screen was closed without an answer. The device is
    /// turned away, but the digits were not compared, so it does not count as a
    /// mismatch toward the lockout.
    pub fn dismiss_pairing_if_pending(&mut self, session_id: u32, cx: &mut Context<Self>) {
        if self
            .pending_pairing()
            .is_some_and(|pending| pending.session_id == session_id)
            && let Some(running) = self.running.as_mut()
        {
            running.dismiss_pairing(session_id, cx);
            self.refresh(cx);
        }
    }

    /// The kill switch.
    pub fn disconnect_all(&mut self, cx: &mut Context<Self>) {
        if let Some(running) = self.running.as_mut() {
            running.end_all_sessions(cx);
        }
        self.refresh(cx);
    }

    /// Removes a device from the trusted ones, whether or not remote control
    /// is running.
    pub fn forget_device(&mut self, device_id: &str, cx: &mut Context<Self>) {
        let forgotten = if let Some(running) = self.running.as_mut() {
            running.end_sessions_of(device_id, cx);
            Some(forget_pin(&mut running.trust, device_id, cx))
        } else if let Some(RestingTrust::Loaded { store, .. }) = self.resting_trust.as_mut() {
            Some(forget_pin(store, device_id, cx))
        } else {
            None
        };
        if let Some(forgotten) = forgotten {
            self.finish_forgetting(forgotten, cx);
        }
        self.refresh(cx);
    }

    /// Tells the person if a forget did not happen, or happened in memory and
    /// did not reach storage: the device would be trusted again at the next
    /// start.
    fn finish_forgetting(&mut self, forgotten: Forgotten, cx: &mut Context<Self>) {
        let Forgotten { what, result } = forgotten;
        match result {
            Err(error) => self.report_problem(format!("Could not forget {what}: {error}."), cx),
            Ok(None) => {}
            Ok(Some(write)) => {
                cx.spawn(async move |this, cx| {
                    if let Err(error) = write.await {
                        this.update(cx, |this, cx| {
                            this.report_problem(
                                format!(
                                    "Forgetting {what} did not reach storage ({error}), so it \
                                     may be trusted again after Zode restarts."
                                ),
                                cx,
                            );
                        })
                        .ok();
                    }
                })
                .detach();
            }
        }
    }

    /// Takes everything down now, without waiting for the setting to change.
    /// Writing the setting is asynchronous and can fail, and a device that is
    /// in control must not stay in control until it succeeds.
    pub fn turn_off(&mut self, cx: &mut Context<Self>) {
        self.switched_off_by_hand = true;
        self.stop(HostState::Disabled, cx);
    }

    /// Lets the host start again after [`Self::turn_off`], whatever the setting
    /// read in the meantime: a person who turns remote control on has asked
    /// for it, even if the write that turned it off did not go through.
    pub(crate) fn resume_after_manual_off(&mut self, cx: &mut Context<Self>) {
        if self.switched_off_by_hand {
            self.switched_off_by_hand = false;
            self.sync(cx);
        }
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
        if let Some(running) = self.running.as_mut() {
            running.unlock_pairing(cx);
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

        if self
            .resting_trust
            .as_ref()
            .is_some_and(|resting| Some(resting.user_id()) != user_id.as_deref())
        {
            self.resting_trust = None;
        }
        if self.seen_user != user_id {
            self.seen_user = user_id.clone();
            cx.notify();
        }

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
        // The pins read while the host was down are the ones to start with:
        // reading them again could miss a change that is still being written.
        let resting_pins = match self.resting_trust.take() {
            Some(RestingTrust::Loaded {
                user_id: loaded_for,
                store,
            }) if loaded_for == user_id => Some(store),
            _ => None,
        };
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
                        if let Some(store) = resting_pins {
                            this.rest_pins(user_id.clone(), store, cx);
                        }
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
            let trust = match resting_pins {
                Some(store) => store,
                None => cx.update(|cx| TrustStore::load(user_id.clone(), cx)).await,
            };
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
            self.rest_pins(user_id, trust, cx);
            cx.notify();
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
                    running.housekeeping(cx);
                    if running.take_changed() {
                        this.refresh(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });

        let pairing_history = match self.resting_pairing.take() {
            Some((kept_for, history)) if kept_for == user_id => history,
            _ => PairingHistory::default(),
        };
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
            sessions_available: self.sessions_available,
            pairing_history,
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
        let stopped_hosting = self.running.take();
        let was_hosting = stopped_hosting.is_some();
        if let Some(hosting) = stopped_hosting {
            self.keep_trust(hosting, cx);
        }
        let was_active =
            was_hosting | self.startup.take().is_some() | self.serving.take().is_some();
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
        // A frame arrives for every keystroke and every chunk of output; most
        // change nothing anyone sees.
        let only_if_changed = matches!(event, RelayEvent::Frame { .. });
        let mut forgotten = None;
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
                            forgotten = Some(forget_pin(&mut running.trust, &device_id, cx));
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
        if let Some(forgotten) = forgotten {
            self.finish_forgetting(forgotten, cx);
        }
        self.refresh_if_wanted(only_if_changed, cx);
    }

    /// Refreshes, unless `only_if_changed` and nothing shown has changed.
    fn refresh_if_wanted(&mut self, only_if_changed: bool, cx: &mut Context<Self>) {
        let changed = self
            .running
            .as_mut()
            .is_some_and(|running| running.take_changed());
        if changed || !only_if_changed {
            self.refresh(cx);
        }
    }

    /// Whether the connection to the relay has room for another frame.
    #[cfg(test)]
    pub(crate) fn relay_has_room(&self, cx: &App) -> bool {
        self.running
            .as_ref()
            .is_some_and(|running| running.client.read(cx).has_capacity_for(1))
    }

    /// Pushes `control` to every device that has said hello, as the host does
    /// when something changes.
    #[cfg(test)]
    pub(crate) fn push_to_all(&mut self, control: &Control, cx: &mut Context<Self>) {
        if let Some(running) = self.running.as_mut() {
            running.broadcast(control, cx);
        }
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

    pub(crate) fn finish_session_request(
        &mut self,
        session_id: u32,
        slot: RequestSlot,
        request_id: u32,
        reply: Result<Vec<Control>, BrowseError>,
        cx: &mut Context<Self>,
    ) {
        if let Some(running) = self.running.as_mut() {
            running.finish_session_request(session_id, slot, request_id, reply, cx);
        }
    }

    fn on_relay_stopped(&mut self, reason: StopReason, cx: &mut Context<Self>) {
        let wipe = reason == StopReason::Revoked;
        let mut wiped = None;
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
                wiped = Some(forget_all_pins(&mut running.trust, cx));
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
        if let Some(wiped) = wiped {
            self.finish_forgetting(wiped, cx);
        }
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
            TerminalMirrorEvent::OutputReady { session_id } => {
                running.flush(*session_id, cx);
            }
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
        // Only a change to the list of terminals is news to the indicator;
        // output streaming and resizes are not, unless they ended a session.
        self.refresh_if_wanted(!matches!(event, TerminalMirrorEvent::ListChanged), cx);
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
        self.refresh_if_wanted(true, cx);
    }

    /// Recomputes what depends on the sessions and tells whoever is watching.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Some(running) = self.running.as_mut() {
            running.take_changed();
        }
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

    #[cfg(test)]
    pub(crate) fn offers_sessions(&self) -> bool {
        self.sessions_available
    }
}
