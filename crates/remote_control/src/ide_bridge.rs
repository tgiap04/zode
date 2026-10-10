//! The project server a controlling Zode asks this one to run.
//!
//! A device that has proven itself asks for `remote_server proxy`, and this
//! module is the pipe between that child and one stream of the encrypted
//! channel: bytes the child writes become data frames, data frames from the
//! device become bytes the child reads. What the bytes mean is the project
//! server's business; this side never parses them.
//!
//! The child and the server daemon it starts are tied to the session that
//! asked: dropping a [`IdeBridge`] kills the first and terminates the second,
//! which is what takes every terminal that session opened with it.

use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _};
use gpui::{AppContext as _, Context, Task, WeakEntity};
use remote_relay_protocol::MAX_INNER_PAYLOAD_LEN;
use smol::channel::{self, Receiver, Sender, TrySendError};
use util::command::{Stdio, new_command};

use crate::remote_host::RemoteHost;

/// The environment variable that points at a project server binary other than
/// the bundled one, for builds run from a source checkout.
pub(crate) const SERVER_PATH_VARIABLE: &str = "ZODE_REMOTE_SERVER_PATH";

/// Chunks the child may have written that the relay has not taken yet. The
/// child's pipe fills and it waits once this does, so a slow relay slows the
/// project server instead of growing this process.
const OUTPUT_QUEUE_CHUNKS: usize = 64;

/// Chunks the device may have sent that the child has not read yet. A device
/// that outruns the child by this much is not using the connection as meant.
const INPUT_QUEUE_CHUNKS: usize = 256;

/// The longest stderr line kept in a log record. A longer line is read, and
/// logged, in pieces of this size, so one without a newline cannot grow memory.
const MAX_LOGGED_LINE: usize = 2000;

/// How long the project server's daemon is given to end its terminals and
/// exit after being asked to, before it is killed.
const STOP_GRACE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdeLaunch {
    pub(crate) program: PathBuf,
    /// Where the project server keeps its per-connection state; the server
    /// derives it from the identifier it is given.
    pub(crate) state_dir: PathBuf,
}

/// Replaces where the project server is looked for, so a test can run a
/// script in its place.
#[cfg(test)]
pub(crate) struct IdeLaunchOverride(pub(crate) IdeLaunch);

#[cfg(test)]
impl gpui::Global for IdeLaunchOverride {}

/// Where the installers put the project server: beside the running executable,
/// or in `bin` beside it on Windows, where the installer already copies a
/// directory whole.
fn bundled_binary() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let directory = executable.parent()?;
    let name = if cfg!(windows) {
        "remote_server.exe"
    } else {
        "remote_server"
    };
    Some(if cfg!(windows) {
        directory.join("bin").join(name)
    } else {
        directory.join(name)
    })
}

fn existing_file(path: PathBuf) -> Option<PathBuf> {
    path.is_file().then_some(path)
}

/// The project server this Zode can run, if there is one.
#[cfg_attr(not(test), allow(unused_variables))]
pub(crate) fn locate(cx: &gpui::App) -> Option<IdeLaunch> {
    #[cfg(test)]
    if let Some(launch) = cx.try_global::<IdeLaunchOverride>() {
        return existing_file(launch.0.program.clone()).map(|program| IdeLaunch {
            program,
            state_dir: launch.0.state_dir.clone(),
        });
    }
    let program = match std::env::var_os(SERVER_PATH_VARIABLE).filter(|path| !path.is_empty()) {
        Some(path) => existing_file(PathBuf::from(path)),
        None => bundled_binary().and_then(existing_file),
    }?;
    Some(IdeLaunch {
        program,
        state_dir: paths::remote_server_state_dir().clone(),
    })
}

pub(crate) enum IdeOutput {
    Data(Vec<u8>),
    /// The child's output ended: it exited, or closed its end.
    Ended,
}

/// One session's project server.
pub(crate) struct IdeBridge {
    pub(crate) stream_id: u32,
    pub(crate) output: Receiver<IdeOutput>,
    input: Sender<Vec<u8>>,
    identifier: String,
    state_dir: PathBuf,
    spawned_at: SystemTime,
    _tasks: Vec<Task<()>>,
}

/// The device sent more than the child could take.
#[derive(Debug)]
pub(crate) struct InputOverflow;

/// A name for one project stream's server. Each stream gets its own, and a
/// random part besides: a device that reopens at once must never share a state
/// directory or pid file with a server that is still being stopped. Short,
/// because the server's socket paths live under it.
fn new_identifier(stream_id: u32) -> String {
    use std::hash::{BuildHasher as _, Hasher as _};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u32(COUNTER.fetch_add(1, Ordering::Relaxed));
    hasher.write_u32(stream_id);
    hasher.write_u32(std::process::id());
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    hasher.write_u128(nanos);
    format!("relay-{stream_id}-{:08x}", hasher.finish() as u32)
}

impl IdeBridge {
    pub(crate) fn start(
        launch: &IdeLaunch,
        stream_id: u32,
        cx: &mut Context<RemoteHost>,
    ) -> io::Result<Self> {
        let identifier = new_identifier(stream_id);
        let mut command = new_command(&launch.program);
        command
            .args([
                OsString::from("proxy"),
                OsString::from("--identifier"),
                OsString::from(&identifier),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Dropping the bridge is what stops the child, so a code path that
            // forgets to say so still cannot leave one behind.
            .kill_on_drop(true);
        let spawned_at = SystemTime::now();
        let mut child = command.spawn()?;
        let (Some(mut stdin), Some(mut stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(io::Error::other("the project server's pipes are missing"));
        };

        let (input, input_receiver) = channel::bounded::<Vec<u8>>(INPUT_QUEUE_CHUNKS);
        let (output_sender, output) = channel::bounded::<IdeOutput>(OUTPUT_QUEUE_CHUNKS);

        let writer = cx.background_spawn(async move {
            while let Ok(chunk) = input_receiver.recv().await {
                let written = async {
                    stdin.write_all(&chunk).await?;
                    stdin.flush().await
                }
                .await;
                if let Err(error) = written {
                    log::debug!("the project server stopped reading: {error}");
                    break;
                }
            }
        });

        let logger = cx.background_spawn(async move {
            let mut stderr = futures::io::BufReader::new(stderr);
            let mut line = Vec::new();
            loop {
                line.clear();
                let read = (&mut stderr)
                    .take(MAX_LOGGED_LINE as u64)
                    .read_until(b'\n', &mut line)
                    .await;
                match read {
                    Ok(0) => break,
                    Ok(_) => {
                        let shown = String::from_utf8_lossy(&line);
                        log::debug!("(project server) {}", shown.trim_end());
                    }
                    Err(error) => {
                        log::debug!("the project server's log could not be read: {error}");
                        break;
                    }
                }
            }
        });

        // Owns the child: this task being dropped is what kills it.
        let reader = cx.spawn(async move |this: WeakEntity<RemoteHost>, cx| {
            let mut buffer = vec![0u8; MAX_INNER_PAYLOAD_LEN];
            loop {
                let read = match stdout.read(&mut buffer).await {
                    Ok(0) => break,
                    Ok(read) => read,
                    Err(error) => {
                        log::debug!("the project server's output could not be read: {error}");
                        break;
                    }
                };
                let Some(chunk) = buffer.get(..read) else {
                    break;
                };
                if output_sender
                    .send(IdeOutput::Data(chunk.to_vec()))
                    .await
                    .is_err()
                {
                    return;
                }
                if this.update(cx, |host, cx| host.flush_all(cx)).is_err() {
                    return;
                }
            }
            match child.status().await {
                Ok(status) => log::debug!("the project server exited: {status}"),
                Err(error) => log::debug!("the project server's status is unknown: {error}"),
            }
            if output_sender.send(IdeOutput::Ended).await.is_err() {
                return;
            }
            if this.update(cx, |host, cx| host.flush_all(cx)).is_err() {
                log::debug!("the host went away before the project server's end was reported");
            }
        });

        Ok(Self {
            stream_id,
            output,
            input,
            identifier,
            state_dir: launch.state_dir.clone(),
            spawned_at,
            _tasks: vec![writer, logger, reader],
        })
    }

    /// Hands bytes from the device to the child, in order. A child that has
    /// already gone is not an error here: its end is reported through the
    /// output.
    pub(crate) fn send_input(&self, bytes: Vec<u8>) -> Result<(), InputOverflow> {
        match self.input.try_send(bytes) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(InputOverflow),
            Err(TrySendError::Closed(_)) => Ok(()),
        }
    }
}

impl Drop for IdeBridge {
    fn drop(&mut self) {
        stop_server(self.state_dir.join(&self.identifier), self.spawned_at);
    }
}

/// Stops the server the proxy started, which ends every terminal it holds.
///
/// The request to stop goes out before this returns, so that it is made even
/// when the process is quitting; the wait for the server to obey, the kill for
/// one that does not, and the cleanup run on a thread of their own, which does
/// not depend on any executor still being there.
fn stop_server(state_dir: PathBuf, spawned_at: SystemTime) {
    let pid_file = state_dir.join("server.pid");
    let pid = pid_written_by_this_launch(&pid_file, spawned_at);
    let asked = pid.is_some_and(|pid| {
        with_server_process(pid, &pid_file, spawned_at, |process| {
            // Termination lets the server hang up its terminals first; where
            // that signal does not exist the process is simply killed.
            if process.kill_with(sysinfo::Signal::Term).is_none() {
                process.kill();
            }
        })
        .is_some()
    });
    let finish = {
        let state_dir = state_dir.clone();
        move || {
            if let (true, Some(pid)) = (asked, pid) {
                wait_then_kill(pid, &pid_file, spawned_at);
            }
            remove_state(&state_dir);
        }
    };
    if let Err(error) = std::thread::Builder::new()
        .name("project server teardown".into())
        .spawn(finish)
    {
        log::warn!("could not start the project server's teardown thread: {error}");
        remove_state(&state_dir);
    }
}

/// The pid in the file, trusted only if this connection's own launch wrote it:
/// a file left by an earlier run names a process that may since have become
/// someone else's.
fn pid_written_by_this_launch(pid_file: &Path, spawned_at: SystemTime) -> Option<u32> {
    let written_by_this_launch = std::fs::metadata(pid_file)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| modified >= spawned_at);
    if !written_by_this_launch {
        return None;
    }
    match std::fs::read_to_string(pid_file) {
        Ok(contents) => match contents.trim().parse::<u32>() {
            Ok(pid) => Some(pid),
            Err(error) => {
                log::warn!("the project server's pid file is unreadable: {error}");
                None
            }
        },
        Err(error) => {
            log::warn!("the project server's pid file could not be read: {error}");
            None
        }
    }
}

/// Runs `act` on the process `pid` names, if it is still the server this
/// launch started and not another process that was given the number since. The
/// server's command line carries the path of this stream's own pid file, which
/// no other process has, and it cannot have started before this launch did.
fn with_server_process<R>(
    pid: u32,
    pid_file: &Path,
    spawned_at: SystemTime,
    act: impl FnOnce(&sysinfo::Process) -> R,
) -> Option<R> {
    let pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        true,
        sysinfo::ProcessRefreshKind::nothing().with_cmd(sysinfo::UpdateKind::Always),
    );
    let process = system.process(pid)?;
    let launched = spawned_at
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    // Start times have a resolution of a second.
    if process.start_time().saturating_add(1) < launched {
        log::warn!("process {pid} is older than the project server it was taken for");
        return None;
    }
    let pid_file = pid_file.to_string_lossy();
    let is_the_server = process
        .cmd()
        .iter()
        .any(|argument| argument.to_string_lossy().contains(pid_file.as_ref()));
    if !is_the_server {
        log::warn!("process {pid} is not the project server its pid file named");
        return None;
    }
    Some(act(process))
}

fn wait_then_kill(pid: u32, pid_file: &Path, spawned_at: SystemTime) {
    let started = Instant::now();
    while started.elapsed() < STOP_GRACE {
        if with_server_process(pid, pid_file, spawned_at, |_| ()).is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let killed = with_server_process(pid, pid_file, spawned_at, |process| process.kill());
    if killed == Some(false) {
        log::warn!("could not stop the project server (process {pid})");
    }
}

fn remove_state(state_dir: &Path) {
    match std::fs::remove_dir_all(state_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => log::warn!("could not clean up the project server's state: {error}"),
    }
}
