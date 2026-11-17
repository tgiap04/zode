//! Which rows are selected, and which one the keyboard is on.
//!
//! Keyed by the row's own key (`container::resource::row_key`) rather than by
//! its position, unlike the database grid's: this list is re-read every few
//! seconds and by every engine event, and a row that is stopped, started or
//! removed moves every position below it. A position-keyed selection would
//! silently slide onto a neighbour -- which for a bulk remove means the wrong
//! container.
//!
//! The key and not the id, because the id is not unique: `docker images` prints
//! one row per tag and every tag of an image shares its id.
//!
//! The cursor is separate from the selection: the arrows move the cursor, space
//! puts the row under it in or out, so a selection of several rows can be built
//! from the keyboard alone.

use std::collections::HashSet;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RowSelection {
    ids: HashSet<String>,
    /// Where a shift-click extends from.
    anchor: Option<String>,
    cursor: Option<String>,
}

impl RowSelection {
    pub(crate) fn contains(&self, id: &str) -> bool {
        self.ids.contains(id)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub(crate) fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    /// A plain click: this row and no other.
    pub(crate) fn select_only(&mut self, id: &str) {
        self.ids.clear();
        self.ids.insert(id.to_string());
        self.anchor = Some(id.to_string());
        self.cursor = Some(id.to_string());
    }

    /// Command-click, or space: this row in if it was out, out if it was in.
    pub(crate) fn toggle(&mut self, id: &str) {
        if !self.ids.remove(id) {
            self.ids.insert(id.to_string());
        }
        self.anchor = Some(id.to_string());
        self.cursor = Some(id.to_string());
    }

    /// Shift-click: every row from the anchor to `id`, in the order shown. With
    /// no anchor among the rows shown there is nothing to extend, so it is a
    /// plain click.
    pub(crate) fn extend_to(&mut self, id: &str, order: &[&str]) {
        let position = |wanted: &str| order.iter().position(|candidate| *candidate == wanted);
        let (Some(to), Some(from)) = (position(id), self.anchor.as_deref().and_then(position))
        else {
            self.select_only(id);
            return;
        };
        self.ids = order[from.min(to)..=from.max(to)]
            .iter()
            .map(|id| id.to_string())
            .collect();
        self.cursor = Some(id.to_string());
    }

    pub(crate) fn select_all(&mut self, order: &[&str]) {
        self.ids = order.iter().map(|id| id.to_string()).collect();
        self.anchor = order.first().map(|id| id.to_string());
        self.cursor = order.last().map(|id| id.to_string());
    }

    /// Moves the cursor `delta` rows, stopping at the first and last, and
    /// answers where it landed. Starting from nothing, down lands on the first
    /// row and up on the last. The selection itself is left alone.
    pub(crate) fn move_cursor(&mut self, delta: isize, order: &[&str]) -> Option<usize> {
        let last = order.len().checked_sub(1)?;
        let current = self
            .cursor
            .as_deref()
            .and_then(|cursor| order.iter().position(|id| *id == cursor));
        let target = match current {
            Some(current) => current.saturating_add_signed(delta).min(last),
            None if delta >= 0 => 0,
            None => last,
        };
        let id = order[target].to_string();
        self.anchor = Some(id.clone());
        self.cursor = Some(id);
        Some(target)
    }

    /// Forgets every row that is no longer in `order`, so a list that was read
    /// again cannot leave a selection pointing at something that is gone.
    pub(crate) fn keep_only(&mut self, order: &[&str]) {
        let present: HashSet<&str> = order.iter().copied().collect();
        self.ids.retain(|id| present.contains(id.as_str()));
        if self
            .anchor
            .as_deref()
            .is_some_and(|anchor| !present.contains(anchor))
        {
            self.anchor = None;
        }
        if self
            .cursor
            .as_deref()
            .is_some_and(|cursor| !present.contains(cursor))
        {
            self.cursor = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDER: [&str; 6] = ["a", "b", "c", "d", "e", "f"];

    fn selected(selection: &RowSelection) -> Vec<&str> {
        ORDER
            .iter()
            .copied()
            .filter(|id| selection.contains(id))
            .collect()
    }

    #[test]
    fn a_plain_click_selects_that_row_alone() {
        let mut selection = RowSelection::default();
        selection.select_only("b");
        selection.select_only("d");
        assert_eq!(selected(&selection), ["d"]);
        assert_eq!(selection.cursor(), Some("d"));
    }

    #[test]
    fn shift_extends_from_the_anchor_in_either_direction() {
        let mut selection = RowSelection::default();
        selection.select_only("c");
        selection.extend_to("e", &ORDER);
        assert_eq!(selected(&selection), ["c", "d", "e"]);
        selection.extend_to("a", &ORDER);
        assert_eq!(
            selected(&selection),
            ["a", "b", "c"],
            "the anchor stays at c"
        );
    }

    #[test]
    fn shift_without_an_anchor_among_the_rows_is_a_plain_click() {
        let mut selection = RowSelection::default();
        selection.extend_to("d", &ORDER);
        assert_eq!(selected(&selection), ["d"]);

        // An anchor that has been filtered out of view is no anchor.
        selection.select_only("a");
        selection.extend_to("e", &["c", "d", "e"]);
        assert_eq!(selection.ids.len(), 1);
        assert!(selection.contains("e"));
    }

    #[test]
    fn toggling_adds_and_removes_one_row_and_moves_the_anchor() {
        let mut selection = RowSelection::default();
        selection.select_only("a");
        selection.toggle("c");
        selection.toggle("e");
        assert_eq!(selected(&selection), ["a", "c", "e"]);
        selection.toggle("c");
        assert_eq!(selected(&selection), ["a", "e"]);
        selection.extend_to("f", &ORDER);
        assert_eq!(selected(&selection), ["c", "d", "e", "f"]);
    }

    #[test]
    fn the_cursor_moves_without_touching_the_selection() {
        let mut selection = RowSelection::default();
        selection.select_only("b");
        selection.toggle("d");
        assert_eq!(selection.move_cursor(1, &ORDER), Some(4));
        assert_eq!(selected(&selection), ["b", "d"]);
        assert_eq!(selection.cursor(), Some("e"));
    }

    #[test]
    fn the_cursor_stops_at_both_ends() {
        let mut selection = RowSelection::default();
        assert_eq!(
            selection.move_cursor(1, &ORDER),
            Some(0),
            "down starts at the top"
        );
        assert_eq!(selection.move_cursor(-5, &ORDER), Some(0));
        assert_eq!(selection.move_cursor(99, &ORDER), Some(5));

        let mut from_below = RowSelection::default();
        assert_eq!(
            from_below.move_cursor(-1, &ORDER),
            Some(5),
            "up starts at the bottom"
        );
        assert_eq!(
            from_below.move_cursor(1, &[]),
            None,
            "no rows, nowhere to go"
        );
    }

    #[test]
    fn a_list_that_lost_rows_drops_them_from_the_selection() {
        let mut selection = RowSelection::default();
        selection.select_only("a");
        selection.toggle("c");
        selection.keep_only(&["b", "c"]);
        assert_eq!(selected(&selection), ["c"]);
        assert_eq!(selection.cursor(), Some("c"));
        selection.keep_only(&["z"]);
        assert!(selection.is_empty());
        assert_eq!(selection.cursor(), None);
    }

    #[test]
    fn select_all_takes_exactly_the_rows_shown() {
        let mut selection = RowSelection::default();
        selection.select_all(&["b", "d"]);
        assert_eq!(selected(&selection), ["b", "d"]);
        selection.clear();
        assert!(selection.is_empty() && selection.cursor().is_none());
    }
}
