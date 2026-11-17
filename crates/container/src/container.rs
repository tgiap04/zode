//! Container and orchestration engines, behind one trait.
//!
//! Two engines answer here and they are not the same shape: Docker and Podman
//! are a command line over a local daemon, Kubernetes is a command line over a
//! cluster. The trait exists so neither has to wear the other's vocabulary --
//! see [`backend::ContainerBackend`].

pub mod backend;
pub mod destructive;
pub mod docker_backend;
mod docker_inspect;
mod docker_read;
mod docker_resources;
mod docker_types;
mod docker_write;
mod event_stream;
pub mod inspect;
pub mod kubeconfig;
pub mod kubernetes_backend;
mod kubernetes_inspect;
mod kubernetes_read;
mod kubernetes_types;
pub mod operations;
mod podman;
pub mod resource;
mod secret_masking;
pub mod stats;

#[cfg(any(test, feature = "test-support"))]
pub mod backend_test_suite;
#[cfg(any(test, feature = "test-support"))]
pub mod fake_backend;

#[cfg(test)]
mod inspect_tests;
#[cfg(test)]
mod read_side_tests;
#[cfg(test)]
mod scripted_engine_tests;
#[cfg(test)]
mod secret_masking_tests;
#[cfg(test)]
mod stats_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod write_side_tests;

pub use backend::{BackendEvent, BackendKind, ContainerBackend, ContainerError};
pub use destructive::{Dependents, DestructivePlan, FORCE_CONFIRMATION, Intent, PruneScope};
pub use docker_backend::DockerBackend;
pub use inspect::{InspectRow, InspectSection, Inspection, looks_secret, mask_secrets_in_text};
pub use kubeconfig::{KubeContext, Kubeconfig};
pub use kubernetes_backend::{KubernetesBackend, Scope};
pub use operations::{
    ComposeOp, ComposeTarget, ImageLayer, Op, PortMapping, PortProtocol, RunSpec, VolumeMount,
};
pub use resource::{ComposeLabels, Resource, ResourceAction, ResourceKind, RunState, row_key};
pub use stats::Stats;
