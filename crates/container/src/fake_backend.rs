//! A backend that never leaves the process.
//!
//! Exists so the trait, the suite and later the panel can be exercised without
//! an engine on the machine. It answers the same trait over the same types --
//! only the command line is gone.

use async_trait::async_trait;
use futures::stream::BoxStream;

use crate::backend::{BackendEvent, BackendKind, ContainerBackend, ContainerError};
use crate::inspect::{InspectRow, InspectSection, Inspection};
use crate::operations::{
    ComposeOp, ComposeTarget, ImageLayer, Op, RunSpec, check_label_key, check_name,
    check_reference, check_subnet,
};
use crate::resource::{Resource, ResourceAction, ResourceKind, RunState};
use crate::stats::Stats;

/// How the fake should misbehave, so the unhappy paths get tested too.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Misbehaviour {
    #[default]
    None,
    EngineMissing,
    EngineNotRunning,
    NotAuthorized,
    NotConfigured,
    FailEveryList,
}

/// A write-side request that reached the fake, with its values as given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Pull(String),
    PullCommand(String),
    Tag {
        source: String,
        target: String,
    },
    History(String),
    CreateVolume {
        name: String,
        driver: Option<String>,
        labels: Vec<(String, String)>,
    },
    CreateNetwork {
        name: String,
        driver: Option<String>,
        subnet: Option<String>,
    },
    Connect {
        network: String,
        container: String,
        alias: Option<String>,
    },
    Disconnect {
        network: String,
        container: String,
    },
    ComposeCommand {
        project: ComposeTarget,
        op: ComposeOp,
        services: Vec<String>,
    },
    ComposeDownPlan {
        project: ComposeTarget,
        with_volumes: bool,
    },
    Run(RunSpec),
}

pub struct FakeBackend {
    kind: BackendKind,
    kinds: &'static [ResourceKind],
    resources: Vec<Resource>,
    misbehaviour: Misbehaviour,
    /// Every `act` that reached this backend, in order. Read by tests that need
    /// to prove a command was -- or was not -- issued.
    acted: parking_lot::Mutex<Vec<(ResourceAction, String)>>,
    /// Every plan that reached `destroy`, so a test can prove one did *not*.
    destroyed: parking_lot::Mutex<Vec<crate::DestructivePlan>>,
    /// Ids an `act` is refused for, to prove one bad container does not hide
    /// what happened to the rest.
    refused_ids: Vec<String>,
    inspections: Vec<(String, Inspection)>,
    stats: Vec<Stats>,
    usages: Vec<(String, Vec<Resource>)>,
    layers: Vec<(String, Vec<ImageLayer>)>,
    /// Write-side operations this fake claims not to have, so a view test can
    /// see an entry point stay undrawn.
    withheld: Vec<Op>,
    /// Whether a forced removal does nothing here, the way it does on Docker
    /// for a volume or a network.
    force_withheld: bool,
    /// Every write-side request, in order.
    requests: parking_lot::Mutex<Vec<Request>>,
}

const DOCKER_KINDS: &[ResourceKind] = &[
    ResourceKind::Container,
    ResourceKind::Image,
    ResourceKind::Volume,
    ResourceKind::Network,
    ResourceKind::ComposeProject,
];

impl FakeBackend {
    /// A fake standing in for Docker, holding one running container.
    pub fn docker() -> Self {
        Self {
            kind: BackendKind::Docker,
            kinds: DOCKER_KINDS,
            resources: vec![Resource {
                kind: ResourceKind::Container,
                id: "c0ffee".into(),
                name: "fake-postgres".into(),
                state: RunState::Running,
                detail: vec![("Image", "postgres:16".into())],
                parent: None,
                compose: None,
            }],
            misbehaviour: Misbehaviour::None,
            acted: parking_lot::Mutex::new(Vec::new()),
            destroyed: parking_lot::Mutex::new(Vec::new()),
            refused_ids: Vec::new(),
            inspections: vec![("c0ffee".into(), canned_inspection("fake-postgres"))],
            stats: vec![Stats {
                id: "c0ffee".into(),
                name: "fake-postgres".into(),
                cpu_percent: Some(1.5),
                memory_used: Some(64 * 1024 * 1024),
                memory_limit: Some(512 * 1024 * 1024),
                pids: Some(7),
                ..Stats::default()
            }],
            usages: Vec::new(),
            layers: Vec::new(),
            withheld: Vec::new(),
            force_withheld: false,
            requests: parking_lot::Mutex::new(Vec::new()),
        }
    }

    pub fn empty(kind: BackendKind, kinds: &'static [ResourceKind]) -> Self {
        Self {
            kind,
            kinds,
            resources: Vec::new(),
            misbehaviour: Misbehaviour::None,
            acted: parking_lot::Mutex::new(Vec::new()),
            destroyed: parking_lot::Mutex::new(Vec::new()),
            refused_ids: Vec::new(),
            inspections: Vec::new(),
            stats: Vec::new(),
            usages: Vec::new(),
            layers: Vec::new(),
            withheld: Vec::new(),
            force_withheld: false,
            requests: parking_lot::Mutex::new(Vec::new()),
        }
    }

    /// The same fake, answering with `resources` instead of what it held.
    pub fn holding(mut self, resources: Vec<Resource>) -> Self {
        self.resources = resources;
        self
    }

    /// The same fake, refusing every action on `id`.
    pub fn refusing_actions_on(mut self, id: &str) -> Self {
        self.refused_ids.push(id.to_string());
        self
    }

    /// The same fake, describing `id` with `inspection`.
    pub fn inspecting(mut self, id: &str, inspection: Inspection) -> Self {
        self.inspections.push((id.to_string(), inspection));
        self
    }

    /// The same fake, reporting `stats` for whichever of its ids are asked
    /// about.
    pub fn with_stats(mut self, stats: Vec<Stats>) -> Self {
        self.stats = stats;
        self
    }

    /// The same fake, saying `users` are the containers using `id`.
    pub fn used_by(mut self, id: &str, users: Vec<Resource>) -> Self {
        self.usages.push((id.to_string(), users));
        self
    }

    /// The same fake, describing the history of image `id`.
    pub fn with_history(mut self, id: &str, layers: Vec<ImageLayer>) -> Self {
        self.layers.push((id.to_string(), layers));
        self
    }

    /// The same fake, without `op`.
    pub fn not_offering(mut self, op: Op) -> Self {
        self.withheld.push(op);
        self
    }

    /// The same fake, on which forcing a removal does nothing, so a view test
    /// can see the force option stay undrawn.
    pub fn without_force(mut self) -> Self {
        self.force_withheld = true;
        self
    }

    /// Every write-side request that reached this fake.
    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().clone()
    }

    /// The common start of a write-side call: refuse what is not offered,
    /// write the request down, then fail the way the fake was told to.
    fn admit(&self, op: Op, request: Request) -> Result<(), ContainerError> {
        if !self.offers(op) {
            return Err(crate::backend::not_offered(&format!("{op:?}")));
        }
        self.requests.lock().push(request);
        match self.misbehave() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub fn misbehaving(mut self, misbehaviour: Misbehaviour) -> Self {
        self.misbehaviour = misbehaviour;
        self
    }

    pub fn acted(&self) -> Vec<(ResourceAction, String)> {
        self.acted.lock().clone()
    }

    pub fn destroyed(&self) -> Vec<crate::DestructivePlan> {
        self.destroyed.lock().clone()
    }

    fn misbehave(&self) -> Option<ContainerError> {
        match self.misbehaviour {
            Misbehaviour::None => None,
            Misbehaviour::EngineMissing => Some(ContainerError::EngineMissing {
                program: "fake".into(),
            }),
            Misbehaviour::EngineNotRunning => Some(ContainerError::EngineNotRunning {
                program: "fake".into(),
            }),
            Misbehaviour::NotAuthorized => Some(ContainerError::NotAuthorized {
                detail: "forbidden".into(),
            }),
            Misbehaviour::NotConfigured => Some(ContainerError::NotConfigured {
                detail: "no context chosen".into(),
            }),
            Misbehaviour::FailEveryList => Some(ContainerError::CommandFailed {
                program: "fake".into(),
                stderr: "as asked".into(),
            }),
        }
    }
}

#[async_trait]
impl ContainerBackend for FakeBackend {
    fn kind(&self) -> BackendKind {
        self.kind
    }

    fn supported_kinds(&self) -> &'static [ResourceKind] {
        self.kinds
    }

    fn supported_actions(&self, kind: ResourceKind) -> &'static [ResourceAction] {
        match kind {
            ResourceKind::Container | ResourceKind::Pod => &[
                ResourceAction::Start,
                ResourceAction::Stop,
                ResourceAction::Restart,
                ResourceAction::Pause,
                ResourceAction::Unpause,
            ],
            _ => &[],
        }
    }

    async fn list(&self, kind: ResourceKind) -> Result<Vec<Resource>, ContainerError> {
        if let Some(error) = self.misbehave() {
            return Err(error);
        }
        if !self.kinds.contains(&kind) {
            return Err(ContainerError::CommandFailed {
                program: "fake".into(),
                stderr: format!("{kind:?} is not a kind this engine has a word for"),
            });
        }
        Ok(self
            .resources
            .iter()
            .filter(|resource| resource.kind == kind)
            .cloned()
            .collect())
    }

    async fn act(
        &self,
        _kind: ResourceKind,
        action: ResourceAction,
        id: &str,
    ) -> Result<(), ContainerError> {
        if let Some(error) = self.misbehave() {
            return Err(error);
        }
        if !self.resources.iter().any(|resource| resource.id == id) {
            return Err(ContainerError::NotFound { id: id.into() });
        }
        if self.refused_ids.iter().any(|refused| refused == id) {
            return Err(ContainerError::CommandFailed {
                program: "fake".into(),
                stderr: format!("{id} refused {action:?}"),
            });
        }
        self.acted.lock().push((action, id.to_string()));
        Ok(())
    }

    fn watch(&self) -> Option<BoxStream<'static, BackendEvent>> {
        None
    }

    async fn prune_targets(
        &self,
        _scope: crate::PruneScope,
    ) -> Option<Result<Vec<Resource>, ContainerError>> {
        if let Some(error) = self.misbehave() {
            return Some(Err(error));
        }
        Some(Ok(self.resources.clone()))
    }

    async fn destroy(&self, plan: &crate::DestructivePlan) -> Result<(), ContainerError> {
        if let Some(error) = self.misbehave() {
            return Err(error);
        }
        self.destroyed.lock().push(plan.clone());
        Ok(())
    }

    fn force_effect(&self, kind: ResourceKind) -> Option<String> {
        let removable = matches!(
            kind,
            ResourceKind::Container
                | ResourceKind::Image
                | ResourceKind::Volume
                | ResourceKind::Network
                | ResourceKind::Pod
        );
        (removable && !self.force_withheld)
            .then(|| format!("Forced: the fake removes this {kind:?} and what uses it."))
    }

    fn supports_inspect(&self, kind: ResourceKind) -> bool {
        self.kinds.contains(&kind) && kind != ResourceKind::ComposeProject
    }

    async fn inspect(
        &self,
        kind: ResourceKind,
        id: &str,
    ) -> Result<Option<Inspection>, ContainerError> {
        if !self.supports_inspect(kind) {
            return Ok(None);
        }
        if let Some(error) = self.misbehave() {
            return Err(error);
        }
        self.inspections
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, inspection)| Some(inspection.clone()))
            .ok_or_else(|| ContainerError::NotFound { id: id.into() })
    }

    fn supports_stats(&self) -> bool {
        true
    }

    async fn stats(&self, ids: &[String]) -> Result<Option<Vec<Stats>>, ContainerError> {
        // Like the real backends, which never start a command for nobody.
        if ids.is_empty() {
            return Ok(Some(Vec::new()));
        }
        if let Some(error) = self.misbehave() {
            return Err(error);
        }
        Ok(Some(
            self.stats
                .iter()
                .filter(|stats| ids.contains(&stats.id))
                .cloned()
                .collect(),
        ))
    }

    fn supports_usages(&self, kind: ResourceKind) -> bool {
        self.kinds.contains(&kind)
            && matches!(
                kind,
                ResourceKind::Volume | ResourceKind::Network | ResourceKind::Image
            )
    }

    async fn usages(
        &self,
        kind: ResourceKind,
        id: &str,
    ) -> Result<Option<Vec<Resource>>, ContainerError> {
        if !self.supports_usages(kind) {
            return Ok(None);
        }
        if let Some(error) = self.misbehave() {
            return Err(error);
        }
        Ok(Some(
            self.usages
                .iter()
                .find(|(known, _)| known == id)
                .map(|(_, users)| users.clone())
                .unwrap_or_default(),
        ))
    }

    fn offers(&self, op: Op) -> bool {
        !self.withheld.contains(&op)
    }

    async fn compose_available(&self) -> bool {
        self.offers(Op::Compose)
    }

    fn compose_command(
        &self,
        project: &ComposeTarget,
        op: ComposeOp,
        services: &[String],
    ) -> Option<(String, Vec<String>)> {
        if !self.offers(Op::Compose) {
            return None;
        }
        let args = crate::docker_write::compose_argv(
            project,
            crate::docker_write::compose_verb(op),
            services,
        )
        .ok()?;
        self.requests.lock().push(Request::ComposeCommand {
            project: project.clone(),
            op,
            services: services.to_vec(),
        });
        Some(("fake".to_string(), args))
    }

    /// Containers whose compose label names the project; networks and volumes
    /// whose `parent` does, since only containers carry labels here.
    async fn compose_down_plan(
        &self,
        project: &ComposeTarget,
        with_volumes: bool,
    ) -> Result<Option<crate::DestructivePlan>, ContainerError> {
        if !self.offers(Op::Compose) {
            return Ok(None);
        }
        check_name("compose project", &project.project)?;
        self.admit(
            Op::Compose,
            Request::ComposeDownPlan {
                project: project.clone(),
                with_volumes,
            },
        )?;
        let of_kind = |kind: ResourceKind| -> Vec<Resource> {
            self.resources
                .iter()
                .filter(|resource| {
                    resource.kind == kind
                        && match kind {
                            ResourceKind::Container => resource
                                .compose
                                .as_ref()
                                .is_some_and(|labels| labels.project == project.project),
                            _ => resource.parent.as_deref() == Some(project.project.as_str()),
                        }
                })
                .cloned()
                .collect()
        };
        Ok(crate::DestructivePlan::compose_down(
            project.clone(),
            of_kind(ResourceKind::Container),
            of_kind(ResourceKind::Network),
            with_volumes.then(|| of_kind(ResourceKind::Volume)),
        ))
    }

    fn pull_command(&self, reference: &str) -> Option<(String, Vec<String>)> {
        if !self.offers(Op::ImagePull) || check_reference("image reference", reference).is_err() {
            return None;
        }
        self.requests
            .lock()
            .push(Request::PullCommand(reference.to_string()));
        Some((
            "fake".to_string(),
            vec!["pull".into(), "--".into(), reference.to_string()],
        ))
    }

    async fn pull(&self, reference: &str) -> Result<(), ContainerError> {
        check_reference("image reference", reference)?;
        self.admit(Op::ImagePull, Request::Pull(reference.to_string()))
    }

    async fn tag(&self, source: &str, target: &str) -> Result<(), ContainerError> {
        check_reference("source image", source)?;
        check_reference("target image", target)?;
        self.admit(
            Op::ImageTag,
            Request::Tag {
                source: source.to_string(),
                target: target.to_string(),
            },
        )
    }

    async fn history(&self, id: &str) -> Result<Vec<ImageLayer>, ContainerError> {
        check_reference("image", id)?;
        self.admit(Op::ImageHistory, Request::History(id.to_string()))?;
        self.layers
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, layers)| layers.clone())
            .ok_or_else(|| ContainerError::NotFound { id: id.into() })
    }

    fn run_command(&self, spec: &RunSpec) -> Result<(String, Vec<String>), ContainerError> {
        if !self.offers(Op::ImageRun) {
            return Err(crate::backend::not_offered("ImageRun"));
        }
        let args = crate::docker_write::run_argv(spec)?;
        self.requests.lock().push(Request::Run(spec.clone()));
        Ok(("fake".to_string(), args))
    }

    async fn create_volume(
        &self,
        name: &str,
        driver: Option<&str>,
        labels: &[(String, String)],
    ) -> Result<(), ContainerError> {
        check_name("volume name", name)?;
        if let Some(driver) = driver {
            check_name("volume driver", driver)?;
        }
        for (key, _) in labels {
            check_label_key(key)?;
        }
        self.admit(
            Op::VolumeCreate,
            Request::CreateVolume {
                name: name.to_string(),
                driver: driver.map(String::from),
                labels: labels.to_vec(),
            },
        )
    }

    async fn create_network(
        &self,
        name: &str,
        driver: Option<&str>,
        subnet: Option<&str>,
    ) -> Result<(), ContainerError> {
        check_name("network name", name)?;
        if let Some(driver) = driver {
            check_name("network driver", driver)?;
        }
        if let Some(subnet) = subnet {
            check_subnet(subnet)?;
        }
        self.admit(
            Op::NetworkCreate,
            Request::CreateNetwork {
                name: name.to_string(),
                driver: driver.map(String::from),
                subnet: subnet.map(String::from),
            },
        )
    }

    async fn connect(
        &self,
        network: &str,
        container: &str,
        alias: Option<&str>,
    ) -> Result<(), ContainerError> {
        check_name("network", network)?;
        check_name("container", container)?;
        if let Some(alias) = alias {
            check_name("network alias", alias)?;
        }
        self.admit(
            Op::NetworkConnect,
            Request::Connect {
                network: network.to_string(),
                container: container.to_string(),
                alias: alias.map(String::from),
            },
        )
    }

    async fn disconnect(&self, network: &str, container: &str) -> Result<(), ContainerError> {
        check_name("network", network)?;
        check_name("container", container)?;
        self.admit(
            Op::NetworkConnect,
            Request::Disconnect {
                network: network.to_string(),
                container: container.to_string(),
            },
        )
    }

    fn logs_command(&self, kind: ResourceKind, id: &str) -> Option<(String, Vec<String>)> {
        (kind == ResourceKind::Container)
            .then(|| ("fake".to_string(), vec!["logs".into(), id.to_string()]))
    }

    fn exec_command(&self, kind: ResourceKind, id: &str) -> Option<(String, Vec<String>)> {
        (kind == ResourceKind::Container)
            .then(|| ("fake".to_string(), vec!["exec".into(), id.to_string()]))
    }
}

/// An inspection with one secret row, so a view test can prove masking and
/// reveal without an engine.
fn canned_inspection(name: &str) -> Inspection {
    Inspection {
        sections: vec![
            InspectSection {
                title: "State".into(),
                rows: vec![InspectRow::new("Status", "running")],
            },
            InspectSection {
                title: "Environment".into(),
                rows: vec![
                    InspectRow::new("POSTGRES_DB", name),
                    InspectRow::secret("POSTGRES_PASSWORD", "fake-not-real"),
                ],
            },
        ],
        raw_json: format!("{{\"Name\":\"{name}\",\"Env\":[\"POSTGRES_PASSWORD=********\"]}}"),
        raw_json_revealed: format!(
            "{{\"Name\":\"{name}\",\"Env\":[\"POSTGRES_PASSWORD=fake-not-real\"]}}"
        ),
    }
}
