//! Right-click menus for the tree's leaf rows.
//!
//! Deleting is the only entry that can lose work, so it is the only one that
//! asks twice -- and an unmerged branch says so, because "deleted" and
//! "deleted along with three commits nobody else has" are different events.

use git::repository::Branch;
use gpui::{Entity, Window};
use project::git_store::RepositoryId;
use ui::{ContextMenu, prelude::*};

use crate::branch_panel::remote::RemoteOp;

use crate::branch_panel::panel::BranchPanel;
use crate::branch_service;

impl BranchPanel {
    pub(crate) fn branch_context_menu(
        &self,
        id: RepositoryId,
        branch: Branch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ContextMenu> {
        let panel = cx.entity();
        ContextMenu::build(window, cx, move |menu, _window, _cx| {
            let checkout = (panel.clone(), branch.clone());
            let copy = branch.name().to_string();
            let rename = (panel.clone(), branch.clone());
            let delete = (panel.clone(), branch.clone());
            let worktree = (panel.clone(), branch.clone());

            menu.entry("Checkout", None, move |window, cx| {
                let (panel, branch) = checkout.clone();
                panel.update(cx, |panel, cx| {
                    panel.checkout_branch(id, branch, window, cx);
                });
            })
            .entry("Open in New Worktree", None, move |window, cx| {
                let (panel, branch) = worktree.clone();
                panel.update(cx, |panel, cx| {
                    panel.open_worktree_for(branch, window, cx);
                });
            })
            .separator()
            .entry("Rename Branch", None, move |window, cx| {
                let (panel, branch) = rename.clone();
                panel.update(cx, |panel, cx| {
                    panel.begin_rename_branch(id, branch, window, cx);
                });
            })
            .entry("Copy Branch Name", None, move |_window, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
            })
            .separator()
            .entry("Delete Branch", None, move |window, cx| {
                let (panel, branch) = delete.clone();
                panel.update(cx, |panel, cx| {
                    panel.confirm_delete_branch(id, branch, window, cx);
                });
            })
        })
    }

    fn confirm_delete_branch(
        &mut self,
        id: RepositoryId,
        branch: Branch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if branch.is_head {
            return;
        }
        let Some(repo) = self.repository(id, cx) else {
            return;
        };

        // "Gone" means the upstream this branch tracked no longer exists, so
        // its commits may live nowhere else. Say so rather than letting the
        // generic wording imply this is routine.
        let unmerged = branch.upstream.is_none();
        let detail = if unmerged {
            format!(
                "{} has no upstream. Any commits only on this branch will be unreachable.",
                branch.name()
            )
        } else {
            format!("Delete {}?", branch.name())
        };

        let is_remote = branch.is_remote();
        let name = branch.name().to_string();
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            "Delete branch",
            Some(&detail),
            &["Delete", "Cancel"],
            cx,
        );

        let task = cx.spawn(async move |_, cx| {
            if answer.await.ok() != Some(0) {
                return Ok(());
            }
            branch_service::delete_branch(repo, is_remote, name, cx).await
        });

        let label = if is_remote {
            "git branch -dr"
        } else {
            "git branch -d"
        };
        self.report_failure(task, label, window, cx);
    }

    pub(crate) fn open_worktree_for(
        &mut self,
        branch: Branch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = cx;
        window.dispatch_action(
            Box::new(zed_actions::CreateWorktree {
                worktree_name: None,
                branch_target: zed_actions::NewWorktreeBranchTarget::ExistingBranch {
                    name: branch.name().to_string(),
                },
            }),
            cx,
        );
    }
}

impl BranchPanel {
    /// Anchors a menu at the pointer. Held in panel state so it survives the
    /// frame that opened it, and is subscribed so dismissing it clears the
    /// handle rather than leaving a hidden entity alive.
    pub(crate) fn open_context_menu(
        &mut self,
        menu: Entity<ContextMenu>,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let subscription =
            cx.subscribe_in(&menu, window, |panel, _, _: &gpui::DismissEvent, _, cx| {
                panel.context_menu.take();
                cx.notify();
            });
        self.context_menu = Some((menu, position, subscription));
        cx.notify();
    }

    pub(crate) fn stash_context_menu(
        &self,
        id: RepositoryId,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ContextMenu> {
        let panel = cx.entity();
        ContextMenu::build(window, cx, move |menu, _window, _cx| {
            let apply = panel.clone();
            let pop = panel.clone();
            let drop = panel.clone();

            menu.entry("Apply Stash", None, move |window, cx| {
                apply.update(cx, |panel, cx| {
                    panel.stash_op(id, index, StashOp::Apply, window, cx)
                });
            })
            .entry("Pop Stash", None, move |window, cx| {
                pop.update(cx, |panel, cx| {
                    panel.stash_op(id, index, StashOp::Pop, window, cx)
                });
            })
            .separator()
            .entry("Drop Stash", None, move |window, cx| {
                drop.update(cx, |panel, cx| {
                    panel.stash_op(id, index, StashOp::Drop, window, cx)
                });
            })
        })
    }

    fn stash_op(
        &mut self,
        id: RepositoryId,
        index: usize,
        op: StashOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.repository(id, cx) else {
            return;
        };

        // Dropping a stash is unrecoverable through the UI, so it asks; apply
        // and pop leave the work reachable either way.
        let confirm = matches!(op, StashOp::Drop).then(|| {
            window.prompt(
                gpui::PromptLevel::Warning,
                "Drop stash",
                Some(&format!("stash@{{{index}}} will be discarded.")),
                &["Drop", "Cancel"],
                cx,
            )
        });

        let task = cx.spawn(async move |_, cx| {
            if let Some(confirm) = confirm
                && confirm.await.ok() != Some(0)
            {
                return Ok(());
            }
            match op {
                StashOp::Apply => {
                    repo.update(cx, |repo, cx| repo.stash_apply(Some(index), cx))
                        .await
                }
                StashOp::Pop => {
                    repo.update(cx, |repo, cx| repo.stash_pop(Some(index), cx))
                        .await
                }
                StashOp::Drop => {
                    repo.update(cx, |repo, cx| repo.stash_drop(Some(index), cx))
                        .await?
                }
            }
        });

        self.report_failure(task, op.git_label(), window, cx);
    }

    /// Opens an existing linked worktree in this workspace.
    pub(crate) fn switch_to_worktree(
        &mut self,
        worktree: &git::repository::Worktree,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = cx;
        window.dispatch_action(
            Box::new(zed_actions::SwitchWorktree {
                path: worktree.path.clone(),
                display_name: crate::branch_panel::tree::worktree_label(worktree),
            }),
            cx,
        );
    }
}

#[derive(Clone, Copy)]
pub(crate) enum StashOp {
    Apply,
    Pop,
    Drop,
}

impl StashOp {
    fn git_label(self) -> &'static str {
        match self {
            StashOp::Apply => "git stash apply",
            StashOp::Pop => "git stash pop",
            StashOp::Drop => "git stash drop",
        }
    }
}

impl BranchPanel {
    /// The repository row's own menu. Fetch lives here as well as in the
    /// header, because the header acts on one repository and a monorepo has
    /// several.
    pub(crate) fn repo_context_menu(
        &self,
        id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ContextMenu> {
        let panel = cx.entity();
        ContextMenu::build(window, cx, move |menu, _window, _cx| {
            let fetch = panel.clone();
            let new_branch = panel.clone();

            menu.entry("Fetch", None, move |window, cx| {
                fetch.update(cx, |panel, cx| {
                    panel.run_remote_op(id, RemoteOp::Fetch, window, cx);
                });
            })
            .separator()
            .entry("New Branch\u{2026}", None, move |window, cx| {
                new_branch.update(cx, |panel, cx| panel.begin_new_branch(id, window, cx));
            })
        })
    }
}
