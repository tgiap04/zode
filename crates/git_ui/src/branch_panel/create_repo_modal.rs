//! The form that names the repository before it is created on a host.
//!
//! Small on purpose: a name and a visibility. Everything else the host CLI
//! decides. Visibility defaults to private because the reverse mistake --
//! publishing a repository the user meant to keep to themselves -- cannot be
//! undone by deleting it later.

use gpui::{
    DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Render, SharedString, Window,
    prelude::*,
};
use ui::{
    KeyBinding, Modal, ModalFooter, ModalHeader, Section, ToggleButtonGroup, ToggleButtonSimple,
    prelude::*,
};
use ui_input::InputField;
use workspace::ModalView;

pub(crate) struct CreateRepoModal {
    name_input: Entity<InputField>,
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
        let name_input = cx.new(|cx| {
            let input = InputField::new(window, cx, "Repository name");
            input.set_text(&default_name, window, cx);
            input
        });

        Self {
            name_input,
            focus_handle: cx.focus_handle(),
            private: true,
            confirmed: None,
        }
    }

    pub(crate) fn confirmed(&self) -> Option<(String, bool)> {
        self.confirmed.clone()
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).text(cx).trim().to_string();
        // An empty name is a cancel, not an error: the user cleared the field
        // and pressed Enter, which reads as "never mind".
        if !name.is_empty() {
            self.confirmed = Some((name, self.private));
        }
        cx.emit(DismissEvent);
    }
}

/// The name field, so that the form opens ready to be typed into.
impl Focusable for CreateRepoModal {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.name_input.focus_handle(cx)
    }
}

impl EventEmitter<DismissEvent> for CreateRepoModal {}
impl ModalView for CreateRepoModal {}

impl Render for CreateRepoModal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let private = self.private;
        let focus_handle = self.focus_handle.clone();

        div()
            .key_context("CreateRepoModal")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &menu::Confirm, _, cx| this.confirm(cx)))
            .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
            .elevation_3(cx)
            .occlude()
            .w(rems(28.))
            .max_h(rems(40.))
            .child(
                Modal::new("create-repo", None)
                    .header(
                        ModalHeader::new()
                            .child(
                                Headline::new("Create repository on host")
                                    .size(HeadlineSize::Small),
                            )
                            .description("Uses the host's own CLI (gh or glab)."),
                    )
                    .section(
                        Section::new().child(
                            v_flex()
                                .w_full()
                                .gap_3()
                                .child(self.name_input.clone())
                                .child(
                                    div().debug_selector(|| "repo-visibility".into()).child(
                                        ToggleButtonGroup::single_row(
                                            "repo-visibility",
                                            [
                                                ToggleButtonSimple::new(
                                                    "Private",
                                                    cx.listener(|this, _, _, cx| {
                                                        this.private = true;
                                                        cx.notify();
                                                    }),
                                                ),
                                                ToggleButtonSimple::new(
                                                    "Public",
                                                    cx.listener(|this, _, _, cx| {
                                                        this.private = false;
                                                        cx.notify();
                                                    }),
                                                ),
                                            ],
                                        )
                                        .selected_index(usize::from(!private))
                                        .label_size(LabelSize::Default),
                                    ),
                                ),
                        ),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_1()
                                .child(
                                    Button::new("cancel", "Cancel")
                                        .on_click(cx.listener(|_, _, _, cx| cx.emit(DismissEvent))),
                                )
                                .child(
                                    Button::new("create", "Create")
                                        .style(ButtonStyle::Filled)
                                        .key_binding(
                                            KeyBinding::for_action_in(
                                                &menu::Confirm,
                                                &focus_handle,
                                                cx,
                                            )
                                            .size(rems_from_px(12.)),
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
                                ),
                        ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::Cell;
    use std::rc::Rc;

    fn open<'a>(
        default_name: &'static str,
        cx: &'a mut TestAppContext,
    ) -> (Entity<CreateRepoModal>, &'a mut VisualTestContext) {
        cx.update(|cx| {
            let store = settings::SettingsStore::test(cx);
            cx.set_global(store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            editor::init(cx);
        });
        let (modal, cx) =
            cx.add_window_view(|window, cx| CreateRepoModal::new(default_name.into(), window, cx));
        // What the modal layer does the moment it shows a modal.
        modal.update_in(cx, |modal, window, cx| {
            let focus = modal.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_resize(gpui::size(gpui::px(1000.), gpui::px(800.)));
        cx.run_until_parked();
        (modal, cx)
    }

    fn confirm(
        modal: &Entity<CreateRepoModal>,
        cx: &mut VisualTestContext,
    ) -> (Option<(String, bool)>, bool) {
        let dismissed = Rc::new(Cell::new(false));
        let _subscription = cx.update(|_, cx| {
            let dismissed = dismissed.clone();
            cx.subscribe(modal, move |_, _: &DismissEvent, _| dismissed.set(true))
        });
        modal.update(cx, |modal, cx| modal.confirm(cx));
        cx.run_until_parked();
        (
            modal.read_with(cx, |modal, _| modal.confirmed()),
            dismissed.get(),
        )
    }

    #[gpui::test]
    async fn the_form_opens_ready_to_be_typed_into(cx: &mut TestAppContext) {
        let (modal, cx) = open("zode", cx);
        modal.update_in(cx, |modal, window, cx| {
            assert!(
                modal
                    .name_input
                    .read(cx)
                    .editor()
                    .focus_handle(cx)
                    .is_focused(window),
                "what the modal layer focuses is the name field itself"
            );
        });
    }

    /// Private unless said otherwise: publishing by default is the mistake
    /// that deleting the repository later cannot take back.
    #[gpui::test]
    async fn confirming_hands_back_the_trimmed_name_and_keeps_it_private(cx: &mut TestAppContext) {
        let (modal, cx) = open("  zode  ", cx);

        let (confirmed, dismissed) = confirm(&modal, cx);

        assert_eq!(confirmed, Some(("zode".to_string(), true)));
        assert!(dismissed);
    }

    #[gpui::test]
    async fn choosing_public_is_what_gets_confirmed(cx: &mut TestAppContext) {
        let (modal, cx) = open("zode", cx);

        let bounds = cx
            .debug_bounds("repo-visibility")
            .expect("the visibility choice was not drawn");
        let second_segment = gpui::point(
            bounds.origin.x + bounds.size.width * 0.75,
            bounds.center().y,
        );
        cx.simulate_click(second_segment, Modifiers::default());
        cx.run_until_parked();

        assert!(!modal.read_with(cx, |modal, _| modal.private));
        let (confirmed, _) = confirm(&modal, cx);
        assert_eq!(confirmed, Some(("zode".to_string(), false)));
    }

    /// Clearing the field and pressing Enter reads as "never mind".
    #[gpui::test]
    async fn an_empty_name_is_a_cancel(cx: &mut TestAppContext) {
        let (modal, cx) = open("   ", cx);

        let (confirmed, dismissed) = confirm(&modal, cx);

        assert_eq!(confirmed, None);
        assert!(dismissed, "the form closes either way");
    }
}
