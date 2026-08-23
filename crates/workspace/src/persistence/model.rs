use super::{SerializedAxis, SerializedWindowBounds};
use crate::{
    Member, Pane, PaneAxis, SerializableItemRegistry, Workspace, WorkspaceId, item::ItemHandle,
    multi_workspace::SerializedProjectGroupState, path_list::PathList,
};
use anyhow::{Context, Result};
use async_recursion::async_recursion;
use collections::IndexSet;
use db::sqlez::{
    bindable::{Bind, Column, StaticColumnCount},
    statement::Statement,
};
use gpui::{AsyncWindowContext, Entity, SharedString, WeakEntity, WindowId};

use language::{Toolchain, ToolchainScope};
use project::{
    Project, ProjectGroupKey, bookmark_store::SerializedBookmark,
    debugger::breakpoint_store::SourceBreakpoint,
};
use remote::RemoteConnectionOptions;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use util::{ResultExt, path_list::SerializedPathList};
use uuid::Uuid;

#[derive(
    Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct RemoteConnectionId(pub u64);

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum RemoteConnectionKind {
    Ssh,
    Wsl,
    Docker,
}

#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub enum SerializedWorkspaceLocation {
    Local,
    Remote(RemoteConnectionOptions),
}

impl SerializedWorkspaceLocation {
    /// Get sorted paths
    pub fn sorted_paths(&self) -> Arc<Vec<PathBuf>> {
        unimplemented!()
    }
}

/// A workspace entry from a previous session, containing all the info needed
/// to restore it including which window it belonged to (for MultiWorkspace grouping).
#[derive(Debug, PartialEq, Clone)]
pub struct SessionWorkspace {
    pub workspace_id: WorkspaceId,
    pub location: SerializedWorkspaceLocation,
    pub paths: PathList,
    pub window_id: Option<WindowId>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SerializedProjectGroup {
    pub path_list: SerializedPathList,
    pub(crate) location: SerializedWorkspaceLocation,
    #[serde(default = "default_expanded")]
    pub expanded: bool,
    /// What the avatar draws instead of initials derived from the name.
    ///
    /// `serde(default)` and not `Option`-by-accident: records written before
    /// this field existed are the common case, not an error case, and they must
    /// keep loading.
    #[serde(default)]
    pub initials: Option<String>,
    /// `#RRGGBB`, or `None` for the default panel background.
    ///
    /// Text rather than a parsed colour, because this string is what a person
    /// might edit by hand and an unparsable value has to be refusable.
    #[serde(default)]
    pub colour: Option<String>,
}

fn default_expanded() -> bool {
    true
}

impl SerializedProjectGroup {
    /// Takes the whole state rather than a field list.
    ///
    /// There is exactly one caller, and every presentation field added later
    /// would otherwise change this signature again.
    pub fn from_group(group: &crate::multi_workspace::ProjectGroupState) -> Self {
        Self {
            path_list: group.key.path_list().serialize(),
            location: match group.key.host() {
                Some(host) => SerializedWorkspaceLocation::Remote(host),
                None => SerializedWorkspaceLocation::Local,
            },
            expanded: group.expanded,
            initials: group.initials.as_ref().map(|initials| initials.to_string()),
            colour: group.colour.map(crate::project_appearance::colour_to_hex),
        }
    }

    pub fn into_restored_state(self) -> SerializedProjectGroupState {
        let path_list = PathList::deserialize(&self.path_list);
        let host = match self.location {
            SerializedWorkspaceLocation::Local => None,
            SerializedWorkspaceLocation::Remote(opts) => Some(opts),
        };
        let colour = self.colour.as_deref().and_then(|hex| {
            let parsed = crate::project_appearance::colour_from_hex(hex);
            if parsed.is_none() {
                log::warn!("project group has an unreadable colour {hex:?}; using the default");
            }
            parsed
        });
        SerializedProjectGroupState {
            key: ProjectGroupKey::new(host, path_list),
            expanded: self.expanded,
            initials: self
                .initials
                .as_deref()
                .and_then(|initials| {
                    let kept = crate::project_appearance::sanitize_initials(initials);
                    // Said out loud for the same reason the colour is: this
                    // string can be hand-edited, and silently cutting it would
                    // leave someone wondering why four letters became two.
                    if kept.as_deref() != Some(initials) {
                        log::warn!("project group initials {initials:?} were trimmed to {kept:?}");
                    }
                    kept
                })
                .map(SharedString::from),
            colour,
        }
    }
}

impl From<SerializedProjectGroup> for ProjectGroupKey {
    fn from(value: SerializedProjectGroup) -> Self {
        value.into_restored_state().key
    }
}

/// Per-window state for a MultiWorkspace, persisted to KVP.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MultiWorkspaceState {
    pub active_workspace_id: Option<WorkspaceId>,
    pub sidebar_open: bool,
    #[serde(alias = "project_group_keys")]
    pub project_groups: Vec<SerializedProjectGroup>,
    #[serde(default)]
    pub sidebar_state: Option<String>,
}

/// The serialized state of a single MultiWorkspace window from a previous session:
/// the active workspace to restore plus window-level state (project group keys,
/// sidebar).
#[derive(Debug, Clone)]
pub struct SerializedMultiWorkspace {
    pub active_workspace: SessionWorkspace,
    pub state: MultiWorkspaceState,
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct SerializedWorkspace {
    pub(crate) id: WorkspaceId,
    pub(crate) location: SerializedWorkspaceLocation,
    pub(crate) paths: PathList,
    pub(crate) center_group: SerializedPaneGroup,
    pub(crate) window_bounds: Option<SerializedWindowBounds>,
    pub(crate) centered_layout: bool,
    pub(crate) display: Option<Uuid>,
    pub(crate) docks: DockStructure,
    pub(crate) session_id: Option<String>,
    pub(crate) bookmarks: BTreeMap<Arc<Path>, Vec<SerializedBookmark>>,
    pub(crate) breakpoints: BTreeMap<Arc<Path>, Vec<SourceBreakpoint>>,
    pub(crate) user_toolchains: BTreeMap<ToolchainScope, IndexSet<Toolchain>>,
    pub(crate) window_id: Option<u64>,
}

#[derive(Debug, PartialEq, Clone, Default, Serialize, Deserialize)]
pub struct DockStructure {
    pub left: DockData,
    pub right: DockData,
    pub bottom: DockData,
}

impl RemoteConnectionKind {
    pub(crate) fn serialize(&self) -> &'static str {
        match self {
            RemoteConnectionKind::Ssh => "ssh",
            RemoteConnectionKind::Wsl => "wsl",
            RemoteConnectionKind::Docker => "docker",
        }
    }

    pub(crate) fn deserialize(text: &str) -> Option<Self> {
        match text {
            "ssh" => Some(Self::Ssh),
            "wsl" => Some(Self::Wsl),
            "docker" => Some(Self::Docker),
            _ => None,
        }
    }
}

impl Column for DockStructure {
    fn column(statement: &mut Statement, start_index: i32) -> Result<(Self, i32)> {
        let (left, next_index) = DockData::column(statement, start_index)?;
        let (right, next_index) = DockData::column(statement, next_index)?;
        let (bottom, next_index) = DockData::column(statement, next_index)?;
        Ok((
            DockStructure {
                left,
                right,
                bottom,
            },
            next_index,
        ))
    }
}

impl Bind for DockStructure {
    fn bind(&self, statement: &Statement, start_index: i32) -> Result<i32> {
        let next_index = statement.bind(&self.left, start_index)?;
        let next_index = statement.bind(&self.right, next_index)?;
        statement.bind(&self.bottom, next_index)
    }
}

#[derive(Debug, PartialEq, Clone, Default, Serialize, Deserialize)]
pub struct DockData {
    pub visible: bool,
    pub active_panel: Option<String>,
    pub zoom: bool,
}

impl Column for DockData {
    fn column(statement: &mut Statement, start_index: i32) -> Result<(Self, i32)> {
        let (visible, next_index) = Option::<bool>::column(statement, start_index)?;
        let (active_panel, next_index) = Option::<String>::column(statement, next_index)?;
        let (zoom, next_index) = Option::<bool>::column(statement, next_index)?;
        Ok((
            DockData {
                visible: visible.unwrap_or(false),
                active_panel,
                zoom: zoom.unwrap_or(false),
            },
            next_index,
        ))
    }
}

impl Bind for DockData {
    fn bind(&self, statement: &Statement, start_index: i32) -> Result<i32> {
        let next_index = statement.bind(&self.visible, start_index)?;
        let next_index = statement.bind(&self.active_panel, next_index)?;
        statement.bind(&self.zoom, next_index)
    }
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum SerializedPaneGroup {
    Group {
        axis: SerializedAxis,
        flexes: Option<Vec<f32>>,
        children: Vec<SerializedPaneGroup>,
    },
    Pane(SerializedPane),
}

#[cfg(test)]
impl Default for SerializedPaneGroup {
    fn default() -> Self {
        Self::Pane(SerializedPane {
            children: vec![SerializedItem::default()],
            active: false,
            pinned_count: 0,
        })
    }
}

impl SerializedPaneGroup {
    #[async_recursion(?Send)]
    pub(crate) async fn deserialize(
        self,
        project: &Entity<Project>,
        workspace_id: WorkspaceId,
        workspace: WeakEntity<Workspace>,
        cx: &mut AsyncWindowContext,
    ) -> Option<(
        Member,
        Option<Entity<Pane>>,
        Vec<Option<Box<dyn ItemHandle>>>,
    )> {
        match self {
            SerializedPaneGroup::Group {
                axis,
                children,
                flexes,
            } => {
                let mut current_active_pane = None;
                let mut members = Vec::new();
                let mut items = Vec::new();
                for child in children {
                    if let Some((new_member, active_pane, new_items)) = child
                        .deserialize(project, workspace_id, workspace.clone(), cx)
                        .await
                    {
                        members.push(new_member);
                        items.extend(new_items);
                        current_active_pane = current_active_pane.or(active_pane);
                    }
                }

                if members.is_empty() {
                    return None;
                }

                if members.len() == 1 {
                    return Some((members.remove(0), current_active_pane, items));
                }

                Some((
                    Member::Axis(PaneAxis::load(axis.0, members, flexes)),
                    current_active_pane,
                    items,
                ))
            }
            SerializedPaneGroup::Pane(serialized_pane) => {
                let pane = workspace
                    .update_in(cx, |workspace, window, cx| {
                        workspace.add_pane(window, cx).downgrade()
                    })
                    .log_err()?;
                let active = serialized_pane.active;
                let new_items = serialized_pane
                    .deserialize_to(project, &pane, workspace_id, workspace.clone(), cx)
                    .await
                    .context("Could not deserialize pane)")
                    .log_err()?;

                if pane
                    .read_with(cx, |pane, _| pane.items_len() != 0)
                    .log_err()?
                {
                    let pane = pane.upgrade()?;
                    Some((
                        Member::Pane(pane.clone()),
                        active.then_some(pane),
                        new_items,
                    ))
                } else {
                    let pane = pane.upgrade()?;
                    workspace
                        .update_in(cx, |workspace, window, cx| {
                            workspace.force_remove_pane(&pane, &None, window, cx)
                        })
                        .log_err()?;
                    None
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq, Default, Clone)]
pub struct SerializedPane {
    pub(crate) active: bool,
    pub(crate) children: Vec<SerializedItem>,
    pub(crate) pinned_count: usize,
}

impl SerializedPane {
    pub fn new(children: Vec<SerializedItem>, active: bool, pinned_count: usize) -> Self {
        SerializedPane {
            children,
            active,
            pinned_count,
        }
    }

    pub async fn deserialize_to(
        &self,
        project: &Entity<Project>,
        pane: &WeakEntity<Pane>,
        workspace_id: WorkspaceId,
        workspace: WeakEntity<Workspace>,
        cx: &mut AsyncWindowContext,
    ) -> Result<Vec<Option<Box<dyn ItemHandle>>>> {
        let mut item_tasks = Vec::new();
        let mut active_item_index = None;
        let mut preview_item_index = None;
        for (index, item) in self.children.iter().enumerate() {
            let project = project.clone();
            item_tasks.push(pane.update_in(cx, |_, window, cx| {
                SerializableItemRegistry::deserialize(
                    &item.kind,
                    project,
                    workspace.clone(),
                    workspace_id,
                    item.item_id,
                    window,
                    cx,
                )
            })?);
            if item.active {
                active_item_index = Some(index);
            }
            if item.preview {
                preview_item_index = Some(index);
            }
        }

        let mut items = Vec::new();
        for item_handle in futures::future::join_all(item_tasks).await {
            let item_handle = item_handle.log_err();
            items.push(item_handle.clone());

            if let Some(item_handle) = item_handle {
                pane.update_in(cx, |pane, window, cx| {
                    pane.add_item(item_handle.clone(), true, true, None, window, cx);
                })?;
            }
        }

        if let Some(active_item_index) = active_item_index {
            pane.update_in(cx, |pane, window, cx| {
                pane.activate_item(active_item_index, false, false, window, cx);
            })?;
        }

        if let Some(preview_item_index) = preview_item_index {
            pane.update(cx, |pane, cx| {
                if let Some(item) = pane.item_for_index(preview_item_index) {
                    pane.set_preview_item_id(Some(item.item_id()), cx);
                }
            })?;
        }
        pane.update(cx, |pane, _| {
            pane.set_pinned_count(self.pinned_count.min(items.len()));
        })?;

        anyhow::Ok(items)
    }
}

pub type GroupId = i64;
pub type PaneId = i64;
pub type ItemId = u64;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SerializedItem {
    pub kind: Arc<str>,
    pub item_id: ItemId,
    pub active: bool,
    pub preview: bool,
}

impl SerializedItem {
    pub fn new(kind: impl AsRef<str>, item_id: ItemId, active: bool, preview: bool) -> Self {
        Self {
            kind: Arc::from(kind.as_ref()),
            item_id,
            active,
            preview,
        }
    }
}

#[cfg(test)]
impl Default for SerializedItem {
    fn default() -> Self {
        SerializedItem {
            kind: Arc::from("Terminal"),
            item_id: 100000,
            active: false,
            preview: false,
        }
    }
}

impl StaticColumnCount for SerializedItem {
    fn column_count() -> usize {
        4
    }
}
impl Bind for &SerializedItem {
    fn bind(&self, statement: &Statement, start_index: i32) -> Result<i32> {
        let next_index = statement.bind(&self.kind, start_index)?;
        let next_index = statement.bind(&self.item_id, next_index)?;
        let next_index = statement.bind(&self.active, next_index)?;
        statement.bind(&self.preview, next_index)
    }
}

impl Column for SerializedItem {
    fn column(statement: &mut Statement, start_index: i32) -> Result<(Self, i32)> {
        let (kind, next_index) = Arc::<str>::column(statement, start_index)?;
        let (item_id, next_index) = ItemId::column(statement, next_index)?;
        let (active, next_index) = bool::column(statement, next_index)?;
        let (preview, next_index) = bool::column(statement, next_index)?;
        Ok((
            SerializedItem {
                kind,
                item_id,
                active,
                preview,
            },
            next_index,
        ))
    }
}

#[cfg(test)]
mod project_group_record_tests {
    use super::*;

    /// A record in the shape written before the avatar had its own initials and
    /// colour.
    ///
    /// Built by serializing a real record and then *removing* the two new keys,
    /// rather than by hand-writing JSON: a hand-written shape can be wrong in a
    /// way that makes the test pass for the wrong reason, and this shape is
    /// provably the one on disk.
    fn record_without_the_new_fields() -> String {
        let group = SerializedProjectGroup {
            path_list: PathList::new(&["/tmp/project"]).serialize(),
            location: SerializedWorkspaceLocation::Local,
            expanded: true,
            initials: None,
            colour: None,
        };
        let mut value = serde_json::to_value(&group).expect("a record serializes");
        let object = value.as_object_mut().expect("a record is a JSON object");
        assert!(
            object.remove("initials").is_some() && object.remove("colour").is_some(),
            "the keys being removed must actually be there, or this test proves nothing"
        );
        value.to_string()
    }

    /// This is the shape sitting in every existing installation's key-value
    /// store right now — the common path, not an edge case. It has to load, and
    /// it has to load as "no overrides" rather than as a failure.
    #[test]
    fn a_record_without_the_new_fields_still_loads() {
        let group: SerializedProjectGroup = serde_json::from_str(&record_without_the_new_fields())
            .expect("an older record must still deserialize");
        assert_eq!(group.initials, None);
        assert_eq!(group.colour, None);

        let restored = group.into_restored_state();
        assert_eq!(restored.initials, None);
        assert_eq!(restored.colour, None);
        assert!(restored.expanded);
    }

    /// A colour edited by hand into something unreadable costs the colour, not
    /// the project.
    #[test]
    fn an_unreadable_colour_is_dropped_rather_than_fatal() {
        let group = SerializedProjectGroup {
            path_list: PathList::new(&["/tmp/project"]).serialize(),
            location: SerializedWorkspaceLocation::Local,
            expanded: true,
            initials: Some("ABCDE".to_string()),
            colour: Some("not-a-colour".to_string()),
        };
        let restored = group.into_restored_state();
        assert_eq!(restored.colour, None, "the colour is refused");
        assert_eq!(
            restored.initials.as_ref().map(|initials| initials.as_ref()),
            Some("AB"),
            "and over-long initials from disk are capped like typed ones"
        );
        assert!(
            !restored.key.path_list().paths().is_empty(),
            "the project itself survives"
        );
    }

    /// The colour a person picked comes back as the colour they picked.
    #[test]
    fn initials_and_colour_survive_the_record() {
        let colour = crate::project_appearance::colour_from_hex("#3b82f6").unwrap();
        let group = SerializedProjectGroup {
            path_list: PathList::new(&["/tmp/project"]).serialize(),
            location: SerializedWorkspaceLocation::Local,
            expanded: false,
            initials: Some("ZO".to_string()),
            colour: Some(crate::project_appearance::colour_to_hex(colour)),
        };
        let json = serde_json::to_string(&group).unwrap();
        let restored = serde_json::from_str::<SerializedProjectGroup>(&json)
            .unwrap()
            .into_restored_state();
        assert_eq!(restored.initials.as_ref().map(|i| i.as_ref()), Some("ZO"));
        assert_eq!(restored.colour, Some(colour));
        assert!(!restored.expanded);
    }
}
