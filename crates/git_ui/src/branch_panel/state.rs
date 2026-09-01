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

use crate::branch_panel::tree::{RowKey, SectionKind};

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
    Section(String, String),
    RemoteGroup(String, String),
    BranchAgents(String, String),
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub(crate) struct SerializedBranchPanel {
    pub(crate) expanded: HashSet<StoredKey>,
}

impl StoredKey {
    /// Resolves a live row key against the repository paths of this session.
    pub(crate) fn from_row_key(key: &RowKey, repo_path: &str) -> Self {
        match key {
            RowKey::Repo(_) => StoredKey::Repo(repo_path.to_string()),
            RowKey::Section(_, kind) => {
                StoredKey::Section(repo_path.to_string(), kind.label().to_string())
            }
            RowKey::RemoteGroup(_, remote) => {
                StoredKey::RemoteGroup(repo_path.to_string(), remote.to_string())
            }
            RowKey::BranchAgents(_, branch) => {
                StoredKey::BranchAgents(repo_path.to_string(), branch.to_string())
            }
        }
    }

    /// Turns a stored entry back into a live key, if it names this repository.
    ///
    /// Returns `None` for another repository's entry, and for a section label
    /// this build no longer has -- a stored key from a future version must be
    /// ignored, not panic.
    pub(crate) fn to_row_key(&self, id: RepositoryId, repo_path: &str) -> Option<RowKey> {
        match self {
            StoredKey::Repo(path) if path == repo_path => Some(RowKey::Repo(id)),
            StoredKey::Section(path, label) if path == repo_path => SectionKind::ALL
                .into_iter()
                .find(|kind| kind.label() == label)
                .map(|kind| RowKey::Section(id, kind)),
            StoredKey::RemoteGroup(path, remote) if path == repo_path => {
                Some(RowKey::RemoteGroup(id, remote.clone().into()))
            }
            StoredKey::BranchAgents(path, branch) if path == repo_path => {
                Some(RowKey::BranchAgents(id, branch.clone().into()))
            }
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

        serde_json::from_str::<Self>(&raw).log_err()
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
