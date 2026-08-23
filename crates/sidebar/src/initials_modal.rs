//! The two letters drawn on a project's avatar.
//!
//! Called "initials" and not "name" throughout, including in what the user
//! reads: it changes the avatar and nothing else. The panel row still shows the
//! project's real name, the tooltip still shows its path, and the window title
//! never moves. A box labelled *Rename* that left all three alone would be
//! promising more than it does.

use editor::Editor;
use gpui::{DismissEvent, Entity, EventEmitter, Focusable, SharedString, WeakEntity};
use project::ProjectGroupKey;
use ui::prelude::*;
use workspace::{ModalView, MultiWorkspace, project_appearance::MAX_INITIALS};

pub struct InitialsModal {
    multi_workspace: WeakEntity<MultiWorkspace>,
    key: ProjectGroupKey,
    label: SharedString,
    editor: Entity<Editor>,
}

impl EventEmitter<DismissEvent> for InitialsModal {}
impl ModalView for InitialsModal {}

impl Focusable for InitialsModal {
    fn focus_handle(&self, cx: &App) -> gpui::FocusHandle {
        self.editor.focus_handle(cx)
    }
}

impl InitialsModal {
    pub fn new(
        multi_workspace: WeakEntity<MultiWorkspace>,
        key: ProjectGroupKey,
        label: SharedString,
        current: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            // Pre-filled with what is there now, so editing two letters does not
            // start by clearing them.
            if let Some(current) = current.as_ref() {
                editor.set_text(current.clone(), window, cx);
                editor.select_all(&Default::default(), window, cx);
            }
            editor.set_placeholder_text("Two letters", window, cx);
            editor
        });

        Self {
            multi_workspace,
            key,
            label,
            editor,
        }
    }

    fn cancel(&mut self, _: &menu::Cancel, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    fn confirm(&mut self, _: &menu::Confirm, _window: &mut Window, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).text(cx);
        // An empty field is the way back to initials derived from the name --
        // without it there would be no undo for a choice someone regrets. The
        // trim and the cap live in `set_project_initials`, one door for all
        // callers.
        //
        // A project that left this window while the box was open takes the edit
        // with it, silently. See the same note on `ColourModal::commit`.
        self.multi_workspace
            .update(cx, |multi_workspace, cx| {
                multi_workspace.set_project_initials(&self.key, &text, cx);
            })
            .ok();
        cx.emit(DismissEvent);
    }

    fn clear(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            editor.set_text("", window, cx);
        });
        self.confirm(&menu::Confirm, window, cx);
    }
}

impl Render for InitialsModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();

        v_flex()
            .key_context("InitialsModal")
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::confirm))
            .elevation_3(cx)
            .w_80()
            .overflow_hidden()
            .child(
                div()
                    .p_2()
                    .border_b_1()
                    .border_color(colors.border_variant)
                    .child(
                        Label::new(format!("Initials for {}", self.label))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            )
            .child(
                div()
                    .p_2()
                    .border_b_1()
                    .border_color(colors.border_variant)
                    .child(self.editor.clone()),
            )
            .child(
                h_flex()
                    .bg(colors.editor_background)
                    .rounded_b_sm()
                    .w_full()
                    .p_2()
                    .gap_1()
                    .justify_between()
                    .child(
                        Label::new(format!(
                            "The avatar draws at most {MAX_INITIALS}; the rest is cut."
                        ))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                    )
                    .child(
                        Button::new("initials-use-default", "Use Default")
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(Self::clear)),
                    ),
            )
    }
}
