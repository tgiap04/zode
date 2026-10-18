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

        let results = self
            .panel
            .update(cx, |panel, cx| panel.render_results(window, cx));
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
