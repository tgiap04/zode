use github::GithubUser;
use gpui::{AnyElement, Focusable as _, IntoElement, MouseButton, Render, Window, px};
use ui::prelude::*;

use crate::avatar::render_avatar;
use crate::issues::assignee_picker::{AssigneePicker, AssigneeResults};
use crate::untrusted_text::reveal_hidden_characters;

const RESULT_HEIGHT: f32 = 32.;
const RESULTS_MAX_HEIGHT: f32 = 240.;

/// An account as a chip: its picture or initials, then `@login`.
pub(crate) fn render_person_chip(user: &GithubUser, cx: &App) -> impl IntoElement {
    h_flex()
        .flex_none()
        .h(px(22.))
        .gap_1()
        .pl_0p5()
        .pr_1p5()
        .rounded_full()
        .bg(cx.theme().colors().element_background)
        .border_1()
        .border_color(cx.theme().colors().border_variant)
        .child(render_avatar(
            &user.login,
            user.name.as_deref(),
            user.avatar_url.as_deref(),
            px(16.),
            cx,
        ))
        .child(
            Label::new(format!("@{}", reveal_hidden_characters(&user.login)))
                .size(LabelSize::Small)
                .single_line(),
        )
}

fn message_row(icon: IconName, text: String, color: Color) -> AnyElement {
    h_flex()
        .h(px(RESULT_HEIGHT))
        .px_2()
        .gap_2()
        .child(Icon::new(icon).size(IconSize::Small).color(color))
        .child(Label::new(text).size(LabelSize::Small).color(color))
        .into_any_element()
}

impl AssigneePicker {
    fn render_result(&self, index: usize, user: &GithubUser, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors();
        let highlighted = index == self.highlighted;
        let chosen = self.selected.contains(&user.login);
        let login = reveal_hidden_characters(&user.login).into_owned();
        let name = user
            .name
            .as_deref()
            .map(|name| reveal_hidden_characters(name).into_owned());
        let picked = user.clone();
        h_flex()
            .id(("github-assignee-result", index))
            .h(px(RESULT_HEIGHT))
            .px_2()
            .gap_2()
            .cursor_pointer()
            .when(highlighted, |row| row.bg(colors.element_selected))
            .hover(|row| row.bg(colors.element_hover))
            // Keeps the search field focused, so the list is still there
            // when the click lands.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(cx.listener(move |this, _, window, cx| this.pick(picked.clone(), window, cx)))
            .child(render_avatar(
                &user.login,
                user.name.as_deref(),
                user.avatar_url.as_deref(),
                px(20.),
                cx,
            ))
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1p5()
                    .child(
                        Label::new(name.clone().unwrap_or_else(|| login.clone()))
                            .single_line()
                            .truncate(),
                    )
                    .when(name.is_some(), |line| {
                        line.child(
                            Label::new(format!("@{login}"))
                                .size(LabelSize::Small)
                                .color(Color::Muted)
                                .single_line(),
                        )
                    }),
            )
            .when(chosen, |row| {
                row.child(
                    Icon::new(IconName::Check)
                        .size(IconSize::Small)
                        .color(Color::Accent),
                )
            })
            .into_any_element()
    }

    fn render_results(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows = match &self.results() {
            AssigneeResults::Idle | AssigneeResults::Searching => vec![message_row(
                IconName::LoadCircle,
                "Searching…".to_string(),
                Color::Muted,
            )],
            AssigneeResults::Failed(error) => vec![message_row(
                IconName::Warning,
                format!("Could not search people: {error}"),
                Color::Error,
            )],
            AssigneeResults::Found(users) if users.is_empty() => vec![message_row(
                IconName::Person,
                if self.query().trim().is_empty() {
                    "Nobody can be assigned in this repository".to_string()
                } else {
                    format!(
                        "No users match “{}”",
                        reveal_hidden_characters(self.query().trim())
                    )
                },
                Color::Muted,
            )],
            AssigneeResults::Found(users) => users
                .iter()
                .enumerate()
                .map(|(index, user)| self.render_result(index, user, cx))
                .collect(),
        };
        v_flex()
            .id("github-assignee-results")
            .w_full()
            .max_h(px(RESULTS_MAX_HEIGHT))
            .overflow_y_scroll()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors().border_variant)
            .bg(cx.theme().colors().elevated_surface_background)
            .children(rows)
            .into_any_element()
    }
}

impl Render for AssigneePicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let focused = self.query_editor.focus_handle(cx).is_focused(window);
        v_flex()
            .key_context("GithubAssigneePicker")
            .on_action(cx.listener(|this, _: &menu::SelectNext, _window, cx| this.select_next(cx)))
            .on_action(
                cx.listener(|this, _: &menu::SelectPrevious, _window, cx| this.select_previous(cx)),
            )
            .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| this.confirm(window, cx)))
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| {
                // Esc closes the results first, and only then whatever holds
                // the picker.
                if this.is_open() {
                    this.close(cx);
                } else {
                    cx.propagate();
                }
            }))
            .w_full()
            .gap_1()
            .child(
                h_flex()
                    .h(px(28.))
                    .px_2()
                    .gap_1p5()
                    .rounded_md()
                    .border_1()
                    .border_color(if focused {
                        colors.border_focused
                    } else {
                        colors.border
                    })
                    .bg(colors.editor_background)
                    .child(
                        Icon::new(IconName::MagnifyingGlass)
                            .size(IconSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(div().flex_1().child(self.query_editor.clone())),
            )
            .when(self.is_open(), |column| {
                column.child(self.render_results(cx))
            })
    }
}
