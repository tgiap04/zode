//! What the reader decided about the checkouts, held once for the whole app.
//!
//! The thing this describes is a *checkout* -- a path on disk, shared by every
//! window that opens it. The record used to be keyed by workspace
//! (`BranchPanel-{workspace_id}`), which meant switching checkout built a second
//! `Workspace`, a second `BranchPanel`, and read a different record: nothing was
//! reset, a different drawer was opened. Keyed by path, there is one drawer.
//!
//! Held in a process-global entity rather than per panel for the same reason
//! `SessionStore` is: a `MultiWorkspace` window keeps several panels alive at
//! once, and two private copies of one record overwrite each other's writes.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;
use collections::HashSet;
use db::kvp::KeyValueStore;
use gpui::{App, AppContext as _, Context, Entity, Global, Task};
use util::ResultExt as _;

use crate::branch_panel::state::{BRANCH_PANEL_KEY, SerializedBranchPanel, StoredKey};

/// Writes are coalesced behind this delay so opening five sections in a row is
/// one database round-trip rather than five.
const WRITE_THROTTLE: Duration = Duration::from_millis(500);

struct GlobalCheckoutViewState(Entity<CheckoutViewState>);

impl Global for GlobalCheckoutViewState {}

pub(crate) struct CheckoutViewState {
    /// Agent lists the reader opened. Absent means closed.
    expanded: HashSet<StoredKey>,
    /// Repositories the reader closed. Absent means open -- the opposite
    /// polarity to `expanded`, and deliberately so: a repository nobody has
    /// touched should draw open, an agent list nobody has touched should not.
    collapsed: HashSet<StoredKey>,
    pinned: Vec<String>,
    order: Vec<String>,
    /// Whether the shared record has been read. A per-workspace record is
    /// offered as a seed only while the shared one could still be the only
    /// thing on disk; after that it is history.
    loaded: bool,
    /// Whether the read found a record. A legacy blob must never overwrite
    /// state this build has already written.
    found_shared_record: bool,
    /// The throttled write in flight. Held rather than detached so dropping the
    /// global cancels it -- a detached write outlives what wanted it.
    pending_write: Option<Task<Option<()>>>,
}

impl CheckoutViewState {
    /// The record, created on first use.
    pub(crate) fn global(cx: &mut App) -> Entity<Self> {
        if let Some(global) = cx.try_global::<GlobalCheckoutViewState>() {
            return global.0.clone();
        }
        let state = cx.new(|_| Self::new());
        cx.set_global(GlobalCheckoutViewState(state.clone()));
        state
    }

    pub(crate) fn new() -> Self {
        Self {
            expanded: HashSet::default(),
            collapsed: HashSet::default(),
            pinned: Vec::new(),
            order: Vec::new(),
            loaded: false,
            found_shared_record: false,
            pending_write: None,
        }
    }

    /// Reads the shared record. Idempotent -- a second call after the first has
    /// landed does nothing, so every panel may await it.
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) -> Task<()> {
        if self.loaded {
            return Task::ready(());
        }
        let kvp = KeyValueStore::global(cx);
        cx.spawn(async move |this, cx| {
            let raw = cx
                .background_spawn(async move { kvp.read_kvp(BRANCH_PANEL_KEY) })
                .await
                .context("loading checkout view state")
                .log_err()
                .flatten();

            this.update(cx, |this, cx| {
                if let Some(raw) = raw
                    && let Some(record) = SerializedBranchPanel::parse(&raw)
                {
                    this.expanded = record.expanded;
                    this.collapsed = record.collapsed;
                    this.pinned = record.pinned;
                    this.order = record.order;
                    this.found_shared_record = true;
                }
                // Set whether or not a record was found: "we looked" is what
                // gates the legacy seed, not "we found something".
                this.loaded = true;
                cx.notify();
            })
            .ok();
        })
    }

    /// Folds a per-workspace record from an older build into the shared one.
    ///
    /// Ignored once a shared record was found: that record is this build's own
    /// output and is newer than anything a previous layout wrote. Two
    /// workspaces opening together can both find it absent and both seed, and
    /// the last write wins -- the same accepted race as
    /// `Dock::load_workspace_scoped_size_state`, and for the same reason: the
    /// key-value store has no compare-and-swap, and the cost is one wrong
    /// starting shape that a single gesture corrects.
    pub(crate) fn seed_from_legacy(&mut self, legacy: SerializedBranchPanel, cx: &mut Context<Self>) {
        if self.found_shared_record {
            return;
        }

        let mut changed = false;
        for key in legacy.expanded {
            // A `Repo` entry in `expanded` is an old blob's shape, where the
            // set meant "open" for both kinds. Dropping it here keeps the
            // polarity split from being undone by a seed.
            if matches!(key, StoredKey::Repo(_)) {
                continue;
            }
            changed |= self.expanded.insert(key);
        }
        for key in legacy.collapsed {
            changed |= self.collapsed.insert(key);
        }
        for path in legacy.pinned {
            if !self.pinned.contains(&path) {
                self.pinned.push(path);
                changed = true;
            }
        }
        // Merging two orders is not idempotent, so the first non-empty one wins
        // outright and later records only contribute paths it never named.
        // Interleaving them would invent an arrangement nobody chose.
        if self.order.is_empty() {
            changed |= !legacy.order.is_empty();
            self.order = legacy.order;
        } else {
            for path in legacy.order {
                if !self.order.contains(&path) {
                    self.order.push(path);
                    changed = true;
                }
            }
        }

        if changed {
            self.schedule_write(cx);
            cx.notify();
        }
    }

    /// Whether a row draws open.
    ///
    /// The two polarities, unchanged from the per-panel version they replace:
    /// a repository is open unless it was closed, an agent list is closed
    /// unless it was opened.
    pub(crate) fn is_open(&self, key: &StoredKey) -> bool {
        match key {
            StoredKey::Repo(_) => !self.collapsed.contains(key),
            StoredKey::WorktreeAgents(..) => self.expanded.contains(key),
        }
    }

    pub(crate) fn toggle(&mut self, key: StoredKey, cx: &mut Context<Self>) {
        let set = match key {
            StoredKey::Repo(_) => &mut self.collapsed,
            StoredKey::WorktreeAgents(..) => &mut self.expanded,
        };
        if !set.remove(&key) {
            set.insert(key);
        }
        self.schedule_write(cx);
        cx.notify();
    }

    pub(crate) fn pinned(&self) -> &[String] {
        &self.pinned
    }

    pub(crate) fn order(&self) -> &[String] {
        &self.order
    }

    pub(crate) fn set_pinned(&mut self, pinned: Vec<String>, cx: &mut Context<Self>) {
        if self.pinned == pinned {
            return;
        }
        self.pinned = pinned;
        self.schedule_write(cx);
        cx.notify();
    }

    pub(crate) fn set_order(&mut self, order: Vec<String>, cx: &mut Context<Self>) {
        if self.order == order {
            return;
        }
        self.order = order;
        self.schedule_write(cx);
        cx.notify();
    }

    /// Drops entries for checkouts of `repo_path` that no longer exist.
    ///
    /// The record is app-wide now, so without this it accumulates every
    /// worktree anyone ever opened. Scoped to the one repository whose live
    /// checkouts the caller can actually see -- a panel that cannot see another
    /// project must never delete that project's state.
    pub(crate) fn prune(
        &mut self,
        repo_path: &Path,
        live_checkouts: &[PathBuf],
        cx: &mut Context<Self>,
    ) {
        // `git worktree list` always names the main checkout, so an empty list
        // means the store is still warming up rather than that the worktrees
        // are gone. Pruning on that would erase the record on every launch.
        if live_checkouts.is_empty() {
            return;
        }
        let repo_path = repo_path.to_string_lossy().to_string();
        let live: HashSet<String> = live_checkouts
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect();

        let before = self.expanded.len();
        self.expanded.retain(|key| match key {
            StoredKey::WorktreeAgents(repo, worktree) => {
                *repo != repo_path || live.contains(worktree)
            }
            StoredKey::Repo(_) => true,
        });
        if self.expanded.len() != before {
            self.schedule_write(cx);
            cx.notify();
        }
    }

    fn schedule_write(&mut self, cx: &mut Context<Self>) {
        let record = SerializedBranchPanel {
            expanded: self.expanded.clone(),
            collapsed: self.collapsed.clone(),
            pinned: self.pinned.clone(),
            order: self.order.clone(),
        };
        // Resolved here rather than inside the task: the store is reachable
        // from this context, and reaching back for it after an await is how a
        // write outlives the app that wanted it.
        let kvp = KeyValueStore::global(cx);
        // Replacing the task drops the previous one, which cancels a write that
        // has not fired yet -- that is the throttle.
        self.pending_write = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(WRITE_THROTTLE).await;
            let value = serde_json::to_string(&record).log_err()?;
            cx.background_spawn(async move {
                kvp.write_kvp(BRANCH_PANEL_KEY.to_string(), value).await
            })
            .await
            .context("writing checkout view state")
            .log_err()
            .map(|_| ())
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn agents(repo: &str, worktree: &str) -> StoredKey {
        StoredKey::WorktreeAgents(repo.to_string(), worktree.to_string())
    }

    fn legacy(
        expanded: Vec<StoredKey>,
        pinned: Vec<&str>,
        order: Vec<&str>,
    ) -> SerializedBranchPanel {
        SerializedBranchPanel {
            expanded: expanded.into_iter().collect(),
            collapsed: HashSet::default(),
            pinned: pinned.into_iter().map(str::to_string).collect(),
            order: order.into_iter().map(str::to_string).collect(),
        }
    }

    /// Nothing recorded: a repository draws open, an agent list draws closed.
    /// Collapsing the two into one set breaks one of them, which is why they
    /// are two.
    #[gpui::test]
    fn polarity_differs_between_the_two_kinds(cx: &mut TestAppContext) {
        let state = cx.new(|_| CheckoutViewState::new());
        state.read_with(cx, |state, _| {
            assert!(
                state.is_open(&StoredKey::Repo("/repos/zode".into())),
                "a repository nobody has closed draws open"
            );
            assert!(
                !state.is_open(&agents("/repos/zode", "/repos/zode")),
                "an agent list nobody has opened draws closed"
            );
        });
    }

    /// Toggling flips only the set its own kind governs.
    #[gpui::test]
    fn toggle_writes_to_the_set_its_kind_governs(cx: &mut TestAppContext) {
        let state = cx.new(|_| CheckoutViewState::new());
        let key = agents("/repos/zode", "/repos/zode");
        state.update(cx, |state, cx| {
            state.toggle(key.clone(), cx);
            assert!(state.is_open(&key), "opened");
            assert!(state.collapsed.is_empty(), "the repo set must be untouched");
            state.toggle(key.clone(), cx);
            assert!(!state.is_open(&key), "and closed again");
        });
    }

    /// A record from the old per-workspace key seeds the shared one while the
    /// shared one is still absent.
    #[gpui::test]
    fn a_legacy_record_seeds_an_empty_shared_record(cx: &mut TestAppContext) {
        let state = cx.new(|_| CheckoutViewState::new());
        let key = agents("/repos/zode", "/repos/zode");
        state.update(cx, |state, cx| {
            state.seed_from_legacy(legacy(vec![key.clone()], vec!["/repos/zode"], vec![]), cx);
            assert!(state.is_open(&key), "the legacy entry must carry over");
            assert_eq!(state.pinned(), ["/repos/zode"]);
        });
    }

    /// Once the shared record has been read and found present, a legacy blob is
    /// history and must not overwrite it.
    #[gpui::test]
    fn a_legacy_record_is_ignored_once_the_shared_one_exists(cx: &mut TestAppContext) {
        let state = cx.new(|_| CheckoutViewState::new());
        let stale = agents("/repos/zode", "/repos/old");
        state.update(cx, |state, cx| {
            state.found_shared_record = true;
            state.loaded = true;
            state.seed_from_legacy(legacy(vec![stale.clone()], vec!["/repos/old"], vec![]), cx);
            assert!(
                !state.is_open(&stale),
                "a per-workspace record must not resurrect state the shared one has moved past"
            );
            assert!(state.pinned().is_empty());
        });
    }

    /// Two legacy records merge, and the first non-empty order wins outright —
    /// interleaving two orders invents an arrangement nobody chose.
    #[gpui::test]
    fn merging_legacy_orders_keeps_the_first_and_appends_the_rest(cx: &mut TestAppContext) {
        let state = cx.new(|_| CheckoutViewState::new());
        state.update(cx, |state, cx| {
            state.seed_from_legacy(legacy(vec![], vec![], vec!["/a", "/b"]), cx);
            state.seed_from_legacy(legacy(vec![], vec![], vec!["/b", "/c"]), cx);
            assert_eq!(
                state.order(),
                ["/a", "/b", "/c"],
                "the first order stands and the second only contributes what it did not name"
            );
        });
    }

    /// A checkout that no longer exists stops being remembered, or the app-wide
    /// record grows for ever.
    #[gpui::test]
    fn prune_drops_a_checkout_that_is_gone(cx: &mut TestAppContext) {
        let state = cx.new(|_| CheckoutViewState::new());
        let live = agents("/repos/zode", "/repos/zode");
        let gone = agents("/repos/zode", "/repos/zode-old");
        state.update(cx, |state, cx| {
            state.toggle(live.clone(), cx);
            state.toggle(gone.clone(), cx);
            state.prune(
                Path::new("/repos/zode"),
                &[PathBuf::from("/repos/zode")],
                cx,
            );
            assert!(state.is_open(&live), "the surviving checkout keeps its state");
            assert!(!state.is_open(&gone), "the removed one is forgotten");
        });
    }

    /// Pruning one repository must never reach into another's entries — a panel
    /// that cannot see a project has no standing to delete its state.
    #[gpui::test]
    fn prune_never_touches_another_repository(cx: &mut TestAppContext) {
        let state = cx.new(|_| CheckoutViewState::new());
        let elsewhere = agents("/repos/other", "/repos/other-wt");
        state.update(cx, |state, cx| {
            state.toggle(elsewhere.clone(), cx);
            state.prune(
                Path::new("/repos/zode"),
                &[PathBuf::from("/repos/zode")],
                cx,
            );
            assert!(
                state.is_open(&elsewhere),
                "another project's state survives a prune it was never part of"
            );
        });
    }

    /// A blob written before repositories recorded their own closure lists the
    /// ones that were *open* -- `Repo` entries under the old, single-set
    /// design landed in `expanded`. Those entries govern nothing here:
    /// `is_open` for a `Repo` key never consults `expanded`, only `collapsed`.
    /// Seeding them in anyway would carry them forward forever, rewritten on
    /// every save for the life of the record, for a key nothing ever reads.
    #[gpui::test]
    fn a_stray_repo_entry_in_legacy_expanded_is_dropped(cx: &mut TestAppContext) {
        let state = cx.new(|_| CheckoutViewState::new());
        let repo = StoredKey::Repo("/repos/zode".to_string());
        state.update(cx, |state, cx| {
            state.seed_from_legacy(legacy(vec![repo.clone()], vec![], vec![]), cx);
            assert!(
                !state.expanded.contains(&repo),
                "a repository entry has no meaning in the opened set and must be dropped, \
                 not carried forward under the new record"
            );
            assert!(
                state.is_open(&repo),
                "dropping it leaves the repository at its default, which is open"
            );
        });
    }

    /// An empty checkout list means the git store is still warming up, not that
    /// every worktree vanished. Pruning on it would erase the record on launch.
    #[gpui::test]
    fn prune_with_no_live_checkouts_changes_nothing(cx: &mut TestAppContext) {
        let state = cx.new(|_| CheckoutViewState::new());
        let key = agents("/repos/zode", "/repos/zode");
        state.update(cx, |state, cx| {
            state.toggle(key.clone(), cx);
            state.prune(Path::new("/repos/zode"), &[], cx);
            assert!(
                state.is_open(&key),
                "a warm-up rebuild must not be read as every worktree having been removed"
            );
        });
    }
}
