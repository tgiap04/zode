//! Kubernetes, through `kubectl`.
//!
//! Not the `kube` crate: it pulls `k8s-openapi`, which is one of the largest
//! compile-time dependencies in the ecosystem, and neither is in this
//! workspace's lockfile today. Release CI here was measured at ~145 minutes and
//! has just been cut; spending that saving on a dependency would undo it.
//! `kubectl get -o json` prints the same line-delimited JSON this crate already
//! reads from `docker ps`, so the cost is one external tool rather than one
//! large build.
//!
//! The one thing that is NOT inferred from a failed command is "nothing has been
//! chosen to look at". With no context, `kubectl` falls back to
//! `localhost:8080` and reports `connection refused` -- which would send
//! somebody hunting a cluster that was never the problem. So the kubeconfig is
//! read first, and an empty one answers `NotConfigured`.

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt as _};
use std::io;
use util::command::Command;

use crate::backend::{BackendEvent, BackendKind, ContainerBackend, ContainerError};
use crate::kubeconfig::{self, Kubeconfig};
use crate::kubernetes_types::PodList;
use crate::resource::{Resource, ResourceAction, ResourceKind};

const KUBERNETES_KINDS: &[ResourceKind] = &[ResourceKind::Pod];

/// A pod does not restart.
///
/// `kubectl` has no such verb, and the nearest thing --
/// `kubectl rollout restart` -- acts on a *controller*, not on the pod. Offering
/// Restart here would either do nothing or quietly do something else to
/// something else. Start and Stop are equally absent: a pod is created and
/// deleted, never paused.
const POD_ACTIONS: &[ResourceAction] = &[];

pub struct KubernetesBackend {
    program: String,
    /// Which context and namespace a list is taken from. `None` means "whatever
    /// the kubeconfig says is current", which is what a person running `kubectl`
    /// by hand would get.
    scope: Option<Scope>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub context: String,
    pub namespace: Option<String>,
}

impl KubernetesBackend {
    pub fn new() -> Self {
        Self {
            program: "kubectl".into(),
            scope: None,
        }
    }

    pub fn with_scope(scope: Scope) -> Self {
        Self {
            program: "kubectl".into(),
            scope: Some(scope),
        }
    }

    /// A backend pointed at a named program, for tests that need one which
    /// certainly is or is not there.
    #[cfg(test)]
    pub(crate) fn with_program(program: &str) -> Self {
        Self {
            program: program.into(),
            scope: None,
        }
    }

    /// What the kubeconfig offers. Read every time rather than cached: somebody
    /// running `kubectl config use-context` in a terminal beside the editor
    /// expects the panel to keep up.
    pub async fn kubeconfig(&self) -> Result<Kubeconfig, ContainerError> {
        let stdout = self.stdout(kubeconfig::command(&self.program)).await?;
        kubeconfig::parse(&stdout)
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(&self.program);
        command.args(args);
        if let Some(scope) = &self.scope {
            command.args(["--context", &scope.context]);
            if let Some(namespace) = &scope.namespace {
                command.args(["--namespace", namespace]);
            }
        }
        command
    }

    /// Runs a command and hands back stdout, classifying the ways it can fail.
    ///
    /// A missing binary is told apart structurally, by the spawn's own
    /// `NotFound`. Everything else is read out of stderr, because `kubectl`
    /// returns exit 1 for all of it and the words are the only distinguishing
    /// signal there is.
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
        Err(classify(&self.program, &stderr))
    }
}

impl Default for KubernetesBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Splits a `namespace/name` id back into its two halves.
///
/// An id with no slash is a name with no namespace, which is what a cluster
/// without namespaces (or a caller that built the id itself) would produce.
fn split_id(id: &str) -> (Option<String>, String) {
    match id.split_once('/') {
        Some((namespace, name)) if !namespace.is_empty() && !name.is_empty() => {
            (Some(namespace.to_string()), name.to_string())
        }
        _ => (None, id.to_string()),
    }
}

/// Reads `kubectl`'s own words for what went wrong.
///
/// Matched on substrings, which is fragile by nature -- so anything unrecognised
/// falls through to `CommandFailed` carrying the text verbatim, rather than being
/// forced into whichever variant looks closest. A wrong label is worse than no
/// label: it tells somebody to fix the wrong thing.
fn classify(program: &str, stderr: &str) -> ContainerError {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("forbidden") || lower.contains("cannot list resource") {
        return ContainerError::NotAuthorized {
            detail: stderr.trim().to_string(),
        };
    }
    if lower.contains("not found") && lower.contains("context") {
        return ContainerError::NotConfigured {
            detail: stderr.trim().to_string(),
        };
    }
    if lower.contains("connection refused")
        || lower.contains("could not find any host")
        || lower.contains("no such host")
        || lower.contains("i/o timeout")
    {
        return ContainerError::EngineNotRunning {
            program: program.to_string(),
        };
    }
    ContainerError::CommandFailed {
        program: program.to_string(),
        stderr: stderr.trim().to_string(),
    }
}

#[async_trait]
impl ContainerBackend for KubernetesBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Kubernetes
    }

    fn supported_kinds(&self) -> &'static [ResourceKind] {
        KUBERNETES_KINDS
    }

    fn supported_actions(&self, _kind: ResourceKind) -> &'static [ResourceAction] {
        POD_ACTIONS
    }

    async fn list(&self, kind: ResourceKind) -> Result<Vec<Resource>, ContainerError> {
        if kind != ResourceKind::Pod {
            return Err(ContainerError::CommandFailed {
                program: self.program.clone(),
                stderr: format!("{kind:?} is not a kind Kubernetes has a word for here"),
            });
        }

        // Asked before the `get`, and this order is the whole point: with no
        // context, `kubectl get` reports a refused connection to localhost:8080,
        // which is a true statement about the wrong thing.
        if self.scope.is_none() {
            let config = self.kubeconfig().await?;
            if config.is_empty() {
                return Err(ContainerError::NotConfigured {
                    detail: "no kubeconfig context to look at".into(),
                });
            }
            if config.effective().is_none() {
                return Err(ContainerError::NotConfigured {
                    detail: format!(
                        "{} contexts and none current; choose one",
                        config.contexts.len()
                    ),
                });
            }
        }

        let stdout = self
            .stdout(self.command(&["get", "pods", "-o", "json"]))
            .await?;
        let list: PodList =
            serde_json::from_str(&stdout).map_err(|error| ContainerError::Parse {
                detail: format!("could not read `kubectl get pods` output: {error}"),
            })?;
        Ok(list
            .items
            .into_iter()
            .map(|pod| pod.into_resource())
            .collect())
    }

    async fn act(
        &self,
        _kind: ResourceKind,
        action: ResourceAction,
        _id: &str,
    ) -> Result<(), ContainerError> {
        // Unreachable through the UI, which asks `supported_actions` first, and
        // deliberately an error rather than a silent success: a caller that got
        // here has misunderstood what a pod is.
        Err(ContainerError::CommandFailed {
            program: self.program.clone(),
            stderr: format!(
                "a pod cannot be {}ed -- it is created and deleted, and only its \
                 controller can be restarted",
                action.label().to_ascii_lowercase()
            ),
        })
    }

    /// Kubernetes has no prune. Nothing here is "unused" in the sense Docker
    /// means -- a pod either belongs to a controller or it does not.
    async fn prune_targets(
        &self,
        _scope: crate::PruneScope,
    ) -> Option<Result<Vec<Resource>, ContainerError>> {
        None
    }

    /// `kubectl delete pod`.
    ///
    /// Ordinary work in Kubernetes when the pod has a controller -- it comes
    /// straight back. Irreversible when it does not. The plan's warning carries
    /// that difference; this only runs what was confirmed.
    async fn destroy(&self, plan: &crate::DestructivePlan) -> Result<(), ContainerError> {
        let crate::Intent::Remove { kind } = plan.intent() else {
            return Err(ContainerError::CommandFailed {
                program: self.program.clone(),
                stderr: "kubernetes has nothing to prune".into(),
            });
        };
        if kind != ResourceKind::Pod {
            return Err(ContainerError::CommandFailed {
                program: self.program.clone(),
                stderr: format!("{kind:?} is not something this backend deletes"),
            });
        }
        for target in plan.targets() {
            let (namespace, name) = split_id(&target.id);
            let mut args = vec!["delete".to_string(), "pod".to_string()];
            if let Some(namespace) = namespace {
                args.push("-n".into());
                args.push(namespace);
            }
            args.push("--".into());
            args.push(name);
            let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
            self.stdout(self.command(&borrowed)).await?;
        }
        Ok(())
    }

    /// `kubectl logs -f -n <namespace> <pod>`.
    ///
    /// The id carries both halves (`namespace/name`) because a pod name is unique
    /// only within a namespace; it is split back apart here rather than being
    /// passed through as one word, which `kubectl` would read as a pod called
    /// "namespace/name".
    fn logs_command(&self, kind: ResourceKind, id: &str) -> Option<(String, Vec<String>)> {
        if kind != ResourceKind::Pod {
            return None;
        }
        let (namespace, name) = split_id(id);
        let mut args = vec!["logs".to_string(), "-f".to_string()];
        if let Some(namespace) = namespace {
            args.push("-n".into());
            args.push(namespace);
        }
        args.push("--".into());
        args.push(name);
        Some((self.program.clone(), args))
    }

    /// `kubectl exec -it -n <namespace> <pod> -- sh`.
    ///
    /// No `-c <container>`: a pod with several containers needs one chosen, and
    /// choosing silently would open a shell in whichever happened to be first.
    /// `kubectl` itself picks the first and says so on stderr, which is a
    /// visible, correctable outcome rather than a hidden one.
    fn exec_command(&self, kind: ResourceKind, id: &str) -> Option<(String, Vec<String>)> {
        if kind != ResourceKind::Pod {
            return None;
        }
        let (namespace, name) = split_id(id);
        let mut args = vec!["exec".to_string(), "-it".to_string()];
        if let Some(namespace) = namespace {
            args.push("-n".into());
            args.push(namespace);
        }
        args.push(name);
        args.push("--".into());
        args.push("sh".into());
        Some((self.program.clone(), args))
    }

    /// `kubectl get pods --watch`, mapped to "pods went stale".
    ///
    /// `--output-watch-events` (verified present on kubectl v1.34.1) wraps each
    /// object as `{"type":"ADDED","object":{...}}`. Only the fact that something
    /// arrived is used -- the same choice as Docker's, and for the same reason.
    ///
    /// Pretty-printed over several lines, unlike `docker events`. That is why the
    /// reader parses values out of a buffer rather than splitting on newlines.
    fn watch(&self) -> Option<BoxStream<'static, BackendEvent>> {
        // Without a scope this would watch whatever `kubectl` thinks is current,
        // and with no context that is `localhost:8080` -- a process that fails
        // and is restarted forever. A watch is only started once something has
        // been chosen.
        self.scope.as_ref()?;
        let command = self.command(&[
            "get",
            "pods",
            "--watch",
            "--output-watch-events",
            "-o",
            "json",
        ]);
        let values = crate::event_stream::json_values::<WatchEvent>(command)?;
        Some(
            values
                .map(|_| BackendEvent::Stale {
                    kind: ResourceKind::Pod,
                })
                .boxed(),
        )
    }
}

/// One `{"type":..,"object":..}` from `kubectl get --watch
/// --output-watch-events`.
///
/// Nothing is read out of it. It exists so the reader has a type to parse into
/// and so a half-arrived object is recognised as incomplete rather than as
/// garbage -- which is what makes the buffered reader work.
#[derive(Debug, Clone, serde::Deserialize)]
struct WatchEvent {
    #[serde(default, rename = "type")]
    _event_type: String,
}
