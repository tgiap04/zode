//! The devices allowed to control this Zode, and how to take that away.

use gpui::{App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Subscription, Window};
use ui::{ListItem, ListItemSpacing, Modal, ModalFooter, ModalHeader, Section, prelude::*};
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

        let rows = if devices.is_empty() {
            v_flex().child(
                h_flex().px_2().py_1p5().child(
                    Label::new("No device is trusted. Pair one from the Zode web app.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                ),
            )
        } else {
            v_flex().children(devices.iter().map(|device| {
                let host = self.host.clone();
                let device_id = device.device_id.clone();
                ListItem::new(SharedString::from(format!(
                    "trusted-device-{}",
                    device.device_id
                )))
                .inset(true)
                .spacing(ListItemSpacing::Sparse)
                .selectable(false)
                .child(Label::new(device.name.clone()))
                .end_slot(
                    Button::new(
                        SharedString::from(format!("forget-{}", device.device_id)),
                        "Forget",
                    )
                    .label_size(LabelSize::Small)
                    .on_click(move |_, _window, cx| {
                        host.update(cx, |host, cx| host.forget_device(&device_id, cx));
                    }),
                )
            }))
        };
        // Bounded so a long list scrolls inside the card instead of pushing the
        // footer out of it.
        let list = div()
            .id("remote-trusted-devices-list")
            .debug_selector(|| "remote-trusted-devices-list".into())
            .w_full()
            .max_h(rems(22.))
            .overflow_y_scroll()
            .child(rows);

        div()
            .key_context("RemoteControlDevices")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .occlude()
            .w(rems(40.))
            .max_h(rems(40.))
            .on_action(cx.listener(|_, _: &menu::Cancel, _window, cx| cx.emit(DismissEvent)))
            .child(
                Modal::new("remote-devices", None)
                    .header(
                        ModalHeader::new().child(
                            Headline::new("Devices that can control this Zode")
                                .size(HeadlineSize::Small),
                        ),
                    )
                    .section(Section::new().padded(false).child(list))
                    .when(locked, |modal| {
                        let host = self.host.clone();
                        modal.section(
                            Section::new().child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        h_flex()
                                            .gap_1p5()
                                            .items_center()
                                            .child(
                                                Icon::new(IconName::Warning)
                                                    .size(IconSize::XSmall)
                                                    .color(Color::Warning),
                                            )
                                            .child(
                                                Label::new(
                                                    "Pairing is locked after repeated \
                                                     mismatches. It stays locked until you \
                                                     unlock it here.",
                                                )
                                                .size(LabelSize::Small)
                                                .color(Color::Warning),
                                            ),
                                    )
                                    .child(
                                        Button::new("remote-unlock-pairing", "Unlock Pairing")
                                            .on_click(move |_, _window, cx| {
                                                host.update(cx, |host, cx| host.unlock_pairing(cx));
                                            }),
                                    ),
                            ),
                        )
                    })
                    .footer(
                        ModalFooter::new()
                            .start_slot::<Button>(connected.then(|| {
                                let host = self.host.clone();
                                Button::new("remote-disconnect-all", "Disconnect All").on_click(
                                    move |_, _window, cx| {
                                        host.update(cx, |host, cx| host.disconnect_all(cx));
                                    },
                                )
                            }))
                            .end_slot(
                                Button::new("remote-devices-close", "Close").on_click(
                                    cx.listener(|_, _, _window, cx| cx.emit(DismissEvent)),
                                ),
                            ),
                    ),
            )
    }
}
