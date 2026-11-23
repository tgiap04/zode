//! What the panel tells a person about physical phones: why one listed phone
//! cannot be used yet, and the way to add one over Wi-Fi.

use devices::{
    Device, DeviceKind, DeviceState, NO_PERMISSIONS_REASON, OFFLINE_REASON, UNAUTHORIZED_REASON,
};
use gpui::{AnyElement, Context};
use ui::prelude::*;

use crate::ConnectWirelessly;
use crate::devices_panel::DevicesPanel;

/// What to do about a physical Android phone that is listed but cannot be used.
pub(crate) fn guidance_for(device: &Device) -> Option<&'static str> {
    if !matches!(
        device.kind,
        DeviceKind::AndroidUsb | DeviceKind::AndroidWireless
    ) {
        return None;
    }
    let DeviceState::Unusable(reason) = &device.state else {
        return None;
    };
    match reason.as_str() {
        UNAUTHORIZED_REASON => Some(
            "Unlock the phone and accept the \"Allow USB debugging?\" prompt, then Reload. \
             If no prompt appears, unplug the cable and plug it in again.",
        ),
        OFFLINE_REASON => Some(
            "Replug the cable, or turn USB debugging off and on, then Reload. \
             A phone connected over Wi-Fi needs Connect wirelessly again.",
        ),
        NO_PERMISSIONS_REASON => Some(
            "On Linux, adb needs a udev rule for this phone. See the setup guide, \
             then replug the phone and Reload.",
        ),
        _ => None,
    }
}

pub(crate) fn guidance_line(device: &Device, row_id: &str) -> Option<AnyElement> {
    let text = guidance_for(device)?;
    let selector = format!("{row_id}-guidance");
    Some(
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector)
            .w_full()
            .child(Label::new(text).size(LabelSize::XSmall).color(Color::Muted))
            .into_any_element(),
    )
}

impl DevicesPanel {
    /// Clicking dispatches the action rather than opening the dialog here: the
    /// click is handled while the panel is being updated, and the dialog is
    /// opened through the workspace.
    pub(crate) fn render_wireless_button(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .px_3()
            .pb_1()
            .debug_selector(|| "devices-connect-wireless".to_string())
            .child(
                Button::new("devices-connect-wireless", "Connect wirelessly\u{2026}")
                    .label_size(LabelSize::Small)
                    .on_click(cx.listener(|_, _, window, cx| {
                        window.dispatch_action(Box::new(ConnectWirelessly), cx)
                    })),
            )
            .into_any_element()
    }
}
