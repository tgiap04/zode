//! Drawing one row of the panel.
//!
//! Every decision about *what* is in the list was already made by
//! `tree::build_rows`. This module only turns a `TreeRow` into pixels.

use gpui::{AnyElement, ClickEvent};
use ui::{Tooltip, prelude::*};

use crate::branch_panel::panel::BranchPanel;
use crate::branch_panel::tree::TreeRow;

mod agent;
mod worktree_card;

/// The mark a checkout carries once agents in it stop asking before they act.
///
/// `IconName::Warning` in `Color::Error` and not `LockOutlined`, which reads as
/// "locked" -- the opposite of what is true here. There is no shield-off glyph
/// in `crates/icons`, and this is not the change that adds one.
///
/// Lives here rather than in `worktree_card` only because that file was already
/// within fifteen lines of the 200-line guidance.
pub(super) fn bypass_mark(enabled: bool) -> Option<impl IntoElement> {
    enabled.then(|| {
        div()
            .id("agent-permission-bypass")
            .child(
                Icon::new(IconName::Warning)
                    .size(IconSize::XSmall)
                    .color(Color::Error),
            )
            .tooltip(Tooltip::text(
                "Agents run in this checkout without permission prompts",
            ))
    })
}

impl BranchPanel {
    pub(crate) fn render_row(
        &self,
        ix: usize,
        row: &TreeRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match row {
            TreeRow::Repo {
                id,
                name,
                current_branch,
                expanded,
            } => self.repo_row(ix, *id, name.clone(), current_branch.clone(), *expanded, cx),

            TreeRow::Worktree {
                id,
                worktree,
                agents,
                expanded,
            } => self.worktree_card(ix, *id, worktree, agents, *expanded, row, cx),

            TreeRow::Empty { label } => h_flex()
                .w_full()
                .px_3()
                .py_1()
                .child(
                    Label::new(label.clone())
                        .size(LabelSize::Small)
                        .color(Color::Disabled)
                        .italic(),
                )
                .into_any_element(),
        }
    }

    /// The project heading: an icon, the repository name, and the actions that
    /// belong to the repository rather than to one checkout.
    fn repo_row(
        &self,
        ix: usize,
        id: project::git_store::RepositoryId,
        name: SharedString,
        current_branch: Option<SharedString>,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tooltip = current_branch.unwrap_or_else(|| "detached HEAD".into());

        h_flex()
            .id(("repo", ix))
            .w_full()
            .h(px(34.))
            .px_2()
            .gap_1p5()
            .child(
                div()
                    .flex_none()
                    .size(px(20.))
                    .rounded_sm()
                    .bg(cx.theme().colors().element_background)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(IconName::FolderGit)
                            .size(IconSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .child(
                div().flex_1().min_w_0().child(
                    Label::new(name)
                        .weight(gpui::FontWeight::SEMIBOLD)
                        .truncate(),
                ),
            )
            .child(
                Icon::new(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(IconSize::XSmall)
                .color(Color::Muted),
            )
            .child(
                IconButton::new(("repo-menu", ix), IconName::Ellipsis)
                    .icon_size(IconSize::XSmall)
                    .on_click(cx.listener(move |panel, event: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        let menu = panel.repo_context_menu(id, window, cx);
                        panel.open_context_menu(menu, event.position(), window, cx);
                    })),
            )
            .when(!self.menu_is_open(), |this| {
                this.tooltip(move |_, cx| Tooltip::simple(tooltip.clone(), cx))
            })
            .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                panel.toggle_row(crate::branch_panel::tree::RowKey::Repo(id), cx);
            }))
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |panel, event: &gpui::MouseDownEvent, window, cx| {
                    let menu = panel.repo_context_menu(id, window, cx);
                    panel.open_context_menu(menu, event.position, window, cx);
                }),
            )
            .into_any_element()
    }
}
