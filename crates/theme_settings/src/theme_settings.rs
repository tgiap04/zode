#![deny(missing_docs)]

//! # Theme Settings
//!
//! This crate provides theme settings integration for Zed,
//! bridging the theme system with the settings infrastructure.

mod schema;
mod settings;

use std::sync::Arc;

use ::settings::{IntoGpui, Settings, SettingsStore};
use anyhow::{Context as _, Result};
use gpui::{App, Font, HighlightStyle, Pixels, Refineable, px};
use gpui_util::ResultExt;
use theme::{
    AccentColors, Appearance, AppearanceContent, DEFAULT_DARK_THEME, DEFAULT_ICON_THEME_NAME,
    GlobalTheme, LoadThemes, PlayerColor, PlayerColors, StatusColors, SyntaxTheme,
    SystemAppearance, SystemColors, Theme, ThemeColors, ThemeFamily, ThemeRegistry,
    ThemeSettingsProvider, ThemeStyles, default_color_scales, try_parse_color,
};

pub use crate::schema::{
    FontStyleContent, FontWeightContent, HighlightStyleContent, StatusColorsContent,
    ThemeColorsContent, ThemeContent, ThemeFamilyContent, ThemeStyleContent,
    WindowBackgroundContent, status_colors_refinement, syntax_overrides, theme_colors_refinement,
};
use crate::settings::adjust_buffer_font_size;
pub use crate::settings::{
    BufferLineHeight, FontFamilyName, IconThemeName, IconThemeSelection, ThemeAppearanceMode,
    ThemeName, ThemeSelection, ThemeSettings, adjust_ui_font_size, adjusted_font_size,
    appearance_to_mode, clamp_font_size, default_theme, observe_buffer_font_size_adjustment,
    reset_buffer_font_size, reset_ui_font_size, set_icon_theme, set_mode, set_theme, setup_ui_font,
};
pub use theme::UiDensity;

struct ThemeSettingsProviderImpl;

impl ThemeSettingsProvider for ThemeSettingsProviderImpl {
    fn ui_font<'a>(&'a self, cx: &'a App) -> &'a Font {
        &ThemeSettings::get_global(cx).ui_font
    }

    fn buffer_font<'a>(&'a self, cx: &'a App) -> &'a Font {
        &ThemeSettings::get_global(cx).buffer_font
    }

    fn ui_font_size(&self, cx: &App) -> Pixels {
        ThemeSettings::get_global(cx).ui_font_size(cx)
    }

    fn buffer_font_size(&self, cx: &App) -> Pixels {
        ThemeSettings::get_global(cx).buffer_font_size(cx)
    }

    fn ui_density(&self, cx: &App) -> UiDensity {
        ThemeSettings::get_global(cx).ui_density
    }
}

/// Initialize the theme system with settings integration.
///
/// This is the full initialization for the application. It calls [`theme::init`]
/// and then wires up settings observation for theme/font changes.
pub fn init(themes_to_load: LoadThemes, cx: &mut App) {
    let load_user_themes = matches!(&themes_to_load, LoadThemes::All(_));

    theme::init(themes_to_load, cx);
    theme::set_theme_settings_provider(Box::new(ThemeSettingsProviderImpl), cx);

    if load_user_themes {
        let registry = ThemeRegistry::global(cx);
        load_bundled_themes(&registry);
    }

    let theme = configured_theme(cx);
    let icon_theme = configured_icon_theme(cx);
    GlobalTheme::update_theme(cx, theme);
    GlobalTheme::update_icon_theme(cx, icon_theme);

    let settings = ThemeSettings::get_global(cx);

    let mut prev_buffer_font_size_settings = settings.buffer_font_size_settings();
    let mut prev_ui_font_size_settings = settings.ui_font_size_settings();
    let mut prev_theme_name = settings.theme.name(SystemAppearance::global(cx).0);
    let mut prev_icon_theme_name = settings.icon_theme.name(SystemAppearance::global(cx).0);
    let mut prev_theme_overrides = (
        settings.experimental_theme_overrides.clone(),
        settings.theme_overrides.clone(),
    );

    cx.observe_global::<SettingsStore>(move |cx| {
        let settings = ThemeSettings::get_global(cx);

        let buffer_font_size_settings = settings.buffer_font_size_settings();
        let ui_font_size_settings = settings.ui_font_size_settings();
        let theme_name = settings.theme.name(SystemAppearance::global(cx).0);
        let icon_theme_name = settings.icon_theme.name(SystemAppearance::global(cx).0);
        let theme_overrides = (
            settings.experimental_theme_overrides.clone(),
            settings.theme_overrides.clone(),
        );

        if buffer_font_size_settings != prev_buffer_font_size_settings {
            prev_buffer_font_size_settings = buffer_font_size_settings;
            reset_buffer_font_size(cx);
        }

        if ui_font_size_settings != prev_ui_font_size_settings {
            prev_ui_font_size_settings = ui_font_size_settings;
            reset_ui_font_size(cx);
        }

        if theme_name != prev_theme_name || theme_overrides != prev_theme_overrides {
            prev_theme_name = theme_name;
            prev_theme_overrides = theme_overrides;
            reload_theme(cx);
        }

        if icon_theme_name != prev_icon_theme_name {
            prev_icon_theme_name = icon_theme_name;
            reload_icon_theme(cx);
        }
    })
    .detach();
}

fn configured_theme(cx: &mut App) -> Arc<Theme> {
    let themes = ThemeRegistry::default_global(cx);
    let theme_settings = ThemeSettings::get_global(cx);
    let system_appearance = SystemAppearance::global(cx);

    let theme_name = theme_settings.theme.name(*system_appearance);

    let theme = match themes.get(&theme_name.0) {
        Ok(theme) => theme,
        Err(err) => {
            if themes.extensions_loaded() {
                log::error!("{err}");
            }
            themes
                .get(default_theme(*system_appearance))
                .unwrap_or_else(|_| themes.get(DEFAULT_DARK_THEME).unwrap())
        }
    };
    theme_settings.apply_theme_overrides(theme)
}

fn configured_icon_theme(cx: &mut App) -> Arc<theme::IconTheme> {
    let themes = ThemeRegistry::default_global(cx);
    let theme_settings = ThemeSettings::get_global(cx);
    let system_appearance = SystemAppearance::global(cx);

    let icon_theme_name = theme_settings.icon_theme.name(*system_appearance);

    match themes.get_icon_theme(&icon_theme_name.0) {
        Ok(theme) => theme,
        Err(err) => {
            if themes.extensions_loaded() {
                log::error!("{err}");
            }
            themes.get_icon_theme(DEFAULT_ICON_THEME_NAME).unwrap()
        }
    }
}

/// Reloads the current theme from settings.
pub fn reload_theme(cx: &mut App) {
    let theme = configured_theme(cx);
    GlobalTheme::update_theme(cx, theme);
    cx.refresh_windows();
}

/// Reloads the current icon theme from settings.
pub fn reload_icon_theme(cx: &mut App) {
    let icon_theme = configured_icon_theme(cx);
    GlobalTheme::update_icon_theme(cx, icon_theme);
    cx.refresh_windows();
}

/// Loads the themes bundled with the Zed binary into the registry.
pub fn load_bundled_themes(registry: &ThemeRegistry) {
    let theme_paths = registry
        .assets()
        .list("themes/")
        .expect("failed to list theme assets")
        .into_iter()
        .filter(|path| path.ends_with(".json"));

    for path in theme_paths {
        let Some(theme) = registry.assets().load(&path).log_err().flatten() else {
            continue;
        };

        let Some(theme_family) = serde_json::from_slice(&theme)
            .with_context(|| format!("failed to parse theme at path \"{path}\""))
            .log_err()
        else {
            continue;
        };

        let refined = refine_theme_family(theme_family);
        registry.insert_theme_families([refined]);
    }
}

/// Loads a user theme from the given bytes into the registry.
pub fn load_user_theme(registry: &ThemeRegistry, bytes: &[u8]) -> Result<()> {
    let theme = deserialize_user_theme(bytes)?;
    let refined = refine_theme_family(theme);
    registry.insert_theme_families([refined]);
    Ok(())
}

/// Deserializes a user theme from the given bytes.
pub fn deserialize_user_theme(bytes: &[u8]) -> Result<ThemeFamilyContent> {
    let theme_family: ThemeFamilyContent = serde_json_lenient::from_slice(bytes)?;

    for theme in &theme_family.themes {
        if theme
            .style
            .colors
            .deprecated_scrollbar_thumb_background
            .is_some()
        {
            log::warn!(
                r#"Theme "{theme_name}" is using a deprecated style property: scrollbar_thumb.background. Use `scrollbar.thumb.background` instead."#,
                theme_name = theme.name
            )
        }
    }

    Ok(theme_family)
}

/// Refines a [`ThemeFamilyContent`] and its [`ThemeContent`]s into a [`ThemeFamily`].
pub fn refine_theme_family(theme_family_content: ThemeFamilyContent) -> ThemeFamily {
    let id = uuid::Uuid::new_v4().to_string();
    let name = theme_family_content.name.clone();
    let author = theme_family_content.author.clone();

    let themes: Vec<Theme> = theme_family_content
        .themes
        .iter()
        .map(|theme_content| refine_theme(theme_content))
        .collect();

    ThemeFamily {
        id,
        name: name.into(),
        author: author.into(),
        themes,
        scales: default_color_scales(),
    }
}

/// Refines a [`ThemeContent`] into a [`Theme`].
pub fn refine_theme(theme: &ThemeContent) -> Theme {
    let appearance = match theme.appearance {
        AppearanceContent::Light => Appearance::Light,
        AppearanceContent::Dark => Appearance::Dark,
    };

    let mut refined_status_colors = match theme.appearance {
        AppearanceContent::Light => StatusColors::light(),
        AppearanceContent::Dark => StatusColors::dark(),
    };
    let mut status_colors_refinement = status_colors_refinement(&theme.style.status);
    theme::apply_status_color_defaults(&mut status_colors_refinement);
    refined_status_colors.refine(&status_colors_refinement);

    let mut refined_player_colors = match theme.appearance {
        AppearanceContent::Light => PlayerColors::light(),
        AppearanceContent::Dark => PlayerColors::dark(),
    };
    merge_player_colors(&mut refined_player_colors, &theme.style.players);

    let mut refined_theme_colors = match theme.appearance {
        AppearanceContent::Light => ThemeColors::light(),
        AppearanceContent::Dark => ThemeColors::dark(),
    };
    let mut theme_colors_refinement =
        theme_colors_refinement(&theme.style.colors, &status_colors_refinement);
    theme::apply_theme_color_defaults(&mut theme_colors_refinement, &refined_player_colors);
    refined_theme_colors.refine(&theme_colors_refinement);

    let mut refined_accent_colors = match theme.appearance {
        AppearanceContent::Light => AccentColors::light(),
        AppearanceContent::Dark => AccentColors::dark(),
    };
    merge_accent_colors(&mut refined_accent_colors, &theme.style.accents);

    let syntax_highlights = theme.style.syntax.iter().map(|(syntax_token, highlight)| {
        (
            syntax_token.clone(),
            HighlightStyle {
                color: highlight
                    .color
                    .as_ref()
                    .and_then(|color| try_parse_color(color).ok()),
                background_color: highlight
                    .background_color
                    .as_ref()
                    .and_then(|color| try_parse_color(color).ok()),
                font_style: highlight.font_style.map(|s| s.into_gpui()),
                font_weight: highlight.font_weight.map(|w| w.into_gpui()),
                ..Default::default()
            },
        )
    });
    let syntax_theme = Arc::new(SyntaxTheme::new(syntax_highlights));

    let window_background_appearance = theme
        .style
        .window_background_appearance
        .map(|w| w.into_gpui())
        .unwrap_or_default();

    Theme {
        id: uuid::Uuid::new_v4().to_string(),
        name: theme.name.clone().into(),
        appearance,
        styles: ThemeStyles {
            system: SystemColors::default(),
            window_background_appearance,
            accents: refined_accent_colors,
            colors: refined_theme_colors,
            status: refined_status_colors,
            player: refined_player_colors,
            syntax: syntax_theme,
        },
    }
}

/// Merges player color overrides into the given [`PlayerColors`].
pub fn merge_player_colors(
    player_colors: &mut PlayerColors,
    user_player_colors: &[::settings::PlayerColorContent],
) {
    if user_player_colors.is_empty() {
        return;
    }

    for (idx, player) in user_player_colors.iter().enumerate() {
        let cursor = player
            .cursor
            .as_ref()
            .and_then(|color| try_parse_color(color).ok());
        let background = player
            .background
            .as_ref()
            .and_then(|color| try_parse_color(color).ok());
        let selection = player
            .selection
            .as_ref()
            .and_then(|color| try_parse_color(color).ok());

        if let Some(player_color) = player_colors.0.get_mut(idx) {
            *player_color = PlayerColor {
                cursor: cursor.unwrap_or(player_color.cursor),
                background: background.unwrap_or(player_color.background),
                selection: selection.unwrap_or(player_color.selection),
            };
        } else {
            player_colors.0.push(PlayerColor {
                cursor: cursor.unwrap_or_default(),
                background: background.unwrap_or_default(),
                selection: selection.unwrap_or_default(),
            });
        }
    }
}

/// Merges accent color overrides into the given [`AccentColors`].
pub fn merge_accent_colors(
    accent_colors: &mut AccentColors,
    user_accent_colors: &[::settings::AccentContent],
) {
    if user_accent_colors.is_empty() {
        return;
    }

    let colors = user_accent_colors
        .iter()
        .filter_map(|accent_color| {
            accent_color
                .0
                .as_ref()
                .and_then(|color| try_parse_color(color).ok())
        })
        .collect::<Vec<_>>();

    if !colors.is_empty() {
        accent_colors.0 = Arc::from(colors);
    }
}

/// Increases the buffer font size by 1 pixel, without persisting the result in the settings.
/// This will be effective until the app is restarted.
pub fn increase_buffer_font_size(cx: &mut App) {
    adjust_buffer_font_size(cx, |size| size + px(1.0));
}

/// Decreases the buffer font size by 1 pixel, without persisting the result in the settings.
/// This will be effective until the app is restarted.
pub fn decrease_buffer_font_size(cx: &mut App) {
    adjust_buffer_font_size(cx, |size| size - px(1.0));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn bundled_theme_files() -> Vec<PathBuf> {
        let themes_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/themes");
        let mut files = Vec::new();
        let mut dirs = vec![themes_dir];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).expect("failed to read themes dir") {
                let path = entry.expect("failed to read theme dir entry").path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|ext| ext == "json") {
                    files.push(path);
                }
            }
        }
        assert!(!files.is_empty(), "no bundled themes found");
        files.sort();
        files
    }

    fn parse_family(path: &PathBuf) -> ThemeFamilyContent {
        let bytes = std::fs::read(path).expect("failed to read theme");
        // Mirrors the call in `load_bundled_themes`, which swallows parse failures
        // via `log_err()` — a broken theme is silently absent at runtime, so it has
        // to be caught here instead.
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|err| panic!("failed to parse theme at {}: {err}", path.display()))
    }

    #[test]
    fn bundled_themes_parse() {
        for path in bundled_theme_files() {
            let family = parse_family(&path);
            for theme in &family.themes {
                assert_eq!(
                    theme.style.players.len(),
                    8,
                    "{} in {} must define 8 player colors: index 0 drives the local editor \
                     and terminal cursor, index 7 drives the agent/absent color, and \
                     `merge_player_colors` silently keeps the built-in palette when the list \
                     is short",
                    theme.name,
                    path.display()
                );
                assert!(
                    !theme.style.syntax.is_empty(),
                    "{} in {} has no syntax styles",
                    theme.name,
                    path.display()
                );
            }
        }
    }

    #[test]
    fn bundled_theme_colors_parse() {
        fn check(value: &serde_json::Value, path: &PathBuf) {
            match value {
                serde_json::Value::String(string) if string.starts_with('#') => {
                    assert!(
                        try_parse_color(string).is_ok(),
                        "unparseable color {string:?} in {}",
                        path.display()
                    );
                }
                serde_json::Value::Array(items) => items.iter().for_each(|item| check(item, path)),
                serde_json::Value::Object(entries) => {
                    entries.values().for_each(|entry| check(entry, path))
                }
                _ => {}
            }
        }

        for path in bundled_theme_files() {
            let bytes = std::fs::read(&path).expect("failed to read theme");
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).expect("failed to parse theme as JSON");
            check(&value, &path);
        }
    }

    #[test]
    fn default_themes_are_bundled() {
        let bundled = bundled_theme_files()
            .iter()
            .flat_map(|path| parse_family(path).themes)
            .map(|theme| theme.name)
            .collect::<Vec<_>>();

        // The first two are the configured defaults; the third is the last-resort
        // fallback that `configured_theme` reaches for, and it names a different
        // theme on purpose.
        for default in [
            ::settings::DEFAULT_LIGHT_THEME,
            ::settings::DEFAULT_DARK_THEME,
            theme::DEFAULT_DARK_THEME,
        ] {
            assert!(
                bundled.iter().any(|name| name == default),
                "default theme {default:?} is not among the bundled themes {bundled:?}"
            );
        }

        // `assets/settings/default.json` is what actually drives a fresh install;
        // the constants above only mirror it. A name that matches neither a bundled
        // theme nor the constants means the app silently falls back at startup.
        let defaults =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/settings/default.json");
        let bytes = std::fs::read(&defaults).expect("failed to read default settings");
        let settings: serde_json_lenient::Value =
            serde_json_lenient::from_slice(&bytes).expect("failed to parse default settings");
        for mode in ["light", "dark"] {
            let name = settings["theme"][mode]
                .as_str()
                .unwrap_or_else(|| panic!("default.json has no theme.{mode}"));
            assert!(
                bundled.iter().any(|bundled_name| bundled_name == name),
                "default.json theme.{mode} = {name:?} is not among the bundled themes {bundled:?}"
            );
        }
    }

    /// The title bar and status bar are meant to read as one surface with the
    /// editor, with no rule between them. Upstream VS Code does not do this --
    /// its 2026 themes give the chrome its own near-identical shade -- so the
    /// importer, or anyone re-deriving these files from source, will put that
    /// shade back. It is a 3% difference that looks like nothing in a diff and
    /// like a seam on screen.
    #[test]
    fn the_2026_chrome_shares_the_editor_background() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/themes/vscode-2026/vscode-2026.json");
        let family = parse_family(&path);
        assert!(!family.themes.is_empty(), "vscode-2026 defines no themes");

        for theme in &family.themes {
            let colors = &theme.style.colors;
            let editor = colors
                .editor_background
                .as_ref()
                .unwrap_or_else(|| panic!("{} has no editor.background", theme.name));

            for (field, value) in [
                ("title_bar.background", &colors.title_bar_background),
                (
                    "title_bar.inactive_background",
                    &colors.title_bar_inactive_background,
                ),
                ("status_bar.background", &colors.status_bar_background),
            ] {
                assert_eq!(
                    value.as_ref(),
                    Some(editor),
                    "{} sets {field} away from editor.background ({editor})",
                    theme.name
                );
            }
        }
    }
}
