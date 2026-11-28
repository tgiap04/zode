use editor::Editor;
use gpui::{
    Action as _, ClipboardItem, DismissEvent, Entity, EventEmitter, FocusHandle, KeystrokeEvent,
    Modifiers, Pixels, Point, Subscription, Task, anchored, deferred,
};
use settings::Settings as _;
use translate::{
    TranslateError, TranslateRequest, TranslateSettings, language_name, provider_display_name,
};
use ui::{ContextMenu, Tooltip, prelude::*};
use util::ResultExt as _;
use workspace::{PopoverLayer, Workspace};
use zode_actions::OpenSettingsAt;

use crate::{
    EditorReplaceTarget, ReplaceBlocked,
    hidden_characters::{preview_line, remove_hidden_characters},
};

/// Where the Translate settings live; the settings editor opens at this path.
const TRANSLATE_SETTINGS_PATH: &str = "translate";

/// What a view hands the popover: the text to translate, the window point to
/// show it at, and, when the text can be written back, where to write it.
pub struct SelectionSource {
    pub text: String,
    pub position: Point<Pixels>,
    pub replace_target: Option<ReplaceTarget>,
    /// The selection's length in characters when a source measured it over
    /// the limit without copying it; `text` is then empty.
    pub too_long: Option<usize>,
    /// The text is what a program in a terminal copied by itself, not
    /// something the user selected. It is shown for the user to confirm and
    /// sent only then.
    pub program_copy: bool,
}

/// Where a translation can be written back to.
pub enum ReplaceTarget {
    /// The selected span of an editor.
    Editor(EditorReplaceTarget),
    /// The input of the program in a terminal, replaced whole.
    TerminalInput(crate::terminal_source::TerminalInputTarget),
}

impl ReplaceTarget {
    fn check(&self, original: &str, cx: &App) -> Result<(), ReplaceBlocked> {
        match self {
            Self::Editor(target) => target.check(original, cx),
            Self::TerminalInput(target) => target.check(),
        }
    }

    fn replace(
        &self,
        original: &str,
        replacement: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<(), ReplaceBlocked> {
        match self {
            Self::Editor(target) => target.replace(original, replacement, window, cx),
            Self::TerminalInput(target) => target.replace(replacement, cx),
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Editor(_) => "Replace",
            Self::TerminalInput(_) => "Replace Input",
        }
    }

    fn tooltip(&self) -> &'static str {
        match self {
            Self::Editor(_) => "Replace the selection with the translation",
            Self::TerminalInput(_) => "Clear the agent's input and paste the translation",
        }
    }
}

/// How much of a program's copy the confirmation shows.
const PROGRAM_COPY_PREVIEW_CHARACTERS: usize = 120;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PopoverState {
    Loading,
    Done(SharedString),
    Failed(TranslateError),
    TooLong {
        length: usize,
        limit: usize,
    },
    /// No provider can run until the user changes a setting or the key.
    NotConfigured(TranslateError),
    /// Text a program in the terminal copied by itself, shown for the user to
    /// confirm before it is sent anywhere.
    ConfirmProgramCopy {
        preview: SharedString,
        characters: usize,
    },
}

impl PopoverState {
    fn from_result(result: Result<String, TranslateError>) -> Self {
        match result {
            Ok(text) => Self::Done(text.into()),
            Err(TranslateError::TooLong { length, limit }) => Self::TooLong { length, limit },
            Err(
                error @ (TranslateError::MissingKey { .. }
                | TranslateError::InvalidKey { .. }
                | TranslateError::Forbidden { .. }
                | TranslateError::KeyForOtherServer { .. }
                | TranslateError::InvalidConfiguration(_)),
            ) => Self::NotConfigured(error),
            Err(error) => Self::Failed(error),
        }
    }
}

pub struct TranslationPopover {
    source: SelectionSource,
    from: String,
    to: String,
    state: PopoverState,
    /// How many hidden characters were removed from the shown translation.
    hidden_characters_removed: usize,
    /// Why the last press of Replace did nothing, when the reason passes --
    /// the agent still writing -- so the button stays to be pressed again.
    replace_notice: Option<ReplaceBlocked>,
    /// Escape closes the popover only while the focus is in here, so it
    /// still reaches a modal, the command palette or vim elsewhere.
    escape_scope: Option<FocusHandle>,
    /// Drawn inside a host that already floats it (the editor's menu), so
    /// only the card is rendered.
    hosted_in_menu: bool,
    _translation: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DismissEvent> for TranslationPopover {}

impl TranslationPopover {
    /// Shows a popover for `source` on the workspace's popover layer, starting
    /// the translation right away. It replaces any popover already there, and
    /// dropping that one cancels its request.
    pub fn deploy(
        source: SelectionSource,
        from: String,
        to: String,
        workspace: &Entity<Workspace>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let popover_layer = workspace.read(cx).popover_layer().clone();
        Self::deploy_on(source, from, to, &popover_layer, window, cx)
    }

    /// Same as [`Self::deploy`], for callers that are updating the workspace
    /// and so cannot read it.
    pub fn deploy_on(
        source: SelectionSource,
        from: String,
        to: String,
        popover_layer: &Entity<PopoverLayer>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let popover = cx.new(|cx| Self::new(source, from, to, window, cx));
        popover_layer.update(cx, |popover_layer, cx| {
            popover_layer.show_popover(popover.clone(), cx)
        });
        popover
    }

    /// Shows the popover in `editor`'s own floating menu, for windows that
    /// have no workspace. The menu takes focus, so Escape and a click outside
    /// close it the way they close any menu; closing the menu drops the
    /// popover and cancels its request.
    pub fn deploy_in_editor(
        source: SelectionSource,
        from: String,
        to: String,
        editor: &Entity<Editor>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let position = source.position;
        let popover = cx.new(|cx| {
            let mut popover = Self::new(source, from, to, window, cx);
            popover.hosted_in_menu = true;
            popover
        });
        let menu = ContextMenu::build(window, cx, {
            let popover = popover.clone();
            move |menu, _, _| menu.custom_row(move |_, _| popover.clone().into_any_element())
        });
        let weak_menu = menu.downgrade();
        cx.subscribe(&popover, move |_, _: &DismissEvent, cx| {
            // The menu is already gone when it was the one that closed.
            weak_menu.update(cx, |_, cx| cx.emit(DismissEvent)).ok();
        })
        .detach();
        editor.update(cx, |editor, cx| {
            editor.deploy_mouse_context_menu(position, menu, window, cx)
        });
        popover
    }

    fn new(
        source: SelectionSource,
        from: String,
        to: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Escape and Enter are intercepted rather than bound: the popover
        // never takes focus, so the editor keeps it and a second keypress
        // still reaches the editor's `TranslateSelection` handler.
        let window_handle = window.window_handle();
        let popover = cx.weak_entity();
        let intercept = cx.intercept_keystrokes(move |event: &KeystrokeEvent, window, cx| {
            let key = event.keystroke.key.as_str();
            let is_plain_key =
                matches!(key, "escape" | "enter") && event.keystroke.modifiers == Modifiers::none();
            if !is_plain_key || window.window_handle() != window_handle {
                return;
            }
            let handled = popover
                .update(cx, |popover, cx| {
                    let focus_inside = popover
                        .escape_scope
                        .as_ref()
                        .is_none_or(|scope| scope.contains_focused(window, cx));
                    if !focus_inside {
                        return false;
                    }
                    if key == "escape" {
                        cx.emit(DismissEvent);
                        true
                    } else {
                        // Enter belongs to whatever has the focus unless the
                        // popover is waiting for exactly this answer.
                        popover.confirm_program_copy(cx)
                    }
                })
                .unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        });
        let mut this = Self {
            source,
            from,
            to,
            state: PopoverState::Loading,
            hidden_characters_removed: 0,
            replace_notice: None,
            escape_scope: None,
            hosted_in_menu: false,
            _translation: Task::ready(()),
            _subscriptions: vec![intercept],
        };
        if this.source.program_copy && this.source.too_long.is_none() {
            this.state = PopoverState::ConfirmProgramCopy {
                preview: preview_line(&this.source.text, PROGRAM_COPY_PREVIEW_CHARACTERS).into(),
                characters: this.source.text.chars().count(),
            };
        } else {
            this.request_translation(cx);
        }
        this
    }

    pub fn state(&self) -> &PopoverState {
        &self.state
    }

    pub fn position(&self) -> Point<Pixels> {
        self.source.position
    }

    #[cfg(test)]
    pub(crate) fn source_text(&self) -> &str {
        &self.source.text
    }

    pub fn direction(&self) -> (&str, &str) {
        (&self.from, &self.to)
    }

    /// Escape closes the popover only while the focus is inside `scope`.
    pub fn set_escape_scope(&mut self, scope: FocusHandle) {
        self.escape_scope = Some(scope);
    }

    /// Ties the popover to the view the text came from: it closes when the
    /// focus leaves that view (its tab is closed or switched away, a modal or
    /// the command palette opens), and Escape closes it only while the focus
    /// is still there.
    pub fn follow_focus_of(
        &mut self,
        source_focus: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let subscription = cx.on_focus_out(source_focus, window, |_, _, _, cx| {
            cx.emit(DismissEvent);
        });
        self._subscriptions.push(subscription);
        self.set_escape_scope(source_focus.clone());
    }

    /// Closes the popover when `entity` is released.
    pub fn dismiss_on_release<T: 'static>(&mut self, entity: &Entity<T>, cx: &mut Context<Self>) {
        let subscription = cx.observe_release(entity, |_, _, cx| cx.emit(DismissEvent));
        self._subscriptions.push(subscription);
    }

    /// Closes the popover when `entity` emits an event `should_dismiss`
    /// accepts, such as a selection change in the editor the text came from.
    pub fn dismiss_on<T, E>(
        &mut self,
        entity: &Entity<T>,
        should_dismiss: impl Fn(&E) -> bool + 'static,
        cx: &mut Context<Self>,
    ) where
        T: EventEmitter<E>,
        E: 'static,
    {
        let subscription = cx.subscribe(entity, move |_, _, event: &E, cx| {
            if should_dismiss(event) {
                cx.emit(DismissEvent);
            }
        });
        self._subscriptions.push(subscription);
    }

    /// Closes the popover when `entity` notifies and `should_dismiss` accepts
    /// its new state. For sources that emit no event when their selection
    /// changes.
    pub fn dismiss_when<T: 'static>(
        &mut self,
        entity: &Entity<T>,
        should_dismiss: impl Fn(&T, &App) -> bool + 'static,
        cx: &mut Context<Self>,
    ) {
        let subscription = cx.observe(entity, move |_, entity, cx| {
            if should_dismiss(entity.read(cx), cx) {
                cx.emit(DismissEvent);
            }
        });
        self._subscriptions.push(subscription);
    }

    /// `None` when there is nothing to replace, so no Replace button is shown.
    pub fn replace_availability(&self, cx: &App) -> Option<Result<(), ReplaceBlocked>> {
        let PopoverState::Done(_) = &self.state else {
            return None;
        };
        let target = self.source.replace_target.as_ref()?;
        match target.check(&self.source.text, cx) {
            Err(ReplaceBlocked::ReadOnly) => None,
            result => Some(result),
        }
    }

    /// Writes the translation over the original text, as one undoable edit,
    /// and closes the popover. Refuses when the original text has changed.
    pub fn replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let PopoverState::Done(translation) = &self.state else {
            return;
        };
        let Some(target) = self.source.replace_target.as_ref() else {
            return;
        };
        match target.replace(&self.source.text, translation, window, cx) {
            Ok(()) => cx.emit(DismissEvent),
            Err(reason) => {
                self.replace_notice = Some(reason);
                cx.notify();
            }
        }
    }

    /// Sends a program's copy for translation once the user has agreed.
    /// Returns whether it was waiting for that.
    pub fn confirm_program_copy(&mut self, cx: &mut Context<Self>) -> bool {
        if !matches!(self.state, PopoverState::ConfirmProgramCopy { .. }) {
            return false;
        }
        self.request_translation(cx);
        true
    }

    pub fn swap_direction(&mut self, cx: &mut Context<Self>) {
        if matches!(self.state, PopoverState::ConfirmProgramCopy { .. }) {
            return;
        }
        std::mem::swap(&mut self.from, &mut self.to);
        self.request_translation(cx);
    }

    pub fn copy_translation(&self, cx: &mut App) {
        if let PopoverState::Done(translation) = &self.state {
            cx.write_to_clipboard(ClipboardItem::new_string(translation.to_string()));
        }
    }

    fn request_translation(&mut self, cx: &mut Context<Self>) {
        self.hidden_characters_removed = 0;
        if let Some(length) = self.source.too_long {
            self.state = PopoverState::TooLong {
                length,
                limit: TranslateSettings::get_global(cx).max_characters,
            };
            cx.notify();
            return;
        }
        self.state = PopoverState::Loading;
        let translation = translate::translate(
            TranslateRequest {
                text: self.source.text.clone(),
                from: self.from.clone(),
                to: self.to.clone(),
            },
            cx,
        );
        // Replacing the task drops the previous request, which cancels it.
        self._translation = cx.spawn(async move |this, cx| {
            let result = translation.await;
            this.update(cx, |this, cx| {
                let result = result.map(|translation| {
                    let (cleaned, removed) = remove_hidden_characters(&translation);
                    this.hidden_characters_removed = removed;
                    cleaned
                });
                this.state = PopoverState::from_result(result);
                cx.notify();
            })
            .log_err();
        });
        cx.notify();
    }

    pub(crate) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.dispatch_action(
            OpenSettingsAt {
                path: TRANSLATE_SETTINGS_PATH.to_string(),
            }
            .boxed_clone(),
            cx,
        );
        cx.emit(DismissEvent);
    }

    fn render_header(&self, cx: &App) -> impl IntoElement {
        let provider = provider_display_name(TranslateSettings::get_global(cx).provider);
        let language = |code: &str| language_name(code).unwrap_or(code).to_string();
        Label::new(format!(
            "{provider} · {} → {}",
            language(&self.from),
            language(&self.to)
        ))
        .size(LabelSize::Small)
        .color(Color::Muted)
    }

    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        match &self.state {
            PopoverState::Loading => Label::new("Translating…")
                .color(Color::Muted)
                .into_any_element(),
            PopoverState::Done(translation) => v_flex()
                .gap_1()
                .child(
                    div()
                        .id("translation-text")
                        .max_h(rems(16.))
                        .overflow_y_scroll()
                        .child(Label::new(translation.clone())),
                )
                .when(self.hidden_characters_removed > 0, |this| {
                    this.child(
                        div()
                            .debug_selector(|| "translate-hidden-characters-removed".into())
                            .child(
                                Label::new(format!(
                                    "Removed {} hidden or control characters from the answer.",
                                    self.hidden_characters_removed
                                ))
                                .size(LabelSize::Small)
                                .color(Color::Warning),
                            ),
                    )
                })
                .into_any_element(),
            PopoverState::Failed(error) => Label::new(error.to_string())
                .color(Color::Error)
                .into_any_element(),
            PopoverState::TooLong { length, limit } => Label::new(
                TranslateError::TooLong {
                    length: *length,
                    limit: *limit,
                }
                .to_string(),
            )
            .color(Color::Warning)
            .into_any_element(),
            PopoverState::ConfirmProgramCopy {
                preview,
                characters,
            } => {
                let provider = provider_display_name(TranslateSettings::get_global(cx).provider);
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .debug_selector(|| "translate-program-copy-notice".into())
                            .child(
                                Label::new(
                                    "The program in this terminal copied this text. You did not select it.",
                                )
                                .size(LabelSize::Small)
                                .color(Color::Warning),
                            ),
                    )
                    .child(
                        div()
                            .max_h(rems(4.))
                            .overflow_hidden()
                            .child(Label::new(preview.clone())),
                    )
                    .child(
                        Label::new(format!(
                            "{characters} characters. They are sent to {provider} only if you translate."
                        ))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                    )
                    .child(
                        div()
                            .debug_selector(|| "translate-confirm-program-copy".into())
                            .child(
                                Button::new("translate-confirm-program-copy", "Translate")
                                    .style(ButtonStyle::Filled)
                                    .label_size(LabelSize::Small)
                                    .tooltip(Tooltip::text("Send this text (Enter)"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_program_copy(cx);
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            PopoverState::NotConfigured(error) => v_flex()
                .gap_1()
                .child(Label::new(error.to_string()).color(Color::Warning))
                .child(
                    Button::new("translate-open-settings", "Open Translate settings")
                        .style(ButtonStyle::Filled)
                        .label_size(LabelSize::Small)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_settings(window, cx)),
                        ),
                )
                .into_any_element(),
        }
    }

    fn render_actions(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let is_done = matches!(self.state, PopoverState::Done(_));
        if !is_done && !matches!(self.state, PopoverState::Failed(_)) {
            return None;
        }
        let replace = self.replace_availability(cx).map(|availability| {
            let blocked_reason = availability.err().map(|reason| reason.to_string());
            let notice = blocked_reason
                .clone()
                .or_else(|| self.replace_notice.map(|reason| reason.to_string()));
            let (label, tooltip) = self
                .source
                .replace_target
                .as_ref()
                .map_or(("Replace", ""), |target| (target.label(), target.tooltip()));
            h_flex()
                .gap_1()
                .when_some(notice, |this, reason| {
                    this.child(
                        div()
                            .debug_selector(|| "translate-replace-blocked-reason".into())
                            .child(
                                Label::new(reason)
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            ),
                    )
                })
                .child(
                    div().debug_selector(|| "translate-replace".into()).child(
                        Button::new("translate-replace", label)
                            .style(ButtonStyle::Filled)
                            .label_size(LabelSize::Small)
                            .disabled(blocked_reason.is_some())
                            .tooltip(Tooltip::text(tooltip))
                            .on_click(cx.listener(|this, _, window, cx| this.replace(window, cx))),
                    ),
                )
        });
        Some(
            h_flex()
                .gap_1()
                .justify_between()
                .child(
                    h_flex()
                        .gap_0p5()
                        .when(is_done, |this| {
                            this.child(
                                IconButton::new("translate-copy", IconName::Copy)
                                    .icon_size(IconSize::Small)
                                    .tooltip(Tooltip::text("Copy translation"))
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.copy_translation(cx)),
                                    ),
                            )
                        })
                        .child(
                            IconButton::new("translate-swap", IconName::ArrowRightLeft)
                                .icon_size(IconSize::Small)
                                .tooltip(Tooltip::text("Swap direction"))
                                .on_click(cx.listener(|this, _, _, cx| this.swap_direction(cx))),
                        ),
                )
                .children(replace),
        )
    }
}

impl Render for TranslationPopover {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let card = v_flex()
            .id("translation-popover")
            .occlude()
            .when(!self.hosted_in_menu, |card| card.elevation_2(cx))
            .p_2()
            .gap_1p5()
            .min_w(rems(14.))
            .max_w(rems(28.))
            .on_mouse_down_out(cx.listener(|_, _, _, cx| cx.emit(DismissEvent)))
            .child(self.render_header(cx))
            .child(self.render_body(cx))
            .children(self.render_actions(cx));
        if self.hosted_in_menu {
            return card.into_any_element();
        }
        deferred(
            anchored()
                .position(self.source.position)
                .snap_to_window_with_margin(px(8.))
                .child(card),
        )
        .with_priority(1)
        .into_any_element()
    }
}
