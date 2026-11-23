//! Editing the language pairs and their shortcuts. Each pair is a keybinding
//! of `translate::TranslateSelection`, so every change here is a change to the
//! user keymap, made the way the keymap editor makes it.

use std::sync::Arc;
use std::time::Duration;

use fs::Fs;
use gpui::{
    App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, KeybindingKeystroke, Render,
    SharedString, Subscription, Task, Window,
};
use keymap_editor::{
    KeymapEventChannel, keystroke_input::KeystrokeInput, keystrokes_match_exactly,
};
use translate::{known_languages, language_name};
use ui::{
    ContextMenu, DropdownMenu, DropdownStyle, Modal, ModalHeader, Section, Tooltip, prelude::*,
};
use workspace::ModalView;

use crate::language_pairs::{
    LanguagePair, PairEdit, active_language_pairs, conflicts_for, contexts_for_new_pair,
    write_pair_edit,
};

/// How long to wait for the app to reload the keymap after a write before the
/// controls come back anyway, so a reload that never comes cannot lock the
/// dialog.
const RELOAD_FALLBACK: Duration = Duration::from_secs(5);

/// Every edit is a read-modify-write of the user keymap built from `rows`, so
/// a second edit waits until the first is written and the keymap has been
/// reloaded; before that the rows describe the keymap as it was.
#[derive(Clone, Copy, Debug, PartialEq)]
enum WriteState {
    Idle,
    Writing,
    AwaitingReload,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum EditTarget {
    Row(usize),
    NewPair,
}

struct PairRow {
    pair: LanguagePair,
    keystroke_input: Entity<KeystrokeInput>,
    _subscription: Subscription,
}

/// A conflict shown before anything is written. Saving the same edit again
/// while it is shown writes it anyway, as the keymap editor does.
struct ConflictWarning {
    target: EditTarget,
    keystrokes: Vec<KeybindingKeystroke>,
    message: SharedString,
}

pub struct ShortcutsModal {
    fs: Arc<dyn Fs>,
    rows: Vec<PairRow>,
    new_from: String,
    new_to: String,
    new_pair_input: Entity<KeystrokeInput>,
    warning: Option<ConflictWarning>,
    notice: Option<SharedString>,
    error: Option<SharedString>,
    focus_handle: FocusHandle,
    write_state: WriteState,
    write_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl ShortcutsModal {
    pub fn new(fs: Arc<dyn Fs>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let new_pair_input = cx.new(|cx| KeystrokeInput::new(None, window, cx));
        let mut subscriptions = vec![cx.subscribe(&new_pair_input, |this, _, _, cx| {
            this.clear_messages_for(EditTarget::NewPair, cx);
        })];
        if cx.has_global::<KeymapEventChannel>() {
            subscriptions.push(
                cx.observe_global_in::<KeymapEventChannel>(window, |this, window, cx| {
                    this.reload(window, cx)
                }),
            );
        }
        let mut this = Self {
            fs,
            rows: Vec::new(),
            new_from: "en".to_string(),
            new_to: "vi".to_string(),
            new_pair_input,
            warning: None,
            notice: None,
            error: None,
            focus_handle: cx.focus_handle(),
            write_state: WriteState::Idle,
            write_task: None,
            _subscriptions: subscriptions,
        };
        this.reload(window, cx);
        this
    }

    /// Rebuilds the rows from the active keymap, which the app reloads after
    /// every write to the keymap file. Rows are kept as they are when no pair
    /// changed, so a reload for an unrelated binding does not throw away a
    /// shortcut being recorded.
    pub(crate) fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.write_state == WriteState::AwaitingReload {
            self.write_state = WriteState::Idle;
        }
        let pairs = active_language_pairs(cx);
        let unchanged = pairs.len() == self.rows.len()
            && pairs
                .iter()
                .zip(&self.rows)
                .all(|(pair, row)| *pair == row.pair);
        if unchanged {
            cx.notify();
            return;
        }
        self.rows = pairs
            .into_iter()
            .enumerate()
            .map(|(index, pair)| {
                let keystroke_input = cx.new(|cx| {
                    let mut input = KeystrokeInput::new(None, window, cx);
                    input.set_keystrokes(pair.keystrokes.clone(), cx);
                    input
                });
                let subscription = cx.subscribe(&keystroke_input, move |this, _, _, cx| {
                    this.clear_messages_for(EditTarget::Row(index), cx);
                });
                PairRow {
                    pair,
                    keystroke_input,
                    _subscription: subscription,
                }
            })
            .collect();
        self.warning = None;
        cx.notify();
    }

    pub(crate) fn pairs(&self) -> Vec<LanguagePair> {
        self.rows.iter().map(|row| row.pair.clone()).collect()
    }

    #[cfg(test)]
    pub(crate) fn row_input(&self, index: usize) -> Option<Entity<KeystrokeInput>> {
        self.rows.get(index).map(|row| row.keystroke_input.clone())
    }

    #[cfg(test)]
    pub(crate) fn new_pair_input(&self) -> Entity<KeystrokeInput> {
        self.new_pair_input.clone()
    }

    #[cfg(test)]
    pub(crate) fn set_new_pair_languages(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        self.new_from = from.to_string();
        self.new_to = to.to_string();
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn warning(&self) -> Option<SharedString> {
        self.warning.as_ref().map(|warning| warning.message.clone())
    }

    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<SharedString> {
        self.error.clone()
    }

    fn is_busy(&self) -> bool {
        self.write_state != WriteState::Idle
    }

    pub(crate) fn save_row(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let Some(row) = self.rows.get(index) else {
            return;
        };
        let keystrokes = recorded_keystrokes(&row.keystroke_input, cx);
        if keystrokes.is_empty() || keystrokes_match_exactly(&keystrokes, &row.pair.keystrokes) {
            return;
        }
        let pair = row.pair.clone();
        let contexts = pair
            .bindings
            .iter()
            .map(|binding| binding.context.clone())
            .collect::<Vec<_>>();
        if self.warn_about_conflicts(EditTarget::Row(index), &keystrokes, &contexts, cx) {
            return;
        }
        let edit = PairEdit::Reassign { pair, keystrokes };
        self.write(edit, "Shortcut saved to your keymap.", cx);
    }

    pub(crate) fn delete_row(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let Some(row) = self.rows.get(index) else {
            return;
        };
        let edit = PairEdit::Remove {
            pair: row.pair.clone(),
        };
        self.write(edit, "Language pair removed from your keymap.", cx);
    }

    pub(crate) fn add_pair(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let keystrokes = recorded_keystrokes(&self.new_pair_input, cx);
        if keystrokes.is_empty() {
            self.error = Some("Record a shortcut for the new pair first.".into());
            cx.notify();
            return;
        }
        if self.new_from == self.new_to {
            self.error = Some("Pick two different languages.".into());
            cx.notify();
            return;
        }
        let pairs = self.pairs();
        let contexts = contexts_for_new_pair(&pairs);
        if self.warn_about_conflicts(EditTarget::NewPair, &keystrokes, &contexts, cx) {
            return;
        }
        let edit = PairEdit::Add {
            from: self.new_from.clone(),
            to: self.new_to.clone(),
            keystrokes,
            contexts,
        };
        self.new_pair_input
            .update(cx, |input, cx| input.set_keystrokes(Vec::new(), cx));
        self.write(edit, "Language pair added to your keymap.", cx);
    }

    /// Shows a warning and returns true when the keystrokes already do
    /// something in one of the contexts, unless that same warning is already
    /// on screen: then the second save is the user saying "use it anyway".
    fn warn_about_conflicts(
        &mut self,
        target: EditTarget,
        keystrokes: &[KeybindingKeystroke],
        contexts: &[Option<String>],
        cx: &mut Context<Self>,
    ) -> bool {
        let conflicts = conflicts_for(keystrokes, contexts, cx);
        if conflicts.is_empty() {
            return false;
        }
        let already_warned = self.warning.as_ref().is_some_and(|warning| {
            warning.target == target && keystrokes_match_exactly(&warning.keystrokes, keystrokes)
        });
        if already_warned {
            self.warning = None;
            return false;
        }
        let names = conflicts
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(", ");
        self.warning = Some(ConflictWarning {
            target,
            keystrokes: keystrokes.to_vec(),
            message: format!("This shortcut is already used by {names}.").into(),
        });
        self.notice = None;
        cx.notify();
        true
    }

    fn write(&mut self, edit: PairEdit, done: &'static str, cx: &mut Context<Self>) {
        let fs = self.fs.clone();
        let keyboard_mapper = cx.keyboard_mapper().clone();
        self.error = None;
        self.notice = None;
        self.write_state = WriteState::Writing;
        self.write_task = Some(cx.spawn(async move |this, cx| {
            let result = write_pair_edit(edit, fs, keyboard_mapper).await;
            let written = result.is_ok();
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.notice = Some(done.into());
                        this.write_state = WriteState::AwaitingReload;
                    }
                    Err(error) => {
                        this.error = Some(format!("{error:#}").into());
                        this.write_state = WriteState::Idle;
                    }
                }
                cx.notify();
            })
            .ok();
            if !written {
                return;
            }
            cx.background_executor().timer(RELOAD_FALLBACK).await;
            this.update(cx, |this, cx| {
                if this.write_state == WriteState::AwaitingReload {
                    log::warn!("the keymap did not reload after a translate shortcut change");
                    this.write_state = WriteState::Idle;
                    cx.notify();
                }
            })
            .ok();
        }));
        cx.notify();
    }

    fn clear_messages_for(&mut self, target: EditTarget, cx: &mut Context<Self>) {
        if self
            .warning
            .as_ref()
            .is_some_and(|warning| warning.target == target)
        {
            self.warning = None;
        }
        self.error = None;
        self.notice = None;
        cx.notify();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

fn recorded_keystrokes(input: &Entity<KeystrokeInput>, cx: &App) -> Vec<KeybindingKeystroke> {
    let mut keystrokes = input.read(cx).keystrokes().to_vec();
    for keystroke in &mut keystrokes {
        keystroke.remove_key_char();
    }
    keystrokes
}

fn language_label(code: &str) -> SharedString {
    match language_name(code) {
        Some(name) => format!("{name} ({code})").into(),
        None => code.to_string().into(),
    }
}

impl ModalView for ShortcutsModal {}

impl EventEmitter<DismissEvent> for ShortcutsModal {}

impl Focusable for ShortcutsModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ShortcutsModal {
    fn render_row(&self, index: usize, row: &PairRow, cx: &mut Context<Self>) -> AnyElement {
        let pair = &row.pair;
        let busy = self.is_busy();
        let changed = !keystrokes_match_exactly(
            &recorded_keystrokes(&row.keystroke_input, cx),
            &pair.keystrokes,
        );
        let direction = format!(
            "{} → {}",
            language_label(&pair.from),
            language_label(&pair.to)
        );
        h_flex()
            .id(("translate-pair", index))
            .w_full()
            .gap_2()
            .justify_between()
            .child(Label::new(direction).size(LabelSize::Small))
            .child(
                h_flex()
                    .gap_1()
                    .child(div().w(rems(12.)).child(row.keystroke_input.clone()))
                    .child(
                        Button::new(("translate-pair-save", index), "Save")
                            .disabled(!changed || busy)
                            .on_click(cx.listener(move |this, _, _, cx| this.save_row(index, cx))),
                    )
                    .child(
                        IconButton::new(("translate-pair-delete", index), IconName::Trash)
                            .icon_size(IconSize::Small)
                            .disabled(busy)
                            .tooltip(Tooltip::text("Remove this language pair"))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.delete_row(index, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_language_picker(
        &self,
        id: &'static str,
        selected: &str,
        is_from: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let this = cx.weak_entity();
        let selected_code = selected.to_string();
        let key = SharedString::from(format!("{id}-{selected}"));
        let menu = window.use_keyed_state(key, cx, move |window, cx| {
            ContextMenu::new(window, cx, move |mut menu, _, _| {
                for (code, _) in known_languages() {
                    let this = this.clone();
                    menu = menu.toggleable_entry(
                        language_label(code),
                        *code == selected_code,
                        IconPosition::End,
                        None,
                        move |_, cx| {
                            this.update(cx, |this, cx| {
                                if is_from {
                                    this.new_from = code.to_string();
                                } else {
                                    this.new_to = code.to_string();
                                }
                                this.clear_messages_for(EditTarget::NewPair, cx);
                            })
                            .ok();
                        },
                    );
                }
                menu
            })
        });
        DropdownMenu::new(id, language_label(selected), menu)
            .style(DropdownStyle::Outlined)
            .into_any_element()
    }

    fn render_new_pair(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let has_keystrokes = !recorded_keystrokes(&self.new_pair_input, cx).is_empty();
        v_flex()
            .gap_1()
            .child(
                Label::new("Add Language Pair")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(self.render_language_picker(
                        "translate-new-from",
                        &self.new_from,
                        true,
                        window,
                        cx,
                    ))
                    .child(Label::new("→").color(Color::Muted))
                    .child(self.render_language_picker(
                        "translate-new-to",
                        &self.new_to,
                        false,
                        window,
                        cx,
                    ))
                    .child(div().w(rems(12.)).child(self.new_pair_input.clone()))
                    .child(
                        Button::new("translate-pair-add", "Add")
                            .style(ButtonStyle::Filled)
                            .disabled(!has_keystrokes || self.is_busy())
                            .on_click(cx.listener(|this, _, _, cx| this.add_pair(cx))),
                    ),
            )
            .into_any_element()
    }
}

impl Render for ShortcutsModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| self.render_row(index, row, cx))
            .collect::<Vec<_>>();
        let empty = rows.is_empty();
        let new_pair = self.render_new_pair(window, cx);

        v_flex()
            .key_context("TranslateShortcutsModal")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .occlude()
            .w(rems(44.))
            .on_action(cx.listener(|this, _: &menu::Cancel, _, cx| this.cancel(cx)))
            .child(
                Modal::new("translate-shortcuts", None)
                    .header(
                        ModalHeader::new()
                            .child(Headline::new("Translate Shortcuts").size(HeadlineSize::Small))
                            .description(
                                "Select text and press a shortcut to translate it. \
                                 Click a shortcut to record a new one; changes go to your keymap.",
                            ),
                    )
                    .section(
                        Section::new()
                            .when(empty, |section| {
                                section.child(
                                    Label::new("No language pairs yet.")
                                        .size(LabelSize::Small)
                                        .color(Color::Muted),
                                )
                            })
                            .children(rows),
                    )
                    .section(
                        Section::new()
                            .child(new_pair)
                            .when_some(self.warning.as_ref(), |section, warning| {
                                section.child(
                                    Label::new(format!(
                                        "{} Save again to use it anyway.",
                                        warning.message
                                    ))
                                    .size(LabelSize::Small)
                                    .color(Color::Warning),
                                )
                            })
                            .when_some(self.error.clone(), |section, error| {
                                section.child(
                                    Label::new(error).size(LabelSize::Small).color(Color::Error),
                                )
                            })
                            .when_some(self.notice.clone(), |section, notice| {
                                section.child(
                                    Label::new(notice)
                                        .size(LabelSize::Small)
                                        .color(Color::Success),
                                )
                            }),
                    ),
            )
    }
}
