use github::{GithubUser, Issue, IssueState};
use gpui::{AnyElement, Window};
use ui::{Callout, ContextMenu, DropdownMenu, DropdownStyle, IconPosition, prelude::*};

use crate::issues::IssueDetailView;
use crate::issues::assignee_picker_render::render_person_chip;
use crate::issues::issue_actions::{CLOSE_REASONS, close_reason_label};
use crate::issues::issue_editor_properties_render::{property_row, removable};
use crate::issues::issue_editor_render::{composer_field, editor_field};
use crate::issues::issue_list_render::{filter_trigger, label_menu_entry};
use crate::issues::issue_row_render::render_label_chip;
use crate::untrusted_text::reveal_hidden_characters;

pub(crate) fn callout(severity: Severity, text: String) -> AnyElement {
    Callout::new()
        .severity(severity)
        .description(text)
        .into_any_element()
}

impl IssueDetailView {
    pub(crate) fn render_write_actions(
        &self,
        issue: &Issue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let this = cx.entity().downgrade();
        let state_control = match issue.state {
            IssueState::Open => {
                let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
                    for reason in CLOSE_REASONS {
                        let this = this.clone();
                        menu = menu.entry(close_reason_label(reason), None, move |window, cx| {
                            this.update(cx, |this, cx| this.close(reason, window, cx))
                                .ok();
                        });
                    }
                    menu
                });
                DropdownMenu::new("github-issue-close", "Close…", menu)
                    .disabled(self.is_busy())
                    .into_any_element()
            }
            IssueState::Closed => Button::new("github-issue-reopen", "Reopen")
                .label_size(LabelSize::Small)
                .disabled(self.is_busy())
                .on_click(cx.listener(|this, _, window, cx| this.reopen(window, cx)))
                .into_any_element(),
        };
        h_flex()
            .gap_1()
            .child(
                Button::new("github-issue-edit", "Edit")
                    .label_size(LabelSize::Small)
                    .disabled(self.edit.is_some())
                    .on_click(cx.listener(|this, _, window, cx| this.start_editing(window, cx))),
            )
            .child(state_control)
            .into_any_element()
    }

    pub(crate) fn render_edit_form(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let edit = self.edit.as_ref()?;
        Some(
            v_flex()
                .key_context("GithubIssueEdit")
                .on_action(
                    cx.listener(|this, _: &menu::SecondaryConfirm, _window, cx| this.save_edit(cx)),
                )
                .gap_1()
                .child(editor_field("Title", &edit.title, cx))
                .child(composer_field("Description", edit.description.clone()))
                .when_some(edit.error.clone(), |column, error| {
                    column.child(callout(Severity::Warning, error))
                })
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new(
                                "github-issue-save",
                                if edit.saving { "Saving…" } else { "Save" },
                            )
                            .style(ButtonStyle::Filled)
                            .disabled(edit.saving)
                            .on_click(cx.listener(|this, _, _window, cx| this.save_edit(cx))),
                        )
                        .child(
                            Button::new("github-issue-cancel-edit", "Cancel")
                                .disabled(edit.saving)
                                .on_click(
                                    cx.listener(|this, _, _window, cx| this.cancel_editing(cx)),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    pub(crate) fn render_triage(
        &mut self,
        issue: &Issue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let picker = self.assignee_picker(window, cx);
        picker.update(cx, |picker, _| picker.set_selected(issue.assignees.clone()));
        let this = cx.entity().downgrade();
        let busy = self.is_busy();
        let present = issue
            .labels
            .iter()
            .map(|label| label.name.clone())
            .collect::<Vec<_>>();
        let addable = self
            .labels
            .iter()
            .filter(|label| !present.contains(&label.name))
            .cloned()
            .collect::<Vec<_>>();
        let label_menu = ContextMenu::build(window, cx, {
            let this = this.clone();
            move |mut menu, _, _| {
                if addable.is_empty() {
                    return menu.label("No other labels");
                }
                for label in &addable {
                    let this = this.clone();
                    let name = label.name.clone();
                    menu = label_menu_entry(menu, label, false, move |window, cx| {
                        let name = name.clone();
                        this.update(cx, |this, cx| this.add_label(name, window, cx))
                            .ok();
                    });
                }
                menu
            }
        });
        let milestones = self.milestones.clone();
        let current = issue.milestone.as_ref().map(|milestone| milestone.number);
        let milestone_menu = ContextMenu::build(window, cx, move |menu, _, _| {
            let mut menu = menu.toggleable_entry(
                "No milestone",
                current.is_none(),
                IconPosition::Start,
                None,
                {
                    let this = this.clone();
                    move |window, cx| {
                        this.update(cx, |this, cx| this.set_milestone(None, window, cx))
                            .ok();
                    }
                },
            );
            for milestone in milestones {
                let this = this.clone();
                let number = milestone.number;
                let shown = reveal_hidden_characters(&milestone.title).into_owned();
                menu = menu.toggleable_entry(
                    shown,
                    current == Some(number),
                    IconPosition::Start,
                    None,
                    move |window, cx| {
                        this.update(cx, |this, cx| this.set_milestone(Some(number), window, cx))
                            .ok();
                    },
                );
            }
            menu
        });
        let milestone_text = issue
            .milestone
            .as_ref()
            .map_or("None".to_string(), |milestone| {
                reveal_hidden_characters(&milestone.title).into_owned()
            });
        let labels = issue.labels.iter().enumerate().map(|(index, label)| {
            let name = label.name.clone();
            removable(
                render_label_chip(label, cx),
                ("github-issue-remove-label", index),
                format!("the label {}", reveal_hidden_characters(&label.name)),
                busy,
                cx.listener(move |this, _, window, cx| this.remove_label(name.clone(), window, cx)),
            )
        });
        let assignees = issue.assignees.iter().enumerate().map(|(index, login)| {
            let user = GithubUser {
                login: login.clone(),
                name: None,
                avatar_url: issue.assignee_avatars.get(login).cloned(),
            };
            let removed = login.clone();
            removable(
                render_person_chip(&user, cx),
                ("github-issue-unassign", index),
                format!("@{}", reveal_hidden_characters(login)),
                busy,
                cx.listener(move |this, _, window, cx| {
                    this.remove_assignee(removed.clone(), window, cx)
                }),
            )
        });
        let me = self
            .login
            .clone()
            .filter(|me| !issue.assignees.iter().any(|login| login == me.as_ref()));
        let label_row = h_flex().gap_1().flex_wrap().children(labels).child(
            DropdownMenu::new_with_element(
                "github-issue-add-label",
                filter_trigger(IconName::Plus, None, "Add label".to_string(), false),
                label_menu,
            )
            .style(DropdownStyle::Ghost)
            .trigger_size(ButtonSize::Compact)
            .disabled(busy),
        );
        let assignee_column = v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .children(assignees)
                    .when_some(me, |row, me| {
                        row.child(
                            Button::new("github-issue-assign-me", "Assign me")
                                .label_size(LabelSize::Small)
                                .style(ButtonStyle::Subtle)
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.add_assignees(vec![me.to_string()], None, window, cx)
                                })),
                        )
                    }),
            )
            .child(div().max_w(rems(24.)).child(picker));
        let milestone = DropdownMenu::new("github-issue-milestone", milestone_text, milestone_menu)
            .style(DropdownStyle::Ghost)
            .trigger_size(ButtonSize::Compact)
            .disabled(busy);
        v_flex()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors().border_variant)
            .child(property_row(IconName::Bookmark, "Labels", label_row, cx))
            .child(property_row(
                IconName::Person,
                "Assignees",
                assignee_column,
                cx,
            ))
            .child(property_row(IconName::Pin, "Milestone", milestone, cx))
            .into_any_element()
    }

    pub(crate) fn render_comment_box(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let composer = self.comment_composer(window, cx);
        v_flex()
            .key_context("GithubIssueComment")
            // macOS: ⌘↩ is unbound in a growing editor and reaches here as
            // SecondaryConfirm. Linux and Windows: Ctrl-↩ is the editor's own
            // newline, so it is caught on the way down while Ctrl is held.
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
                        "github-issue-send-comment",
                        if self.sending_comment {
                            "Sending…"
                        } else {
                            "Comment"
                        },
                    )
                    .style(ButtonStyle::Filled)
                    .disabled(self.sending_comment)
                    .on_click(cx.listener(|this, _, window, cx| this.send_comment(window, cx))),
                ),
            )
            .into_any_element()
    }
}
