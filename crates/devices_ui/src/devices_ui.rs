//! The Devices panel: iOS simulators, Android emulators and physical devices
//! of this machine, listed once when the panel is first shown and again on
//! Reload. The only thing that runs on a timer is the one device a Boot or
//! Shutdown was just clicked on, and only until it settles.

mod device_actions;
mod device_follow;
mod device_list_render;
mod device_operations;
mod device_row_render;
mod device_snapshot;
mod devices_panel;
mod devices_panel_dock;
mod devices_panel_settings;
mod physical_device_guidance;
mod run_app;
mod run_app_render;
mod run_app_start;
mod run_app_trust;
mod run_task;
mod run_terminals;
mod stray_emulators;
mod wireless_modal;
mod wireless_modal_render;

#[cfg(test)]
mod device_actions_tests;
#[cfg(test)]
mod devices_panel_tests;
#[cfg(test)]
mod run_app_native_ui_tests;
#[cfg(test)]
mod run_app_test_support;
#[cfg(test)]
mod run_app_tests;
#[cfg(test)]
mod run_task_chained_tests;
#[cfg(test)]
mod run_task_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod wireless_modal_tests;

use gpui::{App, actions};
use workspace::Workspace;

pub use devices_panel::DevicesPanel;
pub use devices_panel_settings::DevicesPanelSettings;
use wireless_modal::WirelessModal;

actions!(
    devices_panel,
    [
        /// Toggles focus on the Devices panel.
        ToggleFocus,
        /// Reads the simulators, emulators and devices again.
        Reload,
        /// Pairs and connects an Android phone over Wi-Fi.
        ConnectWirelessly,
    ]
);

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
            workspace.toggle_panel_focus::<DevicesPanel>(window, cx);
        });
        workspace.register_action(|workspace, _: &Reload, _window, cx| {
            if let Some(panel) = workspace.panel::<DevicesPanel>(cx) {
                panel.update(cx, |panel, cx| panel.reload(cx));
            }
        });
        workspace.register_action(|workspace, _: &ConnectWirelessly, window, cx| {
            let Some(panel) = workspace.panel::<DevicesPanel>(cx) else {
                return;
            };
            if !panel.read(cx).is_supported(cx) {
                return;
            }
            let panel = panel.downgrade();
            workspace.toggle_modal(window, cx, move |window, cx| {
                WirelessModal::new(panel, window, cx)
            });
        });
    })
    .detach();
}
