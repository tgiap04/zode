//! Docker and Podman `inspect` output as sections.
//!
//! The Docker shapes were captured from Docker 29.4.3 and trimmed. The Podman
//! shapes are written from Podman's documented output and are **not observed**:
//! Podman is not installed on the machine this was written on. Where they
//! differ the mapping accepts both -- `Entrypoint` as an array or a string,
//! `Name` with or without a leading slash, a network object in snake_case.

use serde_json::Value;

use crate::inspect::{
    InspectRow, Inspection, RowList, SectionList, dig, env_rows, field, finish, flag, format_bytes,
    label_rows, list_len, number, option_rows, sorted_entries, text, timestamp_of, trim_decimal,
    words, yes_no,
};
use crate::resource::ResourceKind;

pub(crate) fn inspection(kind: ResourceKind, root: &Value) -> Inspection {
    let mut sections = SectionList::default();
    match kind {
        ResourceKind::Container => container(root, &mut sections),
        ResourceKind::Image => image(root, &mut sections),
        ResourceKind::Volume => volume(root, &mut sections),
        ResourceKind::Network => network(root, &mut sections),
        ResourceKind::Pod | ResourceKind::ComposeProject => {}
    }
    finish(root, sections.0)
}

fn container(root: &Value, sections: &mut SectionList) {
    let state = root.get("State");
    let mut rows = RowList::default();
    rows.add("Status", state.and_then(|state| text(state.get("Status"))));
    rows.add(
        "Started",
        state.and_then(|state| timestamp_of(state.get("StartedAt"))),
    );
    rows.add(
        "Finished",
        state.and_then(|state| timestamp_of(state.get("FinishedAt"))),
    );
    rows.add(
        "Exit code",
        state.and_then(|state| text(state.get("ExitCode"))),
    );
    if state.and_then(|state| flag(state.get("OOMKilled"))) == Some(true) {
        rows.add("OOM killed", Some(yes_no(true)));
    }
    rows.add("Error", state.and_then(|state| text(state.get("Error"))));
    // Podman has printed this under both names across versions.
    rows.add(
        "Health",
        state
            .and_then(|state| field(state, &["Health", "Healthcheck"]))
            .and_then(|health| text(health.get("Status"))),
    );
    rows.add("Restart count", text(root.get("RestartCount")));
    sections.push("State", rows.0);

    let config = root.get("Config");
    let mut rows = RowList::default();
    rows.add(
        "Name",
        text(root.get("Name")).map(|name| name.trim_start_matches('/').to_string()),
    );
    rows.add("Image", config.and_then(|config| text(config.get("Image"))));
    rows.add("Image ID", text(root.get("Image")));
    rows.add_command(
        "Command",
        config.and_then(|config| words(config.get("Cmd"))),
    );
    rows.add_command(
        "Entrypoint",
        config.and_then(|config| words(config.get("Entrypoint"))),
    );
    rows.add(
        "Working dir",
        config.and_then(|config| text(config.get("WorkingDir"))),
    );
    rows.add("User", config.and_then(|config| text(config.get("User"))));
    rows.add(
        "Hostname",
        config.and_then(|config| text(config.get("Hostname"))),
    );
    rows.add("Created", timestamp_of(root.get("Created")));
    sections.push("Config", rows.0);

    sections.push(
        "Environment",
        env_rows(config.and_then(|config| config.get("Env"))),
    );
    sections.push("Ports", port_rows(root));
    sections.push("Mounts", mount_rows(root.get("Mounts")));
    sections.push(
        "Networks",
        network_rows(dig(root, &["NetworkSettings", "Networks"])),
    );
    sections.push(
        "Labels",
        label_rows(config.and_then(|config| config.get("Labels"))),
    );
    sections.push("Restart policy", restart_policy_rows(root));
    sections.push("Resources", resource_rows(root.get("HostConfig")));
}

/// Published ports. A running container's `NetworkSettings.Ports` is the truth;
/// a stopped one has none, so the configured bindings stand in.
fn port_rows(root: &Value) -> Vec<InspectRow> {
    let live = dig(root, &["NetworkSettings", "Ports"]);
    let ports = if live
        .and_then(Value::as_object)
        .is_some_and(|map| !map.is_empty())
    {
        live
    } else {
        dig(root, &["HostConfig", "PortBindings"])
    };
    sorted_entries(ports)
        .into_iter()
        .map(|(container_port, bindings)| {
            let published: Vec<String> = bindings
                .as_array()
                .into_iter()
                .flatten()
                .map(|binding| {
                    let address = match text(binding.get("HostIp")) {
                        Some(address) => address,
                        None => "0.0.0.0".to_string(),
                    };
                    let port = text(binding.get("HostPort")).unwrap_or_default();
                    format!("{address}:{port}")
                })
                .collect();
            let value = if published.is_empty() {
                "not published".to_string()
            } else {
                published.join(", ")
            };
            InspectRow::new(container_port.as_str(), value)
        })
        .collect()
}

fn mount_rows(mounts: Option<&Value>) -> Vec<InspectRow> {
    mounts
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|mount| {
            let kind = text(mount.get("Type")).unwrap_or_default();
            let source = if kind == "volume" {
                text(mount.get("Name")).or_else(|| text(mount.get("Source")))
            } else {
                text(mount.get("Source"))
            }
            .unwrap_or_default();
            let access = match flag(mount.get("RW")) {
                Some(true) => "rw".to_string(),
                Some(false) => "ro".to_string(),
                None => text(mount.get("Mode")).unwrap_or_default(),
            };
            let destination = text(mount.get("Destination")).unwrap_or_default();
            InspectRow::new(destination, format!("{kind} {source} ({access})"))
        })
        .collect()
}

fn network_rows(networks: Option<&Value>) -> Vec<InspectRow> {
    sorted_entries(networks)
        .into_iter()
        .map(|(name, endpoint)| {
            let mut parts = Vec::new();
            if let Some(address) = text(endpoint.get("IPAddress")) {
                parts.push(format!("IP {address}"));
            }
            if let Some(gateway) = text(endpoint.get("Gateway")) {
                parts.push(format!("gateway {gateway}"));
            }
            let aliases: Vec<&str> = endpoint
                .get("Aliases")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            if !aliases.is_empty() {
                parts.push(format!("aliases {}", aliases.join(", ")));
            }
            let value = if parts.is_empty() {
                "connected".to_string()
            } else {
                parts.join(" · ")
            };
            InspectRow::new(name.as_str(), value)
        })
        .collect()
}

fn restart_policy_rows(root: &Value) -> Vec<InspectRow> {
    let Some(policy) = dig(root, &["HostConfig", "RestartPolicy"]) else {
        return Vec::new();
    };
    let name = text(policy.get("Name")).unwrap_or_else(|| "no".to_string());
    let mut rows = vec![InspectRow::new("Policy", name)];
    if let Some(retries) = number(policy.get("MaximumRetryCount")).filter(|retries| *retries > 0) {
        rows.push(InspectRow::new("Maximum retries", retries.to_string()));
    }
    rows
}

fn resource_rows(host_config: Option<&Value>) -> Vec<InspectRow> {
    let Some(host_config) = host_config else {
        return Vec::new();
    };
    let memory = match number(host_config.get("Memory")).filter(|bytes| *bytes > 0) {
        Some(bytes) => format_bytes(bytes),
        None => "unlimited".to_string(),
    };
    let nano_cpus = number(host_config.get("NanoCpus")).filter(|cpus| *cpus > 0);
    let quota = number(host_config.get("CpuQuota")).filter(|quota| *quota > 0);
    let period = number(host_config.get("CpuPeriod")).filter(|period| *period > 0);
    let cpus = match (nano_cpus, quota, period) {
        (Some(nano), _, _) => trim_decimal(nano as f64 / 1e9),
        (None, Some(quota), Some(period)) => trim_decimal(quota as f64 / period as f64),
        _ => "unlimited".to_string(),
    };
    let mut rows = vec![
        InspectRow::new("Memory limit", memory),
        InspectRow::new("CPU limit", cpus),
    ];
    if let Some(limit) = number(host_config.get("PidsLimit")).filter(|limit| *limit > 0) {
        rows.push(InspectRow::new("PIDs limit", limit.to_string()));
    }
    rows
}

fn image(root: &Value, sections: &mut SectionList) {
    let config = root.get("Config");
    let mut rows = RowList::default();
    rows.add("ID", text(root.get("Id")));
    rows.add("Tags", joined_strings(root.get("RepoTags")));
    rows.add("Digests", joined_strings(root.get("RepoDigests")));
    rows.add("Created", timestamp_of(root.get("Created")));
    rows.add("Size", number(root.get("Size")).map(format_bytes));
    rows.add("Architecture", text(root.get("Architecture")));
    rows.add("OS", text(root.get("Os")));
    rows.add_command(
        "Entrypoint",
        config.and_then(|config| words(config.get("Entrypoint"))),
    );
    rows.add_command(
        "Command",
        config.and_then(|config| words(config.get("Cmd"))),
    );
    rows.add(
        "Working dir",
        config.and_then(|config| text(config.get("WorkingDir"))),
    );
    rows.add("User", config.and_then(|config| text(config.get("User"))));
    let exposed: Vec<&str> = sorted_entries(config.and_then(|config| config.get("ExposedPorts")))
        .into_iter()
        .map(|(port, _)| port.as_str())
        .collect();
    rows.add(
        "Exposed ports",
        (!exposed.is_empty()).then(|| exposed.join(", ")),
    );
    rows.add(
        "Layers",
        list_len(dig(root, &["RootFS", "Layers"])).map(|count| count.to_string()),
    );
    sections.push("Image", rows.0);
    sections.push(
        "Environment",
        env_rows(config.and_then(|config| config.get("Env"))),
    );
    // Docker keeps image labels under Config; Podman also prints them at the
    // top level.
    let labels = config
        .and_then(|config| config.get("Labels"))
        .filter(|labels| labels.as_object().is_some_and(|map| !map.is_empty()))
        .or_else(|| root.get("Labels"));
    sections.push("Labels", label_rows(labels));
}

fn joined_strings(value: Option<&Value>) -> Option<String> {
    let joined = value
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    (!joined.is_empty()).then_some(joined)
}

fn volume(root: &Value, sections: &mut SectionList) {
    let mut rows = RowList::default();
    rows.add("Name", text(root.get("Name")));
    rows.add("Driver", text(root.get("Driver")));
    rows.add("Mountpoint", text(root.get("Mountpoint")));
    rows.add("Scope", text(root.get("Scope")));
    rows.add("Created", timestamp_of(root.get("CreatedAt")));
    rows.add("Mount count", text(root.get("MountCount")));
    sections.push("Volume", rows.0);
    sections.push("Options", option_rows(root.get("Options")));
    sections.push("Labels", label_rows(root.get("Labels")));
}

/// Docker's network object is PascalCase and Podman's snake_case; each key is
/// looked up under both.
fn network(root: &Value, sections: &mut SectionList) {
    let mut rows = RowList::default();
    rows.add("Name", text(field(root, &["Name", "name"])));
    rows.add("ID", text(field(root, &["Id", "id"])));
    rows.add("Driver", text(field(root, &["Driver", "driver"])));
    rows.add("Scope", text(root.get("Scope")));
    rows.add("Interface", text(root.get("network_interface")));
    rows.add(
        "Internal",
        flag(field(root, &["Internal", "internal"])).map(yes_no),
    );
    rows.add("Attachable", flag(root.get("Attachable")).map(yes_no));
    rows.add(
        "IPv6",
        flag(field(root, &["EnableIPv6", "ipv6_enabled"])).map(yes_no),
    );
    rows.add("DNS", flag(root.get("dns_enabled")).map(yes_no));
    rows.add(
        "Created",
        timestamp_of(field(root, &["Created", "created"])),
    );
    sections.push("Network", rows.0);

    let configs = dig(root, &["IPAM", "Config"]).or_else(|| root.get("subnets"));
    let ipam: Vec<InspectRow> = configs
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|config| {
            let subnet = text(field(config, &["Subnet", "subnet"]))
                .map(|subnet| InspectRow::new("Subnet", subnet));
            let gateway = text(field(config, &["Gateway", "gateway"]))
                .map(|gateway| InspectRow::new("Gateway", gateway));
            subnet.into_iter().chain(gateway)
        })
        .collect();
    sections.push("IPAM", ipam);

    let attached: Vec<InspectRow> = sorted_entries(root.get("Containers"))
        .into_iter()
        .map(|(id, endpoint)| {
            let name = text(endpoint.get("Name")).unwrap_or_else(|| id.clone());
            let mut addresses = Vec::new();
            addresses.extend(text(endpoint.get("IPv4Address")));
            addresses.extend(text(endpoint.get("IPv6Address")));
            InspectRow::new(name, addresses.join(", "))
        })
        .collect();
    sections.push("Containers", attached);

    sections.push("Options", option_rows(field(root, &["Options", "options"])));
    sections.push("Labels", label_rows(field(root, &["Labels", "labels"])));
}
