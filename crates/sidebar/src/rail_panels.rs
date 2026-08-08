use crate::Sidebar;
use crate::rail::{RAIL_ICON_GAP, RAIL_ICON_SIZE};
use gpui::{AnyElement, App, Context, Entity, Window};
use std::sync::Arc;
use ui::{Tooltip, prelude::*};
use settings::Settings as _;
use workspace::dock::{Dock, PanelHandle};
use workspace::{SidebarSide, WorkspaceSettings};

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
        Some(match WorkspaceSettings::get_global(cx).multi_project.sidebar_side {
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
        let side = WorkspaceSettings::get_global(cx).multi_project.sidebar_side;

        // Same predicate the status bar consults, so the two can never both claim
        // a panel or both drop one.
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

        let (is_open, active_index, close_dock) = {
            let dock = self.rail_dock(cx)?;
            let dock = dock.read(cx);
            (
                dock.is_open(),
                dock.active_panel_index(),
                dock.toggle_action(),
            )
        };

        let mut buttons = Vec::with_capacity(panels.len());
        for (index, panel) in panels.iter().enumerate() {
            let Some(icon) = panel.icon(window, cx) else {
                continue;
            };
            let is_active = is_open && Some(index) == active_index;

            // Clicking the panel already on screen closes the dock, the way VS
            // Code does; clicking any other one brings it forward.
            let (action, tooltip) = if is_active {
                (close_dock.boxed_clone(), "Hide Panel")
            } else {
                let Some(tooltip) = panel.icon_tooltip(window, cx) else {
                    continue;
                };
                (panel.toggle_action(window, cx), tooltip)
            };

            buttons.push(
                IconButton::new(panel.persistent_name(), icon)
                    .icon_size(RAIL_ICON_SIZE)
                    .toggle_state(is_active)
                    .tooltip({
                        let action = action.boxed_clone();
                        move |_window, cx| Tooltip::for_action(tooltip, &*action, cx)
                    })
                    // Dispatch rather than driving the dock directly: this body
                    // runs inside `Sidebar::update`, and toggling a panel reaches
                    // back into the workspace. See `render_rail_footer`.
                    .on_click(move |_, window, cx| {
                        window.dispatch_action(action.boxed_clone(), cx)
                    }),
            );
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
