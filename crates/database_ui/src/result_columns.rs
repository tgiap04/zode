//! How wide each column of the results starts out.
//!
//! The grid used to share its width equally between every column, so a table
//! with twenty of them cut every name and every value down to a few letters.
//! Each column now starts as wide as what it holds and the grid scrolls
//! sideways past the edge; a column can still be dragged wider or narrower.

use crate::database_panel::DatabasePanel;
use crate::query::{QueryState, cell_text};
use database::protocol::ResultSet;
use gpui::{AbsoluteLength, Context, SharedString, rems};
use ui::{ResizableColumnsState, TableResizeBehavior};

/// How many rows are read to size a column. A page can be thousands of rows,
/// and the first screenful says enough about what a column holds.
const ROWS_SAMPLED: usize = 50;

/// About one character of the grid's small text, in rems.
const REMS_PER_CHARACTER: f32 = 0.5;

/// The cell's padding on both sides, plus room for the resize handle.
const PADDING_REMS: f32 = 1.5;

/// A column holding one digit still has a name worth reading.
const MIN_COLUMN_REMS: f32 = 3.5;

/// A column of long text stops here and truncates; dragging widens it further.
const MAX_COLUMN_REMS: f32 = 18.0;

/// The width a column starts at, from the longest of its name and the values
/// sampled from it, in characters.
pub(crate) fn column_width_rems(longest_characters: usize) -> f32 {
    (longest_characters as f32 * REMS_PER_CHARACTER + PADDING_REMS)
        .clamp(MIN_COLUMN_REMS, MAX_COLUMN_REMS)
}

/// What the header asks room for, in characters: the name, the type beside
/// it, and a gap and an icon's worth for the key or the sort arrow. Counting
/// the name alone cut a short column such as `id integer` to an ellipsis.
fn header_characters(name: &str, type_name: &str) -> usize {
    let type_characters = type_name.chars().count();
    name.chars().count() + type_characters + if type_characters > 0 { 1 } else { 0 } + 2
}

fn starting_widths(result: &ResultSet) -> Vec<AbsoluteLength> {
    result
        .columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let longest_value = result
                .rows
                .iter()
                .take(ROWS_SAMPLED)
                .filter_map(|row| row.get(index))
                .map(|cell| cell_text(cell).chars().count())
                .max()
                .unwrap_or(0);
            let longest = header_characters(&column.name, &column.type_name).max(longest_value);
            AbsoluteLength::Rems(rems(column_width_rems(longest)))
        })
        .collect()
}

impl DatabasePanel {
    /// Starts the column widths over when the result has different columns,
    /// and keeps them -- dragged ones included -- when it has the same ones,
    /// which is what paging through one statement gives.
    pub(crate) fn sync_result_columns(&mut self, cx: &mut Context<Self>) {
        let QueryState::Done(page) = &self.query else {
            return;
        };
        let names: Vec<SharedString> = page
            .result
            .columns
            .iter()
            .map(|column| SharedString::from(column.name.clone()))
            .collect();
        if names == self.result_column_names {
            return;
        }
        let widths = starting_widths(&page.result);
        let count = widths.len();
        self.result_column_widths.update(cx, |state, _cx| {
            *state = ResizableColumnsState::new(
                count,
                widths,
                vec![TableResizeBehavior::MinSize(MIN_COLUMN_REMS); count],
            );
        });
        self.result_column_names = names;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_starts_as_wide_as_what_it_holds_within_limits() {
        assert_eq!(column_width_rems(0), MIN_COLUMN_REMS);
        assert_eq!(
            column_width_rems(10),
            10.0 * REMS_PER_CHARACTER + PADDING_REMS
        );
        assert!(
            column_width_rems(10) > column_width_rems(4),
            "a longer name gets a wider column"
        );
        assert_eq!(column_width_rems(10_000), MAX_COLUMN_REMS);
    }

    /// A short name with its type beside it still fits, rather than starting
    /// cut to an ellipsis.
    #[test]
    fn the_header_counts_the_type_beside_the_name() {
        assert_eq!(header_characters("id", "integer"), 2 + 1 + 7 + 2);
        assert_eq!(header_characters("id", ""), 2 + 2);
        assert!(column_width_rems(header_characters("id", "integer")) > MIN_COLUMN_REMS);
    }
}
