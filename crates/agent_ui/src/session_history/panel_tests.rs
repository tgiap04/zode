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
        let sessions = vec![
            session("one", "/root", "Newest here", 300),
            session("two", "/root", "Older here", 200),
            session("elsewhere", "/other", "Another project", 100),
        ];
        panel
            .store
            .update(cx, |store, cx| store.set_index_for_test(sessions, cx));
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

    // Three levels now: row 0 is the agent header, row 1 the `/root` project
    // header under it, and rows 2 and 3 its two sessions.
    let first = cx
        .debug_bounds("agent-history-row:2")
        .expect("the first session must be drawn — a zero-row uniform_list is silent");
    let second = cx
        .debug_bounds("agent-history-row:3")
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
        cx.debug_bounds("agent-history-row:4").is_none(),
        "four rows and no more — one agent header, one project header, two \
         sessions. The session from another project must not be drawn: this panel \
         is scoped to the project its workspace has open"
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
        let sessions = vec![session("one", "/root", "Newest here", 300)];
        panel
            .store
            .update(cx, |store, cx| store.set_index_for_test(sessions, cx));
        cx.notify();
    });
    cx.run_until_parked();

    // Row 0 is the agent header, row 1 the project header, so the single session
    // is row 2.
    let ellipsis = cx
        .debug_bounds("agent-history-menu:2")
        .expect("the session row's ellipsis must be drawn");
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
        let sessions = vec![session("elsewhere", "/other", "Another project", 100)];
        panel
            .store
            .update(cx, |store, cx| store.set_index_for_test(sessions, cx));
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

/// A provider the test owns outright.
///
/// The other tests in this file *clear* `panel.providers` so nothing reads the
/// developer's real `~/.claude`. A delete has to resolve a provider to learn what
/// to trash, so these replace it instead: `paths_to_trash` hands back exactly the
/// paths the test put into `FakeFs`, and nothing else here touches a disk.
struct TestProvider {
    paths: collections::HashMap<Arc<str>, Vec<PathBuf>>,
}

impl agent_sessions::SessionProvider for TestProvider {
    fn agent(&self) -> AgentKind {
        AgentKind::Claude
    }

    fn availability(&self) -> agent_sessions::Availability {
        agent_sessions::Availability::Ready
    }

    fn list(&self) -> anyhow::Result<Vec<SessionSummary>> {
        Ok(Vec::new())
    }

    fn find(&self, _id: &str) -> anyhow::Result<Option<SessionSummary>> {
        Ok(None)
    }

    fn new_session_command(
        &self,
        _id: &str,
        _cwd: &std::path::Path,
    ) -> Option<agent_sessions::ResumeCommand> {
        None
    }

    fn counts(&self, _session: &SessionSummary) -> anyhow::Result<agent_sessions::SessionCounts> {
        Ok(agent_sessions::SessionCounts::default())
    }

    fn resume_command(
        &self,
        _session: &SessionSummary,
        _fork: agent_sessions::Fork,
    ) -> Option<agent_sessions::ResumeCommand> {
        None
    }

    fn paths_to_trash(&self, session: &SessionSummary) -> Vec<PathBuf> {
        self.paths.get(&session.id).cloned().unwrap_or_default()
    }
}

/// Opens a workspace with the history panel docked, focused and drawn.
///
/// `roots` empty models a window with no folder open. `trash_paths` becomes the
/// test provider's answer to `paths_to_trash`.
async fn panel_with(
    roots: &[&str],
    sessions: Vec<SessionSummary>,
    trash_paths: Vec<(&str, Vec<PathBuf>)>,
    fs: Arc<FakeFs>,
    cx: &mut TestAppContext,
) -> (Entity<AgentHistoryPanel>, VisualTestContext) {
    let root_refs: Vec<&std::path::Path> = roots.iter().map(std::path::Path::new).collect();
    let project = Project::test(fs.clone(), root_refs, cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);

    let panel: Entity<AgentHistoryPanel> = workspace.update_in(&mut cx, |workspace, window, cx| {
        cx.new(|cx| AgentHistoryPanel::new(workspace, window, cx))
    });
    panel.update(&mut cx, |panel, _| {
        panel.providers = vec![Arc::new(TestProvider {
            paths: trash_paths
                .into_iter()
                .map(|(id, paths)| (Arc::from(id), paths))
                .collect(),
        })];
    });
    workspace.update_in(&mut cx, |workspace, window, cx| {
        workspace.add_panel(panel.clone(), window, cx);
        workspace.right_dock().update(cx, |dock, cx| {
            dock.set_open(true, window, cx);
        });
        workspace.toggle_panel_focus::<AgentHistoryPanel>(window, cx);
    });
    cx.run_until_parked();

    panel.update(&mut cx, |panel, cx| {
        panel
            .store
            .update(cx, |store, cx| store.set_index_for_test(sessions, cx));
        cx.notify();
    });
    cx.run_until_parked();

    (panel, cx)
}

fn click_delete_all(cx: &mut VisualTestContext) {
    let button = cx
        .debug_bounds("agent-history-delete-all")
        .expect("the delete-all button must be drawn in the header");
    cx.simulate_click(button.center(), gpui::Modifiers::default());
    cx.run_until_parked();
}

fn remaining_ids(panel: &Entity<AgentHistoryPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    panel.read_with(cx, |panel, cx| {
        panel
            .store
            .read(cx)
            .index()
            .sessions()
            .iter()
            .map(|session| session.id.to_string())
            .collect()
    })
}

fn trashed_names(fs: &Arc<FakeFs>) -> Vec<String> {
    let mut names: Vec<String> = fs
        .trash_entries()
        .into_iter()
        .map(|entry| entry.name.to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The scope test. Every session of this project goes; another project's stays.
#[gpui::test]
async fn deleting_all_takes_this_projects_sessions_and_no_others(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/logs",
        json!({ "one.jsonl": "", "two.jsonl": "", "elsewhere.jsonl": "" }),
    )
    .await;
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;

    let (panel, mut cx) = panel_with(
        &["/root"],
        vec![
            session("one", "/root", "First", 300),
            session("two", "/root", "Second", 200),
            session("elsewhere", "/other", "Another project", 100),
        ],
        vec![
            ("one", vec![PathBuf::from("/logs/one.jsonl")]),
            ("two", vec![PathBuf::from("/logs/two.jsonl")]),
            ("elsewhere", vec![PathBuf::from("/logs/elsewhere.jsonl")]),
        ],
        fs.clone(),
        cx,
    )
    .await;

    click_delete_all(&mut cx);

    let prompt = cx.pending_prompt().expect("a delete must confirm first");
    assert_eq!(prompt.0, "Delete all history for this project?");
    assert!(
        prompt.1.contains("2 sessions"),
        "the count must exclude the other project's session, got: {}",
        prompt.1
    );

    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();

    assert_eq!(
        trashed_names(&fs),
        vec!["one.jsonl".to_string(), "two.jsonl".to_string()],
        "only this project's transcripts may be taken"
    );
    assert_eq!(
        remaining_ids(&panel, &mut cx),
        vec!["elsewhere".to_string()],
        "the other project's session must survive in the shared index"
    );
}

/// The second scope test: a filter narrows the list, never the delete.
#[gpui::test]
async fn the_search_filter_does_not_narrow_the_delete(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/logs",
        json!({ "a.jsonl": "", "b.jsonl": "", "c.jsonl": "" }),
    )
    .await;
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;

    let (panel, mut cx) = panel_with(
        &["/root"],
        vec![
            session("a", "/root", "Keeper", 300),
            session("b", "/root", "Something else", 200),
            session("c", "/root", "Third thing", 100),
        ],
        vec![
            ("a", vec![PathBuf::from("/logs/a.jsonl")]),
            ("b", vec![PathBuf::from("/logs/b.jsonl")]),
            ("c", vec![PathBuf::from("/logs/c.jsonl")]),
        ],
        fs.clone(),
        cx,
    )
    .await;

    // A query that leaves only one row on screen.
    panel.update_in(&mut cx, |panel, window, cx| {
        panel.filter_editor.update(cx, |editor, cx| {
            editor.set_text("Keeper", window, cx);
        });
    });
    cx.run_until_parked();

    // The filter has to have really bitten, or the assertion below proves
    // nothing: row 0 is the agent header, row 1 the project header, row 2 the
    // single match, and there must be no row 3.
    assert!(
        cx.debug_bounds("agent-history-row:2").is_some(),
        "the matching session must still be drawn"
    );
    assert!(
        cx.debug_bounds("agent-history-row:3").is_none(),
        "the filter must have narrowed the list to one session"
    );

    click_delete_all(&mut cx);
    let prompt = cx.pending_prompt().expect("a delete must confirm first");
    assert!(
        prompt.1.contains("3 sessions"),
        "the filter shows one row but all three belong to the project, got: {}",
        prompt.1
    );

    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();

    assert_eq!(trashed_names(&fs).len(), 3);
    assert!(
        remaining_ids(&panel, &mut cx).is_empty(),
        "every session of the project goes, whatever the search box says"
    );
}

#[gpui::test]
async fn cancelling_changes_nothing(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/logs", json!({ "one.jsonl": "" })).await;
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;

    let (panel, mut cx) = panel_with(
        &["/root"],
        vec![session("one", "/root", "First", 300)],
        vec![("one", vec![PathBuf::from("/logs/one.jsonl")])],
        fs.clone(),
        cx,
    )
    .await;

    click_delete_all(&mut cx);
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();

    assert!(
        fs.trash_entries().is_empty(),
        "cancelling must not touch the disk"
    );
    assert_eq!(remaining_ids(&panel, &mut cx), vec!["one".to_string()]);
}

/// With no folder open there is no project, so the control cannot act. Disabled
/// is not observable from bounds alone -- that a click raises no prompt is the
/// behaviour that actually matters.
#[gpui::test]
async fn with_no_project_open_the_button_cannot_prompt(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/logs", json!({ "elsewhere.jsonl": "" }))
        .await;

    let (_panel, mut cx) = panel_with(
        &[],
        vec![session("elsewhere", "/other", "Another project", 100)],
        vec![("elsewhere", vec![PathBuf::from("/logs/elsewhere.jsonl")])],
        fs.clone(),
        cx,
    )
    .await;

    click_delete_all(&mut cx);

    assert!(
        !cx.has_pending_prompt(),
        "a window with no folder open has no project to delete the history of"
    );
    assert!(fs.trash_entries().is_empty());
}

/// The *other* empty rule, and the one the render gate cannot cover.
///
/// The project has sessions, so the button is enabled and `delete_all` really
/// runs -- but no provider offers a path for any of them, so there is nothing
/// to take. It must say so rather than do nothing: a button that neither acts
/// nor explains reads as broken, which is exactly how this was reported.
/// `with_no_project_open_the_button_cannot_prompt` proves nothing here: a
/// disabled `IconButton` drops its `on_click` entirely, so that test never
/// enters the method at all.
#[gpui::test]
async fn a_project_whose_sessions_own_no_files_says_so(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;

    let (panel, mut cx) = panel_with(
        &["/root"],
        vec![
            session("one", "/root", "First", 300),
            session("two", "/root", "Second", 200),
        ],
        // Enabled by the render gate -- the project has sessions -- but the
        // provider hands back nothing to trash for either of them.
        vec![("one", Vec::new()), ("two", Vec::new())],
        fs.clone(),
        cx,
    )
    .await;

    click_delete_all(&mut cx);

    let prompt = cx
        .pending_prompt()
        .expect("an empty delete must explain itself, not fall silent");
    assert_eq!(prompt.0, "Nothing left to delete");
    assert!(
        !prompt.1.contains("will move to the trash"),
        "it must not read like a delete about to happen, got: {}",
        prompt.1
    );
    cx.simulate_prompt_answer("Ok");
    cx.run_until_parked();

    assert!(fs.trash_entries().is_empty());
    assert_eq!(
        remaining_ids(&panel, &mut cx),
        vec!["one".to_string(), "two".to_string()],
        "saying there is nothing to take must not take anything"
    );
}

/// A single session with nothing left on disk offers the one thing still on
/// the table: taking the row off the list.
///
/// This is the reported bug's own shape. A Copilot session written by the VS
/// Code extension used to reach here and get silence; now the store can name
/// its directory, so the route is reserved for a session whose files really
/// are gone -- and that route has to answer.
#[gpui::test]
async fn a_session_with_nothing_on_disk_offers_to_drop_the_row(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;

    // `delete_session` resolves the *real* provider, not the panel's test one:
    // it is a free function so the sidebar can call it without a panel. So the
    // session is shaped to make the real provider come back empty -- Claude
    // builds both of its paths from `log_path`, and this one has none, which
    // is precisely the state a transcript deleted outside the editor leaves.
    let gone = SessionSummary {
        log_path: None,
        log_bytes: 0,
        ..session("gone", "/root", "Nothing left of this one", 300)
    };
    let (panel, mut cx) = panel_with(
        &["/root"],
        vec![gone, session("kept", "/root", "Still here", 200)],
        Vec::new(),
        fs.clone(),
        cx,
    )
    .await;

    let target = panel.read_with(&mut cx, |panel, cx| {
        panel
            .sessions(cx)
            .iter()
            .find(|session| session.id.as_ref() == "gone")
            .expect("the session is in the index")
            .clone()
    });
    panel.update_in(&mut cx, |panel, window, cx| {
        panel.delete(&target, window, cx);
    });
    cx.run_until_parked();

    let prompt = cx
        .pending_prompt()
        .expect("a delete with nothing to take must still answer");
    assert_eq!(prompt.0, "Nothing left to delete");
    assert!(
        prompt.1.contains("own store"),
        "it must say the agent's own store is untouched, got: {}",
        prompt.1
    );

    cx.simulate_prompt_answer("Remove From List");
    cx.run_until_parked();

    assert!(
        fs.trash_entries().is_empty(),
        "dropping a row must never reach the disk"
    );
    assert_eq!(
        remaining_ids(&panel, &mut cx),
        vec!["kept".to_string()],
        "and the row the user asked about is the only one that goes"
    );
}

/// The partial-failure rule. `FakeFs::trash` errors on a path it never held, so
/// the second session fails without any injection machinery.
#[gpui::test]
async fn a_session_whose_files_fail_to_trash_stays_listed(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/logs", json!({ "good.jsonl": "" })).await;
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;

    let (panel, mut cx) = panel_with(
        &["/root"],
        vec![
            session("good", "/root", "Trashes cleanly", 300),
            session("bad", "/root", "Never existed on disk", 200),
        ],
        vec![
            ("good", vec![PathBuf::from("/logs/good.jsonl")]),
            ("bad", vec![PathBuf::from("/logs/missing.jsonl")]),
        ],
        fs.clone(),
        cx,
    )
    .await;

    click_delete_all(&mut cx);
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();

    assert_eq!(trashed_names(&fs), vec!["good.jsonl".to_string()]);
    assert_eq!(
        remaining_ids(&panel, &mut cx),
        vec!["bad".to_string()],
        "a session whose files did not all reach the trash must keep describing the disk"
    );
}
