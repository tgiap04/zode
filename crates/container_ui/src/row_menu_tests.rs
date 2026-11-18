//! Right-click menus, selection, bulk actions and quick filters, driven through
//! the drawn panel over a fake engine.

use std::sync::Arc;

use container::fake_backend::FakeBackend;
use container::resource::ComposeLabels;
use container::{ContainerBackend, Resource, ResourceAction, ResourceKind, RunState};
use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext};

use crate::confirm_modal::ConfirmDestructive;
use crate::container_panel::{Busy, ContainerPanel, ListState};
use crate::container_panel_tests::{init_test, tab_in_workspace};
use crate::row_menu::{MenuItem, PortLink, render_items};

fn container(name: &str, state: RunState) -> Resource {
    Resource {
        kind: ResourceKind::Container,
        id: format!("id-{name}"),
        name: name.into(),
        state,
        detail: vec![("Image", "nginx:1".into())],
        parent: None,
        compose: None,
    }
}

fn composed(name: &str, state: RunState, project: &str) -> Resource {
    Resource {
        compose: Some(ComposeLabels {
            project: project.into(),
            service: name.into(),
            working_dir: None,
            config_files: Vec::new(),
        }),
        ..container(name, state)
    }
}

fn image(name: &str, dangling: bool) -> Resource {
    Resource {
        kind: ResourceKind::Image,
        id: format!("id-{name}"),
        name: name.into(),
        state: RunState::Unknown,
        detail: if dangling {
            vec![("Dangling", "yes".into())]
        } else {
            Vec::new()
        },
        parent: None,
        compose: None,
    }
}

fn fleet() -> Vec<Resource> {
    vec![
        container("web", RunState::Running),
        container("api", RunState::Running),
        container("db", RunState::Stopped),
        container("cache", RunState::Running),
    ]
}

fn id(name: &str) -> String {
    format!("id-{name}")
}

/// A drawn panel over a fake engine holding `resources`.
fn panel_holding(
    resources: Vec<Resource>,
    cx: &mut TestAppContext,
) -> (
    Entity<ContainerPanel>,
    Arc<FakeBackend>,
    &mut VisualTestContext,
) {
    init_test(cx);
    let backend = Arc::new(FakeBackend::docker().holding(resources));
    let engine: Arc<dyn ContainerBackend> = backend.clone();
    let (panel, cx) = cx.add_window_view(|_window, cx| ContainerPanel::with_backend(engine, cx));
    panel.update(cx, |panel, cx| panel.reload(cx));
    draw(cx);
    (panel, backend, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

/// `debug_bounds` wants a `'static` selector. Leaked: a handful of short
/// strings in a test process.
fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

/// The row of `name`, whichever kind is on screen: an image's row is keyed by
/// its id *and* its name, since every tag of an image shares the id.
fn row_bounds(name: &str, cx: &mut VisualTestContext) -> gpui::Bounds<gpui::Pixels> {
    cx.debug_bounds(leak(format!("container-row:{}", id(name))))
        .or_else(|| cx.debug_bounds(leak(format!("container-row:{}\u{0}{name}", id(name)))))
        .unwrap_or_else(|| panic!("the row of {name} must be drawn"))
}

fn click(name: &str, modifiers: Modifiers, cx: &mut VisualTestContext) {
    let bounds = row_bounds(name, cx);
    cx.simulate_click(
        gpui::point(bounds.left() + gpui::px(10.), bounds.center().y),
        modifiers,
    );
    draw(cx);
}

fn press(selector: &str, cx: &mut VisualTestContext) {
    let bounds = cx
        .debug_bounds(leak(selector.to_string()))
        .unwrap_or_else(|| panic!("{selector} must be drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

fn command() -> Modifiers {
    Modifiers::secondary_key()
}

fn selected_names(panel: &Entity<ContainerPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    panel.read_with(cx, |panel, cx| {
        panel
            .selected_resources(cx)
            .into_iter()
            .map(|resource| resource.name)
            .collect()
    })
}

fn visible_names(panel: &Entity<ContainerPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    panel.read_with(cx, |panel, cx| {
        panel
            .visible_resources(cx)
            .into_iter()
            .map(|resource| resource.name)
            .collect()
    })
}

fn acted(backend: &FakeBackend) -> Vec<(ResourceAction, String)> {
    let mut acted = backend.acted();
    acted.sort_by(|a, b| a.1.cmp(&b.1));
    acted
}

mod selecting {
    use super::*;

    #[gpui::test]
    async fn command_click_builds_a_selection_and_shift_extends_it(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(fleet(), cx);
        click("web", command(), cx);
        click("cache", command(), cx);
        assert_eq!(selected_names(&panel, cx), ["web", "cache"]);

        // The anchor is the last row toggled, so shift reaches back from it.
        click("api", Modifiers::shift(), cx);
        assert_eq!(selected_names(&panel, cx), ["api", "db", "cache"]);

        click("db", command(), cx);
        assert_eq!(selected_names(&panel, cx), ["api", "cache"]);
    }

    #[gpui::test]
    async fn a_plain_click_opens_a_row_that_has_logs_and_drops_the_selection(
        cx: &mut TestAppContext,
    ) {
        let (panel, _backend, cx) = panel_holding(fleet(), cx);
        click("web", command(), cx);
        click("api", command(), cx);
        assert_eq!(selected_names(&panel, cx).len(), 2);

        click("db", Modifiers::none(), cx);
        panel.read_with(cx, |panel, _| {
            let detail = panel.detail.as_ref().expect("the row opens");
            assert_eq!(detail.resource.name, "db");
            assert!(
                panel.selection.is_empty(),
                "a stale selection would wait behind it"
            );
        });
    }

    /// With nothing to show for a kind, a plain click only selects.
    #[gpui::test]
    async fn a_plain_click_selects_a_row_that_opens_nothing(cx: &mut TestAppContext) {
        init_test(cx);
        let engine: Arc<dyn ContainerBackend> = crate::detail_tests::Scripted::bare(
            FakeBackend::docker().holding(vec![image("nginx", false), image("redis", false)]),
        );
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(engine, cx));
        panel.update(cx, |panel, cx| panel.choose_kind(ResourceKind::Image, cx));
        draw(cx);
        panel.read_with(cx, |panel, _| assert!(!panel.rows_open_a_detail()));
        click("nginx", Modifiers::none(), cx);
        click("redis", Modifiers::none(), cx);
        assert_eq!(selected_names(&panel, cx), ["redis"]);
        panel.read_with(cx, |panel, _| assert!(panel.detail.is_none()));
    }

    /// An image has Inspect, so a plain click opens it rather than selecting.
    #[gpui::test]
    async fn a_plain_click_on_an_image_opens_its_inspect(cx: &mut TestAppContext) {
        let (panel, _backend, cx) =
            panel_holding(vec![image("nginx", false), image("redis", false)], cx);
        panel.update(cx, |panel, cx| panel.choose_kind(ResourceKind::Image, cx));
        draw(cx);
        click("nginx", Modifiers::none(), cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                panel.detail.as_ref().map(|detail| detail.tab),
                Some(crate::detail_tabs::DetailTab::Inspect)
            );
        });
    }

    #[gpui::test]
    async fn changing_kind_forgets_the_selection(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(fleet(), cx);
        click("web", command(), cx);
        assert_eq!(selected_names(&panel, cx), ["web"]);
        panel.update(cx, |panel, cx| panel.choose_kind(ResourceKind::Image, cx));
        draw(cx);
        panel.update(cx, |panel, cx| {
            panel.choose_kind(ResourceKind::Container, cx)
        });
        draw(cx);
        assert!(selected_names(&panel, cx).is_empty());
        assert!(cx.debug_bounds("container-selection-bar").is_none());
    }

    #[gpui::test]
    async fn the_bar_counts_the_selection_and_clear_empties_it(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(fleet(), cx);
        assert!(cx.debug_bounds("container-selection-bar").is_none());
        click("web", command(), cx);
        click("api", command(), cx);
        assert!(cx.debug_bounds("container-selection-bar").is_some());
        press("container-bulk:Clear", cx);
        assert!(selected_names(&panel, cx).is_empty());
        assert!(cx.debug_bounds("container-selection-bar").is_none());
    }
}

mod bulk {
    use super::*;

    #[gpui::test]
    async fn stopping_a_selection_sends_one_act_per_row_that_is_running(cx: &mut TestAppContext) {
        let (panel, backend, cx) = panel_holding(fleet(), cx);
        for name in ["web", "api", "db"] {
            click(name, command(), cx);
        }
        press("container-bulk:Stop", cx);
        assert_eq!(
            acted(&backend),
            [
                (ResourceAction::Stop, id("api")),
                (ResourceAction::Stop, id("web")),
            ],
            "db is already stopped, so it is not asked to stop"
        );
        assert_eq!(
            selected_names(&panel, cx).len(),
            3,
            "the selection survives the action"
        );
    }

    #[gpui::test]
    async fn the_bar_offers_only_what_some_selected_row_can_take(cx: &mut TestAppContext) {
        let (_panel, _backend, cx) = panel_holding(fleet(), cx);
        click("web", command(), cx);
        click("api", command(), cx);
        assert!(cx.debug_bounds("container-bulk:Stop").is_some());
        assert!(cx.debug_bounds("container-bulk:Restart").is_some());
        assert!(
            cx.debug_bounds("container-bulk:Start").is_none(),
            "both are running"
        );
        click("db", command(), cx);
        assert!(cx.debug_bounds("container-bulk:Start").is_some());
    }

    #[gpui::test]
    async fn a_bulk_remove_opens_one_dialog_listing_every_selected_row(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (multi_workspace, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
        let backend = Arc::new(FakeBackend::docker().holding(fleet()));
        let engine: Arc<dyn ContainerBackend> = backend.clone();
        let panel = tab_in_workspace(&workspace, vec![engine], cx);
        panel.update(cx, |panel, cx| panel.reload(cx));
        draw(cx);

        for name in ["web", "db", "cache"] {
            click(name, command(), cx);
        }
        press("container-bulk:Remove", cx);

        let modal = workspace
            .read_with(cx, |workspace, cx| {
                workspace.active_modal::<ConfirmDestructive>(cx)
            })
            .expect("one confirmation opens");
        modal.read_with(cx, |modal, _| {
            let names: Vec<&str> = modal
                .plan()
                .targets()
                .iter()
                .map(|target| target.name.as_str())
                .collect();
            assert_eq!(names, ["web", "db", "cache"], "every selected row, once");
            assert_eq!(modal.plan().confirmation(), "3");
        });
        assert!(
            backend.destroyed().is_empty(),
            "opening the dialog removes nothing"
        );

        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("2", window, cx);
            modal.press_remove(window, cx);
        });
        cx.run_until_parked();
        assert!(
            backend.destroyed().is_empty(),
            "a wrong answer removes nothing"
        );

        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("3", window, cx);
            modal.press_remove(window, cx);
        });
        cx.run_until_parked();
        let destroyed = backend.destroyed();
        assert_eq!(destroyed.len(), 1, "one plan, not one per row");
        assert_eq!(destroyed[0].targets().len(), 3);
    }

    #[gpui::test]
    async fn a_row_the_filters_hide_is_never_part_of_a_bulk_remove(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(fleet(), cx);
        panel.update(cx, |panel, cx| panel.select_all_rows(cx));
        panel.update(cx, |panel, cx| panel.toggle_running_only(cx));
        draw(cx);

        assert_eq!(selected_names(&panel, cx), ["web", "api", "cache"]);
        let ids: Vec<String> = panel.read_with(cx, |panel, cx| {
            panel
                .selected_resources(cx)
                .into_iter()
                .map(|resource| resource.id)
                .collect()
        });
        let plan = panel
            .update(cx, |panel, cx| panel.plan_removal_of(&ids, cx))
            .await
            .expect("planning")
            .expect("three rows to remove");
        assert!(
            plan.targets().iter().all(|target| target.name != "db"),
            "db is hidden, so nobody agreed to remove it"
        );
        assert_eq!(plan.targets().len(), 3);
    }

    #[gpui::test]
    async fn a_plan_names_only_rows_the_list_has_and_none_that_are_busy(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(fleet(), cx);
        panel.update(cx, |panel, _| {
            panel.mark_busy([id("web")], Busy::Action(ResourceAction::Stop), 99)
        });
        let ids = [id("web"), id("api"), "id-ghost".to_string()];
        let plan = panel
            .update(cx, |panel, cx| panel.plan_removal_of(&ids, cx))
            .await
            .expect("planning")
            .expect("api remains");
        let names: Vec<&str> = plan.targets().iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            ["api"],
            "not the busy row and not the one that is not there"
        );
        let none_of = |ids: Vec<String>, cx: &mut VisualTestContext| {
            panel.update(cx, |panel, cx| panel.plan_removal_of(&ids, cx))
        };
        assert!(
            none_of(vec!["id-ghost".to_string()], cx)
                .await
                .expect("planning")
                .is_none()
        );
        assert!(none_of(Vec::new(), cx).await.expect("planning").is_none());
    }

    #[gpui::test]
    async fn a_busy_row_is_skipped_by_a_bulk_action(cx: &mut TestAppContext) {
        let (panel, backend, cx) = panel_holding(fleet(), cx);
        panel.update(cx, |panel, _| {
            panel.mark_busy([id("web")], Busy::Removing, 99)
        });
        let rows: Vec<Resource> = vec![
            container("web", RunState::Running),
            container("api", RunState::Running),
        ];
        panel.update(cx, |panel, cx| {
            panel.act_many(ResourceAction::Restart, &rows, cx)
        });
        cx.run_until_parked();
        assert_eq!(acted(&backend), [(ResourceAction::Restart, id("api"))]);
    }
}

mod menus {
    use super::*;

    async fn right_click(name: &str, cx: &mut VisualTestContext) {
        let bounds = row_bounds(name, cx);
        cx.simulate_mouse_down(
            gpui::point(bounds.left() + gpui::px(10.), bounds.center().y),
            MouseButton::Right,
            Modifiers::none(),
        );
        draw(cx);
    }

    #[gpui::test]
    async fn right_clicking_a_row_opens_its_menu_and_dismissing_clears_it(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(fleet(), cx);
        right_click("web", cx).await;
        let menu = panel
            .read_with(cx, |panel, _| {
                panel.context_menu.as_ref().map(|(menu, ..)| menu.clone())
            })
            .expect("the right-click opened a menu");

        menu.update(cx, |_, cx| cx.emit(gpui::DismissEvent));
        draw(cx);
        panel.read_with(cx, |panel, _| assert!(panel.context_menu.is_none()));
    }

    #[gpui::test]
    async fn right_clicking_empty_space_opens_the_panel_menu(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(Vec::new(), cx);
        cx.simulate_mouse_down(
            gpui::point(gpui::px(300.), gpui::px(300.)),
            MouseButton::Right,
            Modifiers::none(),
        );
        draw(cx);
        panel.read_with(cx, |panel, _| assert!(panel.context_menu.is_some()));
    }

    #[gpui::test]
    async fn a_busy_row_has_no_menu(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(fleet(), cx);
        panel.update(cx, |panel, _| {
            panel.mark_busy([id("web")], Busy::Removing, 99)
        });
        draw(cx);
        right_click("web", cx).await;
        panel.read_with(cx, |panel, _| assert!(panel.context_menu.is_none()));
    }

    #[gpui::test]
    async fn the_menu_of_a_selected_row_is_for_the_whole_selection(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(fleet(), cx);
        click("web", command(), cx);
        click("api", command(), cx);
        panel.read_with(cx, |panel, cx| {
            let web = container("web", RunState::Running);
            let (targets, items) = panel.row_menu_plan(&web, cx).expect("a menu");
            assert_eq!(targets.len(), 2);
            assert!(items.contains(&MenuItem::Remove));
            assert!(
                !items.contains(&MenuItem::OpenShell),
                "a shell is for one row"
            );

            let cache = container("cache", RunState::Running);
            let (targets, items) = panel.row_menu_plan(&cache, cx).expect("a menu");
            assert_eq!(
                targets.len(),
                1,
                "a row outside the selection is its own target"
            );
            assert!(items.contains(&MenuItem::OpenShell));
        });
        assert_eq!(
            selected_names(&panel, cx),
            ["web", "api"],
            "the plan left the selection alone"
        );
    }

    /// Builds a menu of `items` over `targets` and confirms its first entry,
    /// exactly as the user would with the keyboard.
    fn confirm_first(
        panel: &Entity<ContainerPanel>,
        items: Vec<MenuItem>,
        targets: Vec<Resource>,
        cx: &mut VisualTestContext,
    ) {
        let weak = panel.downgrade();
        let menu = cx.update(|window, cx| {
            ui::ContextMenu::build(window, cx, move |menu, _, _| {
                render_items(menu, &items, &targets, &weak)
            })
        });
        menu.update_in(cx, |menu, window, cx| {
            menu.select_first(&menu::SelectFirst, window, cx);
            menu.confirm(&menu::Confirm, window, cx);
        });
        cx.run_until_parked();
    }

    #[gpui::test]
    async fn a_lifecycle_entry_asks_the_engine(cx: &mut TestAppContext) {
        let (panel, backend, cx) = panel_holding(fleet(), cx);
        confirm_first(
            &panel,
            vec![MenuItem::Act(ResourceAction::Pause)],
            vec![container("web", RunState::Running)],
            cx,
        );
        assert_eq!(acted(&backend), [(ResourceAction::Pause, id("web"))]);
    }

    #[gpui::test]
    async fn the_copy_entries_put_the_right_text_on_the_clipboard(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(fleet(), cx);
        let rows = vec![
            container("web", RunState::Running),
            container("api", RunState::Running),
        ];
        for (item, expected) in [
            (MenuItem::CopyName, "web\napi"),
            (MenuItem::CopyId, "id-web\nid-api"),
            (MenuItem::CopyImage, "nginx:1\nnginx:1"),
        ] {
            confirm_first(&panel, vec![item.clone()], rows.clone(), cx);
            assert_eq!(
                cx.read_from_clipboard()
                    .and_then(|item| item.text())
                    .as_deref(),
                Some(expected),
                "{item:?}"
            );
        }
    }

    #[gpui::test]
    async fn a_published_port_opens_in_the_browser(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(fleet(), cx);
        confirm_first(
            &panel,
            vec![MenuItem::OpenPorts(vec![PortLink {
                label: "localhost:8080".into(),
                url: "http://localhost:8080".into(),
            }])],
            vec![container("web", RunState::Running)],
            cx,
        );
        assert_eq!(cx.opened_url().as_deref(), Some("http://localhost:8080"));
    }

    #[gpui::test]
    async fn copy_address_puts_every_published_address_on_the_clipboard(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(fleet(), cx);
        confirm_first(
            &panel,
            vec![MenuItem::CopyAddress(vec![
                "10.0.0.7:8080".into(),
                "10.0.0.7:9090".into(),
            ])],
            vec![container("web", RunState::Running)],
            cx,
        );
        assert_eq!(
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some("10.0.0.7:8080\n10.0.0.7:9090")
        );
    }

    #[gpui::test]
    async fn the_remove_entry_goes_through_the_confirmation_and_nothing_else(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (multi_workspace, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
        let backend = Arc::new(FakeBackend::docker().holding(fleet()));
        let engine: Arc<dyn ContainerBackend> = backend.clone();
        let panel = tab_in_workspace(&workspace, vec![engine], cx);
        panel.update(cx, |panel, cx| panel.reload(cx));
        draw(cx);

        confirm_first(
            &panel,
            vec![MenuItem::Remove],
            vec![container("db", RunState::Stopped)],
            cx,
        );
        assert!(backend.destroyed().is_empty());
        let modal = workspace
            .read_with(cx, |workspace, cx| {
                workspace.active_modal::<ConfirmDestructive>(cx)
            })
            .expect("the dialog is open");
        modal.read_with(cx, |modal, _| {
            assert_eq!(modal.plan().confirmation(), "db");
        });
    }
}

mod ports {
    use super::*;
    use util::command::CommandHost;
    use util::paths::PathStyle;

    /// A host with a name for messages and, maybe, an address a browser on this
    /// machine reaches it at. The two are different things: an SSH host is shown
    /// by its nickname and reached at a real host name, a WSL distro is shown by
    /// its distro name and reached at localhost.
    struct Addressed {
        name: Option<&'static str>,
        address: Option<&'static str>,
    }

    impl CommandHost for Addressed {
        fn command(
            &self,
            program: &str,
            _args: &[&str],
            _env: &[(&str, &str)],
            _cwd: Option<&std::path::Path>,
        ) -> anyhow::Result<util::command::Command> {
            Ok(util::command::Command::new(program))
        }

        fn path_style(&self) -> PathStyle {
            PathStyle::local()
        }

        fn remote_name(&self) -> Option<String> {
            self.name.map(String::from)
        }

        fn environment_variable(&self, _name: &str) -> Option<String> {
            None
        }

        fn reachable_address(&self) -> Option<String> {
            self.address.map(String::from)
        }
    }

    fn web() -> Resource {
        Resource {
            detail: vec![
                ("Image", "nginx:1".into()),
                ("Ports", "0.0.0.0:8080->80/tcp, :::8080->80/tcp".into()),
            ],
            ..container("web", RunState::Running)
        }
    }

    /// The items the row menu offers a view whose engines run on `host`.
    fn menu_on(host: Addressed, cx: &mut TestAppContext) -> Vec<MenuItem> {
        init_test(cx);
        let host: Arc<dyn CommandHost> = Arc::new(host);
        cx.update(|cx| cx.set_global(crate::LocalHostForTest(host)));
        let backend: Arc<dyn ContainerBackend> =
            Arc::new(FakeBackend::docker().holding(vec![web()]));
        let (panel, cx) =
            cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
        panel.update(cx, |panel, cx| panel.reload(cx));
        draw(cx);
        panel.read_with(cx, |panel, cx| {
            panel
                .row_menu_plan(&web(), cx)
                .expect("an idle row has a menu")
                .1
        })
    }

    fn links(items: &[MenuItem]) -> Option<Vec<String>> {
        items.iter().find_map(|item| match item {
            MenuItem::OpenPorts(links) => Some(links.iter().map(|link| link.url.clone()).collect()),
            _ => None,
        })
    }

    #[gpui::test]
    async fn this_machine_opens_localhost(cx: &mut TestAppContext) {
        let items = menu_on(
            Addressed {
                name: None,
                address: None,
            },
            cx,
        );
        assert_eq!(
            links(&items).as_deref(),
            Some(&["http://localhost:8080".to_string()][..])
        );
    }

    #[gpui::test]
    async fn an_ssh_host_opens_its_real_address_not_the_name_it_is_shown_by(
        cx: &mut TestAppContext,
    ) {
        let items = menu_on(
            Addressed {
                name: Some("build-box"),
                address: Some("10.0.0.7"),
            },
            cx,
        );
        assert_eq!(
            links(&items).as_deref(),
            Some(&["http://10.0.0.7:8080".to_string()][..]),
            "build-box is a nickname from the SSH config; the address is what answers"
        );
    }

    #[gpui::test]
    async fn a_wsl_distro_opens_localhost_not_its_distro_name(cx: &mut TestAppContext) {
        let items = menu_on(
            Addressed {
                name: Some("Ubuntu-22.04"),
                address: Some("localhost"),
            },
            cx,
        );
        assert_eq!(
            links(&items).as_deref(),
            Some(&["http://localhost:8080".to_string()][..])
        );
    }

    #[gpui::test]
    async fn a_host_with_no_known_address_opens_nothing_and_copies_instead(
        cx: &mut TestAppContext,
    ) {
        let items = menu_on(
            Addressed {
                name: Some("build-box"),
                address: None,
            },
            cx,
        );
        assert_eq!(
            links(&items),
            None,
            "a name that might be a host is not an address"
        );
        assert!(
            items.contains(&MenuItem::CopyAddress(vec!["8080".into()])),
            "the port can still be copied"
        );
    }
}

mod filters {
    use super::*;

    fn mixed() -> Vec<Resource> {
        vec![
            composed("web", RunState::Running, "shop"),
            composed("db", RunState::Stopped, "shop"),
            composed("blog", RunState::Running, "blog"),
            container("loose", RunState::Running),
            image("nginx:1", false),
            image("<none> (abc)", true),
        ]
    }

    #[gpui::test]
    async fn running_only_narrows_the_list_and_toggling_again_restores_it(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(mixed(), cx);
        assert_eq!(visible_names(&panel, cx).len(), 4);
        panel.update(cx, |panel, cx| panel.toggle_running_only(cx));
        draw(cx);
        assert_eq!(visible_names(&panel, cx), ["web", "blog", "loose"]);
        panel.update(cx, |panel, cx| panel.toggle_running_only(cx));
        assert_eq!(visible_names(&panel, cx).len(), 4);
        panel.read_with(cx, |panel, _| assert!(!panel.quick_filter_active()));
    }

    #[gpui::test]
    async fn a_compose_project_keeps_only_its_containers(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(mixed(), cx);
        panel.update(cx, |panel, cx| {
            panel.choose_compose_project(Some("shop".into()), cx)
        });
        assert_eq!(visible_names(&panel, cx), ["web", "db"]);
        panel.update(cx, |panel, cx| panel.toggle_running_only(cx));
        assert_eq!(
            visible_names(&panel, cx),
            ["web"],
            "the two narrow together"
        );
        panel.update(cx, |panel, cx| panel.choose_compose_project(None, cx));
        assert_eq!(visible_names(&panel, cx), ["web", "blog", "loose"]);
    }

    #[gpui::test]
    async fn dangling_only_applies_to_images_and_each_kind_keeps_its_own(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(mixed(), cx);
        panel.update(cx, |panel, cx| panel.toggle_running_only(cx));
        panel.update(cx, |panel, cx| panel.choose_kind(ResourceKind::Image, cx));
        draw(cx);
        assert_eq!(
            visible_names(&panel, cx).len(),
            2,
            "running-only is the containers' filter"
        );
        panel.update(cx, |panel, cx| panel.toggle_dangling_only(cx));
        assert_eq!(visible_names(&panel, cx), ["<none> (abc)"]);

        panel.update(cx, |panel, cx| {
            panel.choose_kind(ResourceKind::Container, cx)
        });
        draw(cx);
        assert_eq!(
            visible_names(&panel, cx),
            ["web", "blog", "loose"],
            "the containers' filter is still on after a trip to the images"
        );
    }

    #[gpui::test]
    async fn the_filters_and_the_search_narrow_together(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(mixed(), cx);
        panel.update(cx, |panel, cx| panel.toggle_running_only(cx));
        panel.update_in(cx, |panel, window, cx| {
            let editor = panel.search_field(window, cx).read(cx).editor().clone();
            editor.set_text("b", window, cx);
        });
        draw(cx);
        assert_eq!(
            visible_names(&panel, cx),
            ["web", "blog"],
            "db matches `b` but is stopped"
        );
    }

    #[gpui::test]
    async fn a_filter_that_matches_nothing_says_so_and_keeps_the_toggles(cx: &mut TestAppContext) {
        let (panel, _backend, cx) = panel_holding(vec![container("db", RunState::Stopped)], cx);
        panel.update(cx, |panel, cx| panel.toggle_running_only(cx));
        draw(cx);
        assert!(visible_names(&panel, cx).is_empty());
        panel.read_with(cx, |panel, cx| {
            assert!(
                panel.shows_search(cx),
                "the toggle that hides everything stays reachable"
            );
            assert!(matches!(panel.state, ListState::Ready(_)));
        });
    }
}

mod keys {
    use super::*;

    fn bind_like_the_keymap(cx: &mut VisualTestContext) {
        use gpui::KeyBinding;
        cx.update(|_, cx| {
            cx.bind_keys([
                KeyBinding::new("down", menu::SelectNext, Some("ContainerResults")),
                KeyBinding::new("up", menu::SelectPrevious, Some("ContainerResults")),
                KeyBinding::new("enter", menu::Confirm, Some("ContainerResults")),
                KeyBinding::new("escape", menu::Cancel, Some("ContainerResults")),
                KeyBinding::new(
                    "space",
                    zed_actions::container::ToggleRowSelection,
                    Some("ContainerResults"),
                ),
                KeyBinding::new(
                    "delete",
                    zed_actions::container::RemoveSelectedRows,
                    Some("ContainerResults"),
                ),
                KeyBinding::new(
                    "cmd-a",
                    zed_actions::container::SelectAllRows,
                    Some("ContainerResults"),
                ),
            ]);
        });
    }

    async fn panel_in_a_workspace(
        cx: &mut TestAppContext,
    ) -> (
        Entity<workspace::Workspace>,
        Entity<ContainerPanel>,
        Arc<FakeBackend>,
        &mut VisualTestContext,
    ) {
        init_test(cx);
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (multi_workspace, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
        let backend = Arc::new(FakeBackend::docker().holding(fleet()));
        let engine: Arc<dyn ContainerBackend> = backend.clone();
        let panel = tab_in_workspace(&workspace, vec![engine], cx);
        panel.update(cx, |panel, cx| panel.reload(cx));
        draw(cx);
        bind_like_the_keymap(cx);
        (workspace, panel, backend, cx)
    }

    #[gpui::test]
    async fn the_arrows_move_space_selects_and_delete_asks_first(cx: &mut TestAppContext) {
        let (workspace, panel, backend, cx) = panel_in_a_workspace(cx).await;
        click("web", command(), cx);
        cx.simulate_keystrokes("down space");
        assert_eq!(selected_names(&panel, cx), ["web", "api"]);
        cx.simulate_keystrokes("down down");
        panel.read_with(cx, |panel, _| {
            assert_eq!(panel.selection.cursor(), Some("id-cache"))
        });
        cx.simulate_keystrokes("down");
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                panel.selection.cursor(),
                Some("id-cache"),
                "stops at the last row"
            )
        });

        cx.simulate_keystrokes("delete");
        let modal = workspace
            .read_with(cx, |workspace, cx| {
                workspace.active_modal::<ConfirmDestructive>(cx)
            })
            .expect("delete opens the confirmation");
        modal.read_with(cx, |modal, _| {
            let names: Vec<&str> = modal
                .plan()
                .targets()
                .iter()
                .map(|target| target.name.as_str())
                .collect();
            assert_eq!(
                names,
                ["web", "api"],
                "the selection, not the row under the cursor"
            );
        });
        assert!(backend.destroyed().is_empty());
    }

    #[gpui::test]
    async fn delete_with_nothing_selected_asks_about_the_row_under_the_cursor(
        cx: &mut TestAppContext,
    ) {
        let (workspace, panel, _backend, cx) = panel_in_a_workspace(cx).await;
        click("web", command(), cx);
        cx.simulate_keystrokes("space");
        assert!(
            selected_names(&panel, cx).is_empty(),
            "space took web back out"
        );
        cx.simulate_keystrokes("down");
        cx.simulate_keystrokes("delete");
        let modal = workspace
            .read_with(cx, |workspace, cx| {
                workspace.active_modal::<ConfirmDestructive>(cx)
            })
            .expect("the cursor row is asked about");
        modal.read_with(cx, |modal, _| {
            assert_eq!(modal.plan().targets().len(), 1);
            assert_eq!(modal.plan().targets()[0].name, "api");
        });
    }

    #[gpui::test]
    async fn enter_opens_the_row_under_the_cursor(cx: &mut TestAppContext) {
        let (_workspace, panel, _backend, cx) = panel_in_a_workspace(cx).await;
        click("api", command(), cx);
        cx.simulate_keystrokes("enter");
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                panel.detail.as_ref().map(|d| d.resource.name.as_str()),
                Some("api")
            );
        });
    }

    #[gpui::test]
    async fn select_all_takes_the_visible_rows_and_escape_clears(cx: &mut TestAppContext) {
        let (_workspace, panel, _backend, cx) = panel_in_a_workspace(cx).await;
        click("web", command(), cx);
        panel.update(cx, |panel, cx| panel.toggle_running_only(cx));
        draw(cx);
        cx.simulate_keystrokes("cmd-a");
        assert_eq!(selected_names(&panel, cx), ["web", "api", "cache"]);
        cx.simulate_keystrokes("escape");
        assert!(selected_names(&panel, cx).is_empty());
    }

    /// The keys are bound under the list's own context, so a text box above it
    /// and a log terminal beside it never see them.
    #[gpui::test]
    async fn the_search_box_does_not_hear_the_list_keys(cx: &mut TestAppContext) {
        let (workspace, panel, backend, cx) = panel_in_a_workspace(cx).await;
        click("web", command(), cx);
        click("api", command(), cx);

        cx.update(|window, cx| {
            let focus = panel
                .read(cx)
                .search
                .as_ref()
                .expect("the field is drawn")
                .input_focus(cx);
            window.focus(&focus, cx);
        });
        // Both words are in every row's image, so the rows all stay.
        cx.simulate_input("n x");
        let typed = panel.read_with(cx, |panel, cx| {
            panel
                .search
                .as_ref()
                .expect("the field is drawn")
                .typed_text(cx)
        });
        assert_eq!(
            typed, "n x",
            "a space typed in the box is a space, not a toggle"
        );
        cx.simulate_keystrokes("space delete down");
        assert_eq!(selected_names(&panel, cx), ["web", "api"], "nothing moved");
        assert!(
            workspace
                .read_with(cx, |workspace, cx| workspace
                    .active_modal::<ConfirmDestructive>(cx))
                .is_none(),
            "delete in the search box opened no removal"
        );
        assert!(backend.destroyed().is_empty());

        // The same keys, with the list focused, do act: the silence above is
        // the context's doing and not a keymap that binds nothing.
        cx.update(|window, cx| {
            let focus = panel.read(cx).results_focus.clone();
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("space");
        assert_eq!(
            selected_names(&panel, cx),
            ["web"],
            "space took the cursor row out"
        );
    }
}

mod keymaps {
    const MACOS: &str = include_str!("../../../assets/keymaps/default-macos.json");
    const LINUX: &str = include_str!("../../../assets/keymaps/default-linux.json");
    const WINDOWS: &str = include_str!("../../../assets/keymaps/default-windows.json");

    /// The text of the first `"context": "ContainerResults"` block.
    fn block(keymap: &str) -> &str {
        let start = keymap
            .find("\"context\": \"ContainerResults\"")
            .expect("the list keys are bound");
        let rest = &keymap[start..];
        &rest[..rest.find("\n  },").expect("the block ends")]
    }

    #[test]
    fn every_platform_binds_the_list_keys_to_the_list_only() {
        for (platform, keymap) in [("macos", MACOS), ("linux", LINUX), ("windows", WINDOWS)] {
            let list = block(keymap);
            for action in [
                "\"down\": \"menu::SelectNext\"",
                "\"up\": \"menu::SelectPrevious\"",
                "\"enter\": \"menu::Confirm\"",
                "\"space\": \"container::ToggleRowSelection\"",
                "\"delete\": \"container::RemoveSelectedRows\"",
                "\"backspace\": \"container::RemoveSelectedRows\"",
                "::SelectAllRows\"",
            ] {
                assert!(list.contains(action), "{platform} lacks {action}");
            }
            assert!(
                !keymap.contains("\"context\": \"ContainerPanel\""),
                "{platform} binds list keys for the whole panel, search box included"
            );
        }
    }
}
