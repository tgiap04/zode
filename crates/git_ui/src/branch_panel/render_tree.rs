//! Drawing one row of the tree.
//!
//! Every decision about *what* is in the list was already made by
//! `tree::build_rows`. This module only turns a `TreeRow` into pixels, which is
//! why it has no tests: there is no logic left in it to get wrong.

use gpui::{AnyElement, ClickEvent, FontWeight};
use ui::{Tooltip, prelude::*};

use crate::branch_panel::panel::BranchPanel;
use crate::branch_panel::tree::{SectionKind, TreeRow, worktree_label};

mod branch_card;
mod leaf;

/// Indentation per level. Deliberately small: the rows are cards with their own
/// horizontal margin, and a generous indent on top of that would leave a 280px
/// panel with no room for a branch name.
const INDENT: Pixels = px(8.);

impl BranchPanel {
    pub(crate) fn render_row(
        &self,
        ix: usize,
        row: &TreeRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let indent = INDENT * row.depth() as f32;

        match row {
            TreeRow::Repo {
                id,
                name,
                current_branch,
                expanded,
            } => self.repo_row(
                ix,
                *id,
                name.clone(),
                current_branch.clone(),
                *expanded,
                row,
                cx,
            ),

            TreeRow::Section {
                id,
                kind,
                count,
                expanded,
            } => {
                let id = *id;
                let is_local = matches!(kind, SectionKind::Local);
                self.section_row(ix, indent, *expanded, kind.label().into(), *count, row, cx)
                    // Only Local gets a create button: a branch is always
                    // created locally, whatever it will later track.
                    .when(is_local, |this| {
                        this.child(
                            IconButton::new(("new-branch", ix), IconName::Plus)
                                .icon_size(IconSize::XSmall)
                                .tooltip(move |_, cx| Tooltip::simple("New Branch", cx))
                                .on_click(cx.listener(move |panel, _, window, cx| {
                                    panel.begin_new_branch(id, window, cx);
                                })),
                        )
                    })
                    .into_any_element()
            }

            TreeRow::RemoteGroup {
                remote,
                count,
                expanded,
                ..
            } => self
                .section_row(ix, indent, *expanded, remote.clone(), *count, row, cx)
                .into_any_element(),

            TreeRow::Branch { id, branch, .. } => self.branch_card(ix, indent, *id, branch, cx),

            TreeRow::Worktree { worktree, .. } => {
                let label = worktree_label(worktree);
                let path: SharedString = worktree.path.display().to_string().into();
                let switch_to = worktree.clone();
                Self::leaf_row(
                    ("worktree", ix),
                    indent,
                    IconName::GitWorktree,
                    label.into(),
                )
                .tooltip(move |_, cx| Tooltip::simple(path.clone(), cx))
                .on_click(cx.listener(move |panel, _: &ClickEvent, window, cx| {
                    panel.switch_to_worktree(&switch_to, window, cx);
                }))
                .into_any_element()
            }

            TreeRow::Stash { id, entry, .. } => {
                let message: SharedString = entry.message.clone().into();
                let (id, index) = (*id, entry.index);
                Self::leaf_row(
                    ("stash", ix),
                    indent,
                    IconName::Archive,
                    format!("{}: {}", entry.index, message).into(),
                )
                .on_click(cx.listener(move |panel, event: &ClickEvent, window, cx| {
                    let menu = panel.stash_context_menu(id, index, window, cx);
                    panel.open_context_menu(menu, event.position(), window, cx);
                }))
                .into_any_element()
            }

            TreeRow::Tag { id, tag } => {
                let (id, checkout) = (*id, tag.clone());
                let tooltip = tag.message.clone().unwrap_or_else(|| tag.sha.clone());
                Self::leaf_row(("tag", ix), indent, IconName::Bookmark, tag.name.clone())
                    .tooltip(move |_, cx| Tooltip::simple(tooltip.clone(), cx))
                    .on_click(cx.listener(move |panel, _: &ClickEvent, window, cx| {
                        panel.checkout_tag(id, checkout.clone(), window, cx);
                    }))
                    .into_any_element()
            }

            TreeRow::Empty { label } => h_flex()
                .w_full()
                .pl(indent + px(28.))
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
    /// belong to that repository. The whole row toggles, so the chevron is an
    /// affordance rather than the only target.
    #[allow(clippy::too_many_arguments)]
    fn repo_row(
        &self,
        ix: usize,
        id: project::git_store::RepositoryId,
        name: SharedString,
        current_branch: Option<SharedString>,
        expanded: bool,
        row: &TreeRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = row.toggle_key();
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
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Label::new(name).weight(FontWeight::SEMIBOLD).truncate()),
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
                        let menu = panel.repo_context_menu(id, window, cx);
                        panel.open_context_menu(menu, event.position(), window, cx);
                    })),
            )
            .child(
                IconButton::new(("repo-new-branch", ix), IconName::Plus)
                    .icon_size(IconSize::XSmall)
                    .tooltip(move |_, cx| Tooltip::simple("New Branch", cx))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.begin_new_branch(id, window, cx);
                    })),
            )
            .tooltip(move |_, cx| Tooltip::simple(tooltip.clone(), cx))
            .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                if let Some(key) = key.clone() {
                    panel.toggle_row(key, cx);
                }
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

    /// A quiet divider between groups of cards: small, muted, and carrying the
    /// count so a collapsed section still says how much is inside it.
    #[allow(clippy::too_many_arguments)]
    fn section_row(
        &self,
        ix: usize,
        indent: Pixels,
        expanded: bool,
        label: SharedString,
        count: usize,
        row: &TreeRow,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let key = row.toggle_key();

        h_flex()
            .id(("section", ix))
            .w_full()
            .h(px(24.))
            .pl(indent + px(8.))
            .pr_2()
            .gap_1()
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
                div().flex_1().min_w_0().child(
                    Label::new(label)
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                        .weight(FontWeight::SEMIBOLD)
                        .truncate(),
                ),
            )
            .child(
                Label::new(count.to_string())
                    .size(LabelSize::XSmall)
                    .color(Color::Disabled),
            )
            .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                if let Some(key) = key.clone() {
                    panel.toggle_row(key, cx);
                }
            }))
    }
}
