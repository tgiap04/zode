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
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _};
use gpui::{AppContext as _, Context, Global, Task, WeakEntity};
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
/// exit after being asked to, before it is killed. Also how long a server that
/// has not yet written its pid file is waited for.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// How often the teardown thread looks for a pid file that is not there yet.
const PID_FILE_POLL: Duration = Duration::from_millis(25);

/// The file whose appearance beside the pid file asks a server to stop where
/// there is no signal to do it with. The server reads the same name.
const STOP_REQUEST_FILE: &str = "server.stop";

/// How many project servers may be in the middle of being stopped before no
/// more are started. A device that opens and closes the project stream in a
/// loop starts a server each time, and each takes a moment to go.
const MAX_SERVERS_BEING_STOPPED: usize = 4;

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

/// How many project servers are being stopped right now, on this app. One per
/// teardown thread still running.
#[derive(Default)]
struct ServersBeingStopped(Arc<AtomicUsize>);

impl Global for ServersBeingStopped {}

/// One project server being stopped, counted until this is dropped.
struct StopInProgress(Arc<AtomicUsize>);

impl StopInProgress {
    fn begin(counter: &Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(counter.clone())
    }
}

impl Drop for StopInProgress {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// What `IdeBridge::start` refuses with when too many project servers are
/// still being stopped. A kind of its own, because the kinds an `io::Error`
/// has are also what spawning a process fails with: `WouldBlock` is what a
/// full process table says.
#[derive(Debug)]
struct TooManyStopping;

impl std::fmt::Display for TooManyStopping {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("project servers are still being stopped")
    }
}

impl std::error::Error for TooManyStopping {}

/// Whether `error`, from [`IdeBridge::start`], is the refusal to start another
/// project server while too many are still being stopped, as opposed to the
/// server failing to start.
pub(crate) fn is_too_many_stopping(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<TooManyStopping>())
}

/// Whether another project server may be started while `being_stopped` are
/// still on their way out.
fn admit_launch(being_stopped: &AtomicUsize) -> io::Result<()> {
    if being_stopped.load(Ordering::SeqCst) >= MAX_SERVERS_BEING_STOPPED {
        return Err(io::Error::other(TooManyStopping));
    }
    Ok(())
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
    being_stopped: Arc<AtomicUsize>,
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
        let being_stopped = cx.default_global::<ServersBeingStopped>().0.clone();
        admit_launch(&being_stopped)?;
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
            being_stopped,
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
        stop_server(
            self.state_dir.join(&self.identifier),
            self.spawned_at,
            StopInProgress::begin(&self.being_stopped),
        );
    }
}

/// Stops the server the proxy started, which ends every terminal it holds.
///
/// The request to stop goes out before this returns when the server is already
/// known, so that it is made even when the process is quitting; the wait for
/// the server to obey, the kill for one that does not, and the cleanup run on a
/// thread of their own, which does not depend on any executor still being
/// there. So does the wait for a server that has not yet said who it is: the
/// proxy starts it, and a session that ends before it has written its pid file
/// would otherwise leave it running with nobody to stop it.
///
/// What runs before this returns is a read of the pid file and a look at that
/// one process. It cannot move to the thread: a quitting app may be gone before
/// the thread runs, and the look is what makes sure the pid still names the
/// server and not whatever was given the number since.
fn stop_server(state_dir: PathBuf, spawned_at: SystemTime, in_progress: StopInProgress) {
    let pid_file = state_dir.join("server.pid");
    let known = pid_written_by_this_launch(&pid_file, spawned_at);
    let asked = known.filter(|pid| ask_to_stop(*pid, &pid_file, spawned_at, &state_dir));
    let finish = {
        let state_dir = state_dir.clone();
        move || {
            let _in_progress = in_progress;
            let to_wait_for = match known {
                Some(_) => asked,
                None => wait_for_pid_written_by_this_launch(&pid_file, spawned_at)
                    .filter(|pid| ask_to_stop(*pid, &pid_file, spawned_at, &state_dir)),
            };
            if let Some(pid) = to_wait_for {
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

/// Asks the server `pid` names to stop, if it is still the server this launch
/// started. Whether it was asked.
fn ask_to_stop(pid: u32, pid_file: &Path, spawned_at: SystemTime, state_dir: &Path) -> bool {
    with_server_process(pid, pid_file, spawned_at, |process| {
        // Termination lets the server hang up its terminals first.
        if process.kill_with(sysinfo::Signal::Term).is_some() {
            return;
        }
        // Where there is no such signal, the server is asked in a file it
        // looks for, and killed if it does not go.
        if let Err(error) = std::fs::write(state_dir.join(STOP_REQUEST_FILE), b"") {
            log::warn!("could not ask the project server to stop: {error}");
            process.kill();
        }
    })
    .is_some()
}

/// The pid in the file, trusted only if this connection's own launch wrote it:
/// a file left by an earlier run names a process that may since have become
/// someone else's. `None` while there is nothing to trust, which includes a
/// file that is still being written.
fn pid_written_by_this_launch(pid_file: &Path, spawned_at: SystemTime) -> Option<u32> {
    let written_by_this_launch = std::fs::metadata(pid_file)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| modified >= spawned_at);
    if !written_by_this_launch {
        return None;
    }
    match std::fs::read_to_string(pid_file) {
        Ok(contents) => contents
            .trim()
            .parse::<u32>()
            .inspect_err(|error| {
                log::debug!("the project server's pid file is unreadable: {error}")
            })
            .ok(),
        Err(error) => {
            log::debug!("the project server's pid file could not be read: {error}");
            None
        }
    }
}

/// [`pid_written_by_this_launch`], looked for until [`STOP_GRACE`] has passed.
fn wait_for_pid_written_by_this_launch(pid_file: &Path, spawned_at: SystemTime) -> Option<u32> {
    let started = Instant::now();
    loop {
        if let Some(pid) = pid_written_by_this_launch(pid_file, spawned_at) {
            return Some(pid);
        }
        if started.elapsed() >= STOP_GRACE {
            log::debug!("no project server named itself in {}", pid_file.display());
            return None;
        }
        std::thread::sleep(PID_FILE_POLL);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launches_are_admitted_until_enough_servers_are_still_being_stopped() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut in_progress = Vec::new();
        for _ in 0..MAX_SERVERS_BEING_STOPPED {
            assert!(admit_launch(&counter).is_ok());
            in_progress.push(StopInProgress::begin(&counter));
        }
        let refused = admit_launch(&counter).expect_err("too many are on their way out");
        assert!(is_too_many_stopping(&refused));
        assert!(
            !is_too_many_stopping(&io::Error::from(io::ErrorKind::WouldBlock)),
            "spawning fails with WouldBlock too, which is not this"
        );
        assert!(!is_too_many_stopping(&io::Error::other("something else")));

        in_progress.pop();
        assert!(admit_launch(&counter).is_ok(), "one has gone");
        drop(in_progress);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_pid_file_from_before_the_launch_is_not_trusted() {
        let directory = tempfile::tempdir().expect("a directory");
        let pid_file = directory.path().join("server.pid");
        std::fs::write(&pid_file, "4242").expect("writes");
        let before = SystemTime::now() - Duration::from_secs(60);
        let after = SystemTime::now() + Duration::from_secs(60);
        assert_eq!(pid_written_by_this_launch(&pid_file, before), Some(4242));
        assert_eq!(pid_written_by_this_launch(&pid_file, after), None);
    }

    #[test]
    fn a_pid_file_still_being_written_is_not_yet_a_pid() {
        let directory = tempfile::tempdir().expect("a directory");
        let pid_file = directory.path().join("server.pid");
        let before = SystemTime::now() - Duration::from_secs(60);
        assert_eq!(
            pid_written_by_this_launch(&pid_file, before),
            None,
            "absent"
        );
        std::fs::write(&pid_file, "").expect("writes");
        assert_eq!(pid_written_by_this_launch(&pid_file, before), None, "empty");
    }

    /// The name the server looks for is one the server spells for itself.
    #[test]
    fn the_request_file_has_the_name_the_server_reads() {
        assert_eq!(STOP_REQUEST_FILE, "server.stop");
    }

    #[cfg(unix)]
    mod teardown {
        use super::*;
        use util::command::new_command;

        /// Like the real server it writes its pid from the process that holds
        /// its final command line, which carries the pid file's path, and it
        /// goes when asked to. It names itself once `go` exists, so a test says
        /// when and does not depend on how long anything takes.
        fn daemon_naming_itself_once(go: &Path, pid_file: &Path) -> util::command::Child {
            let script = "while [ ! -e \"$2\" ]; do sleep 0.02; done; echo $$ > \"$1\"; \
                 sleep 1000 & child=$!; trap 'kill $child; exit 0' TERM; wait $child";
            new_command("sh")
                .args(["-c", script, "daemon"])
                .arg(pid_file)
                .arg(go)
                .spawn()
                .expect("a stand-in for the server")
        }

        fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
            let give_up = Instant::now() + Duration::from_secs(20);
            while !done() {
                assert!(Instant::now() < give_up, "{what}");
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        /// The session ends in the moment between the proxy starting the server
        /// and the server saying who it is.
        #[test]
        fn a_server_that_names_itself_after_its_session_ended_is_still_stopped() {
            let directory = tempfile::tempdir().expect("a directory");
            let state_dir = directory.path().join("relay-1-abcd");
            std::fs::create_dir_all(&state_dir).expect("a state dir");
            let pid_file = state_dir.join("server.pid");
            let spawned_at = SystemTime::now();
            let go = directory.path().join("go");
            let mut daemon = daemon_naming_itself_once(&go, &pid_file);
            let being_stopped = Arc::new(AtomicUsize::new(0));

            stop_server(
                state_dir.clone(),
                spawned_at,
                StopInProgress::begin(&being_stopped),
            );
            assert!(!pid_file.exists(), "the server has not said who it is yet");
            assert_eq!(
                being_stopped.load(Ordering::SeqCst),
                1,
                "the stop is under way"
            );
            std::fs::write(&go, b"").expect("lets the server name itself");

            wait_for("the server was never stopped", || {
                daemon.try_status().expect("a status").is_some()
            });
            wait_for("the state was never cleaned up", || {
                !state_dir.exists() && being_stopped.load(Ordering::SeqCst) == 0
            });
        }

        #[test]
        fn a_server_that_has_named_itself_is_asked_to_stop_before_stop_server_returns() {
            let directory = tempfile::tempdir().expect("a directory");
            let state_dir = directory.path().join("relay-2-abcd");
            std::fs::create_dir_all(&state_dir).expect("a state dir");
            let pid_file = state_dir.join("server.pid");
            let spawned_at = SystemTime::now();
            let go = directory.path().join("go");
            std::fs::write(&go, b"").expect("lets the server name itself");
            let mut daemon = daemon_naming_itself_once(&go, &pid_file);
            wait_for("the server never named itself", || {
                std::fs::read_to_string(&pid_file).is_ok_and(|text| !text.trim().is_empty())
            });
            let being_stopped = Arc::new(AtomicUsize::new(0));

            stop_server(
                state_dir.clone(),
                spawned_at,
                StopInProgress::begin(&being_stopped),
            );

            wait_for("the server was never stopped", || {
                daemon.try_status().expect("a status").is_some()
            });
            wait_for("the state was never cleaned up", || {
                !state_dir.exists() && being_stopped.load(Ordering::SeqCst) == 0
            });
        }
    }
}
