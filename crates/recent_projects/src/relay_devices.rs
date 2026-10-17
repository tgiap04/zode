//! The Zodes of this account that can be opened from here: pairing with one,
//! connecting to it, choosing a folder on it, and opening the terminals and
//! agents it shares.

use gpui::{
    Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, SharedString,
    Subscription, Task, WeakEntity, Window,
};
use remote::RelayConnectionOptions;
use remote_relay_client::{
    ConnectError, PairingAttempt, RelayHost, RelayInitiator, RelayInitiatorEvent, RelaySession,
    RelaySessionEvent,
};
use remote_relay_protocol::TerminalSummary;
use workspace::Workspace;

use crate::relay_mirror::open_mirror_tab;

pub(crate) enum RelayDevicesEvent {
    /// The person wants a folder on this host as a project.
    OpenProject(RelayConnectionOptions),
    /// Back to the list of remote servers.
    Back,
}

pub(crate) enum State {
    Loading,
    Hosts(Vec<RelayHost>),
    Pairing {
        attempt: Entity<PairingAttempt>,
        _watch: Subscription,
    },
    Connecting(RelayHost),
    Host {
        session: Entity<RelaySession>,
        _watch: Subscription,
    },
    Problem(SharedString),
}

pub(crate) struct RelayDevices {
    focus_handle: FocusHandle,
    workspace: WeakEntity<Workspace>,
    initiator: Option<Entity<RelayInitiator>>,
    pub(crate) state: State,
    working: Option<Task<()>>,
    _watch_initiator: Option<Subscription>,
    _release: Subscription,
}

impl EventEmitter<RelayDevicesEvent> for RelayDevices {}
impl EventEmitter<DismissEvent> for RelayDevices {}

impl Focusable for RelayDevices {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl RelayDevices {
    /// Opening this is what connects to the relay: nothing is reached before.
    pub(crate) fn new(workspace: WeakEntity<Workspace>, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            workspace,
            initiator: None,
            state: State::Loading,
            working: None,
            _watch_initiator: None,
            // A pairing left open by closing the dialog would hold a relay
            // session until the other Zode gave up on it.
            _release: cx.on_release(|this, cx| {
                if let State::Pairing { attempt, .. } = &this.state {
                    attempt.update(cx, |attempt, cx| attempt.cancel(cx));
                }
            }),
        };
        match RelayInitiator::get_or_create(cx) {
            Ok(initiator) => {
                this._watch_initiator = Some(cx.subscribe(
                    &initiator,
                    |this, _, _: &RelayInitiatorEvent, cx| {
                        if matches!(this.state, State::Hosts(_)) {
                            this.reload_hosts(cx);
                        }
                    },
                ));
                this.initiator = Some(initiator);
                this.reload_hosts(cx);
            }
            Err(error) => this.state = State::Problem(format!("{error:#}").into()),
        }
        this
    }

    fn reload_hosts(&mut self, cx: &mut Context<Self>) {
        let Some(initiator) = self.initiator.clone() else {
            return;
        };
        let hosts = initiator.update(cx, |initiator, cx| initiator.hosts(cx));
        self.working = Some(cx.spawn(async move |this, cx| {
            let state = match hosts.await {
                Ok(hosts) => State::Hosts(hosts),
                Err(error) => State::Problem(format!("{error:#}").into()),
            };
            this.update(cx, |this, cx| {
                // A listing that arrives after the person moved on must not
                // pull them back.
                if matches!(this.state, State::Loading | State::Hosts(_)) {
                    this.state = state;
                    cx.notify();
                }
            })
            .ok();
        }));
        if !matches!(self.state, State::Hosts(_)) {
            self.state = State::Loading;
        }
        cx.notify();
    }

    pub(crate) fn pair(&mut self, host: RelayHost, cx: &mut Context<Self>) {
        let Some(initiator) = self.initiator.clone() else {
            return;
        };
        let attempt = initiator.update(cx, |initiator, cx| initiator.pair(host, cx));
        let watch = cx.observe(&attempt, |_, _, cx| cx.notify());
        self.state = State::Pairing {
            attempt,
            _watch: watch,
        };
        cx.notify();
    }

    pub(crate) fn connect(&mut self, host: RelayHost, cx: &mut Context<Self>) {
        let Some(initiator) = self.initiator.clone() else {
            return;
        };
        let connecting =
            initiator.update(cx, |initiator, cx| initiator.connect(&host.device_id, cx));
        self.state = State::Connecting(host);
        cx.notify();
        self.working = Some(cx.spawn(async move |this, cx| {
            let outcome = connecting.await;
            this.update(cx, |this, cx| {
                this.state = match outcome {
                    Ok(session) => {
                        let watch =
                            cx.subscribe(&session, |this, _, event: &RelaySessionEvent, cx| {
                                if let RelaySessionEvent::Closed(reason) = event {
                                    this.state = State::Problem(
                                        format!("The connection ended: {reason}").into(),
                                    );
                                }
                                cx.notify();
                            });
                        State::Host {
                            session,
                            _watch: watch,
                        }
                    }
                    Err(ConnectError::NotPaired) => State::Problem(
                        "That Zode does not trust this one yet, or has forgotten it. Go back and \
                         pair them again."
                            .into(),
                    ),
                    Err(error) => State::Problem(error.to_string().into()),
                };
                cx.notify();
            })
            .ok();
        }));
    }

    pub(crate) fn forget_and_pair(&mut self, host: RelayHost, cx: &mut Context<Self>) {
        if let Some(initiator) = &self.initiator {
            initiator.update(cx, |initiator, cx| {
                initiator.forget_host(&host.device_id, cx)
            });
        }
        self.pair(host, cx);
    }

    pub(crate) fn back_to_hosts(&mut self, cx: &mut Context<Self>) {
        if let State::Pairing { attempt, .. } = &self.state {
            attempt.update(cx, |attempt, cx| attempt.cancel(cx));
        }
        self.state = State::Loading;
        self.reload_hosts(cx);
    }

    pub(crate) fn open_terminal(
        &mut self,
        session: Entity<RelaySession>,
        terminal: TerminalSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        match open_mirror_tab(&session, &terminal, &workspace, window, cx) {
            Ok(()) => cx.emit(DismissEvent),
            Err(error) => {
                self.state = State::Problem(format!("{error:#}").into());
                cx.notify();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, TestAppContext};
    use http_client::{AsyncBody, FakeHttpClient, Response};
    use remote_relay_client::{
        RelayEnvironment,
        test_support::{FakeRelay, InMemoryKeychain, StaticCredentials},
    };
    use std::sync::Arc;

    fn initiator(relay: &FakeRelay, cx: &mut TestAppContext) -> Entity<RelayInitiator> {
        cx.update(|cx| cx.set_global(db::AppDatabase::test_new()));
        let rows = serde_json::json!([
            {"deviceId": "host-device", "kind": "ide", "name": "Studio", "publicKey": remote_relay_client::encode_public_key(&[7; 32])},
            {"deviceId": "browser-1", "kind": "web", "name": "Browser", "publicKey": null},
        ])
        .to_string();
        let environment = RelayEnvironment {
            credentials: Arc::new(StaticCredentials::new("user", "client-device", "token")),
            keychain: InMemoryKeychain::new(),
            http_client: FakeHttpClient::create(move |_| {
                let rows = rows.clone();
                async move {
                    Ok(Response::builder()
                        .status(200)
                        .body(AsyncBody::from(rows))
                        .expect("a response"))
                }
            }),
            api_url: "https://api.test/api".into(),
            device_name: "Laptop".into(),
        };
        let transport = relay.client_transport("client-device");
        let initiator =
            cx.new(|cx| RelayInitiator::new(environment, transport, "9.9.9".into(), cx));
        cx.update(|cx| RelayInitiator::register_global(&initiator, cx));
        initiator
    }

    #[gpui::test]
    async fn the_list_shows_other_zodes_and_leaves_out_browsers(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        let _initiator = initiator(&relay, cx);
        let view = cx.new(|cx| RelayDevices::new(WeakEntity::new_invalid(), cx));
        cx.run_until_parked();
        view.read_with(cx, |view, _| match &view.state {
            State::Hosts(hosts) => {
                assert_eq!(hosts.len(), 1);
                assert_eq!(hosts[0].name, "Studio");
                assert!(!hosts[0].paired, "nothing is paired until a person says so");
            }
            _ => panic!("expected the host list"),
        });
    }

    #[gpui::test]
    async fn without_an_account_the_dialog_says_to_sign_in(cx: &mut TestAppContext) {
        let view = cx.new(|cx| RelayDevices::new(WeakEntity::new_invalid(), cx));
        view.read_with(cx, |view, _| {
            assert!(matches!(view.state, State::Problem(_)));
        });
    }
}
