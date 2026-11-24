//! The question put to the person at this screen when another device asks to
//! be trusted: here are six digits, are they the same over there?

use gpui::{
    App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Subscription,
    Window,
};
use ui::{Modal, ModalFooter, ModalHeader, Section, prelude::*};
use workspace::ModalView;

use crate::{pairing_flow::PendingDecision, remote_host::RemoteHost};

pub struct PairingModal {
    host: Entity<RemoteHost>,
    session_id: u32,
    device_name: String,
    /// What the device is already trusted as, when trusting it again would
    /// replace what is saved for it.
    replaces: Option<String>,
    /// The device read the QR code: there are no digits, and the question is
    /// only whether to replace the saved key.
    via_qr: bool,
    code: String,
    answered: bool,
    focus_handle: FocusHandle,
    _observation: Subscription,
    _release: Subscription,
}

impl EventEmitter<DismissEvent> for PairingModal {}
impl ModalView for PairingModal {}

impl Focusable for PairingModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl PairingModal {
    pub fn new(
        host: Entity<RemoteHost>,
        decision: PendingDecision,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let session_id = decision.session_id;
        // Closes itself once the question is no longer being asked: answered
        // from somewhere else, or run out of time.
        let observation = cx.observe(&host, move |this, host, cx| {
            let still_asked = host
                .read(cx)
                .pending_pairing()
                .is_some_and(|pending| pending.session_id == session_id);
            if !still_asked {
                this.answered = true;
                cx.emit(DismissEvent);
            }
        });
        // A question closed without being answered turns the device away. The
        // digits were not compared, so it is not a mismatch: closing this
        // because another window or modal took its place must not push the
        // person toward the lockout.
        let release = cx.on_release({
            let host = host.clone();
            move |this, cx| {
                if !this.answered {
                    host.update(cx, |host, cx| {
                        host.dismiss_pairing_if_pending(session_id, cx)
                    });
                }
            }
        });
        Self {
            host,
            session_id,
            device_name: decision.peer_name,
            replaces: decision.replaces,
            via_qr: decision.via_qr,
            code: decision.code,
            answered: false,
            focus_handle: cx.focus_handle(),
            _observation: observation,
            _release: release,
        }
    }

    pub(crate) fn session_id(&self) -> u32 {
        self.session_id
    }

    fn answer(&mut self, trust: bool, cx: &mut Context<Self>) {
        self.answered = true;
        let session_id = self.session_id;
        self.host.update(cx, |host, cx| {
            if host
                .pending_pairing()
                .is_some_and(|pending| pending.session_id == session_id)
            {
                host.decide_pairing(session_id, trust, cx);
            }
        });
        cx.emit(DismissEvent);
    }
}

impl Render for PairingModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let via_qr = self.via_qr;
        let (title, trust_label, reject_label) = if via_qr {
            (
                format!(
                    "Replace the key for {}?",
                    self.replaces.as_deref().unwrap_or(&self.device_name)
                ),
                "Trust It",
                "Don't Trust",
            )
        } else {
            (
                format!("{} wants to control this Zode", self.device_name),
                "The Digits Match: Trust It",
                "Reject",
            )
        };
        div()
            .key_context("RemoteControlPairing")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .occlude()
            .w(rems(28.))
            .max_h(rems(40.))
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.answer(false, cx)))
            .child(
                Modal::new("remote-pairing", None)
                    .header(
                        ModalHeader::new().child(Headline::new(title).size(HeadlineSize::Small)),
                    )
                    .section(
                        Section::new()
                            .when(via_qr, |section| {
                                section.child(
                                    div()
                                        .debug_selector(|| "remote-pairing-replace-question".into())
                                        .child(
                                            Label::new(format!(
                                                "{} was re-paired with a QR code and is now using \
                                                 a different key. Trusting it replaces what is \
                                                 saved for it and ends any connection made with \
                                                 the old key.",
                                                self.device_name
                                            ))
                                            .size(LabelSize::Small),
                                        ),
                                )
                            })
                            .when(!via_qr, |section| {
                                section
                                    .when_some(self.replaces.clone(), |section, existing| {
                                        section.child(
                                            Label::new(format!(
                                                "{existing} is already trusted. Trusting this \
                                                 request replaces what is saved for it and ends \
                                                 any connection made with the old key."
                                            ))
                                            .size(LabelSize::Small)
                                            .color(Color::Warning),
                                        )
                                    })
                                    .child(
                                        Label::new(
                                            "Look at the other device. If it shows the same six \
                                             digits, it is the one you meant. Trusting it lets it \
                                             watch your agents and terminals and type into them.",
                                        )
                                        .size(LabelSize::Small)
                                        .color(Color::Muted),
                                    )
                                    .child(
                                        v_flex()
                                            .debug_selector(|| "remote-pairing-digits".into())
                                            .w_full()
                                            .py_4()
                                            .gap_1p5()
                                            .items_center()
                                            .child(
                                                Label::new(self.code.clone())
                                                    .size(LabelSize::Large)
                                                    .weight(FontWeight::MEDIUM)
                                                    .buffer_font(cx),
                                            ),
                                    )
                            }),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_1()
                                // `Button` has no probe of its own, so each one is
                                // wrapped for a test to find its bounds and click it.
                                .child(
                                    div()
                                        .debug_selector(|| "remote-pairing-reject".into())
                                        .child(
                                            Button::new("remote-pairing-reject", reject_label)
                                                .on_click(cx.listener(|this, _, _window, cx| {
                                                    this.answer(false, cx)
                                                })),
                                        ),
                                )
                                .child(
                                    div()
                                        .debug_selector(|| "remote-pairing-trust".into())
                                        .child(
                                            Button::new("remote-pairing-trust", trust_label)
                                                .style(ButtonStyle::Filled)
                                                .on_click(cx.listener(|this, _, _window, cx| {
                                                    this.answer(true, cx)
                                                })),
                                        ),
                                ),
                        ),
                    ),
            )
    }
}
