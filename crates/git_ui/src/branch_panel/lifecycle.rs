//! Constructing the panel and keeping its rows current.
//!
//! Split out of `panel.rs` so the `Panel` trait implementation there stays
//! readable next to the struct it describes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use collections::{HashMap, HashSet};
use gpui::{
    App, AppContext as _, AsyncWindowContext, Context, Entity, ListAlignment, ListState,
    SharedString, WeakEntity, Window, px,
};
use project::git_store::RepositoryId;
use workspace::Workspace;

use crate::branch_panel::checkout_state::CheckoutViewState;
use crate::branch_panel::panel::BranchPanel;
use crate::branch_panel::state::{SerializedBranchPanel, StoredKey};
use crate::branch_panel::tree::{
    AgentActivity, AgentEntry, RowKey, SubagentKey, TreeRow, build_rows,
};

/// How often the panel redraws while an agent is live.
///
/// Fast enough that the mark keeps up with an answer starting and finishing,
/// slow enough that it is a rounding error next to drawing the frame it asks
/// for.
const ACTIVITY_TICK: std::time::Duration = std::time::Duration::from_millis(250);

impl BranchPanel {
    pub fn new(
        workspace: &mut Workspace,
        _window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        let workspace_handle = workspace.weak_handle();
        let workspace_entity = cx.entity();
        cx.new(|cx| {
            let mut subscriptions = Vec::new();

            // Created here rather than reused from `load` for panels that never
            // go through it (every test in this module, and any future caller
            // that builds a panel directly). `CheckoutViewState::global` is
            // idempotent -- a panel built after `load` already ran the shared
            // read gets the same entity back, not a second one.
            let checkout_state = CheckoutViewState::global(cx);
            // Registering an observe does not read the entity, so it is safe
            // here where a `read` inside this closure would panic -- see the
            // note on the git store subscription below.
            subscriptions.push(cx.observe(&checkout_state, |panel: &mut Self, _, cx| {
                panel.mark_stale(cx)
            }));
            // Same reason, for the same kind of record: one process-global store
            // shared by every panel is only observably shared if the panels
            // watch it. Without this, enabling a bypass in one window leaves the
            // other window's card unmarked until something unrelated redraws it.
            let bypass = agent_ui::PermissionBypassStore::global(cx);
            subscriptions.push(cx.observe(&bypass, |panel: &mut Self, _, cx| panel.mark_stale(cx)));

            let mut panel = Self {
                workspace: workspace_handle,
                focus_handle: cx.focus_handle(),
                checkout_state,
                list_state: ListState::new(0, ListAlignment::Top, px(256.)),
                session_store: None,
                _session_subscription: None,
                _agent_tab_names: Vec::new(),
                row_kinds: Vec::new(),
                is_active: false,
                stale: true,
                rebuild_count: 0,
                repos: Vec::new(),
                last_known_agents: HashMap::default(),
                expanded_subagents: HashSet::default(),
                past_subagents: HashMap::default(),
                _subagent_reads: HashMap::default(),
                rows: Vec::new(),
                running_remote_ops: collections::HashSet::default(),
                reloading: false,
                _reload_task: None,
                context_menu: None,
                _activity_tick: None,
                _subscriptions: Vec::new(),
            };

            // The git store is taken from the `workspace` we were handed, not
            // read back through `panel.workspace`. This body runs inside
            // `Workspace::update`, and reading the workspace entity from in
            // there panics -- the same re-entrancy trap the project rail's
            // panel toggle hit once before.
            let store = workspace.project().read(cx).git_store().clone();
            subscriptions.push(Self::observe_git_store(cx, &store));
            // Registering a subscription does not read the entity, so this is
            // safe where a `read` inside this closure would panic.
            subscriptions.push(Self::observe_agent_tabs(cx, &workspace_entity));
            panel._subscriptions = subscriptions;
            panel
        })
    }

    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> anyhow::Result<Entity<Self>> {
        let checkout_state = cx.update(|_, cx| CheckoutViewState::global(cx))?;

        // Both reads below are awaited before the panel is built, and that
        // ordering is load-bearing, not incidental. `BranchPanel::load`
        // completing on the first poll of the `futures::join!` in
        // `zed.rs:593` (`initialize_panels`) is what stops `starts_open: true`
        // collapsing a dock stack that is being restored -- see
        // docs/journals/2026-09-11-dock-state-read-from-the-wrong-owner.md,
        // "Latent, recorded, not fixed" #1. Removing this await is a change to
        // dock restore behaviour, not a cleanup.
        let shared_load = checkout_state.update(&mut cx, |state, cx| state.load(cx));
        // The panel draws the permission-bypass mark and builds the menu entry
        // from this record, so it has to be read here too. Awaiting only
        // `checkout_state` left the card dark and the menu unchecked for a
        // bypassed checkout after every restart, while the launch path -- which
        // does its own load -- went on appending the flag.
        let bypass = cx.update(|_, cx| agent_ui::PermissionBypassStore::global(cx))?;
        let bypass_load = bypass.update(&mut cx, |store, cx| store.load(cx));
        let legacy = SerializedBranchPanel::load(&workspace, &mut cx).await;
        shared_load.await;
        bypass_load.await;

        // Offered only after the shared read landed: `seed_from_legacy` itself
        // no-ops once a shared record was found, but that flag is only correct
        // once `load` above has actually finished setting it.
        if let Some(legacy) = legacy {
            checkout_state.update(&mut cx, |state, cx| state.seed_from_legacy(legacy, cx));
        }

        workspace.update_in(&mut cx, |workspace, window, cx| {
            BranchPanel::new(workspace, window, cx)
        })
    }

    /// Re-reads git, rather than only rebuilding from what the store already holds.
    ///
    /// `mark_stale` on its own would rebuild the tree out of the same cached
    /// `RepositorySnapshot` and produce identical rows -- useless as a reload. So this
    /// also schedules each repository's scan, which re-runs `git worktree list` inside
    /// `compute_snapshot`, keeping this module a reader rather than something that shells
    /// out to git itself.
    ///
    /// Two things then bring the result in, and both are needed. A scan that finds the list
    /// changed emits `GitWorktreeListChanged`, which the panel's existing subscription turns
    /// into a rebuild. A scan that finds nothing changed emits **nothing at all** -- so the
    /// completion below is what settles the spinner, and it is why this waits on the scan
    /// rather than on an event.
    ///
    /// The scan is a keyed job (`ReloadGitState`), and the queue drops a keyed job when a
    /// newer one with the same key is already waiting -- so leaning on the button
    /// coalesces instead of queueing one scan per press.
    pub(crate) fn reload(&mut self, cx: &mut Context<Self>) {
        // The agent rows come from the workspace, not from git, so they are refreshed by
        // the rebuild itself rather than by any scan.
        self.mark_stale(cx);

        let Some(git_store) = self.git_store(cx) else {
            return;
        };
        let scans = git_store.update(cx, |git_store, cx| {
            git_store.refresh_all_repositories_and_wait(cx)
        });

        self.reloading = true;
        self._reload_task = Some(cx.spawn(async move |panel, cx| {
            scans.await;
            panel
                .update(cx, |panel, cx| {
                    panel.reloading = false;
                    // The scan may have changed nothing, in which case no repository event
                    // fired and this is the only thing that redraws the settled icon.
                    panel.mark_stale(cx);
                    cx.notify();
                })
                // A failed update means the panel is gone, and so is this task.
                .ok();
        }));
    }

    /// Marks the tree for a rebuild. Rebuilding happens in `render`, so a burst
    /// of git events collapses into one rebuild, and a hidden panel does none.
    pub(crate) fn mark_stale(&mut self, cx: &mut Context<Self>) {
        self.stale = true;
        if self.is_active {
            cx.notify();
        }
    }

    /// Rebuilds `repos` and `rows` if anything changed. Called from `render`.
    pub(crate) fn refresh_if_stale(&mut self, cx: &mut Context<Self>) {
        if !self.stale {
            return;
        }
        self.stale = false;
        self.rebuild_count += 1;
        self.repos = self.collect_repos(cx);
        self.hold_known_agents(cx);
        self.prune_dead_checkouts(cx);

        // Resolved once per rebuild rather than looked up per row: a row only
        // carries a `RepositoryId`, which means nothing on disk, so every row
        // needs the id turned back into the path the shared record is keyed
        // by. Building this here keeps `row_open_for` an O(1) lookup instead
        // of a scan of `self.repos` per row.
        //
        // `anchor`, not `path` -- see `RepoData::anchor`. The record has to
        // answer the same question from every checkout of a repository, and
        // `path` is a different directory in each of them.
        let paths: HashMap<RepositoryId, Arc<Path>> = self
            .repos
            .iter()
            .map(|repo| (repo.id, repo.anchor.clone()))
            .collect();

        // Cloning the handle rather than borrowing `self.checkout_state`
        // directly: `state` below ties its lifetime to `cx`, and `self.repos`
        // is read immutably by `build_rows` at the same time. Both borrows are
        // shared and coexist fine -- it is the `self.rows = rows` afterwards
        // that needs them gone first, which is why this is scoped into its
        // own block.
        let checkout_state = self.checkout_state.clone();
        let rows = {
            let state = checkout_state.read(cx);
            // No filter from the panel: the header carries one button, and the row
            // builder's filter stays for whatever exposes one next.
            build_rows(
                &self.repos,
                &|key| Self::row_open_for(state, &paths, key),
                "",
            )
        };
        self.rows = rows;
        self.sync_list_state();
        self.track_agent_tab_names(cx);
        self.follow_listed_subagents(cx);
    }

    /// Keeps the subagent caches to exactly what this panel is drawing.
    ///
    /// Reads run from here rather than from the click that opens a disclosure,
    /// because that disclosure cannot be drawn until the read has landed: it
    /// says how many subagents there are, and until something has counted them
    /// the honest answer is to draw nothing. An open tab is skipped — it
    /// follows its own session already, and asking twice reads one transcript
    /// twice.
    ///
    /// The three maps are then trimmed back to what is listed, which is what
    /// keeps them from growing for the life of a window that has browsed a lot
    /// of checkouts. Trimming also does the invalidating: resuming a past
    /// session turns its row into an open one, which drops the cached read, so
    /// when the tab closes and the row goes back to being past it is read
    /// again — the subagents it gained while it was open are in that answer,
    /// where a cache kept on "have we ever read this id" would have missed them
    /// forever.
    fn follow_listed_subagents(&mut self, cx: &mut Context<Self>) {
        let drawn: Vec<&AgentEntry> = self
            .rows
            .iter()
            .filter_map(|row| match row {
                TreeRow::Worktree {
                    agents, expanded, ..
                } if *expanded => Some(agents),
                _ => None,
            })
            .flat_map(|agents| agents.iter())
            .collect();

        let open_keys: HashSet<SubagentKey> = drawn
            .iter()
            .filter_map(|entry| entry.subagent_key())
            .collect();
        let past: Vec<(SharedString, Arc<str>)> = drawn
            .iter()
            .filter_map(|entry| match entry {
                AgentEntry::Past { agent, id, .. } => Some((agent.clone(), id.clone())),
                AgentEntry::Open { .. } => None,
            })
            .collect();
        let past_ids: HashSet<Arc<str>> = past.iter().map(|(_, id)| id.clone()).collect();

        // Dropping a read that is still running cancels it, which is the right
        // answer for a session nothing is drawing any more.
        self.past_subagents.retain(|id, _| past_ids.contains(id));
        self._subagent_reads.retain(|id, _| past_ids.contains(id));
        self.expanded_subagents
            .retain(|key| open_keys.contains(key));

        for (agent, id) in past {
            self.read_past_subagents(&agent, &id, cx);
        }
    }

    /// Whether the disk index cannot yet be trusted to speak for a checkout it
    /// currently says nothing about.
    ///
    /// `None` counts as settling: no sweep has ever run for this panel, so
    /// there is no "before" to compare an empty answer against, and treating
    /// it as trustworthy would let the very first rebuild evict a hold it
    /// never had the chance to earn. Once a store exists,
    /// `SessionStore::scanning` is assigned synchronously inside `refresh`
    /// (`session_store.rs:97-99` -- via `session_store.rs:94-99`), so
    /// `is_scanning()` is already true by the time this rebuild asks. That
    /// ordering is structural, not lucky: it is the whole reason
    /// `hold_known_agents` catches the frame a sweep starts in, rather than
    /// trailing it by one.
    pub(crate) fn index_is_settling(&self, cx: &App) -> bool {
        match &self.session_store {
            Some(store) => store.read(cx).is_scanning(),
            None => true,
        }
    }

    /// Backfills a checkout's agent list from what it last showed while the
    /// disk index cannot yet speak for it, and forgets the hold once a
    /// checkout genuinely has nothing or stops being listed at all.
    ///
    /// A separate pass over `self.repos` rather than a change inside
    /// `collect_repos`/`agents_by_checkout` (`data.rs`), for two reasons: it
    /// keeps `build_rows` a pure function of its input exactly as `data.rs`
    /// states, and it keeps the borrows simple -- `collect_repos` takes
    /// `&self` and reads through a closure, so it cannot also take `&mut self`
    /// to write a hold as it goes.
    pub(crate) fn hold_known_agents(&mut self, cx: &App) {
        let settling = self.index_is_settling(cx);

        for repo in self.repos.iter_mut() {
            for worktree in repo.worktrees.iter() {
                let path: Arc<Path> = Arc::from(worktree.path.as_path());
                match repo.agents.get(&path) {
                    // Non-empty, from either source: this is the freshest
                    // truth this checkout has, so it replaces whatever was
                    // held before -- extending instead would let a closed tab
                    // and its later, disk-swept twin sit side by side.
                    Some(agents) if !agents.is_empty() => {
                        self.last_known_agents.insert(path, agents.clone());
                    }
                    // Empty, and the index cannot yet be believed: hand back
                    // what this checkout last showed, if anything did.
                    _ if settling => {
                        if let Some(held) = self.last_known_agents.get(&path) {
                            repo.agents.insert(path, held.clone());
                        }
                    }
                    // Empty, and nothing is in flight to excuse it: the
                    // checkout genuinely has nothing now, so the hold is
                    // stale and is dropped rather than left to answer for a
                    // checkout that no longer needs it.
                    _ => {
                        self.last_known_agents.remove(&path);
                    }
                }
            }
        }

        let live_paths: HashSet<Arc<Path>> = self
            .repos
            .iter()
            .flat_map(|repo| repo.worktrees.iter().map(|w| Arc::from(w.path.as_path())))
            .collect();
        // A repository with no checkouts yet is warming up, not truly empty --
        // `build_rows` makes the same call about an empty worktree list
        // (`tree/build.rs:57-64`, "an empty list means something went wrong").
        // Its checkouts are therefore missing from `live_paths` for a reason
        // that is not "they are gone", so their holds are spared.
        //
        // Scoped to the repositories actually warming up, rather than skipping
        // eviction entirely while any one of them is: a single project still
        // being read would otherwise hold every other project's entries alive,
        // which is how a bounded cache stops being bounded.
        let warming: Vec<Arc<Path>> = self
            .repos
            .iter()
            .filter(|repo| repo.worktrees.is_empty())
            .map(|repo| repo.path.clone())
            .collect();
        // Bounded by the number of checkouts currently on screen (CLAUDE.md:
        // no unbounded caches) -- a checkout that stops being listed, because
        // its worktree was removed or its repository closed, stops being held
        // for it too.
        self.last_known_agents.retain(|path, _| {
            live_paths.contains(path) || warming.iter().any(|repo| path.starts_with(repo))
        });
    }

    /// Drops checkouts of each repository that no longer exist, scoped to that
    /// repository alone -- a panel showing one project has no standing to
    /// prune another's entries out of the shared record.
    fn prune_dead_checkouts(&self, cx: &mut Context<Self>) {
        for repo in &self.repos {
            let live: Vec<PathBuf> = repo.worktrees.iter().map(|w| w.path.clone()).collect();
            let anchor = repo.anchor.clone();
            self.checkout_state
                .update(cx, |state, cx| state.prune(&anchor, &live, cx));
            // Same anchor, same live list: a checkout that stops existing should
            // not keep a permission bypass recorded against its path.
            agent_ui::PermissionBypassStore::global(cx)
                .update(cx, |store, cx| store.prune(&anchor, &live, cx));
        }
    }

    /// Listens to every open agent tab, so renaming one redraws the row that
    /// names it.
    ///
    /// A rename emits `UpdateTab` on the view and touches nothing else: no git
    /// command ran and no row was rebuilt. The activity tick below carries the
    /// case where the agent is still alive, but it stops the moment the CLI
    /// exits -- so a tab renamed after its agent finished sat under its old
    /// name until something unrelated rebuilt the panel.
    ///
    /// A notify is all this needs and all it does. `AgentEntry::label` reads
    /// the name through the view at render, so there is nothing to rebuild;
    /// marking the tree stale here would re-read every repository to change one
    /// string.
    ///
    /// Refreshed from the workspace rather than from the rows: the rows are
    /// what this keeps correct, so deriving the listeners from them would mean
    /// a tab whose row has not been built yet is the one tab nobody is
    /// listening to.
    pub(crate) fn track_agent_tab_names(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            self._agent_tab_names.clear();
            return;
        };
        let views: Vec<_> = workspace
            .read(cx)
            .items_of_type::<agent_ui::AgentView>(cx)
            .collect();
        self._agent_tab_names = views
            .into_iter()
            .map(|view| {
                cx.subscribe(&view, |panel, _, event, cx| {
                    if matches!(event, agent_ui::AgentViewEvent::UpdateTab) {
                        // Only when the panel is on screen. An invisible panel
                        // rebuilds from scratch when it comes back.
                        if panel.is_active {
                            cx.notify();
                        }
                    }
                })
            })
            .collect();
    }

    /// Keeps the panel redrawing while a live agent is listed, and stops when
    /// none is.
    ///
    /// An agent's mark is the one thing here that changes without an event to
    /// hang a redraw on: no git command ran, no row was rebuilt, the CLI simply
    /// started or stopped writing. So it is polled. The tick exists only while
    /// there is something whose mark could change -- a panel showing nothing
    /// but finished transcripts costs nothing, which is the same rule the
    /// rebuild follows.
    pub(crate) fn track_agent_activity(&mut self, cx: &mut Context<Self>) {
        let live = self.rows.iter().any(|row| match row {
            TreeRow::Worktree { agents, .. } => agents
                .iter()
                .any(|agent| agent.activity(cx) != AgentActivity::Gone),
            _ => false,
        });

        if !live {
            self._activity_tick = None;
            return;
        }
        if self._activity_tick.is_some() {
            return;
        }

        self._activity_tick = Some(cx.spawn(async move |panel, cx| {
            loop {
                cx.background_executor().timer(ACTIVITY_TICK).await;
                // The panel owns this task, so a failed update means the panel
                // is gone and so is the task about to be dropped with it.
                if panel.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
        }));
    }

    /// Tells `ListState` which rows changed.
    ///
    /// `reset` would be the easy call, but it discards the scroll position, and
    /// expanding a section near the bottom of a long list would then throw the
    /// user back to the top -- the row they just clicked scrolled out of sight,
    /// which reads as the toggle having done nothing at all.
    ///
    /// A row's measured height depends only on its variant (a branch card is
    /// two lines, a section header one), never on its contents, so comparing
    /// variants is enough to find the slice that actually moved. `splice`
    /// re-anchors the scroll offset around it.
    fn sync_list_state(&mut self) {
        let new_kinds: Vec<_> = self.rows.iter().map(std::mem::discriminant).collect();

        // Defensive: the two are kept in step by this function alone, but a
        // silent disagreement would corrupt every splice after it.
        if self.list_state.item_count() != self.row_kinds.len() {
            self.list_state.reset(new_kinds.len());
            self.row_kinds = new_kinds;
            return;
        }

        if let Some((old_range, new_count)) = ui::utils::changed_range(&self.row_kinds, &new_kinds)
        {
            self.list_state.splice(old_range, new_count);
            self.row_kinds = new_kinds;
        }
    }

    /// Whether a row is drawn open, resolved through the shared record.
    ///
    /// The two polarities live in `CheckoutViewState::is_open` now -- a
    /// repository is open until somebody closes it, every other row closed
    /// until somebody opens it. See that method's doc for why the repository
    /// goes the other way round. `paths` is the O(1) index `refresh_if_stale`
    /// builds once per rebuild; a row whose repository is not in it (gone
    /// between the collect and the build, in principle) draws by the row's
    /// own default rather than asking the record about an id it cannot name.
    fn row_open_for(
        state: &CheckoutViewState,
        paths: &HashMap<RepositoryId, Arc<Path>>,
        key: &RowKey,
    ) -> bool {
        let Some(path) = paths.get(&key.repository_id()) else {
            return matches!(key, RowKey::Repo(_));
        };
        state.is_open(&StoredKey::from_row_key(key, &path.to_string_lossy()))
    }

    /// The same question as `row_open_for`, for callers outside a rebuild --
    /// tests, exclusively, which is why this resolves the path itself instead
    /// of taking the `paths` index: it is not called once per row here, so
    /// the scan `refresh_if_stale` avoids is not a cost this pays. Gated to
    /// test builds because nothing else calls it -- a production caller
    /// wanting this should go through `refresh_if_stale`'s own resolution
    /// instead of paying for a fresh scan of `self.repos`.
    #[cfg(test)]
    pub(crate) fn row_is_open(&self, key: &RowKey, cx: &App) -> bool {
        let Some(repo) = self
            .repos
            .iter()
            .find(|repo| repo.id == key.repository_id())
        else {
            return matches!(key, RowKey::Repo(_));
        };
        let anchor = repo.anchor.to_string_lossy();
        self.checkout_state
            .read(cx)
            .is_open(&StoredKey::from_row_key(key, &anchor))
    }

    pub(crate) fn toggle_row(&mut self, key: RowKey, cx: &mut Context<Self>) {
        let Some(path) = self
            .repos
            .iter()
            .find(|repo| repo.id == key.repository_id())
            .map(|repo| repo.anchor.to_string_lossy().to_string())
        else {
            return;
        };
        let stored = StoredKey::from_row_key(&key, &path);
        self.checkout_state
            .update(cx, |state, cx| state.toggle(stored, cx));
        self.stale = true;
        cx.notify();
    }

    /// Checkouts the reader pinned, resolved to paths for callers comparing
    /// against a `Worktree::path`. Read straight from the shared record on
    /// every call rather than cached on the panel: two panels must agree the
    /// instant either one changes it.
    pub(crate) fn pinned(&self, cx: &App) -> collections::HashSet<PathBuf> {
        self.checkout_state
            .read(cx)
            .pinned()
            .iter()
            .map(PathBuf::from)
            .collect()
    }

    /// The reader's own drag order, resolved to paths. See `pinned` for why
    /// this is not cached.
    pub(crate) fn manual_order(&self, cx: &App) -> Vec<PathBuf> {
        self.checkout_state
            .read(cx)
            .order()
            .iter()
            .map(PathBuf::from)
            .collect()
    }

    /// Creates the shared session store the first time the panel is drawn, and
    /// asks it for its one sweep.
    ///
    /// Not at construction: reading the agents' histories opens every
    /// transcript on disk. `AgentHistoryPanel` already carries the rule that
    /// none of that belongs on the startup path, and a panel nobody opens must
    /// not pay for it either.
    pub(crate) fn ensure_session_store(&mut self, cx: &mut Context<Self>) {
        if self.session_store.is_some() {
            return;
        }
        let store = agent_ui::SessionStore::global(cx);
        // The sweep lands on the store, so the panel has to be told when it
        // does. Held in a field, never detached: a detached observe outlives
        // the panel and fires into a dropped handle.
        self._session_subscription = Some(cx.observe(&store, |panel, _, cx| {
            panel.mark_stale(cx);
        }));
        store.update(cx, |store, cx| store.refresh(cx));
        self.session_store = Some(store);
    }
}

#[cfg(test)]
mod tests;
