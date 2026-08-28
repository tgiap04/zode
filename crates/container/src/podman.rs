//! What Podman has that Docker does not: a pod.
//!
//! Podman shares Docker's command line for containers, images, volumes and
//! networks, so those go through the same backend unchanged. A pod is the one
//! object with no Docker equivalent, which is why it lives here and why
//! `supported_kinds` is asked per engine rather than being a shared constant.
//!
//! **Keys not observed.** Podman is not installed on the machine this was
//! written on, so this follows podman's documented `pod ps --format json`
//! output. Phase 08's success criteria record that as an open debt rather than
//! hiding it; anyone with podman should check it and say so here.

use serde::Deserialize;

use crate::resource::{Resource, ResourceKind, RunState};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct PodmanPod {
    #[serde(default, alias = "ID")]
    pub(crate) id: String,
    #[serde(default, alias = "Name")]
    pub(crate) name: String,
    /// `Running`, `Exited`, `Created`, `Degraded`, ...
    #[serde(default, alias = "Status")]
    pub(crate) status: String,
    /// How many containers the pod holds. Podman prints these as a list; only
    /// the count is shown, because the containers themselves are listed under
    /// Containers with this pod as their parent.
    #[serde(default, alias = "Containers")]
    pub(crate) containers: Vec<serde_json::Value>,
}

impl PodmanPod {
    pub(crate) fn into_resource(self) -> Resource {
        // Matched case-insensitively: podman has printed both `Running` and
        // `running` across versions, and a change of case is not a change of
        // meaning.
        let state = match self.status.to_ascii_lowercase().as_str() {
            "running" => RunState::Running,
            "exited" | "created" | "stopped" | "dead" => RunState::Stopped,
            // `Degraded` means some containers are up and some are not. Neither
            // dot is true, so neither is drawn.
            _ => RunState::Unknown,
        };
        let mut detail = Vec::new();
        if !self.status.is_empty() {
            detail.push(("Status", self.status));
        }
        detail.push(("Containers", self.containers.len().to_string()));
        let name = if self.name.is_empty() {
            self.id.clone()
        } else {
            self.name
        };
        Resource {
            kind: ResourceKind::Pod,
            id: self.id,
            name,
            state,
            detail,
            parent: None,
        }
    }
}
