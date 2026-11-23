use gpui::{AnyElement, Window, px};
use ui::{Checkbox, ContextMenu, DropdownMenu, ToggleState, prelude::*};

use crate::issues::issue_actions::{BulkIssueAction, CLOSE_REASONS, close_reason_label};
use crate::issues::issue_detail_write_render::callout;
use crate::issues::issue_list::IssueList;
use crate::untrusted_text::reveal_hidden_characters;

impl IssueList {
    pub(crate) fn render_row_checkbox(&self, number: u64, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        let state = if self.selected.contains(&number) {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        };
        Checkbox::new(("github-issue-select", number as usize), state)
            .on_click(move |_, _window, cx| {
                cx.stop_propagation();
                this.update(cx, |this, cx| this.toggle_selected(number, cx))
                    .ok();
            })
            .into_any_element()
    }

    pub(crate) fn render_new_issue_button(&self, cx: &mut Context<Self>) -> AnyElement {
        Button::new("github-issues-new", "New issue")
            .label_size(LabelSize::Small)
            .start_icon(Icon::new(IconName::Plus).size(IconSize::XSmall))
            .style(ButtonStyle::Filled)
            .on_click(cx.listener(|this, _, window, cx| this.new_issue(window, cx)))
            .into_any_element()
    }

    /// Shown while issues are ticked: what can be done to all of them, each
    /// through a confirmation that lists them.
    pub(crate) fn render_selection_bar(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.selected.is_empty() {
            return None;
        }
        let this = cx.entity().downgrade();
        let close_menu = ContextMenu::build(window, cx, {
            let this = this.clone();
            move |mut menu, _, _| {
                for reason in CLOSE_REASONS {
                    let this = this.clone();
                    menu = menu.entry(close_reason_label(reason), None, move |window, cx| {
                        this.update(cx, |this, cx| {
                            this.start_bulk(BulkIssueAction::Close(reason), window, cx)
                        })
                        .ok();
                    });
                }
                menu
            }
        });
        let labels = self.labels.clone();
        let label_menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
            for label in labels {
                let this = this.clone();
                let shown = reveal_hidden_characters(&label.name).into_owned();
                menu = menu.entry(shown, None, move |window, cx| {
                    let action = BulkIssueAction::AddLabel(label.name.clone());
                    this.update(cx, |this, cx| this.start_bulk(action, window, cx))
                        .ok();
                });
            }
            menu
        });
        let offers = self.offers_bulk_actions();
        Some(
            h_flex()
                .h(px(32.))
                .px_2()
                .gap_1()
                .bg(cx.theme().colors().element_selected)
                .border_b_1()
                .border_color(cx.theme().colors().border_variant)
                .child(
                    Label::new(format!("{} selected", self.selected.len())).size(LabelSize::Small),
                )
                .when(offers, |row| {
                    row.child(DropdownMenu::new(
                        "github-issues-bulk-close",
                        "Close…",
                        close_menu,
                    ))
                    .child(DropdownMenu::new(
                        "github-issues-bulk-label",
                        "Add label…",
                        label_menu,
                    ))
                })
                .when(!offers, |row| {
                    row.child(
                        Label::new(format!(
                            "@{} may not triage issues here",
                            reveal_hidden_characters(&self.context.login)
                        ))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                    )
                })
                .child(
                    Button::new("github-issues-clear-selection", "Clear")
                        .label_size(LabelSize::Small)
                        .on_click(cx.listener(|this, _, _window, cx| this.clear_selection(cx))),
                )
                .into_any_element(),
        )
    }

    pub(crate) fn render_write_messages(&self) -> Vec<AnyElement> {
        let mut messages = Vec::new();
        if let Some(error) = &self.write_error {
            messages.push(callout(Severity::Error, error.clone()));
        }
        if let Some(notice) = &self.notice {
            let severity = if notice.contains("did not apply") {
                Severity::Warning
            } else {
                Severity::Info
            };
            messages.push(callout(severity, notice.clone()));
        }
        messages
    }

    pub(crate) fn render_context_menu(&self) -> Option<AnyElement> {
        let (menu, position, _) = self.context_menu.as_ref()?;
        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(*position)
                    .anchor(gpui::Anchor::TopLeft)
                    .child(menu.clone()),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}
