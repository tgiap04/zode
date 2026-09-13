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
        let pinned = self.pinned(cx).contains(&path);
        // The repository's own checkout is not something git will remove, and
        // offering it would be offering to fail.
        let can_delete = !worktree.is_main;
        let label = worktree_label(worktree);
        let worktree = worktree.clone();
        // Read here rather than inside the builder: the builder closure gets no
        // `App`, and the entry has to draw the state it will toggle.
        let bypass_on = agent_ui::PermissionBypassStore::global(cx)
            .read(cx)
            .is_enabled(&path);

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

            // Directly under the "New {agent}" entries because it changes what
            // they do, and nowhere else: this is the only menu whose subject is
            // a checkout. Under an agent it would appear five times and mean
            // five things depending on where the workspace happens to point.
            let bypass = panel.clone();
            let path_for_bypass = path.clone();
            let menu = menu.item(
                ContextMenuEntry::new("Run Agents Without Permission Prompts")
                    .toggleable(IconPosition::Start, bypass_on)
                    .handler(move |window, cx| {
                        bypass.update(cx, |panel, cx| {
                            panel.toggle_agent_permission_bypass(
                                id,
                                path_for_bypass.clone(),
                                window,
                                cx,
                            );
                        });
                    }),
            );

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
        let path_string = path.to_string_lossy().to_string();
        self.checkout_state.update(cx, |state, cx| {
            let mut pinned = state.pinned().to_vec();
            match pinned.iter().position(|pinned| pinned == &path_string) {
                Some(index) => {
                    pinned.remove(index);
                }
                None => pinned.push(path_string),
            }
            state.set_pinned(pinned, cx);
        });
        self.mark_stale(cx);
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

        let order = order
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect();
        self.checkout_state
            .update(cx, |state, cx| state.set_order(order, cx));
        self.mark_stale(cx);
        cx.notify();
    }

    /// Asks before removing, and names what will go.
    ///
    /// `force` is deliberately false: git refuses a worktree with uncommitted
    /// changes, and that refusal is the last guard between a stray click and
    /// somebody's afternoon. The error it returns is shown rather than
    /// swallowed and retried with force.
    /// Turns the per-checkout permission bypass on or off.
    ///
    /// Asks before switching it **on** and not before switching it off. Putting
    /// friction on the way back to asking-for-permission would be friction in
    /// the wrong direction.
    pub(crate) fn toggle_agent_permission_bypass(
        &mut self,
        id: RepositoryId,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The repository's own anchor, never the checkout it is open at -- the
        // record is pruned by repository, and `work_directory_abs_path` is a
        // different directory in every checkout. See `RepoData::anchor`.
        let Some(anchor) = self
            .repos
            .iter()
            .find(|repo| repo.id == id)
            .map(|repo| repo.anchor.clone())
        else {
            return;
        };
        let store = agent_ui::PermissionBypassStore::global(cx);

        if store.read(cx).is_enabled(&path) {
            store.update(cx, |store, cx| store.set(&path, &anchor, false, cx));
            self.mark_stale(cx);
            cx.notify();
            return;
        }

        // Three clauses, and the middle one is the one readers get wrong: the
        // scope is the checkout, but the agent's reach is the whole machine.
        let detail = format!(
            "{}\n\nAgents started in this checkout will not ask before they act.\n\nThey still run as you, with your credentials and your network access -- this limits where the setting applies, not what an agent can reach.\n\nOther checkouts are unaffected.",
            path.display()
        );
        let prompt = window.prompt(
            gpui::PromptLevel::Warning,
            "Run agents here without permission prompts?",
            Some(&detail),
            // Cancel second, so a stray Return does not switch off a safety
            // control.
            &["Enable", "Cancel"],
            cx,
        );

        cx.spawn_in(window, async move |panel, cx| {
            if prompt.await.ok() != Some(0) {
                return;
            }
            panel
                .update(cx, |panel, cx| {
                    agent_ui::PermissionBypassStore::global(cx)
                        .update(cx, |store, cx| store.set(&path, &anchor, true, cx));
                    panel.mark_stale(cx);
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

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
                    let path_string = path.to_string_lossy().to_string();
                    panel.checkout_state.update(cx, |state, cx| {
                        let mut pinned = state.pinned().to_vec();
                        pinned.retain(|pinned| pinned != &path_string);
                        state.set_pinned(pinned, cx);

                        let mut order = state.order().to_vec();
                        order.retain(|kept| kept != &path_string);
                        state.set_order(order, cx);
                    });

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
