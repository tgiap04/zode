//! Running a host tool (`docker`, `kubectl`, `gh`, `git`) on the machine a
//! remote project lives on.
//!
//! The tool runs through the same local command that reaches the host for
//! debug adapters: `wsl.exe --distribution D --cd DIR --exec env K=V... tool
//! args...` for WSL, with no shell in between; `ssh ... -T host 'cd DIR &&
//! exec env K=V... tool args...'` for SSH, which the host's login shell reads
//! once; `docker exec` for a dev container. None of these reads the user's rc
//! files, so `PATH` and the few variables the tools read are carried over from
//! the project's remote environment, which the remote server loads from the
//! user's own shell.
//!
//! Dropping a command stops the local wrapper only. The tool on the host ends
//! when it next writes to the pipe the wrapper held: at once for a one-shot
//! command, on the next event for a `docker events` or `kubectl --watch`.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result, anyhow};
use collections::HashMap;
use parking_lot::RwLock;
use remote::{Interactive, RemoteConnection, RemoteConnectionOptions};
use util::command::{Command, CommandHost};
use util::paths::PathStyle;

use crate::agent_server_store::without_windows_drive_mounts;

/// What a tool needs from the user's shell that the wrapper's command lacks.
///
/// Not the whole environment: it holds the shell's own bookkeeping (`PWD`,
/// `SHLVL`, `_`) and every variable would be spelled out on the wrapper's
/// command line, which `wsl.exe` and `ssh` both cap.
const FORWARDED_VARIABLES: &[&str] = &[
    "PATH",
    "KUBECONFIG",
    "DOCKER_HOST",
    "DOCKER_CONTEXT",
    "CONTAINER_HOST",
    "GIT_SSH_COMMAND",
    "GIT_SSH",
    // Hosts and not tokens: every value here is spelled out on the wrapper's
    // command line, which other processes of the same user on this machine
    // can read. The forge CLIs keep their sign-in on the host itself.
    "GH_HOST",
    "GITLAB_HOST",
];

/// Forwarded from a WSL distro only.
///
/// A remote server over SSH outlives the session that started it, so the
/// agent socket it loaded can be one that is gone, and it would override the
/// live one the new SSH session already gives the tool.
const WSL_ONLY_VARIABLES: &[&str] = &["SSH_AUTH_SOCK"];

/// The `PATH` a WSL tool gets when the distro's own environment could not be
/// loaded. Without one, WSL's default applies, and it appends the Windows
/// `PATH`.
pub(crate) const DEFAULT_LINUX_PATH: &str =
    "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// The host of a project opened over a remote server.
pub struct RemoteCommandHost {
    /// The connection commands are built through right now; `None` while it
    /// is down. Replaced on reconnect rather than held from creation: an SSH
    /// connection's commands go through its own control socket, which the
    /// connection that replaces it does not share.
    connection: RwLock<Option<Arc<dyn RemoteConnection>>>,
    host_name: String,
    path_style: PathStyle,
    environment: HashMap<String, String>,
    forwarded: HashMap<String, String>,
}

impl RemoteCommandHost {
    /// `environment` is the project's environment on the host, as loaded by
    /// the remote server; empty when it could not be loaded.
    pub fn new(
        connection: Arc<dyn RemoteConnection>,
        environment: HashMap<String, String>,
    ) -> Self {
        let options = connection.connection_options();
        let host_name = options.display_name();
        // Same rule as locating an agent CLI: WSL appends the Windows `PATH`,
        // and a `docker.exe` or `gh.exe` found under `/mnt/c` would act on the
        // Windows machine the user asked to keep out of this project.
        let is_wsl_connection = matches!(options, RemoteConnectionOptions::Wsl(_));
        let inside_wsl = is_wsl_connection || environment.contains_key("WSL_DISTRO_NAME");
        let wsl_only = WSL_ONLY_VARIABLES.iter().filter(|_| is_wsl_connection);
        let mut forwarded: HashMap<String, String> = FORWARDED_VARIABLES
            .iter()
            .chain(wsl_only)
            .filter_map(|name| {
                let value = environment.get(*name)?;
                let value = if *name == "PATH" && inside_wsl {
                    without_windows_drive_mounts(value)
                } else {
                    value.clone()
                };
                (!value.is_empty()).then(|| (name.to_string(), value))
            })
            .collect();
        if inside_wsl && !forwarded.contains_key("PATH") {
            forwarded.insert("PATH".to_string(), DEFAULT_LINUX_PATH.to_string());
        }
        Self {
            path_style: connection.path_style(),
            connection: RwLock::new(Some(connection)),
            host_name,
            environment,
            forwarded,
        }
    }

    /// Points later commands at `connection`, the one the project's remote
    /// client holds now.
    pub fn set_connection(&self, connection: Option<Arc<dyn RemoteConnection>>) {
        *self.connection.write() = connection;
    }
}

impl CommandHost for RemoteCommandHost {
    fn command(
        &self,
        program: &str,
        args: &[&str],
        env: &[(&str, &str)],
        cwd: Option<&Path>,
    ) -> Result<Command> {
        let mut variables = self.forwarded.clone();
        for (name, value) in env {
            variables.insert(name.to_string(), value.to_string());
        }
        let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        let working_dir = cwd.map(|cwd| cwd.to_string_lossy().into_owned());
        let connection = self
            .connection
            .read()
            .clone()
            .ok_or_else(|| anyhow!("{} is not connected", self.host_name))?;
        let template = connection
            .build_command(
                Some(program.to_string()),
                &args,
                &variables,
                working_dir,
                None,
                Interactive::No,
            )
            .with_context(|| format!("{program} cannot be run on {}", self.host_name))?;

        let mut command = Command::new(&template.program);
        command.args(&template.args).envs(&template.env);
        // The wrapper is the only handle on the remote tool: one left running
        // after its caller gave up keeps the tool (a `docker events`, a `git
        // fetch`) running on the host with nobody reading it.
        command.kill_on_drop(true);
        Ok(command)
    }

    fn path_style(&self) -> PathStyle {
        self.path_style
    }

    fn remote_name(&self) -> Option<String> {
        Some(self.host_name.clone())
    }

    fn environment_variable(&self, name: &str) -> Option<String> {
        self.environment.get(name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::channel::mpsc::{Sender, UnboundedReceiver, UnboundedSender};
    use gpui::{App, AsyncApp, Task};
    use parking_lot::Mutex;
    use remote::{CommandTemplate, RemoteClientDelegate, WslConnectionOptions};
    use rpc::proto::Envelope;
    use std::path::PathBuf;
    use util::paths::RemotePathBuf;

    #[derive(Clone, Debug, PartialEq)]
    struct Built {
        program: Option<String>,
        args: Vec<String>,
        env: HashMap<String, String>,
        working_dir: Option<String>,
        interactive: Interactive,
    }

    /// A connection that records what it is asked to build and answers with a
    /// fixed wrapper, or refuses the way the relay transport does.
    struct RecordingConnection {
        options: RemoteConnectionOptions,
        wrapper: CommandTemplate,
        refuse: bool,
        built: Mutex<Vec<Built>>,
    }

    impl RecordingConnection {
        fn wsl(distro: &str) -> Self {
            Self {
                options: RemoteConnectionOptions::Wsl(WslConnectionOptions {
                    distro_name: distro.to_string(),
                    user: None,
                }),
                wrapper: CommandTemplate {
                    program: "wsl.exe".to_string(),
                    args: vec!["--distribution".to_string(), distro.to_string()],
                    env: HashMap::default(),
                },
                refuse: false,
                built: Mutex::new(Vec::new()),
            }
        }

        fn refusing(distro: &str) -> Self {
            Self {
                refuse: true,
                ..Self::wsl(distro)
            }
        }

        fn running(program: &str, args: &[&str]) -> Self {
            Self {
                wrapper: CommandTemplate {
                    program: program.to_string(),
                    args: args.iter().map(|arg| arg.to_string()).collect(),
                    env: HashMap::default(),
                },
                ..Self::wsl("Ubuntu-22.04")
            }
        }

        fn last_built(&self) -> Built {
            self.built
                .lock()
                .last()
                .cloned()
                .expect("the host asked the connection for a command")
        }
    }

    #[async_trait(?Send)]
    impl RemoteConnection for RecordingConnection {
        fn start_proxy(
            &self,
            _unique_identifier: String,
            _reconnect: bool,
            _incoming_tx: UnboundedSender<Envelope>,
            _outgoing_rx: UnboundedReceiver<Envelope>,
            _connection_activity_tx: Sender<()>,
            _delegate: Arc<dyn RemoteClientDelegate>,
            _cx: &mut AsyncApp,
        ) -> Task<Result<i32>> {
            Task::ready(Err(anyhow!("not a real connection")))
        }

        fn upload_directory(
            &self,
            _src_path: PathBuf,
            _dest_path: RemotePathBuf,
            _cx: &App,
        ) -> Task<Result<()>> {
            Task::ready(Err(anyhow!("not a real connection")))
        }

        async fn kill(&self) -> Result<()> {
            Ok(())
        }

        fn has_been_killed(&self) -> bool {
            false
        }

        fn build_command(
            &self,
            program: Option<String>,
            args: &[String],
            env: &HashMap<String, String>,
            working_dir: Option<String>,
            _port_forward: Option<(u16, String, u16)>,
            interactive: Interactive,
        ) -> Result<CommandTemplate> {
            if self.refuse {
                return Err(anyhow!(
                    "there is no local command that reaches another Zode"
                ));
            }
            self.built.lock().push(Built {
                program,
                args: args.to_vec(),
                env: env.clone(),
                working_dir,
                interactive,
            });
            Ok(self.wrapper.clone())
        }

        fn build_forward_ports_command(
            &self,
            _forwards: Vec<(u16, String, u16)>,
        ) -> Result<CommandTemplate> {
            Err(anyhow!("not a real connection"))
        }

        fn connection_options(&self) -> RemoteConnectionOptions {
            self.options.clone()
        }

        fn path_style(&self) -> PathStyle {
            PathStyle::Posix
        }

        fn shell(&self) -> String {
            "sh".to_string()
        }

        fn default_system_shell(&self) -> String {
            "sh".to_string()
        }

        fn has_wsl_interop(&self) -> bool {
            true
        }
    }

    fn environment(variables: &[(&str, &str)]) -> HashMap<String, String> {
        variables
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    #[gpui::test]
    async fn a_local_project_runs_its_tools_on_this_machine(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
        });
        let fs = fs::FakeFs::new(cx.executor());
        let project = crate::Project::test(fs, [] as [&Path; 0], cx).await;

        let host = project
            .update(cx, |project, cx| project.command_host(cx))
            .await
            .expect("a local project always has a host");

        assert_eq!(host.remote_name(), None);
    }

    #[test]
    fn a_wsl_tool_runs_through_the_connection_with_the_distro_path_in_the_project_directory() {
        let connection = Arc::new(RecordingConnection::wsl("Ubuntu-22.04"));
        let host = RemoteCommandHost::new(
            connection.clone(),
            environment(&[(
                "PATH",
                "/home/me/.local/bin:/mnt/c/Program Files/Docker/Docker/resources/bin:/usr/bin",
            )]),
        );

        let command = host
            .command(
                "docker",
                &["ps", "-a"],
                &[],
                Some(Path::new("/home/me/app")),
            )
            .expect("a WSL connection builds commands");

        assert_eq!(
            connection.last_built(),
            Built {
                program: Some("docker".to_string()),
                args: vec!["ps".to_string(), "-a".to_string()],
                env: environment(&[("PATH", "/home/me/.local/bin:/usr/bin")]),
                working_dir: Some("/home/me/app".to_string()),
                interactive: Interactive::No,
            }
        );
        assert_eq!(command.get_program(), "wsl.exe");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["--distribution", "Ubuntu-22.04"]);
        assert_eq!(host.remote_name().as_deref(), Some("Ubuntu-22.04"));
    }

    #[test]
    fn only_the_variables_tools_read_are_carried_and_the_caller_s_reach_the_tool() {
        let connection = Arc::new(RecordingConnection::wsl("Ubuntu-22.04"));
        let host = RemoteCommandHost::new(
            connection.clone(),
            environment(&[
                ("PATH", "/usr/bin"),
                ("KUBECONFIG", "/home/me/.kube/work"),
                ("SHLVL", "2"),
                ("HOME", "/home/me"),
            ]),
        );

        host.command("git", &["status"], &[("GIT_TERMINAL_PROMPT", "0")], None)
            .expect("a WSL connection builds commands");

        let built = connection.last_built();
        assert_eq!(
            built.env,
            environment(&[
                ("PATH", "/usr/bin"),
                ("KUBECONFIG", "/home/me/.kube/work"),
                ("GIT_TERMINAL_PROMPT", "0"),
            ])
        );
        assert_eq!(built.working_dir, None);
        assert_eq!(
            host.environment_variable("HOME").as_deref(),
            Some("/home/me")
        );
    }

    struct RemoteRig {
        project: gpui::Entity<crate::Project>,
        remote_client: gpui::Entity<remote::RemoteClient>,
        _server_keepalive: gpui::Entity<()>,
    }

    async fn remote_project(
        cx: &mut gpui::TestAppContext,
        server_cx: &mut gpui::TestAppContext,
    ) -> RemoteRig {
        let fs = fs::FakeFs::new(cx.executor());
        let (project, server_keepalive) = crate::Project::test_remote(fs, cx, server_cx).await;
        let remote_client = project.read_with(cx, |project, _| {
            project
                .remote_client()
                .expect("a remote project has a remote client")
        });
        RemoteRig {
            project,
            remote_client,
            _server_keepalive: server_keepalive,
        }
    }

    async fn host_of(
        project: &gpui::Entity<crate::Project>,
        cx: &mut gpui::TestAppContext,
    ) -> Arc<dyn CommandHost> {
        project
            .update(cx, |project, cx| project.command_host(cx))
            .await
            .expect("a connected remote project has a host")
    }

    /// With no folder open there is no environment to load the user's `PATH`
    /// from; keeping that host would leave every later tool without it.
    #[gpui::test]
    async fn a_host_built_without_its_environment_is_not_kept(
        cx: &mut gpui::TestAppContext,
        server_cx: &mut gpui::TestAppContext,
    ) {
        let rig = remote_project(cx, server_cx).await;

        let without_folder = host_of(&rig.project, cx).await;
        rig.project.update(cx, |project, cx| {
            project.add_test_remote_worktree("/home/me/app", cx);
        });
        let with_folder = host_of(&rig.project, cx).await;
        let again = host_of(&rig.project, cx).await;

        assert!(without_folder.remote_name().is_some());
        assert!(
            !Arc::ptr_eq(&without_folder, &with_folder),
            "a host with no environment must not be handed out again"
        );
        assert!(
            Arc::ptr_eq(&with_folder, &again),
            "a host with its environment is kept"
        );
    }

    #[gpui::test]
    async fn a_reconnect_while_the_environment_loads_is_followed(
        cx: &mut gpui::TestAppContext,
        server_cx: &mut gpui::TestAppContext,
    ) {
        let rig = remote_project(cx, server_cx).await;
        rig.project.update(cx, |project, cx| {
            project.add_test_remote_worktree("/home/me/app", cx);
        });

        let resolving = rig
            .project
            .update(cx, |project, cx| project.command_host(cx));
        rig.remote_client.update(cx, |remote_client, cx| {
            remote_client.force_server_not_running(cx)
        });
        let host = resolving.await.expect("the host was built");

        let built = host
            .command("docker", &["ps"], &[], None)
            .err()
            .map(|error| format!("{error:#}"));
        assert_eq!(
            built
                .as_deref()
                .map(|message| message.ends_with("is not connected")),
            Some(true),
            "the connection that went away while the host was built must not be used: {built:?}"
        );
    }

    /// The ssh wrapper and the forge credentials a person keeps in their shell
    /// live on the host the tool runs on, and the wrapper's command has no rc
    /// files to find them in.
    #[test]
    fn the_host_s_git_ssh_and_forge_settings_reach_the_tool() {
        let connection = Arc::new(RecordingConnection::wsl("Ubuntu-22.04"));
        let carried = [
            ("PATH", "/usr/bin"),
            ("GIT_SSH_COMMAND", "ssh -i /home/me/.ssh/work"),
            ("GIT_SSH", "/home/me/bin/ssh-wrapper"),
            ("GH_HOST", "github.example.com"),
            ("GITLAB_HOST", "gitlab.example.com"),
        ];
        let host = RemoteCommandHost::new(connection.clone(), environment(&carried));

        host.command("git", &["fetch"], &[], None)
            .expect("a WSL connection builds commands");

        assert_eq!(connection.last_built().env, environment(&carried));
    }

    /// The forwarded variables are spelled out on the `wsl.exe` or `ssh`
    /// command line, which any process of the same user on this machine can
    /// read. A token stays where the tool keeps it on the host.
    #[test]
    fn forge_tokens_are_never_put_on_the_wrapper_s_command_line() {
        let connection = Arc::new(RecordingConnection::wsl("Ubuntu-22.04"));
        let secrets = [
            ("PATH", "/usr/bin"),
            ("GH_TOKEN", "gh-token"),
            ("GITHUB_TOKEN", "github-token"),
            ("GITLAB_TOKEN", "gitlab-token"),
        ];
        let host = RemoteCommandHost::new(connection.clone(), environment(&secrets));

        host.command("gh", &["auth", "status"], &[], None)
            .expect("a WSL connection builds commands");

        assert_eq!(
            connection.last_built().env,
            environment(&[("PATH", "/usr/bin")])
        );
    }

    /// Without the distro's own `PATH`, WSL's default would apply, and it
    /// appends the Windows one: a `docker.exe` there would act on the Windows
    /// machine.
    #[test]
    fn a_wsl_distro_whose_environment_did_not_load_still_gets_a_linux_path() {
        let connection = Arc::new(RecordingConnection::wsl("Ubuntu-22.04"));
        let host = RemoteCommandHost::new(connection.clone(), HashMap::default());

        host.command("docker", &["ps"], &[], None)
            .expect("a WSL connection builds commands");

        let built = connection.last_built();
        let path = built.env.get("PATH").expect("a PATH for the tool");
        assert_eq!(path, DEFAULT_LINUX_PATH);
        assert!(!path.contains("/mnt/"));
    }

    #[test]
    fn the_agent_socket_is_carried_from_a_wsl_distro_only() {
        let socket = [("SSH_AUTH_SOCK", "/tmp/ssh-agent.sock")];
        let wsl = Arc::new(RecordingConnection::wsl("Ubuntu-22.04"));
        let ssh = Arc::new(RecordingConnection {
            options: RemoteConnectionOptions::Ssh(remote::SshConnectionOptions {
                host: "build-box".into(),
                ..Default::default()
            }),
            ..RecordingConnection::wsl("unused")
        });

        RemoteCommandHost::new(wsl.clone(), environment(&socket))
            .command("git", &["fetch"], &[], None)
            .expect("a WSL connection builds commands");
        RemoteCommandHost::new(ssh.clone(), environment(&socket))
            .command("git", &["fetch"], &[], None)
            .expect("an SSH connection builds commands");

        assert_eq!(
            wsl.last_built().env,
            environment(&[socket[0], ("PATH", DEFAULT_LINUX_PATH)])
        );
        assert_eq!(
            ssh.last_built().env,
            HashMap::default(),
            "a long-lived SSH server's socket may be gone; the live session gives its own"
        );
    }

    #[test]
    fn a_connection_that_cannot_run_tools_says_so_and_names_the_host() {
        let host = RemoteCommandHost::new(
            Arc::new(RecordingConnection::refusing("Ubuntu-22.04")),
            HashMap::default(),
        );

        let error = host
            .command("gh", &["--version"], &[], None)
            .err()
            .expect("the connection refused to build a command");

        let message = format!("{error:#}");
        assert!(
            message.contains("gh cannot be run on Ubuntu-22.04"),
            "the message must name the tool and the host: {message}"
        );
    }

    /// The process this machine holds is the wrapper; when the caller drops it
    /// the wrapper must go, or the tool keeps running on the host.
    #[cfg(unix)]
    #[test]
    fn a_dropped_tool_process_does_not_outlive_its_handle() {
        let host = RemoteCommandHost::new(
            Arc::new(RecordingConnection::running("sleep", &["30"])),
            HashMap::default(),
        );
        let mut command = host
            .command("docker", &["events"], &[], None)
            .expect("a WSL connection builds commands");
        let child = command.spawn().expect("sleep starts");
        let process_id = child.id().to_string();
        drop(child);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut alive = true;
        while std::time::Instant::now() < deadline {
            let listed = smol::block_on(
                Command::new("ps")
                    .args(["-o", "stat=", "-p", &process_id])
                    .output(),
            )
            .expect("ps runs");
            let state = String::from_utf8_lossy(&listed.stdout);
            if state.trim().is_empty() || state.trim().starts_with('Z') {
                alive = false;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            !alive,
            "the wrapper process {process_id} outlived its handle"
        );
    }

    #[test]
    fn after_a_reconnect_commands_go_through_the_new_connection() {
        let first = Arc::new(RecordingConnection::wsl("Ubuntu-22.04"));
        let second = Arc::new(RecordingConnection::wsl("Ubuntu-22.04"));
        let host = RemoteCommandHost::new(first.clone(), HashMap::default());

        host.set_connection(None);
        let while_down = host
            .command("docker", &["ps"], &[], None)
            .err()
            .map(|error| format!("{error:#}"));
        host.set_connection(Some(second.clone()));
        host.command("docker", &["ps"], &[], None)
            .expect("the new connection builds commands");

        assert_eq!(
            while_down.as_deref(),
            Some("Ubuntu-22.04 is not connected"),
            "a dropped connection must refuse, never build through the old one"
        );
        assert!(first.built.lock().is_empty());
        assert_eq!(second.built.lock().len(), 1);
    }
}
