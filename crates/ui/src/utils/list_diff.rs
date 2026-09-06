//! Telling `gpui::ListState` what actually changed.
//!
//! `ListState` caches a measured height per row, so a list whose rows are not
//! all the same height has to invalidate that cache when the rows move.
//! `ListState::reset` is the easy call and the wrong one: it also discards the
//! scroll position, so expanding a section near the bottom of a long list
//! throws the reader back to the top -- the row they just clicked scrolls out
//! of sight, which reads as the click having done nothing.
//!
//! `splice` keeps the scroll anchored, but needs to be told which slice moved.
//! When a row's height depends only on its *kind* -- a two-line card, a
//! one-line header -- comparing kinds is enough to find that slice.

use std::ops::Range;

/// The slice that differs between two row layouts, as the `(old_range,
/// new_count)` pair `gpui::ListState::splice` expects. `None` when they match.
///
/// Compares by common prefix and suffix, which is exactly right for the shape
/// these lists change in: expanding or collapsing inserts or removes one
/// contiguous run and leaves everything around it alone.
pub fn changed_range<T: PartialEq>(old: &[T], new: &[T]) -> Option<(Range<usize>, usize)> {
    if old == new {
        return None;
    }

    let prefix = old
        .iter()
        .zip(new)
        .take_while(|(old, new)| old == new)
        .count();
    // The prefix and the suffix must not both claim the same element, or the
    // range runs backwards and `splice` panics. Repeated kinds -- three
    // identical rows in a row -- are exactly when they try to.
    let unmatched = old.len().min(new.len()) - prefix;
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take_while(|(old, new)| old == new)
        .count()
        .min(unmatched);

    Some((prefix..old.len() - suffix, new.len() - suffix - prefix))
}

#[cfg(test)]
mod tests {
    use super::changed_range;

    #[test]
    fn an_unchanged_layout_needs_no_splice() {
        assert_eq!(changed_range(&[1, 2, 3], &[1, 2, 3]), None);
    }

    #[test]
    fn expanding_a_section_splices_only_the_rows_it_revealed() {
        assert_eq!(
            changed_range(&[0, 1, 9], &[0, 1, 5, 5, 5, 9]),
            Some((2..2, 3))
        );
    }

    #[test]
    fn collapsing_a_section_splices_only_the_rows_it_hid() {
        assert_eq!(
            changed_range(&[0, 1, 5, 5, 5, 9], &[0, 1, 9]),
            Some((2..5, 0))
        );
    }

    #[test]
    fn a_layout_replaced_wholesale_splices_everything() {
        assert_eq!(changed_range(&[1, 1], &[2, 2, 2]), Some((0..2, 3)));
    }

    /// A repeated element makes the prefix and the suffix reach for the same
    /// row. Unclamped, that produces a backwards range and a panic inside
    /// `splice`.
    #[test]
    fn a_repeated_row_kind_cannot_make_the_range_run_backwards() {
        let (range, count) = changed_range(&[7, 7], &[7, 7, 7]).expect("layout changed");
        assert!(range.start <= range.end);
        assert_eq!(2 - (range.end - range.start) + count, 3);
    }

    #[test]
    fn an_empty_list_filling_up_splices_from_zero() {
        assert_eq!(changed_range::<u8>(&[], &[1, 2]), Some((0..0, 2)));
    }
}
