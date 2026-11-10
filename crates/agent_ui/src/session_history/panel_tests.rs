use crate::session_history::AgentHistoryPanel;
use agent_sessions::{AgentKind, Deletion, SessionSummary};
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
    // The trigger is only painted while the pointer is over its row, so a click
    // that did not first arrive there would reach nothing -- as it would for a
    // person. The move is the arrival.
    cx.simulate_mouse_move(ellipsis.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
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
    // Opening the menu takes the pointer off the row, which hides the trigger
    // unless the row knows its menu is open. That knowledge is the handle.
    assert!(
        panel.update(cx, |panel, _| panel
            .menu_handle(&Arc::from("one"))
            .is_deployed()),
        "the row must know its menu is open, or its trigger vanishes under it"
    );
}

/// A row's menu handle lives in the panel and stores the closure that builds
/// the menu, so anything that closure captures strongly -- including the panel
/// itself -- is kept alive by the panel for as long as the panel lives.
///
/// The menu is opened here as well as drawn: the built `ContextMenu` is a second
/// owner of the entry handlers, and the handle holds it too. The panel is taken
/// out of the workspace rather than the window closed, because the dock outlives
/// its window in this harness and would hold the panel regardless.
#[gpui::test]
async fn a_panel_with_an_open_row_menu_is_released_when_removed(cx: &mut TestAppContext) {
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

    let ellipsis = cx
        .debug_bounds("agent-history-menu:2")
        .expect("the session row's ellipsis must be drawn");
    cx.simulate_mouse_move(ellipsis.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_click(ellipsis.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("MENU_ITEM-Delete").is_some(),
        "the menu must be open, or this test says nothing about what it holds"
    );

    let weak_panel = panel.downgrade();
    workspace.update_in(cx, |workspace, window, cx| {
        workspace.remove_panel(&panel, window, cx);
    });
    drop(panel);
    cx.run_until_parked();
    // One more effect cycle: a dropped entity's resources are kept until the end
    // of the cycle it was dropped in.
    cx.update(|_, _| {});

    weak_panel.assert_released();
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
/// developer's real `~/.claude`. A delete has to resolve a provider to learn
/// what to take, so these replace it instead: `deletion` hands back exactly
/// what the test put into `deletions`, and nothing else here touches a disk.
///
/// `still_present` answers `find` -- default empty, so every id reads as
/// "gone", which is what a `Command` delete's idempotency re-check (C2/H9)
/// wants for the common case. A test proving the *other* half -- a real
/// failure, where the store still holds the session -- adds its id here.
struct TestProvider {
    deletions: collections::HashMap<Arc<str>, Deletion>,
    still_present: collections::HashSet<Arc<str>>,
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

    fn find(&self, id: &str) -> anyhow::Result<Option<SessionSummary>> {
        if self.still_present.contains(id) {
            return Ok(Some(session(id, "/root", "still present", 0)));
        }
        Ok(None)
    }

    fn new_session_command(
        &self,
        _id: &str,
        _cwd: &std::path::Path,
    ) -> Option<agent_sessions::AgentCommand> {
        None
    }

    fn counts(&self, _session: &SessionSummary) -> anyhow::Result<agent_sessions::SessionCounts> {
        Ok(agent_sessions::SessionCounts::default())
    }

    fn resume_command(
        &self,
        _session: &SessionSummary,
        _fork: agent_sessions::Fork,
    ) -> Option<agent_sessions::AgentCommand> {
        None
    }

    fn deletion(&self, session: &SessionSummary) -> Deletion {
        self.deletions
            .get(&session.id)
            .cloned()
            .unwrap_or(Deletion::Nothing)
    }
}

/// Opens a workspace with the history panel docked, focused and drawn.
///
/// `roots` empty models a window with no folder open. `trash_paths` becomes
/// the test provider's answer to `deletion` -- a non-empty vec becomes
/// `Deletion::Trash`, an empty one `Deletion::Nothing`, matching what
/// `paths_to_trash` used to mean before this returned a `Deletion`.
async fn panel_with(
    roots: &[&str],
    sessions: Vec<SessionSummary>,
    trash_paths: Vec<(&str, Vec<PathBuf>)>,
    fs: Arc<FakeFs>,
    cx: &mut TestAppContext,
) -> (Entity<AgentHistoryPanel>, VisualTestContext) {
    let deletions = trash_paths
        .into_iter()
        .map(|(id, paths)| {
            let deletion = if paths.is_empty() {
                Deletion::Nothing
            } else {
                Deletion::Trash(paths)
            };
            (Arc::from(id), deletion)
        })
        .collect();
    panel_with_provider(
        roots,
        sessions,
        TestProvider {
            deletions,
            still_present: collections::HashSet::default(),
        },
        fs,
        cx,
    )
    .await
}

/// The general form of [`panel_with`], for a test that needs a `TestProvider`
/// shaped by hand -- a `Deletion::Command` target, or a `find` answer other
/// than "gone" (C2/H9's real-failure case).
async fn panel_with_provider(
    roots: &[&str],
    sessions: Vec<SessionSummary>,
    provider: TestProvider,
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
        panel.providers = vec![Arc::new(provider)];
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

/// A Claude session only has a sidecar directory when it ran subagents, yet
/// its deletion always lists one. A path that is already absent counts as
/// gone, so the session must not stay listed.
#[gpui::test]
async fn deleting_all_forgets_a_session_whose_sidecar_directory_never_existed(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/logs", json!({ "one.jsonl": "" })).await;
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;

    let (panel, mut cx) = panel_with(
        &["/root"],
        vec![session("one", "/root", "First", 300)],
        vec![(
            "one",
            vec![PathBuf::from("/logs/one.jsonl"), PathBuf::from("/logs/one")],
        )],
        fs.clone(),
        cx,
    )
    .await;

    click_delete_all(&mut cx);
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();

    assert_eq!(trashed_names(&fs), vec!["one.jsonl".to_string()]);
    assert!(
        remaining_ids(&panel, &mut cx).is_empty(),
        "an absent sidecar directory must not keep the row listed"
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

/// The partial-failure rule. `FakeFs::trash` errors on a path whose parent
/// directory it never held, even when a missing path is otherwise tolerated, so
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
            ("bad", vec![PathBuf::from("/unmounted/missing.jsonl")]),
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

/// H7 for the single delete: `delete_session`'s `Trash` arm must keep the same
/// discipline the bulk sweep already had -- a row is forgotten only once
/// everything it named is really gone, not unconditionally once the attempt
/// is over.
///
/// `delete_session` resolves the *real* Claude provider (see the comment on
/// `a_session_with_nothing_on_disk_offers_to_drop_the_row`), and its `Trash`
/// arm checks `path.exists()` against the real OS before ever reaching the
/// injected `Fs`, which is longstanding behaviour here. So the path
/// this test fails to trash has to be a real file: written straight to the
/// host's temp directory, never registered with `FakeFs`, and cleaned up
/// after.
#[gpui::test]
async fn a_single_delete_whose_trash_fails_keeps_the_row(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;

    let real_log = std::env::temp_dir().join(format!("zode-h7-test-{}.jsonl", std::process::id()));
    std::fs::write(&real_log, "").expect("writing the real fixture file must succeed");

    let target = SessionSummary {
        log_path: Some(real_log.clone()),
        log_bytes: 10,
        ..session("bad", "/root", "Never reaches the trash", 300)
    };
    let (panel, mut cx) =
        panel_with(&["/root"], vec![target.clone()], Vec::new(), fs.clone(), cx).await;

    panel.update_in(&mut cx, |panel, window, cx| {
        panel.delete(&target, window, cx);
    });
    cx.run_until_parked();

    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();

    // Best-effort cleanup of the real fixture file; its outcome does not bear
    // on what this test is proving.
    std::fs::remove_file(&real_log).ok();

    assert!(
        fs.trash_entries().is_empty(),
        "a real file `FakeFs` never held cannot reach its trash"
    );
    assert_eq!(
        remaining_ids(&panel, &mut cx),
        vec!["bad".to_string()],
        "a failed trash must keep the row (H7), matching delete_all's own discipline"
    );
}

/// The single delete's happy path with the store idle: the row leaves the
/// panel with no manual refresh.
///
/// The transcript is a real file (see the comment on
/// `a_single_delete_whose_trash_fails_keeps_the_row`) that is also registered
/// with `FakeFs`, so the trash call itself succeeds.
#[gpui::test]
async fn a_single_delete_that_trashes_cleanly_drops_the_row(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;

    let real_log =
        std::env::temp_dir().join(format!("zode-single-ok-{}.jsonl", std::process::id()));
    std::fs::write(&real_log, "").expect("writing the real fixture file must succeed");
    if let Some(parent) = real_log.parent() {
        fs.insert_tree(parent, json!({})).await;
    }
    fs.insert_file(&real_log, Vec::new()).await;

    let target = SessionSummary {
        log_path: Some(real_log.clone()),
        log_bytes: 10,
        ..session("gone", "/root", "Trashes cleanly", 300)
    };
    let (panel, mut cx) = panel_with(
        &["/root"],
        vec![target.clone(), session("kept", "/root", "Still here", 200)],
        Vec::new(),
        fs.clone(),
        cx,
    )
    .await;

    panel.update_in(&mut cx, |panel, window, cx| {
        panel.delete(&target, window, cx);
    });
    cx.run_until_parked();
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();

    std::fs::remove_file(&real_log).ok();

    assert_eq!(
        trashed_names(&fs).len(),
        1,
        "the transcript must reach the trash"
    );
    assert_eq!(
        remaining_ids(&panel, &mut cx),
        vec!["kept".to_string()],
        "a clean single delete must drop the row without a refresh"
    );
}

/// C1: a bulk plan mixing a recoverable and an irreversible session must warn
/// about both, name them separately, and offer "Delete" rather than "Move to
/// Trash". Walked by hand once, per the phase's own success criterion.
///
/// The irreversible target's CLI resolves to `Missing` here -- `resolve_agent_binary`
/// always answers that way under `cfg!(test)` unless the project environment
/// carries a `PATH` -- which is itself a real, correct outcome for this sweep:
/// the row must stay, not vanish, while the session is still in its own store.
#[gpui::test]
async fn a_mixed_bulk_plan_warns_it_is_not_fully_recoverable(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/logs", json!({ "trashed.jsonl": "" }))
        .await;
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;

    let mut deletions = collections::HashMap::default();
    deletions.insert(
        Arc::from("trashed"),
        Deletion::Trash(vec![PathBuf::from("/logs/trashed.jsonl")]),
    );
    deletions.insert(
        Arc::from("commanded"),
        Deletion::Command(agent_sessions::AgentCommand {
            program: "opencode".into(),
            args: vec![
                "session".to_string(),
                "delete".to_string(),
                "commanded".to_string(),
            ],
            cwd: PathBuf::from("/root"),
        }),
    );
    let provider = TestProvider {
        deletions,
        still_present: collections::HashSet::default(),
    };

    let (panel, mut cx) = panel_with_provider(
        &["/root"],
        vec![
            session("trashed", "/root", "Recoverable", 300),
            // A real `Command` session always carries `log_bytes: 0` -- there
            // is no file here for this editor to have measured -- so the
            // fixture states that explicitly rather than inheriting the
            // helper's default, which would silently inflate the trash byte
            // total below with a number nobody measured.
            SessionSummary {
                log_bytes: 0,
                ..session("commanded", "/root", "Not recoverable", 200)
            },
        ],
        provider,
        fs.clone(),
        cx,
    )
    .await;

    click_delete_all(&mut cx);

    let prompt = cx
        .pending_prompt()
        .expect("a mixed plan must confirm first");
    assert_eq!(
        prompt.0, "Delete all history for this project?",
        "the dialog title is unchanged"
    );
    assert!(
        prompt.1.contains("1 session") && prompt.1.contains("cannot be undone"),
        "the irreversible half must be named on its own, got: {}",
        prompt.1
    );
    assert!(
        prompt.1.contains("1 KB"),
        "the trash byte figure must come only from the recoverable session, got: {}",
        prompt.1
    );
    assert!(
        !prompt.1.contains("2 session"),
        "the two counts must never be merged into one total, got: {}",
        prompt.1
    );

    cx.simulate_prompt_answer("Delete");
    cx.run_until_parked();

    assert_eq!(
        trashed_names(&fs),
        vec!["trashed.jsonl".to_string()],
        "the recoverable half still moves to the trash"
    );
    assert_eq!(
        remaining_ids(&panel, &mut cx),
        vec!["commanded".to_string()],
        "the irreversible session, whose CLI is not installed under test, must \
         stay listed rather than vanish while still in its own store"
    );
}

/// A tab running `id`, alive or not, in the same app as the panel.
fn tab_running(
    id: &str,
    alive: bool,
    panel: &Entity<AgentHistoryPanel>,
    cx: &mut VisualTestContext,
) -> Entity<crate::AgentView> {
    let project = panel.read_with(cx, |panel, cx| {
        panel
            .workspace()
            .upgrade()
            .expect("the panel's workspace is open")
            .read(cx)
            .project()
            .clone()
    });
    let id = id.to_string();
    let view = cx.new(|cx| {
        crate::AgentView::test_new(
            project::AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
            zed_actions::agent::AgentViewMode::Terminal,
            project,
            gpui::WeakEntity::new_invalid(),
            crate::SessionOrigin::new(crate::agent_view::SessionIntent::Tracked(id.into()), None),
            cx,
        )
    });
    view.update(cx, |view, cx| view.simulate_cli_alive(alive, cx));
    repaint(cx);
    view
}

/// A tab opening or closing marks the panel dirty, and the frame that follows is
/// only published to `debug_bounds` once the platform asks for one; resizing the
/// window to the size it already has is how a test makes it ask.
fn repaint(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    let size = cx.update(|window, _| window.viewport_size());
    cx.simulate_resize(size);
    cx.run_until_parked();
}

fn open_session(id: &str) -> SessionSummary {
    SessionSummary {
        log_path: Some(PathBuf::from(format!("/logs/{id}.jsonl"))),
        ..session(id, "/root", "A conversation", 300)
    }
}

/// The store notifies for a forgotten session as well as for a finished sweep.
/// A delete landing in the middle of a sweep must not read as the sweep being
/// over: Delete-all would plan from the index the sweep is about to replace.
#[gpui::test]
async fn a_delete_landing_mid_sweep_does_not_end_the_wait(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let (panel, mut cx) = panel_with(
        &["/root"],
        vec![session("one", "/root", "First", 300)],
        Vec::new(),
        fs,
        cx,
    )
    .await;

    panel.update(&mut cx, |panel, cx| panel.refresh(cx));
    assert!(panel.read_with(&cx, |panel, _| panel.loading));

    panel.update(&mut cx, |panel, cx| {
        panel.store.update(cx, |store, cx| store.forget("one", cx));
    });
    assert!(
        panel.read_with(&cx, |panel, _| panel.loading),
        "the sweep has not landed, so the panel is still waiting for it"
    );

    cx.run_until_parked();
    assert!(
        !panel.read_with(&cx, |panel, _| panel.loading),
        "and the sweep landing is what ends the wait"
    );
}

#[gpui::test]
async fn deleting_a_session_a_live_tab_runs_says_so(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let (panel, mut cx) = panel_with(
        &["/root"],
        vec![open_session("running"), open_session("idle")],
        Vec::new(),
        fs,
        cx,
    )
    .await;
    let _running = tab_running("running", true, &panel, &mut cx);
    let _ended = tab_running("idle", false, &panel, &mut cx);

    for (id, warned) in [("running", true), ("idle", false)] {
        let target = panel.read_with(&cx, |panel, cx| {
            panel
                .sessions(cx)
                .iter()
                .find(|session| session.id.as_ref() == id)
                .expect("the session is in the index")
                .clone()
        });
        panel.update_in(&mut cx, |panel, window, cx| {
            panel.delete(&target, window, cx)
        });
        cx.run_until_parked();

        let prompt = cx.pending_prompt().expect("a delete must confirm first");
        assert_eq!(prompt.0, "Delete this session?");
        assert_eq!(
            prompt.1.contains("An open tab is running this session"),
            warned,
            "{id}: {}",
            prompt.1
        );
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
    }
}

#[gpui::test]
async fn deleting_all_counts_the_sessions_live_tabs_run(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/logs", json!({ "a.jsonl": "", "b.jsonl": "" }))
        .await;
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let (panel, mut cx) = panel_with(
        &["/root"],
        vec![
            session("a", "/root", "First", 300),
            session("b", "/root", "Second", 200),
        ],
        vec![
            ("a", vec![PathBuf::from("/logs/a.jsonl")]),
            ("b", vec![PathBuf::from("/logs/b.jsonl")]),
        ],
        fs,
        cx,
    )
    .await;
    let _running = tab_running("a", true, &panel, &mut cx);

    click_delete_all(&mut cx);

    let prompt = cx.pending_prompt().expect("a delete must confirm first");
    assert!(
        prompt
            .1
            .contains("1 of these sessions is running in an open tab"),
        "{}",
        prompt.1
    );
    cx.simulate_prompt_answer("Cancel");
}

#[gpui::test]
async fn deleting_all_with_no_live_tab_adds_nothing_to_the_prompt(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/logs", json!({ "a.jsonl": "" })).await;
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let (panel, mut cx) = panel_with(
        &["/root"],
        vec![session("a", "/root", "First", 300)],
        vec![("a", vec![PathBuf::from("/logs/a.jsonl")])],
        fs,
        cx,
    )
    .await;
    let _ended = tab_running("a", false, &panel, &mut cx);

    click_delete_all(&mut cx);

    let prompt = cx.pending_prompt().expect("a delete must confirm first");
    assert!(!prompt.1.contains("open tab"), "{}", prompt.1);
    cx.simulate_prompt_answer("Cancel");
}

#[test]
fn the_note_about_open_tabs_counts_what_it_is_about() {
    use crate::session_history::actions::running_in_tabs_note;
    assert_eq!(running_in_tabs_note(0, 1), None);
    assert_eq!(running_in_tabs_note(0, 9), None);
    assert!(
        running_in_tabs_note(1, 1)
            .unwrap()
            .starts_with("An open tab is running this session.")
    );
    assert!(
        running_in_tabs_note(1, 9)
            .unwrap()
            .starts_with("1 of these sessions is running in an open tab.")
    );
    assert!(
        running_in_tabs_note(3, 9)
            .unwrap()
            .starts_with("3 of these sessions are running in open tabs.")
    );
}

/// A row's "directory is gone" state is answered by a background check kept in
/// the panel, not by a stat per row per frame. Until the check lands a
/// directory reads as present.
#[gpui::test]
async fn the_panel_learns_which_working_directories_are_gone_in_the_background(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let here = tempfile::tempdir().unwrap();
    let here_path = here.path().to_string_lossy().into_owned();
    let gone_path = here.path().join("removed").to_string_lossy().into_owned();
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let (panel, cx) = panel_with(
        &["/root"],
        vec![
            session("kept", &here_path, "Kept", 300),
            session("lost", &gone_path, "Lost", 200),
        ],
        Vec::new(),
        fs,
        cx,
    )
    .await;
    cx.executor().allow_parking();
    cx.run_until_parked();

    panel.read_with(&cx, |panel, _| {
        assert!(panel.cwd_alive(std::path::Path::new(&here_path)));
        assert!(!panel.cwd_alive(std::path::Path::new(&gone_path)));
        assert!(
            panel.cwd_alive(std::path::Path::new("/never/asked")),
            "a directory nobody has checked reads as present"
        );
    });
}

/// A tab for session `id` that lives in `workspace`, added to its active pane.
fn tab_in_workspace(
    id: Option<&str>,
    workspace: &Entity<workspace::Workspace>,
    cx: &mut VisualTestContext,
) -> Entity<crate::AgentView> {
    workspace.update_in(cx, |workspace, window, cx| {
        let project = workspace.project().clone();
        let weak_workspace = workspace.weak_handle();
        let intent = id.map_or(crate::agent_view::SessionIntent::Untracked, |id| {
            crate::agent_view::SessionIntent::Tracked(id.to_string().into())
        });
        let view = cx.new(|cx| {
            crate::AgentView::test_new(
                project::AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
                zed_actions::agent::AgentViewMode::Terminal,
                project,
                weak_workspace,
                crate::SessionOrigin::new(intent, None),
                cx,
            )
        });
        workspace.add_item_to_active_pane(Box::new(view.clone()), None, true, window, cx);
        view
    })
}

fn active_item_of(
    workspace: &Entity<workspace::Workspace>,
    cx: &mut VisualTestContext,
) -> Option<gpui::EntityId> {
    workspace.read_with(cx, |workspace, cx| {
        workspace
            .active_pane()
            .read(cx)
            .active_item()
            .map(|item| item.item_id())
    })
}

fn panel_workspace(
    panel: &Entity<AgentHistoryPanel>,
    cx: &mut VisualTestContext,
) -> Entity<workspace::Workspace> {
    panel.read_with(cx, |panel, _| {
        panel
            .workspace()
            .upgrade()
            .expect("the panel's workspace is open")
    })
}

/// Row 0 is the agent header and row 1 the project header, so the first session
/// is row 2.
const FIRST_SESSION_ROW: &str = "agent-history-row:2";
const FIRST_GO_TO_TAB: &str = "agent-history-go-to-tab:2";

#[gpui::test]
async fn a_session_with_a_tab_is_open_whether_or_not_its_cli_is_running(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let (panel, mut cx) = panel_with(
        &["/root"],
        vec![
            open_session("running"),
            open_session("exited"),
            open_session("closed"),
        ],
        Vec::new(),
        fs,
        cx,
    )
    .await;
    let _running = tab_running("running", true, &panel, &mut cx);
    let _exited = tab_running("exited", false, &panel, &mut cx);

    let open = cx.update(|_, cx| crate::agent_tabs::open_sessions(cx));
    let claude = project::AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string());
    for (id, expected) in [("running", true), ("exited", true), ("closed", false)] {
        assert_eq!(
            open.contains(&(claude.clone(), gpui::SharedString::from(id))),
            expected,
            "{id}"
        );
    }
}

#[gpui::test]
async fn a_row_turns_open_when_its_tab_opens_and_clears_when_it_closes(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let (panel, mut cx) = panel_with(
        &["/root"],
        vec![open_session("one"), open_session("two")],
        Vec::new(),
        fs,
        cx,
    )
    .await;
    assert!(
        cx.debug_bounds(FIRST_GO_TO_TAB).is_none(),
        "no tab is open yet"
    );

    let workspace = panel_workspace(&panel, &mut cx);
    // The newest session sorts first, and both are 300 seconds old, so look the
    // row up by id rather than by position.
    let open_id = panel.read_with(&cx, |panel, cx| panel.sessions(cx)[0].id.to_string());
    let tab = tab_in_workspace(Some(&open_id), &workspace, &mut cx);
    repaint(&mut cx);
    assert!(
        cx.debug_bounds(FIRST_GO_TO_TAB).is_some(),
        "opening the tab must mark its row without anything else prompting a redraw"
    );
    assert!(
        cx.debug_bounds("agent-history-go-to-tab:3").is_none(),
        "and only that row"
    );

    workspace.update_in(&mut cx, |workspace, window, cx| {
        let pane = workspace.active_pane().clone();
        pane.update(cx, |pane, cx| {
            pane.close_item_by_id(tab.entity_id(), workspace::SaveIntent::Skip, window, cx)
                .detach();
        });
    });
    drop(tab);
    // Dropped outside an update, so the release is only seen by the next one.
    cx.update(|_, _| {});
    repaint(&mut cx);
    assert!(
        cx.debug_bounds(FIRST_GO_TO_TAB).is_none(),
        "closing the tab must clear the mark"
    );
}

#[gpui::test]
async fn go_to_tab_activates_the_tab_in_its_pane(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let (panel, mut cx) =
        panel_with(&["/root"], vec![open_session("one")], Vec::new(), fs, cx).await;
    let workspace = panel_workspace(&panel, &mut cx);
    let target = tab_in_workspace(Some("one"), &workspace, &mut cx);
    let other = tab_in_workspace(None, &workspace, &mut cx);
    cx.run_until_parked();
    assert_eq!(active_item_of(&workspace, &mut cx), Some(other.entity_id()));

    let button = cx
        .debug_bounds(FIRST_GO_TO_TAB)
        .expect("an open row offers Go to Tab");
    cx.simulate_mouse_move(button.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_click(button.center(), gpui::Modifiers::default());
    cx.run_until_parked();

    assert_eq!(
        active_item_of(&workspace, &mut cx),
        Some(target.entity_id())
    );
    assert!(
        panel.read_with(&cx, |panel, _| panel.expanded_rows.is_empty()),
        "the click must not also toggle the row"
    );
    assert!(cx.debug_bounds(FIRST_SESSION_ROW).is_some());
}

#[gpui::test]
async fn go_to_tab_reaches_a_tab_in_another_window(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    fs.insert_tree("/elsewhere", json!({ "b.txt": "" })).await;
    let (panel, mut cx) = panel_with(
        &["/root"],
        vec![open_session("one")],
        Vec::new(),
        fs.clone(),
        cx,
    )
    .await;

    let other_project = Project::test(fs, ["/elsewhere".as_ref()], &mut cx).await;
    let other_window =
        cx.add_window(|window, cx| MultiWorkspace::test_new(other_project.clone(), window, cx));
    let other_workspace = other_window
        .read_with(&cx, |multi, _| multi.workspace().clone())
        .unwrap();
    let mut other_cx = VisualTestContext::from_window(other_window.into(), &cx);
    let target = tab_in_workspace(Some("one"), &other_workspace, &mut other_cx);
    let neighbour = tab_in_workspace(None, &other_workspace, &mut other_cx);
    cx.run_until_parked();
    assert_eq!(
        active_item_of(&other_workspace, &mut other_cx),
        Some(neighbour.entity_id())
    );

    let button = cx
        .debug_bounds(FIRST_GO_TO_TAB)
        .expect("an open row offers Go to Tab, whichever window holds the tab");
    cx.simulate_mouse_move(button.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_click(button.center(), gpui::Modifiers::default());
    cx.run_until_parked();

    assert_eq!(
        active_item_of(&other_workspace, &mut other_cx),
        Some(target.entity_id())
    );
    assert_eq!(
        cx.update(|_, cx| cx.active_window()),
        Some(other_window.into()),
        "the window holding the tab must come forward"
    );
    drop(panel);
}

#[gpui::test]
async fn the_panel_is_notified_when_a_tab_opens_or_closes(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let (panel, mut cx) =
        panel_with(&["/root"], vec![open_session("one")], Vec::new(), fs, cx).await;
    let notifications = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let _observation = cx.update(|_, cx| {
        let notifications = notifications.clone();
        cx.observe(&panel, move |_, _| {
            notifications.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        })
    });
    let count = || notifications.load(std::sync::atomic::Ordering::SeqCst);

    let tab = tab_running("one", false, &panel, &mut cx);
    let after_open = count();
    assert!(after_open > 0, "a tab opening must redraw the panel");

    drop(tab);
    cx.update(|_, _| {});
    cx.run_until_parked();
    assert!(count() > after_open, "a tab closing must redraw the panel");
}

/// A remote project's sweep asks the host to delete each session by agent and
/// id, and touches nothing here: the fixture's paths exist on this machine, so
/// a sweep that trashed locally would have something to take. Only what the
/// host says is gone leaves the list, a target with nothing to take is never
/// sent, and one failure does not stop the rest.
#[gpui::test]
async fn a_remote_projects_sweep_asks_the_host_and_trashes_nothing_here(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/logs",
        json!({ "gone.jsonl": "", "failed.jsonl": "", "nothing.jsonl": "", "later.jsonl": "" }),
    )
    .await;
    let store = cx.update(|cx| crate::SessionStore::install_global_for_test(Vec::new(), cx));
    store.update(cx, |store, cx| {
        store.set_index_for_test(
            ["gone", "failed", "nothing", "later"]
                .into_iter()
                .map(|id| session(id, "/home/u/p", id, 1))
                .collect(),
            cx,
        )
    });
    let target = |id: &str, deletion: Deletion| super::DeleteTarget {
        id: Arc::from(id),
        agent: AgentKind::Claude,
        deletion: deletion.into(),
    };
    let trash = |id: &str| Deletion::Trash(vec![PathBuf::from(format!("/logs/{id}.jsonl"))]);
    let asked: Arc<std::sync::Mutex<Vec<(AgentKind, String)>>> = Arc::default();

    cx.update(|cx| {
        let asked = asked.clone();
        super::actions::sweep_on_remote_host(
            store.clone(),
            vec![
                target("gone", trash("gone")),
                target("failed", trash("failed")),
                target("nothing", Deletion::Nothing),
                target("later", trash("later")),
            ],
            move |agent, id, _| {
                if let Ok(mut asked) = asked.lock() {
                    asked.push((agent, id.to_string()));
                }
                gpui::Task::ready(Ok(if id.as_ref() == "failed" {
                    agent_sessions::wire::WireDeleteOutcome::Failed {
                        message: "no trash on this host".to_string(),
                    }
                } else {
                    agent_sessions::wire::WireDeleteOutcome::Gone
                }))
            },
            cx,
        )
    })
    .await;
    cx.run_until_parked();

    let asked = asked.lock().map(|asked| asked.clone()).unwrap_or_default();
    assert_eq!(
        asked,
        vec![
            (AgentKind::Claude, "gone".to_string()),
            (AgentKind::Claude, "failed".to_string()),
            (AgentKind::Claude, "later".to_string()),
        ]
    );
    assert_eq!(
        fs.trash_entries().len(),
        0,
        "nothing on this machine may be trashed for a remote project"
    );
    let mut remaining: Vec<String> = store.read_with(cx, |store, _| {
        store
            .index()
            .sessions()
            .iter()
            .map(|session| session.id.to_string())
            .collect()
    });
    remaining.sort();
    assert_eq!(remaining, vec!["failed".to_string(), "nothing".to_string()]);
}

/// Turns the panel's project into a collaboration guest's, whose sessions are
/// on a collaborator's machine that nothing here can ask.
fn make_project_not_local(
    panel: &Entity<AgentHistoryPanel>,
    cx: &mut VisualTestContext,
) -> Entity<workspace::Workspace> {
    let workspace = panel
        .read_with(cx, |panel, _| panel.workspace().upgrade())
        .expect("the panel's workspace is alive");
    let project = workspace.read_with(cx, |workspace, _| workspace.project().clone());
    project.update(cx, |project, _| project.mark_as_collab_for_testing());
    workspace
}

/// "Delete all" in a shared project must refuse before it builds a plan or
/// asks anything: the paths it would plan are this machine's.
#[gpui::test]
async fn deleting_all_in_a_shared_project_refuses_before_asking(cx: &mut TestAppContext) {
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
    make_project_not_local(&panel, &mut cx);

    click_delete_all(&mut cx);

    assert!(
        !cx.has_pending_prompt(),
        "a shared project's delete-all must not offer to trash this machine's files"
    );
    assert!(
        has_message(
            &panel_workspace(&panel, &mut cx),
            super::actions::SHARED_DELETE_REFUSAL,
            &mut cx
        ),
        "the refusal is said, not swallowed"
    );
    assert!(trashed_names(&fs).is_empty());
    assert_eq!(remaining_ids(&panel, &mut cx), vec!["one".to_string()]);
}

/// Both public entry points refuse in a shared project, whoever calls them --
/// the git panel's checkout removal reaches `execute_session_deletion` too.
#[gpui::test]
async fn a_shared_projects_deletes_never_reach_the_trash(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/logs", json!({ "one.jsonl": "", "two.jsonl": "" }))
        .await;
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let target = SessionSummary {
        log_path: Some(PathBuf::from("/logs/one.jsonl")),
        log_bytes: 10,
        ..session("one", "/root", "First", 300)
    };
    let (panel, mut cx) =
        panel_with(&["/root"], vec![target.clone()], Vec::new(), fs.clone(), cx).await;
    let workspace = make_project_not_local(&panel, &mut cx);

    cx.update(|window, cx| super::actions::delete_session(&workspace, &target, window, cx));
    cx.run_until_parked();
    assert!(
        !cx.has_pending_prompt(),
        "a single delete must refuse rather than ask"
    );
    assert!(has_message(
        &workspace,
        super::actions::SHARED_DELETE_REFUSAL,
        &mut cx
    ));

    let sweep = cx.update(|_, cx| {
        super::actions::execute_session_deletion(
            &workspace,
            vec![super::DeleteTarget {
                id: Arc::from("two"),
                agent: AgentKind::Claude,
                deletion: Deletion::Trash(vec![PathBuf::from("/logs/two.jsonl")]).into(),
            }],
            cx,
        )
    });
    sweep.await;
    cx.run_until_parked();

    assert!(
        trashed_names(&fs).is_empty(),
        "nothing on this machine may be trashed for a shared project, got {:?}",
        trashed_names(&fs)
    );
}

/// Whether `workspace` shows the notification a message of exactly `text`
/// raises.
fn has_message(
    workspace: &Entity<workspace::Workspace>,
    text: &'static str,
    cx: &mut VisualTestContext,
) -> bool {
    let id = workspace::notifications::NotificationId::named(text.into());
    workspace.read_with(cx, |workspace, _| {
        workspace.notification_ids().contains(&id)
    })
}

fn trash_target(id: &str) -> super::DeleteTarget {
    super::DeleteTarget {
        id: Arc::from(id),
        agent: AgentKind::Claude,
        deletion: Deletion::Trash(vec![PathBuf::from(format!("/logs/{id}.jsonl"))]).into(),
    }
}

fn index_ids(store: &Entity<crate::SessionStore>, cx: &mut TestAppContext) -> Vec<String> {
    let mut ids: Vec<String> = store.read_with(cx, |store, _| {
        store
            .index()
            .sessions()
            .iter()
            .map(|session| session.id.to_string())
            .collect()
    });
    ids.sort();
    ids
}

/// A host that answers about one session but not another is still there, and
/// the batch goes on. A host that does not answer at all is not: every request
/// after it would wait out the same deadline, so the batch ends at the first
/// one and says how far it got.
#[gpui::test]
async fn a_remote_sweep_stops_at_the_first_request_the_host_does_not_answer(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let ids = ["gone", "failed", "nothing", "dead", "after"];
    let store = cx.update(|cx| crate::SessionStore::install_global_for_test(Vec::new(), cx));
    store.update(cx, |store, cx| {
        store.set_index_for_test(
            ids.into_iter()
                .map(|id| session(id, "/home/u/p", id, 1))
                .collect(),
            cx,
        )
    });
    let asked: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();

    let report = cx
        .update(|cx| {
            let asked = asked.clone();
            super::actions::sweep_on_remote_host(
                store.clone(),
                ids.into_iter().map(trash_target).collect(),
                move |_, id, _| {
                    if let Ok(mut asked) = asked.lock() {
                        asked.push(id.to_string());
                    }
                    use agent_sessions::wire::WireDeleteOutcome;
                    gpui::Task::ready(match id.as_ref() {
                        "gone" => Ok(WireDeleteOutcome::Gone),
                        "failed" => Ok(WireDeleteOutcome::Failed {
                            message: "busy".to_string(),
                        }),
                        "nothing" => Ok(WireDeleteOutcome::NothingToDelete),
                        _ => Err(anyhow::anyhow!("link down")),
                    })
                },
                cx,
            )
        })
        .await;
    cx.run_until_parked();

    let asked = asked.lock().map(|asked| asked.clone()).unwrap_or_default();
    assert_eq!(asked, vec!["gone", "failed", "nothing", "dead"]);
    assert_eq!(
        report.stopped_message().as_deref(),
        Some("Deleted 1 of 5 sessions; the remote host stopped answering: link down")
    );
    assert_eq!(
        index_ids(&store, cx),
        vec!["after", "dead", "failed", "nothing"],
        "only what the host said is gone leaves the list"
    );
}

/// The local sweep takes what this machine's provider named. Kept beside the
/// remote tests so that a change routing deletes elsewhere cannot quietly stop
/// a local project's sweep from trashing anything.
#[gpui::test]
async fn a_local_sweep_trashes_what_it_was_given(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/logs", json!({ "one.jsonl": "", "two.jsonl": "" }))
        .await;
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let project = Project::test(fs.clone(), [std::path::Path::new("/root")], cx).await;
    let agent_store = project.read_with(cx, |project, _| project.agent_server_store().clone());
    let store = cx.update(|cx| crate::SessionStore::install_global_for_test(Vec::new(), cx));
    store.update(cx, |store, cx| {
        store.set_index_for_test(
            vec![
                session("one", "/root", "One", 1),
                session("host", "/root", "Host", 1),
            ],
            cx,
        )
    });

    cx.update(|cx| {
        super::actions::sweep_session_deletion(
            fs.clone(),
            agent_store,
            vec![
                trash_target("one"),
                super::DeleteTarget {
                    id: Arc::from("host"),
                    agent: AgentKind::Claude,
                    deletion: super::actions::TargetDeletion::OnHost(
                        super::actions::HostDeletion::Trash,
                    ),
                },
            ],
            cx,
        )
    })
    .await;
    cx.run_until_parked();

    assert_eq!(trashed_names(&fs), vec!["one.jsonl".to_string()]);
    assert_eq!(
        index_ids(&store, cx),
        vec!["host"],
        "a remote host's target is neither acted on nor forgotten here"
    );
}

/// The host still lists the session but owns nothing to take. Forgetting it
/// on the spot would bring it back on the next sweep, so the reader is asked,
/// as a local session with nothing on disk is.
#[gpui::test]
async fn a_remote_delete_with_nothing_left_asks_before_dropping_the_row(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let target = session("one", "/root", "First", 300);
    let (panel, mut cx) =
        panel_with(&["/root"], vec![target.clone()], Vec::new(), fs.clone(), cx).await;
    let workspace = panel_workspace(&panel, &mut cx);
    let store = panel.read_with(&cx, |panel, _| panel.store.clone());

    cx.update(|window, cx| {
        super::actions::delete_on_remote_host(
            &workspace,
            store,
            &target,
            |_, _, _| {
                gpui::Task::ready(Ok(agent_sessions::wire::WireDeleteOutcome::NothingToDelete))
            },
            window,
            cx,
        )
    });
    cx.run_until_parked();
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();

    let follow_up = cx
        .pending_prompt()
        .expect("nothing left to take is asked about, not forgotten");
    assert_eq!(follow_up.0, "Nothing left to delete");
    assert!(
        follow_up.1.contains("on the remote host"),
        "{}",
        follow_up.1
    );
    assert_eq!(remaining_ids(&panel, &mut cx), vec!["one".to_string()]);

    cx.simulate_prompt_answer("Remove From List");
    cx.run_until_parked();
    assert!(remaining_ids(&panel, &mut cx).is_empty());
}

/// The panel's own `remote` mark no longer refuses a resume: whether one goes
/// ahead is the project's host's to say, which for this local project is this
/// machine. A real remote host's answer is covered by the tests over a fake
/// remote server below.
#[gpui::test]
async fn a_panel_marked_remote_leaves_the_resume_to_the_projects_host(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let here = std::env::temp_dir();
    let target = session("one", &here.to_string_lossy(), "First", 300);
    let (panel, mut cx) =
        panel_with(&["/root"], vec![target.clone()], Vec::new(), fs.clone(), cx).await;
    let workspace = panel_workspace(&panel, &mut cx);
    panel.update(&mut cx, |panel, _| panel.remote = true);
    let before = active_item_of(&workspace, &mut cx);

    panel.update_in(&mut cx, |panel, window, cx| {
        panel.resume(&target, agent_sessions::Fork::Continue, window, cx)
    });
    cx.run_until_parked();

    assert_ne!(
        active_item_of(&workspace, &mut cx),
        before,
        "the resumed session's tab opened"
    );
}

/// A collaboration guest's sessions are on the collaborator's machine, which
/// nothing here can ask: a resume refuses and says so, rather than opening a
/// tab that would start fresh while looking resumed.
#[gpui::test]
async fn resuming_in_a_shared_project_refuses_and_says_why(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let here = std::env::temp_dir();
    let target = session("one", &here.to_string_lossy(), "First", 300);
    let (panel, mut cx) =
        panel_with(&["/root"], vec![target.clone()], Vec::new(), fs.clone(), cx).await;
    let workspace = make_project_not_local(&panel, &mut cx);
    let before = active_item_of(&workspace, &mut cx);

    for fork in [agent_sessions::Fork::Continue, agent_sessions::Fork::New] {
        panel.update_in(&mut cx, |panel, window, cx| {
            panel.resume(&target, fork, window, cx)
        });
        cx.run_until_parked();
    }

    assert!(has_message(
        &workspace,
        super::actions::SHARED_RESUME_REFUSAL,
        &mut cx
    ));
    assert_eq!(active_item_of(&workspace, &mut cx), before, "no tab opened");
}

#[test]
fn a_rows_resume_controls_follow_its_directory_and_its_agent() {
    let alive = super::row::resume_controls(true, true);
    assert!(alive.resume && alive.fork);
    let codex = super::row::resume_controls(true, false);
    assert!(codex.resume && !codex.fork);
    let gone = super::row::resume_controls(false, true);
    assert!(!gone.resume && !gone.fork);
    assert_eq!(
        gone.fork_tooltip,
        "Continue in New Session — the directory is gone"
    );
}

/// The file manager here can show a local project's paths and a WSL distro's,
/// through its share. An SSH host's paths name nothing on this machine -- or
/// worse, something else.
#[test]
fn only_paths_this_machine_can_reach_are_revealed() {
    use super::panel::reveal_reaches_the_path;
    assert!(reveal_reaches_the_path(false, false));
    assert!(reveal_reaches_the_path(true, true));
    assert!(!reveal_reaches_the_path(true, false));
}

/// A project opened over a fake remote server, whose session requests are
/// answered here, so a test sees exactly what the server was asked.
struct RemoteRig {
    workspace: Entity<workspace::Workspace>,
    project: Entity<Project>,
    deleted: Arc<std::sync::Mutex<Vec<(AgentKind, String)>>>,
    _server_keepalive: Entity<()>,
}

/// `folders` are the directories the server says it has, or `None` for a
/// server built before the session requests it is sent, which says it cannot
/// read them.
async fn remote_rig(
    folders: Option<Vec<&'static str>>,
    cx: &mut TestAppContext,
    server_cx: &mut TestAppContext,
) -> (RemoteRig, VisualTestContext) {
    init_test(cx);
    cx.update(|cx| release_channel::init(semver::Version::new(0, 0, 0), cx));
    server_cx.update(|cx| release_channel::init(semver::Version::new(0, 0, 0), cx));
    let params = cx.update(workspace::AppState::test);
    let (options, server_session, connect_guard) = remote::RemoteClient::fake_server(cx, server_cx);
    let server_keepalive = server_cx.new(|_| ());
    server_session.add_request_handler(
        server_keepalive.downgrade(),
        |_, _: rpc::TypedEnvelope<rpc::proto::Ping>, _| async { Ok(rpc::proto::Ack {}) },
    );
    let deleted: Arc<std::sync::Mutex<Vec<(AgentKind, String)>>> = Arc::default();
    server_session.add_request_handler(server_keepalive.downgrade(), {
        let deleted = deleted.clone();
        move |_, envelope: rpc::TypedEnvelope<rpc::proto::AgentSessionRequest>, _| {
            let answer =
                answer_session_request(&envelope.payload.op_json, folders.as_deref(), &deleted);
            async move { answer }
        }
    });
    drop(connect_guard);
    let remote_client = remote::RemoteClient::connect_mock(options, cx).await;
    let project = cx.update(|cx| {
        Project::remote(
            remote_client,
            params.client.clone(),
            params.node_runtime.clone(),
            params.user_store.clone(),
            params.languages.clone(),
            params.fs.clone(),
            false,
            cx,
        )
    });
    let window = cx.add_window({
        let project = project.clone();
        move |window, cx| {
            let workspace =
                cx.new(|cx| workspace::Workspace::new(None, project, params, window, cx));
            MultiWorkspace::new(workspace, window, cx)
        }
    });
    let workspace = window
        .read_with(cx, |multi_workspace, _| multi_workspace.workspace().clone())
        .expect("the window is open");
    let cx = VisualTestContext::from_window(window.into(), cx);
    (
        RemoteRig {
            workspace,
            project,
            deleted,
            _server_keepalive: server_keepalive,
        },
        cx,
    )
}

fn answer_session_request(
    op_json: &str,
    folders: Option<&[&'static str]>,
    deleted: &std::sync::Mutex<Vec<(AgentKind, String)>>,
) -> anyhow::Result<rpc::proto::AgentSessionResponse> {
    use agent_sessions::wire::{SessionOp, SessionOpResult, WireDeleteOutcome};
    use anyhow::Context as _;
    let Some(folders) = folders else {
        return Err(
            anyhow::anyhow!("unknown variant").context("a session request this server cannot read")
        );
    };
    let result = match serde_json::from_str::<SessionOp>(op_json)? {
        SessionOp::List { .. } => SessionOpResult::List {
            sessions: Vec::new(),
            unavailable: Vec::new(),
        },
        SessionOp::DirectoriesExist { paths } => SessionOpResult::DirectoriesExist {
            exists: paths
                .iter()
                .map(|path| folders.contains(&path.as_str()))
                .collect(),
        },
        SessionOp::Delete { agent, id } => {
            deleted
                .lock()
                .map_err(|_| anyhow::anyhow!("the record of deletes is poisoned"))?
                .push((agent.into(), id));
            SessionOpResult::Delete {
                outcome: WireDeleteOutcome::Gone,
            }
        }
        other => anyhow::bail!("this fake server does not answer {other:?}"),
    };
    Ok(rpc::proto::AgentSessionResponse {
        result_json: serde_json::to_string(&result).context("encoding the answer")?,
    })
}

fn deletes_seen(rig: &RemoteRig) -> Vec<(AgentKind, String)> {
    rig.deleted
        .lock()
        .map(|deleted| deleted.clone())
        .unwrap_or_default()
}

/// A checkout removal or "delete all" in a remote project names each session
/// to the server by agent and id, and touches nothing on this machine --
/// though files here spell the very paths the sessions were listed with.
#[gpui::test]
async fn a_remote_projects_sweep_asks_its_host_for_each_session_and_trashes_nothing_here(
    cx: &mut TestAppContext,
    server_cx: &mut TestAppContext,
) {
    use fs::Fs as _;
    let (rig, mut cx) = remote_rig(Some(Vec::new()), cx, server_cx).await;
    let client_fs = rig
        .project
        .read_with(&cx, |project, _| project.fs().as_fake());
    client_fs
        .insert_tree(
            "/home/u/.claude/projects/-home-u-p",
            json!({ "one.jsonl": "", "two.jsonl": "" }),
        )
        .await;
    let on_host = |id: &str, agent: AgentKind| SessionSummary {
        agent,
        log_path: Some(PathBuf::from(format!(
            "/home/u/.claude/projects/-home-u-p/{id}.jsonl"
        ))),
        log_bytes: 10,
        ..session(id, "/home/u/p", id, 300)
    };
    let sessions = [
        on_host("one", AgentKind::Claude),
        on_host("two", AgentKind::Codex),
    ];
    let plan = super::DeleteAll::on_host(sessions.iter());
    assert!(plan.targets.iter().all(super::DeleteTarget::is_on_host));

    let sweep = cx
        .update(|_, cx| super::actions::execute_session_deletion(&rig.workspace, plan.targets, cx));
    sweep.await;
    cx.run_until_parked();

    assert_eq!(
        deletes_seen(&rig),
        vec![
            (AgentKind::Claude, "one".to_string()),
            (AgentKind::Codex, "two".to_string())
        ],
        "the server is asked for each session, by agent and id"
    );
    assert!(
        trashed_names(&client_fs).is_empty(),
        "nothing on this machine is trashed, got {:?}",
        trashed_names(&client_fs)
    );
    assert!(
        client_fs
            .is_file(std::path::Path::new(
                "/home/u/.claude/projects/-home-u-p/one.jsonl"
            ))
            .await
    );
}

/// Whether a remote session may be resumed is the server's to say: its folder
/// is a path there, and this machine has none of it.
#[gpui::test]
async fn a_remote_sessions_folder_is_checked_on_its_host(
    cx: &mut TestAppContext,
    server_cx: &mut TestAppContext,
) {
    let (rig, mut cx) = remote_rig(Some(vec!["/home/u/p"]), cx, server_cx).await;
    let host = rig
        .project
        .read_with(&cx, |project, cx| project.agent_session_host(cx))
        .expect("a remote project has a session host");
    assert!(host.is_remote());
    let there = session("one", "/home/u/p", "First", 300);
    let gone = session("two", "/home/u/gone", "Second", 300);

    let refusal_there = cx.update(|_, cx| {
        super::actions::resume_refusal_on(&host, &there, agent_sessions::Fork::Continue, cx)
    });
    let refusal_gone = cx.update(|_, cx| {
        super::actions::resume_refusal_on(&host, &gone, agent_sessions::Fork::Continue, cx)
    });
    cx.run_until_parked();

    assert_eq!(refusal_there.await.ok(), Some(None));
    assert_eq!(
        refusal_gone.await.ok(),
        Some(Some(super::ResumeRefusal::CwdGone))
    );
}

/// A server that cannot answer has not said the folder is gone. The resume
/// is refused, and the reason -- here, a server to update -- is said.
#[gpui::test]
async fn a_resume_its_host_cannot_check_says_why_and_opens_nothing(
    cx: &mut TestAppContext,
    server_cx: &mut TestAppContext,
) {
    let (rig, mut cx) = remote_rig(None, cx, server_cx).await;
    let target = session("one", "/home/u/p", "First", 300);
    let before = active_item_of(&rig.workspace, &mut cx);

    cx.update(|window, cx| {
        super::actions::resume_session(
            &rig.workspace,
            &target,
            agent_sessions::Fork::Continue,
            window,
            cx,
        )
    });
    cx.run_until_parked();

    let failed =
        workspace::notifications::NotificationId::unique::<super::actions::ResumeCheckFailed>();
    assert!(
        rig.workspace.read_with(&cx, |workspace, _| workspace
            .notification_ids()
            .contains(&failed)),
        "the failed check is said, not swallowed"
    );
    assert_eq!(
        active_item_of(&rig.workspace, &mut cx),
        before,
        "no tab opened"
    );
}

/// The panel is built inside the workspace's own update, which is how
/// `AgentHistoryPanel::load` adds it to a window. Reading the workspace back
/// from there panics, so a remote project's panel must reach its host without
/// going through the workspace.
#[gpui::test]
async fn a_remote_project_s_panel_is_built_while_its_workspace_is_updating(
    cx: &mut TestAppContext,
    server_cx: &mut TestAppContext,
) {
    let (rig, mut cx) = remote_rig(Some(vec!["/home/u/p"]), cx, server_cx).await;

    let panel = rig.workspace.update_in(&mut cx, |workspace, window, cx| {
        cx.new(|cx| AgentHistoryPanel::new(workspace, window, cx))
    });
    cx.run_until_parked();

    panel.read_with(&cx, |panel, _| {
        assert!(panel.remote, "the panel of a remote project reads its host");
    });
}
