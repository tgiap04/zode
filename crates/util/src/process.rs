use anyhow::{Context as _, Result};
use std::process::Stdio;

/// A wrapper around `smol::process::Child` that ensures all subprocesses
/// are killed when the process is terminated by using process groups.
pub struct Child {
    process: smol::process::Child,
}

impl std::ops::Deref for Child {
    type Target = smol::process::Child;

    fn deref(&self) -> &Self::Target {
        &self.process
    }
}

impl std::ops::DerefMut for Child {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.process
    }
}

impl Child {
    #[cfg(not(windows))]
    pub fn spawn(
        mut command: std::process::Command,
        stdin: Stdio,
        stdout: Stdio,
        stderr: Stdio,
    ) -> Result<Self> {
        crate::set_pre_exec_to_start_new_session(&mut command);
        let mut command = smol::process::Command::from(command);
        let process = command
            .stdin(stdin)
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .with_context(|| {
                format!(
                    "failed to spawn command {}",
                    crate::redact::redact_command(&format!("{command:?}"))
                )
            })?;
        Ok(Self { process })
    }

    #[cfg(windows)]
    pub fn spawn(
        command: std::process::Command,
        stdin: Stdio,
        stdout: Stdio,
        stderr: Stdio,
    ) -> Result<Self> {
        // TODO(windows): create a job object and add the child process handle to it,
        // see https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects
        let mut command = smol::process::Command::from(command);
        let process = command
            .stdin(stdin)
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .with_context(|| {
                format!(
                    "failed to spawn command {}",
                    crate::redact::redact_command(&format!("{command:?}"))
                )
            })?;

        Ok(Self { process })
    }

    pub fn into_inner(self) -> smol::process::Child {
        self.process
    }

    #[cfg(not(windows))]
    pub fn kill(&mut self) -> Result<()> {
        let pid = self.process.id();
        unsafe {
            libc::killpg(pid as i32, libc::SIGKILL);
        }
        Ok(())
    }

    #[cfg(windows)]
    pub fn kill(&mut self) -> Result<()> {
        // TODO(windows): terminate the job object in kill
        self.process.kill()?;
        Ok(())
    }
}

/// Runs `f` with `SIGCHLD` blocked on the calling thread, restoring the
/// previous mask afterwards. A no-op off macOS.
///
/// Threads inherit the signal mask of the thread that spawns them. Wasmtime's
/// exception-handler thread on macOS aborts the whole process if a signal
/// interrupts its `mach_msg` wait, and `SIGCHLD` from an exiting child process
/// is delivered to whichever thread has it unblocked. Creating the engine
/// inside this closure makes that thread start with the signal blocked.
pub fn with_sigchld_blocked<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: the sets are plain, fully initialised by `sigemptyset` before
        // use, and `pthread_sigmask` only changes the calling thread's mask.
        let previous = unsafe {
            let mut blocked: libc::sigset_t = std::mem::zeroed();
            let mut previous: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut blocked);
            libc::sigaddset(&mut blocked, libc::SIGCHLD);
            if libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, &mut previous) != 0 {
                return f();
            }
            previous
        };
        struct Restore(libc::sigset_t);
        impl Drop for Restore {
            fn drop(&mut self) {
                // SAFETY: `self.0` is the mask `pthread_sigmask` returned for
                // this same thread.
                unsafe {
                    libc::pthread_sigmask(libc::SIG_SETMASK, &self.0, std::ptr::null_mut());
                }
            }
        }
        let _restore = Restore(previous);
        f()
    }
    #[cfg(not(target_os = "macos"))]
    f()
}

#[cfg(all(test, target_os = "macos"))]
mod sigchld_tests {
    use super::with_sigchld_blocked;

    fn sigchld_is_blocked() -> bool {
        // SAFETY: querying the calling thread's mask with a null new set.
        unsafe {
            let mut current: libc::sigset_t = std::mem::zeroed();
            libc::pthread_sigmask(libc::SIG_SETMASK, std::ptr::null(), &mut current);
            libc::sigismember(&current, libc::SIGCHLD) == 1
        }
    }

    #[test]
    fn blocks_only_inside_the_closure_and_threads_inherit_it() {
        assert!(!sigchld_is_blocked());
        let (inside, inherited) = with_sigchld_blocked(|| {
            let inherited = std::thread::spawn(sigchld_is_blocked).join().unwrap();
            (sigchld_is_blocked(), inherited)
        });
        assert!(inside);
        assert!(inherited, "a thread born inside starts with it blocked");
        assert!(!sigchld_is_blocked());
    }

    #[test]
    fn an_already_blocked_signal_stays_blocked_afterwards() {
        with_sigchld_blocked(|| {
            with_sigchld_blocked(|| {});
            assert!(sigchld_is_blocked());
        });
    }
}
