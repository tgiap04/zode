use anyhow::{Context as _, Result};
use async_trait::async_trait;
use futures::io::{BufReader, BufWriter};
use futures::{
    AsyncBufReadExt as _, AsyncRead, AsyncWrite, AsyncWriteExt as _, Stream, StreamExt as _,
};
use gpui::AsyncApp;
use smol::channel;
use smol::process::Child;
use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use util::TryFutureExt as _;
use util::shell::Shell;
use util::shell_builder::ShellBuilder;

/// One line of JSON in, one line of JSON out.
///
/// Deliberately narrower than a driver: `FakeTransport` implements it in
/// process so the client's own tests never spawn anything.
#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, message: String) -> Result<()>;
    fn receive(&self) -> Pin<Box<dyn Stream<Item = String> + Send>>;
    /// A driver's stderr. The only place a driver may log: a stray line on
    /// stdout breaks the framing for every message after it.
    fn receive_err(&self) -> Pin<Box<dyn Stream<Item = String> + Send>>;
}

/// How to start a driver.
#[derive(Clone, Debug)]
pub struct DriverBinary {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

/// A driver running as a child process, spoken to over its stdio.
///
/// Mirrors `context_server::transport::StdioTransport`, which solves the same
/// problem for MCP servers. Not reused directly: its constructor takes
/// `ModelContextServerBinary`, so sharing it would put an MCP type in the
/// public API of a database driver and pull that crate's HTTP and OAuth
/// transports in behind it. If a third caller ever wants this, it is worth
/// lifting into a crate of its own rather than picking one of the two to
/// depend on the other.
pub struct StdioTransport {
    outbound: channel::Sender<String>,
    inbound: channel::Receiver<String>,
    errors: channel::Receiver<String>,
    child: DriverProcess,
}

/// The two kinds of child a driver runs as. One type on Linux and Windows,
/// where `util::command::Child` is smol's; two on macOS, where it is not.
enum DriverProcess {
    Shell(Child),
    Prepared(util::command::Child),
}

impl DriverProcess {
    fn kill(&mut self) -> std::io::Result<()> {
        match self {
            Self::Shell(child) => child.kill(),
            Self::Prepared(child) => child.kill(),
        }
    }
}

impl StdioTransport {
    pub fn new(
        binary: &DriverBinary,
        working_directory: Option<&PathBuf>,
        cx: &AsyncApp,
    ) -> Result<Self> {
        let builder = ShellBuilder::new(&Shell::System, cfg!(windows)).non_interactive();
        let mut command =
            builder.build_smol_command(Some(binary.executable.display().to_string()), &binary.args);

        command
            .envs(binary.env.clone())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // A driver holding a database connection open after the editor has
            // let go of it is a connection nobody will ever close.
            .kill_on_drop(true);

        if let Some(working_directory) = working_directory {
            command.current_dir(working_directory);
        }

        let mut child = command
            .spawn()
            .with_context(|| format!("failed to spawn database driver {command:?}"))?;

        let (stdin, stdout, stderr) = (
            child.stdin.take().context("driver stdin was not piped")?,
            child.stdout.take().context("driver stdout was not piped")?,
            child.stderr.take().context("driver stderr was not piped")?,
        );
        Ok(Self::speak_through(
            DriverProcess::Shell(child),
            stdin,
            stdout,
            stderr,
            cx,
        ))
    }

    /// A driver started by a command someone else built.
    ///
    /// What a remote project's driver runs through: the command is the local
    /// wrapper (`wsl.exe`, `ssh`) that reaches the host, and only its stdio is
    /// this transport's business. Spawned as given, with no shell in between,
    /// since the wrapper has already been told exactly what to run.
    pub fn from_command(mut command: util::command::Command, cx: &AsyncApp) -> Result<Self> {
        command
            .stdin(util::command::Stdio::piped())
            .stdout(util::command::Stdio::piped())
            .stderr(util::command::Stdio::piped())
            // Ending the wrapper is the only way left to end a driver on a
            // remote host, and one left running holds the database open there.
            .kill_on_drop(true);

        let mut child = command
            .spawn()
            .with_context(|| format!("failed to spawn database driver {command:?}"))?;

        let (stdin, stdout, stderr) = (
            child.stdin.take().context("driver stdin was not piped")?,
            child.stdout.take().context("driver stdout was not piped")?,
            child.stderr.take().context("driver stderr was not piped")?,
        );
        Ok(Self::speak_through(
            DriverProcess::Prepared(child),
            stdin,
            stdout,
            stderr,
            cx,
        ))
    }

    fn speak_through(
        child: DriverProcess,
        stdin: impl AsyncWrite + Unpin + Send + 'static,
        stdout: impl AsyncRead + Unpin + Send + 'static,
        stderr: impl AsyncRead + Unpin + Send + 'static,
        cx: &AsyncApp,
    ) -> Self {
        let (inbound_tx, inbound) = channel::unbounded::<String>();
        let (outbound, outbound_rx) = channel::unbounded::<String>();
        let (errors_tx, errors) = channel::unbounded::<String>();

        cx.spawn(async move |_| write_lines(stdin, outbound_rx).log_err().await)
            .detach();
        cx.spawn(async move |_| read_lines(stdout, inbound_tx).await)
            .detach();
        cx.spawn(async move |_| read_lines(stderr, errors_tx).await)
            .detach();

        Self {
            outbound,
            inbound,
            errors,
            child,
        }
    }
}

async fn read_lines<R>(reader: R, sender: channel::Sender<String>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    while let Ok(read) = reader.read_line(&mut line).await {
        if read == 0 {
            break;
        }
        if sender.send(line.clone()).await.is_err() {
            break;
        }
        line.clear();
    }
}

async fn write_lines<W>(writer: W, messages: channel::Receiver<String>) -> Result<()>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut writer = BufWriter::new(writer);
    let mut messages = Box::pin(messages);
    while let Some(message) = messages.next().await {
        writer.write_all(message.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
    }
    Ok(())
}

#[async_trait]
impl Transport for StdioTransport {
    async fn send(&self, message: String) -> Result<()> {
        Ok(self.outbound.send(message).await?)
    }

    fn receive(&self) -> Pin<Box<dyn Stream<Item = String> + Send>> {
        Box::pin(self.inbound.clone())
    }

    fn receive_err(&self) -> Pin<Box<dyn Stream<Item = String> + Send>> {
        Box::pin(self.errors.clone())
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        // `kill_on_drop` covers the ordinary path; this covers the one where
        // the child outlives its handle because something else held it.
        if let Err(error) = self.child.kill() {
            log::warn!("could not kill database driver: {error}");
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Duration;

    async fn next_line(
        lines: &mut Pin<Box<dyn Stream<Item = String> + Send>>,
        cx: &gpui::TestAppContext,
    ) -> Option<String> {
        let timer = cx.background_executor.timer(Duration::from_secs(10));
        match futures::future::select(lines.next(), timer).await {
            futures::future::Either::Left((line, _)) => line,
            futures::future::Either::Right(_) => panic!("the driver said nothing for 10s"),
        }
    }

    #[gpui::test]
    async fn a_prepared_command_speaks_line_json(cx: &mut gpui::TestAppContext) {
        cx.executor().allow_parking();
        let mut command = util::command::Command::new("/bin/sh");
        command.args([
            "-c",
            "while IFS= read -r line; do printf '%s\\n' \"$line\"; done",
        ]);
        let transport = StdioTransport::from_command(command, &cx.to_async()).expect("sh starts");

        let mut lines = transport.receive();
        transport
            .send(r#"{"jsonrpc":"2.0","id":1}"#.to_string())
            .await
            .expect("the driver's stdin is open");
        assert_eq!(
            next_line(&mut lines, cx).await.as_deref(),
            Some("{\"jsonrpc\":\"2.0\",\"id\":1}\n")
        );
    }

    /// Dropping the transport must end the process even when the process
    /// ignores its stdin closing: a remote wrapper is exactly such a process.
    #[gpui::test]
    async fn dropping_a_prepared_driver_ends_its_process(cx: &mut gpui::TestAppContext) {
        cx.executor().allow_parking();
        let mut command = util::command::Command::new("/bin/sh");
        command.args(["-c", "echo ready; exec sleep 600"]);
        let transport = StdioTransport::from_command(command, &cx.to_async()).expect("sh starts");

        let mut lines = transport.receive();
        assert_eq!(next_line(&mut lines, cx).await.as_deref(), Some("ready\n"));
        drop(transport);
        assert_eq!(
            next_line(&mut lines, cx).await,
            None,
            "stdout closes only once the process holding it has ended"
        );
    }
}
