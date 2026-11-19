//! The write side of the trait: which commands are built for each operation,
//! what is refused before any command exists, and what a plan has to carry.
//!
//! Commands are asserted through [`RecordingHost`], which writes down every
//! command it is asked for and hands back a program that does not exist -- so
//! the exact argument list is proven without an engine on the machine.

use std::sync::Arc;

use crate::backend::{BackendKind, ContainerBackend, ContainerError};
use crate::backend_test_suite::{RecordedCommand, RecordingHost};
use crate::destructive::{Dependents, DestructivePlan, FORCE_CONFIRMATION, Intent};
use crate::docker_backend::DockerBackend;
use crate::fake_backend::FakeBackend;
use crate::kubernetes_backend::{KubernetesBackend, Scope};
use crate::operations::{
    ComposeOp, ComposeTarget, Op, PortMapping, PortProtocol, RunSpec, VolumeMount,
};
use crate::resource::{ComposeLabels, Resource, ResourceKind, RunState};

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    smol::block_on(future)
}

fn asked(program: &str, args: &[&str]) -> RecordedCommand {
    RecordedCommand {
        program: program.to_string(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        env: Vec::new(),
        cwd: None,
    }
}

fn words(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| arg.to_string()).collect()
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

fn shop() -> ComposeTarget {
    ComposeTarget::new("shop")
        .with_working_dir(Some("/srv/shop".into()))
        .with_config_text("/srv/shop/a.yml, /srv/shop/b.yml")
}

fn docker_on(host: &Arc<RecordingHost>) -> DockerBackend {
    DockerBackend::docker(host.clone())
}

mod compose_commands {
    use super::*;

    #[test]
    fn every_verb_carries_the_project_directory_and_each_config_file() {
        let backend = DockerBackend::docker(crate::tests::local());
        let prefix = [
            "compose",
            "-p",
            "shop",
            "--project-directory",
            "/srv/shop",
            "-f",
            "/srv/shop/a.yml",
            "-f",
            "/srv/shop/b.yml",
        ];
        for (op, tail) in [
            (ComposeOp::Up, &["up", "-d"][..]),
            (ComposeOp::Restart, &["restart"]),
            (ComposeOp::Stop, &["stop"]),
            (ComposeOp::Start, &["start"]),
            (ComposeOp::Pull, &["pull"]),
            (ComposeOp::Build, &["build"]),
            (ComposeOp::Logs, &["logs", "-f"]),
        ] {
            let (program, args) = backend
                .compose_command(&shop(), op, &[])
                .expect("compose is offered");
            let expected: Vec<&str> = prefix.iter().chain(tail).copied().collect();
            assert_eq!(program, "docker");
            assert_eq!(args, words(&expected), "{op:?}");
        }
    }

    #[test]
    fn services_follow_the_verb_each_as_its_own_word() {
        let backend = DockerBackend::podman(crate::tests::local());
        let (program, args) = backend
            .compose_command(
                &ComposeTarget::new("shop").with_config_text("/srv/shop/compose.yaml"),
                ComposeOp::Restart,
                &["web".into(), "db-1".into()],
            )
            .expect("compose is offered");
        assert_eq!(program, "podman");
        assert_eq!(
            args,
            words(&[
                "compose",
                "-p",
                "shop",
                "-f",
                "/srv/shop/compose.yaml",
                "restart",
                "web",
                "db-1"
            ])
        );
    }

    #[test]
    fn a_value_that_would_be_read_as_an_option_offers_nothing() {
        let backend = DockerBackend::docker(crate::tests::local());
        let build = |target: ComposeTarget, services: &[&str]| {
            let services: Vec<String> = services.iter().map(|s| s.to_string()).collect();
            backend.compose_command(&target, ComposeOp::Up, &services)
        };
        assert!(build(ComposeTarget::new("-x"), &[]).is_none());
        assert!(build(ComposeTarget::new("a b"), &[]).is_none());
        assert!(build(ComposeTarget::new("shop; rm -rf /"), &[]).is_none());
        assert!(build(ComposeTarget::new(""), &[]).is_none());
        assert!(build(ComposeTarget::new("shop"), &["--rm"]).is_none());
        assert!(build(ComposeTarget::new("shop"), &["web;id"]).is_none());
        assert!(
            build(
                ComposeTarget::new("shop").with_working_dir(Some("-x".into())),
                &[]
            )
            .is_none()
        );
        assert!(build(ComposeTarget::new("shop").with_config_text("-f"), &[]).is_none());
        assert!(
            build(
                ComposeTarget::new("shop").with_config_text("/srv/shop/compose.yaml"),
                &["web"]
            )
            .is_some(),
            "and a good one still builds"
        );
    }

    #[test]
    fn a_compose_target_reads_the_labels_and_the_project_row() {
        let labels = ComposeLabels {
            project: "shop".into(),
            service: "web".into(),
            working_dir: Some("/srv/shop".into()),
            config_files: vec!["/srv/shop/a.yml".into(), "/srv/shop/b.yml".into()],
        };
        let target = ComposeTarget::from_labels(&labels);
        assert_eq!(target.project, "shop");
        assert_eq!(target.working_dir.as_deref(), Some("/srv/shop"));
        assert_eq!(target.config_files, ["/srv/shop/a.yml", "/srv/shop/b.yml"]);

        let mut row = thing(ResourceKind::ComposeProject, "shop");
        row.id = "shop".into();
        row.detail = vec![("Config", "/srv/shop/a.yml,/srv/shop/b.yml".into())];
        let from_row = ComposeTarget::from_project(&row).expect("a project row");
        assert_eq!(
            from_row.config_files,
            ["/srv/shop/a.yml", "/srv/shop/b.yml"]
        );
        assert!(ComposeTarget::from_project(&thing(ResourceKind::Image, "x")).is_none());
    }

    #[test]
    fn compose_availability_asks_the_engine_for_its_compose_version() {
        let host = Arc::new(RecordingHost::new("host"));
        let backend = docker_on(&host);
        assert!(!block_on(backend.compose_available()));
        assert_eq!(
            host.recorded(),
            vec![asked("docker", &["compose", "version"])]
        );
    }

    #[test]
    fn taking_a_project_down_runs_down_and_volumes_only_when_chosen() {
        for (volumes, tail) in [
            (None, &["down"][..]),
            (Some(Vec::new()), &["down", "--volumes"]),
        ] {
            let host = Arc::new(RecordingHost::new("host"));
            let plan = DestructivePlan::compose_down(
                shop(),
                vec![thing(ResourceKind::Container, "shop-web-1")],
                Vec::new(),
                volumes,
            )
            .expect("one container goes");
            let outcome = block_on(docker_on(&host).destroy(&plan));
            assert!(matches!(
                outcome,
                Err(ContainerError::HostUnavailable { .. })
            ));
            let mut expected = vec![
                "compose",
                "-p",
                "shop",
                "--project-directory",
                "/srv/shop",
                "-f",
                "/srv/shop/a.yml",
                "-f",
                "/srv/shop/b.yml",
            ];
            expected.extend(tail);
            assert_eq!(host.recorded(), vec![asked("docker", &expected)]);
        }
    }

    #[test]
    fn the_down_plan_is_gathered_by_the_projects_own_label() {
        let host = Arc::new(RecordingHost::new("host"));
        let _ = block_on(docker_on(&host).compose_down_plan(&shop(), true));
        assert_eq!(
            host.recorded().first(),
            Some(&asked(
                "docker",
                &[
                    "ps",
                    "-a",
                    "--filter",
                    "label=com.docker.compose.project=shop",
                    "--format",
                    "{{json .}}"
                ]
            ))
        );
        let refused = block_on(
            docker_on(&Arc::new(RecordingHost::new("h")))
                .compose_down_plan(&ComposeTarget::new("-x"), false),
        );
        assert!(matches!(refused, Err(ContainerError::Invalid { .. })));
    }
}

mod image_commands {
    use super::*;

    #[test]
    fn pull_tag_and_history_end_options_before_the_value() {
        let host = Arc::new(RecordingHost::new("host"));
        let backend = docker_on(&host);
        let _ = block_on(backend.pull("ghcr.io/acme/app:1.2@sha256:abc"));
        let _ = block_on(backend.tag("app:1", "registry.local:5000/app:1"));
        let _ = block_on(backend.history("sha256:abc"));
        assert_eq!(
            host.recorded(),
            vec![
                asked("docker", &["pull", "--", "ghcr.io/acme/app:1.2@sha256:abc"]),
                asked(
                    "docker",
                    &["tag", "--", "app:1", "registry.local:5000/app:1"]
                ),
                asked(
                    "docker",
                    &[
                        "image",
                        "history",
                        "--no-trunc",
                        "--format",
                        "{{json .}}",
                        "--",
                        "sha256:abc"
                    ]
                ),
            ]
        );
    }

    #[test]
    fn a_terminal_pull_is_the_same_command() {
        let backend = DockerBackend::docker(crate::tests::local());
        assert_eq!(
            backend.pull_command("alpine:3.20"),
            Some(("docker".to_string(), words(&["pull", "--", "alpine:3.20"])))
        );
        assert_eq!(backend.pull_command("--all-tags"), None);
        assert_eq!(backend.pull_command("alpine 3"), None);
    }

    #[test]
    fn a_hostile_reference_is_refused_before_anything_runs() {
        let host = Arc::new(RecordingHost::new("host"));
        let backend = docker_on(&host);
        for hostile in ["--privileged", "-a", "alpine latest", "a;b c", "", "x\ny"] {
            assert!(matches!(
                block_on(backend.pull(hostile)),
                Err(ContainerError::Invalid { .. })
            ));
            assert!(matches!(
                block_on(backend.tag(hostile, "ok:1")),
                Err(ContainerError::Invalid { .. })
            ));
            assert!(matches!(
                block_on(backend.tag("ok:1", hostile)),
                Err(ContainerError::Invalid { .. })
            ));
        }
        assert!(matches!(
            block_on(backend.history("-q")),
            Err(ContainerError::Invalid { .. })
        ));
        assert_eq!(host.recorded(), vec![], "nothing was asked of the engine");
    }
}

mod run_command {
    use super::*;

    fn full_spec() -> RunSpec {
        RunSpec {
            image: "postgres:16".into(),
            name: Some("db".into()),
            ports: vec![
                PortMapping {
                    host: 5433,
                    container: 5432,
                    protocol: PortProtocol::Tcp,
                },
                PortMapping {
                    host: 5000,
                    container: 5000,
                    protocol: PortProtocol::Udp,
                },
            ],
            env: vec![
                ("POSTGRES_PASSWORD".into(), "pa ss;word".into()),
                ("EXTRA".into(), "--privileged".into()),
            ],
            volumes: vec![
                VolumeMount {
                    source: "pgdata".into(),
                    target: "/var/lib/postgresql/data".into(),
                    read_only: false,
                },
                VolumeMount {
                    source: "/srv/init scripts".into(),
                    target: "/docker-entrypoint-initdb.d".into(),
                    read_only: true,
                },
            ],
            network: Some("backend".into()),
            detach: true,
            remove_on_exit: true,
            command: words(&["postgres", "-c", "log_statement=all"]),
        }
    }

    #[test]
    fn every_value_is_its_own_word_and_a_value_stays_whole() {
        let backend = DockerBackend::docker(crate::tests::local());
        let (program, args) = backend.run_command(&full_spec()).expect("a valid spec");
        assert_eq!(program, "docker");
        assert_eq!(
            args,
            words(&[
                "run",
                "--detach",
                "--rm",
                "--name",
                "db",
                "--network",
                "backend",
                "--publish",
                "5433:5432/tcp",
                "--publish",
                "5000:5000/udp",
                "--env",
                "POSTGRES_PASSWORD=pa ss;word",
                "--env",
                "EXTRA=--privileged",
                "--volume",
                "pgdata:/var/lib/postgresql/data",
                "--volume",
                "/srv/init scripts:/docker-entrypoint-initdb.d:ro",
                "postgres:16",
                "postgres",
                "-c",
                "log_statement=all",
            ])
        );
    }

    #[test]
    fn a_bare_spec_is_just_the_image() {
        let backend = DockerBackend::podman(crate::tests::local());
        let spec = RunSpec {
            image: "alpine".into(),
            ..RunSpec::default()
        };
        assert_eq!(
            backend.run_command(&spec),
            Ok(("podman".to_string(), words(&["run", "alpine"])))
        );
    }

    #[test]
    fn each_hostile_value_is_refused_by_name() {
        let backend = DockerBackend::docker(crate::tests::local());
        let refused = |change: &dyn Fn(&mut RunSpec)| {
            let mut spec = full_spec();
            change(&mut spec);
            match backend.run_command(&spec) {
                Err(ContainerError::Invalid { detail }) => detail,
                other => panic!("expected a refusal, got {other:?}"),
            }
        };
        assert!(refused(&|s| s.image = "--privileged".into()).contains("image"));
        assert!(refused(&|s| s.image = "alpine latest".into()).contains("image"));
        assert!(refused(&|s| s.image = String::new()).contains("image"));
        assert!(refused(&|s| s.name = Some("-x".into())).contains("name"));
        assert!(refused(&|s| s.name = Some("a b".into())).contains("name"));
        assert!(refused(&|s| s.network = Some("--net".into())).contains("network"));
        assert!(refused(&|s| s.env = vec![("A B".into(), "v".into())]).contains("environment"));
        assert!(refused(&|s| s.env = vec![("-X".into(), "v".into())]).contains("environment"));
        assert!(refused(&|s| s.env = vec![("1A".into(), "v".into())]).contains("environment"));
        assert!(refused(&|s| s.env = vec![("A".into(), "v\0".into())]).contains("NUL"));
        assert!(refused(&|s| s.ports[0].host = 0).contains("port"));
        assert!(refused(&|s| s.volumes[0].source = "-v".into()).contains("volume source"),);
        assert!(
            refused(&|s| s.volumes[0].source = "a:b".into()).contains("volume source"),
            "a colon would move the split"
        );
        assert!(refused(&|s| s.volumes[0].target = "/a:/b".into()).contains("volume target"),);
    }

    #[test]
    fn a_windows_drive_letter_is_not_a_second_separator() {
        let backend = DockerBackend::docker(crate::tests::local());
        let spec = RunSpec {
            image: "alpine".into(),
            volumes: vec![VolumeMount {
                source: "C:\\work".into(),
                target: "/work".into(),
                read_only: false,
            }],
            ..RunSpec::default()
        };
        let (_, args) = backend.run_command(&spec).expect("a drive path is a path");
        assert!(args.contains(&"C:\\work:/work".to_string()), "{args:?}");
    }
}

mod volume_and_network_commands {
    use super::*;

    #[test]
    fn create_and_connect_build_one_word_per_value() {
        let host = Arc::new(RecordingHost::new("host"));
        let backend = docker_on(&host);
        let _ = block_on(backend.create_volume(
            "pgdata",
            Some("local"),
            &[("com.acme/owner".into(), "a team; with =".into())],
        ));
        let _ = block_on(backend.create_volume("plain", None, &[]));
        let _ = block_on(backend.create_network("backend", Some("bridge"), Some("10.5.0.0/16")));
        let _ = block_on(backend.create_network("plain", None, None));
        let _ = block_on(backend.connect("backend", "db", Some("database")));
        let _ = block_on(backend.connect("backend", "db", None));
        let _ = block_on(backend.disconnect("backend", "db"));
        assert_eq!(
            host.recorded(),
            vec![
                asked(
                    "docker",
                    &[
                        "volume",
                        "create",
                        "--driver",
                        "local",
                        "--label",
                        "com.acme/owner=a team; with =",
                        "--",
                        "pgdata"
                    ]
                ),
                asked("docker", &["volume", "create", "--", "plain"]),
                asked(
                    "docker",
                    &[
                        "network",
                        "create",
                        "--driver",
                        "bridge",
                        "--subnet",
                        "10.5.0.0/16",
                        "--",
                        "backend"
                    ]
                ),
                asked("docker", &["network", "create", "--", "plain"]),
                asked(
                    "docker",
                    &[
                        "network", "connect", "--alias", "database", "--", "backend", "db"
                    ]
                ),
                asked("docker", &["network", "connect", "--", "backend", "db"]),
                asked("docker", &["network", "disconnect", "--", "backend", "db"]),
            ]
        );
    }

    #[test]
    fn a_hostile_value_is_refused_before_anything_runs() {
        let host = Arc::new(RecordingHost::new("host"));
        let backend = docker_on(&host);
        let invalid = |outcome: Result<(), ContainerError>| {
            assert!(
                matches!(outcome, Err(ContainerError::Invalid { .. })),
                "{outcome:?}"
            );
        };
        for hostile in ["-x", "a b", "a;b", "", "--driver=evil"] {
            invalid(block_on(backend.create_volume(hostile, None, &[])));
            invalid(block_on(backend.create_network(hostile, None, None)));
            invalid(block_on(backend.connect(hostile, "db", None)));
            invalid(block_on(backend.connect("net", hostile, None)));
            invalid(block_on(backend.connect("net", "db", Some(hostile))));
            invalid(block_on(backend.disconnect(hostile, "db")));
            invalid(block_on(backend.disconnect("net", hostile)));
            invalid(block_on(backend.create_volume("ok", Some(hostile), &[])));
            invalid(block_on(backend.create_network("ok", Some(hostile), None)));
        }
        invalid(block_on(backend.create_network("ok", None, Some("-x/8"))));
        invalid(block_on(backend.create_network(
            "ok",
            None,
            Some("10.0.0.0"),
        )));
        invalid(block_on(backend.create_volume(
            "ok",
            None,
            &[("a=b".into(), "v".into())],
        )));
        invalid(block_on(backend.create_volume(
            "ok",
            None,
            &[("-l".into(), "v".into())],
        )));
        assert_eq!(host.recorded(), vec![], "nothing was asked of the engine");
    }
}

mod removal {
    use super::*;

    fn plan_for(kind: ResourceKind) -> DestructivePlan {
        DestructivePlan::remove(kind, vec![thing(kind, "one")]).expect("one target")
    }

    #[test]
    fn force_is_a_flag_only_a_forced_plan_carries() {
        // Docker's `volume rm -f` and `network rm -f` still refuse what is in
        // use, so a forced plan for them is never run (see `forced_removal`).
        for (kind, plain, forced) in [
            (ResourceKind::Image, &["rmi"][..], &["rmi", "--force"][..]),
            (ResourceKind::Container, &["rm"], &["rm", "--force"]),
        ] {
            for (plan, head) in [
                (plan_for(kind), plain),
                (
                    plan_for(kind)
                        .forced(FORCE_CONFIRMATION)
                        .expect("typed the word"),
                    forced,
                ),
            ] {
                let host = Arc::new(RecordingHost::new("host"));
                let _ = block_on(docker_on(&host).destroy(&plan));
                let mut expected = head.to_vec();
                expected.extend(["--", "id-one"]);
                assert_eq!(host.recorded(), vec![asked("docker", &expected)]);
            }
        }
    }

    #[test]
    fn a_plan_is_forced_only_by_typing_the_word() {
        assert!(plan_for(ResourceKind::Image).forced("").is_none());
        assert!(plan_for(ResourceKind::Image).forced("yes").is_none());
        assert!(plan_for(ResourceKind::Image).forced("FORCE").is_none());
        let forced = plan_for(ResourceKind::Image)
            .forced(" force ")
            .expect("a trailing space is a typing artefact");
        assert!(forced.is_forced());
        assert!(!plan_for(ResourceKind::Image).is_forced());
        assert!(
            forced.warning().is_some_and(|text| text.contains("Forced")),
            "the dialog must say what force does"
        );

        let prune = DestructivePlan::prune(
            crate::PruneScope::Reclaimable,
            vec![thing(ResourceKind::Image, "x")],
        )
        .expect("one target");
        assert!(prune.forced(FORCE_CONFIRMATION).is_none());
        let down = DestructivePlan::compose_down(
            shop(),
            vec![thing(ResourceKind::Container, "c")],
            Vec::new(),
            None,
        )
        .expect("one target");
        assert!(down.forced(FORCE_CONFIRMATION).is_none());
    }

    #[test]
    fn the_removal_plan_says_what_depends_on_each_target() {
        let in_use = thing(ResourceKind::Volume, "pgdata");
        let idle = thing(ResourceKind::Volume, "scratch");
        let user = thing(ResourceKind::Container, "db");
        let backend = FakeBackend::docker().used_by(&in_use.id, vec![user.clone()]);

        let plan = block_on(backend.removal_plan(ResourceKind::Volume, vec![in_use.clone(), idle]))
            .expect("the fake answers")
            .expect("two targets");
        assert!(plan.has_dependents());
        assert_eq!(
            plan.dependents(),
            [Dependents {
                target_id: in_use.id,
                containers: vec![user]
            }]
        );
        assert_eq!(plan.targets().len(), 2);
        assert!(
            block_on(backend.removal_plan(ResourceKind::Volume, Vec::new()))
                .expect("fine")
                .is_none()
        );
    }

    #[test]
    fn a_kind_with_no_usages_is_planned_without_asking() {
        let backend = FakeBackend::docker();
        let plan = block_on(backend.removal_plan(
            ResourceKind::Container,
            vec![thing(ResourceKind::Container, "c")],
        ))
        .expect("fine")
        .expect("one target");
        assert!(!plan.has_dependents());
        assert!(plan.dependents().is_empty());
    }

    #[test]
    fn a_down_plan_lists_volumes_only_when_they_were_chosen() {
        let containers = vec![thing(ResourceKind::Container, "web")];
        let networks = vec![thing(ResourceKind::Network, "net")];
        let kept =
            DestructivePlan::compose_down(shop(), containers.clone(), networks.clone(), None)
                .expect("targets");
        assert_eq!(kept.targets().len(), 2);
        assert_eq!(
            kept.intent(),
            Intent::ComposeDown {
                with_volumes: false
            }
        );
        assert_eq!(kept.confirmation(), "shop");
        assert!(kept.warning().is_some_and(|text| text.contains("kept")));
        assert_eq!(
            kept.compose_target().map(|t| t.project.as_str()),
            Some("shop")
        );

        let lost = DestructivePlan::compose_down(
            shop(),
            containers,
            networks,
            Some(vec![thing(ResourceKind::Volume, "data")]),
        )
        .expect("targets");
        assert_eq!(lost.targets().len(), 3);
        assert_eq!(lost.intent(), Intent::ComposeDown { with_volumes: true });
        assert!(
            lost.warning()
                .is_some_and(|text| text.contains("cannot be recovered"))
        );
        assert!(
            DestructivePlan::compose_down(shop(), Vec::new(), Vec::new(), Some(Vec::new()))
                .is_none(),
            "nothing to remove is nothing to confirm"
        );
    }
}

mod kubernetes_destroy {
    use super::*;

    fn pod_plan(id: &str) -> DestructivePlan {
        let mut pod = thing(ResourceKind::Pod, "web");
        pod.id = id.to_string();
        DestructivePlan::remove(ResourceKind::Pod, vec![pod]).expect("one target")
    }

    /// `command()` puts `--context` and `--namespace` last, which for a command
    /// that ends in `--` makes kubectl read them as two more pod names.
    #[test]
    fn scope_flags_come_before_the_separator() {
        let host = Arc::new(RecordingHost::new("host"));
        let backend = KubernetesBackend::with_kubeconfig(
            host.clone(),
            Some("/home/me/.kube/config".into()),
            Some(Scope {
                context: "prod".into(),
                namespace: Some("apps".into()),
            }),
        );
        let _ = block_on(backend.destroy(&pod_plan("shop/web-1")));
        assert_eq!(
            host.recorded(),
            vec![asked(
                "kubectl",
                &[
                    "--kubeconfig",
                    "/home/me/.kube/config",
                    "--context",
                    "prod",
                    "--namespace",
                    "apps",
                    "delete",
                    "pod",
                    "-n",
                    "shop",
                    "--",
                    "web-1"
                ]
            )]
        );
    }

    #[test]
    fn without_a_scope_the_command_is_just_the_deletion() {
        let host = Arc::new(RecordingHost::new("host"));
        let backend = KubernetesBackend::with_kubeconfig(host.clone(), None, None);
        let _ = block_on(backend.destroy(&pod_plan("-odd")));
        assert_eq!(
            host.recorded(),
            vec![asked("kubectl", &["delete", "pod", "--", "-odd"])]
        );
    }

    #[test]
    fn force_and_compose_are_not_things_kubernetes_does() {
        let host = Arc::new(RecordingHost::new("host"));
        let backend = KubernetesBackend::with_kubeconfig(host.clone(), None, None);
        let forced = pod_plan("a/b").forced(FORCE_CONFIRMATION).expect("forced");
        assert!(block_on(backend.destroy(&forced)).is_err());
        let down = DestructivePlan::compose_down(
            shop(),
            vec![thing(ResourceKind::Container, "c")],
            Vec::new(),
            None,
        )
        .expect("one target");
        assert!(block_on(backend.destroy(&down)).is_err());
        assert_eq!(host.recorded(), vec![], "refused before any command");
    }
}

mod capabilities {
    use super::*;

    const ALL: [Op; 8] = [
        Op::Compose,
        Op::ImagePull,
        Op::ImageTag,
        Op::ImageHistory,
        Op::ImageRun,
        Op::VolumeCreate,
        Op::NetworkCreate,
        Op::NetworkConnect,
    ];

    #[test]
    fn docker_and_podman_offer_everything_and_kubernetes_nothing() {
        for backend in [
            DockerBackend::docker(crate::tests::local()),
            DockerBackend::podman(crate::tests::local()),
        ] {
            for op in ALL {
                assert!(backend.offers(op), "{:?} {op:?}", backend.kind());
            }
        }
        let kubernetes = KubernetesBackend::with_program("kubectl");
        for op in ALL {
            assert!(!kubernetes.offers(op), "{op:?}");
        }
    }

    #[test]
    fn what_kubernetes_does_not_offer_it_refuses_without_a_command() {
        let host = Arc::new(RecordingHost::new("host"));
        let backend = KubernetesBackend::with_kubeconfig(host.clone(), None, None);
        let not_offered = |outcome: Result<(), ContainerError>| {
            assert!(
                matches!(outcome, Err(ContainerError::NotOffered { .. })),
                "{outcome:?}"
            );
        };
        not_offered(block_on(backend.pull("alpine")));
        not_offered(block_on(backend.tag("a", "b")));
        not_offered(block_on(backend.create_volume("v", None, &[])));
        not_offered(block_on(backend.create_network("n", None, None)));
        not_offered(block_on(backend.connect("n", "c", None)));
        not_offered(block_on(backend.disconnect("n", "c")));
        assert!(matches!(
            block_on(backend.history("x")),
            Err(ContainerError::NotOffered { .. })
        ));
        assert!(matches!(
            backend.run_command(&RunSpec::default()),
            Err(ContainerError::NotOffered { .. })
        ));
        assert!(backend.pull_command("alpine").is_none());
        assert!(
            backend
                .compose_command(&shop(), ComposeOp::Up, &[])
                .is_none()
        );
        assert!(!block_on(backend.compose_available()));
        assert!(
            block_on(backend.compose_down_plan(&shop(), false))
                .expect("nothing to ask")
                .is_none()
        );
        assert_eq!(host.recorded(), vec![], "nothing was asked of the engine");
        assert_eq!(backend.kind(), BackendKind::Kubernetes);
    }
}

mod the_fake {
    use super::*;
    use crate::fake_backend::{Misbehaviour, Request};
    use crate::operations::ImageLayer;

    #[test]
    fn it_writes_down_each_request_in_order() {
        let backend = FakeBackend::docker();
        block_on(async {
            backend.pull("alpine:3.20").await.expect("offered");
            backend.tag("alpine:3.20", "mine:1").await.expect("offered");
            backend
                .create_volume("data", Some("local"), &[("a".into(), "b".into())])
                .await
                .expect("offered");
            backend
                .create_network("net", None, Some("10.1.0.0/16"))
                .await
                .expect("offered");
            backend
                .connect("net", "c0ffee", Some("db"))
                .await
                .expect("offered");
            backend.disconnect("net", "c0ffee").await.expect("offered");
        });
        let spec = RunSpec {
            image: "alpine".into(),
            ..RunSpec::default()
        };
        backend.run_command(&spec).expect("offered");
        backend
            .compose_command(&shop(), ComposeOp::Up, &["web".into()])
            .expect("offered");
        assert_eq!(
            backend.requests(),
            vec![
                Request::Pull("alpine:3.20".into()),
                Request::Tag {
                    source: "alpine:3.20".into(),
                    target: "mine:1".into()
                },
                Request::CreateVolume {
                    name: "data".into(),
                    driver: Some("local".into()),
                    labels: vec![("a".into(), "b".into())]
                },
                Request::CreateNetwork {
                    name: "net".into(),
                    driver: None,
                    subnet: Some("10.1.0.0/16".into())
                },
                Request::Connect {
                    network: "net".into(),
                    container: "c0ffee".into(),
                    alias: Some("db".into())
                },
                Request::Disconnect {
                    network: "net".into(),
                    container: "c0ffee".into()
                },
                Request::Run(spec),
                Request::ComposeCommand {
                    project: shop(),
                    op: ComposeOp::Up,
                    services: vec!["web".into()]
                },
            ]
        );
    }

    #[test]
    fn a_refused_value_is_not_a_request() {
        let backend = FakeBackend::docker();
        assert!(matches!(
            block_on(backend.pull("--all-tags")),
            Err(ContainerError::Invalid { .. })
        ));
        assert!(matches!(
            block_on(backend.connect("-n", "c", None)),
            Err(ContainerError::Invalid { .. })
        ));
        assert!(
            backend
                .compose_command(&ComposeTarget::new("-p"), ComposeOp::Up, &[])
                .is_none()
        );
        assert_eq!(backend.requests(), vec![]);
    }

    #[test]
    fn what_it_does_not_offer_it_refuses_and_does_not_record() {
        let backend = FakeBackend::docker()
            .not_offering(Op::ImagePull)
            .not_offering(Op::Compose)
            .not_offering(Op::ImageRun);
        assert!(!backend.offers(Op::ImagePull));
        assert!(backend.offers(Op::ImageTag));
        assert!(matches!(
            block_on(backend.pull("alpine")),
            Err(ContainerError::NotOffered { .. })
        ));
        assert!(backend.pull_command("alpine").is_none());
        assert!(
            backend
                .compose_command(&shop(), ComposeOp::Up, &[])
                .is_none()
        );
        assert!(!block_on(backend.compose_available()));
        assert!(
            block_on(backend.compose_down_plan(&shop(), false))
                .expect("not an error")
                .is_none()
        );
        assert!(matches!(
            backend.run_command(&RunSpec {
                image: "alpine".into(),
                ..RunSpec::default()
            }),
            Err(ContainerError::NotOffered { .. })
        ));
        assert_eq!(backend.requests(), vec![]);
    }

    #[test]
    fn a_failing_engine_is_still_asked() {
        let backend = FakeBackend::docker().misbehaving(Misbehaviour::EngineNotRunning);
        assert!(matches!(
            block_on(backend.pull("alpine")),
            Err(ContainerError::EngineNotRunning { .. })
        ));
        assert_eq!(backend.requests(), vec![Request::Pull("alpine".into())]);
    }

    #[test]
    fn history_answers_for_the_image_it_was_given() {
        let layer = ImageLayer {
            created_by: "RUN make".into(),
            size: "1MB".into(),
            created: "now".into(),
        };
        let backend = FakeBackend::docker().with_history("sha256:abc", vec![layer.clone()]);
        assert_eq!(block_on(backend.history("sha256:abc")), Ok(vec![layer]));
        assert_eq!(
            block_on(backend.history("sha256:def")),
            Err(ContainerError::NotFound {
                id: "sha256:def".into()
            })
        );
    }

    #[test]
    fn the_down_plan_lists_the_projects_own_things() {
        let in_project = |mut resource: Resource, project: &str| {
            resource.parent = Some(project.to_string());
            resource
        };
        let mut web = thing(ResourceKind::Container, "shop-web-1");
        web.compose = Some(ComposeLabels {
            project: "shop".into(),
            service: "web".into(),
            working_dir: None,
            config_files: Vec::new(),
        });
        let mut other = thing(ResourceKind::Container, "blog-web-1");
        other.compose = Some(ComposeLabels {
            project: "blog".into(),
            service: "web".into(),
            working_dir: None,
            config_files: Vec::new(),
        });
        let backend =
            FakeBackend::empty(BackendKind::Docker, &[ResourceKind::Container]).holding(vec![
                web,
                other,
                in_project(thing(ResourceKind::Network, "shop_default"), "shop"),
                in_project(thing(ResourceKind::Network, "blog_default"), "blog"),
                in_project(thing(ResourceKind::Volume, "shop_data"), "shop"),
            ]);
        let names = |plan: &DestructivePlan| -> Vec<String> {
            plan.targets().iter().map(|t| t.name.clone()).collect()
        };
        let kept = block_on(backend.compose_down_plan(&shop(), false))
            .expect("offered")
            .expect("something goes");
        assert_eq!(names(&kept), ["shop-web-1", "shop_default"]);
        let lost = block_on(backend.compose_down_plan(&shop(), true))
            .expect("offered")
            .expect("something goes");
        assert_eq!(names(&lost), ["shop-web-1", "shop_default", "shop_data"]);
        assert!(
            block_on(backend.compose_down_plan(&ComposeTarget::new("none"), false))
                .expect("offered")
                .is_none()
        );
    }
}
