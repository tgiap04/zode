use crate::DatabasePanel;
use fs::FakeFs;
use gpui::{AppContext as _, TestAppContext, UpdateGlobal as _};
use project::Project;
use settings::{Settings as _, SettingsStore};
use workspace::MultiWorkspace;

/// What the fake driver says the `users` table is made of.
pub(crate) fn users_columns() -> Vec<database::protocol::ColumnDef> {
    vec![
        database::protocol::ColumnDef {
            name: "id".into(),
            type_name: "integer".into(),
            nullable: false,
            primary_key: true,
        },
        database::protocol::ColumnDef {
            name: "email".into(),
            type_name: "text".into(),
            nullable: true,
            primary_key: false,
        },
    ]
}

fn init_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let settings_store = SettingsStore::test(cx);
        cx.set_global(settings_store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        project::DisableAiSettings::register(cx);
        // The factory behind every `InputField`, which the dialog and the
        // password prompt are built from.
        editor::init(cx);
        // Registers the workspace actions, so a test can reach the buttons the
        // way a click does instead of calling the handler behind them. A button
        // wired to nothing is a defect this crate's own history has already
        // shipped once.
        crate::init(cx);
    });
}

async fn workspace_with_panel(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<workspace::Workspace>,
    gpui::Entity<DatabasePanel>,
    &mut gpui::VisualTestContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

    // A tab, not a dock panel: the database has no column any more.
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        let handle = cx.weak_entity();
        let languages = workspace.project().read(cx).languages().clone();
        let panel = cx.new(|cx| DatabasePanel::standalone(handle, languages, window, cx));
        workspace.add_item_to_active_pane(Box::new(panel.clone()), None, true, window, cx);
        panel
    });

    (workspace, panel, cx)
}

/// The database is a pane item now, so it lands in a tab and nowhere else.
///
/// It used to be a dock panel with an own column; that column is gone, because a
/// result grid is the one thing here that cannot be made narrow and stay
/// readable, and a column is the one place that cannot be wide.
#[gpui::test]
async fn the_panel_lands_in_a_tab(cx: &mut TestAppContext) {
    let (workspace, panel, cx) = workspace_with_panel(cx).await;

    workspace.read_with(cx, |workspace, cx| {
        assert_eq!(
            workspace
                .items_of_type::<DatabasePanel>(cx)
                .map(|view| view.entity_id())
                .collect::<Vec<_>>(),
            vec![panel.entity_id()],
            "exactly one, and it is a tab"
        );
        assert!(
            workspace.active_item(cx).is_some(),
            "and it is what the pane is showing"
        );
    });
}

/// Settings fire on every keystroke in the settings file. Tearing the node list
/// down each time would drop a session someone is browsing because they typed a
/// character in an unrelated entry.
#[gpui::test]
async fn editing_one_connection_leaves_the_others_alone(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;

    set_connections(cx, &[("a", "/tmp/a.sqlite"), ("b", "/tmp/b.sqlite")]);
    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.connection_names(), vec!["a", "b"]);
    });

    // `b` moves; `a` is untouched and must be the very same node.
    set_connections(
        cx,
        &[("a", "/tmp/a.sqlite"), ("b", "/tmp/elsewhere.sqlite")],
    );
    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.connection_names(), vec!["a", "b"]);
        assert_eq!(
            panel.connection_url("b").as_deref(),
            Some("/tmp/elsewhere.sqlite"),
            "the moved connection must follow the settings"
        );
    });
}

/// A connection removed from settings must leave the tree, taking whatever it
/// had open with it.
#[gpui::test]
async fn removing_a_connection_from_settings_removes_its_node(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;

    set_connections(cx, &[("a", "/tmp/a.sqlite"), ("b", "/tmp/b.sqlite")]);
    set_connections(cx, &[("a", "/tmp/a.sqlite")]);

    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.connection_names(), vec!["a"]);
    });
}

/// An entry missing any of name/driver/url cannot be reached, and a node that
/// fails on every click is worse than no node. Settings are hand-written, so
/// this happens.
#[gpui::test]
async fn a_half_written_connection_is_dropped_rather_than_drawn(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;

    cx.update(|_window, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.database.get_or_insert_default().connections = Some(vec![
                    settings::DatabaseConnectionContent {
                        name: Some("whole".into()),
                        driver: Some("sqlite".into()),
                        url: Some("/tmp/whole.sqlite".into()),
                        safety: None,
                    },
                    settings::DatabaseConnectionContent {
                        name: Some("no driver".into()),
                        driver: None,
                        url: Some("/tmp/x.sqlite".into()),
                        safety: None,
                    },
                ]);
            });
        });
    });
    cx.run_until_parked();

    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.connection_names(), vec!["whole"]);
    });
}

/// Nothing is opened by simply being configured: a driver process per
/// connection at startup is a cost nobody asked for.
#[gpui::test]
async fn configured_connections_stay_closed_until_clicked(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    set_connections(cx, &[("a", "/tmp/a.sqlite")]);

    panel.read_with(cx, |panel, _| {
        assert_eq!(
            panel.rows().len(),
            1,
            "an unopened connection contributes one row and no children"
        );
    });
}

/// The menu is owned by the panel rather than by the row it was opened from:
/// rows are recycled by the virtualised list, so one owned by a row would
/// vanish the moment the tree scrolled.
#[gpui::test]
async fn right_clicking_a_connection_opens_a_menu_the_panel_owns(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    set_connections(cx, &[("a", "/tmp/a.sqlite")]);

    panel.update_in(cx, |panel, window, cx| {
        panel.deploy_connection_menu(0, gpui::point(gpui::px(10.), gpui::px(10.)), window, cx);
        assert!(
            panel.context_menu.is_some(),
            "a right-click on a connection must leave a menu on the panel"
        );
    });

    // Drawn with the menu up: the deferred anchor is the part that would fail,
    // and asserting on the field alone would never reach it.
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

fn set_connections(cx: &mut gpui::VisualTestContext, entries: &[(&str, &str)]) {
    set_connections_for(cx, "sqlite", entries);
}

fn set_connections_for(cx: &mut gpui::VisualTestContext, driver: &str, entries: &[(&str, &str)]) {
    let entries: Vec<_> = entries
        .iter()
        .map(|(name, url)| settings::DatabaseConnectionContent {
            name: Some((*name).to_string()),
            driver: Some(driver.into()),
            url: Some((*url).to_string()),
            safety: None,
        })
        .collect();

    cx.update(|_window, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.database.get_or_insert_default().connections = Some(entries.clone());
            });
        });
    });
    cx.run_until_parked();
}

/// The invariant phase 07 exists to check, asserted here so it fails the moment
/// it is broken rather than three phases later.
///
/// Nothing in this crate may name an engine: a driver formats its own values
/// and describes its own schema, so PostgreSQL must arrive without a line
/// changing in here. An `if engine == "postgres"` would make that phase's
/// acceptance meaningless.
#[test]
fn no_source_file_in_this_crate_names_an_engine() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();

    for entry in std::fs::read_dir(&source).expect("the crate has a src directory") {
        let path = entry.expect("a readable entry").path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        // This test, the registry that must name the drivers Zode ships, and
        // the dialect -- the one module statements are built in, which has to
        // know which engine it is writing for -- are the places an engine name
        // legitimately appears.
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if matches!(
            name.as_ref(),
            "driver_registry.rs" | "database_panel_tests.rs" | "sql_dialect.rs"
        ) {
            continue;
        }

        // Comments stripped first: the invariant is about what the code
        // *does*, and the comments explaining why an engine must not be named
        // inevitably name one.
        let text = std::fs::read_to_string(&path).expect("a readable source file");
        // Stops at `#[cfg(test)]`, which in this crate is always the last item
        // in a file: a fixture may name an engine as *data* -- that is not the
        // shipped code deciding anything.
        let text = text
            .split_once("#[cfg(test)]")
            .map_or(text.as_str(), |(code, _tests)| code);

        let code: String = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
            .to_ascii_lowercase();

        for engine in ["sqlite", "postgres", "mysql"] {
            if code.contains(engine) {
                offenders.push(format!("{name} branches on `{engine}`"));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "this crate must not know which engines exist -- {}",
        offenders.join(", ")
    );
}

/// A drag moves a region by the distance travelled, which means the first move
/// of a drag has nothing to measure against and must move nothing. Getting this
/// wrong makes every grab jump by the distance from wherever the pointer last
/// happened to be.
#[gpui::test]
async fn a_drag_moves_a_region_by_how_far_it_travelled(cx: &mut TestAppContext) {
    use crate::panel_layout::Split;
    use gpui::px;

    let (_workspace, panel, cx) = workspace_with_panel(cx).await;

    panel.update(cx, |panel, cx| {
        let start = panel.tree_height;

        panel.drag_split(Split::TreeAndSql, px(300.), cx);
        assert_eq!(
            panel.tree_height, start,
            "the first move of a drag only sets the reference"
        );

        panel.drag_split(Split::TreeAndSql, px(340.), cx);
        assert_eq!(panel.tree_height, start + px(40.));

        panel.drag_split(Split::TreeAndSql, px(320.), cx);
        assert_eq!(panel.tree_height, start + px(20.), "and back again");

        assert_eq!(
            panel.sql_height,
            crate::panel_layout::DEFAULT_SQL_HEIGHT,
            "dragging one boundary must leave the other where it was"
        );
    });
}

/// Dragged past the top, a region must stop rather than become a sliver that
/// cannot be grabbed to drag back.
#[gpui::test]
async fn a_region_cannot_be_dragged_away_to_nothing(cx: &mut TestAppContext) {
    use crate::panel_layout::Split;
    use gpui::px;

    let (_workspace, panel, cx) = workspace_with_panel(cx).await;

    panel.update(cx, |panel, cx| {
        panel.drag_split(Split::SqlAndResults, px(500.), cx);
        panel.drag_split(Split::SqlAndResults, px(-5_000.), cx);
        assert!(
            panel.sql_height > px(0.),
            "a region dragged past the top must keep a height someone can grab"
        );
    });
}

/// The one boundary here that is a vertical line. It is measured on the
/// pointer's x, and a drag that read y instead would move the table list by
/// however far the pointer happened to fall down the screen.
#[gpui::test]
async fn the_table_list_edge_is_dragged_sideways(cx: &mut TestAppContext) {
    use crate::panel_layout::Split;
    use gpui::px;

    let (_workspace, panel, cx) = workspace_with_panel(cx).await;

    panel.update(cx, |panel, cx| {
        let start = panel.tree_width;

        panel.drag_split(Split::TreeAndBody, px(400.), cx);
        assert_eq!(
            panel.tree_width, start,
            "the first move of a drag only sets the reference"
        );

        panel.drag_split(Split::TreeAndBody, px(460.), cx);
        assert_eq!(panel.tree_width, start + px(60.));

        assert_eq!(
            panel.tree_height,
            crate::panel_layout::DEFAULT_TREE_HEIGHT,
            "widening the list must not also make it taller"
        );

        panel.drag_split(Split::TreeAndBody, px(-5_000.), cx);
        assert!(
            panel.tree_width > px(0.),
            "a list dragged past the left edge must keep a width someone can grab"
        );
    });
}

/// A modal in a real window, for the add-connection tests.
///
/// Built through the workspace like the real one is: it takes the project's
/// `Fs` from there, and a modal handed a dangling workspace would pass tests the
/// shipped one could not.
/// Builds the add-connection modal, with `installed` standing in for drivers
/// that have been downloaded.
///
/// Zode bundles no drivers and a test runs beside a test binary, so by default
/// nothing resolves and every engine is correctly listed as absent. A test
/// about what happens once a driver is present has to say which.
pub(crate) async fn modal_for_test<'a>(
    cx: &'a mut TestAppContext,
    installed: &[&str],
) -> (
    gpui::Entity<crate::connection_modal::ConnectionModal>,
    &'a mut gpui::VisualTestContext,
) {
    modal_on_host_for_test(cx, installed, |_| crate::driver_registry::DriverHost::Local).await
}

/// [`modal_for_test`], listing and starting the drivers of whatever host
/// `drivers` names instead of this machine's.
pub(crate) async fn modal_on_host_for_test<'a>(
    cx: &'a mut TestAppContext,
    installed: &[&str],
    drivers: impl FnOnce(&mut gpui::App) -> crate::driver_registry::DriverHost,
) -> (
    gpui::Entity<crate::connection_modal::ConnectionModal>,
    &'a mut gpui::VisualTestContext,
) {
    init_test(cx);
    let drivers = cx.update(|cx| {
        for id in installed {
            crate::driver_registry::install_for_test(id, cx);
        }
        drivers(cx)
    });
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

    let modal = cx.update(|window, cx| {
        cx.new(|cx| {
            crate::connection_modal::ConnectionModal::new(
                workspace.downgrade(),
                drivers,
                window,
                cx,
            )
        })
    });
    (modal, cx)
}

/// `Workspace::register_action` runs its handler inside `cx.listener`, which
/// means the workspace is already leased. A handler that reaches back through a
/// workspace handle aborts the process -- the same trap `Panel::position` is
/// commented about in `database_panel.rs`, reached by a different road.
///
/// Dispatched rather than called directly, because calling the method is
/// exactly what does *not* reproduce it: the header button works and the empty
/// state's button crashes, and only the action path tells them apart.
#[gpui::test]
async fn dispatching_add_connection_opens_the_dialog_rather_than_aborting(cx: &mut TestAppContext) {
    init_test(cx);
    cx.update(crate::init);

    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

    workspace.update_in(cx, |workspace, window, cx| {
        let panel = cx.new(|cx| DatabasePanel::new(workspace, window, cx));
        workspace.add_item_to_active_pane(Box::new(panel), None, true, window, cx);
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        window.dispatch_action(Box::new(zed_actions::database::AddConnection), cx);
    });
    cx.run_until_parked();

    workspace.read_with(cx, |workspace, cx| {
        assert!(
            workspace
                .active_modal::<crate::connection_modal::ConnectionModal>(cx)
                .is_some(),
            "the action must open the dialog"
        );
    });

    // The other action this crate registers, dispatched for the same reason:
    // what is being checked is that no handler reaches back through a workspace
    // handle, and that is a property of the whole set, not of one entry.
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(zed_actions::database::ToggleDatabase), cx);
    });
    cx.run_until_parked();
}

/// Closing one connection must not empty another. The index comparison is the
/// only thing keeping them apart, and getting it wrong wipes a grid nobody
/// touched.
#[gpui::test]
async fn disconnecting_clears_only_that_connection(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    set_connections(cx, &[("a", "/tmp/a.sqlite"), ("b", "/tmp/b.sqlite")]);

    panel.update(cx, |panel, _cx| {
        panel.active = Some(1);
        panel.open_table = Some(crate::database_panel::OpenTable {
            connection: 1,
            schema: "main".into(),
            table: "people".into(),
            columns: None,
            columns_load: 0,
        });

        panel.forget_connection_state(0);
        assert_eq!(panel.active, Some(1), "connection 0 is not connection 1");
        assert!(panel.open_table.is_some());

        panel.forget_connection_state(1);
        assert_eq!(panel.active, None);
        assert!(
            panel.open_table.is_none(),
            "a grid from a closed session cannot be paged or re-run"
        );
    });
}

/// Offering to close what is already closed is a button that does nothing, so
/// the state it reads has to be right.
#[gpui::test]
async fn a_connection_that_was_never_opened_reports_itself_closed(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    set_connections(cx, &[("a", "/tmp/a.sqlite")]);

    panel.update(cx, |panel, cx| {
        assert!(!panel.is_connected(0));
        // A no-op, and it must stay one rather than clearing anything.
        panel.active = Some(0);
        panel.disconnect(0, cx);
        assert_eq!(panel.active, Some(0));
    });
}

/// The row's power button does two jobs and must never offer the wrong one:
/// closing what is already closed, or opening what is already open.
#[gpui::test]
async fn a_closed_connection_offers_to_open_again(cx: &mut TestAppContext) {
    // The only test here that reaches the connect path, and connecting starts a
    // driver: `Session::open` builds a `StdioTransport`, which spawns a child
    // process through `smol::process`. That brings up the `async-io` reactor on
    // a thread of its own, and the reactor then wakes gpui tasks from there --
    // which the test scheduler counts as non-determinism and reports at
    // `end_test`, whether or not this test ever looks at the driver.
    //
    // Same reason `neovim_backed_test_context` and the `debugger_ui` tests say
    // it: a test that starts a real process cannot also promise every wake-up
    // arrives on one thread. It went unnoticed until a macOS runner ran the
    // test eighteen times slower than a developer machine and gave the reactor
    // long enough to fire inside the test's own window.
    cx.executor().allow_parking();

    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    set_connections(cx, &[("a", "/tmp/a.sqlite")]);
    // Zode bundles no drivers and a test runs beside a test binary, so nothing
    // resolves and `reconnect` would refuse before it ever reached the driver.
    // What this test is about is the button, so the driver is put there.
    cx.update(|_window, cx| crate::driver_registry::install_for_test("sqlite", cx));

    panel.update_in(cx, |panel, window, cx| {
        assert!(!panel.is_connected(0), "nothing opens by being configured");

        // What the button calls when the row is not connected. It leaves the
        // node reaching for its driver rather than sitting where it was.
        panel.reconnect(0, window, cx);
        assert!(
            panel.is_reaching_for_a_driver(0),
            "connecting again must actually start, not quietly do nothing"
        );
    });
}

/// Two failures that used to be one sentence.
///
/// A driver Zode has never heard of is a settings or extension problem. A
/// driver Zode ships but has not downloaded yet is a click away from working.
/// Saying the second in the words of the first sent people looking for an
/// extension that was never the answer -- and now that Zode bundles no drivers
/// at all, that is the common case rather than the rare one.
#[gpui::test]
async fn a_driver_that_is_merely_undownloaded_does_not_blame_a_missing_extension(
    cx: &mut TestAppContext,
) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    set_connections(cx, &[("a", "/tmp/a.sqlite")]);

    panel.update_in(cx, |panel, window, cx| {
        panel.reconnect(0, window, cx);

        let failure = panel.failure(0).expect("connecting must fail, and say why");
        assert!(
            failure.contains("not installed") && failure.contains("connection dialog"),
            "a driver that has yet to be downloaded must offer the download: {failure}"
        );
        assert!(
            !failure.contains("extension"),
            "`sqlite` is a driver Zode ships; nothing about an extension is true here: {failure}"
        );
        assert!(
            !panel.is_reaching_for_a_driver(0),
            "nothing may be started for a driver that is not on the machine"
        );
    });
}

/// In a remote window a connection's driver is the host's: the host is asked
/// for it, and the host's path is what runs, through the host. This machine
/// has no `postgres` driver, so any fallback to its drivers fails here.
#[gpui::test]
async fn opening_a_connection_in_a_remote_window_runs_the_hosts_driver(cx: &mut TestAppContext) {
    use crate::driver_registry::remote_test_support::{
        HOST_NAME, REMOTE_POSTGRES, RecordingHost, remote_host,
    };
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    let local_store = tempfile::tempdir().expect("a temporary directory");
    let service = std::rc::Rc::new(RecordingHost::with_postgres());
    cx.update(|_window, cx| {
        cx.set_global(zed_credentials_provider::ZedCredentialsProvider(
            std::sync::Arc::new(MemoryCredentials::default()),
        ));
        crate::driver_registry::set_installer_for_test(local_store.path().to_path_buf(), cx);
    });
    panel.update(cx, |panel, cx| {
        panel.drivers = Some(remote_host(service.clone(), cx));
    });
    set_connections_for(cx, "postgres", &[("staging", "postgres://localhost/app")]);

    panel.update_in(cx, |panel, window, cx| {
        panel.toggle_connection(0, window, cx)
    });
    cx.run_until_parked();

    assert_eq!(
        service.asked.borrow().clone(),
        vec![("postgres".to_string(), false)],
        "the host is asked for the driver, and asked to install nothing"
    );
    assert_eq!(
        service.command_host.programs.lock().unwrap().clone(),
        vec![REMOTE_POSTGRES.to_string()],
        "the host's path, run through the host"
    );
    panel.read_with(cx, |panel, _| {
        let failure = panel.failure(0).expect("the test host runs nothing");
        assert!(failure.contains(&format!("on {HOST_NAME}")), "{failure}");
    });
    cx.update(|_window, cx| {
        assert!(
            !crate::driver_registry::global(cx)
                .read(cx)
                .get("postgres")
                .is_some_and(|driver| driver.is_installed()),
            "nothing of this machine's stood in"
        );
        assert_eq!(crate::driver_registry::installer(cx).waiters("postgres"), 0);
    });
    assert!(
        database::install::test_support::directory_entries(local_store.path(), "postgres")
            .is_empty()
    );
}

/// A view standing on its own lays the table list beside the data once it is
/// wide enough, without anybody toggling full screen.
///
/// Measured off the drawn tab rather than off `side_by_side()`: the flag and the
/// element tree are resolved in different places, and agreeing with the flag
/// would prove nothing about the layout.
///
/// What actually separates the two layouts here is the data view *existing*: the
/// stacked layout holds it back until there is a connection to run against, and
/// the side-by-side one draws all three regions regardless. That is the
/// assertion doing the work -- checked by forcing the stacked layout, which fails
/// on the missing region. The geometry below is a second, weaker check that the
/// regions are ordered across rather than down; it cannot fail on its own.
#[gpui::test]
async fn a_wide_editor_tab_stands_the_table_list_beside_the_data(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    set_connections(cx, &[("local", "/tmp/a.sqlite")]);

    cx.run_until_parked();

    panel.update_in(cx, |panel, window, cx| {
        window.focus(&panel.focus_handle.clone(), cx);
    });
    cx.run_until_parked();
    cx.dispatch_action(zed_actions::database::OpenInEditorTab);
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();

    let tree = cx
        .debug_bounds("database-tree-column")
        .expect("the table list must be drawn in the tab");
    let data = cx.debug_bounds("database-data-view").expect(
        "a standalone view must draw the data view with no connection open -- \
             the column holds it back, the side-by-side layout does not",
    );

    assert!(
        tree.right() <= data.left(),
        "a wide tab must stand the list beside the data, got the list ending at {:?} \
         and the data starting at {:?}",
        tree.right(),
        data.left()
    );
}

/// The floating-window button opens a second window and survives doing it.
///
/// The survival is the point. Both handlers run inside `register_action`, which
/// leases the workspace for the whole call, and anything reached from there that
/// takes a workspace handle and calls `update` on it aborts the process -- a trap
/// this crate's own comments record paying for twice. Opening a window from that
/// position is exactly where it would happen again, so this dispatches the real
/// action rather than calling the function.
///
/// It does not check what the window looks like: a test window is not an
/// `NSPanel`, so `WindowKind::Floating` and its window level are beyond what any
/// test here can see. Those need an eye on a real build.
#[gpui::test]
async fn opening_in_a_floating_window_adds_a_window_without_aborting(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    set_connections(cx, &[("local", "/tmp/a.sqlite")]);

    cx.run_until_parked();

    let before = cx.update(|_window, cx| cx.windows().len());

    panel.update_in(cx, |panel, window, cx| {
        window.focus(&panel.focus_handle.clone(), cx);
    });
    cx.run_until_parked();
    cx.dispatch_action(zed_actions::database::OpenInFloatingWindow);
    cx.run_until_parked();

    let after = cx.update(|_window, cx| cx.windows().len());
    assert_eq!(
        after,
        before + 1,
        "the action must open exactly one window, went from {before} to {after}"
    );
}

/// A standalone view must lay its *regions* out sideways too, not just pick the
/// sideways top-level branch.
///
/// The defect this pins: the top-level branch moved to `side_by_side()` while
/// `render_tree` and `render_nothing_chosen` kept reading `full_screen`, which is
/// false in a tab. The table list then took the column's `flex_1()` under a row
/// parent and drew, in its own comment's words, full width and zero rows -- a
/// wide blank panel with no error anywhere.
///
/// Two assertions because two things were wrong, and each pins one of them: the
/// list is a fixed width rather than grown to fill, and the waiting-room
/// placeholder is the roomy one rather than the column's single grey line.
#[gpui::test]
async fn a_standalone_view_lays_its_regions_out_sideways(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    set_connections(cx, &[("local", "/tmp/a.sqlite")]);

    cx.run_until_parked();

    panel.update_in(cx, |panel, window, cx| {
        window.focus(&panel.focus_handle.clone(), cx);
    });
    cx.run_until_parked();
    cx.dispatch_action(zed_actions::database::OpenInEditorTab);
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();

    let tree = cx
        .debug_bounds("database-tree-column")
        .expect("the table list must be drawn in the tab");
    assert_eq!(
        tree.size.width,
        crate::panel_layout::DEFAULT_TREE_WIDTH,
        "the list must hold its own width beside the data, not grow to fill the row -- \
         growing is the column's branch, and under a row parent it draws zero rows"
    );

    assert!(
        cx.debug_bounds("database-no-table-placeholder").is_some(),
        "a view laid out sideways must draw the roomy waiting-room placeholder, \
         not the column's one-line note"
    );
}

/// The rail button opens the tab, and a second press steps back.
///
/// Put away rather than closed: closing would end every open session and lose a
/// half-written statement over the second press of a button whose whole job is to
/// be pressed twice.
#[gpui::test]
async fn the_toggle_action_opens_the_tab_and_puts_it_away(cx: &mut TestAppContext) {
    init_test(cx);
    cx.update(crate::init);

    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

    assert_eq!(
        workspace.read_with(cx, |workspace, cx| workspace
            .items_of_type::<DatabasePanel>(cx)
            .count()),
        0,
        "nothing is open before the button is pressed"
    );

    cx.update(|window, cx| {
        window.dispatch_action(Box::new(zed_actions::database::ToggleDatabase), cx)
    });
    cx.run_until_parked();

    assert_eq!(
        workspace.read_with(cx, |workspace, cx| workspace
            .items_of_type::<DatabasePanel>(cx)
            .count()),
        1,
        "the action the rail button dispatches must open the tab; if this fails \
         the button is wired to something nothing handles"
    );

    // Pressed again with nothing else in the pane: put away, never closed.
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(zed_actions::database::ToggleDatabase), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        workspace.read_with(cx, |workspace, cx| workspace
            .items_of_type::<DatabasePanel>(cx)
            .count()),
        1,
        "the tab must still be there, with its sessions -- put away, not closed"
    );
}

/// Pressing again must bring the existing tab forward, not open a second one.
///
/// Two would be two sets of connections and two scratch buffers, each unaware of
/// the other.
#[gpui::test]
async fn the_toggle_action_never_opens_a_second_tab(cx: &mut TestAppContext) {
    init_test(cx);
    cx.update(crate::init);

    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

    for _ in 0..3 {
        cx.update(|window, cx| {
            window.dispatch_action(Box::new(zed_actions::database::ToggleDatabase), cx)
        });
        cx.run_until_parked();
    }

    assert_eq!(
        workspace.read_with(cx, |workspace, cx| workspace
            .items_of_type::<DatabasePanel>(cx)
            .count()),
        1,
        "however many times it is pressed, there is one database"
    );
}

/// What `render_row` draws for each kind of line, laid out one above the other.
struct TreeRows {
    panel: gpui::Entity<DatabasePanel>,
    /// Each row with the selector its bounds are read back under.
    rows: Vec<(&'static str, crate::connection_tree::Row)>,
}

impl gpui::Render for TreeRows {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use ui::prelude::*;

        let rows = self
            .rows
            .iter()
            .enumerate()
            .map(|(position, (selector, row))| {
                let element = self
                    .panel
                    .update(cx, |panel, cx| panel.render_row(position, row, window, cx));
                div()
                    .debug_selector(|| (*selector).into())
                    .child(element)
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        v_flex().w(gpui::px(400.)).children(rows)
    }
}

/// A panel with a connection `a` in it, and `rows` drawn from it in a window of
/// their own.
///
/// No workspace: drawing a row asks nothing of one, and a window of its own is
/// the only way to put a bare element tree on screen and measure it.
async fn drawn_rows<'a>(
    rows: Vec<(&'static str, crate::connection_tree::Row)>,
    cx: &'a mut TestAppContext,
) -> (gpui::Entity<DatabasePanel>, &'a mut gpui::VisualTestContext) {
    use crate::database_panel::SchemaNode;
    use database::protocol::SchemaRef;

    init_test(cx);
    let languages = std::sync::Arc::new(language::LanguageRegistry::new(cx.executor()));
    let (view, cx) = cx.add_window_view(|window, cx| {
        let panel = cx.new(|cx| {
            DatabasePanel::standalone(
                gpui::WeakEntity::new_invalid(),
                languages.clone(),
                window,
                cx,
            )
        });
        TreeRows {
            panel,
            rows: Vec::new(),
        }
    });
    let panel = view.read_with(cx, |view, _| view.panel.clone());
    set_connections(cx, &[("a", "/tmp/a.sqlite")]);
    panel.update(cx, |panel, _| {
        panel.connections[0].schemas.push(SchemaNode {
            schema: SchemaRef {
                name: "main".into(),
                is_default: true,
            },
            expanded: true,
            tables: None,
            tables_load: 0,
        });
    });

    view.update(cx, |view, cx| {
        view.rows = rows;
        cx.notify();
    });
    draw_again(cx);
    (panel, cx)
}

fn table_row(depth: usize) -> crate::connection_tree::Row {
    crate::connection_tree::Row::Table {
        index: 0,
        schema: "main".into(),
        name: "users".into(),
        kind: database::protocol::TableKind::Table,
        depth,
        open: false,
    }
}

/// `uniform_list` measures the first row and gives every other row that height,
/// so a connection (which carries a button) drawn taller than a note or a column
/// (which carry none) is either clipped or leaves a gap, depending on which way
/// it falls.
#[gpui::test]
async fn tree_rows_are_one_height_whatever_they_carry(cx: &mut TestAppContext) {
    use crate::connection_tree::Row;

    let (_panel, cx) = drawn_rows(
        vec![
            ("tree-row-connection", Row::Connection { index: 0 }),
            (
                "tree-row-schema",
                Row::Schema {
                    index: 0,
                    schema_index: 0,
                },
            ),
            ("tree-row-table", table_row(2)),
            (
                "tree-row-column",
                Row::Column {
                    index: 0,
                    name: "id".into(),
                    type_name: "integer".into(),
                    primary_key: true,
                    nullable: false,
                    depth: 3,
                },
            ),
            (
                "tree-row-note",
                Row::Note {
                    text: "No tables".into(),
                    depth: 2,
                },
            ),
        ],
        cx,
    )
    .await;

    let heights: Vec<_> = [
        "tree-row-connection",
        "tree-row-schema",
        "tree-row-table",
        "tree-row-column",
        "tree-row-note",
    ]
    .into_iter()
    .map(|selector| bounds_of(cx, selector).size.height)
    .collect();
    assert!(
        heights.iter().all(|height| *height == heights[0]),
        "every kind of line must be one height, got {heights:?}"
    );

    let button = cx.update(|window, _| ui::ButtonSize::Default.rems().to_pixels(window.rem_size()));
    assert!(
        (heights[0] - button).abs() < gpui::px(0.5),
        "and that height is a button's, got {:?} against {button:?}",
        heights[0]
    );
}

/// The indent is a margin inside the row, so a click handler hung on the row
/// itself leaves the indent dead while the highlight spans it. A click on the
/// far left of a deep row is a click on that row.
#[gpui::test]
async fn a_click_in_the_indent_of_a_row_is_a_click_on_the_row(cx: &mut TestAppContext) {
    let (panel, cx) = drawn_rows(vec![("tree-row-table", table_row(2))], cx).await;
    panel.read_with(cx, |panel, _| {
        assert!(panel.open_table.is_none(), "nothing is open to begin with");
    });

    let row = bounds_of(cx, "tree-row-table");
    cx.simulate_click(
        gpui::point(row.left() + gpui::px(4.), row.center().y),
        gpui::Modifiers::none(),
    );
    cx.run_until_parked();

    panel.read_with(cx, |panel, _| {
        assert!(
            panel.open_table.is_some(),
            "a click in the indent must open the table, the way a click on its name does"
        );
    });
}

/// Opens the add-connection dialog the way the buttons do, and draws it.
async fn drawn_dialog(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<crate::connection_modal::ConnectionModal>,
    &mut gpui::VisualTestContext,
) {
    let (workspace, _panel, cx) = workspace_with_panel(cx).await;
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(zed_actions::database::AddConnection), cx);
    });
    cx.run_until_parked();
    let modal = workspace
        .read_with(cx, |workspace, cx| {
            workspace.active_modal::<crate::connection_modal::ConnectionModal>(cx)
        })
        .expect("the action must open the dialog");
    draw_again(cx);
    (modal, cx)
}

fn draw_again(cx: &mut gpui::VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

fn bounds_of(
    cx: &mut gpui::VisualTestContext,
    selector: &'static str,
) -> gpui::Bounds<gpui::Pixels> {
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} must be drawn"))
}

/// An engine row's name and description sat in a column that was given no
/// width of its own, so the row squeezed it to nothing and both labels drew
/// as a lone ellipsis.
#[gpui::test]
async fn an_engine_row_gives_its_name_the_room_the_row_has(cx: &mut TestAppContext) {
    let (_modal, cx) = drawn_dialog(cx).await;

    let list = bounds_of(cx, "database-engine-list");
    let text = bounds_of(cx, "database-engine-text");
    assert!(
        text.size.width > list.size.width / 2.,
        "the name column must take the row's free width, got {text:?} in {list:?}"
    );
}

/// The dialog used to put Cancel, Save and Save & Connect in its header on every
/// step after the first, and Cancel and Continue in a footer on the first. Two
/// places for the same job is the defect: the actions belong under what they
/// act on, whichever screen it is.
#[gpui::test]
async fn the_dialog_keeps_its_actions_in_a_footer_on_every_step(cx: &mut TestAppContext) {
    use crate::connection_modal::Step;
    use crate::driver_registry::CatalogueEntry;

    let (modal, cx) = drawn_dialog(cx).await;

    let list = bounds_of(cx, "database-engine-list");
    let actions = bounds_of(cx, "database-modal-actions");
    assert!(
        actions.top() >= list.bottom(),
        "the engine list's actions must sit under it, got {actions:?} against {list:?}"
    );

    modal.update_in(cx, |modal, window, cx| modal.import_from_url(window, cx));
    draw_again(cx);
    let body = bounds_of(cx, "database-form-body");
    let actions = bounds_of(cx, "database-modal-actions");
    assert!(
        actions.top() >= body.bottom(),
        "the form's actions must sit under it, not above it in a header, got {actions:?} against {body:?}"
    );

    modal.update(cx, |modal, cx| {
        modal.step = Step::Unreachable {
            engine: CatalogueEntry {
                driver: "example".into(),
                name: "Example".into(),
                description: "An engine".into(),
                group: "Relational".into(),
                installed: false,
                downloaded: false,
                hand_placed: false,
            },
            message: "No such driver".into(),
        };
        cx.notify();
    });
    draw_again(cx);
    let body = bounds_of(cx, "database-form-body");
    let actions = bounds_of(cx, "database-modal-actions");
    assert!(
        actions.top() >= body.bottom(),
        "a step that failed must keep its actions where the others have them"
    );
}

/// The prompt had no buttons at all, so the only way out was a key nobody was
/// told about. Cancel must still be the key's job as well as the button's.
#[gpui::test]
async fn the_password_prompt_has_a_footer_and_cancel_closes_it(cx: &mut TestAppContext) {
    let (workspace, panel, cx) = workspace_with_panel(cx).await;
    set_connections(cx, &[("a", "/tmp/a.sqlite")]);

    panel.update_in(cx, |panel, window, cx| {
        panel.prompt_for_password(0, window, cx)
    });
    draw_again(cx);
    assert!(
        workspace.read_with(cx, |workspace, cx| workspace
            .active_modal::<crate::password_prompt::PasswordPrompt>(cx)
            .is_some()),
        "asking for a password must open the prompt"
    );

    let field = bounds_of(cx, "database-password-field");
    let actions = bounds_of(cx, "database-modal-actions");
    assert!(
        actions.top() >= field.bottom(),
        "the prompt's actions must sit under its field, got {actions:?} against {field:?}"
    );

    cx.dispatch_action(menu::Cancel);
    cx.run_until_parked();
    assert!(
        workspace.read_with(cx, |workspace, cx| workspace
            .active_modal::<crate::password_prompt::PasswordPrompt>(cx)
            .is_none()),
        "cancelling must close the prompt"
    );
}

/// The list narrows as the search is typed into, not on the next thing that
/// happens to redraw the dialog.
#[gpui::test]
async fn typing_in_the_search_redraws_the_list(cx: &mut TestAppContext) {
    let (modal, cx) = drawn_dialog(cx).await;
    assert!(
        cx.debug_bounds("database-engine-list").is_some(),
        "the list is drawn before anything is typed"
    );

    modal.update_in(cx, |modal, window, cx| {
        modal.search_editor.update(cx, |search, cx| {
            search.set_text("matches no engine at all", window, cx)
        });
    });
    cx.run_until_parked();

    assert!(
        cx.debug_bounds("database-engine-list").is_none(),
        "a search that matches nothing must have emptied the drawn list"
    );
}

/// The tree's rows are drawn by a virtualised list, so a handler that only
/// works when the row is reached some other way (a menu deployed by calling the
/// method, a toggle called directly) proves nothing about the pointer. These
/// drive the rows the way a hand does: by where it clicks.
#[gpui::test]
async fn right_clicking_a_drawn_connection_row_leaves_the_panel_a_menu(cx: &mut TestAppContext) {
    use crate::connection_tree::Row;

    let (panel, cx) = drawn_rows(
        vec![("tree-row-connection", Row::Connection { index: 0 })],
        cx,
    )
    .await;
    panel.read_with(cx, |panel, _| {
        assert!(
            panel.context_menu.is_none(),
            "no menu until one is asked for"
        );
    });

    let row = bounds_of(cx, "tree-row-connection");
    let position = gpui::point(row.left() + gpui::px(60.), row.center().y);
    cx.simulate_mouse_down(position, gpui::MouseButton::Right, gpui::Modifiers::none());

    panel.read_with(cx, |panel, _| {
        let (_, anchor, _) = panel
            .context_menu
            .as_ref()
            .expect("a right-click on the row itself must open its menu");
        assert_eq!(*anchor, position, "and the menu opens where the pointer is");
    });
}

/// The power button sits inside the row it belongs to, so a click on it is also
/// geometrically a click on the row. It has to be only the button's: a
/// reconnect that also selected the connection would swap the scratch buffer
/// under someone who only wanted to retry.
#[gpui::test]
async fn pressing_the_power_button_does_not_also_choose_the_row(cx: &mut TestAppContext) {
    use crate::connection_tree::Row;

    let (panel, cx) = drawn_rows(
        vec![("tree-row-connection", Row::Connection { index: 0 })],
        cx,
    )
    .await;
    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.active, None);
        assert_eq!(
            panel.connections[0].schemas.len(),
            1,
            "the row starts with a schema the button's reconnect will throw away"
        );
    });

    // The button is the last thing in the row, a little in from its right edge.
    let row = bounds_of(cx, "tree-row-connection");
    cx.simulate_click(
        gpui::point(row.right() - gpui::px(16.), row.center().y),
        gpui::Modifiers::none(),
    );
    cx.run_until_parked();

    panel.read_with(cx, |panel, _| {
        assert!(
            panel.connections[0].schemas.is_empty(),
            "reconnecting starts from nothing, so the click reached the button"
        );
        assert!(
            panel.failure(0).is_some(),
            "and the connection was actually attempted"
        );
        assert_eq!(
            panel.active, None,
            "but the click stopped at the button and never chose the row"
        );
    });
}

/// The arrow hangs in the gutter of the row it opens, inside the area that
/// toggles the row on a click. Two handlers on one gesture would open it and
/// close it again in the same click.
#[gpui::test]
async fn pressing_a_rows_arrow_toggles_it_exactly_once(cx: &mut TestAppContext) {
    use crate::connection_tree::{INDENT_STEP, Row};

    let (panel, cx) = drawn_rows(
        vec![(
            "tree-row-schema",
            Row::Schema {
                index: 0,
                schema_index: 0,
            },
        )],
        cx,
    )
    .await;
    panel.read_with(cx, |panel, _| {
        assert!(panel.connections[0].schemas[0].expanded);
    });

    // A schema row is one level in, and every row is drawn one level deeper than
    // its depth, so its arrow sits at the end of two steps of indent.
    let row = bounds_of(cx, "tree-row-schema");
    cx.simulate_click(
        gpui::point(row.left() + INDENT_STEP * 2., row.center().y),
        gpui::Modifiers::none(),
    );
    cx.run_until_parked();

    panel.read_with(cx, |panel, _| {
        assert!(
            !panel.connections[0].schemas[0].expanded,
            "one click on the arrow is one toggle: open becomes closed, and stays closed"
        );
    });
}

/// The arrow on a connection row is the same gesture as a click on its name:
/// it opens the connection and makes it the one queries go to.
#[gpui::test]
async fn pressing_a_connections_arrow_chooses_the_connection(cx: &mut TestAppContext) {
    use crate::connection_tree::{INDENT_STEP, Row};

    let (panel, cx) = drawn_rows(
        vec![("tree-row-connection", Row::Connection { index: 0 })],
        cx,
    )
    .await;
    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.active, None);
        assert!(panel.failure(0).is_none());
    });

    let row = bounds_of(cx, "tree-row-connection");
    cx.simulate_click(
        gpui::point(row.left() + INDENT_STEP, row.center().y),
        gpui::Modifiers::none(),
    );
    cx.run_until_parked();

    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.active, Some(0), "the arrow chooses the connection");
        assert!(
            panel.failure(0).is_some(),
            "and tries to open it -- this build has no driver for it to reach"
        );
    });
}

/// Keeps what it is given, so a test can read back what the prompt wrote
/// without a keychain being involved.
#[derive(Default)]
struct MemoryCredentials {
    entries: std::sync::Mutex<std::collections::HashMap<String, (String, Vec<u8>)>>,
}

impl MemoryCredentials {
    fn entry(&self, key: &str) -> Option<(String, Vec<u8>)> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
            .cloned()
    }
}

type CredentialsFuture<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<T>> + 'a>>;

impl credentials_provider::CredentialsProvider for MemoryCredentials {
    fn read_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a gpui::AsyncApp,
    ) -> CredentialsFuture<'a, Option<(String, Vec<u8>)>> {
        Box::pin(async move { Ok(self.entry(url)) })
    }

    fn write_credentials<'a>(
        &'a self,
        url: &'a str,
        username: &'a str,
        password: &'a [u8],
        _cx: &'a gpui::AsyncApp,
    ) -> CredentialsFuture<'a, ()> {
        Box::pin(async move {
            self.entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(url.to_string(), (username.to_string(), password.to_vec()));
            Ok(())
        })
    }

    fn delete_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a gpui::AsyncApp,
    ) -> CredentialsFuture<'a, ()> {
        Box::pin(async move {
            self.entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(url);
            Ok(())
        })
    }
}

/// The prompt closes itself when its field loses focus, and the button that
/// submits it is somewhere the pointer has to travel to. If pressing the button
/// moved focus first, the prompt would be gone before the click landed and the
/// password would never be saved.
#[gpui::test]
async fn pressing_connect_with_the_field_focused_saves_rather_than_dismisses(
    cx: &mut TestAppContext,
) {
    use crate::password_prompt::PasswordPrompt;
    use gpui::Focusable as _;

    let (workspace, _panel, cx) = workspace_with_panel(cx).await;
    let store = std::sync::Arc::new(MemoryCredentials::default());
    cx.update(|_window, cx| {
        cx.set_global(zed_credentials_provider::ZedCredentialsProvider(
            store.clone(),
        ));
    });

    let saved = std::rc::Rc::new(std::cell::Cell::new(false));
    workspace.update_in(cx, |workspace, window, cx| {
        let saved = saved.clone();
        workspace.toggle_modal(window, cx, move |window, cx| {
            PasswordPrompt::new(
                "staging".into(),
                "postgres://staging/app".into(),
                "app".into(),
                Box::new(move |_window, _cx| saved.set(true)),
                window,
                cx,
            )
        });
    });
    draw_again(cx);

    let prompt = workspace
        .read_with(cx, |workspace, cx| {
            workspace.active_modal::<PasswordPrompt>(cx)
        })
        .expect("the prompt must be showing");
    assert!(
        prompt.update_in(cx, |prompt, window, cx| prompt
            .focus_handle(cx)
            .is_focused(window)),
        "the field is what holds focus, which is what makes a click elsewhere a blur"
    );

    cx.simulate_input("hunter2");
    let actions = bounds_of(cx, "database-modal-actions");
    cx.simulate_click(
        gpui::point(actions.right() - gpui::px(10.), actions.center().y),
        gpui::Modifiers::none(),
    );
    cx.run_until_parked();

    assert!(
        saved.get(),
        "the click on Connect must reach the button, not a prompt already dismissed"
    );
    assert_eq!(
        store.entry("postgres://staging/app"),
        Some(("app".to_string(), b"hunter2".to_vec())),
        "and what was typed is what reaches the keychain"
    );
    assert!(
        workspace.read_with(cx, |workspace, cx| workspace
            .active_modal::<PasswordPrompt>(cx)
            .is_none()),
        "a saved password closes the prompt"
    );
}

/// `render_results` in a frame of a known size, so what it draws can be held
/// against the room it was given.
struct ResultsFrame {
    panel: gpui::Entity<DatabasePanel>,
}

impl gpui::Render for ResultsFrame {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use ui::prelude::*;

        let results = self.panel.update(cx, |panel, cx| {
            panel.sync_result_columns(cx);
            panel.render_results(window, cx)
        });
        div()
            .debug_selector(|| "results-frame".into())
            .w(gpui::px(400.))
            .h(gpui::px(200.))
            .child(results)
    }
}

/// A driver's own words can run to a page. Centred on both axes and unbounded,
/// a tall message pushes "Query failed" off the top of the pane with nothing to
/// scroll back with.
#[gpui::test]
async fn a_long_driver_error_leaves_the_failure_title_in_view(cx: &mut TestAppContext) {
    init_test(cx);
    let languages = std::sync::Arc::new(language::LanguageRegistry::new(cx.executor()));
    let (view, cx) = cx.add_window_view(|window, cx| {
        let panel = cx.new(|cx| {
            DatabasePanel::standalone(
                gpui::WeakEntity::new_invalid(),
                languages.clone(),
                window,
                cx,
            )
        });
        ResultsFrame { panel }
    });
    let panel = view.read_with(cx, |view, _| view.panel.clone());

    let message = (1..=60)
        .map(|line| format!("line {line} of a long driver error"))
        .collect::<Vec<_>>()
        .join("\n");
    panel.update(cx, |panel, cx| {
        panel.query = crate::query::QueryState::Failed(crate::query::QueryError {
            message: message.into(),
            read_only: false,
        });
        cx.notify();
    });
    view.update(cx, |_, cx| cx.notify());
    draw_again(cx);

    let frame = bounds_of(cx, "results-frame");
    let error = bounds_of(cx, "database-query-error");
    // The padding the failure state keeps on every side. The title sits between
    // it and the message, so a message that starts below it left the title room.
    let padding = cx.update(|window, _| window.rem_size());
    assert!(
        error.top() >= frame.top() + padding,
        "the message starts at {:?}, so the title above it was pushed out of the frame at {:?}",
        error.top(),
        frame.top()
    );
    assert!(
        error.bottom() <= frame.bottom() - padding,
        "the message ends at {:?}, below the frame's {:?}",
        error.bottom(),
        frame.bottom()
    );
}

fn team_session_global(cx: &mut gpui::VisualTestContext) {
    cx.update(|_window, cx| {
        let account = cx.new(|_| {
            zode_account::Account::for_test(zode_account::AccountStatus::SignedIn(
                zode_account::AccountUser {
                    id: "u-ada".into(),
                    email: "ada@example.com".into(),
                    name: None,
                    avatar_url: None,
                },
            ))
        });
        let team = cx.new(|cx| zode_team_vault::TeamVaultSession::new(account, cx));
        zode_team_vault::TeamVaultSession::set_global(team, cx);
    });
}

/// A team connection as `sync_team_connections` leaves it once pulled.
fn push_team_node(
    panel: &mut DatabasePanel,
    byte: u8,
    name: &str,
    fetched: bool,
    password: Option<&str>,
) {
    use crate::database_panel::{ConnectionNode, NodeState};
    use crate::team_connections::TeamOrigin;

    panel.team_group.name = Some("Acme".into());
    panel.connections.push(ConnectionNode {
        config: crate::ConnectionConfig {
            name: name.into(),
            driver: if fetched {
                "postgres".into()
            } else {
                String::new()
            },
            url: if fetched {
                format!("postgres://app@{name}.internal/app")
            } else {
                String::new()
            },
        },
        state: NodeState::Idle,
        expanded: false,
        schemas: Vec::new(),
        collapse_schema_level: false,
        team: Some(TeamOrigin {
            entry: zode_env_sync::EntryId::parse(&format!("{byte:02x}").repeat(16))
                .expect("a valid id"),
            revision: "1".into(),
            fetched,
            shared_password: password.map(|password| zeroize::Zeroizing::new(password.into())),
        }),
        from_project: false,
        unattended: false,
        schemas_load: 0,
    });
}

/// Settings fire on every keystroke in the settings file. A team connection
/// is not in settings, so a reload must neither drop it nor mistake it for a
/// personal connection of the same name.
#[gpui::test]
async fn team_connections_survive_a_settings_reload_under_their_own_header(
    cx: &mut TestAppContext,
) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    set_connections(cx, &[("staging", "/tmp/staging.sqlite")]);
    panel.update(cx, |panel, _cx| {
        push_team_node(panel, 0xa1, "staging", true, None);
    });

    set_connections(
        cx,
        &[
            ("staging", "/tmp/staging.sqlite"),
            ("local", "/tmp/local.sqlite"),
        ],
    );

    panel.update(cx, |panel, _cx| {
        assert_eq!(
            panel.connection_names(),
            vec!["staging", "local", "staging"],
            "personal connections first, the team's after them, the same name kept apart"
        );
        assert!(panel.connections[0].team.is_none());
        assert!(panel.connections[2].team.is_some());

        let rows = panel.rows();
        let header = rows
            .iter()
            .position(|row| matches!(row, crate::connection_tree::Row::TeamHeader { .. }))
            .expect("the team's connections sit under a header");
        assert!(
            matches!(
                rows.get(header + 1),
                Some(crate::connection_tree::Row::Connection { index: 2 })
            ),
            "the header comes right before the first team connection"
        );
    });
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

/// A team that shares nothing, or has not been loaded, still shows where its
/// connections would be, with the button that loads them.
#[gpui::test]
async fn an_empty_team_group_draws_its_header_and_note(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    panel.update(cx, |panel, _cx| {
        panel.team_group.name = Some("Acme".into());
        panel.team_group.note = Some("Press reload to load shared connections".into());
        let rows = panel.rows();
        assert!(matches!(
            rows.first(),
            Some(crate::connection_tree::Row::TeamHeader { .. })
        ));
        assert!(matches!(
            rows.get(1),
            Some(crate::connection_tree::Row::Note { .. })
        ));
    });
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

/// The shared password goes into the keychain once, after it worked, under
/// the same key `Set Password…` uses -- and the copy in memory goes with it,
/// so every later open reads the keychain like any personal connection.
#[gpui::test]
async fn a_shared_password_is_stored_once_and_then_forgotten(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    let store = std::sync::Arc::new(MemoryCredentials::default());
    cx.update(|_window, cx| {
        cx.set_global(zed_credentials_provider::ZedCredentialsProvider(
            store.clone(),
        ));
    });
    panel.update(cx, |panel, _cx| {
        push_team_node(panel, 0xa1, "staging", true, Some("hunter2"));
    });

    panel.update(cx, |panel, cx| {
        let shared = panel.connections[0]
            .team
            .as_ref()
            .and_then(|team| team.shared_password.clone())
            .expect("the fixture shares a password");
        // What a successful open hands over: the keychain had nothing.
        let choice = crate::team_connections::choose_secret(None, Some(shared));
        assert_eq!(choice.secret.as_deref(), Some("hunter2"));
        let remember = choice.remember.expect("kept after it worked");
        panel.remember_shared_password(0, remember, cx);
    });
    cx.run_until_parked();

    assert_eq!(
        store.entry("postgres://app@staging.internal/app"),
        Some(("staging".to_string(), b"hunter2".to_vec())),
        "stored under the URL, as Set Password… does"
    );
    panel.update(cx, |panel, _cx| {
        assert!(
            panel.connections[0]
                .team
                .as_ref()
                .is_some_and(|team| team.shared_password.is_none()),
            "the in-memory copy is dropped once the keychain has it"
        );
        let next = crate::team_connections::choose_secret(
            Some("hunter2".into()),
            panel.connections[0]
                .team
                .as_ref()
                .and_then(|team| team.shared_password.clone()),
        );
        assert_eq!(next.remember, None, "the next open stores nothing again");
    });
}

/// A team connection's settings live sealed in the team vault, so opening it
/// pulls them first -- and with no team key here, it says so rather than
/// starting a driver with an empty URL.
#[gpui::test]
async fn opening_an_unfetched_team_connection_pulls_it_first(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    team_session_global(cx);
    cx.update(|_window, cx| crate::driver_registry::install_for_test("postgres", cx));
    panel.update(cx, |panel, _cx| {
        push_team_node(panel, 0xa1, "staging", false, None);
    });

    panel.update_in(cx, |panel, window, cx| {
        panel.toggle_connection(0, window, cx);
        assert!(
            panel.is_reaching_for_a_driver(0),
            "the row shows it is working while the entry is fetched"
        );
    });
    cx.run_until_parked();

    panel.update(cx, |panel, _cx| {
        let failure = panel
            .failure(0)
            .expect("without the team key the pull fails");
        assert!(failure.contains("team key"), "{failure}");
        assert!(
            panel.connections[0]
                .team
                .as_ref()
                .is_some_and(|team| !team.fetched),
            "nothing was opened with settings it never received"
        );
    });
}

/// A pull is out while the list is rebuilt around the node it is for -- a
/// settings edit adds a personal connection in front of it. The answer lands on
/// that node, not on whatever now has its old place.
#[gpui::test]
async fn a_team_connection_fetched_while_the_list_moves_still_gets_its_answer(
    cx: &mut TestAppContext,
) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    team_session_global(cx);
    panel.update(cx, |panel, _cx| {
        push_team_node(panel, 0xa1, "staging", false, None);
    });

    panel.update_in(cx, |panel, window, cx| {
        panel.toggle_connection(0, window, cx)
    });
    cx.update(|_window, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.database.get_or_insert_default().connections =
                    Some(vec![settings::DatabaseConnectionContent {
                        name: Some("local".into()),
                        driver: Some("sqlite".into()),
                        url: Some("/tmp/local.sqlite".into()),
                        safety: None,
                    }]);
            });
        });
    });
    cx.run_until_parked();

    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.connection_names(), vec!["local", "staging"]);
        assert!(panel.connections[0].team.is_none());
        let failure = panel
            .failure(1)
            .expect("the team node got the answer, which is a failure here");
        assert!(failure.contains("team key"), "{failure}");
        assert!(
            panel.failure(0).is_none(),
            "the personal connection that took its place did not"
        );
    });
}

/// The write to the keychain is out while the list moves: forgetting the
/// shared password has to reach the node it was shared with.
#[gpui::test]
async fn a_shared_password_is_forgotten_by_its_own_node_after_the_list_moves(
    cx: &mut TestAppContext,
) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    let store = std::sync::Arc::new(MemoryCredentials::default());
    cx.update(|_window, cx| {
        cx.set_global(zed_credentials_provider::ZedCredentialsProvider(store));
    });
    panel.update(cx, |panel, _cx| {
        push_team_node(panel, 0xa1, "staging", true, Some("hunter2"));
    });

    panel.update(cx, |panel, cx| {
        panel.remember_shared_password(0, zeroize::Zeroizing::new("hunter2".into()), cx);
    });
    cx.update(|_window, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.database.get_or_insert_default().connections =
                    Some(vec![settings::DatabaseConnectionContent {
                        name: Some("local".into()),
                        driver: Some("sqlite".into()),
                        url: Some("/tmp/local.sqlite".into()),
                        safety: None,
                    }]);
            });
        });
    });
    cx.run_until_parked();

    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.connection_names(), vec!["local", "staging"]);
        assert!(
            panel.connections[1]
                .team
                .as_ref()
                .is_some_and(|team| team.shared_password.is_none()),
            "the in-memory copy went from the node it belonged to"
        );
    });
}

/// Sharing a connection leaves the password at home unless asked.
#[gpui::test]
async fn sharing_a_connection_starts_without_its_password(cx: &mut TestAppContext) {
    let (workspace, panel, cx) = workspace_with_panel(cx).await;
    team_session_global(cx);
    set_connections(cx, &[("staging", "/tmp/staging.sqlite")]);

    panel.update_in(cx, |panel, window, cx| panel.share_with_team(0, window, cx));
    draw_again(cx);

    let modal = workspace
        .read_with(cx, |workspace, cx| {
            workspace.active_modal::<crate::team_connections::ShareConnectionModal>(cx)
        })
        .expect("the share window must be showing");
    modal.read_with(cx, |modal, _cx| {
        assert!(!modal.include_password, "the password box starts unticked");
    });
    assert!(cx.debug_bounds("database-share-connection").is_some());
}

/// A team connection is not in settings: it has no pin to offer, and the menu
/// still opens on it.
#[gpui::test]
async fn right_clicking_a_team_connection_opens_its_menu(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    panel.update(cx, |panel, _cx| {
        push_team_node(panel, 0xa1, "staging", true, None);
    });
    panel.update_in(cx, |panel, window, cx| {
        panel.deploy_connection_menu(0, gpui::point(gpui::px(10.), gpui::px(10.)), window, cx);
        assert!(panel.context_menu.is_some());
    });
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

/// Which connection is open is remembered by place in the list. Removing one
/// above it shifts every place by one, and a panel still pointing at the old
/// place would be showing -- and running queries against -- a different
/// database than the one the user opened.
#[gpui::test]
async fn removing_an_earlier_connection_keeps_the_open_one_open(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    set_connections(cx, &[("a", "/tmp/a.sqlite"), ("b", "/tmp/b.sqlite")]);

    panel.update(cx, |panel, _cx| {
        panel.active = Some(1);
        panel.open_table = Some(crate::database_panel::OpenTable {
            connection: 1,
            schema: "main".into(),
            table: "people".into(),
            columns: None,
            columns_load: 0,
        });
    });

    set_connections(cx, &[("b", "/tmp/b.sqlite")]);

    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.connection_names(), vec!["b"]);
        assert_eq!(
            panel.active,
            Some(0),
            "`b` is still the one queries run against"
        );
        assert_eq!(
            panel.open_table.as_ref().map(|open| open.connection),
            Some(0),
            "and its table is still the one showing"
        );
    });

    // The open connection itself going away takes what it was showing with it.
    set_connections(cx, &[("c", "/tmp/c.sqlite")]);
    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.active, None);
        assert!(panel.open_table.is_none());
    });
}

async fn settings_file_text(
    workspace: &gpui::Entity<workspace::Workspace>,
    cx: &mut gpui::VisualTestContext,
) -> String {
    let fs = workspace.read_with(cx, |workspace, cx| {
        workspace.project().read(cx).fs().clone()
    });
    fs.load(paths::settings_file()).await.unwrap_or_default()
}

fn memory_credentials(cx: &mut gpui::VisualTestContext) -> std::sync::Arc<MemoryCredentials> {
    let store = std::sync::Arc::new(MemoryCredentials::default());
    cx.update(|_window, cx| {
        cx.set_global(zed_credentials_provider::ZedCredentialsProvider(
            store.clone(),
        ));
    });
    store
}

fn store_password(store: &MemoryCredentials, url: &str) {
    store
        .entries
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(url.to_string(), ("app".to_string(), b"secret".to_vec()));
}

/// Removing a connection takes everything this machine kept for it: the
/// settings entry, the password, and the pin. A password left in the keychain
/// for a connection nobody can see any more is one nobody will ever clean up.
#[gpui::test]
async fn removing_a_connection_forgets_its_entry_its_password_and_its_pin(cx: &mut TestAppContext) {
    let (workspace, panel, cx) = workspace_with_panel(cx).await;
    let store = memory_credentials(cx);
    store_password(&store, "/tmp/a.sqlite");
    store_password(&store, "/tmp/b.sqlite");
    set_connections(cx, &[("a", "/tmp/a.sqlite"), ("b", "/tmp/b.sqlite")]);
    // The same two on disk, which is what removing has to edit.
    let fs = workspace.read_with(cx, |workspace, cx| {
        workspace.project().read(cx).fs().clone()
    });
    fs.create_dir(paths::config_dir())
        .await
        .expect("config dir");
    fs.atomic_write(
        paths::settings_file().clone(),
        r#"{
  "database": {
    "connections": [
      { "name": "a", "driver": "sqlite", "url": "/tmp/a.sqlite" },
      { "name": "b", "driver": "sqlite", "url": "/tmp/b.sqlite" }
    ]
  }
}
"#
        .to_string(),
    )
    .await
    .expect("settings written");
    panel.update(cx, |panel, cx| panel.toggle_pin("a", cx));

    panel.update(cx, |panel, cx| panel.remove_connection("a", cx));
    cx.run_until_parked();

    assert!(
        store.entry("/tmp/a.sqlite").is_none(),
        "the removed connection's password is gone"
    );
    assert!(store.entry("/tmp/b.sqlite").is_some(), "and nobody else's");
    assert!(
        !panel.read_with(cx, |panel, _| panel.is_pinned("a")),
        "a pin outliving its connection would pin the next one given that name"
    );
    let settings = settings_file_text(&workspace, cx).await;
    assert!(
        !settings.contains("/tmp/a.sqlite"),
        "the entry is gone from settings:\n{settings}"
    );
    assert!(
        settings.contains("/tmp/b.sqlite"),
        "and only that entry:\n{settings}"
    );
}

/// The keychain holds one password per URL, so two connections to the same
/// server share it. Removing one must not log the other out.
#[gpui::test]
async fn a_password_another_connection_still_uses_is_kept(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    let store = memory_credentials(cx);
    store_password(&store, "/tmp/shared.sqlite");
    set_connections(
        cx,
        &[("a", "/tmp/shared.sqlite"), ("b", "/tmp/shared.sqlite")],
    );

    panel.update(cx, |panel, cx| panel.remove_connection("a", cx));
    cx.run_until_parked();

    assert!(store.entry("/tmp/shared.sqlite").is_some());
}

/// Removing is not undoable, so the menu entry only asks; nothing is removed
/// until the answer is "Remove".
#[gpui::test]
async fn removing_from_the_menu_asks_first(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    let store = memory_credentials(cx);
    store_password(&store, "/tmp/a.sqlite");
    set_connections(cx, &[("a", "/tmp/a.sqlite")]);

    panel.update_in(cx, |panel, window, cx| {
        panel.confirm_remove_connection(0, window, cx)
    });
    cx.run_until_parked();
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(
        store.entry("/tmp/a.sqlite").is_some(),
        "cancelling removes nothing"
    );

    panel.update_in(cx, |panel, window, cx| {
        panel.confirm_remove_connection(0, window, cx)
    });
    cx.run_until_parked();
    cx.simulate_prompt_answer("Remove");
    cx.run_until_parked();
    assert!(store.entry("/tmp/a.sqlite").is_none());
}

/// A grid cell is one truncated line, so a long or multi-line value could not
/// be read in full anywhere. Clicking a row opens it in a detail view, one
/// field per column; clicking the same row again closes it.
#[gpui::test]
async fn clicking_a_row_opens_its_detail_and_clicking_again_closes_it(cx: &mut TestAppContext) {
    use database::protocol::{Cell, ResultColumn, ResultSet};

    init_test(cx);
    let languages = std::sync::Arc::new(language::LanguageRegistry::new(cx.executor()));
    let (view, cx) = cx.add_window_view(|window, cx| {
        let panel = cx.new(|cx| {
            DatabasePanel::standalone(
                gpui::WeakEntity::new_invalid(),
                languages.clone(),
                window,
                cx,
            )
        });
        ResultsFrame { panel }
    });
    let panel = view.read_with(cx, |view, _| view.panel.clone());

    panel.update(cx, |panel, cx| {
        panel.query = crate::query::QueryState::Done(crate::query::Page {
            result: ResultSet {
                columns: vec![
                    ResultColumn {
                        name: "id".into(),
                        type_name: "int".into(),
                    },
                    ResultColumn {
                        name: "note".into(),
                        type_name: "text".into(),
                    },
                ],
                rows: vec![
                    vec![
                        Cell::Number { value: "1".into() },
                        Cell::Text {
                            value: "first line\nsecond line".into(),
                        },
                    ],
                    vec![Cell::Number { value: "2".into() }, Cell::Null],
                ],
                truncated: false,
                elapsed_ms: 1,
            },
            sql: "select * from notes".into(),
            offset: 0,
            limit: 200,
            selected_row: None,
            selection: Default::default(),
            browse: None,
            connection: None,
            kind: crate::safe_mode::StatementKind::Read,
            origin: Default::default(),
            edits: Default::default(),
        });
        cx.notify();
    });
    view.update(cx, |_, cx| cx.notify());
    draw_again(cx);
    assert!(
        cx.debug_bounds("database-row-detail").is_none(),
        "nothing is shown before a row is clicked"
    );
    assert_eq!(
        panel.read_with(cx, |panel, cx| panel.result_column_widths.read(cx).cols()),
        2,
        "each column of the result is given a width of its own"
    );

    let row = bounds_of(cx, "database-result-row-1");
    cx.simulate_click(row.center(), gpui::Modifiers::none());
    draw_again(cx);
    bounds_of(cx, "database-row-detail");
    bounds_of(cx, "database-row-detail-field-0");
    bounds_of(cx, "database-row-detail-field-1");
    assert_eq!(
        panel.read_with(cx, |panel, _| match &panel.query {
            crate::query::QueryState::Done(page) => page.selected_row,
            _ => None,
        }),
        Some(1)
    );

    // Near the row's left edge, not its centre: the row is as wide as its
    // columns and scrolls sideways, so its centre can sit under the detail now
    // drawn beside it.
    let row = bounds_of(cx, "database-result-row-1");
    let visible_part = gpui::point(row.left() + gpui::px(8.), row.center().y);
    cx.simulate_click(visible_part, gpui::Modifiers::none());
    draw_again(cx);
    assert!(
        cx.debug_bounds("database-row-detail").is_none(),
        "clicking the shown row again closes the detail"
    );
}

/// Opening a folder whose `.idea/` names data sources lists them in the
/// panel's "Project" group, with the user the IDE signs in as.
#[gpui::test]
async fn a_projects_own_data_sources_are_listed_when_it_opens(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/proxy",
        serde_json::json!({
            ".idea": {
                "dataSources.xml": r#"<project version="4">
  <component name="DataSourceManagerImpl">
    <data-source source="LOCAL" name="auth" uuid="a1">
      <jdbc-url>jdbc:postgresql://127.0.0.1:15441/auth</jdbc-url>
    </data-source>
    <data-source source="LOCAL" name="company" uuid="c1">
      <jdbc-url>jdbc:postgresql://127.0.0.1:15442/company</jdbc-url>
    </data-source>
  </component>
</project>"#,
                "dataSources.local.xml": r#"<project version="4">
  <component name="dataSourceStorageLocal">
    <data-source name="auth" uuid="a1"><user-name>auth</user-name></data-source>
  </component>
</project>"#,
            },
        }),
    )
    .await;
    let project = Project::test(fs, ["/proxy".as_ref()], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        let handle = cx.weak_entity();
        let languages = workspace.project().read(cx).languages().clone();
        let panel = cx.new(|cx| DatabasePanel::standalone(handle, languages, window, cx));
        workspace.add_item_to_active_pane(Box::new(panel.clone()), None, true, window, cx);
        panel
    });
    cx.run_until_parked();

    panel.read_with(cx, |panel, _| {
        let listed: Vec<(String, String, bool)> = panel
            .connections
            .iter()
            .map(|node| {
                (
                    node.config.name.clone(),
                    node.config.url.clone(),
                    node.from_project,
                )
            })
            .collect();
        assert_eq!(
            listed,
            vec![
                (
                    "auth".to_string(),
                    "postgresql://auth@127.0.0.1:15441/auth".to_string(),
                    true
                ),
                (
                    "company".to_string(),
                    "postgresql://127.0.0.1:15442/company".to_string(),
                    true
                ),
            ]
        );
        assert!(
            panel
                .rows()
                .iter()
                .any(|row| matches!(row, crate::connection_tree::Row::ProjectHeader)),
            "the project's connections sit under their own header"
        );
    });
}

/// A project connection that opens on its own and is refused for want of a
/// password waits for a click rather than raising a prompt for every database
/// at once.
#[test]
fn an_unattended_connection_refused_for_a_password_does_not_prompt() {
    use crate::panel_connections::after_refusal;
    assert!(
        after_refusal(false),
        "a click that is refused asks for the password"
    );
    assert!(
        !after_refusal(true),
        "a connection opened on its own waits for a click"
    );
}

type DriverCalls = std::sync::Arc<database::fake_driver::FakeDriver>;

/// How many times the fake driver was asked for `method` so far.
fn asked(calls: &DriverCalls, method: &str) -> usize {
    calls
        .calls
        .lock()
        .iter()
        .filter(|call| *call == method)
        .count()
}

/// A panel whose only connection is open on an in-process driver, with its
/// schemas listed. Nothing here starts a process: what is exercised is the
/// real client and the real loaders, over a shortened pipe.
async fn panel_open_on_a_fake_driver(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<DatabasePanel>,
    DriverCalls,
    &mut gpui::VisualTestContext,
) {
    use crate::database_panel::NodeState;
    use database::client::{DEFAULT_REQUEST_TIMEOUT, DriverClient};
    use database::fake_driver::{FakeBehaviour, FakeDriver};

    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    set_connections(cx, &[("a", "/tmp/a.sqlite")]);

    let driver = std::sync::Arc::new(FakeDriver::new(FakeBehaviour::Well));
    let calls = driver.clone();
    let client =
        cx.update(|_window, cx| DriverClient::new(driver, DEFAULT_REQUEST_TIMEOUT, &cx.to_async()));
    panel.update(cx, |panel, cx| {
        let session = crate::session::Session::for_test(client, Default::default());
        panel.connections[0].state = NodeState::Connected(std::sync::Arc::new(session));
        panel.connections[0].expanded = true;
        panel.load_schemas(0, cx);
    });
    cx.run_until_parked();
    (panel, calls, cx)
}

fn table_names(panel: &DatabasePanel) -> Vec<String> {
    panel.connections[0]
        .schemas
        .first()
        .and_then(|schema| schema.tables.as_ref())
        .map(|tables| tables.iter().map(|table| table.name.clone()).collect())
        .unwrap_or_default()
}

#[gpui::test]
async fn refreshing_a_connection_reads_what_is_showing_again(cx: &mut TestAppContext) {
    let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;
    assert_eq!(asked(&calls, "list_schemas"), 1);
    assert_eq!(asked(&calls, "list_tables"), 1);
    panel.update(cx, |panel, cx| {
        panel.toggle_table(0, "public", "users", cx);
    });
    cx.run_until_parked();
    assert_eq!(asked(&calls, "describe_table"), 1);

    panel.update(cx, |panel, cx| panel.refresh_connection(0, cx));
    cx.run_until_parked();

    assert_eq!(
        asked(&calls, "list_schemas"),
        2,
        "the schema list is read again"
    );
    assert_eq!(
        asked(&calls, "list_tables"),
        2,
        "so is every expanded schema"
    );
    assert_eq!(asked(&calls, "describe_table"), 2, "and the open table");
    panel.read_with(cx, |panel, _| {
        assert!(
            panel.connections[0].expanded,
            "the connection stays expanded"
        );
        assert!(panel.connections[0].schemas[0].expanded);
        assert_eq!(table_names(panel), ["users", "active_users"]);
        let open = panel
            .open_table
            .as_ref()
            .expect("the open table stays open");
        assert_eq!(open.columns.as_ref().map(Vec::len), Some(2));
    });
}

/// Refreshing is the answer to "the database changed under me", so a tree that
/// shows nothing while it re-reads would turn that into "it all vanished".
#[gpui::test]
async fn the_old_tree_stays_up_until_the_new_one_arrives(cx: &mut TestAppContext) {
    let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;

    panel.update(cx, |panel, cx| panel.refresh_connection(0, cx));
    panel.read_with(cx, |panel, _| {
        assert_eq!(table_names(panel), ["users", "active_users"]);
    });
    cx.run_until_parked();
}

#[gpui::test]
async fn refreshing_a_schema_reads_only_its_tables(cx: &mut TestAppContext) {
    let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;

    panel.update(cx, |panel, cx| panel.refresh_schema(0, "public", cx));
    cx.run_until_parked();

    assert_eq!(
        asked(&calls, "list_schemas"),
        1,
        "the schema list is not read"
    );
    assert_eq!(asked(&calls, "list_tables"), 2);
}

/// A schema nobody opened has nothing on screen to refresh, and reading it
/// would be work the next expand does anyway. What it must do is forget what it
/// had, so that expand shows the database as it is then.
#[gpui::test]
async fn refreshing_a_collapsed_schema_forgets_its_tables_without_reading_them(
    cx: &mut TestAppContext,
) {
    let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;
    panel.update(cx, |panel, cx| {
        panel.connections[0].schemas[0].expanded = false;
        panel.refresh_schema(0, "public", cx);
        assert!(panel.connections[0].schemas[0].tables.is_none());
    });
    cx.run_until_parked();
    assert_eq!(asked(&calls, "list_tables"), 1);

    panel.update(cx, |panel, cx| panel.toggle_schema(0, 0, cx));
    cx.run_until_parked();
    assert_eq!(asked(&calls, "list_tables"), 2);
    panel.read_with(cx, |panel, _| assert_eq!(table_names(panel).len(), 2));
}

#[gpui::test]
async fn refreshing_a_table_describes_it_again_and_only_if_it_is_open(cx: &mut TestAppContext) {
    let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;

    panel.update(cx, |panel, cx| {
        panel.refresh_table(0, "public", "users", cx)
    });
    cx.run_until_parked();
    assert_eq!(
        asked(&calls, "describe_table"),
        0,
        "nothing open, nothing to read"
    );
    panel.read_with(cx, |panel, _| assert!(panel.open_table.is_none()));

    panel.update(cx, |panel, cx| panel.toggle_table(0, "public", "users", cx));
    cx.run_until_parked();
    panel.update(cx, |panel, cx| {
        panel.refresh_table(0, "public", "other", cx)
    });
    cx.run_until_parked();
    assert_eq!(
        asked(&calls, "describe_table"),
        1,
        "a different table is not this one"
    );

    panel.update(cx, |panel, cx| {
        panel.refresh_table(0, "public", "users", cx)
    });
    cx.run_until_parked();
    assert_eq!(asked(&calls, "describe_table"), 2);
    panel.read_with(cx, |panel, _| {
        assert_eq!(
            panel
                .open_table
                .as_ref()
                .and_then(|open| open.columns.as_ref())
                .map(Vec::len),
            Some(2)
        );
    });
}

/// The two ways a late answer lands wrong: after a refresh began, and after
/// the thing it was for was replaced. Both are simulated by what a refresh
/// does to the node -- it names a new load, so an answer to an earlier one is
/// not what the node is waiting for.
#[gpui::test]
async fn an_answer_to_a_superseded_load_is_dropped(cx: &mut TestAppContext) {
    let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;

    panel.update(cx, |panel, cx| {
        panel.connections[0].schemas.clear();
        panel.load_schemas(0, cx);
        assert_ne!(panel.connections[0].schemas_load, 0);
        panel.connections[0].schemas_load = 0;
    });
    cx.run_until_parked();
    panel.read_with(cx, |panel, _| {
        assert!(
            panel.connections[0].schemas.is_empty(),
            "the schema list was superseded"
        );
    });

    panel.update(cx, |panel, cx| panel.refresh_connection(0, cx));
    cx.run_until_parked();
    panel.update(cx, |panel, cx| {
        panel.connections[0].schemas[0].tables = None;
        panel.load_tables(0, 0, cx);
        panel.connections[0].schemas[0].tables_load = 0;
    });
    cx.run_until_parked();
    panel.read_with(cx, |panel, _| {
        assert!(
            panel.connections[0].schemas[0].tables.is_none(),
            "the table list was superseded"
        );
    });

    panel.update(cx, |panel, cx| {
        panel.toggle_table(0, "public", "users", cx);
        panel.open_table.as_mut().expect("open").columns_load = 0;
    });
    cx.run_until_parked();
    panel.read_with(cx, |panel, _| {
        let open = panel.open_table.as_ref().expect("still open");
        assert!(open.columns.is_none(), "the column list was superseded");
    });
}

/// Two refreshes in a row are both answered, and the later one is what stays.
#[gpui::test]
async fn refreshing_twice_in_a_row_settles_on_one_tree(cx: &mut TestAppContext) {
    let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;

    panel.update(cx, |panel, cx| {
        panel.refresh_connection(0, cx);
        panel.refresh_connection(0, cx);
    });
    cx.run_until_parked();

    assert_eq!(asked(&calls, "list_schemas"), 3);
    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.connections[0].schemas.len(), 1);
        assert_eq!(table_names(panel), ["users", "active_users"]);
    });
}

#[gpui::test]
async fn refreshing_what_is_not_open_does_nothing(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    set_connections(cx, &[("a", "/tmp/a.sqlite")]);

    panel.update(cx, |panel, cx| {
        panel.refresh_connection(0, cx);
        panel.refresh_connection(9, cx);
        panel.refresh_schema(0, "main", cx);
        panel.refresh_table(0, "main", "t", cx);
        panel.refresh_active(cx);
        assert!(
            !panel.is_connected(0),
            "a refresh is not a way to open a connection"
        );
    });
}

#[gpui::test]
async fn the_header_refresh_acts_on_the_active_connection_only(cx: &mut TestAppContext) {
    let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;

    panel.update(cx, |panel, cx| {
        panel.active = None;
        panel.refresh_active(cx);
    });
    cx.run_until_parked();
    assert_eq!(
        asked(&calls, "list_schemas"),
        1,
        "no active connection, nothing read"
    );

    panel.update(cx, |panel, cx| {
        panel.active = Some(0);
        panel.refresh_active(cx);
    });
    cx.run_until_parked();
    assert_eq!(asked(&calls, "list_schemas"), 2);
}

#[test]
fn a_refresh_keeps_a_schema_as_it_was_left_and_a_new_one_opens_by_default() {
    use crate::panel_refresh::schema_expanded;
    assert!(schema_expanded(Some(true), false, false));
    assert!(
        !schema_expanded(Some(false), true, false),
        "closed by hand stays closed"
    );
    assert!(
        schema_expanded(None, true, false),
        "a new default schema opens"
    );
    assert!(!schema_expanded(None, false, false));
    assert!(
        schema_expanded(Some(false), false, true),
        "a lone schema has no node to close"
    );
}

#[gpui::test]
async fn an_open_connection_has_a_dialect_and_a_closed_one_does_not(cx: &mut TestAppContext) {
    let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
    panel.update(cx, |panel, cx| {
        let dialect = panel.dialect(0).expect("open, so the driver has spoken");
        assert_eq!(dialect.engine(), crate::sql_dialect::Engine::Sqlite);
        panel.disconnect(0, cx);
        assert!(panel.dialect(0).is_none());
    });
}

/// The action is the keyboard road to the same refresh the header button
/// does, and the panel has to answer it where it is focused.
#[gpui::test]
async fn the_refresh_action_reaches_the_focused_panel(cx: &mut TestAppContext) {
    let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;
    panel.update(cx, |panel, _| panel.active = Some(0));
    cx.update(|window, cx| {
        let focus = panel.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
    });
    cx.run_until_parked();

    cx.dispatch_action(zed_actions::database::RefreshDatabase);
    cx.run_until_parked();

    assert_eq!(asked(&calls, "list_schemas"), 2);
}

mod tree_menus {
    use super::*;
    use crate::query::{QueryState, TableBrowse};
    use crate::sql_dialect::Dialect;
    use crate::tree_menus::{Generate, TableAction, generated_statement, table_actions};
    use database::protocol::{Capabilities, ColumnDef};

    fn sqlite() -> Dialect {
        Dialect::detect("sqlite", &Capabilities::default())
    }

    fn mongodb() -> Dialect {
        Dialect::detect(
            "mongodb",
            &Capabilities {
                read_table_template: Some(r#"{"find": {table}, "$db": {schema}}"#.into()),
                ..Default::default()
            },
        )
    }

    fn column(name: &str, primary_key: bool) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            type_name: "text".into(),
            nullable: true,
            primary_key,
        }
    }

    fn sql_of(panel: &DatabasePanel) -> String {
        match &panel.query {
            QueryState::Done(page) => page.sql.clone(),
            _ => panic!("a page was expected"),
        }
    }

    #[test]
    fn a_sql_table_offers_everything_and_generate_has_four_entries() {
        let actions = table_actions(&sqlite());
        assert_eq!(
            actions,
            vec![
                TableAction::OpenData,
                TableAction::CountRows,
                TableAction::CopyName,
                TableAction::CopyQualifiedName,
                TableAction::Generate(Generate::Select),
                TableAction::Generate(Generate::Insert),
                TableAction::Generate(Generate::Update),
                TableAction::Generate(Generate::Delete),
                TableAction::Refresh,
            ]
        );
    }

    /// A document store has no `INSERT` to write, so the entry is not shown
    /// rather than shown and failing.
    #[test]
    fn a_collection_has_no_generate_entries() {
        let actions = table_actions(&mongodb());
        assert!(
            !actions
                .iter()
                .any(|action| matches!(action, TableAction::Generate(_))),
            "{actions:?}"
        );
        assert!(actions.contains(&TableAction::OpenData));
        assert!(actions.contains(&TableAction::Refresh));
        for kind in [
            Generate::Select,
            Generate::Insert,
            Generate::Update,
            Generate::Delete,
        ] {
            assert_eq!(
                generated_statement(&mongodb(), kind, "d", "c", &[column("a", true)]),
                None
            );
        }
    }

    #[test]
    fn generated_statements_use_the_primary_key_to_pin_a_row() {
        let columns = [column("id", true), column("email", false)];
        let statement = |kind| {
            generated_statement(&sqlite(), kind, "main", "users", &columns).unwrap_or_default()
        };
        assert_eq!(
            statement(Generate::Select),
            "SELECT \"id\", \"email\"\nFROM \"main\".\"users\";"
        );
        assert_eq!(
            statement(Generate::Insert),
            "INSERT INTO \"main\".\"users\" (\"id\", \"email\")\nVALUES (<id>, <email>);"
        );
        assert_eq!(
            statement(Generate::Update),
            "UPDATE \"main\".\"users\"\nSET \"email\" = <email>\nWHERE \"id\" = <id>;"
        );
        assert_eq!(
            statement(Generate::Delete),
            "DELETE FROM \"main\".\"users\"\nWHERE \"id\" = <id>;"
        );
    }

    /// No key means no way to name one row, so the `WHERE` is left to write
    /// -- as a comment, never as nothing.
    #[test]
    fn a_table_without_a_key_gets_a_where_to_write() {
        let columns = [column("note", false)];
        assert_eq!(
            generated_statement(&sqlite(), Generate::Update, "main", "log", &columns),
            Some("UPDATE \"main\".\"log\"\nSET \"note\" = <note>\n-- WHERE <condition>;".into())
        );
        assert_eq!(
            generated_statement(&sqlite(), Generate::Delete, "main", "log", &columns),
            Some("DELETE FROM \"main\".\"log\"\n-- WHERE <condition>;".into())
        );
    }

    #[test]
    fn a_table_of_only_key_columns_still_updates_and_one_without_columns_does_not() {
        let keys = [column("a", true), column("b", true)];
        assert_eq!(
            generated_statement(&sqlite(), Generate::Update, "main", "link", &keys),
            Some(
                "UPDATE \"main\".\"link\"\nSET \"a\" = <a>, \"b\" = <b>\nWHERE \"a\" = <a> AND \"b\" = <b>;"
                    .into()
            )
        );
        for kind in [Generate::Insert, Generate::Update] {
            assert_eq!(
                generated_statement(&sqlite(), kind, "main", "empty", &[]),
                None
            );
        }
    }

    #[gpui::test]
    async fn opening_a_table_runs_the_dialect_read_and_marks_the_page(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.open_table_data(0, "public", "users", window, cx)
        });
        cx.run_until_parked();

        panel.read_with(cx, |panel, _| {
            assert_eq!(
                sql_of(panel),
                "SELECT *\nFROM \"public\".\"users\"\nORDER BY \"id\" ASC"
            );
            let QueryState::Done(page) = &panel.query else {
                panic!("a page was expected");
            };
            assert_eq!(
                page.browse,
                Some(TableBrowse {
                    key_columns: vec!["id".into()],
                    columns: crate::database_panel_tests::users_columns(),
                    ..TableBrowse::new(
                        ("a".to_string(), crate::database_panel::NodeOrigin::Personal),
                        "public",
                        "users"
                    )
                })
            );
            assert_eq!(panel.active, Some(0));
        });

        // Paging re-runs the same read and stays a browse of the same table.
        panel.update_in(cx, |panel, window, cx| panel.page(true, window, cx));
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            let QueryState::Done(page) = &panel.query else {
                panic!("a page was expected");
            };
            assert!(page.offset > 0);
            assert!(page.browse.is_some(), "the next page is still the table");
        });
    }

    #[gpui::test]
    async fn counting_rows_is_a_plain_page_not_a_browse(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.count_rows(0, "public", "users", window, cx)
        });
        cx.run_until_parked();

        panel.read_with(cx, |panel, _| {
            assert_eq!(sql_of(panel), "SELECT COUNT(*)\nFROM \"public\".\"users\"");
            let QueryState::Done(page) = &panel.query else {
                panic!("a page was expected");
            };
            assert!(page.browse.is_none(), "there is nothing to sort in a count");
        });
    }

    /// The same two entries on a collection run the statements a document
    /// store reads, not a `SELECT` it would refuse.
    #[gpui::test]
    async fn a_collection_is_read_with_its_own_statements(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.connections[0].config.driver = "mongodb".into();
            panel.open_table_data(0, "app", "users", window, cx)
        });
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                sql_of(panel),
                r#"{"find": "users", "sort": {"id": 1}, "$db": "app"}"#
            );
        });

        panel.update_in(cx, |panel, window, cx| {
            panel.count_rows(0, "app", "users", window, cx)
        });
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            assert!(sql_of(panel).contains(r#""aggregate": "users""#));
        });
    }

    fn editor_text(
        panel: &gpui::Entity<DatabasePanel>,
        cx: &mut gpui::VisualTestContext,
    ) -> String {
        panel.read_with(cx, |panel, cx| panel.sql_editor.read(cx).text(cx))
    }

    #[gpui::test]
    async fn generate_describes_the_table_when_its_columns_are_not_showing(
        cx: &mut TestAppContext,
    ) {
        let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;
        assert_eq!(asked(&calls, "describe_table"), 0);

        panel.update_in(cx, |panel, window, cx| {
            panel.generate_statement(0, "public", "users", Generate::Insert, window, cx)
        });
        cx.run_until_parked();

        assert_eq!(asked(&calls, "describe_table"), 1);
        assert_eq!(
            editor_text(&panel, cx),
            "INSERT INTO \"public\".\"users\" (\"id\", \"email\")\nVALUES (<id>, <email>);"
        );
        assert_eq!(asked(&calls, "query"), 0, "a template is never run");
        panel.read_with(cx, |panel, _| assert_eq!(panel.active, Some(0)));
    }

    #[gpui::test]
    async fn generate_reuses_the_columns_already_on_screen(cx: &mut TestAppContext) {
        let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;
        panel.update(cx, |panel, cx| panel.toggle_table(0, "public", "users", cx));
        cx.run_until_parked();
        assert_eq!(asked(&calls, "describe_table"), 1);

        panel.update_in(cx, |panel, window, cx| {
            panel.generate_statement(0, "public", "users", Generate::Update, window, cx)
        });
        cx.run_until_parked();

        assert_eq!(asked(&calls, "describe_table"), 1, "no second round trip");
        assert_eq!(
            editor_text(&panel, cx),
            "UPDATE \"public\".\"users\"\nSET \"email\" = <email>\nWHERE \"id\" = <id>;"
        );
    }

    #[gpui::test]
    async fn inserted_text_goes_at_the_cursor_of_the_connections_own_buffer(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.insert_into_editor(0, "SELECT ".into(), window, cx);
            panel.insert_into_editor(0, "\"email\"".into(), window, cx);
        });
        assert_eq!(editor_text(&panel, cx), "SELECT \"email\"");
    }

    #[gpui::test]
    async fn a_new_query_needs_an_open_connection(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.new_query(0, window, cx);
            assert_eq!(panel.active, Some(0));
            panel.active = None;
            panel.disconnect(0, cx);
            panel.new_query(0, window, cx);
            assert_eq!(panel.active, None, "a closed connection takes no query");
        });
    }

    #[gpui::test]
    async fn every_level_of_the_tree_opens_a_menu_on_the_panel(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        let position = gpui::point(gpui::px(10.), gpui::px(10.));

        panel.update_in(cx, |panel, window, cx| {
            assert!(panel.context_menu.is_none());
            panel.deploy_schema_menu(0, "public".into(), position, window, cx);
            assert!(panel.context_menu.is_some(), "schema");
            panel.context_menu = None;
            panel.deploy_table_menu(0, "public".into(), "users".into(), position, window, cx);
            assert!(panel.context_menu.is_some(), "table");
            panel.context_menu = None;
            panel.deploy_column_menu(0, "id".into(), position, window, cx);
            assert!(panel.context_menu.is_some(), "column");
            panel.context_menu = None;

            // Nothing to ask of a connection that is not open.
            panel.disconnect(0, cx);
            panel.deploy_schema_menu(0, "public".into(), position, window, cx);
            panel.deploy_table_menu(0, "public".into(), "users".into(), position, window, cx);
            panel.deploy_column_menu(0, "id".into(), position, window, cx);
            assert!(panel.context_menu.is_none());
        });
        cx.run_until_parked();
    }

    /// The row underneath a right-click must not also act on it: opening a
    /// menu that also expands the table moves the very row the menu is for.
    #[gpui::test]
    async fn a_right_click_opens_the_menu_and_leaves_the_table_alone(cx: &mut TestAppContext) {
        let (panel, cx) = drawn_rows(vec![("tree-row-table", table_row(2))], cx).await;
        let calls = connect_fake_driver(&panel, cx);
        panel.update(cx, |panel, _| {
            panel.connections[0].schemas[0].tables = Some(Vec::new());
        });
        let centre = bounds_of(cx, "tree-row-table").center();

        cx.simulate_mouse_down(centre, gpui::MouseButton::Right, gpui::Modifiers::none());
        cx.simulate_mouse_up(centre, gpui::MouseButton::Right, gpui::Modifiers::none());
        draw_again(cx);

        panel.read_with(cx, |panel, _| {
            assert!(panel.context_menu.is_some(), "the menu opened");
            assert!(panel.open_table.is_none(), "the table did not open");
            assert!(matches!(panel.query, QueryState::Idle), "no query ran");
        });
        assert_eq!(asked(&calls, "describe_table"), 0);
        assert_eq!(asked(&calls, "query"), 0);

        panel.update(cx, |panel, _| panel.context_menu = None);
        cx.simulate_click(centre, gpui::Modifiers::none());
        draw_again(cx);
        panel.read_with(cx, |panel, _| {
            assert!(panel.open_table.is_some(), "a left click still opens it");
        });
        assert_eq!(asked(&calls, "query"), 1, "and shows its rows");
    }

    /// Installs an in-process driver as the open session of connection `a`.
    fn connect_fake_driver(
        panel: &gpui::Entity<DatabasePanel>,
        cx: &mut gpui::VisualTestContext,
    ) -> DriverCalls {
        use crate::database_panel::NodeState;
        use database::client::{DEFAULT_REQUEST_TIMEOUT, DriverClient};
        use database::fake_driver::{FakeBehaviour, FakeDriver};

        let driver = std::sync::Arc::new(FakeDriver::new(FakeBehaviour::Well));
        let calls = driver.clone();
        let client = cx.update(|_window, cx| {
            DriverClient::new(driver, DEFAULT_REQUEST_TIMEOUT, &cx.to_async())
        });
        panel.update(cx, |panel, _| {
            let session = crate::session::Session::for_test(client, Default::default());
            panel.connections[0].state = NodeState::Connected(std::sync::Arc::new(session));
        });
        calls
    }
}

mod tree_filter {
    use super::*;
    use crate::connection_tree::Row;
    use crate::database_panel::SchemaNode;
    use database::protocol::SchemaRef;

    fn describe(panel: &DatabasePanel) -> Vec<String> {
        panel
            .rows()
            .iter()
            .map(|row| match row {
                Row::ProjectHeader => "project header".to_string(),
                Row::TeamHeader { name } => format!("team header {name}"),
                Row::Connection { index } => format!("connection {index}"),
                Row::Schema { schema_index, .. } => format!("schema {schema_index}"),
                Row::Table { name, .. } => format!("table {name}"),
                Row::Column { name, .. } => format!("column {name}"),
                Row::Note { text, .. } => format!("note {text}"),
            })
            .collect()
    }

    fn filter(panel: &gpui::Entity<DatabasePanel>, text: &str, cx: &mut gpui::VisualTestContext) {
        panel.update(cx, |panel, cx| panel.set_tree_filter(text, cx));
    }

    /// The fake driver's `public` schema holds `users` and `active_users`;
    /// `users` is open, showing `id` and `email`. The schema level is kept in
    /// the tree: a lone schema is flattened away by default, and the schema
    /// rows are half of what the filter has to get right.
    async fn tree_with_open_table(
        cx: &mut TestAppContext,
    ) -> (
        gpui::Entity<DatabasePanel>,
        DriverCalls,
        &mut gpui::VisualTestContext,
    ) {
        let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;
        panel.update(cx, |panel, cx| {
            panel.connections[0].collapse_schema_level = false;
            panel.toggle_table(0, "public", "users", cx)
        });
        cx.run_until_parked();
        (panel, calls, cx)
    }

    const WHOLE_TREE: [&str; 6] = [
        "connection 0",
        "schema 0",
        "table users",
        "column id",
        "column email",
        "table active_users",
    ];

    #[gpui::test]
    async fn an_empty_filter_leaves_the_tree_as_it_is(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = tree_with_open_table(cx).await;
        panel.read_with(cx, |panel, _| assert_eq!(describe(panel), WHOLE_TREE));

        filter(&panel, "act", cx);
        filter(&panel, "  ", cx);
        panel.read_with(cx, |panel, _| assert_eq!(describe(panel), WHOLE_TREE));
    }

    #[gpui::test]
    async fn a_filter_keeps_the_matching_tables_and_the_levels_above_them(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = tree_with_open_table(cx).await;

        filter(&panel, "ACT", cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                describe(panel),
                ["connection 0", "schema 0", "table active_users"]
            );
        });
    }

    /// A table that matches by name shows every column; one that matches only
    /// through a column shows just the columns that did.
    #[gpui::test]
    async fn the_open_tables_columns_are_filtered_unless_the_table_matched(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, cx) = tree_with_open_table(cx).await;

        filter(&panel, "user", cx);
        panel.read_with(cx, |panel, _| assert_eq!(describe(panel), WHOLE_TREE));

        filter(&panel, "mail", cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                describe(panel),
                ["connection 0", "schema 0", "table users", "column email"]
            );
        });
    }

    #[gpui::test]
    async fn a_filter_nothing_matches_says_so(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = tree_with_open_table(cx).await;

        filter(&panel, "zzz", cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(describe(panel), ["note Nothing matches “zzz”"]);
        });
    }

    #[gpui::test]
    async fn a_connection_whose_name_matches_stays_without_its_tables(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = tree_with_open_table(cx).await;
        panel.update(cx, |panel, _| {
            panel.connections[0].config.name = "warehouse".into();
        });

        filter(&panel, "ware", cx);
        panel.read_with(cx, |panel, _| assert_eq!(describe(panel), ["connection 0"]));
    }

    /// Only what is loaded is searched, and the tree says which part is not.
    #[gpui::test]
    async fn a_schema_never_opened_says_it_was_not_searched(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = tree_with_open_table(cx).await;
        panel.update(cx, |panel, _| {
            panel.connections[0].schemas.push(SchemaNode {
                schema: SchemaRef {
                    name: "archive".into(),
                    is_default: false,
                },
                expanded: false,
                tables: None,
                tables_load: 0,
            });
        });

        filter(&panel, "act", cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                describe(panel),
                [
                    "connection 0",
                    "schema 0",
                    "table active_users",
                    "schema 1",
                    "note Not loaded — expand to search",
                ]
            );
        });

        filter(&panel, "", cx);
        panel.read_with(cx, |panel, _| {
            assert!(
                !describe(panel)
                    .iter()
                    .any(|line| line.contains("Not loaded")),
                "the note belongs to filtering only"
            );
        });
    }

    /// A match inside a collapsed schema is still a match: a filter that only
    /// searched what happened to be open would miss most of the tree.
    #[gpui::test]
    async fn a_filter_looks_inside_collapsed_schemas_it_has_loaded(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = tree_with_open_table(cx).await;
        panel.update(cx, |panel, _| {
            panel.connections[0].schemas[0].expanded = false;
        });
        panel.read_with(cx, |panel, _| {
            assert_eq!(describe(panel), ["connection 0", "schema 0"]);
        });

        filter(&panel, "act", cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                describe(panel),
                ["connection 0", "schema 0", "table active_users"]
            );
        });
    }

    /// A lone schema has no row of its own, and the filter must not invent one.
    #[gpui::test]
    async fn a_flattened_schema_level_stays_flat_while_filtering(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = tree_with_open_table(cx).await;
        panel.update(cx, |panel, _| {
            panel.connections[0].collapse_schema_level = true;
        });

        filter(&panel, "act", cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(describe(panel), ["connection 0", "table active_users"]);
        });
    }

    #[gpui::test]
    async fn a_refresh_keeps_the_filter(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = tree_with_open_table(cx).await;
        filter(&panel, "act", cx);

        panel.update(cx, |panel, cx| panel.refresh_connection(0, cx));
        cx.run_until_parked();

        // Flat again: the driver reports one schema, which is what the refresh
        // reads back.
        panel.read_with(cx, |panel, _| {
            assert_eq!(describe(panel), ["connection 0", "table active_users"]);
        });
    }

    #[gpui::test]
    async fn typing_in_the_box_narrows_the_tree_and_escape_clears_it(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = tree_with_open_table(cx).await;
        let input = panel.read_with(cx, |panel, _| panel.tree_filter_input.clone());

        cx.update(|window, cx| {
            let editor = input.read(cx).editor().clone();
            editor.set_text("act", window, cx);
        });
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            assert!(panel.tree_filter.is_active(), "typing sets the filter");
            assert_eq!(describe(panel).len(), 3);
        });

        cx.update(|window, cx| {
            let focus = gpui::Focusable::focus_handle(input.read(cx), cx);
            window.focus(&focus, cx);
        });
        cx.dispatch_action(editor::actions::Cancel);
        cx.run_until_parked();
        panel.read_with(cx, |panel, cx| {
            assert!(!panel.tree_filter.is_active(), "escape clears the filter");
            assert!(panel.tree_filter_input.read(cx).is_empty(cx));
            assert_eq!(describe(panel), WHOLE_TREE);
        });
    }
}

mod grid_copy {
    use super::*;
    use crate::query::{Page, QueryState, TableBrowse};
    use crate::row_serializers::CopyFormat;
    use database::protocol::{Cell, ResultColumn, ResultSet};
    use gpui::{ClipboardItem, Modifiers};

    fn text(value: &str) -> Cell {
        Cell::Text {
            value: value.into(),
        }
    }

    fn number(value: &str) -> Cell {
        Cell::Number {
            value: value.into(),
        }
    }

    /// Four rows of `id`, `note`, shown as the page on screen.
    fn show_rows(
        panel: &gpui::Entity<DatabasePanel>,
        browse: Option<TableBrowse>,
        cx: &mut gpui::VisualTestContext,
    ) {
        panel.update(cx, |panel, cx| {
            panel.query = QueryState::Done(Page {
                result: ResultSet {
                    columns: vec![
                        ResultColumn {
                            name: "id".into(),
                            type_name: "int".into(),
                        },
                        ResultColumn {
                            name: "note".into(),
                            type_name: "text".into(),
                        },
                    ],
                    rows: vec![
                        vec![number("1"), text("alpha")],
                        vec![number("2"), Cell::Null],
                        vec![number("3"), text("gamma\twith a tab")],
                        vec![number("4"), text("delta")],
                    ],
                    truncated: false,
                    elapsed_ms: 1,
                },
                sql: "select * from notes".into(),
                offset: 0,
                limit: 200,
                selected_row: None,
                selection: Default::default(),
                browse,
                connection: panel.node_key(0),
                kind: crate::safe_mode::StatementKind::Read,
                origin: Default::default(),
                edits: Default::default(),
            });
            cx.notify();
        });
        draw_again(cx);
    }

    fn selected(
        panel: &gpui::Entity<DatabasePanel>,
        cx: &mut gpui::VisualTestContext,
    ) -> Vec<usize> {
        panel.read_with(cx, |panel, _| match &panel.query {
            QueryState::Done(page) => page.selection.rows(),
            _ => Vec::new(),
        })
    }

    fn detail_row(
        panel: &gpui::Entity<DatabasePanel>,
        cx: &mut gpui::VisualTestContext,
    ) -> Option<usize> {
        panel.read_with(cx, |panel, _| match &panel.query {
            QueryState::Done(page) => page.selected_row,
            _ => None,
        })
    }

    /// Near the left edge of the row: it is as wide as its columns, and the
    /// detail drawn beside it can cover its centre.
    fn click_row(cx: &mut gpui::VisualTestContext, row: usize, offset: f32, modifiers: Modifiers) {
        let selector: &'static str = Box::leak(format!("database-result-row-{row}").into());
        let bounds = bounds_of(cx, selector);
        cx.simulate_click(
            gpui::point(bounds.left() + gpui::px(offset), bounds.center().y),
            modifiers,
        );
        draw_again(cx);
    }

    fn shift() -> Modifiers {
        Modifiers {
            shift: true,
            ..Modifiers::none()
        }
    }

    fn clipboard(cx: &mut gpui::VisualTestContext) -> Option<String> {
        cx.read_from_clipboard().and_then(|item| item.text())
    }

    fn sentinel(cx: &mut gpui::VisualTestContext) {
        cx.write_to_clipboard(ClipboardItem::new_string("untouched".into()));
    }

    /// Plain click keeps meaning "show me this row"; the modifiers only change
    /// what is selected, so building a range does not flicker the detail.
    #[gpui::test]
    async fn modifier_clicks_extend_the_selection_and_leave_the_detail_alone(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        show_rows(&panel, None, cx);

        click_row(cx, 1, 8., Modifiers::none());
        assert_eq!(selected(&panel, cx), [1]);
        assert_eq!(
            detail_row(&panel, cx),
            Some(1),
            "the detail opens as before"
        );

        click_row(cx, 3, 8., shift());
        assert_eq!(selected(&panel, cx), [1, 2, 3]);
        assert_eq!(
            detail_row(&panel, cx),
            Some(1),
            "a range does not toggle it"
        );

        click_row(cx, 2, 8., Modifiers::secondary_key());
        assert_eq!(selected(&panel, cx), [1, 3]);
        assert_eq!(detail_row(&panel, cx), Some(1));

        click_row(cx, 0, 8., Modifiers::none());
        assert_eq!(selected(&panel, cx), [0], "a plain click replaces it");
        assert_eq!(detail_row(&panel, cx), Some(0));
    }

    #[gpui::test]
    async fn cmd_c_on_a_range_of_rows_copies_them_as_tsv(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        show_rows(&panel, None, cx);

        click_row(cx, 0, 8., Modifiers::none());
        click_row(cx, 2, 8., shift());
        cx.dispatch_action(zed_actions::database::CopySelection);

        assert_eq!(
            clipboard(cx).as_deref(),
            Some("1\talpha\n2\t\n3\t\"gamma\twith a tab\"\n"),
            "three rows by two columns, the tab quoted and the null empty"
        );
    }

    #[gpui::test]
    async fn cmd_c_on_one_clicked_cell_copies_its_value(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        show_rows(&panel, None, cx);

        // The `id` column is as wide as a short column goes, so this lands in
        // `note`.
        click_row(cx, 3, 120., Modifiers::none());
        cx.dispatch_action(zed_actions::database::CopySelection);
        assert_eq!(clipboard(cx).as_deref(), Some("delta"));

        click_row(cx, 3, 8., Modifiers::none());
        cx.dispatch_action(zed_actions::database::CopySelection);
        assert_eq!(clipboard(cx).as_deref(), Some("4"), "the other column");
    }

    /// A null has no text; pasting the word would paste a value that was never
    /// there.
    #[gpui::test]
    async fn cmd_c_on_a_null_cell_copies_nothing(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        show_rows(&panel, None, cx);
        sentinel(cx);

        click_row(cx, 1, 120., Modifiers::none());
        cx.dispatch_action(zed_actions::database::CopySelection);

        assert_eq!(clipboard(cx).as_deref(), Some("untouched"));
    }

    #[gpui::test]
    async fn cmd_a_selects_every_row_and_the_arrows_move_one(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        show_rows(&panel, None, cx);

        click_row(cx, 0, 8., Modifiers::none());
        cx.dispatch_action(zed_actions::database::SelectAllRows);
        assert_eq!(selected(&panel, cx), [0, 1, 2, 3]);

        cx.dispatch_action(zed_actions::database::CopySelection);
        let copied = clipboard(cx).unwrap_or_default();
        assert_eq!(copied.lines().count(), 4, "four rows and no header");

        // The detail is on row 0 from the first click, so a plain click on row
        // 1 moves it rather than closing it.
        click_row(cx, 1, 8., Modifiers::none());
        assert_eq!(detail_row(&panel, cx), Some(1));
        cx.dispatch_action(menu::SelectNext);
        cx.dispatch_action(menu::SelectNext);
        assert_eq!(selected(&panel, cx), [3]);
        assert_eq!(detail_row(&panel, cx), Some(3), "an open detail follows");
        cx.dispatch_action(menu::SelectNext);
        assert_eq!(selected(&panel, cx), [3], "it stops at the last row");
        cx.dispatch_action(menu::SelectPrevious);
        assert_eq!(selected(&panel, cx), [2]);
        for _ in 0..5 {
            cx.dispatch_action(menu::SelectPrevious);
        }
        assert_eq!(selected(&panel, cx), [0], "and at the first");
    }

    #[gpui::test]
    async fn a_new_page_starts_with_nothing_selected(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        show_rows(&panel, None, cx);
        panel.update(cx, |panel, cx| panel.select_all_rows(cx));
        assert_eq!(selected(&panel, cx), [0, 1, 2, 3]);

        panel.update_in(cx, |panel, window, cx| {
            panel.open_table_data(0, "public", "users", window, cx)
        });
        cx.run_until_parked();

        assert_eq!(selected(&panel, cx), Vec::<usize>::new());
    }

    #[gpui::test]
    async fn rows_copy_as_csv_json_and_insert_for_the_table_they_came_from(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        let browse = TableBrowse::new(
            ("a".to_string(), crate::database_panel::NodeOrigin::Personal),
            "main",
            "notes",
        );
        show_rows(&panel, Some(browse), cx);
        panel.update(cx, |panel, cx| {
            panel.active = Some(0);
            panel.select_all_rows(cx);
        });

        panel.update(cx, |panel, cx| panel.copy_rows_as(CopyFormat::Csv, cx));
        assert!(
            clipboard(cx)
                .unwrap_or_default()
                .starts_with("id,note\r\n1,alpha\r\n2,\r\n"),
        );

        panel.update(cx, |panel, cx| panel.copy_rows_as(CopyFormat::Json, cx));
        let json = clipboard(cx).unwrap_or_default();
        assert!(
            json.starts_with("[\n  {\"id\": 1, \"note\": \"alpha\"}"),
            "{json}"
        );

        panel.update(cx, |panel, cx| panel.copy_rows_as(CopyFormat::Insert, cx));
        let insert = clipboard(cx).unwrap_or_default();
        assert!(
            insert.starts_with(
                "INSERT INTO \"main\".\"notes\" (\"id\", \"note\") VALUES (1, 'alpha');\n"
            ),
            "{insert}"
        );
        assert_eq!(insert.lines().count(), 4, "one statement per row");
    }

    #[gpui::test]
    async fn insert_is_not_written_for_a_document_store(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        show_rows(&panel, None, cx);
        panel.update(cx, |panel, cx| {
            panel.connections[0].config.driver = "mongodb".into();
            panel.active = Some(0);
            panel.select_all_rows(cx);
        });
        sentinel(cx);

        panel.update(cx, |panel, cx| panel.copy_rows_as(CopyFormat::Insert, cx));

        assert_eq!(clipboard(cx).as_deref(), Some("untouched"));
    }

    /// The menu acts on the rows it was opened over: one outside the selection
    /// becomes the selection, one inside it keeps it.
    #[gpui::test]
    async fn right_clicking_a_cell_opens_a_menu_over_the_selected_rows(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        show_rows(&panel, None, cx);
        click_row(cx, 0, 8., Modifiers::none());
        click_row(cx, 2, 8., shift());

        let bounds = bounds_of(cx, "database-result-row-3");
        let point = gpui::point(bounds.left() + gpui::px(8.), bounds.center().y);
        cx.simulate_mouse_down(point, gpui::MouseButton::Right, Modifiers::none());
        cx.simulate_mouse_up(point, gpui::MouseButton::Right, Modifiers::none());
        draw_again(cx);
        panel.read_with(cx, |panel, _| {
            assert!(panel.context_menu.is_some(), "the cell has a menu");
        });
        assert_eq!(
            selected(&panel, cx),
            [3],
            "outside the selection replaces it"
        );

        panel.update(cx, |panel, _| panel.context_menu = None);
        panel.update(cx, |panel, cx| panel.select_all_rows(cx));
        let bounds = bounds_of(cx, "database-result-row-1");
        let point = gpui::point(bounds.left() + gpui::px(8.), bounds.center().y);
        cx.simulate_mouse_down(point, gpui::MouseButton::Right, Modifiers::none());
        cx.simulate_mouse_up(point, gpui::MouseButton::Right, Modifiers::none());
        draw_again(cx);
        assert_eq!(selected(&panel, cx), [0, 1, 2, 3], "inside it keeps it");
    }
}

mod browse_sort_and_filter {
    use super::*;
    use crate::browse_filter_bar::filter_for_cell;
    use crate::query::{FilterOp, QueryState};
    use database::protocol::Cell;

    fn page_of(panel: &DatabasePanel) -> (String, u64) {
        match &panel.query {
            QueryState::Done(page) => (page.sql.clone(), page.offset),
            _ => panic!("a page was expected"),
        }
    }

    async fn browsing(
        cx: &mut TestAppContext,
    ) -> (gpui::Entity<DatabasePanel>, &mut gpui::VisualTestContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.open_table_data(0, "public", "users", window, cx)
        });
        draw_again(cx);
        (panel, cx)
    }

    fn click_header(cx: &mut gpui::VisualTestContext, selector: &'static str) {
        let bounds = bounds_of(cx, selector);
        cx.simulate_click(
            gpui::point(bounds.left() + gpui::px(4.), bounds.center().y),
            gpui::Modifiers::none(),
        );
        draw_again(cx);
    }

    #[gpui::test]
    async fn clicking_a_header_cycles_ascending_descending_and_back_from_the_first_row(
        cx: &mut TestAppContext,
    ) {
        let (panel, cx) = browsing(cx).await;
        // Off the first page, to see the re-read start over rather than stay.
        panel.update(cx, |panel, _| {
            if let QueryState::Done(page) = &mut panel.query {
                page.offset = 2;
            }
        });

        click_header(cx, "database-sort-id");
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                page_of(panel),
                (
                    "SELECT *\nFROM \"public\".\"users\"\nORDER BY \"id\" ASC".into(),
                    0
                )
            );
        });

        click_header(cx, "database-sort-id");
        panel.read_with(cx, |panel, _| {
            assert!(page_of(panel).0.ends_with("ORDER BY \"id\" DESC"));
        });

        click_header(cx, "database-sort-id");
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                page_of(panel).0,
                "SELECT *\nFROM \"public\".\"users\"\nORDER BY \"id\" ASC",
                "unsorted is the key's order, not the server's"
            );
        });

        click_header(cx, "database-sort-email");
        click_header(cx, "database-sort-id");
        panel.read_with(cx, |panel, _| {
            assert!(
                page_of(panel).0.ends_with("ORDER BY \"id\" ASC"),
                "another column replaces the sort and starts ascending"
            );
        });
    }

    #[gpui::test]
    async fn paging_keeps_the_order_and_the_filter(cx: &mut TestAppContext) {
        let (panel, cx) = browsing(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.toggle_filter_bar(window, cx);
            panel.set_filter_op(0, FilterOp::Greater, cx);
            panel.browse_bar.type_into_row(0, "1", window, cx);
            panel.apply_filters(window, cx);
        });
        draw_again(cx);
        click_header(cx, "database-sort-email");

        let expected =
            "SELECT *\nFROM \"public\".\"users\"\nWHERE \"id\" > 1\nORDER BY \"email\" ASC";
        panel.read_with(cx, |panel, _| {
            assert_eq!(page_of(panel), (expected.into(), 0));
        });

        panel.update_in(cx, |panel, window, cx| panel.page(true, window, cx));
        draw_again(cx);
        panel.read_with(cx, |panel, _| {
            let (sql, offset) = page_of(panel);
            assert_eq!(sql, expected);
            assert!(offset > 0, "the next page, not the first again");
            assert!(panel.has_active_filters());
        });
    }

    #[gpui::test]
    async fn a_filter_row_with_nothing_typed_filters_nothing(cx: &mut TestAppContext) {
        let (panel, cx) = browsing(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.toggle_filter_bar(window, cx);
            assert_eq!(panel.browse_bar.row_count(), 1);
            panel.apply_filters(window, cx);
        });
        draw_again(cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                page_of(panel).0,
                "SELECT *\nFROM \"public\".\"users\"\nORDER BY \"id\" ASC"
            );
        });
    }

    #[gpui::test]
    async fn the_bar_is_collapsed_until_asked_for_and_the_pager_toggles_it(
        cx: &mut TestAppContext,
    ) {
        let (panel, cx) = browsing(cx).await;
        assert!(cx.debug_bounds("database-filter-bar").is_none());

        panel.update_in(cx, |panel, window, cx| panel.toggle_filter_bar(window, cx));
        draw_again(cx);
        assert!(cx.debug_bounds("database-filter-bar").is_some());

        panel.update_in(cx, |panel, window, cx| panel.toggle_filter_bar(window, cx));
        draw_again(cx);
        assert!(cx.debug_bounds("database-filter-bar").is_none());
    }

    #[gpui::test]
    async fn filter_by_this_value_adds_to_the_filters_and_opens_the_bar(cx: &mut TestAppContext) {
        let (panel, cx) = browsing(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.filter_by_value(0, 0, window, cx)
        });
        draw_again(cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                page_of(panel),
                (
                    "SELECT *\nFROM \"public\".\"users\"\nWHERE \"id\" = 1\nORDER BY \"id\" ASC"
                        .into(),
                    0
                ),
                "the id column holds numbers, so the value is bare"
            );
        });
        assert!(cx.debug_bounds("database-filter-bar").is_some());

        // The fake's second row has a null email.
        panel.update_in(cx, |panel, window, cx| {
            panel.filter_by_value(0, 1, window, cx)
        });
        draw_again(cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(panel.browse_bar.row_count(), 2);
            assert!(
                page_of(panel).0.ends_with(
                    "WHERE \"id\" = 1 AND \"email\" = 'user0@example.com'\nORDER BY \"id\" ASC"
                ),
                "{}",
                page_of(panel).0
            );
        });
    }

    #[gpui::test]
    async fn clearing_removes_every_filter_and_folds_the_bar_away(cx: &mut TestAppContext) {
        let (panel, cx) = browsing(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.filter_by_value(0, 0, window, cx)
        });
        draw_again(cx);
        panel.update_in(cx, |panel, window, cx| panel.clear_filters(window, cx));
        draw_again(cx);

        panel.read_with(cx, |panel, _| {
            assert_eq!(
                page_of(panel),
                (
                    "SELECT *\nFROM \"public\".\"users\"\nORDER BY \"id\" ASC".into(),
                    0
                )
            );
            assert!(!panel.has_active_filters());
        });
        assert!(cx.debug_bounds("database-filter-bar").is_none());
    }

    /// A filter that fails is the one thing the bar is needed for.
    #[gpui::test]
    async fn the_bar_stays_when_the_filtered_read_fails(cx: &mut TestAppContext) {
        let (panel, cx) = browsing(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.filter_by_value(0, 0, window, cx)
        });
        draw_again(cx);
        panel.update(cx, |panel, cx| {
            panel.query = QueryState::Failed(crate::query::QueryError {
                message: "no such column".into(),
                read_only: false,
            });
            cx.notify();
        });
        draw_again(cx);
        assert!(cx.debug_bounds("database-filter-bar").is_some());
    }

    #[gpui::test]
    async fn a_typed_statement_has_no_sort_and_no_filter(cx: &mut TestAppContext) {
        let (panel, cx) = browsing(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.count_rows(0, "public", "users", window, cx)
        });
        draw_again(cx);
        panel.read_with(cx, |panel, _| {
            assert!(!panel.can_sort());
            assert!(!panel.can_filter());
        });
        assert!(cx.debug_bounds("database-sort-id").is_none());
        assert!(cx.debug_bounds("database-toggle-filter").is_none());
        panel.update_in(cx, |panel, window, cx| panel.sort_by("id", window, cx));
        panel.read_with(cx, |panel, _| {
            assert!(
                page_of(panel).0.contains("COUNT"),
                "a sort request on a count changes nothing"
            );
        });
    }

    #[gpui::test]
    async fn a_document_store_sorts_but_is_not_offered_a_filter(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        panel.update(cx, |panel, _| {
            panel.connections[0].config.driver = "mongodb".into();
        });
        panel.update_in(cx, |panel, window, cx| {
            panel.open_table_data(0, "app", "users", window, cx)
        });
        draw_again(cx);

        assert!(cx.debug_bounds("database-toggle-filter").is_none());
        panel.update_in(cx, |panel, window, cx| {
            panel.toggle_filter_bar(window, cx);
            assert_eq!(panel.browse_bar.row_count(), 0, "the toggle does nothing");
        });

        click_header(cx, "database-sort-id");
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                page_of(panel).0,
                r#"{"find": "users", "sort": {"id": 1}, "$db": "app"}"#
            );
        });
    }

    #[gpui::test]
    async fn opening_a_table_afresh_drops_the_filters_of_the_last_one(cx: &mut TestAppContext) {
        let (panel, cx) = browsing(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.filter_by_value(0, 0, window, cx)
        });
        draw_again(cx);
        panel.update_in(cx, |panel, window, cx| {
            panel.open_table_data(0, "public", "users", window, cx)
        });
        draw_again(cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                page_of(panel).0,
                "SELECT *\nFROM \"public\".\"users\"\nORDER BY \"id\" ASC"
            );
            assert_eq!(panel.browse_bar.row_count(), 0);
        });
        assert!(cx.debug_bounds("database-filter-bar").is_none());
    }

    #[test]
    fn the_cell_menu_offers_a_filter_for_a_value_and_a_null_but_not_a_blob() {
        assert!(filter_for_cell("a".into(), &Cell::Text { value: "x".into() }).is_some());
        assert!(filter_for_cell("a".into(), &Cell::Null).is_some());
        assert!(filter_for_cell("a".into(), &Cell::Binary { byte_len: 1 }).is_none());
    }
}

/// Running the statement the cursor is in, and the history of what ran.
mod statement_and_history {
    use super::*;
    use crate::panel_queries::RunScope;
    use crate::query::QueryState;
    use crate::query_history::{HistoryEntry, MAX_ENTRIES, Outcome, QueryHistory};
    use editor::{HighlightKey, SelectionEffects};
    use multi_buffer::MultiBufferOffset;
    use picker::PickerDelegate as _;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A name no other test shares: every test in the process reads and writes
    /// the one fallback key-value store.
    fn unique_connection(panel: &gpui::Entity<DatabasePanel>, cx: &mut gpui::VisualTestContext) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!("history-test-{}", NEXT.fetch_add(1, Ordering::SeqCst));
        panel.update(cx, |panel, _| panel.connections[0].config.name = name);
    }

    fn connection_name(
        panel: &gpui::Entity<DatabasePanel>,
        cx: &mut gpui::VisualTestContext,
    ) -> String {
        panel.read_with(cx, |panel, _| panel.connections[0].config.name.clone())
    }

    /// Fills the buffer with `marked` minus its `|`, which is where the cursor
    /// goes, and points the panel at the connection.
    async fn typed<'a>(
        cx: &'a mut TestAppContext,
        marked: &str,
    ) -> (gpui::Entity<DatabasePanel>, &'a mut gpui::VisualTestContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        unique_connection(&panel, cx);
        let offset = marked.find('|').expect("a cursor marker");
        let text = marked.replacen('|', "", 1);
        panel.update_in(cx, |panel, window, cx| {
            panel.set_active(0, window, cx);
            panel.sql_editor.update(cx, |editor, cx| {
                editor.set_text(text, window, cx);
                editor.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
                    selections.select_ranges([MultiBufferOffset(offset)..MultiBufferOffset(offset)])
                });
            });
        });
        (panel, cx)
    }

    fn editor_text(
        panel: &gpui::Entity<DatabasePanel>,
        cx: &mut gpui::VisualTestContext,
    ) -> String {
        panel.read_with(cx, |panel, cx| panel.sql_editor.read(cx).text(cx))
    }

    fn sent(panel: &gpui::Entity<DatabasePanel>, cx: &mut gpui::VisualTestContext) -> String {
        panel.read_with(cx, |panel, _| match &panel.query {
            QueryState::Done(page) => page.sql.clone(),
            _ => panic!("a page was expected"),
        })
    }

    fn run(panel: &gpui::Entity<DatabasePanel>, scope: RunScope, cx: &mut gpui::VisualTestContext) {
        panel.update_in(cx, |panel, window, cx| panel.run_query(scope, window, cx));
        cx.run_until_parked();
    }

    fn stored(name: &str, cx: &mut gpui::VisualTestContext) -> QueryHistory {
        let store = cx.update(|_, cx| db::kvp::KeyValueStore::global(cx));
        crate::query_history::read_stored(&store, name).expect("the store reads")
    }

    #[gpui::test]
    async fn the_cursor_picks_one_statement_out_of_several(cx: &mut TestAppContext) {
        let (panel, cx) = typed(cx, "select 1;\nselect 2|;\nselect 3").await;
        run(&panel, RunScope::Statement, cx);
        assert_eq!(sent(&panel, cx), "select 2");
    }

    #[gpui::test]
    async fn run_all_sends_the_whole_buffer(cx: &mut TestAppContext) {
        let (panel, cx) = typed(cx, "select 1;\nselect 2|;\nselect 3").await;
        run(&panel, RunScope::All, cx);
        assert_eq!(sent(&panel, cx), "select 1;\nselect 2;\nselect 3");
    }

    #[gpui::test]
    async fn a_selection_wins_over_the_cursor_for_both_scopes(cx: &mut TestAppContext) {
        let (panel, cx) = typed(cx, "select 1;\nselect 2|;\nselect 3").await;
        panel.update_in(cx, |panel, window, cx| {
            panel.sql_editor.update(cx, |editor, cx| {
                editor.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
                    // `select 3`
                    selections.select_ranges([MultiBufferOffset(20)..MultiBufferOffset(28)])
                });
            });
        });
        run(&panel, RunScope::Statement, cx);
        assert_eq!(sent(&panel, cx), "select 3");
        run(&panel, RunScope::All, cx);
        assert_eq!(sent(&panel, cx), "select 3");
    }

    #[gpui::test]
    async fn a_document_buffer_is_not_split_at_a_semicolon(cx: &mut TestAppContext) {
        let (panel, cx) = typed(cx, r#"{"find": "a;b"|}"#).await;
        panel.update(cx, |panel, _| {
            panel.connections[0].config.driver = "mongodb".into()
        });
        run(&panel, RunScope::Statement, cx);
        assert_eq!(sent(&panel, cx), r#"{"find": "a;b"}"#);
    }

    #[gpui::test]
    async fn a_buffer_of_only_blanks_runs_nothing(cx: &mut TestAppContext) {
        let (panel, cx) = typed(cx, "  \n -- nothing|\n").await;
        run(&panel, RunScope::Statement, cx);
        panel.read_with(cx, |panel, _| {
            assert!(matches!(panel.query, QueryState::Idle));
        });
    }

    #[gpui::test]
    async fn the_statement_that_ran_is_lit_until_the_timer_or_an_edit(cx: &mut TestAppContext) {
        let (panel, cx) = typed(cx, "select 1;\nselect 2|;").await;
        let lit = |panel: &gpui::Entity<DatabasePanel>, cx: &mut gpui::VisualTestContext| {
            panel.read_with(cx, |panel, cx| {
                panel
                    .sql_editor
                    .read(cx)
                    .has_background_highlights(HighlightKey::HighlightOnYank)
            })
        };

        run(&panel, RunScope::Statement, cx);
        assert!(lit(&panel, cx), "the statement is lit right after it ran");
        cx.background_executor
            .timer(std::time::Duration::from_millis(700))
            .await;
        cx.run_until_parked();
        assert!(!lit(&panel, cx), "the timer puts it out");

        run(&panel, RunScope::Statement, cx);
        assert!(lit(&panel, cx));
        panel.update_in(cx, |panel, window, cx| {
            panel
                .sql_editor
                .update(cx, |editor, cx| editor.insert("x", window, cx));
        });
        cx.run_until_parked();
        assert!(!lit(&panel, cx), "typing puts it out at once");
    }

    #[gpui::test]
    async fn running_a_whole_buffer_does_not_light_anything(cx: &mut TestAppContext) {
        let (panel, cx) = typed(cx, "select 1;\nselect 2|;").await;
        run(&panel, RunScope::All, cx);
        panel.read_with(cx, |panel, cx| {
            assert!(
                !panel
                    .sql_editor
                    .read(cx)
                    .has_background_highlights(HighlightKey::HighlightOnYank)
            );
        });
    }

    #[gpui::test]
    async fn a_run_is_remembered_and_survives_a_new_panel(cx: &mut TestAppContext) {
        let (panel, cx) = typed(cx, "select 1;\nselect 2|;").await;
        let name = connection_name(&panel, cx);
        run(&panel, RunScope::Statement, cx);

        let history = stored(&name, cx).newest_first();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].sql, "select 2");
        assert!(
            matches!(history[0].outcome, Outcome::Rows { count: 3 }),
            "the rows of the first page are counted"
        );

        // A restart is a panel that has never heard of the run, reading the
        // same store: the next run lands after it rather than replacing it.
        panel.update(cx, |panel, _| panel.history.clear());
        run(&panel, RunScope::All, cx);
        let history = stored(&name, cx).newest_first();
        assert_eq!(
            history
                .iter()
                .map(|entry| entry.sql.as_str())
                .collect::<Vec<_>>(),
            ["select 1;\nselect 2;", "select 2"]
        );
    }

    #[gpui::test]
    async fn paging_and_opening_a_table_are_not_history(cx: &mut TestAppContext) {
        let (panel, cx) = typed(cx, "select 1|").await;
        let name = connection_name(&panel, cx);
        run(&panel, RunScope::Statement, cx);
        panel.update_in(cx, |panel, window, cx| panel.page(true, window, cx));
        cx.run_until_parked();
        panel.update_in(cx, |panel, window, cx| {
            panel.open_table_data(0, "public", "users", window, cx)
        });
        cx.run_until_parked();

        assert_eq!(stored(&name, cx).len(), 1, "only the typed statement");
    }

    #[gpui::test]
    async fn a_failed_entry_is_stored_with_its_reason(cx: &mut TestAppContext) {
        let (panel, cx) = typed(cx, "select 1|").await;
        let name = connection_name(&panel, cx);
        panel.update(cx, |panel, cx| {
            panel.record_history(
                name.clone(),
                HistoryEntry::failed("bad".into(), 1, 2, "no such table: bad"),
                cx,
            )
        });
        cx.run_until_parked();

        let history = stored(&name, cx).newest_first();
        assert!(matches!(
            &history[0].outcome,
            Outcome::Failed { message } if message == "no such table: bad"
        ));
    }

    #[gpui::test]
    async fn the_stored_history_never_passes_the_cap(cx: &mut TestAppContext) {
        let (panel, cx) = typed(cx, "select 1|").await;
        let name = connection_name(&panel, cx);
        panel.update(cx, |panel, cx| {
            for number in 0..MAX_ENTRIES + 20 {
                panel.record_history(
                    name.clone(),
                    HistoryEntry {
                        sql: format!("select {number}"),
                        started_at_ms: 0,
                        elapsed_ms: 0,
                        outcome: Outcome::Rows { count: 0 },
                    },
                    cx,
                );
            }
        });
        cx.run_until_parked();

        let history = stored(&name, cx);
        assert_eq!(history.len(), MAX_ENTRIES);
        assert_eq!(
            history.newest_first()[0].sql,
            format!("select {}", MAX_ENTRIES + 19)
        );
    }

    #[gpui::test]
    async fn each_connection_keeps_its_own_history(cx: &mut TestAppContext) {
        let (panel, cx) = typed(cx, "select 1|").await;
        let name = connection_name(&panel, cx);
        run(&panel, RunScope::Statement, cx);
        assert_eq!(stored(&name, cx).len(), 1);
        assert_eq!(stored(&format!("{name}-other"), cx).len(), 0);
    }

    async fn store_entries(name: &str, sql: &[&str], cx: &mut gpui::VisualTestContext) {
        let store = cx.update(|_, cx| db::kvp::KeyValueStore::global(cx));
        let mut history = stored(name, cx);
        for sql in sql {
            history.record(HistoryEntry {
                sql: (*sql).into(),
                started_at_ms: 1_700_000_000_000,
                elapsed_ms: 1,
                outcome: Outcome::Rows { count: 1 },
            });
        }
        crate::query_history::write_stored(
            store,
            name.to_string(),
            history.to_json().expect("serialises"),
        )
        .await
        .expect("the store accepts it");
    }

    /// Nothing reads the store until the list is asked for, so the list opens
    /// on a read that is still out and has to show what comes back.
    #[gpui::test]
    async fn the_first_open_of_the_list_shows_what_an_earlier_session_stored(
        cx: &mut TestAppContext,
    ) {
        let (panel, cx) = typed(cx, "select 1|").await;
        let name = connection_name(&panel, cx);
        store_entries(&name, &["select 'a'", "select 'b'"], cx).await;

        let picker = open_history(&panel, cx);

        picker.read_with(cx, |picker, _| {
            assert_eq!(picker.delegate.match_count(), 2);
        });
    }

    #[gpui::test]
    async fn a_search_typed_while_the_history_loads_still_applies_when_it_lands(
        cx: &mut TestAppContext,
    ) {
        let (panel, cx) = typed(cx, "select 1|").await;
        let name = connection_name(&panel, cx);
        store_entries(&name, &["select 'a'", "select 'b'"], cx).await;

        panel.update_in(cx, |panel, window, cx| panel.show_query_history(window, cx));
        let picker = panel.read_with(cx, |panel, _| {
            panel
                .history_popover
                .as_ref()
                .map(|(picker, _)| picker.clone())
                .expect("the list is open")
        });
        picker.update_in(cx, |picker, window, cx| picker.set_query("'b'", window, cx));
        cx.run_until_parked();

        picker.read_with(cx, |picker, _| {
            assert_eq!(picker.delegate.match_count(), 1);
        });
    }

    /// Two windows keep one list per connection between them. Each writes down
    /// what it ran, on top of what is stored, not the list as it remembers it.
    #[gpui::test]
    async fn what_another_window_stored_in_between_is_not_overwritten(cx: &mut TestAppContext) {
        let (panel, cx) = typed(cx, "select 'mine'|").await;
        let name = connection_name(&panel, cx);
        run(&panel, RunScope::Statement, cx);

        store_entries(&name, &["select 'theirs'"], cx).await;

        panel.update_in(cx, |panel, window, cx| {
            panel.sql_editor.update(cx, |editor, cx| {
                editor.set_text("select 'mine too'", window, cx)
            });
        });
        run(&panel, RunScope::All, cx);

        let sql: Vec<String> = stored(&name, cx)
            .newest_first()
            .into_iter()
            .map(|entry| entry.sql)
            .collect();
        assert_eq!(
            sql,
            ["select 'mine too'", "select 'theirs'", "select 'mine'"]
        );
    }

    /// A list that cannot be read is not an empty one: writing the next run
    /// over it would turn a read error into the loss of everything stored.
    #[gpui::test]
    async fn a_store_that_cannot_be_read_is_not_written_over(cx: &mut TestAppContext) {
        let (panel, cx) = typed(cx, "select 1|").await;
        let name = connection_name(&panel, cx);
        let store = cx.update(|_, cx| db::kvp::KeyValueStore::global(cx));
        crate::query_history::write_stored(store, name.clone(), "{not json".into())
            .await
            .expect("the store accepts anything");

        run(&panel, RunScope::Statement, cx);

        let store = cx.update(|_, cx| db::kvp::KeyValueStore::global(cx));
        assert!(
            crate::query_history::read_stored(&store, &name).is_err(),
            "the unreadable list is still what is stored"
        );
        let picker = open_history(&panel, cx);
        picker.read_with(cx, |picker, _| {
            assert_eq!(
                picker.delegate.match_count(),
                1,
                "this session's run is still listed"
            );
        });
    }

    /// Clearing is the one write that replaces: it is what the user asked for.
    #[gpui::test]
    async fn clearing_is_not_undone_by_what_was_stored_before(cx: &mut TestAppContext) {
        let (panel, cx) = with_two_runs(cx).await;
        let name = connection_name(&panel, cx);
        panel.update(cx, |panel, cx| panel.clear_history(name.clone(), cx));
        run(&panel, RunScope::All, cx);
        assert_eq!(stored(&name, cx).len(), 1, "only the run after the clear");
    }

    fn open_history(
        panel: &gpui::Entity<DatabasePanel>,
        cx: &mut gpui::VisualTestContext,
    ) -> gpui::Entity<picker::Picker<crate::query_history_popover::HistoryDelegate>> {
        panel.update_in(cx, |panel, window, cx| panel.show_query_history(window, cx));
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            panel
                .history_popover
                .as_ref()
                .map(|(picker, _)| picker.clone())
                .expect("the list is open")
        })
    }

    async fn with_two_runs(
        cx: &mut TestAppContext,
    ) -> (gpui::Entity<DatabasePanel>, &mut gpui::VisualTestContext) {
        let (panel, cx) = typed(cx, "select 'users'|").await;
        run(&panel, RunScope::Statement, cx);
        panel.update_in(cx, |panel, window, cx| {
            panel.sql_editor.update(cx, |editor, cx| {
                editor.set_text("select 'orders'", window, cx)
            });
        });
        run(&panel, RunScope::All, cx);
        (panel, cx)
    }

    #[gpui::test]
    async fn the_history_list_shows_newest_first_and_search_narrows_it(cx: &mut TestAppContext) {
        let (panel, cx) = with_two_runs(cx).await;
        let picker = open_history(&panel, cx);
        picker.read_with(cx, |picker, _| {
            assert_eq!(picker.delegate.match_count(), 2);
        });

        picker.update_in(cx, |picker, window, cx| {
            picker.set_query("USERS", window, cx)
        });
        cx.run_until_parked();
        picker.read_with(cx, |picker, _| {
            assert_eq!(picker.delegate.match_count(), 1, "case is ignored");
        });

        picker.update_in(cx, |picker, window, cx| {
            picker.set_query("nothing like it", window, cx)
        });
        cx.run_until_parked();
        picker.read_with(cx, |picker, _| assert_eq!(picker.delegate.match_count(), 0));
    }

    #[gpui::test]
    async fn choosing_an_entry_inserts_it_and_a_secondary_choice_runs_it(cx: &mut TestAppContext) {
        let (panel, cx) = with_two_runs(cx).await;
        open_history(&panel, cx);

        // Newest first, so the first row is `orders`; choosing `users` is the
        // second.
        cx.dispatch_action(menu::SelectNext);
        cx.dispatch_action(menu::Confirm);
        cx.run_until_parked();
        assert_eq!(editor_text(&panel, cx), "select 'orders'\n\nselect 'users'");
        panel.read_with(cx, |panel, _| {
            assert!(panel.history_popover.is_none(), "choosing closes the list");
        });

        open_history(&panel, cx);
        cx.dispatch_action(menu::SelectNext);
        cx.dispatch_action(menu::SecondaryConfirm);
        cx.run_until_parked();
        assert_eq!(sent(&panel, cx), "select 'users'");
        assert_eq!(
            editor_text(&panel, cx),
            "select 'orders'\n\nselect 'users'",
            "running from the list leaves the buffer alone"
        );
    }

    #[gpui::test]
    async fn clearing_history_empties_the_list_and_the_store(cx: &mut TestAppContext) {
        let (panel, cx) = with_two_runs(cx).await;
        let name = connection_name(&panel, cx);
        assert_eq!(stored(&name, cx).len(), 2);

        panel.update(cx, |panel, cx| panel.clear_history(name.clone(), cx));
        cx.run_until_parked();
        assert_eq!(stored(&name, cx).len(), 0);
        let picker = open_history(&panel, cx);
        picker.read_with(cx, |picker, _| assert_eq!(picker.delegate.match_count(), 0));
    }

    #[gpui::test]
    async fn the_history_button_toggles_the_list(cx: &mut TestAppContext) {
        let (panel, cx) = typed(cx, "select 1|").await;
        let _ = open_history(&panel, cx);
        panel.update_in(cx, |panel, window, cx| panel.show_query_history(window, cx));
        panel.read_with(cx, |panel, _| assert!(panel.history_popover.is_none()));
    }
}

/// Exporting a result to a file.
mod export {
    use super::*;
    use crate::export_results::{ExportFormat, ExportScope, ExportStatus};
    use crate::panel_queries::RunScope;
    use crate::query::TableBrowse;
    use crate::row_serializers::{to_csv, to_json};
    use database::fake_driver::FakeBehaviour;
    use database::protocol::{Cell, ResultColumn, ResultSet};
    use editor::SelectionEffects;
    use std::path::{Path, PathBuf};

    /// A panel that has run a statement against the fake driver, which owns
    /// three rows, with pages of one row.
    async fn exported_from(
        cx: &mut TestAppContext,
    ) -> (
        gpui::Entity<DatabasePanel>,
        DriverCalls,
        std::sync::Arc<dyn fs::Fs>,
        &mut gpui::VisualTestContext,
    ) {
        let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;
        cx.update(|_window, cx| {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |settings| {
                    settings.database.get_or_insert_default().page_size = Some(1);
                });
            });
        });
        panel.update_in(cx, |panel, window, cx| {
            panel.set_active(0, window, cx);
            panel.sql_editor.update(cx, |editor, cx| {
                editor.set_text("select * from users", window, cx);
                editor.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
                    selections.select_ranges([
                        multi_buffer::MultiBufferOffset(0)..multi_buffer::MultiBufferOffset(0)
                    ])
                });
            });
            panel.run_query(RunScope::All, window, cx);
        });
        cx.run_until_parked();
        let fs = panel.read_with(cx, |panel, cx| {
            panel
                .workspace
                .read_with(cx, |workspace, cx| {
                    workspace.project().read(cx).fs().clone()
                })
                .expect("the workspace is alive")
        });
        fs.create_dir(Path::new("/exports"))
            .await
            .expect("the folder the dialog would have chosen");
        (panel, calls, fs, cx)
    }

    /// The three rows the fake driver owns, as one result.
    fn the_three_rows() -> ResultSet {
        let row = |id: &str, email: Option<&str>| {
            vec![
                Cell::Number { value: id.into() },
                email.map_or(Cell::Null, |email| Cell::Text {
                    value: email.into(),
                }),
            ]
        };
        ResultSet {
            columns: vec![
                ResultColumn {
                    name: "id".into(),
                    type_name: "integer".into(),
                },
                ResultColumn {
                    name: "email".into(),
                    type_name: "text".into(),
                },
            ],
            rows: vec![
                row("1", Some("user0@example.com")),
                row("2", None),
                row("3", Some("user2@example.com")),
            ],
            truncated: false,
            elapsed_ms: 0,
        }
    }

    fn start(
        panel: &gpui::Entity<DatabasePanel>,
        path: &Path,
        format: ExportFormat,
        scope: ExportScope,
        cx: &mut gpui::VisualTestContext,
    ) {
        let path = path.to_path_buf();
        panel.update(cx, |panel, cx| panel.export_to(path, format, scope, cx));
    }

    fn status(
        panel: &gpui::Entity<DatabasePanel>,
        cx: &mut gpui::VisualTestContext,
    ) -> ExportStatus {
        panel.read_with(cx, |panel, _| panel.export.status.clone())
    }

    fn target(name: &str) -> PathBuf {
        PathBuf::from("/exports").join(name)
    }

    /// Pages of one row, so that three rows are three trips to the server.
    fn one_row_pages(panel: &gpui::Entity<DatabasePanel>, cx: &mut gpui::VisualTestContext) {
        panel.update(cx, |panel, _| panel.export.min_page_rows = 1);
    }

    async fn names_in(fs: &std::sync::Arc<dyn fs::Fs>, folder: &str) -> Vec<String> {
        use futures::StreamExt as _;
        let mut entries = fs
            .read_dir(Path::new(folder))
            .await
            .expect("the folder reads");
        let mut names = Vec::new();
        while let Some(entry) = entries.next().await {
            let entry = entry.expect("an entry");
            names.push(
                entry
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
        }
        names.sort();
        names
    }

    #[gpui::test]
    async fn every_page_lands_in_one_csv_identical_to_the_whole_result(cx: &mut TestAppContext) {
        let (panel, calls, fs, cx) = exported_from(cx).await;
        one_row_pages(&panel, cx);
        let path = target("users.csv");
        start(&panel, &path, ExportFormat::Csv, ExportScope::AllRows, cx);
        cx.run_until_parked();

        let whole = the_three_rows();
        assert_eq!(
            fs.load(&path).await.expect("the file was written"),
            to_csv(&whole, &[0, 1, 2])
        );
        assert_eq!(
            status(&panel, cx),
            ExportStatus::Finished {
                path,
                rows: 3,
                capped: false,
                binary: 0
            }
        );
        // One page was on screen; the export asked for all three again, one
        // page at a time.
        assert_eq!(asked(&calls, "query"), 1 + 3);
    }

    /// Every page costs the server the rows before it, so a table read in the
    /// pager's pages of a couple of hundred takes time that grows with the
    /// square of its length.
    #[gpui::test]
    async fn an_export_reads_in_bigger_pages_than_the_pager_shows(cx: &mut TestAppContext) {
        let (panel, calls, fs, cx) = exported_from(cx).await;
        let path = target("users.csv");
        start(&panel, &path, ExportFormat::Csv, ExportScope::AllRows, cx);
        cx.run_until_parked();

        assert_eq!(
            fs.load(&path).await.expect("the file was written"),
            to_csv(&the_three_rows(), &[0, 1, 2])
        );
        assert_eq!(
            asked(&calls, "query"),
            1 + 1,
            "the pager's page of one row, then every row in one read"
        );
    }

    #[gpui::test]
    async fn every_page_lands_in_one_valid_json_array(cx: &mut TestAppContext) {
        let (panel, _calls, fs, cx) = exported_from(cx).await;
        one_row_pages(&panel, cx);
        let path = target("users.json");
        start(&panel, &path, ExportFormat::Json, ExportScope::AllRows, cx);
        cx.run_until_parked();

        let text = fs.load(&path).await.expect("the file was written");
        assert_eq!(text, to_json(&the_three_rows(), &[0, 1, 2]));
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(parsed.as_array().map(Vec::len), Some(3));
    }

    #[gpui::test]
    async fn the_current_page_is_written_without_asking_the_server_again(cx: &mut TestAppContext) {
        let (panel, calls, fs, cx) = exported_from(cx).await;
        let path = target("page.csv");
        start(
            &panel,
            &path,
            ExportFormat::Csv,
            ExportScope::CurrentPage,
            cx,
        );
        cx.run_until_parked();

        assert_eq!(
            fs.load(&path).await.expect("the file was written"),
            to_csv(&the_three_rows(), &[0])
        );
        assert_eq!(asked(&calls, "query"), 1, "nothing beyond the page run");
    }

    #[gpui::test]
    async fn the_row_limit_stops_an_export_and_says_so(cx: &mut TestAppContext) {
        let (panel, _calls, fs, cx) = exported_from(cx).await;
        one_row_pages(&panel, cx);
        cx.update(|_window, cx| {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |settings| {
                    settings.database.get_or_insert_default().export_row_limit = Some(2);
                });
            });
        });
        let path = target("capped.csv");
        start(&panel, &path, ExportFormat::Csv, ExportScope::AllRows, cx);
        cx.run_until_parked();

        assert_eq!(
            fs.load(&path).await.expect("the file was written"),
            to_csv(&the_three_rows(), &[0, 1])
        );
        assert_eq!(
            status(&panel, cx),
            ExportStatus::Finished {
                path,
                rows: 2,
                capped: true,
                binary: 0
            }
        );
    }

    #[gpui::test]
    async fn a_limit_equal_to_the_rows_is_not_a_cap(cx: &mut TestAppContext) {
        let (panel, _calls, _fs, cx) = exported_from(cx).await;
        one_row_pages(&panel, cx);
        cx.update(|_window, cx| {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |settings| {
                    settings.database.get_or_insert_default().export_row_limit = Some(3);
                });
            });
        });
        start(
            &panel,
            &target("exact.csv"),
            ExportFormat::Csv,
            ExportScope::AllRows,
            cx,
        );
        cx.run_until_parked();
        assert!(matches!(
            status(&panel, cx),
            ExportStatus::Finished {
                rows: 3,
                capped: false,
                ..
            }
        ));
    }

    /// Cancelling between pages leaves a file that is complete as far as it
    /// goes -- for JSON, a closed array -- and a state that says it stopped.
    #[gpui::test]
    async fn cancelling_after_the_first_page_keeps_that_page_in_a_valid_file(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, fs, cx) = exported_from(cx).await;
        one_row_pages(&panel, cx);
        let path = target("cancelled.json");
        start(&panel, &path, ExportFormat::Json, ExportScope::AllRows, cx);

        let mut ticks = 0;
        while !matches!(status(&panel, cx), ExportStatus::Running { rows, .. } if rows >= 1) {
            assert!(cx.executor().tick(), "the export stalled before page one");
            ticks += 1;
            assert!(ticks < 10_000, "the export never reached page one");
        }
        panel.update(cx, |panel, cx| panel.cancel_export(cx));
        cx.run_until_parked();

        let text = fs.load(&path).await.expect("the file was written");
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("still valid JSON");
        assert_eq!(parsed.as_array().map(Vec::len), Some(1), "page one only");
        assert_eq!(
            status(&panel, cx),
            ExportStatus::Cancelled {
                path,
                rows: 1,
                binary: 0
            }
        );
    }

    #[gpui::test]
    async fn a_closed_connection_fails_the_export_and_writes_nothing(cx: &mut TestAppContext) {
        let (panel, _calls, fs, cx) = exported_from(cx).await;
        panel.update(cx, |panel, _| {
            panel.connections[0].state = crate::database_panel::NodeState::Idle;
        });
        let path = target("never.csv");
        start(&panel, &path, ExportFormat::Csv, ExportScope::AllRows, cx);
        cx.run_until_parked();

        assert!(matches!(
            status(&panel, cx),
            ExportStatus::Failed { path: Some(_), .. }
        ));
        assert!(!fs.is_file(&path).await, "no file for a refused export");
    }

    #[gpui::test]
    async fn sql_is_refused_for_a_statement_with_no_table(cx: &mut TestAppContext) {
        let (panel, _calls, fs, cx) = exported_from(cx).await;
        let path = target("never.sql");
        start(&panel, &path, ExportFormat::Sql, ExportScope::AllRows, cx);
        cx.run_until_parked();

        assert!(matches!(status(&panel, cx), ExportStatus::Failed { .. }));
        assert!(!fs.is_file(&path).await);
    }

    #[gpui::test]
    async fn sql_for_an_opened_table_writes_inserts_into_it(cx: &mut TestAppContext) {
        let (panel, _calls, fs, cx) = exported_from(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.open_table_data(0, "public", "users", window, cx)
        });
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                match &panel.query {
                    crate::query::QueryState::Done(page) => page.browse.clone(),
                    _ => None,
                },
                Some(TableBrowse {
                    key_columns: vec!["id".into()],
                    columns: crate::database_panel_tests::users_columns(),
                    ..TableBrowse::new(
                        ("a".to_string(), crate::database_panel::NodeOrigin::Personal),
                        "public",
                        "users"
                    )
                })
            );
        });
        let path = target("users.sql");
        start(&panel, &path, ExportFormat::Sql, ExportScope::AllRows, cx);
        cx.run_until_parked();

        let text = fs.load(&path).await.expect("the file was written");
        assert_eq!(text.lines().count(), 3);
        assert!(
            text.lines().all(|line| line
                .starts_with("INSERT INTO \"public\".\"users\" (\"id\", \"email\") VALUES (")),
            "{text}"
        );
    }

    #[gpui::test]
    async fn a_second_export_cannot_start_while_one_runs(cx: &mut TestAppContext) {
        let (panel, _calls, fs, cx) = exported_from(cx).await;
        one_row_pages(&panel, cx);
        let (first, second) = (target("first.csv"), target("second.csv"));
        start(&panel, &first, ExportFormat::Csv, ExportScope::AllRows, cx);
        start(&panel, &second, ExportFormat::Csv, ExportScope::AllRows, cx);
        cx.run_until_parked();

        assert!(fs.is_file(&first).await);
        assert!(!fs.is_file(&second).await);
    }

    /// Closing the panel drops the export's task. What it had written is not a
    /// file anybody asked for, and the file it was to replace is untouched.
    #[gpui::test]
    async fn abandoning_an_export_in_flight_leaves_the_old_file_and_no_temp_file(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, fs, cx) = exported_from(cx).await;
        one_row_pages(&panel, cx);
        let path = target("abandoned.csv");
        fs.write(&path, b"old\r\n")
            .await
            .expect("an earlier export");
        start(&panel, &path, ExportFormat::Csv, ExportScope::AllRows, cx);

        let mut ticks = 0;
        while !matches!(status(&panel, cx), ExportStatus::Running { rows, .. } if rows >= 1) {
            assert!(cx.executor().tick(), "the export stalled before page one");
            ticks += 1;
            assert!(ticks < 10_000, "the export never reached page one");
        }
        panel.update(cx, |panel, _| panel.abandon_export());
        cx.run_until_parked();

        assert_eq!(fs.load(&path).await.expect("it reads"), "old\r\n");
        assert_eq!(names_in(&fs, "/exports").await, ["abandoned.csv"]);
    }

    #[gpui::test]
    async fn the_export_menu_opens_over_a_result(cx: &mut TestAppContext) {
        let (panel, _calls, _fs, cx) = exported_from(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.deploy_export_menu(gpui::point(gpui::px(10.), gpui::px(10.)), window, cx);
            assert!(panel.context_menu.is_some());
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    #[gpui::test]
    async fn the_row_limit_defaults_to_a_million(cx: &mut TestAppContext) {
        init_test(cx);
        cx.update(|cx| {
            assert_eq!(
                crate::DatabaseSettings::get_global(cx).export_row_limit,
                1_000_000
            );
        });
    }

    /// The file the dialog was told to replace is the user's until the new one
    /// is complete: a read that fails before the first page comes back has
    /// written nothing worth the old file's place.
    #[gpui::test]
    async fn a_failed_export_leaves_the_file_it_was_to_replace_alone(cx: &mut TestAppContext) {
        let (panel, _calls, fs, cx) = exported_from(cx).await;
        let path = target("report.csv");
        fs.write(&path, b"old,contents\r\n")
            .await
            .expect("an earlier export");
        open_on_fake_driver(&panel, 0, FakeBehaviour::Dies, Default::default(), cx);

        start(&panel, &path, ExportFormat::Csv, ExportScope::AllRows, cx);
        cx.run_until_parked();

        assert!(matches!(
            status(&panel, cx),
            ExportStatus::Failed { path: Some(_), .. }
        ));
        assert!(fs.is_file(&path).await, "the old file is still there");
        assert_eq!(
            fs.load(&path).await.expect("it reads"),
            "old,contents\r\n",
            "and it is the old one"
        );
        assert_eq!(
            names_in(&fs, "/exports").await,
            ["report.csv"],
            "with no half-written sibling"
        );
    }

    #[gpui::test]
    async fn a_failed_export_of_a_new_file_leaves_nothing_behind(cx: &mut TestAppContext) {
        let (panel, _calls, fs, cx) = exported_from(cx).await;
        open_on_fake_driver(&panel, 0, FakeBehaviour::Dies, Default::default(), cx);

        start(
            &panel,
            &target("fresh.csv"),
            ExportFormat::Csv,
            ExportScope::AllRows,
            cx,
        );
        cx.run_until_parked();

        assert!(matches!(status(&panel, cx), ExportStatus::Failed { .. }));
        assert!(names_in(&fs, "/exports").await.is_empty());
    }

    #[gpui::test]
    async fn a_finished_export_replaces_the_old_file_and_leaves_no_temp_file(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, fs, cx) = exported_from(cx).await;
        let path = target("report.csv");
        fs.write(&path, b"old,contents\r\n")
            .await
            .expect("an earlier export");

        start(&panel, &path, ExportFormat::Csv, ExportScope::AllRows, cx);
        cx.run_until_parked();

        assert_eq!(
            fs.load(&path).await.expect("the file was written"),
            to_csv(&the_three_rows(), &[0, 1, 2])
        );
        assert_eq!(names_in(&fs, "/exports").await, ["report.csv"]);
    }

    /// Cancelled with no row written, the export has nothing to put in the old
    /// file's place.
    #[gpui::test]
    async fn cancelling_before_any_row_keeps_the_old_file(cx: &mut TestAppContext) {
        let (panel, _calls, fs, cx) = exported_from(cx).await;
        let path = target("report.csv");
        fs.write(&path, b"old,contents\r\n")
            .await
            .expect("an earlier export");

        start(&panel, &path, ExportFormat::Csv, ExportScope::AllRows, cx);
        panel.update(cx, |panel, cx| panel.cancel_export(cx));
        cx.run_until_parked();

        assert_eq!(
            status(&panel, cx),
            ExportStatus::Cancelled {
                path: path.clone(),
                rows: 0,
                binary: 0
            }
        );
        assert_eq!(fs.load(&path).await.expect("it reads"), "old,contents\r\n");
        assert_eq!(names_in(&fs, "/exports").await, ["report.csv"]);
    }

    /// Stopping asks the server to stop as well, so that a page that takes
    /// minutes does not have to be waited out; whatever it then answers with is
    /// the stop, not a failure.
    #[gpui::test]
    async fn cancelling_interrupts_the_page_in_flight_and_is_not_a_failure(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, fs, cx) = exported_from(cx).await;
        let calls = open_on_fake_driver(&panel, 0, FakeBehaviour::Silent, Default::default(), cx);
        let path = target("slow.csv");

        start(&panel, &path, ExportFormat::Csv, ExportScope::AllRows, cx);
        cx.run_until_parked();
        assert_eq!(asked(&calls, "query"), 1, "page one is out, unanswered");
        assert_eq!(asked(&calls, "cancel"), 0);

        panel.update(cx, |panel, cx| panel.cancel_export(cx));
        cx.run_until_parked();
        assert_eq!(asked(&calls, "cancel"), 1, "the server was asked to stop");

        cx.executor()
            .advance_clock(database::client::DEFAULT_REQUEST_TIMEOUT * 2);
        cx.run_until_parked();
        assert!(
            matches!(status(&panel, cx), ExportStatus::Cancelled { rows: 0, .. }),
            "{:?}",
            status(&panel, cx)
        );
        assert!(names_in(&fs, "/exports").await.is_empty());
    }

    async fn after_a_write(
        cx: &mut TestAppContext,
    ) -> (
        gpui::Entity<DatabasePanel>,
        DriverCalls,
        std::sync::Arc<dyn fs::Fs>,
        &mut gpui::VisualTestContext,
    ) {
        let (panel, calls, fs, cx) = exported_from(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.run_page(
                "UPDATE users SET n = n + 1 RETURNING *".into(),
                0,
                2,
                None,
                window,
                cx,
            );
        });
        cx.run_until_parked();
        (panel, calls, fs, cx)
    }

    /// `UPDATE ... RETURNING` read page by page is the same update, run once
    /// per page.
    #[gpui::test]
    async fn every_row_of_a_write_is_not_exported_by_running_it_again(cx: &mut TestAppContext) {
        let (panel, calls, fs, cx) = after_a_write(cx).await;
        let before = asked(&calls, "query");
        let path = target("write.csv");

        start(&panel, &path, ExportFormat::Csv, ExportScope::AllRows, cx);
        cx.run_until_parked();

        assert_eq!(
            asked(&calls, "query"),
            before,
            "the write was not sent again"
        );
        assert!(matches!(status(&panel, cx), ExportStatus::Failed { .. }));
        assert!(names_in(&fs, "/exports").await.is_empty());
    }

    #[gpui::test]
    async fn the_rows_on_screen_after_a_write_can_still_be_saved(cx: &mut TestAppContext) {
        let (panel, calls, fs, cx) = after_a_write(cx).await;
        let before = asked(&calls, "query");
        let path = target("write.csv");

        start(
            &panel,
            &path,
            ExportFormat::Csv,
            ExportScope::CurrentPage,
            cx,
        );
        cx.run_until_parked();

        assert_eq!(asked(&calls, "query"), before);
        let text = fs.load(&path).await.expect("the file was written");
        assert_eq!(
            text.lines().count(),
            1 + 2,
            "the header and the page's two rows"
        );
    }

    /// The connection is found again when the export starts: the list may have
    /// been rebuilt since the page was read, so the place it had is another's.
    #[gpui::test]
    async fn the_export_follows_its_connection_when_the_list_moves(cx: &mut TestAppContext) {
        let (panel, _calls, _fs, cx) = exported_from(cx).await;
        panel.update(cx, |panel, _| {
            let key = panel.node_key(0).expect("the open connection");
            // The project names a database the same as the personal one, and
            // sits in front of it.
            let mut project =
                crate::database_panel::ConnectionNode::idle(panel.connections[0].config.clone());
            project.from_project = true;
            panel.connections.insert(0, project);
            panel.active = panel.index_of_key(&key);
        });
        let path = target("moved.csv");

        start(&panel, &path, ExportFormat::Csv, ExportScope::AllRows, cx);
        cx.run_until_parked();

        assert!(
            matches!(status(&panel, cx), ExportStatus::Finished { rows: 3, .. }),
            "{:?}",
            status(&panel, cx)
        );
    }
}

/// What the safety level of a connection does to a statement run from the
/// editor. Only what somebody typed is gated; see `safe_mode`.
mod write_gate {
    use super::*;
    use crate::panel_queries::RunScope;
    use crate::query::QueryState;
    use crate::query_history::{Outcome, QueryHistory};
    use crate::safe_mode::SafetyLevel;
    use editor::SelectionEffects;
    use multi_buffer::MultiBufferOffset;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A project connection, whose level is the local override this sets, under
    /// a name no other test shares: every test in the process reads and writes
    /// the one fallback key-value store.
    async fn typed_on_a_project_connection<'a>(
        cx: &'a mut TestAppContext,
        sql: &str,
        level: Option<SafetyLevel>,
    ) -> (
        gpui::Entity<DatabasePanel>,
        DriverCalls,
        String,
        &'a mut gpui::VisualTestContext,
    ) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;
        let name = format!("gate-test-{}", NEXT.fetch_add(1, Ordering::SeqCst));
        panel.update_in(cx, |panel, window, cx| {
            panel.connections[0].config.name = name.clone();
            panel.connections[0].from_project = true;
            if let Some(level) = level {
                panel.safety_overrides.insert(
                    crate::safe_mode::override_key(
                        &name,
                        crate::database_panel::NodeOrigin::Project,
                    ),
                    level,
                );
            }
            panel.set_active(0, window, cx);
            panel.sql_editor.update(cx, |editor, cx| {
                editor.set_text(sql.to_string(), window, cx);
                editor.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
                    selections.select_ranges([MultiBufferOffset(0)..MultiBufferOffset(0)])
                });
            });
        });
        (panel, calls, name, cx)
    }

    fn run(panel: &gpui::Entity<DatabasePanel>, cx: &mut gpui::VisualTestContext) {
        panel.update_in(cx, |panel, window, cx| {
            panel.run_query(RunScope::All, window, cx)
        });
        cx.run_until_parked();
    }

    fn history_of(name: &str, cx: &mut gpui::VisualTestContext) -> QueryHistory {
        let store = cx.update(|_, cx| db::kvp::KeyValueStore::global(cx));
        crate::query_history::read_stored(&store, name).expect("the store reads")
    }

    fn refused_for_read_only(
        panel: &gpui::Entity<DatabasePanel>,
        cx: &mut gpui::VisualTestContext,
    ) {
        panel.read_with(cx, |panel, _| match &panel.query {
            QueryState::Failed(error) => {
                assert!(error.read_only, "the grid says the connection is read-only");
                assert!(
                    error.message.contains("read-only") || error.message.contains("Read Only"),
                    "the message says why: {}",
                    error.message
                );
            }
            _ => panic!("the refusal belongs in the result area"),
        });
    }

    #[gpui::test]
    async fn read_only_refuses_a_write_and_the_driver_never_hears_of_it(cx: &mut TestAppContext) {
        let (panel, calls, name, cx) =
            typed_on_a_project_connection(cx, "DELETE FROM users", Some(SafetyLevel::ReadOnly))
                .await;
        run(&panel, cx);

        assert_eq!(asked(&calls, "query"), 0, "nothing reached the driver");
        assert!(!cx.has_pending_prompt(), "a refusal is not a question");
        refused_for_read_only(&panel, cx);

        let history = history_of(&name, cx).newest_first();
        assert_eq!(history.len(), 1, "the refusal is recorded");
        assert_eq!(history[0].sql, "DELETE FROM users");
        assert!(matches!(history[0].outcome, Outcome::Failed { .. }));
    }

    #[gpui::test]
    async fn read_only_hides_a_write_behind_a_comment_or_a_second_statement_no_better(
        cx: &mut TestAppContext,
    ) {
        for sql in [
            "-- harmless\nupdate users set email = null",
            "select 1; drop table users",
            "with x as (select 1) delete from users",
        ] {
            let (panel, calls, _name, cx) =
                typed_on_a_project_connection(cx, sql, Some(SafetyLevel::ReadOnly)).await;
            run(&panel, cx);
            assert_eq!(asked(&calls, "query"), 0, "`{sql}` must not get through");
            refused_for_read_only(&panel, cx);
        }
    }

    #[gpui::test]
    async fn read_only_still_runs_a_read(cx: &mut TestAppContext) {
        let (panel, calls, _name, cx) =
            typed_on_a_project_connection(cx, "select * from users", Some(SafetyLevel::ReadOnly))
                .await;
        run(&panel, cx);
        assert_eq!(asked(&calls, "query"), 1);
        panel.read_with(cx, |panel, _| {
            assert!(matches!(panel.query, QueryState::Done(_)))
        });
    }

    #[gpui::test]
    async fn the_default_asks_before_a_write_and_runs_it_on_run(cx: &mut TestAppContext) {
        let (panel, calls, name, cx) =
            typed_on_a_project_connection(cx, "DELETE FROM users", None).await;
        run(&panel, cx);

        let (title, detail) = cx.pending_prompt().expect("a write asks first");
        assert_eq!(
            title,
            format!("This statement changes data on {name}. Run?")
        );
        assert!(detail.contains("DELETE FROM users"), "the prompt shows it");
        assert_eq!(asked(&calls, "query"), 0, "not before the answer");

        cx.simulate_prompt_answer("Run");
        cx.run_until_parked();
        assert_eq!(asked(&calls, "query"), 1);
        assert_eq!(history_of(&name, cx).len(), 1, "an answered run is history");
    }

    #[gpui::test]
    async fn cancelling_the_prompt_sends_nothing(cx: &mut TestAppContext) {
        let (panel, calls, name, cx) =
            typed_on_a_project_connection(cx, "UPDATE users SET email = NULL", None).await;
        run(&panel, cx);
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();

        assert_eq!(asked(&calls, "query"), 0);
        panel.read_with(cx, |panel, _| {
            assert!(matches!(panel.query, QueryState::Idle), "nothing happened");
        });
        assert_eq!(history_of(&name, cx).len(), 0, "a declined run did not run");
    }

    #[gpui::test]
    async fn a_read_runs_without_asking_by_default(cx: &mut TestAppContext) {
        let (panel, calls, _name, cx) =
            typed_on_a_project_connection(cx, "select * from users for update", None).await;
        run(&panel, cx);
        assert!(!cx.has_pending_prompt());
        assert_eq!(asked(&calls, "query"), 1);
        drop(panel);
    }

    #[gpui::test]
    async fn unrestricted_runs_a_write_without_asking(cx: &mut TestAppContext) {
        let (panel, calls, _name, cx) =
            typed_on_a_project_connection(cx, "DELETE FROM users", Some(SafetyLevel::Unrestricted))
                .await;
        run(&panel, cx);
        assert!(!cx.has_pending_prompt());
        assert_eq!(asked(&calls, "query"), 1);
        drop(panel);
    }

    /// The prompt stays up while the user does other things, one of which is
    /// tightening the connection: the answer has to be read against the level
    /// of now, not the level when it was asked.
    #[gpui::test]
    async fn a_level_tightened_while_the_prompt_is_up_wins_over_run(cx: &mut TestAppContext) {
        let (panel, calls, name, cx) =
            typed_on_a_project_connection(cx, "DELETE FROM users", None).await;
        run(&panel, cx);
        assert!(cx.has_pending_prompt());

        panel.update(cx, |panel, cx| {
            panel.set_safety(0, SafetyLevel::ReadOnly, cx)
        });
        cx.simulate_prompt_answer("Run");
        cx.run_until_parked();

        assert_eq!(asked(&calls, "query"), 0);
        refused_for_read_only(&panel, cx);
        assert_eq!(history_of(&name, cx).len(), 1);
    }

    /// Answering "Run" after moving to another connection must not run the
    /// statement there: it was confirmed for the one it was typed against.
    #[gpui::test]
    async fn a_run_confirmed_for_one_connection_is_not_sent_to_another(cx: &mut TestAppContext) {
        let (panel, calls, _name, cx) =
            typed_on_a_project_connection(cx, "DELETE FROM users", None).await;
        run(&panel, cx);
        assert!(cx.has_pending_prompt());

        panel.update(cx, |panel, _| panel.active = None);
        cx.simulate_prompt_answer("Run");
        cx.run_until_parked();
        assert_eq!(asked(&calls, "query"), 0);
    }

    #[gpui::test]
    async fn a_personal_connection_takes_its_level_from_settings(cx: &mut TestAppContext) {
        let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;
        panel.read_with(cx, |panel, cx| {
            assert_eq!(panel.safety_of(0, cx), SafetyLevel::ConfirmWrites);
        });
        cx.update(|_window, cx| {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |settings| {
                    for connection in settings
                        .database
                        .get_or_insert_default()
                        .connections
                        .get_or_insert_default()
                    {
                        connection.safety = Some(settings::DatabaseSafety::ReadOnly);
                    }
                });
            });
        });
        cx.run_until_parked();
        panel.read_with(cx, |panel, cx| {
            assert_eq!(panel.safety_of(0, cx), SafetyLevel::ReadOnly);
        });

        panel.update_in(cx, |panel, window, cx| {
            panel.set_active(0, window, cx);
            panel.sql_editor.update(cx, |editor, cx| {
                editor.set_text("DROP TABLE users".to_string(), window, cx)
            });
            panel.run_query(RunScope::All, window, cx);
        });
        cx.run_until_parked();
        assert_eq!(asked(&calls, "query"), 0);
        refused_for_read_only(&panel, cx);
    }

    /// A badge on every row would be a badge on none: only the two levels that
    /// differ from what a connection does unconfigured say anything.
    #[gpui::test]
    async fn only_a_read_only_or_unrestricted_connection_wears_a_badge(cx: &mut TestAppContext) {
        let (_workspace, panel, cx) = workspace_with_panel(cx).await;
        set_connections(
            cx,
            &[
                ("locked", "/tmp/locked.sqlite"),
                ("loose", "/tmp/loose.sqlite"),
                ("plain", "/tmp/plain.sqlite"),
            ],
        );
        cx.update(|_window, cx| {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |settings| {
                    let connections = settings
                        .database
                        .get_or_insert_default()
                        .connections
                        .get_or_insert_default();
                    connections[0].safety = Some(settings::DatabaseSafety::ReadOnly);
                    connections[1].safety = Some(settings::DatabaseSafety::Unrestricted);
                });
            });
        });
        draw_again(cx);

        assert!(cx.debug_bounds("database-safety-badge-0").is_some());
        assert!(cx.debug_bounds("database-safety-badge-1").is_some());
        assert!(cx.debug_bounds("database-safety-badge-2").is_none());
        panel.read_with(cx, |panel, cx| {
            assert_eq!(panel.safety_of(2, cx), SafetyLevel::ConfirmWrites);
        });
    }

    /// The menu's choice for a personal connection lands in the settings file,
    /// next to the connection it belongs to and no other.
    #[gpui::test]
    async fn a_personal_connections_level_is_written_beside_it_in_settings(
        cx: &mut TestAppContext,
    ) {
        let (_workspace, panel, cx) = workspace_with_panel(cx).await;
        set_connections(
            cx,
            &[("one", "/tmp/one.sqlite"), ("two", "/tmp/two.sqlite")],
        );
        let fs = panel.read_with(cx, |panel, cx| {
            panel
                .workspace
                .read_with(cx, |workspace, cx| {
                    workspace.project().read(cx).fs().clone()
                })
                .expect("the workspace is alive")
        });
        fs.create_dir(paths::config_dir())
            .await
            .expect("the settings folder");
        fs.save(
            paths::settings_file(),
            &r#"{ "database": { "connections": [
                { "name": "one", "driver": "sqlite", "url": "/tmp/one.sqlite" },
                { "name": "two", "driver": "sqlite", "url": "/tmp/two.sqlite" }
            ] } }"#
                .into(),
            Default::default(),
        )
        .await
        .expect("a settings file to edit");

        panel.update(cx, |panel, cx| {
            panel.set_safety(1, SafetyLevel::ReadOnly, cx)
        });
        cx.run_until_parked();

        let written = fs
            .load(paths::settings_file())
            .await
            .expect("the settings file");
        let written: serde_json::Value =
            serde_json::from_str(&written).expect("the file is still json");
        let connections = &written["database"]["connections"];
        assert_eq!(connections[1]["safety"], "read_only");
        assert!(
            connections[0].get("safety").is_none(),
            "the other is untouched"
        );
    }

    /// A project connection has nowhere in settings to keep a level, so it is
    /// kept on this machine -- and is still there for the next panel.
    #[gpui::test]
    async fn a_project_connections_level_is_kept_locally_and_read_back(cx: &mut TestAppContext) {
        let (panel, _calls, name, cx) = typed_on_a_project_connection(cx, "select 1", None).await;
        panel.update(cx, |panel, cx| {
            panel.set_safety(0, SafetyLevel::ReadOnly, cx)
        });
        cx.run_until_parked();

        let read_back = cx.update(|_window, cx| crate::safe_mode::read_overrides(cx));
        assert_eq!(
            read_back.get(&crate::safe_mode::override_key(
                &name,
                crate::database_panel::NodeOrigin::Project
            )),
            Some(&SafetyLevel::ReadOnly)
        );
        panel.read_with(cx, |panel, cx| {
            assert_eq!(panel.safety_of(0, cx), SafetyLevel::ReadOnly);
        });
    }

    fn remembered(sql: &str) -> crate::query_history::HistoryEntry {
        crate::query_history::HistoryEntry {
            sql: sql.to_string(),
            started_at_ms: crate::query_history::now_ms(),
            elapsed_ms: 1,
            outcome: Outcome::Rows { count: 1 },
        }
    }

    /// What a connection's history holds when the list is opened: stored, and
    /// read back, so that the list is what a real session would show.
    fn remember(
        panel: &gpui::Entity<DatabasePanel>,
        connection: &str,
        sql: &str,
        cx: &mut gpui::VisualTestContext,
    ) {
        panel.update(cx, |panel, cx| {
            panel.record_history(connection.to_string(), remembered(sql), cx)
        });
        cx.run_until_parked();
    }

    fn open_history_of_the_active_connection(
        panel: &gpui::Entity<DatabasePanel>,
        cx: &mut gpui::VisualTestContext,
    ) {
        panel.update_in(cx, |panel, window, cx| panel.show_query_history(window, cx));
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            assert!(panel.history_popover.is_some(), "the list is open");
        });
    }

    /// The secondary key on the first row, which is the newest entry and runs
    /// it.
    fn run_the_newest_entry(cx: &mut gpui::VisualTestContext) {
        cx.dispatch_action(menu::SecondaryConfirm);
        cx.run_until_parked();
    }

    fn run_from_history(panel: &gpui::Entity<DatabasePanel>, cx: &mut gpui::VisualTestContext) {
        open_history_of_the_active_connection(panel, cx);
        run_the_newest_entry(cx);
    }

    fn failed_with(
        panel: &gpui::Entity<DatabasePanel>,
        cx: &mut gpui::VisualTestContext,
    ) -> String {
        panel.read_with(cx, |panel, _| match &panel.query {
            QueryState::Failed(error) => error.message.to_string(),
            _ => panic!("the refusal belongs in the result area"),
        })
    }

    /// The statement a read-only connection refused is in its history, and
    /// running it from there is running it: the secondary key must not be a way
    /// round the level.
    #[gpui::test]
    async fn read_only_refuses_a_write_run_from_the_history(cx: &mut TestAppContext) {
        let (panel, calls, name, cx) =
            typed_on_a_project_connection(cx, "DELETE FROM users", Some(SafetyLevel::ReadOnly))
                .await;
        run(&panel, cx);
        assert_eq!(history_of(&name, cx).len(), 1, "the refusal is recorded");
        panel.update(cx, |panel, _| panel.query = QueryState::Idle);

        run_from_history(&panel, cx);

        assert_eq!(asked(&calls, "query"), 0, "the driver never hears of it");
        assert!(!cx.has_pending_prompt(), "a refusal is not a question");
        refused_for_read_only(&panel, cx);
    }

    #[gpui::test]
    async fn confirm_writes_asks_before_a_history_run_of_a_write_and_cancel_sends_nothing(
        cx: &mut TestAppContext,
    ) {
        let (panel, calls, name, cx) = typed_on_a_project_connection(cx, "select 1", None).await;
        remember(&panel, &name, "DELETE FROM users", cx);

        run_from_history(&panel, cx);

        let (title, detail) = cx.pending_prompt().expect("a write asks first");
        assert_eq!(
            title,
            format!("This statement changes data on {name}. Run?")
        );
        assert!(detail.contains("DELETE FROM users"), "the prompt shows it");
        assert_eq!(asked(&calls, "query"), 0, "not before the answer");

        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(asked(&calls, "query"), 0);
        panel.read_with(cx, |panel, _| {
            assert!(matches!(panel.query, QueryState::Idle), "nothing happened");
        });
        assert_eq!(history_of(&name, cx).len(), 1, "a declined run did not run");
    }

    #[gpui::test]
    async fn answering_run_sends_the_history_entry_once(cx: &mut TestAppContext) {
        let (panel, calls, name, cx) = typed_on_a_project_connection(cx, "select 1", None).await;
        remember(&panel, &name, "DELETE FROM users", cx);

        run_from_history(&panel, cx);
        cx.simulate_prompt_answer("Run");
        cx.run_until_parked();

        assert_eq!(asked(&calls, "query"), 1);
        assert_eq!(history_of(&name, cx).len(), 2, "the run is history too");
    }

    #[gpui::test]
    async fn unrestricted_runs_a_write_from_the_history_without_asking(cx: &mut TestAppContext) {
        let (panel, calls, name, cx) =
            typed_on_a_project_connection(cx, "select 1", Some(SafetyLevel::Unrestricted)).await;
        remember(&panel, &name, "DELETE FROM users", cx);

        run_from_history(&panel, cx);

        assert!(!cx.has_pending_prompt());
        assert_eq!(asked(&calls, "query"), 1);
    }

    #[gpui::test]
    async fn a_read_runs_from_the_history_without_asking_at_any_level(cx: &mut TestAppContext) {
        for level in [
            None,
            Some(SafetyLevel::ReadOnly),
            Some(SafetyLevel::Unrestricted),
        ] {
            let (panel, calls, name, cx) =
                typed_on_a_project_connection(cx, "select 1", level).await;
            remember(&panel, &name, "select * from users", cx);

            run_from_history(&panel, cx);

            assert!(!cx.has_pending_prompt(), "{level:?}");
            assert_eq!(asked(&calls, "query"), 1, "{level:?}");
            panel.read_with(cx, |panel, _| {
                assert!(matches!(panel.query, QueryState::Done(_)), "{level:?}");
            });
        }
    }

    /// Two connections, named for this test alone, both from the project so that
    /// each can be given its own level.
    async fn two_project_connections(
        cx: &mut TestAppContext,
        first_level: SafetyLevel,
        second_level: SafetyLevel,
    ) -> (
        gpui::Entity<DatabasePanel>,
        Vec<DriverCalls>,
        [String; 2],
        &mut gpui::VisualTestContext,
    ) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, Ordering::SeqCst);
        let names = [format!("gate-pair-{id}-a"), format!("gate-pair-{id}-b")];
        let (panel, drivers, cx) =
            panel_with_connections(cx, &[names[0].as_str(), names[1].as_str()]).await;
        panel.update_in(cx, |panel, window, cx| {
            for (index, level) in [first_level, second_level].into_iter().enumerate() {
                panel.connections[index].from_project = true;
                panel.safety_overrides.insert(
                    crate::safe_mode::override_key(
                        &names[index],
                        crate::database_panel::NodeOrigin::Project,
                    ),
                    level,
                );
            }
            panel.set_active(0, window, cx);
        });
        (panel, drivers, names, cx)
    }

    /// The list is the first connection's; by the time a row is chosen the
    /// second is the active one. The statement belongs to the first.
    #[gpui::test]
    async fn an_entry_runs_on_the_connection_it_was_run_on_when_another_is_active(
        cx: &mut TestAppContext,
    ) {
        let (panel, drivers, names, cx) =
            two_project_connections(cx, SafetyLevel::ConfirmWrites, SafetyLevel::ConfirmWrites)
                .await;
        remember(&panel, &names[0], "select * from users", cx);
        open_history_of_the_active_connection(&panel, cx);
        panel.update_in(cx, |panel, window, cx| panel.set_active(1, window, cx));

        run_the_newest_entry(cx);

        assert_eq!(
            asked(&drivers[0], "query"),
            1,
            "the first connection ran it"
        );
        assert_eq!(
            asked(&drivers[1], "query"),
            0,
            "the active one was not asked"
        );
        panel.read_with(cx, |panel, _| {
            assert_eq!(panel.active, Some(0), "the grid is the first connection's");
        });
    }

    /// The level that counts is the one on the connection the statement is
    /// sent to: an unrestricted connection that happens to be active is not a
    /// way to run what a read-only one refuses.
    #[gpui::test]
    async fn the_level_of_the_connection_that_ran_an_entry_decides_not_the_active_ones(
        cx: &mut TestAppContext,
    ) {
        let (panel, drivers, names, cx) =
            two_project_connections(cx, SafetyLevel::ReadOnly, SafetyLevel::Unrestricted).await;
        remember(&panel, &names[0], "DELETE FROM users", cx);
        open_history_of_the_active_connection(&panel, cx);
        panel.update_in(cx, |panel, window, cx| panel.set_active(1, window, cx));

        run_the_newest_entry(cx);

        assert_eq!(asked(&drivers[0], "query"), 0);
        assert_eq!(asked(&drivers[1], "query"), 0, "nor was the other one");
        assert!(!cx.has_pending_prompt());
        refused_for_read_only(&panel, cx);
    }

    #[gpui::test]
    async fn an_entry_of_a_connection_that_left_the_list_is_not_run_anywhere(
        cx: &mut TestAppContext,
    ) {
        let (panel, drivers, names, cx) =
            two_project_connections(cx, SafetyLevel::ConfirmWrites, SafetyLevel::Unrestricted)
                .await;
        remember(&panel, &names[0], "select * from users", cx);
        open_history_of_the_active_connection(&panel, cx);
        panel.update_in(cx, |panel, window, cx| {
            panel.set_active(1, window, cx);
            panel.connections.remove(0);
            panel.active = Some(0);
        });

        run_the_newest_entry(cx);

        assert_eq!(asked(&drivers[0], "query"), 0);
        assert_eq!(
            asked(&drivers[1], "query"),
            0,
            "the one now in its place was not asked"
        );
        assert!(failed_with(&panel, cx).contains("no longer"));
    }

    #[gpui::test]
    async fn an_entry_of_a_connection_that_is_not_connected_is_not_run(cx: &mut TestAppContext) {
        let (panel, drivers, names, cx) =
            two_project_connections(cx, SafetyLevel::ConfirmWrites, SafetyLevel::Unrestricted)
                .await;
        remember(&panel, &names[0], "select * from users", cx);
        open_history_of_the_active_connection(&panel, cx);
        panel.update_in(cx, |panel, window, cx| {
            panel.set_active(1, window, cx);
            panel.connections[0].state = crate::database_panel::NodeState::Idle;
        });

        run_the_newest_entry(cx);

        assert_eq!(asked(&drivers[0], "query"), 0);
        assert_eq!(asked(&drivers[1], "query"), 0);
        assert!(failed_with(&panel, cx).contains("not connected"));
        panel.read_with(cx, |panel, _| {
            assert_eq!(panel.active, Some(1), "nothing was switched to");
        });
    }
}

/// Open Data learns the table's key before it reads a page: the key orders the
/// pages and names the rows.
mod open_data_key {
    use super::*;
    use crate::query::QueryState;
    use crate::row_identity::NoRowIdentity;

    fn page_sql(panel: &DatabasePanel) -> String {
        match &panel.query {
            QueryState::Done(page) => page.sql.clone(),
            _ => panic!("a page was expected"),
        }
    }

    #[gpui::test]
    async fn open_data_describes_the_table_once_and_orders_by_its_key(cx: &mut TestAppContext) {
        let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;
        assert_eq!(asked(&calls, "describe_table"), 0);
        panel.update_in(cx, |panel, window, cx| {
            panel.open_table_data(0, "public", "users", window, cx)
        });
        cx.run_until_parked();

        assert_eq!(asked(&calls, "describe_table"), 1);
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                page_sql(panel),
                "SELECT *\nFROM \"public\".\"users\"\nORDER BY \"id\" ASC"
            );
            let QueryState::Done(page) = &panel.query else {
                panic!("a page was expected");
            };
            assert_eq!(
                page.browse
                    .as_ref()
                    .map(|browse| browse.key_columns.clone()),
                Some(vec!["id".to_string()])
            );
        });

        // A page turn does not describe it again: the key travels with the page.
        panel.update_in(cx, |panel, window, cx| panel.page(true, window, cx));
        cx.run_until_parked();
        assert_eq!(asked(&calls, "describe_table"), 1);
        panel.read_with(cx, |panel, _| {
            assert!(page_sql(panel).ends_with("ORDER BY \"id\" ASC"));
        });
    }

    #[gpui::test]
    async fn open_data_reuses_the_columns_the_tree_is_already_showing(cx: &mut TestAppContext) {
        let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;
        panel.update(cx, |panel, cx| panel.toggle_table(0, "public", "users", cx));
        cx.run_until_parked();
        assert_eq!(asked(&calls, "describe_table"), 1);

        panel.update_in(cx, |panel, window, cx| {
            panel.open_table_data(0, "public", "users", window, cx)
        });
        cx.run_until_parked();
        assert_eq!(asked(&calls, "describe_table"), 1, "nothing left to ask");
        panel.read_with(cx, |panel, _| {
            assert!(page_sql(panel).ends_with("ORDER BY \"id\" ASC"));
        });
    }

    /// The first table's description is still out when the second is opened:
    /// only the second may land a page.
    #[gpui::test]
    async fn the_table_opened_last_is_the_page_that_lands(cx: &mut TestAppContext) {
        let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.open_table_data(0, "public", "users", window, cx);
            panel.open_table_data(0, "public", "active_users", window, cx);
        });
        cx.run_until_parked();

        assert_eq!(asked(&calls, "query"), 1, "the first open was dropped");
        panel.read_with(cx, |panel, _| {
            assert!(page_sql(panel).contains("\"active_users\""));
        });
    }

    #[gpui::test]
    async fn a_row_of_an_opened_table_has_an_identity_and_a_typed_result_does_not(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.open_table_data(0, "public", "users", window, cx)
        });
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            let QueryState::Done(page) = &panel.query else {
                panic!("a page was expected");
            };
            let identity = page.row_identity(0).expect("the table has a key");
            assert_eq!(identity.columns, vec!["id".to_string()]);
            assert_eq!(identity.values.len(), 1);
            assert_eq!(page.row_identity(999), Err(NoRowIdentity::NoSuchRow));
        });

        panel.update_in(cx, |panel, window, cx| {
            panel.run_page("select * from users".into(), 0, 10, None, window, cx)
        });
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            let QueryState::Done(page) = &panel.query else {
                panic!("a page was expected");
            };
            assert_eq!(page.row_identity(0), Err(NoRowIdentity::NotATable));
        });
    }
}

/// Puts an in-process driver behind connection `index` as its open session.
fn open_on_fake_driver(
    panel: &gpui::Entity<DatabasePanel>,
    index: usize,
    behaviour: database::fake_driver::FakeBehaviour,
    capabilities: database::protocol::Capabilities,
    cx: &mut gpui::VisualTestContext,
) -> DriverCalls {
    use crate::database_panel::NodeState;
    use database::client::{DEFAULT_REQUEST_TIMEOUT, DriverClient};

    let driver = std::sync::Arc::new(database::fake_driver::FakeDriver::new(behaviour));
    let calls = driver.clone();
    let client =
        cx.update(|_window, cx| DriverClient::new(driver, DEFAULT_REQUEST_TIMEOUT, &cx.to_async()));
    panel.update(cx, |panel, _| {
        let session = crate::session::Session::for_test(client, capabilities);
        panel.connections[index].state = NodeState::Connected(std::sync::Arc::new(session));
    });
    calls
}

/// The connections the settings name, one url per name so that a node keeps
/// its session across a reload that only adds or removes others.
fn list_connections(cx: &mut gpui::VisualTestContext, names: &[&str]) {
    let urls: Vec<String> = names
        .iter()
        .map(|name| format!("/tmp/{name}.sqlite"))
        .collect();
    let entries: Vec<(&str, &str)> = names
        .iter()
        .zip(&urls)
        .map(|(name, url)| (*name, url.as_str()))
        .collect();
    set_connections(cx, &entries);
}

/// A panel whose connections `names` are all open, each on a driver of its
/// own, with their schemas and the tables of the open schema read.
async fn panel_with_connections<'a>(
    cx: &'a mut TestAppContext,
    names: &[&str],
) -> (
    gpui::Entity<DatabasePanel>,
    Vec<DriverCalls>,
    &'a mut gpui::VisualTestContext,
) {
    use database::fake_driver::FakeBehaviour;

    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    list_connections(cx, names);
    let mut drivers = Vec::new();
    for index in 0..names.len() {
        drivers.push(open_on_fake_driver(
            &panel,
            index,
            FakeBehaviour::Well,
            Default::default(),
            cx,
        ));
        panel.update(cx, |panel, cx| {
            panel.connections[index].expanded = true;
            panel.load_schemas(index, cx);
        });
    }
    cx.run_until_parked();
    (panel, drivers, cx)
}

/// Everything that held a connection by its place in the list, after the list
/// moved under it. A removed connection shifts every one behind it, and a
/// statement sent to the connection now standing at an old place is sent to a
/// database nobody chose.
mod stale_connection_places {
    use super::*;
    use crate::query::QueryState;
    use crate::tree_menus::Generate;
    use database::protocol::Capabilities;

    fn position() -> gpui::Point<gpui::Pixels> {
        gpui::point(gpui::px(10.), gpui::px(10.))
    }

    #[gpui::test]
    async fn sorting_after_the_list_shifted_runs_on_the_connection_that_was_opened(
        cx: &mut TestAppContext,
    ) {
        let (panel, drivers, cx) = panel_with_connections(cx, &["a", "b", "c"]).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.open_table_data(1, "public", "users", window, cx)
        });
        cx.run_until_parked();
        assert_eq!(asked(&drivers[1], "query"), 1, "b was read");

        // `a` goes away: `b` is now first and `c` stands where `b` was.
        list_connections(cx, &["b", "c"]);
        panel.update_in(cx, |panel, window, cx| panel.sort_by("id", window, cx));
        cx.run_until_parked();

        assert_eq!(asked(&drivers[1], "query"), 2, "b ran the sorted read");
        assert_eq!(
            asked(&drivers[2], "query"),
            0,
            "c, at b's old place, was never asked"
        );
    }

    /// Open Data waits for the table to be described. A list that shifts in
    /// the meantime must not turn the page into a read of another database.
    #[gpui::test]
    async fn a_table_opened_while_the_list_shifts_is_read_from_its_own_connection(
        cx: &mut TestAppContext,
    ) {
        let (panel, drivers, cx) = panel_with_connections(cx, &["a", "b", "c"]).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.open_table_data(1, "public", "users", window, cx)
        });
        list_connections(cx, &["b", "c"]);
        cx.run_until_parked();

        assert_eq!(asked(&drivers[1], "query"), 1, "b was read");
        assert_eq!(asked(&drivers[2], "query"), 0, "c was not");
    }

    #[gpui::test]
    async fn a_tables_answer_lands_on_its_connection_after_the_list_shifts(
        cx: &mut TestAppContext,
    ) {
        let (panel, _drivers, cx) = panel_with_connections(cx, &["a", "b", "c"]).await;
        panel.update(cx, |panel, cx| {
            panel.connections[1].schemas[0].tables = None;
            panel.load_tables(1, 0, cx);
        });
        list_connections(cx, &["b", "c"]);

        panel.read_with(cx, |panel, _| {
            assert_eq!(panel.connection_names(), ["b", "c"]);
            let schema = &panel.connections[0].schemas[0];
            assert!(schema.tables.is_some(), "b got the tables it asked for");
            assert_eq!(schema.tables_load, 0, "and is no longer waiting");
        });
    }

    #[gpui::test]
    async fn a_schema_list_answer_lands_on_its_connection_after_the_list_shifts(
        cx: &mut TestAppContext,
    ) {
        let (panel, _drivers, cx) = panel_with_connections(cx, &["a", "b", "c"]).await;
        panel.update(cx, |panel, cx| {
            panel.connections[1].schemas.clear();
            panel.load_schemas(1, cx);
        });
        list_connections(cx, &["b", "c"]);

        panel.read_with(cx, |panel, _| {
            assert_eq!(panel.connections[0].schemas.len(), 1, "b got its schemas");
            assert_eq!(panel.connections[0].schemas_load, 0);
        });
    }

    #[gpui::test]
    async fn a_menu_open_while_the_list_reloads_is_put_away(cx: &mut TestAppContext) {
        let (panel, _drivers, cx) = panel_with_connections(cx, &["a", "b", "c"]).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.deploy_connection_menu(1, position(), window, cx);
            assert!(panel.context_menu.is_some());
        });

        list_connections(cx, &["b", "c"]);

        panel.read_with(cx, |panel, _| {
            assert!(
                panel.context_menu.is_none(),
                "an entry of it would act on whichever connection now has its place"
            );
        });
    }

    /// Pinning reloads the list from inside the menu's own entry, while the menu
    /// is being updated: putting the menu away must not update it again.
    #[gpui::test]
    async fn pinning_from_the_menu_reloads_the_list_and_closes_the_menu(cx: &mut TestAppContext) {
        let (panel, _drivers, cx) = panel_with_connections(cx, &["a", "b"]).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.deploy_connection_menu(1, position(), window, cx)
        });
        // Refresh, New Query, then Pin.
        for _ in 0..3 {
            cx.dispatch_action(menu::SelectNext);
        }
        cx.dispatch_action(menu::Confirm);
        cx.run_until_parked();

        panel.read_with(cx, |panel, _| {
            assert!(panel.is_pinned("b"), "the entry ran");
            assert!(panel.context_menu.is_none(), "and the menu is gone");
        });
    }

    #[gpui::test]
    async fn a_statement_generated_across_a_reload_goes_to_the_connection_it_was_asked_on(
        cx: &mut TestAppContext,
    ) {
        let (panel, _drivers, cx) = panel_with_connections(cx, &["a", "b", "c"]).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.generate_statement(1, "public", "users", Generate::Delete, window, cx)
        });
        list_connections(cx, &["b", "c"]);

        panel.read_with(cx, |panel, cx| {
            assert_eq!(panel.active, Some(0), "b, not the one now at b's old place");
            assert!(panel.sql_editor.read(cx).text(cx).contains("DELETE"));
        });
    }

    #[gpui::test]
    async fn a_statement_generated_for_a_connection_that_went_away_is_dropped(
        cx: &mut TestAppContext,
    ) {
        let (panel, _drivers, cx) = panel_with_connections(cx, &["a", "b", "c"]).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.generate_statement(1, "public", "users", Generate::Delete, window, cx)
        });
        list_connections(cx, &["a", "c"]);

        panel.read_with(cx, |panel, cx| {
            assert_eq!(panel.active, None, "nothing was pointed anywhere");
            assert_eq!(panel.sql_editor.read(cx).text(cx), "");
        });
    }

    /// `Copy as INSERT` quotes for the engine the rows came from, not for
    /// whichever connection was clicked since.
    #[gpui::test]
    async fn copy_as_insert_uses_the_dialect_of_the_connection_that_ran_the_page(
        cx: &mut TestAppContext,
    ) {
        use crate::row_serializers::CopyFormat;

        let (panel, _drivers, cx) = panel_with_connections(cx, &["a", "b"]).await;
        panel.update(cx, |panel, _| {
            panel.connections[0].config.driver = "postgres".into();
            panel.connections[1].config.driver = "mysql".into();
        });
        open_on_fake_driver(
            &panel,
            1,
            database::fake_driver::FakeBehaviour::Well,
            Capabilities {
                identifier_quote: Some("`".into()),
                ..Default::default()
            },
            cx,
        );
        panel.update_in(cx, |panel, window, cx| {
            panel.set_active(0, window, cx);
            panel.run_page("select * from users".into(), 0, 10, None, window, cx);
        });
        cx.run_until_parked();

        panel.update_in(cx, |panel, window, cx| {
            panel.set_active(1, window, cx);
            panel.select_all_rows(cx);
            panel.copy_rows_as(CopyFormat::Insert, cx);
        });

        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default();
        assert!(
            copied.contains("(\"id\", \"email\")"),
            "quoted for the connection that ran it: {copied}"
        );
    }

    #[gpui::test]
    async fn the_page_on_screen_follows_its_connection_through_a_reload(cx: &mut TestAppContext) {
        let (panel, drivers, cx) = panel_with_connections(cx, &["a", "b", "c"]).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.open_table_data(1, "public", "users", window, cx)
        });
        cx.run_until_parked();
        list_connections(cx, &["b", "c"]);

        panel.update_in(cx, |panel, window, cx| panel.page(true, window, cx));
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            assert!(matches!(panel.query, QueryState::Done(_)));
        });
        assert_eq!(asked(&drivers[2], "query"), 0, "c was not asked for a page");
    }
}

/// The page on screen came from one connection, and turning it reads that
/// connection again whichever one has been clicked since.
mod a_page_turn_stays_on_its_connection {
    use super::*;
    use crate::database_panel::NodeState;
    use crate::query::QueryState;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A first page of a read, two of its three rows, on the first of two
    /// connections.
    async fn first_page_on_the_first_of_two(
        cx: &mut TestAppContext,
    ) -> (
        gpui::Entity<DatabasePanel>,
        Vec<DriverCalls>,
        [String; 2],
        &mut gpui::VisualTestContext,
    ) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, Ordering::SeqCst);
        let names = [format!("paging-pair-{id}-a"), format!("paging-pair-{id}-b")];
        let (panel, drivers, cx) =
            panel_with_connections(cx, &[names[0].as_str(), names[1].as_str()]).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.set_active(0, window, cx);
            panel.run_page("select * from users".into(), 0, 2, None, window, cx);
        });
        cx.run_until_parked();
        assert_eq!(asked(&drivers[0], "query"), 1);
        (panel, drivers, names, cx)
    }

    fn next_page(panel: &gpui::Entity<DatabasePanel>, cx: &mut gpui::VisualTestContext) {
        panel.update_in(cx, |panel, window, cx| panel.page(true, window, cx));
        cx.run_until_parked();
    }

    #[gpui::test]
    async fn the_next_page_is_read_from_the_connection_the_page_came_from(cx: &mut TestAppContext) {
        let (panel, drivers, names, cx) = first_page_on_the_first_of_two(cx).await;
        panel.update_in(cx, |panel, window, cx| panel.set_active(1, window, cx));

        next_page(&panel, cx);

        assert_eq!(
            asked(&drivers[0], "query"),
            2,
            "the first one gave the page"
        );
        assert_eq!(
            asked(&drivers[1], "query"),
            0,
            "the clicked one was not asked"
        );
        panel.read_with(cx, |panel, _| {
            let QueryState::Done(page) = &panel.query else {
                panic!("the second page was expected");
            };
            assert_eq!(page.offset, 2);
            assert_eq!(
                page.connection,
                Some((
                    names[0].clone(),
                    crate::database_panel::NodeOrigin::Personal
                ))
            );
            assert_eq!(panel.active, Some(0), "the grid is the first connection's");
        });
    }

    #[gpui::test]
    async fn a_page_of_a_connection_that_left_the_list_is_not_turned_on_another(
        cx: &mut TestAppContext,
    ) {
        let (panel, drivers, names, cx) = first_page_on_the_first_of_two(cx).await;
        panel.update_in(cx, |panel, window, cx| panel.set_active(1, window, cx));
        list_connections(cx, &[names[1].as_str()]);

        next_page(&panel, cx);

        assert_eq!(asked(&drivers[0], "query"), 1);
        assert_eq!(
            asked(&drivers[1], "query"),
            0,
            "the one that took its place"
        );
        panel.read_with(cx, |panel, _| match &panel.query {
            QueryState::Failed(error) => assert!(error.message.contains("no longer")),
            _ => panic!("the page should say it could not be turned"),
        });
    }

    #[gpui::test]
    async fn a_page_of_a_connection_that_closed_is_not_turned_on_another(cx: &mut TestAppContext) {
        let (panel, drivers, _names, cx) = first_page_on_the_first_of_two(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.set_active(1, window, cx);
            panel.connections[0].state = NodeState::Idle;
        });

        next_page(&panel, cx);

        assert_eq!(asked(&drivers[0], "query"), 1);
        assert_eq!(asked(&drivers[1], "query"), 0);
        panel.read_with(cx, |panel, _| match &panel.query {
            QueryState::Failed(error) => assert!(error.message.contains("not connected")),
            _ => panic!("the page should say it could not be turned"),
        });
    }
}

/// A page of a statement that changes data is shown once: turning it would
/// send the statement again, and it was confirmed once.
mod paging_a_write {
    use super::*;
    use crate::query::QueryState;

    async fn ran_with_two_rows_a_page<'a>(
        cx: &'a mut TestAppContext,
        sql: &str,
    ) -> (
        gpui::Entity<DatabasePanel>,
        DriverCalls,
        &'a mut gpui::VisualTestContext,
    ) {
        let (panel, calls, cx) = panel_open_on_a_fake_driver(cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.set_active(0, window, cx);
            panel.run_page(sql.into(), 0, 2, None, window, cx);
        });
        cx.run_until_parked();
        (panel, calls, cx)
    }

    #[gpui::test]
    async fn turning_the_page_of_a_write_does_not_send_it_again(cx: &mut TestAppContext) {
        let (panel, calls, cx) =
            ran_with_two_rows_a_page(cx, "UPDATE users SET n = n + 1 RETURNING *").await;
        assert_eq!(asked(&calls, "query"), 1);
        panel.read_with(cx, |panel, _| {
            let QueryState::Done(page) = &panel.query else {
                panic!("a page was expected");
            };
            assert!(page.result.truncated, "the fake has a third row");
            assert!(!page.has_next(), "the pager does not offer it");
            assert!(!page.has_previous());
        });

        panel.update_in(cx, |panel, window, cx| panel.page(true, window, cx));
        cx.run_until_parked();

        assert_eq!(asked(&calls, "query"), 1, "the write was not sent again");
        panel.read_with(cx, |panel, _| {
            let QueryState::Done(page) = &panel.query else {
                panic!("the page is still showing");
            };
            assert_eq!(page.offset, 0);
        });
    }

    #[gpui::test]
    async fn a_write_hidden_behind_a_read_is_not_paged_either(cx: &mut TestAppContext) {
        let (panel, calls, cx) = ran_with_two_rows_a_page(
            cx,
            "WITH gone AS (DELETE FROM users RETURNING *) SELECT * FROM gone",
        )
        .await;

        panel.update_in(cx, |panel, window, cx| panel.page(true, window, cx));
        cx.run_until_parked();
        assert_eq!(asked(&calls, "query"), 1);
    }

    #[gpui::test]
    async fn a_read_still_turns_its_pages(cx: &mut TestAppContext) {
        let (panel, calls, cx) = ran_with_two_rows_a_page(cx, "SELECT * FROM users").await;
        panel.read_with(cx, |panel, _| {
            let QueryState::Done(page) = &panel.query else {
                panic!("a page was expected");
            };
            assert!(page.has_next());
        });

        panel.update_in(cx, |panel, window, cx| panel.page(true, window, cx));
        cx.run_until_parked();

        assert_eq!(asked(&calls, "query"), 2);
        panel.read_with(cx, |panel, _| {
            let QueryState::Done(page) = &panel.query else {
                panic!("the second page was expected");
            };
            assert_eq!(page.offset, 2);
            assert!(page.has_previous());
        });
    }
}

/// A refresh is asked for with a tree on screen, and a refresh that fails must
/// not take the tree away -- or the connection it hangs from.
mod refresh_failures {
    use super::*;
    use database::fake_driver::FakeBehaviour;

    #[gpui::test]
    async fn a_failed_schema_refresh_keeps_the_session_and_the_rows(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        open_on_fake_driver(&panel, 0, FakeBehaviour::Dies, Default::default(), cx);

        panel.update(cx, |panel, cx| panel.refresh_connection(0, cx));
        cx.run_until_parked();

        panel.read_with(cx, |panel, _| {
            assert!(panel.is_connected(0), "the session is still open");
            assert_eq!(panel.connections[0].schemas.len(), 1);
            assert_eq!(table_names(panel), ["users", "active_users"]);
        });
    }

    #[gpui::test]
    async fn a_failed_tables_refresh_keeps_the_session_and_the_rows(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        open_on_fake_driver(&panel, 0, FakeBehaviour::Dies, Default::default(), cx);

        panel.update(cx, |panel, cx| panel.refresh_schema(0, "public", cx));
        cx.run_until_parked();

        panel.read_with(cx, |panel, _| {
            assert!(panel.is_connected(0));
            assert_eq!(table_names(panel), ["users", "active_users"]);
        });
    }

    /// Not silent: the rows on screen are no longer what the database says,
    /// and the one who asked to refresh has to be told that.
    #[gpui::test]
    async fn a_failed_refresh_says_so_in_the_tree_and_the_next_one_clears_it(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        let notes = |panel: &DatabasePanel| -> Vec<String> {
            panel
                .rows()
                .into_iter()
                .filter_map(|row| match row {
                    crate::connection_tree::Row::Note { text, .. } => Some(text.to_string()),
                    _ => None,
                })
                .collect()
        };
        panel.read_with(cx, |panel, _| assert!(notes(panel).is_empty()));

        open_on_fake_driver(&panel, 0, FakeBehaviour::Dies, Default::default(), cx);
        panel.update(cx, |panel, cx| panel.refresh_connection(0, cx));
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            let notes = notes(panel);
            assert_eq!(notes.len(), 1, "{notes:?}");
            assert!(notes[0].starts_with("Could not refresh"), "{notes:?}");
        });

        open_on_fake_driver(&panel, 0, FakeBehaviour::Well, Default::default(), cx);
        panel.update(cx, |panel, cx| panel.refresh_connection(0, cx));
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| assert!(notes(panel).is_empty()));
    }

    /// The first read of a tree has nothing to keep, so a failure there is
    /// still the connection's failure.
    #[gpui::test]
    async fn a_failed_first_schema_read_still_fails_the_connection(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        open_on_fake_driver(&panel, 0, FakeBehaviour::Dies, Default::default(), cx);
        panel.update(cx, |panel, cx| {
            panel.connections[0].schemas.clear();
            panel.load_schemas(0, cx);
        });
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| assert!(panel.failure(0).is_some()));
    }
}

/// Open Data, and how many times a table is described to open it.
mod describing_for_open_data {
    use super::*;
    use crate::query::QueryState;
    use database::fake_driver::FakeBehaviour;

    #[gpui::test]
    async fn a_table_that_cannot_be_described_is_still_read_unkeyed(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        let dying = open_on_fake_driver(&panel, 0, FakeBehaviour::Dies, Default::default(), cx);

        panel.update_in(cx, |panel, window, cx| {
            panel.open_table_data(0, "public", "users", window, cx)
        });
        cx.run_until_parked();

        assert_eq!(asked(&dying, "describe_table"), 1);
        assert_eq!(
            asked(&dying, "query"),
            1,
            "the table is read even though its key could not be learned"
        );
        panel.read_with(cx, |panel, _| {
            assert!(
                panel.is_connected(0),
                "a refused describe is not a dead session"
            );
            assert!(
                !matches!(panel.query, QueryState::Idle),
                "the grid heard about the read"
            );
        });
    }

    /// A click on a table opens its columns under it and its rows in the grid:
    /// both want the same description.
    #[gpui::test]
    async fn clicking_a_table_describes_it_once_for_the_tree_and_the_grid(cx: &mut TestAppContext) {
        let (panel, cx) = drawn_rows(vec![("tree-row-table", table_row(2))], cx).await;
        let calls = open_on_fake_driver(&panel, 0, FakeBehaviour::Well, Default::default(), cx);
        panel.update(cx, |panel, _| {
            panel.connections[0].schemas[0].tables = Some(Vec::new());
        });
        let centre = bounds_of(cx, "tree-row-table").center();

        cx.simulate_click(centre, gpui::Modifiers::none());
        draw_again(cx);

        assert_eq!(asked(&calls, "describe_table"), 1, "one description");
        assert_eq!(asked(&calls, "query"), 1, "and the rows");
        panel.read_with(cx, |panel, _| {
            let open = panel.open_table.as_ref().expect("the columns are showing");
            assert_eq!(open.columns.as_ref().map(Vec::len), Some(2));
            assert!(matches!(panel.query, QueryState::Done(_)));
        });
    }
}

/// What the panel keeps for as long as it lives must not grow with every query.
mod task_growth {
    use super::*;

    #[gpui::test]
    async fn finished_tasks_are_not_kept(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        for _ in 0..20 {
            panel.update_in(cx, |panel, window, cx| {
                panel.set_active(0, window, cx);
                panel.run_page("select * from users".into(), 0, 10, None, window, cx);
            });
            cx.run_until_parked();
        }
        panel.read_with(cx, |panel, _| {
            assert!(
                panel.tasks.len() < 5,
                "{} tasks kept after 20 finished queries",
                panel.tasks.len()
            );
        });
    }

    #[gpui::test]
    async fn reading_the_projects_data_sources_again_does_not_keep_every_task(
        cx: &mut TestAppContext,
    ) {
        let (_workspace, panel, cx) = workspace_with_panel(cx).await;
        for _ in 0..20 {
            panel.update_in(cx, |panel, window, cx| {
                panel.load_project_connections(window, cx)
            });
            cx.run_until_parked();
        }
        panel.read_with(cx, |panel, _| {
            assert!(
                panel.tasks.len() < 5,
                "{} tasks kept after 20 finished reads",
                panel.tasks.len()
            );
        });
    }

    #[gpui::test]
    async fn keeping_a_shared_password_does_not_keep_every_task(cx: &mut TestAppContext) {
        let (_workspace, panel, cx) = workspace_with_panel(cx).await;
        cx.update(|_window, cx| {
            cx.set_global(zed_credentials_provider::ZedCredentialsProvider(
                std::sync::Arc::new(MemoryCredentials::default()),
            ));
        });
        panel.update(cx, |panel, _cx| {
            push_team_node(panel, 0xa1, "staging", true, Some("hunter2"));
        });
        for _ in 0..20 {
            panel.update(cx, |panel, cx| {
                panel.remember_shared_password(0, zeroize::Zeroizing::new("hunter2".into()), cx)
            });
            cx.run_until_parked();
        }
        panel.read_with(cx, |panel, _| {
            assert!(
                panel.tasks.len() < 5,
                "{} tasks kept after 20 finished writes",
                panel.tasks.len()
            );
        });
    }
}

/// The arrow keys belong to the grid, not to the panel around it: a single-line
/// editor in the panel hands an arrow on when it has nothing to do with it.
mod grid_keys {
    use super::*;
    use crate::query::{Page, QueryState};
    use database::protocol::{Cell, ResultColumn, ResultSet};

    fn show_three_rows(panel: &gpui::Entity<DatabasePanel>, cx: &mut gpui::VisualTestContext) {
        panel.update(cx, |panel, cx| {
            let number = |value: &str| Cell::Number {
                value: value.into(),
            };
            panel.query = QueryState::Done(Page {
                result: ResultSet {
                    columns: vec![ResultColumn {
                        name: "id".into(),
                        type_name: "int".into(),
                    }],
                    rows: vec![vec![number("1")], vec![number("2")], vec![number("3")]],
                    truncated: false,
                    elapsed_ms: 1,
                },
                sql: "select * from notes".into(),
                offset: 0,
                limit: 200,
                selected_row: None,
                selection: Default::default(),
                browse: None,
                connection: None,
                kind: crate::safe_mode::StatementKind::Read,
                origin: Default::default(),
                edits: Default::default(),
            });
            cx.notify();
        });
        draw_again(cx);
    }

    fn selected(
        panel: &gpui::Entity<DatabasePanel>,
        cx: &mut gpui::VisualTestContext,
    ) -> Vec<usize> {
        panel.read_with(cx, |panel, _| match &panel.query {
            QueryState::Done(page) => page.selection.rows(),
            _ => Vec::new(),
        })
    }

    #[gpui::test]
    async fn the_arrows_move_the_cursor_while_the_grid_has_focus_and_not_in_the_filter_box(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, cx) = panel_open_on_a_fake_driver(cx).await;
        cx.update(|_window, cx| {
            cx.bind_keys([
                gpui::KeyBinding::new("down", menu::SelectNext, Some("DatabaseResults")),
                gpui::KeyBinding::new("up", menu::SelectPrevious, Some("DatabaseResults")),
            ]);
        });
        show_three_rows(&panel, cx);

        let bounds = bounds_of(cx, "database-result-row-0");
        cx.simulate_click(
            gpui::point(bounds.left() + gpui::px(8.), bounds.center().y),
            gpui::Modifiers::none(),
        );
        draw_again(cx);
        assert_eq!(selected(&panel, cx), [0]);
        cx.simulate_keystrokes("down");
        assert_eq!(selected(&panel, cx), [1], "the grid answers the arrow");

        cx.update(|window, cx| {
            use gpui::Focusable as _;
            let focus = panel.read(cx).tree_filter_input.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("down");
        assert_eq!(
            selected(&panel, cx),
            [1],
            "typing in the filter does not move the grid"
        );
    }
}

/// Which keys the panel answers to, read from the bundled keymaps.
mod bundled_keymaps {
    const MACOS: &str = include_str!("../../../assets/keymaps/default-macos.json");
    const LINUX: &str = include_str!("../../../assets/keymaps/default-linux.json");
    const WINDOWS: &str = include_str!("../../../assets/keymaps/default-windows.json");

    /// The lines of the block whose `"context"` is exactly `context`.
    fn block<'a>(keymap: &'a str, context: &str) -> Vec<&'a str> {
        let marker = format!("\"context\": \"{context}\",");
        keymap
            .lines()
            .skip_while(|line| line.trim() != marker)
            .skip(1)
            .take_while(|line| line.trim() != "},")
            .collect()
    }

    fn database_actions(keymap: &str, context: &str) -> Vec<String> {
        let mut actions: Vec<String> = block(keymap, context)
            .into_iter()
            .filter_map(|line| line.split("\"database::").nth(1))
            .filter_map(|rest| rest.split('"').next())
            .map(str::to_string)
            .collect();
        actions.sort();
        actions
    }

    #[test]
    fn every_platform_binds_the_same_database_actions() {
        let expected = database_actions(MACOS, "DatabasePanel");
        assert!(
            expected.contains(&"CancelQuery".to_string()),
            "the macOS block was not found: {expected:?}"
        );
        assert_eq!(database_actions(LINUX, "DatabasePanel"), expected, "linux");
        assert_eq!(
            database_actions(WINDOWS, "DatabasePanel"),
            expected,
            "windows"
        );
    }

    #[test]
    fn refreshing_the_database_has_a_key_on_every_platform() {
        for (platform, keymap) in [("macos", MACOS), ("linux", LINUX), ("windows", WINDOWS)] {
            assert!(
                database_actions(keymap, "DatabasePanel").contains(&"RefreshDatabase".to_string()),
                "{platform} leaves RefreshDatabase unbound"
            );
        }
    }

    /// Bound in `DatabasePanel`, an arrow reaches the grid from a text box in
    /// the panel that hands it on.
    #[test]
    fn the_arrows_are_bound_for_the_grid_only() {
        for (platform, keymap) in [("macos", MACOS), ("linux", LINUX), ("windows", WINDOWS)] {
            let panel = block(keymap, "DatabasePanel").join("\n");
            assert!(
                !panel.contains("menu::SelectNext") && !panel.contains("menu::SelectPrevious"),
                "{platform} binds the arrows for the whole panel"
            );
            let grid = block(keymap, "DatabaseResults").join("\n");
            assert!(
                grid.contains("\"down\": \"menu::SelectNext\"")
                    && grid.contains("\"up\": \"menu::SelectPrevious\""),
                "{platform} does not bind them for the grid"
            );
        }
    }
}

/// Editing a table's cells in the grid: what may be edited, what a double-click
/// does, and what saving sends to the driver and does with its answer.
mod grid_editing_ui {
    use super::*;
    use crate::grid_editing::CellAddress;
    use crate::query::{Page, QueryState};
    use crate::safe_mode::SafetyLevel;
    use database::fake_driver::FakeBehaviour;
    use database::protocol::{Capabilities, ColumnValue, EditValue, RowChange};
    use std::sync::atomic::{AtomicUsize, Ordering};

    type Cx = gpui::VisualTestContext;
    type Panel = gpui::Entity<DatabasePanel>;

    /// `users` opened with Open Data on a project connection, whose level is the
    /// local override `level` sets, under a name no other test shares.
    async fn users_table(
        cx: &mut TestAppContext,
        behaviour: FakeBehaviour,
        level: Option<SafetyLevel>,
    ) -> (Panel, DriverCalls, &mut Cx) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let (_workspace, panel, cx) = workspace_with_panel(cx).await;
        list_connections(cx, &["a"]);
        let capabilities = Capabilities {
            row_updates: behaviour != FakeBehaviour::NoRowUpdates,
            ..Default::default()
        };
        let calls = open_on_fake_driver(&panel, 0, behaviour, capabilities, cx);
        panel.update(cx, |panel, cx| {
            panel.connections[0].expanded = true;
            panel.load_schemas(0, cx);
        });
        cx.run_until_parked();
        let name = format!("edit-test-{}", NEXT.fetch_add(1, Ordering::SeqCst));
        panel.update_in(cx, |panel, window, cx| {
            panel.connections[0].config.name = name.clone();
            panel.connections[0].from_project = true;
            if let Some(level) = level {
                panel.safety_overrides.insert(
                    crate::safe_mode::override_key(
                        &name,
                        crate::database_panel::NodeOrigin::Project,
                    ),
                    level,
                );
            }
            panel.open_table_data(0, "public", "users", window, cx);
        });
        draw_again(cx);
        (panel, calls, cx)
    }

    fn with_page<R>(panel: &Panel, cx: &mut Cx, read: impl FnOnce(&Page) -> R) -> R {
        panel.read_with(cx, |panel, _| match &panel.query {
            QueryState::Done(page) => read(page),
            _ => panic!("a page was expected"),
        })
    }

    fn staged(panel: &Panel, cx: &mut Cx) -> Vec<(usize, usize, Option<String>, Option<String>)> {
        with_page(panel, cx, |page| {
            page.edits
                .staged
                .iter()
                .map(|(address, cell)| {
                    (
                        address.row,
                        address.column,
                        cell.value.clone(),
                        cell.error.clone(),
                    )
                })
                .collect()
        })
    }

    fn notice(panel: &Panel, cx: &mut Cx) -> String {
        with_page(panel, cx, |page| {
            page.edits
                .notice
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default()
        })
    }

    fn failure(panel: &Panel, cx: &mut Cx) -> String {
        with_page(panel, cx, |page| {
            page.edits
                .failure
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default()
        })
    }

    fn editing(panel: &Panel, cx: &mut Cx) -> Option<(usize, usize, String)> {
        panel.read_with(cx, |panel, cx| match &panel.query {
            QueryState::Done(page) => page.edits.editor.as_ref().map(|editor| {
                (
                    editor.address.row,
                    editor.address.column,
                    editor.input.read(cx).text(cx),
                )
            }),
            _ => None,
        })
    }

    fn open_editor(panel: &Panel, row: usize, column: usize, cx: &mut Cx) -> bool {
        let opened = panel.update_in(cx, |panel, window, cx| {
            panel.begin_edit(row, column, window, cx)
        });
        draw_again(cx);
        opened
    }

    fn type_into_editor(panel: &Panel, text: &str, cx: &mut Cx) {
        panel.update_in(cx, |panel, window, cx| {
            let QueryState::Done(page) = &panel.query else {
                return;
            };
            let Some(editor) = page.edits.editor.as_ref() else {
                return;
            };
            let editor = editor.input.read(cx).editor().clone();
            editor.set_text(text, window, cx);
        });
    }

    fn confirm(cx: &mut Cx) {
        cx.dispatch_action(menu::Confirm);
        draw_again(cx);
    }

    /// Stages `text` into the email cell of `row`, the way Enter does.
    fn edit_email(panel: &Panel, row: usize, text: &str, cx: &mut Cx) {
        assert!(open_editor(panel, row, 1, cx), "the cell opens");
        type_into_editor(panel, text, cx);
        confirm(cx);
    }

    fn column_is(panel: &Panel, name: &str, type_name: &str, nullable: bool, cx: &mut Cx) {
        panel.update(cx, |panel, _| {
            let QueryState::Done(page) = &mut panel.query else {
                panic!("a page was expected");
            };
            let browse = page.browse.as_mut().expect("a table");
            let column = browse
                .columns
                .iter_mut()
                .find(|column| column.name == name)
                .expect("the column is described");
            column.type_name = type_name.into();
            column.nullable = nullable;
        });
    }

    fn save(panel: &Panel, cx: &mut Cx) {
        panel.update_in(cx, |panel, window, cx| panel.request_save(window, cx));
        cx.run_until_parked();
    }

    fn update_calls(calls: &DriverCalls) -> usize {
        calls.updates.lock().len()
    }

    fn click_on(cx: &mut Cx, selector: &'static str) {
        let at = bounds_of(cx, selector).center();
        cx.simulate_click(at, gpui::Modifiers::none());
        draw_again(cx);
    }

    fn double_click(cx: &mut Cx, selector: &'static str) {
        let at = bounds_of(cx, selector).center();
        cx.simulate_click(at, gpui::Modifiers::none());
        cx.simulate_event(gpui::MouseDownEvent {
            button: gpui::MouseButton::Left,
            position: at,
            modifiers: gpui::Modifiers::none(),
            click_count: 2,
            first_mouse: false,
        });
        cx.simulate_event(gpui::MouseUpEvent {
            button: gpui::MouseButton::Left,
            position: at,
            modifiers: gpui::Modifiers::none(),
            click_count: 2,
        });
        draw_again(cx);
    }

    fn email_change(id: &str, value: EditValue) -> RowChange {
        RowChange {
            key: vec![ColumnValue {
                column: "id".into(),
                value: EditValue::Text { value: id.into() },
            }],
            set: vec![ColumnValue {
                column: "email".into(),
                value,
            }],
        }
    }

    fn text_value(value: &str) -> EditValue {
        EditValue::Text {
            value: value.into(),
        }
    }

    #[gpui::test]
    async fn double_clicking_a_cell_opens_an_editor_holding_its_value(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        double_click(cx, "database-cell-0-1");

        assert_eq!(
            editing(&panel, cx),
            Some((0, 1, "user0@example.com".to_string()))
        );
        bounds_of(cx, "database-cell-0-1");
    }

    /// The first click of a double-click still selects the row and opens its
    /// detail, as a click always did; the second must not close it again.
    #[gpui::test]
    async fn a_double_click_that_edits_leaves_the_row_selected_and_shown(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        double_click(cx, "database-cell-0-1");
        assert_eq!(with_page(&panel, cx, |page| page.selected_row), Some(0));
    }

    #[gpui::test]
    async fn double_clicking_a_key_cell_says_why_it_cannot_be_edited(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        double_click(cx, "database-cell-0-0");

        assert_eq!(editing(&panel, cx), None);
        assert!(
            notice(&panel, cx).contains("part of the primary key"),
            "{}",
            notice(&panel, cx)
        );
        assert_eq!(
            with_page(&panel, cx, |page| page.selected_row),
            None,
            "where nothing opens, the second click is the click it always was"
        );
    }

    #[gpui::test]
    async fn a_statement_somebody_typed_is_not_editable(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.run_page("select * from users".into(), 0, 10, None, window, cx)
        });
        draw_again(cx);

        assert!(!open_editor(&panel, 0, 1, cx));
        assert!(notice(&panel, cx).contains("Open Data"));
    }

    #[gpui::test]
    async fn a_driver_without_row_updates_cannot_edit(cx: &mut TestAppContext) {
        let (panel, calls, cx) = users_table(cx, FakeBehaviour::NoRowUpdates, None).await;
        assert!(!open_editor(&panel, 0, 1, cx));
        assert!(notice(&panel, cx).contains("cannot update rows"));
        assert_eq!(update_calls(&calls), 0);
    }

    #[gpui::test]
    async fn a_read_only_connection_cannot_edit(cx: &mut TestAppContext) {
        let (panel, _calls, cx) =
            users_table(cx, FakeBehaviour::Well, Some(SafetyLevel::ReadOnly)).await;
        double_click(cx, "database-cell-0-1");
        assert_eq!(editing(&panel, cx), None);
        assert!(notice(&panel, cx).contains("Read Only"));
    }

    #[gpui::test]
    async fn a_table_without_a_key_cannot_be_edited(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        panel.update(cx, |panel, _| {
            if let QueryState::Done(page) = &mut panel.query {
                page.browse.as_mut().expect("a table").key_columns.clear();
            }
        });
        assert!(!open_editor(&panel, 0, 1, cx));
        assert!(notice(&panel, cx).contains("no primary key"));
    }

    #[gpui::test]
    async fn a_table_whose_columns_are_not_known_cannot_be_edited(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        panel.update(cx, |panel, _| {
            if let QueryState::Done(page) = &mut panel.query {
                page.browse.as_mut().expect("a table").columns.clear();
            }
        });
        assert!(!open_editor(&panel, 0, 1, cx));
        assert!(notice(&panel, cx).contains("column types"));
    }

    #[gpui::test]
    async fn enter_stages_the_value_and_the_bar_appears(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        assert!(cx.debug_bounds("database-edit-bar").is_none());

        edit_email(&panel, 0, "new@example.com", cx);

        assert_eq!(editing(&panel, cx), None, "the box is closed");
        assert_eq!(
            staged(&panel, cx),
            vec![(0, 1, Some("new@example.com".to_string()), None)]
        );
        bounds_of(cx, "database-edit-bar");
        bounds_of(cx, "database-edit-save");
        bounds_of(cx, "database-edit-discard");
        bounds_of(cx, "database-edit-preview");
    }

    #[gpui::test]
    async fn escape_throws_away_what_was_typed(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        assert!(open_editor(&panel, 0, 1, cx));
        type_into_editor(&panel, "typed", cx);
        cx.dispatch_action(menu::Cancel);
        draw_again(cx);

        assert_eq!(editing(&panel, cx), None);
        assert_eq!(staged(&panel, cx), vec![]);
        assert!(
            !cx.has_pending_prompt(),
            "a box thrown away is not a question"
        );
    }

    /// Typing nothing is not an edit: a cell the user did not change must not
    /// turn up in the save, nor be held to its column's type.
    #[gpui::test]
    async fn an_unchanged_value_is_no_edit_and_is_never_validated(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        column_is(&panel, "email", "integer", true, cx);

        assert!(open_editor(&panel, 0, 1, cx));
        confirm(cx);

        assert_eq!(staged(&panel, cx), vec![]);
        assert!(cx.debug_bounds("database-edit-bar").is_none());
    }

    /// Changing it back is not an edit either.
    #[gpui::test]
    async fn changing_a_value_back_removes_the_edit(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "other", cx);
        assert_eq!(staged(&panel, cx).len(), 1);
        edit_email(&panel, 0, "user0@example.com", cx);
        assert_eq!(staged(&panel, cx), vec![]);
    }

    /// A NULL cell opened and confirmed untouched stays NULL: the empty box is
    /// not the empty string.
    #[gpui::test]
    async fn confirming_an_untouched_null_cell_keeps_it_null(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        assert!(open_editor(&panel, 1, 1, cx));
        assert_eq!(editing(&panel, cx), Some((1, 1, String::new())));
        confirm(cx);
        assert_eq!(staged(&panel, cx), vec![]);
    }

    #[gpui::test]
    async fn tab_stages_and_moves_to_the_next_cell_that_can_be_edited(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        assert!(open_editor(&panel, 0, 1, cx));
        type_into_editor(&panel, "first", cx);

        cx.dispatch_action(zed_actions::database::EditNextCell);
        draw_again(cx);

        assert_eq!(
            staged(&panel, cx),
            vec![(0, 1, Some("first".to_string()), None)]
        );
        // The key column of row 1 is skipped: it is the email that opens.
        assert_eq!(editing(&panel, cx), Some((1, 1, String::new())));

        cx.dispatch_action(zed_actions::database::EditPreviousCell);
        draw_again(cx);
        assert_eq!(editing(&panel, cx), Some((0, 1, "first".to_string())));
    }

    #[gpui::test]
    async fn clicking_inside_the_editor_keeps_it_open(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        assert!(open_editor(&panel, 0, 1, cx));
        let before = with_page(&panel, cx, |page| page.selected_row);

        // On the box's own border, which the text inside does not cover: the
        // press belongs to the box and not to the row behind it.
        let bounds = bounds_of(cx, "database-cell-0-1");
        cx.simulate_click(
            gpui::point(bounds.left() + gpui::px(1.), bounds.center().y),
            gpui::Modifiers::none(),
        );
        draw_again(cx);

        assert!(editing(&panel, cx).is_some(), "the box is still there");
        assert_eq!(with_page(&panel, cx, |page| page.selected_row), before);
    }

    /// Clicking away is leaving: what was typed is kept, as a spreadsheet does.
    #[gpui::test]
    async fn moving_the_focus_away_stages_what_was_typed(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        assert!(open_editor(&panel, 0, 1, cx));
        type_into_editor(&panel, "by blur", cx);

        panel.update_in(cx, |panel, window, cx| {
            window.focus(&panel.results_focus, cx);
        });
        draw_again(cx);

        assert_eq!(editing(&panel, cx), None);
        assert_eq!(
            staged(&panel, cx),
            vec![(0, 1, Some("by blur".to_string()), None)]
        );
    }

    /// Each column holds the value to its type, and says so on the cell.
    #[gpui::test]
    async fn a_value_the_column_cannot_hold_is_marked_and_blocks_saving(cx: &mut TestAppContext) {
        let (panel, calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        column_is(&panel, "email", "integer", true, cx);

        edit_email(&panel, 0, "abc", cx);

        let marked = staged(&panel, cx);
        assert_eq!(marked.len(), 1);
        let error = marked[0].3.clone().expect("the cell says what is wrong");
        assert!(!error.is_empty());
        let bar = failure(&panel, cx);
        assert_eq!(bar, "");
        // The bar names the cell, so the sentence is read without hovering.
        panel.read_with(cx, |panel, _| {
            let QueryState::Done(page) = &panel.query else {
                panic!("a page");
            };
            let problem = page.first_problem().expect("a problem to say");
            assert!(problem.starts_with("row 1, email: "), "{problem}");
        });

        save(&panel, cx);
        assert!(!cx.has_pending_prompt(), "nothing to confirm");
        assert_eq!(update_calls(&calls), 0, "an invalid value is never sent");
        assert!(
            failure(&panel, cx).contains("invalid"),
            "{}",
            failure(&panel, cx)
        );
    }

    #[gpui::test]
    async fn the_error_follows_the_type_of_each_column(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        let mut messages = Vec::new();
        for (type_name, input) in [
            ("integer", "1.5"),
            ("real", "abc"),
            ("integer", "99999999999999999999"),
        ] {
            column_is(&panel, "email", type_name, true, cx);
            edit_email(&panel, 0, input, cx);
            messages.push(
                staged(&panel, cx)[0]
                    .3
                    .clone()
                    .unwrap_or_else(|| panic!("`{input}` is not a {type_name}")),
            );
        }
        messages.dedup();
        assert_eq!(
            messages.len(),
            3,
            "a message per kind of mistake: {messages:?}"
        );
    }

    #[gpui::test]
    async fn a_null_into_a_column_that_refuses_it_is_marked(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        column_is(&panel, "email", "text", false, cx);
        panel.update(cx, |panel, cx| panel.set_cell_null(0, 1, cx));
        assert_eq!(
            staged(&panel, cx),
            vec![(0, 1, None, Some("cannot be NULL".to_string()))]
        );
    }

    #[gpui::test]
    async fn a_value_typed_to_fit_replaces_the_mark(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        column_is(&panel, "email", "integer", true, cx);
        edit_email(&panel, 0, "abc", cx);
        edit_email(&panel, 0, "12", cx);
        assert_eq!(
            staged(&panel, cx),
            vec![(0, 1, Some("12".to_string()), None)]
        );
    }

    #[gpui::test]
    async fn the_default_level_shows_the_update_and_saves_only_on_save(cx: &mut TestAppContext) {
        let (panel, calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        let queries = asked(&calls, "query");
        edit_email(&panel, 0, "new@example.com", cx);

        save(&panel, cx);

        let (title, detail) = cx.pending_prompt().expect("a write asks first");
        assert!(
            title.starts_with("Save 1 change to users on edit-test-"),
            "{title}"
        );
        assert!(detail.contains("UPDATE \"public\".\"users\""), "{detail}");
        assert!(
            detail.contains("SET \"email\" = 'new@example.com'"),
            "{detail}"
        );
        assert!(detail.contains("WHERE \"id\" = 1;"), "{detail}");
        assert_eq!(update_calls(&calls), 0, "nothing before the answer");

        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(update_calls(&calls), 0, "Cancel sends nothing");
        assert_eq!(staged(&panel, cx).len(), 1, "and keeps the edit");

        save(&panel, cx);
        cx.simulate_prompt_answer("Save");
        cx.run_until_parked();

        let sent = calls.updates.lock().clone();
        assert_eq!(sent.len(), 1, "exactly one batch");
        assert_eq!(sent[0].schema, "public");
        assert_eq!(sent[0].table, "users");
        assert_eq!(
            sent[0].changes,
            vec![email_change("1", text_value("new@example.com"))]
        );
        assert_eq!(staged(&panel, cx), vec![], "saved edits are not pending");
        assert_eq!(
            asked(&calls, "query"),
            queries + 1,
            "the page is read again"
        );
        assert!(
            notice(&panel, cx).contains("Saved 1 change in 1 row"),
            "{}",
            notice(&panel, cx)
        );
    }

    #[gpui::test]
    async fn unrestricted_saves_without_asking(cx: &mut TestAppContext) {
        let (panel, calls, cx) =
            users_table(cx, FakeBehaviour::Well, Some(SafetyLevel::Unrestricted)).await;
        edit_email(&panel, 1, "set@example.com", cx);
        save(&panel, cx);

        assert!(!cx.has_pending_prompt());
        assert_eq!(
            calls.updates.lock()[0].changes,
            vec![email_change("2", text_value("set@example.com"))]
        );
    }

    /// Several cells in a row and several rows travel as one batch, each row
    /// once, addressed by its own key.
    #[gpui::test]
    async fn every_edit_travels_in_one_batch_by_row(cx: &mut TestAppContext) {
        let (panel, calls, cx) =
            users_table(cx, FakeBehaviour::Well, Some(SafetyLevel::Unrestricted)).await;
        edit_email(&panel, 2, "c@example.com", cx);
        edit_email(&panel, 0, "a@example.com", cx);
        panel.update(cx, |panel, cx| panel.set_cell_null(1, 1, cx));
        // Row 1 was NULL already, so this is no edit.
        assert_eq!(staged(&panel, cx).len(), 2);
        panel.update(cx, |panel, cx| panel.set_cell_null(0, 1, cx));
        save(&panel, cx);

        let sent = calls.updates.lock().clone();
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0].changes,
            vec![
                email_change("1", EditValue::Null),
                email_change("3", text_value("c@example.com")),
            ]
        );
    }

    #[gpui::test]
    async fn read_only_after_staging_sends_nothing(cx: &mut TestAppContext) {
        let (panel, calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "x", cx);
        panel.update(cx, |panel, cx| {
            panel.set_safety(0, SafetyLevel::ReadOnly, cx)
        });

        save(&panel, cx);

        assert!(!cx.has_pending_prompt());
        assert_eq!(update_calls(&calls), 0);
        assert!(
            failure(&panel, cx).contains("Read Only"),
            "{}",
            failure(&panel, cx)
        );
        assert_eq!(staged(&panel, cx).len(), 1, "the edit is kept");
    }

    /// The prompt stays up while the user does other things, one of which is
    /// tightening the connection.
    #[gpui::test]
    async fn a_level_tightened_while_the_prompt_is_up_wins_over_save(cx: &mut TestAppContext) {
        let (panel, calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "x", cx);
        save(&panel, cx);
        assert!(cx.has_pending_prompt());

        panel.update(cx, |panel, cx| {
            panel.set_safety(0, SafetyLevel::ReadOnly, cx)
        });
        cx.simulate_prompt_answer("Save");
        cx.run_until_parked();

        assert_eq!(update_calls(&calls), 0);
        assert!(failure(&panel, cx).contains("Read Only"));
    }

    /// The yes was for these changes: a cell edited under the open question
    /// makes it a different save.
    #[gpui::test]
    async fn an_edit_made_under_the_open_prompt_is_not_saved_by_its_yes(cx: &mut TestAppContext) {
        let (panel, calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "first", cx);
        save(&panel, cx);
        panel.update(cx, |panel, cx| {
            let environment = panel.edit_environment(cx).expect("connected");
            if let QueryState::Done(page) = &mut panel.query {
                page.stage_cell(
                    &environment.dialect,
                    CellAddress { row: 2, column: 1 },
                    Some("sneaked in".into()),
                );
            }
        });
        cx.simulate_prompt_answer("Save");
        cx.run_until_parked();

        assert_eq!(update_calls(&calls), 0);
        assert!(failure(&panel, cx).contains("Save again"));
    }

    #[gpui::test]
    async fn a_second_save_while_one_is_out_sends_nothing_more(cx: &mut TestAppContext) {
        let (panel, calls, cx) =
            users_table(cx, FakeBehaviour::Well, Some(SafetyLevel::Unrestricted)).await;
        edit_email(&panel, 0, "x", cx);
        panel.update_in(cx, |panel, window, cx| {
            panel.request_save(window, cx);
            panel.request_save(window, cx);
        });
        cx.run_until_parked();
        assert_eq!(update_calls(&calls), 1);
    }

    #[gpui::test]
    async fn a_conflict_marks_the_row_and_keeps_the_edits(cx: &mut TestAppContext) {
        let (panel, calls, cx) = users_table(
            cx,
            FakeBehaviour::UpdateConflicts { change: 1 },
            Some(SafetyLevel::Unrestricted),
        )
        .await;
        edit_email(&panel, 0, "a", cx);
        edit_email(&panel, 2, "c", cx);
        save(&panel, cx);

        assert_eq!(update_calls(&calls), 1);
        assert_eq!(staged(&panel, cx).len(), 2, "every edit is still there");
        let rows = with_page(&panel, cx, |page| {
            page.edits.row_errors.keys().copied().collect::<Vec<_>>()
        });
        assert_eq!(rows, vec![2], "the row of change 1 is the one marked");
        let message = failure(&panel, cx);
        assert!(message.contains("row 3"), "{message}");
        assert!(
            message.contains("changed or removed since it was read"),
            "{message}"
        );
        draw_again(cx);
        bounds_of(cx, "database-edit-bar");
    }

    #[gpui::test]
    async fn a_rejected_value_marks_the_cell_the_driver_named(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(
            cx,
            FakeBehaviour::UpdateRejectsValue { change: 1 },
            Some(SafetyLevel::Unrestricted),
        )
        .await;
        edit_email(&panel, 0, "a", cx);
        edit_email(&panel, 2, "c", cx);
        save(&panel, cx);

        let marked = staged(&panel, cx);
        assert_eq!(marked[0].3, None, "row 0 was fine");
        let message = marked[1].3.clone().expect("the cell the driver named");
        assert!(
            message.contains("the engine rejected the value"),
            "{message}"
        );
        assert!(failure(&panel, cx).contains("rejected"));
    }

    #[gpui::test]
    async fn a_driver_that_refuses_the_write_says_so_in_the_bar(cx: &mut TestAppContext) {
        let (panel, _calls, cx) =
            users_table(cx, FakeBehaviour::ReadOnly, Some(SafetyLevel::Unrestricted)).await;
        edit_email(&panel, 0, "a", cx);
        save(&panel, cx);

        assert!(
            failure(&panel, cx).contains("read-only"),
            "{}",
            failure(&panel, cx)
        );
        assert_eq!(staged(&panel, cx).len(), 1);
    }

    #[gpui::test]
    async fn preview_shows_the_update_without_sending_it(cx: &mut TestAppContext) {
        let (panel, calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "o'hara", cx);
        panel.update_in(cx, |panel, window, cx| panel.show_save_preview(window, cx));
        cx.run_until_parked();

        let (title, detail) = cx.pending_prompt().expect("the preview is a dialog");
        assert!(title.starts_with("SQL for 1 unsaved change"), "{title}");
        assert!(detail.contains("'o''hara'"), "values are quoted: {detail}");
        cx.simulate_prompt_answer("Close");
        cx.run_until_parked();
        assert_eq!(update_calls(&calls), 0);
        assert_eq!(staged(&panel, cx).len(), 1, "the edits stay");
    }

    #[gpui::test]
    async fn the_discard_button_drops_the_edits_at_once(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "x", cx);
        click_on(cx, "database-edit-discard");
        draw_again(cx);
        assert_eq!(staged(&panel, cx), vec![]);
        assert!(cx.debug_bounds("database-edit-bar").is_none());
    }

    #[gpui::test]
    async fn escape_on_the_grid_asks_before_discarding(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "x", cx);
        panel.update_in(cx, |panel, window, cx| {
            window.focus(&panel.results_focus, cx);
        });
        cx.dispatch_action(menu::Cancel);
        cx.run_until_parked();

        let (title, _) = cx.pending_prompt().expect("escape asks");
        assert_eq!(title, "Discard 1 unsaved change?");
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(staged(&panel, cx).len(), 1);

        cx.dispatch_action(menu::Cancel);
        cx.run_until_parked();
        cx.simulate_prompt_answer("Discard");
        cx.run_until_parked();
        assert_eq!(staged(&panel, cx), vec![]);
    }

    /// Every way of reading a page over this one asks first, and a no leaves
    /// the page and its edits as they were.
    #[gpui::test]
    async fn reading_another_page_asks_and_cancel_keeps_everything(cx: &mut TestAppContext) {
        let (panel, calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "kept", cx);
        let queries = asked(&calls, "query");

        let ways: Vec<(
            &str,
            Box<dyn Fn(&mut DatabasePanel, &mut gpui::Window, &mut gpui::Context<DatabasePanel>)>,
        )> = vec![
            (
                "next page",
                Box::new(|panel, window, cx| panel.page(true, window, cx)),
            ),
            (
                "sort",
                Box::new(|panel, window, cx| panel.sort_by("email", window, cx)),
            ),
            (
                "open data",
                Box::new(|panel, window, cx| {
                    panel.open_table_data(0, "public", "users", window, cx)
                }),
            ),
            (
                "count",
                Box::new(|panel, window, cx| panel.count_rows(0, "public", "users", window, cx)),
            ),
            (
                "typed statement",
                Box::new(|panel, window, cx| {
                    panel.run_page("select 1".into(), 0, 10, None, window, cx)
                }),
            ),
        ];
        for (way, start) in ways {
            // Not `page(true)` alone: the fake has three rows, so give it a
            // reason to have a next page.
            panel.update(cx, |panel, _| {
                if let QueryState::Done(page) = &mut panel.query {
                    page.result.truncated = true;
                }
            });
            panel.update_in(cx, |panel, window, cx| start(panel, window, cx));
            cx.run_until_parked();

            let (title, _) = cx
                .pending_prompt()
                .unwrap_or_else(|| panic!("{way} must ask"));
            assert_eq!(title, "Discard 1 unsaved change?", "{way}");
            cx.simulate_prompt_answer("Cancel");
            cx.run_until_parked();

            assert_eq!(asked(&calls, "query"), queries, "{way} read nothing");
            assert_eq!(staged(&panel, cx).len(), 1, "{way} kept the edit");
            assert!(matches!(
                panel.read_with(cx, |p, _| matches!(p.query, QueryState::Done(_))),
                true
            ));
        }

        panel.update_in(cx, |panel, window, cx| panel.page(true, window, cx));
        cx.run_until_parked();
        cx.simulate_prompt_answer("Discard");
        cx.run_until_parked();
        assert_eq!(asked(&calls, "query"), queries + 1, "a yes reads the page");
        assert_eq!(
            with_page(&panel, cx, |page| page.edits.staged.len()),
            0,
            "and the new page starts clean"
        );
    }

    /// Typing a statement and running it is the one read the panel guards at its
    /// door rather than at the read: asking after the confirmation of a write
    /// would be two questions for one run.
    #[gpui::test]
    async fn running_a_typed_statement_asks_about_the_edits_first(cx: &mut TestAppContext) {
        let (panel, calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "kept", cx);
        let queries = asked(&calls, "query");
        panel.update_in(cx, |panel, window, cx| {
            panel.sql_editor.update(cx, |editor, cx| {
                editor.set_text("select 1", window, cx);
            });
            panel.run_query(crate::panel_queries::RunScope::All, window, cx);
        });
        cx.run_until_parked();

        let (title, _) = cx.pending_prompt().expect("asks");
        assert_eq!(title, "Discard 1 unsaved change?");
        cx.simulate_prompt_answer("Discard");
        cx.run_until_parked();
        assert!(!cx.has_pending_prompt(), "one question, not two");
        assert_eq!(asked(&calls, "query"), queries + 1);
    }

    #[gpui::test]
    async fn with_no_edits_reading_another_page_does_not_ask(cx: &mut TestAppContext) {
        let (panel, calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        let queries = asked(&calls, "query");
        panel.update_in(cx, |panel, window, cx| {
            panel.sort_by("email", window, cx);
        });
        cx.run_until_parked();
        assert!(!cx.has_pending_prompt());
        assert_eq!(asked(&calls, "query"), queries + 1);
    }

    #[gpui::test]
    async fn a_half_typed_edit_counts_as_unsaved(cx: &mut TestAppContext) {
        let (panel, calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        assert!(open_editor(&panel, 0, 1, cx));
        type_into_editor(&panel, "half", cx);
        let queries = asked(&calls, "query");
        panel.update_in(cx, |panel, window, cx| panel.sort_by("email", window, cx));
        cx.run_until_parked();

        assert!(cx.has_pending_prompt(), "what is in the box is not lost");
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(asked(&calls, "query"), queries);
        assert_eq!(
            staged(&panel, cx),
            vec![(0, 1, Some("half".to_string()), None)],
            "asking kept what was typed as an edit"
        );
    }

    #[gpui::test]
    async fn the_cell_menu_offers_edit_null_and_revert_where_they_apply(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "x", cx);
        panel.read_with(cx, |panel, cx| {
            let key = panel.cell_menu(0, 0, cx);
            assert!(key.offered && !key.editable && !key.nullable && !key.staged);
            let email = panel.cell_menu(0, 1, cx);
            assert!(email.editable && email.nullable && email.staged);
            let plain = panel.cell_menu(2, 1, cx);
            assert!(plain.editable && !plain.staged);
        });
        panel.update_in(cx, |panel, window, cx| {
            panel.deploy_cell_menu(0, 1, gpui::point(gpui::px(10.), gpui::px(10.)), window, cx);
            assert!(panel.context_menu.is_some());
        });
        draw_again(cx);

        panel.update(cx, |panel, cx| panel.revert_cell_edit(0, 1, cx));
        assert_eq!(staged(&panel, cx), vec![]);
    }

    #[gpui::test]
    async fn set_to_null_stages_a_null_and_saves_it_as_one(cx: &mut TestAppContext) {
        let (panel, calls, cx) =
            users_table(cx, FakeBehaviour::Well, Some(SafetyLevel::Unrestricted)).await;
        panel.update(cx, |panel, cx| panel.set_cell_null(0, 1, cx));
        assert_eq!(staged(&panel, cx), vec![(0, 1, None, None)]);
        save(&panel, cx);
        assert_eq!(
            calls.updates.lock()[0].changes,
            vec![email_change("1", EditValue::Null)]
        );
    }

    /// The bindings of the bundled keymaps that matter to the box, bound the
    /// same way: Tab, Enter and Escape are the editor's own bindings first, and
    /// it hands them on because it is one line.
    #[gpui::test]
    async fn the_keys_of_the_bundled_keymap_drive_the_box(cx: &mut TestAppContext) {
        use gpui::KeyBinding;
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        cx.update(|_, cx| {
            cx.bind_keys([
                KeyBinding::new("tab", menu::SelectNext, None),
                KeyBinding::new("enter", menu::Confirm, None),
                KeyBinding::new("escape", menu::Cancel, None),
                KeyBinding::new("down", menu::SelectNext, Some("DatabaseResults")),
                KeyBinding::new("escape", editor::actions::Cancel, Some("Editor")),
                KeyBinding::new("tab", editor::actions::Tab, Some("Editor")),
                KeyBinding::new("down", zed_actions::editor::MoveDown, Some("Editor")),
                KeyBinding::new(
                    "tab",
                    zed_actions::database::EditNextCell,
                    Some("DatabaseCellEditor > Editor"),
                ),
                KeyBinding::new(
                    "cmd-s",
                    zed_actions::database::SaveChanges,
                    Some("DatabaseResults"),
                ),
            ]);
        });

        assert!(open_editor(&panel, 0, 1, cx));
        type_into_editor(&panel, "by keys", cx);
        cx.simulate_keystrokes("down");
        assert_eq!(
            with_page(&panel, cx, |page| page.selection.rows()),
            Vec::<usize>::new(),
            "an arrow in the box does not move the grid's selection"
        );
        cx.simulate_keystrokes("tab");
        draw_again(cx);
        assert_eq!(
            staged(&panel, cx),
            vec![(0, 1, Some("by keys".to_string()), None)]
        );
        assert_eq!(editing(&panel, cx), Some((1, 1, String::new())));

        cx.simulate_keystrokes("escape");
        assert_eq!(editing(&panel, cx), None, "Escape closes the box");
        assert!(!cx.has_pending_prompt());

        assert!(open_editor(&panel, 2, 1, cx));
        type_into_editor(&panel, "enter", cx);
        cx.simulate_keystrokes("enter");
        assert_eq!(staged(&panel, cx).len(), 2, "Enter stages");

        assert!(open_editor(&panel, 1, 1, cx));
        type_into_editor(&panel, "then save", cx);
        cx.simulate_keystrokes("cmd-s");
        cx.run_until_parked();
        let (title, _) = cx.pending_prompt().expect("cmd-s saves, through the gate");
        assert!(title.starts_with("Save 3 changes to users"), "{title}");
    }

    fn set_cell(
        panel: &Panel,
        row: usize,
        column: usize,
        cell: database::protocol::Cell,
        cx: &mut Cx,
    ) {
        panel.update(cx, |panel, _| {
            let QueryState::Done(page) = &mut panel.query else {
                panic!("a page was expected");
            };
            page.result.rows[row][column] = cell;
        });
    }

    fn text_cell(value: &str) -> database::protocol::Cell {
        database::protocol::Cell::Text {
            value: value.into(),
        }
    }

    fn row_updates() -> Capabilities {
        Capabilities {
            row_updates: true,
            ..Default::default()
        }
    }

    fn stage_directly(panel: &Panel, row: usize, column: usize, value: &str, cx: &mut Cx) {
        let dialect = crate::sql_dialect::Dialect::detect("postgres", &Default::default());
        panel.update(cx, |panel, _| {
            if let QueryState::Done(page) = &mut panel.query {
                page.stage_cell(&dialect, CellAddress { row, column }, Some(value.into()));
            }
        });
    }

    fn page_is_shown(panel: &Panel, cx: &mut Cx) -> bool {
        panel.read_with(cx, |panel, _| matches!(panel.query, QueryState::Done(_)))
    }

    fn lose_the_connection(panel: &Panel, cx: &mut Cx) {
        panel.update(cx, |panel, _| {
            panel.connections[0].state = crate::database_panel::NodeState::Idle;
        });
    }

    /// What the settings reload does to a connection whose url moved: an idle
    /// node of the same name in the place of the open one -- and then someone
    /// connects it again.
    fn replace_with_another_database(panel: &Panel, cx: &mut Cx) -> DriverCalls {
        panel.update(cx, |panel, _| {
            let mut config = panel.connections[0].config.clone();
            config.url = "/tmp/production.sqlite".into();
            let mut node = crate::database_panel::ConnectionNode::idle(config);
            node.from_project = true;
            panel.connections[0] = node;
        });
        open_on_fake_driver(panel, 0, FakeBehaviour::Well, row_updates(), cx)
    }

    /// The box holds a line break as `\n` whatever the cell holds, so a cell
    /// with `\r\n` in it, opened and left alone, must not read as changed.
    #[gpui::test]
    async fn a_cell_with_windows_line_breaks_opened_and_left_alone_is_no_edit(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        for original in ["a\r\nb", "a\rb", "a\r\nb\r\nc\n"] {
            set_cell(&panel, 0, 1, text_cell(original), cx);
            assert!(open_editor(&panel, 0, 1, cx));
            panel.read_with(cx, |panel, cx| {
                assert!(
                    !panel.has_unsaved_edits(cx),
                    "{original:?}: a box nobody typed in is not a half-typed edit"
                );
            });
            confirm(cx);
            assert_eq!(staged(&panel, cx), vec![], "{original:?}");
            assert!(cx.debug_bounds("database-edit-bar").is_none());
        }
    }

    #[gpui::test]
    async fn walking_over_cells_with_windows_line_breaks_stages_none_of_them(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        set_cell(&panel, 0, 1, text_cell("a\r\nb"), cx);
        set_cell(&panel, 1, 1, text_cell("c\rd"), cx);
        set_cell(&panel, 2, 1, text_cell("e\r\nf"), cx);
        assert!(open_editor(&panel, 0, 1, cx));
        for _ in 0..3 {
            panel.update_in(cx, |panel, window, cx| {
                panel.edit_neighbour(true, window, cx)
            });
            draw_again(cx);
        }
        assert_eq!(staged(&panel, cx), vec![]);
        assert!(cx.debug_bounds("database-edit-bar").is_none());
    }

    #[gpui::test]
    async fn a_save_never_reaches_a_database_that_replaced_the_one_the_page_was_read_from(
        cx: &mut TestAppContext,
    ) {
        let (panel, calls, cx) =
            users_table(cx, FakeBehaviour::Well, Some(SafetyLevel::Unrestricted)).await;
        edit_email(&panel, 0, "meant for staging", cx);
        let production = replace_with_another_database(&panel, cx);

        save(&panel, cx);

        assert!(!cx.has_pending_prompt());
        assert_eq!(update_calls(&calls), 0);
        assert_eq!(update_calls(&production), 0, "nothing reaches the new one");
        let message = failure(&panel, cx);
        assert!(message.contains("connection changed"), "{message}");
        assert!(message.contains("reload"), "{message}");
        assert_eq!(staged(&panel, cx).len(), 1, "the edit is kept");

        assert!(!open_editor(&panel, 1, 1, cx));
        assert!(notice(&panel, cx).contains("connection changed"));
    }

    #[gpui::test]
    async fn the_yes_of_a_prompt_does_not_carry_over_to_another_database(cx: &mut TestAppContext) {
        let (panel, calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "meant for staging", cx);
        save(&panel, cx);
        assert!(cx.has_pending_prompt());

        let production = replace_with_another_database(&panel, cx);
        cx.simulate_prompt_answer("Save");
        cx.run_until_parked();

        assert_eq!(update_calls(&calls), 0);
        assert_eq!(update_calls(&production), 0);
        assert!(failure(&panel, cx).contains("connection changed"));
    }

    #[gpui::test]
    async fn discarding_or_leaving_while_a_save_is_out_is_refused(cx: &mut TestAppContext) {
        let (panel, calls, cx) =
            users_table(cx, FakeBehaviour::Well, Some(SafetyLevel::Unrestricted)).await;
        edit_email(&panel, 0, "x", cx);
        let queries = asked(&calls, "query");
        panel.update_in(cx, |panel, window, cx| panel.request_save(window, cx));

        panel.update_in(cx, |panel, window, cx| panel.discard_changes(window, cx));
        assert_eq!(
            staged(&panel, cx).len(),
            1,
            "Discard does not unsend a write"
        );
        assert!(notice(&panel, cx).contains("A save is in progress"));

        panel.update_in(cx, |panel, window, cx| panel.confirm_discard(window, cx));
        assert!(!cx.has_pending_prompt(), "a yes could not undo it either");

        panel.update_in(cx, |panel, window, cx| panel.sort_by("email", window, cx));
        assert!(!cx.has_pending_prompt());

        cx.run_until_parked();
        assert_eq!(update_calls(&calls), 1);
        assert_eq!(
            asked(&calls, "query"),
            queries + 1,
            "only the read after the save: the sort was refused"
        );
        assert_eq!(staged(&panel, cx), vec![]);
        assert!(
            notice(&panel, cx).contains("Saved 1 change"),
            "{}",
            notice(&panel, cx)
        );
    }

    /// The question was asked before the save went out, and answered after.
    #[gpui::test]
    async fn a_yes_to_discard_given_after_a_save_went_out_changes_nothing(cx: &mut TestAppContext) {
        let (panel, calls, cx) =
            users_table(cx, FakeBehaviour::Well, Some(SafetyLevel::Unrestricted)).await;
        edit_email(&panel, 0, "x", cx);
        let queries = asked(&calls, "query");
        panel.update_in(cx, |panel, window, cx| panel.sort_by("email", window, cx));
        cx.run_until_parked();
        assert!(cx.has_pending_prompt(), "asked about the edit");

        panel.update_in(cx, |panel, window, cx| panel.request_save(window, cx));
        cx.simulate_prompt_answer("Discard");
        cx.run_until_parked();

        assert_eq!(update_calls(&calls), 1);
        assert_eq!(asked(&calls, "query"), queries + 1, "the save's own read");
        assert!(
            notice(&panel, cx).contains("Saved 1 change"),
            "the save was answered as the save it was: {}",
            notice(&panel, cx)
        );
    }

    /// What the paths that replace a page without asking would leave behind: a
    /// new page, saving again, while the first save has not answered.
    #[gpui::test]
    async fn the_answer_to_an_earlier_save_is_not_taken_for_a_later_ones(cx: &mut TestAppContext) {
        let (panel, calls, cx) = users_table(
            cx,
            FakeBehaviour::UpdateConflicts { change: 1 },
            Some(SafetyLevel::Unrestricted),
        )
        .await;
        stage_directly(&panel, 0, 1, "first", cx);
        panel.update_in(cx, |panel, window, cx| panel.request_save(window, cx));

        panel.update(cx, |panel, _| {
            if let QueryState::Done(page) = &mut panel.query {
                page.edits = Default::default();
            }
        });
        stage_directly(&panel, 0, 1, "second", cx);
        stage_directly(&panel, 2, 1, "third", cx);
        panel.update_in(cx, |panel, window, cx| panel.request_save(window, cx));
        cx.run_until_parked();

        assert_eq!(update_calls(&calls), 2);
        assert_eq!(
            staged(&panel, cx).len(),
            2,
            "the second save failed, so its edits are still there"
        );
        assert!(
            failure(&panel, cx).contains("changed or removed since it was read"),
            "{}",
            failure(&panel, cx)
        );
    }

    #[gpui::test]
    async fn a_save_with_the_connection_gone_says_so_and_keeps_the_edits(cx: &mut TestAppContext) {
        let (panel, calls, cx) =
            users_table(cx, FakeBehaviour::Well, Some(SafetyLevel::Unrestricted)).await;
        edit_email(&panel, 0, "x", cx);
        lose_the_connection(&panel, cx);

        save(&panel, cx);

        assert_eq!(update_calls(&calls), 0);
        let message = failure(&panel, cx);
        assert!(message.contains("Not connected"), "{message}");
        assert_eq!(staged(&panel, cx).len(), 1);
    }

    #[gpui::test]
    async fn a_yes_with_the_connection_gone_says_so_too(cx: &mut TestAppContext) {
        let (panel, calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "x", cx);
        save(&panel, cx);
        assert!(cx.has_pending_prompt());
        lose_the_connection(&panel, cx);
        cx.simulate_prompt_answer("Save");
        cx.run_until_parked();

        assert_eq!(update_calls(&calls), 0);
        assert!(failure(&panel, cx).contains("Not connected"));
    }

    /// Leaving a half-typed box while there is nothing to check it against
    /// must not throw what was typed away.
    #[gpui::test]
    async fn a_half_typed_value_survives_the_connection_going_away(cx: &mut TestAppContext) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        assert!(open_editor(&panel, 0, 1, cx));
        type_into_editor(&panel, "typed just now", cx);
        lose_the_connection(&panel, cx);

        panel.update_in(cx, |panel, window, cx| panel.confirm_editor(window, cx));

        let kept = staged(&panel, cx);
        assert_eq!(kept.len(), 1, "what was typed is not dropped");
        assert_eq!(kept[0].2.as_deref(), Some("typed just now"));
        assert!(
            kept[0].3.is_some(),
            "and it says it has not been checked, so it cannot be saved as it is"
        );
    }

    #[gpui::test]
    async fn a_page_turn_over_a_closed_connection_keeps_the_page_and_its_edits(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "x", cx);
        panel.update(cx, |panel, _| {
            if let QueryState::Done(page) = &mut panel.query {
                page.result.truncated = true;
            }
        });
        lose_the_connection(&panel, cx);

        panel.update_in(cx, |panel, window, cx| panel.page(true, window, cx));
        cx.run_until_parked();

        assert!(page_is_shown(&panel, cx), "the page was not replaced");
        assert_eq!(staged(&panel, cx).len(), 1);
        assert!(
            failure(&panel, cx).contains("not connected"),
            "{}",
            failure(&panel, cx)
        );
    }

    /// The write went out; saying that nothing was sent would be untrue.
    #[gpui::test]
    async fn a_save_that_landed_is_never_reported_as_not_sent(cx: &mut TestAppContext) {
        let (panel, calls, cx) =
            users_table(cx, FakeBehaviour::Well, Some(SafetyLevel::Unrestricted)).await;
        edit_email(&panel, 0, "x", cx);
        panel.update_in(cx, |panel, window, cx| panel.request_save(window, cx));
        lose_the_connection(&panel, cx);
        cx.run_until_parked();

        assert_eq!(update_calls(&calls), 1);
        assert!(page_is_shown(&panel, cx), "the page stays, edits cleared");
        assert_eq!(staged(&panel, cx), vec![]);
        let said = notice(&panel, cx);
        assert!(said.contains("Saved 1 change"), "{said}");
        assert!(!said.contains("nothing was sent"), "{said}");
    }

    #[gpui::test]
    async fn describing_a_table_that_has_no_columns_does_not_replace_a_page_with_edits(
        cx: &mut TestAppContext,
    ) {
        use crate::database_panel::OpenTable;
        use crate::tree_menus::Generate;
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "x", cx);
        panel.update_in(cx, |panel, window, cx| {
            panel.open_table = Some(OpenTable {
                connection: 0,
                schema: "public".into(),
                table: "users".into(),
                columns: Some(Vec::new()),
                columns_load: 0,
            });
            panel.generate_statement(0, "public", "users", Generate::Update, window, cx);
        });

        assert!(page_is_shown(&panel, cx));
        assert_eq!(staged(&panel, cx).len(), 1);
        assert!(failure(&panel, cx).contains("no columns"));
    }

    /// A statement from the history is not guarded by the discard question the
    /// editor's run has; a refusal must not be what loses the edits.
    #[gpui::test]
    async fn a_write_refused_from_the_history_does_not_replace_a_page_with_edits(
        cx: &mut TestAppContext,
    ) {
        use crate::panel_queries::ChosenStatement;
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "x", cx);
        panel.update(cx, |panel, cx| {
            panel.set_safety(0, SafetyLevel::ReadOnly, cx)
        });

        panel.update_in(cx, |panel, window, cx| {
            panel.run_gated(
                ChosenStatement {
                    sql: "DELETE FROM users".into(),
                    range: None,
                },
                window,
                cx,
            )
        });

        assert!(page_is_shown(&panel, cx));
        assert_eq!(staged(&panel, cx).len(), 1);
        assert!(failure(&panel, cx).contains("Read Only"));
    }

    /// The page belongs to `a`; the connection removed is `b`, which was only
    /// the active one.
    #[gpui::test]
    async fn removing_the_active_connection_leaves_another_connections_page_alone(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "x", cx);
        set_connections(cx, &[("b", "/tmp/b.sqlite")]);
        panel.update_in(cx, |panel, window, cx| {
            assert_eq!(panel.connection_names()[0], "b");
            panel.set_active(0, window, cx);
        });

        set_connections(cx, &[]);

        panel.read_with(cx, |panel, _| assert_eq!(panel.active, None));
        assert!(page_is_shown(&panel, cx), "the page was not b's");
        assert_eq!(staged(&panel, cx).len(), 1);
    }

    /// Saving finishes in the background while the user types on another
    /// connection: the editor must not be swapped from under them.
    #[gpui::test]
    async fn a_save_finishing_does_not_move_the_active_connection_or_its_editor(
        cx: &mut TestAppContext,
    ) {
        let (panel, calls, cx) =
            users_table(cx, FakeBehaviour::Well, Some(SafetyLevel::Unrestricted)).await;
        edit_email(&panel, 0, "x", cx);
        set_connections(cx, &[("b", "/tmp/b.sqlite")]);
        panel.update_in(cx, |panel, window, cx| {
            panel.set_active(0, window, cx);
            panel.sql_editor.update(cx, |editor, cx| {
                editor.set_text("select 'mine'", window, cx);
            });
        });
        let queries = asked(&calls, "query");

        save(&panel, cx);

        assert_eq!(update_calls(&calls), 1);
        panel.read_with(cx, |panel, cx| {
            assert_eq!(panel.active, Some(0), "still on b");
            assert_eq!(
                panel.sql_editor.read(cx).text(cx),
                "select 'mine'",
                "what is being typed stays"
            );
        });
        assert_eq!(asked(&calls, "query"), queries, "no read over b's view");
        assert!(
            notice(&panel, cx).contains("Saved 1 change"),
            "{}",
            notice(&panel, cx)
        );
        assert!(
            notice(&panel, cx).contains("reload"),
            "{}",
            notice(&panel, cx)
        );
    }

    #[gpui::test]
    async fn a_menu_entry_opened_over_a_page_does_nothing_to_the_page_that_replaced_it(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.deploy_cell_menu(0, 1, gpui::point(gpui::px(10.), gpui::px(10.)), window, cx);
        });
        draw_again(cx);
        // Edit Value, then Set to NULL.
        cx.dispatch_action(menu::SelectNext);
        cx.dispatch_action(menu::SelectNext);

        panel.update_in(cx, |panel, window, cx| panel.sort_by("email", window, cx));
        draw_again(cx);
        cx.dispatch_action(menu::Confirm);
        cx.run_until_parked();

        assert_eq!(
            staged(&panel, cx),
            vec![],
            "the entry was for the rows that were there"
        );
    }

    #[gpui::test]
    async fn escape_in_the_statement_box_does_not_ask_about_the_grids_edits(
        cx: &mut TestAppContext,
    ) {
        use gpui::Focusable as _;
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "x", cx);
        panel.update_in(cx, |panel, window, cx| {
            let handle = panel.sql_editor.focus_handle(cx);
            window.focus(&handle, cx);
        });

        cx.dispatch_action(menu::Cancel);
        cx.run_until_parked();

        assert!(!cx.has_pending_prompt());
        assert_eq!(staged(&panel, cx).len(), 1);
    }

    /// The database named the cell; asking again without the cell having been
    /// edited would only get the same answer.
    #[gpui::test]
    async fn a_value_the_database_refused_is_not_sent_again_until_it_is_edited(
        cx: &mut TestAppContext,
    ) {
        let (panel, calls, cx) = users_table(
            cx,
            FakeBehaviour::UpdateRejectsValue { change: 0 },
            Some(SafetyLevel::Unrestricted),
        )
        .await;
        edit_email(&panel, 0, "a", cx);
        save(&panel, cx);
        assert_eq!(update_calls(&calls), 1);
        assert!(staged(&panel, cx)[0].3.is_some(), "the cell is marked");

        save(&panel, cx);
        assert_eq!(update_calls(&calls), 1, "cmd-s again sends nothing");
        let message = failure(&panel, cx);
        assert!(message.contains("invalid"), "{message}");
        assert!(staged(&panel, cx)[0].3.is_some(), "still marked");

        edit_email(&panel, 0, "b", cx);
        save(&panel, cx);
        assert_eq!(update_calls(&calls), 2, "an edited cell is sent");
    }

    /// Putting the result area away when its connection goes must not throw
    /// away edits nobody was asked about.
    #[gpui::test]
    async fn a_closed_connections_result_is_put_away_unless_edits_wait_on_it(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        let key = panel.read_with(cx, |panel, _| panel.node_key(0).expect("a node"));
        edit_email(&panel, 0, "x", cx);

        panel.update(cx, |panel, _| panel.clear_result_of(&key));
        assert!(page_is_shown(&panel, cx), "the edit is not lost");
        assert_eq!(staged(&panel, cx).len(), 1);

        click_on(cx, "database-edit-discard");
        panel.update(cx, |panel, _| panel.clear_result_of(&key));
        assert!(!page_is_shown(&panel, cx), "nothing is waiting: put away");
    }

    #[gpui::test]
    async fn disconnecting_the_active_connection_leaves_another_connections_page_alone(
        cx: &mut TestAppContext,
    ) {
        let (panel, _calls, cx) = users_table(cx, FakeBehaviour::Well, None).await;
        edit_email(&panel, 0, "x", cx);
        set_connections(cx, &[("b", "/tmp/b.sqlite")]);
        open_on_fake_driver(&panel, 0, FakeBehaviour::Well, row_updates(), cx);
        panel.update_in(cx, |panel, window, cx| panel.set_active(0, window, cx));

        panel.update(cx, |panel, cx| panel.disconnect(0, cx));

        panel.read_with(cx, |panel, _| assert_eq!(panel.active, None));
        assert!(page_is_shown(&panel, cx));
        assert_eq!(staged(&panel, cx).len(), 1);
    }
}
