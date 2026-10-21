use crate::Sidebar;
use gpui::{Context, IntoElement, Render, Window};
use ui::{Tab, prelude::*};

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel_open = self.panel_open(cx);
        let rail = self.render_rail(window, cx);
        // The rail is the outermost column on whichever edge the sidebar stands
        // against -- VS Code's activity bar sits beyond its sidebar, not between
        // the sidebar and the editor. Fixed rail-then-panel order gets that right
        // on the left and mirrors it wrongly on the right, which is the shipped
        // default, so the panel would open on the far side of the rail from the
        // editor it belongs to.
        let panel = panel_open.then(|| self.render_panel(cx).into_any_element());

        h_flex()
            .key_context(self.dispatch_context(window, cx))
            .track_focus(&self.focus_handle)
            .size_full()
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::select_first))
            .on_action(cx.listener(Self::select_last))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::on_focus_sidebar_filter))
            .child(rail)
            .children(panel)
    }
}

impl Sidebar {
    /// The wide project list: filter input plus one named row per project.
    /// Shown only while `MultiWorkspace::sidebar_open` is set; the rail
    /// beside it is always visible.
    fn render_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entry_count = self.contents.entries.len();
        let has_open_projects = self.contents.has_open_projects;
        let has_query = self.has_filter_query(cx);

        v_flex()
            .debug_selector(|| "project-list-panel".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                h_flex()
                    .debug_selector(|| "project-list-header".into())
                    .flex_none()
                    .w_full()
                    .h(Tab::container_height(cx))
                    .px_2()
                    .gap_1()
                    .border_b_1()
                    .border_color(cx.theme().colors().border)
                    .child(self.render_filter_input(cx))
                    .child(self.render_recent_projects_button(cx)),
            )
            .child(if entry_count == 0 {
                if has_open_projects || has_query {
                    self.render_no_results(cx).into_any_element()
                } else {
                    self.render_empty_state(cx).into_any_element()
                }
            } else {
                gpui::list(
                    self.list_state.clone(),
                    cx.processor(|this, ix, window, cx| this.render_list_entry(ix, window, cx)),
                )
                .size_full()
                .into_any_element()
            })
    }
}

#[cfg(test)]
mod tests {
    use crate::Sidebar;
    use crate::sidebar_tests::init_test;
    use fs::FakeFs;
    use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
    use project::Project;
    use ui::Tab;
    use workspace::MultiWorkspace;

    async fn open_panel(cx: &mut TestAppContext) -> (Entity<Sidebar>, &mut VisualTestContext) {
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
        multi_workspace.update(cx, |mw, cx| mw.open_sidebar(cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        (sidebar, cx)
    }

    fn type_into_filter(sidebar: &Entity<Sidebar>, text: &str, cx: &mut VisualTestContext) {
        sidebar.update_in(cx, |sidebar, window, cx| {
            sidebar
                .filter_editor
                .update(cx, |editor, cx| editor.set_text(text, window, cx));
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    /// The row of panel headers beside it -- the editor's tab bar, the dock
    /// sections -- is `Tab::container_height`, so the filter row is too.
    #[gpui::test]
    async fn the_filter_row_is_as_tall_as_a_panel_header(cx: &mut TestAppContext) {
        let (_sidebar, cx) = open_panel(cx).await;
        let header = cx
            .debug_bounds("project-list-header")
            .expect("the panel must draw its header");
        let expected = cx.update(|_, cx| Tab::container_height(cx));
        assert_eq!(header.size.height, expected);
    }

    /// The clear button is in the row it clears. Stacked under the field it
    /// would be cut off by a header of fixed height.
    #[gpui::test]
    async fn the_clear_button_stays_inside_the_header(cx: &mut TestAppContext) {
        let (sidebar, cx) = open_panel(cx).await;
        assert!(
            cx.debug_bounds("ICON-Close").is_none(),
            "nothing to clear before anything is typed"
        );

        type_into_filter(&sidebar, "no project is called this", cx);

        let header = cx
            .debug_bounds("project-list-header")
            .expect("the panel must draw its header");
        let clear = cx
            .debug_bounds("ICON-Close")
            .expect("a filter with text must offer to clear it");
        assert!(
            header.contains(&clear.origin) && header.contains(&clear.bottom_right()),
            "clear {clear:?} must sit inside the header {header:?}"
        );
    }

    /// With nothing matching, the panel says so below the header rather than
    /// instead of it, and the header keeps its height.
    #[gpui::test]
    async fn no_results_draws_under_an_intact_header(cx: &mut TestAppContext) {
        let (sidebar, cx) = open_panel(cx).await;
        type_into_filter(&sidebar, "no project is called this", cx);

        let header = cx
            .debug_bounds("project-list-header")
            .expect("the panel must draw its header");
        let empty = cx
            .debug_bounds("project-list-empty")
            .expect("a filter that matches nothing must say so");
        assert!(
            empty.origin.y >= header.bottom(),
            "the empty state {empty:?} starts below the header {header:?}"
        );
        let expected = cx.update(|_, cx| Tab::container_height(cx));
        assert_eq!(header.size.height, expected);
    }
}
