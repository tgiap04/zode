//! One checkout, drawn as a card.
//!
//! The card is the panel's whole object now: which checkout, where it is, and
//! what has been running in it. Its border encloses the agents too -- that
//! border is what says they belong to this checkout and not the next one.

use std::sync::Arc;

use git::repository::Worktree as GitWorktree;
use gpui::{AnyElement, ClickEvent};
use project::git_store::RepositoryId;
use ui::{Chip, Indicator, Tooltip, prelude::*};

use crate::branch_panel::panel::BranchPanel;
use crate::branch_panel::tree::{AgentEntry, TreeRow, worktree_label};

/// The checkout being dragged, by path.
///
/// A path and not an index: the list rebuilds while a drag is in flight -- a
/// git event is enough -- and an index would then name a different card.
#[derive(Clone)]
pub(crate) struct DraggedCheckout(pub(crate) std::path::PathBuf);

/// What follows the cursor during a drag.
struct DragPreview(SharedString);

impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(cx.theme().colors().elevated_surface_background)
            .border_1()
            .border_color(cx.theme().colors().border_focused)
            .child(Label::new(self.0.clone()).size(LabelSize::Small))
    }
}

impl BranchPanel {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn worktree_card(
        &self,
        ix: usize,
        id: RepositoryId,
        worktree: &GitWorktree,
        agents: &Arc<[AgentEntry]>,
        expanded: bool,
        row: &TreeRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label: SharedString = worktree_label(worktree).into();
        let path: SharedString = worktree.path.display().to_string().into();
        let is_current = self.is_current_checkout(worktree, cx);
        let is_pinned = self.pinned.contains(&worktree.path);
        let toggle_key = row.toggle_key();
        let switch_to = worktree.clone();
        let menu_for = worktree.clone();
        let drag_path = worktree.path.clone();
        let drop_path = worktree.path.clone();
        let drag_label = label.clone();

        let colors = cx.theme().colors();
        let background = if is_current {
            colors.element_selected
        } else {
            colors.elevated_surface_background
        };
        let border = if is_current {
            colors.border_focused
        } else {
            colors.border_variant
        };
        let hover = colors.element_hover;
        let tooltip = path.clone();

        // Wrapped rather than given margins directly: the card has to be
        // `w_full` to fill the panel instead of shrinking to its text, and a
        // `w_full` element with margins overflows its parent.
        div()
            .w_full()
            .px_2()
            .py_1()
            .child(
                v_flex()
                    .id(("worktree", ix))
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .gap_0p5()
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .bg(background)
                    .hover(|style| style.bg(hover))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1p5()
                            .child(Indicator::dot().color(if is_current {
                                Color::Accent
                            } else {
                                Color::Muted
                            }))
                            .child(div().flex_1().min_w_0().child(
                                Label::new(label).size(LabelSize::Small).truncate().color(
                                    if is_current {
                                        Color::Default
                                    } else {
                                        Color::Muted
                                    },
                                ),
                            ))
                            // "main" rather than "current": which checkout the
                            // window is in is already said by the dot and the
                            // background, while which one is the repository's
                            // own is a fact about the repository.
                            .when(is_pinned, |this| {
                                this.child(
                                    Icon::new(IconName::Pin)
                                        .size(IconSize::XSmall)
                                        .color(Color::Accent),
                                )
                            })
                            .when(worktree.is_main, |this| {
                                this.child(
                                    Chip::new("main")
                                        .label_size(LabelSize::XSmall)
                                        .label_color(Color::Muted),
                                )
                            }),
                    )
                    .child(
                        Label::new(path)
                            .size(LabelSize::XSmall)
                            .color(Color::Disabled)
                            .truncate(),
                    )
                    .children(self.render_agents(ix, agents, expanded, toggle_key, cx))
                    .tooltip(move |_, cx| Tooltip::simple(tooltip.clone(), cx))
                    .on_click(cx.listener(move |panel, _: &ClickEvent, window, cx| {
                        panel.switch_to_worktree(&switch_to, window, cx);
                    }))
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(move |panel, event: &gpui::MouseDownEvent, window, cx| {
                            let menu = panel.checkout_context_menu(id, &menu_for, window, cx);
                            panel.open_context_menu(menu, event.position, window, cx);
                        }),
                    )
                    .on_drag(DraggedCheckout(drag_path), move |_, _, _, cx| {
                        cx.new(|_| DragPreview(drag_label.clone()))
                    })
                    .drag_over::<DraggedCheckout>(|style, _, _, cx| {
                        style.border_color(cx.theme().colors().drop_target_background)
                    })
                    .on_drop(
                        cx.listener(move |panel, dragged: &DraggedCheckout, _window, cx| {
                            panel.reorder_checkout(&dragged.0, &drop_path, cx);
                        }),
                    ),
            )
            .into_any_element()
    }

    /// Whether this window is looking at this checkout.
    ///
    /// By path prefix rather than equality: a workspace can be rooted at a
    /// subdirectory of the checkout, and it is still that checkout. The same
    /// question `agents_by_checkout` asks to decide where a running agent
    /// belongs, so the card's mark and the agent list cannot disagree about
    /// where "here" is.
    pub(crate) fn is_current_checkout(&self, worktree: &GitWorktree, cx: &App) -> bool {
        let Some(workspace) = self.workspace.upgrade() else {
            return false;
        };
        workspace
            .read(cx)
            .root_paths(cx)
            .iter()
            .any(|root| root.starts_with(&worktree.path))
    }
}
