use gpui::{IntoElement, Render, Window};
use ui::{Checkbox, Modal, ModalFooter, ModalHeader, Section, ToggleState, prelude::*};

use crate::issues::{render_target, target_line};
use crate::pull_requests::merge_modal::{MergeDialog, MergePhase};
use crate::untrusted_text::reveal_hidden_characters;

impl MergeDialog {
    fn summary_line(&self) -> String {
        let pull_request = &self.details.pull_request;
        format!(
            "#{} {} — {} → {}",
            pull_request.number,
            reveal_hidden_characters(&pull_request.title),
            reveal_hidden_characters(&pull_request.head_ref_name),
            reveal_hidden_characters(&pull_request.base_ref_name),
        )
    }

    fn render_methods(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .flex_wrap()
            .gap_1()
            .children(self.options.methods.iter().copied().map(|method| {
                Button::new(("github-merge-method", method as usize), method.label())
                    .label_size(LabelSize::Small)
                    .toggle_state(self.method == method)
                    .on_click(cx.listener(move |this, _, _window, cx| this.set_method(method, cx)))
            }))
    }

    fn render_branch_choice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.options.github_deletes_branch {
            return Some(
                Label::new("GitHub deletes the head branch after the merge (repository setting).")
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .into_any_element(),
            );
        }
        self.options.offers_branch_deletion.then(|| {
            Checkbox::new(
                "github-merge-delete-branch",
                ToggleState::from(self.delete_branch),
            )
            .label(format!(
                "Delete {} after merging",
                reveal_hidden_characters(&self.details.pull_request.head_ref_name)
            ))
            .on_click(cx.listener(|this, state: &ToggleState, _window, cx| {
                this.set_delete_branch(*state == ToggleState::Selected, cx)
            }))
            .into_any_element()
        })
    }

    fn phase_line(&self) -> Option<(String, Color)> {
        match &self.phase {
            MergePhase::Confirming => None,
            MergePhase::Merging => Some(("Merging…".to_string(), Color::Muted)),
            MergePhase::Merged { branch_note } => Some((
                match branch_note {
                    Some(note) => format!("Merged. {note}"),
                    None => "Merged.".to_string(),
                },
                Color::Success,
            )),
            MergePhase::HeadChanged => Some((
                "New commits were pushed since this opened. Nothing was merged; the pull request was reloaded."
                    .to_string(),
                Color::Warning,
            )),
            MergePhase::NoLongerMergeable(reason) => Some((
                format!("Nothing was merged: {reason} The pull request was reloaded."),
                Color::Warning,
            )),
            MergePhase::Failed(error) => Some((format!("Not merged: {error}"), Color::Error)),
        }
    }
}

impl Render for MergeDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let confirming = self.phase == MergePhase::Confirming;
        let target = target_line(&self.login, &self.key.repository, self.visibility);
        let branch_choice = self.render_branch_choice(cx);
        let phase_line = self.phase_line();
        v_flex()
            .key_context("GithubMergePullRequest")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.cancel(cx)))
            .elevation_3(cx)
            .w(rems(34.))
            .child(
                Modal::new("github-merge-pull-request", None)
                    .header(ModalHeader::new().headline("Merge pull request"))
                    .section(
                        Section::new().child(
                            v_flex()
                                .gap_2()
                                .child(render_target(target, self.visibility_error.clone()))
                                .when(self.visibility_error.is_some(), |column| {
                                    column.child(
                                        h_flex()
                                            .debug_selector(|| {
                                                "github-merge-visibility-retry".to_string()
                                            })
                                            .child(
                                                Button::new(
                                                    "github-merge-visibility-retry",
                                                    "Retry",
                                                )
                                                .label_size(LabelSize::Small)
                                                .on_click(cx.listener(|this, _, _window, cx| {
                                                    this.retry_visibility(cx)
                                                })),
                                            ),
                                    )
                                })
                                .child(Label::new(self.summary_line()))
                                .child(
                                    Label::new(format!(
                                        "Into {}",
                                        reveal_hidden_characters(
                                            &self.details.pull_request.base_ref_name
                                        )
                                    ))
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                                )
                                .when(confirming, |column| {
                                    column
                                        .child(self.render_methods(cx))
                                        .children(branch_choice)
                                })
                                .when_some(phase_line, |column, (text, color)| {
                                    column.child(Label::new(text).color(color))
                                }),
                        ),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_1()
                                .child(
                                    Button::new(
                                        "github-merge-cancel",
                                        if confirming { "Cancel" } else { "Close" },
                                    )
                                    .disabled(self.phase == MergePhase::Merging)
                                    .on_click(cx.listener(|this, _, _window, cx| this.cancel(cx))),
                                )
                                .when(confirming, |row| {
                                    row.child(
                                        Button::new("github-merge-confirm", self.method.label())
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
