//! Where a run's task goes: the workspace's terminal panel. Kept behind a
//! trait so the panel's run logic can be tested without a shell or a PTY.

use std::rc::Rc;

use anyhow::anyhow;
use gpui::{App, Entity, Task, WeakEntity, Window};
use task::{ShellKind, SpawnInTerminal};
use terminal::{TaskStatus, Terminal};
use terminal_view::TerminalView;
use terminal_view::terminal_panel::TerminalPanel;
use workspace::{Pane, Workspace};

pub(crate) trait RunTerminals {
    /// The shell the task's command line is written for.
    fn shell_kind(&self) -> ShellKind;

    /// Called outside any entity update: the terminal panel reads the
    /// workspace, and revealing it may activate a dock this panel sits in.
    fn spawn(
        &self,
        task: SpawnInTerminal,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Rc<dyn RunTerminal>>>;
}

pub(crate) trait RunTerminal {
    /// False once the task has ended or its terminal is gone.
    fn is_running(&self, cx: &App) -> bool;

    /// Brings the run's tab to the front. Called outside any entity update.
    fn reveal(&self, window: &mut Window, cx: &mut App);
}

pub(crate) struct WorkspaceTerminals {
    pub workspace: WeakEntity<Workspace>,
}

impl RunTerminals for WorkspaceTerminals {
    /// `TerminalPanel::spawn_task` wraps a local task in `Shell::System`.
    fn shell_kind(&self) -> ShellKind {
        ShellKind::system()
    }

    fn spawn(
        &self,
        task: SpawnInTerminal,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Rc<dyn RunTerminal>>> {
        let Some(workspace) = self.workspace.upgrade() else {
            return Task::ready(Err(anyhow!("The workspace is closed.")));
        };
        let Some(terminal_panel) = workspace.read(cx).panel::<TerminalPanel>(cx) else {
            return Task::ready(Err(anyhow!("The terminal panel is not available.")));
        };
        let spawned = terminal_panel.update(cx, |terminal_panel, cx| {
            terminal_panel.spawn_task(&task, window, cx)
        });
        let workspace = self.workspace.clone();
        cx.foreground_executor().spawn(async move {
            let terminal = spawned.await?;
            Ok(Rc::new(TaskTerminal {
                workspace,
                terminal,
            }) as Rc<dyn RunTerminal>)
        })
    }
}

struct TaskTerminal {
    workspace: WeakEntity<Workspace>,
    terminal: WeakEntity<Terminal>,
}

impl RunTerminal for TaskTerminal {
    fn is_running(&self, cx: &App) -> bool {
        self.terminal.upgrade().is_some_and(|terminal| {
            terminal
                .read(cx)
                .task()
                .is_some_and(|task| task.status == TaskStatus::Running)
        })
    }

    fn reveal(&self, window: &mut Window, cx: &mut App) {
        let (Some(workspace), Some(terminal)) = (self.workspace.upgrade(), self.terminal.upgrade())
        else {
            return;
        };
        let item_index = |pane: &Entity<Pane>, cx: &App| {
            pane.read(cx).items().position(|item| {
                item.downcast::<TerminalView>()
                    .is_some_and(|view| view.read(cx).terminal() == &terminal)
            })
        };
        let find = |panes: Vec<Entity<Pane>>, cx: &App| {
            panes
                .into_iter()
                .find_map(|pane| Some((item_index(&pane, cx)?, pane)))
        };
        let terminal_panel = workspace.read(cx).panel::<TerminalPanel>(cx);
        let dock_panes = terminal_panel
            .as_ref()
            .map(|panel| panel.read(cx).panes().into_iter().cloned().collect())
            .unwrap_or_default();
        if let Some((index, pane)) = find(dock_panes, cx) {
            pane.update(cx, |pane, cx| {
                pane.activate_item(index, true, true, window, cx)
            });
            workspace.update(cx, |workspace, cx| {
                workspace.focus_panel::<TerminalPanel>(window, cx);
            });
            return;
        }
        // The user may have dragged the tab into the center.
        let center_panes = workspace.read(cx).panes().to_vec();
        if let Some((index, pane)) = find(center_panes, cx) {
            pane.update(cx, |pane, cx| {
                pane.activate_item(index, true, true, window, cx)
            });
        }
    }
}
