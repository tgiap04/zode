use crate::project_list::PanelRow;
use crate::{FocusSidebarFilter, Sidebar};
use gpui::{App, Context, Focusable, KeyContext, Window};
use menu::{Cancel, Confirm, SelectFirst, SelectLast, SelectNext, SelectPrevious};

impl Sidebar {
    pub(crate) fn select_first_entry(&mut self) {
        self.selection = if self.contents.entries.is_empty() {
            None
        } else {
            Some(0)
        };
    }

    pub(crate) fn dispatch_context(&self, window: &Window, cx: &Context<Self>) -> KeyContext {
        let mut context = KeyContext::new_with_defaults();
        context.add("Sidebar");
        if self
            .filter_editor
            .focus_handle(cx)
            .contains_focused(window, cx)
        {
            context.add("menu");
        }
        context
    }

    /// Sends focus on to the filter editor when the sidebar itself is focused.
    ///
    /// Only when the sidebar's *own* handle is the one focused. `on_focus_in`
    /// fires for the whole subtree, so focus arriving at something inside the
    /// sidebar already has an owner — and taking it away from that owner was
    /// dismissing a project's context menu the instant it opened: the menu is
    /// focused two frames after it appears, that focus counted as the sidebar
    /// gaining focus, this method handed it to the filter editor, and
    /// `ContextMenu` cancels itself on blur. The menu drew for one frame and
    /// died, which reads as a menu that never opened.
    pub(crate) fn focus_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus_handle.is_focused(window) {
            return;
        }
        self.filter_editor.update(cx, |editor, cx| {
            editor.focus_handle(cx).focus(window, cx);
        });
    }

    pub(crate) fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_filter_query(cx) {
            self.reset_filter_editor_text(window, cx);
            return;
        }
        self.selection = None;
        cx.notify();
    }

    pub(crate) fn focus_sidebar_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_editor.update(cx, |editor, cx| {
            editor.focus_handle(cx).focus(window, cx);
            editor.select_all(&Default::default(), window, cx);
        });
    }

    pub(crate) fn reset_filter_editor_text(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.filter_editor.update(cx, |editor, cx| {
            if editor.text(cx).is_empty() {
                false
            } else {
                editor.clear(window, cx);
                true
            }
        })
    }

    pub(crate) fn has_filter_query(&self, cx: &App) -> bool {
        !self.filter_editor.read(cx).text(cx).is_empty()
    }

    pub(crate) fn select_next(
        &mut self,
        _: &SelectNext,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let len = self.contents.entries.len();
        if len == 0 {
            return;
        }
        self.selection = Some(self.selection.map_or(0, |ix| (ix + 1).min(len - 1)));
        cx.notify();
    }

    pub(crate) fn select_previous(
        &mut self,
        _: &SelectPrevious,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.contents.entries.is_empty() {
            return;
        }
        self.selection = Some(self.selection.map_or(0, |ix| ix.saturating_sub(1)));
        cx.notify();
    }

    pub(crate) fn select_first(
        &mut self,
        _: &SelectFirst,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_first_entry();
        cx.notify();
    }

    pub(crate) fn select_last(
        &mut self,
        _: &SelectLast,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selection = self.contents.entries.len().checked_sub(1);
        cx.notify();
    }

    pub(crate) fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        // A worktree row names one workspace; the project row above it names a
        // group. Sending both through the group would activate whichever
        // workspace was last used there, which is exactly not what pressing
        // Enter on a particular worktree asks for.
        let Some(row) = self
            .selection
            .and_then(|ix| self.contents.entries.get(ix))
            .cloned()
        else {
            return;
        };
        match row {
            PanelRow::Project(entry) => {
                self.activate_or_open_workspace_for_group(&entry.key, window, cx)
            }
            PanelRow::Worktree(row) => self.activate_worktree(&row.workspace, window, cx),
            PanelRow::Agent(row) => self.open_agent_row(&row, window, cx),
        }
    }

    pub(crate) fn on_focus_sidebar_filter(
        &mut self,
        _: &FocusSidebarFilter,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_sidebar_filter(window, cx);
    }
}
