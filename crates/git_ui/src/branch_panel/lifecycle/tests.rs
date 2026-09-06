//! Tests for the panel's rebuild discipline.
//!
//! The panel's whole performance story rests on one claim: a panel nobody is
//! looking at does no work. That is true *by construction* -- the rebuild lives
//! inside `render`, and a hidden panel is not rendered -- but construction is
//! not evidence. `rebuild_count` makes the claim falsifiable, and these tests
//! are what falsify it if someone later moves the rebuild to the event handler.

use gpui::{AppContext as _, TestAppContext, VisualTestContext};
use project::Project;
use settings::SettingsStore;
use workspace::dock::Panel as _;
use workspace::{AppState, Workspace};

use crate::branch_panel::panel::BranchPanel;

fn init_test(cx: &mut TestAppContext) -> std::sync::Arc<AppState> {
    cx.update(|cx| {
        let settings_store = SettingsStore::test(cx);
        cx.set_global(settings_store);
        let state = AppState::test(cx);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        editor::init(cx);
        crate::init(cx);
        state
    })
}

async fn panel(cx: &mut TestAppContext) -> (gpui::Entity<BranchPanel>, &mut VisualTestContext) {
    let app_state = init_test(cx);
    let project = Project::test(app_state.fs.clone(), [], cx).await;
    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        BranchPanel::new(workspace, window, cx)
    });
    (panel, cx)
}

/// A panel that has never been shown must not have built anything, however many
/// git events arrived while it was hidden.
#[gpui::test]
async fn a_hidden_panel_does_not_rebuild(cx: &mut TestAppContext) {
    let (panel, cx) = panel(cx).await;

    panel.update(cx, |panel, cx| {
        assert!(!panel.is_active, "a fresh panel starts hidden");
        for _ in 0..20 {
            panel.mark_stale(cx);
        }
        assert_eq!(
            panel.rebuild_count, 0,
            "twenty events while hidden must cost zero rebuilds"
        );
        assert!(panel.rows.is_empty());
    });
}

/// A burst of events collapses into one rebuild rather than one each. This is
/// what keeps a `git fetch` that touches fifty refs from rebuilding the tree
/// fifty times.
#[gpui::test]
async fn a_burst_of_events_costs_one_rebuild(cx: &mut TestAppContext) {
    let (panel, cx) = panel(cx).await;

    panel.update_in(cx, |panel, window, cx| {
        panel.set_active(true, window, cx);
        for _ in 0..20 {
            panel.mark_stale(cx);
        }
        panel.refresh_if_stale(cx);
        assert_eq!(
            panel.rebuild_count, 1,
            "the stale flag is read once at render, not once per event"
        );
    });
}

/// Rendering an unchanged panel repeatedly must not rebuild: the flag, not the
/// frame, decides.
#[gpui::test]
async fn rendering_an_unchanged_panel_rebuilds_nothing(cx: &mut TestAppContext) {
    let (panel, cx) = panel(cx).await;

    panel.update_in(cx, |panel, window, cx| {
        panel.set_active(true, window, cx);
        panel.refresh_if_stale(cx);
        let after_first = panel.rebuild_count;

        for _ in 0..5 {
            panel.refresh_if_stale(cx);
        }
        assert_eq!(
            panel.rebuild_count, after_first,
            "five more frames with nothing changed cost nothing"
        );
    });
}

/// Being shown again marks the tree stale, because events that arrived while
/// hidden were deliberately not acted on.
#[gpui::test]
async fn becoming_visible_schedules_exactly_one_rebuild(cx: &mut TestAppContext) {
    let (panel, cx) = panel(cx).await;

    panel.update_in(cx, |panel, window, cx| {
        panel.set_active(true, window, cx);
        panel.refresh_if_stale(cx);
        panel.set_active(false, window, cx);

        panel.mark_stale(cx);
        assert_eq!(panel.rebuild_count, 1, "still hidden, still no work");

        panel.set_active(true, window, cx);
        panel.refresh_if_stale(cx);
        assert_eq!(panel.rebuild_count, 2, "one rebuild on being shown again");
    });
}

/// Hiding the panel stops the agent-activity tick.
///
/// The tick is the one thing here that runs while the user does nothing, so it
/// has to answer to the same rule as everything else: a panel nobody is
/// looking at costs nothing. Nothing else would notice it leaking -- it fires
/// into an entity that is simply not being drawn -- which is exactly why it is
/// asserted rather than trusted.
#[gpui::test]
async fn hiding_the_panel_stops_the_activity_tick(cx: &mut TestAppContext) {
    let (panel, cx) = panel(cx).await;

    panel.update_in(cx, |panel, window, cx| {
        panel.set_active(true, window, cx);
        panel._activity_tick = Some(gpui::Task::ready(()));

        panel.set_active(false, window, cx);
        assert!(
            panel._activity_tick.is_none(),
            "a hidden panel must not keep asking to be redrawn"
        );
    });
}

/// With nothing live to watch, the tick is not started at all.
#[gpui::test]
async fn a_panel_with_no_live_agent_runs_no_tick(cx: &mut TestAppContext) {
    let (panel, cx) = panel(cx).await;

    panel.update_in(cx, |panel, window, cx| {
        panel.set_active(true, window, cx);
        panel.refresh_if_stale(cx);
        panel.track_agent_activity(cx);
        assert!(
            panel._activity_tick.is_none(),
            "finished transcripts have no mark that can change"
        );
    });
}

/// Opening an agent tab has to reach the panel.
///
/// It did not. The panel learned about agents only when something else
/// rebuilt it -- a git event, or switching checkouts, which throws the panel
/// away and builds a new one. So pressing New Agent on the rail or in the
/// editor added nothing visible, and the list appeared to "need" a switch away
/// and back. That was not a refresh; it was a different panel.
///
/// The tab here never starts a CLI -- there is no agent binary in a test -- and
/// it does not need to. What is asserted is that the tab's arrival reaches the
/// panel at all, which is the link that was missing.
#[gpui::test]
async fn opening_an_agent_tab_marks_the_panel_stale(cx: &mut TestAppContext) {
    let (panel, cx) = panel(cx).await;

    let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());

    panel.update_in(cx, |panel, window, cx| {
        panel.set_active(true, window, cx);
        panel.refresh_if_stale(cx);
        assert!(!panel.stale, "nothing has happened yet");
    });

    workspace
        .update_in(cx, |workspace, window, cx| {
            agent_ui::AgentView::open_tracked(
                workspace,
                project::CLAUDE_CODE_AGENT_ID,
                Default::default(),
                window,
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();

    panel.read_with(cx, |panel, _| {
        assert!(
            panel.stale,
            "a new agent tab must reach the panel without a checkout switch"
        );
    });
}

/// Opening a file must not.
///
/// The same subscription sees every item this workspace opens, and rebuilding
/// the tree on each one would undo the panel's whole rebuild discipline for
/// events that cannot change what it shows.
#[gpui::test]
async fn opening_an_ordinary_item_does_not(cx: &mut TestAppContext) {
    let (panel, cx) = panel(cx).await;

    let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());

    panel.update_in(cx, |panel, window, cx| {
        panel.set_active(true, window, cx);
        panel.refresh_if_stale(cx);
    });

    workspace
        .update_in(cx, |workspace, window, cx| {
            let editor = cx.new(|cx| editor::Editor::single_line(window, cx));
            workspace.add_item_to_active_pane(Box::new(editor), None, true, window, cx);
        })
        .unwrap();
    cx.run_until_parked();

    panel.read_with(cx, |panel, _| {
        assert!(
            !panel.stale,
            "opening a file is not news to the branch panel"
        );
    });
}

/// Restoring the expanded sections from disk must be a one-shot per repository.
///
/// It was not: the stored entries were re-applied on every rebuild, and since
/// collapsing a row *is* a rebuild, any section that was open when the panel
/// was last saved sprang straight back open. Local, the remote groups and Tags
/// -- the sections most likely to be left open -- could not be closed at all.
mod restoring_expansion {
    use std::path::PathBuf;
    use std::sync::Arc;

    use project::git_store::RepositoryId;

    use crate::branch_panel::state::StoredKey;
    use crate::branch_panel::tree::{RepoData, RowKey};

    use super::panel;
    use gpui::TestAppContext;

    const REPO_PATH: &str = "/repos/zode";

    fn repo_data(id: RepositoryId) -> RepoData {
        RepoData {
            id,
            path: Arc::from(PathBuf::from(REPO_PATH).as_path()),
            name: "zode".into(),
            current_branch: Some("develop".into()),
            branches: Vec::new(),
            worktrees: Arc::from([]),
            agents: Default::default(),
        }
    }

    #[gpui::test]
    async fn a_collapsed_section_stays_collapsed(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let id = RepositoryId(1);
        let checkout = std::sync::Arc::from(std::path::Path::new("/repos/zode/wt"));
        let key = RowKey::WorktreeAgents(id, std::sync::Arc::clone(&checkout));

        panel.update(cx, |panel, _| {
            panel.repos = vec![repo_data(id)];
            panel.stored_expanded.insert(StoredKey::WorktreeAgents(
                REPO_PATH.to_string(),
                "/repos/zode/wt".to_string(),
            ));

            panel.adopt_stored_expansion();
            assert!(
                panel.expanded.contains(&key),
                "a stored checkout must open on the first build after it is restored"
            );

            panel.expanded.remove(&key);
            panel.adopt_stored_expansion();
            assert!(
                !panel.expanded.contains(&key),
                "a card the user closed must not be reopened by the restored state"
            );
        });
    }
}
