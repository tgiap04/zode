//! Which rows of the result are selected, and which column was last clicked.
//!
//! Held by the page rather than the panel, for the reason `selected_row` is:
//! paging or running another statement must start with nothing selected, not
//! with whatever now sits at the old indices. Bounded by the page, so it needs
//! no cap of its own.

use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GridSelection {
    rows: BTreeSet<usize>,
    /// Where a shift-click extends from: the row last clicked on its own or
    /// with the toggle modifier.
    anchor: Option<usize>,
    /// The row the keyboard moves from.
    cursor: Option<usize>,
    column: Option<usize>,
}

impl GridSelection {
    pub(crate) fn contains(&self, row: usize) -> bool {
        self.rows.contains(&row)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The selected rows, ascending.
    pub(crate) fn rows(&self) -> Vec<usize> {
        self.rows.iter().copied().collect()
    }

    pub(crate) fn set_column(&mut self, column: usize) {
        self.column = Some(column);
    }

    /// The one cell that is selected, when exactly one row is and a column was
    /// clicked: what `copy` copies as a value rather than as a row.
    pub(crate) fn single_cell(&self) -> Option<(usize, usize)> {
        let mut rows = self.rows.iter();
        match (rows.next(), rows.next(), self.column) {
            (Some(&row), None, Some(column)) => Some((row, column)),
            _ => None,
        }
    }

    /// A plain click: this row and no other.
    pub(crate) fn select_only(&mut self, row: usize) {
        self.rows.clear();
        self.rows.insert(row);
        self.anchor = Some(row);
        self.cursor = Some(row);
    }

    /// Shift-click: every row from the anchor to `row`. Without an anchor
    /// there is nothing to extend, so it is a plain click.
    pub(crate) fn extend_to(&mut self, row: usize) {
        let Some(anchor) = self.anchor else {
            self.select_only(row);
            return;
        };
        self.rows = (anchor.min(row)..=anchor.max(row)).collect();
        self.cursor = Some(row);
    }

    /// Command-click: this row in if it was out, out if it was in.
    pub(crate) fn toggle(&mut self, row: usize) {
        if !self.rows.remove(&row) {
            self.rows.insert(row);
        }
        self.anchor = Some(row);
        self.cursor = Some(row);
    }

    pub(crate) fn select_all(&mut self, row_count: usize) {
        self.rows = (0..row_count).collect();
        self.anchor = (row_count > 0).then_some(0);
        self.cursor = row_count.checked_sub(1);
    }

    /// Moves the cursor `delta` rows, stopping at the first and last, and
    /// selects only the row it lands on. Answers that row, `None` when the page
    /// has no rows.
    ///
    /// Starting from nothing, down lands on the first row and up on the last,
    /// which is where each would go if there were a row above or below the
    /// page.
    pub(crate) fn move_cursor(&mut self, delta: isize, row_count: usize) -> Option<usize> {
        let last = row_count.checked_sub(1)?;
        let target = match self.cursor {
            Some(cursor) => cursor.saturating_add_signed(delta).min(last),
            None if delta >= 0 => 0,
            None => last,
        };
        self.select_only(target);
        Some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_click_selects_that_row_alone() {
        let mut selection = GridSelection::default();
        selection.select_only(3);
        selection.select_only(5);
        assert_eq!(selection.rows(), [5]);
        assert!(selection.contains(5) && !selection.contains(3));
    }

    #[test]
    fn shift_extends_from_the_anchor_in_either_direction() {
        let mut selection = GridSelection::default();
        selection.select_only(4);
        selection.extend_to(7);
        assert_eq!(selection.rows(), [4, 5, 6, 7]);
        selection.extend_to(2);
        assert_eq!(selection.rows(), [2, 3, 4], "the anchor stays at 4");
        selection.extend_to(4);
        assert_eq!(selection.rows(), [4]);
    }

    #[test]
    fn shift_without_an_anchor_is_a_plain_click() {
        let mut selection = GridSelection::default();
        selection.extend_to(6);
        assert_eq!(selection.rows(), [6]);
        selection.extend_to(8);
        assert_eq!(selection.rows(), [6, 7, 8]);
    }

    #[test]
    fn the_toggle_adds_and_removes_one_row_and_moves_the_anchor() {
        let mut selection = GridSelection::default();
        selection.select_only(1);
        selection.toggle(4);
        selection.toggle(6);
        assert_eq!(selection.rows(), [1, 4, 6]);
        selection.toggle(4);
        assert_eq!(selection.rows(), [1, 6]);

        selection.extend_to(8);
        assert_eq!(
            selection.rows(),
            [4, 5, 6, 7, 8],
            "the anchor is the last row toggled"
        );
    }

    #[test]
    fn select_all_takes_every_row_and_nothing_on_an_empty_page() {
        let mut selection = GridSelection::default();
        selection.select_all(4);
        assert_eq!(selection.rows(), [0, 1, 2, 3]);
        selection.select_all(0);
        assert!(selection.is_empty());
    }

    #[test]
    fn the_keyboard_moves_one_row_and_stops_at_the_ends() {
        let mut selection = GridSelection::default();
        assert_eq!(selection.move_cursor(1, 3), Some(0), "down from nothing");
        assert_eq!(selection.move_cursor(1, 3), Some(1));
        assert_eq!(selection.move_cursor(1, 3), Some(2));
        assert_eq!(selection.move_cursor(1, 3), Some(2), "stops at the last");
        assert_eq!(selection.rows(), [2]);
        assert_eq!(selection.move_cursor(-5, 3), Some(0), "stops at the first");

        let mut selection = GridSelection::default();
        assert_eq!(selection.move_cursor(-1, 3), Some(2), "up from nothing");
        assert_eq!(selection.move_cursor(1, 0), None);
    }

    #[test]
    fn a_cell_is_single_only_with_one_row_and_a_clicked_column() {
        let mut selection = GridSelection::default();
        assert_eq!(selection.single_cell(), None);
        selection.select_only(2);
        assert_eq!(selection.single_cell(), None, "no column yet");
        selection.set_column(1);
        assert_eq!(selection.single_cell(), Some((2, 1)));
        selection.toggle(3);
        assert_eq!(selection.single_cell(), None, "two rows are not a cell");
        selection.select_only(5);
        assert_eq!(
            selection.single_cell(),
            Some((5, 1)),
            "the column outlives a change of rows"
        );
    }
}
