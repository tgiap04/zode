//! Reading the repositories out of the git store.
//!
//! Nothing here runs a git command. `RepositorySnapshot` already carries
//! `branch_list`, `linked_worktrees`, `stash_entries` and the current `branch`,
//! and the store keeps them current and announces every change. The panel is a
//! pure reader on top of that -- which is why a closed panel costs nothing at
//! all, and why two panels open at once do not double the git work.

use gpui::{App, Context, Entity, EntityId, SharedString, Subscription};
use project::git_store::{GitStore, GitStoreEvent, RepositoryEvent};

use crate::branch_panel::panel::BranchPanel;
use std::path::Path;
use std::sync::Arc;

use crate::branch_panel::tree::{AgentEntry, RepoData};
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

    /// Marks the tree stale when an agent tab opens or closes, and asks the
    /// session index to re-read disk when one closes.
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
    /// `ItemRemoved` carries only an id, so the panel remembers which ids are
    /// agent tabs (`agent_tabs`, seeded in `new` and kept by `ItemAdded`) and
    /// treats a removal of one of them as news. It must not be inferred from
    /// the rows: those are rebuilt only when the panel is drawn, so a tab
    /// opened and closed between two draws is not in them, and the close would
    /// go unnoticed.
    ///
    /// The finished session's transcript lands on disk the moment the tab
    /// closes, but the process-global index is only as fresh as its last sweep
    /// and may still be missing it; without the refresh, its row would not
    /// appear until something unrelated happened to trigger a sweep.
    pub(crate) fn observe_agent_tabs(
        cx: &mut Context<Self>,
        workspace: &Entity<Workspace>,
    ) -> Subscription {
        cx.subscribe(workspace, |panel, workspace, event, cx| match event {
            workspace::Event::ItemAdded { item } => {
                if item.downcast::<agent_ui::AgentView>().is_some() {
                    panel.agent_tabs.insert(item.item_id());
                    panel.mark_stale(cx);
                }
            }
            workspace::Event::ItemRemoved { item_id } => {
                // Moving a tab to another pane removes it from one and adds it
                // to the other, and both events are delivered once the move has
                // finished -- so a tab that is still somewhere in the workspace
                // was moved, not closed.
                if !panel.agent_tabs.contains(item_id) {
                    return;
                }
                let moved = workspace
                    .read(cx)
                    .items(cx)
                    .any(|item| item.item_id() == *item_id);
                if moved {
                    return;
                }
                if panel.agent_tabs.remove(item_id) {
                    panel.mark_stale(cx);
                    panel.refresh_sessions_after_close(cx);
                }
            }
            _ => {}
        })
    }

    /// The agent tabs already open when the panel is built, which no
    /// `ItemAdded` will ever announce.
    pub(crate) fn seed_agent_tabs(
        workspace: &Workspace,
        cx: &App,
    ) -> collections::HashSet<EntityId> {
        workspace
            .items_of_type::<agent_ui::AgentView>(cx)
            .map(|view| view.entity_id())
            .collect()
    }

    /// Asks the index to re-read disk for a closed agent tab.
    fn refresh_sessions_after_close(&mut self, cx: &mut Context<Self>) {
        // The store the panel already holds, else the shared one if somebody
        // created it: another workspace's panel may be waiting on it even
        // though this one was never drawn. `try_global`, not `global` -- a tab
        // closing must not bring a store into existence.
        let Some(store) = self
            .session_store
            .clone()
            .or_else(|| agent_ui::SessionStore::try_global(cx))
        else {
            return;
        };
        // `SessionStore::refresh` coalesces a call that arrives mid-sweep into
        // one more sweep rather than queuing it, so closing several agent tabs
        // at once still costs at most two.
        store.update(cx, |store, cx| store.refresh(cx));
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
                    anchor: repo.original_repo_abs_path.clone(),
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
            let here = checkout_containing(worktrees, &roots);

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

        let names = agent_ui::SessionNames::try_global(cx);
        let names = names.as_ref().map(|names| names.read(cx));
        for worktree in worktrees {
            let path: Arc<Path> = Arc::from(worktree.path.as_path());
            let past = index
                .sessions_for(&path)
                .filter(|session| !open_sessions.contains(session.id.as_ref()))
                .map(|session| AgentEntry::Past {
                    label: names
                        .and_then(|names| names.name(&session.id).cloned())
                        .unwrap_or_else(|| session.title.clone().into()),
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

/// The checkout a workspace is standing in: the *innermost* one whose path
/// contains a root of it.
///
/// The deepest match and not the first, because one checkout can live inside
/// another. `git.worktree_directory` is a relative path, so a reader who points
/// it into the repository -- `.claude/worktrees`, say -- gets linked worktrees
/// nested under the main checkout, and every one of their paths then starts
/// with the main checkout's too. `all_checkouts` deliberately lists the main
/// one first (`tree.rs:325`), so taking the first match filed every open agent
/// tab under the main row however deep the worktree it was really running in --
/// and the moment that tab closed, the session came back from the index under
/// its real working directory, on a different row. The agent did not go
/// anywhere; the row it had been drawn on was never its own.
///
/// Ties keep the first, so the answer follows `all_checkouts`' own order rather
/// than the iteration accident of `max_by_key`. A tie needs a multi-root
/// workspace whose roots sit in two equally deep checkouts; there is no better
/// answer for that, only a stable one.
pub(crate) fn checkout_containing<'a>(
    worktrees: &'a [git::repository::Worktree],
    roots: &[Arc<Path>],
) -> Option<&'a git::repository::Worktree> {
    worktrees
        .iter()
        .filter(|worktree| roots.iter().any(|root| root.starts_with(&worktree.path)))
        .reduce(|deepest, worktree| {
            if worktree.path.components().count() > deepest.path.components().count() {
                worktree
            } else {
                deepest
            }
        })
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
    /// Falsifiable by dropping the refresh from the `ItemRemoved` arm:
    /// `is_scanning()` then stays `false` after the tab closes.
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

    /// Dragging a tab to another pane removes it from one and adds it to the
    /// other, so the removal event alone cannot tell a move from a close.
    #[gpui::test]
    async fn moving_an_agent_tab_to_another_pane_is_not_a_close(cx: &mut TestAppContext) {
        let (panel, workspace, source, item_id, cx) = panel_with_one_open_agent_tab(cx).await;
        cx.update(|_, cx| {
            agent_ui::SessionStore::install_global_for_test(Vec::new(), cx);
        });
        panel.update(cx, |panel, cx| panel.ensure_session_store(cx));
        cx.run_until_parked();
        let generation_before = panel.read_with(cx, |panel, cx| {
            panel.session_store.as_ref().unwrap().read(cx).generation()
        });

        let destination = workspace.update_in(cx, |workspace, window, cx| {
            workspace.split_pane(source.clone(), workspace::SplitDirection::Right, window, cx)
        });
        cx.update(|window, cx| {
            workspace::move_item(&source, &destination, item_id, 0, true, window, cx);
        });
        panel.read_with(cx, |panel, cx| {
            assert!(
                !panel.session_store.as_ref().unwrap().read(cx).is_scanning(),
                "a move is not a close and must not ask for a sweep"
            );
        });
        cx.run_until_parked();

        panel.read_with(cx, |panel, cx| {
            assert_eq!(
                panel.session_store.as_ref().unwrap().read(cx).generation(),
                generation_before,
                "no sweep ran for the move"
            );
            assert!(
                panel.agent_tabs.contains(&item_id),
                "the moved tab is still an open agent tab"
            );
        });
    }

    /// One window can host several workspaces, each with its own panel, and the
    /// session store is shared. A panel that was never drawn has no store of
    /// its own, but the others are still waiting on the global one.
    ///
    /// Run against a single workspace with the global store installed and the
    /// panel undrawn: that is the half of the situation the closing panel owns.
    #[gpui::test]
    async fn an_undrawn_panel_still_refreshes_the_shared_store_on_close(cx: &mut TestAppContext) {
        let written = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (panel, _workspace, pane, item_id, cx) = panel_with_one_open_agent_tab(cx).await;
        let store = cx.update(|_, cx| {
            agent_ui::SessionStore::install_global_for_test(
                vec![std::sync::Arc::new(LateTranscript {
                    written: written.clone(),
                    with_older: false,
                })],
                cx,
            )
        });
        let generation_before = store.read_with(cx, |store, _| store.generation());

        pane.update_in(cx, |pane, window, cx| {
            pane.remove_item(item_id, true, true, window, cx);
        });
        cx.run_until_parked();

        store.read_with(cx, |store, _| {
            assert_eq!(
                store.generation(),
                generation_before + 1,
                "closing an agent tab must refresh the shared store even from an undrawn panel"
            );
        });
        panel.read_with(cx, |panel, _| {
            assert!(
                panel.session_store.is_none(),
                "and still must not hand the panel a store it never asked for"
            );
        });
    }

    /// **T13.** Closing an ordinary file tab must not.
    ///
    /// Falsifiable by sweeping on every `ItemRemoved` instead of only for the
    /// ids recorded as agent tabs: it would then fail here.
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
        let file_item_id = workspace.update_in(cx, |workspace, window, cx| {
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

    /// The checkout an open agent tab is filed under, with no window needed.
    ///
    /// Built in `all_checkouts`' own order -- the main checkout first, then the
    /// rest by path (`tree.rs:325`) -- because that order is what made the
    /// first-match version wrong.
    mod checkout_containing {
        use super::super::checkout_containing;
        use std::path::{Path, PathBuf};
        use std::sync::Arc;

        fn worktree(path: &str, is_main: bool) -> git::repository::Worktree {
            git::repository::Worktree {
                path: PathBuf::from(path),
                ref_name: None,
                sha: Default::default(),
                is_main,
                is_bare: false,
            }
        }

        fn roots(paths: &[&str]) -> Vec<Arc<Path>> {
            paths
                .iter()
                .map(|path| Arc::from(Path::new(path)))
                .collect()
        }

        /// The regression. A worktree kept inside the repository -- what
        /// `git.worktree_directory: ".claude/worktrees"` produces -- has a path
        /// that starts with the main checkout's, and the main checkout is
        /// listed first. Taking the first match filed the agent under the main
        /// row, so closing its tab moved the session to the row it had always
        /// belonged to and it read as having been deleted.
        #[test]
        fn a_nested_worktree_wins_over_the_main_checkout_that_contains_it() {
            let worktrees = [
                worktree("/repos/zode", true),
                worktree("/repos/zode/.claude/worktrees/feature", false),
            ];

            let here = checkout_containing(
                &worktrees,
                &roots(&["/repos/zode/.claude/worktrees/feature"]),
            )
            .expect("a root inside a checkout has a checkout");

            assert_eq!(
                here.path,
                PathBuf::from("/repos/zode/.claude/worktrees/feature"),
                "the innermost checkout containing the root is the one the window is standing in"
            );
        }

        /// The counterweight: standing in the main checkout must still answer
        /// the main checkout, however many worktrees are nested under it.
        #[test]
        fn standing_in_the_main_checkout_still_answers_the_main_checkout() {
            let worktrees = [
                worktree("/repos/zode", true),
                worktree("/repos/zode/.claude/worktrees/feature", false),
            ];

            let here = checkout_containing(&worktrees, &roots(&["/repos/zode"]))
                .expect("a root inside a checkout has a checkout");

            assert_eq!(here.path, PathBuf::from("/repos/zode"));
        }

        /// A sibling worktree -- the shipped default, `"../worktrees"` -- was
        /// never ambiguous, and must not become so.
        #[test]
        fn a_sibling_worktree_is_unaffected() {
            let worktrees = [
                worktree("/repos/zode", true),
                worktree("/repos/worktrees/feature", false),
            ];

            let here = checkout_containing(&worktrees, &roots(&["/repos/worktrees/feature"]))
                .expect("a root inside a checkout has a checkout");

            assert_eq!(here.path, PathBuf::from("/repos/worktrees/feature"));
        }

        /// Component-wise, not textual: `/repos/zode-kit` is not inside
        /// `/repos/zode`, and filing its agents under a neighbouring project
        /// would be worse than filing them nowhere.
        #[test]
        fn a_name_that_merely_starts_the_same_is_not_inside() {
            let worktrees = [worktree("/repos/zode", true)];

            assert!(checkout_containing(&worktrees, &roots(&["/repos/zode-kit"])).is_none());
        }

        /// A window over something no checkout holds has no checkout to file an
        /// agent under, and must say so rather than guess at the first one.
        #[test]
        fn a_root_outside_every_checkout_has_no_checkout() {
            let worktrees = [
                worktree("/repos/zode", true),
                worktree("/repos/zode/.claude/worktrees/feature", false),
            ];

            assert!(checkout_containing(&worktrees, &roots(&["/elsewhere"])).is_none());
        }
    }

    /// A provider whose transcript only exists once `written` is set, standing
    /// in for a Claude session started after the last sweep.
    struct LateTranscript {
        written: std::sync::Arc<std::sync::atomic::AtomicBool>,
        with_older: bool,
    }

    impl agent_sessions::SessionProvider for LateTranscript {
        fn agent(&self) -> agent_sessions::AgentKind {
            agent_sessions::AgentKind::Claude
        }
        fn availability(&self) -> agent_sessions::Availability {
            agent_sessions::Availability::Ready
        }
        fn list(&self) -> anyhow::Result<Vec<agent_sessions::SessionSummary>> {
            let mut sessions = Vec::new();
            if self.with_older {
                sessions.push(agent_sessions::SessionSummary {
                    id: std::sync::Arc::from("00000000-0000-4000-8000-000000000000"),
                    agent: agent_sessions::AgentKind::Claude,
                    title: "older".into(),
                    preview: String::new(),
                    preview_speaker: None,
                    cwd: std::path::PathBuf::from("/repos/zode"),
                    branch: None,
                    model: None,
                    updated_at: std::time::SystemTime::UNIX_EPOCH,
                    log_path: None,
                    log_bytes: 0,
                });
            }
            if !self.written.load(std::sync::atomic::Ordering::SeqCst) {
                return Ok(sessions);
            }
            sessions.push(agent_sessions::SessionSummary {
                id: std::sync::Arc::from("11111111-1111-4111-8111-111111111111"),
                agent: agent_sessions::AgentKind::Claude,
                title: "hello".into(),
                preview: String::new(),
                preview_speaker: None,
                cwd: std::path::PathBuf::from("/repos/zode"),
                branch: None,
                model: None,
                updated_at: std::time::SystemTime::UNIX_EPOCH,
                log_path: None,
                log_bytes: 0,
            });
            Ok(sessions)
        }
        fn find(&self, _id: &str) -> anyhow::Result<Option<agent_sessions::SessionSummary>> {
            Ok(None)
        }
        fn new_session_command(
            &self,
            _id: &str,
            _cwd: &std::path::Path,
        ) -> Option<agent_sessions::AgentCommand> {
            None
        }
        fn counts(
            &self,
            _session: &agent_sessions::SessionSummary,
        ) -> anyhow::Result<agent_sessions::SessionCounts> {
            Ok(Default::default())
        }
        fn resume_command(
            &self,
            _s: &agent_sessions::SessionSummary,
            _f: agent_sessions::Fork,
        ) -> Option<agent_sessions::AgentCommand> {
            None
        }
        fn deletion(&self, _session: &agent_sessions::SessionSummary) -> agent_sessions::Deletion {
            agent_sessions::Deletion::Nothing
        }
    }

    fn listed(panel: &gpui::Entity<BranchPanel>, cx: &mut VisualTestContext) -> (usize, usize) {
        panel.read_with(cx, |panel, _| {
            let mut open = 0;
            let mut past = 0;
            for row in &panel.rows {
                if let crate::branch_panel::tree::TreeRow::Worktree { agents, .. } = row {
                    for agent in agents.iter() {
                        if agent.is_open() {
                            open += 1;
                        } else {
                            past += 1;
                        }
                    }
                }
            }
            (open, past)
        })
    }

    /// `drawn_while_tab_open`: whether the panel was drawn (rows rebuilt) while
    /// the tab was open. Returns what the panel lists right after the close.
    async fn listed_after_closing_a_late_transcript_tab(
        cx: &mut TestAppContext,
        drawn_while_tab_open: bool,
    ) -> (usize, usize) {
        let (before_reload, after_reload) = close_then_reload(cx, drawn_while_tab_open).await;
        assert_eq!(
            after_reload,
            (0, 1),
            "control: a manual reload always restores the row"
        );
        before_reload
    }

    async fn close_then_reload(
        cx: &mut TestAppContext,
        drawn_while_tab_open: bool,
    ) -> ((usize, usize), (usize, usize)) {
        let written = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (panel, cx) = panel_over_a_repo(cx).await;
        cx.update(|_, cx| {
            agent_ui::SessionStore::install_global_for_test(
                vec![std::sync::Arc::new(LateTranscript {
                    written: written.clone(),
                    with_older: false,
                })],
                cx,
            );
        });
        let workspace = panel
            .read_with(cx, |panel, _| panel.workspace.clone())
            .upgrade()
            .expect("workspace alive");
        panel.update_in(cx, |panel, window, cx| {
            panel.set_active(true, window, cx);
            panel.ensure_session_store(cx);
            panel.refresh_if_stale(cx);
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
        if drawn_while_tab_open {
            panel.update_in(cx, |panel, _, cx| panel.refresh_if_stale(cx));
            assert_eq!(listed(&panel, cx).0, 1, "the open tab must be listed");
        }

        // The CLI has now written its transcript.
        written.store(true, std::sync::atomic::Ordering::SeqCst);

        let pane = workspace.read_with(cx, |workspace, _| workspace.active_pane().clone());
        let item_id = pane.read_with(cx, |pane, _| pane.items().next().unwrap().item_id());
        let close = pane.update_in(cx, |pane, window, cx| {
            pane.close_item_by_id(item_id, workspace::SaveIntent::Close, window, cx)
        });
        close.await.expect("close");
        cx.run_until_parked();
        // What `render` does when the panel is shown.
        panel.update_in(cx, |panel, _, cx| panel.refresh_if_stale(cx));
        let before_reload = listed(&panel, cx);
        panel.update(cx, |panel, cx| panel.reload(cx));
        cx.run_until_parked();
        panel.update_in(cx, |panel, _, cx| panel.refresh_if_stale(cx));
        (before_reload, listed(&panel, cx))
    }

    /// A sweep still in flight when the panel next draws, and an older session
    /// already listed under the checkout: the closed session must join it once
    /// the sweep lands.
    #[gpui::test]
    async fn a_closed_tab_joins_the_listed_sessions_once_the_sweep_lands(cx: &mut TestAppContext) {
        let written = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (panel, cx) = panel_over_a_repo(cx).await;
        cx.update(|_, cx| {
            agent_ui::SessionStore::install_global_for_test(
                vec![std::sync::Arc::new(LateTranscript {
                    written: written.clone(),
                    with_older: true,
                })],
                cx,
            );
        });
        let workspace = panel
            .read_with(cx, |panel, _| panel.workspace.clone())
            .upgrade()
            .expect("workspace alive");
        panel.update_in(cx, |panel, window, cx| {
            panel.set_active(true, window, cx);
            panel.ensure_session_store(cx);
            panel.refresh_if_stale(cx);
        });
        cx.run_until_parked();
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
        written.store(true, std::sync::atomic::Ordering::SeqCst);

        let pane = workspace.read_with(cx, |workspace, _| workspace.active_pane().clone());
        let item_id = pane.read_with(cx, |pane, _| pane.items().next().unwrap().item_id());
        pane.update_in(cx, |pane, window, cx| {
            pane.remove_item(item_id, true, true, window, cx);
        });
        // The panel draws while the sweep has not run yet.
        panel.update_in(cx, |panel, _, cx| panel.refresh_if_stale(cx));
        cx.run_until_parked();
        panel.update_in(cx, |panel, _, cx| panel.refresh_if_stale(cx));
        assert_eq!(listed(&panel, cx), (0, 2));
    }

    #[gpui::test]
    async fn a_closed_tab_comes_back_when_the_panel_was_drawn_while_it_was_open(
        cx: &mut TestAppContext,
    ) {
        let (open, past) = listed_after_closing_a_late_transcript_tab(cx, true).await;
        assert_eq!(
            (open, past),
            (0, 1),
            "closed tab must come back as a Past row"
        );
    }

    /// The owner regression test: the refresh on close must not depend on the
    /// last draw having listed the open tab.
    #[gpui::test]
    async fn a_closed_tab_comes_back_even_if_the_panel_was_not_drawn_while_it_was_open(
        cx: &mut TestAppContext,
    ) {
        let (open, past) = listed_after_closing_a_late_transcript_tab(cx, false).await;
        assert_eq!(
            (open, past),
            (0, 1),
            "closed tab must come back as a Past row"
        );
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
        assert!(
            cx.update(|_, cx| agent_ui::SessionStore::try_global(cx).is_none()),
            "nor a global one: closing a tab must not create the shared store"
        );
    }
}
