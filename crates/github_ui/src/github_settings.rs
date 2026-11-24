use settings::{RegisterSetting, Settings, SettingsContent};

#[derive(Debug, Clone, Copy, PartialEq, RegisterSetting)]
pub struct GithubPanelSettings {
    pub button: bool,
}

impl Settings for GithubPanelSettings {
    fn from_settings(content: &SettingsContent) -> Self {
        // `unwrap()` is the documented contract of `Settings::from_settings`:
        // `default.json` carries the field, so a missing value means the
        // settings ritual (settings_content + default.json together) was not
        // followed, and this is meant to panic until it is fixed.
        let content = content.github_panel.as_ref().unwrap();
        Self {
            button: content.button.unwrap(),
        }
    }
}
