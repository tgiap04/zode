//! Docker and Podman, through their command line.
//!
//! One backend for both: they share these commands, and Podman is told apart
//! only where it genuinely differs. The command line rather than the socket
//! because the CLI already knows where its daemon is -- rootless Podman moves
//! that socket, and reimplementing the search here would be a second place to
//! get it wrong.

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use util::command::{Command, CommandHost};

use crate::backend::{BackendEvent, BackendKind, ContainerBackend, ContainerError};
use crate::docker_resources::{ComposeProject, DockerImage, DockerNetwork, DockerVolume};
use crate::docker_types::DockerContainer;
use crate::inspect::Inspection;
use crate::operations::{ComposeOp, ComposeTarget, ImageLayer, Op, RunSpec};
use crate::podman::PodmanPod;
use crate::resource::{Resource, ResourceAction, ResourceKind};
use crate::stats::Stats;

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
    /// The machine the engine is asked on: this one, or a remote project's
    /// host. Every command below is built through it.
    host: Arc<dyn CommandHost>,
    /// Set once `compose version` has answered, so the panel does not spawn a
    /// process every time a menu opens.
    compose_confirmed: AtomicBool,
}

impl DockerBackend {
    pub fn docker(host: Arc<dyn CommandHost>) -> Self {
        Self {
            program: "docker".into(),
            kind: BackendKind::Docker,
            host,
            compose_confirmed: AtomicBool::new(false),
        }
    }

    pub fn podman(host: Arc<dyn CommandHost>) -> Self {
        Self {
            program: "podman".into(),
            kind: BackendKind::Podman,
            host,
            compose_confirmed: AtomicBool::new(false),
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
            host: Arc::new(util::command::LocalCommandHost),
            compose_confirmed: AtomicBool::new(false),
        }
    }

    pub(crate) fn program(&self) -> &str {
        &self.program
    }

    pub(crate) fn compose_confirmed(&self) -> bool {
        self.compose_confirmed.load(Ordering::Relaxed)
    }

    pub(crate) fn confirm_compose(&self) {
        self.compose_confirmed.store(true, Ordering::Relaxed);
    }

    fn command(&self, args: &[&str]) -> Result<Command, ContainerError> {
        crate::backend::host_command(self.host.as_ref(), &self.program, args)
    }

    /// Runs a command and hands back stdout, classifying the ways it can fail.
    ///
    /// The two "engine is not usable" cases are told apart structurally rather
    /// than by reading stderr: a missing binary surfaces as `NotFound` from the
    /// spawn itself, and a live binary that cannot reach its daemon is confirmed
    /// by asking `info` -- which only happens on the error path, so the common
    /// case pays nothing for it.
    pub(crate) async fn stdout(&self, args: &[&str]) -> Result<String, ContainerError> {
        let mut command = self.command(args)?;
        let output = command.output().await.map_err(|error| {
            crate::backend::spawn_failure(self.host.as_ref(), &self.program, error)
        })?;

        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }

        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if let Some(error) = crate::backend::exit_failure(
            self.host.as_ref(),
            &self.program,
            output.status.code(),
            &stderr,
        ) {
            return Err(error);
        }
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
        let mut command = match self.command(&["info", "--format", "{{.ServerVersion}}"]) {
            Ok(command) => command,
            Err(error) => {
                log::warn!("could not ask whether {} answers: {error}", self.program);
                return false;
            }
        };
        match command.output().await {
            Ok(output) => output.status.success(),
            Err(error) => {
                log::warn!("could not ask whether {} answers: {error}", self.program);
                false
            }
        }
    }
}

/// The command that removes one thing of a given kind.
///
/// `None` for a kind with no removal, which is the only reason this is fallible.
/// `--` before the id every time: the id came from the engine, but the name
/// inside it was chosen by whoever made the resource.
///
/// `--force` only when the plan was forced (see
/// [`crate::DestructivePlan::forced`]): without it a running container or an
/// image in use refuses, which is the engine telling the truth rather than this
/// crate deciding for somebody.
fn remove_args(kind: ResourceKind, id: &str, force: bool) -> Result<Vec<String>, ContainerError> {
    let args = match kind {
        ResourceKind::Container => vec!["rm"],
        ResourceKind::Image => vec!["rmi"],
        ResourceKind::Volume => vec!["volume", "rm"],
        ResourceKind::Network => vec!["network", "rm"],
        ResourceKind::Pod => vec!["pod", "rm"],
        other => {
            return Err(ContainerError::CommandFailed {
                program: "docker".into(),
                stderr: format!("{other:?} cannot be removed"),
            });
        }
    };
    let mut args: Vec<String> = args.into_iter().map(String::from).collect();
    if force {
        args.push("--force".into());
    }
    args.push("--".into());
    args.push(id.to_string());
    Ok(args)
}

/// What `rm`, `rmi` and the rest are given for a target.
///
/// An image row from `docker images` is one *tag* of an image, and the id is
/// shared by every tag of it: `rmi <id>` of an image with two tags is refused
/// ("referenced in multiple repositories"), and with `--force` removes both.
/// A row that names a `repo:tag` is removed by that name, which untags just it.
fn removal_handle(kind: ResourceKind, target: &Resource) -> &str {
    if kind == ResourceKind::Image {
        return crate::docker_resources::tagged_reference(target).unwrap_or(&target.id);
    }
    &target.id
}

impl DockerBackend {
    async fn gather_prune_targets(
        &self,
        scope: crate::PruneScope,
    ) -> Result<Vec<Resource>, ContainerError> {
        let mut targets = Vec::new();

        let images = self
            .stdout(&["images", "-f", "dangling=true", "--format", "{{json .}}"])
            .await?;
        targets.extend(
            parse_json_lines_strict::<DockerImage>(&images)?
                .into_iter()
                .map(DockerImage::into_resource),
        );

        let stopped = self
            .stdout(&[
                "ps",
                "-a",
                "-f",
                "status=exited",
                "-f",
                "status=created",
                "--format",
                "{{json .}}",
            ])
            .await?;
        targets.extend(
            parse_json_lines_strict::<DockerContainer>(&stopped)?
                .into_iter()
                .map(DockerContainer::into_resource),
        );

        if scope.includes_volumes() {
            let volumes = self
                .stdout(&[
                    "volume",
                    "ls",
                    "-f",
                    "dangling=true",
                    "--format",
                    "{{json .}}",
                ])
                .await?;
            targets.extend(
                parse_json_lines_strict::<DockerVolume>(&volumes)?
                    .into_iter()
                    .map(DockerVolume::into_resource),
            );
        }

        Ok(targets)
    }
}

/// The rows of `--format '{{json .}}'` output, each read on its own: one JSON
/// value after another, whether they sit one to a line or are pretty-printed
/// over several (the shape `kubectl --watch` emits, which this reader carries
/// over to). A single `from_str` would fail the moment there were two rows.
///
/// A row that will not parse is its own `Err` and reading resumes at the next
/// line, so one bad row is only itself.
fn json_rows<T: serde::de::DeserializeOwned>(stdout: &str) -> Vec<Result<T, serde_json::Error>> {
    let mut rows = Vec::new();
    let mut rest = stdout;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return rows;
        }
        let mut stream = serde_json::Deserializer::from_str(rest).into_iter::<T>();
        match stream.next() {
            Some(Ok(row)) => {
                rows.push(Ok(row));
                rest = &rest[stream.byte_offset()..];
            }
            Some(Err(error)) => {
                rows.push(Err(error));
                rest = rest.split_once('\n').map_or("", |(_, tail)| tail);
            }
            None => return rows,
        }
    }
}

/// Reads a listing for display: a row that will not parse is dropped with a
/// log, because one unreadable container must not hide the other nine.
///
/// Never for something a destructive plan or a "who depends on this" answer is
/// built from -- there a missing row is a thing silently left out of what is
/// about to be lost. Those use [`parse_json_lines_strict`].
pub(crate) fn parse_json_lines<T: serde::de::DeserializeOwned>(stdout: &str) -> Vec<T> {
    json_rows(stdout)
        .into_iter()
        .enumerate()
        .filter_map(|(index, row)| {
            row.inspect_err(|error| {
                log::warn!(
                    "skipping row {} of container output, which could not be read: {error}",
                    index + 1
                );
            })
            .ok()
        })
        .collect()
}

/// Reads a listing that must be whole: any row that will not parse fails the
/// read, naming which one.
pub(crate) fn parse_json_lines_strict<T: serde::de::DeserializeOwned>(
    stdout: &str,
) -> Result<Vec<T>, ContainerError> {
    json_rows(stdout)
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            row.map_err(|error| ContainerError::Parse {
                detail: format!(
                    "row {} of the engine's list could not be read: {error}",
                    index + 1
                ),
            })
        })
        .collect()
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
                let stdout = self.stdout(&["ps", "-a", "--format", "{{json .}}"]).await?;
                Ok(parse_json_lines::<DockerContainer>(&stdout)
                    .into_iter()
                    .map(DockerContainer::into_resource)
                    .collect())
            }
            ResourceKind::Image => {
                let stdout = self.stdout(&["images", "--format", "{{json .}}"]).await?;
                Ok(parse_json_lines::<DockerImage>(&stdout)
                    .into_iter()
                    .map(DockerImage::into_resource)
                    .collect())
            }
            ResourceKind::Volume => {
                let stdout = self
                    .stdout(&["volume", "ls", "--format", "{{json .}}"])
                    .await?;
                Ok(parse_json_lines::<DockerVolume>(&stdout)
                    .into_iter()
                    .map(DockerVolume::into_resource)
                    .collect())
            }
            ResourceKind::Network => {
                let stdout = self
                    .stdout(&["network", "ls", "--format", "{{json .}}"])
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
                let stdout = self.stdout(&["pod", "ps", "--format", "json"]).await?;
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
                let stdout = self.stdout(&["compose", "ls", "--format", "json"]).await?;
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
        self.stdout(&args).await?;
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
                // A force that only suppresses "no such volume" is not what
                // the person agreed to; refuse before running anything.
                if plan.is_forced() && self.force_effect(kind).is_none() {
                    return Err(crate::backend::not_offered(&format!(
                        "forcing the removal of a {kind:?}"
                    )));
                }
                for target in plan.targets() {
                    let args = remove_args(kind, removal_handle(kind, target), plan.is_forced())?;
                    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
                    self.stdout(&borrowed).await?;
                }
                Ok(())
            }
            crate::Intent::ComposeDown { with_volumes } => {
                let Some(target) = plan.compose_target() else {
                    return Err(ContainerError::Invalid {
                        detail: "a compose down plan without a project".into(),
                    });
                };
                self.compose_down(target, with_volumes).await
            }
            crate::Intent::Prune { scope } => {
                let mut args = vec!["system", "prune", "--force"];
                // Only when the scope says so. The default must never reach here
                // with volumes included -- see `PruneScope`.
                if scope.includes_volumes() {
                    args.push("--volumes");
                }
                self.stdout(&args).await?;
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

    /// What `--force` does, which differs by engine and is what the dialog
    /// has to say. Docker's `volume rm -f` and `network rm -f` only ignore a
    /// missing resource and still refuse one in use, so there is nothing to
    /// offer; Podman's remove the containers that use them.
    fn force_effect(&self, kind: ResourceKind) -> Option<String> {
        let podman = self.kind == BackendKind::Podman;
        let effect = match kind {
            ResourceKind::Container => {
                "Forced: a running container is killed outright, not stopped gracefully, and then removed."
            }
            ResourceKind::Image if podman => {
                "Forced: all containers using this image, running or not, are stopped and removed with it."
            }
            ResourceKind::Image => {
                "Forced: the image is removed even where stopped containers were created from it; \
                 they stay, without their image. An image a running container uses is still refused."
            }
            ResourceKind::Volume if podman => {
                "Forced: every container using this volume is stopped and removed with it, \
                 and the volume's data is deleted."
            }
            ResourceKind::Network if podman => {
                "Forced: every container attached to this network is stopped and removed with it."
            }
            ResourceKind::Pod if podman => {
                "Forced: the pod's running containers are stopped and all of its containers are \
                 removed with it."
            }
            _ => return None,
        };
        Some(effect.to_string())
    }

    fn supports_inspect(&self, kind: ResourceKind) -> bool {
        matches!(
            kind,
            ResourceKind::Container
                | ResourceKind::Image
                | ResourceKind::Volume
                | ResourceKind::Network
        )
    }

    async fn inspect(
        &self,
        kind: ResourceKind,
        id: &str,
    ) -> Result<Option<Inspection>, ContainerError> {
        self.inspect_resource(kind, id).await
    }

    fn supports_stats(&self) -> bool {
        true
    }

    async fn stats(&self, ids: &[String]) -> Result<Option<Vec<Stats>>, ContainerError> {
        self.engine_stats(ids).await.map(Some)
    }

    fn supports_usages(&self, kind: ResourceKind) -> bool {
        matches!(
            kind,
            ResourceKind::Volume | ResourceKind::Network | ResourceKind::Image
        )
    }

    async fn usages(
        &self,
        kind: ResourceKind,
        id: &str,
    ) -> Result<Option<Vec<Resource>>, ContainerError> {
        self.containers_using(kind, id).await
    }

    fn offers(&self, _op: Op) -> bool {
        true
    }

    async fn compose_available(&self) -> bool {
        self.probe_compose().await
    }

    fn compose_command(
        &self,
        project: &ComposeTarget,
        op: ComposeOp,
        services: &[String],
    ) -> Option<(String, Vec<String>)> {
        self.compose_terminal_command(project, op, services)
    }

    async fn compose_down_plan(
        &self,
        project: &ComposeTarget,
        with_volumes: bool,
    ) -> Result<Option<crate::DestructivePlan>, ContainerError> {
        self.gather_compose_down(project, with_volumes).await
    }

    fn pull_command(&self, reference: &str) -> Option<(String, Vec<String>)> {
        self.pull_terminal_command(reference)
    }

    async fn pull(&self, reference: &str) -> Result<(), ContainerError> {
        self.pull_image(reference).await
    }

    async fn tag(&self, source: &str, target: &str) -> Result<(), ContainerError> {
        self.tag_image(source, target).await
    }

    async fn history(&self, id: &str) -> Result<Vec<ImageLayer>, ContainerError> {
        self.image_history(id).await
    }

    fn run_command(&self, spec: &RunSpec) -> Result<(String, Vec<String>), ContainerError> {
        self.run_terminal_command(spec)
    }

    async fn create_volume(
        &self,
        name: &str,
        driver: Option<&str>,
        labels: &[(String, String)],
    ) -> Result<(), ContainerError> {
        self.create_volume_named(name, driver, labels).await
    }

    async fn create_network(
        &self,
        name: &str,
        driver: Option<&str>,
        subnet: Option<&str>,
    ) -> Result<(), ContainerError> {
        self.create_network_named(name, driver, subnet).await
    }

    async fn connect(
        &self,
        network: &str,
        container: &str,
        alias: Option<&str>,
    ) -> Result<(), ContainerError> {
        self.connect_to_network(network, container, alias).await
    }

    async fn disconnect(&self, network: &str, container: &str) -> Result<(), ContainerError> {
        self.disconnect_from_network(network, container).await
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
        let command = match self.command(&["events", "--format", "{{json .}}"]) {
            Ok(command) => command,
            Err(error) => {
                log::warn!("could not start an event stream: {error}");
                return None;
            }
        };
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
