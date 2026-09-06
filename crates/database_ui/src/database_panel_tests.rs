use crate::DatabasePanel;
use fs::FakeFs;
use gpui::{AppContext as _, TestAppContext, UpdateGlobal as _};
use project::Project;
use settings::{Settings as _, SettingsStore};
use workspace::MultiWorkspace;

fn init_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let settings_store = SettingsStore::test(cx);
        cx.set_global(settings_store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        project::DisableAiSettings::register(cx);
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
                    },
                    settings::DatabaseConnectionContent {
                        name: Some("no driver".into()),
                        driver: None,
                        url: Some("/tmp/x.sqlite".into()),
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
    let entries: Vec<_> = entries
        .iter()
        .map(|(name, url)| settings::DatabaseConnectionContent {
            name: Some((*name).to_string()),
            driver: Some("sqlite".into()),
            url: Some((*url).to_string()),
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
        // This test, and the registry that must name the drivers Zode ships,
        // are the two places an engine name legitimately appears.
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name == "driver_registry.rs" || name == "database_panel_tests.rs" {
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
    init_test(cx);
    cx.update(|cx| {
        for id in installed {
            crate::driver_registry::install_for_test(id, cx);
        }
    });
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

    let modal = cx.update(|window, cx| {
        cx.new(|cx| {
            crate::connection_modal::ConnectionModal::new(workspace.downgrade(), window, cx)
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
