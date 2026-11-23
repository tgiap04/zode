use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;

pub const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(10);

const MESSAGE_LIMIT_BYTES: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostOs {
    MacOs,
    Linux,
    Windows,
}

impl HostOs {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    pub fn executable_name(self, stem: &str) -> String {
        match self {
            Self::Windows => format!("{stem}.exe"),
            Self::MacOs | Self::Linux => stem.to_string(),
        }
    }
}

/// One run of a command-line tool. The program must be an absolute path: on
/// macOS the spawn searches Zode's own `PATH`, not the environment passed here.
#[derive(Clone, PartialEq, Eq)]
pub struct ToolInvocation {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub stdin: Option<String>,
    pub timeout: Duration,
}

impl ToolInvocation {
    pub fn new<Arg: Into<String>>(
        program: impl Into<PathBuf>,
        args: impl IntoIterator<Item = Arg>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            env: Vec::new(),
            stdin: None,
            timeout: DEFAULT_TOOL_TIMEOUT,
        }
    }

    pub fn with_stdin(mut self, stdin: String) -> Self {
        self.stdin = Some(stdin);
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

// Written by hand so that stdin, which carries the adb pairing code, never
// reaches a log line through `{:?}`.
impl fmt::Debug for ToolInvocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolInvocation")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("env", &self.env)
            .field("stdin", &self.stdin.as_ref().map(|_| "<redacted>"))
            .field("timeout", &self.timeout)
            .finish()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl ToolOutput {
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }

    pub fn combined(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }

    /// What to show the user when the tool failed: stderr, or stdout when the
    /// tool wrote its error there, cut to a size a notification can hold.
    pub fn failure_message(&self) -> String {
        let stderr = self.stderr.trim();
        let text = if stderr.is_empty() {
            self.stdout.trim()
        } else {
            stderr
        };
        if text.is_empty() {
            return match self.exit_code {
                Some(code) => format!("exited with code {code}"),
                None => "was terminated by a signal".to_string(),
            };
        }
        truncate_message(text)
    }
}

pub(crate) fn truncate_message(text: &str) -> String {
    if text.len() <= MESSAGE_LIMIT_BYTES {
        return text.to_string();
    }
    let mut end = MESSAGE_LIMIT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolFailure {
    RelativeProgram(PathBuf),
    NotFound(PathBuf),
    TimedOut { program: PathBuf, timeout: Duration },
    Io { program: PathBuf, message: String },
}

impl fmt::Display for ToolFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelativeProgram(program) => write!(
                formatter,
                "refused to run {}: the program must be an absolute path",
                program.display()
            ),
            Self::NotFound(program) => write!(formatter, "{} was not found", program.display()),
            Self::TimedOut { program, timeout } => write!(
                formatter,
                "{} did not finish within {} s",
                program.display(),
                timeout.as_secs()
            ),
            Self::Io { program, message } => {
                write!(formatter, "could not run {}: {message}", program.display())
            }
        }
    }
}

impl std::error::Error for ToolFailure {}

/// What matters is that the spawn never searches `PATH`, and a path that
/// starts at a root never is. Windows calls `/usr/bin/xcrun` relative (to the
/// current drive), so `Path::is_absolute` alone would refuse the Mac and Linux
/// hosts the tests describe whenever they run on Windows. `has_root` follows
/// the running system, so a `\\` path stays a bare name on a Mac.
pub(crate) fn is_rooted(path: &Path) -> bool {
    path.is_absolute() || path.has_root()
}

pub fn ensure_absolute_program(invocation: &ToolInvocation) -> Result<(), ToolFailure> {
    if is_rooted(&invocation.program) {
        Ok(())
    } else {
        Err(ToolFailure::RelativeProgram(invocation.program.clone()))
    }
}

/// Settles with the exit code of a detached tool (`None` when a signal ended
/// it) once it exits. Cancelled, never settled, when its exit cannot be known.
pub type DetachedExit = futures::channel::oneshot::Receiver<Option<i32>>;

#[async_trait]
pub trait ToolRunner: Send + Sync {
    /// Runs the tool to completion. A non-zero exit is an `Ok` output; only a
    /// tool that could not be run, or ran past its timeout, is an `Err`.
    async fn run(&self, invocation: ToolInvocation) -> Result<ToolOutput, ToolFailure>;

    /// Starts a tool that outlives Zode, with stdout and stderr in `log_file`.
    fn spawn_detached(
        &self,
        invocation: ToolInvocation,
        log_file: &Path,
    ) -> anyhow::Result<DetachedExit>;
}

pub trait HostSystem: Send + Sync {
    fn os(&self) -> HostOs;
    /// From Zode's own environment, which holds the login shell's variables,
    /// never a project's.
    fn environment_variable(&self, name: &str) -> Option<String>;
    fn find_on_path(&self, program: &str) -> Option<PathBuf>;
    fn home_dir(&self) -> Option<PathBuf>;
    fn is_file(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    /// The path itself is a symbolic link, whatever it points to.
    fn is_symlink(&self, path: &Path) -> bool;
    fn read_to_string(&self, path: &Path) -> std::io::Result<String>;
    /// Fails with `InvalidData` instead of reading a file over `max_bytes`, so
    /// a huge file in a project cannot stall the caller.
    fn read_to_string_at_most(&self, path: &Path, max_bytes: u64) -> std::io::Result<String>;
    /// At most the last `max_bytes` of the file.
    fn read_file_tail(&self, path: &Path, max_bytes: u64) -> std::io::Result<String>;
    fn read_dir_names(&self, path: &Path) -> std::io::Result<Vec<String>>;
    fn remove_file(&self, path: &Path) -> std::io::Result<()>;
    /// A path no other caller is handed, for a tool to write its output to.
    fn temp_file_path(&self, stem: &str) -> PathBuf;
    fn available_memory_bytes(&self) -> Option<u64>;
}
