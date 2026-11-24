use devices::SectionState;
use gpui::{AnyElement, Context, Window};
use ui::prelude::*;

use crate::device_row_render::missing_tool;
use crate::device_snapshot::{SectionId, memory_line};
use crate::devices_panel::DevicesPanel;

impl Render for DevicesPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("DevicesPanel")
            .track_focus(&self.focus_handle)
            .size_full()
            .child(self.render_header(cx))
            .child(self.render_body(cx))
    }
}

fn note(id: &'static str, text: impl Into<SharedString>) -> AnyElement {
    div()
        .id(id)
        .debug_selector(|| id.to_string())
        .px_3()
        .py_2()
        .child(Label::new(text).size(LabelSize::Small).color(Color::Muted))
        .into_any_element()
}

impl DevicesPanel {
    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let reading = self.is_reading();
        h_flex()
            .px_3()
            .py_1p5()
            .justify_between()
            .child(Label::new("Devices").size(LabelSize::Small))
            .when(self.is_supported(cx), |header| {
                header.child(
                    div().debug_selector(|| "devices-reload".to_string()).child(
                        IconButton::new("devices-reload", IconName::RotateCw)
                            .icon_size(IconSize::Small)
                            .loading(reading)
                            .disabled(reading)
                            .tooltip(|_window, cx| ui::Tooltip::simple("Reload", cx))
                            .on_click(cx.listener(|this, _, _window, cx| this.reload(cx))),
                    ),
                )
            })
    }

    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.is_supported(cx) {
            return note(
                "devices-remote-message",
                "Devices works with projects on this computer only.",
            );
        }
        let Some(snapshot) = &self.snapshot else {
            let text = if self.is_reading() {
                "Reading devices…"
            } else {
                "Open this panel to read the devices."
            };
            return note("devices-waiting", text);
        };
        let (text, warn) = memory_line(&snapshot.memory);
        let memory = div()
            .id("devices-memory")
            .debug_selector(|| "devices-memory".to_string())
            .px_3()
            .pb_1()
            .child(Label::new(text).size(LabelSize::Small).color(if warn {
                Color::Warning
            } else {
                Color::Muted
            }));
        // Physical devices come from two tools that fail apart; a tool state
        // identical to its simulator sibling's is said once, not twice.
        let physical_ios = (snapshot.physical_ios != snapshot.ios_simulators
            || !snapshot.physical_ios.devices().is_empty())
        .then_some((SectionId::PhysicalIos, &snapshot.physical_ios));
        let physical_android = (snapshot.physical_android != snapshot.android_emulators
            || !snapshot.physical_android.devices().is_empty())
        .then_some((SectionId::PhysicalAndroid, &snapshot.physical_android));
        let ios_hidden = matches!(snapshot.ios_simulators, SectionState::Unsupported { .. });
        v_flex()
            .id("devices-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(memory)
            .children(self.render_run_note(cx))
            .when(!ios_hidden, |list| {
                list.child(self.render_group(
                    "iOS Simulators",
                    "No simulators installed.",
                    vec![(SectionId::IosSimulators, &snapshot.ios_simulators)],
                    cx,
                ))
            })
            .child(self.render_group(
                "Android Emulators",
                "No emulators created.",
                vec![(SectionId::AndroidEmulators, &snapshot.android_emulators)],
                cx,
            ))
            .child(self.render_group(
                "Physical Devices",
                "No physical devices. Connect one with USB debugging on, then Reload.",
                physical_ios.into_iter().chain(physical_android).collect(),
                cx,
            ))
            .when(
                matches!(snapshot.physical_android, SectionState::Ready(_)),
                |list| list.child(self.render_wireless_button(cx)),
            )
            .into_any_element()
    }

    fn render_group(
        &self,
        title: &'static str,
        empty_text: &'static str,
        sections: Vec<(SectionId, &SectionState)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let nothing_listed = sections.iter().all(|(_, state)| match state {
            SectionState::Ready(devices) => devices.is_empty(),
            _ => false,
        });
        let mut group = v_flex().w_full().child(
            div().px_3().pt_2().pb_1().child(
                Label::new(title)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            ),
        );
        for (id, state) in sections {
            if let Some(message) = self.section_errors.get(&id) {
                group = group.child(
                    div().px_3().pb_1().child(
                        Label::new(format!("Could not read this list again: {message}"))
                            .size(LabelSize::Small)
                            .color(Color::Error),
                    ),
                );
            }
            match state {
                SectionState::Ready(devices) => {
                    for device in devices {
                        group = group.child(self.render_device_row(device, cx));
                    }
                }
                SectionState::ToolMissing { tool, hint } => {
                    group = group.child(missing_tool(title, tool, hint, cx));
                }
                SectionState::Failed { message } => {
                    group = group.child(
                        div()
                            .debug_selector(|| "devices-section-failed".to_string())
                            .px_3()
                            .pb_1()
                            .child(
                                Label::new(message.clone())
                                    .size(LabelSize::Small)
                                    .color(Color::Error),
                            ),
                    );
                }
                SectionState::Unsupported { .. } => {}
            }
        }
        if nothing_listed {
            group = group.child(
                div().px_3().pb_1().child(
                    Label::new(empty_text)
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                ),
            );
        }
        group.into_any_element()
    }
}
