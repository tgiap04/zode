use crate::Sidebar;
use gpui::{Context, IntoElement, Render, Window};
use ui::prelude::*;

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entry_count = self.contents.entries.len();
        let has_open_projects = self.contents.has_open_projects;
        let has_query = self.has_filter_query(cx);

        v_flex()
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
