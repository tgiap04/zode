//! Scaffolding shared by the tests that drive the rail's waiting-agents count
//! and the badge it draws.

use crate::Sidebar;
use crate::sidebar_tests::init_test;
use agent_ui::{AgentView, SessionIntent, SessionOrigin};
use fs::FakeFs;
use gpui::{AppContext as _, Context, Entity, TestAppContext, VisualTestContext, Window};
use project::Project;
use serde_json::json;
use workspace::{MultiWorkspace, Workspace};
use zed_actions::agent::AgentViewMode;

pub(crate) struct TwoProjects {
    pub(crate) multi_workspace: Entity<MultiWorkspace>,
    pub(crate) sidebar: Entity<Sidebar>,
    /// The workspace of `/root_a`, which `/root_b` replaces as the active one.
    pub(crate) workspace_a: Entity<Workspace>,
    pub(crate) project_a: Entity<Project>,
}

/// A window with `/root_a` and `/root_b` on the rail, `/root_b` in front, and
/// the rail drawn.
pub(crate) async fn two_projects(cx: &mut TestAppContext) -> (TwoProjects, &mut VisualTestContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "b.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs, ["/root_b".as_ref()], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a.clone(), window, cx));
    let sidebar = multi_workspace.update_in(cx, |mw, window, cx| {
        let mw_entity = cx.entity();
        let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
        mw.register_sidebar(sidebar.clone(), cx);
        sidebar
    });
    multi_workspace.update(cx, |mw, cx| mw.test_enable_background_retention(cx));
    let workspace_a = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b, window, cx);
    });
    redraw(cx);
    (
        TwoProjects {
            multi_workspace,
            sidebar,
            workspace_a,
            project_a,
        },
        cx,
    )
}

pub(crate) fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

/// A Claude tab that reports turns, built but not yet placed anywhere.
pub(crate) fn new_claude_agent_view(
    workspace: &Workspace,
    project: &Entity<Project>,
    cx: &mut Context<Workspace>,
) -> Entity<AgentView> {
    let handle = workspace.weak_handle();
    let project = project.clone();
    cx.new(|cx| {
        AgentView::test_new(
            project::AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
            AgentViewMode::Terminal,
            project,
            handle,
            SessionOrigin::new(SessionIntent::Tracked("session".into()), None),
            cx,
        )
    })
}

/// Puts a Claude tab, watching its own attention, in the workspace's active
/// pane without focusing it.
pub(crate) fn add_waiting_claude_tab(
    workspace: &Entity<Workspace>,
    project: &Entity<Project>,
    cx: &mut VisualTestContext,
) -> Entity<AgentView> {
    let view = workspace.update_in(cx, |workspace, window: &mut Window, cx| {
        let view = new_claude_agent_view(workspace, project, cx);
        view.update(cx, |view, cx| view.watch_attention_for_tests(window, cx));
        workspace.add_item_to_active_pane(Box::new(view.clone()), None, false, window, cx);
        view
    });
    cx.run_until_parked();
    view
}

pub(crate) fn waiting(sidebar: &Entity<Sidebar>, label: &str, cx: &mut VisualTestContext) -> usize {
    sidebar.read_with(cx, |sidebar, _| {
        sidebar
            .contents
            .rail_entries
            .iter()
            .find(|entry| entry.label.as_ref() == label)
            .map(|entry| entry.waiting_agents)
            .expect("the project is on the rail")
    })
}

/// `debug_bounds` takes a `'static` selector; leaking a handful of short
/// strings in a test process is the price of a per-row one.
pub(crate) fn badge_selector(ix: usize) -> &'static str {
    Box::leak(format!("project-rail-item-waiting-badge:{ix}").into_boxed_str())
}
