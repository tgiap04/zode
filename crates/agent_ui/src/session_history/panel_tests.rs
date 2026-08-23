use crate::session_history::AgentHistoryPanel;
use agent_sessions::{AgentKind, SessionSummary};
use fs::FakeFs;
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext, px};
use project::Project;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};
use workspace::MultiWorkspace;

fn init_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let settings_store = settings::SettingsStore::test(cx);
        cx.set_global(settings_store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        // Before any window exists: `crate::init` registers its actions through
        // `cx.observe_new`, which only fires for workspaces created after it.
        crate::init(cx);
    });
}

fn session(id: &str, cwd: &str, title: &str, secs: u64) -> SessionSummary {
    SessionSummary {
        id: Arc::from(id),
        agent: AgentKind::Claude,
        title: title.to_string(),
        preview: format!("Agent: {title} finished"),
        preview_speaker: Some(agent_sessions::Speaker::Agent),
        cwd: PathBuf::from(cwd),
        branch: Some("main".into()),
        model: Some("claude-opus-5".into()),
        updated_at: UNIX_EPOCH + Duration::from_secs(secs),
        log_path: Some(PathBuf::from("/nowhere/log.jsonl")),
        log_bytes: 1024,
    }
}

/// Rows drawn on a real frame, with real height.
///
/// This is the test that earns its keep: `uniform_list` has no intrinsic height
/// and `div()` lays out as a row, so a list under the wrong parent draws **zero
/// rows and does not panic**. Nothing but measuring a painted frame catches that
/// — `cx.draw()` publishes none, so the panel is docked and the window is left to
/// draw itself.
#[gpui::test]
async fn the_panel_draws_its_rows(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let project = Project::test(fs.clone(), ["/root".as_ref()], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window.into(), cx);

    let panel: Entity<AgentHistoryPanel> = workspace.update_in(cx, |workspace, window, cx| {
        cx.new(|cx| AgentHistoryPanel::new(workspace, window, cx))
    });
    // No providers: a test must never read the developer's own ~/.claude. The
    // sessions are injected below instead, once the panel's own load has run and
    // found nothing.
    panel.update(cx, |panel, _| panel.providers.clear());
    workspace.update_in(cx, |workspace, window, cx| {
        workspace.add_panel(panel.clone(), window, cx);
        workspace.right_dock().update(cx, |dock, cx| {
            dock.set_open(true, window, cx);
        });
        workspace.toggle_panel_focus::<AgentHistoryPanel>(window, cx);
    });
    cx.run_until_parked();

    // Sessions from this project, plus one from somewhere else that must not show.
    panel.update(cx, |panel, cx| {
        panel.sessions = vec![
            session("one", "/root", "Newest here", 300),
            session("two", "/root", "Older here", 200),
            session("elsewhere", "/other", "Another project", 100),
        ];
        cx.notify();
    });
    cx.run_until_parked();

    let panel_bounds = cx
        .debug_bounds("agent-history-panel")
        .expect("the panel must be drawn once its dock is open");
    assert!(
        panel_bounds.size.height > px(0.) && panel_bounds.size.width > px(0.),
        "the panel drew with no area: {panel_bounds:?}"
    );

    // The list itself, full height under a column parent: `flex_1` under a row
    // parent would resolve to zero here and draw nothing.
    let list = cx
        .debug_bounds("agent-history-list")
        .expect("the list must be drawn");
    assert!(
        list.size.height > px(100.),
        "the list must take the panel's height, got {list:?}"
    );

    // Row 0 is the group header for `/root`; rows 1 and 2 are its two sessions.
    let first = cx
        .debug_bounds("agent-history-row:1")
        .expect("the first session must be drawn — a zero-row uniform_list is silent");
    let second = cx
        .debug_bounds("agent-history-row:2")
        .expect("and so must the second");
    for (which, bounds) in [("first", first), ("second", second)] {
        assert!(
            bounds.size.height > px(0.),
            "the {which} row drew with no height: {bounds:?}"
        );
    }
    assert!(
        first.bottom() <= second.origin.y,
        "rows must read top to bottom without overlapping, got {first:?} then {second:?}"
    );
    assert!(
        cx.debug_bounds("agent-history-row:3").is_none(),
        "the session from another project must not be drawn: this panel is scoped \
         to the project its workspace has open"
    );
}

/// A left click on the ellipsis opens the menu — and does **not** expand the row.
///
/// `right_click_menu` there made the button silent on a left click: with no
/// `on_click`, `ButtonLike` never calls `stop_propagation`, so the click bubbled
/// to the row's own handler and toggled its expansion. Both halves are asserted,
/// because the second is what the defect actually looked like from the outside.
#[gpui::test]
async fn the_ellipsis_opens_its_menu_instead_of_expanding_the_row(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let project = Project::test(fs.clone(), ["/root".as_ref()], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window.into(), cx);

    let panel: Entity<AgentHistoryPanel> = workspace.update_in(cx, |workspace, window, cx| {
        cx.new(|cx| AgentHistoryPanel::new(workspace, window, cx))
    });
    panel.update(cx, |panel, _| panel.providers.clear());
    workspace.update_in(cx, |workspace, window, cx| {
        workspace.add_panel(panel.clone(), window, cx);
        workspace.right_dock().update(cx, |dock, cx| {
            dock.set_open(true, window, cx);
        });
        workspace.toggle_panel_focus::<AgentHistoryPanel>(window, cx);
    });
    cx.run_until_parked();
    panel.update(cx, |panel, cx| {
        panel.sessions = vec![session("one", "/root", "Newest here", 300)];
        cx.notify();
    });
    cx.run_until_parked();

    let ellipsis = cx
        .debug_bounds("agent-history-menu:1")
        .expect("row 1's ellipsis must be drawn");
    cx.simulate_click(ellipsis.center(), gpui::Modifiers::default());
    cx.run_until_parked();

    // The menu items carry `MENU_ITEM-{label}` probes of their own, so this is the
    // dropdown really being on screen rather than a proxy for it.
    assert!(
        cx.debug_bounds("MENU_ITEM-Delete").is_some(),
        "a left click on the ellipsis must open the menu"
    );
    assert!(
        panel.read_with(cx, |panel, _| panel.expanded_rows.is_empty()),
        "and must not fall through to the row, which would expand it instead"
    );
}

/// The other half of the scoping claim: with nothing open, nothing is listed.
#[gpui::test]
async fn a_workspace_with_no_worktree_lists_nothing(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window.into(), cx);

    let panel: Entity<AgentHistoryPanel> = workspace.update_in(cx, |workspace, window, cx| {
        cx.new(|cx| AgentHistoryPanel::new(workspace, window, cx))
    });
    panel.update(cx, |panel, _| panel.providers.clear());
    workspace.update_in(cx, |workspace, window, cx| {
        workspace.add_panel(panel.clone(), window, cx);
        workspace.right_dock().update(cx, |dock, cx| {
            dock.set_open(true, window, cx);
        });
        workspace.toggle_panel_focus::<AgentHistoryPanel>(window, cx);
    });
    cx.run_until_parked();
    panel.update(cx, |panel, cx| {
        panel.sessions = vec![session("elsewhere", "/other", "Another project", 100)];
        cx.notify();
    });
    cx.run_until_parked();

    assert!(cx.debug_bounds("agent-history-panel").is_some());
    assert!(
        cx.debug_bounds("agent-history-row:0").is_none(),
        "a workspace with no worktree has no project to show history for"
    );
}

/// The header button dispatches `agent::ToggleHistory`, and a handler registered
/// with `register_action` runs while the workspace is leased. Reaching back
/// through a workspace *handle* from there aborts the process — a trap this tree
/// has paid for twice — so the action is dispatched for real rather than the
/// method being called directly.
#[gpui::test]
async fn the_toggle_action_shows_the_panel_without_aborting(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let project = Project::test(fs.clone(), ["/root".as_ref()], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window.into(), cx);

    let panel: Entity<AgentHistoryPanel> = workspace.update_in(cx, |workspace, window, cx| {
        cx.new(|cx| AgentHistoryPanel::new(workspace, window, cx))
    });
    panel.update(cx, |panel, _| panel.providers.clear());
    workspace.update_in(cx, |workspace, window, cx| {
        // A second panel in the same dock, showing, so the turn-taking claim below
        // has something to take a turn with.
        let other = cx.new(|cx| {
            workspace::dock::test::TestPanel::new(workspace::dock::DockPosition::Right, 0, cx)
        });
        workspace.add_panel(other, window, cx);
        workspace.add_panel(panel.clone(), window, cx);
        // The other panel showing, so "took the column" is a claim with teeth:
        // without this the dock draws one panel either way and the assertion
        // below would pass for stacking too.
        workspace.right_dock().update(cx, |dock, cx| {
            dock.activate_panel(0, window, cx);
            dock.set_open(true, window, cx);
        });
    });
    cx.run_until_parked();

    let showing_other = workspace.read_with(cx, |workspace, cx| {
        workspace
            .right_dock()
            .read(cx)
            .visible_panel()
            .map(|panel| panel.persistent_name().to_string())
    });
    assert_eq!(
        showing_other.as_deref(),
        Some("TestPanel"),
        "the other panel is the one up before the toggle"
    );

    cx.dispatch_action(zed_actions::agent::ToggleHistory);
    cx.run_until_parked();

    assert!(
        workspace.read_with(cx, |workspace, cx| workspace
            .right_dock()
            .read(cx)
            .is_open()),
        "dispatching the toggle must open the dock it lives in"
    );
    assert!(cx.debug_bounds("agent-history-panel").is_some());

    // And it takes the column turn by turn rather than stacking: the other panel
    // in this dock stops being drawn. `toggle_panel_focus` would have stacked
    // them, splitting the column's height between two vertical lists.
    let (visible, names) = workspace.read_with(cx, |workspace, cx| {
        let dock = workspace.right_dock().read(cx);
        (
            dock.visible_panels().count(),
            dock.visible_panels()
                .map(|panel| panel.persistent_name().to_string())
                .collect::<Vec<_>>(),
        )
    });
    assert_eq!(
        (visible, names),
        (1, vec!["Agent History".to_string()]),
        "the history must have taken the column, not joined the other panel in it"
    );
}
