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

use crate::backend::{ContainerBackend, ContainerError};
use crate::resource::ResourceKind;

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
            | Err(ContainerError::NotConfigured { .. }) => {
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

    Ok(reached)
}
