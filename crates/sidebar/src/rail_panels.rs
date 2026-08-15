use crate::Sidebar;
use crate::rail::{RAIL_ICON_GAP, RAIL_ICON_SIZE, rail_side};
use gpui::{AnyElement, App, Context, Entity, Window};
use std::sync::Arc;
use ui::{Tooltip, prelude::*};
use workspace::SidebarSide;
use workspace::dock::{Dock, PanelHandle};

impl Sidebar {
    /// The dock the rail's panel buttons stand for: the one on the rail's own
    /// side. A button parked at the right edge that opens a panel at the left
    /// edge is the whole screen apart from what it did. The bottom dock keeps
    /// its own buttons in the status bar -- the rail stands for the primary
    /// sidebar, not for every dock.
    fn rail_dock(&self, cx: &App) -> Option<Entity<Dock>> {
        let multi_workspace = self.multi_workspace.upgrade()?;
        let workspace = multi_workspace.read(cx).workspace().clone();
        let workspace = workspace.read(cx);
        Some(match rail_side(cx) {
            SidebarSide::Left => workspace.left_dock().clone(),
            SidebarSide::Right => workspace.right_dock().clone(),
        })
    }

    /// The panels the rail draws buttons for. Kept apart from the rendering so a
    /// test can pin down *which* dock feeds the rail without having to take an
    /// element tree apart.
    fn rail_panels(&self, cx: &App) -> Vec<Arc<dyn PanelHandle>> {
        let Some(dock) = self.rail_dock(cx) else {
            return Vec::new();
        };
        let position = dock.read(cx).position();
        let side = rail_side(cx);

        // Same predicate the status bar consults, so the two can never both claim
        // a panel or both drop one. `rail_drawn: true` is a statement of fact
        // rather than a lookup: this runs only from the rail's own render, so the
        // rail demonstrably exists.
        dock.read(cx)
            .panels()
            .filter(|panel| {
                workspace::dock::rail_draws_panel(panel.persistent_name(), position, side, true)
            })
            .cloned()
            .collect()
    }

    /// VS Code's activity bar, folded into the project rail rather than given a
    /// strip of its own — two 48px columns side by side would cost nearly a
    /// hundred pixels of width for one row of icons.
    ///
    /// Entries come straight from `Panel::icon`, so a panel registered later
    /// turns up here without this module knowing it exists. Panels only: VS Code
    /// also lists Search and Extensions, which in Zed are pane items rather than
    /// panels and would each need wiring of their own.
    pub(crate) fn render_rail_panels(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let panels = self.rail_panels(cx);
        if panels.is_empty() {
            return None;
        }

        let dock = self.rail_dock(cx)?;

        let mut buttons = Vec::with_capacity(panels.len());
        for panel in panels.iter() {
            let Some(icon) = panel.icon(window, cx) else {
                continue;
            };
            // Whether *this* panel is up, not whether it is the one the dock
            // calls active — several can be showing at once, so several
            // buttons can be lit at once.
            let is_showing = dock.read(cx).is_panel_visible(panel.panel_id());

            let Some(name) = panel.icon_tooltip(window, cx) else {
                continue;
            };
            let tooltip = if is_showing { "Hide Panel" } else { name };

            let button = IconButton::new(panel.persistent_name(), icon)
                .icon_size(RAIL_ICON_SIZE)
                .toggle_state(is_showing);

            buttons.push(if is_showing {
                // Takes this panel out of the stack and leaves the rest —
                // the dock's own toggle action would take them all down with
                // it. Driven through the dock entity rather than an action
                // because there is no action naming a single panel, and the
                // closure holds no borrow of the sidebar.
                //
                // Safe here, and equally safe in the branch below: what the
                // comment there warns about is running *inside*
                // `Sidebar::update` while building the element, and a click
                // handler runs long after that. The show branch dispatches
                // because an action already names the panel it wants, not
                // because reaching the dock from here would be a problem.
                let dock = dock.clone();
                let panel_id = panel.panel_id();
                button
                    .tooltip(move |_window, cx| Tooltip::simple(tooltip, cx))
                    .on_click(move |_, window, cx| {
                        dock.update(cx, |dock, cx| {
                            dock.hide_panel_by_id(panel_id, window, cx);
                        });
                    })
            } else {
                let action = panel.toggle_action(window, cx);
                button
                    .tooltip({
                        let action = action.boxed_clone();
                        move |_window, cx| Tooltip::for_action(tooltip, &*action, cx)
                    })
                    // Dispatch rather than driving the dock directly: this body
                    // runs inside `Sidebar::update`, and showing a panel reaches
                    // back into the workspace. See `render_rail_footer`.
                    .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
            });
        }

        if buttons.is_empty() {
            return None;
        }

        Some(
            v_flex()
                .flex_shrink_0()
                .py(RAIL_ICON_GAP)
                .gap(RAIL_ICON_GAP)
                .items_center()
                .border_t_1()
                .border_color(cx.theme().colors().border)
                .children(buttons)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::Sidebar;
    use crate::sidebar_tests::init_test;
    use fs::FakeFs;
    use gpui::{AppContext as _, TestAppContext, UpdateGlobal as _};
    use project::Project;
    use workspace::MultiWorkspace;
    use workspace::SidebarSide;
    use workspace::dock::DockPosition;
    use workspace::dock::test::TestPanel;

    /// The rail lights a button per panel that is up, so two stacked panels
    /// light two buttons — the single-active-index reading it used before could
    /// only ever light one.
    #[gpui::test]
    async fn the_rail_lights_every_panel_that_is_showing(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

        workspace.update_in(cx, |workspace, window, cx| {
            for priority in [100, 101] {
                let panel = cx.new(|cx| TestPanel::new(DockPosition::Left, priority, cx));
                workspace.add_panel(panel, window, cx);
            }
            workspace.left_dock().update(cx, |dock, cx| {
                dock.show_panel(0, window, cx);
                dock.show_panel(1, window, cx);
            });
        });

        multi_workspace.update_in(cx, |mw, window, cx| {
            let mw_entity = cx.entity();
            let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
            mw.register_sidebar(sidebar, cx);
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        let lit = workspace.read_with(cx, |workspace, cx| {
            let dock = workspace.left_dock().read(cx);
            dock.panels()
                .filter(|panel| dock.is_panel_visible(panel.panel_id()))
                .count()
        });
        assert_eq!(
            lit, 2,
            "both stacked panels should read as showing, so both rail buttons light"
        );
    }

    /// The rail toggles the dock it stands beside, so the same set of panels has
    /// to yield different buttons depending on which edge it is parked at. A
    /// button on one edge opening a panel on the other is the width of the screen
    /// between cause and effect. The bottom dock stays out of it either way.
    #[gpui::test]
    async fn the_rail_lists_the_dock_on_its_own_side(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

        let sidebar = multi_workspace.update_in(cx, |mw, window, cx| {
            let workspace = mw.workspace().clone();
            workspace.update(cx, |workspace, cx| {
                for (index, position) in [
                    DockPosition::Left,
                    DockPosition::Left,
                    DockPosition::Right,
                    DockPosition::Bottom,
                ]
                .iter()
                .enumerate()
                {
                    let panel = cx.new(|cx| {
                        let mut panel = TestPanel::new(*position, 100 + index as u32, cx);
                        // A panel with no icon contributes no button at all, so
                        // without this the assertions below would pass on an
                        // empty rail regardless of which dock was read.
                        panel.icon = Some(ui::IconName::FileTree);
                        panel
                    });
                    workspace.add_panel(panel, window, cx);
                }
            });

            let mw_entity = cx.entity();
            let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
            mw.register_sidebar(sidebar.clone(), cx);
            sidebar
        });

        for (side, expected) in [(SidebarSide::Left, 2), (SidebarSide::Right, 1)] {
            cx.update(|_, cx| {
                settings::SettingsStore::update_global(cx, |settings, cx| {
                    settings.update_user_settings(cx, |settings| {
                        settings
                            .workspace
                            .multi_project
                            .get_or_insert_default()
                            .sidebar_side = Some(side);
                    });
                });
            });

            sidebar.read_with(cx, |sidebar, cx| {
                assert_eq!(
                    sidebar.rail_panels(cx).len(),
                    expected,
                    "a {side:?} rail must list the panels docked {side:?}"
                );
            });
        }
    }

    /// Reading the workspace and its dock from inside `Sidebar::render` is the
    /// re-entrancy trap that already crashed the rail's panel toggle once, and
    /// it only shows up on a real draw — building the element by hand misses it.
    #[gpui::test]
    async fn rail_draws_with_panels_registered(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

        multi_workspace.update_in(cx, |mw, window, cx| {
            let workspace = mw.workspace().clone();
            workspace.update(cx, |workspace, cx| {
                let panel = cx.new(|cx| {
                    let mut panel = TestPanel::new(DockPosition::Left, 100, cx);
                    panel.icon = Some(ui::IconName::FileTree);
                    panel
                });
                workspace.add_panel(panel, window, cx);
            });

            let mw_entity = cx.entity();
            let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
            mw.register_sidebar(sidebar, cx);
        });

        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }
}
