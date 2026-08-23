//! The right-click menu's table of what can be switched on and off.
//!
//! ```text
//! ✓ Claude Usage
//! ✓ Codex Usage
//! ```
//!
//! **Two rows, and no per-row glyph.** Both of those are corrections to what
//! shipped first, and both were forced by what the components actually do rather
//! than by taste:
//!
//! The reference screenshot this menu comes from lists other agents, remote hosts,
//! a resource manager and ports — none of which exist here. The first attempt
//! substituted this build's five other `status_bar.*` items to keep the menu's
//! shape. Seeing it real, that was the wrong trade: right-clicking the *usage*
//! numbers to hide the *cursor position* is a menu about something else, and those
//! five already have their own home in the settings editor.
//!
//! And the glyphs had to go because `ContextMenu` renders a toggled entry as
//! `Icon::new(icon.unwrap_or(IconName::Check))` — the tick slot **is** the icon
//! slot. Passing an icon silently replaces the checkmark with it, so every row
//! showed its own mark twice and no row showed whether it was on. A menu whose
//! entire job is to say what is on cannot spend its tick slot on decoration.

use settings::StatusBarSettingsContent;
use workspace::StatusBarSettings;

/// One switchable status-bar item.
pub(crate) struct ToggleableItem {
    pub label: &'static str,
    /// Whether it is currently shown.
    pub read: fn(&StatusBarSettings) -> bool,
    /// Writes the opposite of what `read` returned.
    pub write: fn(&mut StatusBarSettingsContent, bool),
}

/// The whole menu, in the order it is drawn.
///
/// A table rather than hand-written builder calls, so the label, the getter and
/// the setter for one row sit together and cannot drift apart.
pub(crate) const TOGGLEABLE_ITEMS: &[ToggleableItem] = &[
    ToggleableItem {
        label: "Claude Usage",
        read: |settings| settings.claude_usage_button,
        write: |content, on| content.claude_usage_button = Some(on),
    },
    ToggleableItem {
        label: "Codex Usage",
        read: |settings| settings.codex_usage_button,
        write: |content, on| content.codex_usage_button = Some(on),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> StatusBarSettings {
        StatusBarSettings {
            show: true,
            show_active_file: false,
            active_language_button: true,
            cursor_position_button: true,
            line_endings_button: false,
            active_encoding_button: settings::EncodingDisplayOptions::NonUtf8,
            claude_usage_button: true,
            codex_usage_button: false,
            agent_usage_display: settings::AgentUsageDisplay::Detailed,
        }
    }

    /// The menu is the two agents and nothing else.
    ///
    /// Asserted as an exact list because the thing that went wrong was scope:
    /// borrowing five unrelated status-bar items to pad the menu out to the
    /// reference screenshot's length.
    #[test]
    fn the_menu_is_the_two_agents_and_nothing_else() {
        let labels: Vec<&str> = TOGGLEABLE_ITEMS.iter().map(|item| item.label).collect();
        assert_eq!(labels, vec!["Claude Usage", "Codex Usage"]);
    }

    /// Every row reads the setting that is actually in force, not the default.
    ///
    /// A menu whose ticks came from the defaults would look right on a fresh
    /// install and be wrong for everyone who has ever changed a setting.
    #[test]
    fn every_row_reads_the_setting_in_force() {
        let settings = settings();
        let ticks: Vec<(&str, bool)> = TOGGLEABLE_ITEMS
            .iter()
            .map(|item| (item.label, (item.read)(&settings)))
            .collect();

        assert_eq!(
            ticks,
            vec![("Claude Usage", true), ("Codex Usage", false)],
            "the fixture switches Codex off, and the menu must say so"
        );
    }

    /// Writing a row sets that row's field and touches nothing else.
    #[test]
    fn writing_a_row_touches_only_its_own_field() {
        for item in TOGGLEABLE_ITEMS {
            let mut content = StatusBarSettingsContent::default();
            (item.write)(&mut content, false);

            let written = [
                content.claude_usage_button.is_some(),
                content.codex_usage_button.is_some(),
                content.show_active_file.is_some(),
                content.active_language_button.is_some(),
                content.cursor_position_button.is_some(),
                content.line_endings_button.is_some(),
                content.active_encoding_button.is_some(),
            ]
            .iter()
            .filter(|written| **written)
            .count();

            assert_eq!(
                written, 1,
                "{} wrote {written} fields; a toggle must write exactly one",
                item.label
            );
        }
    }

    /// The menu never writes a setting belonging to another crate's status item.
    ///
    /// The five items the first version borrowed are still perfectly settable —
    /// from the settings editor and the settings file, which is where they live.
    /// This menu must not reach them.
    #[test]
    fn the_menu_never_writes_another_items_setting() {
        for item in TOGGLEABLE_ITEMS {
            let mut content = StatusBarSettingsContent::default();
            (item.write)(&mut content, false);

            assert!(content.show_active_file.is_none());
            assert!(content.active_language_button.is_none());
            assert!(content.cursor_position_button.is_none());
            assert!(content.line_endings_button.is_none());
            assert!(content.active_encoding_button.is_none());
        }
    }
}
