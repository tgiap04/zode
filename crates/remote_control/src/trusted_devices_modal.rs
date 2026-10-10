//! The devices allowed to control this Zode, and how to take that away.

use gpui::{
    App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Subscription,
    Window,
};
use ui::prelude::*;
use workspace::ModalView;

use crate::remote_host::RemoteHost;

pub struct TrustedDevicesModal {
    host: Entity<RemoteHost>,
    focus_handle: FocusHandle,
    _observation: Subscription,
}

impl EventEmitter<DismissEvent> for TrustedDevicesModal {}
impl ModalView for TrustedDevicesModal {}

impl Focusable for TrustedDevicesModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TrustedDevicesModal {
    pub fn new(host: Entity<RemoteHost>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let observation = cx.observe(&host, |_, _, cx| cx.notify());
        Self {
            host,
            focus_handle: cx.focus_handle(),
            _observation: observation,
        }
    }
}

impl Render for TrustedDevicesModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let host = self.host.read(cx);
        let devices = host.trusted_devices();
        let locked = host.pairing_locked_out();
        let connected = !host.sessions().is_empty();

        let mut list = v_flex().gap_1();
        if devices.is_empty() {
            list = list.child(
                Label::new("No device is trusted. Pair one from the Zode web app.")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            );
        }
        for device in devices {
            let host = self.host.clone();
            let device_id = device.device_id.clone();
            list = list.child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(Label::new(device.name.clone()))
                    .child(
                        Button::new(
                            SharedString::from(format!("forget-{}", device.device_id)),
                            "Forget",
                        )
                        .label_size(LabelSize::Small)
                        .on_click(move |_, _window, cx| {
                            host.update(cx, |host, cx| host.forget_device(&device_id, cx));
                        }),
                    ),
            );
        }

        v_flex()
            .key_context("RemoteControlDevices")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .w(rems(30.))
            .p_4()
            .gap_3()
            .on_action(cx.listener(|_, _: &menu::Cancel, _window, cx| cx.emit(DismissEvent)))
            .child(Label::new("Devices that can control this Zode").weight(FontWeight::MEDIUM))
            .child(list)
            .when(locked, |this| {
                let host = self.host.clone();
                this.child(
                    v_flex()
                        .gap_1()
                        .child(
                            Label::new(
                                "Pairing is locked after repeated mismatches. It stays locked \
                                 until you unlock it here.",
                            )
                            .size(LabelSize::Small)
                            .color(Color::Warning),
                        )
                        .child(
                            Button::new("remote-unlock-pairing", "Unlock pairing")
                                .label_size(LabelSize::Small)
                                .on_click(move |_, _window, cx| {
                                    host.update(cx, |host, cx| host.unlock_pairing(cx));
                                }),
                        ),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .when(connected, |this| {
                        let host = self.host.clone();
                        this.child(
                            Button::new("remote-disconnect-all", "Disconnect all")
                                .label_size(LabelSize::Small)
                                .on_click(move |_, _window, cx| {
                                    host.update(cx, |host, cx| host.disconnect_all(cx));
                                }),
                        )
                    })
                    .child(
                        Button::new("remote-devices-close", "Close")
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(|_, _, _window, cx| cx.emit(DismissEvent))),
                    ),
            )
    }
}
