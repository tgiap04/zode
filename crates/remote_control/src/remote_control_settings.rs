//! The `remote_control` setting.

use std::time::Duration;

use gpui::App;
use settings::{RegisterSetting, Settings, SettingsContent};

const DEFAULT_IDLE_TIMEOUT_MINUTES: u64 = 30;

/// Whether this Zode may be controlled from another device, and how long a
/// connected device may stay silent.
///
/// Read through `try_get` with the shipped defaults as fallback, so a context
/// without a settings store -- a test, or startup before settings load --
/// behaves like the default: off.
#[derive(Debug, Clone, Copy, RegisterSetting)]
pub struct RemoteControlSettings {
    enabled: bool,
    idle_timeout: Option<Duration>,
    push: PushPreferences,
}

/// Which agent events are pushed to paired devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushPreferences {
    pub approval: bool,
    pub answered: bool,
    pub ended: bool,
}

impl PushPreferences {
    pub const ALL: Self = Self {
        approval: true,
        answered: true,
        ended: true,
    };

    pub fn any(&self) -> bool {
        self.approval || self.answered || self.ended
    }
}

impl Settings for RemoteControlSettings {
    fn from_settings(content: &SettingsContent) -> Self {
        // `unwrap()` is the documented contract of `Settings::from_settings`:
        // `default.json` carries both fields, so a missing value means the
        // settings ritual (settings_content.rs + default.json in one commit)
        // was not followed, and this is meant to panic until it is fixed.
        let content = content.remote_control.as_ref().unwrap();
        let minutes = content.idle_timeout_minutes.unwrap();
        let push = content.push_notifications.as_ref().unwrap();
        Self {
            enabled: content.enabled.unwrap(),
            idle_timeout: (minutes > 0).then(|| Duration::from_secs(minutes.saturating_mul(60))),
            push: PushPreferences {
                approval: push.approval.unwrap(),
                answered: push.answered.unwrap(),
                ended: push.ended.unwrap(),
            },
        }
    }
}

impl RemoteControlSettings {
    pub fn is_enabled(cx: &App) -> bool {
        Self::try_get(cx).is_some_and(|setting| setting.enabled)
    }

    /// `None` means a connected device is never disconnected for being quiet.
    pub fn idle_timeout(cx: &App) -> Option<Duration> {
        Self::try_get(cx).map_or(
            Some(Duration::from_secs(DEFAULT_IDLE_TIMEOUT_MINUTES * 60)),
            |setting| setting.idle_timeout,
        )
    }

    /// Without a settings store, everything the shipped defaults push.
    pub fn push_preferences(cx: &App) -> PushPreferences {
        Self::try_get(cx).map_or(PushPreferences::ALL, |setting| setting.push)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, UpdateGlobal as _};
    use settings::SettingsStore;

    #[gpui::test]
    fn remote_control_ships_off_with_a_thirty_minute_idle_limit(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let store = SettingsStore::test(cx);
            cx.set_global(store);
            assert!(!RemoteControlSettings::is_enabled(cx));
            assert_eq!(
                RemoteControlSettings::idle_timeout(cx),
                Some(Duration::from_secs(30 * 60))
            );
            assert_eq!(
                RemoteControlSettings::push_preferences(cx),
                PushPreferences::ALL
            );
        });
    }

    #[gpui::test]
    fn each_pushed_event_can_be_turned_off_alone(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let store = SettingsStore::test(cx);
            cx.set_global(store);
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |content| {
                    content
                        .remote_control
                        .get_or_insert_default()
                        .push_notifications
                        .get_or_insert_default()
                        .answered = Some(false);
                });
            });
            let preferences = RemoteControlSettings::push_preferences(cx);
            assert_eq!(
                preferences,
                PushPreferences {
                    approval: true,
                    answered: false,
                    ended: true,
                }
            );
            assert!(preferences.any());
        });
    }

    #[gpui::test]
    fn the_user_can_turn_it_on_and_the_limit_off(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let store = SettingsStore::test(cx);
            cx.set_global(store);
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |content| {
                    let settings = content.remote_control.get_or_insert_default();
                    settings.enabled = Some(true);
                    settings.idle_timeout_minutes = Some(0);
                });
            });
            assert!(RemoteControlSettings::is_enabled(cx));
            assert_eq!(RemoteControlSettings::idle_timeout(cx), None);
        });
    }

    #[gpui::test]
    fn without_a_settings_store_the_feature_is_off(cx: &mut TestAppContext) {
        // What a context with no store reads must be the safe answer.
        cx.update(|cx| {
            assert!(!RemoteControlSettings::is_enabled(cx));
            assert_eq!(
                RemoteControlSettings::idle_timeout(cx),
                Some(Duration::from_secs(DEFAULT_IDLE_TIMEOUT_MINUTES * 60))
            );
        });
    }
}
