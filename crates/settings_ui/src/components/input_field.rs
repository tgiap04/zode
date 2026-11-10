use std::rc::Rc;

use editor::Editor;
use gpui::{AnyElement, ElementId, Focusable, TextStyleRefinement};
use settings::Settings as _;
use theme_settings::ThemeSettings;
use ui::{Tooltip, prelude::*, rems};

#[derive(IntoElement)]
pub struct SettingsInputField {
    id: Option<ElementId>,
    initial_text: Option<String>,
    placeholder: Option<&'static str>,
    confirm: Option<Rc<dyn Fn(Option<String>, &mut Window, &mut App)>>,
    tab_index: Option<isize>,
    use_buffer_font: bool,
    display_confirm_button: bool,
    display_clear_button: bool,
    clear_on_confirm: bool,
    action_slot: Option<AnyElement>,
    color: Option<Color>,
}

impl SettingsInputField {
    pub fn new() -> Self {
        Self {
            id: None,
            initial_text: None,
            placeholder: None,
            confirm: None,
            tab_index: None,
            use_buffer_font: false,
            display_confirm_button: false,
            display_clear_button: false,
            clear_on_confirm: false,
            action_slot: None,
            color: None,
        }
    }

    pub fn with_initial_text(mut self, initial_text: String) -> Self {
        self.initial_text = Some(initial_text);
        self
    }

    pub fn with_placeholder(mut self, placeholder: &'static str) -> Self {
        self.placeholder = Some(placeholder);
        self
    }

    pub fn on_confirm(
        mut self,
        confirm: impl Fn(Option<String>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.confirm = Some(Rc::new(confirm));
        self
    }

    pub(crate) fn tab_index(mut self, arg: isize) -> Self {
        self.tab_index = Some(arg);
        self
    }
}

impl RenderOnce for SettingsInputField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl ui::IntoElement {
        let settings = ThemeSettings::get_global(cx);
        let use_buffer_font = self.use_buffer_font;
        let color = self.color.map(|c| c.color(cx));
        let styles = TextStyleRefinement {
            font_family: use_buffer_font.then(|| settings.buffer_font.family.clone()),
            font_size: use_buffer_font.then(|| rems(0.75).into()),
            color,
            ..Default::default()
        };

        let first_render_initial_text = window.use_state(cx, |_, _| self.initial_text.clone());

        let editor = if let Some(id) = self.id {
            window.use_keyed_state(id, cx, {
                let initial_text = self.initial_text.clone();
                let placeholder = self.placeholder;
                let mut confirm = self.confirm.clone();
                let shown = first_render_initial_text.clone();

                move |window, cx| {
                    let mut editor = Editor::single_line(window, cx);
                    let editor_focus_handle = editor.focus_handle(cx);
                    if let Some(text) = initial_text {
                        editor.set_text(text, window, cx);
                    }

                    if let Some(confirm) = confirm.take()
                        && !self.display_confirm_button
                        && !self.display_clear_button
                        && !self.clear_on_confirm
                    {
                        cx.on_focus_out(
                            &editor_focus_handle,
                            window,
                            move |editor, _, window, cx| {
                                let text = editor.text(cx);
                                // Leaving a field without changing it is not
                                // an edit: clicking it on the way to a button
                                // beside it used to write its old text -- or
                                // an empty one -- back over the setting.
                                let unchanged = shown.read(cx).as_deref().unwrap_or("") == text;
                                if unchanged {
                                    return;
                                }
                                confirm(Some(text), window, cx);
                            },
                        )
                        .detach();
                    }

                    if let Some(placeholder) = placeholder {
                        editor.set_placeholder_text(placeholder, window, cx);
                    }
                    editor.set_text_style_refinement(styles);
                    editor
                }
            })
        } else {
            window.use_state(cx, {
                let initial_text = self.initial_text.clone();
                let placeholder = self.placeholder;
                let mut confirm = self.confirm.clone();
                let shown = first_render_initial_text.clone();

                move |window, cx| {
                    let mut editor = Editor::single_line(window, cx);
                    let editor_focus_handle = editor.focus_handle(cx);
                    if let Some(text) = initial_text {
                        editor.set_text(text, window, cx);
                    }

                    if let Some(confirm) = confirm.take()
                        && !self.display_confirm_button
                        && !self.display_clear_button
                        && !self.clear_on_confirm
                    {
                        cx.on_focus_out(
                            &editor_focus_handle,
                            window,
                            move |editor, _, window, cx| {
                                let text = editor.text(cx);
                                // Leaving a field without changing it is not
                                // an edit: clicking it on the way to a button
                                // beside it used to write its old text -- or
                                // an empty one -- back over the setting.
                                let unchanged = shown.read(cx).as_deref().unwrap_or("") == text;
                                if unchanged {
                                    return;
                                }
                                confirm(Some(text), window, cx);
                            },
                        )
                        .detach();
                    }

                    if let Some(placeholder) = placeholder {
                        editor.set_placeholder_text(placeholder, window, cx);
                    }
                    editor.set_text_style_refinement(styles);
                    editor
                }
            })
        };

        // When settings change externally (e.g. editing settings.json), the page
        // re-renders but use_keyed_state returns the cached editor with stale text.
        // Reconcile with the expected initial_text when the editor is not focused,
        // so we don't clobber what the user is actively typing.
        // Both ways, empty included: a field that started empty, like a
        // background image not chosen yet, has to show the path a picker
        // writes for it.
        if self.initial_text != *first_render_initial_text.read(cx)
            && !editor.read(cx).is_focused(window)
        {
            *first_render_initial_text.as_mut(cx) = self.initial_text.clone();
            let weak_editor = editor.downgrade();
            let text = self.initial_text.clone().unwrap_or_default();

            window.defer(cx, move |window, cx| {
                weak_editor
                    .update(cx, |editor, cx| {
                        editor.set_text(text, window, cx);
                    })
                    .ok();
            });
        }

        let weak_editor = editor.downgrade();
        let weak_editor_for_button = editor.downgrade();
        let weak_editor_for_clear = editor.downgrade();

        let clear_on_confirm = self.clear_on_confirm;
        let clear_on_confirm_for_button = self.clear_on_confirm;

        let theme_colors = cx.theme().colors();

        let display_confirm_button = self.display_confirm_button;
        let display_clear_button = self.display_clear_button;
        let confirm_for_button = self.confirm.clone();
        let is_editor_empty = editor.read(cx).text(cx).trim().is_empty();
        let is_editor_focused = editor.read(cx).is_focused(window);

        h_flex()
            .group("settings-input-field-editor")
            .relative()
            .py_1()
            .px_2()
            .h_8()
            .min_w_64()
            .rounded_md()
            .border_1()
            .border_color(theme_colors.border)
            .bg(theme_colors.editor_background)
            .when_some(self.tab_index, |this, tab_index| {
                let focus_handle = editor.focus_handle(cx).tab_index(tab_index).tab_stop(true);
                this.track_focus(&focus_handle)
                    .focus(|s| s.border_color(theme_colors.border_focused))
            })
            .child(editor)
            .child(
                h_flex()
                    .absolute()
                    .top_1()
                    .right_1()
                    .invisible()
                    .when(is_editor_focused, |this| this.visible())
                    .group_hover("settings-input-field-editor", |this| this.visible())
                    .when(
                        display_clear_button && !is_editor_empty && is_editor_focused,
                        |this| {
                            this.child(
                                IconButton::new("clear-button", IconName::Close)
                                    .icon_size(IconSize::Small)
                                    .icon_color(Color::Muted)
                                    .tooltip(Tooltip::text("Clear"))
                                    .on_click(move |_, window, cx| {
                                        let Some(editor) = weak_editor_for_clear.upgrade() else {
                                            return;
                                        };
                                        editor.update(cx, |editor, cx| {
                                            editor.set_text("", window, cx);
                                        });
                                    }),
                            )
                        },
                    )
                    .when(
                        display_confirm_button && !is_editor_empty && is_editor_focused,
                        |this| {
                            this.child(
                                IconButton::new("confirm-button", IconName::Check)
                                    .icon_size(IconSize::Small)
                                    .icon_color(Color::Success)
                                    .tooltip(Tooltip::text("Enter to Confirm"))
                                    .on_click(move |_, window, cx| {
                                        let Some(confirm) = confirm_for_button.as_ref() else {
                                            return;
                                        };
                                        let Some(editor) = weak_editor_for_button.upgrade() else {
                                            return;
                                        };
                                        let new_value =
                                            editor.read_with(cx, |editor, cx| editor.text(cx));
                                        let new_value =
                                            (!new_value.is_empty()).then_some(new_value);
                                        confirm(new_value, window, cx);
                                        if clear_on_confirm_for_button {
                                            editor.update(cx, |editor, cx| {
                                                editor.set_text("", window, cx);
                                            });
                                        }
                                    }),
                            )
                        },
                    )
                    .when_some(self.action_slot, |this, action| this.child(action)),
            )
            .when_some(self.confirm, |this, confirm| {
                this.on_action::<menu::Confirm>({
                    move |_, window, cx| {
                        let Some(editor) = weak_editor.upgrade() else {
                            return;
                        };
                        let new_value = editor.read_with(cx, |editor, cx| editor.text(cx));
                        let new_value = (!new_value.is_empty()).then_some(new_value);
                        confirm(new_value, window, cx);
                        if clear_on_confirm {
                            editor.update(cx, |editor, cx| {
                                editor.set_text("", window, cx);
                            });
                        }
                    }
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use gpui::{Context, Entity, Render, TestAppContext, VisualTestContext};
    use settings::SettingsStore;

    use super::*;

    struct Field {
        initial: Option<String>,
        confirmed: Rc<RefCell<Vec<Option<String>>>>,
        /// Something else to focus, the way a click on a neighbouring control
        /// takes focus from the field.
        elsewhere: gpui::FocusHandle,
    }

    impl Render for Field {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let confirmed = self.confirmed.clone();
            v_flex()
                .size_full()
                .child(
                    div()
                        .id("elsewhere")
                        .debug_selector(|| "elsewhere".into())
                        .track_focus(&self.elsewhere)
                        .size_16(),
                )
                .child(
                    div().debug_selector(|| "field".into()).w_64().child(
                        SettingsInputField::new()
                            .tab_index(0)
                            .when_some(self.initial.clone(), |field, text| {
                                field.with_initial_text(text)
                            })
                            .on_confirm(move |text, _window, _cx| {
                                confirmed.borrow_mut().push(text)
                            }),
                    ),
                )
        }
    }

    fn field(
        cx: &mut TestAppContext,
    ) -> (
        Entity<Field>,
        Rc<RefCell<Vec<Option<String>>>>,
        &mut VisualTestContext,
    ) {
        cx.update(|cx| {
            let store = SettingsStore::test(cx);
            cx.set_global(store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            editor::init(cx);
        });
        let confirmed = Rc::new(RefCell::new(Vec::new()));
        let (view, cx) = cx.add_window_view({
            let confirmed = confirmed.clone();
            move |_, cx| Field {
                initial: None,
                confirmed,
                elsewhere: cx.focus_handle(),
            }
        });
        // Focus changes are only reported in an active window.
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        (view, confirmed, cx)
    }

    fn focus_and_blur(view: &Entity<Field>, typed: &str, cx: &mut VisualTestContext) {
        let bounds = cx.debug_bounds("field").expect("the field is drawn");
        cx.simulate_click(bounds.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        assert!(
            cx.update(|window, cx| window.focused(cx).is_some()),
            "clicking the field focuses its editor"
        );
        if !typed.is_empty() {
            cx.simulate_input(typed);
        }
        let elsewhere = view.read_with(cx, |field, _| field.elsewhere.clone());
        cx.update(|window, cx| elsewhere.focus(window, cx));
        cx.run_until_parked();
    }

    /// The background image picker writes the path and the page redraws with
    /// it. A field that started empty used to keep showing nothing, so the
    /// next blur wrote the empty text back over the chosen path.
    #[gpui::test]
    async fn a_value_set_elsewhere_reaches_a_field_that_started_empty(cx: &mut TestAppContext) {
        let (view, confirmed, cx) = field(cx);

        view.update(cx, |field, cx| {
            field.initial = Some("/a.png".into());
            cx.notify();
        });
        cx.run_until_parked();

        focus_and_blur(&view, "x", cx);
        assert_eq!(
            confirmed.borrow().last().cloned(),
            Some(Some("/a.pngx".to_string())),
            "the field shows the new value, so typing extends it"
        );
    }

    /// Focusing a field and leaving it without typing must not write anything:
    /// clicking a field on the way to a button beside it used to blank the
    /// setting.
    #[gpui::test]
    async fn leaving_an_untouched_field_writes_nothing(cx: &mut TestAppContext) {
        let (view, confirmed, cx) = field(cx);
        focus_and_blur(&view, "", cx);
        assert!(
            confirmed.borrow().is_empty(),
            "wrote {:?}",
            confirmed.borrow()
        );
    }
}
