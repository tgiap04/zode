//! Dragging a stacked section's header to reorder it within its own dock.
//!
//! Follows the project rail's drag (`sidebar/src/rail_item.rs`,
//! `sidebar/src/rail.rs`) rather than inventing a second style. Three things
//! differ from it, and each one bites anyone who copies the rail blindly:
//!
//! - **The axis.** `render_stack` stacks across the dock's own axis, so a side
//!   dock divides vertically and the bottom dock horizontally. Both the half
//!   test and the indicator derive from `position.axis()`.
//! - **The indicator cannot be a sibling.** The rail slips its bar in as a flex
//!   child between rows; a stack cannot, because `PaneAxisElement::prepaint`
//!   reads `flexes[ix]` once per child and asserts the two lengths agree. An
//!   extra child indexes out of bounds in release. The bar is drawn absolutely
//!   inside the section instead.
//! - **Drops dispatch by type, not by element.** GPUI routes a drop to every
//!   handler registered for that payload type, so a `DraggedPanel` from one dock
//!   reaches every other dock's headers. Same-dock reordering is a fixed
//!   decision, so every handler checks `dock_id` first.

use super::{Dock, PanelHandle};
use gpui::{
    AnyElement, Axis, Context, Div, DragMoveEvent, EntityId, IntoElement, ParentElement, Render,
    SharedString, Stateful, Styled, Window, px,
};
use std::sync::Arc;
use ui::{IconButton, IconName, Tooltip, prelude::*};

/// A section header in flight.
///
/// `dock_id` is what keeps a drag inside the dock it started in: GPUI hands this
/// payload to every dock's handlers, so without it a section dropped on another
/// dock's header would reorder a stack the user never touched.
pub struct DraggedPanel {
    pub(super) dock_id: EntityId,
    pub(super) panel_id: EntityId,
    pub(super) name: SharedString,
    pub(super) icon: Option<IconName>,
}

impl Render for DraggedPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        h_flex()
            .gap_1p5()
            .px_2()
            .py_1()
            .rounded_sm()
            .border_1()
            .border_color(colors.border)
            .bg(colors.elevated_surface_background)
            .children(
                self.icon
                    .map(|icon| Icon::new(icon).size(IconSize::Small).color(Color::Muted)),
            )
            .child(Label::new(self.name.clone()).size(LabelSize::Small))
    }
}

impl Dock {
    /// Which way this dock's sections are stacked.
    ///
    /// The dock's own axis is the one it is measured along; the sections divide
    /// the other one -- the same inversion `render_stack` makes. Derived rather
    /// than special-cased on `Bottom`, so the two cannot drift apart.
    fn stack_axis(&self) -> Axis {
        match self.position.axis() {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        }
    }

    pub(crate) fn set_stack_drop_gap(&mut self, gap: Option<usize>, cx: &mut Context<Self>) {
        if self.stack_drop_gap != gap {
            self.stack_drop_gap = gap;
            cx.notify();
        }
    }

    /// One section's header: what names it, what closes it, and what drags it.
    ///
    /// Split out of `render_stacked_panel` so the drag lives beside the rest of
    /// the drag rather than in the middle of the dock's layout.
    pub(super) fn render_stack_section_header(
        &self,
        ix: usize,
        panel: &Arc<dyn PanelHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let panel_id = panel.panel_id();
        let name = SharedString::from(panel.persistent_name());
        let icon = panel.icon(window, cx);
        let colors = cx.theme().colors().clone();
        let dock_id = cx.entity_id();

        h_flex()
            .id(("dock-stack-header", panel_id))
            .debug_selector(move || format!("dock-stack-header:{ix}"))
            .relative()
            .flex_none()
            .w_full()
            .px_2()
            .py_1()
            .justify_between()
            .border_b_1()
            .border_color(colors.border)
            .cursor_pointer()
            .on_drag(
                DraggedPanel {
                    dock_id,
                    panel_id,
                    name: name.clone(),
                    icon,
                },
                |dragged, _, _, cx| {
                    cx.new(|_| DraggedPanel {
                        dock_id: dragged.dock_id,
                        panel_id: dragged.panel_id,
                        name: dragged.name.clone(),
                        icon: dragged.icon,
                    })
                },
            )
            .child(
                h_flex()
                    .gap_1p5()
                    .children(
                        icon.map(|icon| Icon::new(icon).size(IconSize::Small).color(Color::Muted)),
                    )
                    .child(Label::new(name).size(LabelSize::Small)),
            )
            .child(
                IconButton::new(("hide-stacked-panel", panel_id), IconName::Close)
                    .icon_size(IconSize::Small)
                    .tooltip(move |_window, cx| Tooltip::simple("Hide Panel", cx))
                    .on_click(cx.listener(move |dock, _, window, cx| {
                        dock.hide_panel_by_id(panel_id, window, cx);
                    })),
            )
            .into_any_element()
    }

    /// Makes a whole section a drop target, not just its header.
    ///
    /// The header is a ~24px strip on a section hundreds of pixels tall, so
    /// "drop it between those two" lands on a panel's content far more often
    /// than on a header. Accepting the drop only where the drag STARTS is the
    /// mistake that made the gesture do nothing at all for the user who
    /// reported it. Grabbing still requires the header -- a drag armed from the
    /// body would fight the panel's own content -- but releasing works anywhere
    /// over the section, with its own midpoint deciding above or below.
    pub(super) fn with_stack_drop_target(
        &self,
        ix: usize,
        showing_count: usize,
        section: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let dock_id = cx.entity_id();
        let axis = self.stack_axis();
        let accent = cx.theme().colors().text_accent;

        // The bar only draws while a drag is actually in flight: a drop that
        // lands nowhere never reaches a handler that could clear the gap, and a
        // bar left behind on a settled stack reads as a rendering fault.
        let dragging = cx.has_active_drag();
        let leading = dragging && self.stack_drop_gap == Some(ix);
        // The gap past the last section is the one no section index can name.
        let trailing =
            dragging && ix + 1 == showing_count && self.stack_drop_gap == Some(showing_count);

        section
            // Claims a gap only while the pointer is inside this section's own
            // bounds, and sections do not overlap -- so the order handlers fire
            // in stops mattering. `on_drag_move` is not hitbox-filtered, so the
            // bounds check is ours to make.
            .on_drag_move(cx.listener(
                move |dock, event: &DragMoveEvent<DraggedPanel>, _window, cx| {
                    if event.drag(cx).dock_id != dock_id {
                        return;
                    }
                    if !event.bounds.contains(&event.event.position) {
                        return;
                    }
                    let past_middle = match axis {
                        Axis::Vertical => event.event.position.y > event.bounds.center().y,
                        Axis::Horizontal => event.event.position.x > event.bounds.center().x,
                    };
                    dock.set_stack_drop_gap(Some(if past_middle { ix + 1 } else { ix }), cx);
                },
            ))
            .on_drop(cx.listener(
                move |dock, dragged: &DraggedPanel, _window, cx| {
                    let gap = dock.stack_drop_gap.take();
                    if dragged.dock_id != dock_id {
                        return;
                    }
                    let Some(gap) = gap else {
                        return;
                    };
                    dock.move_stacked_panel(dragged.panel_id, gap, cx);
                },
            ))
            .when(leading, |this| {
                this.child(drop_indicator(axis, false, accent))
            })
            .when(trailing, |this| {
                this.child(drop_indicator(axis, true, accent))
            })
    }
}

/// The bar marking where a released section would land.
///
/// Absolutely positioned so it never becomes a `pane_axis` child -- see this
/// module's header for why an extra child is an out-of-bounds index rather than
/// merely a layout surprise.
fn drop_indicator(axis: Axis, trailing: bool, colour: gpui::Hsla) -> AnyElement {
    let bar = div().absolute().bg(colour);
    match (axis, trailing) {
        (Axis::Vertical, false) => bar.top_0().left_0().right_0().h(px(2.)),
        (Axis::Vertical, true) => bar.bottom_0().left_0().right_0().h(px(2.)),
        (Axis::Horizontal, false) => bar.left_0().top_0().bottom_0().w(px(2.)),
        (Axis::Horizontal, true) => bar.right_0().top_0().bottom_0().w(px(2.)),
    }
    .into_any_element()
}
