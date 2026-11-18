//! The Compose tab: project and service rows, their menus and buttons, `down`
//! through the confirmation, and opening a project's files -- driven through
//! the drawn panel over the in-process fake engine.
//!
//! The fake writes down every compose command it was asked to build, so "Up
//! produces this argv" is read off its request list. Nothing here runs a
//! compose verb on the machine the tests run on.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use container::fake_backend::{FakeBackend, Misbehaviour, Request};
use container::{
    ComposeLabels, ComposeOp, ComposeTarget, ContainerBackend, DestructivePlan, Intent, Op,
    Resource, ResourceKind, RunState,
};
use gpui::{AppContext as _, Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext};
use terminal_view::terminal_panel::TerminalPanel;
use workspace::Workspace;

use crate::compose_actions::{ComposeCommand, Plugin, service_rows};
use crate::confirm_modal::{ConfirmDestructive, Replan};
use crate::container_panel::{Busy, ContainerPanel};
use crate::container_panel_tests::{init_test, tab_in_workspace};
use crate::row_menu::MenuItem;
use crate::terminal::{TerminalIntent, spawn_in_terminal};

fn project(name: &str) -> Resource {
    Resource {
        kind: ResourceKind::ComposeProject,
        id: name.into(),
        name: name.into(),
        state: RunState::Running,
        detail: vec![
            ("Status", "running(2)".into()),
            ("Config", format!("/work/{name}/compose.yaml")),
        ],
        parent: None,
        compose: None,
    }
}

fn member(name: &str, project: &str, service: &str, state: RunState) -> Resource {
    Resource {
        kind: ResourceKind::Container,
        id: format!("id-{name}"),
        name: name.into(),
        state,
        detail: vec![("Image", "app:1".into())],
        parent: None,
        compose: Some(ComposeLabels {
            project: project.into(),
            service: service.into(),
            working_dir: Some(format!("/work/{project}")),
            config_files: Vec::new(),
        }),
    }
}

fn owned_by(kind: ResourceKind, name: &str, project: &str) -> Resource {
    Resource {
        kind,
        id: name.into(),
        name: name.into(),
        state: RunState::Unknown,
        detail: Vec::new(),
        parent: Some(project.into()),
        compose: None,
    }
}

fn shop() -> Vec<Resource> {
    vec![
        project("shop"),
        project("blog"),
        member("shop-web-1", "shop", "web", RunState::Running),
        member("shop-web-2", "shop", "web", RunState::Stopped),
        member("shop-db-1", "shop", "db", RunState::Running),
        member("blog-app-1", "blog", "app", RunState::Running),
        owned_by(ResourceKind::Network, "shop_default", "shop"),
        owned_by(ResourceKind::Volume, "shop_pgdata", "shop"),
    ]
}

fn shop_target() -> ComposeTarget {
    ComposeTarget {
        project: "shop".into(),
        working_dir: Some("/work/shop".into()),
        config_files: vec!["/work/shop/compose.yaml".into()],
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

/// `debug_bounds` wants a `'static` selector; a handful of short strings leaked
/// in a test process.
fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

fn press(selector: &str, cx: &mut VisualTestContext) {
    let bounds = cx
        .debug_bounds(leak(selector.to_string()))
        .unwrap_or_else(|| panic!("{selector} must be drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

fn drawn(selector: &str, cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(leak(selector.to_string())).is_some()
}

fn commands(backend: &FakeBackend) -> Vec<(ComposeOp, Vec<String>)> {
    backend
        .requests()
        .into_iter()
        .filter_map(|request| match request {
            Request::ComposeCommand { op, services, .. } => Some((op, services)),
            _ => None,
        })
        .collect()
}

/// A drawn panel on the Compose tab of a fake engine.
fn on_compose_tab(
    backend: FakeBackend,
    cx: &mut TestAppContext,
) -> (
    Entity<ContainerPanel>,
    Arc<FakeBackend>,
    &mut VisualTestContext,
) {
    init_test(cx);
    let backend = Arc::new(backend);
    let engine: Arc<dyn ContainerBackend> = backend.clone();
    let (panel, cx) = cx.add_window_view(|_window, cx| ContainerPanel::with_backend(engine, cx));
    panel.update(cx, |panel, cx| {
        panel.choose_kind(ResourceKind::ComposeProject, cx)
    });
    draw(cx);
    (panel, backend, cx)
}

fn run(
    panel: &Entity<ContainerPanel>,
    command: ComposeCommand,
    row: Resource,
    cx: &mut VisualTestContext,
) {
    panel.update_in(cx, |panel, window, cx| {
        panel.run_compose_command(command, row, window, cx)
    });
    draw(cx);
}

fn last_error(panel: &Entity<ContainerPanel>, cx: &mut VisualTestContext) -> Option<String> {
    panel.read_with(cx, |panel, _| {
        panel.last_error.as_ref().map(|error| error.to_string())
    })
}

async fn workspace_over(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    let fs = fs::FakeFs::new(cx.executor());
    let project = project::Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |multi, _| multi.workspace().clone());
    (workspace, cx)
}

/// A workspace holding a tab on the Compose tab of a fake engine.
async fn workspace_panel(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<ContainerPanel>,
    Arc<FakeBackend>,
    &mut VisualTestContext,
) {
    init_test(cx);
    let (workspace, cx) = workspace_over(cx).await;
    let backend = Arc::new(FakeBackend::docker().holding(shop()));
    let engine: Arc<dyn ContainerBackend> = backend.clone();
    let panel = tab_in_workspace(&workspace, vec![engine], cx);
    panel.update(cx, |panel, cx| {
        panel.choose_kind(ResourceKind::ComposeProject, cx)
    });
    draw(cx);
    (workspace, panel, backend, cx)
}

mod verbs {
    use super::*;

    #[gpui::test]
    async fn every_project_verb_asks_the_backend_for_its_own_command(cx: &mut TestAppContext) {
        let (panel, backend, cx) = on_compose_tab(FakeBackend::docker().holding(shop()), cx);
        let ops = [
            ComposeOp::Up,
            ComposeOp::Restart,
            ComposeOp::Stop,
            ComposeOp::Start,
            ComposeOp::Pull,
            ComposeOp::Build,
            ComposeOp::Logs,
        ];
        for op in ops {
            run(&panel, ComposeCommand::Run(op), project("shop"), cx);
        }
        let asked: Vec<_> = backend
            .requests()
            .into_iter()
            .filter_map(|request| match request {
                Request::ComposeCommand {
                    project,
                    op,
                    services,
                } => Some((project, op, services)),
                _ => None,
            })
            .collect();
        assert_eq!(
            asked,
            ops.map(|op| (shop_target(), op, Vec::new())),
            "the project's files and directory travel with every verb, and none \
             names a service"
        );
        assert_eq!(last_error(&panel, cx), None);
    }

    #[gpui::test]
    async fn a_service_verb_names_only_that_service_and_keeps_the_projects_files(
        cx: &mut TestAppContext,
    ) {
        let (panel, backend, cx) = on_compose_tab(FakeBackend::docker().holding(shop()), cx);
        let members = panel.read_with(cx, |panel, _| panel.compose_members.clone());
        let web = service_rows(&project("shop"), &members)
            .into_iter()
            .find(|row| row.name == "web")
            .expect("a web service");
        for op in [ComposeOp::Restart, ComposeOp::Up, ComposeOp::Logs] {
            run(&panel, ComposeCommand::RunService(op), web.clone(), cx);
        }
        assert_eq!(
            commands(&backend),
            [
                (ComposeOp::Restart, vec!["web".to_string()]),
                (ComposeOp::Up, vec!["web".to_string()]),
                (ComposeOp::Logs, vec!["web".to_string()]),
            ]
        );
        let all_the_projects_own = backend.requests().into_iter().all(|request| match request {
            Request::ComposeCommand { project, .. } => project == shop_target(),
            _ => true,
        });
        assert!(all_the_projects_own);
    }

    #[gpui::test]
    async fn a_verb_the_engine_does_not_offer_runs_nothing_and_says_why(cx: &mut TestAppContext) {
        let backend = FakeBackend::docker()
            .holding(shop())
            .not_offering(Op::Compose);
        let (panel, backend, cx) = on_compose_tab(backend, cx);
        run(
            &panel,
            ComposeCommand::Run(ComposeOp::Up),
            project("shop"),
            cx,
        );
        assert!(commands(&backend).is_empty(), "no command was even built");
        let said = last_error(&panel, cx).expect("the refusal reaches the person");
        assert!(said.contains("no compose support"), "{said}");
    }

    #[gpui::test]
    async fn a_project_with_no_file_label_cannot_be_brought_up_but_can_be_stopped(
        cx: &mut TestAppContext,
    ) {
        let mut bare = project("shop");
        bare.detail.retain(|(label, _)| *label != "Config");
        let members: Vec<_> = shop()
            .into_iter()
            .filter(|row| row.kind != ResourceKind::ComposeProject)
            .map(|mut row| {
                if let Some(labels) = row.compose.as_mut() {
                    labels.working_dir = None;
                }
                row
            })
            .collect();
        let rows = [vec![bare.clone()], members].concat();
        let (panel, backend, cx) = on_compose_tab(FakeBackend::docker().holding(rows), cx);

        run(&panel, ComposeCommand::Run(ComposeOp::Up), bare.clone(), cx);
        assert!(commands(&backend).is_empty());
        let said = last_error(&panel, cx).expect("it says why");
        assert!(said.contains("No compose file"), "{said}");

        run(&panel, ComposeCommand::Run(ComposeOp::Stop), bare, cx);
        assert_eq!(commands(&backend), [(ComposeOp::Stop, Vec::new())]);
    }

    #[gpui::test]
    async fn a_missing_plugin_blocks_every_verb_until_a_refresh_finds_it(cx: &mut TestAppContext) {
        let (panel, backend, cx) = on_compose_tab(FakeBackend::docker().holding(shop()), cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(panel.compose_plugin, Plugin::Present)
        });

        panel.update(cx, |panel, _| panel.compose_plugin = Plugin::Missing);
        run(
            &panel,
            ComposeCommand::Run(ComposeOp::Pull),
            project("shop"),
            cx,
        );
        assert!(commands(&backend).is_empty());
        let said = last_error(&panel, cx).expect("it says why");
        assert!(said.contains("not installed"), "{said}");

        panel.update(cx, |panel, cx| panel.reload(cx));
        draw(cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(panel.compose_plugin, Plugin::Present)
        });
    }

    #[gpui::test]
    async fn the_task_a_verb_becomes_keeps_the_argv_unsplit(_cx: &mut TestAppContext) {
        let argv: Vec<String> = ["compose", "-p", "shop", "up", "-d", "web"]
            .map(String::from)
            .into();
        let task = spawn_in_terminal(
            TerminalIntent::Compose(ComposeOp::Up),
            "docker",
            argv.clone(),
            "shop/web",
            ResourceKind::ComposeProject,
        );
        assert_eq!(task.command.as_deref(), Some("docker"));
        assert_eq!(task.args, argv);
        assert!(task.use_new_terminal && task.allow_concurrent_runs);
        assert_eq!(task.label, "compose up: shop/web");

        let logs = spawn_in_terminal(
            TerminalIntent::Compose(ComposeOp::Logs),
            "docker",
            argv,
            "shop/web",
            ResourceKind::ComposeProject,
        );
        assert_ne!(task.id, logs.id, "up and logs of one service are two tabs");
    }

    #[gpui::test]
    async fn a_verb_reaches_the_terminal_panel_without_re_entering_the_workspace(
        cx: &mut TestAppContext,
    ) {
        // `spawn_task` ends in a real PTY spawn, which parks.
        cx.executor().allow_parking();
        init_test(cx);
        cx.update(|cx| terminal_view::init(cx));
        let (workspace, cx) = workspace_over(cx).await;
        let terminal_panel = workspace.update_in(cx, |workspace, window, cx| {
            cx.new(|cx| TerminalPanel::new(workspace, window, cx))
        });
        workspace.update_in(cx, |workspace, window, cx| {
            workspace.add_panel(terminal_panel, window, cx)
        });
        let backend = Arc::new(FakeBackend::docker().holding(shop()));
        let engine: Arc<dyn ContainerBackend> = backend.clone();
        let panel = tab_in_workspace(&workspace, vec![engine], cx);
        panel.update(cx, |panel, cx| {
            panel.choose_kind(ResourceKind::ComposeProject, cx)
        });
        draw(cx);

        press("compose-button:Up:shop", cx);
        assert_eq!(commands(&backend), [(ComposeOp::Up, Vec::new())]);
        assert!(
            workspace.read_with(cx, |workspace, cx| workspace
                .panel::<TerminalPanel>(cx)
                .is_some()),
            "the terminal panel must really be there, or the click proved nothing"
        );
    }
}

mod rows {
    use super::*;

    #[gpui::test]
    async fn a_project_row_draws_the_buttons_of_its_state(cx: &mut TestAppContext) {
        let (_panel, _backend, cx) = on_compose_tab(FakeBackend::docker().holding(shop()), cx);
        for wanted in [
            "Up",
            "Stop",
            "Restart",
            "Pull Images",
            "Build",
            "Logs",
            "Down\u{2026}",
        ] {
            assert!(
                drawn(&format!("compose-button:{wanted}:shop"), cx),
                "{wanted} is a button of a running project"
            );
        }
        assert!(
            !drawn("compose-button:Start:shop", cx),
            "a running project has nothing to start"
        );
        assert!(drawn("compose-button:Show Services:shop", cx));
    }

    #[gpui::test]
    async fn a_button_of_an_engine_without_compose_is_drawn_and_does_nothing(
        cx: &mut TestAppContext,
    ) {
        let backend = FakeBackend::docker()
            .holding(shop())
            .not_offering(Op::Compose);
        let (_panel, backend, cx) = on_compose_tab(backend, cx);
        assert!(drawn("compose-button:Up:shop", cx));
        press("compose-button:Up:shop", cx);
        assert!(
            commands(&backend).is_empty(),
            "a disabled button is not a click"
        );
    }

    #[gpui::test]
    async fn the_chevron_shows_and_hides_a_projects_services(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = on_compose_tab(FakeBackend::docker().holding(shop()), cx);
        assert!(!drawn("container-row:shop/web", cx));

        press("compose-button:Show Services:shop", cx);
        for service in ["shop/web", "shop/db"] {
            assert!(drawn(&format!("container-row:{service}"), cx), "{service}");
        }
        assert!(
            !drawn("container-row:blog/app", cx),
            "only the project that was opened"
        );
        assert!(drawn("compose-button:Hide Services:shop", cx));

        press("compose-button:Hide Services:shop", cx);
        assert!(!drawn("container-row:shop/web", cx));
        panel.read_with(cx, |panel, _| assert!(panel.compose_expanded.is_empty()));
    }

    #[gpui::test]
    async fn a_service_row_has_its_own_buttons_and_is_not_selectable(cx: &mut TestAppContext) {
        let (panel, backend, cx) = on_compose_tab(FakeBackend::docker().holding(shop()), cx);
        press("compose-button:Show Services:shop", cx);

        let row = cx
            .debug_bounds("container-row:shop/db")
            .expect("the service is drawn");
        cx.simulate_click(
            gpui::point(row.left() + gpui::px(10.), row.center().y),
            Modifiers::secondary_key(),
        );
        draw(cx);
        panel.read_with(cx, |panel, cx| {
            assert!(panel.selected_resources(cx).is_empty());
            assert!(panel.detail.is_none(), "a service opens nothing");
        });

        press("compose-button:Restart:shop/db", cx);
        assert_eq!(
            commands(&backend),
            [(ComposeOp::Restart, vec!["db".to_string()])]
        );
        assert!(drawn("compose-button:Open Shell:shop/db", cx));
        assert!(
            !drawn("compose-button:Down\u{2026}:shop/db", cx),
            "down is a project's"
        );
    }

    #[gpui::test]
    async fn the_menus_are_the_projects_and_the_services_own(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = on_compose_tab(FakeBackend::docker().holding(shop()), cx);
        let members = panel.read_with(cx, |panel, _| panel.compose_members.clone());
        let web = service_rows(&project("shop"), &members).remove(1);

        panel.read_with(cx, |panel, cx| {
            let (targets, items) = panel.row_menu_plan(&project("shop"), cx).expect("a menu");
            assert_eq!(targets.len(), 1);
            let has = |items: &[MenuItem], wanted: ComposeCommand| {
                items
                    .iter()
                    .any(|item| matches!(item, MenuItem::Compose(entry) if entry.command == wanted))
            };
            assert!(has(&items, ComposeCommand::Down));
            assert!(has(&items, ComposeCommand::OpenFile));
            assert!(has(
                &items,
                ComposeCommand::ToggleServices { expanded: false }
            ));
            assert!(
                !items.contains(&MenuItem::OpenShell),
                "no shell for a project"
            );

            let (_, items) = panel.row_menu_plan(&web, cx).expect("a service menu");
            assert!(has(&items, ComposeCommand::ServiceShell));
            assert!(!has(&items, ComposeCommand::Down));
        });
    }

    #[gpui::test]
    async fn right_clicking_a_service_opens_a_menu(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = on_compose_tab(FakeBackend::docker().holding(shop()), cx);
        press("compose-button:Show Services:shop", cx);
        let row = cx.debug_bounds("container-row:shop/web").expect("drawn");
        cx.simulate_mouse_down(
            gpui::point(row.left() + gpui::px(10.), row.center().y),
            MouseButton::Right,
            Modifiers::none(),
        );
        draw(cx);
        panel.read_with(cx, |panel, _| assert!(panel.context_menu.is_some()));
    }

    #[gpui::test]
    async fn changing_engine_forgets_what_was_learned_about_the_old_ones_compose(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);
        let first: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker().holding(shop()));
        let second: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker().holding(Vec::new()));
        let (panel, cx) = cx
            .add_window_view(|_window, cx| ContainerPanel::with_backends(vec![first, second], cx));
        panel.update(cx, |panel, cx| {
            panel.choose_kind(ResourceKind::ComposeProject, cx)
        });
        draw(cx);
        panel.update(cx, |panel, cx| panel.toggle_services("shop", cx));
        panel.update(cx, |panel, cx| panel.choose_backend(1, cx));
        draw(cx);
        panel.read_with(cx, |panel, _| {
            assert!(panel.compose_members.is_empty());
            assert!(panel.compose_expanded.is_empty());
        });
    }
}

mod down {
    use super::*;

    fn open_modal(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) -> Entity<ConfirmDestructive> {
        workspace
            .read_with(cx, |workspace, cx| {
                workspace.active_modal::<ConfirmDestructive>(cx)
            })
            .expect("the dialog is open")
    }

    fn names(modal: &Entity<ConfirmDestructive>, cx: &mut VisualTestContext) -> Vec<String> {
        modal.read_with(cx, |modal, _| {
            modal
                .plan()
                .targets()
                .iter()
                .map(|target| target.name.clone())
                .collect()
        })
    }

    #[gpui::test]
    async fn down_goes_through_the_dialog_and_lists_no_volume_until_one_is_chosen(
        cx: &mut TestAppContext,
    ) {
        let (workspace, _panel, backend, cx) = workspace_panel(cx).await;
        press("compose-button:Down\u{2026}:shop", cx);

        assert!(backend.destroyed().is_empty(), "the button only asks");
        let modal = open_modal(&workspace, cx);
        assert_eq!(
            names(&modal, cx),
            ["shop-web-1", "shop-web-2", "shop-db-1", "shop_default"],
            "the project's containers and network, and not its volume"
        );
        modal.read_with(cx, |modal, _| {
            assert_eq!(modal.plan().confirmation(), "shop");
            assert_eq!(
                modal.plan().intent(),
                Intent::ComposeDown {
                    with_volumes: false
                }
            );
        });
    }

    #[gpui::test]
    async fn choosing_volumes_lists_them_and_the_confirmed_plan_carries_the_flag(
        cx: &mut TestAppContext,
    ) {
        let (workspace, panel, backend, cx) = workspace_panel(cx).await;
        press("compose-button:Down\u{2026}:shop", cx);
        let modal = open_modal(&workspace, cx);

        modal.update_in(cx, |modal, window, cx| modal.set_volumes(true, window, cx));
        draw(cx);
        assert_eq!(
            names(&modal, cx),
            [
                "shop-web-1",
                "shop-web-2",
                "shop-db-1",
                "shop_default",
                "shop_pgdata"
            ]
        );
        let asked: Vec<bool> = backend
            .requests()
            .into_iter()
            .filter_map(|request| match request {
                Request::ComposeDownPlan { with_volumes, .. } => Some(with_volumes),
                _ => None,
            })
            .collect();
        assert_eq!(asked, [false, true]);

        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("shop", window, cx);
            modal.press_remove(window, cx);
        });
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.busy_for("shop")),
            Some(Busy::Removing),
            "the project's own row says it is going"
        );
        draw(cx);

        let destroyed = backend.destroyed();
        assert_eq!(destroyed.len(), 1);
        assert_eq!(
            destroyed[0].intent(),
            Intent::ComposeDown { with_volumes: true }
        );
        assert!(
            destroyed[0]
                .targets()
                .iter()
                .any(|target| target.name == "shop_pgdata")
        );
        assert_eq!(
            destroyed[0]
                .compose_target()
                .map(|target| target.project.as_str()),
            Some("shop")
        );
        panel.read_with(cx, |panel, _| assert_eq!(panel.busy_for("shop"), None));
    }

    #[gpui::test]
    async fn a_replanned_list_asks_for_the_name_again(cx: &mut TestAppContext) {
        let (workspace, _panel, backend, cx) = workspace_panel(cx).await;
        press("compose-button:Down\u{2026}:shop", cx);
        let modal = open_modal(&workspace, cx);

        // The name was typed for the list without volumes. Ticking the box
        // changes what the list says, so the answer given to the old one is not
        // an answer to the new one.
        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("shop", window, cx)
        });
        modal.update_in(cx, |modal, window, cx| modal.set_volumes(true, window, cx));
        draw(cx);
        modal.update_in(cx, |modal, window, cx| modal.press_remove(window, cx));
        draw(cx);
        assert!(
            backend.destroyed().is_empty(),
            "one press must not delete the volumes on the strength of a name typed before they were listed"
        );

        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("shop", window, cx);
            modal.press_remove(window, cx);
        });
        draw(cx);
        assert_eq!(backend.destroyed().len(), 1, "typed again, it goes through");
    }

    #[gpui::test]
    async fn a_plan_that_could_not_be_asked_for_keeps_what_was_typed(cx: &mut TestAppContext) {
        init_test(cx);
        let refusing = Arc::new(
            FakeBackend::docker()
                .holding(shop())
                .misbehaving(Misbehaviour::NotAuthorized),
        );
        let plan = DestructivePlan::compose_down(
            shop_target(),
            vec![member("shop-web-1", "shop", "web", RunState::Running)],
            Vec::new(),
            None,
        )
        .expect("a plan");
        let fired = Rc::new(Cell::new(0usize));
        let counter = fired.clone();
        let (modal, cx) = cx.add_window_view(|window, cx| {
            ConfirmDestructive::new(
                plan,
                move |_, _, _| counter.set(counter.get() + 1),
                window,
                cx,
            )
            .replanning(Replan {
                backend: refusing,
                target: shop_target(),
            })
        });
        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("shop", window, cx)
        });
        modal.update_in(cx, |modal, window, cx| modal.set_volumes(true, window, cx));
        draw(cx);
        modal.update_in(cx, |modal, window, cx| modal.press_remove(window, cx));
        assert_eq!(
            fired.get(),
            1,
            "the plan is the one the name was typed for, so the name still counts"
        );
    }

    #[gpui::test]
    async fn unticking_takes_the_volumes_back_out_and_a_wrong_name_removes_nothing(
        cx: &mut TestAppContext,
    ) {
        let (workspace, _panel, backend, cx) = workspace_panel(cx).await;
        press("compose-button:Down\u{2026}:shop", cx);
        let modal = open_modal(&workspace, cx);

        modal.update_in(cx, |modal, window, cx| modal.set_volumes(true, window, cx));
        draw(cx);
        modal.update_in(cx, |modal, window, cx| modal.set_volumes(false, window, cx));
        draw(cx);
        assert!(!names(&modal, cx).contains(&"shop_pgdata".to_string()));

        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("blog", window, cx);
            modal.press_remove(window, cx);
        });
        draw(cx);
        assert!(backend.destroyed().is_empty());

        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("shop", window, cx);
            modal.press_remove(window, cx);
        });
        draw(cx);
        assert_eq!(
            backend.destroyed()[0].intent(),
            Intent::ComposeDown {
                with_volumes: false
            }
        );
    }

    #[gpui::test]
    async fn the_remove_button_is_off_while_the_plan_is_being_asked_for_again(
        cx: &mut TestAppContext,
    ) {
        let (workspace, _panel, backend, cx) = workspace_panel(cx).await;
        press("compose-button:Down\u{2026}:shop", cx);
        let modal = open_modal(&workspace, cx);

        // Ticked and confirmed in one breath: the answer has not come back, so
        // the plan held is still the one without volumes.
        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("shop", window, cx);
            modal.set_volumes(true, window, cx);
            modal.press_remove(window, cx);
        });
        // `destroy` runs on a task, so what a press set going shows only once
        // everything has settled.
        draw(cx);
        assert!(
            backend.destroyed().is_empty(),
            "a plan that is about to change is not confirmed"
        );
        modal.update_in(cx, |modal, window, cx| modal.press_remove(window, cx));
        draw(cx);
        assert!(
            backend.destroyed().is_empty(),
            "the new list asks for the name again: what was typed was for the old one"
        );
        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("shop", window, cx);
            modal.press_remove(window, cx);
        });
        draw(cx);
        assert_eq!(
            backend.destroyed().len(),
            1,
            "once the answer is in and the name is typed, the press goes through"
        );
    }

    #[gpui::test]
    async fn a_plan_that_could_not_be_asked_for_leaves_the_old_one_and_the_tick_off(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);
        let refusing = Arc::new(
            FakeBackend::docker()
                .holding(shop())
                .misbehaving(Misbehaviour::NotAuthorized),
        );
        let plan = DestructivePlan::compose_down(
            shop_target(),
            vec![member("shop-web-1", "shop", "web", RunState::Running)],
            Vec::new(),
            None,
        )
        .expect("a plan");
        let (modal, cx) = cx.add_window_view(|window, cx| {
            ConfirmDestructive::new(plan, |_, _, _| {}, window, cx).replanning(Replan {
                backend: refusing,
                target: shop_target(),
            })
        });
        modal.update_in(cx, |modal, window, cx| modal.set_volumes(true, window, cx));
        draw(cx);
        modal.read_with(cx, |modal, _| {
            assert_eq!(
                modal.plan().intent(),
                Intent::ComposeDown {
                    with_volumes: false
                }
            );
            assert!(!modal.volumes_wanted());
            assert!(modal.planning_failure().is_some());
        });
    }

    #[gpui::test]
    async fn a_project_with_nothing_to_take_down_says_so_instead_of_asking(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);
        let (workspace, cx) = workspace_over(cx).await;
        let engine: Arc<dyn ContainerBackend> =
            Arc::new(FakeBackend::docker().holding(vec![project("shop")]));
        let panel = tab_in_workspace(&workspace, vec![engine], cx);
        panel.update(cx, |panel, cx| {
            panel.choose_kind(ResourceKind::ComposeProject, cx)
        });
        draw(cx);
        press("compose-button:Down\u{2026}:shop", cx);
        assert!(
            workspace
                .read_with(cx, |workspace, cx| workspace
                    .active_modal::<ConfirmDestructive>(cx))
                .is_none()
        );
        let said = last_error(&panel, cx).expect("it says so");
        assert!(said.contains("nothing to take down"), "{said}");
    }

    #[gpui::test]
    async fn with_no_workspace_down_refuses_rather_than_going_unconfirmed(cx: &mut TestAppContext) {
        let (panel, backend, cx) = on_compose_tab(FakeBackend::docker().holding(shop()), cx);
        run(&panel, ComposeCommand::Down, project("shop"), cx);
        assert!(backend.destroyed().is_empty());
        assert!(
            backend
                .requests()
                .iter()
                .all(|request| !matches!(request, Request::ComposeDownPlan { .. })),
            "it did not even ask"
        );
    }
}

mod files {
    use super::*;

    async fn local_workspace_with_file(
        cx: &mut TestAppContext,
    ) -> (
        Entity<Workspace>,
        Entity<ContainerPanel>,
        &mut VisualTestContext,
    ) {
        init_test(cx);
        let fs = fs::FakeFs::new(cx.executor());
        fs::Fs::create_dir(fs.as_ref(), std::path::Path::new("/work/shop"))
            .await
            .expect("a directory for the file");
        fs.insert_file("/work/shop/compose.yaml", b"services: {}\n".to_vec())
            .await;
        let project = project::Project::test(fs, [], cx).await;
        let (multi_workspace, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |multi, _| multi.workspace().clone());
        let engine: Arc<dyn ContainerBackend> = Arc::new(FakeBackend::docker().holding(shop()));
        let panel = tab_in_workspace(&workspace, vec![engine], cx);
        panel.update(cx, |panel, cx| {
            panel.choose_kind(ResourceKind::ComposeProject, cx)
        });
        draw(cx);
        (workspace, panel, cx)
    }

    #[gpui::test]
    async fn open_compose_file_opens_the_file_in_the_editor(cx: &mut TestAppContext) {
        let (workspace, panel, cx) = local_workspace_with_file(cx).await;
        run(&panel, ComposeCommand::OpenFile, project("shop"), cx);
        cx.run_until_parked();

        // A file opened on its own is a worktree of one file, so the file's
        // path is the worktree's.
        let opened: Vec<String> = workspace.read_with(cx, |workspace, cx| {
            let project = workspace.project().read(cx);
            workspace
                .items(cx)
                .filter_map(|item| item.project_path(cx))
                .filter_map(|path| project.worktree_for_id(path.worktree_id, cx))
                .map(|worktree| worktree.read(cx).abs_path().to_string_lossy().into_owned())
                .collect()
        });
        assert_eq!(opened, ["/work/shop/compose.yaml"]);
        assert_eq!(last_error(&panel, cx), None);
    }

    #[gpui::test]
    async fn reveal_working_directory_needs_a_directory(cx: &mut TestAppContext) {
        // The test platform cannot reveal anything, so only the refusal is
        // driven here; whether the entry is open is `compose_actions`' own test.
        let (_workspace, panel, cx) = local_workspace_with_file(cx).await;
        let nowhere = Resource {
            id: "ghost".into(),
            name: "ghost".into(),
            ..project("ghost")
        };
        run(&panel, ComposeCommand::RevealDirectory, nowhere, cx);
        let said = last_error(&panel, cx).expect("it says why");
        assert!(said.contains("working directory"), "{said}");
    }
}
