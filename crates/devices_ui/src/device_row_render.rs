use devices::{Device, DeviceKey, DeviceKind, DeviceState};
use gpui::{AnyElement, Context};
use ui::{Indicator, Tooltip, prelude::*};

use crate::device_actions::offers;
use crate::device_operations::OperationKind;
use crate::devices_panel::DevicesPanel;
use crate::physical_device_guidance::guidance_line;

const DOCS_URL: &str = "https://zodekit.com/docs/devices";

fn row_id(key: &DeviceKey) -> String {
    match key {
        DeviceKey::Simulator(id) => format!("sim-{id}"),
        DeviceKey::Avd(name) => format!("avd-{name}"),
        DeviceKey::AndroidSerial(serial) => format!("android-{serial}"),
        DeviceKey::IosDevice(id) => format!("ios-{id}"),
    }
}

impl DevicesPanel {
    pub(crate) fn render_device_row(&self, device: &Device, cx: &mut Context<Self>) -> AnyElement {
        let id = row_id(&device.key);
        let dot = match &device.state {
            DeviceState::Running => Color::Success,
            DeviceState::Starting | DeviceState::Stopping | DeviceState::Unusable(_) => {
                Color::Warning
            }
            DeviceState::Shutdown => Color::Muted,
        };
        let icon = if device.family.as_deref() == Some("iPad") {
            IconName::Screen
        } else {
            IconName::Smartphone
        };
        let connection = match device.kind {
            DeviceKind::AndroidUsb => Some("USB".to_string()),
            DeviceKind::AndroidWireless => Some("Wireless".to_string()),
            _ => None,
        };
        let unusable = match &device.state {
            DeviceState::Unusable(reason) => Some(reason.clone()),
            _ => None,
        };
        let detail = [device.detail.clone(), connection, unusable]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
        let error = self.device_errors.get(&device.key).cloned();
        let has_stray = self.stray_emulators.contains_key(&device.key);
        let name_row = h_flex()
            .gap_1()
            .child(Label::new(device.name.clone()).truncate())
            .when(device.experimental, |row| {
                row.child(
                    Label::new("Experimental")
                        .size(LabelSize::XSmall)
                        .color(Color::Warning),
                )
            });
        v_flex()
            .id(SharedString::from(id.clone()))
            .debug_selector({
                let id = id.clone();
                move || id
            })
            .w_full()
            .px_3()
            .py_1()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(Indicator::dot().color(dot))
                    .child(Icon::new(icon).size(IconSize::Small).color(Color::Muted))
                    .child(v_flex().flex_1().min_w_0().child(name_row).when(
                        !detail.is_empty(),
                        |column| {
                            column.child(
                                Label::new(detail)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                        },
                    ))
                    .child(self.render_actions(device, &id, cx)),
            )
            .when_some(guidance_line(device, &id), |row, guidance| {
                row.child(guidance)
            })
            .when_some(self.render_app_choice(device, &id, cx), |row, choice| {
                row.child(choice)
            })
            .when_some(error, |row, error| {
                row.child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            div()
                                .id(SharedString::from(format!("{id}-error")))
                                .debug_selector({
                                    let id = id.clone();
                                    move || format!("{id}-error")
                                })
                                .flex_1()
                                .min_w_0()
                                .child(
                                    Label::new(error.message.clone())
                                        .size(LabelSize::Small)
                                        .color(Color::Error),
                                ),
                        )
                        .when_some(error.log_file, |line, log_file| {
                            line.child(
                                Button::new(SharedString::from(format!("{id}-log")), "Open log")
                                    .label_size(LabelSize::Small)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_log(log_file.clone(), window, cx)
                                    })),
                            )
                        })
                        .when(has_stray, |line| {
                            let key = device.key.clone();
                            let element_id = format!("{id}-stop-process");
                            line.child(
                                div()
                                    .debug_selector({
                                        let element_id = element_id.clone();
                                        move || element_id
                                    })
                                    .child(
                                        Button::new(
                                            SharedString::from(element_id),
                                            "Stop emulator",
                                        )
                                        .label_size(LabelSize::Small)
                                        .on_click(
                                            cx.listener(move |this, _, _window, cx| {
                                                this.stop_stray_emulator(&key, cx)
                                            }),
                                        ),
                                    ),
                            )
                        }),
                )
            })
            .into_any_element()
    }

    fn render_actions(&self, device: &Device, id: &str, cx: &mut Context<Self>) -> AnyElement {
        let key = device.key.clone();
        let busy = self.operations.contains_key(&key);
        let mut actions = h_flex().gap_1();
        let button =
            |kind: OperationKind, icon: IconName, tooltip: &'static str, suffix: &'static str| {
                let key = key.clone();
                let element_id = format!("{id}-{suffix}");
                div()
                    .debug_selector({
                        let element_id = element_id.clone();
                        move || element_id
                    })
                    .child(
                        IconButton::new(SharedString::from(element_id), icon)
                            .icon_size(IconSize::Small)
                            .tooltip(Tooltip::text(tooltip))
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _window, cx| match kind {
                                OperationKind::Boot => this.boot_device(&key, cx),
                                OperationKind::Shutdown => this.shutdown_device(&key, cx),
                                OperationKind::ShowWindow => this.show_simulator_window(&key, cx),
                            })),
                    )
            };
        if offers(&OperationKind::Boot, device) {
            actions = actions.child(button(OperationKind::Boot, IconName::Power, "Boot", "boot"));
        }
        if offers(&OperationKind::ShowWindow, device) {
            actions = actions.child(button(
                OperationKind::ShowWindow,
                IconName::Maximize,
                "Show window",
                "window",
            ));
        }
        if offers(&OperationKind::Shutdown, device) {
            actions = actions.child(button(
                OperationKind::Shutdown,
                IconName::Stop,
                "Shut down",
                "shutdown",
            ));
        }
        if let Some(run) = self.render_run_button(device, id, cx) {
            actions = actions.child(run);
        }
        match device.state {
            DeviceState::Starting => actions.child(busy_label("Starting…")),
            DeviceState::Stopping => actions.child(busy_label("Stopping…")),
            _ => actions,
        }
        .into_any_element()
    }
}

fn busy_label(text: &'static str) -> impl IntoElement {
    Label::new(text).size(LabelSize::Small).color(Color::Muted)
}

/// Names what is missing and what to do about it. Zode never installs
/// anything, so there is no install button, only the docs.
pub(crate) fn missing_tool(
    group: &'static str,
    tool: &str,
    hint: &str,
    cx: &mut Context<DevicesPanel>,
) -> AnyElement {
    v_flex()
        .id(SharedString::from(format!("devices-missing-{group}")))
        .px_3()
        .pb_1()
        .gap_1()
        .child(Label::new(format!("{tool} is not set up")).size(LabelSize::Small))
        .child(
            Label::new(hint.to_string())
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
        .child(
            Button::new(
                SharedString::from(format!("devices-docs-{group}")),
                "Open setup guide",
            )
            .label_size(LabelSize::Small)
            .on_click(cx.listener(|_, _, _window, cx| cx.open_url(DOCS_URL))),
        )
        .into_any_element()
}
