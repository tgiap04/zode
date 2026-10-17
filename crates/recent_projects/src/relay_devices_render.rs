//! What the other-Zodes dialog draws for each of its states.

use gpui::{Entity, FontWeight, SharedString, Window};
use release_channel::AppVersion;
use remote::RelayConnectionOptions;
use remote_relay_client::{PairingAttempt, PairingFailure, PairingStep, RelayHost, RelaySession};
use remote_relay_protocol::{AgentStatus, AgentSummary, TerminalSummary};
use ui::{
    Divider, Indicator, ListItem, ListItemSpacing, ListSubHeader, Modal, ModalFooter, ModalHeader,
    Section, prelude::*,
};

use crate::relay_devices::{RelayDevices, RelayDevicesEvent, State};

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
                v_flex().child(Label::new(host.name.clone())).child(
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
        let (name, host_version, can_open, agents, terminals) = {
            let session = session.read(cx);
            (
                session.host_name().to_string(),
                session.host().app_version.clone(),
                session.host().can("ide"),
                session.agents().to_vec(),
                session.terminals().to_vec(),
            )
        };
        let local_version = AppVersion::global(cx).to_string();
        let same_version = local_version == host_version;

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
            .child(ListSubHeader::new("Agents").inset(true))
            .children(self.agent_rows(session, &agents, &terminals, cx))
            .when(agents.is_empty(), |this| {
                this.child(empty_note("None running"))
            })
            .child(ListSubHeader::new("Terminals").inset(true))
            .children(terminals.iter().map(|terminal| {
                self.terminal_row(session, terminal.clone(), cx)
                    .into_any_element()
            }))
            .when(terminals.is_empty(), |this| {
                this.child(empty_note("None open"))
            })
            .into_any_element()
    }

    fn agent_rows(
        &self,
        session: &Entity<RelaySession>,
        agents: &[AgentSummary],
        terminals: &[TerminalSummary],
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        agents
            .iter()
            .filter_map(|agent| {
                // An agent is attached through its terminal, which carries the
                // same id; one with no terminal has nothing to show yet.
                let terminal = terminals.iter().find(|terminal| terminal.id == agent.id)?;
                let label = agent.title.clone().unwrap_or_else(|| agent.name.clone());
                let session = session.clone();
                let terminal = terminal.clone();
                Some(
                    ListItem::new(SharedString::from(format!("relay-agent-{}", agent.id)))
                        .inset(true)
                        .spacing(ListItemSpacing::Sparse)
                        .selectable(false)
                        .child(
                            v_flex().child(Label::new(label)).child(
                                Label::new(format!(
                                    "{} · {}",
                                    agent.name,
                                    agent_status_label(agent.status)
                                ))
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                            ),
                        )
                        .end_slot(
                            Button::new(SharedString::from(format!("agent-{}", agent.id)), "Open")
                                .label_size(LabelSize::Small)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_terminal(
                                        session.clone(),
                                        terminal.clone(),
                                        window,
                                        cx,
                                    )
                                })),
                        )
                        .into_any_element(),
                )
            })
            .collect()
    }

    fn terminal_row(
        &self,
        session: &Entity<RelaySession>,
        terminal: TerminalSummary,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let session = session.clone();
        ListItem::new(SharedString::from(format!(
            "relay-terminal-{}",
            terminal.id
        )))
        .inset(true)
        .spacing(ListItemSpacing::Sparse)
        .selectable(false)
        .child(Label::new(terminal.title.clone()))
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
                let hosts = hosts.clone();
                // Bounded so a long list scrolls inside the dialog instead of
                // pushing the footer out of it.
                div()
                    .id("relay-hosts-list")
                    .debug_selector(|| "relay-hosts-list".into())
                    .w_full()
                    .max_h(rems(22.))
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
                .child(Label::new(format!("Connecting to {}…", host.name)).color(Color::Muted))
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
    use gpui::{Modifiers, TestAppContext, VisualTestContext, WeakEntity};
    use http_client::{AsyncBody, FakeHttpClient, Response};
    use remote_relay_client::{
        RelayEnvironment, RelayInitiator,
        scripted_host::ScriptedHost,
        test_support::{FakeRelay, InMemoryKeychain, StaticCredentials},
    };
    use remote_relay_protocol::DeviceKeypair;
    use std::sync::Arc;

    /// An initiator whose account directory lists `rows`, registered as the
    /// global the dialog reaches for.
    fn initiator_listing(
        relay: &FakeRelay,
        rows: serde_json::Value,
        cx: &mut TestAppContext,
    ) -> Entity<RelayInitiator> {
        cx.update(|cx| cx.set_global(db::AppDatabase::test_new()));
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
}
