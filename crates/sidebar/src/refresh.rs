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

        let query = self.filter_editor.read(cx).text(cx);
        let collapsed = self.collapsed_projects.clone();
        // Read once, not once per row: the index is behind an `Arc`, so this is
        // a refcount bump and every worktree row below shares it.
        let sessions = self
            .session_store
            .as_ref()
            .map(|store| store.read(cx).index().clone());
        self.contents = multi_workspace.read_with(cx, |mw, cx| {
            rebuild_contents(mw, &query, &collapsed, sessions.as_ref(), cx)
        });

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
            .retain(|key, _| self.contents.entries.iter().any(|entry| entry.key() == key));

        self.sync_list_state();
        cx.notify();
    }

    /// Tells `ListState` which rows moved.
    ///
    /// `reset` would be simpler and would throw the scroll position away with
    /// the height cache -- collapsing a project near the bottom of a long list
    /// would jump the reader to the top, which reads as the click having done
    /// nothing. A project header and a worktree row are different heights, so
    /// the cache does have to be invalidated; comparing row kinds finds the one
    /// slice that actually changed.
    fn sync_list_state(&mut self) {
        let new_kinds: Vec<_> = self
            .contents
            .entries
            .iter()
            .map(std::mem::discriminant)
            .collect();

        // The two are kept in step here and nowhere else; a disagreement would
        // corrupt every splice after it.
        if self.list_state.item_count() != self.row_kinds.len() {
            self.list_state.reset(new_kinds.len());
            self.row_kinds = new_kinds;
            return;
        }

        if let Some((old_range, new_count)) = ui::utils::changed_range(&self.row_kinds, &new_kinds)
        {
            self.list_state.splice(old_range, new_count);
            self.row_kinds = new_kinds;
        }
    }

    /// Creates the shared session store the first time the panel is actually
    /// drawn, and asks it for its one sweep.
    ///
    /// Not at construction: reading the agents' histories opens every
    /// transcript on disk, and the history panel already carries the rule that
    /// none of that belongs on the startup path (`AgentHistoryPanel`'s
    /// `loaded_once`). A window whose sidebar panel is never opened must not
    /// pay for it either.
    pub(crate) fn ensure_session_store(&mut self, cx: &mut Context<Self>) {
        if self.session_store.is_some() {
            return;
        }
        let store = agent_ui::SessionStore::global(cx);
        // The sweep lands on the store, so the sidebar has to be told when it
        // does. Held in `_session_subscription`, never detached: a detached
        // observe outlives the sidebar and fires into a dropped handle.
        self._session_subscription = Some(cx.observe(&store, |sidebar, _, cx| {
            sidebar.update_entries(cx);
        }));
        store.update(cx, |store, cx| store.refresh(cx));
        self.session_store = Some(store);
    }
}
