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
        // A database of this test's own. Without it `AppDatabase::global` falls
        // back to a process-wide static (`db.rs:85-86`), and since the checkout
        // record moved to one un-scoped key, every test in this binary would
        // then read and write the same row -- which is a flake, not a suite.
        cx.set_global(db::AppDatabase::test_new());
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

/// Restoring the expanded sections from the shared record must be a one-shot
/// per repository, and the record itself must be the same one every panel and
/// every restart reads.
///
/// It was not: under the old two-set design the stored entries were
/// re-applied on every rebuild, and since collapsing a row *is* a rebuild, any
/// section that was open when the panel was last saved sprang straight back
/// open. With `CheckoutViewState` there is one path-keyed set behind every
/// panel, so there is nothing left to re-apply -- `toggle` removes the key
/// from the only set that exists.
mod restoring_expansion {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use project::git_store::RepositoryId;

    use crate::branch_panel::checkout_state::CheckoutViewState;
    use crate::branch_panel::state::{BRANCH_PANEL_KEY, SerializedBranchPanel, StoredKey};
    use crate::branch_panel::tree::{RepoData, RowKey};

    use super::panel;
    use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};

    use crate::branch_panel::panel::BranchPanel;

    const REPO_PATH: &str = "/repos/zode";

    /// **T7.** `BranchPanel::load` is the only path production ever takes, and
    /// until now nothing exercised it — every other test builds the panel with
    /// `BranchPanel::new` and steps straight over the legacy-seed ordering.
    ///
    /// The ordering is the part that can silently invert: `seed_from_legacy`
    /// no-ops once a shared record was found, but that flag is only correct
    /// after the shared read has finished. Offer the legacy blob too early and
    /// a record from a previous layout overwrites the one this build wrote.
    #[gpui::test]
    async fn load_lets_the_shared_record_win_over_a_legacy_one(cx: &mut TestAppContext) {
        super::init_test(cx);
        let fs = fs::FakeFs::new(cx.background_executor.clone());
        let project = project::Project::test(fs, [], cx).await;
        let window =
            cx.add_window(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));

        let shared = StoredKey::WorktreeAgents(REPO_PATH.to_string(), "/repos/zode/shared".into());
        let stale = StoredKey::WorktreeAgents(REPO_PATH.to_string(), "/repos/zode/stale".into());

        // A shared record on disk, as this build would have written it.
        let record = SerializedBranchPanel {
            expanded: [shared.clone()].into_iter().collect(),
            ..Default::default()
        };
        let value = serde_json::to_string(&record).expect("the record must serialise");
        cx.update(|cx| db::kvp::KeyValueStore::global(cx))
            .write_kvp(BRANCH_PANEL_KEY.to_string(), value)
            .await
            .expect("seeding the shared record must succeed");

        let panel = window
            .update(cx, |multi, window, cx| {
                let weak = multi.workspace().downgrade();
                cx.spawn_in(window, async move |_, cx| {
                    BranchPanel::load(weak, cx.clone()).await
                })
            })
            .expect("the window must still be open")
            .await
            .expect("the panel must load");

        let state: Entity<CheckoutViewState> =
            panel.read_with(cx, |panel, _| panel.checkout_state.clone());
        state.read_with(cx, |state: &CheckoutViewState, _| {
            assert!(
                state.is_open(&shared),
                "the shared record must be the one that came back -- if this fails, \
                 `load` is not reading the un-scoped key at all"
            );
            assert!(
                !state.is_open(&stale),
                "and a legacy record must not be seeded over it"
            );
        });
    }

    /// A second `BranchPanel` over the same workspace `panel_a` is already on.
    ///
    /// Built rather than opening a second window through `panel(cx)` again:
    /// that helper reruns `init_test`, which resets globals (the settings
    /// store, `AppState`, `crate::init`) that must run exactly once per test.
    /// Two panels sharing one `CheckoutViewState` is the property under test,
    /// and `BranchPanel::new` gets that from the global unconditionally --
    /// sharing the workspace too is incidental, not what T1/T6 assert.
    fn second_panel_over_the_same_workspace(
        panel_a: &Entity<BranchPanel>,
        cx: &mut VisualTestContext,
    ) -> Entity<BranchPanel> {
        let workspace = panel_a
            .read_with(cx, |panel, _| panel.workspace.clone())
            .upgrade()
            .expect("the first panel's workspace is still alive");
        workspace.update_in(cx, |workspace, window, cx| {
            BranchPanel::new(workspace, window, cx)
        })
    }

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
        let checkout = Arc::from(Path::new("/repos/zode/wt"));

        panel.update(cx, |panel, cx| {
            panel.repos = vec![repo_data(id)];

            assert!(
                panel.row_is_open(&RowKey::Repo(id), cx),
                "a repository with nothing recorded against it must draw open"
            );
            assert!(
                !panel.row_is_open(&RowKey::WorktreeAgents(id, checkout), cx),
                "a checkout's agents must still stay shut until asked for"
            );
        });
    }

    /// **T1.** A second panel over the same checkout -- a different
    /// `RepositoryId`, the same path, exactly what a checkout switch produces
    /// (see `reports/study-corrections.md` § 1) -- must see what the first
    /// one opened.
    ///
    /// Falsifiable today by the two-set design this replaces: each panel held
    /// its own `expanded: HashSet<RowKey>`, keyed by the session-local id, and
    /// even the persisted half was read from a workspace-scoped KVP key, so a
    /// second panel over the same path had nothing in common with the first
    /// at all. `CheckoutViewState` is one process-global entity, so there is
    /// only one record for both panels to read.
    #[gpui::test]
    async fn a_second_panel_sees_what_the_first_opened(cx: &mut TestAppContext) {
        let (panel_a, cx) = panel(cx).await;
        let panel_b = second_panel_over_the_same_workspace(&panel_a, cx);

        let id_a = RepositoryId(1);
        let id_b = RepositoryId(2);
        let checkout: Arc<Path> = Arc::from(Path::new("/repos/zode/wt"));

        panel_a.update(cx, |panel, _| panel.repos = vec![repo_data(id_a)]);
        panel_b.update(cx, |panel, _| panel.repos = vec![repo_data(id_b)]);

        panel_a.update(cx, |panel, cx| {
            panel.toggle_row(RowKey::WorktreeAgents(id_a, Arc::clone(&checkout)), cx);
        });

        panel_b.read_with(cx, |panel, cx| {
            assert!(
                panel.row_is_open(&RowKey::WorktreeAgents(id_b, Arc::clone(&checkout)), cx),
                "a second panel over the same checkout, under a different \
                 RepositoryId, must see what the first one opened"
            );
        });
    }

    /// A blob from before repositories recorded their closure lists the ones
    /// that were open. Those entries govern nothing now: `is_open` for a
    /// `Repo` key never consults `expanded`, only `collapsed` -- a fact
    /// pinned directly in `checkout_state`'s own
    /// `a_stray_repo_entry_in_legacy_expanded_is_dropped`. This is the
    /// panel-level half: it must still draw open with such an entry seeded.
    #[gpui::test]
    async fn a_repository_entry_from_the_old_format_is_not_kept(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let id = RepositoryId(1);

        let state = panel.read_with(cx, |panel, _| panel.checkout_state.clone());
        state.update(cx, |state, cx| {
            state.seed_from_legacy(
                crate::branch_panel::state::SerializedBranchPanel {
                    expanded: [StoredKey::Repo(REPO_PATH.to_string())].into_iter().collect(),
                    collapsed: Default::default(),
                    pinned: Vec::new(),
                    order: Vec::new(),
                },
                cx,
            );
        });

        panel.update(cx, |panel, cx| {
            panel.repos = vec![repo_data(id)];
            assert!(
                panel.row_is_open(&RowKey::Repo(id), cx),
                "a repository entry stray in the opened set must not close it -- \
                 dropping it leaves the repository at its default, which is open"
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
                !panel.row_is_open(&key, cx),
                "the gesture has to close a repository that was open"
            );

            panel.toggle_row(key.clone(), cx);
            assert!(
                panel.row_is_open(&key, cx),
                "and open it again, rather than the set filling up one way"
            );

            panel.toggle_row(key.clone(), cx);
            assert!(
                !panel.row_is_open(&key, cx),
                "closed once more, ready for the restore below"
            );
        });

        // What a restart hands back: a brand new `CheckoutViewState`, reading
        // the same on-disk record rather than the cached global -- the half a
        // single, un-consumed set makes possible. `panel` is left alive on
        // purpose: the point is that *this* reader, which never touched the
        // toggle above, still gets the closed answer. The write is throttled
        // 500ms behind the toggle, so the clock has to be advanced before a
        // fresh reader can see it land.
        cx.background_executor.advance_clock(Duration::from_millis(600));
        cx.run_until_parked();

        let fresh = cx.new(|_| CheckoutViewState::new());
        fresh.update(cx, |state, cx| state.load(cx)).await;
        fresh.read_with(cx, |state, _| {
            assert!(
                !state.is_open(&StoredKey::Repo(REPO_PATH.to_string())),
                "a repository the reader closed must come back closed on a fresh read"
            );
        });
    }

    /// **T2.** A restored-open row can be closed, and the closure survives
    /// both a rebuild *and* a completely independent read of the record --
    /// the guarantee the old consume-on-adopt trick protected by mutating a
    /// copy of `stored_expanded` on every build (see
    /// `lifecycle.rs:349-357`'s doc, before this phase, for the reasoning).
    ///
    /// With one path-keyed set there is nothing left to consume: `toggle`
    /// removes the key from the only set that exists, the next rebuild asks
    /// that same set, and the next reader off disk gets the same set too. If
    /// a second set is ever reintroduced, this is where it would show: the
    /// live toggle would land in the new set while the persisted copy still
    /// said open, and the fresh reader below would get the stale answer.
    #[gpui::test]
    async fn a_restored_open_row_can_be_closed_and_stays_closed(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let id = RepositoryId(1);
        let checkout: Arc<Path> = Arc::from(Path::new("/repos/zode/wt"));
        let key = RowKey::WorktreeAgents(id, Arc::clone(&checkout));
        let stored = StoredKey::WorktreeAgents(REPO_PATH.to_string(), "/repos/zode/wt".to_string());

        // Seeded open, the way a restart would find it.
        let state = panel.read_with(cx, |panel, _| panel.checkout_state.clone());
        state.update(cx, |state, cx| state.toggle(stored.clone(), cx));

        // Prove the record is genuinely reaching disk BEFORE trusting what its
        // absence means further down. `is_open` answers `false` for an agent
        // list that was never recorded just as readily as for one that was
        // recorded closed -- so the closing assertion at the end of this test
        // passes just as well against a `toggle` that writes nothing at all.
        // This positive read is the half that can tell those apart.
        cx.background_executor.advance_clock(Duration::from_millis(600));
        cx.run_until_parked();
        let seeded = cx.new(|_| CheckoutViewState::new());
        seeded.update(cx, |state, cx| state.load(cx)).await;
        seeded.read_with(cx, |state, _| {
            assert!(
                state.is_open(&stored),
                "the open state must survive a fresh read, or nothing below \
                 this line is measuring persistence at all"
            );
        });

        panel.update(cx, |panel, cx| {
            panel.repos = vec![repo_data(id)];
            assert!(
                panel.row_is_open(&key, cx),
                "seeded open must draw open on the first build after a restore"
            );

            panel.toggle_row(key.clone(), cx);
            assert!(!panel.row_is_open(&key, cx), "the gesture has to close it");

            // Rebuild: the old bug re-adopted the stored entry on every
            // rebuild, and since closing a row is itself a rebuild, the row
            // reopened the instant it closed.
            panel.repos = vec![repo_data(id)];
            assert!(
                !panel.row_is_open(&key, cx),
                "a rebuild must not reopen what was just closed"
            );
        });

        // The write is throttled 500ms behind the toggle; let it land before
        // a fresh reader asks for the record.
        cx.background_executor.advance_clock(Duration::from_millis(600));
        cx.run_until_parked();

        let fresh = cx.new(|_| CheckoutViewState::new());
        fresh.update(cx, |state, cx| state.load(cx)).await;
        fresh.read_with(cx, |state, _| {
            assert!(
                !state.is_open(&stored),
                "a fresh, independent read of the record must still show it closed -- \
                 this is the half a second set would fail"
            );
        });
    }

    #[gpui::test]
    async fn a_collapsed_section_stays_collapsed(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let id = RepositoryId(1);
        let checkout = Arc::from(Path::new("/repos/zode/wt"));
        let key = RowKey::WorktreeAgents(id, Arc::clone(&checkout));

        let state = panel.read_with(cx, |panel, _| panel.checkout_state.clone());
        state.update(cx, |state, cx| {
            state.toggle(
                StoredKey::WorktreeAgents(REPO_PATH.to_string(), "/repos/zode/wt".to_string()),
                cx,
            );
        });

        panel.update(cx, |panel, cx| {
            panel.repos = vec![repo_data(id)];
            assert!(
                panel.row_is_open(&key, cx),
                "a stored checkout must open on the first build after it is restored"
            );

            panel.toggle_row(key.clone(), cx);
            assert!(
                !panel.row_is_open(&key, cx),
                "a card the user closed must not be reopened by the restored state"
            );
        });
    }

    /// **T6.** A pin set in one panel is visible in another -- the deliberate
    /// scope decision recorded as R5 in phase 2's plan: one record, one key,
    /// one write path, made falsifiable rather than assumed.
    #[gpui::test]
    async fn a_pin_set_in_one_panel_is_visible_in_another(cx: &mut TestAppContext) {
        let (panel_a, cx) = panel(cx).await;
        let panel_b = second_panel_over_the_same_workspace(&panel_a, cx);
        let path = PathBuf::from("/repos/zode/wt");

        panel_a.update(cx, |panel, cx| panel.toggle_pinned(&path, cx));

        panel_b.read_with(cx, |panel, cx| {
            assert!(
                panel.pinned(cx).contains(&path),
                "a pin set through one panel must be visible through another -- \
                 they share the one record"
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

/// Tests for `hold_known_agents` -- Defect 2: a checkout's agent list blinking
/// empty for a moment that says nothing about the checkout itself.
///
/// T16 and T20 drive the real workspace (an actual agent tab, actually
/// closed), because that is the case the phase exists for and the one whose
/// timing (`is_scanning()` already true the instant the tab closes) is worth
/// exercising for real. T17-T19 build `RepoData` fixtures directly and call
/// `hold_known_agents` on them, the same way `restoring_expansion` above
/// builds `RepoData` fixtures for `toggle_row`/`row_is_open` -- what is under
/// test there is the three-case rule and the eviction pass, not `collect_repos`
/// or the workspace, and driving those from hand-built input keeps each test
/// pinned to exactly the transition it names instead of however the test's own
/// git/session-store setup happens to shake out.
mod hold_known_agents {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::SystemTime;

    use git::repository::Worktree as GitWorktree;
    use gpui::{TestAppContext, VisualTestContext};
    use project::git_store::RepositoryId;
    use workspace::dock::Panel as _;

    use crate::branch_panel::panel::BranchPanel;
    use crate::branch_panel::tree::{AgentEntry, RepoData, RowKey, TreeRow};

    use super::{panel, panel_over_a_repo};

    const REPO_PATH: &str = "/repos/zode";

    fn worktree(path: &str) -> GitWorktree {
        GitWorktree {
            path: PathBuf::from(path),
            ref_name: Some("refs/heads/main".into()),
            sha: "abc123".into(),
            is_main: path == REPO_PATH,
            is_bare: false,
        }
    }

    fn agent_entry(label: &str) -> AgentEntry {
        AgentEntry::Past {
            label: label.to_string().into(),
            agent: "claude-acp".into(),
            id: Arc::from(label),
            updated_at: SystemTime::UNIX_EPOCH,
        }
    }

    fn entry_label(entry: &AgentEntry) -> String {
        match entry {
            AgentEntry::Past { label, .. } | AgentEntry::Open { label, .. } => label.to_string(),
        }
    }

    fn repo_data(id: RepositoryId, worktrees: Vec<GitWorktree>) -> RepoData {
        repo_data_at(id, REPO_PATH, worktrees)
    }

    /// A second repository, so a test can say something about one project that
    /// is not also true of every project at once.
    fn repo_data_at(id: RepositoryId, path: &str, worktrees: Vec<GitWorktree>) -> RepoData {
        RepoData {
            id,
            path: Arc::from(Path::new(path)),
            name: path
                .rsplit('/')
                .next()
                .unwrap_or(path)
                .to_string()
                .into(),
            current_branch: Some("main".into()),
            branches: Vec::new(),
            agents: Default::default(),
            worktrees: Arc::from(worktrees),
        }
    }

    /// The one checkout row the fixtures above (and `panel_over_a_repo`) put
    /// on screen, resolved from a live panel's actual rows -- the same shape
    /// `tree::tests`' own `agents_of` reads, but off a rendered panel instead
    /// of a hand-built one.
    fn agents_of(panel: &BranchPanel) -> (Arc<[AgentEntry]>, bool) {
        panel
            .rows
            .iter()
            .find_map(|row| match row {
                TreeRow::Worktree {
                    agents, expanded, ..
                } => Some((agents.clone(), *expanded)),
                _ => None,
            })
            .expect("the one checkout under test is listed")
    }

    /// The same question as `agents_of`, for T17-T19's hand-built fixtures:
    /// those tests call `hold_known_agents` directly and never run
    /// `build_rows`, so `panel.rows` stays empty and the answer has to come
    /// from `panel.repos` -- the exact map `hold_known_agents` writes to --
    /// instead.
    fn agents_in_repos(panel: &BranchPanel, path: &Path) -> Arc<[AgentEntry]> {
        panel.repos[0]
            .agents
            .get(path)
            .cloned()
            .unwrap_or_else(|| Arc::from([]))
    }

    /// Opens an agent tab against `panel_over_a_repo`'s own checkout and
    /// rebuilds so the row shows it as an open tab. The session store is
    /// created and its initial sweep already settled by the time this
    /// returns -- the state a real `render` would have reached before any tab
    /// could close.
    async fn panel_with_open_agent_tab(
        cx: &mut TestAppContext,
    ) -> (
        gpui::Entity<BranchPanel>,
        gpui::Entity<workspace::pane::Pane>,
        gpui::EntityId,
        &mut VisualTestContext,
    ) {
        let (panel, cx) = panel_over_a_repo(cx).await;
        let workspace = panel
            .read_with(cx, |panel, _| panel.workspace.clone())
            .upgrade()
            .expect("workspace still alive");

        panel.update_in(cx, |panel, window, cx| {
            panel.set_active(true, window, cx);
            panel.ensure_session_store(cx);
        });
        cx.run_until_parked();
        panel.update_in(cx, |panel, _, cx| panel.refresh_if_stale(cx));

        workspace.update_in(cx, |workspace, window, cx| {
            agent_ui::AgentView::open_tracked(
                workspace,
                project::CLAUDE_CODE_AGENT_ID,
                Default::default(),
                window,
                cx,
            );
        });
        cx.run_until_parked();
        panel.update_in(cx, |panel, _, cx| panel.refresh_if_stale(cx));

        let pane = workspace.read_with(cx, |workspace, _| workspace.active_pane().clone());
        let item_id = pane.read_with(cx, |pane, _| {
            pane.items()
                .next()
                .expect("the agent tab was just opened")
                .item_id()
        });

        (panel, pane, item_id, cx)
    }

    /// **T16.** The control survives the gap: an agent tab's session is not on
    /// disk the instant its tab closes, so without the hold the very next
    /// rebuild finds nothing for that checkout at all.
    ///
    /// Fails against `HEAD` today: `agents_by_checkout` returns nothing for
    /// the closed tab's path once no source is holding it, `tree/build.rs:79`
    /// sets `expanded: false` because `has_agents` is false, and
    /// `render_tree/agent.rs:31` (`render_agents`) returns `None` -- the
    /// checkout's agent list, and its disclosure control, disappears rather
    /// than reading as closed.
    #[gpui::test]
    async fn the_control_survives_the_gap(cx: &mut TestAppContext) {
        let (panel, pane, item_id, cx) = panel_with_open_agent_tab(cx).await;

        let (before, _) = panel.read_with(cx, |panel, _| agents_of(panel));
        assert_eq!(before.len(), 1, "the open tab is listed before it closes");

        pane.update_in(cx, |pane, window, cx| {
            pane.remove_item(item_id, true, true, window, cx);
        });
        // Deliberately not parked yet, the same reason T12 in `data.rs` is
        // not: `observe_agent_tabs` asks the session index to resweep on this
        // close, and the window this test exists to catch is the one between
        // that resweep starting and it landing. Parking first would let the
        // sweep of an index that never had this session on disk settle before
        // the rebuild below ever saw it in flight.
        panel.update_in(cx, |panel, _, cx| panel.refresh_if_stale(cx));

        panel.read_with(cx, |panel, cx| {
            assert!(
                panel
                    .session_store
                    .as_ref()
                    .expect("ensure_session_store already ran")
                    .read(cx)
                    .is_scanning(),
                "the resweep must still be in flight, or this is not testing the hold at all"
            );
        });

        let (after, _) = panel.read_with(cx, |panel, _| agents_of(panel));
        assert_eq!(
            after.len(),
            1,
            "a sweep in flight must not be believed when it says nothing"
        );

        cx.run_until_parked();
    }

    /// **T20.** The expanded flag still follows the held list: a row the
    /// reader opened must not draw closed just because its source blinked.
    ///
    /// Fails against `HEAD` for the same reason as T16: with the list empty,
    /// `has_agents` in `tree/build.rs:74` is false, so
    /// `expanded: has_agents && expanded(..)` is false whatever
    /// `CheckoutViewState` recorded.
    #[gpui::test]
    async fn the_expanded_flag_still_follows_the_held_list(cx: &mut TestAppContext) {
        let (panel, pane, item_id, cx) = panel_with_open_agent_tab(cx).await;

        let key = panel.read_with(cx, |panel, _| {
            let repo = &panel.repos[0];
            RowKey::WorktreeAgents(repo.id, Arc::from(repo.worktrees[0].path.as_path()))
        });
        panel.update(cx, |panel, cx| panel.toggle_row(key, cx));
        panel.update_in(cx, |panel, _, cx| panel.refresh_if_stale(cx));

        let (_, expanded_before) = panel.read_with(cx, |panel, _| agents_of(panel));
        assert!(expanded_before, "the row was just opened");

        pane.update_in(cx, |pane, window, cx| {
            pane.remove_item(item_id, true, true, window, cx);
        });
        panel.update_in(cx, |panel, _, cx| panel.refresh_if_stale(cx));

        let (agents, expanded) = panel.read_with(cx, |panel, _| agents_of(panel));
        assert_eq!(
            agents.len(),
            1,
            "the held list must still be there for the row to draw open over"
        );
        assert!(
            expanded,
            "a row the reader opened must not close just because the source blinked"
        );

        cx.run_until_parked();
    }

    /// **T17.** A checkout that genuinely has nothing keeps nothing -- the
    /// counterweight that stops T16's fix from holding forever.
    ///
    /// Seeds a real hold (a rebuild that actually saw an agent), then rebuilds
    /// twice more with nothing to show and the index already settled
    /// (`is_scanning() == false` throughout, so `settling` is false on both
    /// calls): once to retire the hold, once to confirm it stays retired.
    /// Green both before this phase (nothing is ever held) and after
    /// (something is held, and then correctly let go) -- see the falsification
    /// note below for the version of this that is green only by accident.
    #[gpui::test]
    async fn a_settled_sweep_is_believed(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let id = RepositoryId(1);
        let path: Arc<Path> = Arc::from(Path::new(REPO_PATH));

        panel.update(cx, |panel, cx| panel.ensure_session_store(cx));
        cx.run_until_parked();
        panel.read_with(cx, |panel, cx| {
            assert!(
                !panel.index_is_settling(cx),
                "the initial sweep must have already settled, or every assertion below \
                 would pass with `settling` true throughout instead of false"
            );
        });

        // A rebuild that actually saw an agent: this is what puts something in
        // `last_known_agents` to retire.
        panel.update(cx, |panel, cx| {
            let mut repo = repo_data(id, vec![worktree(REPO_PATH)]);
            repo.agents
                .insert(path.clone(), Arc::from([agent_entry("A")]));
            panel.repos = vec![repo];
            panel.hold_known_agents(cx);
        });
        panel.read_with(cx, |panel, _| {
            assert!(
                panel.last_known_agents.contains_key(&path),
                "a rebuild that saw an agent must record it"
            );
        });

        // Now the checkout has nothing, and the index is not mid-sweep.
        panel.update(cx, |panel, cx| {
            panel.repos = vec![repo_data(id, vec![worktree(REPO_PATH)])];
            panel.hold_known_agents(cx);
        });
        panel.read_with(cx, |panel, _| {
            let agents = agents_in_repos(panel, &path);
            assert!(
                agents.is_empty(),
                "once the index has genuinely settled with nothing, the empty answer \
                 must be believed rather than papered over"
            );
            assert!(
                !panel.last_known_agents.contains_key(&path),
                "a settled empty answer must retire the hold, not just decline to show it"
            );
        });
    }

    /// **T18.** A held list is replaced, not merged, when a fresher non-empty
    /// answer arrives -- and the replacement is what a later restore hands
    /// back, not the union of everything ever seen.
    ///
    /// Falsifiable by extending instead of replacing in the non-empty branch
    /// of `hold_known_agents` (`self.last_known_agents.entry(path).or_default()
    /// .extend(...)` instead of `.insert(path, agents.clone())`): performed
    /// below, see the assertion message for the exact failure.
    #[gpui::test]
    async fn a_held_list_is_replaced_not_merged(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let id = RepositoryId(1);
        let path: Arc<Path> = Arc::from(Path::new(REPO_PATH));

        panel.update(cx, |panel, cx| panel.ensure_session_store(cx));
        cx.run_until_parked();

        // First non-empty answer: "A" becomes the hold.
        panel.update(cx, |panel, cx| {
            let mut repo = repo_data(id, vec![worktree(REPO_PATH)]);
            repo.agents
                .insert(path.clone(), Arc::from([agent_entry("A")]));
            panel.repos = vec![repo];
            panel.hold_known_agents(cx);
        });

        // A second, different non-empty answer must replace it, not sit
        // beside it.
        panel.update(cx, |panel, cx| {
            let mut repo = repo_data(id, vec![worktree(REPO_PATH)]);
            repo.agents
                .insert(path.clone(), Arc::from([agent_entry("B")]));
            panel.repos = vec![repo];
            panel.hold_known_agents(cx);
        });

        // Force the restore branch (checkout empty, sweep in flight) to prove
        // which list is actually held -- a rendered non-empty answer proves
        // nothing about what got cached alongside it.
        panel.update(cx, |panel, cx| {
            panel
                .session_store
                .clone()
                .expect("ensure_session_store already ran")
                .update(cx, |store, cx| store.refresh(cx));
            assert!(
                panel.index_is_settling(cx),
                "the sweep must be in flight for this rebuild to take the restore branch"
            );
            panel.repos = vec![repo_data(id, vec![worktree(REPO_PATH)])];
            panel.hold_known_agents(cx);
        });

        panel.read_with(cx, |panel, _| {
            let agents = agents_in_repos(panel, &path);
            assert_eq!(
                agents.len(),
                1,
                "a merge would restore both \"A\" and \"B\"; a replace restores exactly \
                 the fresher one"
            );
            assert_eq!(
                entry_label(&agents[0]),
                "B",
                "the restored entry must be the replacement, not the one it replaced"
            );
        });

        cx.run_until_parked();
    }

    /// **T19.** A held entry is evicted once its checkout stops being listed,
    /// bounded by the checkouts actually on screen (CLAUDE.md: no unbounded
    /// caches).
    ///
    /// Two checkouts so the eviction pass's own warm-up guard -- skip while
    /// any repository lists no checkouts at all -- does not itself suppress
    /// eviction: a single-checkout repository going to zero worktrees would
    /// look exactly like a git store still warming up.
    ///
    /// Falsifiable by dropping the eviction pass entirely: performed below,
    /// see the assertion message for the exact failure.
    #[gpui::test]
    #[gpui::test]
    async fn a_held_entry_is_evicted_once_its_checkout_stops_being_listed(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let id = RepositoryId(1);
        let removed_path: Arc<Path> = Arc::from(Path::new("/repos/zode/wt-a"));

        panel.update(cx, |panel, cx| panel.ensure_session_store(cx));
        cx.run_until_parked();

        panel.update(cx, |panel, cx| {
            let mut repo = repo_data(id, vec![worktree(REPO_PATH), worktree("/repos/zode/wt-a")]);
            repo.agents
                .insert(removed_path.clone(), Arc::from([agent_entry("A")]));
            panel.repos = vec![repo];
            panel.hold_known_agents(cx);
        });
        panel.read_with(cx, |panel, _| {
            assert!(
                panel.last_known_agents.contains_key(&removed_path),
                "the hold must be recorded while the checkout is still listed"
            );
        });

        // The second worktree is gone from this rebuild's list -- as it would
        // be once it is removed on disk -- while the repository still lists
        // its other checkout, so the warm-up guard does not apply.
        panel.update(cx, |panel, cx| {
            panel.repos = vec![repo_data(id, vec![worktree(REPO_PATH)])];
            panel.hold_known_agents(cx);
        });
        panel.read_with(cx, |panel, _| {
            assert!(
                !panel.last_known_agents.contains_key(&removed_path),
                "a checkout that is no longer listed must not keep its hold forever"
            );
        });
    }

    /// One project still being read must not hold every other project's
    /// entries alive.
    ///
    /// The warm-up guard spares a repository whose checkouts have not come back
    /// from git yet — but it has to spare only that repository. Applied as a
    /// blanket "skip eviction while any repo is warming", a single project
    /// sitting empty keeps every other project's holds forever, which is how a
    /// bounded cache stops being bounded. T19 cannot see the difference: it has
    /// one repository, so both readings behave identically.
    #[gpui::test]
    async fn a_warming_repository_does_not_hold_another_projects_entries(
        cx: &mut TestAppContext,
    ) {
        let (panel, cx) = panel(cx).await;
        let settled = RepositoryId(1);
        let warming = RepositoryId(2);
        let removed: Arc<Path> = Arc::from(Path::new("/repos/zode/wt-a"));
        let warming_checkout: Arc<Path> = Arc::from(Path::new("/repos/other/wt"));

        panel.update(cx, |panel, cx| panel.ensure_session_store(cx));
        cx.run_until_parked();

        // Both repositories listing their checkouts, both holding something.
        panel.update(cx, |panel, cx| {
            let mut first = repo_data(settled, vec![worktree(REPO_PATH), worktree("/repos/zode/wt-a")]);
            first
                .agents
                .insert(removed.clone(), Arc::from([agent_entry("A")]));
            let mut second = repo_data_at(warming, "/repos/other", vec![worktree("/repos/other/wt")]);
            second
                .agents
                .insert(warming_checkout.clone(), Arc::from([agent_entry("B")]));
            panel.repos = vec![first, second];
            panel.hold_known_agents(cx);
        });

        // Now the second repository comes back with no checkouts at all — it is
        // warming up — while the first has genuinely lost one.
        panel.update(cx, |panel, cx| {
            panel.repos = vec![
                repo_data(settled, vec![worktree(REPO_PATH)]),
                repo_data_at(warming, "/repos/other", vec![]),
            ];
            panel.hold_known_agents(cx);
        });

        panel.read_with(cx, |panel, _| {
            assert!(
                panel.last_known_agents.contains_key(&warming_checkout),
                "the warming repository's own hold must survive: its checkouts \
                 are missing because git has not answered yet, not because they are gone"
            );
            assert!(
                !panel.last_known_agents.contains_key(&removed),
                "but a settled repository's removed checkout must still be evicted -- \
                 one project warming up cannot keep another project's entries alive"
            );
        });
    }
}
