//! The read-only calls `KubernetesBackend` answers beyond listing: inspect and
//! metrics.

use std::collections::BTreeMap;

use crate::backend::ContainerError;
use crate::inspect::{Inspection, single_object};
use crate::kubernetes_backend::{KubernetesBackend, split_id};
use crate::stats::{Stats, parse_kube_top};

impl KubernetesBackend {
    /// `kubectl get pod <name> -n <namespace> -o json`.
    ///
    /// The name goes before the flags and without a `--`: [`Self::command`]
    /// appends `--context` after everything given here, and past a `--`
    /// kubectl would read it as another positional. A pod name is a DNS label
    /// and cannot start with a dash; an id that does is refused rather than
    /// trusted to be one (`-A` would list every pod of every namespace).
    pub(crate) async fn inspect_pod(&self, id: &str) -> Result<Inspection, ContainerError> {
        let (namespace, name) = checked_pod_id(id)?;
        let mut args = vec!["get", "pod", name.as_str()];
        if let Some(namespace) = namespace.as_deref() {
            args.extend(["-n", namespace]);
        }
        args.extend(["-o", "json"]);
        let stdout = self
            .stdout(self.command(&args)?)
            .await
            .map_err(|error| missing_means_not_found(error, id))?;
        let root = single_object(&stdout, id)?;
        Ok(crate::kubernetes_inspect::inspection(&root))
    }

    /// `kubectl top pod` per namespace, filtered to the pods asked about.
    ///
    /// `Ok(None)` when the cluster has no metrics server: that is the ordinary
    /// state of a fresh cluster, not a failure to report on every poll.
    /// `kubectl top pod` takes at most one name, so a namespace is listed whole
    /// and the rows picked out of it.
    pub(crate) async fn top_pods(
        &self,
        ids: &[String],
    ) -> Result<Option<Vec<Stats>>, ContainerError> {
        let mut by_namespace: BTreeMap<Option<String>, Vec<String>> = BTreeMap::new();
        for id in ids {
            let (namespace, _) = checked_pod_id(id)?;
            by_namespace.entry(namespace).or_default().push(id.clone());
        }

        let mut collected = Vec::new();
        for (namespace, wanted) in by_namespace {
            let mut args = vec!["top", "pod", "--no-headers"];
            if let Some(namespace) = namespace.as_deref() {
                args.extend(["-n", namespace]);
            }
            let stdout = match self.stdout(self.command(&args)?).await {
                Ok(stdout) => stdout,
                Err(error) if metrics_unavailable(&error) => return Ok(None),
                Err(error) => return Err(error),
            };
            collected.extend(
                parse_kube_top(&stdout, namespace.as_deref())
                    .into_iter()
                    .filter(|stats| wanted.contains(&stats.id)),
            );
        }
        Ok(Some(collected))
    }
}

/// `namespace/name` split, or refused when either half starts with `-`:
/// `kubectl` reads such a word as one of its own flags wherever it is placed.
fn checked_pod_id(id: &str) -> Result<(Option<String>, String), ContainerError> {
    let (namespace, name) = split_id(id);
    for (what, value) in [
        ("namespace", namespace.as_deref()),
        ("pod name", Some(name.as_str())),
    ] {
        if let Some(value) = value.filter(|value| value.starts_with('-')) {
            return Err(ContainerError::Invalid {
                detail: format!("{what} must not start with `-`: {value:?}"),
            });
        }
    }
    Ok((namespace, name))
}

fn missing_means_not_found(error: ContainerError, id: &str) -> ContainerError {
    match &error {
        ContainerError::CommandFailed { stderr, .. }
            if stderr.to_ascii_lowercase().contains("not found") =>
        {
            ContainerError::NotFound { id: id.to_string() }
        }
        _ => error,
    }
}

/// kubectl's words when nothing answers the metrics API: `error: Metrics API
/// not available`, or a 404 for `pods.metrics.k8s.io`.
pub(crate) fn metrics_unavailable(error: &ContainerError) -> bool {
    match error {
        ContainerError::CommandFailed { stderr, .. } => {
            let lower = stderr.to_ascii_lowercase();
            lower.contains("metrics api not available") || lower.contains("metrics.k8s.io")
        }
        _ => false,
    }
}
