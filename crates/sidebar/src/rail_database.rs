use crate::Sidebar;
use crate::rail::{RAIL_ICON_GAP, RAIL_ICON_SIZE};
use gpui::{AnyElement, App, Context, Window};
use ui::{Tooltip, prelude::*};
use workspace::dock::DockColumn;

impl Sidebar {
    /// Whether the database column is showing.
    ///
    /// Read from the workspace's own dock rather than from the panel, so the
    /// button and the column cannot disagree about which is up.
    fn database_column_open(&self, cx: &App) -> bool {
        let Some(multi_workspace) = self.multi_workspace.upgrade() else {
            return false;
        };
        let workspace = multi_workspace.read(cx).workspace().clone();
        let workspace = workspace.read(cx);
        workspace
            .dock_for_column(DockColumn::Database)
            .is_some_and(|dock| dock.read(cx).is_open())
    }

    /// The button that opens the database column.
    ///
    /// Written out rather than enumerated the way `render_rail_panels` does,
    /// and for a concrete reason: that reads `rail_dock()`, which returns the
    /// *tool* dock on the rail's side. An own column is not in it, so no amount
    /// of `Panel::icon` would make a button appear there.
    pub(crate) fn render_rail_database(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_open = self.database_column_open(cx);

        v_flex()
            .flex_shrink_0()
            .py(RAIL_ICON_GAP)
            .items_center()
            .border_t_1()
            .border_color(cx.theme().colors().border)
            .child(
                IconButton::new("project-rail-database", IconName::DatabaseZap)
                    .icon_size(RAIL_ICON_SIZE)
                    .toggle_state(is_open)
                    .tooltip(move |_window, cx| {
                        Tooltip::for_action(
                            if is_open {
                                "Hide Database"
                            } else {
                                "Show Database"
                            },
                            &zed_actions::database::ToggleDatabase,
                            cx,
                        )
                    })
                    // Dispatch rather than reaching into the workspace: this
                    // body runs inside `Sidebar::update`, and opening a panel
                    // reaches back through the workspace. Same trap as
                    // `render_rail_footer` and `render_rail_agents`.
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(zed_actions::database::ToggleDatabase), cx)
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use crate::Sidebar;
    use crate::sidebar_tests::init_test;
    use fs::FakeFs;
    use gpui::{AppContext as _, TestAppContext};
    use project::Project;
    use workspace::MultiWorkspace;
    use workspace::dock::{DockColumn, DockPosition, test::TestPanel};

    /// Reading the workspace and its docks from inside `Sidebar::render` is the
    /// re-entrancy trap that already crashed the rail's panel toggle once, and
    /// it shows up only on a real draw -- building the element by hand misses
    /// it. `database_column_open` reaches through the workspace the same way.
    #[gpui::test]
    async fn the_rail_draws_with_the_database_column_open(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

        multi_workspace.update_in(cx, |mw, window, cx| {
            let workspace = mw.workspace().clone();
            workspace.update(cx, |workspace, cx| {
                let panel = cx.new(|cx| TestPanel::new_database(DockPosition::Left, 104, cx));
                workspace.add_panel(panel, window, cx);
                workspace
                    .dock_for_column(DockColumn::Database)
                    .expect("the database column exists")
                    .update(cx, |dock, cx| {
                        dock.show_panel(0, window, cx);
                        dock.set_open(true, window, cx);
                    });
            });

            let mw_entity = cx.entity();
            let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
            mw.register_sidebar(sidebar, cx);
        });

        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    /// The button lights from the column's own `is_open`, not from the panel,
    /// so the two cannot disagree about which is up.
    #[gpui::test]
    async fn the_button_reads_the_columns_own_state(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

        let sidebar = multi_workspace.update_in(cx, |mw, window, cx| {
            let workspace = mw.workspace().clone();
            workspace.update(cx, |workspace, cx| {
                let panel = cx.new(|cx| TestPanel::new_database(DockPosition::Left, 104, cx));
                workspace.add_panel(panel, window, cx);
            });
            let mw_entity = cx.entity();
            let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
            mw.register_sidebar(sidebar.clone(), cx);
            sidebar
        });
        cx.run_until_parked();

        sidebar.read_with(cx, |sidebar, cx| {
            assert!(
                !sidebar.database_column_open(cx),
                "a column nobody has opened must not light its button"
            );
        });

        multi_workspace.update_in(cx, |mw, window, cx| {
            mw.workspace().update(cx, |workspace, cx| {
                workspace
                    .dock_for_column(DockColumn::Database)
                    .expect("the database column exists")
                    .update(cx, |dock, cx| {
                        dock.show_panel(0, window, cx);
                        dock.set_open(true, window, cx);
                    });
            });
        });
        cx.run_until_parked();

        sidebar.read_with(cx, |sidebar, cx| {
            assert!(
                sidebar.database_column_open(cx),
                "and it must light once the column is up"
            );
        });
    }
}
