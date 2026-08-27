mod base_keymap_setting;
mod content_into_gpui;
mod editable_setting_control;
mod editorconfig_store;
mod keymap_file;
mod settings_file;
mod settings_store;

pub use settings_macros::RegisterSetting;

pub mod settings_content {
    pub use ::settings_content::*;
}

pub mod fallible_options {
    pub use ::settings_content::{FallibleOption, parse_json};
}

#[doc(hidden)]
pub mod private {
    pub use crate::settings_store::{RegisteredSetting, SettingValue};
    pub use inventory;
}

use gpui::{App, Global};

use rust_embed::RustEmbed;
use std::env;
use std::{borrow::Cow, fmt, str};
use util::asset_str;

pub use ::settings_content::*;
pub use base_keymap_setting::*;
pub use content_into_gpui::IntoGpui;
pub use editable_setting_control::*;
pub use editorconfig_store::{
    Editorconfig, EditorconfigEvent, EditorconfigProperties, EditorconfigStore,
};
pub use keymap_file::{
    KeyBindingValidator, KeyBindingValidatorRegistration, KeybindSource, KeybindUpdateOperation,
    KeybindUpdateTarget, KeymapFile, KeymapFileLoadResult,
};
pub use settings_file::*;
pub use settings_json::*;
pub use settings_store::{
    DefaultSemanticTokenRules, InvalidSettingsError, LSP_SETTINGS_SCHEMA_URL_PREFIX,
    LocalSettingsKind, LocalSettingsPath, MigrationStatus, Settings, SettingsFile,
    SettingsJsonSchemaParams, SettingsKey, SettingsLocation, SettingsParseResult, SettingsStore,
};

pub use keymap_file::ActionSequence;

#[derive(Clone, Debug, PartialEq)]
pub struct ActiveSettingsProfileName(pub String);

impl Global for ActiveSettingsProfileName {}

pub trait UserSettingsContentExt {
    fn for_profile(&self, cx: &App) -> Option<&SettingsProfile>;
    fn for_release_channel(&self) -> Option<&SettingsContent>;
    fn for_os(&self) -> Option<&SettingsContent>;
}

impl UserSettingsContentExt for UserSettingsContent {
    fn for_profile(&self, cx: &App) -> Option<&SettingsProfile> {
        let Some(active_profile) = cx.try_global::<ActiveSettingsProfileName>() else {
            return None;
        };
        self.profiles.get(&active_profile.0)
    }

    fn for_release_channel(&self) -> Option<&SettingsContent> {
        self.release_channel_overrides
            .get_by_key(release_channel::RELEASE_CHANNEL.dev_name())
    }

    fn for_os(&self) -> Option<&SettingsContent> {
        self.platform_overrides.get_by_key(env::consts::OS)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash, PartialOrd, Ord, serde::Serialize)]
pub struct WorktreeId(usize);

impl From<WorktreeId> for usize {
    fn from(value: WorktreeId) -> Self {
        value.0
    }
}

impl WorktreeId {
    pub fn from_usize(handle_id: usize) -> Self {
        Self(handle_id)
    }

    pub fn from_proto(id: u64) -> Self {
        Self(id as usize)
    }

    pub fn to_proto(self) -> u64 {
        self.0 as u64
    }

    pub fn to_usize(self) -> usize {
        self.0
    }
}

impl fmt::Display for WorktreeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

#[derive(RustEmbed)]
#[folder = "../../assets"]
#[include = "settings/*"]
#[include = "keymaps/*"]
#[exclude = "*.DS_Store"]
pub struct SettingsAssets;

pub fn init(cx: &mut App) {
    let settings = SettingsStore::new(cx, &default_settings());
    cx.set_global(settings);
    SettingsStore::observe_active_settings_profile_name(cx).detach();
}

pub fn default_settings() -> Cow<'static, str> {
    asset_str::<SettingsAssets>("settings/default.json")
}

pub fn default_semantic_token_rules() -> Cow<'static, str> {
    asset_str::<SettingsAssets>("settings/default_semantic_token_rules.json")
}

#[cfg(target_os = "macos")]
pub const DEFAULT_KEYMAP_PATH: &str = "keymaps/default-macos.json";

#[cfg(target_os = "windows")]
pub const DEFAULT_KEYMAP_PATH: &str = "keymaps/default-windows.json";

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub const DEFAULT_KEYMAP_PATH: &str = "keymaps/default-linux.json";

pub fn default_keymap() -> Cow<'static, str> {
    asset_str::<SettingsAssets>(DEFAULT_KEYMAP_PATH)
}

pub const VIM_KEYMAP_PATH: &str = "keymaps/vim.json";

pub fn vim_keymap() -> Cow<'static, str> {
    asset_str::<SettingsAssets>(VIM_KEYMAP_PATH)
}

pub fn initial_user_settings_content() -> Cow<'static, str> {
    asset_str::<SettingsAssets>("settings/initial_user_settings.json")
}

pub fn initial_server_settings_content() -> Cow<'static, str> {
    asset_str::<SettingsAssets>("settings/initial_server_settings.json")
}

pub fn initial_project_settings_content() -> Cow<'static, str> {
    asset_str::<SettingsAssets>("settings/initial_local_settings.json")
}

pub fn initial_keymap_content() -> Cow<'static, str> {
    asset_str::<SettingsAssets>("keymaps/initial.json")
}

pub fn initial_tasks_content() -> Cow<'static, str> {
    asset_str::<SettingsAssets>("settings/initial_tasks.json")
}

pub fn initial_debug_tasks_content() -> Cow<'static, str> {
    asset_str::<SettingsAssets>("settings/initial_debug_tasks.json")
}

pub fn initial_local_debug_tasks_content() -> Cow<'static, str> {
    asset_str::<SettingsAssets>("settings/initial_local_debug_tasks.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every key seeded into a fresh `settings.json` OVERRIDES `default.json`,
    // which makes a values-carrying seed invisible in review yet decisive at
    // runtime: it pins new installs to whatever was current the day the seed
    // was written, and the shipped theme and font size never take effect.
    #[test]
    fn the_seeded_user_settings_override_nothing() {
        let seed = initial_user_settings_content();
        let parsed: serde_json::Value = crate::parse_json_with_comments(seed.as_ref())
            .expect("seeded user settings must parse as jsonc");
        let object = parsed
            .as_object()
            .expect("seeded user settings must be a JSON object");

        assert!(
            object.is_empty(),
            "seed overrides shipped defaults for: {:?}",
            object.keys().collect::<Vec<_>>()
        );
    }

    // Defaulting to a font that is not shipped fails silently: it renders
    // perfectly on the machine of whoever chose it, and falls back to something
    // else on every machine that never installed it. So assert the files are
    // actually here, not merely that the names read the way we meant.
    #[test]
    fn the_default_fonts_are_bundled_with_the_app() {
        let defaults: serde_json::Value =
            crate::parse_json_with_comments(crate::default_settings().as_ref())
                .expect("default settings must parse as jsonc");

        assert_eq!(defaults["buffer_font_family"], "JetBrains Mono");
        assert_eq!(defaults["terminal"]["font_family"], "JetBrains Mono NL");

        // Every weight, not just Regular: a missing Bold is synthesised by the
        // platform into a smeared fake that nobody notices until it is shipped.
        let fonts = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/fonts/jetbrains-mono");
        for family in ["JetBrainsMono", "JetBrainsMonoNL"] {
            for style in ["Regular", "Italic", "Bold", "BoldItalic"] {
                let file = fonts.join(format!("{family}-{style}.ttf"));
                assert!(
                    file.is_file(),
                    "default font not bundled: {}",
                    file.display()
                );
            }
        }
        assert!(
            fonts.join("OFL.txt").is_file(),
            "the SIL Open Font License must ship with the fonts it covers"
        );

        // Whatever `default.json` names, run it through the same resolver `gpui`
        // uses and require the answer to be a font whose files ship. Deliberately
        // not pinned to `.ZedSans`: the default has been both a literal family
        // name and an alias, `font_name_with_fallbacks` passes a plain name
        // straight through, and a test that pinned the *shape* would go red on a
        // change that broke nothing.
        let declared = defaults["ui_font_family"]
            .as_str()
            .expect("ui_font_family must be a string");
        let ui_family = gpui::font_name_with_fallbacks(declared, ".SystemUIFont");
        assert_eq!(
            ui_family, "Inter",
            "the UI default resolves to a font this test does not know how to locate"
        );
        let ui_fonts =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts/inter");
        // Regular and SemiBold, upright and italic -- the four the UI actually
        // asks for. A missing weight is synthesised into a fake by the platform
        // rather than reported.
        for style in ["Regular", "Italic", "SemiBold", "SemiBoldItalic"] {
            let file = ui_fonts.join(format!("Inter-{style}.ttf"));
            assert!(
                file.is_file(),
                "UI default font not bundled: {}",
                file.display()
            );
        }
        assert!(
            ui_fonts.join("OFL.txt").is_file(),
            "the SIL Open Font License must ship with the fonts it covers"
        );

        // Merged with the platform defaults, so this covers glyphs JetBrains
        // Mono has no design for at all -- CJK being the obvious one.
        for fallbacks in [
            &defaults["buffer_font_fallbacks"],
            &defaults["terminal"]["font_fallbacks"],
        ] {
            assert_eq!(fallbacks, &serde_json::json!([".ZedMono"]));
        }
    }

    /// The rail stands on the LEFT and that edge is no longer a setting --
    /// `Workspace::OWN_COLUMN_POSITION` is where the code says so. This crate
    /// cannot see `workspace` (the dependency runs the other way), so the side is
    /// a literal here; change it there and forget it here and this test goes red,
    /// which is the point of writing it down twice.
    const RAIL_SIDE: &str = "left";

    // A panel docked away from the rail is absent from it entirely -- no error, no
    // empty state, just a rail that quietly has nothing on it. That is what the
    // shipped defaults did before this layout was pinned.
    #[test]
    fn the_rail_riding_panels_dock_where_the_rail_stands() {
        let defaults: serde_json::Value =
            crate::parse_json_with_comments(crate::default_settings().as_ref())
                .expect("default settings must parse as jsonc");

        // These ride the rail, so they have to dock on its side or their buttons
        // are simply absent from it.
        for panel in ["outline_panel", "git_panel"] {
            assert_eq!(
                defaults[panel]["dock"], RAIL_SIDE,
                "{panel} must dock on the rail's side ({RAIL_SIDE}) to appear in it"
            );
        }

        // The project panel has no `dock` key left to disagree with: it is pinned
        // to the right in code so its button lands in the status bar. A default
        // surviving here would be a dead one.
        assert!(
            defaults["project_panel"].get("dock").is_none(),
            "project_panel.dock was removed -- a default left behind here is read by nothing"
        );

        // And the rail's own edge is not a setting any more.
        assert!(
            defaults["multi_project"].get("sidebar_side").is_none(),
            "multi_project.sidebar_side was removed -- a default left behind here is read by nothing"
        );
    }

    // The seed is not just read, it is EDITED: the first setting a user changes
    // is written into this exact text. An empty object with a comment header is
    // the shape the jsonc editor handles worst, and a mangled header is invisible
    // to every other test because the file still parses afterwards.
    #[test]
    fn the_seeded_header_survives_the_first_settings_write() {
        let mut text = initial_user_settings_content().to_string();
        let old: serde_json::Value = crate::parse_json_with_comments(&text)
            .expect("seeded user settings must parse as jsonc");
        let new = serde_json::json!({ "vim_mode": true });

        crate::update_value_in_json_text(
            &mut text,
            &mut Vec::new(),
            2,
            &old,
            &new,
            &mut Vec::new(),
        );

        let written: serde_json::Value = crate::parse_json_with_comments(&text)
            .unwrap_or_else(|error| panic!("first write produced invalid jsonc: {error}\n{text}"));
        assert_eq!(written["vim_mode"], serde_json::json!(true));

        for line in initial_user_settings_content().lines() {
            let line = line.trim();
            if line.starts_with("//") {
                assert!(
                    text.contains(line),
                    "the first write mangled a header line.\nlost: {line}\nresult:\n{text}"
                );
            }
        }
    }
}
