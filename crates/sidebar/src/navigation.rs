use crate::{FocusSidebarFilter, Sidebar};
use gpui::{App, Context, Focusable, KeyContext, Window};
use menu::{Cancel, Confirm, SelectFirst, SelectLast, SelectNext, SelectPrevious};
use project::ProjectGroupKey;

impl Sidebar {
    pub(crate) fn select_first_entry(&mut self) {
        self.selection = if self.contents.entries.is_empty() {
            None
        } else {
            Some(0)
        };
    }

    fn selected_group_key(&self) -> Option<ProjectGroupKey> {
        let ix = self.selection?;
        Some(self.contents.entries.get(ix)?.key.clone())
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

    pub(crate) fn focus_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .filter_editor
            .focus_handle(cx)
            .contains_focused(window, cx)
        {
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
        let Some(key) = self.selected_group_key() else {
            return;
        };
        self.activate_or_open_workspace_for_group(&key, window, cx);
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
