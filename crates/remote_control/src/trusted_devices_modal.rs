//! The devices allowed to control this Zode, and how to take that away.

use gpui::{App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Subscription, Window};
use ui::{ListItem, ListItemSpacing, Modal, ModalFooter, ModalHeader, Section, prelude::*};
use workspace::ModalView;

use crate::remote_host::{RemoteHost, TrustedDevices};

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
        // The devices are the account's, not the connection's: they are listed
        // and can be forgotten while remote control is off or has stopped. The
        // host lets go of what it read when it starts or the account changes, so
        // whatever it says next, they are asked for again if they are missing.
        let observation = cx.observe(&host, |_, host, cx| {
            host.update(cx, |host, cx| host.load_trusted_devices(cx));
            cx.notify();
        });
        host.update(cx, |host, cx| host.load_trusted_devices(cx));
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
        let locked = host.pairing_locked_out();
        let connected = !host.sessions().is_empty();
        let running = host.has_relay_client();
        let (devices, unreadable, notice) = match host.trusted_devices_listing(cx) {
            TrustedDevices::SignedOut => (
                Vec::new(),
                false,
                Some("Sign in to a Zode account to see the devices it trusts."),
            ),
            TrustedDevices::Loading => (
                Vec::new(),
                false,
                Some("Loading the trusted devices\u{2026}"),
            ),
            TrustedDevices::Listed {
                devices,
                unreadable,
            } => {
                let notice = devices
                    .is_empty()
                    .then_some("No device is trusted. Pair one from the Zode web app.");
                (devices, unreadable, notice)
            }
        };

        let listed = if let Some(notice) = notice {
            v_flex().child(
                h_flex().px_2().py_1p5().child(
                    Label::new(notice)
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
        let rows = listed
            .when(unreadable, |rows| {
                rows.child(
                    h_flex().px_2().py_1p5().child(
                        Label::new(
                            "Some saved devices could not be read, so more devices may be \
                         trusted than are listed.",
                        )
                        .size(LabelSize::Small)
                        .color(Color::Warning),
                    ),
                )
            })
            .when(!running && !devices.is_empty(), |rows| {
                rows.child(
                    h_flex().px_2().py_1p5().child(
                        Label::new(
                            "Remote control is not running; devices forgotten here stay forgotten.",
                        )
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                    ),
                )
            });
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
