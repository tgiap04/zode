//! What the Inspect tab must never show, wherever an engine puts it: not only
//! environment values by name, but credentials in a URL, in a command line, in
//! a volume's options and in a Kubernetes annotation that holds a whole manifest.

use serde_json::Value;

use crate::docker_inspect;
use crate::inspect::{
    InspectRow, Inspection, MASK, looks_secret, mask_secrets_in_text, single_object,
};
use crate::kubernetes_inspect;
use crate::resource::ResourceKind;

fn container(document: &str) -> Inspection {
    let root = single_object(document, "x").expect("test document parses");
    docker_inspect::inspection(ResourceKind::Container, &root)
}

fn volume(document: &str) -> Inspection {
    let root = single_object(document, "x").expect("test document parses");
    docker_inspect::inspection(ResourceKind::Volume, &root)
}

fn row<'a>(inspection: &'a Inspection, section: &str, label: &str) -> &'a InspectRow {
    inspection
        .sections
        .iter()
        .find(|candidate| candidate.title == section)
        .and_then(|section| section.rows.iter().find(|row| row.label == label))
        .unwrap_or_else(|| panic!("no {section}/{label} row"))
}

#[test]
fn more_names_that_hold_secrets_are_recognised() {
    for key in [
        "SENTRY_DSN",
        "AUTHORIZATION",
        "BASIC_AUTH",
        "TLS_PRIVATE_KEY_FILE",
        "JWT_SIGNING",
        "SESSION_COOKIE",
        "PASSWORD_SALT",
        "DB_CONNECTION_STRING",
        "connectionString",
    ] {
        assert!(looks_secret(key), "{key} should be masked");
    }
    for key in ["PATH", "POSTGRES_DB", "LOG_LEVEL", "API_URL", "NODE_ENV"] {
        assert!(!looks_secret(key), "{key} should not be masked");
    }
}

#[test]
fn a_url_with_credentials_is_a_secret_whatever_the_variable_is_called() {
    let inspection = container(
        r#"[{"Config":{"Env":[
            "DATABASE_URL=postgres://app:s3cret-pw@db:5432/app?sslmode=disable",
            "REDIS_URI=redis://:onlypass@cache:6379/0",
            "GITHUB_REMOTE=https://ghp_tokenvalue@github.com/org/repo.git",
            "API_URL=https://api.example.com/v1",
            "NOTE=see https://example.com/user@home",
            "PLAIN=1"
        ]}}]"#,
    );
    let database = row(&inspection, "Environment", "DATABASE_URL");
    assert!(database.is_secret());
    assert_eq!(
        database.value,
        "postgres://app:********@db:5432/app?sslmode=disable"
    );
    assert_eq!(
        database.revealed(),
        "postgres://app:s3cret-pw@db:5432/app?sslmode=disable"
    );
    assert_eq!(
        row(&inspection, "Environment", "REDIS_URI").value,
        "redis://:********@cache:6379/0"
    );
    assert_eq!(
        row(&inspection, "Environment", "GITHUB_REMOTE").value,
        "https://********@github.com/org/repo.git"
    );
    for plain in ["API_URL", "NOTE", "PLAIN"] {
        assert!(
            !row(&inspection, "Environment", plain).is_secret(),
            "{plain}"
        );
    }
    for leaked in ["s3cret-pw", "onlypass", "ghp_tokenvalue"] {
        assert!(
            !inspection.raw_json.contains(leaked),
            "{leaked} in raw json"
        );
        assert!(inspection.raw_json_revealed.contains(leaked));
    }
    assert!(inspection.raw_json.contains("example.com/user@home"));
    assert!(inspection.has_secrets());
}

#[test]
fn a_label_that_is_a_url_with_a_password_is_masked_like_any_other_value() {
    let inspection = container(
        r#"[{"Config":{"Labels":{
            "org.opencontainers.image.authors":"Team <team@example.com>",
            "build.source":"https://ci:label-secret-1@git.example.com/org/repo.git"}}}]"#,
    );
    let source = row(&inspection, "Labels", "build.source");
    assert!(source.is_secret());
    assert_eq!(
        source.value,
        "https://ci:********@git.example.com/org/repo.git"
    );
    assert!(!row(&inspection, "Labels", "org.opencontainers.image.authors").is_secret());
    assert!(!inspection.raw_json.contains("label-secret-1"));
}

#[test]
fn text_masking_covers_assignments_flags_and_urls() {
    for (input, expected) in [
        ("DB_PASSWORD=hunter2", "DB_PASSWORD=********"),
        ("--password=hunter2", "--password=********"),
        (
            "app --password hunter2 --user bob",
            "app --password ******** --user bob",
        ),
        (
            "docker run -e API_TOKEN=abc -e PLAIN=x img",
            "docker run -e API_TOKEN=******** -e PLAIN=x img",
        ),
        ("--env=SECRET_KEY=zzz", "--env=SECRET_KEY=********"),
        (
            "curl https://u:pw@host/path",
            "curl https://u:********@host/path",
        ),
        (
            r#"psql "postgres://u:p@h/db""#,
            r#"psql "postgres://u:********@h/db""#,
        ),
        (
            "RUN echo PASSWORD='a b' > f",
            "RUN echo PASSWORD=******** > f",
        ),
        (
            "/bin/sh -c #(nop)  ENV DB_PASSWORD=hunter2",
            "/bin/sh -c #(nop)  ENV DB_PASSWORD=********",
        ),
        ("  keep   the  spacing  ", "  keep   the  spacing  "),
        ("ls -la /tmp", "ls -la /tmp"),
        ("PORT=8080 -p 5432:5432", "PORT=8080 -p 5432:5432"),
        ("https://example.com/a@b", "https://example.com/a@b"),
        ("", ""),
    ] {
        assert_eq!(mask_secrets_in_text(input), expected, "{input:?}");
    }
}

#[test]
fn a_flag_or_value_that_is_missing_has_nothing_to_mask() {
    assert_eq!(mask_secrets_in_text("app --password"), "app --password");
    assert_eq!(mask_secrets_in_text("--token="), "--token=");
}

const LEAKY_CONTAINER: &str = r#"[{
    "Args": ["--api-key", "arg-secret-1"],
    "Config": {
        "Cmd": ["serve", "--db-password=cmd-secret-2", "--port", "80"],
        "Entrypoint": ["sh", "-c", "DB_PASSWORD=entry-secret-3 run"],
        "Healthcheck": {"Test": ["CMD-SHELL", "curl -u admin:pw https://svc:health-secret-4@localhost/up"]},
        "CreateCommand": ["podman", "run", "-e", "API_TOKEN=create-secret-5", "img"],
        "Env": ["PLAIN=1"]
    }
}]"#;

#[test]
fn commands_are_masked_in_the_sections_and_in_the_raw_json() {
    let inspection = container(LEAKY_CONTAINER);
    let command = row(&inspection, "Config", "Command");
    assert!(command.is_secret());
    assert_eq!(command.value, "serve --db-password=******** --port 80");
    assert_eq!(
        command.revealed(),
        "serve --db-password=cmd-secret-2 --port 80"
    );
    let entrypoint = row(&inspection, "Config", "Entrypoint");
    assert_eq!(entrypoint.value, "sh -c DB_PASSWORD=******** run");
    assert!(entrypoint.is_secret());

    for leaked in [
        "arg-secret-1",
        "cmd-secret-2",
        "entry-secret-3",
        "health-secret-4",
        "create-secret-5",
    ] {
        assert!(
            !inspection.raw_json.contains(leaked),
            "{leaked} in raw json"
        );
        assert!(
            inspection.raw_json_revealed.contains(leaked),
            "{leaked} must come back on reveal"
        );
        for section in &inspection.sections {
            for row in &section.rows {
                assert!(!row.value.contains(leaked), "{leaked} in {}", row.label);
            }
        }
    }
    let masked: Value = serde_json::from_str(&inspection.raw_json).expect("still valid json");
    assert_eq!(masked["Config"]["Cmd"][3], "80", "the rest is left alone");
    assert!(inspection.has_secrets());
}

#[test]
fn a_container_with_nothing_secret_has_nothing_to_reveal() {
    let inspection =
        container(r#"[{"Config":{"Cmd":["serve","--port","80"],"Env":["MODE=dev"]}}]"#);
    assert!(!inspection.has_secrets());
    assert_eq!(inspection.raw_json, inspection.raw_json_revealed);
}

#[test]
fn a_volume_s_options_are_masked_by_name_and_inside_the_mount_options() {
    let inspection = volume(
        r#"[{"Name":"share","Driver":"local","Options":{
            "type":"cifs","device":"//nas/share",
            "o":"username=bob,password=cifs-secret-1,addr=nas",
            "password":"plain-secret-2"}}]"#,
    );
    let mount = row(&inspection, "Options", "o");
    assert!(mount.is_secret());
    assert_eq!(mount.value, "username=bob,password=********,addr=nas");
    assert_eq!(
        mount.revealed(),
        "username=bob,password=cifs-secret-1,addr=nas"
    );
    assert_eq!(row(&inspection, "Options", "password").value, MASK);
    assert_eq!(row(&inspection, "Options", "type").value, "cifs");
    for leaked in ["cifs-secret-1", "plain-secret-2"] {
        assert!(
            !inspection.raw_json.contains(leaked),
            "{leaked} in raw json"
        );
        assert!(inspection.raw_json_revealed.contains(leaked));
    }
    assert!(inspection.has_secrets());
}

fn pod_with_annotation(annotation: &str) -> Inspection {
    let document = serde_json::json!({
        "metadata": {
            "name": "web",
            "namespace": "shop",
            "annotations": {
                "kubectl.kubernetes.io/last-applied-configuration": annotation,
                "team": "payments"
            }
        },
        "spec": {"containers": [{"name": "web", "image": "web:1"}]}
    });
    kubernetes_inspect::inspection(&document)
}

#[test]
fn the_last_applied_configuration_annotation_cannot_carry_env_values_out() {
    let manifest = r#"{"spec":{"template":{"spec":{"containers":[{"name":"web","env":[{"name":"DB_PASSWORD","value":"applied-secret-1"},{"name":"MODE","value":"dev"}]}]}}}}"#;
    let inspection = pod_with_annotation(manifest);
    assert!(!inspection.raw_json.contains("applied-secret-1"));
    assert!(inspection.raw_json_revealed.contains("applied-secret-1"));
    assert!(
        inspection.raw_json.contains("payments"),
        "other annotations stay"
    );
    assert!(
        inspection.raw_json.contains("MODE"),
        "the rest of the manifest stays"
    );
    assert!(inspection.has_secrets());
    serde_json::from_str::<Value>(&inspection.raw_json).expect("still valid json");

    let broken = pod_with_annotation(r#"{"env":[{"name":"X_TOKEN","value":"broken-secret-2"#);
    assert!(!broken.raw_json.contains("broken-secret-2"));
}

#[test]
fn the_debug_output_of_inspection_values_never_holds_a_secret() {
    let inspection = container(
        r#"[{"Config":{"Env":["API_TOKEN=debug-secret-1","DATABASE_URL=postgres://u:debug-secret-2@h/db"],"Cmd":["x","--password=debug-secret-3"]}}]"#,
    );
    let row_debug = format!("{:?}", row(&inspection, "Environment", "API_TOKEN"));
    let whole = format!("{inspection:?}");
    for leaked in ["debug-secret-1", "debug-secret-2", "debug-secret-3"] {
        assert!(!row_debug.contains(leaked), "row debug leaks {leaked}");
        assert!(!whole.contains(leaked), "inspection debug leaks {leaked}");
    }
    assert!(row_debug.contains("API_TOKEN"), "{row_debug}");
}
