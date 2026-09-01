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

/// The agent rows under a worktree.
///
/// What makes them cheap is that they come from the shared session index by
/// exact working directory -- one hash per row, no scan. These tests stock that
/// index directly rather than sweeping the real transcripts, which is also the
/// only way they could be deterministic.
mod agents {
    use super::*;
    use agent_sessions::{AgentKind, SessionSummary};
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn session(id: &str, cwd: &str, title: &str) -> SessionSummary {
        SessionSummary {
            id: std::sync::Arc::from(id),
            agent: AgentKind::Claude,
            title: title.to_string(),
            preview: String::new(),
            preview_speaker: None,
            cwd: PathBuf::from(cwd),
            branch: None,
            model: None,
            updated_at: SystemTime::UNIX_EPOCH,
            log_path: None,
            log_bytes: 0,
        }
    }

    fn stock(
        sidebar: &gpui::Entity<Sidebar>,
        sessions: Vec<SessionSummary>,
        cx: &mut gpui::VisualTestContext,
    ) {
        sidebar.update(cx, |sidebar, cx| {
            let store = agent_ui::SessionStore::global(cx);
            store.update(cx, |store, cx| store.set_index_for_test(sessions, cx));
            sidebar.session_store = Some(store);
            sidebar.update_entries(cx);
        });
    }

    #[gpui::test]
    async fn a_session_appears_under_the_worktree_it_ran_in(cx: &mut TestAppContext) {
        let (sidebar, _mw, cx) = two_projects(cx).await;
        stock(
            &sidebar,
            vec![session("one", "/root_a", "Fix the parser")],
            cx,
        );

        sidebar.read_with(cx, |sidebar, _| {
            let agents: Vec<_> = sidebar.contents.agents().collect();
            assert_eq!(agents.len(), 1, "one session, one row");
            assert_eq!(agents[0].label.as_ref(), "Fix the parser");
        });
    }

    /// Exact directory, not prefix. A session from a sibling checkout showing
    /// up under this worktree would be worse than showing nothing: it invites
    /// resuming work in the wrong tree.
    #[gpui::test]
    async fn a_session_from_elsewhere_does_not_appear(cx: &mut TestAppContext) {
        let (sidebar, _mw, cx) = two_projects(cx).await;
        stock(
            &sidebar,
            vec![session("stray", "/somewhere_else", "Not ours")],
            cx,
        );

        sidebar.read_with(cx, |sidebar, _| {
            assert_eq!(sidebar.contents.agents().count(), 0);
        });
    }

    /// Luật 3 reaches the agent rows too: a closed project builds none of them.
    #[gpui::test]
    async fn a_collapsed_project_builds_no_agent_rows(cx: &mut TestAppContext) {
        let (sidebar, _mw, cx) = two_projects(cx).await;
        stock(
            &sidebar,
            vec![
                session("a", "/root_a", "Under A"),
                session("b", "/root_b", "Under B"),
            ],
            cx,
        );
        sidebar.read_with(cx, |sidebar, _| {
            assert_eq!(sidebar.contents.agents().count(), 2, "both projects open");
        });

        let first = sidebar.read_with(cx, |sidebar, _| {
            sidebar.contents.projects().next().unwrap().key.clone()
        });
        sidebar.update(cx, |sidebar, cx| {
            sidebar.toggle_project_expansion(&first, cx)
        });

        sidebar.read_with(cx, |sidebar, _| {
            assert_eq!(
                sidebar.contents.agents().count(),
                1,
                "the collapsed project's sessions are not built, only its neighbour's"
            );
        });
    }

    /// Several sessions in one worktree all belong to it -- this is the case
    /// the whole feature exists for.
    #[gpui::test]
    async fn several_sessions_in_one_worktree_all_show(cx: &mut TestAppContext) {
        let (sidebar, _mw, cx) = two_projects(cx).await;
        stock(
            &sidebar,
            vec![
                session("one", "/root_a", "First"),
                session("two", "/root_a", "Second"),
                session("three", "/root_a", "Third"),
            ],
            cx,
        );

        sidebar.read_with(cx, |sidebar, _| {
            assert_eq!(sidebar.contents.agents().count(), 3);
        });
    }
}
