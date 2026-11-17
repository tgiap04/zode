//! `kubectl get pod -o json` as sections.
//!
//! Written from the documented `v1.Pod` schema, like `kubernetes_types.rs`:
//! there is no cluster on the machine this was built on.

use serde_json::Value;

use crate::inspect::{
    InspectRow, Inspection, RowList, SectionList, dig, finish, label_rows, number, sorted_entries,
    text, timestamp_of,
};

pub(crate) fn inspection(root: &Value) -> Inspection {
    let mut sections = SectionList::default();
    let containers: Vec<&Value> = dig(root, &["spec", "containers"])
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();

    let mut rows = RowList::default();
    rows.add("Name", text(dig(root, &["metadata", "name"])));
    rows.add("Namespace", text(dig(root, &["metadata", "namespace"])));
    rows.add("Phase", text(dig(root, &["status", "phase"])));
    rows.add("Reason", text(dig(root, &["status", "reason"])));
    rows.add("Node", text(dig(root, &["spec", "nodeName"])));
    rows.add("Pod IP", text(dig(root, &["status", "podIP"])));
    rows.add("Host IP", text(dig(root, &["status", "hostIP"])));
    rows.add("Started", timestamp_of(dig(root, &["status", "startTime"])));
    rows.add(
        "Created",
        timestamp_of(dig(root, &["metadata", "creationTimestamp"])),
    );
    rows.add("QoS class", text(dig(root, &["status", "qosClass"])));
    rows.add(
        "Service account",
        text(dig(root, &["spec", "serviceAccountName"])),
    );
    rows.add(
        "Restart policy",
        text(dig(root, &["spec", "restartPolicy"])),
    );
    rows.add("Controller", controller(root));
    sections.push("Pod", rows.0);

    sections.push(
        "Containers",
        containers
            .iter()
            .map(|container| container_row(root, container))
            .collect(),
    );
    sections.push(
        "Ports",
        containers
            .iter()
            .filter_map(|container| port_row(container))
            .collect(),
    );
    for container in &containers {
        let name = text(container.get("name")).unwrap_or_default();
        sections.push(
            format!("Environment ({name})"),
            env_rows(container.get("env")),
        );
    }
    sections.push(
        "Resources",
        containers
            .iter()
            .flat_map(|container| resource_rows(container))
            .collect(),
    );
    sections.push("Conditions", condition_rows(root));
    sections.push("Labels", label_rows(dig(root, &["metadata", "labels"])));
    finish(root, sections.0)
}

fn controller(root: &Value) -> Option<String> {
    dig(root, &["metadata", "ownerReferences"])
        .and_then(Value::as_array)?
        .iter()
        .find(|owner| owner.get("controller").and_then(Value::as_bool) == Some(true))
        .or_else(|| {
            dig(root, &["metadata", "ownerReferences"])
                .and_then(Value::as_array)
                .and_then(|owners| owners.first())
        })
        .map(|owner| {
            format!(
                "{}/{}",
                text(owner.get("kind")).unwrap_or_default(),
                text(owner.get("name")).unwrap_or_default()
            )
        })
}

fn container_row(root: &Value, container: &Value) -> InspectRow {
    let name = text(container.get("name")).unwrap_or_default();
    let status = dig(root, &["status", "containerStatuses"])
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|status| text(status.get("name")).as_deref() == Some(name.as_str()));

    let mut parts = vec![text(container.get("image")).unwrap_or_default()];
    match status {
        Some(status) => {
            parts.push(state_text(status.get("state")));
            parts.push(format!(
                "restarts {}",
                number(status.get("restartCount")).unwrap_or(0)
            ));
            if status.get("ready").and_then(Value::as_bool) == Some(true) {
                parts.push("ready".to_string());
            }
            if let Some(last) = status
                .get("lastState")
                .filter(|last| last.as_object().is_some_and(|map| !map.is_empty()))
            {
                parts.push(format!("last: {}", state_text(Some(last))));
            }
        }
        None => parts.push("no status yet".to_string()),
    }
    InspectRow::new(name, parts.join(" · "))
}

/// A container state object: exactly one of `running`, `waiting`,
/// `terminated`.
fn state_text(state: Option<&Value>) -> String {
    let Some(state) = state else {
        return "unknown".to_string();
    };
    if state.get("running").is_some() {
        return "running".to_string();
    }
    if let Some(waiting) = state.get("waiting") {
        return match text(waiting.get("reason")) {
            Some(reason) => format!("waiting ({reason})"),
            None => "waiting".to_string(),
        };
    }
    if let Some(terminated) = state.get("terminated") {
        let mut details = Vec::new();
        details.extend(text(terminated.get("reason")));
        details.extend(text(terminated.get("exitCode")).map(|code| format!("exit {code}")));
        return if details.is_empty() {
            "terminated".to_string()
        } else {
            format!("terminated ({})", details.join(", "))
        };
    }
    "unknown".to_string()
}

fn port_row(container: &Value) -> Option<InspectRow> {
    let ports: Vec<String> = container
        .get("ports")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|port| {
            let number = text(port.get("containerPort"))?;
            let protocol = text(port.get("protocol")).unwrap_or_else(|| "TCP".to_string());
            Some(format!("{number}/{protocol}"))
        })
        .collect();
    (!ports.is_empty()).then(|| {
        InspectRow::new(
            text(container.get("name")).unwrap_or_default(),
            ports.join(", "),
        )
    })
}

/// Literal values are masked by key; a reference (`valueFrom`) shows where the
/// value lives, which is not itself a secret.
fn env_rows(env: Option<&Value>) -> Vec<InspectRow> {
    env.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let name = text(entry.get("name"))?;
            if let Some(reference) = entry.get("valueFrom") {
                return Some(InspectRow::new(name, reference_text(reference)));
            }
            let value = entry
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Some(crate::inspect::pair_row(&name, value))
        })
        .collect()
}

fn reference_text(reference: &Value) -> String {
    let keyed = |key: &str, noun: &str| {
        let source = reference.get(key)?;
        Some(format!(
            "from {noun} {}/{}",
            text(source.get("name")).unwrap_or_default(),
            text(source.get("key")).unwrap_or_default()
        ))
    };
    keyed("secretKeyRef", "secret")
        .or_else(|| keyed("configMapKeyRef", "config map"))
        .or_else(|| {
            let path = text(dig(reference, &["fieldRef", "fieldPath"]))?;
            Some(format!("from field {path}"))
        })
        .or_else(|| {
            let resource = text(dig(reference, &["resourceFieldRef", "resource"]))?;
            Some(format!("from resource {resource}"))
        })
        .unwrap_or_else(|| "from a reference".to_string())
}

fn resource_rows(container: &Value) -> Vec<InspectRow> {
    let name = text(container.get("name")).unwrap_or_default();
    ["requests", "limits"]
        .into_iter()
        .filter_map(|which| {
            let quantities: Vec<String> = sorted_entries(dig(container, &["resources", which]))
                .into_iter()
                .filter_map(|(resource, quantity)| {
                    Some(format!("{resource} {}", text(Some(quantity))?))
                })
                .collect();
            (!quantities.is_empty())
                .then(|| InspectRow::new(format!("{name} {which}"), quantities.join(", ")))
        })
        .collect()
}

fn condition_rows(root: &Value) -> Vec<InspectRow> {
    dig(root, &["status", "conditions"])
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|condition| {
            let kind = text(condition.get("type"))?;
            let status = text(condition.get("status")).unwrap_or_default();
            let why: Vec<String> = [condition.get("reason"), condition.get("message")]
                .into_iter()
                .filter_map(text)
                .collect();
            let value = if why.is_empty() {
                status
            } else {
                format!("{status} ({})", why.join(": "))
            };
            Some(InspectRow::new(kind, value))
        })
        .collect()
}
