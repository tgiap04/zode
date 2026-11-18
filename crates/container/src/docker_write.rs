//! What `DockerBackend` does beyond reading: compose, image, volume and network
//! operations.
//!
//! Every command is a program and an argument list, each value its own element.
//! A value is checked (see [`crate::operations`]) before it is placed, and a
//! positional value follows a `--`, so a name the engine would read as an option
//! is refused first and could not be one afterwards either.

use std::collections::HashSet;

use serde_json::Value;

use crate::backend::{BackendKind, ContainerBackend as _, ContainerError};
use crate::destructive::DestructivePlan;
use crate::docker_backend::{DockerBackend, parse_json_lines, parse_json_lines_strict};
use crate::docker_read::missing_means_not_found;
use crate::docker_resources::{DockerNetwork, DockerVolume};
use crate::docker_types::DockerContainer;
use crate::inspect::{field, format_bytes, number, text};
use crate::operations::{
    ComposeOp, ComposeTarget, ImageLayer, RunSpec, check_env_key, check_label_key, check_mount,
    check_name, check_path, check_port, check_reference, check_subnet, check_text,
};
use crate::resource::Resource;

const COMPOSE_PROJECT_LABEL: &str = "com.docker.compose.project";
/// What the engine stamps on a volume it created for a container that was
/// given none by name.
const ANONYMOUS_VOLUME_LABEL: &str = "com.docker.volume.anonymous";

/// A volume name made by the engine rather than a person: 64 hex digits.
///
/// Engines older than the anonymous label, and Podman, name them this way and
/// mark nothing, so the name is the only sign there is.
fn is_generated_volume_name(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn owned(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| arg.to_string()).collect()
}

/// `compose -p <project> [--project-directory <dir>] [-f <file>]...`.
fn compose_prefix(target: &ComposeTarget) -> Result<Vec<String>, ContainerError> {
    check_name("compose project", &target.project)?;
    let mut args = owned(&["compose", "-p", &target.project]);
    if let Some(directory) = &target.working_dir {
        check_path("compose project directory", directory)?;
        args.extend(["--project-directory".to_string(), directory.clone()]);
    }
    for file in &target.config_files {
        check_path("compose file", file)?;
        args.extend(["-f".to_string(), file.clone()]);
    }
    Ok(args)
}

pub(crate) fn compose_verb(op: ComposeOp) -> &'static [&'static str] {
    match op {
        ComposeOp::Up => &["up", "-d"],
        ComposeOp::Restart => &["restart"],
        ComposeOp::Stop => &["stop"],
        ComposeOp::Start => &["start"],
        ComposeOp::Pull => &["pull"],
        ComposeOp::Build => &["build"],
        ComposeOp::Logs => &["logs", "-f"],
    }
}

pub(crate) fn compose_argv(
    target: &ComposeTarget,
    verb: &[&str],
    services: &[String],
) -> Result<Vec<String>, ContainerError> {
    let mut args = compose_prefix(target)?;
    args.extend(owned(verb));
    for service in services {
        check_name("compose service", service)?;
        args.push(service.clone());
    }
    Ok(args)
}

pub(crate) fn run_argv(spec: &RunSpec) -> Result<Vec<String>, ContainerError> {
    check_reference("image", &spec.image)?;
    let mut args = owned(&["run"]);
    if spec.detach {
        args.push("--detach".into());
    }
    if spec.remove_on_exit {
        args.push("--rm".into());
    }
    if let Some(name) = &spec.name {
        check_name("container name", name)?;
        args.extend(["--name".to_string(), name.clone()]);
    }
    if let Some(network) = &spec.network {
        check_name("network", network)?;
        args.extend(["--network".to_string(), network.clone()]);
    }
    for mapping in &spec.ports {
        check_port(mapping)?;
        args.extend([
            "--publish".to_string(),
            format!(
                "{}:{}/{}",
                mapping.host,
                mapping.container,
                mapping.protocol.word()
            ),
        ]);
    }
    for (key, value) in &spec.env {
        check_env_key(key)?;
        check_text("environment variable value", value)?;
        args.extend(["--env".to_string(), format!("{key}={value}")]);
    }
    for mount in &spec.volumes {
        check_mount(mount)?;
        let suffix = if mount.read_only { ":ro" } else { "" };
        args.extend([
            "--volume".to_string(),
            format!("{}:{}{suffix}", mount.source, mount.target),
        ]);
    }
    args.push(spec.image.clone());
    for word in &spec.command {
        check_text("command", word)?;
        args.push(word.clone());
    }
    Ok(args)
}

/// What `history` printed, whichever engine printed it: Docker one JSON object
/// per line with PascalCase keys, Podman an array whose keys differ in case and
/// whose sizes are numbers. **Podman's keys not observed** -- read tolerantly.
fn parse_history(stdout: &str) -> Result<Vec<ImageLayer>, ContainerError> {
    let trimmed = stdout.trim();
    let rows: Vec<Value> = if trimmed.starts_with('[') {
        serde_json::from_str(trimmed).map_err(|error| ContainerError::Parse {
            detail: format!("could not read the image history: {error}"),
        })?
    } else {
        parse_json_lines(trimmed)
    };
    Ok(rows
        .iter()
        .map(|row| ImageLayer {
            created_by: text(field(row, &["CreatedBy", "createdBy", "created_by"]))
                .unwrap_or_default(),
            size: match field(row, &["Size", "size"]) {
                Some(Value::Number(_)) => number(field(row, &["Size", "size"]))
                    .map(format_bytes)
                    .unwrap_or_default(),
                other => text(other).unwrap_or_default(),
            },
            created: text(field(
                row,
                &[
                    "CreatedSince",
                    "CreatedAt",
                    "createdAt",
                    "created",
                    "Created",
                ],
            ))
            .unwrap_or_default(),
        })
        .collect())
}

impl DockerBackend {
    async fn stdout_of(&self, args: Vec<String>) -> Result<String, ContainerError> {
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        self.stdout(&borrowed).await
    }

    pub(crate) fn compose_terminal_command(
        &self,
        target: &ComposeTarget,
        op: ComposeOp,
        services: &[String],
    ) -> Option<(String, Vec<String>)> {
        target
            .require_config_files()
            .and_then(|()| compose_argv(target, compose_verb(op), services))
            .inspect_err(|error| log::warn!("no compose command offered: {error}"))
            .ok()
            .map(|args| (self.program().to_string(), args))
    }

    /// Whether `<engine> compose version` answers.
    ///
    /// A yes is remembered for the life of the backend; a no is asked again,
    /// because installing compose while the panel is open is an ordinary thing
    /// to do and the next menu should notice.
    pub(crate) async fn probe_compose(&self) -> bool {
        if self.compose_confirmed() {
            return true;
        }
        let present = self.stdout(&["compose", "version"]).await.is_ok();
        if present {
            self.confirm_compose();
        }
        present
    }

    /// Lists what `compose down` for `target` would remove.
    ///
    /// Containers and networks by the project's own label, the same thing
    /// compose selects on. With `--volumes`, compose also removes the
    /// *anonymous* volumes of those containers (`docker rm -v`), which carry no
    /// project label -- a plan that listed only the labelled volumes would say
    /// "5 volumes" and take 23. So the plan is the labelled volumes plus the
    /// anonymous ones the project's containers mount; never a named volume the
    /// project merely uses, which is external and which compose leaves alone.
    ///
    /// Built from read-only listings and not from `compose down --dry-run`:
    /// that prints human text, does not itemise anonymous volumes (checked
    /// against Compose v5.4: it names only the volumes the compose file
    /// declares), needs the compose files to load, and a command whose
    /// failure mode is deletion is the wrong thing to ask for a confirmation.
    pub(crate) async fn gather_compose_down(
        &self,
        target: &ComposeTarget,
        with_volumes: bool,
    ) -> Result<Option<DestructivePlan>, ContainerError> {
        check_name("compose project", &target.project)?;
        target.require_config_files()?;
        let filter = format!("label={COMPOSE_PROJECT_LABEL}={}", target.project);
        let containers = parse_json_lines_strict::<DockerContainer>(
            &self
                .stdout(&["ps", "-a", "--filter", &filter, "--format", "{{json .}}"])
                .await?,
        )?;
        let networks = parse_json_lines_strict::<DockerNetwork>(
            &self
                .stdout(&[
                    "network",
                    "ls",
                    "--filter",
                    &filter,
                    "--format",
                    "{{json .}}",
                ])
                .await?,
        )?;
        let volumes = if with_volumes {
            Some(self.volumes_taken_by_down(&filter, &containers).await?)
        } else {
            None
        };
        Ok(DestructivePlan::compose_down(
            target.clone(),
            containers
                .into_iter()
                .map(DockerContainer::into_resource)
                .collect(),
            networks
                .into_iter()
                .map(DockerNetwork::into_resource)
                .collect(),
            volumes,
        ))
    }

    /// The project's labelled volumes, then the anonymous volumes of
    /// `containers`, each once.
    async fn volumes_taken_by_down(
        &self,
        project_filter: &str,
        containers: &[DockerContainer],
    ) -> Result<Vec<Resource>, ContainerError> {
        let labelled = parse_json_lines_strict::<DockerVolume>(
            &self
                .stdout(&[
                    "volume",
                    "ls",
                    "--filter",
                    project_filter,
                    "--format",
                    "{{json .}}",
                ])
                .await?,
        )?;
        let mut volumes: Vec<Resource> = labelled
            .into_iter()
            .map(DockerVolume::into_resource)
            .collect();
        let mut seen: HashSet<String> = volumes.iter().map(|volume| volume.id.clone()).collect();

        let mut candidates = Vec::new();
        for name in self.mounted_volume_names(containers).await? {
            if seen.insert(name.clone()) {
                candidates.push(name);
            }
        }
        if candidates.is_empty() {
            return Ok(volumes);
        }

        // Only the candidates are asked about: a machine can hold hundreds of
        // anonymous volumes of other projects.
        let name_filters: Vec<String> = candidates
            .iter()
            .map(|name| format!("name={name}"))
            .collect();
        let anonymous_label = format!("label={ANONYMOUS_VOLUME_LABEL}");
        let mut args = vec!["volume", "ls", "--filter", anonymous_label.as_str()];
        for name_filter in &name_filters {
            args.extend(["--filter", name_filter.as_str()]);
        }
        args.extend(["--format", "{{json .}}"]);
        let marked = parse_json_lines_strict::<DockerVolume>(&self.stdout(&args).await?)?;

        for name in candidates {
            let marked_row = marked.iter().find(|row| row.name == name).cloned();
            if marked_row.is_none() && !is_generated_volume_name(&name) {
                continue;
            }
            let mut resource = marked_row
                .unwrap_or_else(|| DockerVolume {
                    name,
                    driver: String::new(),
                    mountpoint: String::new(),
                })
                .into_resource();
            resource.detail.push(("Anonymous", "yes".to_string()));
            volumes.push(resource);
        }
        Ok(volumes)
    }

    /// The names of the volumes (not bind mounts) the containers mount, in
    /// the order `container inspect` printed them. Possibly repeated.
    async fn mounted_volume_names(
        &self,
        containers: &[DockerContainer],
    ) -> Result<Vec<String>, ContainerError> {
        if containers.is_empty() {
            return Ok(Vec::new());
        }
        let mut args = vec!["container", "inspect", "--"];
        args.extend(containers.iter().map(|container| container.id.as_str()));
        let stdout = self.stdout(&args).await?;
        let inspected: Vec<Value> =
            serde_json::from_str(stdout.trim()).map_err(|error| ContainerError::Parse {
                detail: format!("could not read the containers' mounts: {error}"),
            })?;
        Ok(inspected
            .iter()
            .filter_map(|container| container.get("Mounts").and_then(Value::as_array))
            .flatten()
            .filter(|mount| mount.get("Type").and_then(Value::as_str) == Some("volume"))
            .filter_map(|mount| text(mount.get("Name")))
            .collect())
    }

    /// `compose down`, run for a confirmed plan.
    pub(crate) async fn compose_down(
        &self,
        target: &ComposeTarget,
        with_volumes: bool,
    ) -> Result<(), ContainerError> {
        target.require_config_files()?;
        let verb: &[&str] = if with_volumes {
            &["down", "--volumes"]
        } else {
            &["down"]
        };
        self.stdout_of(compose_argv(target, verb, &[])?).await?;
        Ok(())
    }

    pub(crate) fn pull_terminal_command(&self, reference: &str) -> Option<(String, Vec<String>)> {
        check_reference("image reference", reference).ok()?;
        Some((
            self.program().to_string(),
            owned(&["pull", "--", reference]),
        ))
    }

    pub(crate) async fn pull_image(&self, reference: &str) -> Result<(), ContainerError> {
        check_reference("image reference", reference)?;
        self.stdout(&["pull", "--", reference]).await?;
        Ok(())
    }

    pub(crate) async fn tag_image(&self, source: &str, target: &str) -> Result<(), ContainerError> {
        check_reference("source image", source)?;
        check_reference("target image", target)?;
        self.stdout(&["tag", "--", source, target]).await?;
        Ok(())
    }

    pub(crate) async fn image_history(&self, id: &str) -> Result<Vec<ImageLayer>, ContainerError> {
        check_reference("image", id)?;
        let format = if self.kind() == BackendKind::Podman {
            "json"
        } else {
            "{{json .}}"
        };
        let stdout = self
            .stdout(&[
                "image",
                "history",
                "--no-trunc",
                "--format",
                format,
                "--",
                id,
            ])
            .await
            .map_err(|error| missing_means_not_found(error, id))?;
        parse_history(&stdout)
    }

    pub(crate) fn run_terminal_command(
        &self,
        spec: &RunSpec,
    ) -> Result<(String, Vec<String>), ContainerError> {
        Ok((self.program().to_string(), run_argv(spec)?))
    }

    pub(crate) async fn create_volume_named(
        &self,
        name: &str,
        driver: Option<&str>,
        labels: &[(String, String)],
    ) -> Result<(), ContainerError> {
        check_name("volume name", name)?;
        let mut args = owned(&["volume", "create"]);
        if let Some(driver) = driver {
            check_name("volume driver", driver)?;
            args.extend(["--driver".to_string(), driver.to_string()]);
        }
        for (key, value) in labels {
            check_label_key(key)?;
            check_text("label value", value)?;
            args.extend(["--label".to_string(), format!("{key}={value}")]);
        }
        args.extend(["--".to_string(), name.to_string()]);
        self.stdout_of(args).await?;
        Ok(())
    }

    pub(crate) async fn create_network_named(
        &self,
        name: &str,
        driver: Option<&str>,
        subnet: Option<&str>,
    ) -> Result<(), ContainerError> {
        check_name("network name", name)?;
        let mut args = owned(&["network", "create"]);
        if let Some(driver) = driver {
            check_name("network driver", driver)?;
            args.extend(["--driver".to_string(), driver.to_string()]);
        }
        if let Some(subnet) = subnet {
            check_subnet(subnet)?;
            args.extend(["--subnet".to_string(), subnet.to_string()]);
        }
        args.extend(["--".to_string(), name.to_string()]);
        self.stdout_of(args).await?;
        Ok(())
    }

    pub(crate) async fn connect_to_network(
        &self,
        network: &str,
        container: &str,
        alias: Option<&str>,
    ) -> Result<(), ContainerError> {
        check_name("network", network)?;
        check_name("container", container)?;
        let mut args = owned(&["network", "connect"]);
        if let Some(alias) = alias {
            check_name("network alias", alias)?;
            args.extend(["--alias".to_string(), alias.to_string()]);
        }
        args.extend(["--".to_string(), network.to_string(), container.to_string()]);
        self.stdout_of(args).await?;
        Ok(())
    }

    pub(crate) async fn disconnect_from_network(
        &self,
        network: &str,
        container: &str,
    ) -> Result<(), ContainerError> {
        check_name("network", network)?;
        check_name("container", container)?;
        self.stdout(&["network", "disconnect", "--", network, container])
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_reads_docker_lines_and_podman_arrays() {
        let docker = concat!(
            r#"{"CreatedAt":"2026-11-12T22:26:55+07:00","CreatedBy":"ENTRYPOINT [\"/api\"]","CreatedSince":"4 days ago","ID":"sha256:c0","Size":"0B"}"#,
            "\n",
            r#"{"CreatedBy":"RUN apk add","CreatedSince":"5 days ago","ID":"<missing>","Size":"12.3MB"}"#,
            "\n"
        );
        assert_eq!(
            parse_history(docker).expect("docker lines"),
            vec![
                ImageLayer {
                    created_by: "ENTRYPOINT [\"/api\"]".into(),
                    size: "0B".into(),
                    created: "4 days ago".into(),
                },
                ImageLayer {
                    created_by: "RUN apk add".into(),
                    size: "12.3MB".into(),
                    created: "5 days ago".into(),
                },
            ]
        );
        let podman =
            r#"[{"id":"ab","created":"2 days ago","createdBy":"/bin/sh -c x","size":2048}]"#;
        assert_eq!(
            parse_history(podman).expect("podman array"),
            vec![ImageLayer {
                created_by: "/bin/sh -c x".into(),
                size: "2.0 KiB".into(),
                created: "2 days ago".into(),
            }]
        );
        assert_eq!(parse_history("").expect("nothing"), Vec::new());
        assert!(matches!(
            parse_history("[not json"),
            Err(ContainerError::Parse { .. })
        ));
    }
}
