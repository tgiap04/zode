use std::ops::Range;

use tree_sitter::{Query, QueryMatch};

use crate::MigrationPatterns;

/// Matches a binding whose action is a bare string (`"cmd-q": "zed::Quit"`) or the
/// first element of an array (`"cmd-=": ["zed::IncreaseUiFontSize", {...}]`). The
/// shared array patterns only cover a fixed second element, and the renamed
/// actions take every argument shape.
const KEYMAP_ACTION_NAME_PATTERN: &str = r#"(document
    (array
        (object
            (pair
                key: (string (string_content) @name)
                value: (object
                    (pair
                        key: (string)
                        value: [
                            (string (string_content) @action_name)
                            (array . (string (string_content) @action_name))
                        ]
                    )
                )
            )
        )
    )
    (#eq? @name "bindings")
)"#;

pub const KEYMAP_PATTERNS: MigrationPatterns =
    &[(KEYMAP_ACTION_NAME_PATTERN, rename_zed_namespace_action)];

fn rename_zed_namespace_action(
    contents: &str,
    mat: &QueryMatch,
    query: &Query,
) -> Option<(Range<usize>, String)> {
    let action_name_ix = query.capture_index_for_name("action_name")?;
    let action_name_range = mat
        .nodes_for_capture_index(action_name_ix)
        .next()?
        .byte_range();
    let action_name = contents.get(action_name_range.clone())?;
    let renamed = rename_action(action_name)?;
    Some((action_name_range, renamed))
}

/// Actions whose own names said Zed, renamed along with the namespace.
const RENAMED_ACTIONS: &[(&str, &str)] = &[
    ("zed::OpenZedUrl", "zode::OpenZodeUrl"),
    ("zed::OpenZedRepo", "zode::OpenZodeRepo"),
    ("cli::RegisterZedScheme", "cli::RegisterZodeScheme"),
];

pub(crate) fn rename_action(action_name: &str) -> Option<String> {
    if let Some((_, renamed)) = RENAMED_ACTIONS.iter().find(|(old, _)| *old == action_name) {
        return Some(renamed.to_string());
    }
    action_name
        .strip_prefix("zed::")
        .map(|name| format!("zode::{name}"))
}

#[cfg(test)]
mod tests {
    use crate::migrate_keymap;

    #[test]
    fn string_and_array_actions_move_to_the_zode_namespace() {
        let migrated = migrate_keymap(
            r#"[
  {
    "bindings": {
      "cmd-q": "zed::Quit",
      "cmd-=": ["zed::IncreaseUiFontSize", { "persist": false }],
      "cmd-k": ["zed::Unbind", "editor::NewLine"]
    }
  }
]"#,
        )
        .unwrap();
        pretty_assertions::assert_eq!(
            migrated.as_deref(),
            Some(
                r#"[
  {
    "bindings": {
      "cmd-q": "zode::Quit",
      "cmd-=": ["zode::IncreaseUiFontSize", { "persist": false }],
      "cmd-k": ["zode::Unbind", "editor::NewLine"]
    }
  }
]"#
            )
        );
    }

    #[test]
    fn actions_named_after_zed_get_their_zode_names() {
        let migrated = migrate_keymap(
            r#"[{ "bindings": { "a": "zed::OpenZedUrl", "b": "zed::OpenZedRepo", "c": "cli::RegisterZedScheme" } }]"#,
        )
        .unwrap();
        assert_eq!(
            migrated.as_deref(),
            Some(
                r#"[{ "bindings": { "a": "zode::OpenZodeUrl", "b": "zode::OpenZodeRepo", "c": "cli::RegisterZodeScheme" } }]"#
            )
        );
    }

    #[test]
    fn migrating_an_already_migrated_keymap_changes_nothing() {
        let migrated = migrate_keymap(r#"[{ "bindings": { "cmd-q": "zed::Quit" } }]"#)
            .unwrap()
            .expect("first run migrates");
        assert_eq!(migrate_keymap(&migrated).unwrap(), None);
    }

    #[test]
    fn other_namespaces_and_lookalikes_are_untouched() {
        let keymap = r#"[
  {
    "context": "Editor",
    "bindings": {
      "cmd-s": "workspace::Save",
      "cmd-x": "zed_actions::Something",
      "cmd-y": ["task::Spawn", { "task_name": "zed::Quit" }]
    }
  }
]"#;
        assert_eq!(migrate_keymap(keymap).unwrap(), None);
    }
}
