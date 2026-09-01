use gpui::{App, Entity, SharedString, WeakEntity};
use project::ProjectGroupKey;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use util::path_list::PathList;
use workspace::Workspace;

/// One project group. Also the rail's unit: the rail draws one button per
/// project and never nests, so this stays a flat list there whatever the
/// panel is doing.
#[derive(Clone, Debug)]
pub(crate) struct ListEntry {
    pub(crate) key: ProjectGroupKey,
    pub(crate) label: SharedString,
    pub(crate) highlight_positions: Vec<usize>,
    pub(crate) is_active: bool,
    /// `None` when the group has no open workspace right now (a
    /// remembered-but-closed project) -- there's no `Project` entity to
    /// read a lifecycle label off of.
    pub(crate) activity: Option<project::ProjectActivity>,
    /// FR7: whether this project still carries diagnostic summaries left
    /// over from a hibernated server generation. See
    /// `Project::has_stale_diagnostics`.
    pub(crate) is_reindexing: bool,
    /// Whether the panel is showing this project's worktrees.
    pub(crate) expanded: bool,
    /// How many workspaces this group holds -- the main checkout plus every
    /// linked worktree opened from it. Shown on the header so a collapsed
    /// project still says how much is inside it.
    pub(crate) worktree_count: usize,
}

/// One open workspace under a project: the main checkout, or a git worktree
/// linked to it.
///
/// A group holds more than one of these because `ProjectGroupKey` is keyed by
/// the **main** worktree path (`ProjectGroupKey::from_project`), so opening a
/// linked worktree already lands it in the same group as the repository it came
/// from. Nothing here creates that relationship; it only stops flattening it
/// away.
#[derive(Clone, Debug)]
pub(crate) struct WorktreeRow {
    /// The project group this sits under.
    pub(crate) key: ProjectGroupKey,
    /// Weak on purpose. A row outlives the workspace it names -- a closed
    /// workspace must be collectable, and a strong handle held by a stale row
    /// would keep its whole project alive with nothing pointing at it.
    pub(crate) workspace: WeakEntity<Workspace>,
    pub(crate) label: SharedString,
    /// This workspace's own root. The key a session lookup uses in Phase 03,
    /// and what distinguishes one worktree from another.
    pub(crate) path: Arc<Path>,
    /// The repository's own checkout rather than a linked worktree.
    pub(crate) is_main: bool,
    pub(crate) is_active: bool,
}

/// A row of the panel's tree, flattened.
///
/// Flattened rather than rendered recursively for the same two reasons the
/// branch panel's tree is: the list element needs a length up front, and a
/// plain `Vec` can be asserted against in a test without standing up a window.
#[derive(Clone, Debug)]
pub(crate) enum PanelRow {
    Project(ListEntry),
    Worktree(WorktreeRow),
}

/// Views over the flattened rows.
///
/// Test-only for now: production code walks `entries` in order because that is
/// what the list draws. Phase 03 gives `worktrees` a caller when the agent rows
/// hang off it.
#[cfg(test)]
impl SidebarContents {
    /// The project rows only. What most assertions about "how many projects
    /// are showing" actually mean, now that a project can bring rows of its
    /// own along.
    pub(crate) fn projects(&self) -> impl Iterator<Item = &ListEntry> {
        self.entries.iter().filter_map(|row| match row {
            PanelRow::Project(entry) => Some(entry),
            PanelRow::Worktree(_) => None,
        })
    }

    pub(crate) fn worktrees(&self) -> impl Iterator<Item = &WorktreeRow> {
        self.entries.iter().filter_map(|row| match row {
            PanelRow::Worktree(row) => Some(row),
            PanelRow::Project(_) => None,
        })
    }
}

impl PanelRow {
    pub(crate) fn key(&self) -> &ProjectGroupKey {
        match self {
            PanelRow::Project(entry) => &entry.key,
            PanelRow::Worktree(row) => &row.key,
        }
    }
}

#[derive(Default)]
pub(crate) struct SidebarContents {
    /// The panel's rows, projects and their worktrees interleaved in display
    /// order. A collapsed project contributes exactly one row.
    pub(crate) entries: Vec<PanelRow>,
    /// Every project group, ignoring the filter query. The rail is always
    /// visible and is the only way to switch projects when the panel is
    /// closed, so narrowing it by a query typed into the panel would hide
    /// projects the user can no longer reach any other way.
    pub(crate) rail_entries: Vec<ListEntry>,
    pub(crate) has_open_projects: bool,
}

/// Simple, dependency-free fuzzy matcher: every character of `query` must
/// appear in `candidate`, in order, case-insensitively. Returns the byte
/// offsets it matched at, for highlighting. Salvaged verbatim from the
/// pre-hard-fork sidebar — deliberately not `fuzzy_nucleo` (that crate
/// scores/ranks across many candidates; this only needs a single
/// label-against-query check plus positions to highlight).
pub(crate) fn fuzzy_match_positions(query: &str, candidate: &str) -> Option<Vec<usize>> {
    let mut positions = Vec::new();
    let mut query_chars = query.chars().peekable();

    for (byte_idx, candidate_char) in candidate.char_indices() {
        if let Some(&query_char) = query_chars.peek() {
            if candidate_char.eq_ignore_ascii_case(&query_char) {
                positions.push(byte_idx);
                query_chars.next();
            }
        } else {
            break;
        }
    }

    if query_chars.peek().is_none() {
        Some(positions)
    } else {
        None
    }
}

pub(crate) fn workspace_path_list(workspace: &Entity<Workspace>, cx: &App) -> PathList {
    PathList::new(&workspace.read(cx).root_paths(cx))
}

/// FR2/FR3: builds the sidebar's entry list from `MultiWorkspace` — the
/// single source of truth (NFR1). Re-derived from scratch on every change
/// rather than incrementally patched, so there's no separate "did I miss
/// an update" bug class to worry about.
pub(crate) fn rebuild_contents(
    multi_workspace: &workspace::MultiWorkspace,
    query: &str,
    collapsed: &HashSet<Vec<PathBuf>>,
    cx: &App,
) -> SidebarContents {
    let workspaces: Vec<_> = multi_workspace.workspaces().cloned().collect();
    let active_workspace = multi_workspace.workspace().clone();
    let has_open_projects = workspaces
        .iter()
        .any(|ws| !workspace_path_list(ws, cx).paths().is_empty());

    let groups = multi_workspace.project_groups(cx);
    let mut all_paths: Vec<PathBuf> = groups
        .iter()
        .flat_map(|group| group.key.path_list().paths().iter().cloned())
        .collect();
    all_paths.sort();
    all_paths.dedup();
    let path_details =
        util::disambiguate::compute_disambiguation_details(&all_paths, |path, detail| {
            project::path_suffix(path, detail)
        });
    let path_detail_map: HashMap<PathBuf, usize> =
        all_paths.into_iter().zip(path_details).collect();

    let mut rail_entries = Vec::new();
    for group in &groups {
        if group.key.path_list().paths().is_empty() {
            continue;
        }
        let label = group.key.display_name(&path_detail_map);
        let is_active = group.workspaces.contains(&active_workspace);
        let (activity, is_reindexing) = group
            .workspaces
            .first()
            .map(|workspace| {
                let project = workspace.read(cx).project().read(cx);
                (Some(project.activity()), project.has_stale_diagnostics(cx))
            })
            .unwrap_or((None, false));
        rail_entries.push(ListEntry {
            key: group.key.clone(),
            label,
            highlight_positions: Vec::new(),
            is_active,
            activity,
            is_reindexing,
            expanded: is_expanded(collapsed, &group.key),
            worktree_count: group.workspaces.len(),
        });
    }

    let matched: Vec<ListEntry> = if query.is_empty() {
        rail_entries.clone()
    } else {
        rail_entries
            .iter()
            .filter_map(|entry| {
                let highlight_positions = fuzzy_match_positions(query, &entry.label)?;
                Some(ListEntry {
                    highlight_positions,
                    ..entry.clone()
                })
            })
            .collect()
    };

    // A collapsed project contributes exactly one row: its children are not
    // built, not just not drawn. Building them would make the cost of the list
    // the cost of everything open rather than the cost of what is shown.
    let mut entries = Vec::with_capacity(matched.len());
    for entry in matched {
        let expanded_here = entry.expanded;
        let key = entry.key.clone();
        entries.push(PanelRow::Project(entry));
        if !expanded_here {
            continue;
        }
        let Some(group) = groups.iter().find(|group| group.key == key) else {
            continue;
        };
        for workspace in &group.workspaces {
            if let Some(row) = worktree_row(&key, workspace, &active_workspace, cx) {
                entries.push(PanelRow::Worktree(row));
            }
        }
    }

    SidebarContents {
        entries,
        rail_entries,
        has_open_projects,
    }
}

fn is_expanded(collapsed: &HashSet<Vec<PathBuf>>, key: &ProjectGroupKey) -> bool {
    // Open by default. A project whose only workspace is its own checkout still
    // shows that row, because the row is where the agents live -- hiding it by
    // default would hide the feature.
    !collapsed.contains(&collapsed_marker(key))
}

/// What the collapsed set stores for a project.
///
/// The **paths**, not the `ProjectGroupKey`: the key carries a host and is a
/// session-local shape, while the paths are what is still true after a restart.
/// Storing the *collapsed* projects rather than the expanded ones is what makes
/// "open by default" survive serialisation without writing a row for every
/// project anyone has ever opened.
pub(crate) fn collapsed_marker(key: &ProjectGroupKey) -> Vec<PathBuf> {
    key.path_list().paths().to_vec()
}

/// One open workspace, as a row. `None` for a workspace with no root -- an
/// empty window is not a worktree of anything.
fn worktree_row(
    key: &ProjectGroupKey,
    workspace: &Entity<Workspace>,
    active_workspace: &Entity<Workspace>,
    cx: &App,
) -> Option<WorktreeRow> {
    let path: Arc<Path> = workspace.read(cx).root_paths(cx).first()?.clone();
    let project = workspace.read(cx).project().read(cx);

    // `ordered_pairs` yields (main worktree, this folder). They differ exactly
    // when the folder is a linked worktree of the main one -- which is the
    // whole distinction this row draws.
    let is_main = project
        .worktree_paths(cx)
        .ordered_pairs()
        .find(|(_, folder)| folder.as_path() == path.as_ref())
        .map(|(main, folder)| main == folder)
        .unwrap_or(true);

    let label = branch_label(workspace, &path, cx).unwrap_or_else(|| folder_label(&path));

    Some(WorktreeRow {
        key: key.clone(),
        workspace: workspace.downgrade(),
        label,
        path,
        is_main,
        is_active: workspace == active_workspace,
    })
}

/// The branch this workspace has checked out, if a repository of its own
/// covers it. Read from the snapshot the git store already keeps -- no git
/// command runs for a sidebar row.
fn branch_label(workspace: &Entity<Workspace>, path: &Arc<Path>, cx: &App) -> Option<SharedString> {
    let project = workspace.read(cx).project().read(cx);
    let git_store = project.git_store().read(cx);
    let repository = git_store
        .repositories()
        .values()
        .find(|repository| repository.read(cx).work_directory_abs_path.as_ref() == path.as_ref())?;
    let branch = repository.read(cx).branch.as_ref()?;
    Some(branch.name().to_string().into())
}

fn folder_label(path: &Arc<Path>) -> SharedString {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
        .into()
}
