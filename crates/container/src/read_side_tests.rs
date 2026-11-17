//! The read side of the trait: which commands are built, what the fake says,
//! and what a backend does with canned engine output.

use std::sync::Arc;

use crate::backend::{BackendKind, ContainerBackend, ContainerError};
use crate::backend_test_suite::{RecordedCommand, RecordingHost, shared_suite};
use crate::docker_backend::DockerBackend;
use crate::fake_backend::{FakeBackend, Misbehaviour};
use crate::kubernetes_backend::{KubernetesBackend, Scope};
use crate::resource::{Resource, ResourceKind, RunState};

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

mod commands_built {
    use super::*;

    #[test]
    fn docker_inspect_names_the_noun_and_ends_flags_before_the_id() {
        let host = Arc::new(RecordingHost::new("host"));
        let backend = DockerBackend::docker(host.clone());
        for kind in [
            ResourceKind::Container,
            ResourceKind::Image,
            ResourceKind::Volume,
            ResourceKind::Network,
        ] {
            let outcome = block_on(backend.inspect(kind, "--rm"));
            assert!(matches!(
                outcome,
                Err(ContainerError::HostUnavailable { .. })
            ));
        }
        assert_eq!(
            host.recorded(),
            vec![
                asked("docker", &["container", "inspect", "--", "--rm"]),
                asked("docker", &["image", "inspect", "--", "--rm"]),
                asked("docker", &["volume", "inspect", "--", "--rm"]),
                asked("docker", &["network", "inspect", "--", "--rm"]),
            ]
        );
    }

    #[test]
    fn a_kind_docker_cannot_inspect_asks_nothing() {
        let host = Arc::new(RecordingHost::new("host"));
        let backend = DockerBackend::docker(host.clone());
        for kind in [ResourceKind::Pod, ResourceKind::ComposeProject] {
            assert!(!backend.supports_inspect(kind));
            assert_eq!(block_on(backend.inspect(kind, "x")), Ok(None));
            assert!(!backend.supports_usages(kind));
            assert_eq!(block_on(backend.usages(kind, "x")), Ok(None));
        }
        assert!(host.recorded().is_empty());
    }

    #[test]
    fn stats_asks_for_exactly_the_ids_given_and_nothing_for_none() {
        let host = Arc::new(RecordingHost::new("host"));
        let docker = DockerBackend::docker(host.clone());
        let podman = DockerBackend::podman(host.clone());
        let ids = vec!["abc".to_string(), "-dash".to_string()];

        assert_eq!(block_on(docker.stats(&[])), Ok(Some(Vec::new())));
        assert!(block_on(docker.stats(&ids)).is_err());
        assert!(block_on(podman.stats(&ids)).is_err());

        assert_eq!(
            host.recorded(),
            vec![
                asked(
                    "docker",
                    &[
                        "stats",
                        "--no-stream",
                        "--format",
                        "{{json .}}",
                        "--",
                        "abc",
                        "-dash"
                    ]
                ),
                asked(
                    "podman",
                    &[
                        "stats",
                        "--no-stream",
                        "--format",
                        "json",
                        "--",
                        "abc",
                        "-dash"
                    ]
                ),
            ]
        );
    }

    #[test]
    fn usages_filter_by_the_right_key_with_the_id_as_one_word() {
        let host = Arc::new(RecordingHost::new("host"));
        let backend = DockerBackend::docker(host.clone());
        let hostile = "a b; rm -rf";
        for kind in [
            ResourceKind::Volume,
            ResourceKind::Network,
            ResourceKind::Image,
        ] {
            assert!(backend.supports_usages(kind));
            assert!(block_on(backend.usages(kind, hostile)).is_err());
        }
        let filters: Vec<String> = host
            .recorded()
            .into_iter()
            .map(|command| command.args.join("|"))
            .collect();
        assert_eq!(
            filters,
            vec![
                "ps|-a|--filter|volume=a b; rm -rf|--format|{{json .}}",
                "ps|-a|--filter|network=a b; rm -rf|--format|{{json .}}",
                "ps|-a|--filter|ancestor=a b; rm -rf|--format|{{json .}}",
            ]
        );
    }

    #[test]
    fn kubernetes_inspect_puts_the_name_before_the_scope_flags() {
        let host = Arc::new(RecordingHost::new("host"));
        let backend = KubernetesBackend::with_scope(
            host.clone(),
            Scope {
                context: "work".into(),
                namespace: Some("shop".into()),
            },
        );
        assert!(block_on(backend.inspect(ResourceKind::Pod, "shop/web")).is_err());
        assert_eq!(
            block_on(backend.inspect(ResourceKind::Image, "x")),
            Ok(None)
        );
        assert_eq!(
            host.recorded(),
            vec![asked(
                "kubectl",
                &[
                    "get",
                    "pod",
                    "web",
                    "-n",
                    "shop",
                    "-o",
                    "json",
                    "--context",
                    "work",
                    "--namespace",
                    "shop"
                ]
            )]
        );
    }

    #[test]
    fn kubernetes_top_lists_each_namespace_once() {
        let host = Arc::new(RecordingHost::answering_with("host", "sh", &["-c", "true"]));
        let backend = KubernetesBackend::new(host.clone());
        let ids = vec![
            "a/one".to_string(),
            "b/two".to_string(),
            "a/three".to_string(),
        ];
        assert_eq!(block_on(backend.stats(&ids)), Ok(Some(Vec::new())));
        assert_eq!(
            host.recorded().len(),
            2,
            "one `top` per namespace, not one per pod"
        );
    }

    #[test]
    fn capabilities_are_declared_per_engine() {
        let host = Arc::new(RecordingHost::new("host"));
        let docker = DockerBackend::docker(host.clone());
        assert!(docker.supports_stats());
        assert!(docker.supports_inspect(ResourceKind::Container));
        assert!(!docker.supports_usages(ResourceKind::Container));

        let kubernetes = KubernetesBackend::new(host);
        assert!(kubernetes.supports_stats());
        assert!(kubernetes.supports_inspect(ResourceKind::Pod));
        assert!(!kubernetes.supports_inspect(ResourceKind::Container));
        assert!(!kubernetes.supports_usages(ResourceKind::Volume));
        assert_eq!(kubernetes.kind(), BackendKind::Kubernetes);
    }
}

mod the_fake {
    use super::*;

    fn container(id: &str) -> Resource {
        Resource {
            kind: ResourceKind::Container,
            id: id.into(),
            name: id.into(),
            state: RunState::Running,
            detail: Vec::new(),
            parent: None,
            compose: None,
        }
    }

    #[test]
    fn answers_inspect_stats_and_usages() {
        let backend = FakeBackend::docker().used_by("data", vec![container("c0ffee")]);

        let inspection = block_on(backend.inspect(ResourceKind::Container, "c0ffee"))
            .expect("inspect")
            .expect("offered");
        assert!(inspection.has_secrets());
        assert!(!inspection.raw_json.contains("fake-not-real"));
        assert!(inspection.raw_json_revealed.contains("fake-not-real"));

        let stats = block_on(backend.stats(&["c0ffee".into(), "other".into()]))
            .expect("stats")
            .expect("offered");
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].memory_limit, Some(512 * 1024 * 1024));

        let users = block_on(backend.usages(ResourceKind::Volume, "data"))
            .expect("usages")
            .expect("offered");
        assert_eq!(users, vec![container("c0ffee")]);
        assert_eq!(
            block_on(backend.usages(ResourceKind::Volume, "unused")),
            Ok(Some(Vec::new()))
        );
    }

    #[test]
    fn inspecting_something_unknown_is_not_found() {
        assert_eq!(
            block_on(FakeBackend::docker().inspect(ResourceKind::Container, "gone")),
            Err(ContainerError::NotFound { id: "gone".into() })
        );
    }

    #[test]
    fn misbehaviour_reaches_every_new_call() {
        let backend = FakeBackend::docker().misbehaving(Misbehaviour::NotAuthorized);
        assert!(matches!(
            block_on(backend.inspect(ResourceKind::Container, "c0ffee")),
            Err(ContainerError::NotAuthorized { .. })
        ));
        assert!(matches!(
            block_on(backend.stats(&["c0ffee".into()])),
            Err(ContainerError::NotAuthorized { .. })
        ));
        assert!(matches!(
            block_on(backend.usages(ResourceKind::Volume, "v")),
            Err(ContainerError::NotAuthorized { .. })
        ));
    }

    #[test]
    fn the_suite_catches_a_backend_that_offers_what_it_declines() {
        struct Liar;
        #[async_trait::async_trait]
        impl ContainerBackend for Liar {
            fn kind(&self) -> BackendKind {
                BackendKind::Docker
            }
            fn supported_kinds(&self) -> &'static [ResourceKind] {
                &[ResourceKind::Container]
            }
            fn supported_actions(
                &self,
                _: ResourceKind,
            ) -> &'static [crate::resource::ResourceAction] {
                &[]
            }
            async fn list(&self, _: ResourceKind) -> Result<Vec<Resource>, ContainerError> {
                Ok(Vec::new())
            }
            async fn act(
                &self,
                _: ResourceKind,
                _: crate::resource::ResourceAction,
                _: &str,
            ) -> Result<(), ContainerError> {
                Ok(())
            }
            fn watch(
                &self,
            ) -> Option<futures::stream::BoxStream<'static, crate::backend::BackendEvent>>
            {
                None
            }
            fn logs_command(&self, _: ResourceKind, _: &str) -> Option<(String, Vec<String>)> {
                None
            }
            async fn prune_targets(
                &self,
                _: crate::PruneScope,
            ) -> Option<Result<Vec<Resource>, ContainerError>> {
                None
            }
            async fn destroy(&self, _: &crate::DestructivePlan) -> Result<(), ContainerError> {
                Ok(())
            }
            fn exec_command(&self, _: ResourceKind, _: &str) -> Option<(String, Vec<String>)> {
                None
            }
            // Declines inspect, then answers it anyway.
            async fn inspect(
                &self,
                _: ResourceKind,
                _: &str,
            ) -> Result<Option<crate::Inspection>, ContainerError> {
                Ok(Some(crate::Inspection {
                    sections: Vec::new(),
                    raw_json: String::new(),
                    raw_json_revealed: String::new(),
                }))
            }
        }
        let error = block_on(shared_suite(&Liar)).expect_err("the suite must object");
        assert!(error.contains("does not support inspect"), "{error}");
    }
}

/// Canned engine output fed through the real backends, by pointing the host at
/// `sh`. Unix only: it needs a shell to stand in for the engine.
#[cfg(unix)]
mod canned_engine_output {
    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/fixtures");

    fn printing(fixture: &str) -> Arc<RecordingHost> {
        Arc::new(RecordingHost::answering_with(
            "host",
            "sh",
            &["-c", "cat \"$0\"", &format!("{FIXTURES}/{fixture}")],
        ))
    }

    fn failing(stderr: &str) -> Arc<RecordingHost> {
        Arc::new(RecordingHost::answering_with(
            "host",
            "sh",
            &["-c", "echo \"$0\" >&2; exit 1", stderr],
        ))
    }

    #[test]
    fn docker_inspect_end_to_end_masks_the_secret() {
        let backend = DockerBackend::docker(printing("docker-container-inspect.json"));
        let inspection = block_on(backend.inspect(ResourceKind::Container, "abc"))
            .expect("inspect")
            .expect("offered");
        assert!(inspection.has_secrets());
        assert!(!inspection.raw_json.contains("hunter2-not-real"));
        assert!(inspection.raw_json_revealed.contains("hunter2-not-real"));
    }

    #[test]
    fn podman_stats_end_to_end() {
        let backend = DockerBackend::podman(printing("podman-stats.json"));
        let stats = block_on(backend.stats(&["3f0e9c1a7b2d".into()]))
            .expect("stats")
            .expect("offered");
        assert_eq!(stats[0].name, "db");
    }

    #[test]
    fn docker_usages_end_to_end_lists_containers() {
        let host = Arc::new(RecordingHost::answering_with(
            "host",
            "sh",
            &[
                "-c",
                "printf '%s\\n' '{\"ID\":\"abc\",\"Names\":\"web\",\"Image\":\"nginx\",\"State\":\"running\"}'",
            ],
        ));
        let backend = DockerBackend::docker(host);
        let users = block_on(backend.usages(ResourceKind::Volume, "data"))
            .expect("usages")
            .expect("offered");
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].name, "web");
        assert_eq!(users[0].state, RunState::Running);
    }

    #[test]
    fn docker_inspect_of_an_empty_array_is_not_found() {
        let host = Arc::new(RecordingHost::answering_with(
            "host",
            "sh",
            &["-c", "echo '[]'"],
        ));
        let backend = DockerBackend::docker(host);
        assert_eq!(
            block_on(backend.inspect(ResourceKind::Volume, "gone")),
            Err(ContainerError::NotFound { id: "gone".into() })
        );
    }

    #[test]
    fn docker_inspect_of_unparsable_output_is_a_parse_error() {
        let host = Arc::new(RecordingHost::answering_with(
            "host",
            "sh",
            &["-c", "echo nope"],
        ));
        let backend = DockerBackend::docker(host);
        assert!(matches!(
            block_on(backend.inspect(ResourceKind::Image, "x")),
            Err(ContainerError::Parse { .. })
        ));
    }

    #[test]
    fn kubernetes_inspect_end_to_end_and_not_found() {
        let backend = KubernetesBackend::new(printing("kubectl-pod.json"));
        let inspection = block_on(backend.inspect(ResourceKind::Pod, "shop/web"))
            .expect("inspect")
            .expect("offered");
        assert!(inspection.has_secrets());
        assert!(!inspection.raw_json.contains("k8s-not-real"));

        let gone = KubernetesBackend::new(failing(
            "Error from server (NotFound): pods \"web\" not found",
        ));
        assert_eq!(
            block_on(gone.inspect(ResourceKind::Pod, "shop/web")),
            Err(ContainerError::NotFound {
                id: "shop/web".into()
            })
        );
    }

    #[test]
    fn kubernetes_top_filters_to_the_pods_asked_about() {
        let backend = KubernetesBackend::new(printing("kubectl-top-pods.txt"));
        let stats = block_on(backend.stats(&["shop/worker-0".into()]))
            .expect("top")
            .expect("metrics available");
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].id, "shop/worker-0");
        assert_eq!(stats[0].cpu_percent, Some(150.0));
    }

    #[test]
    fn a_cluster_without_metrics_is_none_not_an_error() {
        for words in [
            "error: Metrics API not available",
            "Error from server (NotFound): the server could not find the requested resource (get pods.metrics.k8s.io)",
        ] {
            let backend = KubernetesBackend::new(failing(words));
            assert_eq!(
                block_on(backend.stats(&["shop/web".into()])),
                Ok(None),
                "{words}"
            );
        }
    }

    #[test]
    fn any_other_top_failure_still_surfaces() {
        let backend = KubernetesBackend::new(failing("error: something else broke"));
        assert!(matches!(
            block_on(backend.stats(&["shop/web".into()])),
            Err(ContainerError::CommandFailed { .. })
        ));
    }
}
