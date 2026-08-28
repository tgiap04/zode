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

use crate::resource::{Resource, ResourceKind, RunState};

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
        }
    }
}
