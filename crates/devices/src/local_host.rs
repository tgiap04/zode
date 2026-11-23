use std::io::{self, Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use futures::AsyncWriteExt as _;
use util::command::Stdio;

use crate::detached_spawn::{spawn_detached, terminate_process_group};
use crate::host::{
    DetachedTool, HostOs, HostSystem, ToolFailure, ToolInvocation, ToolOutput, ToolRunner,
    ensure_absolute_program,
};

/// This machine, as Zode's process sees it.
#[derive(Clone, Copy, Debug)]
pub struct LocalHost {
    os: HostOs,
}

impl LocalHost {
    pub fn new() -> Self {
        Self {
            os: HostOs::current(),
        }
    }
}

impl Default for LocalHost {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolRunner for LocalHost {
    async fn run(&self, invocation: ToolInvocation) -> Result<ToolOutput, ToolFailure> {
        ensure_absolute_program(&invocation)?;
        let program = invocation.program.clone();
        let io_failure = |error: io::Error| match error.kind() {
            io::ErrorKind::NotFound => ToolFailure::NotFound(program.clone()),
            _ => ToolFailure::Io {
                program: program.clone(),
                message: error.to_string(),
            },
        };

        let mut command = util::command::new_command(&invocation.program);
        command
            .args(&invocation.args)
            .envs(invocation.env.iter().map(|(name, value)| (name, value)))
            .stdin(if invocation.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(io_failure)?;

        let run = async move {
            if let Some(text) = &invocation.stdin
                && let Some(mut stdin) = child.stdin.take()
            {
                // A tool that fails on its arguments exits without reading
                // its input; what it printed then is the real error, so a
                // closed pipe must not hide it.
                for result in [stdin.write_all(text.as_bytes()).await, stdin.close().await] {
                    match result {
                        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => break,
                        result => result?,
                    }
                }
            }
            let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
            // `status` drops what is left of stdin, so the tool sees the end
            // of its input.
            let (stdout, stderr, status) =
                futures::try_join!(read_capped(stdout), read_capped(stderr), child.status())?;
            Ok::<_, io::Error>((status, stdout, stderr))
        };
        let (status, stdout, stderr) = race_with_timeout(run, invocation.timeout)
            .await
            .ok_or_else(|| ToolFailure::TimedOut {
                program: invocation.program.clone(),
                timeout: invocation.timeout,
            })?
            .map_err(io_failure)?;
        Ok(ToolOutput {
            exit_code: status.code(),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    }

    fn spawn_detached(
        &self,
        invocation: ToolInvocation,
        log_file: &Path,
    ) -> anyhow::Result<DetachedTool> {
        ensure_absolute_program(&invocation)?;
        spawn_detached(&invocation, log_file)
    }

    fn terminate_process_group(&self, process_id: u32) -> anyhow::Result<()> {
        terminate_process_group(process_id)
    }
}

/// What one tool run may hand back on each stream.
pub(crate) const MAX_TOOL_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;

/// Keeps the first [`MAX_TOOL_OUTPUT_BYTES`] and reads the rest into the void,
/// so a tool that talks a lot still finishes instead of blocking on a full pipe.
async fn read_capped(stream: Option<impl futures::AsyncRead + Unpin>) -> io::Result<Vec<u8>> {
    use futures::AsyncReadExt as _;

    let Some(mut stream) = stream else {
        return Ok(Vec::new());
    };
    let mut kept = Vec::new();
    (&mut stream)
        .take(MAX_TOOL_OUTPUT_BYTES)
        .read_to_end(&mut kept)
        .await?;
    futures::io::copy(&mut stream, &mut futures::io::sink()).await?;
    Ok(kept)
}

/// `None` when the timeout won. Dropping the losing future drops the child,
/// which `kill_on_drop` then kills.
#[allow(
    clippy::disallowed_methods,
    reason = "a tool's timeout is wall-clock time, and this crate has no GPUI executor"
)]
async fn race_with_timeout<T>(work: impl Future<Output = T>, timeout: Duration) -> Option<T> {
    smol::future::or(async { Some(work.await) }, async {
        smol::Timer::after(timeout).await;
        None
    })
    .await
}

pub(crate) fn too_large(max_bytes: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("the file is larger than {max_bytes} bytes"),
    )
}

impl HostSystem for LocalHost {
    fn os(&self) -> HostOs {
        self.os
    }

    fn environment_variable(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|value| !value.is_empty())
    }

    fn find_on_path(&self, program: &str) -> Option<PathBuf> {
        which::which(program).ok()
    }

    fn home_dir(&self) -> Option<PathBuf> {
        Some(util::paths::home_dir().clone())
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn is_symlink(&self, path: &Path) -> bool {
        path.is_symlink()
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn read_to_string_at_most(&self, path: &Path, max_bytes: u64) -> io::Result<String> {
        let file = std::fs::File::open(path)?;
        let mut bytes = Vec::new();
        // One byte past the limit tells a file at the limit from a longer one,
        // even when the file grows between a metadata check and the read.
        file.take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > max_bytes {
            return Err(too_large(max_bytes));
        }
        String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    fn read_file_tail(&self, path: &Path, max_bytes: u64) -> io::Result<String> {
        let mut file = std::fs::File::open(path)?;
        let length = file.metadata()?.len();
        file.seek(SeekFrom::Start(length.saturating_sub(max_bytes)))?;
        let mut bytes = Vec::new();
        file.take(max_bytes).read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn read_dir_names(&self, path: &Path) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(path)? {
            if let Some(name) = entry?.file_name().to_str() {
                names.push(name.to_string());
            }
        }
        Ok(names)
    }

    fn file_len(&self, path: &Path) -> io::Result<u64> {
        Ok(std::fs::metadata(path)?.len())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn create_temp_dir(&self, stem: &str) -> io::Result<PathBuf> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("zode-{}-{sequence}-{stem}", std::process::id()));
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt as _;
            let mut builder = std::fs::DirBuilder::new();
            builder.mode(0o700);
            builder
        };
        #[cfg(not(unix))]
        let builder = std::fs::DirBuilder::new();
        builder.create(&path)?;
        Ok(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_dir_all(path)
    }

    fn available_memory_bytes(&self) -> Option<u64> {
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        Some(system.available_memory()).filter(|bytes| *bytes > 0)
    }
}
