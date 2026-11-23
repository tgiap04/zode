//! The board's columns and the table's row order, worked out from the items,
//! the view and the project's fields. Sorting and grouping every item costs
//! far more than drawing a frame should, so the result is kept until
//! something it depends on changes.
//!
//! Everything that changes `items`, `details` or the picked view calls
//! [`ProjectsSection::layout_changed`]; in tests every read also checks the
//! kept layout against a fresh one, so a missed call fails the test that
//! drew it.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{ListAlignment, ListState, px};

use crate::projects::project_layout::{
    BoardColumn, ColumnKey, board_columns_in_order, sorted_items,
};
use crate::projects::projects_section::ProjectsSection;

/// One column's scrolling list of cards.
struct ColumnList {
    state: ListState,
    /// The [`LayoutCache::revision`] the list's item count was set for.
    revision: u64,
}

#[derive(Default)]
pub(super) struct LayoutCache {
    order: RefCell<Option<Rc<Vec<usize>>>>,
    columns: RefCell<Option<Rc<Vec<BoardColumn>>>>,
    /// Bumped by every [`ProjectsSection::layout_changed`].
    revision: u64,
    column_lists: RefCell<HashMap<ColumnKey, ColumnList>>,
    /// How many times a layout was worked out, for tests.
    pub(super) builds: Cell<usize>,
}

impl LayoutCache {
    /// Another project's columns share nothing with the last one's.
    pub(super) fn forget_column_lists(&mut self) {
        self.column_lists.get_mut().clear();
    }

    fn clear(&mut self) {
        *self.order.get_mut() = None;
        *self.columns.get_mut() = None;
        self.revision += 1;
    }

    #[cfg(test)]
    pub(super) fn column_list_keys(&self) -> Vec<ColumnKey> {
        self.column_lists.borrow().keys().cloned().collect()
    }
}

impl ProjectsSection {
    /// The items, the view or the fields changed: the kept layout is stale.
    pub(super) fn layout_changed(&mut self) {
        self.layout.clear();
    }

    /// The list that draws `key`'s `count` cards, only the ones in view.
    /// Cards differ in height, so the list measures them as they come into
    /// view; when the layout changed the count is set again and what was
    /// measured is dropped, but the place scrolled to is kept.
    pub(super) fn column_list(&self, key: &ColumnKey, count: usize) -> ListState {
        let mut lists = self.layout.column_lists.borrow_mut();
        let revision = self.layout.revision;
        let list = lists.entry(key.clone()).or_insert_with(|| ColumnList {
            state: ListState::new(count, ListAlignment::Top, px(400.)),
            revision,
        });
        if list.revision != revision {
            list.state.splice(0..list.state.item_count(), count);
            list.revision = revision;
        }
        list.state.clone()
    }

    /// Scrolls the column holding `item_id` to show its card.
    pub(super) fn reveal_card(&self, item_id: &str) {
        let Some(columns) = self.columns() else {
            return;
        };
        let lists = self.layout.column_lists.borrow();
        for column in columns.iter() {
            let position = column.items.iter().position(|index| {
                self.items
                    .get(*index)
                    .is_some_and(|item| item.id == item_id)
            });
            if let (Some(position), Some(list)) = (position, lists.get(&column.key)) {
                list.state.scroll_to_reveal_item(position);
                return;
            }
        }
    }

    /// Item indices in the view's order.
    pub(crate) fn sorted_order(&self) -> Option<Rc<Vec<usize>>> {
        let details = self.details()?;
        let kept = self.layout.order.borrow().clone();
        if let Some(kept) = kept {
            #[cfg(test)]
            assert_eq!(
                *kept,
                sorted_items(self.view(), details, self.items()),
                "the layout changed without `layout_changed`"
            );
            return Some(kept);
        }
        self.layout.builds.set(self.layout.builds.get() + 1);
        let order = Rc::new(sorted_items(self.view(), details, self.items()));
        *self.layout.order.borrow_mut() = Some(order.clone());
        Some(order)
    }

    /// The board's columns, items in the view's order.
    pub(crate) fn columns(&self) -> Option<Rc<Vec<BoardColumn>>> {
        let details = self.details()?;
        let kept = self.layout.columns.borrow().clone();
        if let Some(kept) = kept {
            #[cfg(test)]
            assert_eq!(
                *kept,
                crate::projects::project_layout::board_columns(self.view(), details, self.items()),
                "the layout changed without `layout_changed`"
            );
            return Some(kept);
        }
        let order = self.sorted_order()?;
        self.layout.builds.set(self.layout.builds.get() + 1);
        let columns = Rc::new(board_columns_in_order(
            self.view(),
            details,
            self.items(),
            order.to_vec(),
        ));
        // Lists of columns the view no longer has would stay for good.
        self.layout
            .column_lists
            .borrow_mut()
            .retain(|key, _| columns.iter().any(|column| &column.key == key));
        *self.layout.columns.borrow_mut() = Some(columns.clone());
        Some(columns)
    }
}
