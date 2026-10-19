//! Running git for a reader that must be able to stop it: what it prints is
//! cut off at a limit, and it is killed and collected whenever its answer is
//! no longer wanted -- because it had said enough, because something went
//! wrong, or because the caller stopped waiting.

use std::{io, process::ExitStatus};

use anyhow::{Context as _, Result, bail};
use futures::{AsyncRead, AsyncReadExt as _, future::join};
use util::{
    ResultExt as _,
    command::{Child, Command, Stdio},
};

/// How much of what git says on its error output is kept to explain a failure.
const MAX_STDERR_BYTES: usize = 4 * 1024;

#[derive(Debug)]
pub(super) struct Output {
    pub stdout: Vec<u8>,
    /// What git said on its error output, as far as it was kept.
    pub stderr: String,
    /// More was available than the limit allowed, and git was stopped. There is
    /// no exit status then.
    pub overflowed: bool,
    pub status: Option<ExitStatus>,
}

impl Output {
    /// The output, if git was stopped for saying too much or ended in success or
    /// in one of the exit codes `also_accepted`; otherwise an error that carries
    /// what git said.
    pub(super) fn ensure_succeeded(self, what: &str, also_accepted: &[i32]) -> Result<Self> {
        let Some(status) = self.status else {
            return Ok(self);
        };
        if status.success()
            || status
                .code()
                .is_some_and(|code| also_accepted.contains(&code))
        {
            return Ok(self);
        }
        bail!("git {what} exited with {status}: {}", self.stderr);
    }
}

/// A git process that is stopped and collected when this is dropped, unless it
/// was waited for first.
///
/// A process that is killed and never waited for stays in the process table
/// as a zombie, and where the platform's own child type does not wait for it
/// by itself (macOS) that is for as long as the editor runs.
struct Running {
    child: Option<Child>,
    /// Git was stopped for having said enough, which is not a reason to look
    /// for what it started.
    stopped_for_saying_enough: bool,
}

impl Running {
    fn new(child: Child) -> Self {
        Self {
            child: Some(child),
            stopped_for_saying_enough: false,
        }
    }

    fn child(&mut self) -> Result<&mut Child> {
        self.child.as_mut().context("git was already collected")
    }

    fn kill(&mut self) {
        if let Some(child) = self.child.as_mut()
            && let Err(error) = child.kill()
        {
            log::debug!("could not stop git: {error}");
        }
    }

    /// Stops git because it has printed all that is wanted of it.
    fn stop_for_saying_enough(&mut self) {
        self.stopped_for_saying_enough = true;
        self.kill();
    }

    async fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.child().map_err(io::Error::other)?.status().await;
        // Collected, or not collectable: either way there is nothing left to
        // stop, and a process id that has been given back must not be signalled.
        self.child = None;
        status
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if !self.stopped_for_saying_enough
            && let Some(child) = self.child.as_ref()
        {
            kill_descendants(child.id());
        }
        self.kill();
        let Some(mut child) = self.child.take() else {
            return;
        };
        smol::spawn(async move {
            child.status().await.log_err();
        })
        .detach();
    }
}

/// The processes below `root`, those furthest from it first, given every
/// process as `(id, parent id)`.
#[cfg(unix)]
fn descendants_furthest_first(root: u32, processes: &[(u32, u32)]) -> Vec<u32> {
    let mut found: Vec<u32> = Vec::new();
    let mut pending = vec![root];
    while let Some(parent) = pending.pop() {
        for &(id, parent_of_id) in processes {
            if parent_of_id == parent && id != root && !found.contains(&id) {
                found.push(id);
                pending.push(id);
            }
        }
    }
    // A process is found after the one that started it.
    found.reverse();
    found
}

/// Every process there is, as `(id, parent id)`; empty if that cannot be asked.
#[cfg(unix)]
#[allow(
    clippy::disallowed_methods,
    reason = "`ps` is not git, and this runs while a git is being stopped, where waiting for an executor is not an option"
)]
fn process_table() -> Vec<(u32, u32)> {
    let listing = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=", "-o", "ppid="])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    let listing = match listing {
        Ok(listing) if listing.status.success() => listing.stdout,
        Ok(listing) => {
            log::debug!("could not list processes: {}", listing.status);
            return Vec::new();
        }
        Err(error) => {
            log::debug!("could not list processes: {error}");
            return Vec::new();
        }
    };
    String::from_utf8_lossy(&listing)
        .lines()
        .filter_map(|line| {
            let mut columns = line.split_whitespace();
            let id = columns.next()?.parse().ok()?;
            let parent = columns.next()?.parse().ok()?;
            Some((id, parent))
        })
        .collect()
}

/// Kills what git started and is still running -- a `clean` filter that never
/// answers, say -- which killing git alone leaves behind for as long as it
/// lives. Before git itself is killed: once it is gone its children are
/// no one's, and there is no telling whose they were.
///
/// Best effort, and only where there is a `ps` to ask. Giving git a process
/// group of its own to signal as one would be better, but the command type
/// this runs through cannot start one.
#[cfg(unix)]
#[allow(
    clippy::disallowed_methods,
    reason = "`kill` is not git, and this runs while a git is being stopped, where waiting for an executor is not an option"
)]
fn kill_descendants(root: u32) {
    let ids = descendants_furthest_first(root, &process_table());
    if ids.is_empty() {
        return;
    }
    let killed = std::process::Command::new("sh")
        .args(["-c", "kill -KILL \"$@\" 2>/dev/null", "kill"])
        .args(ids.iter().map(u32::to_string))
        .stdin(std::process::Stdio::null())
        .status();
    if let Err(error) = killed {
        log::debug!("could not stop what git started: {error}");
    }
}

#[cfg(not(unix))]
fn kill_descendants(_root: u32) {}

/// What is left of git's error output once enough has been kept, read and thrown
/// away, so that git never waits on a full pipe.
async fn read_error_output(mut stderr: impl AsyncRead + Unpin) -> String {
    let mut kept = Vec::new();
    if let Err(error) = (&mut stderr)
        .take(MAX_STDERR_BYTES as u64)
        .read_to_end(&mut kept)
        .await
    {
        log::debug!("could not read what git said: {error}");
    } else {
        futures::io::copy(&mut stderr, &mut futures::io::sink())
            .await
            .log_err();
    }
    String::from_utf8_lossy(&kept).trim().to_string()
}

/// Runs `command` and keeps at most `limit` bytes of its output. A program that
/// has more to say is stopped rather than waited for, so its output is not
/// bounded by what it chooses to produce. Dropping this future stops the program
/// too.
pub(super) async fn run(mut command: Command, limit: usize) -> Result<Output> {
    // Not `kill_on_drop`: that would signal a process id after the process
    // behind it was collected.
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut running = Running::new(command.spawn().context("could not start git")?);
    let child = running.child()?;
    let stdout = child.stdout.take().context("git has no output to read")?;
    let stderr = child
        .stderr
        .take()
        .context("git has no error output to read")?;

    let (stdout, stderr) = {
        let read_stdout = async {
            let mut bytes = Vec::new();
            stdout
                .take(limit as u64 + 1)
                .read_to_end(&mut bytes)
                .await
                .context("could not read what git printed")?;
            if bytes.len() > limit {
                // Before the error output is waited for, which ends only when
                // git does.
                running.stop_for_saying_enough();
            }
            anyhow::Ok(bytes)
        };
        join(read_stdout, read_error_output(stderr)).await
    };
    let stdout = stdout?;
    if stdout.len() > limit {
        return Ok(Output {
            stdout,
            stderr,
            overflowed: true,
            status: None,
        });
    }
    let status = running.wait().await.context("could not wait for git")?;
    Ok(Output {
        stdout,
        stderr,
        overflowed: false,
        status: Some(status),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use std::{path::Path, time::Duration};

    use gpui::TestAppContext;
    use util::command::new_command;

    use super::*;

    #[allow(
        clippy::disallowed_methods,
        reason = "the program under test is a shell script standing in for git, which the crate's rule about git commands does not mean"
    )]
    fn shell(script: &str) -> Command {
        let mut command = new_command("sh");
        command.args(["-c", script]);
        command
    }

    /// Whether a process of this id is in the process table at all, a zombie
    /// included.
    #[allow(
        clippy::disallowed_methods,
        reason = "`ps` is a read-only query in a test, where blocking is harmless"
    )]
    fn is_in_the_process_table(pid: &str) -> bool {
        let listing = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", pid])
            .output()
            .expect("ps runs");
        !String::from_utf8_lossy(&listing.stdout).trim().is_empty()
    }

    fn wait_until_gone(pid_file: &Path) {
        let pid = std::fs::read_to_string(pid_file).expect("the program wrote its pid");
        let pid = pid.trim();
        let give_up = std::time::Instant::now() + Duration::from_secs(10);
        while is_in_the_process_table(pid) {
            assert!(
                std::time::Instant::now() < give_up,
                "process {pid} was stopped and never collected"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// A program with no end to what it prints is stopped at the limit; an
    /// unbounded read would never return.
    #[gpui::test]
    async fn a_program_that_never_stops_printing_is_stopped_at_the_limit(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let output = run(shell("exec yes"), 1000).await.expect("collected");
        assert!(output.overflowed);
        assert_eq!(output.stdout.len(), 1001);
        assert!(output.status.is_none());
    }

    /// Where nobody collects it, a stopped program stays in the process table
    /// as a zombie for as long as the editor runs.
    #[gpui::test]
    async fn a_program_stopped_for_printing_too_much_is_collected(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let directory = tempfile::tempdir().expect("a directory");
        let pid_file = directory.path().join("pid");
        let script = format!("echo $$ > '{}'; exec yes", pid_file.display());
        let output = run(shell(&script), 1000).await.expect("collected");
        assert!(output.overflowed);
        wait_until_gone(&pid_file);
    }

    #[gpui::test]
    async fn a_program_that_is_no_longer_waited_for_is_stopped_and_collected(
        cx: &mut TestAppContext,
    ) {
        cx.executor().allow_parking();
        let directory = tempfile::tempdir().expect("a directory");
        let pid_file = directory.path().join("pid");
        let script = format!("echo $$ > '{}'; exec sleep 1000", pid_file.display());
        let mut waiting = Box::pin(run(shell(&script), 1000));
        let give_up = std::time::Instant::now() + Duration::from_secs(10);
        while !pid_file.exists() {
            assert!(std::time::Instant::now() < give_up, "the program never ran");
            // Polled once and found not finished: this is what a caller that
            // goes away leaves behind.
            assert!(futures::poll!(waiting.as_mut()).is_pending());
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(waiting);
        wait_until_gone(&pid_file);
    }

    #[test]
    fn what_a_process_started_is_found_below_it_and_before_it() {
        let table = [
            (1, 0),
            (10, 1),
            (11, 10),
            (12, 10),
            (13, 12),
            (20, 1),
            (21, 20),
            // Its own parent: a table that is wrong must not loop.
            (30, 30),
        ];
        let found = descendants_furthest_first(10, &table);
        assert_eq!(found.len(), 3, "{found:?}");
        for (child, parent) in [(13, 12), (12, 10), (11, 10)] {
            let child_at = found.iter().position(|id| *id == child);
            let parent_at = found.iter().position(|id| *id == parent);
            assert!(child_at.is_some(), "{child} in {found:?}");
            if let Some(parent_at) = parent_at {
                assert!(
                    child_at < Some(parent_at),
                    "{child} before {parent}: {found:?}"
                );
            }
        }
        assert!(!found.contains(&10), "not the root itself");
        assert!(descendants_furthest_first(21, &table).is_empty());
        assert!(descendants_furthest_first(30, &table).is_empty());
        assert!(descendants_furthest_first(99, &table).is_empty());
    }

    /// A `clean` filter that never answers is what git is waiting on when it
    /// is stopped, and it is not git's to be stopped by.
    #[gpui::test]
    async fn what_git_started_is_stopped_with_it(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let directory = tempfile::tempdir().expect("a directory");
        let shell_file = directory.path().join("shell");
        let child_file = directory.path().join("child");
        let script = format!(
            "echo $$ > '{}'; sleep 1000 & echo $! > '{}'; wait",
            shell_file.display(),
            child_file.display()
        );
        let mut waiting = Box::pin(run(shell(&script), 1000));
        let give_up = std::time::Instant::now() + Duration::from_secs(10);
        let written =
            |file: &Path| std::fs::read_to_string(file).is_ok_and(|text| !text.trim().is_empty());
        while !(written(&shell_file) && written(&child_file)) {
            assert!(std::time::Instant::now() < give_up, "the program never ran");
            assert!(futures::poll!(waiting.as_mut()).is_pending());
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(waiting);
        wait_until_gone(&shell_file);
        wait_until_gone(&child_file);
    }

    #[gpui::test]
    async fn what_git_says_on_its_error_output_comes_back_with_the_exit_status(
        cx: &mut TestAppContext,
    ) {
        cx.executor().allow_parking();
        let output = run(shell("echo 'fatal: no such thing' >&2; exit 3"), 1000)
            .await
            .expect("collected");
        assert_eq!(output.status.and_then(|status| status.code()), Some(3));
        let error = output
            .ensure_succeeded("test", &[])
            .expect_err("an exit code of 3 is a failure");
        assert!(
            format!("{error:#}").contains("fatal: no such thing"),
            "{error:#}"
        );
    }

    #[gpui::test]
    async fn an_exit_code_the_caller_expects_is_not_a_failure(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let output = run(shell("echo different; exit 1"), 1000)
            .await
            .expect("collected");
        assert!(output.ensure_succeeded("test", &[]).is_err());
        let output = run(shell("echo different; exit 1"), 1000)
            .await
            .expect("collected");
        let output = output.ensure_succeeded("test", &[1]).expect("expected");
        assert_eq!(output.stdout, b"different\n");
    }

    /// Git does not exit until its error output has been taken, so what is not
    /// kept is still read.
    #[gpui::test]
    async fn a_flood_of_error_output_neither_blocks_the_program_nor_is_kept(
        cx: &mut TestAppContext,
    ) {
        cx.executor().allow_parking();
        let output = run(
            shell("head -c 2000000 /dev/zero | tr '\\0' 'e' >&2; echo done"),
            1000,
        )
        .await
        .expect("collected");
        assert_eq!(output.stdout, b"done\n");
        assert_eq!(output.stderr.len(), MAX_STDERR_BYTES);
        assert!(output.status.is_some_and(|status| status.success()));
    }
}
