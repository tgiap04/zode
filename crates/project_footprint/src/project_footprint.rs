//! The per-project CPU/RAM footer badge: its setting, the indicator and its
//! popover. Measuring -- which PIDs belong to a project and what they cost --
//! lives in `process_footprint`, which the remote server shares.

use gpui::App;
use settings::{RegisterSetting, Settings, SettingsContent};

/// Whether the footer badge is shown at all. Read through `try_get` with a
/// `true` fallback, exactly as `KeepDisplayAwakeSetting::is_enabled` does, so
/// a context without a settings store behaves like the default instead of
/// silently switching the indicator off.
#[derive(RegisterSetting)]
pub struct ProjectFootprintSetting(pub bool);

impl Settings for ProjectFootprintSetting {
    /// Panics if `default.json` is missing `project_footprint_indicator`, per
    /// `Settings::from_settings`' convention: `#[derive(RegisterSetting)]`
    /// registers this type at link time, so every `SettingsStore` in the
    /// process evaluates it and a forgotten default fails loudly and
    /// immediately rather than silently taking a fallback.
    fn from_settings(content: &SettingsContent) -> Self {
        Self(
            content
                .project_footprint_indicator
                .expect("project_footprint_indicator missing from default.json"),
        )
    }
}

impl ProjectFootprintSetting {
    pub fn is_enabled(cx: &App) -> bool {
        Self::try_get(cx).map(|setting| setting.0).unwrap_or(true)
    }
}

pub use process_footprint::{
    FootprintRoots, Footprints, Pid, ProcessSampler, ProjectFootprint, SysinfoProcessSampler,
    collect,
};

mod footprint_format;
pub use footprint_format::{format_cpu, format_memory};

mod footprint_popover;

mod footprint_indicator;
pub use footprint_indicator::ProjectFootprintIndicator;

#[cfg(test)]
mod project_footprint_tests;

#[cfg(test)]
mod footprint_indicator_tests;
