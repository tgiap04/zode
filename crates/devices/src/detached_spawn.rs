use std::fs::{self, OpenOptions};
use std::path::Path;
use std::process::Stdio;

use anyhow::Context as _;

use crate::host::{DetachedExit, ToolInvocation};

/// Starts a tool that must outlive Zode, such as the Android emulator.
///
/// `util::command::Command` can neither put the child in its own process group
/// nor set Windows' detach flags, and without them quitting Zode (or a Ctrl-C
/// in the terminal that launched it) takes the emulator down too. Hence the
/// one direct use of `std::process::Command` in this crate. It blocks while
/// the process is created, so callers run it off the main thread.
#[allow(
    clippy::disallowed_methods,
    reason = "needs a process group and detach flags that util::command does not expose; callers run it in the background"
)]
pub(crate) fn spawn_detached(
    invocation: &ToolInvocation,
    log_file: &Path,
) -> anyhow::Result<DetachedExit> {
    if let Some(log_dir) = log_file.parent() {
        fs::create_dir_all(log_dir)
            .with_context(|| format!("could not create {}", log_dir.display()))?;
    }
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)
        .with_context(|| format!("could not open {}", log_file.display()))?;
    let stderr = stdout
        .try_clone()
        .with_context(|| format!("could not open {}", log_file.display()))?;

    let mut command = util::command::new_std_command(&invocation.program);
    command
        .args(&invocation.args)
        .envs(invocation.env.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("could not start {}", invocation.program.display()))?;
    let program = invocation.program.clone();
    let (exit_sender, exit) = futures::channel::oneshot::channel();
    // Reaps the child when it exits so it does not linger as a zombie, and
    // reports the exit: a tool that dies at once explains itself in its log
    // long before anything else would notice. The child is never killed here.
    std::thread::Builder::new()
        .name("detached-tool-reaper".to_string())
        .spawn(move || match child.wait() {
            Ok(status) => {
                log::debug!("{} exited with {status}", program.display());
                // Nobody may be waiting any more; that is fine.
                exit_sender.send(status.code()).ok();
            }
            Err(error) => log::warn!("could not wait for {}: {error}", program.display()),
        })
        .context("could not start the thread that waits for the tool")?;
    Ok(exit)
}
