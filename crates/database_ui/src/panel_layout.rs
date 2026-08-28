//! How tall each of the column's three regions stands, and the handles that
//! change it.
//!
//! A local implementation rather than `workspace::pane_group::pane_axis`, which
//! is what gives the docks their resizable sections: that is `pub(crate)` to
//! `workspace`, and widening it to reach three fixed regions inside one panel
//! would be a larger change to shared code than the thing it buys.

use crate::database_panel::DatabasePanel;
use gpui::{DragMoveEvent, Empty, MouseButton, MouseDownEvent, Pixels, Render, px};
use ui::prelude::*;

/// Where the tree stops and the scratch buffer starts, before anyone drags it.
/// About a dozen rows: enough for a connection and its tables without pushing
/// the grid off the bottom.
pub(crate) const DEFAULT_TREE_HEIGHT: Pixels = px(240.0);

/// Sized to about six lines: a statement, plus room to think.
pub(crate) const DEFAULT_SQL_HEIGHT: Pixels = px(132.0);

/// How wide the table list stands in full screen, before anyone drags it.
/// Enough for a schema-qualified table name without wrapping.
pub(crate) const DEFAULT_TREE_WIDTH: Pixels = px(280.0);

/// Below this a region is a sliver that cannot be read or dragged back, which
/// is a state the user cannot undo with the same gesture that caused it.
const MIN_REGION_HEIGHT: Pixels = px(56.0);

/// How tall a region may be dragged. Generous, and only a backstop: the panel's
/// own height is the real limit, and a region past it is shrunk by the flexbox
/// rather than by this.
const MAX_REGION_HEIGHT: Pixels = px(1200.0);

/// Narrower than this and a table name is unreadable, which is the only thing
/// the list is for.
const MIN_REGION_WIDTH: Pixels = px(160.0);

/// A backstop of the same kind as `MAX_REGION_HEIGHT`: past this the list is
/// eating the data it exists to open.
const MAX_REGION_WIDTH: Pixels = px(900.0);

pub(crate) const LAYOUT_KEY: &str = "database-column-layout";

/// Which boundary is being dragged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Split {
    /// Between the connection tree and the scratch buffer.
    TreeAndSql,
    /// Between the scratch buffer and the results.
    SqlAndResults,
    /// Between the table list and the data and statement beside it. Full screen
    /// only, and the one boundary here that is a vertical line.
    TreeAndBody,
}

impl Split {
    /// Whether the boundary is a vertical line, and therefore whether a drag
    /// along it is measured on the pointer's x rather than its y.
    pub(crate) fn is_vertical(self) -> bool {
        matches!(self, Split::TreeAndBody)
    }
}

/// The payload GPUI carries for the length of a drag. Empty on purpose -- the
/// handle is the thing being moved, and a preview drawn under the pointer would
/// be a second thing to look at.
#[derive(Clone, Copy)]
pub(crate) struct DraggedSplit(pub(crate) Split);

impl Render for DraggedSplit {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl DatabasePanel {
    /// Applies one step of a drag.
    ///
    /// By delta from the last position rather than from the pointer's absolute
    /// place in the window: a region's own bounds are not known here, and
    /// tracking them would mean measuring on every frame to answer a question
    /// only asked while a handle is held.
    pub(crate) fn drag_split(&mut self, split: Split, pointer: Pixels, cx: &mut Context<Self>) {
        let previous = match self.split_drag {
            Some((dragged, last)) if dragged == split => Some(last),
            _ => None,
        };
        self.split_drag = Some((split, pointer));

        // The first move of a drag only sets the reference the rest measure
        // against. Moving on it would jump the region by the distance from
        // wherever the pointer last was.
        let Some(last) = previous else {
            return;
        };

        let (size, minimum, maximum) = match split {
            Split::TreeAndSql => (&mut self.tree_height, MIN_REGION_HEIGHT, MAX_REGION_HEIGHT),
            Split::SqlAndResults => (&mut self.sql_height, MIN_REGION_HEIGHT, MAX_REGION_HEIGHT),
            Split::TreeAndBody => (&mut self.tree_width, MIN_REGION_WIDTH, MAX_REGION_WIDTH),
        };
        *size = (*size + (pointer - last)).clamp(minimum, maximum);

        self.persist_layout(cx);
        cx.notify();
    }

    /// A double click puts a region back where it started, which is the only
    /// way out of a layout someone has dragged into a corner.
    fn reset_split(&mut self, split: Split, cx: &mut Context<Self>) {
        match split {
            Split::TreeAndSql => self.tree_height = DEFAULT_TREE_HEIGHT,
            Split::SqlAndResults => self.sql_height = DEFAULT_SQL_HEIGHT,
            Split::TreeAndBody => self.tree_width = DEFAULT_TREE_WIDTH,
        }
        self.persist_layout(cx);
        cx.notify();
    }

    /// The width goes under its own key rather than as a third entry in the
    /// heights list: that list is read positionally, and a width sitting in it
    /// would be read back as a height by any build that predates this.
    fn persist_layout(&mut self, cx: &mut Context<Self>) {
        let heights = vec![f32::from(self.tree_height), f32::from(self.sql_height)];
        let tree_width = f32::from(self.tree_width);
        self.workspace
            .update(cx, |workspace, cx| {
                workspace.persist_workspace_state(LAYOUT_KEY, "heights", &heights, cx);
                workspace.persist_workspace_state(LAYOUT_KEY, "tree-width", &tree_width, cx);
            })
            .ok();
    }

    /// The grab strip between two regions.
    pub(crate) fn render_split_handle(
        &self,
        split: Split,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = match split {
            Split::TreeAndSql => "database-split-tree",
            Split::SqlAndResults => "database-split-sql",
            Split::TreeAndBody => "database-split-body",
        };
        let vertical = split.is_vertical();

        let panel = cx.weak_entity();

        div()
            .id(id)
            .flex_none()
            .map(|handle| {
                if vertical {
                    handle.w(px(5.0)).h_full().cursor_col_resize()
                } else {
                    handle.h(px(5.0)).w_full().cursor_row_resize()
                }
            })
            .hover(|style| style.bg(cx.theme().colors().border_focused))
            .on_drag(DraggedSplit(split), move |dragged, _offset, _window, cx| {
                // Cleared here, at the *start* of a drag, rather than when the
                // last one ended: a drag released outside this handle never
                // reaches its mouse-up, and a reference left over from then
                // would make this grab jump by the distance between the two.
                panel.update(cx, |panel, _cx| panel.split_drag = None).ok();
                cx.stop_propagation();
                cx.new(|_| *dragged)
            })
            // Claimed so the drag does not also reach the tree row underneath,
            // which would open a connection every time a handle is grabbed.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _: &MouseDownEvent, _window, cx| cx.stop_propagation()),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseUpEvent, _window, cx| {
                    if event.click_count == 2 {
                        this.reset_split(split, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .occlude()
    }

    /// Routes a drag-move to the handle it belongs to.
    pub(crate) fn on_split_dragged(
        &mut self,
        event: &DragMoveEvent<DraggedSplit>,
        cx: &mut Context<Self>,
    ) {
        let DraggedSplit(split) = *event.drag(cx);
        let pointer = if split.is_vertical() {
            event.event.position.x
        } else {
            event.event.position.y
        };
        self.drag_split(split, pointer, cx);
    }

    /// The heights this project was last left with.
    ///
    /// Read once at load, like the pins: nothing else writes them.
    pub(crate) fn saved_heights(heights: Option<Vec<f32>>) -> (Pixels, Pixels) {
        let read = |index: usize, fallback: Pixels| {
            heights
                .as_ref()
                .and_then(|heights| heights.get(index).copied())
                .filter(|height| height.is_finite() && *height > 0.0)
                .map_or(fallback, px)
                .clamp(MIN_REGION_HEIGHT, MAX_REGION_HEIGHT)
        };
        (read(0, DEFAULT_TREE_HEIGHT), read(1, DEFAULT_SQL_HEIGHT))
    }

    /// How wide this project last left the table list, held to the same limits
    /// a drag is: nothing validates what is in the key-value store.
    pub(crate) fn saved_tree_width(width: Option<f32>) -> Pixels {
        width
            .filter(|width| width.is_finite() && *width > 0.0)
            .map_or(DEFAULT_TREE_WIDTH, px)
            .clamp(MIN_REGION_WIDTH, MAX_REGION_WIDTH)
    }
}

impl DatabasePanel {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_or_partial_saved_heights_fall_back_rather_than_collapse() {
        assert_eq!(
            DatabasePanel::saved_heights(None),
            (DEFAULT_TREE_HEIGHT, DEFAULT_SQL_HEIGHT)
        );
        assert_eq!(
            DatabasePanel::saved_heights(Some(vec![300.0])).1,
            DEFAULT_SQL_HEIGHT,
            "a truncated record must not take the second region down with it"
        );
    }

    /// The key-value store is not a schema and nothing validates what lands in
    /// it. A zero or a NaN here would draw a region nobody can grab to undo it.
    #[test]
    fn a_nonsensical_saved_height_is_ignored() {
        for bad in [0.0, -50.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                DatabasePanel::saved_heights(Some(vec![bad, bad])),
                (DEFAULT_TREE_HEIGHT, DEFAULT_SQL_HEIGHT),
                "{bad} is not a height"
            );
        }
    }

    #[test]
    fn a_nonsensical_or_absent_saved_tree_width_is_ignored() {
        assert_eq!(DatabasePanel::saved_tree_width(None), DEFAULT_TREE_WIDTH);
        for bad in [0.0, -50.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                DatabasePanel::saved_tree_width(Some(bad)),
                DEFAULT_TREE_WIDTH,
                "{bad} is not a width"
            );
        }
        assert_eq!(
            DatabasePanel::saved_tree_width(Some(9.0)),
            MIN_REGION_WIDTH,
            "a width too narrow to read a table name is brought back"
        );
    }

    #[test]
    fn a_saved_height_beyond_the_limits_is_brought_back_inside_them() {
        let (tree, sql) = DatabasePanel::saved_heights(Some(vec![4.0, 99_000.0]));
        assert_eq!(tree, MIN_REGION_HEIGHT);
        assert_eq!(sql, MAX_REGION_HEIGHT);
    }
}
