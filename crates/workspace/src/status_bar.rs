use crate::status_bar_toggles::{self, StatusBarItemSpec};
use crate::{ItemHandle, MultiWorkspace, Pane};
use gpui::{
    Anchor, AnyView, App, Context, Decorations, Entity, IntoElement, ParentElement, Render, Styled,
    Subscription, WeakEntity, Window,
};
use settings::SettingsStore;
use std::any::TypeId;
use theme::CLIENT_SIDE_DECORATION_ROUNDING;
use ui::prelude::*;
use ui::{ContextMenu, right_click_menu};
use util::ResultExt;

pub trait StatusItemView: Render {
    /// Event callback that is triggered when the active pane item changes.
    fn set_active_pane_item(
        &mut self,
        active_pane_item: Option<&dyn crate::ItemHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    );
}

trait StatusItemViewHandle: Send {
    fn to_any(&self) -> AnyView;
    fn set_active_pane_item(
        &self,
        active_pane_item: Option<&dyn ItemHandle>,
        window: &mut Window,
        cx: &mut App,
    );
    fn item_type(&self) -> TypeId;
}

#[derive(Default)]
struct SidebarStatus {
    open: bool,
}

impl SidebarStatus {
    fn query(multi_workspace: &Option<WeakEntity<MultiWorkspace>>, cx: &App) -> Self {
        multi_workspace
            .as_ref()
            .and_then(|mw| mw.upgrade())
            .map(|mw| {
                let mw = mw.read(cx);
                let enabled = mw.multi_workspace_enabled(cx);
                Self {
                    open: mw.sidebar_open() && enabled,
                }
            })
            .unwrap_or_default()
    }
}

/// Which side of the bar a slot lives on. `Right` renders reversed (see
/// `render_right_tools`), so keeping storage sorted by rank on both sides is
/// what gets correct visual order on the right without reasoning about the
/// reversal anywhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusBarSide {
    Left,
    Right,
}

/// An item together with the rank it was registered at. Ranks are assigned
/// once, in registration order, and never recomputed -- they are what lets
/// an item that is removed and later reinserted at its own rank return to
/// its original position instead of the end of its group.
struct Slot {
    rank: usize,
    item: Box<dyn StatusItemViewHandle>,
}

/// Where a slot of `rank` belongs in a vector already sorted by rank.
/// `partition_point` is O(log n): the vector is sorted by construction,
/// since ranks only ever increase and every insertion preserves that order.
/// A duplicate `rank` sorts before the existing entry that shares it, which
/// keeps the result deterministic rather than depending on comparison order.
fn insertion_index(present_ranks: &[usize], rank: usize) -> usize {
    present_ranks.partition_point(|&present| present < rank)
}

pub struct StatusBar {
    left_items: Vec<Slot>,
    right_items: Vec<Slot>,
    next_rank: usize,
    active_pane: Entity<Pane>,
    multi_workspace: Option<WeakEntity<MultiWorkspace>>,
    /// Registered by `register_toggleable_item`. `pub(crate)` rather than
    /// private so the registration and reconciliation logic
    /// (`status_bar_toggles.rs`) can live outside this file -- the field
    /// itself has to be declared here because Rust requires every field of
    /// a type to be declared where the type itself is declared.
    pub(crate) specs: Vec<StatusBarItemSpec>,
    /// Reconciles `specs` against the settings in force on every settings
    /// change (see `status_bar_toggles::StatusBar::reconcile`). Dropped
    /// with `StatusBar`; nothing else holds this subscription.
    _settings: Subscription,
    _observe_active_pane: Subscription,
}

impl Render for StatusBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = SidebarStatus::query(&self.multi_workspace, cx);

        h_flex()
            .w_full()
            // No `justify_between`: the filler child between the two groups
            // (`render_toggle_menu_surface`) is `flex_1`, so it already
            // absorbs all the free space `justify_between` would have
            // distributed -- keeping both would just make the second one a
            // no-op.
            .gap(DynamicSpacing::Base08.rems(cx))
            .p(DynamicSpacing::Base04.rems(cx))
            .bg(cx.theme().colors().status_bar_background)
            .map(|el| match window.window_decorations() {
                Decorations::Server => el,
                Decorations::Client { tiling, .. } => el
                    .when(!(tiling.bottom || tiling.right), |el| {
                        el.rounded_br(CLIENT_SIDE_DECORATION_ROUNDING)
                    })
                    // Only the left corner can be covered: the sidebar stands
                    // against that edge and nothing puts it on the other one.
                    .when(!(tiling.bottom || tiling.left) && !sidebar.open, |el| {
                        el.rounded_bl(CLIENT_SIDE_DECORATION_ROUNDING)
                    })
                    // This border is to avoid a transparent gap in the rounded corners
                    .mb(px(-1.))
                    .border_b(px(1.0))
                    .border_color(cx.theme().colors().status_bar_background),
            })
            .child(self.render_left_tools())
            .child(self.render_toggle_menu_surface())
            .child(self.render_right_tools())
    }
}

impl StatusBar {
    fn render_left_tools(&self) -> impl IntoElement {
        h_flex()
            .gap_1()
            .min_w_0()
            .overflow_x_hidden()
            .children(self.left_items.iter().map(|slot| slot.item.to_any()))
    }

    fn render_right_tools(&self) -> impl IntoElement {
        h_flex()
            .flex_shrink_0()
            .gap_1()
            .overflow_x_hidden()
            .children(self.right_items.iter().rev().map(|slot| slot.item.to_any()))
    }

    /// The empty gap between the two groups, made right-clickable to list
    /// every registered item. A sibling of the two groups and never a
    /// wrapper around them: GPUI dispatches the bubble phase in *reverse*
    /// registration order (`gpui/src/window.rs:4396`), and
    /// `RightClickMenu::paint` registers its listener *after* painting its
    /// child, so a menu wrapping the whole bar would register last, fire
    /// first, and swallow every right-click meant for an item's own menu
    /// (e.g. `agent-usage-toggles`).
    ///
    /// `flex_1` is what actually absorbs the space `justify_between` used
    /// to (see the comment in `render`). The `min_w` keeps a right-clickable
    /// sliver alive even after the left group's `min_w_0()` +
    /// `overflow_x_hidden()` has shrunk it as far as it will go on a
    /// crowded, narrow window -- without it, the gap could reach zero width
    /// and take the feature with it exactly when a user most wants to prune
    /// the bar.
    fn render_toggle_menu_surface(&self) -> impl IntoElement {
        right_click_menu::<ContextMenu>("status-bar-empty-area")
            // Stated rather than left to the default corner, which opens
            // downward off the bottom of the window and relies on
            // `snap_to_window_with_margin` to shove it back -- a fix that
            // stops landing correctly once the menu grows tall, and this
            // one has fifteen rows and two headers.
            .anchor(Anchor::BottomLeft)
            .attach(Anchor::TopLeft)
            .menu(status_bar_toggles::build_item_menu(&self.specs))
            .trigger(|_is_open, _window, _cx| div().flex_1().min_w(px(24.)).h_full())
    }
}

impl StatusBar {
    pub fn new(
        active_pane: &Entity<Pane>,
        multi_workspace: Option<WeakEntity<MultiWorkspace>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            left_items: Default::default(),
            right_items: Default::default(),
            next_rank: 0,
            active_pane: active_pane.clone(),
            multi_workspace,
            specs: Vec::new(),
            _observe_active_pane: cx.observe_in(active_pane, window, |this, _, window, cx| {
                this.update_active_pane_item(window, cx)
            }),
            _settings: cx.observe_global_in::<SettingsStore>(window, |_this, window, cx| {
                // Rebuilding an item calls back into `this` from inside this
                // observer; if the settings change was itself triggered from
                // inside a `Workspace` update, doing that synchronously would
                // double-borrow and panic. Deferring moves the reconcile pass
                // out of the active update -- one frame, imperceptible for a
                // settings change.
                cx.defer_in(window, |this, window, cx| this.reconcile(window, cx));
            }),
        };
        this.update_active_pane_item(window, cx);
        this
    }

    pub fn set_multi_workspace(
        &mut self,
        multi_workspace: WeakEntity<MultiWorkspace>,
        cx: &mut Context<Self>,
    ) {
        self.multi_workspace = Some(multi_workspace);
        cx.notify();
    }

    /// Allocates the next rank, in registration order. The counter is owned
    /// here (not derived from any table) so it never collides with ranks
    /// already assigned to the dock buttons `Workspace::new` adds before
    /// `initialize_workspace` runs.
    fn allocate_rank(&mut self) -> usize {
        let rank = self.next_rank;
        self.next_rank += 1;
        rank
    }

    fn items_for_side(&self, side: StatusBarSide) -> &Vec<Slot> {
        match side {
            StatusBarSide::Left => &self.left_items,
            StatusBarSide::Right => &self.right_items,
        }
    }

    fn items_for_side_mut(&mut self, side: StatusBarSide) -> &mut Vec<Slot> {
        match side {
            StatusBarSide::Left => &mut self.left_items,
            StatusBarSide::Right => &mut self.right_items,
        }
    }

    pub fn add_left_item<T>(
        &mut self,
        item: Entity<T>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize
    where
        T: 'static + StatusItemView,
    {
        let active_pane_item = self.active_pane.read(cx).active_item();
        item.set_active_pane_item(active_pane_item.as_deref(), window, cx);

        let rank = self.allocate_rank();
        self.left_items.push(Slot {
            rank,
            item: Box::new(item),
        });
        cx.notify();
        rank
    }

    pub fn item_of_type<T: StatusItemView>(&self) -> Option<Entity<T>> {
        self.left_items
            .iter()
            .chain(self.right_items.iter())
            .find_map(|slot| slot.item.to_any().downcast().log_err())
    }

    pub fn position_of_item<T>(&self) -> Option<usize>
    where
        T: StatusItemView,
    {
        for (index, slot) in self.left_items.iter().enumerate() {
            if slot.item.item_type() == TypeId::of::<T>() {
                return Some(index);
            }
        }
        for (index, slot) in self.right_items.iter().enumerate() {
            if slot.item.item_type() == TypeId::of::<T>() {
                return Some(index + self.left_items.len());
            }
        }
        None
    }

    /// Inserts at a flat left-then-right index, unrelated to rank order.
    /// This always allocates the newest (largest) rank but can insert
    /// anywhere in the vector, so it can leave that side's slots out of
    /// rank order. Harmless today since nothing calls this alongside
    /// `insert_item_at_rank`/`remove_item_by_rank`, but new callers should
    /// prefer the rank-based API.
    pub fn insert_item_after<T>(
        &mut self,
        position: usize,
        item: Entity<T>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) where
        T: 'static + StatusItemView,
    {
        let active_pane_item = self.active_pane.read(cx).active_item();
        item.set_active_pane_item(active_pane_item.as_deref(), window, cx);

        let rank = self.allocate_rank();
        let slot = Slot {
            rank,
            item: Box::new(item),
        };

        if position < self.left_items.len() {
            self.left_items.insert(position + 1, slot)
        } else {
            self.right_items
                .insert(position + 1 - self.left_items.len(), slot)
        }
        cx.notify()
    }

    pub fn remove_item_at(&mut self, position: usize, cx: &mut Context<Self>) {
        if position < self.left_items.len() {
            self.left_items.remove(position);
        } else {
            self.right_items.remove(position - self.left_items.len());
        }
        cx.notify();
    }

    pub fn add_right_item<T>(
        &mut self,
        item: Entity<T>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize
    where
        T: 'static + StatusItemView,
    {
        let active_pane_item = self.active_pane.read(cx).active_item();
        item.set_active_pane_item(active_pane_item.as_deref(), window, cx);

        let rank = self.allocate_rank();
        self.right_items.push(Slot {
            rank,
            item: Box::new(item),
        });
        cx.notify();
        rank
    }

    /// Inserts `item` on `side` so that side's slots stay sorted by rank --
    /// this is what lets an item removed by `remove_item_by_rank` come back
    /// at the same position instead of the end of its group.
    pub fn insert_item_at_rank<T>(
        &mut self,
        side: StatusBarSide,
        rank: usize,
        item: Entity<T>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) where
        T: 'static + StatusItemView,
    {
        let active_pane_item = self.active_pane.read(cx).active_item();
        item.set_active_pane_item(active_pane_item.as_deref(), window, cx);

        let items = self.items_for_side_mut(side);
        let present_ranks: Vec<usize> = items.iter().map(|slot| slot.rank).collect();
        let index = insertion_index(&present_ranks, rank);
        items.insert(
            index,
            Slot {
                rank,
                item: Box::new(item),
            },
        );
        cx.notify();
    }

    /// Removes the slot carrying `rank` on `side`, if present. Returns
    /// whether a slot was removed.
    pub fn remove_item_by_rank(
        &mut self,
        side: StatusBarSide,
        rank: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        let items = self.items_for_side_mut(side);
        match items.binary_search_by_key(&rank, |slot| slot.rank) {
            Ok(index) => {
                items.remove(index);
                cx.notify();
                true
            }
            Err(_) => false,
        }
    }

    pub fn contains_rank(&self, side: StatusBarSide, rank: usize) -> bool {
        self.items_for_side(side)
            .binary_search_by_key(&rank, |slot| slot.rank)
            .is_ok()
    }

    pub fn set_active_pane(
        &mut self,
        active_pane: &Entity<Pane>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_pane = active_pane.clone();
        self._observe_active_pane = cx.observe_in(active_pane, window, |this, _, window, cx| {
            this.update_active_pane_item(window, cx)
        });
        self.update_active_pane_item(window, cx);
    }

    fn update_active_pane_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active_pane_item = self.active_pane.read(cx).active_item();
        for slot in self.left_items.iter().chain(&self.right_items) {
            slot.item
                .set_active_pane_item(active_pane_item.as_deref(), window, cx);
        }
    }
}

impl<T: StatusItemView> StatusItemViewHandle for Entity<T> {
    fn to_any(&self) -> AnyView {
        self.clone().into()
    }

    fn set_active_pane_item(
        &self,
        active_pane_item: Option<&dyn ItemHandle>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.update(cx, |this, cx| {
            this.set_active_pane_item(active_pane_item, window, cx)
        });
    }

    fn item_type(&self) -> TypeId {
        TypeId::of::<T>()
    }
}

impl From<&dyn StatusItemViewHandle> for AnyView {
    fn from(val: &dyn StatusItemViewHandle) -> Self {
        val.to_any()
    }
}

#[cfg(test)]
#[path = "status_bar_tests.rs"]
mod tests;
