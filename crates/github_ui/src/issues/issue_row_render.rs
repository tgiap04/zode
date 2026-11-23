//! One row of the issue list, and the small pieces other issue views reuse:
//! the state icon, the label chip and the assignee faces.

use std::sync::Arc;

use github::{Issue, IssueState, Label as IssueLabel, MAX_SUB_ISSUE_DEPTH, StateReason};
use gpui::{AnyElement, MouseButton, MouseDownEvent, Window, px};
use time::OffsetDateTime;
use ui::{Disclosure, Tooltip, prelude::*};

use crate::avatar::render_people;
use crate::issues::epic_tree::{TreeRow, progress_label};
use crate::issues::issue_filters::label_color;
use crate::issues::issue_list::IssueList;
use crate::issues::issue_list_render::updated_label;
use crate::issues::issue_type_picker::render_type_chip;
use crate::untrusted_text::reveal_hidden_characters;

pub(crate) const ROW_HEIGHT: f32 = 36.;
const INDENT: f32 = 16.;
const SHOWN_LABELS: usize = 3;

/// The icon, its colour and what it means. Each state has its own shape, so
/// colour is never the only difference.
pub(crate) fn state_icon(issue: &Issue) -> (IconName, Color, &'static str) {
    match (issue.state, issue.state_reason) {
        (IssueState::Open, _) => (IconName::TodoProgress, Color::Success, "Open"),
        (IssueState::Closed, Some(StateReason::NotPlanned)) => {
            (IconName::XCircle, Color::Muted, "Closed as not planned")
        }
        (IssueState::Closed, Some(StateReason::Duplicate)) => {
            (IconName::XCircle, Color::Muted, "Closed as duplicate")
        }
        (IssueState::Closed, _) => (IconName::TodoComplete, Color::Accent, "Closed as completed"),
    }
}

pub(crate) fn render_state_icon(id: impl Into<ElementId>, issue: &Issue) -> AnyElement {
    let (icon, color, meaning) = state_icon(issue);
    div()
        .id(id.into())
        .flex_none()
        .tooltip(Tooltip::text(meaning))
        .child(Icon::new(icon).size(IconSize::Small).color(color))
        .into_any_element()
}

/// A pill with the label's own colour as a dot, so the name is always drawn
/// in the theme's text colour whatever the label's colour is.
pub(crate) fn render_label_chip(label: &IssueLabel, cx: &App) -> AnyElement {
    let colors = cx.theme().colors();
    let dot = label_color(&label.color).unwrap_or(colors.text_muted);
    h_flex()
        .flex_none()
        .h(px(20.))
        .gap_1()
        .px_1p5()
        .rounded_full()
        .bg(colors.element_background)
        .border_1()
        .border_color(colors.border_variant)
        .child(div().flex_none().size(px(8.)).rounded_full().bg(dot))
        .child(
            Label::new(reveal_hidden_characters(&label.name).into_owned())
                .size(LabelSize::XSmall)
                .single_line(),
        )
        .into_any_element()
}

/// The issue's assignees as overlapping faces with their logins.
pub(crate) fn render_assignee_faces(
    id: impl Into<ElementId>,
    issue: &Issue,
    cx: &App,
) -> Option<AnyElement> {
    let people = issue
        .assignees
        .iter()
        .map(|login| {
            (
                reveal_hidden_characters(login).into_owned(),
                issue.assignee_avatars.get(login).cloned(),
            )
        })
        .collect::<Vec<_>>();
    render_people(id, &people, px(180.), cx)
}

/// What an opened row's sub-issues are doing while they are not shown, in
/// a line as tall as a row so the list can be drawn by range.
pub(crate) fn render_status_entry(depth: usize, text: &str) -> AnyElement {
    h_flex()
        .h(px(ROW_HEIGHT))
        .flex_none()
        .pl(px(INDENT * (depth as f32 + 3.)))
        .child(
            Label::new(text.to_string())
                .size(LabelSize::Small)
                .color(Color::Muted)
                .single_line(),
        )
        .into_any_element()
}

impl IssueList {
    /// The chevron that opens a parent row, or the same width of nothing so
    /// titles line up.
    fn render_disclosure(&self, index: usize, row: &TreeRow, cx: &mut Context<Self>) -> AnyElement {
        let opens = row.issue.sub_issues_summary.total > 0 && row.depth + 1 < MAX_SUB_ISSUE_DEPTH;
        if !opens {
            return div().flex_none().w(px(INDENT)).into_any_element();
        }
        let this = cx.entity().downgrade();
        let key = row.key.clone();
        Disclosure::new(
            ("github-issue-disclosure", index),
            self.sub_issues_of(&row.key).is_some(),
        )
        .on_toggle_expanded(
            Arc::new(move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                this.update(cx, |this, cx| this.toggle_sub_issues(key.clone(), cx))
                    .ok();
            }) as Arc<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>,
        )
        .into_any_element()
    }

    fn render_row_meta(
        &self,
        index: usize,
        issue: &Issue,
        now: OffsetDateTime,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let comments = issue.comment_count;
        h_flex()
            .flex_none()
            .gap_3()
            .justify_end()
            .children(render_assignee_faces(
                ("github-issue-assignees", index),
                issue,
                cx,
            ))
            .when(comments > 0, |meta| {
                meta.child(
                    h_flex()
                        .id(("github-issue-comments", index))
                        .gap_1()
                        .tooltip(Tooltip::text(if comments == 1 {
                            "1 comment".to_string()
                        } else {
                            format!("{comments} comments")
                        }))
                        .child(
                            Icon::new(IconName::Thread)
                                .size(IconSize::XSmall)
                                .color(Color::Muted),
                        )
                        .child(
                            Label::new(comments.to_string())
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        ),
                )
            })
            .child(
                div().w(px(84.)).flex().justify_end().child(
                    Label::new(updated_label(&issue.updated_at, now))
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .single_line(),
                ),
            )
            .into_any_element()
    }

    pub(crate) fn render_row(
        &self,
        index: usize,
        row: &TreeRow,
        now: OffsetDateTime,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let issue = &row.issue;
        let number = issue.number;
        // Sub-issues are shown under their parent but are not rows of this
        // list: they are not selected or given the row menu.
        let top_level = row.depth == 0;
        let ticked = top_level && self.selected.contains(&number);
        let key = row.key.clone();
        let colors = cx.theme().colors();
        let hidden_labels = issue.labels.len().saturating_sub(SHOWN_LABELS);
        h_flex()
            .id(("github-issue-row", index))
            .debug_selector(|| format!("github-issue-row-{index}"))
            .w_full()
            .h(px(ROW_HEIGHT))
            .flex_none()
            .gap_2()
            .px_2()
            .border_b_1()
            .border_color(colors.border_variant)
            .cursor_pointer()
            .when(ticked, |row| row.bg(colors.element_selected))
            .hover(|row| row.bg(colors.element_hover))
            .on_click(
                cx.listener(move |this, _, window, cx| {
                    this.open_issue_key(key.clone(), window, cx)
                }),
            )
            .when(top_level, |row| {
                row.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        this.deploy_row_menu(number, event.position, window, cx)
                    }),
                )
                .child(self.render_row_checkbox(number, cx))
            })
            .when(!top_level, |element| {
                element.child(div().flex_none().w(px(INDENT * (row.depth as f32 + 1.))))
            })
            .child(self.render_disclosure(index, row, cx))
            .child(render_state_icon(("github-issue-state", index), issue))
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .overflow_hidden()
                    .child(
                        div().min_w_0().flex_shrink().child(
                            Label::new(reveal_hidden_characters(&issue.title).into_owned())
                                .single_line()
                                .truncate(),
                        ),
                    )
                    .child(
                        div().flex_none().child(
                            Label::new(format!("#{number}"))
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        ),
                    )
                    .when_some(issue.issue_type.as_deref(), |line, name| {
                        line.child(
                            div()
                                .flex_none()
                                .child(render_type_chip(name, &self.issue_types)),
                        )
                    })
                    .when_some(
                        progress_label(issue.sub_issues_summary),
                        |line, progress| {
                            line.child(
                                div().flex_none().child(
                                    Label::new(progress)
                                        .size(LabelSize::Small)
                                        .color(Color::Muted),
                                ),
                            )
                        },
                    )
                    .children(
                        issue
                            .labels
                            .iter()
                            .take(SHOWN_LABELS)
                            .map(|label| render_label_chip(label, cx)),
                    )
                    .when(hidden_labels > 0, |line| {
                        line.child(
                            Label::new(format!("+{hidden_labels}"))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                    }),
            )
            .child(self.render_row_meta(index, issue, now, cx))
            .into_any_element()
    }
}
