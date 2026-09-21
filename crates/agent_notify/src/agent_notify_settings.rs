//! The `agent_finished_notification` setting.

use std::time::Duration;

use gpui::App;
use settings::{RegisterSetting, Settings, SettingsContent};

/// Floor on [`AgentFinishedNotificationSetting::quiet_period`].
///
/// The debounce this setting sits on top of (`AgentView`'s own answering
/// debounce, `ACTIVITY_TICK` in `agent_view.rs`) already settles on a scale of
/// a couple of seconds. A `quiet_period_ms` below that would notify on every
/// pause inside a single reply -- the exact strobe that debounce exists to
/// prevent one layer down.
const MIN_QUIET_PERIOD: Duration = Duration::from_millis(2000);

/// The shipped default, mirrored in `assets/settings/default.json`.
const DEFAULT_QUIET_PERIOD_MS: u64 = 12_000;

/// Whether to notify, and how long to wait before believing an agent is done.
///
/// Read through `try_get` with the documented defaults as fallback, the way
/// `KeepDisplayAwakeSetting::is_enabled` does, so a context without a
/// settings store -- a test, or startup before settings load -- behaves like
/// the shipped default instead of silently doing nothing.
#[derive(Debug, Clone, Copy, RegisterSetting)]
pub(crate) struct AgentFinishedNotificationSetting {
    enabled: bool,
    quiet_period: Duration,
}

impl Settings for AgentFinishedNotificationSetting {
    fn from_settings(content: &SettingsContent) -> Self {
        // `unwrap()` here is the documented contract of `Settings::from_settings`,
        // not an exception to it: `default.json` carries both fields, so a
        // missing value means the settings ritual (settings_content.rs +
        // default.json in one commit) was not followed, and this is meant to
        // panic in every binary that links this crate until it is fixed --
        // exactly as `KeepDisplayAwakeSetting` and `WhichKeySettings` do.
        let content = content.agent_finished_notification.as_ref().unwrap();
        let quiet_period_ms = content.quiet_period_ms.unwrap();
        Self {
            enabled: content.enabled.unwrap(),
            quiet_period: Duration::from_millis(quiet_period_ms).max(MIN_QUIET_PERIOD),
        }
    }
}

impl AgentFinishedNotificationSetting {
    pub(crate) fn is_enabled(cx: &App) -> bool {
        Self::try_get(cx)
            .map(|setting| setting.enabled)
            .unwrap_or(true)
    }

    pub(crate) fn quiet_period(cx: &App) -> Duration {
        Self::try_get(cx)
            .map(|setting| setting.quiet_period)
            .unwrap_or(Duration::from_millis(DEFAULT_QUIET_PERIOD_MS))
    }
}
