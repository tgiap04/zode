//! Terminals the remote server runs for a client that cannot start a local
//! process attached to one -- the relay has no `ssh` to run -- so a pty here
//! stands in for it and its bytes travel as messages.
//!
//! No terminal emulator sits in between: the output is the child's own bytes,
//! delivered as they were written, and the client's emulator is what draws and
//! answers terminal queries. The alacritty event loop is used only for the
//! parts that are platform-specific and easy to get wrong (polling the pty,
//! partial writes, resizing, noticing the child exit); its own screen model is
//! never read and keeps no scrollback.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use alacritty_terminal::{
    event::{Event as TerminalEvent, EventListener, WindowSize},
    event_loop::{EventLoop, EventLoopSender, Msg},
    grid::Dimensions,
    sync::FairMutex,
    term::{Config, Term},
    tty,
};
use anyhow::{Context as _, Result, anyhow};
use gpui::Context;
#[cfg(unix)]
use parking_lot::Condvar;
use parking_lot::Mutex;
use rpc::{AnyProtoClient, proto};
use smol::channel::{Receiver, bounded};
use util::ResultExt as _;

use crate::HeadlessProject;

/// Live terminals one client may hold at once. A client that wants more is
/// told so, rather than being allowed to exhaust the host's processes.
pub const MAX_TERMINALS_PER_CLIENT: usize = 16;

/// How long a closed terminal's child may take to go after the hang-up.
#[cfg(unix)]
pub(crate) const REAP_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// The largest width or height a client may give a terminal. A pty has no use
/// for more, and the kernel and the child's own tables grow with it.
const MAX_PTY_DIMENSION: u16 = 1000;

/// Input accepted for a terminal whose child has not read it yet. A child that
/// stops reading must not make this server buffer a client's typing, or a
/// paste, without end; the terminal is closed instead.
const MAX_PENDING_INPUT_BYTES: usize = 4 * 1024 * 1024;

/// The size of the emulator behind the event loop. Its screen is never read,
/// so it stays small whatever size the client asks the pty to be.
const EMULATOR_SIZE: Size = Size {
    columns: 80,
    rows: 24,
};

/// Watcher threads still running, so that a test can wait for every child to be
/// gone and collected before the process exits: a thread still unwinding while
/// the process tears down its statics is a crash of the test binary.
#[cfg(test)]
pub(crate) static LIVE_WATCHERS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
struct WatcherCount;

#[cfg(test)]
impl WatcherCount {
    fn new() -> Self {
        LIVE_WATCHERS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

#[cfg(test)]
impl Drop for WatcherCount {
    fn drop(&mut self) {
        LIVE_WATCHERS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The most bytes sent in one `TerminalOutput`.
const OUTPUT_BATCH_BYTES: usize = 64 * 1024;

/// Chunks that may wait for the client to take the previous batch. When they
/// are all taken the reader thread blocks, the pty fills up, and the child's
/// writes block: a client that cannot keep up slows the process down instead
/// of making the server buffer without limit.
const OUTPUT_QUEUE_CHUNKS: usize = 16;

enum Pumped {
    Output(Vec<u8>),
    Exited(Option<i32>),
}

struct Size {
    columns: usize,
    rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// Remembers how the child ended; everything else the emulator reports is for
/// the client to act on.
#[derive(Clone, Default)]
struct ExitRecorder {
    status: Arc<Mutex<Option<i32>>>,
    /// Set the moment the event loop has collected the child, after which its
    /// pid may belong to anyone and must not be signalled.
    reaped: Arc<AtomicBool>,
}

impl EventListener for ExitRecorder {
    fn send_event(&self, event: TerminalEvent) {
        if let TerminalEvent::ChildExit(raw_status) = event {
            *self.status.lock() = Some(raw_status);
            self.reaped.store(true, Ordering::SeqCst);
        }
    }
}

fn exit_code(raw_status: i32) -> Option<i32> {
    #[cfg(unix)]
    {
        <std::process::ExitStatus as std::os::unix::process::ExitStatusExt>::from_raw(raw_status)
            .code()
    }
    #[cfg(windows)]
    {
        Some(raw_status)
    }
}

/// Waits for a hung-up child to be gone and collects it. The event loop reaps
/// a child that exits by itself, but one it was told to abandon would stay a
/// zombie for as long as this long-lived server runs. A child that ignores the
/// hang-up is killed after a grace period.
#[cfg(unix)]
fn reap_child(pid: i32) {
    use std::time::{Duration, Instant};

    const POLL: Duration = Duration::from_millis(25);
    const GRACE: Duration = REAP_GRACE;
    const GIVE_UP: Duration = Duration::from_secs(10);

    let started = Instant::now();
    let mut killed = false;
    loop {
        let mut status = 0;
        // SAFETY: `waitpid` only reads the pid and writes the status.
        match unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) } {
            // Still running.
            0 => {}
            -1 if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted => {
                continue;
            }
            // Collected, or not ours to collect any more: the event loop got
            // to it first.
            _ => return,
        }
        let waited = started.elapsed();
        if waited >= GIVE_UP {
            log::warn!("terminal process {pid} would not end");
            return;
        }
        if waited >= GRACE && !killed {
            killed = true;
            // SAFETY: the `waitpid` above has just reported this pid as a
            // child of ours that is still running, so it has not been
            // collected and cannot have been given to another process.
            if unsafe { libc::kill(pid, libc::SIGKILL) } != 0 {
                log::debug!("terminal process {pid} was already gone");
            }
        }
        std::thread::sleep(POLL);
    }
}

/// Kills the child of a closed terminal that has not gone within the grace
/// period. Stops looking as soon as the child is reported collected, whether by
/// the event loop or by the watcher, so a pid that has been given to some other
/// process is not signalled.
#[cfg(unix)]
fn kill_if_it_lingers(pid: i32, reaped: Arc<AtomicBool>) {
    use std::time::{Duration, Instant};

    let spawned = std::thread::Builder::new()
        .name(format!("terminal {pid} killer"))
        .spawn(move || {
            let started = Instant::now();
            while started.elapsed() < REAP_GRACE {
                if reaped.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            if reaped.load(Ordering::SeqCst) {
                return;
            }
            // SAFETY: nothing has recorded this child as collected, so the pid
            // still names it. Collection and this check are not atomic, but the
            // flag is set the instant either collector returns, which leaves a
            // window of a few instructions rather than the whole grace period.
            if unsafe { libc::kill(pid, libc::SIGKILL) } != 0 {
                log::debug!("terminal process {pid} was already gone");
            }
        });
    if let Err(error) = spawned {
        log::warn!("could not start the watcher for terminal process {pid}: {error}");
    }
}

fn window_size(columns: u32, rows: u32) -> WindowSize {
    WindowSize {
        num_cols: u16::try_from(columns)
            .unwrap_or(u16::MAX)
            .clamp(1, MAX_PTY_DIMENSION),
        num_lines: u16::try_from(rows)
            .unwrap_or(u16::MAX)
            .clamp(1, MAX_PTY_DIMENSION),
        cell_width: 1,
        cell_height: 1,
    }
}

/// Bytes for the child that it has not taken yet, written to the pty by a
/// thread of its own so that the amount waiting is known and can be capped: the
/// event loop's own queue cannot be seen from outside.
#[cfg(unix)]
struct InputFeeder {
    shared: Arc<FeederShared>,
}

#[cfg(unix)]
#[derive(Default)]
struct FeederShared {
    queue: Mutex<InputQueue>,
    wake: Condvar,
}

#[cfg(unix)]
#[derive(Default)]
struct InputQueue {
    bytes: std::collections::VecDeque<u8>,
    closed: bool,
}

/// The child has more waiting than it is allowed to.
struct InputOverflow;

#[cfg(unix)]
impl FeederShared {
    fn push(&self, data: &[u8]) -> Result<(), InputOverflow> {
        let mut queue = self.queue.lock();
        if queue.closed {
            return Ok(());
        }
        if queue.bytes.len().saturating_add(data.len()) > MAX_PENDING_INPUT_BYTES {
            return Err(InputOverflow);
        }
        queue.bytes.extend(data);
        self.wake.notify_one();
        Ok(())
    }

    fn close(&self) {
        self.queue.lock().closed = true;
        self.wake.notify_all();
    }
}

#[cfg(unix)]
impl InputFeeder {
    fn start(master: &std::fs::File, terminal_id: u64) -> Result<Self> {
        let file = master
            .try_clone()
            .context("duplicating the terminal's descriptor")?;
        let shared = Arc::new(FeederShared::default());
        std::thread::Builder::new()
            .name(format!("terminal {terminal_id} input"))
            .spawn({
                let shared = shared.clone();
                move || Self::run(&shared, &file)
            })
            .context("starting the terminal's input thread")?;
        Ok(Self { shared })
    }

    fn run(shared: &FeederShared, master: &std::fs::File) {
        use std::io::{ErrorKind, Write as _};
        const WRITE_CHUNK: usize = 16 * 1024;
        let mut master = master;
        loop {
            let chunk: Vec<u8> = {
                let mut queue = shared.queue.lock();
                loop {
                    if queue.closed {
                        return;
                    }
                    if !queue.bytes.is_empty() {
                        break;
                    }
                    shared.wake.wait(&mut queue);
                }
                queue.bytes.iter().take(WRITE_CHUNK).copied().collect()
            };
            match master.write(&chunk) {
                Ok(0) => {
                    shared.close();
                    return;
                }
                Ok(written) => {
                    let mut queue = shared.queue.lock();
                    let drained = written.min(queue.bytes.len());
                    queue.bytes.drain(..drained);
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    wait_until_writable(master);
                }
                Err(error) => {
                    log::debug!("the terminal's process no longer takes input: {error}");
                    shared.close();
                    return;
                }
            }
        }
    }
}

#[cfg(unix)]
impl Drop for InputFeeder {
    fn drop(&mut self) {
        self.shared.close();
    }
}

/// Waits a short while for the pty to take more; short so that a closed
/// terminal's thread is noticed and ended promptly.
#[cfg(unix)]
fn wait_until_writable(master: &std::fs::File) {
    use std::os::fd::AsRawFd as _;
    let mut descriptor = libc::pollfd {
        fd: master.as_raw_fd(),
        events: libc::POLLOUT,
        revents: 0,
    };
    // SAFETY: `descriptor` is one valid `pollfd`, and the count says so.
    // Whatever poll reports, the caller retries the write and learns the
    // outcome from that.
    unsafe { libc::poll(&mut descriptor, 1, 100) };
}

#[cfg(unix)]
struct SetOnDrop(Arc<AtomicBool>);

#[cfg(unix)]
impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct LiveTerminal {
    sender: EventLoopSender,
    #[cfg(unix)]
    input: InputFeeder,
    /// The child, until the watcher has collected it. A child that ignores the
    /// hang-up would otherwise block the event loop's end, and with it the
    /// watcher, for as long as it lives.
    #[cfg(unix)]
    child_pid: Option<i32>,
    /// Set once the child has been collected, by the event loop or by the
    /// watcher.
    #[cfg_attr(not(unix), allow(dead_code))]
    reaped: Arc<AtomicBool>,
    /// Set when the client asked for the terminal to go away, so that the exit
    /// it causes is not reported back as if the process had ended on its own.
    closed: Arc<AtomicBool>,
    project_id: u64,
    session: AnyProtoClient,
}

impl LiveTerminal {
    fn send(&self, message: Msg) {
        // The loop having ended already is the usual way to get here: the
        // process exited before the client closed the terminal.
        if self.sender.send(message).is_err() {
            log::debug!("the terminal's event loop has already ended");
        }
    }
}

impl Drop for LiveTerminal {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        // Ending the loop drops the pty, which hangs up the child.
        self.send(Msg::Shutdown);
        #[cfg(unix)]
        if let Some(pid) = self.child_pid {
            kill_if_it_lingers(pid, self.reaped.clone());
        }
    }
}

#[derive(Default)]
pub struct HeadlessTerminals {
    live: HashMap<u64, LiveTerminal>,
}

impl HeadlessTerminals {
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.live.len()
    }

    pub fn create(
        &mut self,
        request: proto::CreateTerminal,
        session: AnyProtoClient,
        cx: &mut Context<HeadlessProject>,
    ) -> Result<()> {
        let terminal_id = request.terminal_id;
        if self.live.contains_key(&terminal_id) {
            return Err(anyhow!("terminal {terminal_id} already exists"));
        }
        if self.live.len() >= MAX_TERMINALS_PER_CLIENT {
            return Err(anyhow!(
                "too many terminals: at most {MAX_TERMINALS_PER_CLIENT} may be open at once"
            ));
        }

        let size = window_size(request.columns, request.rows);
        let options = tty::Options {
            shell: request
                .program
                .map(|program| tty::Shell::new(program, request.args)),
            working_directory: request.cwd.map(PathBuf::from),
            // Output the child wrote just before it exited is still delivered.
            drain_on_exit: true,
            env: request.env.into_iter().collect(),
            #[cfg(windows)]
            escape_args: false,
        };
        // Dropping a pty waits for its child, and building what the pty goes
        // into can fail, so a child that ignores the hang-up must not be able
        // to hold this thread: it is killed after the grace period unless
        // everything below succeeded first. Declared before the pty so that it
        // is dropped after it on every way out.
        #[cfg(unix)]
        let construction_done = Arc::new(AtomicBool::new(false));
        #[cfg(unix)]
        let _construction_done = SetOnDrop(construction_done.clone());
        let pty = tty::new(&options, size, terminal_id).context("starting the terminal process")?;
        #[cfg(unix)]
        let child_pid = i32::try_from(pty.child().id()).ok();
        #[cfg(unix)]
        if let Some(pid) = child_pid {
            kill_if_it_lingers(pid, construction_done);
        }
        #[cfg(unix)]
        let input = InputFeeder::start(pty.file(), terminal_id)?;
        #[cfg(unix)]
        let input_for_watcher = input.shared.clone();

        let recorder = ExitRecorder::default();
        let reaped = recorder.reaped.clone();
        let term = Arc::new(FairMutex::new(Term::new(
            Config {
                scrolling_history: 0,
                ..Config::default()
            },
            &EMULATOR_SIZE,
            recorder.clone(),
        )));
        let mut event_loop = EventLoop::new(term, recorder.clone(), pty, true, false)
            .context("starting the terminal's event loop")?;

        let (output_tx, output_rx) = bounded::<Pumped>(OUTPUT_QUEUE_CHUNKS);
        event_loop.set_output_tap(Some(Box::new({
            let output_tx = output_tx.clone();
            move |bytes| {
                for chunk in bytes.chunks(OUTPUT_BATCH_BYTES) {
                    // Blocking here is the backpressure described on
                    // `OUTPUT_QUEUE_CHUNKS`. A closed channel means the
                    // terminal was closed and nobody wants the bytes.
                    if output_tx
                        .send_blocking(Pumped::Output(chunk.to_vec()))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        })));
        let sender = event_loop.channel();
        let loop_thread = event_loop.spawn();

        let closed = Arc::new(AtomicBool::new(false));
        let project_id = request.project_id;
        // Entered before the watcher exists, so that if the watcher cannot be
        // started, removing the entry is what ends the loop and its child.
        self.live.insert(
            terminal_id,
            LiveTerminal {
                sender,
                #[cfg(unix)]
                input,
                #[cfg(unix)]
                child_pid,
                reaped: reaped.clone(),
                closed: closed.clone(),
                project_id,
                session: session.clone(),
            },
        );

        #[cfg(test)]
        let watcher_count = WatcherCount::new();
        let watcher = std::thread::Builder::new()
            .name(format!("terminal {terminal_id} watcher"))
            .spawn({
                let closed = closed.clone();
                move || {
                    #[cfg(test)]
                    let _watcher_count = watcher_count;
                    // Joining drops the pty, and with it the child's session.
                    if loop_thread.join().is_err() {
                        log::error!("the event loop of terminal {terminal_id} panicked");
                    }
                    // Dropping the pty waited for the child, so it is collected.
                    reaped.store(true, Ordering::SeqCst);
                    #[cfg(unix)]
                    input_for_watcher.close();
                    #[cfg(unix)]
                    if let Some(child_pid) = child_pid {
                        reap_child(child_pid);
                    }
                    if closed.load(Ordering::SeqCst) {
                        return;
                    }
                    let code = recorder.status.lock().take().and_then(exit_code);
                    // After the last output in the queue, so that nothing the
                    // process wrote is reported after it was said to be over.
                    output_tx.send_blocking(Pumped::Exited(code)).ok();
                }
            });
        if let Err(error) = watcher {
            self.live.remove(&terminal_id);
            return Err(anyhow!(error).context("starting the terminal's watcher thread"));
        }

        cx.spawn(async move |this, cx| {
            Self::pump(project_id, terminal_id, output_rx, session, closed.clone()).await;
            // The id may have been reused by a terminal made after this one
            // was closed, which must be left alone.
            this.update(cx, |this, _| {
                let is_ours = this
                    .terminals
                    .live
                    .get(&terminal_id)
                    .is_some_and(|live| Arc::ptr_eq(&live.closed, &closed));
                if is_ours {
                    this.terminals.live.remove(&terminal_id);
                }
            })
            .ok();
        })
        .detach();
        Ok(())
    }

    /// Sends the process's output to the client, one batch at a time, and then
    /// says it exited. Stops as soon as the terminal is closed: nobody wants
    /// what is still queued.
    async fn pump(
        project_id: u64,
        terminal_id: u64,
        output: Receiver<Pumped>,
        session: AnyProtoClient,
        closed: Arc<AtomicBool>,
    ) {
        let mut pending_exit = None;
        while pending_exit.is_none() {
            let Ok(first) = output.recv().await else {
                return;
            };
            if closed.load(Ordering::SeqCst) {
                return;
            }
            let mut batch = match first {
                Pumped::Output(bytes) => bytes,
                Pumped::Exited(code) => {
                    pending_exit = Some(code);
                    Vec::new()
                }
            };
            while pending_exit.is_none() && batch.len() < OUTPUT_BATCH_BYTES {
                match output.try_recv() {
                    Ok(Pumped::Output(bytes)) => batch.extend_from_slice(&bytes),
                    Ok(Pumped::Exited(code)) => pending_exit = Some(code),
                    Err(_) => break,
                }
            }
            if !batch.is_empty() {
                let sent = session
                    .request(proto::TerminalOutput {
                        project_id,
                        terminal_id,
                        data: batch,
                    })
                    .await;
                if let Err(error) = sent {
                    log::warn!(
                        "terminal {terminal_id}: the client stopped taking output: {error:#}"
                    );
                    return;
                }
            }
        }
        if closed.load(Ordering::SeqCst) {
            return;
        }
        if let Some(exit_code) = pending_exit {
            session
                .send(proto::TerminalExited {
                    project_id,
                    terminal_id,
                    exit_code,
                })
                .log_err();
        }
    }

    /// Hands the client's typing to the process. A process that has left more
    /// than [`MAX_PENDING_INPUT_BYTES`] unread is not reading at all, and the
    /// terminal is closed rather than buffering for it.
    pub fn input(&mut self, terminal_id: u64, data: Vec<u8>) {
        let Some(terminal) = self.live.get(&terminal_id) else {
            log::debug!("input for terminal {terminal_id}, which is not open");
            return;
        };
        // The event loop never writes an empty message.
        if data.is_empty() {
            return;
        }
        #[cfg(unix)]
        let accepted = terminal.input.shared.push(&data);
        #[cfg(not(unix))]
        let accepted: Result<(), InputOverflow> = {
            terminal.send(Msg::Input(std::borrow::Cow::Owned(data)));
            Ok(())
        };
        if accepted.is_err() {
            log::warn!(
                "terminal {terminal_id}: its process left over {MAX_PENDING_INPUT_BYTES} bytes of input unread; closing it"
            );
            let (project_id, session) = (terminal.project_id, terminal.session.clone());
            self.live.remove(&terminal_id);
            session
                .send(proto::TerminalExited {
                    project_id,
                    terminal_id,
                    exit_code: None,
                })
                .log_err();
        }
    }

    pub fn resize(&self, terminal_id: u64, columns: u32, rows: u32) {
        match self.live.get(&terminal_id) {
            Some(terminal) => terminal.send(Msg::Resize(window_size(columns, rows))),
            None => log::debug!("resize of terminal {terminal_id}, which is not open"),
        }
    }

    /// Ends the terminal's process. Closing one that is already gone is not an
    /// error: the client and the process can end at the same moment.
    pub fn close(&mut self, terminal_id: u64) {
        self.live.remove(&terminal_id);
    }

    /// Ends every terminal's process, for a server that is being told to stop.
    /// The flags say, one per terminal, when its child has been collected.
    pub fn close_all(&mut self) -> Vec<Arc<AtomicBool>> {
        self.live
            .drain()
            .map(|(_, terminal)| terminal.reaped.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_requested_size_is_kept_within_what_a_pty_can_use() {
        let huge = window_size(u32::MAX, u32::MAX);
        assert_eq!((huge.num_cols, huge.num_lines), (1000, 1000));
        let none = window_size(0, 0);
        assert_eq!((none.num_cols, none.num_lines), (1, 1));
        let ordinary = window_size(120, 40);
        assert_eq!((ordinary.num_cols, ordinary.num_lines), (120, 40));
    }

    #[cfg(unix)]
    #[test]
    fn input_the_process_has_not_taken_is_capped() {
        let shared = FeederShared::default();
        let chunk = vec![0u8; 1024 * 1024];
        for _ in 0..4 {
            assert!(shared.push(&chunk).is_ok());
        }
        assert!(
            shared.push(b"x").is_err(),
            "one byte over the cap is refused"
        );
        assert_eq!(
            shared.queue.lock().bytes.len(),
            4 * 1024 * 1024,
            "a refused push adds nothing"
        );
        shared.close();
        assert!(
            shared.push(&chunk).is_ok(),
            "a closed terminal ignores input"
        );
    }
}
