use anyhow::{Context as _, Result};
use collections::HashMap;
use gpui::{App, AppContext as _, AsyncApp, Context, Entity, Task, WeakEntity};

use futures::{FutureExt, StreamExt as _, channel::mpsc, future::Shared};
use itertools::Itertools as _;
use language::LanguageName;
use remote::RemoteClient;
use rpc::{TypedEnvelope, proto};
use settings::{Settings, SettingsLocation};
use smol::channel::bounded;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use task::{Shell, ShellBuilder, ShellKind, SpawnInTerminal};
use terminal::{
    RemoteBackedOptions, RemoteTerminalCommand, TaskState, TaskStatus, Terminal, TerminalBounds,
    TerminalBuilder, insert_zed_terminal_env,
    terminal_settings::{self, TerminalSettings},
};
use util::{
    ResultExt as _, command::new_std_command, get_default_system_shell, get_system_shell, maybe,
    rel_path::RelPath,
};

use crate::{Project, ProjectPath};

#[derive(Default)]
pub struct Terminals {
    pub(crate) local_handles: Vec<WeakEntity<terminal::Terminal>>,
    remote: RemoteTerminals,
}

/// Terminals whose process runs on the remote server, by the id this side
/// gave them, so that what the server sends about one finds it.
#[derive(Default)]
struct RemoteTerminals {
    next_id: u64,
    live: HashMap<u64, WeakEntity<terminal::Terminal>>,
}

struct RpcLook {
    cursor_shape: terminal_settings::CursorShape,
    alternate_scroll: terminal_settings::AlternateScroll,
    max_scroll_history_lines: Option<usize>,
}

/// What to start on the remote server. A missing program is the host's own
/// login shell.
struct RpcLaunch {
    program: Option<String>,
    args: Vec<String>,
    env: HashMap<String, String>,
    working_directory: Option<Arc<Path>>,
}

impl Project {
    pub fn active_entry_directory(&self, cx: &App) -> Option<PathBuf> {
        let entry_id = self.active_entry()?;
        let worktree = self.worktree_for_entry(entry_id, cx)?;
        let worktree = worktree.read(cx);
        let entry = worktree.entry_for_id(entry_id)?;

        let absolute_path = worktree.absolutize(entry.path.as_ref());
        if entry.is_dir() {
            Some(absolute_path)
        } else {
            absolute_path.parent().map(|p| p.to_path_buf())
        }
    }

    pub fn active_project_directory(&self, cx: &App) -> Option<Arc<Path>> {
        self.active_entry()
            .and_then(|entry_id| self.worktree_for_entry(entry_id, cx))
            .into_iter()
            .chain(self.worktrees(cx))
            .find_map(|tree| tree.read(cx).root_dir())
    }

    pub fn first_project_directory(&self, cx: &App) -> Option<PathBuf> {
        let worktree = self.worktrees(cx).next()?;
        let worktree = worktree.read(cx);
        if worktree.root_entry()?.is_dir() {
            Some(worktree.abs_path().to_path_buf())
        } else {
            None
        }
    }

    pub fn create_terminal_task(
        &mut self,
        spawn_task: SpawnInTerminal,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Terminal>>> {
        let is_via_remote = self.remote_client.is_some();

        let path: Option<Arc<Path>> = if let Some(cwd) = &spawn_task.cwd {
            if is_via_remote {
                Some(Arc::from(cwd.as_ref()))
            } else {
                let cwd = cwd.to_string_lossy();
                let tilde_substituted = shellexpand::tilde(&cwd);
                Some(Arc::from(Path::new(tilde_substituted.as_ref())))
            }
        } else {
            self.active_project_directory(cx)
        };

        let mut settings_location = None;
        if let Some(path) = path.as_ref()
            && let Some((worktree, _)) = self.find_worktree(path, cx)
        {
            settings_location = Some(SettingsLocation {
                worktree_id: worktree.read(cx).id(),
                path: RelPath::empty(),
            });
        }
        let settings = TerminalSettings::get(settings_location, cx).clone();
        let detect_venv = settings.detect_venv.as_option().is_some();

        let (completion_tx, completion_rx) = bounded(1);

        let local_path = if is_via_remote { None } else { path.clone() };
        let task_state = Some(TaskState {
            spawned_task: spawn_task.clone(),
            status: TaskStatus::Running,
            completion_rx,
        });
        let remote_client = self.remote_client.clone();
        let over_rpc = remote_client
            .as_ref()
            .is_some_and(|client| client.read(cx).terminals_over_rpc());
        let shell = match &remote_client {
            Some(remote_client) => remote_client
                .read(cx)
                .shell()
                .unwrap_or_else(get_default_system_shell),
            None => get_system_shell(),
        };
        let path_style = self.path_style(cx);
        let shell_kind = ShellKind::new(&shell, path_style.is_windows());

        // Prepare a task for resolving the environment
        let env_task =
            self.resolve_directory_environment(&shell, path.clone(), remote_client.clone(), cx);

        let project_path_contexts = self
            .active_entry()
            .and_then(|entry_id| self.path_for_entry(entry_id, cx))
            .into_iter()
            .chain(
                self.visible_worktrees(cx)
                    .map(|wt| wt.read(cx).id())
                    .map(|worktree_id| ProjectPath {
                        worktree_id,
                        path: Arc::from(RelPath::empty()),
                    }),
            );
        let toolchains = project_path_contexts
            .filter(|_| detect_venv)
            .map(|p| self.active_toolchain(p, LanguageName::new_static("Python"), cx))
            .collect::<Vec<_>>();
        let lang_registry = self.languages.clone();
        cx.spawn(async move |project, cx| {
            let mut env = env_task.await.unwrap_or_default();
            env.extend(settings.env);

            let activation_script = maybe!(async {
                for toolchain in toolchains {
                    let Some(toolchain) = toolchain.await else {
                        continue;
                    };
                    let language = lang_registry
                        .language_for_name(&toolchain.language_name.0)
                        .await
                        .ok();
                    let lister = language?.toolchain_lister()?;
                    let future =
                        cx.update(|cx| lister.activation_script(&toolchain, shell_kind, cx));
                    return Some(future.await);
                }
                None
            })
            .await
            .unwrap_or_default();

            if over_rpc && let Some(remote_client) = remote_client.clone() {
                env.extend(spawn_task.env);
                let (program, args) = if activation_script.is_empty() {
                    (spawn_task.command, spawn_task.args)
                } else {
                    let separator = shell_kind.sequential_commands_separator();
                    let activation_script = activation_script.join(&format!("{separator} "));
                    let to_run = match &spawn_task.command {
                        Some(command) => {
                            let command = shell_kind.prepend_command_prefix(command);
                            let command = shell_kind.try_quote_prefix_aware(&command);
                            let args = spawn_task
                                .args
                                .iter()
                                .filter_map(|arg| shell_kind.try_quote(arg));
                            command.into_iter().chain(args).join(" ")
                        }
                        None => format!("exec {shell} -l"),
                    };
                    let arg = format!("{activation_script}{separator} {to_run}");
                    (Some(shell.clone()), shell_kind.args_for_shell(true, arg))
                };
                let launch = RpcLaunch {
                    program,
                    args,
                    env,
                    working_directory: path,
                };
                return project
                    .update(cx, |this, cx| {
                        this.spawn_rpc_terminal(
                            remote_client,
                            launch,
                            RemoteBackedOptions {
                                task: task_state,
                                completion_tx: Some(completion_tx),
                                title_override: None,
                            },
                            RpcLook {
                                cursor_shape: settings.cursor_shape,
                                alternate_scroll: settings.alternate_scroll,
                                max_scroll_history_lines: settings.max_scroll_history_lines,
                            },
                            cx,
                        )
                    })?
                    .await;
            }

            let builder = project
                .update(cx, move |_, cx| {
                    let format_to_run = || {
                        if let Some(command) = &spawn_task.command {
                            let command = shell_kind.prepend_command_prefix(command);
                            let command = shell_kind.try_quote_prefix_aware(&command);
                            let args = spawn_task
                                .args
                                .iter()
                                .filter_map(|arg| shell_kind.try_quote(&arg));

                            command.into_iter().chain(args).join(" ")
                        } else {
                            // todo: this breaks for remotes to windows
                            format!("exec {shell} -l")
                        }
                    };

                    let (shell, env) = {
                        env.extend(spawn_task.env);
                        match remote_client {
                            Some(remote_client) => match activation_script.clone() {
                                activation_script if !activation_script.is_empty() => {
                                    let separator = shell_kind.sequential_commands_separator();
                                    let activation_script =
                                        activation_script.join(&format!("{separator} "));
                                    let to_run = format_to_run();

                                    let arg = format!("{activation_script}{separator} {to_run}");
                                    let args = shell_kind.args_for_shell(true, arg);
                                    let shell = remote_client
                                        .read(cx)
                                        .shell()
                                        .unwrap_or_else(get_default_system_shell);

                                    create_remote_shell(
                                        Some((&shell, &args)),
                                        env,
                                        path,
                                        remote_client,
                                        cx,
                                    )?
                                }
                                _ => create_remote_shell(
                                    spawn_task
                                        .command
                                        .as_ref()
                                        .map(|command| (command, &spawn_task.args)),
                                    env,
                                    path,
                                    remote_client,
                                    cx,
                                )?,
                            },
                            None => match activation_script.clone() {
                                activation_script if !activation_script.is_empty() => {
                                    let separator = shell_kind.sequential_commands_separator();
                                    let activation_script =
                                        activation_script.join(&format!("{separator} "));
                                    let to_run = format_to_run();

                                    let arg = format!("{activation_script}{separator} {to_run}");
                                    let args = shell_kind.args_for_shell(true, arg);

                                    (
                                        Shell::WithArguments {
                                            program: shell,
                                            args,
                                            title_override: None,
                                        },
                                        env,
                                    )
                                }
                                _ => (
                                    if let Some(program) = spawn_task.command {
                                        Shell::WithArguments {
                                            program,
                                            args: spawn_task.args,
                                            title_override: None,
                                        }
                                    } else {
                                        Shell::System
                                    },
                                    env,
                                ),
                            },
                        }
                    };
                    anyhow::Ok(TerminalBuilder::new(
                        local_path.map(|path| path.to_path_buf()),
                        task_state,
                        shell,
                        env,
                        settings.cursor_shape,
                        settings.alternate_scroll,
                        settings.max_scroll_history_lines,
                        settings.path_hyperlink_regexes,
                        settings.path_hyperlink_timeout_ms,
                        is_via_remote,
                        cx.entity_id().as_u64(),
                        Some(completion_tx),
                        cx,
                        activation_script,
                        path_style,
                    ))
                })??
                .await?;
            project.update(cx, move |this, cx| {
                let terminal_handle = cx.new(|cx| builder.subscribe(cx));

                this.terminals
                    .local_handles
                    .push(terminal_handle.downgrade());

                let id = terminal_handle.entity_id();
                cx.observe_release(&terminal_handle, move |project, _terminal, cx| {
                    let handles = &mut project.terminals.local_handles;

                    if let Some(index) = handles
                        .iter()
                        .position(|terminal| terminal.entity_id() == id)
                    {
                        handles.remove(index);
                        cx.notify();
                    }
                })
                .detach();

                terminal_handle
            })
        })
    }

    pub fn create_terminal_shell(
        &mut self,
        cwd: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Terminal>>> {
        self.create_terminal_shell_internal(cwd, false, cx)
    }

    /// Creates a local terminal even if the project is remote.
    /// In remote projects: opens in Zed's launch directory (bypasses SSH).
    /// In local projects: opens in the project directory (same as regular terminals).
    pub fn create_local_terminal(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Terminal>>> {
        let working_directory = if self.remote_client.is_some() {
            // Remote project: don't use remote paths, let shell use Zed's cwd
            None
        } else {
            // Local project: use project directory like normal terminals
            self.active_project_directory(cx).map(|p| p.to_path_buf())
        };
        self.create_terminal_shell_internal(working_directory, true, cx)
    }

    /// Internal method for creating terminal shells.
    /// If force_local is true, creates a local terminal even if the project has a remote client.
    /// This allows "breaking out" to a local shell in remote projects.
    fn create_terminal_shell_internal(
        &mut self,
        cwd: Option<PathBuf>,
        force_local: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Terminal>>> {
        let path = cwd.map(|p| Arc::from(&*p));
        let is_via_remote = !force_local && self.remote_client.is_some();

        let mut settings_location = None;
        if let Some(path) = path.as_ref()
            && let Some((worktree, _)) = self.find_worktree(path, cx)
        {
            settings_location = Some(SettingsLocation {
                worktree_id: worktree.read(cx).id(),
                path: RelPath::empty(),
            });
        }
        let settings = TerminalSettings::get(settings_location, cx).clone();
        let detect_venv = settings.detect_venv.as_option().is_some();
        let local_path = if is_via_remote { None } else { path.clone() };

        let project_path_contexts = self
            .active_entry()
            .and_then(|entry_id| self.path_for_entry(entry_id, cx))
            .into_iter()
            .chain(
                self.visible_worktrees(cx)
                    .map(|wt| wt.read(cx).id())
                    .map(|worktree_id| ProjectPath {
                        worktree_id,
                        path: RelPath::empty().into(),
                    }),
            );
        let toolchains = project_path_contexts
            .filter(|_| detect_venv)
            .map(|p| self.active_toolchain(p, LanguageName::new_static("Python"), cx))
            .collect::<Vec<_>>();
        let remote_client = if force_local {
            None
        } else {
            self.remote_client.clone()
        };
        let over_rpc = remote_client
            .as_ref()
            .is_some_and(|client| client.read(cx).terminals_over_rpc());
        let shell = match &remote_client {
            Some(remote_client) => remote_client
                .read(cx)
                .shell()
                .unwrap_or_else(get_default_system_shell),
            None => settings.shell.program(),
        };
        let env_shell = match &remote_client {
            Some(_) => shell.clone(),
            None => get_system_shell(),
        };

        let path_style = self.path_style(cx);

        // Prepare a task for resolving the environment
        let env_task =
            self.resolve_directory_environment(&env_shell, path.clone(), remote_client.clone(), cx);

        let lang_registry = self.languages.clone();
        cx.spawn(async move |project, cx| {
            let shell_kind = ShellKind::new(&shell, path_style.is_windows());
            let mut env = env_task.await.unwrap_or_default();
            env.extend(settings.env);

            let activation_script = maybe!(async {
                for toolchain in toolchains {
                    let Some(toolchain) = toolchain.await else {
                        continue;
                    };
                    let language = lang_registry
                        .language_for_name(&toolchain.language_name.0)
                        .await
                        .ok();
                    let lister = language?.toolchain_lister()?;
                    let future =
                        cx.update(|cx| lister.activation_script(&toolchain, shell_kind, cx));
                    return Some(future.await);
                }
                None
            })
            .await
            .unwrap_or_default();

            if over_rpc && let Some(remote_client) = remote_client.clone() {
                let launch = RpcLaunch {
                    program: None,
                    args: Vec::new(),
                    env,
                    working_directory: path,
                };
                return project
                    .update(cx, |this, cx| {
                        this.spawn_rpc_terminal(
                            remote_client,
                            launch,
                            RemoteBackedOptions::default(),
                            RpcLook {
                                cursor_shape: settings.cursor_shape,
                                alternate_scroll: settings.alternate_scroll,
                                max_scroll_history_lines: settings.max_scroll_history_lines,
                            },
                            cx,
                        )
                    })?
                    .await;
            }

            let builder = project
                .update(cx, move |_, cx| {
                    let (shell, env) = {
                        match remote_client {
                            Some(remote_client) => {
                                create_remote_shell(None, env, path, remote_client, cx)?
                            }
                            None => (settings.shell, env),
                        }
                    };
                    anyhow::Ok(TerminalBuilder::new(
                        local_path.map(|path| path.to_path_buf()),
                        None,
                        shell,
                        env,
                        settings.cursor_shape,
                        settings.alternate_scroll,
                        settings.max_scroll_history_lines,
                        settings.path_hyperlink_regexes,
                        settings.path_hyperlink_timeout_ms,
                        is_via_remote,
                        cx.entity_id().as_u64(),
                        None,
                        cx,
                        activation_script,
                        path_style,
                    ))
                })??
                .await?;
            project.update(cx, move |this, cx| {
                let terminal_handle = cx.new(|cx| builder.subscribe(cx));

                this.terminals
                    .local_handles
                    .push(terminal_handle.downgrade());

                let id = terminal_handle.entity_id();
                cx.observe_release(&terminal_handle, move |project, _terminal, cx| {
                    let handles = &mut project.terminals.local_handles;

                    if let Some(index) = handles
                        .iter()
                        .position(|terminal| terminal.entity_id() == id)
                    {
                        handles.remove(index);
                        cx.notify();
                    }
                })
                .detach();

                terminal_handle
            })
        })
    }

    pub fn clone_terminal(
        &mut self,
        terminal: &Entity<Terminal>,
        cx: &mut Context<'_, Project>,
        cwd: Option<PathBuf>,
    ) -> Task<Result<Entity<Terminal>>> {
        // We cannot clone the task's terminal, as it will effectively re-spawn the task, which might not be desirable.
        // For now, create a new shell instead.
        if terminal.read(cx).task().is_some() {
            return self.create_terminal_shell(cwd, cx);
        }
        // A copy of this terminal's launch would start a local process, but
        // this one's process is on the remote server.
        if self
            .remote_client
            .as_ref()
            .is_some_and(|client| client.read(cx).terminals_over_rpc())
        {
            return self.create_terminal_shell(cwd, cx);
        }
        let local_path = if self.is_via_remote_server() {
            None
        } else {
            cwd
        };

        let builder = terminal.read(cx).clone_builder(cx, local_path);
        cx.spawn(async |project, cx| {
            let terminal = builder.await?;
            project.update(cx, |project, cx| {
                let terminal_handle = cx.new(|cx| terminal.subscribe(cx));

                project
                    .terminals
                    .local_handles
                    .push(terminal_handle.downgrade());

                let id = terminal_handle.entity_id();
                cx.observe_release(&terminal_handle, move |project, _terminal, cx| {
                    let handles = &mut project.terminals.local_handles;

                    if let Some(index) = handles
                        .iter()
                        .position(|terminal| terminal.entity_id() == id)
                    {
                        handles.remove(index);
                        cx.notify();
                    }
                })
                .detach();

                terminal_handle
            })
        })
    }

    pub fn terminal_settings<'a>(
        &'a self,
        path: &'a Option<PathBuf>,
        cx: &'a App,
    ) -> &'a TerminalSettings {
        let mut settings_location = None;
        if let Some(path) = path.as_ref()
            && let Some((worktree, _)) = self.find_worktree(path, cx)
        {
            settings_location = Some(SettingsLocation {
                worktree_id: worktree.read(cx).id(),
                path: RelPath::empty(),
            });
        }
        TerminalSettings::get(settings_location, cx)
    }

    pub fn exec_in_shell(
        &self,
        command: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<smol::process::Command>> {
        let path = self.first_project_directory(cx);
        let remote_client = self.remote_client.clone();
        let settings = self.terminal_settings(&path, cx).clone();
        let shell = remote_client
            .as_ref()
            .and_then(|remote_client| remote_client.read(cx).shell())
            .map(Shell::Program)
            .unwrap_or(Shell::System);
        let is_windows = self.path_style(cx).is_windows();
        let builder = ShellBuilder::new(&shell, is_windows).non_interactive();
        let (command, args) = builder.build(Some(command), &Vec::new());

        let env_task = self.resolve_directory_environment(
            &shell.program(),
            path.as_ref().map(|p| Arc::from(&**p)),
            remote_client.clone(),
            cx,
        );

        cx.spawn(async move |project, cx| {
            let mut env = env_task.await.unwrap_or_default();
            env.extend(settings.env);

            project.update(cx, move |_, cx| {
                match remote_client {
                    Some(remote_client) => {
                        let command_template = remote_client.read(cx).build_command(
                            Some(command),
                            &args,
                            &env,
                            None,
                            None,
                        )?;
                        let mut command = new_std_command(command_template.program);
                        command.args(command_template.args);
                        command.envs(command_template.env);
                        Ok(command)
                    }
                    None => {
                        let mut command = new_std_command(command);
                        command.args(args);
                        command.envs(env);
                        if let Some(path) = path {
                            command.current_dir(path);
                        }
                        Ok(command)
                    }
                }
                .map(|mut process| {
                    util::set_pre_exec_to_start_new_session(&mut process);
                    smol::process::Command::from(process)
                })
            })?
        })
    }

    pub fn local_terminal_handles(&self) -> &Vec<WeakEntity<terminal::Terminal>> {
        &self.terminals.local_handles
    }

    /// FR3/FR5 (Phase 5 of multi-project-window-switching): shrinks
    /// scrollback for every local terminal this project owns, including
    /// task terminals — FR5's highest-leverage case, since those always
    /// use `MAX_SCROLL_HISTORY_LINES` (100_000) regardless of user
    /// settings and are the terminals most likely to be holding real
    /// memory when hibernated (e.g. a long-running `npm run dev` left in
    /// the background). See `Terminal::limit_scroll_history` for why a
    /// repeat call per terminal is a safe no-op.
    ///
    /// Only reaches terminals that already exist at the moment this is
    /// called (the `Warm -> Hibernated` transition). Assumes nothing
    /// creates a *new* terminal for an already-`Hibernated` project
    /// in between hibernate and wake — true today, since terminal
    /// creation is normally a user action against the `Active` project.
    /// If that ever changes, such a terminal would keep its full
    /// scrollback until the next hibernate cycle catches it.
    pub(crate) fn limit_terminal_scroll_history(&self, lines: usize, cx: &mut Context<Self>) {
        for terminal in self.local_terminal_handles() {
            if let Some(terminal) = terminal.upgrade() {
                terminal.update(cx, |terminal, _cx| terminal.limit_scroll_history(lines));
            }
        }
    }

    /// Undoes `limit_terminal_scroll_history` for every local terminal
    /// this project owns. Called unconditionally on wake, same
    /// guard-at-the-resource reasoning as every other wake path in this
    /// plan: a terminal this project never actually shrunk (e.g. it was
    /// created after wake, or the setting was off when this project last
    /// hibernated) simply no-ops.
    pub(crate) fn restore_terminal_scroll_history(&self, cx: &mut Context<Self>) {
        for terminal in self.local_terminal_handles() {
            if let Some(terminal) = terminal.upgrade() {
                terminal.update(cx, |terminal, _cx| terminal.restore_scroll_history_limit());
            }
        }
    }

    /// Starts a terminal process on the remote server and returns the
    /// terminal that shows it, once the server has accepted it.
    fn spawn_rpc_terminal(
        &mut self,
        remote_client: Entity<RemoteClient>,
        launch: RpcLaunch,
        options: RemoteBackedOptions,
        look: RpcLook,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Terminal>>> {
        let proto_client = remote_client.read(cx).proto_client();
        let host = remote_client.read(cx).connection_options().display_name();
        let mut env = launch.env;
        env.remove("SHLVL");
        insert_zed_terminal_env(&mut env, &release_channel::AppVersion::global(cx));

        let terminal_id = self.terminals.remote.next_id;
        self.terminals.remote.next_id += 1;

        let (commands_tx, mut commands_rx) = mpsc::unbounded();
        let options = RemoteBackedOptions {
            title_override: options
                .title_override
                .or_else(|| Some(format!("{host} — Terminal"))),
            ..options
        };
        let builder = match TerminalBuilder::new_remote_backed(
            commands_tx,
            options,
            look.cursor_shape,
            look.alternate_scroll,
            look.max_scroll_history_lines,
            cx.entity_id().as_u64(),
            cx.background_executor(),
            self.path_style(cx),
        ) {
            Ok(builder) => builder,
            Err(error) => return Task::ready(Err(error)),
        };
        let terminal = cx.new(|cx| builder.subscribe(cx));

        // Registered before the server hears of it: output can arrive as soon
        // as the process starts.
        self.terminals
            .remote
            .live
            .insert(terminal_id, terminal.downgrade());
        self.terminals.local_handles.push(terminal.downgrade());
        let entity_id = terminal.entity_id();
        cx.observe_release(&terminal, move |project, _terminal, cx| {
            project
                .terminals
                .local_handles
                .retain(|handle| handle.entity_id() != entity_id);
            // Ids are never reused, so the entry can only be this terminal's.
            project.terminals.remote.live.remove(&terminal_id);
            cx.notify();
        })
        .detach();

        let bounds = TerminalBounds::default();
        let create = proto_client.request(proto::CreateTerminal {
            project_id: rpc::proto::REMOTE_SERVER_PROJECT_ID,
            terminal_id,
            program: launch.program,
            args: launch.args,
            env: env.into_iter().collect(),
            cwd: launch
                .working_directory
                .map(|path| path.to_string_lossy().into_owned()),
            columns: u32::try_from(bounds.num_columns()).unwrap_or(u32::MAX),
            rows: u32::try_from(bounds.num_lines()).unwrap_or(u32::MAX),
        });
        let weak_terminal = terminal.downgrade();
        cx.spawn(async move |project, cx| {
            if let Err(error) = create.await {
                project
                    .update(cx, |project, _| {
                        project.terminals.remote.live.remove(&terminal_id);
                    })
                    .ok();
                return Err(error.context("starting the terminal on the remote machine"));
            }

            // Only now do typed bytes and resizes follow, so none can reach
            // the server before the terminal exists there.
            cx.spawn(async move |cx| {
                let forget_terminal = |cx: &mut AsyncApp| {
                    project
                        .update(cx, |project, _| {
                            project.terminals.remote.live.remove(&terminal_id);
                        })
                        .ok();
                };
                while let Some(command) = commands_rx.next().await {
                    let sent = match command {
                        RemoteTerminalCommand::Input(data) => {
                            proto_client.send(proto::TerminalInput {
                                project_id: rpc::proto::REMOTE_SERVER_PROJECT_ID,
                                terminal_id,
                                data,
                            })
                        }
                        RemoteTerminalCommand::Resize { columns, rows } => {
                            proto_client.send(proto::ResizeTerminal {
                                project_id: rpc::proto::REMOTE_SERVER_PROJECT_ID,
                                terminal_id,
                                columns: u32::from(columns),
                                rows: u32::from(rows),
                            })
                        }
                        RemoteTerminalCommand::Close => {
                            let closed = proto_client
                                .request(proto::CloseTerminal {
                                    project_id: rpc::proto::REMOTE_SERVER_PROJECT_ID,
                                    terminal_id,
                                })
                                .await
                                .map(|_| ());
                            // The server does not report an exit it caused,
                            // so the terminal is told here.
                            if let Some(terminal) = weak_terminal.upgrade() {
                                terminal.update(cx, |terminal, cx| {
                                    terminal.remote_process_exited(None, cx)
                                });
                            }
                            forget_terminal(cx);
                            closed
                        }
                    };
                    if let Err(error) = sent {
                        log::warn!(
                            "terminal {terminal_id}: the remote connection is gone: {error:#}"
                        );
                        forget_terminal(cx);
                        return;
                    }
                }
                forget_terminal(cx);
                // The terminal was dropped, which ends its process.
                proto_client
                    .request(proto::CloseTerminal {
                        project_id: rpc::proto::REMOTE_SERVER_PROJECT_ID,
                        terminal_id,
                    })
                    .await
                    .log_err();
            })
            .detach();
            Ok(terminal)
        })
    }

    pub(crate) async fn handle_terminal_output(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::TerminalOutput>,
        mut cx: AsyncApp,
    ) -> Result<proto::Ack> {
        this.update(&mut cx, |this, cx| {
            let terminal = this
                .terminals
                .remote
                .live
                .get(&envelope.payload.terminal_id)
                .and_then(|terminal| terminal.upgrade())
                .with_context(|| format!("no terminal {}", envelope.payload.terminal_id))?;
            terminal.update(cx, |terminal, cx| {
                terminal.feed_remote_output(&envelope.payload.data, cx)
            });
            anyhow::Ok(())
        })?;
        Ok(proto::Ack {})
    }

    pub(crate) async fn handle_terminal_exited(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::TerminalExited>,
        mut cx: AsyncApp,
    ) -> Result<()> {
        this.update(&mut cx, |this, cx| {
            let terminal = this
                .terminals
                .remote
                .live
                .remove(&envelope.payload.terminal_id)
                .and_then(|terminal| terminal.upgrade());
            if let Some(terminal) = terminal {
                terminal.update(cx, |terminal, cx| {
                    terminal.remote_process_exited(envelope.payload.exit_code, cx)
                });
            }
        });
        Ok(())
    }

    fn resolve_directory_environment(
        &self,
        shell: &str,
        path: Option<Arc<Path>>,
        remote_client: Option<Entity<RemoteClient>>,
        cx: &mut App,
    ) -> Shared<Task<Option<HashMap<String, String>>>> {
        if let Some(path) = &path {
            let shell = Shell::Program(shell.to_string());
            self.environment
                .update(cx, |project_env, cx| match &remote_client {
                    Some(remote_client) => project_env.remote_directory_environment(
                        &shell,
                        path.clone(),
                        remote_client.clone(),
                        cx,
                    ),
                    None => project_env.local_directory_environment(&shell, path.clone(), cx),
                })
        } else {
            Task::ready(None).shared()
        }
    }
}

fn create_remote_shell(
    spawn_command: Option<(&String, &Vec<String>)>,
    mut env: HashMap<String, String>,
    working_directory: Option<Arc<Path>>,
    remote_client: Entity<RemoteClient>,
    cx: &mut App,
) -> Result<(Shell, HashMap<String, String>)> {
    insert_zed_terminal_env(&mut env, &release_channel::AppVersion::global(cx));

    let (program, args) = match spawn_command {
        Some((program, args)) => (Some(program.clone()), args),
        None => (None, &Vec::new()),
    };

    let command = remote_client.read(cx).build_command(
        program,
        args.as_slice(),
        &env,
        working_directory.map(|path| path.display().to_string()),
        None,
    )?;

    log::debug!("Connecting to a remote server: {:?}", command.program);
    let host = remote_client.read(cx).connection_options().display_name();

    Ok((
        Shell::WithArguments {
            program: command.program,
            args: command.args,
            title_override: Some(format!("{} — Terminal", host)),
        },
        command.env,
    ))
}
