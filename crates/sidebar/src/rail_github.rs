use crate::Sidebar;
use crate::rail::rail_button;
use github_ui::{GithubPanelSettings, GithubView};
use gpui::{AnyElement, App, Context, Window};
use settings::Settings as _;
use ui::{Tooltip, prelude::*};

impl Sidebar {
    /// Whether a GitHub tab is open anywhere in the editor's panes. A pane
    /// item, not a dock panel, so there is no dock state to read.
    pub(crate) fn github_tab_open(&self, cx: &App) -> bool {
        let Some(multi_workspace) = self.multi_workspace.upgrade() else {
            return false;
        };
        let workspace = multi_workspace.read(cx).workspace().clone();
        workspace
            .read(cx)
            .items_of_type::<GithubView>(cx)
            .next()
            .is_some()
    }

    pub(crate) fn shows_github_button(cx: &App) -> bool {
        GithubPanelSettings::get_global(cx).button
    }

    /// The button that opens the GitHub tab, beside the database and
    /// container buttons. Wired by hand like theirs, since `Panel::icon` only
    /// does this for dock panels.
    pub(crate) fn render_rail_github(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_open = self.github_tab_open(cx);

        rail_button("project-rail-github", IconName::Github)
            .toggle_state(is_open)
            .tooltip(move |_window, cx| {
                Tooltip::for_action(
                    if is_open {
                        "Hide GitHub"
                    } else {
                        "Show GitHub"
                    },
                    &zode_actions::github::Toggle,
                    cx,
                )
            })
            // Dispatch rather than reaching into the workspace: this body runs
            // inside `Sidebar::update`, and opening a tab reaches back through
            // the workspace. Same trap as `render_rail_database`.
            .on_click(|_, window, cx| {
                window.dispatch_action(Box::new(zode_actions::github::Toggle), cx)
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use crate::Sidebar;
    use crate::sidebar_tests::init_test;
    use fs::FakeFs;
    use github_ui::GithubView;
    use gpui::{AppContext as _, TestAppContext, UpdateGlobal as _};
    use project::Project;
    use settings::SettingsStore;
    use workspace::MultiWorkspace;

    /// Reading the workspace from inside `Sidebar::render` is the re-entrancy
    /// trap that crashed the rail once; only a real draw shows it. The tab is
    /// opened through the action, so a button wired to nothing fails here.
    #[gpui::test]
    async fn the_github_button_lights_once_the_tab_is_open(cx: &mut TestAppContext) {
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
        sidebar.read_with(cx, |sidebar, cx| assert!(!sidebar.github_tab_open(cx)));

        cx.update(|window, cx| window.dispatch_action(Box::new(zode_actions::github::Toggle), cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        sidebar.read_with(cx, |sidebar, cx| assert!(sidebar.github_tab_open(cx)));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
        workspace.read_with(cx, |workspace, cx| {
            assert_eq!(workspace.items_of_type::<GithubView>(cx).count(), 1);
        });
    }

    #[gpui::test]
    fn the_github_button_follows_its_setting(cx: &mut TestAppContext) {
        init_test(cx);
        cx.update(|cx| {
            assert!(Sidebar::shows_github_button(cx), "on by default");
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |settings| {
                    settings.github_panel.get_or_insert_default().button = Some(false);
                });
            });
            assert!(!Sidebar::shows_github_button(cx));
        });
    }
}
