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

/// A panel over a project that really holds a repository, so `collect_repos`
/// has something to find.
///
/// The plain `panel` helper opens an empty project, which leaves the rebuild
/// with no repositories and nothing to say about how a repository is drawn.
async fn panel_over_a_repo(
    cx: &mut TestAppContext,
) -> (gpui::Entity<BranchPanel>, &mut VisualTestContext) {
    init_test(cx);
    let fs = fs::FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/repos/zode",
        serde_json::json!({
            ".git": {},
            "src": { "main.rs": "fn main() {}" },
        }),
    )
    .await;
    let project = Project::test(fs.clone(), ["/repos/zode".as_ref()], cx).await;
    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        BranchPanel::new(workspace, window, cx)
    });
    (panel, cx)
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

/// Pressing reload must actually cause a rebuild, not merely schedule a git scan whose
/// result the panel then ignores.
///
/// What this does NOT cover: that `git worktree list` really re-ran. That half lives in
/// the store's keyed job queue, which is not observable from here, and the harness builds
/// a project with no repository at all -- so `refresh_all_repositories` iterates nothing.
/// The scan is asserted by reading `refresh_all_repositories`, not by this test; what is
/// asserted here is the panel half, which is the half that could silently regress if
/// someone dropped the `mark_stale` call believing the subscription would cover it.
#[gpui::test]
async fn reload_marks_the_tree_stale_so_the_next_frame_rebuilds(cx: &mut TestAppContext) {
    let (panel, cx) = panel(cx).await;

    panel.update_in(cx, |panel, window, cx| {
        panel.set_active(true, window, cx);
        panel.refresh_if_stale(cx);
        let before = panel.rebuild_count;

        panel.reload(cx);
        panel.refresh_if_stale(cx);
        assert_eq!(
            panel.rebuild_count,
            before + 1,
            "reload must rebuild the tree, not just poke git"
        );
    });
}

/// The spinner must settle. A reload that finds nothing changed emits no repository event
/// at all, so if the icon were keyed off `GitWorktreeListChanged` it would spin forever in
/// the commonest case -- which is why `reload` awaits the scan instead. This asserts the
/// far end of that: `reloading` goes back to false on its own.
#[gpui::test]
async fn the_reload_spinner_settles_when_the_scan_finishes(cx: &mut TestAppContext) {
    let (panel, cx) = panel(cx).await;

    panel.update_in(cx, |panel, window, cx| {
        panel.set_active(true, window, cx);
        panel.reload(cx);
        assert!(panel.reloading, "the icon spins as soon as it is pressed");
    });

    cx.run_until_parked();

    panel.update(cx, |panel, _| {
        assert!(
            !panel.reloading,
            "the icon must settle once the scan is done, not spin forever"
        );
    });
}

/// Reload obeys the same rule as every other trigger here: a panel nobody is looking at
/// does no work. Pressing it cannot be the one path that rebuilds a hidden panel.
#[gpui::test]
async fn reload_on_a_hidden_panel_still_costs_nothing(cx: &mut TestAppContext) {
    let (panel, cx) = panel(cx).await;

    panel.update(cx, |panel, cx| {
        assert!(!panel.is_active, "a fresh panel starts hidden");
        // Also the no-repository path: this project has none, so reload must be a safe
        // no-op rather than a panic.
        for _ in 0..5 {
            panel.reload(cx);
        }
        assert_eq!(
            panel.rebuild_count, 0,
            "five reloads while hidden must cost zero rebuilds"
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

    /// A repository nobody has closed is open, and a checkout's agents stay
    /// shut until somebody asks for them.
    ///
    /// The two rows go opposite ways round on purpose. Closing a repository
    /// hides every checkout under it, so "closed unless recorded otherwise"
    /// meant a worktree you had just made, a project you had just opened, or a
    /// fresh machine all arrived at a panel listing nothing at all.
    #[gpui::test]
    async fn a_repository_nobody_has_closed_is_open(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let id = RepositoryId(1);
        let checkout = std::sync::Arc::from(std::path::Path::new("/repos/zode/wt"));

        panel.update(cx, |panel, _| {
            panel.repos = vec![repo_data(id)];

            assert!(
                panel.row_is_open(&RowKey::Repo(id)),
                "a repository with nothing recorded against it must draw open"
            );
            assert!(
                !panel.row_is_open(&RowKey::WorktreeAgents(id, checkout)),
                "a checkout's agents must still stay shut until asked for"
            );
        });
    }

    /// A blob from before repositories recorded their closure lists the ones
    /// that were open. Those entries govern nothing now and must be dropped on
    /// the way in, not parked in a set nobody reads and rewritten on every
    /// save for the life of the workspace.
    #[gpui::test]
    async fn a_repository_entry_from_the_old_format_is_not_kept(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let id = RepositoryId(1);

        panel.update(cx, |panel, _| {
            panel.repos = vec![repo_data(id)];
            panel
                .stored_expanded
                .insert(StoredKey::Repo(REPO_PATH.to_string()));

            panel.adopt_stored_expansion();

            assert!(
                !panel.expanded.contains(&RowKey::Repo(id)),
                "a repository entry has no meaning in the opened set and must be dropped"
            );
            assert!(
                panel.stored_expanded.is_empty(),
                "and it must still be consumed, or the next rebuild re-adopts it"
            );
            assert!(
                panel.row_is_open(&RowKey::Repo(id)),
                "dropping it leaves the repository at its default, which is open"
            );
        });
    }

    /// Closing a repository is remembered, and survives the restore.
    #[gpui::test]
    async fn a_repository_the_reader_closed_stays_closed(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let id = RepositoryId(1);
        let key = RowKey::Repo(id);

        panel.update(cx, |panel, cx| {
            panel.repos = vec![repo_data(id)];

            panel.toggle_row(key.clone(), cx);
            assert!(
                !panel.row_is_open(&key),
                "the gesture has to close a repository that was open"
            );

            panel.toggle_row(key.clone(), cx);
            assert!(
                panel.row_is_open(&key),
                "and open it again, rather than the set filling up one way"
            );
        });

        // What a restart hands back: the closure recorded by path, with no
        // live repository id yet.
        panel.update(cx, |panel, _| {
            panel.collapsed.clear();
            panel
                .stored_collapsed
                .insert(StoredKey::Repo(REPO_PATH.to_string()));

            panel.adopt_stored_expansion();
            assert!(
                !panel.row_is_open(&key),
                "a repository the reader closed must come back closed"
            );
        });
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

/// An open row shows what its tab is called *now*, not what it was called when
/// the tree was built.
///
/// The row stores a label at build time and a rename rebuilds nothing, so the
/// stored copy goes stale the moment the user commits one. Built with a
/// deliberately wrong stored label: if the accessor ever goes back to reading
/// it, this says so immediately.
#[gpui::test]
async fn an_open_rows_label_follows_its_tab(cx: &mut TestAppContext) {
    use crate::branch_panel::tree::AgentEntry;

    let (panel, cx) = panel(cx).await;
    let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());

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

    let view = workspace
        .read_with(cx, |workspace, cx| {
            workspace
                .items_of_type::<agent_ui::AgentView>(cx)
                .next()
                .expect("the agent tab just opened")
        })
        .unwrap();
    let live = view.read_with(cx, |view, _| view.tab_label());

    let entry = AgentEntry::Open {
        label: "what it was called an hour ago".into(),
        agent: project::CLAUDE_CODE_AGENT_ID.into(),
        view: view.downgrade(),
    };

    cx.update(|_, cx| {
        assert_eq!(
            entry.label(cx),
            live,
            "the row must read the tab, not the copy taken when it was built"
        );
        assert_eq!(
            entry.stored_label().as_ref(),
            "what it was called an hour ago",
            "and the build-time copy is still there as the fallback"
        );
    });
}

/// Renaming a tab has to redraw the panel even when its agent has already
/// exited.
///
/// The 250ms activity tick carries the live case, but it stops as soon as no
/// listed agent is running -- so without a listener a tab renamed after its
/// agent finished kept its old name until something unrelated rebuilt the
/// panel. `UpdateTab` is the event a rename emits; this raises exactly that.
#[gpui::test]
async fn renaming_an_agent_tab_redraws_the_panel(cx: &mut TestAppContext) {
    let (panel, cx) = panel(cx).await;
    let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());

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

    // Active and rebuilt: that is when the panel picks up its listeners.
    panel.update_in(cx, |panel, window, cx| {
        panel.set_active(true, window, cx);
        panel.refresh_if_stale(cx);
    });
    cx.run_until_parked();
    panel.read_with(cx, |panel, _| {
        assert_eq!(
            panel._agent_tab_names.len(),
            1,
            "the panel must be listening to the one open agent tab"
        );
    });

    let redraws = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let _watcher = cx.new(|cx| {
        let redraws = redraws.clone();
        vec![
            cx.observe(&panel, move |_: &mut Vec<gpui::Subscription>, _, _| {
                redraws.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }),
        ]
    });

    let view = workspace
        .read_with(cx, |workspace, cx| {
            workspace
                .items_of_type::<agent_ui::AgentView>(cx)
                .next()
                .expect("the agent tab is open")
        })
        .unwrap();
    view.update(cx, |_, cx| {
        cx.emit(agent_ui::AgentViewEvent::UpdateTab);
    });
    cx.run_until_parked();

    assert!(
        redraws.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "a renamed tab must wake the panel that names it"
    );
}

/// The complaint itself, end to end: a workspace with nothing recorded against
/// it draws its repository open, so the checkouts under it are on screen.
///
/// The tests above pin `row_is_open` and the restore in isolation. This one
/// goes through the real rebuild -- `collect_repos` off the git store, then
/// `build_rows` -- which is where the bug actually lived.
#[gpui::test]
async fn a_workspace_with_nothing_recorded_lists_its_checkouts(cx: &mut TestAppContext) {
    use crate::branch_panel::tree::TreeRow;

    let (panel, cx) = panel_over_a_repo(cx).await;
    cx.run_until_parked();

    let repo_rows = panel.update(cx, |panel, cx| {
        panel.stale = true;
        panel.refresh_if_stale(cx);
        panel
            .rows
            .iter()
            .filter_map(|row| match row {
                TreeRow::Repo { expanded, .. } => Some(*expanded),
                _ => None,
            })
            .collect::<Vec<_>>()
    });

    // Guards against the test passing by finding nothing to check: an empty
    // row list would satisfy "no closed repository" without proving anything.
    assert!(
        !repo_rows.is_empty(),
        "the harness must give the rebuild a repository to draw, or this proves nothing"
    );
    assert!(
        repo_rows.iter().all(|expanded| *expanded),
        "a repository nobody has closed must be drawn open, so its checkouts show"
    );
}

/// Rows drop their tooltip while one of this panel's menus is open, because
/// GPUI paints tooltips after every deferred draw and a menu is a deferred
/// draw — so a row's tooltip lands on top of the menu that row just opened.
/// `menu_is_open` is the whole of that suppression, which makes its lifecycle
/// load-bearing: leave it stuck on and every tooltip in the panel disappears
/// for good; leave it stuck off and the menu goes back under the tooltip.
///
/// What this does NOT prove is that no tooltip is painted — `tooltip_requests`
/// is `pub(crate)` to `gpui`, so no crate outside it can observe one. The
/// painting order was read from the source (`gpui/src/window.rs:2552` then
/// `:2559`) and the suppression itself is structural.
#[gpui::test]
async fn a_menu_suppresses_tooltips_only_while_it_is_open(cx: &mut TestAppContext) {
    let (panel, cx) = panel(cx).await;

    panel.update(cx, |panel, _| {
        assert!(
            !panel.menu_is_open(),
            "a panel with no menu must not be suppressing tooltips"
        );
    });

    let menu = panel.update_in(cx, |panel, window, cx| {
        let menu = ui::ContextMenu::build(window, cx, |menu, _, _| menu);
        panel.open_context_menu(menu.clone(), gpui::Point::default(), window, cx);
        menu
    });
    panel.update(cx, |panel, _| {
        assert!(
            panel.menu_is_open(),
            "an open menu must suppress the row tooltips that would cover it"
        );
    });

    // Dismissal runs through the subscription `open_context_menu` registered;
    // if that ever stops clearing the field, tooltips never come back.
    menu.update(cx, |_, cx| cx.emit(gpui::DismissEvent));
    cx.run_until_parked();
    panel.update(cx, |panel, _| {
        assert!(
            !panel.menu_is_open(),
            "a dismissed menu must hand the tooltips back"
        );
    });
}
