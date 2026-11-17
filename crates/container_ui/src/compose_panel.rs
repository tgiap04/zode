//! Doing what a compose entry says: running a verb in a terminal tab, taking a
//! project down through the confirmation, opening its files, expanding its
//! services.
//!
//! Every verb is the backend's `compose_command`, never assembled here: which
//! words `compose` takes, and where the project's files and directory go, is
//! the engine's vocabulary. The view hands over a target and an operation and
//! runs what comes back in a terminal, so output streams.

use std::path::{Path, PathBuf};

use container::{ComposeOp, ContainerError, Op, Resource, ResourceKind, RunState};
use gpui::{AsyncWindowContext, Context, WeakEntity, Window};
use workspace::OpenOptions;

use crate::compose_actions::{
    ComposeCommand, ComposeContext, all_inside, running_member, service_labels, target_of,
};
use crate::confirm_modal::Replan;
use crate::container_panel::ContainerPanel;
use crate::terminal::TerminalIntent;

impl ContainerPanel {
    /// Whether the engine has compose at all, which is a different question from
    /// whether the plugin is installed (`compose_plugin`).
    pub(crate) fn compose_offered(&self) -> bool {
        self.backend()
            .is_some_and(|backend| backend.offers(Op::Compose))
    }

    /// The folders the project has open, where the editor can reach a file on a
    /// remote host.
    fn worktree_roots(&self, cx: &gpui::App) -> Vec<PathBuf> {
        let Some(workspace) = self.workspace.as_ref() else {
            return Vec::new();
        };
        workspace
            .read_with(cx, |workspace, cx| {
                workspace
                    .project()
                    .read(cx)
                    .visible_worktrees(cx)
                    .map(|worktree| worktree.read(cx).abs_path().to_path_buf())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Everything the entries of `row` depend on.
    ///
    /// `cx` is given only where a workspace may be read -- a menu opening, not a
    /// frame being drawn -- and then decides whether a remote file's reachability
    /// is worked out.
    pub(crate) fn compose_context(&self, row: &Resource, cx: Option<&gpui::App>) -> ComposeContext {
        let remote = self.host_name().is_some();
        let target = target_of(row, &self.compose_members);
        let project = service_labels(row)
            .map(|labels| labels.project.as_str())
            .unwrap_or(row.id.as_str());
        let has_directory = target
            .as_ref()
            .is_some_and(|target| target.working_dir.is_some());
        let files = target.map(|target| target.config_files).unwrap_or_default();
        let files_outside =
            remote && cx.is_some_and(|cx| !all_inside(&files, &self.worktree_roots(cx)));
        let member_running = match service_labels(row) {
            Some(labels) => running_member(&self.compose_members, labels).is_some(),
            None => self.compose_members.iter().any(|member| {
                member.state == RunState::Running
                    && member
                        .compose
                        .as_ref()
                        .is_some_and(|labels| labels.project == project)
            }),
        };
        ComposeContext {
            offered: self.compose_offered(),
            plugin: self.compose_plugin,
            remote,
            has_files: !files.is_empty(),
            has_directory,
            files_outside,
            expanded: self.compose_expanded.contains(project),
            state: row.state,
            member_running,
            shell: self.backend().is_some_and(|backend| {
                backend
                    .exec_command(ResourceKind::Container, "probe")
                    .is_some()
            }),
        }
    }

    pub(crate) fn run_compose_command(
        &mut self,
        command: ComposeCommand,
        row: Resource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The menus and buttons already draw a blocked entry as unavailable;
        // this is the same answer for a caller that did not look.
        if let Some(reason) = self.compose_context(&row, Some(cx)).blocker(command) {
            self.last_error = Some(ContainerError::Invalid { detail: reason });
            cx.notify();
            return;
        }
        match command {
            ComposeCommand::Run(op) | ComposeCommand::RunService(op) => {
                self.run_compose_op(op, &row, window, cx)
            }
            ComposeCommand::ServiceShell => {
                let Some(member) = service_labels(&row)
                    .and_then(|labels| running_member(&self.compose_members, labels))
                    .cloned()
                else {
                    return;
                };
                self.open_terminal_of(
                    ResourceKind::Container,
                    TerminalIntent::Shell,
                    member.id,
                    member.name,
                    window,
                    cx,
                );
            }
            ComposeCommand::Down => self.start_compose_down(row, window, cx),
            ComposeCommand::OpenFile => self.open_compose_files(&row, window, cx),
            ComposeCommand::RevealDirectory => {
                if let Some(directory) =
                    target_of(&row, &self.compose_members).and_then(|target| target.working_dir)
                {
                    cx.reveal_path(Path::new(&directory));
                }
            }
            ComposeCommand::ToggleServices { .. } => self.toggle_services(&row.id, cx),
        }
    }

    fn run_compose_op(
        &mut self,
        op: ComposeOp,
        row: &Resource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(backend) = self.backend().cloned() else {
            return;
        };
        let Some(target) = target_of(row, &self.compose_members) else {
            return;
        };
        let (services, name) = match service_labels(row) {
            Some(labels) => (
                vec![labels.service.clone()],
                format!("{}/{}", labels.project, labels.service),
            ),
            None => (Vec::new(), target.project.clone()),
        };
        match backend.compose_command(&target, op, &services) {
            Some((program, args)) => self.open_command_terminal(
                TerminalIntent::Compose(op),
                ResourceKind::ComposeProject,
                program,
                args,
                name,
                window,
                cx,
            ),
            // Said rather than dropped: a button that does nothing is the defect
            // this panel has shipped before.
            None => {
                self.last_error = Some(ContainerError::Invalid {
                    detail: format!(
                        "compose could not be run for {name}: check the project's name, \
                         files and directory"
                    ),
                });
                cx.notify();
            }
        }
    }

    pub(crate) fn toggle_services(&mut self, project: &str, cx: &mut Context<Self>) {
        if !self.compose_expanded.remove(project) {
            self.compose_expanded.insert(project.to_string());
        }
        cx.notify();
    }

    /// Finds what `compose down` would remove, *then* asks.
    ///
    /// The order is the one `start_prune` keeps and for the same reason: a dialog
    /// opened before the list was gathered could only say "this project", which
    /// is not something anybody can consent to.
    fn start_compose_down(&mut self, row: Resource, window: &mut Window, cx: &mut Context<Self>) {
        let Some(backend) = self.backend().cloned() else {
            return;
        };
        let Some(target) = target_of(&row, &self.compose_members) else {
            return;
        };
        if self.workspace.is_none() {
            // No workspace, no modal layer: refusing is right, the alternative
            // is taking a project down with no confirmation at all.
            log::warn!("no workspace to show a compose down confirmation in");
            return;
        }
        self.last_error = None;
        self.spawn_tracked(window, cx, async move |this, cx| {
            let planned = backend.compose_down_plan(&target, false).await;
            let applied = this.update_in(cx, |this, window, cx| match planned {
                Ok(Some(plan)) => {
                    let replan = Replan {
                        backend: backend.clone(),
                        target: target.clone(),
                    };
                    this.open_confirmation_replanning(plan, replan, window, cx);
                }
                Ok(None) => {
                    this.last_error = Some(ContainerError::Invalid {
                        detail: format!("{} has nothing to take down", target.project),
                    });
                    cx.notify();
                }
                Err(error) => {
                    this.last_error = Some(error);
                    cx.notify();
                }
            });
            if let Err(error) = applied {
                log::debug!("panel closed before the compose down dialog could open: {error}");
            }
        });
    }

    /// Opens each compose file in the workspace's editor.
    ///
    /// On this machine any path opens. On a remote host only what lies inside
    /// the project's folders does -- the editor reaches nothing else there -- and
    /// the menu says so for the rest; a file that is skipped here anyway is
    /// reported rather than lost.
    ///
    /// Done from a task rather than inside the click: opening an editor changes
    /// the pane's active item, which is this panel, and the click is still
    /// inside an update of it.
    fn open_compose_files(&mut self, row: &Resource, window: &mut Window, cx: &mut Context<Self>) {
        let Some(files) = target_of(row, &self.compose_members).map(|target| target.config_files)
        else {
            return;
        };
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        let remote = self.host_name().is_some();
        self.spawn_tracked(window, cx, async move |this, cx| {
            let started = workspace.update_in(cx, |workspace, window, cx| {
                let mut opening = Vec::new();
                for file in files {
                    let task = if remote {
                        let found = workspace.project().read(cx).find_project_path(&file, cx);
                        match found {
                            Some(path) => workspace.open_path(path, None, true, window, cx),
                            None => {
                                log::warn!("{file} is outside the project's folders");
                                continue;
                            }
                        }
                    } else {
                        workspace.open_abs_path(
                            PathBuf::from(&file),
                            OpenOptions::default(),
                            window,
                            cx,
                        )
                    };
                    opening.push((file, task));
                }
                opening
            });
            let opening = match started {
                Ok(opening) => opening,
                Err(error) => {
                    log::debug!("workspace closed before compose files could open: {error}");
                    return;
                }
            };
            for (file, task) in opening {
                let Err(error) = task.await else {
                    continue;
                };
                let reported = this.update(cx, |this, cx| {
                    this.last_error = Some(ContainerError::CommandFailed {
                        program: "Open Compose File".into(),
                        stderr: format!("could not open {file}: {error:#}"),
                    });
                    cx.notify();
                });
                if let Err(error) = reported {
                    log::debug!("panel closed before an open failure could be shown: {error}");
                }
            }
        });
    }

    /// Keeps a task of the panel's own in `actions` until it is done, and takes
    /// it out as its last act -- the self-removal `act` and `destroy` use, once.
    fn spawn_tracked(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        work: impl AsyncFnOnce(WeakEntity<Self>, &mut AsyncWindowContext) + 'static,
    ) {
        let action_id = self.next_action_id;
        self.next_action_id += 1;
        self.actions.insert(
            action_id,
            cx.spawn_in(window, async move |this, cx| {
                work(this.clone(), cx).await;
                // The task's last statement, so it is not polled again; see the
                // matching comment in `ContainerPanel::act`.
                if let Err(error) = this.update(cx, |this, _cx| {
                    this.actions.remove(&action_id);
                }) {
                    log::debug!("panel closed before its task could self-remove: {error}");
                }
            }),
        );
    }
}
