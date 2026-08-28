//! What the kubeconfig says, without reading the kubeconfig.
//!
//! Read through `kubectl config view -o json` rather than by parsing
//! `~/.kube/config`: the real answer is the merge of `$KUBECONFIG`, the default
//! path and any in-cluster service account, and `kubectl` is the only thing that
//! knows how that merge comes out. Parsing the file would be a second, wrong
//! implementation of it.
//!
//! `kubectl config get-contexts -o json` does NOT work -- kubectl refuses
//! `--output json` for that subcommand outright ("--output json is not available
//! in kubectl config get-contexts"). `config view` is the one that answers in
//! JSON.

use serde::Deserialize;
use util::command::Command;

use crate::backend::ContainerError;

/// One context the kubeconfig offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KubeContext {
    pub name: String,
    /// The namespace the context itself names, if any. Absent means `default`,
    /// which is kubectl's own rule rather than a guess made here.
    pub namespace: Option<String>,
}

/// Everything the panel needs to choose what to look at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Kubeconfig {
    pub contexts: Vec<KubeContext>,
    pub current: Option<String>,
}

impl Kubeconfig {
    /// Whether there is anything at all to look at.
    ///
    /// A kubeconfig with no contexts is the ordinary state of a developer
    /// machine, not a fault -- this very repository's machine is one. It has to
    /// be answered before any `get` runs, because with no context `kubectl`
    /// falls back to `localhost:8080` and reports a refused connection.
    pub fn is_empty(&self) -> bool {
        self.contexts.is_empty()
    }

    /// The context a list would be taken from: the one named current, or the
    /// only one there is.
    pub fn effective(&self) -> Option<&KubeContext> {
        if let Some(current) = &self.current
            && !current.is_empty()
        {
            return self.contexts.iter().find(|c| &c.name == current);
        }
        // Exactly one and nothing named current is unambiguous. Two with nothing
        // named current is not, and guessing there would silently point somebody
        // at the wrong cluster.
        match self.contexts.as_slice() {
            [only] => Some(only),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    /// Null rather than absent when there are none -- observed on kubectl
    /// v1.34.1 against an empty kubeconfig.
    #[serde(default)]
    contexts: Option<Vec<RawNamedContext>>,
    #[serde(default, rename = "current-context")]
    current_context: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawNamedContext {
    name: String,
    #[serde(default)]
    context: Option<RawContextBody>,
}

#[derive(Debug, Deserialize)]
struct RawContextBody {
    #[serde(default)]
    namespace: Option<String>,
}

pub(crate) fn parse(stdout: &str) -> Result<Kubeconfig, ContainerError> {
    let raw: RawConfig = serde_json::from_str(stdout).map_err(|error| ContainerError::Parse {
        detail: format!("could not read `kubectl config view` output: {error}"),
    })?;
    Ok(Kubeconfig {
        contexts: raw
            .contexts
            .unwrap_or_default()
            .into_iter()
            .map(|named| KubeContext {
                name: named.name,
                namespace: named.context.and_then(|body| body.namespace),
            })
            .collect(),
        current: raw.current_context.filter(|name| !name.is_empty()),
    })
}

pub(crate) fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.args(["config", "view", "-o", "json"]);
    command
}
