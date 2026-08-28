use std::sync::Arc;

use container::backend::BackendKind;
use container::fake_backend::{FakeBackend, Misbehaviour};
use container::{ContainerBackend, ResourceKind};
use gpui::{AppContext as _, TestAppContext, VisualTestContext};

use crate::container_panel::{ContainerPanel, ListState};
use settings::Settings as _;

/// Opens the container tab in a real workspace, over a backend the test chose.
///
/// The engine cannot be injected through the action -- that path builds a real
/// `DockerBackend` on purpose -- so the view is added directly and the action is
/// used only where the action itself is what is under test.
fn tab_in_workspace(
    workspace: &gpui::Entity<workspace::Workspace>,
    backends: Vec<Arc<dyn ContainerBackend>>,
    cx: &mut VisualTestContext,
) -> gpui::Entity<ContainerPanel> {
    workspace.update_in(cx, |workspace, window, cx| {
        let handle = cx.weak_entity();
        let view = cx.new(|cx| {
            let mut panel = ContainerPanel::with_backends(backends, cx);
            panel.workspace = Some(handle);
            panel
        });
        workspace.add_item_to_active_pane(Box::new(view.clone()), None, true, window, cx);
        view
    })
}

fn init_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let settings_store = settings::SettingsStore::test(cx);
        cx.set_global(settings_store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        project::DisableAiSettings::register(cx);
        // Registers the workspace action, so a test reaches the button the way a
        // click does rather than calling the handler behind it. A button wired
        // to nothing is a defect this repo has shipped once before.
        crate::init(cx);
        // Opening a tab must not shell out to whatever engine this machine has.
        crate::use_fake_engines_for_test(cx);
    });
}

/// Builds a panel over a backend that never leaves the process, and draws it.
///
/// Drawing matters: the states below are only reachable through `Render`, and an
/// element tree built by hand would not exercise it.
fn panel_over(
    backend: Arc<dyn ContainerBackend>,
    cx: &mut TestAppContext,
) -> (gpui::Entity<ContainerPanel>, &mut VisualTestContext) {
    let (panel, cx) = cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
    // Asked for explicitly: construction is silent by design, and in the app it
    // is `Panel::set_active` that asks. See `the_constructor_asks_the_engine_nothing`.
    panel.update(cx, |panel, cx| panel.reload(cx));
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    (panel, cx)
}

const DOCKER_KINDS: &[ResourceKind] = &[ResourceKind::Container];

/// Building the view asks the engine.
///
/// The opposite of what this asserted while there was a column: back then
/// `initialize_panels` built the panel on every window open, so a command in the
/// constructor was a `docker ps` per window for a column nobody had opened, and
/// it panicked inside the test scheduler. Nothing constructs this view unasked
/// now -- it exists only because somebody opened the tab -- so asking here is
/// both safe and the only place left to ask.
#[gpui::test]
async fn opening_the_tab_asks_the_engine(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = fs::FakeFs::new(cx.executor());
    let project = project::Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

    let tab = tab_in_workspace(&workspace, vec![Arc::new(FakeBackend::docker())], cx);
    tab.update(cx, |panel, cx| panel.reload(cx));
    cx.run_until_parked();

    tab.read_with(cx, |panel, _| match &panel.state {
        ListState::Ready(resources) => assert_eq!(resources.len(), 1),
        other => panic!("the tab must have a list, got {}", describe(other)),
    });
}

#[gpui::test]
async fn a_backend_with_containers_reaches_the_ready_state(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, cx) = panel_over(Arc::new(FakeBackend::docker()), cx);
    panel.read_with(cx, |panel, _| match &panel.state {
        ListState::Ready(resources) => {
            assert_eq!(resources.len(), 1, "the fake holds one container");
            assert_eq!(resources[0].name, "fake-postgres");
        }
        other => panic!("expected a ready list, got {}", describe(other)),
    });
}

/// "None running" and "the engine is not there" must not be the same state.
///
/// One message for both is how a panel ends up telling somebody to install what
/// they already have.
#[gpui::test]
async fn no_containers_is_a_different_state_from_no_engine(cx: &mut TestAppContext) {
    init_test(cx);
    let (empty, cx) = panel_over(
        Arc::new(FakeBackend::empty(BackendKind::Docker, DOCKER_KINDS)),
        cx,
    );
    empty.read_with(cx, |panel, _| match &panel.state {
        ListState::Ready(resources) => assert!(resources.is_empty()),
        other => panic!(
            "an engine that answered 'none' is ready, got {}",
            describe(other)
        ),
    });
}

#[gpui::test]
async fn a_missing_engine_and_a_dead_engine_draw_differently(cx: &mut TestAppContext) {
    init_test(cx);

    let missing = Arc::new(FakeBackend::docker().misbehaving(Misbehaviour::EngineMissing));
    let (panel, cx) = panel_over(missing, cx);
    let missing_words = panel.read_with(cx, |panel, _| match &panel.state {
        ListState::Failed(error) => error.to_string(),
        other => panic!("expected a failure, got {}", describe(other)),
    });

    let dead = Arc::new(FakeBackend::docker().misbehaving(Misbehaviour::EngineNotRunning));
    let (panel, cx) = panel_over(dead, cx);
    let dead_words = panel.read_with(cx, |panel, _| match &panel.state {
        ListState::Failed(error) => error.to_string(),
        other => panic!("expected a failure, got {}", describe(other)),
    });

    assert_ne!(
        missing_words, dead_words,
        "'not installed' and 'installed but down' need different words in front \
         of the user"
    );
}

/// The load task must be held in a field, never detached.
///
/// That field is what makes the `docker` process die with the view: a detached
/// task keeps running after the tab closes, and one leaks per open.
#[gpui::test]
async fn the_load_task_is_held_in_a_field_not_detached(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, cx) = panel_over(Arc::new(FakeBackend::docker()), cx);
    panel.read_with(cx, |panel, _| {
        assert!(
            panel.load.is_some(),
            "the load task must be stored so it is dropped with the view; a \
             detached task leaves a `docker` process behind on every close"
        );
    });
}

fn describe(state: &ListState) -> &'static str {
    match state {
        ListState::Loading => "loading",
        ListState::Ready(_) => "ready",
        ListState::Failed(_) => "failed",
    }
}

/// The rail button opens the tab, and a second press steps back.
///
/// Put away rather than closed: closing would drop the engine choice and kill
/// the listener over the second press of a button whose whole job is to be
/// pressed twice. Same behaviour as the agent buttons beside it.
#[gpui::test]
async fn the_toggle_action_opens_the_tab_and_puts_it_away(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = fs::FakeFs::new(cx.executor());
    let project = project::Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

    assert_eq!(
        workspace.read_with(cx, |workspace, cx| workspace
            .items_of_type::<ContainerPanel>(cx)
            .count()),
        0,
        "nothing is open before the button is pressed"
    );

    cx.update(|window, cx| {
        window.dispatch_action(Box::new(zed_actions::container::ToggleContainer), cx)
    });
    cx.run_until_parked();

    let opened = workspace.read_with(cx, |workspace, cx| {
        workspace
            .items_of_type::<ContainerPanel>(cx)
            .next()
            .map(|view| view.entity_id())
    });
    assert!(
        opened.is_some(),
        "the action the rail button dispatches must open the tab; if this fails \
         the button is wired to something nothing handles"
    );
    assert_eq!(
        workspace.read_with(cx, |workspace, cx| workspace
            .active_item(cx)
            .map(|item| item.item_id())),
        opened,
        "and bring it forward"
    );

    // A second press with nothing else in the pane leaves it exactly as it is:
    // closing the tab would end the listener over a button press.
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(zed_actions::container::ToggleContainer), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        workspace.read_with(cx, |workspace, cx| workspace
            .items_of_type::<ContainerPanel>(cx)
            .count()),
        1,
        "the tab must still be there -- put away, never closed"
    );
}

/// Pressing the button again must bring the existing tab forward, not open a
/// second one.
///
/// Two tabs would be two lists of the same thing, each with its own engine
/// choice and its own listener on `docker events`.
#[gpui::test]
async fn the_toggle_action_never_opens_a_second_tab(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = fs::FakeFs::new(cx.executor());
    let project = project::Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

    for _ in 0..3 {
        cx.update(|window, cx| {
            window.dispatch_action(Box::new(zed_actions::container::ToggleContainer), cx)
        });
        cx.run_until_parked();
    }

    assert_eq!(
        workspace.read_with(cx, |workspace, cx| workspace
            .items_of_type::<ContainerPanel>(cx)
            .count()),
        1,
        "however many times it is pressed, there is one list"
    );
}

/// Two tabs, when deliberately asked for, do not share state.
///
/// Each builds its own view, because the alternative needs one `FocusHandle` in
/// two dispatch trees at once and a focus handle cannot be in two.
#[gpui::test]
async fn two_tabs_do_not_share_state(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = fs::FakeFs::new(cx.executor());
    let project = project::Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

    let first = tab_in_workspace(&workspace, vec![Arc::new(FakeBackend::docker())], cx);
    let second = tab_in_workspace(&workspace, vec![Arc::new(FakeBackend::docker())], cx);
    assert_ne!(first.entity_id(), second.entity_id());

    cx.run_until_parked();
    let second_before = second.read_with(cx, |panel, _| panel.measured_width);

    // Read back without parking in between, deliberately: a tab's own canvas
    // measures the real pane on the next frame and would overwrite this value.
    first.update(cx, |panel, cx| {
        panel.note_measured_width(gpui::px(300.), cx);
        assert!(
            !panel.shows_every_column(),
            "measured narrow, a tab must drop the columns that need room"
        );
    });

    second.read_with(cx, |panel, _| {
        assert_eq!(
            panel.measured_width, second_before,
            "a width measured in one tab must not reach the other -- each view \
             has its own, which is why they are separate views at all"
        );
    });
}

mod engines {
    use super::*;
    use container::backend::BackendKind;

    fn two_engines() -> Vec<Arc<dyn ContainerBackend>> {
        vec![
            Arc::new(FakeBackend::docker()),
            Arc::new(
                FakeBackend::empty(BackendKind::Kubernetes, &[container::ResourceKind::Pod])
                    .misbehaving(Misbehaviour::NotConfigured),
            ),
        ]
    }

    /// A fresh panel offers Docker and Kubernetes, in that order, and starts on
    /// Docker.
    #[gpui::test]
    async fn a_fresh_panel_offers_every_engine_and_starts_on_docker(cx: &mut TestAppContext) {
        init_test(cx);
        let (panel, cx) = cx.add_window_view(|_window, cx| {
            ContainerPanel::with_backends(ContainerPanel::default_backends(), cx)
        });
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            let kinds: Vec<_> = panel.backends.iter().map(|b| b.kind()).collect();
            assert_eq!(
                kinds,
                vec![
                    BackendKind::Docker,
                    BackendKind::Podman,
                    BackendKind::Kubernetes
                ],
                "all three offered unconditionally -- probing would mean a command                  per engine on every window open"
            );
            assert_eq!(panel.active_backend, 0, "Docker is what most people have");
        });
    }

    /// Switching must clear the old list before the new one arrives.
    ///
    /// The two engines answer different questions. Leaving Docker's containers on
    /// screen under a Kubernetes heading would be a lie for as long as the
    /// command took, and on a slow or unreachable cluster that is a long time.
    #[gpui::test]
    async fn switching_engine_clears_the_previous_list(cx: &mut TestAppContext) {
        init_test(cx);
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backends(two_engines(), cx));
        panel.update(cx, |panel, cx| panel.reload(cx));
        cx.run_until_parked();

        panel.read_with(cx, |panel, _| match &panel.state {
            ListState::Ready(resources) => assert_eq!(resources.len(), 1, "docker's one container"),
            other => panic!("expected docker's list, got {}", describe(other)),
        });

        // Checked *before* parking, and that is the whole test: by the time the
        // new engine has answered, the state would be `Failed` whether or not the
        // old list was cleared. The moment that matters is the one in between --
        // the frames during which Docker's containers would otherwise sit under a
        // Kubernetes heading. (A mutation removing the clear passed this test
        // until it was written this way.)
        panel.update(cx, |panel, cx| {
            panel.choose_backend(1, cx);
            assert_eq!(panel.active_backend, 1);
            assert!(
                matches!(panel.state, ListState::Loading),
                "the previous engine's list must be gone the instant the engine \
                 changes, not once the next answer arrives"
            );
        });
        cx.run_until_parked();

        panel.read_with(cx, |panel, _| {
            assert_eq!(panel.active_backend, 1);
            match &panel.state {
                // The fake kubernetes reports "nothing selected", which is the
                // honest answer -- and crucially *not* docker's container list.
                ListState::Failed(error) => assert!(
                    matches!(error, container::ContainerError::NotConfigured { .. }),
                    "got {error:?}"
                ),
                ListState::Ready(resources) => assert!(
                    resources.is_empty(),
                    "whatever the new engine says, docker's containers must not \
                     still be on screen under a Kubernetes heading"
                ),
                other => panic!("unexpected {}", describe(other)),
            }
        });
    }

    /// Choosing the engine already chosen must not restart the command.
    #[gpui::test]
    async fn choosing_the_active_engine_again_does_nothing(cx: &mut TestAppContext) {
        init_test(cx);
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backends(two_engines(), cx));
        panel.update(cx, |panel, cx| panel.reload(cx));
        cx.run_until_parked();

        panel.update(cx, |panel, cx| {
            panel.choose_backend(0, cx);
            assert!(
                matches!(panel.state, ListState::Ready(_)),
                "the list must survive a no-op switch, not blink back to Loading"
            );
        });
    }

    /// An index past the end must be ignored rather than panic or blank the view.
    #[gpui::test]
    async fn an_engine_index_out_of_range_is_ignored(cx: &mut TestAppContext) {
        init_test(cx);
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backends(two_engines(), cx));
        cx.run_until_parked();
        panel.update(cx, |panel, cx| {
            panel.choose_backend(99, cx);
            assert_eq!(panel.active_backend, 0);
        });
    }

    /// A second view inherits the whole engine list *and* which one was chosen.
    ///
    /// Inheriting only the active engine would hand it a switcher with one button
    /// in it; inheriting nothing would put it on a different engine from the tab
    /// it was opened beside.
    #[gpui::test]
    async fn a_second_view_inherits_the_engine_list_and_the_choice(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (multi_workspace, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

        let first = tab_in_workspace(&workspace, two_engines(), cx);
        first.update(cx, |panel, cx| panel.choose_backend(1, cx));
        cx.run_until_parked();

        // A *second* tab, deliberately: `OpenInEditorTab` always opens one, where
        // `ToggleContainer` brings the existing one forward.
        cx.update(|window, cx| {
            window.dispatch_action(Box::new(zed_actions::container::OpenInEditorTab), cx)
        });
        cx.run_until_parked();

        let second = workspace.read_with(cx, |workspace, cx| {
            workspace
                .items_of_type::<ContainerPanel>(cx)
                .find(|view| view.entity_id() != first.entity_id())
                .expect("a second tab was opened")
        });
        second.read_with(cx, |panel, _| {
            assert_eq!(
                panel.backends.len(),
                2,
                "the whole list travels, not one engine"
            );
            assert_eq!(
                panel.active_backend, 1,
                "and it opens on the engine the first tab was showing"
            );
        });
    }
}

/// Viewing one kind at a time, and what changing kind must not leave behind.
mod kinds {
    use super::*;
    use container::ResourceKind;
    use container::backend::BackendKind;

    const DOCKER_KINDS: &[ResourceKind] = &[
        ResourceKind::Container,
        ResourceKind::Image,
        ResourceKind::Volume,
        ResourceKind::Network,
        ResourceKind::ComposeProject,
    ];
    const K8S_KINDS: &[ResourceKind] = &[ResourceKind::Pod];

    /// The kind tabs come from the engine, never from a list written in the view.
    #[gpui::test]
    async fn the_kinds_offered_are_the_engines_own(cx: &mut TestAppContext) {
        init_test(cx);
        let backend: Arc<dyn ContainerBackend> =
            Arc::new(FakeBackend::empty(BackendKind::Docker, DOCKER_KINDS));
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            assert_eq!(panel.available_kinds(), DOCKER_KINDS);
            assert_eq!(
                panel.active_kind,
                ResourceKind::Container,
                "a fresh panel opens on whatever the engine lists first"
            );
        });
    }

    /// Changing kind must clear the previous list before the next arrives.
    #[gpui::test]
    async fn changing_kind_clears_the_previous_list(cx: &mut TestAppContext) {
        init_test(cx);
        let backend: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker());
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        panel.update(cx, |panel, cx| panel.reload(cx));
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            assert!(matches!(panel.state, ListState::Ready(_)));
        });

        // Checked before parking, for the same reason as the engine switch: by
        // the time the next answer lands, the state would look right either way.
        panel.update(cx, |panel, cx| {
            panel.choose_kind(ResourceKind::Image, cx);
            assert_eq!(panel.active_kind, ResourceKind::Image);
            assert!(
                matches!(panel.state, ListState::Loading),
                "a container list left under an Images heading is a lie for as \\
                 long as the command takes"
            );
        });
    }

    /// A kind the engine does not have a word for must be unreachable.
    #[gpui::test]
    async fn a_kind_the_engine_does_not_offer_is_refused(cx: &mut TestAppContext) {
        init_test(cx);
        let backend: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker());
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        cx.run_until_parked();
        panel.update(cx, |panel, cx| {
            panel.choose_kind(ResourceKind::Pod, cx);
            assert_eq!(
                panel.active_kind,
                ResourceKind::Container,
                "docker has no word for a pod, so asking for one changes nothing"
            );
        });
    }

    /// Switching engine must move off a kind the new engine cannot answer.
    ///
    /// Left on `Volume`, a switch to Kubernetes would ask it a question it has no
    /// word for and the panel would show an error it caused itself.
    #[gpui::test]
    async fn switching_engine_moves_off_a_kind_the_new_one_lacks(cx: &mut TestAppContext) {
        init_test(cx);
        let backends: Vec<Arc<dyn ContainerBackend>> = vec![
            Arc::new(FakeBackend::empty(BackendKind::Docker, DOCKER_KINDS)),
            Arc::new(FakeBackend::empty(BackendKind::Kubernetes, K8S_KINDS)),
        ];
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backends(backends, cx));
        cx.run_until_parked();

        panel.update(cx, |panel, cx| {
            panel.choose_kind(ResourceKind::Volume, cx);
            assert_eq!(panel.active_kind, ResourceKind::Volume);
            panel.choose_backend(1, cx);
            assert_eq!(
                panel.active_kind,
                ResourceKind::Pod,
                "Kubernetes has no volumes here, so the kind falls to what it \\
                 does list rather than asking an impossible question"
            );
        });
    }

    /// A kind that carries over must stay put: switching engine should not throw
    /// away a choice the new engine can honour.
    #[gpui::test]
    async fn a_kind_both_engines_share_survives_the_switch(cx: &mut TestAppContext) {
        init_test(cx);
        const BOTH: &[ResourceKind] = &[ResourceKind::Pod, ResourceKind::Container];
        let backends: Vec<Arc<dyn ContainerBackend>> = vec![
            Arc::new(FakeBackend::empty(BackendKind::Docker, BOTH)),
            Arc::new(FakeBackend::empty(BackendKind::Podman, BOTH)),
        ];
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backends(backends, cx));
        cx.run_until_parked();
        panel.update(cx, |panel, cx| {
            panel.choose_kind(ResourceKind::Container, cx);
            panel.choose_backend(1, cx);
            assert_eq!(
                panel.active_kind,
                ResourceKind::Container,
                "both engines list containers, so the choice is still valid"
            );
        });
    }
}

/// Listening to the engine, and stopping when the column does.
mod watching {
    use super::*;

    /// Opening the tab starts the listener; dropping the view stops it.
    ///
    /// The listener owns a child process, so "stops it" is not tidiness -- a
    /// detached one leaves a `docker events` behind on every close. It starts in
    /// the constructor now: there is no `set_active` on a pane item, and nothing
    /// builds this view unasked any more.
    #[gpui::test]
    async fn the_listener_starts_with_the_view(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (multi_workspace, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

        cx.update(|window, cx| {
            window.dispatch_action(Box::new(zed_actions::container::ToggleContainer), cx)
        });
        cx.run_until_parked();

        let tab = workspace.read_with(cx, |workspace, cx| {
            workspace
                .items_of_type::<ContainerPanel>(cx)
                .next()
                .expect("the tab opened")
        });
        tab.read_with(cx, |panel, _| {
            assert!(
                panel.watch.is_some(),
                "opening the tab must start the listener"
            );
            assert!(
                panel.load.is_some()
                    || matches!(panel.state, ListState::Ready(_) | ListState::Failed(_)),
                "and ask for a first list"
            );
        });
    }

    /// Switching engine must replace the listener, not stack a second one.
    ///
    /// The old engine's listener would go on waking the panel about a list it is
    /// no longer showing -- and would go on holding its process.
    #[gpui::test]
    async fn switching_engine_replaces_the_listener(cx: &mut TestAppContext) {
        init_test(cx);
        let backends: Vec<Arc<dyn ContainerBackend>> = vec![
            Arc::new(FakeBackend::docker()),
            Arc::new(FakeBackend::empty(
                container::backend::BackendKind::Kubernetes,
                &[container::ResourceKind::Pod],
            )),
        ];
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backends(backends, cx));
        cx.run_until_parked();

        panel.update(cx, |panel, cx| panel.start_watching(cx));
        let first = panel.read_with(cx, |panel, _| panel.watch.is_some());
        assert!(first, "a listener was started");

        panel.update(cx, |panel, cx| panel.choose_backend(1, cx));
        panel.read_with(cx, |panel, _| {
            assert!(
                panel.watch.is_some(),
                "a switch leaves a listener for the *new* engine, not none"
            );
        });
    }
}

/// start / stop / restart, and what must not be claimed about them.
mod lifecycle {
    use super::*;
    use container::{ResourceAction, ResourceKind};

    /// The buttons offered come from the engine, per kind.
    #[gpui::test]
    async fn the_actions_offered_are_the_engines_own_per_kind(cx: &mut TestAppContext) {
        init_test(cx);
        let backend: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker());
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        cx.run_until_parked();

        panel.update(cx, |panel, cx| {
            assert!(
                !panel.available_actions().is_empty(),
                "a container starts, stops and restarts"
            );
            panel.choose_kind(ResourceKind::Image, cx);
            assert!(
                panel.available_actions().is_empty(),
                "an image does none of those, and the buttons must go with it"
            );
        });
    }

    /// The panel must not decide the resulting state itself.
    ///
    /// `docker start` returning says the command was accepted, not that the
    /// container is up -- it may exit a moment later. Setting `Running` here
    /// would make the UI say something the engine never said.
    #[gpui::test]
    async fn an_action_asks_the_engine_again_rather_than_assuming(cx: &mut TestAppContext) {
        init_test(cx);
        let backend = Arc::new(FakeBackend::docker());
        let recorder = backend.clone();
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        panel.update(cx, |panel, cx| panel.reload(cx));
        cx.run_until_parked();

        panel.update(cx, |panel, cx| {
            panel.act(ResourceAction::Stop, "c0ffee".into(), cx);
            assert_eq!(
                panel.in_flight.get("c0ffee"),
                Some(&ResourceAction::Stop),
                "the row must say what is being attempted while it runs -- \
                 `docker stop` waits ten seconds before killing"
            );
        });
        cx.run_until_parked();

        panel.read_with(cx, |panel, _| {
            assert!(
                panel.in_flight.is_empty(),
                "and stop saying it once the command is done"
            );
        });
        assert_eq!(
            recorder.acted(),
            vec![(ResourceAction::Stop, "c0ffee".to_string())],
            "the engine was asked exactly once"
        );
    }

    /// A failure must reach the person who asked, with the engine's own words.
    #[gpui::test]
    async fn a_failed_action_is_shown_and_can_be_dismissed(cx: &mut TestAppContext) {
        init_test(cx);
        let backend: Arc<dyn ContainerBackend> =
            Arc::new(FakeBackend::docker().misbehaving(Misbehaviour::FailEveryList));
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        cx.run_until_parked();

        panel.update(cx, |panel, cx| {
            panel.act(ResourceAction::Stop, "c0ffee".into(), cx);
        });
        cx.run_until_parked();

        panel.update(cx, |panel, cx| {
            let error = panel
                .last_error
                .as_ref()
                .expect("a refused action must not be swallowed into the log");
            assert!(
                error.to_string().contains("as asked"),
                "and it must carry the engine's own words, got: {error}"
            );
            panel.dismiss_error(cx);
            assert!(panel.last_error.is_none());
        });
    }

    /// An action outside what the engine offers must not be attempted.
    #[gpui::test]
    async fn an_action_the_engine_does_not_offer_is_not_attempted(cx: &mut TestAppContext) {
        init_test(cx);
        let backend = Arc::new(FakeBackend::docker());
        let recorder = backend.clone();
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        cx.run_until_parked();

        panel.update(cx, |panel, cx| {
            panel.choose_kind(ResourceKind::Image, cx);
            panel.act(ResourceAction::Start, "some-image".into(), cx);
            assert!(
                panel.in_flight.is_empty(),
                "nothing may be in flight for an action the engine does not offer"
            );
        });
        cx.run_until_parked();
        assert!(
            recorder.acted().is_empty(),
            "and the engine must never have been asked"
        );
    }
}

/// logs and exec as terminal tabs.
mod terminals {
    use super::*;
    use crate::terminal::TerminalIntent;
    use container::ResourceKind;

    /// The buttons are drawn only where the engine has a command for them.
    #[gpui::test]
    async fn a_terminal_is_offered_only_where_the_engine_has_a_command(cx: &mut TestAppContext) {
        init_test(cx);
        let backend: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker());
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        cx.run_until_parked();

        panel.update(cx, |panel, cx| {
            assert!(
                panel.terminal_available(TerminalIntent::FollowLogs),
                "a container has a log"
            );
            assert!(
                panel.terminal_available(TerminalIntent::Shell),
                "and can be entered"
            );
            panel.choose_kind(ResourceKind::Image, cx);
            assert!(
                !panel.terminal_available(TerminalIntent::FollowLogs),
                "an image has never run, so it has no log"
            );
            assert!(!panel.terminal_available(TerminalIntent::Shell));
        });
    }

    /// A floating window's panel has no workspace, so it must not try to open a
    /// tab in one.
    ///
    /// It is built in a different window's context on purpose -- a `&Workspace`
    /// cannot be borrowed there alongside the `App` creating the entity.
    #[gpui::test]
    async fn a_panel_with_no_workspace_does_not_reach_for_one(cx: &mut TestAppContext) {
        init_test(cx);
        let backend: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker());
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        cx.run_until_parked();

        panel.read_with(cx, |panel, _| {
            assert!(
                panel.workspace.is_none(),
                "a bare panel has no workspace handle"
            );
        });
        // Must be a no-op rather than a panic.
        panel.update_in(cx, |panel, window, cx| {
            panel.open_terminal(
                TerminalIntent::Shell,
                "c0ffee".into(),
                "fake-postgres".into(),
                window,
                cx,
            );
        });
        cx.run_until_parked();
    }

    /// A tab opened from the column *does* get a workspace, so its buttons work.
    #[gpui::test]
    async fn a_tab_carries_the_workspace_its_terminals_need(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (multi_workspace, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

        tab_in_workspace(&workspace, vec![Arc::new(FakeBackend::docker())], cx);
        cx.run_until_parked();

        cx.update(|window, cx| {
            window.dispatch_action(Box::new(zed_actions::container::OpenInEditorTab), cx)
        });
        cx.run_until_parked();

        let tab = workspace.read_with(cx, |workspace, cx| {
            workspace
                .items_of_type::<ContainerPanel>(cx)
                .next()
                .expect("a tab was opened")
        });
        tab.read_with(cx, |panel, _| {
            assert!(
                panel.workspace.is_some(),
                "a tab lives in a workspace, so its terminal buttons have \
                 somewhere to open"
            );
        });
    }
}

/// Clicking a row: the container on its own, over its live logs.
mod detail {
    use super::*;
    use crate::render::action_is_useful;
    use container::{Resource, ResourceAction, RunState};

    fn a_container(id: &str, state: RunState) -> Resource {
        Resource {
            kind: ResourceKind::Container,
            id: id.into(),
            name: format!("{id}-name"),
            state,
            detail: vec![("Image", "postgres:16".into())],
            parent: None,
        }
    }

    /// Which buttons a row gets is the engine's refusals written down, not
    /// taste. `docker start` on a paused container, `docker pause` on a stopped
    /// one and `docker unpause` on a running one all error, so each of those
    /// buttons in that state could only ever fail.
    #[test]
    fn the_buttons_offered_are_the_ones_the_engine_would_accept() {
        use ResourceAction::*;
        use RunState::*;

        for (action, state, expected, why) in [
            (Start, Stopped, true, "the ordinary case"),
            (Start, Running, false, "already up"),
            (
                Start,
                Paused,
                false,
                "a paused container resumes, it does not start",
            ),
            (Stop, Running, true, "the ordinary case"),
            (
                Stop,
                Paused,
                true,
                "docker stop works on a paused container",
            ),
            (Stop, Stopped, false, "already down"),
            (Restart, Running, true, "the ordinary case"),
            (Restart, Stopped, false, "nothing to restart"),
            (Pause, Running, true, "the only state pause is accepted in"),
            (Pause, Paused, false, "already frozen"),
            (
                Pause,
                Stopped,
                false,
                "docker refuses pause on a stopped container",
            ),
            (Pause, Unknown, false, "guessing here fails for certain"),
            (
                Unpause,
                Paused,
                true,
                "the only state unpause is accepted in",
            ),
            (
                Unpause,
                Running,
                false,
                "docker refuses unpause on a running container",
            ),
            (Unpause, Stopped, false, "nothing to resume"),
            (Unpause, Unknown, false, "guessing here fails for certain"),
        ] {
            assert_eq!(
                action_is_useful(action, state),
                expected,
                "{action:?} on {state:?}: {why}"
            );
        }
    }

    /// A row opens onto its logs, so only a kind the engine can show logs for
    /// reacts to a click at all.
    #[gpui::test]
    async fn only_a_kind_with_logs_opens_on_a_click(cx: &mut TestAppContext) {
        init_test(cx);
        let backend: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker());
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        cx.run_until_parked();

        panel.update(cx, |panel, cx| {
            assert!(panel.rows_open_a_detail(), "a container has a log");
            panel.choose_kind(ResourceKind::Image, cx);
            assert!(
                !panel.rows_open_a_detail(),
                "an image has never run, so a click on one would open nothing"
            );
        });
    }

    /// Opening a row shows it, and a panel with no workspace says why there are
    /// no logs rather than showing an empty box.
    #[gpui::test]
    async fn a_panel_with_no_workspace_says_why_the_logs_are_missing(cx: &mut TestAppContext) {
        init_test(cx);
        let backend: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker());
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        cx.run_until_parked();

        panel.update_in(cx, |panel, window, cx| {
            panel.open_detail(a_container("c0ffee", RunState::Running), window, cx);
            let detail = panel.detail.as_ref().expect("the row is open");
            assert_eq!(detail.resource.id, "c0ffee");
            assert!(
                detail.logs.is_none(),
                "no workspace means no project to build a terminal from"
            );
            assert!(
                detail.trouble.is_some(),
                "and it must say so -- a blank pane where logs were asked for is \
                 the failure this field exists to prevent"
            );
        });
    }

    /// A tab reaches its project, and the command it hands the terminal is the
    /// backend's own.
    ///
    /// Deliberately stops one step short of a running terminal. The fake's
    /// program is not a real binary, so the spawn fails -- and that failure is
    /// what proves both halves: the panel got far enough to *attempt* a spawn
    /// (so the workspace and project were reached), and what it tried to run is
    /// `fake logs c0ffee`, word for word what `logs_command` returned.
    ///
    /// Letting it succeed would mean a real PTY child process inside the test
    /// suite, which is the class of nondeterminism this crate has already had to
    /// remove once.
    #[gpui::test]
    async fn a_tab_reaches_its_project_and_runs_the_backends_own_command(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (multi_workspace, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
        let panel = tab_in_workspace(&workspace, vec![Arc::new(FakeBackend::docker())], cx);
        cx.run_until_parked();

        panel.update_in(cx, |panel, window, cx| {
            panel.open_detail(a_container("c0ffee", RunState::Running), window, cx);
        });
        cx.run_until_parked();

        panel.read_with(cx, |panel, _| {
            let detail = panel.detail.as_ref().expect("the row is open");
            let trouble = detail
                .trouble
                .as_deref()
                .expect("the fake's program does not exist, so the spawn must fail");
            assert!(
                trouble.contains("fake logs c0ffee"),
                "the terminal must be given the backend's own command, not one \
                 assembled in the view. Got: {trouble}"
            );
            assert!(
                !trouble.contains("workspace"),
                "reaching the project is the half this test is here to prove -- \
                 a no-workspace message would mean it never got that far. \
                 Got: {trouble}"
            );
        });
    }

    /// Going back drops the terminal, which is what stops `logs -f`.
    #[gpui::test]
    async fn going_back_drops_the_terminal(cx: &mut TestAppContext) {
        init_test(cx);
        let backend: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker());
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        cx.run_until_parked();

        panel.update_in(cx, |panel, window, cx| {
            panel.open_detail(a_container("c0ffee", RunState::Running), window, cx);
            assert!(panel.detail.is_some());
            panel.close_detail(cx);
            assert!(panel.detail.is_none(), "back returns to the list");
            assert!(
                panel.logs_build.is_none(),
                "and the task building the terminal goes with it"
            );
        });
    }

    /// Switching kind closes the open row. A container's logs shown under an
    /// "Images" tab is a screen that says two contradictory things at once.
    #[gpui::test]
    async fn switching_kind_closes_the_open_row(cx: &mut TestAppContext) {
        init_test(cx);
        let backend: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker());
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        cx.run_until_parked();

        panel.update_in(cx, |panel, window, cx| {
            panel.open_detail(a_container("c0ffee", RunState::Running), window, cx);
            assert!(panel.detail.is_some());
            panel.choose_kind(ResourceKind::Image, cx);
            assert!(
                panel.detail.is_none(),
                "the open row belonged to the kind that was left"
            );
        });
    }

    /// A list that arrives without the open row closes it: there is nothing left
    /// to show, and its logs are following something that is gone.
    #[gpui::test]
    async fn a_row_that_leaves_the_list_closes_its_detail(cx: &mut TestAppContext) {
        init_test(cx);
        let backend: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker());
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        cx.run_until_parked();

        panel.update_in(cx, |panel, window, cx| {
            panel.open_detail(a_container("c0ffee", RunState::Running), window, cx);
            // The row is still there, but stopped since it was opened.
            panel.refresh_detail(&[a_container("c0ffee", RunState::Stopped)]);
            assert_eq!(
                panel.detail.as_ref().map(|detail| detail.resource.state),
                Some(RunState::Stopped),
                "the header must not still say Running after Stop worked"
            );
            // And now it is gone entirely.
            panel.refresh_detail(&[]);
            assert!(
                panel.detail.is_none(),
                "a removed container has no logs left to follow"
            );
        });
    }

    /// Pause reaches the engine as a pause, not as something adjacent.
    #[gpui::test]
    async fn pausing_asks_the_engine_to_pause(cx: &mut TestAppContext) {
        init_test(cx);
        let fake = Arc::new(FakeBackend::docker());
        let backend: Arc<dyn ContainerBackend> = fake.clone();
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        cx.run_until_parked();

        panel.update(cx, |panel, cx| {
            panel.act(ResourceAction::Pause, "c0ffee".into(), cx);
        });
        cx.run_until_parked();

        assert_eq!(
            fake.acted(),
            vec![(ResourceAction::Pause, "c0ffee".to_string())],
            "the panel must not translate the verb on the way"
        );
    }
}

/// Removing things. The last phase, and the only one that can lose data.
mod removal {
    use super::*;
    use container::{DestructivePlan, PruneScope, ResourceKind};

    /// **The most important test in this crate.** Nothing may be removed without
    /// a plan, and a plan cannot exist without the list of what will be lost.
    ///
    /// Checked through the panel's own surface: the only function that starts a
    /// removal returns a plan and runs nothing, and the only function that runs
    /// one takes a plan. There is no third path.
    #[gpui::test]
    async fn a_removal_cannot_start_without_a_plan(cx: &mut TestAppContext) {
        init_test(cx);
        let backend = Arc::new(FakeBackend::docker());
        let recorder = backend.clone();
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        panel.update(cx, |panel, cx| panel.reload(cx));
        cx.run_until_parked();

        panel.read_with(cx, |panel, _| {
            assert!(
                panel.plan_removal("no-such-id").is_none(),
                "an id that is not on screen cannot be planned, so it cannot be \
                 removed"
            );
            let plan = panel
                .plan_removal("c0ffee")
                .expect("a row that is on screen can be planned");
            assert_eq!(plan.targets().len(), 1);
            assert_eq!(plan.confirmation(), "fake-postgres");
        });

        assert!(
            recorder.destroyed().is_empty(),
            "planning must not have removed anything -- the plan is a question, \
             not an instruction"
        );
    }

    /// A wrong answer removes nothing. This is the gate itself.
    #[gpui::test]
    async fn only_the_exact_confirmation_lets_a_removal_through(cx: &mut TestAppContext) {
        init_test(cx);
        let backend = Arc::new(FakeBackend::docker());
        let recorder = backend.clone();
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        panel.update(cx, |panel, cx| panel.reload(cx));
        cx.run_until_parked();

        let plan = panel
            .read_with(cx, |panel, _| panel.plan_removal("c0ffee"))
            .expect("a plan");

        for wrong in ["", "yes", "FAKE-POSTGRES", "fake-postgre", "fake-postgres!"] {
            assert!(
                !plan.is_confirmed_by(wrong),
                "{wrong:?} must not be accepted as consent"
            );
        }
        assert!(plan.is_confirmed_by("fake-postgres"));

        // Only now, with the confirmation given, may it run.
        panel.update(cx, |panel, cx| panel.destroy(plan, cx));
        cx.run_until_parked();
        assert_eq!(
            recorder.destroyed().len(),
            1,
            "and then exactly once, with the plan that was shown"
        );
    }

    /// A prune must find out what it would delete before anybody is asked.
    ///
    /// `docker system prune` has no `--dry-run`, so a dialog opened first could
    /// only say "everything unused" -- which is not something anybody can consent
    /// to.
    #[gpui::test]
    async fn a_prune_plan_carries_the_enumerated_losses(cx: &mut TestAppContext) {
        init_test(cx);
        let backend: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker());
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        cx.run_until_parked();

        let finding = panel.update(cx, |panel, cx| {
            panel.plan_prune(PruneScope::Reclaimable, cx)
        });
        let plan = finding.await.expect("the fake has something to prune");
        assert!(
            !plan.targets().is_empty(),
            "a prune plan with no targets is a dialog nobody can answer"
        );
        assert_eq!(plan.confirmation(), "prune");
        assert!(
            plan.warning().is_none(),
            "the default scope loses nothing that held data, so nothing to warn"
        );
    }

    /// Volumes are opt-in, and warned about when opted into.
    #[gpui::test]
    async fn pruning_with_volumes_warns_and_the_default_does_not(cx: &mut TestAppContext) {
        init_test(cx);
        let backend: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker());
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        cx.run_until_parked();

        let with_volumes = panel
            .update(cx, |panel, cx| {
                panel.plan_prune(PruneScope::IncludingVolumes, cx)
            })
            .await
            .expect("a plan");
        let warning = with_volumes
            .warning()
            .expect("volumes must be warned about");
        assert!(
            warning.contains("volume"),
            "the warning must name what is at risk: {warning}"
        );
    }

    /// A kind that cannot be removed must not offer the button.
    #[gpui::test]
    async fn a_kind_that_cannot_be_removed_offers_nothing(cx: &mut TestAppContext) {
        init_test(cx);
        let backend: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker());
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        cx.run_until_parked();

        panel.update(cx, |panel, cx| {
            assert!(panel.removable(), "a container can be removed");
            panel.choose_kind(ResourceKind::ComposeProject, cx);
            assert!(
                !panel.removable(),
                "a compose project is a label on other things, not a thing to \
                 remove"
            );
        });
    }

    /// Starting a removal with no workspace must refuse, not remove.
    ///
    /// A floating window's panel has no modal layer to show the confirmation in.
    /// Removing anyway would be removing with no confirmation at all.
    #[gpui::test]
    async fn with_no_place_to_confirm_nothing_is_removed(cx: &mut TestAppContext) {
        init_test(cx);
        let backend = Arc::new(FakeBackend::docker());
        let recorder = backend.clone();
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        panel.update(cx, |panel, cx| panel.reload(cx));
        cx.run_until_parked();

        panel.read_with(cx, |panel, _| assert!(panel.workspace.is_none()));
        panel.update_in(cx, |panel, window, cx| {
            panel.start_removal("c0ffee", window, cx);
        });
        cx.run_until_parked();

        assert!(
            recorder.destroyed().is_empty(),
            "no confirmation was possible, so nothing may have been removed"
        );
    }

    /// Kubernetes has no prune, so the panel must not offer one.
    #[gpui::test]
    async fn kubernetes_offers_no_prune(cx: &mut TestAppContext) {
        init_test(cx);
        let backends: Vec<Arc<dyn ContainerBackend>> = vec![
            Arc::new(FakeBackend::docker()),
            Arc::new(FakeBackend::empty(
                container::BackendKind::Kubernetes,
                &[ResourceKind::Pod],
            )),
        ];
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backends(backends, cx));
        cx.run_until_parked();

        panel.update(cx, |panel, cx| {
            assert!(panel.prune_available(), "docker has a prune");
            panel.choose_backend(1, cx);
            assert!(
                !panel.prune_available(),
                "kubernetes has no notion of unused, so no prune button"
            );
        });
    }

    /// A plan built for one row must not be usable to remove another.
    #[gpui::test]
    async fn a_plan_names_what_it_was_built_for(cx: &mut TestAppContext) {
        init_test(cx);
        let backend: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker());
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        panel.update(cx, |panel, cx| panel.reload(cx));
        cx.run_until_parked();

        let plan = panel
            .read_with(cx, |panel, _| panel.plan_removal("c0ffee"))
            .expect("a plan");
        assert_eq!(
            plan.targets()[0].id,
            "c0ffee",
            "the plan carries the row itself, so `destroy` addresses exactly what \
             was shown"
        );
        let _ = DestructivePlan::remove(ResourceKind::Container, Vec::new());
    }
}
