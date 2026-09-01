use gpui::Pixels;
use settings::{RegisterSetting, Settings};
use ui::px;
use workspace::dock::DockPosition;

/// Settings for the branch panel.
///
/// Every field is `unwrap`ped out of the content struct, exactly as
/// `GitPanelSettings` does: the defaults live in `assets/settings/default.json`
/// and a missing key there is a startup panic rather than a compile error. Add
/// a field here only together with its default over there.
#[derive(Debug, Clone, PartialEq, RegisterSetting)]
pub struct BranchPanelSettings {
    pub button: bool,
    pub dock: DockPosition,
    pub default_width: Pixels,
    pub starts_open: bool,
}

impl Settings for BranchPanelSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let branch_panel = content.branch_panel.clone().unwrap();
        Self {
            button: branch_panel.button.unwrap(),
            dock: branch_panel.dock.unwrap().into(),
            default_width: px(branch_panel.default_width.unwrap()),
            starts_open: branch_panel.starts_open.unwrap(),
        }
    }
}
