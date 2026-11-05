use mach2::exception_types::{
    EXC_MASK_ALL, EXCEPTION_DEFAULT, exception_behavior_t, exception_mask_t,
};
use mach2::port::{MACH_PORT_NULL, mach_port_t};
use mach2::thread_status::{THREAD_STATE_NONE, thread_state_flavor_t};
use smol::Unblock;
use std::collections::BTreeMap;
use std::ffi::{CString, OsStr, OsString};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::FromRawFd;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};
use std::ptr;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stdio {
    /// A new pipe should be arranged to connect the parent and child processes.
    #[default]
    Piped,
    /// The child inherits from the corresponding parent descriptor.
    Inherit,
    /// This stream will be ignored (redirected to `/dev/null`).
    Null,
}

impl Stdio {
    pub fn piped() -> Self {
        Self::Piped
    }

    pub fn inherit() -> Self {
        Self::Inherit
    }

    pub fn null() -> Self {
        Self::Null
    }
}

unsafe extern "C" {
    fn posix_spawnattr_setexceptionports_np(
        attr: *mut libc::posix_spawnattr_t,
        mask: exception_mask_t,
        new_port: mach_port_t,
        behavior: exception_behavior_t,
        new_flavor: thread_state_flavor_t,
    ) -> libc::c_int;

    fn posix_spawn_file_actions_addchdir_np(
        file_actions: *mut libc::posix_spawn_file_actions_t,
        path: *const libc::c_char,
    ) -> libc::c_int;

    fn posix_spawn_file_actions_addinherit_np(
        file_actions: *mut libc::posix_spawn_file_actions_t,
        filedes: libc::c_int,
    ) -> libc::c_int;

    static environ: *const *mut libc::c_char;
}

#[derive(Debug)]
pub struct Command {
    program: OsString,
    args: Vec<OsString>,
    envs: BTreeMap<OsString, Option<OsString>>,
    env_clear: bool,
    current_dir: Option<PathBuf>,
    stdin_cfg: Option<Stdio>,
    stdout_cfg: Option<Stdio>,
    stderr_cfg: Option<Stdio>,
    kill_on_drop: bool,
}

impl Command {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().to_owned(),
            args: Vec::new(),
            envs: BTreeMap::new(),
            env_clear: false,
            current_dir: None,
            stdin_cfg: None,
            stdout_cfg: None,
            stderr_cfg: None,
            kill_on_drop: false,
        }
    }

    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
        self.args.push(arg.as_ref().to_owned());
        self
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|a| a.as_ref().to_owned()));
        self
    }

    pub fn get_args(&self) -> impl Iterator<Item = &OsStr> {
        self.args.iter().map(|s| s.as_os_str())
    }

    pub fn env(&mut self, key: impl AsRef<OsStr>, val: impl AsRef<OsStr>) -> &mut Self {
        self.envs
            .insert(key.as_ref().to_owned(), Some(val.as_ref().to_owned()));
        self
    }

    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        for (key, val) in vars {
            self.envs
                .insert(key.as_ref().to_owned(), Some(val.as_ref().to_owned()));
        }
        self
    }

    pub fn env_remove(&mut self, key: impl AsRef<OsStr>) -> &mut Self {
        let key = key.as_ref().to_owned();
        if self.env_clear {
            self.envs.remove(&key);
        } else {
            self.envs.insert(key, None);
        }
        self
    }

    pub fn env_clear(&mut self) -> &mut Self {
        self.env_clear = true;
        self.envs.clear();
        self
    }

    pub fn current_dir(&mut self, dir: impl AsRef<Path>) -> &mut Self {
        self.current_dir = Some(dir.as_ref().to_owned());
        self
    }

    pub fn stdin(&mut self, cfg: Stdio) -> &mut Self {
        self.stdin_cfg = Some(cfg);
        self
    }

    pub fn stdout(&mut self, cfg: Stdio) -> &mut Self {
        self.stdout_cfg = Some(cfg);
        self
    }

    pub fn stderr(&mut self, cfg: Stdio) -> &mut Self {
        self.stderr_cfg = Some(cfg);
        self
    }

    pub fn kill_on_drop(&mut self, kill_on_drop: bool) -> &mut Self {
        self.kill_on_drop = kill_on_drop;
        self
    }

    pub fn spawn(&mut self) -> io::Result<Child> {
        let current_dir = self
            .current_dir
            .as_deref()
            .unwrap_or_else(|| Path::new("."));

        // Optimization: if no environment modifications were requested, pass None
        // to spawn_posix so it uses the `environ` global directly, avoiding a
        // full copy of the environment. This matches std::process::Command behavior.
        let envs = if self.env_clear || !self.envs.is_empty() {
            let mut result = BTreeMap::<OsString, OsString>::new();
            if !self.env_clear {
                for (key, val) in std::env::vars_os() {
                    result.insert(key, val);
                }
            }
            for (key, maybe_val) in &self.envs {
                if let Some(val) = maybe_val {
                    result.insert(key.clone(), val.clone());
                } else {
                    result.remove(key);
                }
            }
            Some(result.into_iter().collect::<Vec<_>>())
        } else {
            None
        };

        spawn_posix_spawn(
            &self.program,
            &self.args,
            current_dir,
            envs.as_deref(),
            self.stdin_cfg.unwrap_or_default(),
            self.stdout_cfg.unwrap_or_default(),
            self.stderr_cfg.unwrap_or_default(),
            self.kill_on_drop,
        )
    }

    pub async fn output(&mut self) -> io::Result<Output> {
        self.stdin_cfg.get_or_insert(Stdio::null());
        self.stdout_cfg.get_or_insert(Stdio::piped());
        self.stderr_cfg.get_or_insert(Stdio::piped());

        let child = self.spawn()?;
        child.output().await
    }

    pub async fn status(&mut self) -> io::Result<ExitStatus> {
        let mut child = self.spawn()?;
        child.status().await
    }

    pub fn get_program(&self) -> &OsStr {
        self.program.as_os_str()
    }
}

#[derive(Debug)]
pub struct Child {
    pid: libc::pid_t,
    pub stdin: Option<Unblock<std::fs::File>>,
    pub stdout: Option<Unblock<std::fs::File>>,
    pub stderr: Option<Unblock<std::fs::File>>,
    kill_on_drop: bool,
    exit: Arc<SharedExit>,
}

impl Drop for Child {
    fn drop(&mut self) {
        let mut state = self.exit.lock();
        if state.status.is_some() {
            return;
        }
        if self.kill_on_drop
            && let Err(error) = send_sigkill(self.pid)
        {
            log::debug!("could not kill dropped child {}: {error}", self.pid);
        }
        // A `status()` already blocked in `waitpid` will reap the child itself;
        // a second waiter would race it for the same pid.
        if state.blocking_waiters > 0 {
            return;
        }
        state.handed_to_reaper = true;
        drop(state);
        REAPER.adopt(self.pid, self.exit.clone());
    }
}

impl Child {
    pub fn id(&self) -> u32 {
        self.pid as u32
    }

    pub fn kill(&mut self) -> io::Result<()> {
        let state = self.exit.lock();
        // Once reaped, the pid may already belong to an unrelated process.
        if state.status.is_some() {
            return Ok(());
        }
        send_sigkill(self.pid)
    }

    pub fn try_status(&mut self) -> io::Result<Option<ExitStatus>> {
        let mut state = self.exit.lock();
        if let Some(status) = state.status {
            return Ok(Some(status));
        }
        if state.blocking_waiters > 0 {
            return Ok(None);
        }
        let status = waitpid_retrying(self.pid, libc::WNOHANG)?;
        if status.is_some() {
            state.status = status;
            self.exit.exited.notify_all();
        }
        Ok(status)
    }

    pub fn status(
        &mut self,
    ) -> impl std::future::Future<Output = io::Result<ExitStatus>> + Send + 'static {
        self.stdin.take();

        let pid = self.pid;
        let exit = self.exit.clone();

        async move {
            if let Some(status) = exit.lock().status {
                return Ok(status);
            }
            smol::unblock(move || exit.wait_blocking(pid)).await
        }
    }

    pub async fn output(mut self) -> io::Result<Output> {
        use futures_lite::AsyncReadExt;

        let status = self.status();

        let stdout = self.stdout.take();
        let stdout_future = async move {
            let mut data = Vec::new();
            if let Some(mut stdout) = stdout {
                stdout.read_to_end(&mut data).await?;
            }
            io::Result::Ok(data)
        };

        let stderr = self.stderr.take();
        let stderr_future = async move {
            let mut data = Vec::new();
            if let Some(mut stderr) = stderr {
                stderr.read_to_end(&mut data).await?;
            }
            io::Result::Ok(data)
        };

        let (stdout_data, stderr_data) =
            futures_lite::future::try_zip(stdout_future, stderr_future).await?;
        let status = status.await?;

        Ok(Output {
            status,
            stdout: stdout_data,
            stderr: stderr_data,
        })
    }
}

/// What is known about a child's exit, shared by its `Child`, the futures its
/// `status()` returned, and the reaper. Every `waitpid` on the pid is coordinated
/// through it so the pid is reaped exactly once: a second `waitpid` either fails
/// with `ECHILD` or, after pid reuse, reaps a different child.
#[derive(Debug, Default)]
struct SharedExit {
    state: Mutex<ExitState>,
    exited: Condvar,
}

#[derive(Debug, Default)]
struct ExitState {
    status: Option<ExitStatus>,
    blocking_waiters: usize,
    /// From here on only the reaper may `waitpid` this pid.
    handed_to_reaper: bool,
    reaper_error: Option<i32>,
}

impl SharedExit {
    fn lock(&self) -> MutexGuard<'_, ExitState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wait_blocking(&self, pid: libc::pid_t) -> io::Result<ExitStatus> {
        let mut state = self.lock();
        loop {
            if let Some(status) = state.status {
                return Ok(status);
            }
            if let Some(error) = state.reaper_error {
                return Err(io::Error::from_raw_os_error(error));
            }
            if !state.handed_to_reaper && state.blocking_waiters == 0 {
                break;
            }
            state = self
                .exited
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        state.blocking_waiters += 1;
        drop(state);

        let result = waitpid_retrying(pid, 0).and_then(|status| {
            status.ok_or_else(|| io::Error::other("blocking waitpid returned no status"))
        });

        let mut state = self.lock();
        state.blocking_waiters = state.blocking_waiters.saturating_sub(1);
        if let Ok(status) = result {
            state.status = Some(status);
        }
        self.exited.notify_all();
        result
    }

    /// Returns whether the reaper is done with this pid.
    fn reap_without_blocking(&self, pid: libc::pid_t) -> bool {
        let mut state = self.lock();
        if state.status.is_some() {
            return true;
        }
        let done = match waitpid_retrying(pid, libc::WNOHANG) {
            Ok(None) => return false,
            Ok(Some(status)) => {
                state.status = Some(status);
                true
            }
            Err(error) => {
                log::warn!("could not reap dropped child {pid}: {error}");
                state.reaper_error = Some(error.raw_os_error().unwrap_or(libc::ECHILD));
                true
            }
        };
        self.exited.notify_all();
        done
    }
}

/// How often the reaper polls dropped children that have not exited yet.
const REAP_INTERVAL: Duration = Duration::from_millis(100);

/// Reaps children whose `Child` was dropped before their exit was collected.
///
/// One thread for all of them rather than one per child, and `waitpid` on each
/// adopted pid rather than `waitpid(-1)`, which would steal exits from other
/// code in the process that spawns its own children.
struct Reaper {
    queue: Mutex<ReaperQueue>,
    adopted: Condvar,
}

struct ReaperQueue {
    children: Vec<(libc::pid_t, Arc<SharedExit>)>,
    thread_running: bool,
}

static REAPER: Reaper = Reaper {
    queue: Mutex::new(ReaperQueue {
        children: Vec::new(),
        thread_running: false,
    }),
    adopted: Condvar::new(),
};

impl Reaper {
    fn lock(&self) -> MutexGuard<'_, ReaperQueue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn adopt(&'static self, pid: libc::pid_t, exit: Arc<SharedExit>) {
        let mut queue = self.lock();
        queue.children.push((pid, exit));
        // Retried on every adoption, so one failed thread start -- out of
        // memory or thread limits -- does not stop reaping for the life of
        // the process.
        if !queue.thread_running {
            match std::thread::Builder::new()
                .name("child-reaper".into())
                .spawn(move || self.run())
            {
                Ok(_) => queue.thread_running = true,
                Err(error) => log::error!("could not start the child reaper: {error}"),
            }
        }
        drop(queue);
        self.adopted.notify_one();
    }

    fn run(&self) {
        let mut queue = self.lock();
        loop {
            while queue.children.is_empty() {
                queue = self
                    .adopted
                    .wait(queue)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            queue
                .children
                .retain(|(pid, exit)| !exit.reap_without_blocking(*pid));
            if !queue.children.is_empty() {
                queue = match self.adopted.wait_timeout(queue, REAP_INTERVAL) {
                    Ok((queue, _)) => queue,
                    Err(poisoned) => poisoned.into_inner().0,
                };
            }
        }
    }
}

fn send_sigkill(pid: libc::pid_t) -> io::Result<()> {
    let result = unsafe { libc::kill(pid, libc::SIGKILL) };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn waitpid_retrying(pid: libc::pid_t, flags: libc::c_int) -> io::Result<Option<ExitStatus>> {
    loop {
        let mut status: libc::c_int = 0;
        let result = unsafe { libc::waitpid(pid, &mut status, flags) };
        if result == -1 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if result == 0 {
            return Ok(None);
        }
        return Ok(Some(ExitStatus::from_raw(status)));
    }
}

fn spawn_posix_spawn(
    program: &OsStr,
    args: &[OsString],
    current_dir: &Path,
    envs: Option<&[(OsString, OsString)]>,
    stdin_cfg: Stdio,
    stdout_cfg: Stdio,
    stderr_cfg: Stdio,
    kill_on_drop: bool,
) -> io::Result<Child> {
    let program_cstr = CString::new(program.as_bytes()).map_err(|_| invalid_input_error())?;

    let current_dir_cstr =
        CString::new(current_dir.as_os_str().as_bytes()).map_err(|_| invalid_input_error())?;

    let mut argv_cstrs = vec![program_cstr.clone()];
    for arg in args {
        let cstr = CString::new(arg.as_bytes()).map_err(|_| invalid_input_error())?;
        argv_cstrs.push(cstr);
    }
    let mut argv_ptrs: Vec<*mut libc::c_char> = argv_cstrs
        .iter()
        .map(|s| s.as_ptr() as *mut libc::c_char)
        .collect();
    argv_ptrs.push(ptr::null_mut());

    let envp: Vec<CString> = if let Some(envs) = envs {
        envs.iter()
            .map(|(key, value)| {
                let mut env_str = key.as_bytes().to_vec();
                env_str.push(b'=');
                env_str.extend_from_slice(value.as_bytes());
                CString::new(env_str)
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| invalid_input_error())?
    } else {
        Vec::new()
    };
    let mut envp_ptrs: Vec<*mut libc::c_char> = envp
        .iter()
        .map(|s| s.as_ptr() as *mut libc::c_char)
        .collect();
    envp_ptrs.push(ptr::null_mut());

    let (stdin_read, stdin_write) = match stdin_cfg {
        Stdio::Piped => {
            let (r, w) = create_pipe()?;
            (Some(r), Some(w))
        }
        Stdio::Null => {
            let fd = open_dev_null(libc::O_RDONLY)?;
            (Some(fd), None)
        }
        Stdio::Inherit => (None, None),
    };

    let (stdout_read, stdout_write) = match stdout_cfg {
        Stdio::Piped => {
            let (r, w) = create_pipe()?;
            (Some(r), Some(w))
        }
        Stdio::Null => {
            let fd = open_dev_null(libc::O_WRONLY)?;
            (None, Some(fd))
        }
        Stdio::Inherit => (None, None),
    };

    let (stderr_read, stderr_write) = match stderr_cfg {
        Stdio::Piped => {
            let (r, w) = create_pipe()?;
            (Some(r), Some(w))
        }
        Stdio::Null => {
            let fd = open_dev_null(libc::O_WRONLY)?;
            (None, Some(fd))
        }
        Stdio::Inherit => (None, None),
    };

    let mut attr: libc::posix_spawnattr_t = ptr::null_mut();
    let mut file_actions: libc::posix_spawn_file_actions_t = ptr::null_mut();

    unsafe {
        cvt_nz(libc::posix_spawnattr_init(&mut attr))?;
        cvt_nz(libc::posix_spawn_file_actions_init(&mut file_actions))?;

        cvt_nz(libc::posix_spawnattr_setflags(
            &mut attr,
            libc::POSIX_SPAWN_CLOEXEC_DEFAULT as libc::c_short,
        ))?;

        cvt_nz(posix_spawnattr_setexceptionports_np(
            &mut attr,
            EXC_MASK_ALL,
            MACH_PORT_NULL,
            EXCEPTION_DEFAULT as exception_behavior_t,
            THREAD_STATE_NONE,
        ))?;

        cvt_nz(posix_spawn_file_actions_addchdir_np(
            &mut file_actions,
            current_dir_cstr.as_ptr(),
        ))?;

        if let Some(fd) = stdin_read {
            cvt_nz(libc::posix_spawn_file_actions_adddup2(
                &mut file_actions,
                fd,
                libc::STDIN_FILENO,
            ))?;
            cvt_nz(posix_spawn_file_actions_addinherit_np(
                &mut file_actions,
                libc::STDIN_FILENO,
            ))?;
        }

        if let Some(fd) = stdout_write {
            cvt_nz(libc::posix_spawn_file_actions_adddup2(
                &mut file_actions,
                fd,
                libc::STDOUT_FILENO,
            ))?;
            cvt_nz(posix_spawn_file_actions_addinherit_np(
                &mut file_actions,
                libc::STDOUT_FILENO,
            ))?;
        }

        if let Some(fd) = stderr_write {
            cvt_nz(libc::posix_spawn_file_actions_adddup2(
                &mut file_actions,
                fd,
                libc::STDERR_FILENO,
            ))?;
            cvt_nz(posix_spawn_file_actions_addinherit_np(
                &mut file_actions,
                libc::STDERR_FILENO,
            ))?;
        }

        let mut pid: libc::pid_t = 0;

        let spawn_result = libc::posix_spawnp(
            &mut pid,
            program_cstr.as_ptr(),
            &file_actions,
            &attr,
            argv_ptrs.as_ptr(),
            if envs.is_some() {
                envp_ptrs.as_ptr()
            } else {
                environ
            },
        );

        libc::posix_spawnattr_destroy(&mut attr);
        libc::posix_spawn_file_actions_destroy(&mut file_actions);

        if let Some(fd) = stdin_read {
            libc::close(fd);
        }
        if let Some(fd) = stdout_write {
            libc::close(fd);
        }
        if let Some(fd) = stderr_write {
            libc::close(fd);
        }

        cvt_nz(spawn_result)?;

        Ok(Child {
            pid,
            stdin: stdin_write.map(|fd| Unblock::new(std::fs::File::from_raw_fd(fd))),
            stdout: stdout_read.map(|fd| Unblock::new(std::fs::File::from_raw_fd(fd))),
            stderr: stderr_read.map(|fd| Unblock::new(std::fs::File::from_raw_fd(fd))),
            kill_on_drop,
            exit: Arc::default(),
        })
    }
}

fn create_pipe() -> io::Result<(libc::c_int, libc::c_int)> {
    let mut fds: [libc::c_int; 2] = [0; 2];
    let result = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok((fds[0], fds[1]))
}

fn open_dev_null(flags: libc::c_int) -> io::Result<libc::c_int> {
    let fd = unsafe { libc::open(c"/dev/null".as_ptr() as *const libc::c_char, flags) };
    if fd == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}

/// Zero means `Ok()`, all other values are treated as raw OS errors. Does not look at `errno`.
/// Mirrored after Rust's std `cvt_nz` function.
fn cvt_nz(error: libc::c_int) -> io::Result<()> {
    if error == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(error))
    }
}

fn invalid_input_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid argument: path or argument contains null byte",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_lite::AsyncWriteExt;

    /// A zombie still answers `kill(pid, 0)`; only a reaped pid reports `ESRCH`.
    fn wait_until_reaped(pid: u32) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
            if result == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "process {pid} was not reaped"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn test_drop_reaps_child() {
        let child = Command::new("/usr/bin/true")
            .spawn()
            .expect("failed to spawn command");
        let pid = child.id();
        drop(child);
        wait_until_reaped(pid);
    }

    #[test]
    fn test_kill_on_drop_kills_and_reaps_child() {
        let child = Command::new("/bin/sleep")
            .arg("60")
            .kill_on_drop(true)
            .spawn()
            .expect("failed to spawn command");
        let pid = child.id();
        drop(child);
        wait_until_reaped(pid);
    }

    #[test]
    fn test_output_future_dropped_before_first_poll_reaps_child() {
        let child = Command::new("/usr/bin/true")
            .spawn()
            .expect("failed to spawn command");
        let pid = child.id();
        let output = child.output();
        drop(output);
        wait_until_reaped(pid);
    }

    #[test]
    fn test_output_future_dropped_after_first_poll_reaps_child() {
        let child = Command::new("/bin/sleep")
            .arg("60")
            .kill_on_drop(true)
            .spawn()
            .expect("failed to spawn command");
        let pid = child.id();
        let mut output = Box::pin(child.output());
        let poll = futures::FutureExt::poll_unpin(
            &mut output,
            &mut std::task::Context::from_waker(std::task::Waker::noop()),
        );
        assert!(poll.is_pending(), "sleep 60 cannot have finished yet");
        drop(output);
        wait_until_reaped(pid);
    }

    #[test]
    fn test_awaited_child_reports_its_exit_status_once_reaped() {
        smol::block_on(async {
            let mut child = Command::new("/bin/sh")
                .args(["-c", "exit 7"])
                .spawn()
                .expect("failed to spawn command");
            let pid = child.id();
            let status = child.status().await.expect("status of a live child");
            assert_eq!(status.code(), Some(7));
            assert_eq!(
                child.try_status().expect("try_status after status"),
                Some(status),
                "a reaped child must report the status it was reaped with, \
                 not ECHILD from a second waitpid"
            );
            let second = child.status().await.expect("a second status call");
            assert_eq!(second.code(), Some(7));
            drop(child);
            wait_until_reaped(pid);
        });
    }

    #[test]
    fn test_status_future_outliving_its_dropped_child_still_reports_exit() {
        smol::block_on(async {
            let mut child = Command::new("/bin/sh")
                .args(["-c", "exit 3"])
                .spawn()
                .expect("failed to spawn command");
            let pid = child.id();
            let status = child.status();
            drop(child);
            let status = status.await.expect("status after the child was dropped");
            assert_eq!(status.code(), Some(3));
            wait_until_reaped(pid);
        });
    }

    /// A child dropped while its `status()` is blocked in `waitpid` is that
    /// waiter's to reap: handing it to the reaper as well would have two
    /// `waitpid`s race for one pid, and the loser gets `ECHILD`.
    #[test]
    fn test_child_dropped_while_its_status_waits_is_reaped_by_that_waiter() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "sleep 0.3; exit 5"])
            .spawn()
            .expect("failed to spawn command");
        let pid = child.id();
        let status = child.status();
        let waiter = std::thread::spawn(move || smol::block_on(status));

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while child.exit.lock().blocking_waiters == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "the status future never started waiting"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(
            child
                .try_status()
                .expect("try_status during a blocking wait"),
            None,
            "a second waitpid must not run beside the blocked one"
        );

        drop(child);
        assert!(
            !REAPER
                .lock()
                .children
                .iter()
                .any(|(adopted, _)| *adopted as u32 == pid),
            "the reaper must not adopt a pid a blocked waiter will reap"
        );

        let status = waiter
            .join()
            .expect("the waiter thread finished")
            .expect("the blocked waiter reaped the child");
        assert_eq!(status.code(), Some(5));
        wait_until_reaped(pid);
    }

    #[test]
    fn test_kill_after_the_child_was_reaped_signals_nothing() {
        smol::block_on(async {
            let mut child = Command::new("/usr/bin/true")
                .spawn()
                .expect("failed to spawn command");
            child.status().await.expect("status of a live child");
            assert!(
                child.kill().is_ok(),
                "a reaped child's pid may belong to someone else now"
            );
        });
    }

    #[test]
    fn test_spawning_and_dropping_many_children_leaves_no_zombies() {
        let pids = (0..200)
            .map(|_| {
                let child = Command::new("/usr/bin/true")
                    .spawn()
                    .expect("failed to spawn command");
                child.id()
            })
            .collect::<Vec<_>>();
        for pid in pids {
            wait_until_reaped(pid);
        }
    }

    #[test]
    fn test_spawn_echo() {
        smol::block_on(async {
            let output = Command::new("/bin/echo")
                .args(["-n", "hello world"])
                .output()
                .await
                .expect("failed to run command");

            assert!(output.status.success());
            assert_eq!(output.stdout, b"hello world");
        });
    }

    #[test]
    fn test_spawn_cat_stdin() {
        smol::block_on(async {
            let mut child = Command::new("/bin/cat")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("failed to spawn");

            if let Some(ref mut stdin) = child.stdin {
                stdin
                    .write_all(b"hello from stdin")
                    .await
                    .expect("failed to write");
                stdin.close().await.expect("failed to close");
            }
            drop(child.stdin.take());

            let output = child.output().await.expect("failed to get output");
            assert!(output.status.success());
            assert_eq!(output.stdout, b"hello from stdin");
        });
    }

    #[test]
    fn test_spawn_stderr() {
        smol::block_on(async {
            let output = Command::new("/bin/sh")
                .args(["-c", "echo error >&2"])
                .output()
                .await
                .expect("failed to run command");

            assert!(output.status.success());
            assert_eq!(output.stderr, b"error\n");
        });
    }

    #[test]
    fn test_spawn_exit_code() {
        smol::block_on(async {
            let output = Command::new("/bin/sh")
                .args(["-c", "exit 42"])
                .output()
                .await
                .expect("failed to run command");

            assert!(!output.status.success());
            assert_eq!(output.status.code(), Some(42));
        });
    }

    #[test]
    fn test_spawn_current_dir() {
        smol::block_on(async {
            let output = Command::new("/bin/pwd")
                .current_dir("/tmp")
                .output()
                .await
                .expect("failed to run command");

            assert!(output.status.success());
            let pwd = String::from_utf8_lossy(&output.stdout);
            assert!(pwd.trim() == "/tmp" || pwd.trim() == "/private/tmp");
        });
    }

    #[test]
    fn test_spawn_env() {
        smol::block_on(async {
            let output = Command::new("/bin/sh")
                .args(["-c", "echo $MY_TEST_VAR"])
                .env("MY_TEST_VAR", "test_value")
                .output()
                .await
                .expect("failed to run command");

            assert!(output.status.success());
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "test_value");
        });
    }

    #[test]
    fn test_spawn_status() {
        smol::block_on(async {
            let status = Command::new("/usr/bin/true")
                .status()
                .await
                .expect("failed to run command");

            assert!(status.success());

            let status = Command::new("/usr/bin/false")
                .status()
                .await
                .expect("failed to run command");

            assert!(!status.success());
        });
    }

    #[test]
    fn test_env_remove_removes_set_env() {
        smol::block_on(async {
            let output = Command::new("/bin/sh")
                .args(["-c", "echo ${MY_VAR:-unset}"])
                .env("MY_VAR", "set_value")
                .env_remove("MY_VAR")
                .output()
                .await
                .expect("failed to run command");

            assert!(output.status.success());
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "unset");
        });
    }

    #[test]
    fn test_env_remove_removes_inherited_env() {
        smol::block_on(async {
            // SAFETY: This test is single-threaded and we clean up the var at the end
            unsafe { std::env::set_var("TEST_INHERITED_VAR", "inherited_value") };

            let output = Command::new("/bin/sh")
                .args(["-c", "echo ${TEST_INHERITED_VAR:-unset}"])
                .env_remove("TEST_INHERITED_VAR")
                .output()
                .await
                .expect("failed to run command");

            assert!(output.status.success());
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "unset");

            // SAFETY: Cleaning up test env var
            unsafe { std::env::remove_var("TEST_INHERITED_VAR") };
        });
    }

    #[test]
    fn test_env_after_env_remove() {
        smol::block_on(async {
            let output = Command::new("/bin/sh")
                .args(["-c", "echo ${MY_VAR:-unset}"])
                .env_remove("MY_VAR")
                .env("MY_VAR", "new_value")
                .output()
                .await
                .expect("failed to run command");

            assert!(output.status.success());
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "new_value");
        });
    }

    #[test]
    fn test_env_remove_after_env_clear() {
        smol::block_on(async {
            let output = Command::new("/bin/sh")
                .args(["-c", "echo ${MY_VAR:-unset}"])
                .env_clear()
                .env("MY_VAR", "set_value")
                .env_remove("MY_VAR")
                .output()
                .await
                .expect("failed to run command");

            assert!(output.status.success());
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "unset");
        });
    }

    #[test]
    fn test_stdio_null_stdin() {
        smol::block_on(async {
            let child = Command::new("/bin/cat")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .spawn()
                .expect("failed to spawn");

            let output = child.output().await.expect("failed to get output");
            assert!(output.status.success());
            assert!(
                output.stdout.is_empty(),
                "stdin from /dev/null should produce no output from cat"
            );
        });
    }

    #[test]
    fn test_stdio_null_stdout() {
        smol::block_on(async {
            let mut child = Command::new("/bin/echo")
                .args(["hello"])
                .stdout(Stdio::null())
                .spawn()
                .expect("failed to spawn");

            assert!(
                child.stdout.is_none(),
                "stdout should be None when Stdio::null() is used"
            );

            let status = child.status().await.expect("failed to get status");
            assert!(status.success());
        });
    }

    #[test]
    fn test_stdio_null_stderr() {
        smol::block_on(async {
            let mut child = Command::new("/bin/sh")
                .args(["-c", "echo error >&2"])
                .stderr(Stdio::null())
                .spawn()
                .expect("failed to spawn");

            assert!(
                child.stderr.is_none(),
                "stderr should be None when Stdio::null() is used"
            );

            let status = child.status().await.expect("failed to get status");
            assert!(status.success());
        });
    }

    #[test]
    fn test_stdio_piped_stdin() {
        smol::block_on(async {
            let mut child = Command::new("/bin/cat")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("failed to spawn");

            assert!(
                child.stdin.is_some(),
                "stdin should be Some when Stdio::piped() is used"
            );

            if let Some(ref mut stdin) = child.stdin {
                stdin
                    .write_all(b"piped input")
                    .await
                    .expect("failed to write");
                stdin.close().await.expect("failed to close");
            }
            drop(child.stdin.take());

            let output = child.output().await.expect("failed to get output");
            assert!(output.status.success());
            assert_eq!(output.stdout, b"piped input");
        });
    }
}
