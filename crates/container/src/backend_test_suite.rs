//! What every real backend is held to.
//!
//! Deliberately written against the trait alone, so the same body runs over
//! Docker, Podman and Kubernetes. Its job is not to check that a particular
//! engine works -- each backend's own tests do that -- but that the trait means
//! the same thing to all of them.
//!
//! Docker is on the machine of anyone working on this and in CI, so the suite
//! always has at least one real engine to run against. Without that it would be
//! skipped in every run and could rot unnoticed -- the trap
//! `database/src/driver_test_suite.rs` records having fallen into.

use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use util::command::{Command, CommandHost};
use util::paths::PathStyle;

use crate::backend::{ContainerBackend, ContainerError};
use crate::operations::{ComposeOp, ComposeTarget, Op, RunSpec};
use crate::resource::{Resource, ResourceKind, RunState};

/// Every kind this crate knows, so the suite can check the ones a backend
/// *declines* as well as the ones it claims.
const ALL_KINDS: &[ResourceKind] = &[
    ResourceKind::Container,
    ResourceKind::Image,
    ResourceKind::Volume,
    ResourceKind::Network,
    ResourceKind::ComposeProject,
    ResourceKind::Pod,
];

/// Holds `backend` to the trait's contract.
///
/// Returns the number of checks that actually reached the engine, so a caller
/// can tell "the engine answered every question" from "the engine was not there
/// and every question was skipped" -- a suite that silently checks nothing is
/// the failure mode this count exists to expose.
pub async fn shared_suite(backend: &dyn ContainerBackend) -> Result<usize, String> {
    let supported = backend.supported_kinds();
    if supported.is_empty() {
        return Err("a backend that supports no kind cannot be used at all".into());
    }

    let mut reached = 0usize;
    check_declined_read_side(backend).await?;
    check_write_side_contract(backend).await?;
    let mut samples: Vec<Resource> = Vec::new();

    for kind in ALL_KINDS {
        let claims = supported.contains(kind);
        match backend.list(*kind).await {
            Ok(resources) => {
                if !claims {
                    return Err(format!(
                        "{:?} listed {kind:?}, which it does not declare in \
                         supported_kinds -- the view asks supported_kinds, so \
                         anything outside it is unreachable and must not exist",
                        backend.kind()
                    ));
                }
                reached += 1;
                // The first of each kind, and the first one that is up so the
                // stats check has something to sample.
                samples.extend(resources.first().cloned());
                samples.extend(
                    resources
                        .iter()
                        .find(|resource| resource.state == RunState::Running)
                        .filter(|running| resources.first().map(|first| &first.id) != Some(&running.id))
                        .cloned(),
                );
                for resource in &resources {
                    if resource.kind != *kind {
                        return Err(format!(
                            "asked for {kind:?} and got a {:?} back",
                            resource.kind
                        ));
                    }
                    if resource.id.is_empty() {
                        return Err(format!(
                            "a {kind:?} came back with no id; `act` would have \
                             nothing to address"
                        ));
                    }
                }
            }
            Err(ContainerError::EngineMissing { .. })
            | Err(ContainerError::EngineNotRunning { .. })
            // `NotConfigured` joined these when the second backend arrived: an
            // engine that is installed and reachable but has nothing chosen to
            // look at is just as unable to answer, and just as much *not* a
            // failure of the trait. Kubernetes on a machine with no kubeconfig
            // context is the ordinary case, not an exception.
            | Err(ContainerError::NotConfigured { .. })
            | Err(ContainerError::HostUnavailable { .. }) => {
                // The engine cannot answer. Nothing about the trait is disproven,
                // and every remaining kind would say the same, so stop.
                return Ok(reached);
            }
            Err(error) => {
                if claims {
                    return Err(format!("listing a declared kind {kind:?} failed: {error}"));
                }
                reached += 1;
            }
        }
    }

    for kind in supported {
        let actions = backend.supported_actions(*kind);
        if *kind == ResourceKind::Image && !actions.is_empty() {
            return Err("an image cannot be started, stopped or restarted".into());
        }
    }

    reached += check_live_read_side(backend, &samples).await?;
    Ok(reached)
}

/// What a backend says it does not offer, it must not do -- and saying so
/// needs no engine, because a declined call returns before any command exists.
async fn check_declined_read_side(backend: &dyn ContainerBackend) -> Result<(), String> {
    for kind in ALL_KINDS {
        if !backend.supports_inspect(*kind)
            && !matches!(backend.inspect(*kind, "suite-probe").await, Ok(None))
        {
            return Err(format!(
                "{:?} does not support inspect for {kind:?} yet answered it",
                backend.kind()
            ));
        }
        if !backend.supports_usages(*kind)
            && !matches!(backend.usages(*kind, "suite-probe").await, Ok(None))
        {
            return Err(format!(
                "{:?} does not support usages for {kind:?} yet answered it",
                backend.kind()
            ));
        }
    }
    // Asking about nobody is a question every backend can answer without an
    // engine: "no stats" if it offers none, an empty sample if it does.
    match (backend.supports_stats(), backend.stats(&[]).await) {
        (false, Ok(None)) => {}
        (true, Ok(Some(stats))) if stats.is_empty() => {}
        (supported, other) => {
            return Err(format!(
                "stats for no ids answered {other:?} while supports_stats is {supported}"
            ));
        }
    }
    Ok(())
}

/// The write side's contract, which needs no engine to check: a value that
/// would be read as an option is refused before any command exists, and what a
/// backend does not offer is refused as such. Only inputs that must be
/// refused are used, so an engine that does offer the operation is never asked
/// to do anything.
async fn check_write_side_contract(backend: &dyn ContainerBackend) -> Result<(), String> {
    let hostile = "--suite-probe x";
    let kind = backend.kind();
    let expect_refusal =
        |what: &str, offered: bool, outcome: Result<(), ContainerError>| match (offered, outcome) {
            (true, Err(ContainerError::Invalid { .. }))
            | (false, Err(ContainerError::NotOffered { .. })) => Ok(()),
            (offered, other) => Err(format!(
                "{kind:?}: {what} with a hostile value answered {other:?} \
                 while offers is {offered}"
            )),
        };
    expect_refusal(
        "pull",
        backend.offers(Op::ImagePull),
        backend.pull(hostile).await,
    )?;
    expect_refusal(
        "tag",
        backend.offers(Op::ImageTag),
        backend.tag(hostile, "ok:1").await,
    )?;
    expect_refusal(
        "history",
        backend.offers(Op::ImageHistory),
        backend.history(hostile).await.map(|_| ()),
    )?;
    expect_refusal(
        "create_volume",
        backend.offers(Op::VolumeCreate),
        backend.create_volume(hostile, None, &[]).await,
    )?;
    expect_refusal(
        "create_network",
        backend.offers(Op::NetworkCreate),
        backend.create_network(hostile, None, None).await,
    )?;
    expect_refusal(
        "connect",
        backend.offers(Op::NetworkConnect),
        backend.connect(hostile, "db", None).await,
    )?;
    expect_refusal(
        "disconnect",
        backend.offers(Op::NetworkConnect),
        backend.disconnect("net", hostile).await,
    )?;
    expect_refusal(
        "run_command",
        backend.offers(Op::ImageRun),
        backend
            .run_command(&RunSpec {
                image: hostile.into(),
                ..RunSpec::default()
            })
            .map(|_| ()),
    )?;

    let project = ComposeTarget::new(hostile);
    if backend
        .compose_command(&project, ComposeOp::Up, &[])
        .is_some()
    {
        return Err(format!("{kind:?} built a compose command for {hostile:?}"));
    }
    if backend.pull_command(hostile).is_some() {
        return Err(format!("{kind:?} built a pull command for {hostile:?}"));
    }
    match backend.compose_down_plan(&project, true).await {
        Ok(None) if !backend.offers(Op::Compose) => {}
        Err(ContainerError::Invalid { .. }) if backend.offers(Op::Compose) => {}
        other => {
            return Err(format!(
                "{kind:?}: a compose down plan for {hostile:?} answered {other:?}"
            ));
        }
    }
    if !backend.offers(Op::Compose) && backend.compose_available().await {
        return Err(format!("{kind:?} has no compose yet reports it available"));
    }
    Ok(())
}

/// Inspect and usages against something the engine just listed. A resource that
/// vanished between the list and the call is a race, not a failure.
async fn check_live_read_side(
    backend: &dyn ContainerBackend,
    samples: &[Resource],
) -> Result<usize, String> {
    let mut reached = 0usize;
    for sample in samples {
        if backend.supports_inspect(sample.kind) {
            match backend.inspect(sample.kind, &sample.id).await {
                Ok(Some(inspection)) => {
                    reached += 1;
                    if inspection.sections.is_empty() {
                        return Err(format!(
                            "inspecting {:?} {} gave no sections",
                            sample.kind, sample.id
                        ));
                    }
                    if inspection.raw_json.is_empty() {
                        return Err(format!(
                            "inspecting {:?} {} gave no raw JSON",
                            sample.kind, sample.id
                        ));
                    }
                }
                Ok(None) => {
                    return Err(format!(
                        "{:?} supports inspect for {:?} yet answered None",
                        backend.kind(),
                        sample.kind
                    ));
                }
                Err(ContainerError::NotFound { .. }) => {}
                Err(error) => {
                    return Err(format!(
                        "inspecting {:?} {} failed: {error}",
                        sample.kind, sample.id
                    ));
                }
            }
        }
        if backend.supports_stats()
            && sample.state == RunState::Running
            && matches!(sample.kind, ResourceKind::Container | ResourceKind::Pod)
        {
            // `None` is a legitimate answer (no metrics server); a row is only
            // useful if its id matches the one the list gave.
            match backend.stats(std::slice::from_ref(&sample.id)).await {
                Ok(Some(rows)) => {
                    reached += 1;
                    if let Some(wrong) = rows.iter().find(|row| row.id != sample.id) {
                        return Err(format!(
                            "stats for {} answered a row for {}; the view matches rows by id",
                            sample.id, wrong.id
                        ));
                    }
                }
                Ok(None) | Err(ContainerError::NotFound { .. }) => {}
                Err(error) => return Err(format!("stats for {} failed: {error}", sample.id)),
            }
        }
        if backend.supports_usages(sample.kind) {
            match backend.usages(sample.kind, &sample.id).await {
                Ok(Some(users)) => {
                    reached += 1;
                    if let Some(wrong) = users
                        .iter()
                        .find(|user| user.kind != ResourceKind::Container)
                    {
                        return Err(format!(
                            "usages of {:?} listed a {:?}",
                            sample.kind, wrong.kind
                        ));
                    }
                }
                Ok(None) => {
                    return Err(format!(
                        "{:?} supports usages for {:?} yet answered None",
                        backend.kind(),
                        sample.kind
                    ));
                }
                Err(ContainerError::NotFound { .. }) => {}
                Err(error) => {
                    return Err(format!(
                        "usages of {:?} {} failed: {error}",
                        sample.kind, sample.id
                    ));
                }
            }
        }
    }
    Ok(reached)
}

/// One command a backend asked its host for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedCommand {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
}

/// What a scripted command prints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// Exits 0 having printed this on stdout.
    Stdout(String),
    /// Exits 1 having printed this on stderr.
    Failure(String),
}

impl Reply {
    pub fn stdout(text: &str) -> Self {
        Reply::Stdout(text.to_string())
    }

    pub fn failure(stderr: &str) -> Self {
        Reply::Failure(stderr.to_string())
    }

    fn command(&self) -> Command {
        let (script, text) = match self {
            Reply::Stdout(text) => ("printf '%s' \"$0\"", text),
            Reply::Failure(text) => ("printf '%s' \"$0\" >&2; exit 1", text),
        };
        let mut command = Command::new("sh");
        command.args(["-c", script, text]);
        command
    }
}

/// A host that writes down every command it is asked for.
///
/// What it hands back runs nothing useful: by default a program that is not
/// there, so whatever the backend does next it does without an engine. A
/// backend that reached an engine without asking this host would leave it
/// with nothing written down, which is what the tests built on it check.
pub struct RecordingHost {
    /// `None` for a host standing in for this machine.
    name: Option<String>,
    answer: Option<(String, Vec<String>)>,
    /// Per-command answers; when present they win over `answer`. The first
    /// route whose words are all among a command's arguments replies.
    routes: Vec<(Vec<String>, Reply)>,
    refusal: Option<String>,
    environment: Vec<(String, String)>,
    recorded: Mutex<Vec<RecordedCommand>>,
}

impl RecordingHost {
    /// Hands back a program that does not exist for every command.
    pub fn new(name: &str) -> Self {
        Self {
            name: Some(name.to_string()),
            answer: None,
            routes: Vec::new(),
            refusal: None,
            environment: Vec::new(),
            recorded: Mutex::new(Vec::new()),
        }
    }

    /// The same host, with `variables` in the environment its tools start with.
    pub fn with_environment(self, variables: &[(&str, &str)]) -> Self {
        Self {
            environment: variables
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            ..self
        }
    }

    /// Stands in for this machine: no host name, like `LocalCommandHost`.
    pub fn this_machine() -> Self {
        Self {
            name: None,
            ..Self::new("this machine")
        }
    }

    /// Hands back `program args` for every command, whatever was asked for.
    pub fn answering_with(name: &str, program: &str, args: &[&str]) -> Self {
        Self {
            answer: Some((
                program.to_string(),
                args.iter().map(|arg| arg.to_string()).collect(),
            )),
            ..Self::new(name)
        }
    }

    /// Answers each command by what it asks, so a backend that runs several
    /// commands in a row gets a different reply to each.
    ///
    /// Unix only, like every canned answer here: the reply is printed by `sh`.
    /// A command no route matches fails with `unscripted command` on stderr,
    /// which a test notices rather than a silent empty success.
    pub fn scripted(name: &str, routes: &[(&[&str], Reply)]) -> Self {
        Self {
            routes: routes
                .iter()
                .map(|(words, reply)| {
                    (
                        words.iter().map(|word| word.to_string()).collect(),
                        reply.clone(),
                    )
                })
                .collect(),
            ..Self::new(name)
        }
    }

    /// Refuses every command, the way a remote-control connection does.
    pub fn refusing(name: &str, reason: &str) -> Self {
        Self {
            refusal: Some(reason.to_string()),
            ..Self::new(name)
        }
    }

    pub fn recorded(&self) -> Vec<RecordedCommand> {
        self.recorded.lock().clone()
    }
}

impl CommandHost for RecordingHost {
    fn command(
        &self,
        program: &str,
        args: &[&str],
        env: &[(&str, &str)],
        cwd: Option<&Path>,
    ) -> anyhow::Result<Command> {
        self.recorded.lock().push(RecordedCommand {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            env: env
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            cwd: cwd.map(Path::to_path_buf),
        });
        if let Some(reason) = &self.refusal {
            anyhow::bail!(
                "{program} cannot be run on {}: {reason}",
                self.name.as_deref().unwrap_or("this machine")
            );
        }
        if !self.routes.is_empty() {
            let reply = self
                .routes
                .iter()
                .find(|(words, _)| words.iter().all(|word| args.contains(&word.as_str())))
                .map(|(_, reply)| reply.clone())
                .unwrap_or_else(|| Reply::failure(&format!("unscripted command: {args:?}")));
            return Ok(reply.command());
        }
        Ok(match &self.answer {
            Some((program, args)) => {
                let mut command = Command::new(program);
                command.args(args);
                command
            }
            None => Command::new("zode-recorded-command-that-does-not-exist"),
        })
    }

    fn path_style(&self) -> PathStyle {
        PathStyle::Posix
    }

    fn remote_name(&self) -> Option<String> {
        self.name.clone()
    }

    fn environment_variable(&self, name: &str) -> Option<String> {
        self.environment
            .iter()
            .find(|(variable, _)| variable == name)
            .map(|(_, value)| value.clone())
    }
}
