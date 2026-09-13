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
    /// FR7: whether this project is genuinely mid-reindex after waking.
    /// Narrower than `Project::has_stale_diagnostics` on its own -- see
    /// [`is_reindexing`].
    pub(crate) is_reindexing: bool,
}

/// Whether a project is genuinely mid-reindex, as opposed to merely
/// carrying diagnostics from a hibernated server generation.
///
/// `Project::has_stale_diagnostics` alone is not that question.
/// `LspStore::hibernate` fills `stale_language_servers` with every server
/// it stops, at the moment the project *goes to sleep* -- so the flag is
/// already true for the whole time the project sits hibernated, doing
/// nothing. Only `clear_stale_diagnostics_after_reindex` clears it, and
/// that cannot run until a replacement server has woken and finished a
/// pass.
///
/// The activity label is what separates the two halves of that window.
/// `reconcile_resource_activity` reaches `wake_resources` only on the
/// `Hibernated -> Active` edge, and only *after* `set_activity` has
/// committed the new label -- so a project that is actually reindexing
/// always reads `Active` (or `Warm`, if it lost focus again since), and
/// never `Hibernated`.
///
/// Both readers give this indicator priority over the hibernated one, on
/// the grounds that "just woke and is mid-restart" is the more specific
/// state. That reasoning is sound and was being applied to the wrong
/// projects: without this, a sleeping project drew the re-indexing mark
/// and its "hibernated" mark never appeared at all.
pub(crate) fn is_reindexing(
    activity: project::ProjectActivity,
    has_stale_diagnostics: bool,
) -> bool {
    has_stale_diagnostics && activity != project::ProjectActivity::Hibernated
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
                let activity = project.activity();
                (
                    Some(activity),
                    is_reindexing(activity, project.has_stale_diagnostics(cx)),
                )
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

#[cfg(test)]
mod tests {
    use super::is_reindexing;
    use project::ProjectActivity;

    /// The defect this function was extracted for.
    ///
    /// `LspStore::hibernate` records every server it stops, so
    /// `has_stale_diagnostics` is already true while the project sits
    /// asleep. The sidebar read that as "re-indexing after waking" and
    /// drew the mark on projects that were doing nothing at all -- and
    /// because both readers give re-indexing priority, the hibernated
    /// mark could never appear.
    #[test]
    fn a_sleeping_project_is_not_reindexing() {
        assert!(
            !is_reindexing(ProjectActivity::Hibernated, true),
            "a hibernated project carries stale diagnostics by construction; \
             that is not the same as being mid-reindex"
        );
    }

    /// The counterweight. A fix that simply switched the mark off would
    /// pass the test above and fail the feature.
    #[test]
    fn a_woken_project_that_has_not_finished_indexing_is_reindexing() {
        assert!(
            is_reindexing(ProjectActivity::Active, true),
            "woken and still carrying the previous generation's diagnostics"
        );
        assert!(
            is_reindexing(ProjectActivity::Warm, true),
            "woke, then lost focus again before its servers finished -- still \
             mid-reindex, and `Warm` is reachable from `Active` at any moment"
        );
    }

    #[test]
    fn nothing_stale_means_nothing_to_reindex() {
        for activity in [
            ProjectActivity::Active,
            ProjectActivity::Warm,
            ProjectActivity::Hibernated,
        ] {
            assert!(!is_reindexing(activity, false), "{activity:?}");
        }
    }
}
