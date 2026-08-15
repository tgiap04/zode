mod activity_watch;
mod chrome;
#[cfg(test)]
mod contents_tests;
mod context_menu;
mod navigation;
#[cfg(test)]
mod navigation_tests;
mod project_item;
mod project_list;
mod rail;
mod rail_agents;
mod rail_panels;
mod refresh;
mod render;
mod serialization;
#[cfg(test)]
mod sidebar_tests;
mod workspace_actions;

use crate::project_list::SidebarContents;
use collections::HashMap;
use gpui::{
    App, Context, Entity, EntityId, EventEmitter, FocusHandle, Focusable, ListState, Pixels,
    Subscription, Window, actions, px,
};
use project::ProjectGroupKey;
use recent_projects::sidebar_recent_projects::SidebarRecentProjects;
use ui::{ContextMenu, PopoverMenuHandle, prelude::*};
use workspace::{
    MultiWorkspace, MultiWorkspaceEvent, Sidebar as WorkspaceSidebar, SidebarEvent, SidebarSide,
};

const DEFAULT_WIDTH: Pixels = px(300.0);

actions!(
    sidebar,
    [
        /// Moves focus to the sidebar's search/filter editor.
        FocusSidebarFilter,
    ]
);

/// The sidebar re-derives its entire entry list from scratch on every
/// change via `update_entries` -> `rebuild_contents`. Avoid adding
/// incremental or inter-event coordination state -- if something can be
/// computed from the current world state, compute it in the rebuild.
pub struct Sidebar {
    pub(crate) multi_workspace: gpui::WeakEntity<MultiWorkspace>,
    width: Pixels,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) filter_editor: Entity<editor::Editor>,
    list_state: ListState,
    pub(crate) contents: SidebarContents,
    /// The index of the list item that currently has the keyboard focus.
    /// Not the same as which project is active.
    pub(crate) selection: Option<usize>,
    pub(crate) recent_projects_popover_handle: PopoverMenuHandle<SidebarRecentProjects>,
    /// Keyed by `ProjectGroupKey` rather than list index -- an index would
    /// go stale the moment the entry list reorders or shrinks while a menu
    /// is open, since `PopoverMenu`'s own internal open/closed state is
    /// keyed by its `ElementId`, which is derived from this same key (see
    /// `context_menu.rs`).
    pub(crate) project_header_menu_handles:
        HashMap<ProjectGroupKey, PopoverMenuHandle<ContextMenu>>,
    /// One subscription per currently-open project's `Event::ActivityChanged`
    /// (FR7), kept in sync with the live project set by
    /// `resync_project_activity_subscriptions`. Without this, a project
    /// waking or hibernating in the background would never refresh
    /// `contents` -- `MultiWorkspaceEvent` alone doesn't fire for it.
    pub(crate) project_activity_subscriptions: HashMap<EntityId, Subscription>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl Sidebar {
    pub fn new(
        multi_workspace: Entity<MultiWorkspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        cx.on_focus_in(&focus_handle, window, Self::focus_in)
            .detach();

        let filter_editor = cx.new(|cx| {
            let mut editor = editor::Editor::single_line(window, cx);
            editor.set_placeholder_text("Search…", window, cx);
            editor
        });

        let mut subscriptions = vec![cx.subscribe_in(
            &multi_workspace,
            window,
            |this, _multi_workspace, event: &MultiWorkspaceEvent, _window, cx| match event {
                MultiWorkspaceEvent::ActiveWorkspaceChanged { .. }
                | MultiWorkspaceEvent::WorkspaceAdded(_)
                | MultiWorkspaceEvent::WorkspaceRemoved(_)
                | MultiWorkspaceEvent::ProjectGroupsChanged => {
                    this.update_entries(cx);
                }
            },
        )];

        subscriptions.push(
            cx.subscribe(&filter_editor, |this: &mut Self, _, event, cx| {
                if let editor::EditorEvent::BufferEdited = event {
                    let query = this.filter_editor.read(cx).text(cx);
                    if !query.is_empty() {
                        this.selection.take();
                    }
                    this.update_entries(cx);
                    if !query.is_empty() {
                        this.select_first_entry();
                    }
                }
            }),
        );

        cx.defer_in(window, move |this, _window, cx| {
            this.update_entries(cx);
        });

        Self {
            multi_workspace: multi_workspace.downgrade(),
            width: DEFAULT_WIDTH,
            focus_handle,
            filter_editor,
            list_state: ListState::new(0, gpui::ListAlignment::Top, px(1000.)),
            contents: SidebarContents::default(),
            selection: None,
            recent_projects_popover_handle: PopoverMenuHandle::default(),
            project_header_menu_handles: HashMap::default(),
            project_activity_subscriptions: HashMap::default(),
            _subscriptions: subscriptions,
        }
    }
}

impl EventEmitter<SidebarEvent> for Sidebar {}

impl Focusable for Sidebar {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl WorkspaceSidebar for Sidebar {
    fn width(&self, _cx: &App) -> Pixels {
        self.width
    }

    fn set_width(&mut self, width: Option<Pixels>, cx: &mut Context<Self>) {
        self.width = width.unwrap_or(DEFAULT_WIDTH);
        self.serialize(cx);
        cx.notify();
    }

    fn rail_width(&self, _cx: &App) -> Pixels {
        crate::rail::RAIL_WIDTH
    }

    fn has_notifications(&self, _cx: &App) -> bool {
        false
    }

    fn side(&self, cx: &App) -> SidebarSide {
        crate::rail::rail_side(cx)
    }

    fn prepare_for_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_in(window, cx);
    }

    fn serialized_state(&self, _cx: &App) -> Option<String> {
        self.serialize_to_string()
    }

    fn restore_serialized_state(
        &mut self,
        state: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_serialized_state(state, cx);
    }
}
