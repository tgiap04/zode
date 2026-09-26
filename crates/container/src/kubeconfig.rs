//! What the kubeconfig says, without reading the kubeconfig.
//!
//! Read through `kubectl config view -o json` rather than by parsing
//! `~/.kube/config`: the real answer is the merge of `$KUBECONFIG`, the default
//! path and any in-cluster service account, and `kubectl` is the only thing that
//! knows how that merge comes out. Parsing the file would be a second, wrong
//! implementation of it.
//!
//! That merge is the *default*, not the only path. When somebody has picked a
//! file through the panel's own picker, `command` is given an explicit
//! `--kubeconfig <path>`, and that flag *replaces* the `$KUBECONFIG` merge
//! rather than joining it -- deliberately: the user picked one file, and a
//! result quietly blended with whatever else was on `$KUBECONFIG` would not be
//! the file they chose. Confirmed by hand: `kubectl config view --kubeconfig
//! <path>` with `$KUBECONFIG` also set to a different file returns only what
//! `<path>` contains.
//!
//! `kubectl config get-contexts -o json` does NOT work -- kubectl refuses
//! `--output json` for that subcommand outright ("--output json is not available
//! in kubectl config get-contexts"). `config view` is the one that answers in
//! JSON.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use util::command::Command;

use crate::backend::ContainerError;

/// A file on disk naming what an engine can be aimed at, and what it is aimed
/// at right now.
///
/// `label` names the control for the view -- "Kubeconfig File" -- without the
/// view learning the word "kubeconfig" itself; it only ever repeats a string
/// this crate gave it.
pub struct ConfigSource {
    pub label: &'static str,
    pub path: Option<PathBuf>,
    pub target: Option<String>,
}

/// One thing a config file offers to be aimed at -- a context, in
/// Kubernetes's words, though the view never learns that word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigTarget {
    pub name: String,
    /// Opaque to the view, which only hands back what it was given -- that is
    /// what stops `container_ui` from learning the word "namespace".
    pub detail: Option<String>,
}

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

/// Builds `kubectl config view -o json`, scoped to `path` when one is given.
///
/// `path` goes *before* the subcommand and *replaces* the `$KUBECONFIG` merge
/// described at the top of this module -- see there for how that was checked.
pub(crate) fn command(program: &str, path: Option<&Path>) -> Command {
    let mut command = Command::new(program);
    if let Some(path) = path {
        command.arg("--kubeconfig").arg(path);
    }
    command.args(["config", "view", "-o", "json"]);
    command
}
