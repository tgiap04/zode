//! Which checkouts the reader has switched the permission prompts off for.
//!
//! Keyed by the checkout's own path, because a checkout is a directory on disk
//! shared by every window that opens it -- the same reasoning, and the same
//! defect avoided, as `git_ui`'s `CheckoutViewState`: a record keyed by
//! workspace makes switching checkout open a different drawer.
//!
//! The repository anchor rides along as the *value*, never as the key. It is
//! `original_repo_abs_path` and never `work_directory_abs_path`, which is the
//! checkout you happen to be standing in; `prune` is the only thing that reads
//! it, and it is the only thing that needs to scope itself to one repository.
//!
//! This record is the only input that decides whether a safety control is off,
//! so what can write it matters more than what it holds. One caller writes it:
//! a menu action the reader confirmed. No settings key reaches it, so nothing a
//! repository carries can switch it on.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;
use collections::HashMap;
use db::kvp::KeyValueStore;
use gpui::{App, AppContext as _, Context, Entity, Global, Task};
use serde::{Deserialize, Serialize};
use util::ResultExt as _;

/// One un-scoped key. No workspace id: see the module doc.
const BYPASS_KEY: &str = "AgentPermissionBypass";

/// Writes are coalesced behind this delay, as `CheckoutViewState` does, so
/// toggling several checkouts in a row is one database round-trip.
const WRITE_THROTTLE: Duration = Duration::from_millis(500);

struct GlobalPermissionBypass(Entity<PermissionBypassStore>);

impl Global for GlobalPermissionBypass {}

/// The wire shape. A map rather than a list of pairs, so writing the same
/// checkout twice cannot record it twice.
#[derive(Default, Serialize, Deserialize)]
struct SerializedBypass {
    /// checkout path -> repository anchor.
    #[serde(default)]
    enabled: HashMap<String, String>,
}

pub struct PermissionBypassStore {
    enabled: HashMap<String, String>,
    /// Whether the record has been read. A write before the read has landed
    /// would persist an empty map over whatever is on disk.
    loaded: bool,
    /// Held rather than detached, so dropping the global cancels a write that
    /// has not fired yet.
    pending_write: Option<Task<Option<()>>>,
}

impl PermissionBypassStore {
    /// The record, created on first use.
    pub fn global(cx: &mut App) -> Entity<Self> {
        if let Some(global) = cx.try_global::<GlobalPermissionBypass>() {
            return global.0.clone();
        }
        let store = cx.new(|_| Self::new());
        cx.set_global(GlobalPermissionBypass(store.clone()));
        store
    }

    pub fn new() -> Self {
        Self {
            enabled: HashMap::default(),
            loaded: false,
            pending_write: None,
        }
    }

    /// Reads the record. Idempotent, so every caller may await it.
    pub fn load(&mut self, cx: &mut Context<Self>) -> Task<()> {
        if self.loaded {
            return Task::ready(());
        }
        let kvp = KeyValueStore::global(cx);
        cx.spawn(async move |this, cx| {
            let raw = cx
                .background_spawn(async move { kvp.read_kvp(BYPASS_KEY) })
                .await
                .context("loading the agent permission bypass record")
                .log_err()
                .flatten();

            this.update(cx, |this, cx| {
                if let Some(record) = raw
                    .as_deref()
                    .and_then(|raw| serde_json::from_str::<SerializedBypass>(raw).log_err())
                {
                    // Merged, not assigned. Anything already in memory when the
                    // read lands is a gesture the reader confirmed in the
                    // meantime, and the disk is the older of the two.
                    let mut merged = record.enabled;
                    merged.extend(std::mem::take(&mut this.enabled));
                    this.enabled = merged;
                }
                // Set whether or not anything was found: what gates a write is
                // "we looked", not "we found something".
                this.loaded = true;
                cx.notify();
            })
            .ok();
        })
    }

    /// Whether agents started in `checkout` should skip their permission
    /// prompts. A checkout nobody has touched answers `false`.
    pub fn is_enabled(&self, checkout: &Path) -> bool {
        self.enabled
            .contains_key(checkout.to_string_lossy().as_ref())
    }

    /// `anchor` must be the repository's `original_repo_abs_path`. It is stored
    /// for `prune` alone.
    pub fn set(&mut self, checkout: &Path, anchor: &Path, on: bool, cx: &mut Context<Self>) {
        let checkout = checkout.to_string_lossy().to_string();
        let changed = if on {
            let anchor = anchor.to_string_lossy().to_string();
            // Compared by value, not by presence: re-enabling with a corrected
            // anchor changes the map, and `is_none()` would call that no change
            // and never schedule the write.
            self.enabled.insert(checkout, anchor.clone()) != Some(anchor)
        } else {
            // Removed rather than stored as `false`: a record that keeps its
            // disabled entries grows by one for every checkout anyone ever
            // toggled twice, and never shrinks.
            self.enabled.remove(&checkout).is_some()
        };
        if changed {
            self.schedule_write(cx);
            cx.notify();
        }
    }

    /// Drops entries for checkouts of `anchor` that no longer exist, and touches
    /// nothing belonging to any other repository -- a panel that cannot see a
    /// project has no standing to delete its state.
    pub fn prune(&mut self, anchor: &Path, live_checkouts: &[PathBuf], cx: &mut Context<Self>) {
        // `git worktree list` always names the main checkout, so an empty list
        // means the store is still warming up rather than that the worktrees are
        // gone. Pruning on it erases the record on every launch.
        if live_checkouts.is_empty() {
            return;
        }
        let anchor = anchor.to_string_lossy().to_string();
        let live: collections::HashSet<String> = live_checkouts
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect();

        let before = self.enabled.len();
        self.enabled
            .retain(|checkout, entry_anchor| *entry_anchor != anchor || live.contains(checkout));
        if self.enabled.len() != before {
            self.schedule_write(cx);
            cx.notify();
        }
    }

    fn schedule_write(&mut self, cx: &mut Context<Self>) {
        // Resolved here rather than after the await: reaching back for the store
        // once the task is running is how a write outlives the app that wanted it.
        let kvp = KeyValueStore::global(cx);
        // Replacing the task drops the previous one, which cancels a write that
        // has not fired yet. That is the throttle.
        self.pending_write = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(WRITE_THROTTLE).await;
            // Read after the delay, never snapshotted before it. A snapshot is
            // the map as it was when the *first* of the coalesced gestures
            // happened, so anything that changed during the throttle -- another
            // toggle, or `load` merging the disk in -- would be written back out
            // of existence by a write that predates it.
            let record = this
                .read_with(cx, |this, _| SerializedBypass {
                    enabled: this.enabled.clone(),
                })
                .ok()?;
            let value = serde_json::to_string(&record).log_err()?;
            cx.background_spawn(async move { kvp.write_kvp(BYPASS_KEY.to_string(), value).await })
                .await
                .context("writing the agent permission bypass record")
                .log_err()
                .map(|_| ())
        }));
    }
}

impl Default for PermissionBypassStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    /// A database of this test's own. `AppDatabase::global` otherwise falls back
    /// to a process-wide static, and since this record lives under one un-scoped
    /// key every test in the binary would then read and write the same row --
    /// which is a flake, not a suite. The same trap `checkout_state`'s tests hit.
    fn init_test(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_global(db::AppDatabase::test_new()));
    }

    fn store(cx: &mut TestAppContext) -> Entity<PermissionBypassStore> {
        init_test(cx);
        cx.new(|_| PermissionBypassStore::new())
    }

    /// The safe default, and the only one that can be wrong in the dangerous
    /// direction.
    #[gpui::test]
    fn a_checkout_nobody_touched_has_no_bypass(cx: &mut TestAppContext) {
        let store = store(cx);
        store.read_with(cx, |store, _| {
            assert!(!store.is_enabled(Path::new("/repos/zode/wt-a")));
        });
    }

    /// The anchor has to survive the round trip or `prune` loses its scoping
    /// after one restart and starts deleting other repositories' entries.
    #[gpui::test]
    fn the_record_survives_a_round_trip(cx: &mut TestAppContext) {
        let store = store(cx);
        store.update(cx, |store, cx| {
            store.set(
                Path::new("/repos/zode/wt-a"),
                Path::new("/repos/zode"),
                true,
                cx,
            );
        });

        let json = store.read_with(cx, |store, _| {
            serde_json::to_string(&SerializedBypass {
                enabled: store.enabled.clone(),
            })
            .expect("the record must serialise")
        });
        let parsed: SerializedBypass =
            serde_json::from_str(&json).expect("and parse back into itself");

        assert_eq!(
            parsed.enabled.get("/repos/zode/wt-a").map(String::as_str),
            Some("/repos/zode"),
            "the checkout is the key and its repository anchor is the value"
        );
    }

    /// The whole reason the anchor is stored at all.
    #[gpui::test]
    fn prune_drops_a_gone_checkout_and_leaves_another_repository_alone(cx: &mut TestAppContext) {
        let store = store(cx);
        store.update(cx, |store, cx| {
            store.set(Path::new("/repos/a/gone"), Path::new("/repos/a"), true, cx);
            store.set(Path::new("/repos/a/live"), Path::new("/repos/a"), true, cx);
            store.set(Path::new("/repos/b/live"), Path::new("/repos/b"), true, cx);

            store.prune(Path::new("/repos/a"), &[PathBuf::from("/repos/a/live")], cx);

            assert!(!store.is_enabled(Path::new("/repos/a/gone")), "removed");
            assert!(store.is_enabled(Path::new("/repos/a/live")), "still there");
            assert!(
                store.is_enabled(Path::new("/repos/b/live")),
                "another project's entry is not this panel's to delete"
            );
        });
    }

    /// `git worktree list` always names the main checkout, so an empty list is
    /// the store warming up. Pruning on it erases the record on every launch.
    #[gpui::test]
    fn an_empty_live_list_prunes_nothing(cx: &mut TestAppContext) {
        let store = store(cx);
        store.update(cx, |store, cx| {
            store.set(Path::new("/repos/a/wt"), Path::new("/repos/a"), true, cx);
            store.prune(Path::new("/repos/a"), &[], cx);
            assert!(store.is_enabled(Path::new("/repos/a/wt")));
        });
    }

    /// The Critical defect review found, pinned.
    ///
    /// The panel reads this record to draw its mark and build its menu, and it
    /// was not loading it -- so after a restart every checkout read as "off",
    /// and the first toggle serialised a one-entry map over the real record.
    /// Two halves failed together: the panel did not call `load` (fixed in
    /// `git_ui`, held by compilation), and `load` *assigned* where it had to
    /// merge, so even a panel that did load could clobber a gesture the reader
    /// confirmed while the read was in flight.
    ///
    /// This half is the merge. Falsifiable by restoring `this.enabled =
    /// record.enabled`: the confirmed entry disappears.
    #[gpui::test]
    async fn a_gesture_made_before_the_read_lands_survives_it(cx: &mut TestAppContext) {
        init_test(cx);
        let on_disk = SerializedBypass {
            enabled: [("/repos/other/wt".to_string(), "/repos/other".to_string())]
                .into_iter()
                .collect(),
        };
        cx.update(|cx| db::kvp::KeyValueStore::global(cx))
            .write_kvp(
                BYPASS_KEY.to_string(),
                serde_json::to_string(&on_disk).expect("the record must serialise"),
            )
            .await
            .expect("seeding the record must succeed");

        let store = cx.new(|_| PermissionBypassStore::new());
        // Confirmed before the read has landed -- the reader clicked while the
        // app was still starting.
        store.update(cx, |store, cx| {
            store.set(
                Path::new("/repos/zode/wt"),
                Path::new("/repos/zode"),
                true,
                cx,
            );
        });
        store.update(cx, |store, cx| store.load(cx)).await;

        store.read_with(cx, |store, _| {
            assert!(
                store.is_enabled(Path::new("/repos/zode/wt")),
                "a read that lands after a confirmed gesture must not undo it"
            );
            assert!(
                store.is_enabled(Path::new("/repos/other/wt")),
                "and it must not lose what was already on disk either"
            );
        });

        // And the same has to be true on disk. This is the only test that reads
        // the record back out; without it the write half of this module is
        // exercised by nothing at all.
        cx.executor().advance_clock(WRITE_THROTTLE * 2);
        cx.run_until_parked();
        let written = cx
            .update(|cx| db::kvp::KeyValueStore::global(cx))
            .read_kvp(BYPASS_KEY)
            .expect("the record must be readable")
            .expect("and must have been written");
        let written: SerializedBypass =
            serde_json::from_str(&written).expect("and must parse back");
        assert!(
            written.enabled.contains_key("/repos/zode/wt")
                && written.enabled.contains_key("/repos/other/wt"),
            "the persisted map must hold both, got {:?}",
            written.enabled
        );
    }

    /// Storing `false` would grow the record by one for every checkout anyone
    /// ever toggled twice, and it would never shrink.
    #[gpui::test]
    fn turning_it_off_removes_the_entry_rather_than_storing_false(cx: &mut TestAppContext) {
        let store = store(cx);
        store.update(cx, |store, cx| {
            let checkout = Path::new("/repos/a/wt");
            store.set(checkout, Path::new("/repos/a"), true, cx);
            store.set(checkout, Path::new("/repos/a"), false, cx);

            assert!(!store.is_enabled(checkout));
            assert!(
                store.enabled.is_empty(),
                "off means gone, not recorded as off; got {:?}",
                store.enabled
            );
        });
    }
}
