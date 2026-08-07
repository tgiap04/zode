use crate::Sidebar;
use gpui::{Context, IntoElement, Render, Window};
use ui::prelude::*;

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel_open = self.panel_open(cx);
        let rail = self.render_rail(cx);

        h_flex()
            .key_context(self.dispatch_context(window, cx))
            .track_focus(&self.focus_handle)
            .size_full()
            .pt(self.top_inset(window, cx))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::select_first))
            .on_action(cx.listener(Self::select_last))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::on_focus_sidebar_filter))
            .child(rail)
            .when(panel_open, |this| this.child(self.render_panel(cx)))
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
