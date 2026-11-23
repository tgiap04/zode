use gpui::{Focusable as _, IntoElement, Render, Window};
use ui::{ButtonLike, Callout, Modal, ModalFooter, ModalHeader, Section, prelude::*};

use crate::avatar::render_avatar;
use crate::projects::create_project::{CreateProjectDialog, OwnerChoice, OwnersLoad};
use crate::projects::move_item::WRITE_SCOPE_GUIDANCE;
use crate::projects::projects_section::ProjectsSectionEvent;
use crate::untrusted_text::reveal_hidden_characters;

/// Tab order follows the order things are drawn in.
const OWNER_TAB_INDEX: isize = 0;
const TITLE_TAB_INDEX: isize = 2;
const CANCEL_TAB_INDEX: isize = 3;
const CREATE_TAB_INDEX: isize = 4;

impl CreateProjectDialog {
    fn render_owners(&self, cx: &mut Context<Self>) -> AnyElement {
        let choices = self.owner_choices();
        if choices.is_empty() {
            let text = match &self.owners {
                OwnersLoad::Failed(error) => error.clone(),
                _ => "Reading who may own the project…".into(),
            };
            let color = match &self.owners {
                OwnersLoad::Failed(_) => Color::Warning,
                _ => Color::Muted,
            };
            return Label::new(text)
                .size(LabelSize::Small)
                .color(color)
                .into_any_element();
        }
        let selected = self.owner();
        let colors = cx.theme().colors();
        v_flex()
            .gap_0p5()
            .child(
                Label::new("Owner")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                h_flex()
                    .debug_selector(|| "github-create-project-owners".to_string())
                    .p_0p5()
                    .gap_0p5()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .children(
                        choices
                            .into_iter()
                            .enumerate()
                            .map(|(index, (choice, login))| {
                                let shown = match choice {
                                    OwnerChoice::Viewer => "@me".to_string(),
                                    OwnerChoice::Organization => {
                                        reveal_hidden_characters(&login).into_owned()
                                    }
                                };
                                ButtonLike::new(("github-create-project-owner", index))
                                    .tab_index(OWNER_TAB_INDEX + index as isize)
                                    .toggle_state(choice == selected)
                                    .child(
                                        h_flex()
                                            .gap_1()
                                            .px_1()
                                            .child(render_avatar(
                                                &login,
                                                None,
                                                self.owner_account(choice)
                                                    .and_then(|owner| owner.avatar_url.as_deref()),
                                                px(16.),
                                                cx,
                                            ))
                                            .child(Label::new(shown).size(LabelSize::Small)),
                                    )
                                    .on_click(cx.listener(move |this, _, _window, cx| {
                                        this.select_owner(choice, cx)
                                    }))
                            }),
                    ),
            )
            .into_any_element()
    }

    fn render_title(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let focus_handle = self
            .title
            .focus_handle(cx)
            .tab_index(TITLE_TAB_INDEX)
            .tab_stop(true);
        let focused = focus_handle.is_focused(window);
        let too_long = self
            .title_problem(cx)
            .filter(|_| !self.title_text(cx).is_empty());
        v_flex()
            .gap_0p5()
            .child(
                Label::new("Title")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                div()
                    .track_focus(&focus_handle)
                    .on_action(cx.listener(|this, _: &menu::Confirm, _window, cx| this.submit(cx)))
                    .px_1()
                    .py_0p5()
                    .rounded_sm()
                    .border_1()
                    .border_color(if focused {
                        colors.border_focused
                    } else {
                        colors.border
                    })
                    .bg(colors.editor_background)
                    .child(self.title.clone()),
            )
            .children(too_long.map(|problem| {
                Label::new(problem)
                    .size(LabelSize::Small)
                    .color(Color::Error)
            }))
    }

    fn render_scope_guidance(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let can_sign_in = github::sign_in_with_github_available(cx);
        let forward = |event: fn() -> ProjectsSectionEvent| {
            cx.listener(move |this: &mut Self, _, _window, cx| {
                this.section().update(cx, |_, cx| cx.emit(event())).ok();
                this.cancel(cx);
            })
        };
        let sign_in = forward(|| ProjectsSectionEvent::SignInWithGithub);
        let check_again = forward(|| ProjectsSectionEvent::CheckAgain);
        v_flex()
            .debug_selector(|| "github-create-project-guidance".to_string())
            .gap_2()
            .child(
                Callout::new()
                    .severity(Severity::Info)
                    .title("Creating a project needs another permission")
                    .description(WRITE_SCOPE_GUIDANCE),
            )
            .child(
                h_flex()
                    .gap_2()
                    .when(can_sign_in, |row| {
                        row.child(
                            Button::new("github-create-project-sign-in", "Sign in with GitHub")
                                .style(ButtonStyle::Filled)
                                .on_click(sign_in),
                        )
                    })
                    .child(
                        Button::new("github-create-project-check-again", "Check again")
                            .on_click(check_again),
                    ),
            )
    }

    fn render_form(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let helper = format!(
            "Linked to {}. GitHub adds a Status field (Todo, In progress, Done).",
            self.context.repository
        );
        v_flex()
            .gap_2()
            .child(self.render_owners(cx))
            .child(self.render_title(window, cx))
            .child(
                Label::new(helper)
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .when_some(self.error().map(str::to_string), |column, error| {
                column.child(
                    div()
                        .debug_selector(|| "github-create-project-error".to_string())
                        .child(
                            Callout::new()
                                .severity(Severity::Error)
                                .description(reveal_hidden_characters(&error).into_owned()),
                        ),
                )
            })
    }
}

impl Render for CreateProjectDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let missing_scope = self.missing_scope();
        let can_create = self.can_create(cx);
        let creating = self.creating;
        let body = if missing_scope {
            self.render_scope_guidance(cx).into_any_element()
        } else {
            self.render_form(window, cx).into_any_element()
        };
        let create = (!missing_scope).then(|| {
            Button::new("github-create-project-submit", "Create project")
                .style(ButtonStyle::Filled)
                .tab_index(CREATE_TAB_INDEX)
                .disabled(!can_create)
                .loading(creating)
                .on_click(cx.listener(|this, _, _window, cx| this.submit(cx)))
        });
        v_flex()
            .key_context("GithubCreateProject")
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.cancel(cx)))
            .elevation_3(cx)
            .w(rems(28.))
            .child(
                Modal::new("github-create-project", None)
                    .header(
                        ModalHeader::new()
                            .icon(Icon::new(IconName::Github).size(IconSize::Small))
                            .headline("New project")
                            .description(self.target_line()),
                    )
                    .section(Section::new().child(body))
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_1()
                                .child(
                                    Button::new("github-create-project-cancel", "Cancel")
                                        .tab_index(CANCEL_TAB_INDEX)
                                        .on_click(
                                            cx.listener(|this, _, _window, cx| this.cancel(cx)),
                                        ),
                                )
                                .children(create),
                        ),
                    ),
            )
    }
}
