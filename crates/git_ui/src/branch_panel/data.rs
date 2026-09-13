//! Reading the repositories out of the git store.
//!
//! Nothing here runs a git command. `RepositorySnapshot` already carries
//! `branch_list`, `linked_worktrees`, `stash_entries` and the current `branch`,
//! and the store keeps them current and announces every change. The panel is a
//! pure reader on top of that -- which is why a closed panel costs nothing at
//! all, and why two panels open at once do not double the git work.

use gpui::{App, Context, Entity, SharedString, Subscription};
use project::git_store::{GitStore, GitStoreEvent, RepositoryEvent};

use crate::branch_panel::panel::BranchPanel;
use std::path::Path;
use std::sync::Arc;

use crate::branch_panel::tree::{AgentEntry, RepoData, TreeRow};
use crate::branch_service::process_branches;
use workspace::Workspace;

/// The repository events that can change what the tree shows.
///
/// `StatusesChanged` is deliberately absent: it fires on every keystroke that
/// dirties a file and says nothing about branches, so reacting to it would
/// rebuild the tree continuously while the user types.
fn event_changes_the_tree(event: &RepositoryEvent) -> bool {
    matches!(
        event,
        RepositoryEvent::BranchListChanged
            | RepositoryEvent::HeadChanged
            | RepositoryEvent::StashEntriesChanged
            | RepositoryEvent::GitWorktreeListChanged
    )
}

impl BranchPanel {
    pub(crate) fn git_store(&self, cx: &App) -> Option<Entity<GitStore>> {
        let workspace = self.workspace.upgrade()?;
        Some(workspace.read(cx).project().read(cx).git_store().clone())
    }

    /// Subscribes to the one event stream that matters. Held in the panel's own
    /// `_subscriptions`, so it dies with the panel rather than firing into a
    /// dropped handle forever after.
    pub(crate) fn observe_git_store(
        cx: &mut Context<Self>,
        store: &Entity<GitStore>,
    ) -> Subscription {
        cx.subscribe(store, |panel, _, event, cx| match event {
            GitStoreEvent::RepositoryUpdated(_, repository_event, _) => {
                if event_changes_the_tree(repository_event) {
                    panel.mark_stale(cx);
                }
            }
            GitStoreEvent::RepositoryAdded | GitStoreEvent::RepositoryRemoved(_) => {
                panel.mark_stale(cx);
            }
            _ => {}
        })
    }

    /// Marks the tree stale when an agent tab opens or closes.
    ///
    /// Without this the panel only learned about agents when something *else*
    /// rebuilt it -- a git event, or switching checkouts, which builds a whole
    /// new panel. So pressing New Agent on the rail or in the editor added
    /// nothing visible, and the list only caught up once you switched away and
    /// back. That is not a refresh; that is a new panel.
    ///
    /// "That is a new panel" turned out to be the diagnosis of a second defect
    /// as well, not just an aside: the new panel used to read a record keyed to
    /// the workspace it was built in, so every switch found a different one and
    /// every agent list came back shut. `CheckoutViewState` is keyed by path
    /// instead, so the second panel reads what the first wrote.
    ///
    /// `ItemAdded` is filtered to agent tabs, because opening a file is not
    /// news here. `ItemRemoved` carries only an id, so it cannot be filtered
    /// the same way -- it is acted on only while the panel is actually showing
    /// an open tab, which is the only case where a close could change anything.
    ///
    /// A closing agent tab additionally asks the session index to re-read
    /// disk. `self.rows` still holds the count from *before* this removal --
    /// rows are rebuilt at render (`render.rs`), not on this event -- so
    /// comparing it against the workspace's live `AgentView` count right now
    /// tells us an agent tab (rather than a file tab) just closed, with no
    /// type on the event to filter by. The finished session's transcript
    /// lands on disk the moment the tab closes, but the process-global index
    /// is only as fresh as its last sweep and may still be missing it; without
    /// this, the row for that session would not appear until something
    /// unrelated happened to trigger a sweep.
    pub(crate) fn observe_agent_tabs(
        cx: &mut Context<Self>,
        workspace: &Entity<Workspace>,
    ) -> Subscription {
        cx.subscribe(workspace, |panel, workspace, event, cx| match event {
            workspace::Event::ItemAdded { item } => {
                if item.downcast::<agent_ui::AgentView>().is_some() {
                    panel.mark_stale(cx);
                }
            }
            workspace::Event::ItemRemoved { .. } if panel.lists_an_open_tab() => {
                panel.mark_stale(cx);

                let live_agents = workspace
                    .read(cx)
                    .items_of_type::<agent_ui::AgentView>(cx)
                    .count();
                // Only a decrease is news. Closing a file tab passes
                // `lists_an_open_tab` too (an agent tab can sit open in the
                // same panel), and firing a sweep -- thousands of transcripts
                // read -- on every such close would be the exact cost this
                // comparison exists to avoid.
                if live_agents < panel.open_agents_listed()
                    && let Some(store) = panel.session_store.clone()
                {
                    // `SessionStore::refresh` coalesces a call that arrives
                    // mid-sweep into one more sweep rather than queuing it, so
                    // closing several agent tabs at once still costs at most
                    // two. Read from the field rather than
                    // `agent_ui::SessionStore::global`: a panel that has never
                    // been drawn has no store to ask, and must not create one
                    // just because a tab closed somewhere else in the window.
                    store.update(cx, |store, cx| store.refresh(cx));
                }
            }
            _ => {}
        })
    }

    /// Whether any row currently shows a tab, rather than only transcripts.
    fn lists_an_open_tab(&self) -> bool {
        self.rows.iter().any(|row| match row {
            TreeRow::Worktree { agents, .. } => agents.iter().any(|agent| agent.is_open()),
            _ => false,
        })
    }

    /// How many rows the last rebuild drew as an open tab, across every
    /// checkout. The *before* half of the `ItemRemoved` comparison above.
    fn open_agents_listed(&self) -> usize {
        self.rows
            .iter()
            .map(|row| match row {
                TreeRow::Worktree { agents, .. } => {
                    agents.iter().filter(|agent| agent.is_open()).count()
                }
                _ => 0,
            })
            .sum()
    }

    /// Re-reads every repository snapshot into the shape the tree wants.
    ///
    /// Cheap by construction -- it clones already-materialised `Arc`s and runs
    /// the branch list through the same folding the picker uses. Called only
    /// when the panel is visible and something actually changed.
    pub(crate) fn collect_repos(&self, cx: &App) -> Vec<RepoData> {
        let Some(store) = self.git_store(cx) else {
            return Vec::new();
        };

        // Snapshotted before the closure below rather than read from
        // `checkout_state` inside it: the closure already borrows `self`
        // (through `self.agents_by_checkout`), and pulling the entity read out
        // to a local here is what the phase's borrow-shape note asks for.
        let pinned = self.pinned(cx);
        let manual_order = self.manual_order(cx);

        let mut repos: Vec<RepoData> = store
            .read(cx)
            .repositories()
            .values()
            .map(|repo| {
                let repo = repo.read(cx);
                let checkouts = crate::branch_panel::tree::order_checkouts(
                    crate::branch_panel::tree::all_checkouts(
                        repo.work_directory_abs_path.as_ref(),
                        repo.branch.as_ref(),
                        repo.head_commit.as_ref().map(|commit| commit.sha.clone()),
                        &repo.linked_worktrees,
                    ),
                    &pinned,
                    &manual_order,
                );
                RepoData {
                    id: repo.id,
                    path: repo.work_directory_abs_path.clone(),
                    name: repo_name(repo.work_directory_abs_path.as_ref()),
                    current_branch: repo
                        .branch
                        .as_ref()
                        .map(|branch| SharedString::from(branch.name().to_string())),
                    branches: process_branches(&repo.branch_list),
                    worktrees: std::sync::Arc::from(checkouts.clone()),
                    agents: self.agents_by_checkout(&checkouts, cx),
                }
            })
            .collect();

        // Stable order, or the tree reshuffles itself on every rebuild: the
        // store hands back a HashMap, whose iteration order is arbitrary.
        repos.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.0.cmp(&b.id.0)));
        repos
    }
}

impl BranchPanel {
    /// Which agents belong to which checkout.
    ///
    /// Keyed by the worktree's own path, which is what a checkout actually is:
    /// a worktree created here starts detached and has no branch name, and two
    /// repositories can both have a `main`.
    ///
    /// Two sources, gathered once per rebuild so `build_rows` stays pure:
    ///
    /// - **Open**: every agent tab of this window. They belong to the checkout
    ///   this workspace has open, so opening one from the editor's own New
    ///   Agent button lands under the checkout you are in -- from the moment it
    ///   opens, not from the moment its CLI answers. Whether it is working is
    ///   the mark's business, not this list's.
    /// - **Finished**: from the shared session index, one hash per checkout.
    fn agents_by_checkout(
        &self,
        worktrees: &[git::repository::Worktree],
        cx: &App,
    ) -> collections::HashMap<Arc<Path>, Arc<[AgentEntry]>> {
        let mut by_path: collections::HashMap<Arc<Path>, Vec<AgentEntry>> = Default::default();
        // Sessions an open tab is already showing. Their transcripts are on
        // disk and the index finds them, so without this the same session is
        // listed twice -- once as the tab you are looking at, once as a
        // finished one you could resume into a second copy of itself.
        let mut open_sessions: collections::HashSet<String> = Default::default();

        if let Some(workspace) = self.workspace.upgrade() {
            // The checkout this window is in: the worktree whose path a root of
            // the workspace sits under. A running agent has no other checkout
            // it could be editing.
            let roots = workspace.read(cx).root_paths(cx);
            let here = worktrees
                .iter()
                .find(|worktree| roots.iter().any(|root| root.starts_with(&worktree.path)));

            if let Some(here) = here {
                let mut open = Vec::new();
                for view in workspace.read(cx).items_of_type::<agent_ui::AgentView>(cx) {
                    let tab = view.read(cx);
                    if let Some(id) = tab.session_id() {
                        open_sessions.insert(id.to_string());
                    }
                    open.push(AgentEntry::Open {
                        label: tab.tab_label(),
                        agent: tab.agent_id().to_string().into(),
                        view: view.downgrade(),
                    });
                }
                if !open.is_empty() {
                    by_path
                        .entry(Arc::from(here.path.as_path()))
                        .or_default()
                        .extend(open);
                }
            }
        }

        let Some(index) = self
            .session_store
            .as_ref()
            .map(|store| store.read(cx).index().clone())
        else {
            return finish(by_path);
        };

        for worktree in worktrees {
            let path: Arc<Path> = Arc::from(worktree.path.as_path());
            let past = index
                .sessions_for(&path)
                .filter(|session| !open_sessions.contains(session.id.as_ref()))
                .map(|session| AgentEntry::Past {
                    label: session.title.clone().into(),
                    agent: session.agent.builtin_agent_id().into(),
                    id: session.id.clone(),
                    updated_at: session.updated_at,
                });
            let past: Vec<_> = past.collect();
            if !past.is_empty() {
                by_path.entry(path).or_default().extend(past);
            }
        }

        finish(by_path)
    }
}

/// Freezes the gathered lists so a branch row can clone an `Arc` rather than a
/// `Vec` on every rebuild.
fn finish(
    by_path: collections::HashMap<Arc<Path>, Vec<AgentEntry>>,
) -> collections::HashMap<Arc<Path>, Arc<[AgentEntry]>> {
    by_path
        .into_iter()
        .map(|(path, entries)| (path, Arc::from(entries)))
        .collect()
}

/// The directory name is what the user calls the repository; the full path is
/// too long for a 360px panel and its tail is the part that identifies it.
fn repo_name(path: &std::path::Path) -> SharedString {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
        .into()
}

/// Tests for the `ItemRemoved` sweep decision.
///
/// Lives here rather than beside `lifecycle`'s existing agent-tab tests
/// (`lifecycle/tests.rs`) because that file belongs to a phase running
/// concurrently with this one; duplicating its small harness setup keeps this
/// phase's file list to `data.rs` alone. The setup mirrors
/// `lifecycle::tests::panel_over_a_repo` -- a real repository is required so
/// `collect_repos` has a checkout to attribute an open agent tab to.
#[cfg(test)]
mod tests {
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
            // A database of this test's own. Nothing here reads the checkout
            // record today, so without this the tests still pass -- but
            // `AppDatabase::global` falls back to a process-wide static
            // (`db.rs:85-86`), and the record now lives under one un-scoped
            // key, so the first test in this module that ever calls `load`
            // would start sharing a row with every other test in the binary.
            // That failure arrives as a flake, which is the expensive kind to
            // diagnose; one line here is cheaper than the next investigation.
            cx.set_global(db::AppDatabase::test_new());
            let state = AppState::test(cx);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            editor::init(cx);
            crate::init(cx);
            state
        })
    }

    /// A panel over a project that really holds a repository, so an agent tab
    /// opened against it lands under a checkout `collect_repos` finds.
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
        let (workspace, cx) =
            cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
        let panel = workspace.update_in(cx, |workspace, window, cx| {
            BranchPanel::new(workspace, window, cx)
        });
        (panel, cx)
    }

    /// Opens an agent tab and rebuilds the panel so its row shows the tab as
    /// open, then returns the tab's item id and pane for the test to close.
    async fn panel_with_one_open_agent_tab(
        cx: &mut TestAppContext,
    ) -> (
        gpui::Entity<BranchPanel>,
        gpui::Entity<workspace::Workspace>,
        gpui::Entity<workspace::pane::Pane>,
        gpui::EntityId,
        &mut VisualTestContext,
    ) {
        let (panel, cx) = panel_over_a_repo(cx).await;
        let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());
        let workspace = workspace.upgrade().expect("workspace still alive");

        panel.update_in(cx, |panel, window, cx| {
            panel.set_active(true, window, cx);
            panel.refresh_if_stale(cx);
        });

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

        (panel, workspace, pane, item_id, cx)
    }

    /// **T12.** Closing an agent tab must ask the index to re-read disk.
    ///
    /// Falsifiable today: `observe_agent_tabs` calls only `mark_stale` on
    /// `ItemRemoved`, so `is_scanning()` stays `false` after the tab closes.
    #[gpui::test]
    async fn closing_an_agent_tab_asks_the_index_to_resweep(cx: &mut TestAppContext) {
        let (panel, _workspace, pane, item_id, cx) = panel_with_one_open_agent_tab(cx).await;

        panel.update(cx, |panel, cx| panel.ensure_session_store(cx));
        cx.run_until_parked();
        panel.read_with(cx, |panel, cx| {
            let store = panel
                .session_store
                .as_ref()
                .expect("ensure_session_store just created it");
            assert!(
                !store.read(cx).is_scanning(),
                "the initial sweep must have finished settling before the close, \
                 or the assertion below would pass for the wrong reason"
            );
        });

        pane.update_in(cx, |pane, window, cx| {
            pane.remove_item(item_id, true, true, window, cx);
        });
        // Deliberately not parked yet: the sweep is asynchronous
        // (`SessionStore::refresh` spawns onto the background executor), so
        // the moment to see it *started* is right after the synchronous event
        // dispatch, before the executor has a chance to run it to
        // completion and flip `is_scanning()` back to `false`.
        panel.read_with(cx, |panel, cx| {
            let store = panel
                .session_store
                .as_ref()
                .expect("ensure_session_store already ran");
            assert!(
                store.read(cx).is_scanning(),
                "closing the last open agent tab must ask the session index to resweep"
            );
        });

        cx.run_until_parked();
    }

    /// **T13.** Closing an ordinary file tab must not.
    ///
    /// Green today for the wrong reason -- nothing sweeps on any tab close yet.
    /// Falsifiable the moment the count comparison is dropped in favour of
    /// sweeping on every `ItemRemoved`: it would then fail here.
    #[gpui::test]
    async fn closing_a_file_tab_does_not_resweep(cx: &mut TestAppContext) {
        let (panel, workspace, _pane, _item_id, cx) = panel_with_one_open_agent_tab(cx).await;

        panel.update(cx, |panel, cx| panel.ensure_session_store(cx));
        cx.run_until_parked();

        let generation_before = panel.read_with(cx, |panel, cx| {
            panel
                .session_store
                .as_ref()
                .expect("ensure_session_store already ran")
                .read(cx)
                .generation()
        });

        let pane = workspace.read_with(cx, |workspace, _| workspace.active_pane().clone());
        let file_item_id = workspace
            .update_in(cx, |workspace, window, cx| {
                let editor = cx.new(|cx| editor::Editor::single_line(window, cx));
                let item_id = editor.entity_id();
                workspace.add_item_to_active_pane(Box::new(editor), None, true, window, cx);
                item_id
            });
        cx.run_until_parked();

        pane.update_in(cx, |pane, window, cx| {
            pane.remove_item(file_item_id, true, true, window, cx);
        });
        cx.run_until_parked();

        panel.read_with(cx, |panel, cx| {
            let store = panel.session_store.as_ref().unwrap();
            assert!(
                !store.read(cx).is_scanning(),
                "closing a file tab must not cost a sweep"
            );
            assert_eq!(
                store.read(cx).generation(),
                generation_before,
                "no sweep means no new generation either"
            );
        });
    }

    /// **T14.** Closing several agent tabs in one burst costs at most one more
    /// sweep, not one per tab -- `SessionStore::refresh` coalesces a request
    /// that arrives mid-sweep rather than queuing it.
    ///
    /// Falsifiable by calling `refresh` outside the store's own coalescing
    /// (for example, spawning a fresh sweep per closed tab instead of asking
    /// the shared store).
    #[gpui::test]
    async fn closing_several_agent_tabs_at_once_costs_one_more_sweep(cx: &mut TestAppContext) {
        let (panel, workspace, pane, first_item_id, cx) = panel_with_one_open_agent_tab(cx).await;

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

        panel.update(cx, |panel, cx| panel.ensure_session_store(cx));
        cx.run_until_parked();

        let second_item_id = pane.read_with(cx, |pane, _| {
            pane.items()
                .map(|item| item.item_id())
                .find(|id| *id != first_item_id)
                .expect("a second agent tab was just opened")
        });

        let generation_before = panel.read_with(cx, |panel, cx| {
            panel.session_store.as_ref().unwrap().read(cx).generation()
        });

        pane.update_in(cx, |pane, window, cx| {
            pane.remove_item(first_item_id, true, true, window, cx);
            pane.remove_item(second_item_id, true, true, window, cx);
        });
        // Both `ItemRemoved` events are dispatched synchronously before this
        // call returns, so `refresh` is asked for twice before the executor
        // gets a turn: the first call starts a sweep, the second finds one
        // already running and only sets `rescan_requested`
        // (`session_store.rs:94-99`). Settling both takes exactly two
        // completed sweeps, not two-per-tab and not one shared with T12's
        // single-tab case.
        cx.run_until_parked();

        panel.read_with(cx, |panel, cx| {
            let store = panel.session_store.as_ref().unwrap();
            assert!(
                !store.read(cx).is_scanning(),
                "both the running sweep and its coalesced re-run must have settled"
            );
            assert_eq!(
                store.read(cx).generation(),
                generation_before + 2,
                "two refresh() calls arriving before the first sweep finishes must still \
                 produce exactly the running sweep plus one coalesced re-run -- not a \
                 sweep per closed tab"
            );
        });
    }

    /// **T15.** A panel that has never been drawn has no session store to ask,
    /// and must not create one just because a tab closed.
    ///
    /// Falsifiable by reaching for `agent_ui::SessionStore::global` in the
    /// `ItemRemoved` arm instead of reading `self.session_store`.
    #[gpui::test]
    async fn undrawn_panel_does_not_sweep_on_tab_close(cx: &mut TestAppContext) {
        let (panel, _workspace, pane, item_id, cx) = panel_with_one_open_agent_tab(cx).await;

        panel.read_with(cx, |panel, _| {
            assert!(
                panel.session_store.is_none(),
                "ensure_session_store must not have run in this test"
            );
        });

        pane.update_in(cx, |pane, window, cx| {
            pane.remove_item(item_id, true, true, window, cx);
        });
        cx.run_until_parked();

        panel.read_with(cx, |panel, _| {
            assert!(
                panel.session_store.is_none(),
                "a closed tab must not bring a session store into existence"
            );
        });
    }
}
