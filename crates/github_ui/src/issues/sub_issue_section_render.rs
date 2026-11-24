use github::{Issue, IssueState, ParentIssue};
use gpui::{AnyElement, Window};
use ui::{Tooltip, prelude::*};
use workspace::Workspace;

use crate::github_view::DetailKey;
use crate::issues::epic_tree::{issue_repository, progress_label};
use crate::issues::issue_detail_write_render::callout;
use crate::issues::issue_editor::single_line_editor;
use crate::issues::issue_editor_render::editor_field;
use crate::issues::sub_issue_section::{MoveDirection, PendingRemoval, PickerPurpose};
use crate::issues::{IssueDetailView, open_issue_detail};
use crate::untrusted_text::reveal_hidden_characters;

fn state_icon(state: IssueState) -> Icon {
    match state {
        IssueState::Open => Icon::new(IconName::Circle).color(Color::Success),
        IssueState::Closed => Icon::new(IconName::Check).color(Color::Muted),
    }
    .size(IconSize::Small)
}

fn small_button(id: impl Into<ElementId>, text: &'static str) -> Button {
    Button::new(id, text).label_size(LabelSize::Small)
}

impl IssueDetailView {
    /// Opens a linked issue's own tab, which may be in another repository.
    fn open_linked(&self, key: DetailKey, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = Workspace::for_window(window, cx) else {
            return;
        };
        let backend = self.backend.clone();
        let session = self.session.clone();
        window.defer(cx, move |window, cx| {
            workspace.update(cx, |workspace, cx| {
                open_issue_detail(workspace, backend, session, key, window, cx);
            });
        });
    }

    /// Sub-issues, parent, the picker and the confirmation. Absent for an
    /// issue with no links that this account may not change.
    pub(crate) fn render_epic(
        &mut self,
        issue: &Issue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let editable = self.may_edit_epic();
        let linked = !self.epic.sub_issues.is_empty() || self.epic.parent.is_some();
        if !editable && !linked {
            return None;
        }
        let picker = self.render_picker(window, cx);
        let parent = self.render_parent(editable, cx);
        let sub_issues = self
            .epic
            .sub_issues
            .clone()
            .into_iter()
            .enumerate()
            .map(|(index, sub_issue)| self.render_sub_issue(index, sub_issue, editable, cx))
            .collect::<Vec<_>>();
        let progress = progress_label(issue.sub_issues_summary);
        let load_errors = self
            .epic
            .sub_issues_error
            .iter()
            .chain(&self.epic.parent_error)
            .map(|error| callout(Severity::Warning, error.to_string()))
            .collect::<Vec<_>>();
        Some(
            v_flex()
                .gap_1()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().colors().border_variant)
                .child(parent)
                .child(
                    h_flex()
                        .gap_2()
                        .child(Label::new("Sub-issues").size(LabelSize::Small))
                        .when_some(progress, |row, progress| {
                            row.child(
                                Label::new(progress)
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                        })
                        .child(div().flex_1())
                        .when(editable, |row| {
                            row.child(
                                small_button("github-sub-issue-add", "Add existing")
                                    .disabled(self.epic.writing)
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.open_picker(PickerPurpose::AddSubIssue, cx)
                                    })),
                            )
                            .child(
                                small_button("github-sub-issue-create", "Create sub-issue")
                                    .disabled(self.epic.writing)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.create_sub_issue(window, cx)
                                    })),
                            )
                        }),
                )
                .children(sub_issues)
                .children(self.render_pending_removal(cx))
                .children(picker)
                .when_some(self.epic.error.clone(), |column, error| {
                    column.child(callout(Severity::Error, error))
                })
                .children(load_errors)
                .into_any_element(),
        )
    }

    fn render_parent(&self, editable: bool, cx: &mut Context<Self>) -> AnyElement {
        let writing = self.epic.writing;
        let row = h_flex().gap_1().child(
            Label::new("Parent")
                .size(LabelSize::Small)
                .color(Color::Muted),
        );
        let row = match self.epic.parent.clone() {
            Some(parent) => row
                .child(self.render_parent_link(&parent, cx))
                .when(editable, |row| {
                    row.child(div().flex_1())
                        .child(
                            small_button("github-parent-change", "Change")
                                .disabled(writing)
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.open_picker(PickerPurpose::SetParent, cx)
                                })),
                        )
                        .child(
                            small_button("github-parent-clear", "Clear")
                                .disabled(writing)
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.request_clear_parent(cx)
                                })),
                        )
                }),
            None => row
                .child(
                    Label::new("None")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .when(editable, |row| {
                    row.child(div().flex_1()).child(
                        small_button("github-parent-set", "Set parent")
                            .disabled(writing)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.open_picker(PickerPurpose::SetParent, cx)
                            })),
                    )
                }),
        };
        row.into_any_element()
    }

    fn render_parent_link(&self, parent: &ParentIssue, cx: &mut Context<Self>) -> AnyElement {
        let key = DetailKey {
            repository: parent.repository.clone(),
            number: parent.number,
        };
        let elsewhere =
            (parent.repository != self.key.repository).then(|| format!(" ({})", parent.repository));
        h_flex()
            .id("github-parent-link")
            .gap_1()
            .cursor_pointer()
            .on_click(
                cx.listener(move |this, _, window, cx| this.open_linked(key.clone(), window, cx)),
            )
            .child(state_icon(parent.state))
            .child(
                Label::new(format!(
                    "#{} {}{}",
                    parent.number,
                    reveal_hidden_characters(&parent.title),
                    elsewhere.unwrap_or_default()
                ))
                .size(LabelSize::Small)
                .truncate(),
            )
            .into_any_element()
    }

    fn render_sub_issue(
        &self,
        index: usize,
        sub_issue: Issue,
        editable: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let number = sub_issue.number;
        let node_id = sub_issue.node_id.clone();
        let key = DetailKey {
            repository: issue_repository(&sub_issue).unwrap_or_else(|| self.key.repository.clone()),
            number,
        };
        let last = index + 1 >= self.epic.sub_issues.len();
        let writing = self.epic.writing;
        h_flex()
            .id(("github-sub-issue", index))
            .gap_1()
            .child(
                h_flex()
                    .id(("github-sub-issue-link", index))
                    .flex_1()
                    .gap_1()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_linked(key.clone(), window, cx)
                    }))
                    .child(state_icon(sub_issue.state))
                    .child(
                        Label::new(format!("#{number}"))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new(reveal_hidden_characters(&sub_issue.title).into_owned())
                            .size(LabelSize::Small)
                            .truncate(),
                    )
                    .when_some(
                        progress_label(sub_issue.sub_issues_summary),
                        |row, progress| {
                            row.child(
                                Label::new(progress)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                        },
                    ),
            )
            .when(editable, |row| {
                row.child(
                    IconButton::new(("github-sub-issue-up", index), IconName::ArrowUp)
                        .icon_size(IconSize::Small)
                        .tooltip(Tooltip::text("Move up"))
                        .disabled(writing || index == 0)
                        .on_click(cx.listener({
                            let node_id = node_id.clone();
                            move |this, _, _window, cx| {
                                this.move_sub_issue(&node_id, MoveDirection::Up, cx)
                            }
                        })),
                )
                .child(
                    IconButton::new(("github-sub-issue-down", index), IconName::ArrowDown)
                        .icon_size(IconSize::Small)
                        .tooltip(Tooltip::text("Move down"))
                        .disabled(writing || last)
                        .on_click(cx.listener({
                            let node_id = node_id.clone();
                            move |this, _, _window, cx| {
                                this.move_sub_issue(&node_id, MoveDirection::Down, cx)
                            }
                        })),
                )
                .child(
                    IconButton::new(("github-sub-issue-remove", index), IconName::Close)
                        .icon_size(IconSize::Small)
                        .tooltip(Tooltip::text("Remove from sub-issues"))
                        .disabled(writing)
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            this.request_remove_sub_issue(&node_id, cx)
                        })),
                )
            })
            .into_any_element()
    }

    fn render_pending_removal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let question = match self.epic.pending_removal.as_ref()? {
            PendingRemoval::SubIssue(sub_issue) => format!(
                "Remove #{} “{}” from this issue's sub-issues? The issue itself is kept.",
                sub_issue.number,
                reveal_hidden_characters(&sub_issue.title)
            ),
            PendingRemoval::Parent(parent) => format!(
                "Take this issue out of #{} “{}”? Both issues are kept.",
                parent.number,
                reveal_hidden_characters(&parent.title)
            ),
        };
        Some(
            v_flex()
                .gap_1()
                .child(callout(Severity::Warning, question))
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            small_button("github-epic-confirm-remove", "Remove")
                                .disabled(self.epic.writing)
                                .on_click(
                                    cx.listener(|this, _, _window, cx| this.confirm_removal(cx)),
                                ),
                        )
                        .child(
                            small_button("github-epic-cancel-remove", "Cancel").on_click(
                                cx.listener(|this, _, _window, cx| this.cancel_removal(cx)),
                            ),
                        ),
                )
                .into_any_element(),
        )
    }

    fn render_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.epic.picker.as_mut()?;
        let field = picker
            .field
            .get_or_insert_with(|| {
                single_line_editor("#number or words, then Enter", "", window, cx)
            })
            .clone();
        let label = match picker.purpose {
            PickerPurpose::AddSubIssue => "Add an existing issue as a sub-issue",
            PickerPurpose::SetParent => "Choose the parent issue",
        };
        let results = picker
            .results
            .iter()
            .enumerate()
            .map(|(index, issue)| {
                let picked = issue.clone();
                h_flex()
                    .id(("github-picker-result", index))
                    .gap_1()
                    .px_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(|row| row.bg(cx.theme().colors().element_hover))
                    .on_click(
                        cx.listener(move |this, _, _window, cx| this.pick(picked.clone(), cx)),
                    )
                    .child(state_icon(issue.state))
                    .child(
                        Label::new(format!(
                            "#{} {}",
                            issue.number,
                            reveal_hidden_characters(&issue.title)
                        ))
                        .size(LabelSize::Small)
                        .truncate(),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        let status = if picker.searching {
            Some(
                Label::new("Searching…")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
        } else {
            picker.error.clone().map(|error| {
                Label::new(error)
                    .size(LabelSize::Small)
                    .color(Color::Warning)
            })
        };
        Some(
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            div()
                                .flex_1()
                                .key_context("GithubIssuePicker")
                                .on_action(cx.listener({
                                    let field = field.clone();
                                    move |this, _: &menu::Confirm, _window, cx| {
                                        let text = field.read(cx).text(cx);
                                        this.search_picker(text, cx)
                                    }
                                }))
                                .child(editor_field(label, &field, cx)),
                        )
                        .child(
                            IconButton::new("github-picker-close", IconName::Close)
                                .icon_size(IconSize::Small)
                                .tooltip(Tooltip::text("Close"))
                                .on_click(
                                    cx.listener(|this, _, _window, cx| this.close_picker(cx)),
                                ),
                        ),
                )
                .children(status)
                .children(results)
                .into_any_element(),
        )
    }
}
