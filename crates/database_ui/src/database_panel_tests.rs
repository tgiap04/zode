use crate::DatabasePanel;
use fs::FakeFs;
use gpui::{AppContext as _, TestAppContext, UpdateGlobal as _};
use project::Project;
use settings::{Settings as _, SettingsStore};
use workspace::dock::{DockColumn, DockPosition};
use workspace::{MultiWorkspace, SidebarSide};

fn init_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let settings_store = SettingsStore::test(cx);
        cx.set_global(settings_store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        project::DisableAiSettings::register(cx);
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

    let panel = workspace.update_in(cx, |workspace, window, cx| {
        let panel = cx.new(|cx| DatabasePanel::new(workspace, window, cx));
        workspace.add_panel(panel.clone(), window, cx);
        panel
    });

    (workspace, panel, cx)
}

/// The panel names a `DockPosition` like any left-hand panel, so only
/// `own_column()` keeps it out of the tool dock -- where it would share height
/// with the project and git panels.
#[gpui::test]
async fn the_panel_lands_in_the_database_column(cx: &mut TestAppContext) {
    let (workspace, _panel, cx) = workspace_with_panel(cx).await;

    workspace.read_with(cx, |workspace, cx| {
        assert!(
            workspace
                .dock_for_column(DockColumn::Database)
                .expect("the database column exists")
                .read(cx)
                .panel::<DatabasePanel>()
                .is_some(),
            "the panel must be placed in the database column"
        );
        assert!(
            workspace
                .left_dock()
                .read(cx)
                .panel::<DatabasePanel>()
                .is_none()
                && workspace
                    .right_dock()
                    .read(cx)
                    .panel::<DatabasePanel>()
                    .is_none(),
            "and never in the tool dock its position names"
        );
        assert!(
            workspace
                .agent_dock()
                .read(cx)
                .panel::<DatabasePanel>()
                .is_none(),
            "nor in the other own column"
        );
    });
}

/// Both generic button renderers skip a panel whose icon is `None`. The rail
/// draws this column's button by hand; a second one in the status bar meaning
/// the same thing is worse than none.
#[gpui::test]
async fn the_panel_contributes_no_generic_button(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;

    panel.update_in(cx, |panel, window, cx| {
        use workspace::dock::Panel as _;
        assert!(
            panel.icon(window, cx).is_none(),
            "an icon here would grow a status-bar button beside the rail's own"
        );
    });
}

/// A real draw with the column up, so `render_centre_with_own_columns` and
/// `measure_own_column` run over this panel rather than only over a `TestPanel`.
/// The rail's own button is drawn and tested in `sidebar`, which is where it
/// lives and where the re-entrancy trap is.
#[gpui::test]
async fn the_column_draws_with_the_panel_in_it(cx: &mut TestAppContext) {
    let (workspace, _panel, cx) = workspace_with_panel(cx).await;

    workspace.update_in(cx, |workspace, window, cx| {
        workspace
            .dock_for_column(DockColumn::Database)
            .expect("the database column exists")
            .update(cx, |dock, cx| {
                dock.show_panel(0, window, cx);
                dock.set_open(true, window, cx);
            });
    });

    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

/// The column follows the rail, so the button and the column it opens can never
/// end up at opposite edges of the window.
#[gpui::test]
async fn the_column_follows_the_rail(cx: &mut TestAppContext) {
    let (workspace, _panel, cx) = workspace_with_panel(cx).await;

    for (side, expected) in [
        (SidebarSide::Right, DockPosition::Right),
        (SidebarSide::Left, DockPosition::Left),
    ] {
        cx.update(|_window, cx| {
            SettingsStore::update_global(cx, |settings, cx| {
                settings.update_user_settings(cx, |settings| {
                    settings
                        .workspace
                        .multi_project
                        .get_or_insert_default()
                        .sidebar_side = Some(side);
                });
            });
        });
        cx.run_until_parked();

        workspace.read_with(cx, |workspace, cx| {
            assert_eq!(
                workspace
                    .dock_for_column(DockColumn::Database)
                    .expect("the database column exists")
                    .read(cx)
                    .position(),
                expected,
                "the column must stand on the rail's side ({side:?})"
            );
            assert!(
                workspace
                    .dock_for_column(DockColumn::Database)
                    .expect("the database column exists")
                    .read(cx)
                    .panel::<DatabasePanel>()
                    .is_some(),
                "and the panel must not be hauled into a tool dock on the way"
            );
        });
    }
}

/// An empty column is a legitimate state here -- unlike the agent panel, which
/// closes itself when it holds nothing. Someone who has not added a connection
/// yet still needs somewhere to be told how.
#[gpui::test]
async fn an_empty_column_stays_open(cx: &mut TestAppContext) {
    let (workspace, _panel, cx) = workspace_with_panel(cx).await;

    workspace.update_in(cx, |workspace, window, cx| {
        workspace
            .dock_for_column(DockColumn::Database)
            .expect("the database column exists")
            .update(cx, |dock, cx| {
                dock.show_panel(0, window, cx);
                dock.set_open(true, window, cx);
            });
    });
    cx.run_until_parked();

    workspace.read_with(cx, |workspace, cx| {
        assert!(
            workspace
                .dock_for_column(DockColumn::Database)
                .expect("the database column exists")
                .read(cx)
                .is_open(),
            "a column holding no connections must not close itself"
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
    let (workspace, panel, cx) = workspace_with_panel(cx).await;
    set_connections(cx, &[("a", "/tmp/a.sqlite")]);

    workspace.update_in(cx, |workspace, window, cx| {
        workspace
            .dock_for_column(DockColumn::Database)
            .expect("the database column exists")
            .update(cx, |dock, cx| {
                dock.show_panel(0, window, cx);
                dock.set_open(true, window, cx);
            });
    });

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

/// A modal in a real window, for the add-connection tests.
///
/// Built through the workspace like the real one is: it takes the project's
/// `Fs` from there, and a modal handed a dangling workspace would pass tests the
/// shipped one could not.
pub(crate) async fn modal_for_test(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<crate::connection_modal::ConnectionModal>,
    &mut gpui::VisualTestContext,
) {
    init_test(cx);
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
        workspace.add_panel(panel, window, cx);
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

/// The workspace and `MultiWorkspace` both read this flag to decide what to
/// stop drawing, so it has to be the panel's own state and nothing else.
#[gpui::test]
async fn full_screen_takes_the_whole_window_and_gives_it_back(cx: &mut TestAppContext) {
    let (workspace, panel, cx) = workspace_with_panel(cx).await;

    workspace.update_in(cx, |workspace, window, cx| {
        workspace
            .dock_for_column(DockColumn::Database)
            .expect("the database column exists")
            .update(cx, |dock, cx| {
                dock.show_panel(0, window, cx);
                dock.set_open(true, window, cx);
            });
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        workspace.read_with(cx, |workspace, cx| {
            assert!(
                !workspace.a_column_fills_the_window(window, cx),
                "nothing takes the window until asked"
            );
        });
    });

    panel.update_in(cx, |panel, window, cx| {
        panel.toggle_full_screen(window, cx);
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        workspace.read_with(cx, |workspace, cx| {
            assert_eq!(
                workspace.window_filling_column(window, cx),
                Some(DockColumn::Database),
                "the workspace must see the column asking for the window"
            );
        });
    });

    // Drawn while it holds the window: the branch that drops the centre and the
    // docks only runs here, and asserting on the flag alone would never reach
    // it.
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();

    panel.update_in(cx, |panel, window, cx| {
        panel.toggle_full_screen(window, cx);
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        workspace.read_with(cx, |workspace, cx| {
            assert!(
                workspace.window_filling_column(window, cx).is_none(),
                "closing it must give the window back"
            );
        });
    });
    cx.update(|window, _| window.refresh());
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

/// The rail button is a toggle, and its lit state says the column is up. A lit
/// toggle that does nothing when clicked is the complaint this pins.
///
/// Hidden rather than closed: the panel entity stays, so the tree, the scratch
/// buffer and any open session survive being put away.
#[gpui::test]
async fn the_toggle_action_hides_the_column_on_a_second_use(cx: &mut TestAppContext) {
    init_test(cx);
    cx.update(crate::init);

    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

    let panel = workspace.update_in(cx, |workspace, window, cx| {
        let panel = cx.new(|cx| DatabasePanel::new(workspace, window, cx));
        workspace.add_panel(panel.clone(), window, cx);
        panel
    });
    cx.run_until_parked();

    let column_open = |cx: &mut gpui::VisualTestContext| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .dock_for_column(DockColumn::Database)
                .expect("the database column exists")
                .read(cx)
                .is_open()
        })
    };

    cx.update(|window, cx| {
        window.dispatch_action(Box::new(zed_actions::database::ToggleDatabase), cx)
    });
    cx.run_until_parked();
    assert!(column_open(cx), "the first use must open the column");

    cx.update(|window, cx| {
        window.dispatch_action(Box::new(zed_actions::database::ToggleDatabase), cx)
    });
    cx.run_until_parked();
    assert!(!column_open(cx), "the second use must put it away");

    workspace.read_with(cx, |workspace, cx| {
        assert!(
            workspace
                .dock_for_column(DockColumn::Database)
                .expect("the database column exists")
                .read(cx)
                .panel::<DatabasePanel>()
                .is_some(),
            "hidden, not closed -- the panel and everything it holds must survive"
        );
    });
    drop(panel);

    cx.update(|window, cx| {
        window.dispatch_action(Box::new(zed_actions::database::ToggleDatabase), cx)
    });
    cx.run_until_parked();
    assert!(column_open(cx), "and a third use must bring it back");
}

/// The tool docks are pulled back in when the window narrows; the own columns
/// were not, so a column sized for a wide window kept that width when the
/// sidebar opened beside it and pushed the editor out of view.
#[gpui::test]
async fn a_column_is_pulled_back_in_when_the_space_beside_it_shrinks(cx: &mut TestAppContext) {
    let (workspace, _panel, cx) = workspace_with_panel(cx).await;

    workspace.update_in(cx, |workspace, window, cx| {
        workspace
            .dock_for_column(DockColumn::Database)
            .expect("the database column exists")
            .update(cx, |dock, cx| {
                dock.show_panel(0, window, cx);
                dock.set_open(true, window, cx);
            });
    });
    cx.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
    cx.run_until_parked();

    let column_width = |cx: &mut gpui::VisualTestContext| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .dock_for_column(DockColumn::Database)
                .expect("the database column exists")
                .read(cx)
                .active_panel_size()
                .and_then(|state| state.size)
                .unwrap_or_default()
        })
    };

    workspace.update_in(cx, |workspace, window, cx| {
        workspace
            .dock_for_column(DockColumn::Database)
            .expect("the database column exists")
            .update(cx, |dock, cx| {
                dock.resize_active_panel(Some(gpui::px(1200.)), None, window, cx);
            });
    });
    cx.run_until_parked();
    assert!(column_width(cx) > gpui::px(600.), "the column starts wide");

    // The window narrows -- which is also what happens when the sidebar's panel
    // opens beside the workspace and takes part of the row.
    cx.simulate_resize(gpui::size(gpui::px(500.), gpui::px(900.)));
    cx.run_until_parked();

    assert!(
        column_width(cx) < gpui::px(500.),
        "a column wider than the space it stands in leaves nothing for the editor"
    );
}
/// The row's power button does two jobs and must never offer the wrong one:
/// closing what is already closed, or opening what is already open.
#[gpui::test]
async fn a_closed_connection_offers_to_open_again(cx: &mut TestAppContext) {
    let (_workspace, panel, cx) = workspace_with_panel(cx).await;
    set_connections(cx, &[("a", "/tmp/a.sqlite")]);

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
