//! What the panel remembers across restarts.
//!
//! Deliberately only the shape of the tree, never its contents: branches,
//! worktrees and stashes are read from the git store on every build, so a stale
//! cache can never contradict the repository.

use anyhow::Context as _;
use collections::HashSet;
use db::kvp::KeyValueStore;
use gpui::{AppContext as _, AsyncWindowContext, WeakEntity};
use serde::{Deserialize, Serialize};
use util::ResultExt as _;
use workspace::Workspace;

use crate::branch_panel::tree::RowKey;

pub(crate) const BRANCH_PANEL_KEY: &str = "BranchPanel";

/// A `RowKey` carries a `RepositoryId`, which is assigned per session and means
/// nothing after a restart. What survives is the repository's *path* plus which
/// section it was, so reopening the same project restores the same shape.
///
/// The live set is keyed this way too, not only the stored one. A session id is
/// not the only thing a `RepositoryId` fails to survive: switching checkout
/// builds a second `Workspace` and a second panel, and an id minted in one says
/// nothing in the other.
///
/// Which path, though, is the part that has to be got right. The repository
/// component is `RepoData::anchor` -- the *original* repository's working
/// directory, the same from every checkout -- and never the checkout the panel
/// happens to be open at. Keying it by the latter is what made a reader's open
/// agent list close itself on every checkout switch: the key they wrote while
/// standing in one checkout was not the key the next panel looked up.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum StoredKey {
    /// A repository, by the absolute path of its original checkout.
    Repo(String),
    /// A checkout whose agents are showing: its repository's anchor, then the
    /// checkout's own absolute path.
    WorktreeAgents(String, String),
}

#[derive(Serialize, Debug, Default)]
pub(crate) struct SerializedBranchPanel {
    pub(crate) expanded: HashSet<StoredKey>,
    /// Repositories the reader closed. The opposite polarity to `expanded`,
    /// and a separate field rather than a flipped meaning for the same one:
    /// a blob written before this existed lists the repositories that were
    /// open, and reading those as closed would shut the panel on exactly the
    /// people who had it open.
    #[serde(default)]
    pub(crate) collapsed: HashSet<StoredKey>,
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
    collapsed: Vec<serde_json::Value>,
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
            collapsed: raw
                .collapsed
                .into_iter()
                .filter_map(|entry| serde_json::from_value::<StoredKey>(entry).ok())
                .collect(),
            pinned: raw.pinned,
            order: raw.order,
        }
    }
}

impl StoredKey {
    /// Resolves a live row key against the repository anchors of this session.
    ///
    /// `repo_anchor` must be `RepoData::anchor`. Handing it a checkout path
    /// compiles and reads as correct -- and re-keys every row the next time the
    /// reader switches checkout.
    pub(crate) fn from_row_key(key: &RowKey, repo_anchor: &str) -> Self {
        match key {
            RowKey::Repo(_) => StoredKey::Repo(repo_anchor.to_string()),
            RowKey::WorktreeAgents(_, path) => StoredKey::WorktreeAgents(
                repo_anchor.to_string(),
                path.to_string_lossy().to_string(),
            ),
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

    /// Parses a stored blob, dropping only the entries this build no longer
    /// understands. Shared with `CheckoutViewState`, which reads the same shape
    /// from the un-scoped key.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        serde_json::from_str::<RawSerializedBranchPanel>(raw)
            .log_err()
            .map(Self::from)
    }

    /// Reads the legacy, workspace-scoped record -- read-only from here on.
    /// `CheckoutViewState` is the one writer now; a per-workspace blob is only
    /// ever offered to it as a seed (`seed_from_legacy`), never written back
    /// to its own key. Leaving the old rows unwritten-to but still readable is
    /// the same choice `Dock::load_workspace_scoped_size_state` made, and for
    /// the same reason: a build from before this change still reads them.
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
        let mut collapsed = collections::HashSet::default();
        collapsed.insert(StoredKey::Repo("/repos/zode".into()));
        let written = serde_json::to_string(&SerializedBranchPanel {
            expanded,
            collapsed,
            pinned: vec!["/wt/feature".into()],
            order: vec!["/wt/feature".into(), "/repos/zode".into()],
        })
        .unwrap();

        let parsed: SerializedBranchPanel =
            serde_json::from_str::<RawSerializedBranchPanel>(&written)
                .unwrap()
                .into();

        assert_eq!(parsed.expanded.len(), 1);
        assert_eq!(
            parsed.collapsed.len(),
            1,
            "a repository the reader closed has to survive the restart too"
        );
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
        assert!(
            parsed.collapsed.is_empty(),
            "a blob from before repositories could be closed must read as none \
             closed -- reading its `expanded` list the other way round would \
             shut the panel on whoever had it open"
        );
    }
}
