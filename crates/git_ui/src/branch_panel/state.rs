//! What the panel remembers across restarts.
//!
//! Deliberately only the shape of the tree, never its contents: branches,
//! worktrees and stashes are read from the git store on every build, so a stale
//! cache can never contradict the repository.

use std::time::Duration;

use anyhow::Context as _;
use collections::HashSet;
use db::kvp::KeyValueStore;
use gpui::{AppContext as _, AsyncApp, AsyncWindowContext, WeakEntity};
use serde::{Deserialize, Serialize};
use util::ResultExt as _;
use workspace::Workspace;

use project::git_store::RepositoryId;

use crate::branch_panel::tree::RowKey;

pub(crate) const BRANCH_PANEL_KEY: &str = "BranchPanel";

/// Writes are coalesced behind this delay so opening five sections in a row is
/// one database round-trip rather than five.
const SERIALIZATION_THROTTLE: Duration = Duration::from_millis(500);

/// A `RowKey` carries a `RepositoryId`, which is assigned per session and means
/// nothing after a restart. What survives is the repository's *path* plus which
/// section it was, so reopening the same project restores the same shape.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum StoredKey {
    Repo(String),
    /// A checkout whose agents are showing, by its own absolute path.
    WorktreeAgents(String, String),
}

#[derive(Serialize, Debug, Default)]
pub(crate) struct SerializedBranchPanel {
    pub(crate) expanded: HashSet<StoredKey>,
    /// Checkouts pinned to the top, by absolute path.
    #[serde(default)]
    pub(crate) pinned: Vec<String>,
    /// The order the reader dragged checkouts into. Only what they moved.
    #[serde(default)]
    pub(crate) order: Vec<String>,
}

/// The wire shape, read entry by entry.
///
/// Serde rejects the whole enum on an unknown variant, so a blob written by a
/// build that had rows this one does not -- every blob written before the tree
/// became a list of checkouts -- would throw away the entries this build *can*
/// still read, and log an error on every start until something overwrote it.
/// Reading into `Value` first drops only the entries that no longer mean
/// anything.
#[derive(Deserialize, Default)]
struct RawSerializedBranchPanel {
    #[serde(default)]
    expanded: Vec<serde_json::Value>,
    #[serde(default)]
    pinned: Vec<String>,
    #[serde(default)]
    order: Vec<String>,
}

impl From<RawSerializedBranchPanel> for SerializedBranchPanel {
    fn from(raw: RawSerializedBranchPanel) -> Self {
        Self {
            expanded: raw
                .expanded
                .into_iter()
                .filter_map(|entry| serde_json::from_value::<StoredKey>(entry).ok())
                .collect(),
            pinned: raw.pinned,
            order: raw.order,
        }
    }
}

impl StoredKey {
    /// Resolves a live row key against the repository paths of this session.
    pub(crate) fn from_row_key(key: &RowKey, repo_path: &str) -> Self {
        match key {
            RowKey::Repo(_) => StoredKey::Repo(repo_path.to_string()),
            RowKey::WorktreeAgents(_, path) => {
                StoredKey::WorktreeAgents(repo_path.to_string(), path.to_string_lossy().to_string())
            }
        }
    }

    /// Turns a stored entry back into a live key, if it names this repository.
    ///
    /// Returns `None` for another repository's entry, and for a shape this
    /// build no longer has -- a stored key from a future version must be
    /// ignored, not panic.
    pub(crate) fn to_row_key(&self, id: RepositoryId, repo_path: &str) -> Option<RowKey> {
        match self {
            StoredKey::Repo(path) if path == repo_path => Some(RowKey::Repo(id)),
            StoredKey::WorktreeAgents(path, worktree) if path == repo_path => Some(
                RowKey::WorktreeAgents(id, std::sync::Arc::from(std::path::Path::new(worktree))),
            ),
            _ => None,
        }
    }
}

impl SerializedBranchPanel {
    fn serialization_key(workspace: &Workspace) -> Option<String> {
        workspace
            .database_id()
            .map(|id| i64::from(id).to_string())
            .or(workspace.session_id())
            .map(|id| format!("{BRANCH_PANEL_KEY}-{id:?}"))
    }

    pub(crate) async fn load(
        workspace: &WeakEntity<Workspace>,
        cx: &mut AsyncWindowContext,
    ) -> Option<Self> {
        let (key, kvp) = workspace
            .read_with(cx, |workspace, cx| {
                Self::serialization_key(workspace).map(|key| (key, KeyValueStore::global(cx)))
            })
            .ok()
            .flatten()?;

        let raw: String = cx
            .background_spawn(async move { kvp.read_kvp(&key) })
            .await
            .context("loading branch panel state")
            .log_err()
            .flatten()?;

        serde_json::from_str::<RawSerializedBranchPanel>(&raw)
            .log_err()
            .map(Self::from)
    }

    pub(crate) async fn write(
        self,
        workspace: WeakEntity<Workspace>,
        cx: &mut AsyncApp,
    ) -> Option<()> {
        cx.background_executor().timer(SERIALIZATION_THROTTLE).await;

        let (key, kvp) = workspace
            .read_with(cx, |workspace, cx| {
                Self::serialization_key(workspace).map(|key| (key, KeyValueStore::global(cx)))
            })
            .ok()
            .flatten()?;

        let value = serde_json::to_string(&self).log_err()?;
        cx.background_spawn(async move { kvp.write_kvp(key, value).await })
            .await
            .context("writing branch panel state")
            .log_err()
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::{RawSerializedBranchPanel, SerializedBranchPanel, StoredKey};

    /// A blob written before the tree became a list of checkouts carries
    /// `Section` entries this build has never heard of. It must lose those and
    /// keep the rest, rather than losing everything -- which is what serde does
    /// on its own, because an unknown variant fails the whole enum.
    #[test]
    fn an_entry_from_an_older_shape_is_dropped_not_fatal() {
        let raw = r#"{"expanded":[
            {"Section":["/repos/zode","Local"]},
            {"Repo":"/repos/zode"}
        ]}"#;

        let parsed: SerializedBranchPanel = serde_json::from_str::<RawSerializedBranchPanel>(raw)
            .expect("the outer shape still parses")
            .into();

        assert_eq!(parsed.expanded.len(), 1, "the unknown entry is dropped");
        assert!(
            parsed
                .expanded
                .contains(&StoredKey::Repo("/repos/zode".into())),
            "and the one this build understands survives"
        );
    }

    #[test]
    fn a_blob_of_only_unknown_entries_reads_as_empty() {
        let raw = r#"{"expanded":[{"Tag":["/repos/zode","v1"]}]}"#;

        let parsed: SerializedBranchPanel = serde_json::from_str::<RawSerializedBranchPanel>(raw)
            .expect("the outer shape still parses")
            .into();

        assert!(parsed.expanded.is_empty());
    }

    #[test]
    fn a_round_trip_keeps_what_it_wrote() {
        let mut expanded = collections::HashSet::default();
        expanded.insert(StoredKey::WorktreeAgents(
            "/repos/zode".into(),
            "/wt/feature".into(),
        ));
        let written = serde_json::to_string(&SerializedBranchPanel {
            expanded,
            pinned: vec!["/wt/feature".into()],
            order: vec!["/wt/feature".into(), "/repos/zode".into()],
        })
        .unwrap();

        let parsed: SerializedBranchPanel =
            serde_json::from_str::<RawSerializedBranchPanel>(&written)
                .unwrap()
                .into();

        assert_eq!(parsed.expanded.len(), 1);
        assert_eq!(parsed.pinned, vec!["/wt/feature".to_string()]);
        assert_eq!(parsed.order.len(), 2);
    }

    /// A blob written before pinning existed has neither field. It must read
    /// as "nothing pinned, nothing reordered" rather than failing.
    #[test]
    fn a_blob_without_the_newer_fields_still_reads() {
        let raw = r#"{"expanded":[{"Repo":"/repos/zode"}]}"#;

        let parsed: SerializedBranchPanel = serde_json::from_str::<RawSerializedBranchPanel>(raw)
            .expect("the outer shape still parses")
            .into();

        assert!(parsed.pinned.is_empty());
        assert!(parsed.order.is_empty());
    }
}
