use crate::Sidebar;
use gpui::{Context, IntoElement, Render, Window};
use ui::prelude::*;

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
        self.ensure_session_store(cx);
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
                    .p_2()
                    .gap_2()
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
