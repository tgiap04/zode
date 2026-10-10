//! What the other-Zodes dialog draws for each of its states.

use gpui::{Entity, SharedString, Window};
use release_channel::AppVersion;
use remote::RelayConnectionOptions;
use remote_relay_client::{PairingAttempt, PairingFailure, PairingStep, RelayHost, RelaySession};
use remote_relay_protocol::{AgentStatus, AgentSummary, TerminalSummary};
use ui::{Divider, Modal, ModalFooter, ModalHeader, Section, prelude::*};

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
                Button::new(SharedString::from(format!("repair-{id}")), "Pair again").on_click(
                    cx.listener(move |this, _, _, cx| this.forget_and_pair(host.clone(), cx)),
                )
            } else if host.paired {
                Button::new(SharedString::from(format!("connect-{id}")), "Connect")
                    .style(ButtonStyle::Filled)
                    .disabled(!host.online)
                    .on_click(cx.listener(move |this, _, _, cx| this.connect(host.clone(), cx)))
            } else {
                Button::new(SharedString::from(format!("pair-{id}")), "Pair…")
                    .disabled(!host.online || host.public_key.is_none())
                    .on_click(cx.listener(move |this, _, _, cx| this.pair(host.clone(), cx)))
            }
        };
        h_flex()
            .w_full()
            .px_3()
            .py_2()
            .gap_2()
            .justify_between()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Icon::new(IconName::Circle)
                            .size(IconSize::XSmall)
                            .color(dot),
                    )
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
                    ),
            )
            .child(action)
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
                .gap_3()
                .child(body(format!(
                    "{name} should be showing the same six digits. Only continue if they are \
                     identical."
                )))
                .child(Headline::new(digits).size(HeadlineSize::Large))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("digits-match", "The digits match")
                                .style(ButtonStyle::Filled)
                                .on_click({
                                    let attempt = attempt.clone();
                                    move |_, _, cx| {
                                        attempt.update(cx, |attempt, cx| attempt.confirm_digits(cx))
                                    }
                                }),
                        )
                        .child(Button::new("digits-differ", "Digits differ").on_click({
                            let attempt = attempt.clone();
                            move |_, _, cx| {
                                attempt.update(cx, |attempt, cx| attempt.digits_differ(cx))
                            }
                        })),
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
            .gap_2()
            .p_3()
            .child(
                Button::new("open-project", format!("Open a project on {name}"))
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
            }))
            .child(Divider::horizontal())
            .child(
                Label::new("Agents")
                    .color(Color::Muted)
                    .size(LabelSize::Small),
            )
            .children(self.agent_rows(session, &agents, &terminals, cx))
            .when(agents.is_empty(), |this| {
                this.child(Label::new("None running").color(Color::Muted))
            })
            .child(
                Label::new("Terminals")
                    .color(Color::Muted)
                    .size(LabelSize::Small),
            )
            .children(terminals.iter().map(|terminal| {
                self.terminal_row(session, terminal.clone(), cx)
                    .into_any_element()
            }))
            .when(terminals.is_empty(), |this| {
                this.child(Label::new("None open").color(Color::Muted))
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
                    h_flex()
                        .justify_between()
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
                        .child(
                            Button::new(SharedString::from(format!("agent-{}", agent.id)), "Open")
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
        h_flex()
            .justify_between()
            .child(Label::new(terminal.title.clone()))
            .child(
                Button::new(
                    SharedString::from(format!("terminal-{}", terminal.id)),
                    "Open",
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_terminal(session.clone(), terminal.clone(), window, cx)
                })),
            )
    }
}

impl Render for RelayDevices {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.state {
            State::Loading => Label::new("Looking for your other Zodes…")
                .color(Color::Muted)
                .into_any_element(),
            State::Hosts(hosts) if hosts.is_empty() => v_flex()
                .p_3()
                .gap_1()
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
                v_flex()
                    .children(hosts.iter().map(|host| self.render_host_row(host, cx)))
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
