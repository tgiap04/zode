//! Tests for the panel's tree.
//!
//! The claim these defend is `perf-constraints.md` luật 3: a project the reader
//! closed contributes exactly one row, because its children are never built --
//! not built and then hidden. Without a test the difference is invisible, and
//! "hidden" is the version that makes the list cost what is open rather than
//! what is shown.

use crate::Sidebar;
use crate::sidebar_tests::init_test;
use fs::FakeFs;
use gpui::{AppContext as _, TestAppContext};
use project::Project;
use serde_json::json;
use workspace::MultiWorkspace;

async fn two_projects(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<Sidebar>,
    gpui::Entity<MultiWorkspace>,
    &mut gpui::VisualTestContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "b.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs, ["/root_b".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a, window, cx));
    let sidebar = multi_workspace.update_in(cx, |mw, window, cx| {
        let mw_entity = cx.entity();
        let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
        mw.register_sidebar(sidebar.clone(), cx);
        sidebar
    });
    multi_workspace.update(cx, |mw, cx| mw.test_enable_background_retention(cx));
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b, window, cx);
    });
    cx.run_until_parked();

    (sidebar, multi_workspace.clone(), cx)
}

/// Open by default: the worktree row is where the agents will live, so hiding
/// it until someone finds a chevron would hide the feature.
#[gpui::test]
async fn a_project_shows_its_worktree_by_default(cx: &mut TestAppContext) {
    let (sidebar, _mw, cx) = two_projects(cx).await;

    sidebar.read_with(cx, |sidebar, _| {
        assert_eq!(sidebar.contents.projects().count(), 2);
        assert_eq!(
            sidebar.contents.worktrees().count(),
            2,
            "each project's own checkout is a row under it"
        );
    });
}

/// Luật 3. The rows are not built, so a closed project costs one row however
/// many workspaces it holds.
#[gpui::test]
async fn a_collapsed_project_contributes_exactly_one_row(cx: &mut TestAppContext) {
    let (sidebar, _mw, cx) = two_projects(cx).await;

    let first = sidebar.read_with(cx, |sidebar, _| {
        sidebar.contents.projects().next().unwrap().key.clone()
    });
    sidebar.update(cx, |sidebar, cx| {
        sidebar.toggle_project_expansion(&first, cx)
    });

    sidebar.read_with(cx, |sidebar, _| {
        assert_eq!(
            sidebar.contents.projects().count(),
            2,
            "collapsing hides children, never the project itself"
        );
        assert_eq!(
            sidebar.contents.worktrees().count(),
            1,
            "only the project still open contributes a worktree row"
        );
        assert_eq!(sidebar.contents.entries.len(), 3);
    });
}

/// A rebuild happens on every git event and every workspace change. If the
/// closed set were re-applied from a restored copy on each one -- the shape
/// the branch panel shipped and had to fix -- a project could be collapsed and
/// spring straight back open.
#[gpui::test]
async fn a_collapsed_project_survives_a_rebuild(cx: &mut TestAppContext) {
    let (sidebar, _mw, cx) = two_projects(cx).await;

    let first = sidebar.read_with(cx, |sidebar, _| {
        sidebar.contents.projects().next().unwrap().key.clone()
    });
    sidebar.update(cx, |sidebar, cx| {
        sidebar.toggle_project_expansion(&first, cx)
    });
    sidebar.update(cx, |sidebar, cx| sidebar.update_entries(cx));
    sidebar.update(cx, |sidebar, cx| sidebar.update_entries(cx));

    sidebar.read_with(cx, |sidebar, _| {
        assert_eq!(
            sidebar.contents.worktrees().count(),
            1,
            "two rebuilds must not reopen what the reader closed"
        );
    });
}

/// Toggling is a toggle, not a one-way door.
#[gpui::test]
async fn expanding_again_brings_the_row_back(cx: &mut TestAppContext) {
    let (sidebar, _mw, cx) = two_projects(cx).await;

    let first = sidebar.read_with(cx, |sidebar, _| {
        sidebar.contents.projects().next().unwrap().key.clone()
    });
    sidebar.update(cx, |sidebar, cx| {
        sidebar.toggle_project_expansion(&first, cx);
        sidebar.toggle_project_expansion(&first, cx);
    });

    sidebar.read_with(cx, |sidebar, _| {
        assert_eq!(sidebar.contents.worktrees().count(), 2);
    });
}

/// The rail is the only project switcher visible when the panel is closed, so
/// it stays one button per project whatever the panel's tree is doing.
#[gpui::test]
async fn the_rail_never_nests(cx: &mut TestAppContext) {
    let (sidebar, _mw, cx) = two_projects(cx).await;

    let first = sidebar.read_with(cx, |sidebar, _| {
        sidebar.contents.projects().next().unwrap().key.clone()
    });
    sidebar.update(cx, |sidebar, cx| {
        sidebar.toggle_project_expansion(&first, cx)
    });

    sidebar.read_with(cx, |sidebar, _| {
        assert_eq!(
            sidebar.contents.rail_entries.len(),
            2,
            "the rail lists projects, and collapsing one must not change that"
        );
    });
}
