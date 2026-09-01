use crate::Sidebar;
use gpui::{App, Context, Entity, Task, Window};
use project::ProjectGroupKey;
use remote::RemoteConnectionOptions;
use workspace::{MultiWorkspace, OpenMode, Workspace};

impl Sidebar {
    pub(crate) fn workspace_for_group(
        &self,
        key: &ProjectGroupKey,
        cx: &App,
    ) -> Option<Entity<Workspace>> {
        let mw = self.multi_workspace.upgrade()?;
        let mw = mw.read(cx);
        let active = mw.workspace().clone();
        let active_key = active.read(cx).project_group_key(cx);
        if active_key == *key {
            Some(active)
        } else {
            mw.workspace_for_paths(key.path_list(), key.host().as_ref(), cx)
        }
    }

    pub(crate) fn active_workspace(&self, cx: &App) -> Option<Entity<Workspace>> {
        self.multi_workspace
            .upgrade()
            .map(|mw| mw.read(cx).workspace().clone())
    }

    fn activate_workspace(
        &self,
        workspace: &Entity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(multi_workspace) = self.multi_workspace.upgrade() {
            multi_workspace.update(cx, |mw, cx| {
                mw.activate(workspace.clone(), None, window, cx);
            });
        }
    }

    /// Opens a new workspace for a group that has no open workspaces.
    pub(crate) fn open_workspace_for_group(
        &mut self,
        project_group_key: &ProjectGroupKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(multi_workspace) = self.multi_workspace.upgrade() else {
            return;
        };
        let path_list = project_group_key.path_list().clone();
        let host = project_group_key.host();
        let provisional_key = Some(project_group_key.clone());
        let active_workspace = multi_workspace.read(cx).workspace().clone();
        let modal_workspace = active_workspace.clone();

        let task = multi_workspace.update(cx, |this, cx| {
            this.find_or_create_workspace(
                path_list,
                host,
                provisional_key,
                move |options, window, cx| connect_remote(active_workspace, options, window, cx),
                &[],
                None,
                OpenMode::Activate,
                window,
                cx,
            )
        });

        cx.spawn_in(window, async move |_this, cx| {
            let result = task.await;
            remote_connection::dismiss_connection_modal(&modal_workspace, cx);
            result?;
            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
    }

    /// FR2: activates the project group's last-active workspace, or opens
    /// one if none is currently open. The sidebar's only way to switch
    /// projects -- no separate expand/collapse step.
    pub(crate) fn activate_or_open_workspace_for_group(
        &mut self,
        key: &ProjectGroupKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace = self
            .multi_workspace
            .upgrade()
            .and_then(|mw| mw.read(cx).last_active_workspace_for_group(key, cx))
            .or_else(|| self.workspace_for_group(key, cx));
        if let Some(workspace) = workspace {
            self.activate_workspace(&workspace, window, cx);
        } else {
            self.open_workspace_for_group(key, window, cx);
        }
        // `MultiWorkspace::activate` short-circuits with no event when the
        // target is already active (see its own early return), so nothing
        // else is guaranteed to repaint this clear -- notify explicitly
        // rather than relying on the usual `MultiWorkspaceEvent` ->
        // `update_entries` -> `cx.notify()` chain.
        self.selection = None;
        cx.notify();
    }

    /// Brings one worktree's workspace to the front.
    ///
    /// The row holds a `WeakEntity`, so a workspace closed between the last
    /// rebuild and this click simply does nothing rather than resurrecting
    /// anything -- the next rebuild drops the row.
    pub(crate) fn activate_worktree(
        &mut self,
        workspace: &gpui::WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = workspace.upgrade() else {
            return;
        };
        self.activate_workspace(&workspace, window, cx);
        self.selection = None;
        cx.notify();
    }

    /// Jumps to a running agent's tab, or brings a finished session back.
    ///
    /// Resuming goes through `agent_ui::resume_session`, the same call the
    /// history panel makes -- the rules about what may be resumed (the agent
    /// must support it, the directory must still exist) belong to the operation
    /// and must not be re-decided here.
    pub(crate) fn open_agent_row(
        &mut self,
        row: &crate::project_list::AgentRow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = row.workspace.upgrade() else {
            return;
        };
        self.activate_workspace(&workspace, window, cx);

        match &row.kind {
            crate::project_list::AgentRowKind::Running => {
                let Some(view) = row.view.as_ref().and_then(|view| view.upgrade()) else {
                    return;
                };
                workspace.update(cx, |workspace, cx| {
                    workspace.activate_item(&view, true, true, window, cx);
                });
            }
            crate::project_list::AgentRowKind::Past { session } => {
                let Some(store) = self.session_store.clone() else {
                    return;
                };
                let index = store.read(cx).index().clone();
                let Some(summary) = index.sessions().get(*session as usize) else {
                    return;
                };
                agent_ui::resume_session(
                    &workspace,
                    summary,
                    agent_sessions::Fork::Continue,
                    window,
                    cx,
                );
            }
        }
        self.selection = None;
        cx.notify();
    }

    /// Opens or closes a project's worktree rows.
    ///
    /// Records the *closed* projects: they are open by default, so the set only
    /// ever holds the exceptions, and a project nobody has touched behaves the
    /// same on every machine.
    pub(crate) fn toggle_project_expansion(
        &mut self,
        key: &ProjectGroupKey,
        cx: &mut Context<Self>,
    ) {
        let marker = crate::project_list::collapsed_marker(key);
        if !self.collapsed_projects.remove(&marker) {
            self.collapsed_projects.insert(marker);
        }
        self.update_entries(cx);
        self.serialize(cx);
    }
}

/// Shows a remote-connection modal and establishes an SSH/WSL/Docker
/// connection. Passed to `MultiWorkspace::find_or_create_workspace` as the
/// `connect_remote` argument.
fn connect_remote(
    modal_workspace: Entity<Workspace>,
    connection_options: RemoteConnectionOptions,
    window: &mut Window,
    cx: &mut Context<MultiWorkspace>,
) -> Task<anyhow::Result<Option<Entity<remote::RemoteClient>>>> {
    remote_connection::connect_with_modal(&modal_workspace, connection_options, window, cx)
}
