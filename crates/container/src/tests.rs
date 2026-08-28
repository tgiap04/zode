//! Tests for the crate's own seams.
//!
//! Split between two kinds: those that hold the fake to the trait, and those
//! that put the machine's real engine through the same suite.
//!
//! # Engine tests
//!
//! The second kind is `#[ignore]`d, and this is the correction of a claim that
//! used to stand here -- that Docker is on every machine and in CI, so they
//! always run. Both halves were wrong. A runner may have the CLI and no daemon,
//! and in that state `docker` does not fail, it *waits*: three of these hit
//! nextest's 60-second timeout on Windows CI while passing on every machine
//! that happened to have a daemon up. A test whose result depends on what is
//! installed and running is a report about the machine, not about this crate.
//!
//! Run them deliberately, on a machine with an engine:
//!
//! ```text
//! cargo nextest run -p container --run-ignored all
//! ```
//!
//! Everything they were protecting that can be checked without an engine --
//! which command is built, which flags are absent, how output parses -- is
//! covered by the fake and by the parsing tests, and those always run.

use crate::backend::{BackendKind, ContainerBackend, ContainerError};
use crate::backend_test_suite::shared_suite;
use crate::docker_backend::DockerBackend;
use crate::fake_backend::{FakeBackend, Misbehaviour};
use crate::resource::{ResourceAction, ResourceKind, RunState};

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    smol::block_on(future)
}

#[test]
fn the_fake_satisfies_the_shared_suite() {
    let backend = FakeBackend::docker();
    let reached = block_on(shared_suite(&backend)).expect("the fake must satisfy the suite");
    assert!(
        reached > 0,
        "a suite that reached nothing has proven nothing"
    );
}

/// The suite must run against a real engine, not only the fake.
///
/// Docker is on the machine of anyone working on this and in CI. When it is not,
/// the backend answers `EngineMissing`/`EngineNotRunning` and the suite returns
/// early rather than failing -- so this test is honest on a machine without
/// Docker instead of red.
#[test]
fn real_docker_satisfies_the_shared_suite() {
    let backend = DockerBackend::docker();
    let reached = block_on(shared_suite(&backend)).expect("real docker must satisfy the suite");
    if reached == 0 {
        eprintln!("docker is not usable here; the suite reached no kind");
    }
}

/// A backend that lists a kind it does not declare is unreachable from the view,
/// which asks `supported_kinds`. The suite has to catch that.
#[test]
fn the_suite_rejects_a_backend_that_lists_an_undeclared_kind() {
    struct Liar;

    #[async_trait::async_trait]
    impl ContainerBackend for Liar {
        fn kind(&self) -> BackendKind {
            BackendKind::Docker
        }
        fn supported_kinds(&self) -> &'static [ResourceKind] {
            &[ResourceKind::Container]
        }
        fn supported_actions(&self, _kind: ResourceKind) -> &'static [ResourceAction] {
            &[]
        }
        async fn list(
            &self,
            kind: ResourceKind,
        ) -> Result<Vec<crate::resource::Resource>, ContainerError> {
            // Answers for a kind it never declared.
            Ok(vec![crate::resource::Resource {
                kind,
                id: "x".into(),
                name: "x".into(),
                state: RunState::Unknown,
                detail: Vec::new(),
                parent: None,
            }])
        }
        async fn act(
            &self,
            _kind: ResourceKind,
            _action: ResourceAction,
            _id: &str,
        ) -> Result<(), ContainerError> {
            Ok(())
        }
        fn watch(&self) -> Option<futures::stream::BoxStream<'static, crate::BackendEvent>> {
            None
        }
        async fn prune_targets(
            &self,
            _scope: crate::PruneScope,
        ) -> Option<Result<Vec<crate::resource::Resource>, ContainerError>> {
            None
        }
        async fn destroy(&self, _plan: &crate::DestructivePlan) -> Result<(), ContainerError> {
            Ok(())
        }
        fn logs_command(&self, _kind: ResourceKind, _id: &str) -> Option<(String, Vec<String>)> {
            None
        }
        fn exec_command(&self, _kind: ResourceKind, _id: &str) -> Option<(String, Vec<String>)> {
            None
        }
    }

    let error = block_on(shared_suite(&Liar)).expect_err("the suite must catch an undeclared kind");
    assert!(
        error.contains("supported_kinds"),
        "the failure must name the seam that was breached, got: {error}"
    );
}

#[test]
fn a_missing_engine_is_not_the_same_error_as_a_dead_one() {
    let missing = FakeBackend::docker().misbehaving(Misbehaviour::EngineMissing);
    let dead = FakeBackend::docker().misbehaving(Misbehaviour::EngineNotRunning);
    let missing = block_on(missing.list(ResourceKind::Container)).expect_err("must fail");
    let dead = block_on(dead.list(ResourceKind::Container)).expect_err("must fail");
    assert_ne!(
        missing, dead,
        "'not installed' and 'installed but down' need different words in front \
         of the user; one error for both is how a panel tells somebody to \
         install what they already have"
    );
    assert!(matches!(missing, ContainerError::EngineMissing { .. }));
    assert!(matches!(dead, ContainerError::EngineNotRunning { .. }));
}

/// The suite returns early on an unusable engine instead of failing, so a
/// machine without Docker is quiet rather than red.
#[test]
fn the_suite_is_quiet_when_the_engine_is_not_there() {
    let backend = FakeBackend::docker().misbehaving(Misbehaviour::EngineMissing);
    let reached = block_on(shared_suite(&backend)).expect("an absent engine is not a failure");
    assert_eq!(reached, 0, "and it must report that it reached nothing");
}

#[test]
fn an_image_offers_no_lifecycle_action() {
    let backend = FakeBackend::docker();
    assert!(
        backend.supported_actions(ResourceKind::Image).is_empty(),
        "an image cannot be started, stopped or restarted"
    );
    assert!(
        !backend
            .supported_actions(ResourceKind::Container)
            .is_empty(),
        "a container can"
    );
}

#[test]
fn act_records_what_it_was_asked_and_refuses_what_it_does_not_hold() {
    let backend = FakeBackend::docker();
    block_on(backend.act(ResourceKind::Container, ResourceAction::Stop, "c0ffee"))
        .expect("a held id must be accepted");
    assert_eq!(
        backend.acted(),
        vec![(ResourceAction::Stop, "c0ffee".into())]
    );
    let error = block_on(backend.act(ResourceKind::Container, ResourceAction::Start, "nope"))
        .expect_err("an unheld id must be refused");
    assert!(matches!(error, ContainerError::NotFound { .. }));
}

mod docker_output {
    use crate::docker_types::DockerContainer;
    use crate::resource::RunState;

    /// `--format '{{json .}}'` prints one object per line, not one array. A
    /// single `from_str` fails the moment there are two rows, which is the bug
    /// this reader exists to avoid.
    #[test]
    fn several_rows_on_several_lines_all_parse() {
        let stdout = concat!(
            r#"{"ID":"aaa","Names":"one","Image":"ubuntu:22.04","State":"running","Status":"Up 2 minutes","Ports":""}"#,
            "\n",
            r#"{"ID":"bbb","Names":"two","Image":"ubuntu:24.04","State":"exited","Status":"Exited (0)","Ports":""}"#,
            "\n",
        );
        let rows = crate::docker_backend::parse_json_lines::<DockerContainer>(stdout);
        assert_eq!(rows.len(), 2, "both rows must survive the reader");
        assert_eq!(rows[0].id, "aaa");
        assert_eq!(rows[1].id, "bbb");
    }

    /// The same reader must cope with values pretty-printed over several lines,
    /// because that is the shape `kubectl --watch` emits and this reader carries
    /// over to it.
    #[test]
    fn values_pretty_printed_over_several_lines_also_parse() {
        let stdout = "{\n  \"ID\": \"aaa\",\n  \"Names\": \"one\"\n}\n{\n  \"ID\": \"bbb\",\n  \"Names\": \"two\"\n}\n";
        let rows = crate::docker_backend::parse_json_lines::<DockerContainer>(stdout);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn no_containers_reads_as_no_rows_not_as_an_error() {
        let rows = crate::docker_backend::parse_json_lines::<DockerContainer>("");
        assert!(rows.is_empty());
    }

    /// Docker adds columns between releases. A field this crate does not know
    /// must be ignored, and a field it knows that is absent must leave a blank
    /// cell -- not fail the whole list.
    #[test]
    fn unknown_and_missing_fields_do_not_fail_the_row() {
        let stdout = r#"{"ID":"aaa","SomeFieldFromANewerDocker":"x"}"#;
        let rows = crate::docker_backend::parse_json_lines::<DockerContainer>(stdout);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].image, "", "an absent column is blank, not fatal");
    }

    /// An unrecognised state word from a newer Docker must not be reported as
    /// stopped -- a misleading dot beside a running container is worse than no
    /// dot at all.
    #[test]
    fn an_unknown_state_word_is_unknown_not_stopped() {
        let stdout = r#"{"ID":"aaa","Names":"one","State":"hibernating"}"#;
        let rows = crate::docker_backend::parse_json_lines::<DockerContainer>(stdout);
        let resource = rows.into_iter().next().expect("one row").into_resource();
        assert_eq!(resource.state, RunState::Unknown);
    }

    /// A paused container is neither running nor stopped, and the difference is
    /// the only thing that decides which button its row gets: it resumes, it
    /// does not start.
    ///
    /// The words are observed, not guessed -- `docker run`, `docker pause`,
    /// `docker ps -a --format '{{json .}}'` on Docker 29.4.3 answered
    /// `State: "paused"` with `Status: "Up 2 seconds (Paused)"`.
    #[test]
    fn a_paused_container_is_paused_and_not_stopped() {
        let stdout =
            r#"{"ID":"aaa","Names":"one","State":"paused","Status":"Up 2 seconds (Paused)"}"#;
        let rows = crate::docker_backend::parse_json_lines::<DockerContainer>(stdout);
        let resource = rows.into_iter().next().expect("one row").into_resource();
        assert_eq!(
            resource.state,
            RunState::Paused,
            "reporting this as stopped would offer Start, which docker refuses \
             on a paused container"
        );
    }

    /// Docker's `Names` is comma-separated when a container carries more than
    /// one; the first is the one Docker itself shows.
    #[test]
    fn the_first_of_several_names_is_the_one_shown() {
        let stdout = r#"{"ID":"aaa","Names":"first,second","State":"running"}"#;
        let rows = crate::docker_backend::parse_json_lines::<DockerContainer>(stdout);
        let resource = rows.into_iter().next().expect("one row").into_resource();
        assert_eq!(resource.name, "first");
    }

    /// A row with no name at all falls back to the id rather than rendering an
    /// empty cell nobody can click.
    #[test]
    fn a_nameless_row_falls_back_to_its_id() {
        let stdout = r#"{"ID":"aaa","Names":"","State":"running"}"#;
        let rows = crate::docker_backend::parse_json_lines::<DockerContainer>(stdout);
        let resource = rows.into_iter().next().expect("one row").into_resource();
        assert_eq!(resource.name, "aaa");
    }
}

/// The two "engine is not usable" cases, on the real backend rather than the
/// fake -- the classification lives in `DockerBackend::stdout` and the fake does
/// not exercise it.
mod real_engine_classification {
    use super::block_on;
    use crate::backend::{BackendKind, ContainerBackend, ContainerError};
    use crate::docker_backend::DockerBackend;
    use crate::resource::ResourceKind;

    /// A program that is not on `PATH` must surface as "not installed", told
    /// apart structurally by the spawn's own `NotFound` rather than by reading
    /// any message.
    #[test]
    fn a_program_that_is_not_there_is_engine_missing() {
        let backend = DockerBackend::with_program("zode-no-such-engine", BackendKind::Docker);
        let error =
            block_on(backend.list(ResourceKind::Container)).expect_err("there is no such program");
        assert!(
            matches!(error, ContainerError::EngineMissing { .. }),
            "got {error:?}"
        );
    }

    /// A program that IS there and fails, whose `info` also fails, is an engine
    /// that cannot reach its daemon. `false` is a real binary on every platform
    /// this ships to and always exits non-zero, so it walks exactly that branch
    /// without needing a daemon to stop.
    #[test]
    fn a_program_that_is_there_but_answers_nothing_is_engine_not_running() {
        let backend = DockerBackend::with_program("false", BackendKind::Docker);
        let error = block_on(backend.list(ResourceKind::Container))
            .expect_err("a program that always fails cannot list");
        assert!(
            matches!(error, ContainerError::EngineNotRunning { .. }),
            "a live binary that cannot answer is 'not running', not 'not \
             installed' and not a generic failure; got {error:?}"
        );
    }

    /// The count the backend reports must be the count the CLI reports. Skipped
    /// rather than failed when docker is not usable here.
    #[test]
    #[ignore = "drives whatever engine the machine has; see `engine tests` in the module docs"]
    fn the_backend_counts_what_the_cli_counts() {
        // Through `util::command` rather than `std::process`: the repo disallows
        // the blocking std spawn outright (see clippy.toml), and the platform
        // wrappers behind this one are the reason.
        let cli = block_on(async {
            let mut command = util::command::Command::new("docker");
            command.args(["ps", "-a", "--format", "{{json .}}"]);
            command.output().await
        });
        let Ok(cli) = cli else {
            eprintln!("docker is not on PATH here; nothing to compare against");
            return;
        };
        if !cli.status.success() {
            eprintln!("docker is not usable here; nothing to compare against");
            return;
        }
        let cli_rows = String::from_utf8_lossy(&cli.stdout)
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();

        let backend = DockerBackend::docker();
        let listed = block_on(backend.list(ResourceKind::Container))
            .expect("docker answered the CLI, so it must answer the backend");
        assert_eq!(
            listed.len(),
            cli_rows,
            "the backend must see exactly the containers the CLI prints"
        );
    }
}

/// Kubernetes: the second backend, and the reason the trait exists.
///
/// Its job in this crate is not "Kubernetes works" -- there is no cluster here to
/// prove that against. Its job is that the trait means the same thing to an
/// engine shaped nothing like Docker: watch instead of poll, kubeconfig instead
/// of a socket, and a resource that cannot be started or stopped at all.
mod kubernetes {
    use super::block_on;
    use crate::backend::{BackendKind, ContainerBackend, ContainerError};
    use crate::backend_test_suite::shared_suite;
    use crate::kubeconfig;
    use crate::kubernetes_backend::{KubernetesBackend, Scope};
    use crate::kubernetes_types::PodList;
    use crate::resource::{ResourceAction, ResourceKind, RunState};

    const EMPTY_KUBECONFIG: &str = include_str!("fixtures/kubeconfig-empty.json");
    const PODS: &str = include_str!("fixtures/pods.json");

    /// Captured, not written: this is what `kubectl config view -o json` really
    /// prints on a machine with no contexts, and that machine is the common case.
    #[test]
    fn an_empty_kubeconfig_parses_as_empty_rather_than_failing() {
        let config = kubeconfig::parse(EMPTY_KUBECONFIG).expect("null contexts is valid JSON");
        assert!(config.is_empty(), "no contexts means nothing to look at");
        assert!(
            config.current.is_none(),
            "an empty `current-context` string is not a context"
        );
        assert!(config.effective().is_none());
    }

    /// One context and nothing marked current is unambiguous. Two is not, and
    /// guessing would silently point somebody at the wrong cluster.
    #[test]
    fn one_context_is_unambiguous_and_two_without_a_current_is_not() {
        let one =
            r#"{"contexts":[{"name":"only","context":{"namespace":"apps"}}],"current-context":""}"#;
        let config = kubeconfig::parse(one).expect("valid");
        let effective = config.effective().expect("one context is the answer");
        assert_eq!(effective.name, "only");
        assert_eq!(effective.namespace.as_deref(), Some("apps"));

        let two = r#"{"contexts":[{"name":"a"},{"name":"b"}],"current-context":""}"#;
        let config = kubeconfig::parse(two).expect("valid");
        assert!(
            config.effective().is_none(),
            "two contexts and none current must be asked about, not guessed"
        );

        let two_with_current = r#"{"contexts":[{"name":"a"},{"name":"b"}],"current-context":"b"}"#;
        let config = kubeconfig::parse(two_with_current).expect("valid");
        assert_eq!(config.effective().map(|c| c.name.as_str()), Some("b"));
    }

    /// A pod is not a container, and the trait says so through
    /// `supported_actions` rather than through a branch in the view.
    #[test]
    fn a_pod_offers_no_lifecycle_action_at_all() {
        let backend = KubernetesBackend::new();
        assert!(
            backend.supported_actions(ResourceKind::Pod).is_empty(),
            "kubectl has no restart verb for a pod, and `rollout restart` acts on \
             the controller -- offering it here would do something else to \
             something else"
        );
        assert_eq!(backend.supported_kinds(), &[ResourceKind::Pod]);
        assert_eq!(backend.kind(), BackendKind::Kubernetes);
    }

    /// Reached only by a caller that ignored `supported_actions`. It must say
    /// what is wrong rather than succeed quietly.
    #[test]
    fn acting_on_a_pod_refuses_and_explains() {
        let backend = KubernetesBackend::new();
        let error = block_on(backend.act(ResourceKind::Pod, ResourceAction::Restart, "ns/pod"))
            .expect_err("a pod cannot be restarted");
        let message = error.to_string();
        assert!(
            message.contains("controller"),
            "the refusal must point at what *can* be restarted, got: {message}"
        );
    }

    /// The pod fixture, through the same mapping the backend uses.
    ///
    /// Written from the documented schema rather than captured -- see
    /// `fixtures/README.md`. What it does prove is the mapping: phase to state,
    /// owner to parent, and a namespaced id.
    #[test]
    fn the_pod_mapping_reads_phase_owner_and_namespace() {
        let list: PodList = serde_json::from_str(PODS).expect("the fixture is valid");
        let resources: Vec<_> = list
            .items
            .into_iter()
            .map(|pod| pod.into_resource())
            .collect();
        assert_eq!(resources.len(), 4);

        let owned = &resources[0];
        assert_eq!(owned.kind, ResourceKind::Pod);
        assert_eq!(owned.state, RunState::Running);
        assert_eq!(
            owned.id, "production/web-7d9f8b6c4-xk2mp",
            "a pod name is unique only within a namespace, so the id carries both"
        );
        assert_eq!(
            owned.parent.as_deref(),
            Some("ReplicaSet/web-7d9f8b6c4"),
            "an owned pod names its controller -- what decides how loudly a \
             delete is warned about"
        );

        let bare = &resources[1];
        assert!(
            bare.parent.is_none(),
            "a bare pod has no controller, so nothing will recreate it"
        );

        let finished = &resources[2];
        assert_eq!(
            finished.state,
            RunState::Stopped,
            "Succeeded is not running"
        );
        assert_eq!(finished.parent.as_deref(), Some("Job/migrate"));

        let starting = &resources[3];
        assert_eq!(
            starting.state,
            RunState::Unknown,
            "Pending is on its way up: a stopped dot reads as broken and a \
             running dot reads as ready, and neither is true"
        );
    }

    /// The shared suite, with the second backend. This is what phase 04 is for.
    ///
    /// On a machine with no cluster the backend answers `NotConfigured` and the
    /// suite stops early rather than failing -- so this is honest here instead of
    /// red, exactly as it is for Docker when Docker is absent.
    #[test]
    fn kubernetes_satisfies_the_shared_suite() {
        let backend = KubernetesBackend::new();
        let reached = block_on(shared_suite(&backend))
            .expect("the trait must mean the same thing to kubernetes");
        if reached == 0 {
            eprintln!("no usable kubernetes here; the suite reached no kind");
        }
    }

    /// "Nothing chosen to look at" must not be reported as "the cluster is
    /// down".
    ///
    /// With no context `kubectl` falls back to `localhost:8080` and reports a
    /// refused connection -- a true statement about the wrong thing. The
    /// kubeconfig is therefore read *before* the `get`, and this is that
    /// ordering, checked against the real empty kubeconfig on this machine.
    #[test]
    #[ignore = "drives whatever engine the machine has; see `engine tests` in the module docs"]
    fn no_context_is_not_reported_as_an_unreachable_cluster() {
        let backend = KubernetesBackend::new();
        match block_on(backend.list(ResourceKind::Pod)) {
            Err(ContainerError::NotConfigured { .. }) => {}
            Err(ContainerError::EngineMissing { .. }) => {
                eprintln!("kubectl is not installed here; nothing to check");
            }
            // A machine with a working cluster gets a list, which is also fine.
            Ok(_) => {}
            Err(other) => panic!(
                "an empty kubeconfig must read as 'not configured', not as {other:?} \
                 -- that sends somebody hunting a cluster that was never the problem"
            ),
        }
    }

    #[test]
    fn a_missing_kubectl_is_engine_missing() {
        let backend = KubernetesBackend::with_program("zode-no-such-kubectl");
        let error =
            block_on(backend.list(ResourceKind::Pod)).expect_err("there is no such program");
        assert!(
            matches!(error, ContainerError::EngineMissing { .. }),
            "got {error:?}"
        );
    }

    /// A scope skips the kubeconfig check, because the caller has already chosen.
    #[test]
    #[ignore = "drives whatever engine the machine has; see `engine tests` in the module docs"]
    fn a_scope_is_passed_to_kubectl_rather_than_rediscovered() {
        let backend = KubernetesBackend::with_scope(Scope {
            context: "prod".into(),
            namespace: Some("apps".into()),
        });
        // No cluster here, so the call fails -- but it must fail having *tried*,
        // not having stopped at "no context", which is the branch a scope skips.
        match block_on(backend.list(ResourceKind::Pod)) {
            Err(ContainerError::NotConfigured { detail }) => {
                assert!(
                    detail.contains("context"),
                    "a scoped backend must not report the kubeconfig as empty; \
                     kubectl's own words are allowed, got: {detail}"
                );
            }
            Err(_) | Ok(_) => {}
        }
    }
}

/// Both backends held to the same suite, side by side.
///
/// The point is the pair: a suite that only ever ran against one engine would
/// pass while the trait quietly meant two different things.
#[test]
fn the_suite_means_the_same_thing_to_both_backends() {
    use crate::kubernetes_backend::KubernetesBackend;
    let docker = DockerBackend::docker();
    let kubernetes = KubernetesBackend::new();
    let docker_reached = block_on(shared_suite(&docker)).expect("docker");
    let kubernetes_reached = block_on(shared_suite(&kubernetes)).expect("kubernetes");
    // Reported rather than asserted: whether either engine is usable is a
    // property of the machine, not of the trait. Silence here would hide a run in
    // which neither engine answered anything.
    eprintln!(
        "suite reached {docker_reached} kind(s) on docker, \
         {kubernetes_reached} on kubernetes"
    );
    assert_ne!(
        docker.supported_kinds(),
        kubernetes.supported_kinds(),
        "if the two ever declared the same kinds, one of them would be wearing \
         the other's vocabulary -- which is the failure this trait exists to \
         prevent"
    );
}

/// The four resources that are not containers.
mod docker_resources {
    use super::block_on;
    use crate::docker_backend::{DockerBackend, parse_json_lines};
    use crate::docker_resources::{ComposeProject, DockerImage, DockerNetwork, DockerVolume};
    use crate::resource::{ResourceKind, RunState};
    use crate::{ContainerBackend, ContainerError};

    /// The trap the plan named: `Size` is human text, so ordering by it as a
    /// string puts `999MB` after `1.0GB`. The mapping must not invite that -- the
    /// value is carried as a detail string and nothing here sorts on it.
    #[test]
    fn image_size_is_carried_as_text_and_never_as_an_order() {
        let bigger = r#"{"ID":"a","Repository":"app","Tag":"1","Size":"1.0GB"}"#;
        let smaller = r#"{"ID":"b","Repository":"app","Tag":"2","Size":"999MB"}"#;
        let big = parse_json_lines::<DockerImage>(bigger)
            .pop()
            .expect("one row")
            .into_resource();
        let small = parse_json_lines::<DockerImage>(smaller)
            .pop()
            .expect("one row")
            .into_resource();
        let big_size = big
            .detail
            .iter()
            .find(|(k, _)| *k == "Size")
            .map(|(_, v)| v.as_str())
            .expect("size is shown");
        let small_size = small
            .detail
            .iter()
            .find(|(k, _)| *k == "Size")
            .map(|(_, v)| v.as_str())
            .expect("size is shown");
        assert!(
            small_size > big_size,
            "this is the trap, asserted so nobody 'fixes' it by sorting on the \
             string: {small_size:?} compares greater than {big_size:?}, which is \
             the wrong order. Any real ordering needs bytes, which this output \
             does not carry."
        );
    }

    /// A dangling image has no name to show, so its id has to stand in -- an
    /// empty cell is a row nobody can act on.
    #[test]
    fn a_dangling_image_falls_back_to_its_id() {
        let stdout = r#"{"ID":"deadbeef","Repository":"<none>","Tag":"<none>"}"#;
        let image = parse_json_lines::<DockerImage>(stdout)
            .pop()
            .expect("one row")
            .into_resource();
        assert!(image.name.contains("deadbeef"));
        assert!(
            image
                .detail
                .iter()
                .any(|(k, v)| *k == "Dangling" && v == "yes"),
            "and it must be marked, because that is what makes it prunable"
        );
        assert_eq!(
            image.state,
            RunState::Unknown,
            "an image was never running, so 'stopped' would be a false statement"
        );
    }

    #[test]
    fn an_untagged_but_named_image_shows_just_its_repository() {
        let stdout = r#"{"ID":"a","Repository":"app","Tag":"<none>"}"#;
        let image = parse_json_lines::<DockerImage>(stdout)
            .pop()
            .expect("one row")
            .into_resource();
        assert_eq!(image.name, "app");
    }

    /// A volume's name *is* its handle. If id and name ever drift apart, `act`
    /// addresses something the row does not name.
    #[test]
    fn a_volume_is_addressed_by_its_name() {
        let stdout = r#"{"Name":"pgdata","Driver":"local","Mountpoint":"/var/lib/docker/volumes/pgdata/_data"}"#;
        let volume = parse_json_lines::<DockerVolume>(stdout)
            .pop()
            .expect("one row")
            .into_resource();
        assert_eq!(volume.kind, ResourceKind::Volume);
        assert_eq!(volume.id, volume.name);
        assert_eq!(volume.id, "pgdata");
    }

    #[test]
    fn a_network_keeps_its_id_apart_from_its_name() {
        let stdout = r#"{"ID":"abc123","Name":"bridge","Driver":"bridge","Scope":"local"}"#;
        let network = parse_json_lines::<DockerNetwork>(stdout)
            .pop()
            .expect("one row")
            .into_resource();
        assert_eq!(network.id, "abc123");
        assert_eq!(network.name, "bridge");
    }

    /// `docker compose ls --format json` answers with an array, not one object
    /// per line -- the one command here that does. Reading it with the stream
    /// reader would silently produce one row instead of three.
    #[test]
    fn compose_projects_come_back_as_an_array() {
        let stdout = r#"[{"Name":"shop","Status":"running(3)","ConfigFiles":"/p/compose.yaml"},{"Name":"old","Status":"exited(2)","ConfigFiles":"/q/compose.yaml"}]"#;
        let projects: Vec<ComposeProject> =
            serde_json::from_str(stdout).expect("an array of objects");
        assert_eq!(projects.len(), 2);
        let resources: Vec<_> = projects
            .into_iter()
            .map(|project| project.into_resource())
            .collect();
        assert_eq!(resources[0].state, RunState::Running, "running(3) is up");
        assert_eq!(resources[1].state, RunState::Stopped, "exited(2) is not");
        assert_eq!(resources[0].id, "shop");
    }

    #[test]
    fn no_compose_projects_reads_as_an_empty_list() {
        let projects: Vec<ComposeProject> = serde_json::from_str("[]").expect("valid");
        assert!(projects.is_empty());
    }

    /// Every declared kind must actually answer against real Docker. A kind in
    /// `supported_kinds` that errors is a group the tree draws and cannot fill.
    #[test]
    #[ignore = "drives whatever engine the machine has; see `engine tests` in the module docs"]
    fn real_docker_answers_for_every_kind_it_declares() {
        let backend = DockerBackend::docker();
        let mut answered = 0;
        for kind in backend.supported_kinds() {
            match block_on(backend.list(*kind)) {
                Ok(_) => answered += 1,
                Err(ContainerError::EngineMissing { .. })
                | Err(ContainerError::EngineNotRunning { .. }) => return,
                Err(error) => panic!(
                    "docker declares {kind:?} but cannot list it: {error} -- the \
                     tree would draw a group it can never fill"
                ),
            }
        }
        assert_eq!(
            answered,
            backend.supported_kinds().len(),
            "every declared kind answered"
        );
    }
}

/// Streaming JSON out of a child process, in both the shapes the two engines
/// print.
mod event_streaming {
    use crate::event_stream::json_values;
    use futures::stream::StreamExt as _;
    use serde::Deserialize;
    use util::command::Command;

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct Marked {
        n: u32,
    }

    fn printer(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    /// One object per line: the shape `docker events --format '{{json .}}'`
    /// prints.
    #[test]
    fn one_object_per_line_arrives_as_separate_values() {
        let stream = json_values::<Marked>(printer(r#"printf '{"n":1}\n{"n":2}\n{"n":3}\n'"#))
            .expect("sh is on every platform this ships to");
        let values: Vec<Marked> = smol::block_on(stream.collect());
        assert_eq!(
            values,
            vec![Marked { n: 1 }, Marked { n: 2 }, Marked { n: 3 }]
        );
    }

    /// One object over several lines: the shape `kubectl get --watch -o json`
    /// prints. A reader that split on newlines would find nothing here, which is
    /// the bug this reader exists to avoid.
    #[test]
    fn a_value_split_over_several_lines_still_arrives() {
        let stream = json_values::<Marked>(printer(r#"printf '{\n  "n": 7\n}\n{\n  "n": 8\n}\n'"#))
            .expect("sh");
        let values: Vec<Marked> = smol::block_on(stream.collect());
        assert_eq!(values, vec![Marked { n: 7 }, Marked { n: 8 }]);
    }

    /// Values with no separator at all, back to back.
    #[test]
    fn values_with_no_separator_arrive_separately() {
        let stream = json_values::<Marked>(printer(r#"printf '{"n":1}{"n":2}'"#)).expect("sh");
        let values: Vec<Marked> = smol::block_on(stream.collect());
        assert_eq!(values, vec![Marked { n: 1 }, Marked { n: 2 }]);
    }

    /// A malformed value must not end the stream -- one bad event cannot be
    /// allowed to stop the panel hearing about the next one.
    #[test]
    fn a_malformed_value_does_not_end_the_stream() {
        let stream =
            json_values::<Marked>(printer(r#"printf '{"n":1}\nnot json at all\n{"n":2}\n'"#))
                .expect("sh");
        let values: Vec<Marked> = smol::block_on(stream.collect());
        assert!(
            values.contains(&Marked { n: 1 }),
            "the value before the bad one must survive; got {values:?}"
        );
    }

    #[test]
    fn a_program_that_is_not_there_yields_no_stream_rather_than_panicking() {
        assert!(
            json_values::<Marked>(Command::new("zode-no-such-streamer")).is_none(),
            "an absent engine means no stream, not a crash"
        );
    }

    /// Dropping the stream must take the process with it.
    ///
    /// A `docker events` that outlives its panel is one leaked process per open,
    /// and nothing later would notice. Checked by asking the OS, not by trusting
    /// the code: the marker file stops growing once the process is gone.
    #[test]
    fn dropping_the_stream_kills_the_process() {
        let dir =
            std::env::temp_dir().join(format!("zode-container-stream-{}", std::process::id()));
        let marker = dir.join("ticks");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let _ = std::fs::remove_file(&marker);

        // Writes to *both*: stdout so the stream has something to hand back,
        // and the file so the test can ask the filesystem whether the process is
        // still alive after the stream is gone. An earlier version wrote only to
        // the file and hung forever waiting for a value that was never printed.
        let script = format!(
            "while true; do printf '{{\"n\":1}}\\n'; printf 'x' >> {}; sleep 0.05; done",
            marker.display()
        );
        let stream = json_values::<Marked>(printer(&script)).expect("sh");

        // Read one value, so the process is certainly running and writing.
        let mut stream = Box::pin(stream);
        let first = smol::block_on(stream.next());
        assert_eq!(first, Some(Marked { n: 1 }));
        drop(stream);

        // Let anything already in flight land, then take a reading and compare.
        std::thread::sleep(std::time::Duration::from_millis(300));
        let after_drop = std::fs::metadata(&marker).map(|m| m.len()).unwrap_or(0);
        std::thread::sleep(std::time::Duration::from_millis(400));
        let later = std::fs::metadata(&marker).map(|m| m.len()).unwrap_or(0);

        let _ = std::fs::remove_file(&marker);
        assert_eq!(
            later, after_drop,
            "the file kept growing after the stream was dropped, so the process \
             outlived it -- that is one leaked process per column open"
        );
    }
}

/// Podman: Docker's command line plus the one word Docker has no equivalent for.
mod podman {
    use super::block_on;
    use crate::backend::{BackendKind, ContainerBackend, ContainerError};
    use crate::docker_backend::DockerBackend;
    use crate::podman::PodmanPod;
    use crate::resource::{ResourceAction, ResourceKind, RunState};

    /// Podman lists pods; Docker does not. This is the difference the whole
    /// per-engine `supported_kinds` exists for.
    #[test]
    fn only_podman_lists_pods() {
        let docker = DockerBackend::docker();
        let podman = DockerBackend::podman();
        assert!(!docker.supported_kinds().contains(&ResourceKind::Pod));
        assert!(podman.supported_kinds().contains(&ResourceKind::Pod));
        assert_eq!(podman.kind(), BackendKind::Podman);
    }

    /// A podman pod starts and stops; a Kubernetes pod does neither. Same word,
    /// different object -- which is why actions are asked per backend and not
    /// decided by kind.
    #[test]
    fn a_podman_pod_starts_and_stops_but_does_not_restart() {
        let podman = DockerBackend::podman();
        let actions = podman.supported_actions(ResourceKind::Pod);
        assert!(actions.contains(&ResourceAction::Start));
        assert!(actions.contains(&ResourceAction::Stop));
        assert!(
            !actions.contains(&ResourceAction::Restart),
            "`podman pod restart` exists in recent versions and not in older \
             ones; a button that works on one machine and errors on another is \
             worse than no button"
        );
        assert!(
            DockerBackend::docker()
                .supported_actions(ResourceKind::Pod)
                .is_empty(),
            "and docker offers nothing for a kind it does not have"
        );
    }

    /// An action outside `supported_actions` must be refused, not attempted.
    #[test]
    fn acting_outside_the_declared_actions_is_refused() {
        let docker = DockerBackend::docker();
        let error = block_on(docker.act(ResourceKind::Image, ResourceAction::Start, "abc"))
            .expect_err("an image cannot be started");
        assert!(
            matches!(error, ContainerError::CommandFailed { .. }),
            "got {error:?}"
        );
        // And crucially it must not have run anything: a refusal that shells out
        // first is a refusal that can still do damage.
        assert!(
            error.to_string().contains("cannot be start"),
            "the refusal must be the crate's own words, not the engine's: {error}"
        );
    }

    #[test]
    fn a_degraded_pod_gets_no_dot_at_all() {
        let json = r#"[{"Id":"abc","Name":"web","Status":"Degraded","Containers":[{},{}]}]"#;
        let pods: Vec<PodmanPod> = serde_json::from_str(json).expect("valid");
        let resource = pods.into_iter().next().expect("one").into_resource();
        assert_eq!(
            resource.state,
            RunState::Unknown,
            "some containers up and some down: neither dot is true"
        );
        assert!(
            resource
                .detail
                .iter()
                .any(|(k, v)| *k == "Containers" && v == "2")
        );
    }

    #[test]
    fn a_running_pod_reads_running_whatever_the_case() {
        for status in ["Running", "running"] {
            let json = format!(r#"[{{"Id":"a","Name":"n","Status":"{status}"}}]"#);
            let pods: Vec<PodmanPod> = serde_json::from_str(&json).expect("valid");
            let resource = pods.into_iter().next().expect("one").into_resource();
            assert_eq!(resource.state, RunState::Running, "for {status:?}");
        }
    }

    /// Podman is not installed on the machine this was written on, so this is the
    /// state a developer here actually sees. It must be "not installed", not a
    /// generic failure.
    #[test]
    fn an_absent_podman_says_so_plainly() {
        let podman = DockerBackend::podman();
        match block_on(podman.list(ResourceKind::Container)) {
            Err(ContainerError::EngineMissing { program }) => assert_eq!(program, "podman"),
            // A machine that has podman gets a list, which is also fine.
            Ok(_) => {}
            Err(other) => panic!("an absent podman must read as 'not installed', got {other:?}"),
        }
    }
}

/// The commands that open a terminal, and the one rule they must never break.
mod terminal_commands {
    use crate::ContainerBackend;
    use crate::docker_backend::DockerBackend;
    use crate::kubernetes_backend::KubernetesBackend;
    use crate::resource::ResourceKind;

    /// **The most dangerous surface in this crate.** A container name is chosen
    /// by whoever created it, and these commands run with the user's own
    /// privileges. The id must arrive as one argument, never as text a shell
    /// could split.
    #[test]
    fn a_hostile_name_stays_one_argument() {
        let hostile = "evil; rm -rf /tmp/nothing";
        for backend in [DockerBackend::docker(), DockerBackend::podman()] {
            let (program, args) = backend
                .logs_command(ResourceKind::Container, hostile)
                .expect("a container has a log");
            assert!(!program.contains(';'), "the program is never the payload");
            assert!(
                args.iter().any(|arg| arg == hostile),
                "the name must survive whole, as exactly one argument: {args:?}"
            );
            assert!(
                args.iter().filter(|arg| arg.contains("rm -rf")).count() == 1,
                "and must not have been split into two: {args:?}"
            );
            assert!(
                args.iter().any(|arg| arg == "--"),
                "and `--` must precede it, or a name starting with a dash reads \
                 as a flag: {args:?}"
            );
        }
    }

    /// Only things that have a log get a log command. An image has never run.
    #[test]
    fn only_things_with_a_log_offer_one() {
        let docker = DockerBackend::docker();
        assert!(docker.logs_command(ResourceKind::Container, "a").is_some());
        assert!(docker.logs_command(ResourceKind::Image, "a").is_none());
        assert!(docker.logs_command(ResourceKind::Volume, "a").is_none());
        assert!(docker.logs_command(ResourceKind::Network, "a").is_none());
    }

    /// Only a running container can be entered. A pod is not a process, and
    /// Docker has no pods anyway.
    #[test]
    fn only_a_container_offers_a_shell_on_docker() {
        let docker = DockerBackend::docker();
        assert!(docker.exec_command(ResourceKind::Container, "a").is_some());
        assert!(docker.exec_command(ResourceKind::Image, "a").is_none());
        assert!(docker.exec_command(ResourceKind::Pod, "a").is_none());
    }

    /// A podman pod has a log of its own; a docker "pod" does not exist.
    #[test]
    fn a_podman_pod_has_a_log_and_a_docker_one_does_not() {
        assert!(
            DockerBackend::podman()
                .logs_command(ResourceKind::Pod, "web")
                .is_some()
        );
        assert!(
            DockerBackend::docker()
                .logs_command(ResourceKind::Pod, "web")
                .is_none()
        );
    }

    /// The namespaced id must be split back apart, or `kubectl` looks for a pod
    /// literally called `production/web`.
    #[test]
    fn a_namespaced_pod_id_is_split_into_flag_and_name() {
        let backend = KubernetesBackend::new();
        let (program, args) = backend
            .logs_command(ResourceKind::Pod, "production/web-1")
            .expect("a pod has a log");
        assert_eq!(program, "kubectl");
        assert!(
            args.windows(2).any(|pair| pair == ["-n", "production"]),
            "the namespace must become a flag: {args:?}"
        );
        assert!(
            args.iter().any(|arg| arg == "web-1"),
            "and the bare name must be the argument: {args:?}"
        );
        assert!(
            !args.iter().any(|arg| arg.contains('/')),
            "nothing may still carry the joined form: {args:?}"
        );
    }

    /// An id with no namespace is a name, not a broken id.
    #[test]
    fn an_unnamespaced_pod_id_passes_through_without_a_namespace_flag() {
        let (_, args) = KubernetesBackend::new()
            .logs_command(ResourceKind::Pod, "solo")
            .expect("a pod has a log");
        assert!(
            !args.iter().any(|arg| arg == "-n"),
            "no namespace was given, so none is claimed: {args:?}"
        );
        assert!(args.iter().any(|arg| arg == "solo"));
    }

    /// A pod's shell goes through `--`, or `kubectl` reads `sh` as one of its own
    /// arguments.
    #[test]
    fn a_pod_shell_is_separated_from_kubectls_own_arguments() {
        let (_, args) = KubernetesBackend::new()
            .exec_command(ResourceKind::Pod, "ns/pod")
            .expect("a pod can be entered");
        let separator = args
            .iter()
            .position(|arg| arg == "--")
            .expect("a separator");
        let shell = args.iter().position(|arg| arg == "sh").expect("a shell");
        assert!(
            separator < shell,
            "the shell must come after `--`: {args:?}"
        );
    }
}

/// Removing things: the last phase, and the only one that can lose data.
mod destructive {
    use super::block_on;
    use crate::destructive::{DestructivePlan, Intent, PruneScope};
    use crate::docker_backend::DockerBackend;
    use crate::fake_backend::FakeBackend;
    use crate::resource::{Resource, ResourceKind, RunState};
    use crate::{ContainerBackend, ContainerError};

    fn thing(name: &str, kind: ResourceKind) -> Resource {
        Resource {
            kind,
            id: format!("id-{name}"),
            name: name.to_string(),
            state: RunState::Stopped,
            detail: Vec::new(),
            parent: None,
        }
    }

    /// A plan cannot exist without the list of what will be lost. That is the
    /// whole design: no enumeration, no plan, no dialog, nothing removed.
    #[test]
    fn a_plan_cannot_be_built_without_targets() {
        assert!(
            DestructivePlan::remove(ResourceKind::Container, Vec::new()).is_none(),
            "nothing to remove is nothing to confirm"
        );
        assert!(
            DestructivePlan::prune(PruneScope::Reclaimable, Vec::new()).is_none(),
            "a prune with nothing to prune must not open a dialog"
        );
    }

    /// One target: the name must be typed, because typing it is what makes
    /// somebody read *which* one they are removing.
    #[test]
    fn removing_one_thing_asks_for_its_name() {
        let plan = DestructivePlan::remove(
            ResourceKind::Container,
            vec![thing("zode-postgres", ResourceKind::Container)],
        )
        .expect("one target");
        assert_eq!(plan.confirmation(), "zode-postgres");
        assert!(!plan.is_confirmed_by("zode-postgre"), "a typo is not a yes");
        assert!(
            !plan.is_confirmed_by("ZODE-POSTGRES"),
            "nor is the wrong case"
        );
        assert!(
            !plan.is_confirmed_by("yes"),
            "and nor is agreement in general"
        );
        assert!(plan.is_confirmed_by("zode-postgres"));
        assert!(
            plan.is_confirmed_by("  zode-postgres  "),
            "a trailing space is a typing artefact, not a different answer"
        );
    }

    /// A volume holds data, so removing one is warned about even though the
    /// command is the same shape as any other.
    #[test]
    fn removing_a_volume_carries_a_warning_that_removing_a_network_does_not() {
        let volume = DestructivePlan::remove(
            ResourceKind::Volume,
            vec![thing("pgdata", ResourceKind::Volume)],
        )
        .expect("one");
        let network = DestructivePlan::remove(
            ResourceKind::Network,
            vec![thing("bridge", ResourceKind::Network)],
        )
        .expect("one");
        assert!(volume.warning().is_some(), "a volume holds data");
        assert!(network.warning().is_none(), "a network holds none");
    }

    /// **The most important assertion in this crate.** `--volumes` must be off
    /// unless it was asked for, and warned about when it was: a stopped
    /// database's volume counts as unused, which is how people lose databases.
    #[test]
    fn pruning_volumes_is_opt_in_and_warned_about() {
        let targets = vec![thing("dangling", ResourceKind::Image)];
        let default =
            DestructivePlan::prune(PruneScope::default(), targets.clone()).expect("one target");
        assert_eq!(
            default.intent(),
            Intent::Prune {
                scope: PruneScope::Reclaimable
            },
            "the default scope must never include volumes"
        );
        assert!(
            default.warning().is_none(),
            "and with no volumes there is nothing to warn about"
        );

        let with_volumes =
            DestructivePlan::prune(PruneScope::IncludingVolumes, targets).expect("one target");
        let warning = with_volumes
            .warning()
            .expect("including volumes must be warned about");
        assert!(
            warning.contains("volume"),
            "the warning must name what is at risk: {warning}"
        );
        assert!(
            with_volumes.is_confirmed_by("prune"),
            "and the word itself is what is typed, alongside the enumerated list"
        );
    }

    /// The plan carries the actual rows, so the dialog can show what the panel
    /// showed rather than a count or a description.
    #[test]
    fn a_plan_carries_the_rows_themselves() {
        let plan = DestructivePlan::remove(
            ResourceKind::Image,
            vec![
                thing("a", ResourceKind::Image),
                thing("b", ResourceKind::Image),
            ],
        )
        .expect("two");
        assert_eq!(plan.targets().len(), 2);
        assert_eq!(plan.targets()[1].name, "b");
        assert_eq!(
            plan.confirmation(),
            "2",
            "several at once: one name would be misleading about the rest"
        );
    }

    /// `destroy` takes a plan, not a kind and an id. There is no way to remove
    /// something that was never enumerated.
    #[test]
    fn destroy_only_accepts_a_plan() {
        let backend = FakeBackend::docker();
        let plan = DestructivePlan::remove(
            ResourceKind::Container,
            vec![thing("x", ResourceKind::Container)],
        )
        .expect("one");
        block_on(backend.destroy(&plan)).expect("the fake accepts it");
        assert_eq!(
            backend.destroyed().len(),
            1,
            "and the plan is what reached the engine"
        );
    }

    /// Removing a container must not force. "Remove" was not "kill", and a
    /// running container refusing is the engine telling the truth.
    #[test]
    #[ignore = "drives whatever engine the machine has; see `engine tests` in the module docs"]
    fn removing_a_running_container_is_not_forced() {
        let backend = DockerBackend::docker();
        let plan = DestructivePlan::remove(
            ResourceKind::Container,
            vec![thing("definitely-not-here", ResourceKind::Container)],
        )
        .expect("one");
        match block_on(backend.destroy(&plan)) {
            // No such container: the engine's own refusal, which is the point.
            Err(ContainerError::CommandFailed { stderr, .. }) => assert!(
                !stderr.contains("--force"),
                "nothing here may have added a force flag: {stderr}"
            ),
            Err(ContainerError::EngineMissing { .. })
            | Err(ContainerError::EngineNotRunning { .. }) => {}
            Err(other) => panic!("unexpected {other:?}"),
            Ok(()) => panic!("a container that does not exist cannot have been removed"),
        }
    }

    /// Kubernetes has no prune, and says so by offering none rather than by
    /// pretending to have nothing to prune.
    #[test]
    fn kubernetes_offers_no_prune_at_all() {
        let backend = crate::kubernetes_backend::KubernetesBackend::new();
        assert!(
            block_on(backend.prune_targets(PruneScope::Reclaimable)).is_none(),
            "`None` means the notion does not exist here -- an empty list would \
             mean it does and is empty"
        );
    }

    /// Docker's prune list is gathered from real commands, and it must never
    /// include volumes unless volumes were asked for.
    #[test]
    #[ignore = "drives whatever engine the machine has; see `engine tests` in the module docs"]
    fn dockers_prune_list_only_includes_volumes_when_asked() {
        let backend = DockerBackend::docker();
        let reclaimable =
            block_on(backend.prune_targets(PruneScope::Reclaimable)).expect("docker has a prune");
        let Ok(reclaimable) = reclaimable else {
            eprintln!("docker is not usable here; nothing to compare");
            return;
        };
        assert!(
            !reclaimable
                .iter()
                .any(|target| target.kind == ResourceKind::Volume),
            "the default scope must not list a single volume"
        );
    }
}
