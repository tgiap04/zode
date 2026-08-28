//! A backend that never leaves the process.
//!
//! Exists so the trait, the suite and later the panel can be exercised without
//! an engine on the machine. It answers the same trait over the same types --
//! only the command line is gone.

use async_trait::async_trait;
use futures::stream::BoxStream;

use crate::backend::{BackendEvent, BackendKind, ContainerBackend, ContainerError};
use crate::resource::{Resource, ResourceAction, ResourceKind, RunState};

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
            }],
            misbehaviour: Misbehaviour::None,
            acted: parking_lot::Mutex::new(Vec::new()),
            destroyed: parking_lot::Mutex::new(Vec::new()),
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

    fn logs_command(&self, kind: ResourceKind, id: &str) -> Option<(String, Vec<String>)> {
        (kind == ResourceKind::Container)
            .then(|| ("fake".to_string(), vec!["logs".into(), id.to_string()]))
    }

    fn exec_command(&self, kind: ResourceKind, id: &str) -> Option<(String, Vec<String>)> {
        (kind == ResourceKind::Container)
            .then(|| ("fake".to_string(), vec!["exec".into(), id.to_string()]))
    }
}
