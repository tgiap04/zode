//! Docker and Podman, through their command line.
//!
//! One backend for both: they share these commands, and Podman is told apart
//! only where it genuinely differs. The command line rather than the socket
//! because the CLI already knows where its daemon is -- rootless Podman moves
//! that socket, and reimplementing the search here would be a second place to
//! get it wrong.

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt as _};
use std::io;
use util::command::Command;

use crate::backend::{BackendEvent, BackendKind, ContainerBackend, ContainerError};
use crate::docker_resources::{ComposeProject, DockerImage, DockerNetwork, DockerVolume};
use crate::docker_types::DockerContainer;
use crate::podman::PodmanPod;
use crate::resource::{Resource, ResourceAction, ResourceKind};

/// Podman's kinds are Docker's plus the one word Docker has no equivalent for.
///
/// A pod groups containers; `docker` has no such object, which is why this is a
/// per-engine list rather than a constant shared by both.
const PODMAN_KINDS: &[ResourceKind] = &[
    ResourceKind::Container,
    ResourceKind::Pod,
    ResourceKind::Image,
    ResourceKind::Volume,
    ResourceKind::Network,
    ResourceKind::ComposeProject,
];

const DOCKER_KINDS: &[ResourceKind] = &[
    ResourceKind::Container,
    ResourceKind::Image,
    ResourceKind::Volume,
    ResourceKind::Network,
    ResourceKind::ComposeProject,
];

/// A podman pod starts and stops but does not restart: `podman pod restart`
/// exists in recent versions and is absent in older ones, and offering a button
/// that works on one machine and errors on another is worse than not offering it.
const POD_ACTIONS: &[ResourceAction] = &[ResourceAction::Start, ResourceAction::Stop];

const CONTAINER_ACTIONS: &[ResourceAction] = &[
    ResourceAction::Start,
    ResourceAction::Stop,
    ResourceAction::Restart,
    ResourceAction::Pause,
    ResourceAction::Unpause,
];

pub struct DockerBackend {
    /// `docker` or `podman`. Held as the program name rather than a flag,
    /// because every command below is spelled the same for both.
    program: String,
    kind: BackendKind,
}

impl DockerBackend {
    pub fn docker() -> Self {
        Self {
            program: "docker".into(),
            kind: BackendKind::Docker,
        }
    }

    pub fn podman() -> Self {
        Self {
            program: "podman".into(),
            kind: BackendKind::Podman,
        }
    }

    /// A backend pointed at a named program, for tests that need a program
    /// which certainly is or is not there. Not public: the two shipped engines
    /// are the only ones anything outside this crate may ask for.
    #[cfg(test)]
    pub(crate) fn with_program(program: &str, kind: BackendKind) -> Self {
        Self {
            program: program.into(),
            kind,
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(&self.program);
        command.args(args);
        command
    }

    /// Runs a command and hands back stdout, classifying the ways it can fail.
    ///
    /// The two "engine is not usable" cases are told apart structurally rather
    /// than by reading stderr: a missing binary surfaces as `NotFound` from the
    /// spawn itself, and a live binary that cannot reach its daemon is confirmed
    /// by asking `info` -- which only happens on the error path, so the common
    /// case pays nothing for it.
    async fn stdout(&self, mut command: Command) -> Result<String, ContainerError> {
        let output = match command.output().await {
            Ok(output) => output,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(ContainerError::EngineMissing {
                    program: self.program.clone(),
                });
            }
            Err(error) => {
                return Err(ContainerError::CommandFailed {
                    program: self.program.clone(),
                    stderr: error.to_string(),
                });
            }
        };

        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }

        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if !self.daemon_answers().await {
            return Err(ContainerError::EngineNotRunning {
                program: self.program.clone(),
            });
        }
        Err(ContainerError::CommandFailed {
            program: self.program.clone(),
            stderr,
        })
    }

    /// Whether the daemon is reachable at all.
    async fn daemon_answers(&self) -> bool {
        let mut command = self.command(&["info", "--format", "{{.ServerVersion}}"]);
        match command.output().await {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }
}

/// The command that removes one thing of a given kind.
///
/// `None` for a kind with no removal, which is the only reason this is fallible.
/// `--` before the id every time: the id came from the engine, but the name
/// inside it was chosen by whoever made the resource.
fn remove_args(kind: ResourceKind, id: &str) -> Result<Vec<String>, ContainerError> {
    let args = match kind {
        // Not `-f`: forcing kills a running container, and "remove" was not
        // "kill". A running container refuses, which is the engine telling the
        // truth rather than this crate deciding for somebody.
        ResourceKind::Container => vec!["rm", "--"],
        ResourceKind::Image => vec!["rmi", "--"],
        ResourceKind::Volume => vec!["volume", "rm", "--"],
        ResourceKind::Network => vec!["network", "rm", "--"],
        ResourceKind::Pod => vec!["pod", "rm", "--"],
        other => {
            return Err(ContainerError::CommandFailed {
                program: "docker".into(),
                stderr: format!("{other:?} cannot be removed"),
            });
        }
    };
    let mut args: Vec<String> = args.into_iter().map(String::from).collect();
    args.push(id.to_string());
    Ok(args)
}

impl DockerBackend {
    async fn gather_prune_targets(
        &self,
        scope: crate::PruneScope,
    ) -> Result<Vec<Resource>, ContainerError> {
        let mut targets = Vec::new();

        let images = self
            .stdout(self.command(&["images", "-f", "dangling=true", "--format", "{{json .}}"]))
            .await?;
        targets.extend(
            parse_json_lines::<DockerImage>(&images)
                .into_iter()
                .map(DockerImage::into_resource),
        );

        let stopped = self
            .stdout(self.command(&[
                "ps",
                "-a",
                "-f",
                "status=exited",
                "-f",
                "status=created",
                "--format",
                "{{json .}}",
            ]))
            .await?;
        targets.extend(
            parse_json_lines::<DockerContainer>(&stopped)
                .into_iter()
                .map(DockerContainer::into_resource),
        );

        if scope.includes_volumes() {
            let volumes = self
                .stdout(self.command(&[
                    "volume",
                    "ls",
                    "-f",
                    "dangling=true",
                    "--format",
                    "{{json .}}",
                ]))
                .await?;
            targets.extend(
                parse_json_lines::<DockerVolume>(&volumes)
                    .into_iter()
                    .map(DockerVolume::into_resource),
            );
        }

        Ok(targets)
    }
}

/// Reads however many JSON values stdout holds.
///
/// `--format '{{json .}}'` prints one object per line rather than one array, so
/// a single `from_str` would fail the moment there are two rows. A
/// `StreamDeserializer` reads values back to back and does not care whether they
/// are separated by a newline or pretty-printed over several -- which is the
/// same shape `kubectl --watch` emits, so this reader carries over.
///
/// A row that will not parse is dropped with a log rather than failing the list:
/// one unreadable container must not hide the other nine.
pub(crate) fn parse_json_lines<T: serde::de::DeserializeOwned>(stdout: &str) -> Vec<T> {
    let mut out = Vec::new();
    for value in serde_json::Deserializer::from_str(stdout).into_iter::<T>() {
        match value {
            Ok(value) => out.push(value),
            Err(error) => {
                log::warn!("skipping an unreadable row of container output: {error}");
                break;
            }
        }
    }
    out
}

#[async_trait]
impl ContainerBackend for DockerBackend {
    fn kind(&self) -> BackendKind {
        self.kind
    }

    fn supported_kinds(&self) -> &'static [ResourceKind] {
        match self.kind {
            BackendKind::Podman => PODMAN_KINDS,
            _ => DOCKER_KINDS,
        }
    }

    fn supported_actions(&self, kind: ResourceKind) -> &'static [ResourceAction] {
        match kind {
            ResourceKind::Container => CONTAINER_ACTIONS,
            // `podman pod start|stop` exist, so a pod here really does start and
            // stop -- unlike a Kubernetes pod, which shares only the word. That
            // is the whole reason `supported_actions` is asked per backend and
            // not decided by kind alone.
            ResourceKind::Pod if self.kind == BackendKind::Podman => POD_ACTIONS,
            _ => &[],
        }
    }

    async fn list(&self, kind: ResourceKind) -> Result<Vec<Resource>, ContainerError> {
        match kind {
            ResourceKind::Container => {
                let stdout = self
                    .stdout(self.command(&["ps", "-a", "--format", "{{json .}}"]))
                    .await?;
                Ok(parse_json_lines::<DockerContainer>(&stdout)
                    .into_iter()
                    .map(DockerContainer::into_resource)
                    .collect())
            }
            ResourceKind::Image => {
                let stdout = self
                    .stdout(self.command(&["images", "--format", "{{json .}}"]))
                    .await?;
                Ok(parse_json_lines::<DockerImage>(&stdout)
                    .into_iter()
                    .map(DockerImage::into_resource)
                    .collect())
            }
            ResourceKind::Volume => {
                let stdout = self
                    .stdout(self.command(&["volume", "ls", "--format", "{{json .}}"]))
                    .await?;
                Ok(parse_json_lines::<DockerVolume>(&stdout)
                    .into_iter()
                    .map(DockerVolume::into_resource)
                    .collect())
            }
            ResourceKind::Network => {
                let stdout = self
                    .stdout(self.command(&["network", "ls", "--format", "{{json .}}"]))
                    .await?;
                Ok(parse_json_lines::<DockerNetwork>(&stdout)
                    .into_iter()
                    .map(DockerNetwork::into_resource)
                    .collect())
            }
            // `podman pod ps --format json`. **Keys not observed:** podman is
            // not installed on the machine this was written on, so this struct
            // follows podman's documented output. Anyone with podman should check
            // it -- Phase 08's success criteria record that as an open debt.
            ResourceKind::Pod if self.kind == BackendKind::Podman => {
                let stdout = self
                    .stdout(self.command(&["pod", "ps", "--format", "json"]))
                    .await?;
                // An array, like `docker compose ls`, not one object per line.
                let pods: Vec<PodmanPod> =
                    serde_json::from_str(stdout.trim()).map_err(|error| ContainerError::Parse {
                        detail: format!("could not read `podman pod ps` output: {error}"),
                    })?;
                Ok(pods.into_iter().map(PodmanPod::into_resource).collect())
            }
            ResourceKind::ComposeProject => {
                // The one command here that answers with a JSON *array* rather
                // than an object per line, so it needs `from_str` and not the
                // stream reader. Observed on Docker 29.4.3: `[]` with no
                // projects.
                let stdout = self
                    .stdout(self.command(&["compose", "ls", "--format", "json"]))
                    .await?;
                let projects: Vec<ComposeProject> =
                    serde_json::from_str(stdout.trim()).map_err(|error| ContainerError::Parse {
                        detail: format!("could not read `docker compose ls` output: {error}"),
                    })?;
                Ok(projects
                    .into_iter()
                    .map(ComposeProject::into_resource)
                    .collect())
            }
            // Not a kind this backend declares. Answering with an error rather
            // than an empty list keeps `supported_kinds` the single source of
            // truth -- an empty list would read as "none exist".
            other => Err(ContainerError::CommandFailed {
                program: self.program.clone(),
                stderr: format!("{other:?} is not a kind this backend lists"),
            }),
        }
    }

    async fn act(
        &self,
        kind: ResourceKind,
        action: ResourceAction,
        id: &str,
    ) -> Result<(), ContainerError> {
        if !self.supported_actions(kind).contains(&action) {
            return Err(ContainerError::CommandFailed {
                program: self.program.clone(),
                stderr: format!("{:?} cannot be {}ed", kind, action.label().to_lowercase()),
            });
        }
        let verb = match action {
            ResourceAction::Start => "start",
            ResourceAction::Stop => "stop",
            ResourceAction::Restart => "restart",
            ResourceAction::Pause => "pause",
            ResourceAction::Unpause => "unpause",
        };
        // `--` before the id: it comes from the engine's own listing, but a
        // container name is chosen by whoever ran it and a name starting with a
        // dash would otherwise be read as a flag.
        let args: Vec<&str> = if kind == ResourceKind::Pod {
            vec!["pod", verb, "--", id]
        } else {
            vec![verb, "--", id]
        };
        self.stdout(self.command(&args)).await?;
        Ok(())
    }

    /// Goes and finds what a prune would delete.
    ///
    /// Assembled from ordinary list commands with the engine's own filters,
    /// because `docker system prune` cannot be asked. Each filter mirrors what
    /// prune itself considers unused:
    /// - `images -f dangling=true` -- images nothing names
    /// - `ps -a -f status=exited -f status=created` -- containers not running
    /// - `volume ls -f dangling=true` -- volumes no container uses (volumes only)
    ///
    /// Networks are deliberately left out: `docker network prune` removes custom
    /// networks with nothing attached, and there is no list filter that
    /// distinguishes those from the three built-ins it never touches. Claiming a
    /// network would be deleted when it will not is as misleading as the reverse,
    /// so the list says nothing about them.
    async fn prune_targets(
        &self,
        scope: crate::PruneScope,
    ) -> Option<Result<Vec<Resource>, ContainerError>> {
        Some(self.gather_prune_targets(scope).await)
    }

    async fn destroy(&self, plan: &crate::DestructivePlan) -> Result<(), ContainerError> {
        match plan.intent() {
            crate::Intent::Remove { kind } => {
                for target in plan.targets() {
                    let args = remove_args(kind, &target.id)?;
                    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
                    self.stdout(self.command(&borrowed)).await?;
                }
                Ok(())
            }
            crate::Intent::Prune { scope } => {
                let mut args = vec!["system", "prune", "--force"];
                // Only when the scope says so. The default must never reach here
                // with volumes included -- see `PruneScope`.
                if scope.includes_volumes() {
                    args.push("--volumes");
                }
                self.stdout(self.command(&args)).await?;
                Ok(())
            }
        }
    }

    /// `docker logs -f`, or `podman pod logs -f` for a pod.
    ///
    /// Only for things that have a log. An image has never run, and a volume and
    /// a network never will.
    fn logs_command(&self, kind: ResourceKind, id: &str) -> Option<(String, Vec<String>)> {
        let args = match kind {
            ResourceKind::Container => {
                vec!["logs".into(), "-f".into(), "--".into(), id.to_string()]
            }
            ResourceKind::Pod if self.kind == BackendKind::Podman => vec![
                "pod".into(),
                "logs".into(),
                "-f".into(),
                "--".into(),
                id.to_string(),
            ],
            ResourceKind::ComposeProject => vec![
                "compose".into(),
                "-p".into(),
                id.to_string(),
                "logs".into(),
                "-f".into(),
            ],
            _ => return None,
        };
        Some((self.program.clone(), args))
    }

    /// `docker exec -it <id> <shell>`.
    ///
    /// `sh` and not `bash`: plenty of images have no bash at all, and a shell
    /// that fails to start is worse than a plainer one that works. Only a running
    /// container can be entered -- there is nothing to attach to in a stopped
    /// one, and a pod is not a process.
    fn exec_command(&self, kind: ResourceKind, id: &str) -> Option<(String, Vec<String>)> {
        if kind != ResourceKind::Container {
            return None;
        }
        Some((
            self.program.clone(),
            vec![
                "exec".into(),
                "-it".into(),
                "--".into(),
                id.to_string(),
                "sh".into(),
            ],
        ))
    }

    /// `docker events`, mapped to "this kind went stale".
    ///
    /// The event's own `Type` is the kind: observed on Docker 29.4.3, an event
    /// carries `Action, Actor, Type, scope, time, timeNano`, and `Type` is
    /// `container`, `image`, `volume`, `network`, ... -- the same vocabulary
    /// `ResourceKind` uses, so no mapping table is needed beyond naming them.
    ///
    /// Deliberately says only *which kind* changed, not what changed about it.
    /// A list is cheap to re-read and an incremental update that drifts out of
    /// step with the engine is worse than a refresh.
    fn watch(&self) -> Option<BoxStream<'static, BackendEvent>> {
        let command = self.command(&["events", "--format", "{{json .}}"]);
        let values = crate::event_stream::json_values::<DockerEvent>(command)?;
        Some(
            values
                .filter_map(|event| async move {
                    event.stale_kind().map(|kind| BackendEvent::Stale { kind })
                })
                .boxed(),
        )
    }
}

/// One line of `docker events --format '{{json .}}'`.
///
/// Only `Type` is read. `Action` and `Actor` are what an incremental update
/// would need, and this deliberately does not do one -- see `watch` above.
#[derive(Debug, Clone, serde::Deserialize)]
struct DockerEvent {
    #[serde(default, rename = "Type")]
    kind: String,
}

impl DockerEvent {
    /// The kind this event makes stale, if it is one the panel lists.
    ///
    /// `daemon`, `plugin`, `service` and the rest are dropped rather than
    /// mapped: nothing here shows them, so waking the panel for them would be a
    /// refresh nobody asked for.
    fn stale_kind(&self) -> Option<ResourceKind> {
        match self.kind.as_str() {
            "container" => Some(ResourceKind::Container),
            "image" => Some(ResourceKind::Image),
            "volume" => Some(ResourceKind::Volume),
            "network" => Some(ResourceKind::Network),
            _ => None,
        }
    }
}
