//! Backends against an engine that answers each command differently, so a
//! plan that is built from several commands, or a call that retries, can be
//! checked end to end without an engine on the machine. Unix only: the engine
//! is `sh` printing what the test scripted.

#![cfg(unix)]

use std::sync::Arc;

use crate::backend::{BackendKind, ContainerBackend, ContainerError};
use crate::backend_test_suite::{RecordedCommand, RecordingHost, Reply};
use crate::destructive::{DestructivePlan, FORCE_CONFIRMATION};
use crate::docker_backend::{DockerBackend, parse_json_lines, parse_json_lines_strict};
use crate::docker_resources::DockerImage;
use crate::kubernetes_backend::KubernetesBackend;
use crate::operations::ComposeTarget;
use crate::resource::{Resource, ResourceKind, RunState, row_key};

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    smol::block_on(future)
}

fn commands(host: &RecordingHost) -> Vec<Vec<String>> {
    host.recorded()
        .into_iter()
        .map(|command| command.args)
        .collect()
}

fn asked(program: &str, args: &[&str]) -> RecordedCommand {
    RecordedCommand {
        program: program.to_string(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        env: Vec::new(),
        cwd: None,
    }
}

const PROJECT_FILTER: &str = "label=com.docker.compose.project=shop";
const ANONYMOUS_FILTER: &str = "label=com.docker.volume.anonymous";

fn hex(digit: char) -> String {
    digit.to_string().repeat(64)
}

fn shop() -> ComposeTarget {
    ComposeTarget::new("shop")
        .with_working_dir(Some("/srv/shop".into()))
        .with_config_text("/srv/shop/compose.yaml")
}

fn container_row(id: &str, name: &str) -> String {
    format!(
        r#"{{"ID":"{id}","Names":"{name}","Image":"img","State":"running","Labels":"com.docker.compose.project=shop,com.docker.compose.service=svc"}}"#
    )
}

fn volume_row(name: &str, labels: &str) -> String {
    format!(r#"{{"Name":"{name}","Driver":"local","Labels":"{labels}","Mountpoint":"/v/{name}"}}"#)
}

/// What compose would take with `down --volumes`: web mounts a named project
/// volume, an anonymous one and a bind; db mounts an external named volume and
/// an anonymous volume no label marks (an engine older than the label).
fn shop_engine(extra_container_rows: &str) -> RecordingHost {
    let containers = format!(
        "{}\n{}\n{extra_container_rows}",
        container_row("c1", "shop-web-1"),
        container_row("c2", "shop-db-1")
    );
    let mounts = format!(
        r#"[
        {{"Id":"c1","Mounts":[
            {{"Type":"volume","Name":"shop_data","Destination":"/data"}},
            {{"Type":"volume","Name":"{}","Destination":"/workspace/dist"}},
            {{"Type":"bind","Source":"/srv/shop","Destination":"/app"}}]}},
        {{"Id":"c2","Mounts":[
            {{"Type":"volume","Name":"shared-cache","Destination":"/cache"}},
            {{"Type":"volume","Name":"{}","Destination":"/var/lib/anon"}},
            {{"Type":"volume","Name":"shop_data","Destination":"/again"}}]}}]"#,
        hex('a'),
        hex('b')
    );
    let project_volumes = volume_row("shop_data", "com.docker.compose.project=shop");
    let anonymous = format!(
        "{}\n{}\n",
        volume_row(&hex('a'), "com.docker.volume.anonymous="),
        volume_row(&hex('c'), "com.docker.volume.anonymous=")
    );
    RecordingHost::scripted(
        "host",
        &[
            (&["ps", PROJECT_FILTER], Reply::Stdout(containers)),
            (
                &["network", "ls", PROJECT_FILTER],
                Reply::stdout(
                    r#"{"ID":"n1","Name":"shop_default","Driver":"bridge","Scope":"local"}"#,
                ),
            ),
            (
                &["volume", "ls", PROJECT_FILTER],
                Reply::Stdout(project_volumes),
            ),
            (
                &["volume", "ls", ANONYMOUS_FILTER],
                Reply::Stdout(anonymous),
            ),
            (&["container", "inspect"], Reply::Stdout(mounts)),
        ],
    )
}

fn names_of(plan: &DestructivePlan, kind: ResourceKind) -> Vec<String> {
    plan.targets()
        .iter()
        .filter(|target| target.kind == kind)
        .map(|target| target.name.clone())
        .collect()
}

mod compose_down_plan {
    use super::*;

    #[test]
    fn volumes_lists_the_anonymous_volumes_the_projects_containers_own() {
        let host = Arc::new(shop_engine(""));
        let backend = DockerBackend::docker(host);
        let plan = block_on(backend.compose_down_plan(&shop(), true))
            .expect("the plan is gathered")
            .expect("something goes");

        let mut volumes = names_of(&plan, ResourceKind::Volume);
        volumes.sort();
        let mut expected = vec!["shop_data".to_string(), hex('a'), hex('b')];
        expected.sort();
        assert_eq!(
            volumes, expected,
            "the project's volume once, its anonymous ones (labelled, and unlabelled by an \
             older engine); never the external `shared-cache` nor another project's anonymous volume"
        );
        assert_eq!(names_of(&plan, ResourceKind::Container).len(), 2);
        assert_eq!(names_of(&plan, ResourceKind::Network), ["shop_default"]);
    }

    #[test]
    fn without_volumes_no_volume_command_runs_and_none_is_listed() {
        let host = Arc::new(shop_engine(""));
        let backend = DockerBackend::docker(host.clone());
        let plan = block_on(backend.compose_down_plan(&shop(), false))
            .expect("the plan is gathered")
            .expect("something goes");
        assert!(names_of(&plan, ResourceKind::Volume).is_empty());
        assert!(
            commands(&host)
                .iter()
                .all(|args| !args.contains(&"volume".to_string())
                    && !args.contains(&"inspect".to_string())),
            "{:?}",
            commands(&host)
        );
    }

    #[test]
    fn an_unreadable_row_fails_the_plan_instead_of_shrinking_it() {
        let host = Arc::new(shop_engine("{not json"));
        let backend = DockerBackend::docker(host);
        assert!(matches!(
            block_on(backend.compose_down_plan(&shop(), true)),
            Err(ContainerError::Parse { .. })
        ));
    }

    #[test]
    fn a_project_without_config_files_is_refused_before_anything_is_asked() {
        let host = Arc::new(shop_engine(""));
        let backend = DockerBackend::docker(host.clone());
        for target in [
            ComposeTarget::new("shop"),
            ComposeTarget::new("shop").with_working_dir(Some("/srv/shop".into())),
        ] {
            assert!(matches!(
                block_on(backend.compose_down_plan(&target, true)),
                Err(ContainerError::Invalid { .. })
            ));
            assert!(
                backend
                    .compose_command(&target, crate::ComposeOp::Up, &[])
                    .is_none()
            );
        }
        assert_eq!(host.recorded(), vec![], "nothing was asked of the engine");
    }

    #[test]
    fn taking_down_a_plan_for_an_unknown_model_is_refused_too() {
        let host = Arc::new(RecordingHost::new("host"));
        let backend = DockerBackend::docker(host.clone());
        let plan = DestructivePlan::compose_down(
            ComposeTarget::new("shop"),
            vec![thing(ResourceKind::Container, "c1")],
            Vec::new(),
            Some(Vec::new()),
        )
        .expect("one container goes");
        assert!(matches!(
            block_on(backend.destroy(&plan)),
            Err(ContainerError::Invalid { .. })
        ));
        assert_eq!(host.recorded(), vec![]);
    }
}

fn thing(kind: ResourceKind, name: &str) -> Resource {
    Resource {
        kind,
        id: format!("id-{name}"),
        name: name.to_string(),
        state: RunState::Unknown,
        detail: Vec::new(),
        parent: None,
        compose: None,
    }
}

mod rows_that_do_not_parse {
    use super::*;

    const MIXED: &str = concat!(
        r#"{"ID":"c1","Names":"one","Image":"img","State":"running"}"#,
        "\n",
        "garbage that is not json\n",
        r#"{"ID":"c3","Names":"three","Image":"img","State":"exited"}"#,
        "\n"
    );

    #[test]
    fn the_lenient_reader_skips_the_bad_row_and_keeps_the_rest() {
        let rows = parse_json_lines::<crate::docker_types::DockerContainer>(MIXED);
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["c1", "c3"]);
    }

    #[test]
    fn the_strict_reader_names_the_bad_row() {
        let outcome = parse_json_lines_strict::<crate::docker_types::DockerContainer>(MIXED);
        let Err(ContainerError::Parse { detail }) = outcome else {
            panic!("a bad row must fail the read");
        };
        assert!(detail.contains("row 2"), "{detail}");
        assert!(
            parse_json_lines_strict::<crate::docker_types::DockerContainer>("")
                .expect("nothing is fine")
                .is_empty()
        );
    }

    #[test]
    fn listing_survives_a_bad_row() {
        let host = Arc::new(RecordingHost::scripted(
            "host",
            &[(&["ps"], Reply::stdout(MIXED))],
        ));
        let listed = block_on(DockerBackend::docker(host).list(ResourceKind::Container))
            .expect("a list is plain");
        assert_eq!(listed.len(), 2);
    }

    #[test]
    fn usages_refuse_to_say_nothing_depends_on_it_from_a_half_read_list() {
        let host = Arc::new(RecordingHost::scripted(
            "host",
            &[(&["ps"], Reply::stdout(MIXED))],
        ));
        assert!(matches!(
            block_on(DockerBackend::docker(host).usages(ResourceKind::Volume, "data")),
            Err(ContainerError::Parse { .. })
        ));
    }

    #[test]
    fn a_prune_plan_is_not_built_from_a_half_read_list() {
        let host = Arc::new(RecordingHost::scripted(
            "host",
            &[
                (&["images"], Reply::stdout("")),
                (&["ps"], Reply::stdout(MIXED)),
            ],
        ));
        let outcome =
            block_on(DockerBackend::docker(host).prune_targets(crate::PruneScope::Reclaimable));
        assert!(matches!(outcome, Some(Err(ContainerError::Parse { .. }))));
    }
}

mod compose_labels {
    use super::*;

    #[test]
    fn the_config_files_label_is_read_with_its_commas() {
        let row = r#"{"ID":"c1","Names":"web","Image":"img","State":"running","Labels":"com.docker.compose.project=shop,com.docker.compose.project.config_files=/srv/shop/a.yml,/srv/shop/b.yml,com.docker.compose.project.working_dir=/srv/shop,com.docker.compose.service=web"}"#;
        let host = Arc::new(RecordingHost::scripted(
            "host",
            &[(&["ps"], Reply::stdout(row))],
        ));
        let listed =
            block_on(DockerBackend::docker(host).list(ResourceKind::Container)).expect("listed");
        let labels = listed[0].compose.clone().expect("compose labels");
        assert_eq!(labels.config_files, ["/srv/shop/a.yml", "/srv/shop/b.yml"]);
        assert_eq!(labels.working_dir.as_deref(), Some("/srv/shop"));
        assert_eq!(
            ComposeTarget::from_labels(&labels).config_files,
            ["/srv/shop/a.yml", "/srv/shop/b.yml"]
        );
    }
}

mod stats {
    use super::*;

    /// Answered so the engine counts as running: a failed command is only
    /// a command's failure when the daemon is there.
    fn info() -> (&'static [&'static str], Reply) {
        (&["info"], Reply::stdout("29.0.0"))
    }

    fn podman_row(short_id: &str, name: &str) -> String {
        format!(
            r#"[{{"id":"{short_id}","name":"{name}","cpu_percent":"1.50%","mem_usage":"1MiB / 2MiB","net_io":"0B / 0B","block_io":"0B / 0B","pids":"3"}}]"#
        )
    }

    #[test]
    fn a_short_id_from_the_engine_is_matched_to_the_full_id_that_was_asked_for() {
        let full = format!("3f0e9c1a7b2d{}", "0".repeat(52));
        let host = Arc::new(RecordingHost::scripted(
            "host",
            &[(&["stats"], Reply::Stdout(podman_row("3f0e9c1a7b2d", "db")))],
        ));
        let stats = block_on(DockerBackend::podman(host).stats(std::slice::from_ref(&full)))
            .expect("stats")
            .expect("offered");
        assert_eq!(stats.len(), 1);
        assert_eq!(
            stats[0].id, full,
            "the view matches rows by the id its list gave"
        );
    }

    #[test]
    fn a_name_that_was_asked_for_is_matched_by_name() {
        let host = Arc::new(RecordingHost::scripted(
            "host",
            &[(&["stats"], Reply::Stdout(podman_row("3f0e9c1a7b2d", "web")))],
        ));
        let stats = block_on(DockerBackend::podman(host).stats(&["web".to_string()]))
            .expect("stats")
            .expect("offered");
        assert_eq!(stats[0].id, "web");
    }

    #[test]
    fn a_full_id_from_the_engine_is_matched_to_the_short_id_that_was_asked_for() {
        let full = format!("3f0e9c1a7b2d{}", "0".repeat(52));
        let host = Arc::new(RecordingHost::scripted(
            "host",
            &[(&["stats"], Reply::Stdout(podman_row(&full, "db")))],
        ));
        let stats = block_on(DockerBackend::podman(host).stats(&["3f0e9c1a7b2d".to_string()]))
            .expect("stats")
            .expect("offered");
        assert_eq!(stats[0].id, "3f0e9c1a7b2d");
    }

    #[test]
    fn one_container_that_is_gone_does_not_take_the_others_samples_with_it() {
        let host = Arc::new(RecordingHost::scripted(
            "host",
            &[
                info(),
                (
                    &["stats", "gone"],
                    Reply::failure("Error response from daemon: No such container: gone"),
                ),
                (
                    &["stats"],
                    Reply::stdout(
                        r#"{"ID":"abc","Name":"web","CPUPerc":"2.00%","MemUsage":"1MiB / 2MiB","NetIO":"0B / 0B","BlockIO":"0B / 0B","PIDs":"4"}"#,
                    ),
                ),
            ],
        ));
        let stats = block_on(
            DockerBackend::docker(host.clone()).stats(&["abc".to_string(), "gone".to_string()]),
        )
        .expect("the survivors are sampled")
        .expect("offered");
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].id, "abc");

        let stats_calls: Vec<Vec<String>> = commands(&host)
            .into_iter()
            .filter(|args| args.first().map(String::as_str) == Some("stats"))
            .collect();
        assert_eq!(stats_calls.len(), 2, "{stats_calls:?}");
        assert!(!stats_calls[1].contains(&"gone".to_string()));
        assert!(stats_calls[1].contains(&"abc".to_string()));
    }

    #[test]
    fn several_stale_ids_are_dropped_one_retry_at_a_time_and_all_gone_is_empty() {
        let host = Arc::new(RecordingHost::scripted(
            "host",
            &[
                info(),
                (
                    &["stats", "gone1"],
                    Reply::failure("Error response from daemon: No such container: gone1"),
                ),
                (
                    &["stats", "gone2"],
                    Reply::failure("Error response from daemon: No such container: gone2"),
                ),
            ],
        ));
        let stats = block_on(
            DockerBackend::docker(host).stats(&["gone1".to_string(), "gone2".to_string()]),
        )
        .expect("nothing left to sample is not a failure")
        .expect("offered");
        assert!(stats.is_empty());
    }

    #[test]
    fn a_stale_id_is_told_apart_from_a_longer_name_it_is_part_of() {
        let host = Arc::new(RecordingHost::scripted(
            "host",
            &[
                info(),
                (
                    &["stats", "web2"],
                    Reply::failure("Error response from daemon: No such container: web2"),
                ),
                (
                    &["stats"],
                    Reply::stdout(
                        r#"{"ID":"w1","Name":"web","CPUPerc":"1.00%","MemUsage":"1MiB / 2MiB","NetIO":"0B / 0B","BlockIO":"0B / 0B","PIDs":"1"}"#,
                    ),
                ),
            ],
        ));
        let stats =
            block_on(DockerBackend::docker(host).stats(&["web".to_string(), "web2".to_string()]))
                .expect("web is still sampled")
                .expect("offered");
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].id, "web");
    }

    #[test]
    fn any_other_failure_is_still_a_failure() {
        let host = Arc::new(RecordingHost::scripted(
            "host",
            &[info(), (&["stats"], Reply::failure("permission denied"))],
        ));
        assert!(matches!(
            block_on(DockerBackend::docker(host).stats(&["abc".to_string()])),
            Err(ContainerError::CommandFailed { .. })
        ));
    }
}

mod kubernetes_ids {
    use super::*;

    #[test]
    fn an_id_or_namespace_that_kubectl_would_read_as_a_flag_is_refused_unasked() {
        let host = Arc::new(RecordingHost::new("host"));
        let backend = KubernetesBackend::new(host.clone());
        for id in ["-A", "shop/-A", "-n/web", "--all-namespaces/x"] {
            assert!(
                matches!(
                    block_on(backend.inspect(ResourceKind::Pod, id)),
                    Err(ContainerError::Invalid { .. })
                ),
                "inspect {id}"
            );
            assert!(
                matches!(
                    block_on(backend.stats(&[id.to_string()])),
                    Err(ContainerError::Invalid { .. })
                ),
                "stats {id}"
            );
        }
        assert_eq!(host.recorded(), vec![], "nothing was asked of kubectl");
    }

    #[test]
    fn an_ordinary_id_still_goes_through() {
        let host = Arc::new(RecordingHost::new("host"));
        let backend = KubernetesBackend::new(host.clone());
        let _ = block_on(backend.inspect(ResourceKind::Pod, "shop/web-1"));
        assert_eq!(
            host.recorded(),
            vec![asked(
                "kubectl",
                &["get", "pod", "web-1", "-n", "shop", "-o", "json"]
            )]
        );
    }
}

mod image_identity {
    use super::*;

    fn images() -> Vec<Resource> {
        let rows = concat!(
            r#"{"ID":"abc123","Repository":"nginx","Tag":"1.25","Size":"1MB"}"#,
            "\n",
            r#"{"ID":"abc123","Repository":"nginx","Tag":"latest","Size":"1MB"}"#,
            "\n",
            r#"{"ID":"def456","Repository":"<none>","Tag":"<none>","Size":"1MB"}"#,
            "\n",
            r#"{"ID":"fed789","Repository":"localhost:5000/app","Tag":"<none>","Size":"1MB"}"#,
            "\n"
        );
        parse_json_lines::<DockerImage>(rows)
            .into_iter()
            .map(DockerImage::into_resource)
            .collect()
    }

    #[test]
    fn removing_a_tagged_row_untags_that_tag_and_a_dangling_row_goes_by_id() {
        let rows = images();
        let host = Arc::new(RecordingHost::new("host"));
        let plan = DestructivePlan::remove(ResourceKind::Image, rows).expect("four rows");
        let _ = block_on(DockerBackend::docker(host.clone()).destroy(&plan));
        assert_eq!(
            host.recorded().first(),
            Some(&asked("docker", &["rmi", "--", "nginx:1.25"])),
            "removal stops at the first failure, which is the first command"
        );

        for (row, handle) in [(1, "nginx:latest"), (2, "def456"), (3, "fed789")] {
            let host = Arc::new(RecordingHost::new("host"));
            let plan = DestructivePlan::remove(ResourceKind::Image, vec![images()[row].clone()])
                .expect("one row");
            let _ = block_on(DockerBackend::docker(host.clone()).destroy(&plan));
            assert_eq!(
                host.recorded(),
                vec![asked("docker", &["rmi", "--", handle])]
            );
        }
    }

    #[test]
    fn dependents_are_still_looked_up_by_the_image_id_for_every_row() {
        let rows = images();
        let host = Arc::new(RecordingHost::scripted(
            "host",
            &[(
                &["ps", "ancestor=abc123"],
                Reply::stdout(r#"{"ID":"c1","Names":"web","Image":"nginx:1.25","State":"exited"}"#),
            )],
        ));
        let plan = block_on(
            DockerBackend::docker(host).removal_plan(ResourceKind::Image, rows[..2].to_vec()),
        )
        .expect("planned")
        .expect("two rows");
        assert!(plan.has_dependents());
        assert_eq!(plan.dependents().len(), 2);
        assert!(
            plan.dependents()
                .iter()
                .all(|entry| entry.target_id == "abc123")
        );
    }

    #[test]
    fn rows_that_share_an_image_id_still_have_their_own_key() {
        let rows = images();
        assert_ne!(row_key(&rows[0]), row_key(&rows[1]));
        assert_eq!(row_key(&rows[0]), row_key(&rows[0].clone()));
        let volume = thing(ResourceKind::Volume, "data");
        assert_eq!(row_key(&volume), volume.id);
        let mut container = thing(ResourceKind::Container, "web");
        container.id = "abc123".into();
        assert_eq!(row_key(&container), "abc123");
        assert!(row_key(&rows[0]).starts_with("abc123\u{0}"));
    }
}

mod forced_removal {
    use super::*;

    fn plan(kind: ResourceKind) -> DestructivePlan {
        DestructivePlan::remove(kind, vec![thing(kind, "one")]).expect("one target")
    }

    #[test]
    fn force_only_does_something_where_the_engine_says_it_does() {
        let host = Arc::new(RecordingHost::new("host"));
        let docker = DockerBackend::docker(host.clone());
        let podman = DockerBackend::podman(host.clone());
        let kubernetes = KubernetesBackend::new(host);

        for kind in [ResourceKind::Container, ResourceKind::Image] {
            assert!(docker.force_effect(kind).is_some(), "docker {kind:?}");
        }
        for kind in [
            ResourceKind::Volume,
            ResourceKind::Network,
            ResourceKind::Pod,
            ResourceKind::ComposeProject,
        ] {
            assert_eq!(docker.force_effect(kind), None, "docker {kind:?}");
        }
        for kind in [
            ResourceKind::Container,
            ResourceKind::Image,
            ResourceKind::Volume,
            ResourceKind::Network,
            ResourceKind::Pod,
        ] {
            assert!(podman.force_effect(kind).is_some(), "podman {kind:?}");
        }
        assert_eq!(podman.force_effect(ResourceKind::ComposeProject), None);
        for kind in [ResourceKind::Pod, ResourceKind::Container] {
            assert_eq!(kubernetes.force_effect(kind), None, "kubernetes {kind:?}");
        }
    }

    #[test]
    fn the_warning_is_the_engines_own_account() {
        let host = Arc::new(RecordingHost::new("host"));
        let docker = DockerBackend::docker(host.clone());
        let podman = DockerBackend::podman(host);
        let docker_image = docker.force_effect(ResourceKind::Image).expect("docker");
        let podman_image = podman.force_effect(ResourceKind::Image).expect("podman");
        assert_ne!(docker_image, podman_image);
        assert!(
            podman_image.to_lowercase().contains("containers"),
            "podman removes the containers that use the image: {podman_image}"
        );
        assert!(
            docker_image.to_lowercase().contains("running"),
            "docker still refuses an image a running container uses: {docker_image}"
        );

        let forced = plan(ResourceKind::Image)
            .forced_with_effect(FORCE_CONFIRMATION, &podman_image)
            .expect("typed the word");
        assert!(forced.is_forced());
        assert_eq!(forced.warning(), Some(podman_image.as_str()));
        assert!(
            plan(ResourceKind::Image)
                .forced_with_effect("nope", &podman_image)
                .is_none()
        );
    }

    #[test]
    fn a_forced_plan_for_something_force_cannot_remove_is_refused_unrun() {
        for backend in [BackendKind::Docker, BackendKind::Podman] {
            let host = Arc::new(RecordingHost::new("host"));
            let engine = match backend {
                BackendKind::Podman => DockerBackend::podman(host.clone()),
                _ => DockerBackend::docker(host.clone()),
            };
            let forced = plan(ResourceKind::Volume)
                .forced(FORCE_CONFIRMATION)
                .expect("typed the word");
            let outcome = block_on(engine.destroy(&forced));
            if engine.force_effect(ResourceKind::Volume).is_none() {
                assert!(matches!(outcome, Err(ContainerError::NotOffered { .. })));
                assert_eq!(host.recorded(), vec![], "nothing may run");
            } else {
                assert_eq!(
                    host.recorded(),
                    vec![asked(
                        "podman",
                        &["volume", "rm", "--force", "--", "id-one"]
                    )]
                );
            }
        }
    }
}
