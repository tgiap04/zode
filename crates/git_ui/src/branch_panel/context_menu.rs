//! The repository row's menu, and the pieces every row shares.

use gpui::{DismissEvent, Entity, Window};
use project::git_store::RepositoryId;
use ui::{ContextMenu, prelude::*};

use crate::branch_panel::remote::RemoteOp;

use crate::branch_panel::panel::BranchPanel;

impl BranchPanel {
    /// Opens an existing checkout in this workspace.
    pub(crate) fn switch_to_worktree(
        &mut self,
        worktree: &git::repository::Worktree,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.switch_to_worktree_with_agent(worktree, None, window, cx);
    }

    /// The same switch, optionally starting an agent once it lands.
    ///
    /// Starting an agent in the checkout already open takes the short way: a
    /// switch to where you already are still captures and restores the whole
    /// workspace and flashes a "switching" label, all to arrive back at the
    /// same directory.
    pub(crate) fn switch_to_worktree_with_agent(
        &mut self,
        worktree: &git::repository::Worktree,
        agent: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(agent) = agent.clone()
            && self.is_current_checkout(worktree, cx)
        {
            self.workspace
                .update(cx, |workspace, cx| {
                    agent_ui::AgentView::open_tracked(
                        workspace,
                        &agent,
                        Default::default(),
                        window,
                        cx,
                    );
                })
                .ok();
            return;
        }

        window.dispatch_action(
            Box::new(zed_actions::SwitchWorktree {
                path: worktree.path.clone(),
                display_name: crate::branch_panel::tree::worktree_label(worktree),
                agent,
            }),
            cx,
        );
    }

    /// The repository row's own menu.
    ///
    /// Fetch, pull and push live here rather than in the panel header: they act
    /// on one repository, and a monorepo has several. A header button would be
    /// making a choice the reader never got to see.
    pub(crate) fn repo_context_menu(
        &self,
        id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ContextMenu> {
        let panel = cx.entity();
        ContextMenu::build(window, cx, move |menu, _window, _cx| {
            let mut menu = menu;
            for (label, op) in [
                ("Fetch", RemoteOp::Fetch),
                ("Pull", RemoteOp::Pull),
                ("Push", RemoteOp::Push),
            ] {
                let panel = panel.clone();
                menu = menu.entry(label, None, move |window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.run_remote_op(id, op, window, cx);
                    });
                });
            }

            let create = panel.clone();
            menu.separator()
                .entry("Create Worktree\u{2026}", None, move |window, cx| {
                    create.update(cx, |panel, cx| {
                        panel.open_create_worktree_modal(id, window, cx)
                    });
                })
        })
    }

    /// Puts a menu on screen at `position` and takes it down when it dismisses.
    ///
    /// The subscription rides in the same tuple as the menu, so dropping one
    /// drops the other -- a subscription outliving its menu would fire into a
    /// handle that is gone.
    pub(crate) fn open_context_menu(
        &mut self,
        menu: Entity<ContextMenu>,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let subscription = cx.subscribe_in(
            &menu,
            window,
            |panel: &mut Self, _, _: &DismissEvent, _, cx| {
                panel.context_menu.take();
                cx.notify();
            },
        );
        cx.focus_view(&menu, window);
        self.context_menu = Some((menu, position, subscription));
        cx.notify();
    }

    /// The repository behind a row, if it is still in the store.
    pub(crate) fn repository(
        &self,
        id: RepositoryId,
        cx: &App,
    ) -> Option<Entity<project::git_store::Repository>> {
        self.git_store(cx)?
            .read(cx)
            .repositories()
            .get(&id)
            .cloned()
    }

    /// Runs a git task and raises a toast carrying git's own words if it fails.
    ///
    /// The panel itself needs no success handling: the repository emits the
    /// change and the list rebuilds from it.
    pub(crate) fn report_failure(
        &self,
        task: gpui::Task<anyhow::Result<()>>,
        action: impl Into<SharedString>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = action.into();
        let workspace = self.workspace.clone();
        cx.spawn(async move |_, cx| {
            if let Err(error) = task.await {
                let Some(workspace) = workspace.upgrade() else {
                    return;
                };
                cx.update(|cx| crate::git_panel::show_error_toast(workspace, action, error, cx));
            }
        })
        .detach();
    }
}
