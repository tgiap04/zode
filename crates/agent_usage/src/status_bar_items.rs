//! The right-click menu's table of what can be switched on and off.
//!
//! ```text
//! ✓ ✳ Claude Usage
//! ✓ ◎ Codex Usage
//! ─────────────────
//! ✓ 📄 Active File Name
//! ✓ 🔤 Active Language
//! ✓ ⌶  Cursor Position
//!   ↵  Line Endings
//! ✓ ⎘  Active Encoding
//! ```
//!
//! Five of these belong to other crates' status items. Switching them from here is
//! fine because nothing reaches into another entity: each row reads one field of
//! `status_bar` and writes one field back, and the item that owns it re-reads the
//! setting on its own next render.
//!
//! The reference screenshot this menu comes from lists things this build does not
//! have — other agents, remote hosts, a resource manager, ports. Those rows are not
//! reproduced: a menu entry that leads nowhere is worse than a shorter menu.

use settings::{EncodingDisplayOptions, StatusBarSettingsContent};
use ui::IconName;
use workspace::StatusBarSettings;

/// One switchable status-bar item.
pub(crate) struct ToggleableItem {
    pub label: &'static str,
    pub icon: IconName,
    /// Whether it is currently shown.
    pub read: fn(&StatusBarSettings) -> bool,
    /// Writes the opposite of what `read` returned.
    pub write: fn(&mut StatusBarSettingsContent, bool),
    /// Whether a separator goes above this row.
    pub starts_group: bool,
}

/// The whole menu, in the order it is drawn.
///
/// A table rather than eight hand-written builder calls, so the label, the glyph,
/// the getter and the setter for one row sit on one line and cannot drift apart.
pub(crate) const TOGGLEABLE_ITEMS: &[ToggleableItem] = &[
    ToggleableItem {
        label: "Claude Usage",
        icon: IconName::AiClaude,
        read: |settings| settings.claude_usage_button,
        write: |content, on| content.claude_usage_button = Some(on),
        starts_group: false,
    },
    ToggleableItem {
        label: "Codex Usage",
        icon: IconName::AiOpenAi,
        read: |settings| settings.codex_usage_button,
        write: |content, on| content.codex_usage_button = Some(on),
        starts_group: false,
    },
    ToggleableItem {
        label: "Active File Name",
        icon: IconName::File,
        read: |settings| settings.show_active_file,
        write: |content, on| content.show_active_file = Some(on),
        starts_group: true,
    },
    ToggleableItem {
        label: "Active Language",
        icon: IconName::Code,
        read: |settings| settings.active_language_button,
        write: |content, on| content.active_language_button = Some(on),
        starts_group: false,
    },
    ToggleableItem {
        label: "Cursor Position",
        icon: IconName::CursorIBeam,
        read: |settings| settings.cursor_position_button,
        write: |content, on| content.cursor_position_button = Some(on),
        starts_group: false,
    },
    ToggleableItem {
        label: "Line Endings",
        icon: IconName::Return,
        read: |settings| settings.line_endings_button,
        write: |content, on| content.line_endings_button = Some(on),
        starts_group: false,
    },
    ToggleableItem {
        label: "Active Encoding",
        icon: IconName::Info,
        // Three states pressed into a checkbox, and the mapping is not symmetric:
        // `NonUtf8` shows the button some of the time, so the box is ticked. See
        // `encoding_for_toggle` for why switching it back on does not restore it.
        read: |settings| settings.active_encoding_button != EncodingDisplayOptions::Disabled,
        write: |content, on| content.active_encoding_button = Some(encoding_for_toggle(on)),
        starts_group: false,
    },
];

/// What ticking or clearing the encoding row writes.
///
/// The one row here whose setting is not a `bool`. Clearing it is unambiguous —
/// `Disabled` — but ticking it has to pick between `Enabled` and `NonUtf8`, and
/// `NonUtf8` is what the shipped default is. So a user who switches it off and
/// back on lands on `Enabled` rather than on the conditional default they may have
/// had before, which is a real if small loss: a checkbox cannot express three
/// states, and this is where that shows.
///
/// Named and tested rather than inlined precisely so the loss is on the record.
pub(crate) fn encoding_for_toggle(on: bool) -> EncodingDisplayOptions {
    if on {
        EncodingDisplayOptions::Enabled
    } else {
        EncodingDisplayOptions::Disabled
    }
}

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
            active_encoding_button: EncodingDisplayOptions::NonUtf8,
            claude_usage_button: true,
            codex_usage_button: true,
            agent_usage_display: settings::AgentUsageDisplay::Detailed,
        }
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
            vec![
                ("Claude Usage", true),
                ("Codex Usage", true),
                ("Active File Name", false),
                ("Active Language", true),
                ("Cursor Position", true),
                ("Line Endings", false),
                ("Active Encoding", true),
            ]
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

    /// `NonUtf8` counts as shown, because it does show the button some of the time.
    ///
    /// The row would otherwise read as off while the encoding indicator was
    /// visibly on screen.
    #[test]
    fn the_conditional_encoding_default_counts_as_shown() {
        let encoding = TOGGLEABLE_ITEMS
            .iter()
            .find(|item| item.label == "Active Encoding")
            .unwrap();

        let mut settings = settings();
        settings.active_encoding_button = EncodingDisplayOptions::NonUtf8;
        assert!((encoding.read)(&settings), "non_utf8 shows the button");

        settings.active_encoding_button = EncodingDisplayOptions::Enabled;
        assert!((encoding.read)(&settings));

        settings.active_encoding_button = EncodingDisplayOptions::Disabled;
        assert!(!(encoding.read)(&settings));
    }

    /// Switching the encoding row off and on lands on `Enabled`, not back on
    /// `NonUtf8`.
    ///
    /// This is a real loss, asserted so it is a decision rather than a surprise: a
    /// checkbox has two states and this setting has three, so the conditional
    /// middle state cannot survive a round trip through the tick.
    #[test]
    fn the_encoding_round_trip_does_not_restore_the_conditional_state() {
        assert_eq!(encoding_for_toggle(false), EncodingDisplayOptions::Disabled);
        assert_eq!(
            encoding_for_toggle(true),
            EncodingDisplayOptions::Enabled,
            "off-then-on gives `enabled`; `non_utf8` is only reachable from settings"
        );
    }
}
