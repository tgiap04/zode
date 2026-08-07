use crate::Sidebar;
use crate::project_list::rebuild_contents;
use gpui::Context;
use workspace::SidebarEvent;

impl Sidebar {
    pub(crate) fn serialize(&mut self, cx: &mut Context<Self>) {
        cx.emit(SidebarEvent::SerializeNeeded);
    }

    pub(crate) fn update_entries(&mut self, cx: &mut Context<Self>) {
        let Some(multi_workspace) = self.multi_workspace.upgrade() else {
            return;
        };
        if !multi_workspace.read(cx).multi_workspace_enabled(cx) {
            return;
        }
        self.resync_project_activity_subscriptions(&multi_workspace, cx);

        let scroll_position = self.list_state.logical_scroll_top();
        let query = self.filter_editor.read(cx).text(cx);
        self.contents = multi_workspace.read_with(cx, |mw, cx| rebuild_contents(mw, &query, cx));

        // The entry list can shrink (a project closes, or a filter narrows
        // it) without any navigation method running in between, so a
        // stale `selection` past the new end must be pulled back in
        // bounds here rather than relying on `select_next`/`select_previous`
        // to eventually clamp it themselves.
        if self
            .selection
            .is_some_and(|ix| ix >= self.contents.entries.len())
        {
            self.selection = self.contents.entries.len().checked_sub(1);
        }

        self.project_header_menu_handles
            .retain(|key, _| self.contents.entries.iter().any(|entry| &entry.key == key));

        self.list_state.reset(self.contents.entries.len());
        self.list_state.scroll_to(scroll_position);
        cx.notify();
    }
}
