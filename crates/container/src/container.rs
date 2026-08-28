//! Container and orchestration engines, behind one trait.
//!
//! Two engines answer here and they are not the same shape: Docker and Podman
//! are a command line over a local daemon, Kubernetes is a command line over a
//! cluster. The trait exists so neither has to wear the other's vocabulary --
//! see [`backend::ContainerBackend`].

pub mod backend;
pub mod destructive;
pub mod docker_backend;
mod docker_resources;
mod docker_types;
mod event_stream;
pub mod kubeconfig;
pub mod kubernetes_backend;
mod kubernetes_types;
mod podman;
pub mod resource;

#[cfg(any(test, feature = "test-support"))]
pub mod backend_test_suite;
#[cfg(any(test, feature = "test-support"))]
pub mod fake_backend;

#[cfg(test)]
mod tests;

pub use backend::{BackendEvent, BackendKind, ContainerBackend, ContainerError};
pub use destructive::{DestructivePlan, Intent, PruneScope};
pub use docker_backend::DockerBackend;
pub use kubeconfig::{KubeContext, Kubeconfig};
pub use kubernetes_backend::{KubernetesBackend, Scope};
pub use resource::{Resource, ResourceAction, ResourceKind, RunState};
