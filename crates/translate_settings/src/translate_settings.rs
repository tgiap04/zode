//! The two dialogs behind Settings → Translate: the API key of the translation
//! service, and the language pairs with their shortcuts.

mod api_key_modal;
mod language_pairs;
mod shortcuts_modal;

#[cfg(test)]
mod api_key_modal_tests;
#[cfg(test)]
mod bundled_keymap_pairs_tests;
#[cfg(test)]
mod shortcuts_modal_tests;

use gpui::App;
use workspace::Workspace;
use zode_actions::translate::{EditShortcuts, SetApiKey};

pub use api_key_modal::ApiKeyModal;
pub use language_pairs::{LanguagePair, TRANSLATE_SELECTION_ACTION, active_language_pairs};
pub use shortcuts_modal::ShortcutsModal;

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace
            .register_action(|workspace, _: &SetApiKey, window, cx| {
                workspace.toggle_modal(window, cx, ApiKeyModal::new);
            })
            .register_action(|workspace, _: &EditShortcuts, window, cx| {
                let fs = workspace.app_state().fs.clone();
                workspace
                    .toggle_modal(window, cx, |window, cx| ShortcutsModal::new(fs, window, cx));
            });
    })
    .detach();
}
