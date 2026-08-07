use gpui::{App, Entity, SharedString};
use project::ProjectGroupKey;
use std::collections::HashMap;
use std::path::PathBuf;
use util::path_list::PathList;
use workspace::Workspace;

/// One row in the sidebar's list. Every row is a project group today — the
/// pre-hard-fork sidebar this crate is salvaged from also had a `Thread`
/// variant (AI agent threads nested under each project); that entire
/// concept is gone, which is also why there is no expand/collapse here:
/// collapsing a header only ever meant "hide its threads," and there is
/// nothing left to hide.
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
}

#[derive(Default)]
pub(crate) struct SidebarContents {
    pub(crate) entries: Vec<ListEntry>,
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
        });
    }

    let entries = if query.is_empty() {
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

    SidebarContents {
        entries,
        rail_entries,
        has_open_projects,
    }
}
