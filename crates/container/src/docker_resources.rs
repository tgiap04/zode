//! The Docker resources that are not containers.
//!
//! One file rather than four: these are four mappings of the same shape --
//! parse the engine's PascalCase JSON, pick the fields a row needs, hand back a
//! [`Resource`]. Four files would be four headers for one idea, which CLAUDE.md's
//! "avoid creating many small files" is about.
//!
//! Field names are **observed**, on Docker 29.4.3, not taken from documentation:
//!
//! - `docker images`: `Containers, CreatedAt, CreatedSince, Digest, ID,
//!   Repository, SharedSize, Size, Tag, UniqueSize`
//! - `docker volume ls`: `Availability, Driver, Group, Labels, Links,
//!   Mountpoint, Name, Scope, Size, Status`
//! - `docker network ls`: `CreatedAt, Driver, ID, IPv4, IPv6, Internal, Labels,
//!   Name, Scope`
//!
//! `docker compose ls --format json` is the exception: it was run and returns a
//! JSON **array** (`[]` with no projects) rather than one object per line, but no
//! project existed to observe the keys of. Those are written from the documented
//! shape and marked below.

use serde::Deserialize;

use crate::resource::{Resource, ResourceKind, RunState};

/// What Docker prints where a repository or tag is absent.
const NONE: &str = "<none>";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DockerImage {
    #[serde(rename = "ID")]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) repository: String,
    #[serde(default)]
    pub(crate) tag: String,
    /// Human text -- `"1.2GB"`. Shown, never parsed, and above all never sorted
    /// on: `"999MB"` sorts after `"1.0GB"` as a string, which is the wrong order
    /// and looks plausible.
    #[serde(default)]
    pub(crate) size: String,
    #[serde(default)]
    pub(crate) created_since: String,
}

impl DockerImage {
    /// An image nothing names. Docker prints `<none>` for both halves.
    fn is_dangling(&self) -> bool {
        self.repository.is_empty() || self.repository == NONE
    }

    pub(crate) fn into_resource(self) -> Resource {
        let dangling = self.is_dangling();
        let name = if dangling {
            // Its short id is the only handle a person has on it.
            format!("{NONE} ({})", self.id)
        } else if self.tag.is_empty() || self.tag == NONE {
            self.repository.clone()
        } else {
            format!("{}:{}", self.repository, self.tag)
        };
        let mut detail = Vec::new();
        if !self.size.is_empty() {
            detail.push(("Size", self.size));
        }
        if !self.created_since.is_empty() {
            detail.push(("Created", self.created_since));
        }
        if dangling {
            detail.push(("Dangling", "yes".to_string()));
        }
        Resource {
            kind: ResourceKind::Image,
            id: self.id,
            name,
            // An image is not something that runs. A dot here would say
            // "stopped", which is not true of a thing that was never running.
            state: RunState::Unknown,
            detail,
            parent: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DockerVolume {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) driver: String,
    #[serde(default)]
    pub(crate) mountpoint: String,
}

impl DockerVolume {
    pub(crate) fn into_resource(self) -> Resource {
        let mut detail = Vec::new();
        if !self.driver.is_empty() {
            detail.push(("Driver", self.driver));
        }
        if !self.mountpoint.is_empty() {
            detail.push(("Mountpoint", self.mountpoint));
        }
        Resource {
            kind: ResourceKind::Volume,
            // A volume's name *is* its handle; there is no separate id.
            id: self.name.clone(),
            name: self.name,
            state: RunState::Unknown,
            detail,
            parent: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DockerNetwork {
    #[serde(rename = "ID")]
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) driver: String,
    #[serde(default)]
    pub(crate) scope: String,
}

impl DockerNetwork {
    pub(crate) fn into_resource(self) -> Resource {
        let mut detail = Vec::new();
        if !self.driver.is_empty() {
            detail.push(("Driver", self.driver));
        }
        if !self.scope.is_empty() {
            detail.push(("Scope", self.scope));
        }
        Resource {
            kind: ResourceKind::Network,
            id: self.id,
            name: self.name,
            state: RunState::Unknown,
            detail,
            parent: None,
        }
    }
}

/// One entry of `docker compose ls --format json`.
///
/// **Keys not observed:** the command was run and answers `[]`, but no project
/// existed on this machine to print. Written from the documented shape. Anyone
/// with a compose project running should check these against real output.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ComposeProject {
    pub(crate) name: String,
    /// Human text -- `"running(3)"`.
    #[serde(default)]
    pub(crate) status: String,
    #[serde(default)]
    pub(crate) config_files: String,
}

impl ComposeProject {
    pub(crate) fn into_resource(self) -> Resource {
        // "running(3)" and "exited(2)" are the shapes Docker prints. Matched on
        // the leading word rather than parsed, because the count in brackets is
        // not what the dot is about.
        let state = if self.status.starts_with("running") {
            RunState::Running
        } else if self.status.is_empty() {
            RunState::Unknown
        } else {
            RunState::Stopped
        };
        let mut detail = Vec::new();
        if !self.status.is_empty() {
            detail.push(("Status", self.status));
        }
        if !self.config_files.is_empty() {
            detail.push(("Config", self.config_files));
        }
        Resource {
            kind: ResourceKind::ComposeProject,
            id: self.name.clone(),
            name: self.name,
            state,
            detail,
            parent: None,
        }
    }
}
