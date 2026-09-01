//! The form that names the repository before it is created on a host.
//!
//! Small on purpose: a name and a visibility. Everything else the host CLI
//! decides. Visibility defaults to private because the reverse mistake --
//! publishing a repository the user meant to keep to themselves -- cannot be
//! undone by deleting it later.

use gpui::{
    DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Render, SharedString,
    Window, prelude::*,
};
use ui::{Divider, prelude::*};
use workspace::ModalView;

pub(crate) struct CreateRepoModal {
    name_editor: Entity<editor::Editor>,
    focus_handle: FocusHandle,
    private: bool,
    /// Set only when the user confirmed. Read by the caller after dismissal.
    confirmed: Option<(String, bool)>,
}

impl CreateRepoModal {
    pub(crate) fn new(
        default_name: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name_editor = cx.new(|cx| {
            let mut editor = editor::Editor::single_line(window, cx);
            editor.set_text(default_name.clone(), window, cx);
            editor.set_placeholder_text("Repository name", window, cx);
            editor
        });
        let focus = name_editor.read(cx).focus_handle(cx);
        window.focus(&focus, cx);

        Self {
            name_editor,
            focus_handle: cx.focus_handle(),
            private: true,
            confirmed: None,
        }
    }

    pub(crate) fn confirmed(&self) -> Option<(String, bool)> {
        self.confirmed.clone()
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        let name = self.name_editor.read(cx).text(cx).trim().to_string();
        // An empty name is a cancel, not an error: the user cleared the field
        // and pressed Enter, which reads as "never mind".
        if !name.is_empty() {
            self.confirmed = Some((name, self.private));
        }
        cx.emit(DismissEvent);
    }
}

impl Focusable for CreateRepoModal {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for CreateRepoModal {}
impl ModalView for CreateRepoModal {}

impl Render for CreateRepoModal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let private = self.private;

        v_flex()
            .key_context("CreateRepoModal")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .w(rems(26.))
            .p_3()
            .gap_2()
            .on_action(cx.listener(|this, _: &menu::Confirm, _, cx| this.confirm(cx)))
            .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
            .child(Label::new("Create repository on host").weight(FontWeight::MEDIUM))
            .child(
                Label::new("Uses the host's own CLI (gh or glab).")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(Divider::horizontal())
            .child(self.name_editor.clone())
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("visibility-private", "Private")
                            .toggle_state(private)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.private = true;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("visibility-public", "Public")
                            .toggle_state(!private)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.private = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(Divider::horizontal())
            .child(
                h_flex()
                    .gap_1()
                    .justify_end()
                    .child(
                        Button::new("cancel", "Cancel")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(DismissEvent))),
                    )
                    .child(
                        Button::new("create", "Create")
                            .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
                    ),
            )
    }
}
