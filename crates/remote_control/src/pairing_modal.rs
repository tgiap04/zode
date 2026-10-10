//! The question put to the person at this screen when another device asks to
//! be trusted: here are six digits, are they the same over there?

use gpui::{
    App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Subscription,
    Window,
};
use ui::prelude::*;
use workspace::ModalView;

use crate::{pairing_flow::PendingDecision, remote_host::RemoteHost};

pub struct PairingModal {
    host: Entity<RemoteHost>,
    session_id: u32,
    device_name: String,
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
        // A question dismissed without being answered is answered no.
        let release = cx.on_release({
            let host = host.clone();
            move |this, cx| {
                if !this.answered {
                    host.update(cx, |host, cx| {
                        host.reject_pairing_if_pending(session_id, cx)
                    });
                }
            }
        });
        Self {
            host,
            session_id,
            device_name: decision.peer_name,
            code: decision.code,
            answered: false,
            focus_handle: cx.focus_handle(),
            _observation: observation,
            _release: release,
        }
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
        v_flex()
            .key_context("RemoteControlPairing")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .w(rems(28.))
            .p_4()
            .gap_3()
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.answer(false, cx)))
            .child(
                Label::new(format!("{} wants to control this Zode", self.device_name))
                    .weight(FontWeight::MEDIUM),
            )
            .child(
                Label::new(
                    "Look at the other device. If it shows the same six digits, it is the one \
                     you meant. Trusting it lets it watch your agents and terminals and type \
                     into them.",
                )
                .size(LabelSize::Small)
                .color(Color::Muted),
            )
            .child(
                h_flex().justify_center().py_2().child(
                    Label::new(self.code.clone())
                        .size(LabelSize::Large)
                        .weight(FontWeight::BOLD)
                        .buffer_font(cx),
                ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("remote-pairing-reject", "Reject")
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(|this, _, _window, cx| this.answer(false, cx))),
                    )
                    .child(
                        Button::new("remote-pairing-trust", "The digits match: trust it")
                            .style(ButtonStyle::Filled)
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(|this, _, _window, cx| this.answer(true, cx))),
                    ),
            )
    }
}
