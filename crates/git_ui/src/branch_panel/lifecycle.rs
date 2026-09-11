//! Constructing the panel and keeping its rows current.
//!
//! Split out of `panel.rs` so the `Panel` trait implementation there stays
//! readable next to the struct it describes.

use collections::HashSet;
use gpui::{
    AppContext as _, AsyncWindowContext, Context, Entity, ListAlignment, ListState, Task,
    WeakEntity, Window, px,
};
use workspace::Workspace;

use crate::branch_panel::panel::BranchPanel;
use crate::branch_panel::state::{SerializedBranchPanel, StoredKey};
use crate::branch_panel::tree::{AgentActivity, RowKey, TreeRow, build_rows};

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

            let mut panel = Self {
                workspace: workspace_handle,
                focus_handle: cx.focus_handle(),
                pinned: Default::default(),
                manual_order: Vec::new(),
                list_state: ListState::new(0, ListAlignment::Top, px(256.)),
                session_store: None,
                _session_subscription: None,
                _agent_tab_names: Vec::new(),
                row_kinds: Vec::new(),
                is_active: false,
                stale: true,
                rebuild_count: 0,
                repos: Vec::new(),
                rows: Vec::new(),
                expanded: HashSet::default(),
                collapsed: HashSet::default(),
                stored_expanded: HashSet::default(),
                stored_collapsed: HashSet::default(),
                running_remote_ops: HashSet::default(),
                reloading: false,
                _reload_task: None,
                context_menu: None,
                pending_serialization: Task::ready(None),
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
            // Registering a subscription does not read the entity, so this is
            // safe where a `read` inside this closure would panic.
            panel._subscriptions = subscriptions;
            panel
        })
    }

    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> anyhow::Result<Entity<Self>> {
        let serialized = SerializedBranchPanel::load(&workspace, &mut cx).await;

        workspace.update_in(&mut cx, |workspace, window, cx| {
            let panel = BranchPanel::new(workspace, window, cx);
            if let Some(serialized) = serialized {
                panel.update(cx, |panel, _| {
                    panel.stored_expanded = serialized.expanded;
                    panel.stored_collapsed = serialized.collapsed;
                    panel.pinned = serialized
                        .pinned
                        .into_iter()
                        .map(std::path::PathBuf::from)
                        .collect();
                    panel.manual_order = serialized
                        .order
                        .into_iter()
                        .map(std::path::PathBuf::from)
                        .collect();
                });
            }
            panel
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
        self.adopt_stored_expansion();

        // No filter from the panel: the header carries one button, and the row
        // builder's filter stays for whatever exposes one next.
        let rows = build_rows(&self.repos, &|key| self.row_is_open(key), "");
        self.rows = rows;
        self.sync_list_state();
        self.track_agent_tab_names(cx);
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

    /// Turns the paths restored from disk into live row keys, once the
    /// repositories they name have actually turned up.
    ///
    /// An adopted entry is *consumed*. Leaving it in place would re-insert the
    /// key on every rebuild, and since collapsing a row rebuilds the tree, any
    /// section that happened to be open when the panel was last saved could
    /// never be closed again.
    /// Whether a row is drawn open.
    ///
    /// The builder asks one question and the two sets answer it from opposite
    /// directions: a repository is open until somebody closes it, every other
    /// row closed until somebody opens it. See `BranchPanel::collapsed` for
    /// why the repository goes that way round.
    pub(crate) fn row_is_open(&self, key: &RowKey) -> bool {
        match key {
            RowKey::Repo(_) => !self.collapsed.contains(key),
            RowKey::WorktreeAgents(..) => self.expanded.contains(key),
        }
    }

    fn adopt_stored_expansion(&mut self) {
        // Each set takes only the rows it governs. A blob written before
        // repositories recorded their closure lists the ones that were *open*,
        // and those entries mean nothing here any more -- adopted without this
        // filter they would sit in `expanded` unread, and be written back out
        // on every save for the life of the workspace.
        Self::adopt(
            &self.repos,
            &mut self.stored_expanded,
            &mut self.expanded,
            |key| matches!(key, RowKey::WorktreeAgents(..)),
        );
        Self::adopt(
            &self.repos,
            &mut self.stored_collapsed,
            &mut self.collapsed,
            |key| matches!(key, RowKey::Repo(_)),
        );
    }

    fn adopt(
        repos: &[crate::branch_panel::tree::RepoData],
        stored: &mut HashSet<StoredKey>,
        live: &mut HashSet<RowKey>,
        governs: impl Fn(&RowKey) -> bool,
    ) {
        if stored.is_empty() {
            return;
        }

        let mut adopted = Vec::new();
        for repo in repos {
            let path = repo.path.to_string_lossy().to_string();
            for entry in stored.iter() {
                if let Some(key) = entry.to_row_key(repo.id, &path) {
                    adopted.push((entry.clone(), key));
                }
            }
        }

        // Consumed whether or not it is kept: an entry left in place would be
        // re-adopted on the next rebuild, and since closing a row rebuilds the
        // tree, a row restored open could never be closed again.
        for (entry, key) in adopted {
            stored.remove(&entry);
            if governs(&key) {
                live.insert(key);
            }
        }
    }

    pub(crate) fn toggle_row(&mut self, key: RowKey, cx: &mut Context<Self>) {
        // One gesture, two sets, opposite polarity -- a repository records
        // that it was closed, everything else that it was opened.
        let set = match key {
            RowKey::Repo(_) => &mut self.collapsed,
            RowKey::WorktreeAgents(..) => &mut self.expanded,
        };
        if !set.remove(&key) {
            set.insert(key);
        }
        self.stale = true;
        self.serialize(cx);
        cx.notify();
    }

    /// Turns live row keys back into the path-based form that survives a
    /// restart, for whichever of the two sets is being written.
    fn stored_keys(&self, keys: &HashSet<RowKey>) -> HashSet<StoredKey> {
        let mut stored = HashSet::default();
        for repo in &self.repos {
            let path = repo.path.to_string_lossy().to_string();
            for key in keys {
                if key.repository_id() == repo.id {
                    stored.insert(StoredKey::from_row_key(key, &path));
                }
            }
        }
        stored
    }

    pub(crate) fn serialize(&mut self, cx: &mut Context<Self>) {
        let state = SerializedBranchPanel {
            expanded: self.stored_keys(&self.expanded),
            collapsed: self.stored_keys(&self.collapsed),
            pinned: self
                .pinned
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect(),
            order: self
                .manual_order
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect(),
        };
        let workspace = self.workspace.clone();
        self.pending_serialization = cx.spawn(async move |_, cx| state.write(workspace, cx).await);
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
