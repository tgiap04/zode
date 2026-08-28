use crate::focus_follows_mouse::FocusFollowsMouse as _;
use crate::pane_group::element::pane_axis;
use crate::pane_group::{SURFACE_MARGIN, SURFACE_ROUNDING};
use crate::persistence::model::DockData;
use crate::{DraggedDock, Event, FocusFollowsMouse, ModalLayer, Pane, WorkspaceSettings};
use crate::{Workspace, status_bar::StatusItemView};
use anyhow::Context as _;
use client::proto;
use db::kvp::KeyValueStore;

use gpui::{
    Action, Anchor, AnyView, App, Axis, Context, Entity, EntityId, EventEmitter, FocusHandle,
    Focusable, IntoElement, KeyContext, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement,
    Render, SharedString, StyleRefinement, Styled, Subscription, WeakEntity, Window, deferred, div,
    px,
};
use gpui::{Bounds, Pixels};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use settings::{Settings, SettingsStore};
use std::sync::Arc;
use ui::{
    ContextMenu, CountBadge, Divider, DividerColor, IconButton, Tooltip, prelude::*,
    right_click_menu,
};
use util::ResultExt as _;

pub(crate) const RESIZE_HANDLE_SIZE: Pixels = px(6.);

/// Element-id space for a dock's stack, kept clear of the centre pane group's
/// bases (which start at 0 and grow by `(basis + ix) * 10` as it nests).
const STACK_ELEMENT_BASIS: usize = 1_000_000;

pub enum PanelEvent {
    ZoomIn,
    ZoomOut,
    Activate,
    Close,
}

pub use proto::PanelId;

pub trait Panel: Focusable + EventEmitter<PanelEvent> + Render + Sized {
    fn persistent_name() -> &'static str;
    fn panel_key() -> &'static str;
    fn position(&self, window: &Window, cx: &App) -> DockPosition;
    fn position_is_valid(&self, position: DockPosition) -> bool;
    fn set_position(&mut self, position: DockPosition, window: &mut Window, cx: &mut Context<Self>);
    fn default_size(&self, window: &Window, cx: &App) -> Pixels;
    fn min_size(&self, _window: &Window, _cx: &App) -> Option<Pixels> {
        None
    }
    fn initial_size_state(&self, _window: &Window, _cx: &App) -> PanelSizeState {
        PanelSizeState::default()
    }
    fn size_state_changed(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn supports_flexible_size(&self) -> bool {
        false
    }
    fn has_flexible_size(&self, _window: &Window, _cx: &App) -> bool {
        false
    }
    fn set_flexible_size(
        &mut self,
        _flexible: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
    fn icon(&self, window: &Window, cx: &App) -> Option<ui::IconName>;
    fn icon_tooltip(&self, window: &Window, cx: &App) -> Option<&'static str>;
    fn toggle_action(&self) -> Box<dyn Action>;
    fn icon_label(&self, _window: &Window, _: &App) -> Option<String> {
        None
    }
    fn is_zoomed(&self, _window: &Window, _cx: &App) -> bool {
        false
    }
    /// Whether this panel wants the whole window: the centre, the docks, and
    /// the rail beside them.
    ///
    /// Not `is_zoomed`: that drives an overlay whose bookkeeping is keyed by
    /// `DockPosition`, which an own column shares with the tool dock beside it,
    /// and it draws inside the `Workspace` element -- so it could not cover the
    /// rail, which `MultiWorkspace` draws as its sibling.
    ///
    /// Read only, never written to, so a panel answering it cannot reach back
    /// into the workspace mid-update.
    fn fills_the_window(&self, _window: &Window, _cx: &App) -> bool {
        false
    }
    fn starts_open(&self, _window: &Window, _cx: &App) -> bool {
        false
    }
    fn set_zoomed(&mut self, _zoomed: bool, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn set_active(&mut self, _active: bool, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn pane(&self) -> Option<Entity<Pane>> {
        None
    }
    fn remote_id() -> Option<proto::PanelId> {
        None
    }
    fn activation_priority(&self) -> u32;
    fn enabled(&self, _cx: &App) -> bool {
        true
    }
    /// Which column of its own this panel asks for, if any.
    ///
    /// `None` -- the default -- puts the panel in the tool dock named by
    /// `position`. Anything else routes it to the workspace's column for that
    /// variant, where it never shares vertical space with a tool panel.
    fn own_column(&self) -> Option<DockColumn> {
        None
    }
}

pub trait PanelHandle: Send + Sync {
    fn panel_id(&self) -> EntityId;
    fn persistent_name(&self) -> &'static str;
    fn panel_key(&self) -> &'static str;
    fn position(&self, window: &Window, cx: &App) -> DockPosition;
    fn position_is_valid(&self, position: DockPosition, cx: &App) -> bool;
    fn set_position(&self, position: DockPosition, window: &mut Window, cx: &mut App);
    fn is_zoomed(&self, window: &Window, cx: &App) -> bool;
    fn fills_the_window(&self, window: &Window, cx: &App) -> bool;
    fn set_zoomed(&self, zoomed: bool, window: &mut Window, cx: &mut App);
    fn set_active(&self, active: bool, window: &mut Window, cx: &mut App);
    fn remote_id(&self) -> Option<proto::PanelId>;
    fn pane(&self, cx: &App) -> Option<Entity<Pane>>;
    fn default_size(&self, window: &Window, cx: &App) -> Pixels;
    fn min_size(&self, window: &Window, cx: &App) -> Option<Pixels>;
    fn initial_size_state(&self, window: &Window, cx: &App) -> PanelSizeState;
    fn size_state_changed(&self, window: &mut Window, cx: &mut App);
    fn supports_flexible_size(&self, cx: &App) -> bool;
    fn has_flexible_size(&self, window: &Window, cx: &App) -> bool;
    fn set_flexible_size(&self, flexible: bool, window: &mut Window, cx: &mut App);
    fn icon(&self, window: &Window, cx: &App) -> Option<ui::IconName>;
    fn icon_tooltip(&self, window: &Window, cx: &App) -> Option<&'static str>;
    fn toggle_action(&self, window: &Window, cx: &App) -> Box<dyn Action>;
    fn icon_label(&self, window: &Window, cx: &App) -> Option<String>;
    fn panel_focus_handle(&self, cx: &App) -> FocusHandle;
    fn to_any(&self) -> AnyView;
    fn activation_priority(&self, cx: &App) -> u32;
    fn enabled(&self, cx: &App) -> bool;
    fn own_column(&self, cx: &App) -> Option<DockColumn>;
    /// Cycles the panel to its next valid edge, wrapping at the end.
    ///
    /// Wrapping happens WITHIN the valid edges. The earlier form filtered the
    /// edges but then answered the end of the list with a literal
    /// `DockPosition::Left`, which is the right answer only because `Left`
    /// happens to be first: a panel whose only valid edge is `Right` was moved
    /// to an edge it had just refused, and `MoveFocusedPanelToNextPosition`
    /// carries a keybinding, so that was reachable. Indexing the valid list
    /// keeps the wrap for a panel that can move and leaves a one-edge panel
    /// exactly where it is.
    fn move_to_next_position(&self, window: &mut Window, cx: &mut App) {
        let current_position = self.position(window, cx);
        let valid: Vec<DockPosition> = [
            DockPosition::Left,
            DockPosition::Bottom,
            DockPosition::Right,
        ]
        .into_iter()
        .filter(|position| self.position_is_valid(*position, cx))
        .collect();

        let next_position = valid
            .iter()
            .position(|position| *position == current_position)
            .map(|index| valid[(index + 1) % valid.len()])
            .unwrap_or(current_position);

        self.set_position(next_position, window, cx);
    }
}

impl<T> PanelHandle for Entity<T>
where
    T: Panel,
{
    fn panel_id(&self) -> EntityId {
        Entity::entity_id(self)
    }

    fn persistent_name(&self) -> &'static str {
        T::persistent_name()
    }

    fn panel_key(&self) -> &'static str {
        T::panel_key()
    }

    fn position(&self, window: &Window, cx: &App) -> DockPosition {
        self.read(cx).position(window, cx)
    }

    fn position_is_valid(&self, position: DockPosition, cx: &App) -> bool {
        self.read(cx).position_is_valid(position)
    }

    fn set_position(&self, position: DockPosition, window: &mut Window, cx: &mut App) {
        self.update(cx, |this, cx| this.set_position(position, window, cx))
    }

    fn is_zoomed(&self, window: &Window, cx: &App) -> bool {
        self.read(cx).is_zoomed(window, cx)
    }

    fn fills_the_window(&self, window: &Window, cx: &App) -> bool {
        self.read(cx).fills_the_window(window, cx)
    }

    fn set_zoomed(&self, zoomed: bool, window: &mut Window, cx: &mut App) {
        self.update(cx, |this, cx| this.set_zoomed(zoomed, window, cx))
    }

    fn set_active(&self, active: bool, window: &mut Window, cx: &mut App) {
        self.update(cx, |this, cx| this.set_active(active, window, cx))
    }

    fn pane(&self, cx: &App) -> Option<Entity<Pane>> {
        self.read(cx).pane()
    }

    fn remote_id(&self) -> Option<PanelId> {
        T::remote_id()
    }

    fn default_size(&self, window: &Window, cx: &App) -> Pixels {
        self.read(cx).default_size(window, cx)
    }

    fn min_size(&self, window: &Window, cx: &App) -> Option<Pixels> {
        self.read(cx).min_size(window, cx)
    }

    fn initial_size_state(&self, window: &Window, cx: &App) -> PanelSizeState {
        self.read(cx).initial_size_state(window, cx)
    }

    fn size_state_changed(&self, window: &mut Window, cx: &mut App) {
        self.update(cx, |this, cx| this.size_state_changed(window, cx))
    }

    fn supports_flexible_size(&self, cx: &App) -> bool {
        self.read(cx).supports_flexible_size()
    }

    fn has_flexible_size(&self, window: &Window, cx: &App) -> bool {
        self.read(cx).has_flexible_size(window, cx)
    }

    fn set_flexible_size(&self, flexible: bool, window: &mut Window, cx: &mut App) {
        self.update(cx, |this, cx| this.set_flexible_size(flexible, window, cx))
    }

    fn icon(&self, window: &Window, cx: &App) -> Option<ui::IconName> {
        self.read(cx).icon(window, cx)
    }

    fn icon_tooltip(&self, window: &Window, cx: &App) -> Option<&'static str> {
        self.read(cx).icon_tooltip(window, cx)
    }

    fn toggle_action(&self, _: &Window, cx: &App) -> Box<dyn Action> {
        self.read(cx).toggle_action()
    }

    fn icon_label(&self, window: &Window, cx: &App) -> Option<String> {
        self.read(cx).icon_label(window, cx)
    }

    fn to_any(&self) -> AnyView {
        self.clone().into()
    }

    fn panel_focus_handle(&self, cx: &App) -> FocusHandle {
        self.read(cx).focus_handle(cx)
    }

    fn activation_priority(&self, cx: &App) -> u32 {
        self.read(cx).activation_priority()
    }

    fn enabled(&self, cx: &App) -> bool {
        self.read(cx).enabled(cx)
    }

    fn own_column(&self, cx: &App) -> Option<DockColumn> {
        self.read(cx).own_column()
    }
}

impl From<&dyn PanelHandle> for AnyView {
    fn from(val: &dyn PanelHandle) -> Self {
        val.to_any()
    }
}

/// A container with a fixed [`DockPosition`] adjacent to a certain widown edge.
/// Can contain multiple panels and show/hide itself with all contents.
pub struct Dock {
    position: DockPosition,
    panel_entries: Vec<PanelEntry>,
    workspace: WeakEntity<Workspace>,
    is_open: bool,
    active_panel_index: Option<usize>,
    focus_handle: FocusHandle,
    focus_follows_mouse: FocusFollowsMouse,
    pub(crate) serialized_dock: Option<DockData>,
    /// The stack recorded for this dock, pushed in by the workspace.
    ///
    /// Handed over rather than fetched, for the same reason `serialized_dock`
    /// is: `restore_state` runs from `add_panel`, which itself runs inside a
    /// `Workspace` update, so reading the workspace back through its handle
    /// there aborts the process.
    pub(crate) serialized_stack: Option<DockStackState>,
    /// Which column this dock is, when its `DockPosition` alone cannot say.
    ///
    /// An own column carries a `DockPosition` so resizing knows which way it
    /// grows, and shares that position with the tool dock beside it. Storage
    /// keys, element id space and resize-handle routing all read this instead.
    column: DockColumn,
    /// How the panels showing at once divide the dock's *length*.
    ///
    /// A second axis from `PanelEntry::size_state`, which measures the dock's
    /// *width* (one value for the whole dock, dragged at its outer edge). This
    /// one splits what is left between stacked panels — never confuse the two:
    /// writing a stack proportion through `resize_active_panel` would resize
    /// the dock instead.
    stack_flexes: Arc<Mutex<Vec<f32>>>,
    stack_bounding_boxes: Arc<Mutex<Vec<Option<Bounds<Pixels>>>>>,
    /// The stack as last written down, so an unchanged one is not written again.
    ///
    /// `serialize_workspace` fires on every kind of workspace change, throttled
    /// to 200ms — including all through a divider drag. Without this each of
    /// those ticks cost a JSON encode and an `INSERT OR REPLACE` per dock,
    /// whether or not that dock's stack had moved at all.
    last_persisted_stack: Option<DockStackState>,
    zoom_layer_open: bool,
    modal_layer: Entity<ModalLayer>,
    _subscriptions: [Subscription; 2],
}

impl Focusable for Dock {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Which column of the window a dock is, as distinct from which side it is on.
///
/// `Tool` is one of the three ordinary docks, named by its `DockPosition`.
/// Every other variant is a column of its own drawn beside the centre group:
/// it still carries a `DockPosition` so resizing knows which way it grows, but
/// it shares that position with the tool dock on the same side. Anywhere the
/// two must be told apart -- storage keys, element id space, which resize
/// handle the pointer is on -- reads this rather than the position.
///
/// Adding a variant means giving it a distinct `stack_key` and a distinct
/// `stack_element_basis` offset; both are derived here so they cannot drift.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DockColumn {
    Tool,
    Database,
}

impl DockColumn {
    /// Whether this dock is a column of its own rather than one of the three
    /// tool docks. Chrome, position-following and panel migration all branch on
    /// this and never on the specific variant -- own columns behave alike.
    pub fn is_own_column(self) -> bool {
        !matches!(self, DockColumn::Tool)
    }

    /// What an own column's stack is recorded under. `None` for `Tool`, which
    /// keys by its position instead.
    fn storage_key(self) -> Option<&'static str> {
        match self {
            DockColumn::Tool => None,
            DockColumn::Database => Some("database"),
        }
    }

    /// Distinct slot in the stack element id space, past the three positions.
    /// `None` for `Tool`, which uses its position as the offset.
    fn element_basis_offset(self) -> Option<usize> {
        match self {
            DockColumn::Tool => None,
            DockColumn::Database => Some(DockPosition::COUNT),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DockPosition {
    Left,
    Bottom,
    Right,
}

impl From<settings::DockPosition> for DockPosition {
    fn from(value: settings::DockPosition) -> Self {
        match value {
            settings::DockPosition::Left => Self::Left,
            settings::DockPosition::Bottom => Self::Bottom,
            settings::DockPosition::Right => Self::Right,
        }
    }
}

impl Into<settings::DockPosition> for DockPosition {
    fn into(self) -> settings::DockPosition {
        match self {
            Self::Left => settings::DockPosition::Left,
            Self::Bottom => settings::DockPosition::Bottom,
            Self::Right => settings::DockPosition::Right,
        }
    }
}

impl DockPosition {
    /// How many positions there are, so a fourth dock can take an id space
    /// past all of them rather than sharing one. See `stack_element_basis`.
    const COUNT: usize = 3;

    /// Also the key a dock's stack is recorded under, so these strings are
    /// persisted: renaming one orphans what users had open.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Bottom => "Bottom",
            Self::Right => "Right",
        }
    }

    pub fn axis(&self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Horizontal,
            Self::Bottom => Axis::Vertical,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PanelSizeState {
    pub size: Option<Pixels>,
    #[serde(default)]
    pub flex: Option<f32>,
}

struct PanelEntry {
    panel: Arc<dyn PanelHandle>,
    size_state: PanelSizeState,
    /// Whether this panel is one of the ones the dock is showing.
    ///
    /// A dock shows exactly one at a time today, so exactly one entry carries
    /// this while the dock is open — a stack of several is what this flag is
    /// here to make possible.
    ///
    /// Deliberately a flag on the entry rather than a set of indices held by
    /// the dock: `add_panel` and `remove_panel` shift entry indices, and the
    /// fiddly `+= 1` / `-= 1` fixups that already exist for `active_panel_index`
    /// are exactly the bug this would multiply. A flag moves with the entry it
    /// belongs to and cannot go stale.
    visible: bool,
    _subscriptions: [Subscription; 3],
}

pub struct PanelButtons {
    dock: Entity<Dock>,
    _settings_subscription: Subscription,
}

pub(crate) const PANEL_SIZE_STATE_KEY: &str = "dock_panel_size";

/// Which panels a dock was showing, and how they divided it.
///
/// Kept in the key-value store beside the panel sizes rather than in a column
/// on `workspaces`: that table is read positionally (`impl Column for
/// DockData`), so a new column has to be threaded through every SELECT in the
/// right place or every field after it shifts, silently, on real user data. A
/// key that is simply absent on an older install reads as "no stack recorded"
/// and falls back to the single active panel — which is the behaviour wanted
/// anyway.
pub(crate) const DOCK_STACK_KEY: &str = "dock_stack";

/// Panels are recorded by `persistent_name`, the same handle the serialized
/// `active_panel` uses, so one that has since been removed from the app is
/// skipped on the way back in rather than shifting everything after it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DockStackState {
    pub showing: Vec<String>,
    #[serde(default)]
    pub flexes: Vec<f32>,
}

fn panel_uses_flexible_width(
    position: DockPosition,
    panel: &dyn PanelHandle,
    window: &Window,
    cx: &App,
) -> bool {
    position.axis() == Axis::Horizontal && panel.has_flexible_size(window, cx)
}

fn resize_panel_entry(
    position: DockPosition,
    entry: &mut PanelEntry,
    size: Option<Pixels>,
    flex: Option<f32>,
    window: &mut Window,
    cx: &mut App,
) -> (&'static str, PanelSizeState) {
    let size = size.map(|size| size.max(RESIZE_HANDLE_SIZE).round());
    let uses_flexible_width = panel_uses_flexible_width(position, entry.panel.as_ref(), window, cx);
    if uses_flexible_width {
        entry.size_state.flex = flex;
    } else {
        entry.size_state.size = size;
    }
    entry.panel.size_state_changed(window, cx);
    (entry.panel.panel_key(), entry.size_state)
}

impl Dock {
    pub fn new(
        position: DockPosition,
        modal_layer: Entity<ModalLayer>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        let focus_handle = cx.focus_handle();
        let workspace = cx.entity();
        let dock = cx.new(|cx| {
            let focus_subscription =
                cx.on_focus(&focus_handle, window, |dock: &mut Dock, window, cx| {
                    if let Some(active_entry) = dock.active_panel_entry() {
                        active_entry.panel.panel_focus_handle(cx).focus(window, cx)
                    }
                });
            let zoom_subscription = cx.subscribe(&workspace, |dock, workspace, e: &Event, cx| {
                if matches!(e, Event::ZoomChanged) {
                    let is_zoomed = workspace.read(cx).zoomed.is_some();
                    dock.zoom_layer_open = is_zoomed;
                }
            });
            Self {
                position,
                workspace: workspace.downgrade(),
                panel_entries: Default::default(),
                active_panel_index: None,
                serialized_stack: None,
                column: DockColumn::Tool,
                stack_flexes: Default::default(),
                stack_bounding_boxes: Default::default(),
                last_persisted_stack: None,
                is_open: false,
                focus_handle: focus_handle.clone(),
                focus_follows_mouse: WorkspaceSettings::get_global(cx).focus_follows_mouse,
                _subscriptions: [focus_subscription, zoom_subscription],
                serialized_dock: None,
                zoom_layer_open: false,
                modal_layer,
            }
        });

        cx.on_focus_in(&focus_handle, window, {
            let dock = dock.downgrade();
            move |workspace, window, cx| {
                let Some(dock) = dock.upgrade() else {
                    return;
                };
                let Some(panel) = dock.read(cx).active_panel() else {
                    return;
                };
                if panel.is_zoomed(window, cx) {
                    workspace.zoomed = Some(panel.to_any().downgrade());
                    workspace.zoomed_position = Some(position);
                } else {
                    workspace.zoomed = None;
                    workspace.zoomed_position = None;
                }
                cx.emit(Event::ZoomChanged);
                workspace.dismiss_zoomed_items_to_reveal(Some(position), window, cx);
                workspace.update_active_view_for_followers(window, cx)
            }
        })
        .detach();

        cx.observe_in(&dock, window, move |workspace, dock, window, cx| {
            if dock.read(cx).is_open()
                && let Some(panel) = dock.read(cx).active_panel()
                && panel.is_zoomed(window, cx)
            {
                workspace.zoomed = Some(panel.to_any().downgrade());
                workspace.zoomed_position = Some(position);
                cx.emit(Event::ZoomChanged);
                return;
            }
            if workspace.zoomed_position == Some(position) {
                workspace.zoomed = None;
                workspace.zoomed_position = None;
                cx.emit(Event::ZoomChanged);
            }
        })
        .detach();

        dock
    }

    pub fn position(&self) -> DockPosition {
        self.position
    }

    /// Moves an own column to the other side of the editor.
    ///
    /// Only own columns move this way — the three ordinary docks are defined by
    /// their side and never change it, while these follow the rail. The side
    /// decides which edge carries the border and the resize handle, so it has
    /// to change with it rather than only at construction.
    pub fn set_own_column_position(&mut self, position: DockPosition, cx: &mut Context<Self>) {
        if !self.column.is_own_column() || self.position == position {
            return;
        }
        self.position = position;
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.is_open
    }

    fn resizable(&self, cx: &App) -> bool {
        !(self.zoom_layer_open || self.modal_layer.read(cx).has_active_modal())
    }

    pub fn panel<T: Panel>(&self) -> Option<Entity<T>> {
        self.panel_entries
            .iter()
            .find_map(|entry| entry.panel.to_any().downcast().ok())
    }

    pub fn panel_index_for_type<T: Panel>(&self) -> Option<usize> {
        self.panel_entries
            .iter()
            .position(|entry| entry.panel.to_any().downcast::<T>().is_ok())
    }

    pub fn panel_index_for_persistent_name(&self, ui_name: &str, _cx: &App) -> Option<usize> {
        self.panel_entries
            .iter()
            .position(|entry| entry.panel.persistent_name() == ui_name)
    }

    pub fn panel_index_for_proto_id(&self, panel_id: PanelId) -> Option<usize> {
        self.panel_entries
            .iter()
            .position(|entry| entry.panel.remote_id() == Some(panel_id))
    }

    pub fn panel_for_id(&self, panel_id: EntityId) -> Option<&Arc<dyn PanelHandle>> {
        self.panel_entries
            .iter()
            .find(|entry| entry.panel.panel_id() == panel_id)
            .map(|entry| &entry.panel)
    }

    pub fn first_enabled_panel_idx(&mut self, cx: &mut Context<Self>) -> anyhow::Result<usize> {
        self.panel_entries
            .iter()
            .position(|entry| entry.panel.enabled(cx))
            .with_context(|| {
                format!(
                    "Couldn't find any enabled panel for the {} dock.",
                    self.position.label()
                )
            })
    }

    fn active_panel_entry(&self) -> Option<&PanelEntry> {
        self.active_panel_index
            .and_then(|index| self.panel_entries.get(index))
    }

    pub fn active_panel_index(&self) -> Option<usize> {
        self.active_panel_index
    }

    /// The entry whose stored size decides how wide -- or, at the bottom, how
    /// tall -- the dock is.
    ///
    /// A dock that takes turns is *one column* to the eye: its header is a tab
    /// strip and only ever one panel is drawn below it. So it holds one extent,
    /// and taking that extent from whichever panel happens to be up is what
    /// made the column change width every time you switched tabs -- the project
    /// panel defaults to 240px and the agent history to 420px, and a drag on
    /// the column's edge only ever wrote to the panel showing at the time. Two
    /// panels, two widths, one column: the width had to move.
    ///
    /// Entries are held in `activation_priority` order, so the first is the
    /// column's primary panel. It owns the extent and the others read it. A
    /// dock that stacks has no switcher and each section keeps its own size, so
    /// there this stays the active entry, exactly as before.
    fn size_governing_index(&self) -> Option<usize> {
        if self.takes_turns() {
            (!self.panel_entries.is_empty()).then_some(0)
        } else {
            self.active_panel_index
        }
    }

    fn size_governing_entry(&self) -> Option<&PanelEntry> {
        self.size_governing_index()
            .and_then(|index| self.panel_entries.get(index))
    }

    /// The panel whose size, default size and flexibility describe the dock.
    ///
    /// For `render_dock`, which needs all three from the same place -- reading
    /// the width off one panel and the fixed-or-flexible mode off another would
    /// be a third way for the column to move.
    pub fn size_governing_panel(&self) -> Option<&Arc<dyn PanelHandle>> {
        self.size_governing_entry().map(|entry| &entry.panel)
    }

    pub fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if open != self.is_open {
            self.is_open = open;
            if let Some(active_panel) = self.active_panel_entry() {
                active_panel.panel.set_active(open, window, cx);
            }

            cx.notify();
        }
    }

    pub fn set_panel_zoomed(
        &mut self,
        panel: &AnyView,
        zoomed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for entry in &mut self.panel_entries {
            if entry.panel.panel_id() == panel.entity_id() {
                if zoomed != entry.panel.is_zoomed(window, cx) {
                    entry.panel.set_zoomed(zoomed, window, cx);
                }
            } else if entry.panel.is_zoomed(window, cx) {
                entry.panel.set_zoomed(false, window, cx);
            }
        }

        self.workspace
            .update(cx, |workspace, cx| {
                workspace.serialize_workspace(window, cx);
            })
            .ok();
        cx.notify();
    }

    pub fn zoom_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for entry in &mut self.panel_entries {
            if entry.panel.is_zoomed(window, cx) {
                entry.panel.set_zoomed(false, window, cx);
            }
        }
    }

    pub(crate) fn add_panel<T: Panel>(
        &mut self,
        panel: Entity<T>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let subscriptions = [
            cx.observe(&panel, |_, _, cx| cx.notify()),
            cx.observe_global_in::<SettingsStore>(window, {
                let workspace = workspace.clone();
                let panel = panel.clone();

                move |this, window, cx| {
                    // An own column is the panel's home at either rail side:
                    // the column itself moves (`set_own_column_position`), so
                    // there is no dock to migrate to. Without this the panel is
                    // hauled into a tool dock the moment the rail flips, and
                    // whether that happens comes down to which of the two
                    // settings observers fires first.
                    if this.column.is_own_column() {
                        return;
                    }

                    let new_position = panel.read(cx).position(window, cx);
                    if new_position == this.position {
                        return;
                    }

                    let Ok(new_dock) = workspace.update(cx, |workspace, cx| {
                        if panel.is_zoomed(window, cx) {
                            workspace.zoomed_position = Some(new_position);
                        }
                        match new_position {
                            DockPosition::Left => &workspace.left_dock,
                            DockPosition::Bottom => &workspace.bottom_dock,
                            DockPosition::Right => &workspace.right_dock,
                        }
                        .clone()
                    }) else {
                        return;
                    };

                    let panel_id = Entity::entity_id(&panel);
                    let was_visible = this.is_open()
                        && this
                            .visible_panel()
                            .is_some_and(|active_panel| active_panel.panel_id() == panel_id);
                    let size_state = this
                        .panel_entries
                        .iter()
                        .find(|entry| entry.panel.panel_id() == panel_id)
                        .map(|entry| entry.size_state)
                        .unwrap_or_default();

                    let previous_axis = this.position.axis();
                    let next_axis = new_position.axis();
                    let size_state = if previous_axis == next_axis {
                        size_state
                    } else {
                        PanelSizeState::default()
                    };

                    if !this.remove_panel(&panel, window, cx) {
                        // Panel was already moved from this dock
                        return;
                    }

                    new_dock.update(cx, |new_dock, cx| {
                        let index =
                            new_dock.add_panel(panel.clone(), workspace.clone(), window, cx);
                        if let Some(added_panel) = new_dock.panel_for_id(panel_id).cloned() {
                            new_dock.set_panel_size_state(added_panel.as_ref(), size_state, cx);
                        }
                        if was_visible {
                            new_dock.set_open(true, window, cx);
                            new_dock.activate_panel(index, window, cx);
                        }
                    });

                    workspace
                        .update(cx, |workspace, cx| {
                            workspace.serialize_workspace(window, cx);
                        })
                        .ok();
                }
            }),
            cx.subscribe_in(
                &panel,
                window,
                move |this, panel, event, window, cx| match event {
                    PanelEvent::ZoomIn => {
                        this.set_panel_zoomed(&panel.to_any(), true, window, cx);
                        if !PanelHandle::panel_focus_handle(panel, cx).contains_focused(window, cx)
                        {
                            window.focus(&panel.focus_handle(cx), cx);
                        }
                        workspace
                            .update(cx, |workspace, cx| {
                                workspace.zoomed = Some(panel.downgrade().into());
                                workspace.zoomed_position =
                                    Some(panel.read(cx).position(window, cx));
                                cx.emit(Event::ZoomChanged);
                            })
                            .ok();
                    }
                    PanelEvent::ZoomOut => {
                        this.set_panel_zoomed(&panel.to_any(), false, window, cx);
                        workspace
                            .update(cx, |workspace, cx| {
                                if workspace.zoomed_position == Some(this.position) {
                                    workspace.zoomed = None;
                                    workspace.zoomed_position = None;
                                    cx.emit(Event::ZoomChanged);
                                }
                                cx.notify();
                            })
                            .ok();
                    }
                    PanelEvent::Activate => {
                        if let Some(ix) = this
                            .panel_entries
                            .iter()
                            .position(|entry| entry.panel.panel_id() == Entity::entity_id(panel))
                        {
                            this.set_open(true, window, cx);
                            this.activate_panel(ix, window, cx);
                            window.focus(&panel.read(cx).focus_handle(cx), cx);
                        }
                    }
                    PanelEvent::Close => {
                        if this
                            .visible_panel()
                            .is_some_and(|p| p.panel_id() == Entity::entity_id(panel))
                        {
                            this.set_open(false, window, cx);
                        }
                    }
                },
            ),
        ];

        let index = match self
            .panel_entries
            .binary_search_by_key(&panel.read(cx).activation_priority(), |entry| {
                entry.panel.activation_priority(cx)
            }) {
            Ok(ix) => {
                if cfg!(debug_assertions) {
                    panic!(
                        "Panels `{}` and `{}` have the same activation priority. Each panel must have a unique priority so the status bar order is deterministic.",
                        T::panel_key(),
                        self.panel_entries[ix].panel.panel_key()
                    );
                }
                ix
            }
            Err(ix) => ix,
        };
        if let Some(active_index) = self.active_panel_index.as_mut()
            && *active_index >= index
        {
            *active_index += 1;
        }
        let size_state = panel.read(cx).initial_size_state(window, cx);

        self.panel_entries.insert(
            index,
            PanelEntry {
                panel: Arc::new(panel.clone()),
                size_state,
                // Added put away: `restore_state` and `starts_open` below are
                // what decide to show it, exactly as before.
                visible: false,
                _subscriptions: subscriptions,
            },
        );

        self.restore_state(window, cx);

        if panel.read(cx).starts_open(window, cx) {
            self.activate_panel(index, window, cx);
            self.set_open(true, window, cx);
        }

        cx.notify();
        index
    }

    pub fn restore_state(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(serialized) = self.serialized_dock.clone() {
            if let Some(active_panel) = serialized.active_panel.filter(|_| serialized.visible)
                && let Some(idx) = self.panel_index_for_persistent_name(active_panel.as_str(), cx)
            {
                self.activate_panel(idx, window, cx);
            }

            // Layered over the single active panel rather than replacing it: an
            // install from before stacks has no record here, and one whose
            // recorded panels have all been removed must still come back to the
            // panel above rather than to nothing.
            if serialized.visible
                && let Some(stack) = self.serialized_stack.clone()
            {
                self.apply_stack_state(&stack, window, cx);
            }

            if serialized.zoom
                && let Some(panel) = self.active_panel()
            {
                panel.set_zoomed(true, window, cx)
            }
            self.set_open(serialized.visible, window, cx);
            return true;
        }
        false
    }

    pub fn remove_panel<T: Panel>(
        &mut self,
        panel: &Entity<T>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(panel_ix) = self
            .panel_entries
            .iter()
            .position(|entry| entry.panel.panel_id() == Entity::entity_id(panel))
        {
            if let Some(active_panel_index) = self.active_panel_index.as_mut() {
                match panel_ix.cmp(active_panel_index) {
                    std::cmp::Ordering::Less => {
                        *active_panel_index -= 1;
                    }
                    std::cmp::Ordering::Equal => {
                        self.active_panel_index = None;
                        self.set_open(false, window, cx);
                    }
                    std::cmp::Ordering::Greater => {}
                }
            }

            self.panel_entries.remove(panel_ix);
            self.reset_stack_flexes();
            cx.notify();

            true
        } else {
            false
        }
    }

    pub fn panels_len(&self) -> usize {
        self.panel_entries.len()
    }

    /// The panels docked here, in the order they are shown. `panel_entries` is
    /// private to this module, so anything outside it that wants to draw its own
    /// list of panels — the project rail's panel switcher — goes through here.
    pub fn panels(&self) -> impl Iterator<Item = &Arc<dyn PanelHandle>> {
        self.panel_entries.iter().map(|entry| &entry.panel)
    }

    /// Shows `panel_ix` and puts every other panel in this dock away.
    ///
    /// One panel at a time is what a dock does today, and this is the single
    /// place that decides it — so it is also the single place a stack would
    /// later stop being exclusive.
    pub fn activate_panel(&mut self, panel_ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if Some(panel_ix) != self.active_panel_index {
            if let Some(active_panel) = self.active_panel_entry() {
                active_panel.panel.set_active(false, window, cx);
            }

            self.active_panel_index = Some(panel_ix);
            for (ix, entry) in self.panel_entries.iter_mut().enumerate() {
                entry.visible = ix == panel_ix;
            }
            self.reset_stack_flexes();

            if let Some(active_panel) = self.active_panel_entry() {
                active_panel.panel.set_active(true, window, cx);
            }

            self.persist_stack(cx);
            cx.notify();
        }
    }

    pub fn visible_panel(&self) -> Option<&Arc<dyn PanelHandle>> {
        let entry = self.visible_entry()?;
        Some(&entry.panel)
    }

    pub fn active_panel(&self) -> Option<&Arc<dyn PanelHandle>> {
        let panel_entry = self.active_panel_entry()?;
        Some(&panel_entry.panel)
    }

    /// The panel the dock is drawing, if it is drawing one.
    ///
    /// Reads the entries' own `visible` flags rather than `active_panel_index`,
    /// so what renders and what is marked visible cannot disagree. While a dock
    /// shows one panel the two answers are the same panel; when a stack becomes
    /// possible this is the first of them, and callers wanting all of them want
    /// `visible_entries`.
    fn visible_entry(&self) -> Option<&PanelEntry> {
        self.visible_entries().next()
    }

    fn visible_entries(&self) -> impl Iterator<Item = &PanelEntry> {
        let is_open = self.is_open;
        self.panel_entries
            .iter()
            .filter(move |entry| is_open && entry.visible)
    }

    /// Every panel the dock is showing, in the order they are drawn.
    ///
    /// The point of it is that asking "is this panel up?" does not have to go
    /// through `active_panel_index`, which answers a different question.
    pub fn visible_panels(&self) -> impl Iterator<Item = &Arc<dyn PanelHandle>> {
        self.visible_entries().map(|entry| &entry.panel)
    }

    pub fn is_panel_visible(&self, panel_id: EntityId) -> bool {
        self.visible_panels()
            .any(|panel| panel.panel_id() == panel_id)
    }

    /// Whether this dock shows one panel at a time.
    ///
    /// A dock that draws a header draws a *switcher*: its buttons are a tab
    /// strip, and a tab strip means one tab. A dock without a header has no way
    /// to switch, so there stacking is the only route to a second panel and the
    /// stack stays. That is the whole of the rule, and it is asked in one place
    /// — `show_panel` — so every route in (button, keybinding, palette,
    /// `open_panel`) lands on the same answer. Two routes disagreeing is exactly
    /// the defect this replaced: the header's own two buttons behaved
    /// differently, one taking the column and one splitting it.
    pub fn takes_turns(&self) -> bool {
        dock_header_draws_panel(self.position)
    }

    /// Brings a panel up: alone in a dock that takes turns, alongside whatever
    /// is already there in one that stacks.
    ///
    /// The counterpart to `activate_panel`, which is always exclusive.
    pub fn show_panel(&mut self, panel_ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if panel_ix >= self.panel_entries.len() {
            return;
        }
        let mut displaced_any = false;
        if self.takes_turns() {
            // Collected first: `set_active` reaches back into the app and cannot
            // run while the entries are borrowed.
            let displaced: Vec<usize> = self
                .panel_entries
                .iter()
                .enumerate()
                .filter(|(ix, entry)| *ix != panel_ix && entry.visible)
                .map(|(ix, _)| ix)
                .collect();
            displaced_any = !displaced.is_empty();
            for ix in displaced {
                self.panel_entries[ix].visible = false;
                let panel = self.panel_entries[ix].panel.clone();
                panel.set_active(false, window, cx);
            }
        }
        let Some(entry) = self.panel_entries.get_mut(panel_ix) else {
            return;
        };
        // The flag outlives a closed dock, so a panel can be marked showing
        // while nothing is drawn. Asking for it again then means "open the dock
        // back onto it", not "nothing to do".
        let was_showing = entry.visible;
        entry.visible = true;
        // Displacing a neighbour matters even when this panel was already up: the
        // flexes still describe a split column, and the survivor would keep half
        // the height with nothing beside it.
        if !was_showing || displaced_any {
            self.reset_stack_flexes();
        }
        self.active_panel_index = Some(panel_ix);
        if !was_showing && let Some(entry) = self.panel_entries.get(panel_ix) {
            entry.panel.set_active(true, window, cx);
        }
        self.set_open(true, window, cx);
        self.persist_stack(cx);
        cx.notify();
    }

    /// Takes a panel out of the stack, closing the dock if it was the last.
    ///
    /// Keyed by id rather than index because indices shift under `add_panel`
    /// and `remove_panel`, and this is reached from a click on a header drawn
    /// some frames earlier.
    pub fn hide_panel_by_id(
        &mut self,
        panel_id: EntityId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(panel_ix) = self
            .panel_entries
            .iter()
            .position(|entry| entry.panel.panel_id() == panel_id)
        else {
            return;
        };
        let Some(entry) = self.panel_entries.get_mut(panel_ix) else {
            return;
        };
        if !entry.visible {
            return;
        }
        entry.visible = false;
        self.reset_stack_flexes();
        if let Some(entry) = self.panel_entries.get(panel_ix) {
            entry.panel.set_active(false, window, cx);
        }

        // Whatever is left takes over as the focused one; nothing left means
        // the dock has no reason to hold width.
        //
        // Asked of the stack alone. Folding `is_open` into the predicate — as
        // this did — makes a closed dock report nothing remaining however many
        // panels are still flagged, so hiding one through `close_panel` cleared
        // `active_panel_index` outright and the dock reopened onto a stack with
        // no active panel in it.
        let remaining = self.panel_entries.iter().position(|entry| entry.visible);
        self.active_panel_index = remaining;
        if remaining.is_none() {
            self.set_open(false, window, cx);
        }
        self.persist_stack(cx);
        cx.notify();
    }

    /// Shares the dock's length evenly whenever the stack gains or loses a
    /// panel. `PaneAxis::insert_pane` does the same on a split — a stack that
    /// silently kept a departed panel's share would leave a gap.
    fn reset_stack_flexes(&mut self) {
        let showing = self.panel_entries.iter().filter(|e| e.visible).count();
        *self.stack_flexes.lock() = vec![1.; showing];
    }

    /// Writes the stack down so a restart comes back to it.
    ///
    /// Deferred rather than written inline: this runs from the middle of the
    /// dock's own update, and the workspace is what holds the key-value store.
    /// `resize_active_panel` persists the other axis the same way.
    fn persist_stack(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.stack_state_if_changed() else {
            return;
        };
        let workspace = self.workspace.clone();
        let key = self.stack_key();
        cx.defer(move |cx| {
            if let Some(workspace) = workspace.upgrade() {
                workspace.update(cx, |workspace, cx| {
                    workspace.persist_dock_stack(key, state, cx);
                });
            }
        });
    }

    /// Marks this dock as a column of its own rather than a tool dock.
    ///
    /// Set once, at construction, by the workspace that owns it.
    pub fn mark_as_own_column(&mut self, column: DockColumn) {
        self.column = column;
    }

    /// Which column this dock is. `DockColumn::Tool` for the three ordinary
    /// docks, which are told apart by their position alone.
    pub fn column(&self) -> DockColumn {
        self.column
    }

    /// Where this dock's stack puts its element ids.
    ///
    /// Keyed the same way `stack_key` is, and for the same reason: an own
    /// column shares a `DockPosition` with the dock beside it, so a basis of
    /// `BASIS + position` had the two drawing their handles into one id space.
    /// Out of reach today — the columns hold a single panel, so they never draw
    /// a stack — but the collision would land silently the day one holds two.
    pub(crate) fn stack_element_basis(&self) -> usize {
        let offset = self
            .column
            .element_basis_offset()
            .unwrap_or(self.position as usize);
        STACK_ELEMENT_BASIS + offset
    }

    /// What this dock's stack is recorded under.
    ///
    /// NOT the position: an own column shares a `DockPosition` with whichever
    /// side dock it stands next to, so keying by position would have the two
    /// writing over each other -- an own column, usually empty, blanking the
    /// record of a dock that had panels in it.
    pub fn stack_key(&self) -> &'static str {
        self.column
            .storage_key()
            .unwrap_or_else(|| self.position.label())
    }

    /// The proportions the stack's dividers write into.
    ///
    /// The very handle `render_stack` gives `pane_axis`, so a test can move a
    /// divider the way a drag does. That matters because a drag is the one
    /// change that reaches nothing but the serialize pass — every other one
    /// goes through `persist_stack` on its way past.
    #[cfg(any(test, feature = "test-support"))]
    pub fn stack_flexes(&self) -> Arc<Mutex<Vec<f32>>> {
        self.stack_flexes.clone()
    }

    /// The stack, but only if it differs from the one last written down.
    ///
    /// Both writers go through here — the dock itself when panels come and go,
    /// and `serialize_workspace` when a divider moves — so neither can rewrite
    /// what the other has just stored.
    pub fn stack_state_if_changed(&mut self) -> Option<DockStackState> {
        let state = self.stack_state();
        if self.last_persisted_stack.as_ref() == Some(&state) {
            return None;
        }
        self.last_persisted_stack = Some(state.clone());
        Some(state)
    }

    /// What this dock is showing, in a form that survives a restart.
    pub fn stack_state(&self) -> DockStackState {
        DockStackState {
            showing: self
                .panel_entries
                .iter()
                .filter(|entry| entry.visible)
                .map(|entry| entry.panel.persistent_name().to_string())
                .collect(),
            flexes: self.stack_flexes.lock().clone(),
        }
    }

    /// Puts back a stack recorded by `stack_state`.
    ///
    /// Panels named in the record but no longer in this dock are skipped, and a
    /// record naming none of them leaves the dock untouched for `restore_state`
    /// to handle the old way — an install that predates stacks, or a dock whose
    /// panels have all been removed, must not end up showing nothing.
    pub fn apply_stack_state(
        &mut self,
        state: &DockStackState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // `persistent_name` names a panel *type*, so a record can only ever
        // resolve to distinct entries — but it arrives from disk, and a
        // repeated name would otherwise yield an index twice and leave the
        // proportions describing more sections than there are.
        let mut indices: Vec<usize> = Vec::with_capacity(state.showing.len());
        for name in &state.showing {
            if let Some(ix) = self
                .panel_entries
                .iter()
                .position(|entry| entry.panel.persistent_name() == name)
                && !indices.contains(&ix)
            {
                indices.push(ix);
            }
        }
        if indices.is_empty() {
            return false;
        }
        // A dock that takes turns shows one panel, and a record written before it
        // did — or by hand — must not be the way a stack comes back. Restoring
        // two here would put the split back on the next launch, with nothing the
        // user did to explain it.
        if self.takes_turns() {
            indices.truncate(1);
        }

        for (ix, entry) in self.panel_entries.iter_mut().enumerate() {
            entry.visible = indices.contains(&ix);
        }
        self.active_panel_index = indices.first().copied();

        // Only honour recorded proportions that still describe this many
        // sections; `pane_axis` asserts the two agree, so a stale record has to
        // fall back to an even share rather than reach layout.
        //
        // Each share is checked as well as the sum: these arrive from the
        // key-value store, and `[3.0, -1.0]` sums to exactly the section count
        // while asking layout for a negative height. The sum alone says nothing
        // about the parts.
        if state.flexes.len() == indices.len()
            && state
                .flexes
                .iter()
                .all(|flex| flex.is_finite() && *flex > 0.)
            && (state.flexes.iter().sum::<f32>() - indices.len() as f32).abs() < 0.001
        {
            *self.stack_flexes.lock() = state.flexes.clone();
        } else {
            self.reset_stack_flexes();
        }

        for ix in &indices {
            if let Some(entry) = self.panel_entries.get(*ix) {
                entry.panel.set_active(true, window, cx);
            }
        }
        cx.notify();
        true
    }

    pub fn zoomed_panel(&self, window: &Window, cx: &App) -> Option<Arc<dyn PanelHandle>> {
        let entry = self.visible_entry()?;
        if entry.panel.is_zoomed(window, cx) {
            Some(entry.panel.clone())
        } else {
            None
        }
    }

    pub fn active_panel_size(&self) -> Option<PanelSizeState> {
        if self.is_open {
            self.size_governing_entry().map(|entry| entry.size_state)
        } else {
            None
        }
    }

    pub fn stored_panel_size(
        &self,
        panel: &dyn PanelHandle,
        window: &Window,
        cx: &App,
    ) -> Option<Pixels> {
        self.panel_entries
            .iter()
            .find(|entry| entry.panel.panel_id() == panel.panel_id())
            .map(|entry| {
                entry
                    .size_state
                    .size
                    .unwrap_or_else(|| entry.panel.default_size(window, cx))
            })
    }

    pub fn stored_panel_size_state(&self, panel: &dyn PanelHandle) -> Option<PanelSizeState> {
        self.panel_entries
            .iter()
            .find(|entry| entry.panel.panel_id() == panel.panel_id())
            .map(|entry| entry.size_state)
    }

    pub fn stored_active_panel_size(&self, window: &Window, cx: &App) -> Option<Pixels> {
        if self.is_open {
            self.size_governing_entry().map(|entry| {
                entry
                    .size_state
                    .size
                    .unwrap_or_else(|| entry.panel.default_size(window, cx))
            })
        } else {
            None
        }
    }

    pub fn set_panel_size_state(
        &mut self,
        panel: &dyn PanelHandle,
        size_state: PanelSizeState,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(entry) = self
            .panel_entries
            .iter_mut()
            .find(|entry| entry.panel.panel_id() == panel.panel_id())
        {
            entry.size_state = size_state;
            cx.notify();
            true
        } else {
            false
        }
    }

    pub fn toggle_panel_flexible_size(
        &mut self,
        panel: &dyn PanelHandle,
        current_size: Option<Pixels>,
        current_flex: Option<f32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self
            .panel_entries
            .iter_mut()
            .find(|entry| entry.panel.panel_id() == panel.panel_id())
        else {
            return;
        };
        let currently_flexible = entry.panel.has_flexible_size(window, cx);
        if currently_flexible {
            entry.size_state.size = current_size;
        } else {
            entry.size_state.flex = current_flex;
        }
        let panel_key = entry.panel.panel_key();
        let size_state = entry.size_state;
        let workspace = self.workspace.clone();
        entry
            .panel
            .set_flexible_size(!currently_flexible, window, cx);
        entry.panel.size_state_changed(window, cx);
        cx.defer(move |cx| {
            if let Some(workspace) = workspace.upgrade() {
                workspace.update(cx, |workspace, cx| {
                    workspace.persist_panel_size_state(panel_key, size_state, cx);
                });
            }
        });
        cx.notify();
    }

    pub fn resize_active_panel(
        &mut self,
        size: Option<Pixels>,
        flex: Option<f32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The governing entry, not the active one: in a dock that takes turns
        // the column has a single width, and writing a drag to whichever tab
        // was up meant the next tab ignored it.
        if let Some(index) = self.size_governing_index()
            && let Some(entry) = self.panel_entries.get_mut(index)
        {
            let (panel_key, size_state) =
                resize_panel_entry(self.position, entry, size, flex, window, cx);

            let workspace = self.workspace.clone();
            cx.defer(move |cx| {
                if let Some(workspace) = workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| {
                        workspace.persist_panel_size_state(panel_key, size_state, cx);
                    });
                }
            });
            cx.notify();
        }
    }

    pub fn resize_all_panels(
        &mut self,
        size: Option<Pixels>,
        flex: Option<f32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(active_panel_index) = self.active_panel_index else {
            return;
        };

        let active_panel_uses_flexible_width = {
            let Some(active_entry) = self.panel_entries.get(active_panel_index) else {
                return;
            };
            panel_uses_flexible_width(self.position, active_entry.panel.as_ref(), window, cx)
        };
        let mut size_states_to_persist = Vec::new();
        for entry in &mut self.panel_entries {
            if panel_uses_flexible_width(self.position, entry.panel.as_ref(), window, cx)
                == active_panel_uses_flexible_width
            {
                size_states_to_persist.push(resize_panel_entry(
                    self.position,
                    entry,
                    size,
                    flex,
                    window,
                    cx,
                ));
            }
        }

        let workspace = self.workspace.clone();
        cx.defer(move |cx| {
            if let Some(workspace) = workspace.upgrade() {
                workspace.update(cx, |workspace, cx| {
                    for (panel_key, size_state) in size_states_to_persist {
                        workspace.persist_panel_size_state(panel_key, size_state, cx);
                    }
                });
            }
        });

        cx.notify();
    }

    pub fn toggle_action(&self) -> Box<dyn Action> {
        match self.position {
            DockPosition::Left => crate::ToggleLeftDock.boxed_clone(),
            DockPosition::Bottom => crate::ToggleBottomDock.boxed_clone(),
            DockPosition::Right => crate::ToggleRightDock.boxed_clone(),
        }
    }

    fn dispatch_context() -> KeyContext {
        let mut dispatch_context = KeyContext::new_with_defaults();
        dispatch_context.add("Dock");

        dispatch_context
    }

    pub fn clamp_panel_size(&mut self, max_size: Pixels, window: &Window, cx: &mut App) {
        let max_size = (max_size - RESIZE_HANDLE_SIZE).abs();
        for entry in &mut self.panel_entries {
            let use_flexible = entry.panel.has_flexible_size(window, cx);
            if use_flexible {
                continue;
            }

            let size = entry
                .size_state
                .size
                .unwrap_or_else(|| entry.panel.default_size(window, cx));
            if size > max_size {
                entry.size_state.size = Some(max_size.max(RESIZE_HANDLE_SIZE));
            }
        }
    }

    /// Draws whatever the dock is showing: one panel, or a stack of them.
    ///
    /// A single panel is drawn exactly as it always was — no header, no axis
    /// element — so the case every dock is in most of the time pays nothing
    /// for the case that is new.
    fn render_showing(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // Counted before collecting, so the one-panel case — which is every
        // dock nearly all of the time — really does pay nothing for the case
        // that is new. Collecting first meant a `Vec` and an `Arc` clone per
        // dock per frame to reach a branch that wanted neither.
        let (first, has_more) = {
            let mut showing = self.visible_panels();
            let first = showing.next().cloned();
            (first, showing.next().is_some())
        };
        match first {
            Some(only) if !has_more => only
                .to_any()
                .cached(StyleRefinement::default().v_flex().size_full())
                .into_any_element(),
            Some(_) => {
                let showing: Vec<Arc<dyn PanelHandle>> = self.visible_panels().cloned().collect();
                self.render_stack(showing, window, cx)
            }
            None => div().into_any_element(),
        }
    }

    fn render_stack(
        &mut self,
        showing: Vec<Arc<dyn PanelHandle>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if showing.is_empty() {
            return div().into_any_element();
        }

        // Panels stack across the dock's own extent: a side dock is a column,
        // the bottom dock a row.
        let axis = match self.position.axis() {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        };

        let children: Vec<AnyElement> = showing
            .iter()
            .enumerate()
            .map(|(ix, panel)| self.render_stacked_panel(ix, panel, window, cx))
            .collect();

        pane_axis(
            axis,
            self.stack_element_basis(),
            self.stack_flexes.clone(),
            self.stack_bounding_boxes.clone(),
            self.workspace.clone(),
        )
        // Every child reports as not-a-leaf-pane, which is what switches off
        // the inactive-pane dimming and the active-pane border. Those speak
        // about editor panes; fading whichever panel is unfocused would be a
        // surprise nobody asked this dock for.
        .with_is_leaf_pane_mask(vec![false; children.len()])
        .children(children)
        .into_any_element()
    }

    /// One panel in a stack: a header naming it, then the panel.
    ///
    /// The header exists only here. A dock showing one panel has the rail or
    /// the status bar to say which it is, but a stack has to name its own
    /// sections and give each a way out.
    fn render_stacked_panel(
        &self,
        ix: usize,
        panel: &Arc<dyn PanelHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let panel_id = panel.panel_id();
        let name = SharedString::from(panel.persistent_name());
        let icon = panel.icon(window, cx);
        let colors = cx.theme().colors();

        v_flex()
            .id(("dock-stacked-panel", panel_id))
            // Keyed by position in the stack rather than by name: two panels of
            // one kind can stack, and a test asserting how they divide the dock
            // is asking about first and second, not about which is which.
            .debug_selector(move || format!("dock-stacked-panel:{ix}"))
            .size_full()
            .bg(colors.panel_background)
            .child(
                h_flex()
                    .flex_none()
                    .w_full()
                    .px_2()
                    .py_1()
                    .justify_between()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        h_flex()
                            .gap_1p5()
                            .children(icon.map(|icon| {
                                Icon::new(icon).size(IconSize::Small).color(Color::Muted)
                            }))
                            .child(Label::new(name).size(LabelSize::Small)),
                    )
                    .child(
                        IconButton::new(("hide-stacked-panel", panel_id), IconName::Close)
                            .icon_size(IconSize::Small)
                            .tooltip(move |_window, cx| Tooltip::simple("Hide Panel", cx))
                            .on_click(cx.listener(move |dock, _, window, cx| {
                                dock.hide_panel_by_id(panel_id, window, cx);
                            })),
                    ),
            )
            // `flex_1` with a floor of zero in a column, so the panel sizes
            // against a definite height instead of running past the header.
            .child(
                div().flex_1().min_h_0().child(
                    panel
                        .to_any()
                        .cached(StyleRefinement::default().v_flex().size_full()),
                ),
            )
            .into_any_element()
    }

    /// The dock's own panels, named in a strip along its top edge.
    ///
    /// Only the column facing the rail draws one -- see `dock_header_draws_panel`
    /// for why that column and no other. These are the status bar's buttons,
    /// moved rather than reimplemented: `DockButton` builds both, so the menu
    /// that re-docks a panel and switches it between flexible and fixed width
    /// comes along with them. Nothing about them changes except that they now
    /// stand on top of what they open instead of a window's width away from it.
    ///
    /// Called before the resize-handle closure in `render`, which holds a borrow
    /// of `self` and `cx` for as long as it lives.
    fn render_header(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !dock_header_draws_panel(self.position) {
            return None;
        }
        // An own column has affordances of its own beside the rail; it is not the
        // tool column this rule is about. Enforced here rather than left to the
        // fact that no own column contributes an icon today.
        if self.column.is_own_column() {
            return None;
        }

        // The handle, for the menu's deferred closures only. Never read back --
        // this dock is leased for the duration of its own render.
        let dock_entity = cx.entity();
        // Left to right in registration order. `PanelButtons` reverses this for
        // the right dock so its buttons read inward from the window edge; a strip
        // that starts at the panel's own left edge has nothing to read inward
        // from.
        let buttons: Vec<AnyElement> = self
            .panel_entries
            .iter()
            .filter_map(|entry| {
                Some(
                    DockButton::for_entry(
                        self,
                        &dock_entity,
                        entry,
                        PanelButtonPlacement::DockHeader,
                        window,
                        cx,
                    )?
                    .render(window, cx),
                )
            })
            .collect();

        // No strip at all rather than an empty one: an empty header still costs
        // its padding and its border, which reads as a stray rule above the
        // panel.
        if buttons.is_empty() {
            return None;
        }

        Some(
            h_flex()
                .id("dock-header")
                .debug_selector(|| "dock-header".into())
                .flex_none()
                .w_full()
                .px_1()
                .py_0p5()
                .gap_1()
                // A dock narrower than its own buttons clips them rather than
                // pushing the panel below out of the card.
                .overflow_x_hidden()
                .border_b_1()
                .border_color(cx.theme().colors().border)
                .children(buttons)
                .into_any_element(),
        )
    }

    pub(crate) fn load_persisted_size_state(
        workspace: &Workspace,
        panel_key: &'static str,
        cx: &App,
    ) -> Option<PanelSizeState> {
        let workspace_id = workspace
            .database_id()
            .map(|id| i64::from(id).to_string())
            .or(workspace.session_id())?;
        let kvp = KeyValueStore::global(cx);
        let scope = kvp.scoped(PANEL_SIZE_STATE_KEY);
        scope
            .read(&format!("{workspace_id}:{panel_key}"))
            .log_err()
            .flatten()
            .and_then(|json| serde_json::from_str::<PanelSizeState>(&json).log_err())
    }
}

impl Render for Dock {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dispatch_context = Self::dispatch_context();
        // Built before the resize-handle closure below, which borrows `self`
        // and `cx` for as long as it lives; drawing the panels needs both
        // mutably.
        let showing = self
            .visible_entry()
            .is_some()
            .then(|| self.render_showing(window, cx));
        if let Some(showing) = showing {
            // Built here, before the resize-handle closure below takes its own
            // borrow of `self` and `cx`.
            let header = self.render_header(window, cx);
            let position = self.position;
            let column = self.column;
            let create_resize_handle = || {
                let handle = div()
                    .id("resize-handle")
                    .on_drag(DraggedDock { position, column }, |dock, _, _, cx| {
                        cx.stop_propagation();
                        cx.new(|_| *dock)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|dock, e: &MouseUpEvent, window, cx| {
                            if e.click_count == 2 {
                                dock.resize_active_panel(None, None, window, cx);
                                dock.workspace
                                    .update(cx, |workspace, cx| {
                                        workspace.serialize_workspace(window, cx);
                                    })
                                    .ok();
                                cx.stop_propagation();
                            }
                        }),
                    )
                    .occlude();
                match self.position() {
                    DockPosition::Left => deferred(
                        handle
                            .absolute()
                            .right(-RESIZE_HANDLE_SIZE / 2.)
                            .top(px(0.))
                            .h_full()
                            .w(RESIZE_HANDLE_SIZE)
                            .cursor_col_resize(),
                    ),
                    DockPosition::Bottom => deferred(
                        handle
                            .absolute()
                            .top(-RESIZE_HANDLE_SIZE / 2.)
                            .left(px(0.))
                            .w_full()
                            .h(RESIZE_HANDLE_SIZE)
                            .cursor_row_resize(),
                    ),
                    DockPosition::Right => deferred(
                        handle
                            .absolute()
                            .top(px(0.))
                            .left(-RESIZE_HANDLE_SIZE / 2.)
                            .h_full()
                            .w(RESIZE_HANDLE_SIZE)
                            .cursor_col_resize(),
                    ),
                }
            };

            div()
                .id("dock-panel")
                .debug_selector(|| "dock-panel".into())
                .key_context(dispatch_context)
                .track_focus(&self.focus_handle(cx))
                .focus_follows_mouse(self.focus_follows_mouse, cx)
                .flex()
                .overflow_hidden()
                .map(|this| match self.position().axis() {
                    // Width and height are always set on the workspace wrapper in
                    // render_dock, so fill whatever space the wrapper provides.
                    Axis::Horizontal => this.w_full().h_full().flex_row(),
                    Axis::Vertical => this.h_full().w_full().flex_col(),
                })
                // Every dock is a card, the way the centre group is: inset by
                // the same margin, rounded by the same radius. Drawn here
                // rather than in each panel so no panel can be the one that
                // forgets, and so no panel has to be an exception.
                //
                // The inset is padding on a `size_full` box, not a margin: a
                // 100% box plus a margin exceeds its parent and the parent
                // clips the overflow, silently eating the very seam the margin
                // exists to create. A percentage resolves against the content
                // box, so the padding *is* the gap.
                //
                // `overflow_hidden` on the rounded box is load-bearing: panels
                // paint their own square backgrounds inside it, and without the
                // clip those corners paint straight over the radius.
                .child(
                    div().size_full().p(SURFACE_MARGIN).child(
                        div()
                            .size_full()
                            .rounded(SURFACE_ROUNDING)
                            .border_1()
                            .border_color(cx.theme().colors().border)
                            .bg(cx.theme().colors().panel_background)
                            .overflow_hidden()
                            .child(match header {
                                // The panel gets `flex_1` with a floor of zero
                                // so it sizes against a definite height instead
                                // of running past the header -- the same shape
                                // `render_stacked_panel` uses for the same
                                // reason.
                                Some(header) => v_flex()
                                    .size_full()
                                    .child(header)
                                    .child(div().flex_1().min_h_0().child(showing))
                                    .into_any_element(),
                                // Handed through untouched with no header, which
                                // keeps every dock that has never had one on
                                // exactly the element tree it was laid out
                                // against.
                                None => showing,
                            }),
                    ),
                )
                .when(self.resizable(cx), |this| {
                    this.child(create_resize_handle())
                })
        } else {
            div()
                .id("dock-panel")
                .key_context(dispatch_context)
                .track_focus(&self.focus_handle(cx))
        }
    }
}

impl PanelButtons {
    pub fn new(dock: Entity<Dock>, cx: &mut Context<Self>) -> Self {
        cx.observe(&dock, |_, _, cx| cx.notify()).detach();
        let settings_subscription = cx.observe_global::<SettingsStore>(|_, cx| cx.notify());
        Self {
            dock,
            _settings_subscription: settings_subscription,
        }
    }

    /// Whether the project rail already carries this dock's buttons.
    ///
    /// Deliberately reads settings only. The obvious check -- ask the workspace
    /// whether a sidebar is registered -- would borrow an entity that is mid-render
    /// around this call, which is the re-entrancy that aborts the process.
    ///
    /// That makes `disable_ai` a proxy for `MultiWorkspace::sidebar_render_state`'s
    /// `sidebar.is_some() && !disable_ai`, and the two part company while the
    /// sidebar is unregistered: `zed::init` registers it from a `cx.defer` during
    /// window construction, and skips it entirely for a windowless `MultiWorkspace`.
    /// Both are before or without a first paint, so no frame is drawn from the
    /// disagreement -- but a caller that starts registering the sidebar later, or
    /// conditionally, would silently strip these buttons with no rail to receive
    /// them. Keep registration unconditional and eager.
    fn rail_draws(&self, panel_name: &str, cx: &App) -> bool {
        rail_draws_panel(
            panel_name,
            self.dock.read(cx).position,
            !project::DisableAiSettings::get_global(cx).disable_ai,
        )
    }
}

/// The project panel is the one panel the rail never adopts. Re-docking it should
/// slide its button along the status bar, not lift it into the rail -- the rail is
/// already the project-switching column, and a second project affordance in it
/// reads as a duplicate of what is right below.
const PANEL_ALWAYS_IN_STATUS_BAR: &str = "Project Panel";

/// Whether the project rail draws this panel's button -- and therefore whether the
/// status bar must not.
///
/// Per PANEL, not per dock: the earlier per-dock form swept every panel sharing the
/// rail's edge into it, so re-docking the project panel onto that edge teleported
/// its button out of the status bar.
///
/// Split out from the settings lookup so it can be tested without a window: the
/// rule is a name and three booleans, while reaching it needs a whole workspace.
pub fn rail_draws_panel(panel_name: &str, position: DockPosition, rail_drawn: bool) -> bool {
    rail_drawn
        && panel_name != PANEL_ALWAYS_IN_STATUS_BAR
        // The rail stands against one edge -- `Workspace::OWN_COLUMN_POSITION` --
        // and nothing represents the bottom, so those two docks are never drawn
        // by it.
        && position == Workspace::OWN_COLUMN_POSITION
}

/// Whether this dock names its own panels in a header along its top edge -- and
/// therefore whether the status bar must not.
///
/// One dock does: the column facing the rail. The rail draws the buttons for its
/// own side and nothing represents the bottom dock, which left this one column
/// with no surface of its own -- its buttons sat at the far end of the status
/// bar, a window's width away from the panel they open.
///
/// Derived from the rail's side rather than written down as an edge, so the two
/// cannot drift apart: move the rail and the header moves with it.
///
/// Split out of `render` for the same reason as `rail_draws_panel` above: the
/// rule is one enum, while reaching it needs a whole workspace.
pub fn dock_header_draws_panel(position: DockPosition) -> bool {
    matches!(position, DockPosition::Left | DockPosition::Right)
        && position != Workspace::OWN_COLUMN_POSITION
}

/// The edges the dock menu is allowed to offer for a panel.
///
/// A single edge is NOT a choice: drawing it yields one already-ticked entry that
/// does nothing when clicked. Anything with two or three gets the list unchanged,
/// so this is a no-op for every panel that really can move.
///
/// Split out of `render` so the rule can be tested without a window -- the same
/// reason `rail_draws_panel` above is split out.
pub fn dockable_positions(panel: &Arc<dyn PanelHandle>, cx: &App) -> Vec<DockPosition> {
    let valid: Vec<DockPosition> = [
        DockPosition::Left,
        DockPosition::Right,
        DockPosition::Bottom,
    ]
    .into_iter()
    .filter(|position| panel.position_is_valid(*position, cx))
    .collect();

    if valid.len() >= 2 { valid } else { Vec::new() }
}

/// The dock button itself, in one place.
///
/// A free function rather than a closure so the branch that attaches a
/// right-click menu can rebuild it on every trigger call while the branch
/// without one calls it directly -- two copies of this would drift on the
/// element id, and the id is what invalidates the cached tooltip.
#[allow(clippy::too_many_arguments)]
fn panel_button(
    name: &'static str,
    icon: IconName,
    is_active_button: bool,
    is_menu_open: bool,
    action: Box<dyn Action>,
    tooltip: SharedString,
    focus_handle: FocusHandle,
    icon_label: Option<String>,
) -> Div {
    // Include active state in element ID to invalidate the cached
    // tooltip when panel state changes (e.g., via keyboard shortcut)
    let button = IconButton::new((name, is_active_button as u64), icon)
        .icon_size(IconSize::Small)
        .toggle_state(is_active_button)
        .on_click({
            let action = action.boxed_clone();
            move |_, window, cx| {
                window.focus(&focus_handle, cx);
                window.dispatch_action(action.boxed_clone(), cx)
            }
        })
        .when(!is_menu_open, |this| {
            this.tooltip(move |_window, cx| Tooltip::for_action(tooltip.clone(), &*action, cx))
        });

    div().relative().child(button).when_some(
        icon_label
            .filter(|_| !is_active_button)
            .and_then(|label| label.parse::<usize>().ok()),
        |this, count| this.child(CountBadge::new(count)),
    )
}

/// Where a dock's buttons are drawn.
///
/// The two surfaces differ in less than they look: the menu hangs off a different
/// corner and the status bar carries a divider against its neighbours. What the
/// buttons dispatch, when they light, and what their menu offers is shared -- and
/// shared by construction, not by two copies kept in step by hand.
#[derive(Copy, Clone, PartialEq)]
enum PanelButtonPlacement {
    StatusBar,
    DockHeader,
}

impl PanelButtonPlacement {
    /// Which corner the right-click menu hangs from.
    fn menu_anchors(self, position: DockPosition) -> (Anchor, Anchor) {
        match self {
            // Downward, over the panel below. A header stands at the top of the
            // window, so a menu attached above it would open off screen.
            Self::DockHeader => (Anchor::TopLeft, Anchor::BottomLeft),
            Self::StatusBar => match position {
                DockPosition::Left => (Anchor::BottomLeft, Anchor::TopLeft),
                DockPosition::Bottom | DockPosition::Right => {
                    (Anchor::BottomRight, Anchor::TopRight)
                }
            },
        }
    }

    /// What a test looks this button up by. Named per surface rather than per
    /// panel: what a test needs to settle is which surface drew it, and a
    /// selector carrying only the panel's name cannot answer that.
    fn selector_prefix(self) -> &'static str {
        match self {
            Self::StatusBar => "status-bar-button",
            Self::DockHeader => "dock-header-button",
        }
    }
}

/// One dock button, with everything behind it.
///
/// Built here rather than at each surface because the right-click menu is the
/// only route to re-docking a panel and the only route at all to switching one
/// between flexible and fixed width -- `Workspace::toggle_dock_panel_flexible_size`
/// has no other caller in the tree, no action, no keybinding, no palette entry.
/// A surface that drew the button without the menu would take both away, and for
/// the flex toggle there would be no way back.
struct DockButton {
    /// Handed to the menu's click handlers, which run long after this render.
    /// Deliberately never read here: a dock building its own header is leased
    /// mid-render, and reading it back through the entity aborts the process.
    dock: Entity<Dock>,
    workspace: WeakEntity<Workspace>,
    panel: Arc<dyn PanelHandle>,
    position: DockPosition,
    placement: PanelButtonPlacement,
    name: &'static str,
    icon: IconName,
    is_active_button: bool,
    action: Box<dyn Action>,
    tooltip: SharedString,
    focus_handle: FocusHandle,
    icon_label: Option<String>,
}

impl DockButton {
    /// `None` for a panel that contributes no icon -- a panel whose button
    /// setting is off, which is how those settings take effect.
    ///
    /// The dock arrives as state (`dock`) *and* as a handle (`dock_entity`) for
    /// the reason on the `dock` field: only the deferred closures may hold the
    /// handle.
    fn for_entry(
        dock: &Dock,
        dock_entity: &Entity<Dock>,
        entry: &PanelEntry,
        placement: PanelButtonPlacement,
        window: &Window,
        cx: &App,
    ) -> Option<Self> {
        let icon = entry.panel.icon(window, cx)?;
        let icon_tooltip = entry
            .panel
            .icon_tooltip(window, cx)
            .ok_or_else(|| anyhow::anyhow!("can't render a panel button without an icon tooltip"))
            .log_err()?;

        // Lit per panel rather than per dock: a stacking dock can have several up
        // at once, so several of its buttons can be lit at once. A dock that
        // takes turns lights exactly one, which falls out of the same reading
        // rather than needing a second. The action for one that is up still
        // closes the dock — this button has no way to name a single panel, and
        // `Panel::toggle_action` respects `close_panel_on_toggle`, so it cannot
        // be relied on to hide.
        let is_active_button = dock.is_open && entry.visible;
        let (action, tooltip): (Box<dyn Action>, SharedString) = if is_active_button {
            (
                dock.toggle_action(),
                format!("Close {} Dock", dock.position.label()).into(),
            )
        } else {
            (entry.panel.toggle_action(window, cx), icon_tooltip.into())
        };

        Some(Self {
            dock: dock_entity.clone(),
            workspace: dock.workspace.clone(),
            panel: entry.panel.clone(),
            position: dock.position,
            placement,
            name: entry.panel.persistent_name(),
            icon,
            is_active_button,
            action,
            tooltip,
            focus_handle: dock.focus_handle(cx),
            icon_label: entry.panel.icon_label(window, cx),
        })
    }

    fn render(self, window: &Window, cx: &App) -> AnyElement {
        let Self {
            dock,
            workspace,
            panel,
            position,
            placement,
            name,
            icon,
            is_active_button,
            action,
            tooltip,
            focus_handle,
            icon_label,
        } = self;
        let selector = placement.selector_prefix();

        let supports_flexible = panel.supports_flexible_size(cx);
        let currently_flexible = panel.has_flexible_size(window, cx);

        // Nothing to offer and nothing to flex means no menu at all -- an
        // attached menu with zero entries draws an empty popover.
        let dockable = dockable_positions(&panel, cx);
        if dockable.is_empty() && !supports_flexible {
            return panel_button(
                name,
                icon,
                is_active_button,
                false,
                action,
                tooltip,
                focus_handle,
                icon_label,
            )
            .debug_selector(move || format!("{selector}:{name}"))
            .into_any_element();
        }

        let (menu_anchor, menu_attach) = placement.menu_anchors(position);
        right_click_menu(name)
            .menu(move |window, cx| {
                let dockable = dockable.clone();
                ContextMenu::build(window, cx, |mut menu, _, _cx| {
                    let has_position_entries = !dockable.is_empty();
                    for dock_position in dockable {
                        let is_current = dock_position == position;
                        let panel = panel.clone();
                        menu = menu.toggleable_entry(
                            format!("Dock {}", dock_position.label()),
                            is_current,
                            IconPosition::Start,
                            None,
                            move |window, cx| {
                                if !is_current {
                                    panel.set_position(dock_position, window, cx);
                                }
                            },
                        );
                    }
                    if supports_flexible {
                        if has_position_entries {
                            menu = menu.separator();
                        }
                        let panel_for_flex = panel.clone();
                        let dock_for_flex = dock.clone();
                        let workspace_for_flex = workspace.clone();
                        menu = menu.toggleable_entry(
                            "Flex Width",
                            currently_flexible,
                            IconPosition::Start,
                            None,
                            move |window, cx| {
                                if !currently_flexible {
                                    if let Some(ws) = workspace_for_flex.upgrade() {
                                        ws.update(cx, |workspace, cx| {
                                            workspace.toggle_dock_panel_flexible_size(
                                                &dock_for_flex,
                                                panel_for_flex.as_ref(),
                                                window,
                                                cx,
                                            );
                                        });
                                    }
                                }
                            },
                        );
                        let panel_for_fixed = panel.clone();
                        let dock_for_fixed = dock.clone();
                        let workspace_for_fixed = workspace.clone();
                        menu = menu.toggleable_entry(
                            "Fixed Width",
                            !currently_flexible,
                            IconPosition::Start,
                            None,
                            move |window, cx| {
                                if currently_flexible {
                                    if let Some(ws) = workspace_for_fixed.upgrade() {
                                        ws.update(cx, |workspace, cx| {
                                            workspace.toggle_dock_panel_flexible_size(
                                                &dock_for_fixed,
                                                panel_for_fixed.as_ref(),
                                                window,
                                                cx,
                                            );
                                        });
                                    }
                                }
                            },
                        );
                    }
                    menu
                })
            })
            .anchor(menu_anchor)
            .attach(menu_attach)
            .trigger(move |is_menu_open, _window, _cx| {
                panel_button(
                    name,
                    icon,
                    is_active_button,
                    is_menu_open,
                    action.boxed_clone(),
                    tooltip.clone(),
                    focus_handle.clone(),
                    icon_label.clone(),
                )
                .debug_selector(move || format!("{selector}:{name}"))
            })
            .into_any_element()
    }
}

impl Render for PanelButtons {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dock = self.dock.read(cx);
        let dock_position = dock.position;

        let dock_entity = self.dock.clone();
        let mut buttons: Vec<AnyElement> = dock
            .panel_entries
            .iter()
            .filter_map(|entry| {
                // Skip only what the rail is already drawing: clearing the whole
                // dock would take the project panel's button with it, and that one
                // belongs here whichever edge it is docked to.
                if self.rail_draws(entry.panel.persistent_name(), cx) {
                    return None;
                }
                // And skip the dock that names its panels itself. Both surfaces
                // read the one predicate, so neither can claim a panel the other
                // has already taken -- nor drop one both think the other has.
                if dock_header_draws_panel(dock_position) {
                    return None;
                }
                Some(
                    DockButton::for_entry(
                        dock,
                        &dock_entity,
                        entry,
                        PanelButtonPlacement::StatusBar,
                        window,
                        cx,
                    )?
                    .render(window, cx),
                )
            })
            .collect();

        if dock_position == DockPosition::Right {
            buttons.reverse();
        }

        let has_buttons = !buttons.is_empty();

        h_flex()
            .gap_1()
            .when(
                has_buttons
                    && (dock.position == DockPosition::Bottom
                        || dock.position == DockPosition::Right),
                |this| this.child(Divider::vertical().color(DividerColor::Border)),
            )
            .children(buttons)
            .when(has_buttons && dock.position == DockPosition::Left, |this| {
                this.child(Divider::vertical().color(DividerColor::Border))
            })
    }
}

impl StatusItemView for PanelButtons {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn crate::ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        // Nothing to do, panel buttons don't depend on the active center item
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod test {
    use super::*;
    use gpui::{App, Context, Window, actions, div};

    pub struct TestPanel {
        pub position: DockPosition,
        pub zoomed: bool,
        pub active: bool,
        pub focus_handle: FocusHandle,
        pub default_size: Pixels,
        pub flexible: bool,
        pub own_column: Option<DockColumn>,
        pub activation_priority: u32,
        /// Every edge, by default -- what `position_is_valid` used to answer
        /// unconditionally, so existing tests keep their behaviour. Narrow it to
        /// test a panel that cannot move.
        pub valid_positions: Vec<DockPosition>,
        /// Defaults to `None`, matching a panel that contributes no dock button.
        /// Set it when a test needs the panel to appear in an icon list.
        pub icon: Option<ui::IconName>,
    }
    actions!(test_only, [ToggleTestPanel]);

    impl EventEmitter<PanelEvent> for TestPanel {}

    impl TestPanel {
        pub fn new(position: DockPosition, activation_priority: u32, cx: &mut App) -> Self {
            Self {
                position,
                zoomed: false,
                active: false,
                focus_handle: cx.focus_handle(),
                default_size: px(300.),
                flexible: false,
                own_column: None,
                activation_priority,
                valid_positions: vec![
                    DockPosition::Left,
                    DockPosition::Right,
                    DockPosition::Bottom,
                ],
                icon: None,
            }
        }

        pub fn new_flexible(
            position: DockPosition,
            activation_priority: u32,
            cx: &mut App,
        ) -> Self {
            Self {
                flexible: true,
                ..Self::new(position, activation_priority, cx)
            }
        }

        /// A panel the workspace routes into a column of its own rather than
        /// the dock named by `position`.
        pub fn new_in_column(
            column: DockColumn,
            position: DockPosition,
            activation_priority: u32,
            cx: &mut App,
        ) -> Self {
            Self {
                own_column: Some(column),
                ..Self::new(position, activation_priority, cx)
            }
        }

        pub fn new_database(
            position: DockPosition,
            activation_priority: u32,
            cx: &mut App,
        ) -> Self {
            Self::new_in_column(DockColumn::Database, position, activation_priority, cx)
        }
    }

    impl Render for TestPanel {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().id("test").track_focus(&self.focus_handle(cx))
        }
    }

    /// A second panel *type*, so a test can build a stack of two sections.
    ///
    /// `Panel::persistent_name` is a static method naming a type, so any number
    /// of `TestPanel`s resolve to one name in a record — which is why every
    /// stack test above collapses to a single section, and why the guard on
    /// stale proportions had nothing exercising it. Two sections need two types.
    pub struct OtherTestPanel(pub TestPanel);

    impl OtherTestPanel {
        pub fn new(position: DockPosition, activation_priority: u32, cx: &mut App) -> Self {
            Self(TestPanel::new(position, activation_priority, cx))
        }
    }

    impl EventEmitter<PanelEvent> for OtherTestPanel {}

    impl Render for OtherTestPanel {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().id("other-test-panel")
        }
    }

    impl Focusable for OtherTestPanel {
        fn focus_handle(&self, _cx: &App) -> FocusHandle {
            self.0.focus_handle.clone()
        }
    }

    impl Panel for OtherTestPanel {
        fn persistent_name() -> &'static str {
            "OtherTestPanel"
        }

        fn panel_key() -> &'static str {
            "OtherTestPanel"
        }

        fn position(&self, _window: &Window, _: &App) -> DockPosition {
            self.0.position
        }

        fn position_is_valid(&self, _: DockPosition) -> bool {
            true
        }

        fn set_position(&mut self, position: DockPosition, _: &mut Window, _: &mut Context<Self>) {
            self.0.position = position;
        }

        fn default_size(&self, _window: &Window, _: &App) -> Pixels {
            self.0.default_size
        }

        fn toggle_action(&self) -> Box<dyn Action> {
            ToggleTestPanel.boxed_clone()
        }

        /// `None`, like `TestPanel`'s default: a panel with no icon draws no
        /// dock button, which keeps this out of the button-list assertions.
        /// Delegated to the wrapped panel, like everything else here: a stack
        /// test that needs two named sections has to be able to give each an
        /// icon, and an icon is what puts a panel in a dock's header. Still
        /// `None` by default -- `TestPanel::icon` is.
        fn icon(&self, window: &Window, cx: &App) -> Option<ui::IconName> {
            Panel::icon(&self.0, window, cx)
        }

        fn icon_tooltip(&self, window: &Window, cx: &App) -> Option<&'static str> {
            Panel::icon_tooltip(&self.0, window, cx)
        }

        fn set_active(&mut self, active: bool, _window: &mut Window, _cx: &mut Context<Self>) {
            self.0.active = active;
        }

        fn activation_priority(&self) -> u32 {
            self.0.activation_priority
        }
    }

    impl Panel for TestPanel {
        fn persistent_name() -> &'static str {
            "TestPanel"
        }

        fn panel_key() -> &'static str {
            "TestPanel"
        }

        fn position(&self, _window: &Window, _: &App) -> super::DockPosition {
            self.position
        }

        fn position_is_valid(&self, position: super::DockPosition) -> bool {
            self.valid_positions.contains(&position)
        }

        fn set_position(&mut self, position: DockPosition, _: &mut Window, cx: &mut Context<Self>) {
            self.position = position;
            cx.update_global::<SettingsStore, _>(|_, _| {});
        }

        fn default_size(&self, _window: &Window, _: &App) -> Pixels {
            self.default_size
        }

        fn initial_size_state(&self, _window: &Window, _: &App) -> PanelSizeState {
            PanelSizeState {
                size: None,
                flex: None,
            }
        }

        fn supports_flexible_size(&self) -> bool {
            self.flexible
        }

        fn has_flexible_size(&self, _window: &Window, _: &App) -> bool {
            self.flexible
        }

        fn set_flexible_size(
            &mut self,
            flexible: bool,
            _window: &mut Window,
            _cx: &mut Context<Self>,
        ) {
            self.flexible = flexible;
        }

        fn icon(&self, _window: &Window, _: &App) -> Option<ui::IconName> {
            self.icon
        }

        fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
            self.icon.map(|_| "Test Panel")
        }

        fn toggle_action(&self) -> Box<dyn Action> {
            ToggleTestPanel.boxed_clone()
        }

        fn is_zoomed(&self, _window: &Window, _: &App) -> bool {
            self.zoomed
        }

        fn set_zoomed(&mut self, zoomed: bool, _window: &mut Window, _cx: &mut Context<Self>) {
            self.zoomed = zoomed;
        }

        fn set_active(&mut self, active: bool, _window: &mut Window, _cx: &mut Context<Self>) {
            self.active = active;
        }

        fn activation_priority(&self) -> u32 {
            self.activation_priority
        }

        fn own_column(&self) -> Option<DockColumn> {
            self.own_column
        }
    }

    impl Focusable for TestPanel {
        fn focus_handle(&self, _cx: &App) -> FocusHandle {
            self.focus_handle.clone()
        }
    }
}

#[cfg(test)]
mod rail_coverage_tests {
    use super::{
        DockPosition, PANEL_ALWAYS_IN_STATUS_BAR, Workspace, dock_header_draws_panel,
        rail_draws_panel,
    };

    const ORDINARY: &str = "Outline Panel";

    /// Get this backwards and the status bar either doubles every rail button or
    /// hides the one dock the rail cannot reach -- both silent, neither caught by
    /// anything that only checks the app still draws.
    #[test]
    fn the_rail_draws_the_panels_on_its_own_edge_and_nothing_else() {
        // The negative arms carry this test: only asserting that Left is drawn
        // would pass for a rule that adopts every dock.
        for position in [
            DockPosition::Left,
            DockPosition::Right,
            DockPosition::Bottom,
        ] {
            assert_eq!(
                rail_draws_panel(ORDINARY, position, true),
                position == Workspace::OWN_COLUMN_POSITION,
                "the rail stands on {:?}, so it must draw a {position:?} dock only when they match",
                Workspace::OWN_COLUMN_POSITION
            );
        }

        // With no rail drawn, every panel keeps its status-bar button.
        for position in [
            DockPosition::Left,
            DockPosition::Right,
            DockPosition::Bottom,
        ] {
            assert!(!rail_draws_panel(ORDINARY, position, false));
        }
    }

    /// The reported defect: the project panel began in the status bar, and
    /// right-click -> Dock Right (onto the rail's own edge) did not slide its
    /// button to the other end of the status bar -- it lifted the button into the
    /// rail, because the rule was per dock rather than per panel.
    #[test]
    fn re_docking_the_project_panel_never_lifts_it_into_the_rail() {
        for position in [
            DockPosition::Left,
            DockPosition::Right,
            DockPosition::Bottom,
        ] {
            assert!(
                !rail_draws_panel(PANEL_ALWAYS_IN_STATUS_BAR, position, true),
                "the rail must leave the project panel in the status bar \
                 even when it is docked {position:?}"
            );
        }
    }

    /// The third surface. Get this wrong in one direction and the panel is named
    /// twice -- once over itself and once at the far end of the status bar; wrong
    /// in the other and a dock loses its buttons entirely.
    #[test]
    fn the_header_draws_the_column_facing_the_rail_and_nothing_else() {
        for position in [
            DockPosition::Left,
            DockPosition::Right,
            DockPosition::Bottom,
        ] {
            let expected =
                position != DockPosition::Bottom && position != Workspace::OWN_COLUMN_POSITION;
            assert_eq!(
                dock_header_draws_panel(position),
                expected,
                "the rail stands on {:?}, so the header belongs to the other side \
                 and never to the bottom -- got {} for {position:?}",
                Workspace::OWN_COLUMN_POSITION,
                dock_header_draws_panel(position)
            );
        }
    }

    /// Exactly one surface per dock. The rail and the header both claiming the
    /// rail's own side is the doubling this pair of rules exists to prevent, and
    /// it would draw the same button twice in two places.
    #[test]
    fn no_dock_is_claimed_by_both_the_rail_and_a_header() {
        for position in [
            DockPosition::Left,
            DockPosition::Right,
            DockPosition::Bottom,
        ] {
            assert!(
                !(rail_draws_panel(ORDINARY, position, true) && dock_header_draws_panel(position)),
                "the {position:?} dock is claimed by both the rail and its own header"
            );
        }
    }
}

#[cfg(test)]
mod dockable_position_tests {
    use super::{DockPosition, PanelHandle, dockable_positions, test::TestPanel};
    use gpui::{AppContext as _, TestAppContext, VisualTestContext};
    use settings::SettingsStore;
    use std::sync::Arc;

    /// `TestPanel::set_position` pokes the settings store on its way through, so
    /// the two tests that move a panel need one registered.
    fn init_settings(cx: &mut VisualTestContext) {
        cx.update(|_window, cx| {
            let settings_store = SettingsStore::test(cx);
            cx.set_global(settings_store);
        });
    }

    fn panel(
        cx: &mut VisualTestContext,
        position: DockPosition,
        valid: Vec<DockPosition>,
    ) -> Arc<dyn PanelHandle> {
        let entity = cx.update(|_window, cx| {
            cx.new(|cx| {
                let mut panel = TestPanel::new(position, 0, cx);
                panel.valid_positions = valid;
                panel
            })
        });
        Arc::new(entity) as Arc<dyn PanelHandle>
    }

    /// One edge is not a choice. The two- and three-edge arms are the regression
    /// half: every panel that really can move must keep the list it has today.
    #[gpui::test]
    fn a_panel_with_one_valid_position_is_offered_none(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();

        let one = panel(cx, DockPosition::Right, vec![DockPosition::Right]);
        let two = panel(
            cx,
            DockPosition::Left,
            vec![DockPosition::Left, DockPosition::Right],
        );
        let three = panel(
            cx,
            DockPosition::Left,
            vec![
                DockPosition::Left,
                DockPosition::Right,
                DockPosition::Bottom,
            ],
        );

        cx.update(|_window, cx| {
            assert!(
                dockable_positions(&one, cx).is_empty(),
                "a single valid edge yields one ticked entry that does nothing -- offer none"
            );
            assert_eq!(
                dockable_positions(&two, cx),
                vec![DockPosition::Left, DockPosition::Right],
                "two edges is a real choice and must be offered unchanged"
            );
            assert_eq!(
                dockable_positions(&three, cx),
                vec![
                    DockPosition::Left,
                    DockPosition::Right,
                    DockPosition::Bottom
                ],
                "three edges must be offered unchanged"
            );
        });
    }

    /// Before the wrap was indexed into the valid list this landed on `Left` --
    /// an edge the panel had just refused.
    #[gpui::test]
    fn moving_a_one_position_panel_leaves_it_alone(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        init_settings(cx);
        let one = panel(cx, DockPosition::Right, vec![DockPosition::Right]);

        cx.update(|window, cx| one.move_to_next_position(window, cx));

        cx.update(|window, cx| {
            assert_eq!(
                one.position(window, cx),
                DockPosition::Right,
                "a panel with nowhere to go must stay where it is"
            );
        });
    }

    /// The wrap a three-edge panel depends on. The order is the iteration order
    /// inside `move_to_next_position`, which is not the menu's order.
    #[gpui::test]
    fn moving_a_three_position_panel_still_cycles(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        init_settings(cx);
        let three = panel(
            cx,
            DockPosition::Left,
            vec![
                DockPosition::Left,
                DockPosition::Right,
                DockPosition::Bottom,
            ],
        );

        for expected in [
            DockPosition::Bottom,
            DockPosition::Right,
            DockPosition::Left,
        ] {
            cx.update(|window, cx| three.move_to_next_position(window, cx));
            cx.update(|window, cx| {
                assert_eq!(
                    three.position(window, cx),
                    expected,
                    "cycling must reach {expected:?}"
                );
            });
        }
    }
}
