//! The right-click menu on a checkout card, and what its entries do.
//!
//! Delete is the only entry that can lose work, so it is the only one that asks
//! twice -- and it says what it is about to remove, because "delete worktree"
//! and "delete a directory with two days of uncommitted work in it" look the
//! same from the menu.

use std::path::PathBuf;

use gpui::{ClipboardItem, Entity, Window};
use project::git_store::RepositoryId;
use ui::{ContextMenu, ContextMenuEntry, prelude::*};

use crate::branch_panel::panel::BranchPanel;
use crate::branch_panel::tree::worktree_label;

impl BranchPanel {
    pub(crate) fn checkout_context_menu(
        &self,
        id: RepositoryId,
        worktree: &git::repository::Worktree,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ContextMenu> {
        let panel = cx.entity();
        let path = worktree.path.clone();
        let pinned = self.pinned.contains(&path);
        // The repository's own checkout is not something git will remove, and
        // offering it would be offering to fail.
        let can_delete = !worktree.is_main;
        let label = worktree_label(worktree);
        let worktree = worktree.clone();

        ContextMenu::build(window, cx, move |menu, _window, _cx| {
            let pin = panel.clone();
            let path_for_pin = path.clone();
            let copy = panel.clone();
            let path_for_copy = path.clone();
            let remove = panel.clone();
            let path_for_remove = path.clone();
            let label = label.clone();

            // Starting an agent switches to the checkout first, because an
            // agent runs in the directory its workspace has open -- there is no
            // way to start one "over there" without going there. Listed one per
            // agent rather than behind a submenu: the whole point of the panel
            // is picking a worktree and putting something to work in it, and
            // that should be one click from the card.
            let mut menu = menu;
            for builtin in project::BUILTIN_AGENTS {
                let start = panel.clone();
                let worktree = worktree.clone();
                menu = menu.item(
                    ContextMenuEntry::new(format!("New {}", builtin.display_name))
                        .icon(agent_ui::agent_icon(builtin.id))
                        .icon_position(IconPosition::Start)
                        .icon_color(Color::Custom(agent_ui::agent_color(builtin.id)))
                        .handler(move |window, cx| {
                            start.update(cx, |panel, cx| {
                                panel.switch_to_worktree_with_agent(
                                    &worktree,
                                    Some(builtin.id.to_string()),
                                    window,
                                    cx,
                                );
                            });
                        }),
                );
            }

            let menu = menu
                .separator()
                .item(
                    ContextMenuEntry::new(if pinned { "Unpin" } else { "Pin to Top" })
                        .icon(IconName::Pin)
                        .icon_position(IconPosition::Start)
                        .handler(move |_, cx| {
                            pin.update(cx, |panel, cx| panel.toggle_pinned(&path_for_pin, cx));
                        }),
                )
                .item(
                    ContextMenuEntry::new("Copy Path")
                        .icon(IconName::Copy)
                        .icon_position(IconPosition::Start)
                        .handler(move |_, cx| {
                            copy.update(cx, |_, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    path_for_copy.display().to_string(),
                                ));
                            });
                        }),
                );

            menu.separator().item(
                ContextMenuEntry::new("Delete Worktree\u{2026}")
                    .icon(IconName::Trash)
                    .icon_position(IconPosition::Start)
                    .icon_color(Color::Error)
                    .disabled(!can_delete)
                    .handler(move |window, cx| {
                        remove.update(cx, |panel, cx| {
                            panel.confirm_delete_checkout(
                                id,
                                path_for_remove.clone(),
                                label.clone(),
                                window,
                                cx,
                            )
                        });
                    }),
            )
        })
    }

    pub(crate) fn toggle_pinned(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        if !self.pinned.remove(path) {
            self.pinned.insert(path.to_path_buf());
        }
        self.mark_stale(cx);
        self.serialize(cx);
        cx.notify();
    }

    /// Moves `dragged` to sit where `target` is.
    ///
    /// The whole visible order is written down, not just the pair that moved:
    /// a list that remembers two positions and derives the rest reshuffles
    /// itself the next time anything else changes.
    pub(crate) fn reorder_checkout(
        &mut self,
        dragged: &std::path::Path,
        target: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        if dragged == target {
            return;
        }
        let mut order: Vec<PathBuf> = self
            .repos
            .iter()
            .flat_map(|repo| repo.worktrees.iter())
            .map(|worktree| worktree.path.clone())
            .collect();

        let Some(from) = order.iter().position(|path| path == dragged) else {
            return;
        };
        let moved = order.remove(from);
        let Some(to) = order.iter().position(|path| path == target) else {
            return;
        };
        order.insert(to, moved);

        self.manual_order = order;
        self.mark_stale(cx);
        self.serialize(cx);
        cx.notify();
    }

    /// Asks before removing, and names what will go.
    ///
    /// `force` is deliberately false: git refuses a worktree with uncommitted
    /// changes, and that refusal is the last guard between a stray click and
    /// somebody's afternoon. The error it returns is shown rather than
    /// swallowed and retried with force.
    fn confirm_delete_checkout(
        &mut self,
        id: RepositoryId,
        path: PathBuf,
        label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.repository(id, cx) else {
            return;
        };
        let detail = format!(
            "{}\n\nThe worktree at {} will be removed. Its branch and its commits stay.",
            label,
            path.display()
        );
        let prompt = window.prompt(
            gpui::PromptLevel::Warning,
            "Delete this worktree?",
            Some(&detail),
            &["Delete", "Cancel"],
            cx,
        );

        cx.spawn_in(window, async move |panel, cx| {
            if prompt.await.ok() != Some(0) {
                return;
            }
            let receiver = repo.update(cx, |repo, _| repo.remove_worktree(path.clone(), false));
            // The channel closing means the repository went away mid-delete;
            // there is nothing to report and nothing to retry.
            let Ok(result) = receiver.await else { return };

            panel
                .update_in(cx, |panel, window, cx| {
                    // Whatever happened, the reader's opinions about a path
                    // that may be gone are not worth keeping.
                    panel.pinned.remove(&path);
                    panel.manual_order.retain(|kept| kept != &path);
                    panel.serialize(cx);

                    panel.report_failure(
                        gpui::Task::ready(result),
                        "git worktree remove",
                        window,
                        cx,
                    );
                    panel.mark_stale(cx);
                })
                .ok();
        })
        .detach();
    }
}
