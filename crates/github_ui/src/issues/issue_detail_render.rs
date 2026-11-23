use github::{GithubError, Issue};
use gpui::{Context, IntoElement, Render, Window, px};
use time::OffsetDateTime;
use ui::{Callout, Tooltip, prelude::*};

use crate::github_view_session::session_ended_message;
use crate::issues::issue_detail_view::{IssueComment, IssueDetailView};
use crate::issues::issue_detail_write_render::callout;
use crate::issues::issue_editor_render::render_target;
use crate::issues::issue_list_render::updated_label;
use crate::issues::issue_row_render::{render_assignee_faces, render_label_chip, state_icon};
use crate::issues::issue_type_picker::render_type_chip;
use crate::untrusted_text::reveal_hidden_characters;

fn error_callout(error: &GithubError) -> AnyElement {
    Callout::new()
        .severity(Severity::Warning)
        .description(error.to_string())
        .into_any_element()
}

fn login(login: Option<&str>) -> String {
    login.map_or("a deleted account".to_string(), |login| {
        format!("@{}", reveal_hidden_characters(login))
    })
}

impl IssueDetailView {
    fn render_actions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_1()
            .child(
                Button::new("github-issue-open", "Open on GitHub")
                    .label_size(LabelSize::Small)
                    .on_click(cx.listener(|this, _, _window, cx| this.open_on_github(cx))),
            )
            .child(
                Button::new("github-issue-copy-link", "Copy link")
                    .label_size(LabelSize::Small)
                    .on_click(cx.listener(|this, _, _window, cx| this.copy_link(cx))),
            )
            .child(
                IconButton::new("github-issue-reload", IconName::RotateCw)
                    .tooltip(Tooltip::text("Reload"))
                    .on_click(cx.listener(|this, _, _window, cx| this.reload(cx))),
            )
    }

    fn render_summary(&self, issue: &Issue, cx: &mut Context<Self>) -> impl IntoElement {
        let (icon, color, meaning) = state_icon(issue);
        let colors = cx.theme().colors();
        v_flex()
            .gap_1()
            .child(
                Headline::new(format!(
                    "{} #{}",
                    reveal_hidden_characters(&issue.title),
                    issue.number
                ))
                .size(HeadlineSize::Small),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        h_flex()
                            .h(px(22.))
                            .px_2()
                            .gap_1()
                            .rounded_full()
                            .bg(colors.element_background)
                            .border_1()
                            .border_color(colors.border_variant)
                            .child(Icon::new(icon).size(IconSize::Small).color(color))
                            .child(Label::new(meaning).size(LabelSize::Small)),
                    )
                    .when_some(issue.issue_type.as_deref(), |row, name| {
                        row.child(render_type_chip(name, &self.issue_types))
                    })
                    .child(
                        Label::new(format!("opened by {}", login(issue.author.as_deref())))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .children(render_assignee_faces(
                        "github-issue-detail-assignees",
                        issue,
                        cx,
                    ))
                    .children(
                        issue
                            .labels
                            .iter()
                            .map(|label| render_label_chip(label, cx)),
                    ),
            )
            .when_some(issue.milestone.as_ref(), |column, milestone| {
                column.child(
                    Label::new(format!(
                        "Milestone: {}",
                        reveal_hidden_characters(&milestone.title)
                    ))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
            })
    }

    fn render_comment(
        &self,
        comment: &IssueComment,
        now: OffsetDateTime,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        v_flex()
            .gap_1()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Label::new(login(comment.comment.author.as_deref())).size(LabelSize::Small),
                    )
                    .child(
                        Label::new(updated_label(&comment.comment.created_at, now))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            )
            .child(comment.body.clone())
            .into_any_element()
    }
}

impl IssueDetailView {
    /// Who writes where, so a tab opened from a link into another
    /// repository is never mistaken for one of the picked repository.
    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let error = self.visibility_error.as_ref().map(ToString::to_string);
        let failed = error.is_some();
        h_flex()
            .gap_1()
            .child(render_target(self.target_line(), error))
            .when(failed, |row| {
                row.child(
                    Button::new("github-issue-retry-visibility", "Retry")
                        .label_size(LabelSize::Small)
                        .on_click(cx.listener(|this, _, _window, cx| this.retry_visibility(cx))),
                )
            })
    }

    fn render_session_ended(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_1()
            .child(callout(Severity::Warning, session_ended_message("issue")))
            .child(
                h_flex().child(
                    Button::new("github-issue-reopen-tab", "Reopen")
                        .style(ButtonStyle::Filled)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.reopen_in_current_session(window, cx)
                        })),
                ),
            )
    }
}

impl Render for IssueDetailView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.session_ended(cx) {
            return v_flex()
                .id("github-issue-detail")
                .key_context("GithubIssueDetail")
                .track_focus(&self.focus_handle)
                .size_full()
                .p_3()
                .bg(cx.theme().colors().editor_background)
                .child(self.render_session_ended(cx))
                .into_any_element();
        }
        let now = OffsetDateTime::now_utc();
        let comments = self
            .comments
            .iter()
            .map(|comment| self.render_comment(comment, now, cx))
            .collect::<Vec<_>>();
        let issue = self.issue.clone();
        let write_actions = issue
            .as_ref()
            .map(|issue| self.render_write_actions(issue, window, cx));
        let edit_form = self.render_edit_form(cx);
        let editing = edit_form.is_some();
        let triage = issue
            .as_ref()
            .map(|issue| self.render_triage(issue, window, cx));
        let type_picker = self.render_type_picker(window, cx);
        let epic = issue
            .as_ref()
            .and_then(|issue| self.render_epic(issue, window, cx));
        let comment_box = issue.is_some().then(|| self.render_comment_box(window, cx));
        v_flex()
            .id("github-issue-detail")
            .key_context("GithubIssueDetail")
            .track_focus(&self.focus_handle)
            .size_full()
            .overflow_y_scroll()
            .gap_2()
            .p_3()
            .bg(cx.theme().colors().editor_background)
            .child(self.render_header(cx))
            .child(
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .child(self.render_actions(cx))
                    .children(write_actions),
            )
            .when_some(self.issue_error.as_ref(), |column, error| {
                column.child(error_callout(error))
            })
            .when_some(self.write_error.clone(), |column, error| {
                column.child(callout(Severity::Error, error))
            })
            .when_some(self.write_notice.clone(), |column, notice| {
                column.child(callout(Severity::Warning, notice))
            })
            .children(edit_form)
            .map(|column| match &issue {
                Some(_) if editing => column,
                Some(issue) => column.child(self.render_summary(issue, cx)),
                None if self.issue_error.is_none() => {
                    column.child(Label::new("Loading the issue…").color(Color::Muted))
                }
                None => column,
            })
            .children(triage)
            .children(type_picker)
            .children(epic)
            .when(!editing, |column| {
                column.when_some(self.body.clone(), |column, body| column.child(body))
            })
            .children(comments)
            .when_some(self.comments_error.as_ref(), |column, error| {
                column.child(error_callout(error))
            })
            .when(self.has_more_comments(), |column| {
                column.child(
                    Button::new(
                        "github-issue-more-comments",
                        if self.loading_comments {
                            "Loading…"
                        } else {
                            "Load more comments"
                        },
                    )
                    .disabled(self.loading_comments)
                    .on_click(cx.listener(|this, _, _window, cx| this.load_more_comments(cx))),
                )
            })
            .children(comment_box)
            .into_any_element()
    }
}
