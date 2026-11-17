use serde_json::Value;

use crate::backend::ContainerError;
use crate::docker_inspect;
use crate::inspect::{InspectRow, Inspection, MASK, looks_secret, single_object, timestamp};
use crate::kubernetes_inspect;
use crate::resource::ResourceKind;
use crate::secret_masking::mask_secrets_in_json;

const DOCKER_CONTAINER: &str = include_str!("fixtures/docker-container-inspect.json");
const DOCKER_IMAGE: &str = include_str!("fixtures/docker-image-inspect.json");
const DOCKER_VOLUME: &str = include_str!("fixtures/docker-volume-inspect.json");
const DOCKER_NETWORK: &str = include_str!("fixtures/docker-network-inspect.json");
const PODMAN_CONTAINER: &str = include_str!("fixtures/podman-container-inspect.json");
const PODMAN_IMAGE: &str = include_str!("fixtures/podman-image-inspect.json");
const PODMAN_VOLUME: &str = include_str!("fixtures/podman-volume-inspect.json");
const PODMAN_NETWORK: &str = include_str!("fixtures/podman-network-inspect.json");
const KUBECTL_POD: &str = include_str!("fixtures/kubectl-pod.json");

fn docker(kind: ResourceKind, fixture: &str) -> Inspection {
    let root = single_object(fixture, "x").expect("fixture parses");
    docker_inspect::inspection(kind, &root)
}

fn pod() -> Inspection {
    let root = single_object(KUBECTL_POD, "shop/web").expect("fixture parses");
    kubernetes_inspect::inspection(&root)
}

fn value<'a>(inspection: &'a Inspection, section: &str, label: &str) -> Option<&'a str> {
    inspection
        .sections
        .iter()
        .find(|candidate| candidate.title == section)?
        .rows
        .iter()
        .find(|row| row.label == label)
        .map(|row| row.value.as_str())
}

fn row<'a>(inspection: &'a Inspection, section: &str, label: &str) -> Option<&'a InspectRow> {
    inspection
        .sections
        .iter()
        .find(|candidate| candidate.title == section)?
        .rows
        .iter()
        .find(|row| row.label == label)
}

fn titles(inspection: &Inspection) -> Vec<&str> {
    inspection
        .sections
        .iter()
        .map(|section| section.title.as_str())
        .collect()
}

#[test]
fn secret_looking_keys_are_recognised_case_insensitively() {
    for key in [
        "DB_PASSWORD",
        "db_password",
        "API_TOKEN",
        "AWS_SECRET_ACCESS_KEY",
        "STRIPE_KEY",
        "PGPASS",
        "GOOGLE_CREDENTIALS",
        "apiKey",
    ] {
        assert!(looks_secret(key), "{key} should be masked");
    }
    for key in ["PATH", "POSTGRES_DB", "LOG_LEVEL", "HOME", "TZ"] {
        assert!(!looks_secret(key), "{key} should not be masked");
    }
}

#[test]
fn docker_container_sections_follow_the_documented_order() {
    let inspection = docker(ResourceKind::Container, DOCKER_CONTAINER);
    assert_eq!(
        titles(&inspection),
        [
            "State",
            "Config",
            "Environment",
            "Ports",
            "Mounts",
            "Networks",
            "Labels",
            "Restart policy",
            "Resources"
        ]
    );
}

#[test]
fn docker_container_state_and_config_rows() {
    let inspection = docker(ResourceKind::Container, DOCKER_CONTAINER);
    assert_eq!(value(&inspection, "State", "Status"), Some("running"));
    assert_eq!(value(&inspection, "State", "Health"), Some("healthy"));
    assert_eq!(value(&inspection, "State", "Exit code"), Some("0"));
    assert_eq!(value(&inspection, "State", "Restart count"), Some("0"));
    assert_eq!(
        value(&inspection, "State", "Started"),
        Some("2026-11-17 05:53:48 UTC")
    );
    // The zero time Docker prints for "never finished" is not a date.
    assert_eq!(value(&inspection, "State", "Finished"), None);

    assert_eq!(
        value(&inspection, "Config", "Name"),
        Some("shop-postgres-1")
    );
    assert!(value(&inspection, "Config", "Image").is_some());
    assert!(value(&inspection, "Config", "Entrypoint").is_some());
    assert_eq!(
        value(&inspection, "Restart policy", "Policy"),
        Some("unless-stopped")
    );
}

#[test]
fn docker_container_ports_mounts_and_networks() {
    let inspection = docker(ResourceKind::Container, DOCKER_CONTAINER);
    assert_eq!(
        value(&inspection, "Ports", "5432/tcp"),
        Some("127.0.0.1:5432")
    );
    let mount = value(&inspection, "Mounts", "/var/lib/postgresql/data").expect("data mount");
    assert!(mount.contains("volume"), "{mount}");
    assert!(mount.contains("shop_postgres_data"), "{mount}");
    assert!(mount.contains("rw"), "{mount}");
    let bind = inspection
        .sections
        .iter()
        .find(|s| s.title == "Mounts")
        .expect("mounts");
    assert!(
        bind.rows
            .iter()
            .any(|row| row.value.contains("bind") && row.value.contains("ro")),
        "a read-only bind mount must say so: {:?}",
        bind.rows
    );
    let network = inspection
        .sections
        .iter()
        .find(|section| section.title == "Networks")
        .expect("networks");
    assert!(!network.rows.is_empty());
    assert!(
        network.rows.iter().all(|row| row.value.contains("IP ")),
        "{:?}",
        network.rows
    );
}

#[test]
fn docker_container_resource_limits() {
    let inspection = docker(ResourceKind::Container, DOCKER_CONTAINER);
    assert_eq!(
        value(&inspection, "Resources", "Memory limit"),
        Some("1.0 GiB")
    );
    assert_eq!(
        value(&inspection, "Resources", "CPU limit"),
        Some("unlimited")
    );
}

#[test]
fn environment_masks_secret_values_and_keeps_the_real_one_for_reveal() {
    let inspection = docker(ResourceKind::Container, DOCKER_CONTAINER);
    let password = row(&inspection, "Environment", "POSTGRES_PASSWORD").expect("password row");
    assert_eq!(password.value, MASK);
    assert!(password.is_secret());
    assert_eq!(password.revealed(), "hunter2-not-real");

    let token = row(&inspection, "Environment", "API_TOKEN").expect("token row");
    assert_eq!(token.value, MASK);

    let user = row(&inspection, "Environment", "POSTGRES_USER").expect("user row");
    assert_eq!(user.value, "shop");
    assert!(!user.is_secret());
    assert!(inspection.has_secrets());
}

#[test]
fn raw_json_is_masked_by_default_and_untouched_when_revealed() {
    let inspection = docker(ResourceKind::Container, DOCKER_CONTAINER);
    assert!(!inspection.raw_json.contains("hunter2-not-real"));
    assert!(!inspection.raw_json.contains("tok_not_real_123"));
    assert!(inspection.raw_json.contains("POSTGRES_PASSWORD=********"));
    assert!(inspection.raw_json.contains("POSTGRES_USER=shop"));
    assert!(inspection.raw_json_revealed.contains("hunter2-not-real"));
    assert!(inspection.raw_json_revealed.contains("tok_not_real_123"));
    serde_json::from_str::<Value>(&inspection.raw_json).expect("masked json stays valid");
}

#[test]
fn a_secret_hiding_in_no_section_still_cannot_leak_through_the_display_values() {
    let inspection = docker(ResourceKind::Container, DOCKER_CONTAINER);
    for section in &inspection.sections {
        for row in &section.rows {
            assert!(
                !row.value.contains("hunter2-not-real"),
                "{} leaks",
                row.label
            );
            assert!(
                !row.value.contains("tok_not_real_123"),
                "{} leaks",
                row.label
            );
        }
    }
}

#[test]
fn podman_container_maps_its_own_shapes() {
    let inspection = docker(ResourceKind::Container, PODMAN_CONTAINER);
    // No leading slash on a Podman name, and Entrypoint is a string.
    assert_eq!(value(&inspection, "Config", "Name"), Some("db"));
    assert_eq!(
        value(&inspection, "Config", "Entrypoint"),
        Some("docker-entrypoint.sh")
    );
    assert_eq!(value(&inspection, "State", "Health"), Some("healthy"));
    assert_eq!(value(&inspection, "State", "Restart count"), Some("2"));
    assert_eq!(
        value(&inspection, "Restart policy", "Policy"),
        Some("always")
    );
    // An empty HostIp means every interface.
    assert_eq!(
        value(&inspection, "Ports", "5432/tcp"),
        Some("0.0.0.0:5432")
    );
    assert_eq!(
        value(&inspection, "Ports", "9187/tcp"),
        Some("not published")
    );
    assert_eq!(
        value(&inspection, "Resources", "Memory limit"),
        Some("512.0 MiB")
    );
    assert_eq!(value(&inspection, "Resources", "CPU limit"), Some("1.5"));
    assert_eq!(value(&inspection, "Resources", "PIDs limit"), Some("2048"));
    let network = value(&inspection, "Networks", "app_default").expect("network");
    assert!(network.contains("10.89.0.5"), "{network}");
    assert!(
        network.contains("db"),
        "aliases belong in the row: {network}"
    );
    assert_eq!(
        row(&inspection, "Environment", "POSTGRES_PASSWORD").map(|row| row.value.as_str()),
        Some(MASK)
    );
    assert!(!inspection.raw_json.contains("podman-not-real"));
}

#[test]
fn docker_image_rows() {
    let inspection = docker(ResourceKind::Image, DOCKER_IMAGE);
    assert_eq!(value(&inspection, "Image", "Architecture"), Some("arm64"));
    assert_eq!(value(&inspection, "Image", "OS"), Some("linux"));
    assert_eq!(value(&inspection, "Image", "Layers"), Some("15"));
    assert_eq!(value(&inspection, "Image", "Size"), Some("5.1 MiB"));
    assert_eq!(value(&inspection, "Image", "Entrypoint"), Some("/api"));
    assert_eq!(
        value(&inspection, "Image", "Exposed ports"),
        Some("3000/tcp")
    );
    assert_eq!(value(&inspection, "Image", "User"), Some("65532"));
    assert!(value(&inspection, "Image", "Tags").is_some_and(|tags| tags.contains("devt-kit-api")));
    assert_eq!(
        value(&inspection, "Image", "Created"),
        Some("2026-11-12 15:26:55 UTC")
    );
    assert!(titles(&inspection).contains(&"Labels"));
}

#[test]
fn podman_image_masks_secrets_and_reads_top_level_labels() {
    let inspection = docker(ResourceKind::Image, PODMAN_IMAGE);
    assert_eq!(value(&inspection, "Image", "Layers"), Some("3"));
    assert_eq!(
        value(&inspection, "Image", "Entrypoint"),
        Some("docker-entrypoint.sh")
    );
    assert_eq!(
        value(&inspection, "Labels", "maintainer"),
        Some("PostgreSQL Docker Community")
    );
    assert_eq!(
        row(&inspection, "Environment", "POSTGRES_HOST_AUTH_TOKEN").map(|row| row.value.as_str()),
        Some(MASK)
    );
    assert!(!inspection.raw_json.contains("img-not-real"));
}

#[test]
fn docker_and_podman_volumes() {
    let docker_volume = docker(ResourceKind::Volume, DOCKER_VOLUME);
    assert_eq!(value(&docker_volume, "Volume", "Driver"), Some("local"));
    assert!(
        value(&docker_volume, "Volume", "Mountpoint").is_some_and(|path| path.ends_with("/_data"))
    );
    assert_eq!(
        value(&docker_volume, "Volume", "Created"),
        Some("2026-10-25 05:27:01 UTC")
    );
    assert_eq!(value(&docker_volume, "Volume", "Scope"), Some("local"));
    assert_eq!(
        value(&docker_volume, "Labels", "com.docker.volume.anonymous"),
        Some("")
    );

    let podman_volume = docker(ResourceKind::Volume, PODMAN_VOLUME);
    assert_eq!(value(&podman_volume, "Volume", "Mount count"), Some("1"));
    assert_eq!(value(&podman_volume, "Options", "type"), Some("tmpfs"));
    assert_eq!(
        value(&podman_volume, "Volume", "Created"),
        Some("2026-10-20 09:00:00+01:00")
    );
}

#[test]
fn docker_network_lists_ipam_and_attached_containers() {
    let inspection = docker(ResourceKind::Network, DOCKER_NETWORK);
    assert_eq!(value(&inspection, "Network", "Driver"), Some("bridge"));
    assert_eq!(value(&inspection, "Network", "Scope"), Some("local"));
    assert_eq!(value(&inspection, "Network", "Internal"), Some("no"));
    assert_eq!(value(&inspection, "IPAM", "Subnet"), Some("172.18.0.0/16"));
    assert_eq!(value(&inspection, "IPAM", "Gateway"), Some("172.18.0.1"));
    let attached = inspection
        .sections
        .iter()
        .find(|section| section.title == "Containers")
        .expect("attached containers");
    assert_eq!(attached.rows.len(), 3);
    assert!(
        attached
            .rows
            .iter()
            .any(|row| row.label == "shop-fe-1" && row.value == "172.18.0.3/16")
    );
}

#[test]
fn podman_network_uses_snake_case_keys() {
    let inspection = docker(ResourceKind::Network, PODMAN_NETWORK);
    assert_eq!(value(&inspection, "Network", "Name"), Some("app_default"));
    assert_eq!(value(&inspection, "Network", "Driver"), Some("bridge"));
    assert_eq!(value(&inspection, "Network", "Internal"), Some("no"));
    assert_eq!(value(&inspection, "Network", "IPv6"), Some("no"));
    assert_eq!(value(&inspection, "Network", "Interface"), Some("podman1"));
    assert_eq!(value(&inspection, "IPAM", "Subnet"), Some("10.89.0.0/24"));
    assert_eq!(value(&inspection, "IPAM", "Gateway"), Some("10.89.0.1"));
    assert_eq!(
        value(&inspection, "Labels", "io.podman.compose.project"),
        Some("app")
    );
}

#[test]
fn an_object_missing_every_key_still_inspects_without_panicking() {
    for kind in [
        ResourceKind::Container,
        ResourceKind::Image,
        ResourceKind::Volume,
        ResourceKind::Network,
    ] {
        let inspection = docker(kind, "[{}]");
        assert!(inspection.raw_json.contains("{}"));
        let inspection = docker(
            kind,
            r#"[{"Config":null,"State":[],"Mounts":"x","NetworkSettings":5}]"#,
        );
        let _ = inspection.sections;
    }
}

#[test]
fn kubernetes_pod_sections_and_rows() {
    let inspection = pod();
    assert_eq!(
        titles(&inspection),
        [
            "Pod",
            "Containers",
            "Ports",
            "Environment (web)",
            "Resources",
            "Conditions",
            "Labels"
        ]
    );
    assert_eq!(value(&inspection, "Pod", "Phase"), Some("Running"));
    assert_eq!(value(&inspection, "Pod", "Namespace"), Some("shop"));
    assert_eq!(value(&inspection, "Pod", "Node"), Some("node-a"));
    assert_eq!(value(&inspection, "Pod", "Pod IP"), Some("10.42.0.17"));
    assert_eq!(value(&inspection, "Pod", "QoS class"), Some("Burstable"));
    assert_eq!(
        value(&inspection, "Pod", "Controller"),
        Some("ReplicaSet/web-7d4b9c6f8")
    );

    let web = value(&inspection, "Containers", "web").expect("web");
    assert!(web.contains("ghcr.io/example/web:1.4.2"), "{web}");
    assert!(web.contains("running"), "{web}");
    let sidecar = value(&inspection, "Containers", "sidecar").expect("sidecar");
    assert!(sidecar.contains("CrashLoopBackOff"), "{sidecar}");
    assert!(sidecar.contains("restarts 7"), "{sidecar}");
    assert!(
        sidecar.contains("OOMKilled"),
        "last state matters: {sidecar}"
    );

    assert_eq!(value(&inspection, "Ports", "web"), Some("8080/TCP"));
    assert_eq!(
        value(&inspection, "Conditions", "Ready").map(|text| text.starts_with("False")),
        Some(true)
    );
    assert_eq!(
        value(&inspection, "Resources", "web limits"),
        Some("memory 256Mi")
    );
    assert_eq!(
        value(&inspection, "Resources", "web requests"),
        Some("cpu 100m, memory 128Mi")
    );
}

#[test]
fn kubernetes_env_masks_literals_and_shows_references_plainly() {
    let inspection = pod();
    let literal = row(&inspection, "Environment (web)", "DB_PASSWORD").expect("literal");
    assert_eq!(literal.value, MASK);
    assert_eq!(literal.revealed(), "k8s-not-real");
    assert_eq!(
        value(&inspection, "Environment (web)", "LOG_LEVEL"),
        Some("info")
    );
    // A reference names where the value lives; it is not the value.
    let reference = row(&inspection, "Environment (web)", "API_KEY").expect("reference");
    assert!(!reference.is_secret());
    assert_eq!(reference.value, "from secret web-secrets/api-key");
    assert!(!inspection.raw_json.contains("k8s-not-real"));
    assert!(inspection.raw_json.contains("web-secrets"));
    assert!(inspection.raw_json_revealed.contains("k8s-not-real"));
}

#[test]
fn mask_secrets_in_json_handles_both_env_shapes_and_leaves_the_rest() {
    let mut document: Value = serde_json::from_str(
        r#"{"Config":{"Env":["A_TOKEN=x","PLAIN=y","NOEQUALS"]},
            "spec":{"containers":[{"env":[{"name":"MY_SECRET","value":"z"},{"name":"OK","value":"w"},{"name":"K","valueFrom":{}}]}]}}"#,
    )
    .expect("json");
    mask_secrets_in_json(&mut document);
    assert_eq!(document["Config"]["Env"][0], "A_TOKEN=********");
    assert_eq!(document["Config"]["Env"][1], "PLAIN=y");
    assert_eq!(document["Config"]["Env"][2], "NOEQUALS");
    assert_eq!(
        document["spec"]["containers"][0]["env"][0]["value"],
        "********"
    );
    assert_eq!(document["spec"]["containers"][0]["env"][1]["value"], "w");
}

#[test]
fn timestamps_are_tidied_and_the_zero_time_is_nothing() {
    assert_eq!(
        timestamp("2026-11-17T05:54:00.247167006Z").as_deref(),
        Some("2026-11-17 05:54:00 UTC")
    );
    assert_eq!(
        timestamp("2026-11-10T09:12:41.123+01:00").as_deref(),
        Some("2026-11-10 09:12:41+01:00")
    );
    assert_eq!(
        timestamp("2026-11-12 22:26:55 +0700 +07").as_deref(),
        Some("2026-11-12 22:26:55 +0700 +07")
    );
    assert_eq!(timestamp("0001-01-01T00:00:00Z"), None);
    assert_eq!(timestamp(""), None);
}

#[test]
fn single_object_takes_the_first_of_an_array_and_reports_empty_as_not_found() {
    assert!(single_object(r#"[{"a":1},{"a":2}]"#, "x").is_ok_and(|object| object["a"] == 1));
    assert!(single_object(r#"{"a":1}"#, "x").is_ok());
    assert_eq!(
        single_object("[]", "gone"),
        Err(ContainerError::NotFound { id: "gone".into() })
    );
    assert!(matches!(
        single_object("nope", "x"),
        Err(ContainerError::Parse { .. })
    ));
    assert!(matches!(
        single_object("5", "x"),
        Err(ContainerError::NotFound { .. })
    ));
}
