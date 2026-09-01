//! Constructing the panel and keeping its rows current.
//!
//! Split out of `panel.rs` so the `Panel` trait implementation there stays
//! readable next to the struct it describes.

use collections::HashSet;
use gpui::{
    AppContext as _, AsyncWindowContext, Context, Entity, Focusable as _, ListAlignment, ListState,
    Task, WeakEntity, Window, px,
};
use workspace::Workspace;

use crate::branch_panel::panel::BranchPanel;
use crate::branch_panel::state::{SerializedBranchPanel, StoredKey};
use crate::branch_panel::tree::{RowKey, SectionKind, build_rows};

impl BranchPanel {
    pub fn new(
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        let workspace_handle = workspace.weak_handle();
        cx.new(|cx| {
            let filter_editor = cx.new(|cx| {
                let mut editor = editor::Editor::single_line(window, cx);
                editor.set_placeholder_text("Filter branches", window, cx);
                editor
            });

            let mut subscriptions = vec![cx.subscribe(
                &filter_editor,
                |panel: &mut BranchPanel, _, _: &editor::EditorEvent, cx| {
                    panel.mark_stale(cx);
                },
            )];

            let mut panel = Self {
                workspace: workspace_handle,
                focus_handle: cx.focus_handle(),
                filter_editor,
                filter_visible: false,
                list_state: ListState::new(0, ListAlignment::Top, px(256.)),
                row_kinds: Vec::new(),
                is_active: false,
                stale: true,
                rebuild_count: 0,
                repos: Vec::new(),
                rows: Vec::new(),
                expanded: HashSet::default(),
                stored_expanded: HashSet::default(),
                tags: Default::default(),
                tags_loading: HashSet::default(),
                running_remote_ops: HashSet::default(),
                new_branch: None,
                context_menu: None,
                pending_serialization: Task::ready(None),
                _subscriptions: Vec::new(),
            };

            // The git store is taken from the `workspace` we were handed, not
            // read back through `panel.workspace`. This body runs inside
            // `Workspace::update`, and reading the workspace entity from in
            // there panics -- the same re-entrancy trap the project rail's
            // panel toggle hit once before.
            let store = workspace.project().read(cx).git_store().clone();
            subscriptions.push(Self::observe_git_store(cx, &store));
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
                panel.update(cx, |panel, _| panel.stored_expanded = serialized.expanded);
            }
            panel
        })
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

        let filter = if self.filter_visible {
            self.filter_editor.read(cx).text(cx)
        } else {
            String::new()
        };
        let expanded = &self.expanded;
        self.rows = build_rows(&self.repos, &|key| expanded.contains(key), &filter);
        self.sync_list_state();
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

    /// Shows or hides the filter field. A hidden field applies no filter, so a
    /// query left behind cannot go on silently hiding branches.
    pub(crate) fn toggle_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_visible = !self.filter_visible;
        if self.filter_visible {
            self.filter_editor.focus_handle(cx).focus(window, cx);
        }
        self.mark_stale(cx);
        cx.notify();
    }

    /// Turns the paths restored from disk into live row keys, once the
    /// repositories they name have actually turned up.
    ///
    /// An adopted entry is *consumed*. Leaving it in place would re-insert the
    /// key on every rebuild, and since collapsing a row rebuilds the tree, any
    /// section that happened to be open when the panel was last saved could
    /// never be closed again.
    fn adopt_stored_expansion(&mut self) {
        if self.stored_expanded.is_empty() {
            return;
        }

        let mut adopted = Vec::new();
        for repo in &self.repos {
            let path = repo.path.to_string_lossy().to_string();
            for stored in self.stored_expanded.iter() {
                if let Some(key) = stored.to_row_key(repo.id, &path) {
                    adopted.push((stored.clone(), key));
                }
            }
        }

        for (stored, key) in adopted {
            self.stored_expanded.remove(&stored);
            self.expanded.insert(key);
        }
    }

    pub(crate) fn toggle_row(&mut self, key: RowKey, cx: &mut Context<Self>) {
        if !self.expanded.remove(&key) {
            // Opening the Tags section is the only thing in the panel that
            // triggers a git command, and only the first time.
            if let RowKey::Section(id, SectionKind::Tags) = &key {
                self.load_tags(*id, cx);
            }
            self.expanded.insert(key);
        }
        self.stale = true;
        self.serialize(cx);
        cx.notify();
    }

    fn serialize(&mut self, cx: &mut Context<Self>) {
        let mut stored = HashSet::default();
        for repo in &self.repos {
            let path = repo.path.to_string_lossy().to_string();
            for key in &self.expanded {
                if key.repository_id() == repo.id {
                    stored.insert(StoredKey::from_row_key(key, &path));
                }
            }
        }

        let state = SerializedBranchPanel { expanded: stored };
        let workspace = self.workspace.clone();
        self.pending_serialization = cx.spawn(async move |_, cx| state.write(workspace, cx).await);
    }
}

#[cfg(test)]
mod tests;
