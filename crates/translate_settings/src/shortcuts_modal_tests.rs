use std::sync::Arc;

use fs::{FakeFs, Fs as _};
use gpui::{App, Entity, KeybindingKeystroke, Keystroke, TestAppContext, VisualTestContext};
use schemars::JsonSchema;
use serde::Deserialize;
use settings::{KeybindSource, KeymapFile, KeymapFileLoadResult, SettingsStore};

use crate::language_pairs::{TRANSLATE_SELECTION_ACTION, active_language_pairs};
use crate::shortcuts_modal::ShortcutsModal;

/// Stands in for the real action, which lives in a crate this one must not
/// depend on; the keymap only knows it by name and arguments.
#[derive(Clone, PartialEq, Deserialize, JsonSchema, gpui::Action)]
#[action(namespace = translate)]
struct TranslateSelection {
    from: String,
    to: String,
}

gpui::actions!(test_only, [OtherAction]);

const DEFAULT_KEYMAP: &str = r#"[
  {
    "context": "Editor",
    "bindings": {
      "alt-shift-1": ["translate::TranslateSelection", { "from": "vi", "to": "en" }],
      "alt-@": ["translate::TranslateSelection", { "from": "en", "to": "vi" }],
      "alt-shift-3": "test_only::OtherAction"
    }
  }
]"#;

const USER_KEYMAP: &str = "// My own bindings.\n[]\n";

async fn set_up(
    cx: &mut TestAppContext,
) -> (Arc<FakeFs>, Entity<ShortcutsModal>, &mut VisualTestContext) {
    set_up_with(DEFAULT_KEYMAP, USER_KEYMAP, cx).await
}

/// Binds `default_keymap` the way the app binds its bundled keymap: entries
/// whose action is not linked into this test are skipped, the rest load.
pub(crate) async fn set_up_with<'a>(
    default_keymap: &str,
    user_keymap: &str,
    cx: &'a mut TestAppContext,
) -> (
    Arc<FakeFs>,
    Entity<ShortcutsModal>,
    &'a mut VisualTestContext,
) {
    let fs = FakeFs::new(cx.executor());
    cx.update(|cx| {
        let store = SettingsStore::test(cx);
        cx.set_global(store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        editor::init(cx);
        bind_keymaps(default_keymap, user_keymap, cx);
    });
    if let Some(config_dir) = paths::keymap_file().parent() {
        fs.create_dir(config_dir)
            .await
            .expect("create the config dir");
    }
    fs.insert_file(paths::keymap_file(), user_keymap.as_bytes().to_vec())
        .await;
    let (modal, cx) = cx.add_window_view({
        let fs = fs.clone();
        move |window, cx| ShortcutsModal::new(fs, window, cx)
    });
    cx.run_until_parked();
    (fs, modal, cx)
}

/// Replaces every binding with `default_keymap` and `user_keymap` on top, the
/// way the app reloads its keymaps after the user keymap file changes.
fn bind_keymaps(default_keymap: &str, user_keymap: &str, cx: &mut App) {
    cx.clear_key_bindings();
    let defaults = match KeymapFile::load(default_keymap, cx) {
        KeymapFileLoadResult::Success { key_bindings }
        | KeymapFileLoadResult::SomeFailedToLoad { key_bindings, .. } => key_bindings,
        KeymapFileLoadResult::JsonParseFailure { error } => {
            panic!("the default keymap parses: {error}")
        }
    };
    cx.bind_keys(
        defaults
            .into_iter()
            .map(|binding| binding.with_meta(KeybindSource::Default.meta())),
    );
    let user = KeymapFile::load_panic_on_failure(user_keymap, cx)
        .into_iter()
        .map(|binding| binding.with_meta(KeybindSource::User.meta()));
    cx.bind_keys(user);
}

/// Reloads the keymaps from the user keymap the dialog wrote, then lets the
/// dialog see the change, as the app does on every keymap file change.
pub(crate) async fn reload_keymaps(
    default_keymap: &str,
    fs: &FakeFs,
    modal: &Entity<ShortcutsModal>,
    cx: &mut VisualTestContext,
) {
    let written = user_keymap(fs).await;
    cx.update(|_, cx| bind_keymaps(default_keymap, &written, cx));
    modal.update_in(cx, |modal, window, cx| modal.reload(window, cx));
}

pub(crate) fn row_index(
    modal: &Entity<ShortcutsModal>,
    from: &str,
    to: &str,
    cx: &mut VisualTestContext,
) -> usize {
    modal
        .update(cx, |modal, _| {
            modal
                .pairs()
                .iter()
                .position(|pair| pair.from == from && pair.to == to)
        })
        .unwrap_or_else(|| panic!("a row for {from}→{to}"))
}

pub(crate) fn keystrokes(source: &str) -> Vec<KeybindingKeystroke> {
    source
        .split_whitespace()
        .filter_map(|keystroke| Keystroke::parse(keystroke).ok())
        .map(KeybindingKeystroke::from_keystroke)
        .collect()
}

pub(crate) fn record(
    input: Entity<keymap_editor::keystroke_input::KeystrokeInput>,
    source: &str,
    cx: &mut VisualTestContext,
) {
    input.update(cx, |input, cx| input.set_keystrokes(keystrokes(source), cx));
}

pub(crate) async fn user_keymap(fs: &FakeFs) -> String {
    fs.load(paths::keymap_file()).await.unwrap_or_default()
}

/// The user keymap loaded the way the app loads it, on top of the defaults.
pub(crate) fn bind_user_keymap(contents: &str, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        let user = KeymapFile::load_panic_on_failure(contents, cx)
            .into_iter()
            .map(|binding| binding.with_meta(KeybindSource::User.meta()));
        cx.bind_keys(user);
    });
}

pub(crate) fn pair_names(cx: &mut VisualTestContext) -> Vec<(String, String, String)> {
    cx.update(|_, cx| {
        active_language_pairs(cx)
            .into_iter()
            .map(|pair| {
                let keys = pair
                    .keystrokes
                    .iter()
                    .map(|keystroke| keystroke.inner().unparse())
                    .collect::<Vec<_>>()
                    .join(" ");
                (pair.from, pair.to, keys)
            })
            .collect()
    })
}

#[gpui::test]
async fn lists_the_pairs_of_the_active_keymap(cx: &mut TestAppContext) {
    let (_, modal, cx) = set_up(cx).await;
    let pairs = modal.update(cx, |modal, _| {
        modal
            .pairs()
            .into_iter()
            .map(|pair| (pair.from, pair.to))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        pairs,
        vec![
            ("vi".to_string(), "en".to_string()),
            ("en".to_string(), "vi".to_string())
        ]
    );
}

#[gpui::test]
async fn a_conflicting_shortcut_warns_and_writes_nothing(cx: &mut TestAppContext) {
    let (fs, modal, cx) = set_up(cx).await;
    let input = modal
        .update(cx, |modal, _| modal.row_input(1))
        .expect("en→vi row");
    record(input, "alt-shift-3", cx);

    modal.update(cx, |modal, cx| modal.save_row(1, cx));
    cx.run_until_parked();

    let warning = modal.update(cx, |modal, _| modal.warning());
    assert!(
        warning
            .as_ref()
            .is_some_and(|warning| warning.contains("test_only::OtherAction")),
        "{warning:?}"
    );
    assert_eq!(user_keymap(&fs).await, USER_KEYMAP);

    modal.update(cx, |modal, cx| modal.save_row(1, cx));
    cx.run_until_parked();
    let written = user_keymap(&fs).await;
    assert!(
        written.contains("alt-shift-3") && written.contains(TRANSLATE_SELECTION_ACTION),
        "saving again while warned writes the shortcut:\n{written}"
    );
}

#[gpui::test]
async fn adding_a_pair_writes_a_user_binding(cx: &mut TestAppContext) {
    let (fs, modal, cx) = set_up(cx).await;
    modal.update(cx, |modal, cx| modal.set_new_pair_languages("ja", "vi", cx));
    let input = modal.update(cx, |modal, _| modal.new_pair_input());
    record(input, "alt-shift-5", cx);

    modal.update(cx, |modal, cx| modal.add_pair(cx));
    cx.run_until_parked();

    let written = user_keymap(&fs).await;
    assert!(written.starts_with("// My own bindings."), "{written}");
    let added = cx.update(|_, cx| {
        KeymapFile::load_panic_on_failure(&written, cx)
            .into_iter()
            .filter(|binding| binding.action().name() == TRANSLATE_SELECTION_ACTION)
            .map(|binding| {
                let arguments: serde_json::Value = binding
                    .action_input()
                    .and_then(|input| serde_json::from_str(&input).ok())
                    .unwrap_or_default();
                (
                    binding.predicate().map(|predicate| predicate.to_string()),
                    binding
                        .keystrokes()
                        .iter()
                        .map(|keystroke| keystroke.inner().unparse())
                        .collect::<Vec<_>>(),
                    arguments,
                )
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(
        added,
        ["Editor", "Terminal", "Markdown"]
            .map(|context| (
                Some(context.to_string()),
                vec!["alt-shift-5".to_string()],
                serde_json::json!({ "from": "ja", "to": "vi" })
            ))
            .to_vec(),
        "a new pair is bound wherever the bundled pairs are"
    );

    bind_user_keymap(&written, cx);
    assert!(
        pair_names(cx).contains(&("ja".into(), "vi".into(), "alt-shift-5".into())),
        "{:?}",
        pair_names(cx)
    );
}

#[gpui::test]
async fn deleting_a_default_pair_writes_its_removal(cx: &mut TestAppContext) {
    let (fs, modal, cx) = set_up(cx).await;

    modal.update(cx, |modal, cx| modal.delete_row(1, cx));
    cx.run_until_parked();

    let written = user_keymap(&fs).await;
    assert!(written.contains("unbind"), "{written}");
    bind_user_keymap(&written, cx);
    assert_eq!(
        pair_names(cx),
        vec![("vi".into(), "en".into(), "alt-shift-1".into())],
        "the removed default no longer fires"
    );
}

#[gpui::test]
async fn an_incomplete_new_pair_is_refused_without_writing(cx: &mut TestAppContext) {
    let (fs, modal, cx) = set_up(cx).await;

    modal.update(cx, |modal, cx| modal.add_pair(cx));
    cx.run_until_parked();
    let error = modal.update(cx, |modal, _| modal.error());
    assert!(
        error.as_ref().is_some_and(|error| error.contains("Record")),
        "{error:?}"
    );

    modal.update(cx, |modal, cx| modal.set_new_pair_languages("ja", "ja", cx));
    let input = modal.update(cx, |modal, _| modal.new_pair_input());
    record(input, "alt-shift-6", cx);
    modal.update(cx, |modal, cx| modal.add_pair(cx));
    cx.run_until_parked();
    let error = modal.update(cx, |modal, _| modal.error());
    assert!(
        error
            .as_ref()
            .is_some_and(|error| error.contains("different")),
        "{error:?}"
    );

    assert_eq!(user_keymap(&fs).await, USER_KEYMAP);
}

#[gpui::test]
async fn a_second_edit_waits_for_the_first_write_and_the_reload(cx: &mut TestAppContext) {
    let (fs, modal, cx) = set_up(cx).await;
    let input = modal
        .update(cx, |modal, _| modal.row_input(0))
        .expect("vi→en row");
    record(input, "alt-shift-7", cx);

    modal.update(cx, |modal, cx| modal.save_row(0, cx));
    modal.update(cx, |modal, cx| modal.delete_row(1, cx));
    cx.run_until_parked();
    let written = user_keymap(&fs).await;
    assert!(
        written.contains("alt-shift-7"),
        "the first write is not cancelled by a second edit:\n{written}"
    );
    assert!(
        !written.contains("alt-@"),
        "removing en→vi during the write is refused:\n{written}"
    );

    modal.update(cx, |modal, cx| modal.delete_row(1, cx));
    cx.run_until_parked();
    assert!(
        !user_keymap(&fs).await.contains("alt-@"),
        "rows are stale until the keymap reloads, so edits wait for it"
    );

    reload_keymaps(DEFAULT_KEYMAP, &fs, &modal, cx).await;
    let en_vi = row_index(&modal, "en", "vi", cx);
    modal.update(cx, |modal, cx| modal.delete_row(en_vi, cx));
    cx.run_until_parked();
    let written = user_keymap(&fs).await;
    assert!(written.contains("alt-@"), "{written}");
    reload_keymaps(DEFAULT_KEYMAP, &fs, &modal, cx).await;
    assert_eq!(
        pair_names(cx),
        vec![("vi".into(), "en".into(), "alt-shift-7".into())]
    );
}

#[gpui::test]
async fn an_unrelated_keymap_reload_keeps_a_recording_in_progress(cx: &mut TestAppContext) {
    let (fs, modal, cx) = set_up(cx).await;
    let input = modal
        .update(cx, |modal, _| modal.row_input(0))
        .expect("vi→en row");
    record(input, "alt-shift-7", cx);

    reload_keymaps(DEFAULT_KEYMAP, &fs, &modal, cx).await;

    let recorded = modal.update(cx, |modal, cx| {
        modal.row_input(0).map(|input| {
            input
                .read(cx)
                .keystrokes()
                .iter()
                .map(|keystroke| keystroke.inner().unparse())
                .collect::<Vec<_>>()
        })
    });
    assert_eq!(recorded, Some(vec!["alt-shift-7".to_string()]));
}

const KEYMAP_WITH_WIDER_CONTEXTS: &str = r#"[
  {
    "context": "Editor",
    "bindings": {
      "alt-shift-1": ["translate::TranslateSelection", { "from": "vi", "to": "en" }]
    }
  },
  { "bindings": { "alt-shift-8": "test_only::OtherAction" } },
  { "context": "Workspace", "bindings": { "alt-shift-9": "test_only::OtherAction" } },
  { "context": "Pane", "bindings": { "ctrl-alt-3": "test_only::OtherAction" } },
  {
    "context": "Editor && mode == full",
    "bindings": { "ctrl-alt-1": "test_only::OtherAction" }
  },
  { "context": "ProjectPanel", "bindings": { "ctrl-alt-2": "test_only::OtherAction" } }
]"#;

#[gpui::test]
async fn the_conflict_check_sees_wider_and_deeper_contexts(cx: &mut TestAppContext) {
    let (fs, modal, cx) = set_up_with(KEYMAP_WITH_WIDER_CONTEXTS, USER_KEYMAP, cx).await;
    let input = modal
        .update(cx, |modal, _| modal.row_input(0))
        .expect("vi→en row");

    for keys in ["alt-shift-8", "alt-shift-9", "ctrl-alt-3", "ctrl-alt-1"] {
        record(input.clone(), keys, cx);
        modal.update(cx, |modal, cx| modal.save_row(0, cx));
        let warning = modal.update(cx, |modal, _| modal.warning());
        assert!(
            warning
                .as_ref()
                .is_some_and(|warning| warning.contains("test_only::OtherAction")),
            "{keys}: {warning:?}"
        );
    }
    cx.run_until_parked();
    assert_eq!(user_keymap(&fs).await, USER_KEYMAP);

    record(input, "ctrl-alt-2", cx);
    modal.update(cx, |modal, cx| modal.save_row(0, cx));
    assert_eq!(
        modal.update(cx, |modal, _| modal.warning()),
        None,
        "a binding in a view that holds no editor is no conflict"
    );
}
