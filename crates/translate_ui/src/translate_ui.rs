//! Translates the selected text with a keybinding and shows the result in a
//! popover where the selection is, with a Replace button when the text came
//! from an editor that can be changed.
//!
//! The popover is fed a [`SelectionSource`], so any view that can say what is
//! selected and where it is on screen can use it: `Editor`, the terminal and
//! every view that shows `Markdown`.

mod editor_source;
mod hidden_characters;
mod markdown_source;
mod terminal_source;
mod translation_popover;

#[cfg(test)]
mod editor_hosted_markdown_tests;
#[cfg(test)]
mod input_field_tests;
#[cfg(test)]
mod popover_lifetime_tests;
#[cfg(test)]
mod read_only_sources_tests;
#[cfg(test)]
mod terminal_program_copy_tests;
#[cfg(test)]
mod translate_ui_tests;
#[cfg(test)]
mod untrusted_text_tests;

use std::collections::HashMap;

use editor::Editor;
use gpui::{
    Action, App, Bounds, Context, Entity, EntityId, Focusable, Global, Pixels, Point, WeakEntity,
    Window, px,
};
use markdown::Markdown;
use schemars::JsonSchema;
use serde::Deserialize;
use terminal_view::TerminalView;
use workspace::Workspace;

pub use editor_source::{EditorReplaceTarget, ReplaceBlocked};
pub use translation_popover::{PopoverState, SelectionSource, TranslationPopover};

/// Translates the selected text from one language to another and shows the
/// result next to the selection. Bind one key per language pair, for example
/// `["translate::TranslateSelection", { "from": "vi", "to": "en" }]`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema, Action)]
#[action(namespace = translate)]
#[serde(deny_unknown_fields)]
pub struct TranslateSelection {
    /// Language of the selected text, as a code such as `vi` or `pt-BR`.
    pub from: String,
    /// Language to translate into, as a code such as `en`.
    pub to: String,
}

pub fn init(cx: &mut App) {
    cx.observe_new(|editor: &mut Editor, _window, cx| {
        let editor_handle = cx.weak_entity();
        editor
            .register_action(move |action: &TranslateSelection, window, cx| {
                // `Markdown` drawn inside the editor (hover popovers,
                // diagnostic blocks) gets the key through this handler first;
                // passing it on lets the workspace handler translate it.
                if !editor_source::translate_selection(&editor_handle, action, window, cx) {
                    cx.propagate();
                }
            })
            .detach();
    })
    .detach();

    track_focusable::<TerminalView>(cx);
    track_focusable::<Markdown>(cx);
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace.register_action(translate_focused_read_only_selection);
    })
    .detach();
}

/// The terminal and `Markdown` take no action handlers from other crates, so
/// their keypress bubbles up to the workspace, which looks up which of them
/// holds focus. One registration covers every view that shows `Markdown`.
fn translate_focused_read_only_selection(
    workspace: &mut Workspace,
    action: &TranslateSelection,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let popover_layer = workspace.popover_layer().clone();
    let shown = if let Some(terminal_view) = focused::<TerminalView>(window, cx) {
        terminal_source::translate_selection(&terminal_view, action, &popover_layer, window, cx)
    } else if let Some(markdown) = focused::<Markdown>(window, cx) {
        markdown_source::translate_selection(&markdown, action, &popover_layer, window, cx)
    } else {
        false
    };
    if !shown {
        // A program in the terminal (vim, tmux) may want this key.
        cx.propagate();
    }
}

/// Live entities of one view type, so the focused one can be found when an
/// action reaches the workspace. Entries leave when their entity is released.
struct FocusableEntities<T> {
    by_id: HashMap<EntityId, WeakEntity<T>>,
}

impl<T> Default for FocusableEntities<T> {
    fn default() -> Self {
        Self {
            by_id: HashMap::default(),
        }
    }
}

impl<T: 'static> Global for FocusableEntities<T> {}

fn track_focusable<T: Focusable>(cx: &mut App) {
    cx.observe_new(|_: &mut T, _window, cx: &mut Context<T>| {
        let entity_id = cx.entity_id();
        let entity = cx.weak_entity();
        cx.default_global::<FocusableEntities<T>>()
            .by_id
            .insert(entity_id, entity);
        cx.on_release(move |_, cx| {
            if cx.has_global::<FocusableEntities<T>>() {
                cx.global_mut::<FocusableEntities<T>>()
                    .by_id
                    .remove(&entity_id);
            }
        })
        .detach();
    })
    .detach();
}

fn focused<T: Focusable>(window: &Window, cx: &App) -> Option<Entity<T>> {
    cx.try_global::<FocusableEntities<T>>()?
        .by_id
        .values()
        .filter_map(WeakEntity::upgrade)
        .find(|entity| entity.focus_handle(cx).is_focused(window))
}

/// The mouse position, moved inside `bounds`: the user has just selected
/// with the mouse, so it is near the text, but it may have left the view.
fn mouse_position_within(bounds: Bounds<Pixels>, window: &Window) -> Point<Pixels> {
    let mouse = window.mouse_position();
    if bounds.size.width <= px(0.) || bounds.size.height <= px(0.) {
        return mouse;
    }
    mouse.clamp(&bounds.origin, &bounds.bottom_right())
}
