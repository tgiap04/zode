//! What the other-Zodes dialog draws for each of its states.

use gpui::{Entity, FontWeight, SharedString, Window};
use release_channel::AppVersion;
use remote::RelayConnectionOptions;
use remote_relay_client::{
    PairingAttempt, PairingFailure, PairingStep, RelayHost, RelaySession, sanitize_device_name,
};
use remote_relay_protocol::{AgentStatus, TerminalSummary};
use ui::{
    Divider, Indicator, ListItem, ListItemSpacing, ListSubHeader, Modal, ModalFooter, ModalHeader,
    Section, prelude::*,
};

use crate::relay_devices::{RelayDevices, RelayDevicesEvent, State, display_text};

/// How tall a list inside the dialog may grow before it scrolls, so that the
/// footer stays inside the dialog.
fn list_max_height() -> Rems {
    rems(22.)
}

/// An agent on the other Zode, with its text already made fit to draw and the
/// terminal it is attached through.
struct AgentRow {
    id: String,
    label: String,
    name: String,
    status: AgentStatus,
    terminal: TerminalSummary,
}

impl RelayDevices {
    fn render_host_row(&self, host: &RelayHost, cx: &mut Context<Self>) -> gpui::AnyElement {
        let status = if host.key_changed {
            "Its key changed since it was paired. The other Zode may have been reinstalled or \
             replaced."
        } else if host.paired {
            "Paired"
        } else if host.public_key.is_none() {
            "Not ready to pair"
        } else {
            "Not paired yet"
        };
        let dot = if host.online {
            Color::Success
        } else {
            Color::Muted
        };
        let id = SharedString::from(host.device_id.clone());
        let action = {
            let host = host.clone();
            if host.key_changed {
                Button::new(SharedString::from(format!("repair-{id}")), "Pair Again")
                    .label_size(LabelSize::Small)
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.forget_and_pair(host.clone(), cx)),
                    )
            } else if host.paired {
                Button::new(SharedString::from(format!("connect-{id}")), "Connect")
                    .label_size(LabelSize::Small)
                    .style(ButtonStyle::Filled)
                    .disabled(!host.online)
                    .on_click(cx.listener(move |this, _, _, cx| this.connect(host.clone(), cx)))
            } else {
                Button::new(SharedString::from(format!("pair-{id}")), "Pair…")
                    .label_size(LabelSize::Small)
                    .disabled(!host.online || host.public_key.is_none())
                    .on_click(cx.listener(move |this, _, _, cx| this.pair(host.clone(), cx)))
            }
        };
        ListItem::new(SharedString::from(format!("relay-host-{id}")))
            .inset(true)
            .spacing(ListItemSpacing::Sparse)
            .selectable(false)
            .start_slot(Indicator::dot().color(dot))
            .child(
                v_flex()
                    .child(Label::new(host.name.clone()).truncate())
                    .child(
                        Label::new(if host.online {
                            status.to_string()
                        } else {
                            format!("{status}, offline")
                        })
                        .size(LabelSize::Small)
                        .color(if host.key_changed {
                            Color::Warning
                        } else {
                            Color::Muted
                        }),
                    ),
            )
            .end_slot(action)
            .into_any_element()
    }

    fn render_pairing(
        &self,
        attempt: &Entity<PairingAttempt>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (name, step) = {
            let attempt = attempt.read(cx);
            (attempt.host().name.clone(), attempt.step().clone())
        };
        let body = |text: String| Label::new(text).color(Color::Muted);
        let content = match step {
            PairingStep::Requesting => v_flex()
                .gap_2()
                .child(body(format!("Asking {name} to pair…")))
                .into_any_element(),
            PairingStep::Comparing { digits } => v_flex()
                .child(body(format!(
                    "{name} should be showing the same six digits. Only continue if they are \
                     identical."
                )))
                .child(
                    v_flex().w_full().py_4().gap_1p5().items_center().child(
                        Label::new(digits)
                            .size(LabelSize::Large)
                            .weight(FontWeight::MEDIUM)
                            .buffer_font(cx),
                    ),
                )
                .child(
                    h_flex()
                        .w_full()
                        .gap_1()
                        .justify_end()
                        // `Button` has no probe of its own, so each one is wrapped
                        // for a test to find its bounds and click it.
                        .child(div().debug_selector(|| "relay-digits-differ".into()).child(
                            Button::new("digits-differ", "Digits Differ").on_click({
                                let attempt = attempt.clone();
                                move |_, _, cx| {
                                    attempt.update(cx, |attempt, cx| attempt.digits_differ(cx))
                                }
                            }),
                        ))
                        .child(
                            div().debug_selector(|| "relay-digits-match".into()).child(
                                Button::new("digits-match", "The Digits Match")
                                    .style(ButtonStyle::Filled)
                                    .on_click({
                                        let attempt = attempt.clone();
                                        move |_, _, cx| {
                                            attempt.update(cx, |attempt, cx| {
                                                attempt.confirm_digits(cx)
                                            })
                                        }
                                    }),
                            ),
                        ),
                )
                .into_any_element(),
            PairingStep::WaitingForHost => v_flex()
                .gap_2()
                .child(body(format!(
                    "Now confirm on {name} that the digits match."
                )))
                .into_any_element(),
            PairingStep::Verifying => v_flex()
                .gap_2()
                .child(body(format!("Checking that {name} holds the key…")))
                .into_any_element(),
            PairingStep::Done => v_flex()
                .gap_2()
                .child(body(format!("{name} is paired.")))
                .child(
                    Button::new("continue-after-pairing", "Continue")
                        .style(ButtonStyle::Filled)
                        .on_click({
                            let host = attempt.read(cx).host().clone();
                            cx.listener(move |this, _, _, cx| this.connect(host.clone(), cx))
                        }),
                )
                .into_any_element(),
            PairingStep::Failed(failure) => v_flex()
                .gap_2()
                .child(Label::new(failure.to_string()).color(match failure {
                    PairingFailure::Cancelled => Color::Muted,
                    _ => Color::Error,
                }))
                .into_any_element(),
        };
        content
    }

    fn render_session(
        &self,
        session: &Entity<RelaySession>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let local_version = AppVersion::global(cx).to_string();
        let (name, host_version, same_version, can_open, any_agents, agents, terminals) = {
            let session = session.read(cx);
            let host_version = session.host().app_version.as_str();
            let terminals: Vec<TerminalSummary> = session
                .terminals()
                .iter()
                .map(|terminal| TerminalSummary {
                    id: terminal.id.clone(),
                    title: display_text(&terminal.title),
                    columns: terminal.columns,
                    rows: terminal.rows,
                })
                .collect();
            let agents = session
                .agents()
                .iter()
                .filter_map(|agent| {
                    // An agent is attached through its terminal, which carries
                    // the same id; one with no terminal has nothing to show yet.
                    let terminal = terminals.iter().find(|terminal| terminal.id == agent.id)?;
                    Some(AgentRow {
                        id: agent.id.clone(),
                        label: display_text(agent.title.as_deref().unwrap_or(&agent.name)),
                        name: display_text(&agent.name),
                        status: agent.status,
                        terminal: terminal.clone(),
                    })
                })
                .collect::<Vec<_>>();
            (
                sanitize_device_name(session.host_name()),
                display_text(host_version),
                host_version == local_version,
                session.host().can("ide"),
                !session.agents().is_empty(),
                agents,
                terminals,
            )
        };

        let no_terminals = terminals.is_empty();
        let open_note = if !same_version {
            Some(format!(
                "{name} runs version {host_version} and this Zode runs {local_version}. Update \
                 both to the same version to open its projects. Its terminals and agents below \
                 still work."
            ))
        } else if !can_open {
            Some(format!(
                "{name} was built without the project server, so its projects cannot be opened."
            ))
        } else {
            None
        };

        v_flex()
            .child(
                v_flex()
                    .gap_2()
                    .p_3()
                    .child(
                        Button::new("open-project", format!("Open a Project on {name}"))
                            .style(ButtonStyle::Filled)
                            .disabled(open_note.is_some())
                            .on_click({
                                let options = RelayConnectionOptions {
                                    host_device_id: session.read(cx).host_device_id().to_string(),
                                    host_name: name,
                                };
                                cx.listener(move |_, _, _, cx| {
                                    cx.emit(RelayDevicesEvent::OpenProject(options.clone()))
                                })
                            }),
                    )
                    .children(open_note.map(|note| {
                        Label::new(note)
                            .size(LabelSize::Small)
                            .color(Color::Warning)
                    })),
            )
            .child(Divider::horizontal())
            // Bounded like the list of hosts: the host can share up to a few
            // hundred of each, and the rows below the dialog's edge could
            // otherwise not be reached, nor the Back button under them.
            .child(
                div()
                    .id("relay-session-items")
                    .debug_selector(|| "relay-session-items".into())
                    .w_full()
                    .max_h(list_max_height())
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .child(ListSubHeader::new("Agents").inset(true))
                            .children(self.agent_rows(session, agents, cx))
                            .when(!any_agents, |this| this.child(empty_note("None running")))
                            .child(ListSubHeader::new("Terminals").inset(true))
                            .children(
                                terminals
                                    .into_iter()
                                    .map(|terminal| self.terminal_row(session, terminal, cx)),
                            )
                            .when(no_terminals, |this| this.child(empty_note("None open"))),
                    ),
            )
            .into_any_element()
    }

    fn agent_rows(
        &self,
        session: &Entity<RelaySession>,
        agents: Vec<AgentRow>,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        agents
            .into_iter()
            .map(|agent| {
                let session = session.clone();
                let terminal = agent.terminal;
                ListItem::new(SharedString::from(format!("relay-agent-{}", agent.id)))
                    .inset(true)
                    .spacing(ListItemSpacing::Sparse)
                    .selectable(false)
                    .child(
                        v_flex().child(Label::new(agent.label).truncate()).child(
                            Label::new(format!(
                                "{} · {}",
                                agent.name,
                                agent_status_label(agent.status)
                            ))
                            .size(LabelSize::Small)
                            .color(Color::Muted)
                            .truncate(),
                        ),
                    )
                    .end_slot(
                        Button::new(SharedString::from(format!("agent-{}", agent.id)), "Open")
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_terminal(session.clone(), terminal.clone(), window, cx)
                            })),
                    )
                    .into_any_element()
            })
            .collect()
    }

    fn terminal_row(
        &self,
        session: &Entity<RelaySession>,
        terminal: TerminalSummary,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let session = session.clone();
        ListItem::new(SharedString::from(format!(
            "relay-terminal-{}",
            terminal.id
        )))
        .inset(true)
        .spacing(ListItemSpacing::Sparse)
        .selectable(false)
        .child(Label::new(terminal.title.clone()).truncate())
        .end_slot(
            Button::new(
                SharedString::from(format!("terminal-{}", terminal.id)),
                "Open",
            )
            .label_size(LabelSize::Small)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_terminal(session.clone(), terminal.clone(), window, cx)
            })),
        )
        .into_any_element()
    }
}

impl Render for RelayDevices {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.state {
            State::Loading => centred_message()
                .child(Label::new("Looking for your other Zodes…").color(Color::Muted))
                .into_any_element(),
            State::Hosts(hosts) if hosts.is_empty() => centred_message()
                .child(Label::new("No other Zodes on this account."))
                .child(
                    Label::new(
                        "On the other computer, sign in and turn on remote control in settings.",
                    )
                    .color(Color::Muted)
                    .size(LabelSize::Small),
                )
                .into_any_element(),
            State::Hosts(hosts) => {
                // Bounded so a long list scrolls inside the dialog instead of
                // pushing the footer out of it.
                div()
                    .id("relay-hosts-list")
                    .debug_selector(|| "relay-hosts-list".into())
                    .w_full()
                    .max_h(list_max_height())
                    .overflow_y_scroll()
                    .child(
                        v_flex().children(hosts.iter().map(|host| self.render_host_row(host, cx))),
                    )
                    .into_any_element()
            }
            State::Pairing { attempt, .. } => {
                let attempt = attempt.clone();
                v_flex()
                    .p_3()
                    .child(self.render_pairing(&attempt, cx))
                    .into_any_element()
            }
            State::Connecting(host) => v_flex()
                .p_3()
                .child(
                    Label::new(format!("Connecting to {}…", host.name))
                        .color(Color::Muted)
                        .truncate(),
                )
                .into_any_element(),
            State::Host { session, .. } => {
                let session = session.clone();
                self.render_session(&session, cx)
            }
            State::Problem(message) => v_flex()
                .p_3()
                .child(Label::new(message.clone()).color(Color::Error))
                .into_any_element(),
        };

        Modal::new("relay-devices", None)
            .header(ModalHeader::new().headline("Other Zodes"))
            .section(Section::new().padded(false).child(body))
            .footer(
                ModalFooter::new().end_slot(Button::new("relay-back", "Back").on_click(
                    cx.listener(|this, _, _, cx| {
                        if matches!(this.state, State::Hosts(_) | State::Loading) {
                            cx.emit(RelayDevicesEvent::Back);
                        } else {
                            this.back_to_hosts(cx);
                        }
                    }),
                )),
            )
    }
}

/// The container for a state that has nothing to list, so a screen that is
/// loading and one that is empty sit in the same place.
fn centred_message() -> gpui::Div {
    v_flex()
        .w_full()
        .items_center()
        .justify_center()
        .p_4()
        .gap_1()
}

/// A line inside a list that has no rows to show.
fn empty_note(text: &'static str) -> gpui::Div {
    h_flex()
        .px_2()
        .py_1p5()
        .child(Label::new(text).size(LabelSize::Small).color(Color::Muted))
}

fn agent_status_label(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Working => "Working",
        AgentStatus::WaitingForInput => "Waiting for you",
        AgentStatus::Idle => "Idle",
        AgentStatus::Finished => "Finished",
        AgentStatus::Failed => "Failed",
        AgentStatus::Unknown => "Status unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{DismissEvent, Modifiers, TestAppContext, VisualTestContext, WeakEntity};
    use http_client::{AsyncBody, FakeHttpClient, Response};
    use project::Project;
    use remote_relay_client::{
        RelayEnvironment, RelayInitiator,
        scripted_host::ScriptedHost,
        test_support::{FakeRelay, InMemoryKeychain, StaticCredentials},
    };
    use remote_relay_protocol::{Control, DeviceKeypair};
    use std::{cell::Cell, rc::Rc, sync::Arc};
    use terminal_view::TerminalView;
    use workspace::{AppState, Workspace};

    /// An initiator whose account directory lists `rows`, registered as the
    /// global the dialog reaches for.
    fn initiator_listing(
        relay: &FakeRelay,
        rows: serde_json::Value,
        cx: &mut TestAppContext,
    ) -> Entity<RelayInitiator> {
        fresh_database(cx);
        initiator_on_current_database(relay, rows, cx)
    }

    fn fresh_database(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_global(db::AppDatabase::test_new()));
    }

    /// [`initiator_listing`] over whatever the database already holds, for a
    /// test that seeds it first.
    fn initiator_on_current_database(
        relay: &FakeRelay,
        rows: serde_json::Value,
        cx: &mut TestAppContext,
    ) -> Entity<RelayInitiator> {
        let rows = rows.to_string();
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

    /// What the dialog is built around: a window to draw in, the initiator the
    /// view reaches through its global, and the host it pairs with.
    struct Pairing {
        initiator: Entity<RelayInitiator>,
        host: Entity<ScriptedHost>,
    }

    /// A dialog drawn in a window and brought to the point where the digits are
    /// on screen and the buttons under them can be pressed.
    async fn comparing_digits(
        cx: &mut TestAppContext,
    ) -> (
        Pairing,
        Entity<RelayDevices>,
        Entity<PairingAttempt>,
        &mut VisualTestContext,
    ) {
        cx.update(|cx| {
            workspace::AppState::test(cx);
        });
        let relay = FakeRelay::new();
        let host_key = Arc::new(DeviceKeypair::generate().expect("a key"));
        let host = cx.new(|cx| ScriptedHost::new(relay.transport(), "user", host_key.clone(), cx));
        let initiator = initiator_listing(
            &relay,
            serde_json::json!([
                {"deviceId": "host-device", "kind": "ide", "name": "Studio", "publicKey": remote_relay_client::encode_public_key(host_key.public_key())},
            ]),
            cx,
        );
        let (view, cx) =
            cx.add_window_view(|_window, cx| RelayDevices::new(WeakEntity::new_invalid(), cx));
        cx.run_until_parked();

        let studio = view.read_with(cx, |view, _| match &view.state {
            State::Hosts(hosts) => hosts[0].clone(),
            _ => panic!("expected the host list"),
        });
        view.update(cx, |view, cx| view.pair(studio, cx));
        cx.run_until_parked();
        let attempt = view.read_with(cx, |view, _| match &view.state {
            State::Pairing { attempt, .. } => attempt.clone(),
            _ => panic!("expected a pairing in progress"),
        });
        assert!(
            matches!(
                attempt.read_with(cx, |attempt, _| attempt.step().clone()),
                PairingStep::Comparing { .. }
            ),
            "the digits must be on screen before there is anything to press"
        );
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        (Pairing { initiator, host }, view, attempt, cx)
    }

    fn click(selector: &'static str, cx: &mut VisualTestContext) {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("`{selector}` must be drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    #[gpui::test]
    async fn the_digits_match_button_confirms_here_and_waits_for_the_other_zode(
        cx: &mut TestAppContext,
    ) {
        let (pairing, _view, attempt, cx) = comparing_digits(cx).await;

        click("relay-digits-match", cx);

        assert_eq!(
            attempt.read_with(cx, |attempt, _| attempt.step().clone()),
            PairingStep::WaitingForHost,
            "confirming here is half the answer; the other Zode still has to say yes"
        );
        assert!(
            pairing
                .initiator
                .read_with(cx, |initiator, _| initiator.pinned_hosts().is_empty()),
            "nothing is trusted until both people have said yes"
        );

        pairing
            .host
            .update(cx, |host, cx| host.decide_pairing(true, cx));
        cx.run_until_parked();
        assert_eq!(
            attempt.read_with(cx, |attempt, _| attempt.step().clone()),
            PairingStep::Done
        );
        assert_eq!(
            pairing
                .initiator
                .read_with(cx, |initiator, _| initiator.pinned_hosts().len()),
            1,
            "with both answers in, the host is trusted"
        );
    }

    #[gpui::test]
    async fn the_digits_differ_button_ends_the_pairing_and_trusts_nothing(cx: &mut TestAppContext) {
        let (pairing, view, attempt, cx) = comparing_digits(cx).await;

        click("relay-digits-differ", cx);

        assert_eq!(
            attempt.read_with(cx, |attempt, _| attempt.step().clone()),
            PairingStep::Failed(PairingFailure::DigitsDiffer)
        );
        assert!(
            pairing
                .initiator
                .read_with(cx, |initiator, _| initiator.pinned_hosts().is_empty()),
            "digits that differ must pin nothing"
        );
        assert!(
            pairing
                .host
                .read_with(cx, |host, _| !host.ended_sessions().is_empty()),
            "the other Zode must be told the pairing session is over"
        );
        assert!(
            view.read_with(cx, |view, _| matches!(view.state, State::Pairing { .. })),
            "the dialog stays on the pairing so the failure can be read"
        );
    }

    /// Enough hosts that their rows cannot fit the dialog, so the list either
    /// scrolls inside its bound or pushes the footer out of the dialog.
    #[gpui::test]
    async fn a_long_list_of_other_zodes_scrolls_instead_of_growing(cx: &mut TestAppContext) {
        cx.update(|cx| {
            workspace::AppState::test(cx);
        });
        let relay = FakeRelay::new();
        let rows = (0..20)
            .map(|index| {
                serde_json::json!({
                    "deviceId": format!("host-{index}"),
                    "kind": "ide",
                    "name": format!("Studio {index}"),
                    "publicKey": remote_relay_client::encode_public_key(&[7; 32]),
                })
            })
            .collect::<Vec<_>>();
        let _initiator = initiator_listing(&relay, serde_json::Value::Array(rows), cx);
        let (_view, cx) =
            cx.add_window_view(|_window, cx| RelayDevices::new(WeakEntity::new_invalid(), cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        let list = cx
            .debug_bounds("relay-hosts-list")
            .expect("the list must be drawn");
        let bound = cx.update(|window, _| gpui::rems(22.).to_pixels(window.rem_size()));
        assert!(
            (list.size.height - bound).abs() < gpui::px(1.),
            "twenty rows must fill the list's bound of {bound:?} and scroll past it, got a \
             list {:?} tall",
            list.size.height
        );
    }

    /// A host of this account that lists itself in the directory, and the
    /// initiator the dialog reaches for.
    fn relay_with_host(cx: &mut TestAppContext) -> (Entity<ScriptedHost>, Entity<RelayInitiator>) {
        fresh_database(cx);
        relay_with_host_on_current_database(cx)
    }

    fn relay_with_host_on_current_database(
        cx: &mut TestAppContext,
    ) -> (Entity<ScriptedHost>, Entity<RelayInitiator>) {
        let relay = FakeRelay::new();
        let host_key = Arc::new(DeviceKeypair::generate().expect("a key"));
        let host = cx.new(|cx| ScriptedHost::new(relay.transport(), "user", host_key.clone(), cx));
        let initiator = initiator_on_current_database(
            &relay,
            serde_json::json!([
                {"deviceId": "host-device", "kind": "ide", "name": "Studio", "publicKey": remote_relay_client::encode_public_key(host_key.public_key())},
            ]),
            cx,
        );
        (host, initiator)
    }

    /// Pairs `view` with the host through the same steps a person takes, then
    /// connects to it, and gives the session the dialog ended up holding.
    fn pair_and_connect(
        view: &Entity<RelayDevices>,
        host: &Entity<ScriptedHost>,
        cx: &mut VisualTestContext,
    ) -> Entity<RelaySession> {
        cx.run_until_parked();
        let studio = view.read_with(cx, |view, _| match &view.state {
            State::Hosts(hosts) => hosts[0].clone(),
            _ => panic!("expected the host list"),
        });
        view.update(cx, |view, cx| view.pair(studio.clone(), cx));
        cx.run_until_parked();
        let attempt = view.read_with(cx, |view, _| match &view.state {
            State::Pairing { attempt, .. } => attempt.clone(),
            _ => panic!("expected a pairing in progress"),
        });
        attempt.update(cx, |attempt, cx| attempt.confirm_digits(cx));
        cx.run_until_parked();
        host.update(cx, |host, cx| host.decide_pairing(true, cx));
        cx.run_until_parked();
        view.update(cx, |view, cx| view.connect(studio, cx));
        cx.run_until_parked();
        view.read_with(cx, |view, _| match &view.state {
            State::Host { session, .. } => session.clone(),
            _ => panic!("expected a connected host"),
        })
    }

    /// The id the connected host knows its one session with this Zode by.
    fn host_session_id(host: &Entity<ScriptedHost>, cx: &mut VisualTestContext) -> u32 {
        host.read_with(cx, |host, _| {
            let ids = host.session_ids();
            assert_eq!(ids.len(), 1, "one session with this Zode");
            ids[0]
        })
    }

    #[gpui::test]
    async fn a_host_with_many_terminals_scrolls_instead_of_growing(cx: &mut TestAppContext) {
        cx.update(|cx| {
            workspace::AppState::test(cx);
        });
        let (host, _initiator) = relay_with_host(cx);
        let (view, cx) =
            cx.add_window_view(|_window, cx| RelayDevices::new(WeakEntity::new_invalid(), cx));
        let _session = pair_and_connect(&view, &host, cx);

        let session_id = host_session_id(&host, cx);
        let terminals = (0..40)
            .map(|index| remote_relay_protocol::TerminalSummary {
                id: format!("terminal-{index}"),
                title: format!("shell {index}"),
                columns: 80,
                rows: 24,
            })
            .collect();
        host.update(cx, |host, cx| {
            host.send_control(session_id, &Control::TerminalList { terminals }, cx)
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        let list = cx
            .debug_bounds("relay-session-items")
            .expect("the agents and terminals must be drawn");
        let bound = cx.update(|window, _| list_max_height().to_pixels(window.rem_size()));
        assert!(
            (list.size.height - bound).abs() < gpui::px(1.),
            "forty terminals must fill the list's bound of {bound:?} and scroll past it, got a \
             list {:?} tall",
            list.size.height
        );
    }

    /// A workspace the dialog was opened from, a terminal of the connected host
    /// opened as a tab, and the dialog closed the way the modal layer closes it
    /// on `DismissEvent`: its view and the session it held are dropped.
    struct MirrorTab {
        host: Entity<ScriptedHost>,
        /// The dialog reaches the initiator through a weak global, so a test
        /// that opens a second dialog has to keep it alive itself.
        _initiator: Entity<RelayInitiator>,
        workspace: Entity<Workspace>,
        session: WeakEntity<RelaySession>,
        host_session_id: u32,
        stream_id: u32,
    }

    async fn open_a_mirror_tab_and_close_the_dialog<'a>(
        cx: &'a mut TestAppContext,
        terminal_title: &str,
    ) -> (MirrorTab, &'a mut VisualTestContext) {
        let app_state = cx.update(|cx| {
            let app_state = AppState::test(cx);
            workspace::register_serializable_item::<TerminalView>(cx);
            app_state
        });
        let (host, initiator) = relay_with_host(cx);
        let project = Project::test(app_state.fs.clone(), [], cx).await;
        let (workspace, cx) =
            cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
        let view = workspace.update_in(cx, |_, _, cx| {
            let weak_workspace = cx.weak_entity();
            cx.new(|cx| RelayDevices::new(weak_workspace, cx))
        });
        let session = pair_and_connect(&view, &host, cx);
        let host_session_id = host_session_id(&host, cx);

        let dismissed = Rc::new(Cell::new(false));
        let watch = cx.update(|_, cx| {
            let dismissed = dismissed.clone();
            cx.subscribe(&view, move |_, _: &DismissEvent, _| dismissed.set(true))
        });
        let terminal = remote_relay_protocol::TerminalSummary {
            id: "terminal-1".into(),
            title: terminal_title.into(),
            columns: 80,
            rows: 24,
        };
        view.update_in(cx, |view, window, cx| {
            view.open_terminal(session.clone(), terminal, window, cx)
        });
        cx.run_until_parked();
        assert!(
            dismissed.get(),
            "opening the tab closes the dialog it was opened from"
        );
        let stream_id = host.read_with(cx, |host, _| {
            host.received_controls()
                .iter()
                .find_map(|(_, control)| match control {
                    Control::TerminalAttach {
                        terminal_id,
                        stream_id,
                    } if terminal_id == "terminal-1" => Some(*stream_id),
                    _ => None,
                })
                .expect("the other Zode is asked for the terminal")
        });

        let weak_session = session.downgrade();
        drop(watch);
        drop(view);
        drop(session);
        cx.run_until_parked();
        (
            MirrorTab {
                host,
                _initiator: initiator,
                workspace,
                session: weak_session,
                host_session_id,
                stream_id,
            },
            cx,
        )
    }

    impl MirrorTab {
        fn terminal_view(&self, cx: &mut VisualTestContext) -> Option<Entity<TerminalView>> {
            self.workspace.read_with(cx, |workspace, cx| {
                workspace
                    .active_pane()
                    .read(cx)
                    .items()
                    .find_map(|item| item.downcast::<TerminalView>())
            })
        }

        fn screen(&self, cx: &mut VisualTestContext) -> String {
            let view = self.terminal_view(cx).expect("the tab is open");
            view.read_with(cx, |view, cx| view.terminal().read(cx).get_content())
        }

        fn host_prints(&self, bytes: &[u8], cx: &mut VisualTestContext) {
            self.host.update(cx, |host, cx| {
                host.send_data(self.host_session_id, self.stream_id, bytes, cx)
            });
            cx.run_until_parked();
        }
    }

    #[gpui::test]
    async fn a_mirror_tab_outlives_the_dialog_it_was_opened_from(cx: &mut TestAppContext) {
        let (tab, cx) = open_a_mirror_tab_and_close_the_dialog(cx, "zsh").await;

        assert!(
            tab.session.upgrade().is_some(),
            "the tab owns the session its terminal lives on"
        );
        assert!(
            tab.terminal_view(cx).is_some(),
            "the tab is not closed as exited"
        );

        tab.host_prints(b"still here", cx);
        assert!(
            tab.screen(cx).contains("still here"),
            "what the other Zode prints still arrives"
        );
    }

    #[gpui::test]
    async fn the_session_is_released_when_the_other_zode_ends_the_stream(cx: &mut TestAppContext) {
        let (tab, cx) = open_a_mirror_tab_and_close_the_dialog(cx, "zsh").await;

        tab.host_prints(b"", cx);

        assert!(
            tab.session.upgrade().is_none(),
            "a stream that is over holds nothing open"
        );
    }

    #[gpui::test]
    async fn closing_the_tab_releases_the_session(cx: &mut TestAppContext) {
        let (tab, cx) = open_a_mirror_tab_and_close_the_dialog(cx, "zsh").await;

        let (pane, item_id) = tab.workspace.read_with(cx, |workspace, cx| {
            let pane = workspace.active_pane().clone();
            let item_id = pane
                .read(cx)
                .active_item()
                .expect("the mirror tab is active")
                .item_id();
            (pane, item_id)
        });
        pane.update_in(cx, |pane, window, cx| {
            pane.remove_item(item_id, false, true, window, cx)
        });
        cx.run_until_parked();
        // What the tab held is only let go of by the update that follows it.
        cx.update(|_, _| {});
        cx.run_until_parked();

        assert!(
            tab.host
                .read_with(cx, |host, _| host.received_controls().iter().any(
                    |(_, control)| matches!(control, Control::TerminalDetach { .. })
                )),
            "the other Zode is told the tab is gone"
        );
        assert!(
            tab.session.upgrade().is_none(),
            "nothing keeps the session once the tab is closed"
        );
    }

    #[gpui::test]
    async fn the_title_of_a_mirror_tab_is_made_fit_to_draw(cx: &mut TestAppContext) {
        let hostile = format!("zsh\u{202E}\nrm -rf{}", "x".repeat(500));
        let (tab, cx) = open_a_mirror_tab_and_close_the_dialog(cx, &hostile).await;

        let title = tab
            .terminal_view(cx)
            .expect("the tab is open")
            .read_with(cx, |view, cx| view.terminal().read(cx).title(false));
        assert!(
            title.chars().count() <= 2 * crate::relay_devices::MAX_DISPLAY_CHARS + 2,
            "the title was not clamped: {} characters",
            title.chars().count()
        );
        assert!(
            !title.chars().any(|character| character.is_control()
                || ('\u{202A}'..='\u{202E}').contains(&character)),
            "the title still carries characters that break or reorder the line"
        );
        assert!(title.starts_with("Studio: zshrm -rf"), "got {title:?}");
    }

    /// A terminal tab that is not a mirror, to tell "never recorded" from
    /// "this test never registered terminals as recordable".
    fn ordinary_terminal_tab(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) -> Entity<TerminalView> {
        let (commands, _unused) = futures::channel::mpsc::unbounded();
        let builder = terminal::TerminalBuilder::new_remote_backed(
            commands,
            terminal::RemoteBackedOptions::default(),
            Default::default(),
            settings::AlternateScroll::On,
            None,
            0,
            &cx.background_executor,
            util::paths::PathStyle::local(),
        )
        .expect("a terminal surface");
        let terminal = cx.update(|_, cx| cx.new(|cx| builder.subscribe(cx)));
        let project = workspace.read_with(cx, |workspace, _| workspace.project().downgrade());
        workspace.update_in(cx, |_, window, cx| {
            let weak_workspace = cx.weak_entity();
            cx.new(|cx| TerminalView::new(terminal, weak_workspace, None, project, window, cx))
        })
    }

    #[gpui::test]
    async fn a_mirror_tab_is_left_out_of_the_saved_layout(cx: &mut TestAppContext) {
        use workspace::ItemHandle as _;

        let (tab, cx) = open_a_mirror_tab_and_close_the_dialog(cx, "zsh").await;
        let mirror = tab.terminal_view(cx).expect("the tab is open");
        let ordinary = ordinary_terminal_tab(&tab.workspace, cx);

        cx.update(|_, cx| {
            assert!(
                ordinary.to_serializable_item_handle(cx).is_some(),
                "a terminal tab is ordinarily recorded"
            );
            assert!(
                mirror.to_serializable_item_handle(cx).is_none(),
                "a tab for a terminal on another Zode would come back as a local shell"
            );
        });
    }

    fn pairing_studio(view: &Entity<RelayDevices>, cx: &mut VisualTestContext) -> RelayHost {
        view.read_with(cx, |view, _| match &view.state {
            State::Hosts(hosts) => hosts[0].clone(),
            _ => panic!("expected the host list"),
        })
    }

    fn open_dialog(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) -> Entity<RelayDevices> {
        workspace.update_in(cx, |_, _, cx| {
            let weak_workspace = cx.weak_entity();
            cx.new(|cx| RelayDevices::new(weak_workspace, cx))
        })
    }

    fn connected_session(
        view: &Entity<RelayDevices>,
        cx: &mut VisualTestContext,
    ) -> Entity<RelaySession> {
        view.read_with(cx, |view, _| match &view.state {
            State::Host { session, .. } => session.clone(),
            _ => panic!("expected a connected host"),
        })
    }

    fn mirror_tabs(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> usize {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .active_pane()
                .read(cx)
                .items()
                .filter(|item| item.downcast::<TerminalView>().is_some())
                .count()
        })
    }

    #[gpui::test]
    async fn dialogs_opened_one_after_another_share_one_session_to_a_host(cx: &mut TestAppContext) {
        let (tab, cx) = open_a_mirror_tab_and_close_the_dialog(cx, "zsh").await;

        let second = open_dialog(&tab.workspace, cx);
        cx.run_until_parked();
        let studio = pairing_studio(&second, cx);
        second.update(cx, |view, cx| view.connect(studio, cx));
        cx.run_until_parked();

        let session = connected_session(&second, cx);
        assert_eq!(
            session.entity_id(),
            tab.session.entity_id(),
            "the host is already connected to; its session is used again"
        );
        second.update_in(cx, |view, window, cx| {
            let terminal = remote_relay_protocol::TerminalSummary {
                id: "terminal-2".into(),
                title: "bash".into(),
                columns: 80,
                rows: 24,
            };
            view.open_terminal(session.clone(), terminal, window, cx)
        });
        cx.run_until_parked();

        assert_eq!(mirror_tabs(&tab.workspace, cx), 2);
        assert_eq!(
            tab.host.read_with(cx, |host, _| host.session_ids().len()),
            1,
            "two tabs, one session: the host allows only a few"
        );
    }

    #[gpui::test]
    async fn a_session_nothing_holds_is_not_used_again(cx: &mut TestAppContext) {
        let (tab, cx) = open_a_mirror_tab_and_close_the_dialog(cx, "zsh").await;
        let closed_session = tab.session.entity_id();
        let (pane, item_id) = tab.workspace.read_with(cx, |workspace, cx| {
            let pane = workspace.active_pane().clone();
            let item_id = pane.read(cx).active_item().expect("the tab").item_id();
            (pane, item_id)
        });
        pane.update_in(cx, |pane, window, cx| {
            pane.remove_item(item_id, false, true, window, cx)
        });
        cx.run_until_parked();
        cx.update(|_, _| {});
        cx.run_until_parked();
        assert!(tab.session.upgrade().is_none(), "the tab let go of it");

        let second = open_dialog(&tab.workspace, cx);
        cx.run_until_parked();
        let studio = pairing_studio(&second, cx);
        second.update(cx, |view, cx| view.connect(studio, cx));
        cx.run_until_parked();

        let session = connected_session(&second, cx);
        assert_ne!(session.entity_id(), closed_session);
        assert!(!session.read_with(cx, |session, _| session.is_closed()));
    }

    #[gpui::test]
    async fn pairing_again_says_so_when_the_saved_trust_cannot_be_read(cx: &mut TestAppContext) {
        let app_state = cx.update(AppState::test);
        fresh_database(cx);
        let kvp = cx.update(|cx| db::kvp::KeyValueStore::global(cx));
        kvp.write_kvp("remote_control_hosts:user".into(), "{not json".into())
            .await
            .expect("a saved record");
        let (_host, _initiator) = relay_with_host_on_current_database(cx);
        let project = Project::test(app_state.fs.clone(), [], cx).await;
        let (workspace, cx) =
            cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
        let view = open_dialog(&workspace, cx);
        cx.run_until_parked();
        let studio = pairing_studio(&view, cx);

        view.update(cx, |view, cx| view.forget_and_pair(studio, cx));
        cx.run_until_parked();

        view.read_with(cx, |view, _| match &view.state {
            State::Problem(message) => assert!(
                message.contains("could not be read"),
                "the reason is shown, got {message:?}"
            ),
            _ => panic!("a forget that was refused must not go on to pair"),
        });
    }

    #[gpui::test]
    async fn pairing_again_says_so_when_the_forget_could_not_be_saved(cx: &mut TestAppContext) {
        let app_state = cx.update(AppState::test);
        let (host, _initiator) = relay_with_host(cx);
        let project = Project::test(app_state.fs.clone(), [], cx).await;
        let (workspace, cx) =
            cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
        let view = open_dialog(&workspace, cx);
        let _session = pair_and_connect(&view, &host, cx);

        let connection = cx.update(|_, cx| db::AppDatabase::global(cx).clone());
        connection
            .write(|connection| {
                connection
                    .exec("DROP TABLE kv_store")
                    .and_then(|mut statement| statement())
            })
            .await
            .expect("the table to go");
        let studio = RelayHost {
            device_id: "host-device".into(),
            name: "Studio".into(),
            public_key: None,
            online: true,
            paired: true,
            key_changed: true,
        };

        view.update(cx, |view, cx| view.forget_and_pair(studio, cx));
        cx.run_until_parked();

        view.read_with(cx, |view, _| match &view.state {
            State::Problem(message) => assert!(
                message.contains("could not be saved"),
                "the reason is shown, got {message:?}"
            ),
            _ => panic!("a forget that did not reach the database must not go on to pair"),
        });
    }
}
