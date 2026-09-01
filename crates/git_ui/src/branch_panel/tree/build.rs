//! Building the visible row list from the per-repository snapshots.
//!
//! Split out of `tree.rs` so the data shapes and the algorithm that arranges
//! them stay separately readable. Pure functions only -- no GPUI, no git.

use gpui::SharedString;

use super::{RepoData, RowKey, SectionKind, TreeRow, worktree_label};

/// Builds the visible row list.
///
/// `filter` is matched case-insensitively against branch, worktree and stash
/// labels. A non-empty filter forces every section open: hiding a match behind a
/// collapsed section would make the search look broken.
pub(crate) fn build_rows(
    repos: &[RepoData],
    expanded: &dyn Fn(&RowKey) -> bool,
    filter: &str,
) -> Vec<TreeRow> {
    let filter = filter.trim().to_lowercase();
    let filtering = !filter.is_empty();
    let matches = |text: &str| !filtering || text.to_lowercase().contains(&filter);

    let mut rows = Vec::new();

    for repo in repos {
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

        for kind in SectionKind::ALL {
            push_section(&mut rows, repo, kind, expanded, filtering, &matches);
        }
    }

    rows
}

fn push_section(
    rows: &mut Vec<TreeRow>,
    repo: &RepoData,
    kind: SectionKind,
    expanded: &dyn Fn(&RowKey) -> bool,
    filtering: bool,
    matches: &dyn Fn(&str) -> bool,
) {
    let children = section_children(repo, kind, matches);

    // Under an active filter a section with no match is not worth a line of its
    // own -- the panel should read as the answer to the query, not as the whole
    // tree with empty headers.
    if filtering && children.is_empty() {
        return;
    }

    let section_expanded = filtering || expanded(&RowKey::Section(repo.id, kind));
    rows.push(TreeRow::Section {
        id: repo.id,
        kind,
        count: children.len(),
        expanded: section_expanded,
    });

    if !section_expanded {
        return;
    }

    if children.is_empty() {
        rows.push(TreeRow::Empty {
            label: format!("No {}", kind.label().to_lowercase()).into(),
        });
        return;
    }

    if kind == SectionKind::Remote {
        push_remote_groups(rows, repo, children, expanded, filtering);
    } else {
        rows.extend(children);
    }
}

/// Remote branches are grouped by their remote, so a repo with `origin` and
/// `upstream` does not present one flat list where `origin/main` and
/// `upstream/main` sit side by side looking identical.
fn push_remote_groups(
    rows: &mut Vec<TreeRow>,
    repo: &RepoData,
    children: Vec<TreeRow>,
    expanded: &dyn Fn(&RowKey) -> bool,
    filtering: bool,
) {
    let mut groups: Vec<(SharedString, Vec<TreeRow>)> = Vec::new();
    for row in children {
        let remote: SharedString = match &row {
            TreeRow::Branch { branch, .. } => {
                branch.remote_name().unwrap_or("remote").to_string().into()
            }
            _ => continue,
        };
        match groups.iter_mut().find(|(name, _)| *name == remote) {
            Some((_, rows)) => rows.push(row),
            None => groups.push((remote, vec![row])),
        }
    }

    for (remote, branches) in groups {
        let group_expanded = filtering || expanded(&RowKey::RemoteGroup(repo.id, remote.clone()));
        rows.push(TreeRow::RemoteGroup {
            id: repo.id,
            remote,
            count: branches.len(),
            expanded: group_expanded,
        });
        if group_expanded {
            rows.extend(branches.into_iter().map(|row| match row {
                TreeRow::Branch { id, branch, .. } => TreeRow::Branch {
                    id,
                    branch,
                    depth: 3,
                },
                other => other,
            }));
        }
    }
}

fn section_children(
    repo: &RepoData,
    kind: SectionKind,
    matches: &dyn Fn(&str) -> bool,
) -> Vec<TreeRow> {
    match kind {
        SectionKind::Local => repo
            .branches
            .iter()
            .filter(|branch| !branch.is_remote() && matches(branch.name()))
            .map(|branch| TreeRow::Branch {
                id: repo.id,
                branch: branch.clone(),
                depth: 2,
            })
            .collect(),
        SectionKind::Remote => repo
            .branches
            .iter()
            .filter(|branch| branch.is_remote() && matches(branch.name()))
            .map(|branch| TreeRow::Branch {
                id: repo.id,
                branch: branch.clone(),
                depth: 3,
            })
            .collect(),
        SectionKind::Worktrees => repo
            .worktrees
            .iter()
            .filter(|worktree| matches(&worktree_label(worktree)))
            .map(|worktree| TreeRow::Worktree {
                worktree: worktree.clone(),
            })
            .collect(),
        SectionKind::Tags => repo
            .tags
            .iter()
            .filter(|tag| matches(&tag.name))
            .map(|tag| TreeRow::Tag {
                id: repo.id,
                tag: tag.clone(),
            })
            .collect(),
        SectionKind::Stashes => repo
            .stashes
            .iter()
            .filter(|entry| matches(&entry.message))
            .map(|entry| TreeRow::Stash {
                id: repo.id,
                entry: entry.clone(),
            })
            .collect(),
    }
}
