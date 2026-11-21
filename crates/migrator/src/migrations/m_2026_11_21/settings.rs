use anyhow::Result;
use serde_json::Value;

use super::keymap::rename_action;
use crate::migrations::migrate_settings;

/// Moves settings that name things after Zed to their Zode names: `command_aliases`
/// pointing at a `zed::` action, and font families spelled with upstream's virtual
/// names. Neither old spelling resolves any more, so leaving them would silently drop
/// the user's alias or fall back to a different font.
pub fn rename_zed_names_to_zode(value: &mut Value) -> Result<()> {
    migrate_settings(value, &mut migrate_one)
}

/// The virtual names map to their Zode counterparts; the "Zed Plex" names were only
/// ever aliases for the concrete families, so they become those families.
const FONT_RENAMES: &[(&str, &str)] = &[
    (".ZedMono", ".ZodeMono"),
    (".ZedSans", ".ZodeSans"),
    ("Zed Plex Mono", "Lilex"),
    ("Zed Plex Sans", "IBM Plex Sans"),
];

const FONT_KEYS: &[&str] = &[
    "buffer_font_family",
    "buffer_font_fallbacks",
    "ui_font_family",
    "ui_font_fallbacks",
];

const TERMINAL_FONT_KEYS: &[&str] = &["font_family", "font_fallbacks"];

fn rename_fonts(obj: &mut serde_json::Map<String, Value>, keys: &[&str]) {
    for key in keys {
        match obj.get_mut(*key) {
            Some(Value::String(family)) => rename_font(family),
            Some(Value::Array(families)) => {
                for family in families {
                    if let Value::String(family) = family {
                        rename_font(family);
                    }
                }
            }
            _ => {}
        }
    }
}

fn rename_font(family: &mut String) {
    if let Some((_, renamed)) = FONT_RENAMES.iter().find(|(old, _)| old == family) {
        *family = renamed.to_string();
    }
}

fn migrate_one(obj: &mut serde_json::Map<String, Value>) -> Result<()> {
    rename_fonts(obj, FONT_KEYS);
    if let Some(terminal) = obj
        .get_mut("terminal")
        .and_then(|terminal| terminal.as_object_mut())
    {
        rename_fonts(terminal, TERMINAL_FONT_KEYS);
    }

    if let Some(aliases) = obj
        .get_mut("command_aliases")
        .and_then(|aliases| aliases.as_object_mut())
    {
        for action in aliases.values_mut() {
            if let Some(renamed) = action.as_str().and_then(rename_action) {
                *action = Value::String(renamed);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn migrate(input: Value) -> Value {
        let mut value = input;
        rename_zed_names_to_zode(&mut value).unwrap();
        value
    }

    #[test]
    fn command_aliases_to_zed_actions_move_to_zode() {
        let after = migrate(json!({
            "command_aliases": { "Q": "zed::Quit", "W": "workspace::Save" },
            "macos": { "command_aliases": { "S": "zed::OpenSettings" } }
        }));
        assert_eq!(
            after,
            json!({
                "command_aliases": { "Q": "zode::Quit", "W": "workspace::Save" },
                "macos": { "command_aliases": { "S": "zode::OpenSettings" } }
            })
        );
    }

    #[test]
    fn zed_font_names_move_to_zode_names_or_the_family_they_aliased() {
        let after = migrate(json!({
            "buffer_font_family": "Zed Plex Mono",
            "buffer_font_fallbacks": [".ZedMono", "Menlo"],
            "ui_font_family": ".ZedSans",
            "ui_font_fallbacks": ["Zed Plex Sans"],
            "terminal": { "font_family": ".ZedMono", "font_fallbacks": ["Zed Plex Mono"] },
            "linux": { "ui_font_family": "Zed Plex Sans" }
        }));
        assert_eq!(
            after,
            json!({
                "buffer_font_family": "Lilex",
                "buffer_font_fallbacks": [".ZodeMono", "Menlo"],
                "ui_font_family": ".ZodeSans",
                "ui_font_fallbacks": ["IBM Plex Sans"],
                "terminal": { "font_family": ".ZodeMono", "font_fallbacks": ["Lilex"] },
                "linux": { "ui_font_family": "IBM Plex Sans" }
            })
        );
    }

    #[test]
    fn migrating_twice_equals_migrating_once() {
        let once = migrate(json!({
            "buffer_font_family": "Zed Plex Mono",
            "command_aliases": { "Q": "zed::Quit" }
        }));
        assert_eq!(migrate(once.clone()), once);
    }

    #[test]
    fn settings_without_zed_names_are_untouched() {
        let before = json!({ "theme": "One Dark", "command_aliases": { "W": "workspace::Save" } });
        assert_eq!(migrate(before.clone()), before);
    }
}
