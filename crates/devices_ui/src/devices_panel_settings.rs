use gpui::Pixels;
use settings::{RegisterSetting, Settings, SettingsContent};
use ui::px;

#[derive(Debug, Clone, Copy, PartialEq, RegisterSetting)]
pub struct DevicesPanelSettings {
    pub button: bool,
    pub default_width: Pixels,
}

impl Settings for DevicesPanelSettings {
    fn from_settings(content: &SettingsContent) -> Self {
        // `unwrap()` is the documented contract of `Settings::from_settings`:
        // `default.json` carries both fields, so a missing value means the
        // settings ritual (settings_content.rs + default.json in one commit)
        // was not followed, and this is meant to panic until it is fixed.
        let content = content.devices_panel.as_ref().unwrap();
        Self {
            button: content.button.unwrap(),
            default_width: px(content.default_width.unwrap()),
        }
    }
}
