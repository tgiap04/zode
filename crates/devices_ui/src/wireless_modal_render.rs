use gpui::{ClickEvent, Context, Entity, Window};
use ui::{Modal, ModalFooter, ModalHeader, Section, prelude::*};
use ui_input::InputField;

use crate::wireless_modal::{Step, WirelessModal};

impl WirelessModal {
    fn field(label: &'static str, input: &Entity<InputField>) -> impl IntoElement {
        v_flex()
            .gap_1()
            .child(Label::new(label).size(LabelSize::Small).color(Color::Muted))
            .child(input.clone())
    }
}

impl Render for WirelessModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let working = self.working.is_some();
        let (title, explanation, body) = match self.step {
            Step::Pair => (
                "Pair a phone",
                "On the phone, open Developer options, then Wireless debugging, then \
                 \"Pair device with pairing code\". Enter the address and the code shown there. \
                 Only addresses on your own network are accepted.",
                v_flex()
                    .gap_2()
                    .child(Self::field("Pairing address", &self.pair_address))
                    .child(Self::field("Pairing code", &self.pair_code)),
            ),
            Step::Connect => (
                "Connect a phone",
                "Enter the address shown on the main Wireless debugging screen. Its port is \
                 not the one used for pairing.",
                v_flex()
                    .gap_2()
                    .child(Self::field("Address", &self.connect_address)),
            ),
        };
        let step_buttons =
            match self.step {
                Step::Pair => h_flex()
                    .gap_1()
                    .child(
                        Button::new("devices-wireless-skip", "Already paired")
                            .disabled(working)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.skip_pairing(window, cx)
                            })),
                    )
                    .child(
                        Button::new("devices-wireless-pair", "Pair")
                            .style(ButtonStyle::Filled)
                            .disabled(working)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.pair(window, cx)
                            })),
                    ),
                Step::Connect => h_flex()
                    .gap_1()
                    .child(
                        Button::new("devices-wireless-back", "Back")
                            .disabled(working)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.go_back(window, cx)
                            })),
                    )
                    .child(
                        Button::new("devices-wireless-connect", "Connect")
                            .style(ButtonStyle::Filled)
                            .disabled(working)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.connect(window, cx)
                            })),
                    ),
            };
        div()
            .key_context("WirelessModal")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| this.submit(window, cx)))
            .on_action(cx.listener(|this, _: &menu::Cancel, window, cx| this.cancel(window, cx)))
            .elevation_3(cx)
            .occlude()
            .debug_selector(|| "devices-wireless".to_string())
            .w(rems(28.))
            .child(
                Modal::new("devices-wireless", None)
                    .header(ModalHeader::new().headline(title))
                    .section(
                        Section::new().child(
                            v_flex()
                                .w_full()
                                .gap_2()
                                .when(self.paired, |column| {
                                    column.child(
                                        Label::new("Paired. Now connect.")
                                            .size(LabelSize::Small)
                                            .color(Color::Success),
                                    )
                                })
                                .child(
                                    Label::new(explanation)
                                        .size(LabelSize::Small)
                                        .color(Color::Muted),
                                )
                                .child(body)
                                .when_some(self.error.clone(), |column, error| {
                                    column.child(
                                        div()
                                            .debug_selector(|| "devices-wireless-error".to_string())
                                            .child(
                                                Label::new(error)
                                                    .size(LabelSize::Small)
                                                    .color(Color::Error),
                                            ),
                                    )
                                }),
                        ),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_1()
                                .child(Button::new("devices-wireless-cancel", "Cancel").on_click(
                                    cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.cancel(window, cx)
                                    }),
                                ))
                                .child(step_buttons),
                        ),
                    ),
            )
    }
}
