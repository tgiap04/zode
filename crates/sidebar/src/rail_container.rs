use crate::Sidebar;
use crate::rail::RAIL_ICON_SIZE;
use container_ui::ContainerPanel;
use gpui::{AnyElement, App, Context, Window};
use ui::{Tooltip, prelude::*};

impl Sidebar {
    /// Whether a container tab is open anywhere in the editor's panes.
    ///
    /// Read-only and independent of any dock: this is a pane item rather than a
    /// dock panel, so there is no dock state to consult -- only whether such a
    /// tab exists. Same shape as `agent_is_open` beside it.
    fn container_tab_open(&self, cx: &App) -> bool {
        let Some(multi_workspace) = self.multi_workspace.upgrade() else {
            return false;
        };
        let workspace = multi_workspace.read(cx).workspace().clone();
        workspace
            .read(cx)
            .items_of_type::<ContainerPanel>(cx)
            .next()
            .is_some()
    }

    /// The button that opens the container tab.
    ///
    /// Drawn beside the database button, because that is where it was asked for.
    /// Like the agent buttons, this stands for a pane item rather than a dock
    /// panel, so the wiring the panel buttons get for free from `Panel::icon` is
    /// written out here instead.
    ///
    /// A box rather than any vendor's mark: this column answers for Docker,
    /// Podman and Kubernetes, so a whale would name one of the three and mislead
    /// about the other two.
    pub(crate) fn render_rail_container(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_open = self.container_tab_open(cx);

        IconButton::new("project-rail-container", IconName::Docker)
            .icon_size(RAIL_ICON_SIZE)
            .toggle_state(is_open)
            // A right-click is the only way to go straight to a tab, and a
            // gesture with nothing naming it is a gesture nobody finds -- so the
            // tooltip says so. Same shape as `render_rail_agents`, whose
            // right-click opens an agent's terminal.
            .tooltip(move |_window, cx| {
                Tooltip::with_meta(
                    if is_open {
                        "Hide Containers"
                    } else {
                        "Show Containers"
                    },
                    Some(&zed_actions::container::ToggleContainer),
                    "Right-click to open in an editor tab",
                    cx,
                )
            })
            // Dispatch rather than reaching into the workspace: this body runs
            // inside `Sidebar::update`, and opening a panel reaches back through
            // the workspace. Same trap as `render_rail_database`.
            .on_click(|_, window, cx| {
                window.dispatch_action(Box::new(zed_actions::container::ToggleContainer), cx)
            })
            .on_right_click(|_, window, cx| {
                window.dispatch_action(Box::new(zed_actions::container::OpenInEditorTab), cx)
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use crate::Sidebar;
    use crate::sidebar_tests::init_test;
    use container_ui::ContainerPanel;
    use fs::FakeFs;
    use gpui::{AppContext as _, TestAppContext};
    use project::Project;
    use workspace::MultiWorkspace;

    /// Reading the workspace from inside `Sidebar::render` is the re-entrancy
    /// trap that already crashed the rail's panel toggle once, and it shows up
    /// only on a real draw -- building the element by hand misses it.
    #[gpui::test]
    async fn the_rail_draws_with_the_container_tab_open(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

        multi_workspace.update_in(cx, |mw, window, cx| {
            let mw_entity = cx.entity();
            let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
            mw.register_sidebar(sidebar, cx);
        });
        cx.run_until_parked();

        cx.update(|window, cx| {
            window.dispatch_action(Box::new(zed_actions::container::ToggleContainer), cx)
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    /// The button lights from whether a tab exists, so it cannot disagree with
    /// what is on screen.
    #[gpui::test]
    async fn the_container_button_reads_whether_a_tab_is_open(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

        let sidebar = multi_workspace.update_in(cx, |mw, window, cx| {
            let mw_entity = cx.entity();
            let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
            mw.register_sidebar(sidebar.clone(), cx);
            sidebar
        });
        cx.run_until_parked();

        sidebar.read_with(cx, |sidebar, cx| {
            assert!(
                !sidebar.container_tab_open(cx),
                "a tab nobody has opened must not light its button"
            );
        });

        cx.update(|window, cx| {
            window.dispatch_action(Box::new(zed_actions::container::ToggleContainer), cx)
        });
        cx.run_until_parked();

        sidebar.read_with(cx, |sidebar, cx| {
            assert!(
                sidebar.container_tab_open(cx),
                "and it must light once the tab is up -- if this fails the button \
                 is wired to something nothing handles"
            );
        });

        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
        workspace.read_with(cx, |workspace, cx| {
            assert_eq!(
                workspace.items_of_type::<ContainerPanel>(cx).count(),
                1,
                "exactly one, and it is a tab -- there is no column any more"
            );
        });
    }
}
