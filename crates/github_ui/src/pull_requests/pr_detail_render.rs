use github::{GithubError, PullRequestDetails};
use gpui::{Context, IntoElement, Render, Window};
use time::OffsetDateTime;
use ui::{Callout, Tooltip, prelude::*};

use crate::github_view_session::session_ended_message;
use crate::issues::{callout, composer_field, editor_field, render_target, updated_label};
use crate::pull_requests::check_link::check_link_host;
use crate::pull_requests::merge_modal::MergeAvailability;
use crate::pull_requests::pr_detail_checkout::CheckoutState;
use crate::pull_requests::pr_detail_view::{PullRequestComment, PullRequestDetailView};
use crate::pull_requests::pr_labels::{
    branches_label, check_label, checks_label, file_change_label, login_label,
    review_decision_label, review_state_label, state_label,
};
use crate::untrusted_text::reveal_hidden_characters;

fn error_callout(error: &GithubError) -> AnyElement {
    Callout::new()
        .severity(Severity::Warning)
        .description(error.to_string())
        .into_any_element()
}

fn small(text: impl Into<SharedString>, color: Color) -> Label {
    Label::new(text).size(LabelSize::Small).color(color)
}

fn short_commit(sha: &str) -> String {
    reveal_hidden_characters(sha.get(..12).unwrap_or(sha)).into_owned()
}

fn heading(text: &'static str) -> impl IntoElement {
    Label::new(text).size(LabelSize::Small).color(Color::Muted)
}

impl PullRequestDetailView {
    fn render_actions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let checkout_blocker = self.checkout_blocker(cx);
        let merge = self.merge_availability();
        h_flex()
            .gap_1()
            .flex_wrap()
            .child(
                Button::new("github-pr-open", "Open on GitHub")
                    .label_size(LabelSize::Small)
                    .on_click(cx.listener(|this, _, _window, cx| this.open_on_github(cx))),
            )
            .child(
                IconButton::new("github-pr-reload", IconName::RotateCw)
                    .tooltip(Tooltip::text("Reload"))
                    .on_click(cx.listener(|this, _, _window, cx| this.reload(cx))),
            )
            .when(self.details.is_some(), |row| match checkout_blocker {
                None => row.child(
                    Button::new("github-pr-checkout", "Check out")
                        .label_size(LabelSize::Small)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.start_checkout(window, cx)),
                        ),
                ),
                Some(blocker) => row.child(small(blocker.explanation(), Color::Muted)),
            })
            .map(|row| match merge {
                Some(MergeAvailability::Available(_)) => row.child(
                    Button::new("github-pr-merge", "Merge…")
                        .style(ButtonStyle::Filled)
                        .label_size(LabelSize::Small)
                        .disabled(self.login.is_none() || self.session_ended(cx))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.request_merge(window, cx)),
                        ),
                ),
                Some(MergeAvailability::MergeQueue) => row
                    .child(small(
                        "This repository merges through a merge queue.",
                        Color::Muted,
                    ))
                    .child(
                        Button::new("github-pr-merge-on-github", "Merge on GitHub")
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(|this, _, _window, cx| this.open_on_github(cx))),
                    ),
                Some(MergeAvailability::Checking) => row.child(small(
                    "GitHub is checking whether it can merge; reload in a moment.",
                    Color::Muted,
                )),
                Some(MergeAvailability::Unavailable(reason)) => {
                    row.child(small(reason, Color::Muted))
                }
                None => row,
            })
    }

    fn render_checkout(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.checkout.clone();
        let element = match state {
            CheckoutState::Idle => return None,
            CheckoutState::ForkWarning => {
                small("Waiting for the fork warning to be answered…", Color::Muted)
                    .into_any_element()
            }
            CheckoutState::Naming { error } => {
                let editor = self.branch_editor(window, cx);
                let worktree = self.checkout_worktree_path(cx);
                v_flex()
                    .gap_1()
                    .when_some(worktree, |column, worktree| {
                        column.child(small(
                            format!("Switches the branch of {worktree}"),
                            Color::Muted,
                        ))
                    })
                    .child(editor_field("Local branch name", &editor, cx))
                    .when_some(error, |column, error| {
                        column.child(callout(Severity::Error, error))
                    })
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("github-pr-checkout-confirm", "Check out")
                                    .style(ButtonStyle::Filled)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.confirm_checkout(false, window, cx)
                                    })),
                            )
                            .child(Button::new("github-pr-checkout-cancel", "Cancel").on_click(
                                cx.listener(|this, _, _window, cx| this.cancel_checkout(cx)),
                            )),
                    )
                    .into_any_element()
            }
            CheckoutState::BranchExists { branch } => v_flex()
                .gap_1()
                .child(Label::new(format!(
                    "A local branch {} already exists.",
                    reveal_hidden_characters(&branch)
                )))
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("github-pr-checkout-reuse", "Switch to it").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.confirm_checkout(true, window, cx)
                                }),
                            ),
                        )
                        .child(
                            Button::new("github-pr-checkout-rename", "Use another name").on_click(
                                cx.listener(|this, _, _window, cx| this.rename_checkout_branch(cx)),
                            ),
                        ),
                )
                .into_any_element(),
            CheckoutState::Running => small("Checking out…", Color::Muted).into_any_element(),
            CheckoutState::Finished(text) => callout(Severity::Success, text),
            CheckoutState::PullRequestChanged(fetched) => v_flex()
                .gap_1()
                .child(callout(
                    Severity::Warning,
                    format!(
                        "The pull request changed since it was shown: its head is now {}. \
                         Nothing was checked out. Reload it, look again, then check out.",
                        short_commit(&fetched)
                    ),
                ))
                .child(
                    h_flex().child(Button::new("github-pr-checkout-reload", "Reload").on_click(
                        cx.listener(|this, _, _window, cx| {
                            this.checkout = CheckoutState::Idle;
                            this.reload(cx);
                        }),
                    )),
                )
                .into_any_element(),
            CheckoutState::Failed(text) => callout(Severity::Error, text),
        };
        Some(element)
    }

    fn render_summary(&self, details: &PullRequestDetails) -> impl IntoElement {
        let pull_request = &details.pull_request;
        let (state, state_color) = state_label(pull_request);
        v_flex()
            .gap_1()
            .child(
                Headline::new(format!(
                    "{} #{}",
                    reveal_hidden_characters(&pull_request.title),
                    pull_request.number
                ))
                .size(HeadlineSize::Small),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(Label::new(state).color(state_color))
                    .child(small(
                        format!("by {}", login_label(pull_request.author.as_deref())),
                        Color::Muted,
                    ))
                    .child(small(branches_label(pull_request), Color::Muted))
                    .when_some(pull_request.checks, |row, checks| {
                        let (text, color) = checks_label(checks);
                        row.child(small(format!("checks {text}"), color))
                    })
                    .when_some(pull_request.review_decision, |row, decision| {
                        let (text, color) = review_decision_label(decision);
                        row.child(small(text, color))
                    }),
            )
    }

    fn render_checks(&self, details: &PullRequestDetails, cx: &mut Context<Self>) -> AnyElement {
        let more = details
            .checks_total
            .saturating_sub(details.checks.len() as u64);
        v_flex()
            .gap_0p5()
            .child(heading("Checks"))
            .children(details.checks.iter().enumerate().map(|(index, check)| {
                let (text, color) = check_label(check.state);
                let url = check.url.clone();
                h_flex()
                    .gap_2()
                    .child(small(text, color))
                    .child(small(
                        reveal_hidden_characters(&check.name).into_owned(),
                        Color::Default,
                    ))
                    .when_some(url, |row, url| {
                        row.child(
                            Button::new(("github-pr-check-link", index), "Details")
                                .label_size(LabelSize::XSmall)
                                .tooltip(Tooltip::text(format!("Opens {}", check_link_host(&url))))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_check_details(&url, window, cx)
                                })),
                        )
                    })
            }))
            .when(more > 0, |column| {
                column.child(small(format!("and {more} more on GitHub"), Color::Muted))
            })
            .when(details.checks.is_empty(), |column| {
                column.child(small("No checks.", Color::Muted))
            })
            .into_any_element()
    }

    fn render_reviews(&self, details: &PullRequestDetails) -> AnyElement {
        v_flex()
            .gap_0p5()
            .child(heading("Reviews"))
            .children(details.reviews.iter().map(|review| {
                let (text, color) = review_state_label(review.state);
                h_flex()
                    .gap_2()
                    .child(small(login_label(review.author.as_deref()), Color::Default))
                    .child(small(text, color))
            }))
            .when(details.reviews.is_empty(), |column| {
                column.child(small("No reviews yet.", Color::Muted))
            })
            .into_any_element()
    }

    fn render_files(&self, details: &PullRequestDetails) -> AnyElement {
        let more = details
            .files_total
            .saturating_sub(details.files.len() as u64);
        v_flex()
            .gap_0p5()
            .child(heading("Changed files"))
            .children(details.files.iter().map(|file| {
                h_flex()
                    .gap_2()
                    .child(small(format!("+{}", file.additions), Color::Success))
                    .child(small(format!("−{}", file.deletions), Color::Error))
                    .child(small(
                        reveal_hidden_characters(&file.path).into_owned(),
                        Color::Default,
                    ))
                    .child(small(file_change_label(file.change), Color::Muted))
            }))
            .when(more > 0, |column| {
                column.child(small(format!("and {more} more on GitHub"), Color::Muted))
            })
            .into_any_element()
    }

    fn render_comment(
        &self,
        comment: &PullRequestComment,
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
                    .child(small(
                        login_label(comment.comment.author.as_deref()),
                        Color::Default,
                    ))
                    .child(small(
                        updated_label(&comment.comment.created_at, now),
                        Color::Muted,
                    )),
            )
            .child(comment.body.clone())
            .into_any_element()
    }

    fn render_comment_box(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let composer = self.comment_composer(window, cx);
        v_flex()
            .key_context("GithubPullRequestComment")
            // Same keys as the issue tab: ⌘↩ arrives as SecondaryConfirm on
            // macOS, Ctrl-↩ is caught as the editor's newline elsewhere.
            .on_action(cx.listener(|this, _: &menu::SecondaryConfirm, window, cx| {
                this.send_comment(window, cx)
            }))
            .capture_action(
                cx.listener(|this, _: &editor::actions::Newline, window, cx| {
                    if window.modifiers().secondary() {
                        this.send_comment(window, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .gap_1()
            .children(composer.map(|composer| composer_field("Comment", composer)))
            .when_some(self.comment_error.clone(), |column, error| {
                column.child(callout(Severity::Error, error))
            })
            .child(
                h_flex().child(
                    Button::new(
                        "github-pr-send-comment",
                        if self.sending_comment {
                            "Sending…"
                        } else {
                            "Comment"
                        },
                    )
                    .style(ButtonStyle::Filled)
                    .disabled(self.sending_comment || self.session_ended(cx))
                    .on_click(cx.listener(|this, _, window, cx| this.send_comment(window, cx))),
                ),
            )
            .into_any_element()
    }
}

impl Render for PullRequestDetailView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = OffsetDateTime::now_utc();
        let details = self.details.clone();
        let comments = self
            .comments
            .iter()
            .map(|comment| self.render_comment(comment, now, cx))
            .collect::<Vec<_>>();
        let checkout = self.render_checkout(window, cx);
        let summary = details
            .as_ref()
            .map(|details| self.render_summary(details).into_any_element());
        let sections = details.as_ref().map(|details| {
            [
                self.render_checks(details, cx),
                self.render_reviews(details),
                self.render_files(details),
            ]
        });
        let comment_box = details
            .is_some()
            .then(|| self.render_comment_box(window, cx));
        v_flex()
            .id("github-pull-request-detail")
            .key_context("GithubPullRequestDetail")
            .track_focus(&self.focus_handle)
            .size_full()
            .overflow_y_scroll()
            .gap_2()
            .p_3()
            .bg(cx.theme().colors().editor_background)
            .when_some(self.target_line(), |column, target| {
                column.child(
                    div()
                        .debug_selector(|| "github-pr-target".to_string())
                        .child(render_target(target, self.visibility_error.clone())),
                )
            })
            .when(self.session_ended(cx), |column| {
                column.child(callout(
                    Severity::Warning,
                    session_ended_message("pull request"),
                ))
            })
            .child(self.render_actions(cx))
            .children(checkout)
            .when_some(self.error.as_ref(), |column, error| {
                column.child(error_callout(error))
            })
            .when(details.is_none() && self.error.is_none(), |column| {
                column.child(Label::new("Loading the pull request…").color(Color::Muted))
            })
            .children(summary)
            .when_some(self.body.clone(), |column, body| column.child(body))
            .children(sections.into_iter().flatten())
            .children(comments)
            .when_some(self.comments_error.as_ref(), |column, error| {
                column.child(error_callout(error))
            })
            .when(self.has_more_comments(), |column| {
                column.child(
                    Button::new(
                        "github-pr-more-comments",
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
    }
}
