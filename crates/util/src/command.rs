use std::ffi::OsStr;
use std::path::Path;

use crate::paths::PathStyle;

#[cfg(target_os = "macos")]
mod darwin;

#[cfg(target_os = "macos")]
pub use darwin::{Child, Command, Stdio};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000_u32;

pub fn new_command(program: impl AsRef<OsStr>) -> Command {
    Command::new(program)
}

/// The machine a project lives on, as somewhere to run a tool.
///
/// A local project's tools run here; a WSL or SSH project's run on its host,
/// through a local command (`wsl.exe`, `ssh`) that reaches it. Callers build
/// every tool command through this so that a remote project never falls back
/// to the same tool on this machine.
pub trait CommandHost: Send + Sync {
    /// `program` with `args`, `env` added to its environment, started in `cwd`.
    ///
    /// `env` is a parameter rather than something set on the returned command
    /// afterwards: for a remote host the returned command is the local wrapper,
    /// and a variable set on it would reach the wrapper, not the tool.
    ///
    /// Arguments are text because a remote host's command line is. A caller
    /// with a path that is not UTF-8 passes it lossily, for this machine too:
    /// the tool then reports a file it cannot find, never acts on another.
    fn command(
        &self,
        program: &str,
        args: &[&str],
        env: &[(&str, &str)],
        cwd: Option<&Path>,
    ) -> anyhow::Result<Command>;

    fn path_style(&self) -> PathStyle;

    /// The host's name for messages, `None` for this machine.
    fn remote_name(&self) -> Option<String>;

    /// A variable from the environment tools on this host start with.
    fn environment_variable(&self, name: &str) -> Option<String>;
}

/// This machine.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalCommandHost;

impl CommandHost for LocalCommandHost {
    fn command(
        &self,
        program: &str,
        args: &[&str],
        env: &[(&str, &str)],
        cwd: Option<&Path>,
    ) -> anyhow::Result<Command> {
        let mut command = Command::new(program);
        command.args(args);
        command.envs(env.iter().copied());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        Ok(command)
    }

    fn path_style(&self) -> PathStyle {
        PathStyle::local()
    }

    fn remote_name(&self) -> Option<String> {
        None
    }

    fn environment_variable(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

#[cfg(target_os = "windows")]
pub fn new_std_command(program: impl AsRef<OsStr>) -> std::process::Command {
    use std::os::windows::process::CommandExt;

    let mut command = std::process::Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(target_os = "windows"))]
pub fn new_std_command(program: impl AsRef<OsStr>) -> std::process::Command {
    std::process::Command::new(program)
}

#[cfg(not(target_os = "macos"))]
pub type Child = smol::process::Child;

#[cfg(not(target_os = "macos"))]
pub use std::process::Stdio;

#[cfg(not(target_os = "macos"))]
#[derive(Debug)]
pub struct Command(smol::process::Command);

#[cfg(not(target_os = "macos"))]
impl Command {
    #[inline]
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        #[cfg(target_os = "windows")]
        {
            use smol::process::windows::CommandExt;
            let mut cmd = smol::process::Command::new(program);
            cmd.creation_flags(CREATE_NO_WINDOW);
            Self(cmd)
        }
        #[cfg(not(target_os = "windows"))]
        Self(smol::process::Command::new(program))
    }

    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
        self.0.arg(arg);
        self
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.0.args(args);
        self
    }

    pub fn get_args(&self) -> impl Iterator<Item = &OsStr> {
        self.0.get_args()
    }

    pub fn env(&mut self, key: impl AsRef<OsStr>, val: impl AsRef<OsStr>) -> &mut Self {
        self.0.env(key, val);
        self
    }

    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.0.envs(vars);
        self
    }

    pub fn env_remove(&mut self, key: impl AsRef<OsStr>) -> &mut Self {
        self.0.env_remove(key);
        self
    }

    pub fn env_clear(&mut self) -> &mut Self {
        self.0.env_clear();
        self
    }

    pub fn current_dir(&mut self, dir: impl AsRef<Path>) -> &mut Self {
        self.0.current_dir(dir);
        self
    }

    pub fn stdin(&mut self, cfg: impl Into<Stdio>) -> &mut Self {
        self.0.stdin(cfg.into());
        self
    }

    pub fn stdout(&mut self, cfg: impl Into<Stdio>) -> &mut Self {
        self.0.stdout(cfg.into());
        self
    }

    pub fn stderr(&mut self, cfg: impl Into<Stdio>) -> &mut Self {
        self.0.stderr(cfg.into());
        self
    }

    pub fn kill_on_drop(&mut self, kill_on_drop: bool) -> &mut Self {
        self.0.kill_on_drop(kill_on_drop);
        self
    }

    pub fn spawn(&mut self) -> std::io::Result<Child> {
        self.0.spawn()
    }

    pub async fn output(&mut self) -> std::io::Result<std::process::Output> {
        self.0.output().await
    }

    pub async fn status(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.0.status().await
    }

    pub fn get_program(&self) -> &OsStr {
        self.0.get_program()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_host_builds_the_program_and_arguments_unchanged() {
        let command = LocalCommandHost
            .command("docker", &["ps", "-a", "--format", "{{json .}}"], &[], None)
            .expect("a local command is always built");
        assert_eq!(command.get_program(), "docker");
        let args: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(args, ["ps", "-a", "--format", "{{json .}}"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_local_host_starts_the_tool_in_the_directory_with_the_environment() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let directory = std::fs::canonicalize(directory.path()).expect("canonical directory");
        let mut command = LocalCommandHost
            .command(
                "sh",
                &["-c", "pwd; printf %s \"$ZODE_COMMAND_HOST_TEST\""],
                &[("ZODE_COMMAND_HOST_TEST", "reached")],
                Some(&directory),
            )
            .expect("a local command is always built");
        let output = smol::block_on(command.output()).expect("sh runs");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(stdout, format!("{}\nreached", directory.display()));
    }

    #[test]
    fn a_local_host_reads_this_process_environment() {
        let path = std::env::var("PATH").ok();
        assert!(path.is_some(), "the test process has a PATH");
        assert_eq!(LocalCommandHost.environment_variable("PATH"), path);
        assert_eq!(LocalCommandHost.remote_name(), None);
    }
}
