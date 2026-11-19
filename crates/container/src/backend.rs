use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use util::command::{Command, CommandHost};

use crate::inspect::Inspection;
use crate::kubeconfig::{ConfigSource, ConfigTarget};
use crate::operations::{ComposeOp, ComposeTarget, ImageLayer, Op, RunSpec};
use crate::resource::{Resource, ResourceAction, ResourceKind};
use crate::stats::Stats;

/// Which engine is answering.
///
/// Docker and Podman share a command line and so share a backend; they are told
/// apart here only so the UI can label them, and so Podman can offer the one
/// kind Docker has no word for.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BackendKind {
    Docker,
    Podman,
    Kubernetes,
}

impl BackendKind {
    pub fn label(self) -> &'static str {
        match self {
            BackendKind::Docker => "Docker",
            BackendKind::Podman => "Podman",
            BackendKind::Kubernetes => "Kubernetes",
        }
    }
}

/// Why a backend could not answer.
///
/// `EngineMissing` and `EngineNotRunning` are separate on purpose: "not
/// installed" and "installed but the daemon is down" need different words in
/// front of the user, and collapsing them into one error is how a panel ends up
/// telling somebody to install what they already have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContainerError {
    /// The command-line tool is not on `PATH`.
    EngineMissing { program: String },
    /// The tool is there, but it cannot reach its daemon or cluster.
    EngineNotRunning { program: String },
    /// The tool is there and usable, but nothing has been chosen for it to look
    /// at -- a kubeconfig with no context, say.
    ///
    /// Separate from `EngineNotRunning` because the two are cured differently
    /// and because `kubectl` cannot tell them apart on its own: with no context
    /// it falls back to `localhost:8080` and reports a refused connection, which
    /// would send somebody looking for a cluster that was never the problem.
    /// Docker never returns this.
    NotConfigured { detail: String },
    /// The tool answered, and the answer was "you may not".
    NotAuthorized { detail: String },
    /// Asked about something that is no longer there.
    NotFound { id: String },
    /// The command ran and failed for some other reason. `stderr` is the
    /// engine's own words and is untrusted text -- show it, never act on it.
    CommandFailed { program: String, stderr: String },
    /// The command succeeded and its output could not be read.
    Parse { detail: String },
    /// The machine the engine runs on cannot be asked at all: a remote
    /// project whose connection cannot start a command there. Never answered
    /// by running the engine on this machine instead.
    HostUnavailable { detail: String },
    /// A value was refused before any command was built: a name that would be
    /// read as an option, a reference with a space in it. Nothing ran.
    Invalid { detail: String },
    /// This engine has no such operation. Reaching it means the caller did not
    /// ask [`ContainerBackend::offers`] first.
    NotOffered { operation: String },
}

impl std::fmt::Display for ContainerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerError::EngineMissing { program } => {
                write!(f, "{program} is not installed")
            }
            ContainerError::EngineNotRunning { program } => {
                write!(f, "{program} is installed but not running")
            }
            ContainerError::NotConfigured { detail } => write!(f, "not configured: {detail}"),
            ContainerError::NotAuthorized { detail } => write!(f, "not authorized: {detail}"),
            ContainerError::NotFound { id } => write!(f, "no longer there: {id}"),
            ContainerError::CommandFailed { program, stderr } => {
                write!(f, "{program} failed: {stderr}")
            }
            ContainerError::Parse { detail } => write!(f, "could not read the output: {detail}"),
            ContainerError::HostUnavailable { detail } => write!(f, "{detail}"),
            ContainerError::Invalid { detail } => write!(f, "refused: {detail}"),
            ContainerError::NotOffered { operation } => {
                write!(f, "{operation} is not offered by this engine")
            }
        }
    }
}

impl std::error::Error for ContainerError {}

/// Exit status a POSIX shell, and `env`, give for a program not on `PATH`.
///
/// What a missing engine looks like through a remote host's wrapper, which
/// itself always starts: the spawn's own `NotFound` only ever describes this
/// machine.
pub(crate) const COMMAND_NOT_FOUND_EXIT: i32 = 127;

/// ssh's own exit status when it could not reach the host, kept apart from
/// the tool's: the engines this panel runs never exit with it themselves.
const WRAPPER_FAILED_EXIT: i32 = 255;

/// Why `program` could not be started on `host`.
///
/// For a remote host the process started here is only the wrapper (`wsl.exe`,
/// `ssh`), so its `NotFound` says the wrapper is missing on this machine, not
/// that the engine is missing on the host.
pub(crate) fn spawn_failure(
    host: &dyn CommandHost,
    program: &str,
    error: std::io::Error,
) -> ContainerError {
    match host.remote_name() {
        Some(name) if error.kind() == std::io::ErrorKind::NotFound => {
            ContainerError::HostUnavailable {
                detail: format!("the command that reaches {name} could not be started: {error}"),
            }
        }
        None if error.kind() == std::io::ErrorKind::NotFound => ContainerError::EngineMissing {
            program: program.to_string(),
        },
        _ => ContainerError::CommandFailed {
            program: program.to_string(),
            stderr: error.to_string(),
        },
    }
}

/// What an unsuccessful exit says before the engine's own words are read:
/// the program was not found, or the wrapper never reached the host.
pub(crate) fn exit_failure(
    host: &dyn CommandHost,
    program: &str,
    code: Option<i32>,
    stderr: &str,
) -> Option<ContainerError> {
    match (code, host.remote_name()) {
        (Some(COMMAND_NOT_FOUND_EXIT), _) => Some(ContainerError::EngineMissing {
            program: program.to_string(),
        }),
        (Some(WRAPPER_FAILED_EXIT), Some(name)) => Some(ContainerError::HostUnavailable {
            detail: format!("{name} could not be reached: {}", stderr.trim()),
        }),
        _ => None,
    }
}

/// `program args` on `host`, or why that host cannot run anything.
pub(crate) fn host_command(
    host: &dyn CommandHost,
    program: &str,
    args: &[&str],
) -> Result<Command, ContainerError> {
    host.command(program, args, &[], None)
        .map_err(|error| ContainerError::HostUnavailable {
            detail: format!("{error:#}"),
        })
}

/// Something the panel learned about a resource without asking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendEvent {
    /// Something changed under this kind; the list for it is now stale.
    Stale { kind: ResourceKind },
}

/// One engine the panel can talk to.
///
/// `supported_kinds` and `supported_actions` are what keep this trait honest: a
/// Kubernetes pod is not a Docker container, and the view is written to ask what
/// a backend offers rather than to branch on which backend it is. The day a
/// `pod_phase` field appears on a Docker type, this seam has been breached.
#[async_trait]
pub trait ContainerBackend: Send + Sync {
    fn kind(&self) -> BackendKind;

    /// The kinds this engine has a word for. Docker has no pods; Kubernetes has
    /// no volumes in the sense this panel means.
    fn supported_kinds(&self) -> &'static [ResourceKind];

    /// What may be done to a resource of this kind. An image does not start; a
    /// pod does not restart.
    fn supported_actions(&self, kind: ResourceKind) -> &'static [ResourceAction];

    /// A file on disk naming what this engine can be aimed at.
    ///
    /// `None` is the default, and what keeps Docker and Podman out of this --
    /// neither reads a config file the panel can offer a picker over. `None` is
    /// also what keeps the view from asking which engine it holds: it asks this
    /// instead, and draws a picker only when the answer is `Some`.
    fn config_source(&self) -> Option<ConfigSource> {
        None
    }

    /// What `path` offers to be aimed at, if this engine reads one.
    ///
    /// `path` is `None` when the caller wants whatever this engine reads by
    /// default (its own current file, unchosen). The default implementation
    /// answers with nothing to choose from, which is correct for every engine
    /// that has no such file.
    async fn config_targets(
        &self,
        path: Option<&Path>,
    ) -> Result<Vec<ConfigTarget>, ContainerError> {
        let _ = path;
        Ok(Vec::new())
    }

    /// A new backend aimed at `path` and `target`, or `None` when this engine has
    /// nothing to aim.
    ///
    /// A new value rather than a mutation: the panel holds its backends behind
    /// `Arc<dyn ContainerBackend>`, which offers no `&mut`, and interior
    /// mutability here would put a lock in the path of every `list`.
    fn aimed_at(
        &self,
        path: Option<PathBuf>,
        target: Option<ConfigTarget>,
    ) -> Option<Arc<dyn ContainerBackend>> {
        let _ = (path, target);
        None
    }

    async fn list(&self, kind: ResourceKind) -> Result<Vec<Resource>, ContainerError>;

    /// Does `action` to the resource `id` of kind `kind`.
    ///
    /// The kind is a parameter and not inferred from the id, because the command
    /// differs by kind on the same engine: `podman start` and `podman pod start`
    /// are different verbs for the same word. Callers must ask
    /// `supported_actions(kind)` first; anything outside that answer is an error,
    /// never a silent success.
    async fn act(
        &self,
        kind: ResourceKind,
        action: ResourceAction,
        id: &str,
    ) -> Result<(), ContainerError>;

    /// A stream that says when a list went stale, if this engine has one.
    /// `None` means the panel must poll instead.
    fn watch(&self) -> Option<BoxStream<'static, BackendEvent>>;

    /// The command that follows this resource's log, if it has one.
    ///
    /// Handed back as a program and a list of arguments, never as one string:
    /// the id comes from the engine but the *name* inside it was chosen by
    /// whoever created the resource, and a name with a space or a semicolon in it
    /// must not be able to become two commands. The caller runs it in a terminal
    /// and never through a shell.
    ///
    /// The backend builds it because only the backend knows the syntax --
    /// `docker logs -f x` and `kubectl logs -f -n ns x` are the same intention in
    /// two vocabularies, and the view must not learn either.
    fn logs_command(&self, kind: ResourceKind, id: &str) -> Option<(String, Vec<String>)>;

    /// Everything a prune of this scope would delete, listed one by one.
    ///
    /// `None` for an engine with no such notion. For Docker this is the whole
    /// reason [`crate::DestructivePlan`] exists: `docker system prune` has no
    /// `--dry-run`, so the only way to tell somebody what they are about to lose
    /// is to go and find out first.
    async fn prune_targets(
        &self,
        scope: crate::PruneScope,
    ) -> Option<Result<Vec<Resource>, ContainerError>>;

    /// Carries out a plan whose confirmation has already been given.
    ///
    /// Takes the plan and not a kind and an id, so there is no way to remove
    /// something that was never enumerated and shown.
    async fn destroy(&self, plan: &crate::DestructivePlan) -> Result<(), ContainerError>;

    /// The command that opens a shell inside this resource, if it has one.
    ///
    /// Same argument-splitting rule as [`Self::logs_command`], and it matters
    /// more here: this one is interactive and long-lived.
    fn exec_command(&self, kind: ResourceKind, id: &str) -> Option<(String, Vec<String>)>;

    /// What forcing the removal of a `kind` does on this engine, as the
    /// sentence the confirmation shows -- or `None` when forcing does nothing
    /// there, in which case the view must not offer it.
    ///
    /// Asked per engine and kind because the same flag means different things:
    /// Podman's `--force` removes the containers that use an image, volume or
    /// network, while Docker's `volume rm -f` and `network rm -f` still refuse
    /// what is in use. Use the answer as the plan's warning with
    /// [`crate::DestructivePlan::forced_with_effect`]; [`Self::destroy`]
    /// refuses a forced plan for a kind that answers `None`.
    fn force_effect(&self, kind: ResourceKind) -> Option<String> {
        let _ = kind;
        None
    }

    /// Whether [`Self::inspect`] answers for this kind. What the view asks to
    /// decide whether to draw an Inspect tab, so it never has to call and read
    /// `None` as "no".
    fn supports_inspect(&self, kind: ResourceKind) -> bool {
        let _ = kind;
        false
    }

    /// The full description of one resource, as sections plus the engine's own
    /// JSON. `Ok(None)` when [`Self::supports_inspect`] says no.
    ///
    /// Secret-looking environment values come back masked; see
    /// [`crate::Inspection`].
    async fn inspect(
        &self,
        kind: ResourceKind,
        id: &str,
    ) -> Result<Option<Inspection>, ContainerError> {
        let _ = (kind, id);
        Ok(None)
    }

    /// Whether [`Self::stats`] answers at all.
    fn supports_stats(&self) -> bool {
        false
    }

    /// One sample of resource usage for each of `ids`, taken now rather than
    /// streamed: the view polls while the tab is visible and stops when it is
    /// not. `Ok(None)` when this engine cannot say -- Kubernetes without a
    /// metrics server, for one. An id the engine reports nothing for is simply
    /// absent from the answer.
    async fn stats(&self, ids: &[String]) -> Result<Option<Vec<Stats>>, ContainerError> {
        let _ = ids;
        Ok(None)
    }

    /// Whether [`Self::usages`] answers for this kind.
    fn supports_usages(&self, kind: ResourceKind) -> bool {
        let _ = kind;
        false
    }

    /// The containers that use a volume or a network, or were created from an
    /// image -- the "used by" list a removal should be weighed against.
    /// `Ok(None)` when [`Self::supports_usages`] says no.
    async fn usages(
        &self,
        kind: ResourceKind,
        id: &str,
    ) -> Result<Option<Vec<Resource>>, ContainerError> {
        let _ = (kind, id);
        Ok(None)
    }

    /// Whether this engine has `op` at all -- what the view asks before it
    /// draws the entry point. Static: it says what the engine *can* do, not
    /// whether a tool it needs is installed (see [`Self::compose_available`]).
    fn offers(&self, op: Op) -> bool {
        let _ = op;
        false
    }

    /// Whether the compose tool is there: `docker compose` as the v2 plugin,
    /// `podman compose` only if some provider answers. Asked, not assumed,
    /// because an engine without it fails in a way that reads as the project
    /// being broken. Always false when [`Self::offers`] says no.
    async fn compose_available(&self) -> bool {
        false
    }

    /// A compose verb to run in a terminal, so its output streams. `None` when
    /// compose is not offered or a value was refused (a project, file or
    /// service that would be read as an option).
    ///
    /// `services` limits the verb to those services; empty means all of them.
    fn compose_command(
        &self,
        project: &ComposeTarget,
        op: ComposeOp,
        services: &[String],
    ) -> Option<(String, Vec<String>)> {
        let _ = (project, op, services);
        None
    }

    /// What `compose down` would remove for `project`, listed one by one, or
    /// `Ok(None)` when there is nothing to take down or compose is not offered.
    /// `with_volumes` is the `--volumes` choice and lists the project's volumes
    /// too. Run it through [`Self::destroy`].
    async fn compose_down_plan(
        &self,
        project: &ComposeTarget,
        with_volumes: bool,
    ) -> Result<Option<crate::DestructivePlan>, ContainerError> {
        let _ = (project, with_volumes);
        Ok(None)
    }

    /// A plan for removing `targets`, with what depends on each of them
    /// attached when the engine can say (see [`Self::usages`]). The way to
    /// build a removal plan: it is `DestructivePlan::remove` plus the lookup the
    /// dialog needs to say "these containers use it".
    ///
    /// Dependents are looked up by each target's `id`. For an image row that is
    /// the id every tag of the image shares, so two rows of one image list the
    /// same containers: the list is what would block removing the image's last
    /// tag, which is the conservative thing to show. The rows themselves are
    /// told apart by [`crate::row_key`].
    async fn removal_plan(
        &self,
        kind: ResourceKind,
        targets: Vec<Resource>,
    ) -> Result<Option<crate::DestructivePlan>, ContainerError> {
        let Some(mut plan) = crate::DestructivePlan::remove(kind, targets) else {
            return Ok(None);
        };
        if !self.supports_usages(kind) {
            return Ok(Some(plan));
        }
        let mut dependents = Vec::new();
        for target in plan.targets() {
            let users = self.usages(kind, &target.id).await?.unwrap_or_default();
            if !users.is_empty() {
                dependents.push(crate::destructive::Dependents {
                    target_id: target.id.clone(),
                    containers: users,
                });
            }
        }
        plan = plan.with_dependents(dependents);
        Ok(Some(plan))
    }

    /// `pull <reference>` in a terminal, for its progress output.
    fn pull_command(&self, reference: &str) -> Option<(String, Vec<String>)> {
        let _ = reference;
        None
    }

    /// Pulls an image and waits. Prefer [`Self::pull_command`] when somebody
    /// is watching.
    async fn pull(&self, reference: &str) -> Result<(), ContainerError> {
        let _ = reference;
        Err(not_offered("pulling an image"))
    }

    async fn tag(&self, source: &str, target: &str) -> Result<(), ContainerError> {
        let _ = (source, target);
        Err(not_offered("tagging an image"))
    }

    /// The layers an image was built from, newest first.
    async fn history(&self, id: &str) -> Result<Vec<ImageLayer>, ContainerError> {
        let _ = id;
        Err(not_offered("an image's history"))
    }

    /// `run` for `spec`, to start in a terminal. Every value is its own
    /// argument; an error names the value that was refused.
    fn run_command(&self, spec: &RunSpec) -> Result<(String, Vec<String>), ContainerError> {
        let _ = spec;
        Err(not_offered("running an image"))
    }

    async fn create_volume(
        &self,
        name: &str,
        driver: Option<&str>,
        labels: &[(String, String)],
    ) -> Result<(), ContainerError> {
        let _ = (name, driver, labels);
        Err(not_offered("creating a volume"))
    }

    async fn create_network(
        &self,
        name: &str,
        driver: Option<&str>,
        subnet: Option<&str>,
    ) -> Result<(), ContainerError> {
        let _ = (name, driver, subnet);
        Err(not_offered("creating a network"))
    }

    async fn connect(
        &self,
        network: &str,
        container: &str,
        alias: Option<&str>,
    ) -> Result<(), ContainerError> {
        let _ = (network, container, alias);
        Err(not_offered("connecting a container to a network"))
    }

    /// Detaches a container from a network. Loses no data, so it is a plain
    /// call; the view still confirms it.
    async fn disconnect(&self, network: &str, container: &str) -> Result<(), ContainerError> {
        let _ = (network, container);
        Err(not_offered("disconnecting a container from a network"))
    }
}

pub(crate) fn not_offered(operation: &str) -> ContainerError {
    ContainerError::NotOffered {
        operation: operation.to_string(),
    }
}
