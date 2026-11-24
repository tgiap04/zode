//! Reads a list GitHub hands out a page at a time, up to a bound, so a menu
//! of labels or a tree of sub-issues is not cut at the first page and a
//! cursor that never moves on cannot keep the read going.

use std::collections::HashSet;

use github::{GithubFuture, GithubResult, Page};

/// Pages read for a menu of labels or milestones: enough for 500 of them.
pub(crate) const MAX_OPTION_PAGES: usize = 10;

pub(crate) struct ReadPages<T> {
    pub items: Vec<T>,
    /// More exists than was read: a bound was hit, or GitHub's cursor did
    /// not move on.
    pub cut_short: bool,
}

/// Every page `fetch` yields, at most `max_pages` of them and until
/// `max_items` are held. Stops at a cursor already seen: a read cannot go
/// round in a loop.
pub(crate) async fn read_pages<T>(
    max_pages: usize,
    max_items: usize,
    mut fetch: impl FnMut(Option<String>) -> GithubFuture<Page<T>>,
) -> GithubResult<ReadPages<T>> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = None;
    for _ in 0..max_pages {
        let page = fetch(cursor.take()).await?;
        items.extend(page.items);
        let Some(next) = page.next_cursor else {
            return Ok(ReadPages {
                items,
                cut_short: false,
            });
        };
        if items.len() >= max_items || !seen.insert(next.clone()) {
            return Ok(ReadPages {
                items,
                cut_short: true,
            });
        }
        cursor = Some(next);
    }
    Ok(ReadPages {
        items,
        cut_short: true,
    })
}

/// What a menu of labels or milestones says when GitHub has more than were
/// read, naming how many it holds.
pub(crate) fn options_note(labels: Option<usize>, milestones: Option<usize>) -> Option<String> {
    let (what, shown) = match (labels, milestones) {
        (Some(labels), Some(_)) => ("labels and milestones", labels),
        (Some(labels), None) => ("labels", labels),
        (None, Some(milestones)) => ("milestones", milestones),
        (None, None) => return None,
    };
    Some(format!(
        "Showing the first {shown} {what}; GitHub has more."
    ))
}
