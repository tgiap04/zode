//! Building the visible row list from the per-repository snapshots.
//!
//! Split out of `tree.rs` so the data shapes and the algorithm that arranges
//! them stay separately readable. Pure functions only -- no GPUI, no git.

use std::sync::Arc;

use super::{RepoData, RowKey, TreeRow, worktree_label};

/// Builds the visible row list: each repository, then its checkouts.
///
/// `filter` is matched case-insensitively against a checkout's label and its
/// path. A repository whose checkouts all fail the filter drops out entirely
/// rather than sitting there as an empty header -- under a query the panel
/// should read as the answer, not as the whole list with the matches in it.
pub(crate) fn build_rows(
    repos: &[RepoData],
    expanded: &dyn Fn(&RowKey) -> bool,
    filter: &str,
) -> Vec<TreeRow> {
    let filter = filter.trim().to_lowercase();
    let filtering = !filter.is_empty();

    let mut rows = Vec::new();

    for repo in repos {
        let checkouts: Vec<_> = repo
            .worktrees
            .iter()
            .filter(|worktree| {
                !filtering || {
                    let label = worktree_label(worktree).to_lowercase();
                    let path = worktree.path.to_string_lossy().to_lowercase();
                    label.contains(&filter) || path.contains(&filter)
                }
            })
            .collect();

        if filtering && checkouts.is_empty() {
            continue;
        }

        // A filter forces the repository open: hiding a match behind a
        // collapsed row would make the search look broken.
        let repo_expanded = filtering || expanded(&RowKey::Repo(repo.id));
        rows.push(TreeRow::Repo {
            id: repo.id,
            name: repo.name.clone(),
            current_branch: repo.current_branch.clone(),
            expanded: repo_expanded,
        });

        if !repo_expanded {
            continue;
        }

        if checkouts.is_empty() {
            // `git worktree list` always names the main checkout, so an empty
            // list means something went wrong rather than that there is nothing
            // to show. Saying so beats a blank panel.
            rows.push(TreeRow::Empty {
                label: "No checkouts found".into(),
            });
            continue;
        }

        for worktree in checkouts {
            let path: Arc<std::path::Path> = Arc::from(worktree.path.as_path());
            let agents = repo
                .agents
                .get(&path)
                .cloned()
                .unwrap_or_else(|| Arc::from([]));
            let has_agents = !agents.is_empty();
            rows.push(TreeRow::Worktree {
                id: repo.id,
                worktree: worktree.clone(),
                agents,
                expanded: has_agents && expanded(&RowKey::WorktreeAgents(repo.id, path)),
            });
        }
    }

    rows
}
