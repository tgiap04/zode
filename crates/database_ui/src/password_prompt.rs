//! Putting a connection's password into the OS keychain.
//!
//! The one thing a database client must never do is keep this in a settings
//! file, so the panel needs somewhere to ask for it. It is a modal rather than
//! a field in the tree because a password typed into a row is a password on
//! screen, and because this is answered once and then not again.

use crate::connection_store::{delete_secret, write_secret};
use editor::Editor;
use gpui::{
    App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Render, SharedString,
    Subscription, Task, Window,
};
use ui::prelude::*;
use workspace::ModalView;

/// What the caller wants done once a password is known.
pub(crate) type OnSaved = Box<dyn FnOnce(&mut Window, &mut App) + 'static>;

pub(crate) struct PasswordPrompt {
    /// The connection's display name, so someone with several open knows which
    /// server they are about to hand a password to.
    connection: SharedString,
    /// Where it goes in the keychain: the URL, so the password follows the
    /// server rather than the label.
    key: String,
    /// Written beside the password so the keychain's own UI can identify the
    /// entry. Never read back -- the URL already says who to connect as.
    username: String,
    password_editor: Entity<Editor>,
    /// Run after a successful write, which is how the panel retries the
    /// connection that sent the user here.
    on_saved: Option<OnSaved>,
    /// A failed keychain write, shown rather than swallowed: on a locked or
    /// unavailable keychain this is the only thing that explains why the
    /// password did not stick.
    error: Option<SharedString>,
    _subscription: Subscription,
}

impl PasswordPrompt {
    pub(crate) fn new(
        connection: SharedString,
        key: String,
        username: String,
        on_saved: OnSaved,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let password_editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            // Not a courtesy: this is typed in front of whoever is looking at
            // the screen, and in front of whatever is recording it.
            editor.set_masked(true, cx);
            editor.set_placeholder_text("Password", window, cx);
            editor
        });

        // Dismissed on blur, as every other modal here is: a password box left
        // behind a window is one someone types into later without looking.
        let subscription = cx.subscribe(&password_editor, |_this, _editor, event, cx| {
            if matches!(event, editor::EditorEvent::Blurred) {
                cx.emit(DismissEvent);
            }
        });

        Self {
            connection,
            key,
            username,
            password_editor,
            on_saved: Some(on_saved),
            error: None,
            _subscription: subscription,
        }
    }

    /// Writes what was typed, or forgets the stored password when it is empty.
    ///
    /// Empty means forget rather than "store an empty password": a server that
    /// wants no password is served by having nothing stored, and this is the
    /// only gesture anyone would reach for to undo a mistyped one.
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let password = self.password_editor.read(cx).text(cx);
        let credentials = zed_credentials_provider::global(cx);
        let key = self.key.clone();

        let written: Task<anyhow::Result<()>> = if password.is_empty() {
            delete_secret(credentials, key, cx)
        } else {
            write_secret(credentials, key, self.username.clone(), password, cx)
        };

        cx.spawn_in(window, async move |this, cx| match written.await {
            Ok(()) => {
                this.update_in(cx, |this, window, cx| {
                    if let Some(on_saved) = this.on_saved.take() {
                        on_saved(window, cx);
                    }
                    cx.emit(DismissEvent);
                })
                .ok();
            }
            Err(error) => {
                this.update(cx, |this, cx| {
                    this.error = Some(format!("{error:#}").into());
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

impl ModalView for PasswordPrompt {}

impl EventEmitter<DismissEvent> for PasswordPrompt {}

impl Focusable for PasswordPrompt {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.password_editor.focus_handle(cx)
    }
}

impl Render for PasswordPrompt {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("DatabasePasswordPrompt")
            .elevation_3(cx)
            .w(px(440.))
            .p_3()
            .gap_2()
            .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| this.confirm(window, cx)))
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.cancel(cx)))
            .child(Label::new(format!("Password for {}", self.connection)))
            .child(
                Label::new("Stored in the system keychain, never in your settings.")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                div()
                    .w_full()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .bg(cx.theme().colors().editor_background)
                    .child(self.password_editor.clone()),
            )
            .when_some(self.error.clone(), |element, error| {
                element.child(Label::new(error).size(LabelSize::Small).color(Color::Error))
            })
            .child(
                Label::new("Leave it empty to forget the stored password.")
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
    }
}
