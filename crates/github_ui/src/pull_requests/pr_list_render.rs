use github::{PullRequest, PullRequestStateFilter};
use gpui::{Context, IntoElement, Render, Window};
use time::OffsetDateTime;
use ui::{Chip, prelude::*};

use crate::issues::{ListLoad, updated_label};
use crate::pull_requests::pr_labels::{
    branches_label, checks_label, login_label, review_decision_label, state_label,
};
use crate::pull_requests::pr_list::{PullRequestList, PullRequestMine};
use crate::untrusted_text::reveal_hidden_characters;

const STATES: [(PullRequestStateFilter, &str); 4] = [
    (PullRequestStateFilter::Open, "Open"),
    (PullRequestStateFilter::Closed, "Closed"),
    (PullRequestStateFilter::Merged, "Merged"),
    (PullRequestStateFilter::All, "All"),
];

fn small(text: impl Into<SharedString>, color: Color) -> Label {
    Label::new(text).size(LabelSize::Small).color(color)
}

impl PullRequestList {
    fn render_filters(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state();
        let mine = self.mine();
        h_flex()
            .flex_wrap()
            .gap_1()
            .children(
                STATES
                    .into_iter()
                    .enumerate()
                    .map(|(index, (filter, text))| {
                        Button::new(("github-pr-state", index), text)
                            .label_size(LabelSize::Small)
                            .toggle_state(state == filter)
                            .on_click(
                                cx.listener(move |this, _, _window, cx| this.set_state(filter, cx)),
                            )
                    }),
            )
            .child(div().w_2())
            .children(
                PullRequestMine::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(index, choice)| {
                        let selected = mine == Some(choice);
                        Button::new(("github-pr-mine", index), choice.label())
                            .label_size(LabelSize::Small)
                            .toggle_state(selected)
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.set_mine((!selected).then_some(choice), cx)
                            }))
                    }),
            )
    }

    fn render_row(
        &self,
        pull_request: &PullRequest,
        now: OffsetDateTime,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let number = pull_request.number;
        let (state, state_color) = state_label(pull_request);
        h_flex()
            .id(("github-pr-row", number as usize))
            .w_full()
            .gap_2()
            .px_1()
            .py_0p5()
            .rounded_sm()
            .cursor_pointer()
            .hover(|row| row.bg(cx.theme().colors().element_hover))
            .on_click(
                cx.listener(move |this, _, window, cx| this.open_pull_request(number, window, cx)),
            )
            .child(small(format!("#{number}"), Color::Muted))
            .child(
                Label::new(reveal_hidden_characters(&pull_request.title).into_owned()).truncate(),
            )
            .when(pull_request.is_draft, |row| row.child(Chip::new("Draft")))
            .child(small(branches_label(pull_request), Color::Muted))
            .child(div().flex_1())
            .when_some(pull_request.checks, |row, checks| {
                let (text, color) = checks_label(checks);
                row.child(small(text, color))
            })
            .when_some(pull_request.review_decision, |row, decision| {
                let (text, color) = review_decision_label(decision);
                row.child(small(text, color))
            })
            .when(!pull_request.is_draft, |row| {
                row.child(small(state, state_color))
            })
            .child(small(
                login_label(pull_request.author.as_deref()),
                Color::Muted,
            ))
            .child(small(
                updated_label(&pull_request.updated_at, now),
                Color::Muted,
            ))
            .into_any_element()
    }
}

impl Render for PullRequestList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = OffsetDateTime::now_utc();
        let rows = self
            .visible_rows()
            .into_iter()
            .map(|pull_request| self.render_row(pull_request, now, cx))
            .collect::<Vec<_>>();
        let status = match self.load_state() {
            ListLoad::Loading => Some(small("Loading pull requests…", Color::Muted)),
            ListLoad::Failed(error) => Some(small(error.to_string(), Color::Warning)),
            ListLoad::Idle if rows.is_empty() => {
                Some(small("No pull requests match.", Color::Muted))
            }
            ListLoad::Idle | ListLoad::LoadingMore => None,
        };
        let loading_more = *self.load_state() == ListLoad::LoadingMore;
        v_flex()
            .size_full()
            .gap_1()
            .child(self.render_filters(cx))
            .when(self.mine().is_some(), |column| {
                column.child(small(
                    "“Mine” narrows the pull requests loaded so far.",
                    Color::Muted,
                ))
            })
            .when_some(status, |column, status| column.child(status))
            .child(
                v_flex()
                    .id("github-pr-rows")
                    .flex_1()
                    .overflow_y_scroll()
                    .children(rows)
                    .when(self.has_more(), |column| {
                        column.child(
                            Button::new(
                                "github-prs-load-more",
                                if loading_more {
                                    "Loading…"
                                } else {
                                    "Load more"
                                },
                            )
                            .disabled(loading_more)
                            .on_click(cx.listener(|this, _, _window, cx| this.load_more(cx))),
                        )
                    }),
            )
    }
}
