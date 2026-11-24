//! The dialog against the keymaps the app ships, which bind every pair in
//! three contexts: `Editor`, `Terminal` and `Markdown`.

use gpui::{TestAppContext, VisualTestContext};
use settings::KeymapFile;

use crate::language_pairs::{LanguagePair, TRANSLATE_SELECTION_ACTION, active_language_pairs};
use crate::shortcuts_modal_tests::{
    bind_user_keymap, pair_names, record, reload_keymaps, row_index, set_up_with, user_keymap,
};

const USER_KEYMAP: &str = "[]\n";

const BUNDLED_KEYMAPS: [(&str, &str); 3] = [
    (
        "macos",
        include_str!("../../../assets/keymaps/default-macos.json"),
    ),
    (
        "linux",
        include_str!("../../../assets/keymaps/default-linux.json"),
    ),
    (
        "windows",
        include_str!("../../../assets/keymaps/default-windows.json"),
    ),
];

const PAIR_CONTEXTS: [&str; 3] = ["Editor", "Markdown", "Terminal"];

fn sorted_contexts(pair: &LanguagePair) -> Vec<String> {
    let mut contexts = pair
        .bindings
        .iter()
        .map(|binding| binding.context.clone().unwrap_or_default())
        .collect::<Vec<_>>();
    contexts.sort();
    contexts
}

fn live_pair(from: &str, to: &str, cx: &mut VisualTestContext) -> Option<LanguagePair> {
    cx.update(|_, cx| {
        active_language_pairs(cx)
            .into_iter()
            .find(|pair| pair.from == from && pair.to == to)
    })
}

/// The contexts and keystrokes of the translate bindings in `contents`.
fn written_bindings(contents: &str, cx: &mut VisualTestContext) -> Vec<(String, String)> {
    let mut bindings = cx.update(|_, cx| {
        KeymapFile::load_panic_on_failure(contents, cx)
            .into_iter()
            .filter(|binding| binding.action().name() == TRANSLATE_SELECTION_ACTION)
            .map(|binding| {
                let keys = binding
                    .keystrokes()
                    .iter()
                    .map(|keystroke| keystroke.inner().unparse())
                    .collect::<Vec<_>>()
                    .join(" ");
                (
                    binding
                        .predicate()
                        .map(|predicate| predicate.to_string())
                        .unwrap_or_default(),
                    keys,
                )
            })
            .collect::<Vec<_>>()
    });
    bindings.sort();
    bindings
}

#[gpui::test]
async fn each_bundled_pair_is_one_row_across_its_contexts(cx: &mut TestAppContext) {
    for (platform, keymap) in BUNDLED_KEYMAPS {
        let (_, modal, cx) = set_up_with(keymap, USER_KEYMAP, cx).await;
        let pairs = modal.update(cx, |modal, _| modal.pairs());
        let rows = pairs
            .iter()
            .map(|pair| (pair.from.as_str(), pair.to.as_str(), sorted_contexts(pair)))
            .collect::<Vec<_>>();
        assert_eq!(
            rows,
            vec![
                ("vi", "en", PAIR_CONTEXTS.map(String::from).to_vec()),
                ("en", "vi", PAIR_CONTEXTS.map(String::from).to_vec()),
            ],
            "on {platform}"
        );
    }
}

#[gpui::test]
async fn adding_a_pair_binds_it_in_every_bundled_context(cx: &mut TestAppContext) {
    for (platform, keymap) in BUNDLED_KEYMAPS {
        let (fs, modal, cx) = set_up_with(keymap, USER_KEYMAP, cx).await;
        modal.update(cx, |modal, cx| modal.set_new_pair_languages("ja", "vi", cx));
        let input = modal.update(cx, |modal, _| modal.new_pair_input());
        record(input, "alt-shift-5", cx);

        modal.update(cx, |modal, cx| modal.add_pair(cx));
        cx.run_until_parked();

        let written = user_keymap(&fs).await;
        assert_eq!(
            written_bindings(&written, cx),
            PAIR_CONTEXTS
                .map(|context| (context.to_string(), "alt-shift-5".to_string()))
                .to_vec(),
            "on {platform}:\n{written}"
        );
        bind_user_keymap(&written, cx);
        let added = live_pair("ja", "vi", cx).expect("the new pair is live");
        assert_eq!(
            sorted_contexts(&added),
            PAIR_CONTEXTS.map(String::from).to_vec(),
            "one row on {platform}"
        );
    }
}

#[gpui::test]
async fn a_pair_added_after_removing_every_default_still_binds_every_context(
    cx: &mut TestAppContext,
) {
    let (_, keymap) = BUNDLED_KEYMAPS[0];
    let (fs, modal, cx) = set_up_with(keymap, USER_KEYMAP, cx).await;
    for _ in 0..2 {
        modal.update(cx, |modal, cx| modal.delete_row(0, cx));
        cx.run_until_parked();
        reload_keymaps(keymap, &fs, &modal, cx).await;
    }
    assert_eq!(pair_names(cx), Vec::new(), "both defaults are gone");

    modal.update(cx, |modal, cx| modal.set_new_pair_languages("ja", "vi", cx));
    let input = modal.update(cx, |modal, _| modal.new_pair_input());
    record(input, "alt-shift-5", cx);
    modal.update(cx, |modal, cx| modal.add_pair(cx));
    cx.run_until_parked();

    let written = user_keymap(&fs).await;
    let added = written_bindings(&written, cx)
        .into_iter()
        .filter(|(_, keys)| keys == "alt-shift-5")
        .collect::<Vec<_>>();
    assert_eq!(
        added,
        PAIR_CONTEXTS
            .map(|context| (context.to_string(), "alt-shift-5".to_string()))
            .to_vec(),
        "{written}"
    );
}

#[gpui::test]
async fn reassigning_a_bundled_pair_moves_it_in_every_context(cx: &mut TestAppContext) {
    for (platform, keymap) in BUNDLED_KEYMAPS {
        let (fs, modal, cx) = set_up_with(keymap, USER_KEYMAP, cx).await;
        let input = modal
            .update(cx, |modal, _| modal.row_input(0))
            .expect("vi→en row");
        record(input, "alt-shift-7", cx);

        modal.update(cx, |modal, cx| modal.save_row(0, cx));
        cx.run_until_parked();
        assert_eq!(modal.update(cx, |modal, _| modal.warning()), None);

        bind_user_keymap(&user_keymap(&fs).await, cx);
        let rows = cx.update(|_, cx| active_language_pairs(cx));
        let vi_en = rows
            .iter()
            .filter(|pair| pair.from == "vi" && pair.to == "en")
            .collect::<Vec<_>>();
        assert_eq!(vi_en.len(), 1, "one row on {platform}: {rows:?}");
        assert_eq!(
            pair_names(cx)
                .into_iter()
                .filter(|(from, _, _)| from == "vi")
                .collect::<Vec<_>>(),
            vec![("vi".into(), "en".into(), "alt-shift-7".into())],
            "the old shortcut no longer fires on {platform}"
        );
        assert_eq!(
            sorted_contexts(vi_en[0]),
            PAIR_CONTEXTS.map(String::from).to_vec(),
            "on {platform}"
        );
    }
}

#[gpui::test]
async fn deleting_a_bundled_pair_unbinds_it_in_every_context(cx: &mut TestAppContext) {
    for (platform, keymap) in BUNDLED_KEYMAPS {
        let (fs, modal, cx) = set_up_with(keymap, USER_KEYMAP, cx).await;

        modal.update(cx, |modal, cx| modal.delete_row(0, cx));
        cx.run_until_parked();
        assert_eq!(modal.update(cx, |modal, _| modal.error()), None);

        bind_user_keymap(&user_keymap(&fs).await, cx);
        assert_eq!(live_pair("vi", "en", cx), None, "gone on {platform}");
        let kept = live_pair("en", "vi", cx).expect("the other pair stays");
        assert_eq!(
            sorted_contexts(&kept),
            PAIR_CONTEXTS.map(String::from).to_vec(),
            "on {platform}"
        );
    }
}

#[gpui::test]
async fn the_conflict_check_covers_every_bundled_context(cx: &mut TestAppContext) {
    let (_, keymap) = BUNDLED_KEYMAPS[0];
    let (fs, modal, cx) = set_up_with(keymap, USER_KEYMAP, cx).await;
    let input = modal
        .update(cx, |modal, _| modal.row_input(0))
        .expect("vi→en row");

    record(input.clone(), "cmd-c", cx);
    modal.update(cx, |modal, cx| modal.save_row(0, cx));
    let warning = modal
        .update(cx, |modal, _| modal.warning())
        .unwrap_or_default();
    for action in ["editor::Copy", "terminal::Copy", "markdown::Copy"] {
        assert!(warning.contains(action), "{action} in {warning:?}");
    }

    // Bound in plain `Terminal`, which also matches while text is selected.
    record(input, "cmd-k", cx);
    modal.update(cx, |modal, cx| modal.save_row(0, cx));
    let warning = modal
        .update(cx, |modal, _| modal.warning())
        .unwrap_or_default();
    assert!(warning.contains("terminal::Clear"), "{warning:?}");

    cx.run_until_parked();
    assert_eq!(user_keymap(&fs).await, USER_KEYMAP);
}

/// Moving a default pair away writes an `unbind` for its old shortcut; moving
/// it back must not leave the new binding behind that `unbind`.
#[gpui::test]
async fn a_pair_moved_back_to_its_default_shortcut_stays_live(cx: &mut TestAppContext) {
    for (platform, keymap) in BUNDLED_KEYMAPS {
        let (fs, modal, cx) = set_up_with(keymap, USER_KEYMAP, cx).await;
        let index = row_index(&modal, "vi", "en", cx);
        let default_keys = modal.update(cx, |modal, _| modal.pairs()[index].keystrokes.clone());
        let input = modal
            .update(cx, |modal, _| modal.row_input(index))
            .expect("vi→en row");
        record(input, "alt-shift-7", cx);
        modal.update(cx, |modal, cx| modal.save_row(index, cx));
        cx.run_until_parked();
        reload_keymaps(keymap, &fs, &modal, cx).await;

        let index = row_index(&modal, "vi", "en", cx);
        let input = modal
            .update(cx, |modal, _| modal.row_input(index))
            .expect("vi→en row");
        input.update(cx, |input, cx| {
            input.set_keystrokes(default_keys.clone(), cx)
        });
        modal.update(cx, |modal, cx| modal.save_row(index, cx));
        assert_eq!(
            modal.update(cx, |modal, _| modal.warning()),
            None,
            "on {platform}"
        );
        cx.run_until_parked();
        reload_keymaps(keymap, &fs, &modal, cx).await;

        let written = user_keymap(&fs).await;
        let pair = live_pair("vi", "en", cx)
            .unwrap_or_else(|| panic!("the pair is still live on {platform}:\n{written}"));
        let unparsed = |keys: &[gpui::KeybindingKeystroke]| {
            keys.iter()
                .map(|keystroke| keystroke.inner().unparse())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            unparsed(&pair.keystrokes),
            unparsed(&default_keys),
            "on {platform}"
        );
        assert_eq!(
            sorted_contexts(&pair),
            PAIR_CONTEXTS.map(String::from).to_vec(),
            "on {platform}:\n{written}"
        );
    }
}

#[gpui::test]
async fn a_workspace_shortcut_is_a_conflict(cx: &mut TestAppContext) {
    let (_, keymap) = BUNDLED_KEYMAPS[0];
    let (fs, modal, cx) = set_up_with(keymap, USER_KEYMAP, cx).await;
    let input = modal
        .update(cx, |modal, _| modal.row_input(0))
        .expect("vi→en row");

    record(input, "cmd-shift-p", cx);
    modal.update(cx, |modal, cx| modal.save_row(0, cx));
    let warning = modal
        .update(cx, |modal, _| modal.warning())
        .unwrap_or_default();
    assert!(warning.contains("command_palette::Toggle"), "{warning:?}");
    cx.run_until_parked();
    assert_eq!(user_keymap(&fs).await, USER_KEYMAP);
}
