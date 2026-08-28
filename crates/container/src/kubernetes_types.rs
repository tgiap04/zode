//! The `v1.Pod` fields this crate reads, as `kubectl get -o json` prints them.
//!
//! **Not observed output.** Written from the documented `v1.Pod` schema, because
//! the machine this was built on has `kubectl` v1.34.1 and no cluster at all
//! (`kubectl config view -o json` reports `"contexts": null`). The schema is
//! versioned and stable, which is why it is safe to write against -- but it has
//! not been checked against a live server, and Phase 04's success criteria
//! record that as an open debt rather than hiding it.
//!
//! camelCase, unlike Docker's PascalCase: the Kubernetes API is camelCase
//! throughout. Every field past the name is optional, so a cluster running an
//! older or newer API leaves blanks rather than failing the whole list.

use serde::Deserialize;

use crate::resource::{Resource, ResourceKind, RunState};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct PodList {
    #[serde(default)]
    pub(crate) items: Vec<Pod>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct Pod {
    pub(crate) metadata: PodMetadata,
    #[serde(default)]
    pub(crate) spec: Option<PodSpec>,
    #[serde(default)]
    pub(crate) status: Option<PodStatus>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct PodMetadata {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) namespace: Option<String>,
    /// What made this pod. Empty means nothing did -- a bare pod, which nothing
    /// will recreate if it is deleted. That distinction decides which actions
    /// the panel offers and how loudly it warns, so it is read here rather than
    /// inferred later.
    #[serde(default, rename = "ownerReferences")]
    pub(crate) owner_references: Vec<OwnerReference>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct OwnerReference {
    pub(crate) kind: String,
    pub(crate) name: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct PodSpec {
    #[serde(default)]
    pub(crate) containers: Vec<PodContainer>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct PodContainer {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) image: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct PodStatus {
    /// `Pending`, `Running`, `Succeeded`, `Failed`, `Unknown`.
    #[serde(default)]
    pub(crate) phase: Option<String>,
}

impl Pod {
    /// The controller that owns this pod, if one does.
    ///
    /// Only controller kinds count. A pod owned by nothing but, say, another pod
    /// is still a pod nothing will recreate.
    pub(crate) fn controller(&self) -> Option<&OwnerReference> {
        self.metadata.owner_references.iter().find(|owner| {
            matches!(
                owner.kind.as_str(),
                "ReplicaSet" | "StatefulSet" | "DaemonSet" | "Job" | "ReplicationController"
            )
        })
    }

    pub(crate) fn into_resource(self) -> Resource {
        let phase = self
            .status
            .as_ref()
            .and_then(|status| status.phase.as_deref())
            .unwrap_or_default();
        let state = match phase {
            "Running" => RunState::Running,
            "Succeeded" | "Failed" => RunState::Stopped,
            // `Pending` is neither: it is on its way up. A stopped dot beside a
            // pod that is starting reads as broken, and a running dot beside one
            // with no container yet reads as ready. Neither is true.
            _ => RunState::Unknown,
        };

        let namespace = self
            .metadata
            .namespace
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let image = self
            .spec
            .as_ref()
            .and_then(|spec| spec.containers.first())
            .and_then(|container| container.image.clone())
            .unwrap_or_default();
        let controller = self
            .controller()
            .map(|owner| format!("{}/{}", owner.kind, owner.name));

        let mut detail = vec![("Image", image), ("Namespace", namespace.clone())];
        if !phase.is_empty() {
            detail.push(("Status", phase.to_string()));
        }
        if let Some(controller) = &controller {
            detail.push(("Controller", controller.clone()));
        }

        Resource {
            kind: ResourceKind::Pod,
            // Namespaced, because a pod name is only unique within one. `act`
            // splits it back apart rather than carrying a second field, which
            // would have to be threaded through every caller for this one
            // backend.
            id: format!("{namespace}/{}", self.metadata.name),
            name: self.metadata.name,
            state,
            detail,
            parent: controller,
        }
    }
}
