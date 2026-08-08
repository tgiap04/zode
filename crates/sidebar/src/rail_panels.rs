use crate::Sidebar;
use gpui::{AnyElement, App, Context, Entity, Window};
use std::sync::Arc;
use ui::{Tooltip, prelude::*};
use workspace::dock::{Dock, PanelHandle};

impl Sidebar {
    /// The dock the rail's panel buttons stand for. VS Code hangs its activity
    /// bar off the primary sidebar alone, so the right and bottom docks keep
    /// their own buttons in the status bar and stay out of the rail.
    fn rail_dock(&self, cx: &App) -> Option<Entity<Dock>> {
        let multi_workspace = self.multi_workspace.upgrade()?;
        let workspace = multi_workspace.read(cx).workspace().clone();
        Some(workspace.read(cx).left_dock().clone())
    }

    /// The panels the rail draws buttons for. Kept apart from the rendering so a
    /// test can pin down *which* dock feeds the rail without having to take an
    /// element tree apart.
    fn rail_panels(&self, cx: &App) -> Vec<Arc<dyn PanelHandle>> {
        let Some(dock) = self.rail_dock(cx) else {
            return Vec::new();
        };
        dock.read(cx).panels().cloned().collect()
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
                    .icon_size(IconSize::Small)
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
                .py_1()
                .gap_1()
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
    use gpui::{AppContext as _, TestAppContext};
    use project::Project;
    use workspace::MultiWorkspace;
    use workspace::dock::DockPosition;
    use workspace::dock::test::TestPanel;

    /// The rail hangs off the left dock alone. Without that, panels docked right
    /// or along the bottom would double up as rail buttons.
    #[gpui::test]
    async fn rail_lists_left_dock_panels_only(cx: &mut TestAppContext) {
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
                        // without this the assertion below would pass on an
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

        sidebar.read_with(cx, |sidebar, cx| {
            assert_eq!(
                sidebar.rail_panels(cx).len(),
                2,
                "only the two left-docked panels belong in the rail"
            );
        });
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
