//! Wraps a dock [`workspace::dock::Panel`] so its constructor can be reused to
//! open a second, independent instance as an ordinary tab of this window.
//!
//! Every other tab this crate opens is already an `Item` on its own --
//! `TerminalView`, `AgentView`, `Editor`. A `Panel` is not: it implements
//! `Focusable + EventEmitter<PanelEvent> + Render`, the same first three
//! bounds `Item` asks for, but not `Item` itself, because it also carries dock
//! vocabulary (`position`, `default_size`, `set_zoomed`, ...) that an `Item`
//! has no slot for. This type supplies only what `Item` actually requires --
//! `tab_content_text`, on top of the three bounds the panel already meets --
//! and asks the panel for nothing else.

use gpui::{
    App, Context, Entity, EventEmitter, Focusable, IntoElement, Render, SharedString, Window,
};
use workspace::item::Item;

/// A dock panel, opened as a tab of this window rather than of the dock.
///
/// Two limits come with that, and both are permanent -- accepted when this
/// tier was built, not bugs to chase later:
///
/// 1. **This instance is unreachable by type.** `workspace.panel::<T>(cx)`
///    always returns the dock-registered entity, never the one held here --
///    there are 62 call sites across the workspace that resolve a panel that
///    way, including `ToggleFocus` actions, the buffer-header controls a
///    `GitPanelAddon` adds to every editor, and modals such as the process
///    launcher and the attach picker that start their session in the dock
///    copy specifically. None of them will ever see the copy opened here; a
///    user who runs an action expecting it to reach this tab will watch it
///    act on the dock panel instead, if that panel is even open.
/// 2. **Dock protocol goes unheard.** `position`, `default_size`,
///    `set_active`, `set_zoomed` and `is_zoomed` exist for the dock that
///    hosts a panel to call, and nothing here calls them -- this window has
///    no zoom state and no dock side to place a panel on. Some panels already
///    plan for exactly this: `DebugPanel::set_active` is a no-op today.
///    Nothing breaks by never calling these; they simply never fire.
pub(crate) struct PanelItem<T> {
    panel: Entity<T>,
    title: SharedString,
}

/// Never emitted. `Item` requires `EventEmitter<Self::Event>` on top of the
/// bounds the wrapped panel already carries, and this wrapper adds no new
/// events of its own to emit through it.
pub(crate) enum PanelItemEvent {}

impl<T: 'static> EventEmitter<PanelItemEvent> for PanelItem<T> {}

impl<T> PanelItem<T> {
    pub(crate) fn new(panel: Entity<T>, title: impl Into<SharedString>) -> Self {
        Self {
            panel,
            title: title.into(),
        }
    }
}

impl<T: Focusable> Focusable for PanelItem<T> {
    fn focus_handle(&self, cx: &App) -> gpui::FocusHandle {
        self.panel.focus_handle(cx)
    }
}

impl<T: Render> Render for PanelItem<T> {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.panel.clone()
    }
}

impl<T: Focusable + Render> Item for PanelItem<T> {
    type Event = PanelItemEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        self.title.clone()
    }
}
