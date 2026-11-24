use gpui::{App, Context, EventEmitter, FocusHandle, Focusable, Pixels, Window};
use settings::Settings as _;
use ui::IconName;
use workspace::dock::{DockPosition, Panel, PanelEvent};

use crate::ToggleFocus;
use crate::devices_panel::DevicesPanel;
use crate::devices_panel_settings::DevicesPanelSettings;

impl EventEmitter<PanelEvent> for DevicesPanel {}

impl Focusable for DevicesPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for DevicesPanel {
    fn persistent_name() -> &'static str {
        "Devices"
    }

    fn panel_key() -> &'static str {
        "DevicesPanel"
    }

    fn position(&self, _window: &Window, _cx: &App) -> DockPosition {
        DockPosition::Right
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Right)
    }

    fn set_position(
        &mut self,
        _position: DockPosition,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    fn default_size(&self, _window: &Window, cx: &App) -> Pixels {
        DevicesPanelSettings::get_global(cx).default_width
    }

    fn icon(&self, _window: &Window, cx: &App) -> Option<IconName> {
        DevicesPanelSettings::get_global(cx)
            .button
            .then_some(IconName::Smartphone)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("Devices")
    }

    fn toggle_action(&self) -> Box<dyn gpui::Action> {
        Box::new(ToggleFocus)
    }

    fn starts_open(&self, _window: &Window, _cx: &App) -> bool {
        false
    }

    /// After Agent History (5), so the tool column's order stays as it was.
    fn activation_priority(&self) -> u32 {
        6
    }

    /// Showing the panel is what reads the devices, once. Hiding it stops
    /// nothing: a device being started is followed to its end either way.
    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active && !self.loaded_once {
            self.reload(cx);
        }
    }
}
