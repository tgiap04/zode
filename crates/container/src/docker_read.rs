//! The read-only calls `DockerBackend` answers beyond listing: inspect, stats
//! and "which containers use this".
//!
//! Split from `docker_backend.rs`, which is already the longest file in the
//! crate; these share its `stdout` and nothing else.

use crate::backend::{BackendKind, ContainerBackend as _, ContainerError};
use crate::docker_backend::{DockerBackend, parse_json_lines_strict};
use crate::docker_types::DockerContainer;
use crate::inspect::{Inspection, single_object};
use crate::resource::{Resource, ResourceKind};
use crate::stats::{Stats, parse_engine_stats};

impl DockerBackend {
    /// `docker <noun> inspect -- <id>`, mapped to sections.
    ///
    /// The noun is spelled out (`container inspect`, not bare `inspect`) so a
    /// volume and a container that happen to share a name cannot be confused.
    pub(crate) async fn inspect_resource(
        &self,
        kind: ResourceKind,
        id: &str,
    ) -> Result<Option<Inspection>, ContainerError> {
        let noun = match kind {
            ResourceKind::Container => "container",
            ResourceKind::Image => "image",
            ResourceKind::Volume => "volume",
            ResourceKind::Network => "network",
            _ => return Ok(None),
        };
        let stdout = self
            .stdout(&[noun, "inspect", "--", id])
            .await
            .map_err(|error| missing_means_not_found(error, id))?;
        let root = single_object(&stdout, id)?;
        Ok(Some(crate::docker_inspect::inspection(kind, &root)))
    }

    /// One `stats --no-stream` sample for exactly `ids`.
    ///
    /// Nothing is asked for an empty list: without ids the engine would sample
    /// every running container, which is not what was requested. Podman's
    /// `{{json .}}` template does not carry the same keys as its `json`
    /// format, so each engine is asked in the format its parser was written
    /// against.
    ///
    /// One id the engine no longer knows fails the whole call, so each time it
    /// says "no such container" the ids it names are dropped and the rest asked
    /// again. The ids of the answer are the ones that were asked for: Podman
    /// prints a short id where the list gave a long one.
    pub(crate) async fn engine_stats(&self, ids: &[String]) -> Result<Vec<Stats>, ContainerError> {
        let format = if self.kind() == BackendKind::Podman {
            "json"
        } else {
            "{{json .}}"
        };
        let mut asking: Vec<&str> = ids.iter().map(String::as_str).collect();
        while !asking.is_empty() {
            let mut args = vec!["stats", "--no-stream", "--format", format, "--"];
            args.extend(asking.iter().copied());
            match self.stdout(&args).await {
                Ok(stdout) => {
                    return parse_engine_stats(&stdout).map(|stats| align_ids(stats, ids));
                }
                Err(ContainerError::CommandFailed { program, stderr })
                    if stderr.to_ascii_lowercase().contains("no such container") =>
                {
                    let before = asking.len();
                    asking.retain(|id| !mentions(&stderr, id));
                    if asking.len() == before {
                        return Err(ContainerError::CommandFailed { program, stderr });
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Ok(Vec::new())
    }

    /// Containers (running or not) that mount a volume, are attached to a
    /// network, or descend from an image.
    ///
    /// The id is one argv word inside `--filter`, never a shell string.
    pub(crate) async fn containers_using(
        &self,
        kind: ResourceKind,
        id: &str,
    ) -> Result<Option<Vec<Resource>>, ContainerError> {
        let key = match kind {
            ResourceKind::Volume => "volume",
            ResourceKind::Network => "network",
            ResourceKind::Image => "ancestor",
            _ => return Ok(None),
        };
        let filter = format!("{key}={id}");
        let stdout = self
            .stdout(&["ps", "-a", "--filter", &filter, "--format", "{{json .}}"])
            .await?;
        // Strict: a row dropped here reads as "nothing depends on this".
        Ok(Some(
            parse_json_lines_strict::<DockerContainer>(&stdout)?
                .into_iter()
                .map(DockerContainer::into_resource)
                .collect(),
        ))
    }
}

/// Gives each sample the id it was asked for by: the engine's own id may be a
/// short or a long form of it, or the sample may be known by name only.
fn align_ids(mut stats: Vec<Stats>, asked: &[String]) -> Vec<Stats> {
    for sample in &mut stats {
        let name = sample.name.trim_start_matches('/');
        let handle = asked.iter().find(|handle| {
            !handle.is_empty()
                && (handle.as_str() == sample.id
                    || (!sample.id.is_empty()
                        && (handle.starts_with(&sample.id)
                            || sample.id.starts_with(handle.as_str())))
                    || handle.as_str() == name)
        });
        if let Some(handle) = handle {
            sample.id = handle.clone();
        }
    }
    stats
}

/// Whether `text` names `id` as a whole name, not as the start of a longer one
/// (`web` in `No such container: web2` is not a mention of `web`).
fn mentions(text: &str, id: &str) -> bool {
    let is_name =
        |character: char| character.is_ascii_alphanumeric() || matches!(character, '_' | '-');
    text.match_indices(id).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = &text[start + id.len()..];
        // A dot ends a sentence unless a name carries on after it (`web.1`).
        let name_continues = match after.chars().next() {
            Some('.') => after[1..].chars().next().is_some_and(is_name),
            Some(character) => is_name(character),
            None => false,
        };
        !before.is_some_and(is_name) && !name_continues
    })
}

/// The engine saying "No such container" is the resource having gone, not a
/// failure of the engine.
pub(crate) fn missing_means_not_found(error: ContainerError, id: &str) -> ContainerError {
    match &error {
        ContainerError::CommandFailed { stderr, .. }
            if stderr.to_ascii_lowercase().contains("no such") =>
        {
            ContainerError::NotFound { id: id.to_string() }
        }
        _ => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_engines_no_such_words_mean_the_resource_is_gone() {
        let failed = |stderr: &str| ContainerError::CommandFailed {
            program: "docker".into(),
            stderr: stderr.into(),
        };
        assert_eq!(
            missing_means_not_found(failed("Error: No such container: web"), "web"),
            ContainerError::NotFound { id: "web".into() }
        );
        let other = failed("permission denied");
        assert_eq!(missing_means_not_found(other.clone(), "web"), other);
        let not_running = ContainerError::EngineNotRunning {
            program: "docker".into(),
        };
        assert_eq!(
            missing_means_not_found(not_running.clone(), "web"),
            not_running
        );
    }
}
