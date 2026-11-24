use editor::Editor;
use github::{RepositoryId, RepositoryVisibility};
use gpui::{AnyElement, Entity, Focusable as _, IntoElement, Render, Window, px};
use ui::{Callout, Modal, ModalFooter, ModalHeader, Section, prelude::*};

use crate::issues::issue_editor::CreateIssueDialog;
use crate::markdown_composer::MarkdownComposer;
use crate::untrusted_text::reveal_hidden_characters;

pub(crate) fn editor_field(
    label: &'static str,
    editor: &Entity<Editor>,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(Label::new(label).size(LabelSize::Small).color(Color::Muted))
        .child(
            div()
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().colors().border)
                .bg(cx.theme().colors().editor_background)
                .child(editor.clone()),
        )
}

/// A label over a markdown field's Write and Preview tabs.
pub(crate) fn composer_field(
    label: &'static str,
    composer: Entity<MarkdownComposer>,
) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(Label::new(label).size(LabelSize::Small))
        .child(composer)
}

/// A labelled editor whose border shows keyboard focus, with its own error
/// right under it.
pub(crate) fn focused_field(
    label: &'static str,
    editor: &Entity<Editor>,
    error: Option<&'static str>,
    window: &Window,
    cx: &App,
) -> impl IntoElement {
    let colors = cx.theme().colors();
    let focused = editor.focus_handle(cx).is_focused(window);
    let border = if error.is_some() {
        cx.theme().status().error_border
    } else if focused {
        colors.border_focused
    } else {
        colors.border
    };
    v_flex()
        .gap_1()
        .child(Label::new(label).size(LabelSize::Small))
        .child(
            div()
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(border)
                .bg(colors.editor_background)
                .child(editor.clone()),
        )
        .when_some(error, |field, error| {
            field.child(
                h_flex()
                    .gap_1()
                    .child(
                        Icon::new(IconName::Warning)
                            .size(IconSize::XSmall)
                            .color(Color::Error),
                    )
                    .child(Label::new(error).size(LabelSize::Small).color(Color::Error)),
            )
        })
}

/// Who writes and where, the first thing the dialog shows.
pub(crate) fn render_target(line: String, error: Option<String>) -> impl IntoElement {
    v_flex()
        .gap_0p5()
        .child(
            h_flex()
                .gap_1()
                .child(
                    Icon::new(IconName::Github)
                        .size(IconSize::Small)
                        .color(Color::Muted),
                )
                .child(Label::new(line).size(LabelSize::Small).color(Color::Muted)),
        )
        .when_some(error, |column, error| {
            column.child(
                Label::new(format!("Could not read the repository: {error}"))
                    .size(LabelSize::Small)
                    .color(Color::Warning),
            )
        })
}

/// `@login → owner/repo` and a badge saying who can see the repository.
fn render_target_badge(
    login: &str,
    repository: &RepositoryId,
    visibility: Option<RepositoryVisibility>,
    error: Option<String>,
    cx: &App,
) -> AnyElement {
    let (icon, text) = match visibility {
        Some(RepositoryVisibility::Private) => (IconName::LockOutlined, "Private"),
        Some(RepositoryVisibility::Internal) => (IconName::LockOutlined, "Internal"),
        Some(RepositoryVisibility::Public) => (IconName::Public, "Public"),
        None => (IconName::LoadCircle, "Checking…"),
    };
    v_flex()
        .gap_0p5()
        .child(
            h_flex()
                .gap_1p5()
                .child(
                    Label::new(format!(
                        "@{} → {repository}",
                        reveal_hidden_characters(login)
                    ))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(
                    h_flex()
                        .h(px(18.))
                        .px_1p5()
                        .gap_1()
                        .rounded_full()
                        .border_1()
                        .border_color(cx.theme().colors().border_variant)
                        .child(Icon::new(icon).size(IconSize::XSmall).color(Color::Muted))
                        .child(Label::new(text).size(LabelSize::XSmall).color(Color::Muted)),
                ),
        )
        .when_some(error, |column, error| {
            column.child(
                Label::new(format!("Could not read the repository: {error}"))
                    .size(LabelSize::Small)
                    .color(Color::Warning),
            )
        })
        .into_any_element()
}

impl Render for CreateIssueDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let can_create = self.can_create(cx);
        let visibility_error = self.visibility_error.as_ref().map(ToString::to_string);
        let target = render_target_badge(
            &self.target.login,
            &self.target.repository,
            self.visibility,
            visibility_error,
            cx,
        );
        let properties = self.render_properties(window, cx);
        v_flex()
            .key_context("GithubCreateIssue")
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.cancel(cx)))
            .on_action(cx.listener(|this, _: &menu::SecondaryConfirm, _window, cx| this.submit(cx)))
            // Linux and Windows: Ctrl-↩ is the description's own newline, so
            // it is caught on the way down while Ctrl is held.
            .capture_action(
                cx.listener(|this, _: &editor::actions::Newline, window, cx| {
                    if window.modifiers().secondary() {
                        this.submit(cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .elevation_3(cx)
            .w(rems(42.))
            .child(
                Modal::new("github-create-issue", None)
                    .header(
                        ModalHeader::new()
                            .icon(Icon::new(IconName::Github).color(Color::Muted))
                            .headline("New issue")
                            .child(div().pt_1().child(target)),
                    )
                    .section(
                        Section::new().child(
                            v_flex()
                                .gap_3()
                                .child(focused_field(
                                    "Title",
                                    &self.title,
                                    self.title_error,
                                    window,
                                    cx,
                                ))
                                .child(composer_field("Description", self.description.clone()))
                                .child(properties)
                                .when_some(self.error.clone(), |column, error| {
                                    column.child(
                                        Callout::new()
                                            .severity(Severity::Error)
                                            .title("The issue was not created")
                                            .description(error),
                                    )
                                }),
                        ),
                    )
                    .footer(
                        ModalFooter::new()
                            .start_slot(
                                Label::new("⌘↩ or Ctrl-↩ creates")
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                            .end_slot(
                                h_flex()
                                    .gap_1()
                                    .child(
                                        Button::new("github-create-issue-cancel", "Cancel")
                                            .style(ButtonStyle::Subtle)
                                            .on_click(
                                                cx.listener(|this, _, _window, cx| this.cancel(cx)),
                                            ),
                                    )
                                    .child(
                                        Button::new(
                                            "github-create-issue-submit",
                                            if self.submitting {
                                                "Creating…"
                                            } else {
                                                "Create issue"
                                            },
                                        )
                                        .style(ButtonStyle::Filled)
                                        .loading(self.submitting)
                                        .disabled(!can_create)
                                        .on_click(
                                            cx.listener(|this, _, _window, cx| this.submit(cx)),
                                        ),
                                    ),
                            ),
                    ),
            )
    }
}
