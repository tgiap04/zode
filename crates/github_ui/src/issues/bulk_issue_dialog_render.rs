use gpui::{IntoElement, Render, Window};
use ui::{Modal, ModalFooter, ModalHeader, ProgressBar, Section, prelude::*};

use crate::issues::bulk_issue_actions::{BulkIssueDialog, BulkItem, BulkItemStatus, BulkPhase};
use crate::issues::issue_actions::{BulkIssueAction, close_reason_label};
use crate::issues::issue_editor_render::render_target;
use crate::untrusted_text::reveal_hidden_characters;

fn status_text(status: &BulkItemStatus) -> (String, Color) {
    match status {
        BulkItemStatus::Waiting => ("waiting".to_string(), Color::Muted),
        BulkItemStatus::Running => ("sending…".to_string(), Color::Accent),
        BulkItemStatus::Done => ("done".to_string(), Color::Success),
        BulkItemStatus::NotApplied => (
            "not applied: GitHub ignored it (no permission?)".to_string(),
            Color::Warning,
        ),
        BulkItemStatus::Failed(error) => (format!("failed: {error}"), Color::Error),
        BulkItemStatus::Cancelled => ("cancelled, not sent".to_string(), Color::Muted),
        BulkItemStatus::SignedOut => ("not sent: signed out".to_string(), Color::Muted),
        BulkItemStatus::Interrupted => (
            "stopped while sending; reload to see whether it was applied".to_string(),
            Color::Warning,
        ),
    }
}

fn render_item(item: &BulkItem) -> impl IntoElement {
    let (status, color) = status_text(&item.status);
    h_flex()
        .w_full()
        .gap_2()
        .child(
            Label::new(format!("#{}", item.number))
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
        .child(
            Label::new(reveal_hidden_characters(&item.title).into_owned())
                .size(LabelSize::Small)
                .truncate(),
        )
        .child(div().flex_1())
        .child(Label::new(status).size(LabelSize::Small).color(color))
}

impl BulkIssueDialog {
    fn headline(&self) -> String {
        let count = self.items.len();
        match &self.action {
            BulkIssueAction::Close(reason) => {
                format!("{} — {count} issue(s)", close_reason_label(*reason))
            }
            BulkIssueAction::AddLabel(name) => format!(
                "Add label “{}” to {count} issue(s)",
                reveal_hidden_characters(name)
            ),
        }
    }
}

impl Render for BulkIssueDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (settled, total) = self.progress();
        let phase = self.phase;
        let (cancel_text, confirm) = match phase {
            BulkPhase::Confirming => ("Cancel", Some(format!("Apply to {total} issue(s)"))),
            BulkPhase::Running => ("Stop", None),
            BulkPhase::Finished => ("Close", None),
        };
        v_flex()
            .key_context("GithubBulkIssues")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.cancel(cx)))
            .elevation_3(cx)
            .w(rems(34.))
            .child(
                Modal::new("github-bulk-issues", None)
                    .header(ModalHeader::new().headline(self.headline()))
                    .section(
                        Section::new().child(
                            v_flex()
                                .gap_2()
                                .child(render_target(
                                    self.target_line(),
                                    self.visibility_error.as_ref().map(ToString::to_string),
                                ))
                                .when(self.visibility_error.is_some(), |column| {
                                    column.child(
                                        h_flex().child(
                                            Button::new("github-bulk-retry-visibility", "Retry")
                                                .label_size(LabelSize::Small)
                                                .on_click(cx.listener(|this, _, _window, cx| {
                                                    this.retry_visibility(cx)
                                                })),
                                        ),
                                    )
                                })
                                .child(
                                    v_flex()
                                        .id("github-bulk-issue-items")
                                        .max_h(rems(16.))
                                        .overflow_y_scroll()
                                        .gap_0p5()
                                        .children(self.items.iter().map(render_item)),
                                )
                                .when(phase != BulkPhase::Confirming, |column| {
                                    column
                                        .child(ProgressBar::new(
                                            "github-bulk-progress",
                                            settled as f32,
                                            total.max(1) as f32,
                                            cx,
                                        ))
                                        .child(
                                            Label::new(format!("{settled} of {total}"))
                                                .size(LabelSize::Small)
                                                .color(Color::Muted),
                                        )
                                })
                                .when(phase == BulkPhase::Finished, |column| {
                                    column.child(Label::new(self.report()))
                                }),
                        ),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_1()
                                .child(
                                    Button::new("github-bulk-cancel", cancel_text).on_click(
                                        cx.listener(|this, _, _window, cx| this.cancel(cx)),
                                    ),
                                )
                                .when_some(confirm, |row, text| {
                                    row.child(
                                        Button::new("github-bulk-confirm", text)
                                            .style(ButtonStyle::Filled)
                                            .disabled(self.visibility.is_none())
                                            .on_click(
                                                cx.listener(|this, _, _window, cx| {
                                                    this.confirm(cx)
                                                }),
                                            ),
                                    )
                                }),
                        ),
                    ),
            )
    }
}
