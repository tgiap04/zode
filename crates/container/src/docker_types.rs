//! The engine's own words, as it prints them.
//!
//! Field names are PascalCase because that is what `--format '{{json .}}'`
//! emits, observed against Docker 29.4.3 rather than taken from documentation:
//! `docker ps -a` gives `Command, CreatedAt, ID, Image, Labels, LocalVolumes,
//! Mounts, Names, Networks, Platform, Ports, RunningFor, Size, State, Status`.
//!
//! Every field is optional past the two that identify a row. Docker adds columns
//! between releases, and a missing one must leave a blank cell rather than fail
//! the whole list.

use serde::Deserialize;

use std::collections::HashMap;

use crate::resource::{ComposeLabels, Resource, ResourceKind, RunState};

const COMPOSE_PROJECT: &str = "com.docker.compose.project";
const COMPOSE_SERVICE: &str = "com.docker.compose.service";
const COMPOSE_WORKING_DIR: &str = "com.docker.compose.project.working_dir";

/// `Labels` as the engines print it: Docker joins `key=value` pairs with commas
/// into one string, Podman may print an object. Either, or null, must read.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub(crate) enum Labels {
    Joined(String),
    Map(HashMap<String, String>),
    #[default]
    Absent,
}

impl Labels {
    fn compose(self) -> Option<ComposeLabels> {
        let mut pairs = match self {
            Labels::Map(map) => map,
            Labels::Joined(text) => split_joined_labels(&text),
            Labels::Absent => return None,
        };
        let working_dir = pairs
            .remove(COMPOSE_WORKING_DIR)
            .filter(|dir| !dir.is_empty());
        let project = pairs.remove(COMPOSE_PROJECT);
        let service = pairs.remove(COMPOSE_SERVICE);
        if working_dir.is_none() && project.is_none() && service.is_none() {
            return None;
        }
        Some(ComposeLabels {
            project: project.unwrap_or_default(),
            service: service.unwrap_or_default(),
            working_dir,
        })
    }
}

/// Splits Docker's `a=b,c=d` label string.
///
/// Only the first `=` of a piece divides key from value, so a value may hold
/// `=`. A value may also hold commas, and the one signal that a comma is not a
/// separator is what follows it: Docker joins with a bare `,`, so a piece that
/// has no `=` or that starts with whitespace is the rest of the previous value
/// (`/srv/a, x=y` is one directory, not a directory and a label).
fn split_joined_labels(text: &str) -> HashMap<String, String> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for piece in text.split(',') {
        let starts_a_label = !piece.starts_with(char::is_whitespace);
        match piece.split_once('=').filter(|_| starts_a_label) {
            Some((key, value)) => pairs.push((key.to_string(), value.to_string())),
            None => {
                if let Some((_, value)) = pairs.last_mut() {
                    value.push(',');
                    value.push_str(piece);
                }
            }
        }
    }
    pairs.into_iter().collect()
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DockerContainer {
    #[serde(rename = "ID")]
    pub(crate) id: String,
    /// Plural, and comma-separated when a container carries more than one.
    #[serde(default)]
    pub(crate) names: String,
    #[serde(default)]
    pub(crate) image: String,
    /// `running`, `exited`, `created`, `paused`, ... Lower-case in Docker's
    /// output; matched case-insensitively so a change of case is not a change of
    /// meaning.
    #[serde(default)]
    pub(crate) state: String,
    /// Human text -- "Up 2 minutes". Shown, never parsed.
    #[serde(default)]
    pub(crate) status: String,
    #[serde(default)]
    pub(crate) ports: String,
    #[serde(default)]
    pub(crate) labels: Labels,
}

impl DockerContainer {
    pub(crate) fn into_resource(self) -> Resource {
        let state = match self.state.to_ascii_lowercase().as_str() {
            "running" => RunState::Running,
            "paused" => RunState::Paused,
            "exited" | "created" | "dead" | "removing" | "restarting" => RunState::Stopped,
            // An unknown word from a newer Docker is not "stopped" -- guessing
            // would put a misleading dot beside a container that is up.
            _ => RunState::Unknown,
        };
        // The first of the comma-separated names is the one Docker itself shows.
        let name = self
            .names
            .split(',')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.id)
            .to_string();
        let mut detail = vec![("Image", self.image)];
        if !self.status.is_empty() {
            detail.push(("Status", self.status));
        }
        if !self.ports.is_empty() {
            detail.push(("Ports", self.ports));
        }
        Resource {
            kind: ResourceKind::Container,
            id: self.id,
            name,
            state,
            detail,
            parent: None,
            compose: self.labels.compose(),
        }
    }
}
