//! The Zodes of this account that can be opened from here: pairing with one,
//! connecting to it, choosing a folder on it, and opening the terminals and
//! agents it shares.

use std::collections::HashMap;

use gpui::{
    App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Global, SharedString,
    Subscription, Task, WeakEntity, Window,
};
use remote::RelayConnectionOptions;
use remote_relay_client::{
    ConnectError, PairingAttempt, RelayHost, RelayInitiator, RelayInitiatorEvent, RelaySession,
    RelaySessionEvent, sanitize_display_text,
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

/// The most characters of a title or reason from another computer that are
/// drawn.
pub(crate) const MAX_DISPLAY_CHARS: usize = 64;

/// A title, reason or version that came from another computer, made fit to
/// draw by the same rules as a device name.
pub(crate) fn display_text(text: &str) -> String {
    sanitize_display_text(text, MAX_DISPLAY_CHARS)
}

/// The live session to each host, by device id. A host allows only a few
/// sessions at a time and a mirror tab keeps its session for as long as it is
/// open, so a dialog that took a new session on every opening would have the
/// host refuse the next one. Weak: a session nothing holds is gone, and is not
/// reused.
#[derive(Default)]
struct HostSessions(HashMap<String, WeakEntity<RelaySession>>);

impl Global for HostSessions {}

fn live_session(host_device_id: &str, cx: &App) -> Option<Entity<RelaySession>> {
    let session = cx
        .try_global::<HostSessions>()?
        .0
        .get(host_device_id)?
        .upgrade()?;
    (!session.read(cx).is_closed()).then_some(session)
}

fn remember_session(session: &Entity<RelaySession>, cx: &mut App) {
    let host_device_id = session.read(cx).host_device_id().to_string();
    let sessions = cx.default_global::<HostSessions>();
    sessions.0.retain(|_, held| held.upgrade().is_some());
    sessions.0.insert(host_device_id, session.downgrade());
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
        if let Some(session) = live_session(&host.device_id, cx) {
            self.show_session(session, cx);
            cx.notify();
            return;
        }
        let connecting =
            initiator.update(cx, |initiator, cx| initiator.connect(&host.device_id, cx));
        self.state = State::Connecting(host);
        cx.notify();
        self.working =
            Some(cx.spawn(async move |this, cx| {
                let outcome = connecting.await;
                this.update(cx, |this, cx| {
                    match outcome {
                        Ok(session) => {
                            remember_session(&session, cx);
                            this.show_session(session, cx);
                        }
                        Err(ConnectError::NotPaired) => this.state = State::Problem(
                            "That Zode does not trust this one yet, or has forgotten it. Go back \
                             and pair them again."
                                .into(),
                        ),
                        Err(error) => this.state = State::Problem(error.to_string().into()),
                    }
                    cx.notify();
                })
                .ok();
            }));
    }

    fn show_session(&mut self, session: Entity<RelaySession>, cx: &mut Context<Self>) {
        let watch = cx.subscribe(&session, |this, _, event: &RelaySessionEvent, cx| {
            if let RelaySessionEvent::Closed(reason) = event {
                this.state = State::Problem(
                    format!("The connection ended: {}", display_text(reason)).into(),
                );
            }
            cx.notify();
        });
        self.state = State::Host {
            session,
            _watch: watch,
        };
    }

    /// Pairing again with a host whose key changed starts by forgetting the old
    /// key, and does not go on unless that held: a key that is forgotten here
    /// but comes back at the next start is the key that was meant to be gone.
    pub(crate) fn forget_and_pair(&mut self, host: RelayHost, cx: &mut Context<Self>) {
        let Some(initiator) = self.initiator.clone() else {
            return;
        };
        let forgotten = initiator.update(cx, |initiator, cx| {
            initiator.try_forget_host(&host.device_id, cx)
        });
        if let Err(error) = forgotten {
            self.state =
                State::Problem(format!("{} could not be paired again: {error}", host.name).into());
            cx.notify();
            return;
        }
        let saved = initiator.update(cx, |initiator, cx| initiator.flush_pins_checked(cx));
        self.working = Some(cx.spawn(async move |this, cx| {
            let outcome = saved.await;
            this.update(cx, |this, cx| match outcome {
                Ok(()) => this.pair(host, cx),
                Err(error) => {
                    this.state = State::Problem(
                        format!(
                            "{} was forgotten, but that could not be saved, so it comes back at \
                             the next start: {error}",
                            host.name
                        )
                        .into(),
                    );
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    pub(crate) fn back_to_hosts(&mut self, cx: &mut Context<Self>) {
        // Without an initiator there is no list to go back to: the dialog
        // never got past telling the person to sign in.
        if self.initiator.is_none() {
            cx.emit(RelayDevicesEvent::Back);
            return;
        }
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
    use std::{cell::Cell, rc::Rc, sync::Arc};

    fn initiator(relay: &FakeRelay, cx: &mut TestAppContext) -> Entity<RelayInitiator> {
        initiator_named(relay, "Studio", cx)
    }

    fn initiator_named(
        relay: &FakeRelay,
        host_name: &str,
        cx: &mut TestAppContext,
    ) -> Entity<RelayInitiator> {
        cx.update(|cx| cx.set_global(db::AppDatabase::test_new()));
        let rows = serde_json::json!([
            {"deviceId": "host-device", "kind": "ide", "name": host_name, "publicKey": remote_relay_client::encode_public_key(&[7; 32])},
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

    #[test]
    fn display_text_leaves_an_ordinary_name_alone() {
        assert_eq!(display_text("Studio"), "Studio");
        assert_eq!(display_text("Máy của Tobi"), "Máy của Tobi");
    }

    #[test]
    fn display_text_drops_what_breaks_or_reorders_a_line() {
        assert_eq!(display_text("Stu\ndio\t\u{7}"), "Studio");
        assert_eq!(
            display_text("a\u{202E}b\u{202A}c\u{2066}d\u{2069}e"),
            "abcde"
        );
    }

    #[test]
    fn display_text_drops_what_a_device_name_drops() {
        for hidden in [
            '\u{200E}', '\u{200F}', '\u{061C}', '\u{2028}', '\u{2029}', '\u{200B}',
        ] {
            assert_eq!(display_text(&format!("a{hidden}b")), "ab", "{hidden:?}");
        }
        assert_eq!(display_text("  padded  "), "padded");
    }

    #[test]
    fn display_text_is_cut_to_the_limit_in_characters_not_bytes() {
        let long = "é".repeat(MAX_DISPLAY_CHARS * 4);
        let shown = display_text(&long);
        assert_eq!(shown.chars().count(), MAX_DISPLAY_CHARS);
        assert!(shown.ends_with('…'));

        let exact = "x".repeat(MAX_DISPLAY_CHARS);
        assert_eq!(display_text(&exact), exact, "a name at the limit is whole");
    }

    #[gpui::test]
    async fn a_hostile_host_name_is_listed_clean(cx: &mut TestAppContext) {
        let relay = FakeRelay::new();
        let hostile = format!("Studio\u{202E}\n{}", "x".repeat(500));
        let _initiator = initiator_named(&relay, &hostile, cx);
        let view = cx.new(|cx| RelayDevices::new(WeakEntity::new_invalid(), cx));
        cx.run_until_parked();
        view.read_with(cx, |view, _| match &view.state {
            State::Hosts(hosts) => {
                assert_eq!(hosts[0].name.chars().count(), MAX_DISPLAY_CHARS);
                assert!(hosts[0].name.starts_with("Studiox"));
            }
            _ => panic!("expected the host list"),
        });
    }

    #[gpui::test]
    async fn back_from_the_sign_in_problem_leaves_the_dialog(cx: &mut TestAppContext) {
        let view = cx.new(|cx| RelayDevices::new(WeakEntity::new_invalid(), cx));
        let left = Rc::new(Cell::new(false));
        let _watch = cx.update(|cx| {
            let left = left.clone();
            cx.subscribe(&view, move |_, event: &RelayDevicesEvent, _| {
                if matches!(event, RelayDevicesEvent::Back) {
                    left.set(true);
                }
            })
        });

        view.update(cx, |view, cx| view.back_to_hosts(cx));

        assert!(left.get(), "Back leaves; there is no list to go back to");
        view.read_with(cx, |view, _| {
            assert!(
                matches!(view.state, State::Problem(_)),
                "no loading state is shown for a listing nobody is fetching"
            );
        });
    }
}
