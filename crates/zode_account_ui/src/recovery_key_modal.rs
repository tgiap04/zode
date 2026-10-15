use std::time::Duration;

use gpui::{
    App, ClipboardItem, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, SharedString,
    Subscription, Task, Window,
};
use ui::{Checkbox, KeyBinding, Modal, ModalFooter, ModalHeader, Section, Tooltip, prelude::*};
use ui_input::{ErasedEditorEvent, InputField};
use util::ResultExt as _;
use workspace::ModalView;
use zode_sync::SyncSession;

const COPIED_FEEDBACK: Duration = Duration::from_secs(2);

/// Why the window is open.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RecoveryKeyMode {
    /// First time on this account: make a key and show it once.
    Create,
    /// Show the key this machine already holds.
    Reveal,
    /// Take the key from another machine.
    Enter,
    /// Replace the key and re-encrypt everything stored under it.
    Rotate,
}

/// The recovery key, shown or taken.
///
/// # Why this window blocks
///
/// This is the only moment the user is told something that cannot be
/// recovered. The server holds ciphertext and no key; if the recovery key is
/// lost, the synced data is gone, and no support request can undo it. That is
/// not a caveat to put in a tooltip — it is the deal, and the window says so
/// and requires an acknowledgement before it will close.
///
/// The alternative, a dismissible notice, reliably produces a user who
/// discovers the terms at the exact moment they can no longer act on them.
pub struct RecoveryKeyModal {
    session: Entity<SyncSession>,
    mode: RecoveryKeyMode,
    phrase: Option<SharedString>,
    input: Entity<InputField>,
    /// Live read of what has been typed, so the window can say whether it is a
    /// well-formed key before the user commits to it.
    typed_state: TypedState,
    acknowledged: bool,
    copied_reset: Option<Task<()>>,
    error: Option<SharedString>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TypedState {
    Empty,
    /// Not a key yet, but could still become one as more is typed.
    Incomplete,
    Valid,
    Invalid(SharedString),
}

impl EventEmitter<DismissEvent> for RecoveryKeyModal {}
impl ModalView for RecoveryKeyModal {}

impl Focusable for RecoveryKeyModal {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.mode == RecoveryKeyMode::Enter {
            self.input.focus_handle(cx)
        } else {
            self.focus_handle.clone()
        }
    }
}

impl RecoveryKeyModal {
    pub fn new(
        session: Entity<SyncSession>,
        mode: RecoveryKeyMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputField::new(window, cx, "ZODE-XXXXX-XXXXX-…"));

        let editor = input.read(cx).editor().clone();
        let edited = editor.subscribe(
            Box::new({
                let this = cx.weak_entity();
                move |event, _window, cx| {
                    if event == ErasedEditorEvent::BufferEdited {
                        this.update(cx, |this, cx| {
                            let typed = this.input.read(cx).text(cx);
                            this.typed_state = classify(&typed);
                            this.error = None;
                            cx.notify();
                        })
                        .log_err();
                    }
                }
            }),
            window,
            cx,
        );

        let mut this = Self {
            session: session.clone(),
            mode,
            phrase: None,
            input,
            typed_state: TypedState::Empty,
            acknowledged: false,
            copied_reset: None,
            error: None,
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![edited],
        };

        match mode {
            RecoveryKeyMode::Create => {
                let creating = session.update(cx, |session, cx| session.create_key(cx));
                cx.spawn(async move |this, cx| {
                    let result = creating.await;
                    _ = this.update(cx, |this, cx| {
                        match result {
                            Ok(phrase) => this.phrase = Some(phrase.into()),
                            Err(error) => this.error = Some(format!("{error:#}").into()),
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            RecoveryKeyMode::Reveal => {
                this.phrase = session.read(cx).reveal_recovery_key().map(Into::into);
            }
            RecoveryKeyMode::Rotate => {
                let rotating = session.update(cx, |session, cx| session.rotate_key(cx));
                cx.spawn(async move |this, cx| {
                    let result = rotating.await;
                    _ = this.update(cx, |this, cx| {
                        match result {
                            Ok(phrase) => this.phrase = Some(phrase.into()),
                            Err(error) => this.error = Some(format!("{error}").into()),
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            RecoveryKeyMode::Enter => {}
        }

        this
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        let Some(phrase) = self.phrase.clone() else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(phrase.to_string()));
        self.copied_reset = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FEEDBACK).await;
            _ = this.update(cx, |this, cx| {
                this.copied_reset = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        match self.mode {
            RecoveryKeyMode::Enter => {
                let typed = self.input.read(cx).text(cx);
                let accepting = self
                    .session
                    .update(cx, |session, cx| session.accept_recovery_key(typed, cx));
                cx.spawn(async move |this, cx| match accepting.await {
                    Ok(()) => _ = this.update(cx, |_, cx| cx.emit(DismissEvent)),
                    Err(error) => {
                        _ = this.update(cx, |this, cx| {
                            this.error = Some(format!("{error}").into());
                            cx.notify();
                        })
                    }
                })
                .detach();
            }
            // Closing is the whole action; the key was already written to the
            // keychain before it was shown.
            RecoveryKeyMode::Create | RecoveryKeyMode::Reveal | RecoveryKeyMode::Rotate => {
                if self.can_close() {
                    cx.emit(DismissEvent);
                }
            }
        }
    }

    fn can_close(&self) -> bool {
        match self.mode {
            // The one gate in the whole feature. See the type comment.
            // Rotation is held to it too: the old key stops working the moment
            // the first artifact is re-encrypted, so this string is the only
            // way back to the data.
            RecoveryKeyMode::Create | RecoveryKeyMode::Rotate => {
                self.acknowledged && self.phrase.is_some()
            }
            _ => true,
        }
    }

    fn title(&self) -> &'static str {
        match self.mode {
            RecoveryKeyMode::Create => "Save your recovery key",
            RecoveryKeyMode::Reveal => "Your recovery key",
            RecoveryKeyMode::Enter => "Enter your recovery key",
            RecoveryKeyMode::Rotate => "Your new recovery key",
        }
    }
}

/// What the typed text is, as far as it goes.
///
/// Deliberately does NOT rewrite the buffer as the user types. Folding
/// characters underneath a moving cursor fights whoever is typing, and pasting
/// is the common case anyway. The window reports; it does not correct.
fn classify(typed: &str) -> TypedState {
    let significant = typed
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .count();
    if significant == 0 {
        return TypedState::Empty;
    }
    match zode_sync::recovery_key::decode(typed) {
        Ok(_) => TypedState::Valid,
        Err(zode_sync::RecoveryKeyError::WrongLength { .. }) => TypedState::Incomplete,
        Err(error) => TypedState::Invalid(error.to_string().into()),
    }
}

impl Render for RecoveryKeyModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let copied = self.copied_reset.is_some();
        let confirm_binding = KeyBinding::for_action_in(&menu::Confirm, &self.focus_handle(cx), cx)
            .map(|binding| binding.size(rems_from_px(12.)));

        // `elevation_3` carries the background, border, radius and shadow.
        // Without it the modal is transparent and whatever is behind it reads
        // straight through the middle of the key.
        let shell = div()
            .key_context("RecoveryKeyModal")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .occlude()
            .w(rems(28.))
            .max_h(rems(40.))
            .debug_selector(|| "account-recovery-key-modal".into())
            .on_action(cx.listener(|this, _: &menu::Confirm, _window, cx| this.confirm(cx)))
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| {
                if this.can_close() {
                    cx.emit(DismissEvent);
                }
            }));

        let header = ModalHeader::new()
            .child(Headline::new(self.title()).size(HeadlineSize::Small))
            .description(match self.mode {
                RecoveryKeyMode::Enter => "Type the key from a machine that is already syncing.",
                RecoveryKeyMode::Rotate => {
                    "Your old key no longer opens anything. Every other machine must be given this one."
                }
                _ => "Zode cannot recover this for you. Nobody can.",
            });

        let modal = Modal::new("account-recovery-key", None).header(header);
        let modal = match self.mode {
            RecoveryKeyMode::Enter => modal.section(
                Section::new()
                    .child(self.input.clone())
                    .child(match &self.typed_state {
                        TypedState::Empty => {
                            Label::new("Separators and capitalisation do not matter.")
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                        }
                        TypedState::Incomplete => {
                            Label::new("Keep going — a recovery key is 53 characters.")
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                        }
                        TypedState::Valid => Label::new("That looks like a valid key.")
                            .size(LabelSize::XSmall)
                            .color(Color::Success),
                        TypedState::Invalid(reason) => Label::new(reason.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Error),
                    }),
            ),
            _ => modal.child(
                v_flex()
                    .w_full()
                    .py_4()
                    .px_3()
                    .gap_1p5()
                    .items_center()
                    .bg(colors.editor_background)
                    .border_y_1()
                    .border_color(colors.border_variant)
                    .child(match self.phrase.clone() {
                        Some(phrase) => Label::new(phrase)
                            .size(LabelSize::Small)
                            .buffer_font(cx)
                            .into_any_element(),
                        None => Label::new("Generating…")
                            .size(LabelSize::Small)
                            .color(Color::Muted)
                            .into_any_element(),
                    })
                    .child(
                        Button::new("recovery-key-copy", if copied { "Copied" } else { "Copy" })
                            .label_size(LabelSize::Small)
                            .color(if copied { Color::Success } else { Color::Muted })
                            .start_icon(
                                Icon::new(if copied {
                                    IconName::Check
                                } else {
                                    IconName::Copy
                                })
                                .size(IconSize::Small),
                            )
                            .disabled(self.phrase.is_none())
                            .on_click(cx.listener(|this, _, _window, cx| this.copy(cx))),
                    ),
            ),
        };

        let modal = modal.children(self.error.clone().map(|error| {
            Section::new().child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(
                        Icon::new(IconName::XCircle)
                            .size(IconSize::XSmall)
                            .color(Color::Error),
                    )
                    .child(Label::new(error).size(LabelSize::Small).color(Color::Error)),
            )
        }));

        let acknowledgement =
            matches!(self.mode, RecoveryKeyMode::Create | RecoveryKeyMode::Rotate).then(|| {
                Checkbox::new("recovery-key-ack", self.acknowledged.into())
                    .label("I have saved this key somewhere safe")
                    .label_size(LabelSize::Small)
                    .disabled(self.phrase.is_none())
                    .on_click(cx.listener(|this, state: &ui::ToggleState, _window, cx| {
                        this.acknowledged = *state == ui::ToggleState::Selected;
                        cx.notify();
                    }))
            });

        // `ModalFooter` has no fill of its own, so this band gives it the editor
        // background the body above it uses.
        let footer = div().w_full().bg(colors.editor_background).child(
            ModalFooter::new()
                .start_slot::<Checkbox>(acknowledgement)
                .end_slot(
                h_flex()
                    .gap_1()
                    .when(self.mode == RecoveryKeyMode::Enter, |element| {
                        element.child(
                            Button::new("recovery-key-cancel", "Cancel")
                                .on_click(cx.listener(|_, _, _window, cx| cx.emit(DismissEvent))),
                        )
                    })
                    .child(
                        Button::new(
                            "recovery-key-confirm",
                            match self.mode {
                                RecoveryKeyMode::Enter => "Use This Key",
                                _ => "Done",
                            },
                        )
                        .style(ButtonStyle::Filled)
                        .key_binding(confirm_binding)
                        .disabled(match self.mode {
                            RecoveryKeyMode::Enter => self.typed_state != TypedState::Valid,
                            _ => !self.can_close(),
                        })
                        .when(
                            matches!(
                                self.mode,
                                RecoveryKeyMode::Create | RecoveryKeyMode::Rotate
                            ),
                            |button| {
                                button.tooltip(Tooltip::text(
                                    "Tick the box first — this key cannot be shown again after you lose it",
                                ))
                            },
                        )
                        .on_click(cx.listener(|this, _, _window, cx| this.confirm(cx))),
                    ),
            ),
        );

        shell.child(modal).child(footer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use zode_account::{Account, AccountStatus, AccountUser};

    fn init_test(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            editor::init(cx);
        });
    }

    fn session(cx: &mut TestAppContext) -> Entity<SyncSession> {
        let account = cx.update(|cx| {
            cx.new(|_| {
                Account::for_test(AccountStatus::SignedIn(AccountUser {
                    id: "1".into(),
                    email: "ada@example.com".into(),
                    name: None,
                    avatar_url: None,
                }))
            })
        });
        cx.update(|cx| cx.new(|_| SyncSession::new(account)))
    }

    fn draw(mode: RecoveryKeyMode, cx: &mut TestAppContext) -> Entity<RecoveryKeyModal> {
        init_test(cx);
        let session = session(cx);
        let (modal, cx) =
            cx.add_window_view(|window, cx| RecoveryKeyModal::new(session, mode, window, cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        modal
    }

    #[gpui::test]
    fn the_window_draws_in_every_mode(cx: &mut TestAppContext) {
        draw(RecoveryKeyMode::Reveal, cx);
    }

    #[gpui::test]
    fn the_enter_window_draws(cx: &mut TestAppContext) {
        draw(RecoveryKeyMode::Enter, cx);
    }

    /// A window that shows a key is the small tier of modal widths.
    #[gpui::test]
    fn the_window_is_the_small_width(cx: &mut TestAppContext) {
        init_test(cx);
        let session = session(cx);
        let (_modal, cx) = cx.add_window_view(|window, cx| {
            RecoveryKeyModal::new(session, RecoveryKeyMode::Reveal, window, cx)
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        let bounds = cx
            .debug_bounds("account-recovery-key-modal")
            .expect("the modal must be drawn");
        let small = cx.update(|window, _| rems(28.).to_pixels(window.rem_size()));
        assert_eq!(bounds.size.width, small);
    }

    /// The verdict under the field follows what is typed, which is wired through
    /// the field's own edit event rather than through the editor the window used
    /// to own.
    #[gpui::test]
    fn typing_into_the_field_updates_the_verdict(cx: &mut TestAppContext) {
        init_test(cx);
        let session = session(cx);
        let (modal, cx) = cx.add_window_view(|window, cx| {
            RecoveryKeyModal::new(session, RecoveryKeyMode::Enter, window, cx)
        });
        let valid = zode_sync::recovery_key::encode(&zode_sync::Dek::from_bytes([0x42; 32]));

        let type_into_field = |text: &str, cx: &mut gpui::VisualTestContext| {
            modal.update_in(cx, |modal, window, cx| {
                modal
                    .input
                    .update(cx, |field, cx| field.set_text(text, window, cx));
            });
            cx.run_until_parked();
        };

        type_into_field(&valid, cx);
        modal.read_with(cx, |modal, _| {
            assert_eq!(modal.typed_state, TypedState::Valid)
        });

        type_into_field("ZODE-ABC", cx);
        modal.read_with(cx, |modal, _| {
            assert_eq!(modal.typed_state, TypedState::Incomplete)
        });

        type_into_field("", cx);
        modal.read_with(cx, |modal, _| {
            assert_eq!(modal.typed_state, TypedState::Empty)
        });
    }

    /// The one gate in the feature: a user who has not said they saved the key
    /// cannot dismiss the window that showed it.
    #[gpui::test]
    fn creating_a_key_cannot_be_dismissed_before_it_is_acknowledged(cx: &mut TestAppContext) {
        let modal = draw(RecoveryKeyMode::Create, cx);

        modal.update(cx, |modal, _| {
            assert!(
                modal.phrase.is_some(),
                "creating must produce a key to show"
            );
            assert!(
                !modal.can_close(),
                "an unacknowledged key must keep the window open"
            );
        });

        modal.update(cx, |modal, _| {
            modal.acknowledged = true;
            assert!(modal.can_close(), "acknowledging must release the window");
        });

        modal.update(cx, |modal, _| {
            // Acknowledging something that was never shown must not count —
            // otherwise a failed generation produces a user who confirmed they
            // saved a key that does not exist.
            modal.phrase = None;
            assert!(!modal.can_close(), "there must be a key to have saved");
        });
    }

    /// Typed input is classified as the user goes, so a mistyped key is caught
    /// before it is submitted rather than surfacing later as an
    /// indistinguishable decryption failure.
    #[gpui::test]
    fn typed_input_is_classified_as_it_is_entered(cx: &mut TestAppContext) {
        init_test(cx);
        let dek = zode_sync::Dek::from_bytes([0x42; 32]);
        let valid = zode_sync::recovery_key::encode(&dek);

        assert_eq!(classify(""), TypedState::Empty);
        assert_eq!(classify("ZODE-ABC"), TypedState::Incomplete);
        assert_eq!(classify(&valid), TypedState::Valid);

        // One character changed: right length, wrong check symbol.
        let mut wrong: Vec<char> = valid.replace('-', "").chars().collect();
        wrong[6] = if wrong[6] == '2' { '3' } else { '2' };
        let mistyped: String = wrong.into_iter().collect();
        assert!(
            matches!(classify(&mistyped), TypedState::Invalid(_)),
            "{:?}",
            classify(&mistyped)
        );
    }
}
