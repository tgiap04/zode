//! "Switch Branch..." from the open container's view, against a real git
//! repository and the in-process fake engine.
//!
//! Git is real on purpose: what these prove is that a refusal from git reaches
//! an error dialog and stops the restarts, and a fake git could only prove the
//! panel does what the fake told it to. Every wait is on the panel's own task,
//! taken and awaited, rather than a sleep that guesses how long git takes.

use std::path::Path;
use std::sync::Arc;

use container::fake_backend::FakeBackend;
use container::{
    ComposeLabels, ContainerBackend, Resource, ResourceAction, ResourceKind, RunState,
};
use gpui::{Entity, TestAppContext, VisualTestContext};
use workspace::Workspace;

use crate::branch_switch::SwitchStage;
use crate::branch_switch_modals::{BranchSwitchError, ConfirmBranchSwitch};
use crate::branch_switch_picker::BranchPicker;
use crate::compose_checkout::fixture::{Fixture, fixture, run};
use crate::compose_checkout::{Checkout, CheckoutState, GitFailure};
use crate::container_panel::ContainerPanel;
use crate::container_panel_tests::{init_test, tab_in_workspace};

fn compose_container(id: &str, state: RunState, directory: &Path, project: &str) -> Resource {
    Resource {
        kind: ResourceKind::Container,
        id: id.into(),
        name: format!("{id}-name"),
        state,
        detail: vec![("Image", "app:latest".into())],
        parent: None,
        compose: Some(ComposeLabels {
            project: project.into(),
            service: id.into(),
            working_dir: Some(directory.to_string_lossy().into_owned()),
        }),
    }
}

fn plain_container(id: &str) -> Resource {
    Resource {
        compose: None,
        ..compose_container(id, RunState::Running, Path::new("/"), "")
    }
}

struct Harness {
    workspace: Entity<Workspace>,
    panel: Entity<ContainerPanel>,
    backend: Arc<FakeBackend>,
}

async fn harness(
    cx: &mut TestAppContext,
    backend: FakeBackend,
) -> (Harness, &mut VisualTestContext) {
    // Real git children wake the scheduler from another thread.
    cx.executor().allow_parking();
    init_test(cx);
    let fs = fs::FakeFs::new(cx.executor());
    let project = project::Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |multi, _| multi.workspace().clone());
    let backend = Arc::new(backend);
    let engine: Arc<dyn ContainerBackend> = backend.clone();
    let panel = tab_in_workspace(&workspace, vec![engine], cx);
    panel.update(cx, |panel, cx| panel.reload(cx));
    cx.run_until_parked();
    (
        Harness {
            workspace,
            panel,
            backend,
        },
        cx,
    )
}

impl Harness {
    async fn open(&self, id: &str, cx: &mut VisualTestContext) {
        let resource = self
            .panel
            .read_with(cx, |panel, _| match &panel.state {
                crate::container_panel::ListState::Ready(listing) => {
                    listing.iter().find(|resource| resource.id == id).cloned()
                }
                _ => None,
            })
            .expect("the container is listed");
        self.panel.update_in(cx, |panel, window, cx| {
            panel.open_detail(resource, window, cx)
        });
        self.settle_probe(cx).await;
    }

    /// Waits for the git commands that read the checkout. Twice, because a
    /// reload that lands in between starts a second read.
    async fn settle_probe(&self, cx: &mut VisualTestContext) {
        for _ in 0..2 {
            cx.run_until_parked();
            let task = self
                .panel
                .update(cx, |panel, _| panel.checkout_probe.take());
            if let Some(task) = task {
                task.await;
            }
        }
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    async fn open_picker(&self, cx: &mut VisualTestContext) -> Entity<BranchPicker> {
        self.panel
            .update_in(cx, |panel, window, cx| panel.open_branch_picker(window, cx));
        cx.run_until_parked();
        let picker = self
            .workspace
            .read_with(cx, |workspace, cx| {
                workspace.active_modal::<BranchPicker>(cx)
            })
            .expect("the picker is open");
        let load = picker.update(cx, |picker, _| picker.take_load());
        load.await;
        cx.run_until_parked();
        picker
    }

    async fn choose(&self, title: &str, cx: &mut VisualTestContext) -> Entity<ConfirmBranchSwitch> {
        let picker = self.open_picker(cx).await;
        self.choose_from(&picker, title, cx).await
    }

    async fn choose_from(
        &self,
        picker: &Entity<BranchPicker>,
        title: &str,
        cx: &mut VisualTestContext,
    ) -> Entity<ConfirmBranchSwitch> {
        picker.update_in(cx, |picker, window, cx| picker.choose(title, window, cx));
        let prep = self
            .panel
            .update(cx, |panel, _| panel.confirmation_prep.take());
        prep.expect("choosing starts reading the checkout").await;
        cx.run_until_parked();
        self.workspace
            .read_with(cx, |workspace, cx| {
                workspace.active_modal::<ConfirmBranchSwitch>(cx)
            })
            .expect("the confirmation is open")
    }

    async fn confirm(&self, modal: &Entity<ConfirmBranchSwitch>, cx: &mut VisualTestContext) {
        modal.update_in(cx, |modal, window, cx| modal.confirm(window, cx));
        let task = self.panel.update(cx, |panel, _| panel.switch_task.take());
        task.expect("confirming starts the switch").await;
        self.settle_probe(cx).await;
    }

    fn error_modal(&self, cx: &VisualTestContext) -> Option<Entity<BranchSwitchError>> {
        self.workspace.read_with(cx, |workspace, cx| {
            workspace.active_modal::<BranchSwitchError>(cx)
        })
    }
}

fn project_of(fixture: &Fixture, containers: &[(&str, RunState)]) -> Vec<Resource> {
    containers
        .iter()
        .map(|(id, state)| compose_container(id, *state, &fixture.work, "app"))
        .collect()
}

#[gpui::test]
async fn the_button_is_shown_only_for_a_compose_container_in_a_git_checkout(
    cx: &mut TestAppContext,
) {
    let fixture = fixture();
    let not_a_checkout = tempfile::tempdir().expect("tempdir");
    let mut containers = project_of(&fixture, &[("web", RunState::Running)]);
    containers.push(compose_container(
        "loose",
        RunState::Running,
        not_a_checkout.path(),
        "other",
    ));
    containers.push(plain_container("plain"));
    let (harness, cx) = harness(cx, FakeBackend::docker().holding(containers)).await;

    harness.open("web", cx).await;
    assert!(cx.debug_bounds("container-switch-branch").is_some());
    assert!(cx.debug_bounds("container-checkout-state").is_some());

    harness.open("loose", cx).await;
    assert!(
        cx.debug_bounds("container-switch-branch").is_none(),
        "a compose directory that is not a git checkout gets no button"
    );

    harness.open("plain", cx).await;
    assert!(
        cx.debug_bounds("container-switch-branch").is_none(),
        "a container without compose labels gets no button"
    );
    harness.panel.read_with(cx, |panel, _| {
        assert!(panel.checkout_probe.is_none(), "and no git is run for it");
    });
}

#[gpui::test]
async fn a_panel_without_a_workspace_offers_no_button(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    init_test(cx);
    let fixture = fixture();
    let backend: Arc<dyn ContainerBackend> = Arc::new(
        FakeBackend::docker().holding(project_of(&fixture, &[("web", RunState::Running)])),
    );
    let (panel, cx) = cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
    panel.update(cx, |panel, cx| panel.reload(cx));
    cx.run_until_parked();
    let resource = panel.read_with(cx, |panel, _| match &panel.state {
        crate::container_panel::ListState::Ready(listing) => listing[0].clone(),
        _ => panic!("not loaded"),
    });
    panel.update_in(cx, |panel, window, cx| {
        panel.open_detail(resource, window, cx)
    });
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert!(cx.debug_bounds("container-switch-branch").is_none());
}

#[gpui::test]
async fn the_state_reads_as_the_branch_the_checkout_is_on(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (harness, cx) = harness(
        cx,
        FakeBackend::docker().holding(project_of(&fixture, &[("web", RunState::Running)])),
    )
    .await;
    harness.open("web", cx).await;
    harness.panel.read_with(cx, |panel, _| {
        let checkout = panel.detail.as_ref().and_then(|d| d.checkout.as_ref());
        assert_eq!(checkout.map(|c| c.state.label()), Some("main".to_string()));
    });
}

/// The deleted-branch case, through the panel: the files stay, the state says
/// nothing names the commit any more, and nothing else is disturbed.
#[gpui::test]
async fn a_deleted_branch_reads_as_no_branch_points_here(cx: &mut TestAppContext) {
    let fixture = fixture();
    run(&fixture.work, &["switch", "--detach", "origin/fix.x"]);
    run(&fixture.origin, &["branch", "-D", "fix.x"]);
    run(&fixture.work, &["fetch", "origin", "--prune"]);

    let (harness, cx) = harness(
        cx,
        FakeBackend::docker().holding(project_of(&fixture, &[("web", RunState::Running)])),
    )
    .await;
    harness.open("web", cx).await;
    harness.panel.read_with(cx, |panel, _| {
        let state = &panel
            .detail
            .as_ref()
            .and_then(|d| d.checkout.as_ref())
            .expect("a checkout")
            .state;
        assert!(state.points_nowhere(), "got {state:?}");
        assert!(state.label().contains("no branch points here (deleted?)"));
    });
    assert!(cx.debug_bounds("container-switch-branch").is_some());
    assert!(
        harness.backend.acted().is_empty(),
        "nothing is restarted by looking"
    );
}

#[gpui::test]
async fn the_picker_lists_origin_branches_without_head_and_other_worktrees(
    cx: &mut TestAppContext,
) {
    let fixture = fixture();
    let other = fixture.work.parent().expect("parent").join("other");
    run(
        &fixture.work,
        &[
            "worktree",
            "add",
            "--detach",
            other.to_str().expect("utf8"),
            "origin/fix.x",
        ],
    );
    let (harness, cx) = harness(
        cx,
        FakeBackend::docker().holding(project_of(&fixture, &[("web", RunState::Running)])),
    )
    .await;
    harness.open("web", cx).await;

    let picker = harness.open_picker(cx).await;
    let titles = picker.read_with(cx, |picker, cx| picker.listed_titles(cx));
    assert_eq!(titles.len(), 3, "{titles:?}");
    assert_eq!(&titles[..2], ["origin/fix.x", "origin/main"]);
    assert!(titles[2].starts_with("other ("), "{titles:?}");
    assert!(titles.iter().all(|title| title != "origin/HEAD"));
    assert_eq!(
        picker.read_with(cx, |picker, cx| picker.status_text(cx)),
        None
    );

    picker.update_in(cx, |picker, window, cx| picker.search("fix", window, cx));
    cx.run_until_parked();
    assert_eq!(
        picker.read_with(cx, |picker, cx| picker.listed_titles(cx)),
        vec!["origin/fix.x".to_string()]
    );
}

#[gpui::test]
async fn the_confirmation_ticks_running_containers_of_this_project_and_notes_carried_changes(
    cx: &mut TestAppContext,
) {
    let fixture = fixture();
    let mut containers = project_of(
        &fixture,
        &[
            ("web", RunState::Running),
            ("db", RunState::Running),
            ("job", RunState::Stopped),
        ],
    );
    containers.push(compose_container(
        "stranger",
        RunState::Running,
        Path::new("/somewhere/else"),
        "app",
    ));
    std::fs::write(fixture.work.join("mine.txt"), "wip").expect("write");
    let (harness, cx) = harness(cx, FakeBackend::docker().holding(containers)).await;
    harness.open("web", cx).await;

    let confirmation = harness.choose("origin/fix.x", cx).await;
    confirmation.read_with(cx, |modal, _| {
        assert_eq!(
            modal.restart_ids(),
            vec!["web".to_string(), "db".to_string()]
        );
        let note = modal.carried_note().expect("one change is uncommitted");
        assert!(
            note.starts_with("1 uncommitted change will be carried over"),
            "{note}"
        );
        assert_eq!(modal.worktree_note(), None);
        assert_eq!(modal.orphan_note(), None, "HEAD is on a branch");
    });
    cx.update(|window, _| window.refresh());
    assert!(cx.debug_bounds("confirm-branch-switch-carried").is_some());
}

#[gpui::test]
async fn choosing_a_worktree_says_its_uncommitted_changes_are_not_carried(cx: &mut TestAppContext) {
    let fixture = fixture();
    let other = fixture.work.parent().expect("parent").join("other");
    run(
        &fixture.work,
        &[
            "worktree",
            "add",
            "--detach",
            other.to_str().expect("utf8"),
            "origin/fix.x",
        ],
    );
    let (harness, cx) = harness(
        cx,
        FakeBackend::docker().holding(project_of(&fixture, &[("web", RunState::Running)])),
    )
    .await;
    harness.open("web", cx).await;

    let picker = harness.open_picker(cx).await;
    let title = picker
        .read_with(cx, |picker, cx| picker.listed_titles(cx))
        .into_iter()
        .find(|title| title.starts_with("other"))
        .expect("the worktree is offered");
    let confirmation = harness.choose_from(&picker, &title, cx).await;
    confirmation.read_with(cx, |modal, _| {
        assert!(
            modal
                .worktree_note()
                .is_some_and(|note| note.contains("not carried over"))
        );
        assert_eq!(
            modal.carried_note(),
            None,
            "a clean checkout carries nothing"
        );
    });
}

#[gpui::test]
async fn confirming_restarts_only_the_ticked_containers_after_the_checkout_moves(
    cx: &mut TestAppContext,
) {
    let fixture = fixture();
    let (harness, cx) = harness(
        cx,
        FakeBackend::docker().holding(project_of(
            &fixture,
            &[("web", RunState::Running), ("db", RunState::Running)],
        )),
    )
    .await;
    harness.open("web", cx).await;

    let confirmation = harness.choose("origin/fix.x", cx).await;
    // Unticks the database.
    confirmation.update(cx, |modal, cx| modal.toggle(1, cx));
    harness.confirm(&confirmation, cx).await;

    assert_eq!(
        harness.backend.acted(),
        vec![(ResourceAction::Restart, "web".to_string())]
    );
    assert_eq!(
        std::fs::read_to_string(fixture.work.join("a.txt")).expect("read"),
        "two",
        "the checkout moved"
    );
    harness.panel.read_with(cx, |panel, _| {
        assert_eq!(panel.branch_switch, None);
        let label = panel
            .detail
            .as_ref()
            .and_then(|detail| detail.checkout.as_ref())
            .map(|checkout| checkout.state.label())
            .expect("re-read after the switch");
        assert!(label.ends_with("origin/fix.x"), "{label}");
    });
    assert!(harness.error_modal(cx).is_none());
}

#[gpui::test]
async fn with_nothing_ticked_only_the_checkout_changes(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (harness, cx) = harness(
        cx,
        FakeBackend::docker().holding(project_of(&fixture, &[("web", RunState::Running)])),
    )
    .await;
    harness.open("web", cx).await;
    let confirmation = harness.choose("origin/fix.x", cx).await;
    confirmation.update(cx, |modal, cx| modal.toggle(0, cx));
    cx.update(|window, _| window.refresh());
    harness.confirm(&confirmation, cx).await;

    assert!(harness.backend.acted().is_empty());
    assert_eq!(
        std::fs::read_to_string(fixture.work.join("a.txt")).expect("read"),
        "two"
    );
}

#[gpui::test]
async fn a_conflicting_local_change_shows_git_s_refusal_and_restarts_nothing(
    cx: &mut TestAppContext,
) {
    let fixture = fixture();
    let (harness, cx) = harness(
        cx,
        FakeBackend::docker().holding(project_of(
            &fixture,
            &[("web", RunState::Running), ("db", RunState::Running)],
        )),
    )
    .await;
    harness.open("web", cx).await;
    // Same file the target changes, so git cannot carry it over.
    std::fs::write(fixture.work.join("a.txt"), "local").expect("write");

    let confirmation = harness.choose("origin/fix.x", cx).await;
    harness.confirm(&confirmation, cx).await;

    let error = harness.error_modal(cx).expect("an error dialog is shown");
    error.read_with(cx, |error, _| {
        assert_eq!(error.title(), "Could not switch to origin/fix.x");
        assert!(error.message().contains("a.txt"), "{}", error.message());
    });
    assert!(
        harness.backend.acted().is_empty(),
        "no restart after a refusal"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.work.join("a.txt")).expect("read"),
        "local",
        "the local change is untouched"
    );
    harness.panel.read_with(cx, |panel, _| {
        assert_eq!(panel.branch_switch, None, "the panel is usable again");
    });
}

#[gpui::test]
async fn a_failed_restart_is_reported_after_the_checkout_has_moved(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (harness, cx) = harness(
        cx,
        FakeBackend::docker()
            .holding(project_of(
                &fixture,
                &[("web", RunState::Running), ("db", RunState::Running)],
            ))
            .refusing_actions_on("db"),
    )
    .await;
    harness.open("web", cx).await;
    let confirmation = harness.choose("origin/fix.x", cx).await;
    harness.confirm(&confirmation, cx).await;

    let error = harness.error_modal(cx).expect("an error dialog is shown");
    error.read_with(cx, |error, _| {
        assert!(
            error.message().contains("moved to origin/fix.x"),
            "{}",
            error.message()
        );
        assert!(error.message().contains("db-name"), "{}", error.message());
        assert!(error.message().contains("refused"), "{}", error.message());
    });
    assert_eq!(
        harness.backend.acted(),
        vec![(ResourceAction::Restart, "web".to_string())],
        "the container that could be restarted was"
    );
}

#[gpui::test]
async fn while_switching_the_row_says_so_and_the_button_is_gone(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (harness, cx) = harness(
        cx,
        FakeBackend::docker().holding(project_of(&fixture, &[("web", RunState::Running)])),
    )
    .await;
    harness.open("web", cx).await;
    assert!(cx.debug_bounds("container-switch-branch").is_some());

    for stage in [SwitchStage::Switching, SwitchStage::Restarting] {
        harness.panel.update(cx, |panel, cx| {
            panel.branch_switch = Some(stage);
            cx.notify();
        });
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("container-switch-busy").is_some(),
            "{stage:?}"
        );
        assert!(
            cx.debug_bounds("container-switch-branch").is_none(),
            "the action is disabled while {stage:?}"
        );
    }
    assert_eq!(SwitchStage::Switching.label(), "Switching\u{2026}");
    assert_eq!(SwitchStage::Restarting.label(), "Restarting\u{2026}");

    harness.panel.update_in(cx, |panel, window, cx| {
        panel.open_branch_picker(window, cx);
    });
    cx.run_until_parked();
    assert!(
        harness
            .workspace
            .read_with(cx, |workspace, cx| workspace
                .active_modal::<BranchPicker>(cx))
            .is_none(),
        "no second switch can start while one is running"
    );
}

#[gpui::test]
async fn a_second_error_is_shown_instead_of_swallowed(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (harness, cx) = harness(
        cx,
        FakeBackend::docker().holding(project_of(&fixture, &[("web", RunState::Running)])),
    )
    .await;
    for message in ["first", "second"] {
        harness.panel.update_in(cx, |panel, window, cx| {
            panel.show_switch_error("Failed".into(), message.into(), window, cx)
        });
        cx.run_until_parked();
        let error = harness.error_modal(cx).expect("an error dialog is shown");
        assert_eq!(
            error.read_with(cx, |error, _| error.message().to_string()),
            message
        );
    }
}

#[gpui::test]
async fn a_failed_or_unclear_probe_keeps_the_row_but_a_real_non_checkout_clears_it(
    cx: &mut TestAppContext,
) {
    let fixture = fixture();
    let (harness, cx) = harness(
        cx,
        FakeBackend::docker().holding(project_of(&fixture, &[("web", RunState::Running)])),
    )
    .await;
    harness.open("web", cx).await;
    let has_checkout = |cx: &mut VisualTestContext| {
        harness.panel.read_with(cx, |panel, _| {
            panel.detail.as_ref().is_some_and(|d| d.checkout.is_some())
        })
    };
    let apply = |outcome: Result<Option<Checkout>, GitFailure>, cx: &mut VisualTestContext| {
        harness.panel.update(cx, |panel, cx| {
            panel.apply_checkout_probe("web", outcome, cx)
        })
    };
    assert!(has_checkout(cx));

    apply(
        Err(GitFailure {
            step: "inspect",
            message: "index.lock exists".into(),
            conflicting_files: Vec::new(),
        }),
        cx,
    );
    assert!(
        has_checkout(cx),
        "a transient git error must not blank the row"
    );

    harness.panel.update(cx, |panel, _| {
        panel.branch_switch = Some(SwitchStage::Switching)
    });
    apply(Ok(None), cx);
    assert!(has_checkout(cx), "nor may an unclear answer mid-switch");

    harness
        .panel
        .update(cx, |panel, _| panel.branch_switch = None);
    apply(Ok(None), cx);
    assert!(
        !has_checkout(cx),
        "git saying it is not a repository clears it"
    );

    apply(
        Ok(Some(Checkout {
            root: fixture.work.clone(),
            state: CheckoutState::Branch("main".into()),
        })),
        cx,
    );
    assert!(has_checkout(cx));
}

#[gpui::test]
async fn leaving_a_detached_head_no_branch_holds_warns_about_the_reflog(cx: &mut TestAppContext) {
    let fixture = fixture();
    run(&fixture.work, &["switch", "--detach", "origin/fix.x"]);
    run(&fixture.work, &["commit", "--allow-empty", "-m", "loose"]);
    let loose = run(&fixture.work, &["rev-parse", "--short", "HEAD"]);
    let (harness, cx) = harness(
        cx,
        FakeBackend::docker().holding(project_of(&fixture, &[("web", RunState::Running)])),
    )
    .await;
    harness.open("web", cx).await;

    let confirmation = harness.choose("origin/main", cx).await;
    let note = confirmation
        .read_with(cx, |modal, _| modal.orphan_note())
        .expect("a warning");
    assert_eq!(
        note,
        format!(
            "Commits at {loose} are on no branch; switching away leaves them reachable only \
             through the reflog."
        )
    );
    cx.update(|window, _| window.refresh());
    assert!(cx.debug_bounds("confirm-branch-switch-orphan").is_some());
}

#[gpui::test]
async fn the_picker_names_the_checkout_and_the_remote_it_used(cx: &mut TestAppContext) {
    let fixture = fixture();
    run(&fixture.work, &["remote", "rename", "origin", "upstream"]);
    let (harness, cx) = harness(
        cx,
        FakeBackend::docker().holding(project_of(&fixture, &[("web", RunState::Running)])),
    )
    .await;
    harness.open("web", cx).await;
    let picker = harness.open_picker(cx).await;
    cx.update(|window, _| window.refresh());
    assert!(cx.debug_bounds("container-branch-picker-root").is_some());
    assert_eq!(
        picker.read_with(cx, |picker, cx| picker.listed_titles(cx)),
        vec!["upstream/fix.x".to_string(), "upstream/main".to_string()]
    );

    // A fetch that cannot happen says which remote it tried.
    std::fs::remove_dir_all(&fixture.origin).expect("remove the remote");
    harness.workspace.update_in(cx, |workspace, window, cx| {
        workspace.hide_modal(window, cx);
    });
    let picker = harness.open_picker(cx).await;
    let status = picker
        .read_with(cx, |picker, cx| picker.status_text(cx))
        .expect("a warning");
    assert!(status.starts_with("Could not fetch upstream"), "{status}");
}

#[gpui::test]
async fn a_repository_with_no_remote_offers_worktrees_only_and_says_so(cx: &mut TestAppContext) {
    let fixture = fixture();
    run(&fixture.work, &["remote", "remove", "origin"]);
    let (harness, cx) = harness(
        cx,
        FakeBackend::docker().holding(project_of(&fixture, &[("web", RunState::Running)])),
    )
    .await;
    harness.open("web", cx).await;
    let picker = harness.open_picker(cx).await;
    assert!(
        picker
            .read_with(cx, |picker, cx| picker.listed_titles(cx))
            .is_empty()
    );
    let status = picker
        .read_with(cx, |picker, cx| picker.status_text(cx))
        .expect("a note");
    assert!(status.contains("no remote"), "{status}");
}

#[gpui::test]
async fn no_confirmation_opens_while_a_switch_is_running(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (harness, cx) = harness(
        cx,
        FakeBackend::docker().holding(project_of(&fixture, &[("web", RunState::Running)])),
    )
    .await;
    harness.open("web", cx).await;
    harness.panel.update_in(cx, |panel, window, cx| {
        panel.branch_switch = Some(SwitchStage::Switching);
        panel.open_switch_confirmation(
            crate::compose_checkout::Target::remote_branch("origin", "fix.x"),
            window,
            cx,
        );
        assert!(panel.confirmation_prep.is_none());
    });
}

#[gpui::test]
async fn a_switch_with_no_engine_says_so_instead_of_returning_silently(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    init_test(cx);
    let fs = fs::FakeFs::new(cx.executor());
    let project = project::Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |multi, _| multi.workspace().clone());
    let panel = tab_in_workspace(&workspace, Vec::new(), cx);
    panel.update_in(cx, |panel, window, cx| {
        panel.start_branch_switch(
            crate::branch_switch::SwitchRequest {
                root: std::path::PathBuf::from("/"),
                target: crate::compose_checkout::Target::remote_branch("origin", "x"),
                restart: Vec::new(),
            },
            window,
            cx,
        )
    });
    cx.run_until_parked();
    assert!(
        workspace
            .read_with(cx, |workspace, cx| workspace
                .active_modal::<BranchSwitchError>(cx))
            .is_some()
    );
}
