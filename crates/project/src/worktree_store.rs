use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize},
    },
    time::Duration,
};

use anyhow::{Context as _, Result, anyhow, bail};
use collections::HashMap;
use fs::{Fs, copy_recursive};
use futures::{FutureExt, future::Shared};
use gpui::{
    App, AppContext as _, AsyncApp, Context, Entity, EntityId, EventEmitter, Global, Task,
    WeakEntity,
};
use itertools::Either;
use postage::{prelude::Stream as _, watch};
use rpc::{
    AnyProtoClient, ErrorExt, TypedEnvelope,
    proto::{self, REMOTE_SERVER_PROJECT_ID},
};
use text::ReplicaId;
use util::{
    ResultExt,
    path_list::PathList,
    paths::{PathStyle, RemotePathBuf, SanitizedPath},
    rel_path::RelPath,
};
use worktree::{
    CreatedEntry, Entry, ProjectEntryId, UpdatedEntriesSet, UpdatedGitRepositoriesSet, Worktree,
    WorktreeId,
};

use crate::{ProjectPath, trusted_worktrees::TrustedWorktrees};

/// The current paths for a project's worktrees. Each folder path has a corresponding
/// main worktree path at the same position. The two lists are always the
/// same length and are modified together via `add_path` / `remove_main_path`.
///
/// For non-linked worktrees, the main path and folder path are identical.
/// For linked worktrees, the main path is the original repo and the folder
/// path is the linked worktree location.
#[derive(Default, Debug, Clone)]
pub struct WorktreePaths {
    paths: PathList,
    main_paths: PathList,
}

impl PartialEq for WorktreePaths {
    fn eq(&self, other: &Self) -> bool {
        self.paths == other.paths && self.main_paths == other.main_paths
    }
}

impl WorktreePaths {
    /// Build from two parallel `PathList`s that already share the same
    /// insertion order. Used for deserialization from DB.
    ///
    /// Returns an error if the two lists have different lengths, which
    /// indicates corrupted data from a prior migration bug.
    pub fn from_path_lists(
        main_worktree_paths: PathList,
        folder_paths: PathList,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            main_worktree_paths.paths().len() == folder_paths.paths().len(),
            "main_worktree_paths has {} entries but folder_paths has {}",
            main_worktree_paths.paths().len(),
            folder_paths.paths().len(),
        );
        Ok(Self {
            paths: folder_paths,
            main_paths: main_worktree_paths,
        })
    }

    /// Build for non-linked worktrees where main == folder for every path.
    pub fn from_folder_paths(folder_paths: &PathList) -> Self {
        Self {
            paths: folder_paths.clone(),
            main_paths: folder_paths.clone(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// The folder paths (for workspace matching / `threads_by_paths` index).
    pub fn folder_path_list(&self) -> &PathList {
        &self.paths
    }

    /// The main worktree paths (for group key / `threads_by_main_paths` index).
    pub fn main_worktree_path_list(&self) -> &PathList {
        &self.main_paths
    }

    /// Iterate the (main_worktree_path, folder_path) pairs in insertion order.
    pub fn ordered_pairs(&self) -> impl Iterator<Item = (&PathBuf, &PathBuf)> {
        self.main_paths
            .ordered_paths()
            .zip(self.paths.ordered_paths())
    }

    /// Add a new path pair. If the exact (main, folder) pair already exists,
    /// this is a no-op. Rebuilds both internal `PathList`s to maintain
    /// consistent ordering.
    pub fn add_path(&mut self, main_path: &Path, folder_path: &Path) {
        let already_exists = self
            .ordered_pairs()
            .any(|(m, f)| m.as_path() == main_path && f.as_path() == folder_path);
        if already_exists {
            return;
        }
        let (mut mains, mut folders): (Vec<PathBuf>, Vec<PathBuf>) = self
            .ordered_pairs()
            .map(|(m, f)| (m.clone(), f.clone()))
            .unzip();
        mains.push(main_path.to_path_buf());
        folders.push(folder_path.to_path_buf());
        self.main_paths = PathList::new(&mains);
        self.paths = PathList::new(&folders);
    }

    /// Remove all pairs whose main worktree path matches the given path.
    /// This removes the corresponding entries from both lists.
    pub fn remove_main_path(&mut self, main_path: &Path) {
        let (mains, folders): (Vec<PathBuf>, Vec<PathBuf>) = self
            .ordered_pairs()
            .filter(|(m, _)| m.as_path() != main_path)
            .map(|(m, f)| (m.clone(), f.clone()))
            .unzip();
        self.main_paths = PathList::new(&mains);
        self.paths = PathList::new(&folders);
    }

    /// Remove all pairs whose folder path matches the given path.
    /// This removes the corresponding entries from both lists.
    pub fn remove_folder_path(&mut self, folder_path: &Path) {
        let (mains, folders): (Vec<PathBuf>, Vec<PathBuf>) = self
            .ordered_pairs()
            .filter(|(_, f)| f.as_path() != folder_path)
            .map(|(m, f)| (m.clone(), f.clone()))
            .unzip();
        self.main_paths = PathList::new(&mains);
        self.paths = PathList::new(&folders);
    }
}

enum WorktreeStoreState {
    Local {
        fs: Arc<dyn Fs>,
    },
    Remote {
        upstream_client: AnyProtoClient,
        upstream_project_id: u64,
        path_style: PathStyle,
    },
}

#[derive(Clone)]
pub struct WorktreeIdCounter(Arc<AtomicU64>);

impl Default for WorktreeIdCounter {
    fn default() -> Self {
        Self(Arc::new(AtomicU64::new(1)))
    }
}

impl WorktreeIdCounter {
    pub fn get(cx: &mut App) -> Self {
        cx.default_global::<Self>().clone()
    }

    fn next(&self) -> u64 {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
}

impl Global for WorktreeIdCounter {}

pub struct WorktreeStore {
    next_entry_id: Arc<AtomicUsize>,
    next_worktree_id: WorktreeIdCounter,
    downstream_client: Option<(AnyProtoClient, u64)>,
    retain_worktrees: bool,
    worktrees: Vec<WorktreeHandle>,
    worktrees_reordered: bool,
    scanning_enabled: bool,
    #[allow(clippy::type_complexity)]
    loading_worktrees:
        HashMap<Arc<SanitizedPath>, Shared<Task<Result<Entity<Worktree>, Arc<anyhow::Error>>>>>,
    initial_scan_complete: (watch::Sender<bool>, watch::Receiver<bool>),
    state: WorktreeStoreState,
}

#[derive(Debug)]
pub enum WorktreeStoreEvent {
    WorktreeAdded(Entity<Worktree>),
    WorktreeRemoved(EntityId, WorktreeId),
    WorktreeReleased(EntityId, WorktreeId),
    WorktreeOrderChanged,
    WorktreeUpdateSent(Entity<Worktree>),
    WorktreeUpdatedEntries(WorktreeId, UpdatedEntriesSet),
    WorktreeUpdatedGitRepositories(WorktreeId, UpdatedGitRepositoriesSet),
    WorktreeDeletedEntry(WorktreeId, ProjectEntryId),
    WorktreeUpdatedRootRepoCommonDir(WorktreeId),
}

impl EventEmitter<WorktreeStoreEvent> for WorktreeStore {}

impl WorktreeStore {
    pub fn init(client: &AnyProtoClient) {
        client.add_entity_request_handler(Self::handle_create_project_entry);
        client.add_entity_request_handler(Self::handle_copy_project_entry);
        client.add_entity_request_handler(Self::handle_delete_project_entry);
        client.add_entity_request_handler(Self::handle_expand_project_entry);
        client.add_entity_request_handler(Self::handle_expand_all_for_project_entry);
    }

    pub fn init_remote(client: &AnyProtoClient) {
        client.add_entity_request_handler(Self::handle_allocate_worktree_id);
    }

    pub fn local(
        retain_worktrees: bool,
        fs: Arc<dyn Fs>,
        next_worktree_id: WorktreeIdCounter,
    ) -> Self {
        Self {
            next_entry_id: Default::default(),
            next_worktree_id,
            loading_worktrees: Default::default(),
            downstream_client: None,
            worktrees: Vec::new(),
            worktrees_reordered: false,
            scanning_enabled: true,
            retain_worktrees,
            initial_scan_complete: watch::channel_with(true),
            state: WorktreeStoreState::Local { fs },
        }
    }

    pub fn remote(
        retain_worktrees: bool,
        upstream_client: AnyProtoClient,
        upstream_project_id: u64,
        path_style: PathStyle,
        next_worktree_id: WorktreeIdCounter,
    ) -> Self {
        Self {
            next_entry_id: Default::default(),
            next_worktree_id,
            loading_worktrees: Default::default(),
            downstream_client: None,
            worktrees: Vec::new(),
            worktrees_reordered: false,
            scanning_enabled: true,
            retain_worktrees,
            initial_scan_complete: watch::channel_with(true),
            state: WorktreeStoreState::Remote {
                upstream_client,
                upstream_project_id,
                path_style,
            },
        }
    }

    pub fn next_worktree_id(&self) -> impl Future<Output = Result<WorktreeId>> + use<> {
        let strategy = match (&self.state, &self.downstream_client) {
            // we are a remote server, the client is in charge of assigning worktree ids
            (WorktreeStoreState::Local { .. }, Some((client, REMOTE_SERVER_PROJECT_ID))) => {
                Either::Left(client.clone())
            }
            // we are just a local zed project, we can assign ids
            (WorktreeStoreState::Local { .. }, _) => Either::Right(self.next_worktree_id.next()),
            // we are connected to a remote server, we are in charge of assigning worktree ids
            (WorktreeStoreState::Remote { .. }, _) => Either::Right(self.next_worktree_id.next()),
        };
        async move {
            match strategy {
                Either::Left(client) => Ok(client
                    .request(proto::AllocateWorktreeId {
                        project_id: REMOTE_SERVER_PROJECT_ID,
                    })
                    .await?
                    .worktree_id),
                Either::Right(id) => Ok(id),
            }
            .map(WorktreeId::from_proto)
        }
    }

    pub fn disable_scanner(&mut self) {
        self.scanning_enabled = false;
        *self.initial_scan_complete.0.borrow_mut() = true;
    }

    /// Returns a future that resolves when all visible worktrees have completed
    /// their initial scan (entries populated, git repos detected).
    pub fn wait_for_initial_scan(&self) -> impl Future<Output = ()> + use<> {
        let mut rx = self.initial_scan_complete.1.clone();
        async move {
            let mut done = *rx.borrow();
            while !done {
                if let Some(value) = rx.recv().await {
                    done = value;
                } else {
                    break;
                }
            }
        }
    }

    /// Returns whether all visible worktrees have completed their initial scan.
    pub fn initial_scan_completed(&self) -> bool {
        *self.initial_scan_complete.1.borrow()
    }

    /// Checks whether all visible worktrees have completed their initial scan
    /// and no worktree creations are pending, and updates the watch channel accordingly.
    fn update_initial_scan_state(&mut self, cx: &App) {
        let complete = self.loading_worktrees.is_empty()
            && self
                .visible_worktrees(cx)
                .all(|wt| wt.read(cx).completed_scan_id() >= 1);
        *self.initial_scan_complete.0.borrow_mut() = complete;
    }

    /// Spawns a detached task that waits for a worktree's initial scan to complete,
    /// then rechecks and updates the aggregate initial scan state.
    fn observe_worktree_scan_completion(
        &mut self,
        worktree: &Entity<Worktree>,
        cx: &mut Context<Self>,
    ) {
        let await_scan = worktree.update(cx, |worktree, _cx| worktree.wait_for_snapshot(1));
        cx.spawn(async move |this, cx| {
            await_scan.await.ok();
            this.update(cx, |this, cx| {
                this.update_initial_scan_state(cx);
            })
            .ok();
            anyhow::Ok(())
        })
        .detach();
    }

    /// Iterates through all worktrees, including ones that don't appear in the project panel
    pub fn worktrees(&self) -> impl '_ + DoubleEndedIterator<Item = Entity<Worktree>> {
        self.worktrees
            .iter()
            .filter_map(move |worktree| worktree.upgrade())
    }

    /// Iterates through all user-visible worktrees, the ones that appear in the project panel.
    pub fn visible_worktrees<'a>(
        &'a self,
        cx: &'a App,
    ) -> impl 'a + DoubleEndedIterator<Item = Entity<Worktree>> {
        self.worktrees()
            .filter(|worktree| worktree.read(cx).is_visible())
    }

    /// Iterates through all user-visible worktrees (directories and files that appear in the project panel) and other, invisible single files that could appear e.g. due to drag and drop.
    pub fn visible_worktrees_and_single_files<'a>(
        &'a self,
        cx: &'a App,
    ) -> impl 'a + DoubleEndedIterator<Item = Entity<Worktree>> {
        self.worktrees()
            .filter(|worktree| worktree.read(cx).is_visible() || worktree.read(cx).is_single_file())
    }

    pub fn worktree_for_id(&self, id: WorktreeId, cx: &App) -> Option<Entity<Worktree>> {
        self.worktrees()
            .find(|worktree| worktree.read(cx).id() == id)
    }

    pub fn worktree_for_entry(
        &self,
        entry_id: ProjectEntryId,
        cx: &App,
    ) -> Option<Entity<Worktree>> {
        self.worktrees()
            .find(|worktree| worktree.read(cx).contains_entry(entry_id))
    }

    pub fn find_worktree(
        &self,
        abs_path: impl AsRef<Path>,
        cx: &App,
    ) -> Option<(Entity<Worktree>, Arc<RelPath>)> {
        let abs_path = SanitizedPath::new(abs_path.as_ref());
        for tree in self.worktrees() {
            let path_style = tree.read(cx).path_style();
            if let Some(relative_path) =
                path_style.strip_prefix(abs_path.as_ref(), tree.read(cx).abs_path().as_ref())
            {
                return Some((tree.clone(), relative_path.into_arc()));
            }
        }
        None
    }

    pub fn project_path_for_absolute_path(&self, abs_path: &Path, cx: &App) -> Option<ProjectPath> {
        self.find_worktree(abs_path, cx)
            .map(|(worktree, relative_path)| ProjectPath {
                worktree_id: worktree.read(cx).id(),
                path: relative_path,
            })
    }

    pub fn absolutize(&self, project_path: &ProjectPath, cx: &App) -> Option<PathBuf> {
        let worktree = self.worktree_for_id(project_path.worktree_id, cx)?;
        Some(worktree.read(cx).absolutize(&project_path.path))
    }

    pub fn path_style(&self) -> PathStyle {
        match &self.state {
            WorktreeStoreState::Local { .. } => PathStyle::local(),
            WorktreeStoreState::Remote { path_style, .. } => *path_style,
        }
    }

    pub fn find_or_create_worktree(
        &mut self,
        abs_path: impl AsRef<Path>,
        visible: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<(Entity<Worktree>, Arc<RelPath>)>> {
        let abs_path = abs_path.as_ref();
        if let Some((tree, relative_path)) = self.find_worktree(abs_path, cx) {
            Task::ready(Ok((tree, relative_path)))
        } else {
            let worktree = self.create_worktree(abs_path, visible, cx);
            cx.background_spawn(async move { Ok((worktree.await?, RelPath::empty().into())) })
        }
    }

    pub fn entry_for_id<'a>(&'a self, entry_id: ProjectEntryId, cx: &'a App) -> Option<&'a Entry> {
        self.worktrees()
            .find_map(|worktree| worktree.read(cx).entry_for_id(entry_id))
    }

    pub fn worktree_and_entry_for_id<'a>(
        &'a self,
        entry_id: ProjectEntryId,
        cx: &'a App,
    ) -> Option<(Entity<Worktree>, &'a Entry)> {
        self.worktrees().find_map(|worktree| {
            worktree
                .read(cx)
                .entry_for_id(entry_id)
                .map(|e| (worktree.clone(), e))
        })
    }

    pub fn entry_for_path<'a>(&'a self, path: &ProjectPath, cx: &'a App) -> Option<&'a Entry> {
        self.worktree_for_id(path.worktree_id, cx)?
            .read(cx)
            .entry_for_path(&path.path)
    }

    pub fn copy_entry(
        &mut self,
        entry_id: ProjectEntryId,
        new_project_path: ProjectPath,
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<Entry>>> {
        let Some(old_worktree) = self.worktree_for_entry(entry_id, cx) else {
            return Task::ready(Err(anyhow!("no such worktree")));
        };
        let Some(old_entry) = old_worktree.read(cx).entry_for_id(entry_id) else {
            return Task::ready(Err(anyhow!("no such entry")));
        };
        let Some(new_worktree) = self.worktree_for_id(new_project_path.worktree_id, cx) else {
            return Task::ready(Err(anyhow!("no such worktree")));
        };

        match &self.state {
            WorktreeStoreState::Local { fs } => {
                let old_abs_path = old_worktree.read(cx).absolutize(&old_entry.path);
                let new_abs_path = new_worktree.read(cx).absolutize(&new_project_path.path);
                let fs = fs.clone();
                let copy = cx.background_spawn(async move {
                    copy_recursive(
                        fs.as_ref(),
                        &old_abs_path,
                        &new_abs_path,
                        Default::default(),
                    )
                    .await
                });

                cx.spawn(async move |_, cx| {
                    copy.await?;
                    new_worktree
                        .update(cx, |this, cx| {
                            this.as_local_mut().unwrap().refresh_entry(
                                new_project_path.path,
                                None,
                                cx,
                            )
                        })
                        .await
                })
            }
            WorktreeStoreState::Remote {
                upstream_client,
                upstream_project_id,
                ..
            } => {
                let response = upstream_client.request(proto::CopyProjectEntry {
                    project_id: *upstream_project_id,
                    entry_id: entry_id.to_proto(),
                    new_path: new_project_path.path.to_proto(),
                    new_worktree_id: new_project_path.worktree_id.to_proto(),
                });
                cx.spawn(async move |_, cx| {
                    let response = response.await?;
                    match response.entry {
                        Some(entry) => new_worktree
                            .update(cx, |worktree, cx| {
                                worktree.as_remote_mut().unwrap().insert_entry(
                                    entry,
                                    response.worktree_scan_id as usize,
                                    cx,
                                )
                            })
                            .await
                            .map(Some),
                        None => Ok(None),
                    }
                })
            }
        }
    }

    pub fn rename_entry(
        &mut self,
        entry_id: ProjectEntryId,
        new_project_path: ProjectPath,
        cx: &mut Context<Self>,
    ) -> Task<Result<CreatedEntry>> {
        let Some(old_worktree) = self.worktree_for_entry(entry_id, cx) else {
            return Task::ready(Err(anyhow!("no such worktree")));
        };
        let Some(old_entry) = old_worktree.read(cx).entry_for_id(entry_id).cloned() else {
            return Task::ready(Err(anyhow!("no such entry")));
        };
        let Some(new_worktree) = self.worktree_for_id(new_project_path.worktree_id, cx) else {
            return Task::ready(Err(anyhow!("no such worktree")));
        };

        match &self.state {
            WorktreeStoreState::Local { fs } => {
                let abs_old_path = old_worktree.read(cx).absolutize(&old_entry.path);
                let new_worktree_ref = new_worktree.read(cx);
                let is_root_entry = new_worktree_ref
                    .root_entry()
                    .is_some_and(|e| e.id == entry_id);
                let abs_new_path = if is_root_entry {
                    let abs_path = new_worktree_ref.abs_path();
                    let Some(root_parent_path) = abs_path.parent() else {
                        return Task::ready(Err(anyhow!("no parent for path {:?}", abs_path)));
                    };
                    root_parent_path.join(new_project_path.path.as_std_path())
                } else {
                    new_worktree_ref.absolutize(&new_project_path.path)
                };

                let fs = fs.clone();
                let case_sensitive = new_worktree
                    .read(cx)
                    .as_local()
                    .unwrap()
                    .fs_is_case_sensitive();

                let do_rename =
                    async move |fs: &dyn Fs, old_path: &Path, new_path: &Path, overwrite| {
                        fs.rename(
                            &old_path,
                            &new_path,
                            fs::RenameOptions {
                                overwrite,
                                ..fs::RenameOptions::default()
                            },
                        )
                        .await
                        .with_context(|| format!("renaming {old_path:?} into {new_path:?}"))
                    };

                let rename = cx.background_spawn({
                    let abs_new_path = abs_new_path.clone();
                    async move {
                        // If we're on a case-insensitive FS and we're doing a case-only rename (i.e. `foobar` to `FOOBAR`)
                        // we want to overwrite, because otherwise we run into a file-already-exists error.
                        let overwrite = !case_sensitive
                            && abs_old_path != abs_new_path
                            && abs_old_path.to_str().map(|p| p.to_lowercase())
                                == abs_new_path.to_str().map(|p| p.to_lowercase());

                        // The directory we're renaming into might not exist yet
                        if let Err(e) =
                            do_rename(fs.as_ref(), &abs_old_path, &abs_new_path, overwrite).await
                        {
                            if let Some(err) = e.downcast_ref::<std::io::Error>()
                                && err.kind() == std::io::ErrorKind::NotFound
                            {
                                if let Some(parent) = abs_new_path.parent() {
                                    fs.create_dir(parent).await.with_context(|| {
                                        format!("creating parent directory {parent:?}")
                                    })?;
                                    return do_rename(
                                        fs.as_ref(),
                                        &abs_old_path,
                                        &abs_new_path,
                                        overwrite,
                                    )
                                    .await;
                                }
                            }
                            return Err(e);
                        }
                        Ok(())
                    }
                });

                cx.spawn(async move |_, cx| {
                    rename.await?;
                    Ok(new_worktree
                        .update(cx, |this, cx| {
                            let local = this.as_local_mut().unwrap();
                            if is_root_entry {
                                // We eagerly update `abs_path` and refresh this worktree.
                                // Otherwise, the FS watcher would do it on the `RootUpdated` event,
                                // but with a noticeable delay, so we handle it proactively.
                                local.update_abs_path_and_refresh(
                                    SanitizedPath::new_arc(&abs_new_path),
                                    cx,
                                );
                                Task::ready(Ok(this.root_entry().cloned()))
                            } else {
                                // First refresh the parent directory (in case it was newly created)
                                if let Some(parent) = new_project_path.path.parent() {
                                    let _ = local.refresh_entries_for_paths(vec![parent.into()]);
                                }
                                // Then refresh the new path
                                local.refresh_entry(
                                    new_project_path.path.clone(),
                                    Some(old_entry.path),
                                    cx,
                                )
                            }
                        })
                        .await?
                        .map(CreatedEntry::Included)
                        .unwrap_or_else(|| CreatedEntry::Excluded {
                            abs_path: abs_new_path,
                        }))
                })
            }
            WorktreeStoreState::Remote {
                upstream_client,
                upstream_project_id,
                ..
            } => {
                let response = upstream_client.request(proto::RenameProjectEntry {
                    project_id: *upstream_project_id,
                    entry_id: entry_id.to_proto(),
                    new_path: new_project_path.path.to_proto(),
                    new_worktree_id: new_project_path.worktree_id.to_proto(),
                });
                cx.spawn(async move |_, cx| {
                    let response = response.await?;
                    match response.entry {
                        Some(entry) => new_worktree
                            .update(cx, |worktree, cx| {
                                worktree.as_remote_mut().unwrap().insert_entry(
                                    entry,
                                    response.worktree_scan_id as usize,
                                    cx,
                                )
                            })
                            .await
                            .map(CreatedEntry::Included),
                        None => {
                            let abs_path = new_worktree.read_with(cx, |worktree, _| {
                                worktree.absolutize(&new_project_path.path)
                            });
                            Ok(CreatedEntry::Excluded { abs_path })
                        }
                    }
                })
            }
        }
    }
    pub fn create_worktree(
        &mut self,
        abs_path: impl AsRef<Path>,
        visible: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Worktree>>> {
        let abs_path: Arc<SanitizedPath> = SanitizedPath::new_arc(&abs_path);
        let is_via_collab = matches!(&self.state, WorktreeStoreState::Remote { upstream_client, .. } if upstream_client.is_via_collab());
        if !self.loading_worktrees.contains_key(&abs_path) {
            let task = match &self.state {
                WorktreeStoreState::Remote {
                    upstream_client,
                    path_style,
                    ..
                } => {
                    if upstream_client.is_via_collab() {
                        Task::ready(Err(Arc::new(anyhow!("cannot create worktrees via collab"))))
                    } else {
                        let abs_path = RemotePathBuf::new(abs_path.to_string(), *path_style);
                        self.create_remote_worktree(upstream_client.clone(), abs_path, visible, cx)
                    }
                }
                WorktreeStoreState::Local { fs } => {
                    self.create_local_worktree(fs.clone(), abs_path.clone(), visible, cx)
                }
            };

            self.loading_worktrees
                .insert(abs_path.clone(), task.shared());

            if visible && self.scanning_enabled {
                *self.initial_scan_complete.0.borrow_mut() = false;
            }
        }
        let task = self.loading_worktrees.get(&abs_path).unwrap().clone();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.loading_worktrees.remove(&abs_path);
                if !visible || !this.scanning_enabled || result.is_err() {
                    this.update_initial_scan_state(cx);
                }
            })
            .ok();

            match result {
                Ok(worktree) => {
                    if !is_via_collab {
                        if let Some((trusted_worktrees, worktree_store)) = this
                            .update(cx, |_, cx| {
                                TrustedWorktrees::try_get_global(cx).zip(Some(cx.entity()))
                            })
                            .ok()
                            .flatten()
                        {
                            trusted_worktrees.update(cx, |trusted_worktrees, cx| {
                                trusted_worktrees.can_trust(
                                    &worktree_store,
                                    worktree.read(cx).id(),
                                    cx,
                                );
                            });
                        }

                        this.update(cx, |this, cx| {
                            if this.scanning_enabled && visible {
                                this.observe_worktree_scan_completion(&worktree, cx);
                            }
                        })
                        .ok();
                    }
                    Ok(worktree)
                }
                Err(err) => Err((*err).cloned()),
            }
        })
    }

    fn create_remote_worktree(
        &mut self,
        client: AnyProtoClient,
        abs_path: RemotePathBuf,
        visible: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Worktree>, Arc<anyhow::Error>>> {
        let path_style = abs_path.path_style();
        let mut abs_path = abs_path.to_string();
        // If we start with `/~` that means the ssh path was something like `ssh://user@host/~/home-dir-folder/`
        // in which case want to strip the leading the `/`.
        // On the host-side, the `~` will get expanded.
        // That's what git does too: https://github.com/libgit2/libgit2/issues/3345#issuecomment-127050850
        if abs_path.starts_with("/~") {
            abs_path = abs_path[1..].to_string();
        }
        if abs_path.is_empty() {
            abs_path = "~/".to_string();
        }

        cx.spawn(async move |this, cx| {
            let this = this.upgrade().context("Dropped worktree store")?;

            let path = RemotePathBuf::new(abs_path, path_style);
            let response = client
                .request(proto::AddWorktree {
                    project_id: REMOTE_SERVER_PROJECT_ID,
                    path: path.to_proto(),
                    visible,
                })
                .await?;

            if let Some(existing_worktree) = this.read_with(cx, |this, cx| {
                this.worktree_for_id(WorktreeId::from_proto(response.worktree_id), cx)
            }) {
                return Ok(existing_worktree);
            }

            let root_path_buf = PathBuf::from(response.canonicalized_path.clone());
            let root_name = root_path_buf
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(root_path_buf.to_string_lossy().into_owned());

            let worktree = cx.update(|cx| {
                Worktree::remote(
                    REMOTE_SERVER_PROJECT_ID,
                    ReplicaId::REMOTE_SERVER,
                    proto::WorktreeMetadata {
                        id: response.worktree_id,
                        root_name,
                        visible,
                        abs_path: response.canonicalized_path,
                        root_repo_common_dir: response.root_repo_common_dir,
                    },
                    client,
                    path_style,
                    cx,
                )
            });

            this.update(cx, |this, cx| {
                this.add(&worktree, cx);
            });
            Ok(worktree)
        })
    }

    fn create_local_worktree(
        &mut self,
        fs: Arc<dyn Fs>,
        abs_path: Arc<SanitizedPath>,
        visible: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Worktree>, Arc<anyhow::Error>>> {
        let next_entry_id = self.next_entry_id.clone();
        let scanning_enabled = self.scanning_enabled;

        let next_worktree_id = self.next_worktree_id();

        cx.spawn(async move |this, cx| {
            let worktree_id = next_worktree_id.await?;
            let worktree = Worktree::local(
                SanitizedPath::cast_arc(abs_path.clone()),
                visible,
                fs,
                next_entry_id,
                scanning_enabled,
                worktree_id,
                cx,
            )
            .await?;

            this.update(cx, |this, cx| this.add(&worktree, cx))?;

            if visible {
                cx.update(|cx| {
                    cx.add_recent_document(abs_path.as_path());
                });
            }

            Ok(worktree)
        })
    }

    pub fn add(&mut self, worktree: &Entity<Worktree>, cx: &mut Context<Self>) {
        let worktree_id = worktree.read(cx).id();
        debug_assert!(self.worktrees().all(|w| w.read(cx).id() != worktree_id));

        let push_strong_handle = self.retain_worktrees || worktree.read(cx).is_visible();
        let handle = if push_strong_handle {
            WorktreeHandle::Strong(worktree.clone())
        } else {
            WorktreeHandle::Weak(worktree.downgrade())
        };
        if self.worktrees_reordered {
            self.worktrees.push(handle);
        } else {
            let i = match self
                .worktrees
                .binary_search_by_key(&Some(worktree.read(cx).abs_path()), |other| {
                    other.upgrade().map(|worktree| worktree.read(cx).abs_path())
                }) {
                Ok(i) | Err(i) => i,
            };
            self.worktrees.insert(i, handle);
        }

        cx.emit(WorktreeStoreEvent::WorktreeAdded(worktree.clone()));
        self.send_project_updates(cx);

        let handle_id = worktree.entity_id();
        cx.subscribe(worktree, |_, worktree, event, cx| {
            let worktree_id = worktree.read(cx).id();
            match event {
                worktree::Event::UpdatedEntries(changes) => {
                    cx.emit(WorktreeStoreEvent::WorktreeUpdatedEntries(
                        worktree_id,
                        changes.clone(),
                    ));
                }
                worktree::Event::UpdatedGitRepositories(set) => {
                    cx.emit(WorktreeStoreEvent::WorktreeUpdatedGitRepositories(
                        worktree_id,
                        set.clone(),
                    ));
                }
                worktree::Event::DeletedEntry(id) => {
                    cx.emit(WorktreeStoreEvent::WorktreeDeletedEntry(worktree_id, *id))
                }
                worktree::Event::Deleted => {
                    // The worktree root itself has been deleted (for single-file worktrees)
                    // The worktree will be removed via the observe_release callback
                }
                worktree::Event::UpdatedRootRepoCommonDir { .. } => {
                    cx.emit(WorktreeStoreEvent::WorktreeUpdatedRootRepoCommonDir(
                        worktree_id,
                    ));
                }
            }
        })
        .detach();
        cx.observe_release(worktree, move |this, worktree, cx| {
            cx.emit(WorktreeStoreEvent::WorktreeReleased(
                handle_id,
                worktree.id(),
            ));
            cx.emit(WorktreeStoreEvent::WorktreeRemoved(
                handle_id,
                worktree.id(),
            ));
            this.send_project_updates(cx);
        })
        .detach();
    }

    pub fn remove_worktree(&mut self, id_to_remove: WorktreeId, cx: &mut Context<Self>) {
        self.worktrees.retain(|worktree| {
            if let Some(worktree) = worktree.upgrade() {
                if worktree.read(cx).id() == id_to_remove {
                    cx.emit(WorktreeStoreEvent::WorktreeRemoved(
                        worktree.entity_id(),
                        id_to_remove,
                    ));
                    false
                } else {
                    true
                }
            } else {
                false
            }
        });
        self.update_initial_scan_state(cx);
        self.send_project_updates(cx);
    }

    pub fn worktree_for_main_worktree_path(
        &self,
        path: &Path,
        cx: &App,
    ) -> Option<Entity<Worktree>> {
        self.visible_worktrees(cx).find(|worktree| {
            let worktree = worktree.read(cx);
            if let Some(common_dir) = worktree.root_repo_common_dir() {
                common_dir.parent() == Some(path)
            } else {
                worktree.abs_path().as_ref() == path
            }
        })
    }

    pub fn set_worktrees_reordered(&mut self, worktrees_reordered: bool) {
        self.worktrees_reordered = worktrees_reordered;
    }

    fn upstream_client(&self) -> Option<(AnyProtoClient, u64)> {
        match &self.state {
            WorktreeStoreState::Remote {
                upstream_client,
                upstream_project_id,
                ..
            } => Some((upstream_client.clone(), *upstream_project_id)),
            WorktreeStoreState::Local { .. } => None,
        }
    }

    pub fn set_worktrees_from_proto(
        &mut self,
        worktrees: Vec<proto::WorktreeMetadata>,
        replica_id: ReplicaId,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let mut old_worktrees_by_id = self
            .worktrees
            .drain(..)
            .filter_map(|worktree| {
                let worktree = worktree.upgrade()?;
                Some((worktree.read(cx).id(), worktree))
            })
            .collect::<HashMap<_, _>>();

        let (client, project_id) = self.upstream_client().context("invalid project")?;

        for worktree in worktrees {
            if let Some(old_worktree) =
                old_worktrees_by_id.remove(&WorktreeId::from_proto(worktree.id))
            {
                let push_strong_handle =
                    self.retain_worktrees || old_worktree.read(cx).is_visible();
                let handle = if push_strong_handle {
                    WorktreeHandle::Strong(old_worktree.clone())
                } else {
                    WorktreeHandle::Weak(old_worktree.downgrade())
                };
                self.worktrees.push(handle);
            } else {
                self.add(
                    &Worktree::remote(
                        project_id,
                        replica_id,
                        worktree,
                        client.clone(),
                        self.path_style(),
                        cx,
                    ),
                    cx,
                );
            }
        }
        self.send_project_updates(cx);

        Ok(())
    }

    pub fn move_worktree(
        &mut self,
        source: WorktreeId,
        destination: WorktreeId,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        if source == destination {
            return Ok(());
        }

        let mut source_index = None;
        let mut destination_index = None;
        for (i, worktree) in self.worktrees.iter().enumerate() {
            if let Some(worktree) = worktree.upgrade() {
                let worktree_id = worktree.read(cx).id();
                if worktree_id == source {
                    source_index = Some(i);
                    if destination_index.is_some() {
                        break;
                    }
                } else if worktree_id == destination {
                    destination_index = Some(i);
                    if source_index.is_some() {
                        break;
                    }
                }
            }
        }

        let source_index =
            source_index.with_context(|| format!("Missing worktree for id {source}"))?;
        let destination_index =
            destination_index.with_context(|| format!("Missing worktree for id {destination}"))?;

        if source_index == destination_index {
            return Ok(());
        }

        let worktree_to_move = self.worktrees.remove(source_index);
        self.worktrees.insert(destination_index, worktree_to_move);
        self.worktrees_reordered = true;
        cx.emit(WorktreeStoreEvent::WorktreeOrderChanged);
        cx.notify();
        Ok(())
    }

    pub fn disconnected_from_host(&mut self, cx: &mut App) {
        for worktree in &self.worktrees {
            if let Some(worktree) = worktree.upgrade() {
                worktree.update(cx, |worktree, _| {
                    if let Some(worktree) = worktree.as_remote_mut() {
                        worktree.disconnected_from_host();
                    }
                });
            }
        }
    }

    pub fn send_project_updates(&mut self, cx: &mut Context<Self>) {
        let Some((downstream_client, project_id)) = self.downstream_client.clone() else {
            return;
        };

        let update = proto::UpdateProject {
            project_id,
            worktrees: self.worktree_metadata_protos(cx),
        };

        // collab has bad concurrency guarantees, so we send requests in serial.
        let update_project = if downstream_client.is_via_collab() {
            Some(downstream_client.request(update))
        } else {
            downstream_client.send(update).log_err();
            None
        };
        cx.spawn(async move |this, cx| {
            if let Some(update_project) = update_project {
                update_project.await?;
            }

            this.update(cx, |this, cx| {
                let worktrees = this.worktrees().collect::<Vec<_>>();

                for worktree in worktrees {
                    worktree.update(cx, |worktree, cx| {
                        let client = downstream_client.clone();
                        worktree.observe_updates(project_id, cx, {
                            move |update| {
                                let client = client.clone();
                                async move {
                                    if client.is_via_collab() {
                                        client
                                            .request(update)
                                            .map(|result| result.log_err().is_some())
                                            .await
                                    } else {
                                        client.send(update).log_err().is_some()
                                    }
                                }
                            }
                        });
                    });

                    cx.emit(WorktreeStoreEvent::WorktreeUpdateSent(worktree.clone()))
                }

                anyhow::Ok(())
            })
        })
        .detach_and_log_err(cx);
    }

    pub fn worktree_metadata_protos(&self, cx: &App) -> Vec<proto::WorktreeMetadata> {
        self.worktrees()
            .map(|worktree| {
                let worktree = worktree.read(cx);
                proto::WorktreeMetadata {
                    id: worktree.id().to_proto(),
                    root_name: worktree.root_name_str().to_owned(),
                    visible: worktree.is_visible(),
                    abs_path: worktree.abs_path().to_string_lossy().into_owned(),
                    root_repo_common_dir: worktree
                        .root_repo_common_dir()
                        .map(|p| p.to_string_lossy().into_owned()),
                }
            })
            .collect()
    }

    pub fn shared(
        &mut self,
        remote_id: u64,
        downstream_client: AnyProtoClient,
        cx: &mut Context<Self>,
    ) {
        self.retain_worktrees = true;
        self.downstream_client = Some((downstream_client, remote_id));

        // When shared, retain all worktrees
        for worktree_handle in self.worktrees.iter_mut() {
            match worktree_handle {
                WorktreeHandle::Strong(_) => {}
                WorktreeHandle::Weak(worktree) => {
                    if let Some(worktree) = worktree.upgrade() {
                        *worktree_handle = WorktreeHandle::Strong(worktree);
                    }
                }
            }
        }
        // Only send project updates if we share in a collaborative mode.
        // Otherwise we are the remote server which is currently constructing
        // worktree store before the client actually has set up its message
        // handlers.
        if remote_id != REMOTE_SERVER_PROJECT_ID {
            self.send_project_updates(cx);
        }
    }

    pub fn unshared(&mut self, cx: &mut Context<Self>) {
        self.retain_worktrees = false;
        self.downstream_client.take();

        // When not shared, only retain the visible worktrees
        for worktree_handle in self.worktrees.iter_mut() {
            if let WorktreeHandle::Strong(worktree) = worktree_handle {
                let is_visible = worktree.update(cx, |worktree, _| {
                    worktree.stop_observing_updates();
                    worktree.is_visible()
                });
                if !is_visible {
                    *worktree_handle = WorktreeHandle::Weak(worktree.downgrade());
                }
            }
        }
    }

    pub async fn handle_create_project_entry(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::CreateProjectEntry>,
        mut cx: AsyncApp,
    ) -> Result<proto::ProjectEntryResponse> {
        let worktree = this.update(&mut cx, |this, cx| {
            let worktree_id = WorktreeId::from_proto(envelope.payload.worktree_id);
            this.worktree_for_id(worktree_id, cx)
                .context("worktree not found")
        })?;
        Worktree::handle_create_entry(worktree, envelope.payload, cx).await
    }

    pub async fn handle_copy_project_entry(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::CopyProjectEntry>,
        mut cx: AsyncApp,
    ) -> Result<proto::ProjectEntryResponse> {
        let entry_id = ProjectEntryId::from_proto(envelope.payload.entry_id);
        let new_worktree_id = WorktreeId::from_proto(envelope.payload.new_worktree_id);
        let new_project_path = (
            new_worktree_id,
            RelPath::from_proto(&envelope.payload.new_path)?,
        );
        let (scan_id, entry) = this.update(&mut cx, |this, cx| {
            let Some((_, project_id)) = this.downstream_client else {
                bail!("no downstream client")
            };
            let Some(entry) = this.entry_for_id(entry_id, cx) else {
                bail!("no such entry");
            };
            if entry.is_private && project_id != REMOTE_SERVER_PROJECT_ID {
                bail!("entry is private")
            }

            let new_worktree = this
                .worktree_for_id(new_worktree_id, cx)
                .context("no such worktree")?;
            let scan_id = new_worktree.read(cx).scan_id();
            anyhow::Ok((
                scan_id,
                this.copy_entry(entry_id, new_project_path.into(), cx),
            ))
        })?;
        let entry = entry.await?;
        Ok(proto::ProjectEntryResponse {
            entry: entry.as_ref().map(|entry| entry.into()),
            worktree_scan_id: scan_id as u64,
        })
    }

    pub async fn handle_delete_project_entry(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::DeleteProjectEntry>,
        mut cx: AsyncApp,
    ) -> Result<proto::ProjectEntryResponse> {
        let entry_id = ProjectEntryId::from_proto(envelope.payload.entry_id);
        let worktree = this.update(&mut cx, |this, cx| {
            let Some((_, project_id)) = this.downstream_client else {
                bail!("no downstream client")
            };
            let Some(entry) = this.entry_for_id(entry_id, cx) else {
                bail!("no entry")
            };
            if entry.is_private && project_id != REMOTE_SERVER_PROJECT_ID {
                bail!("entry is private")
            }
            this.worktree_for_entry(entry_id, cx)
                .context("worktree not found")
        })?;
        Worktree::handle_delete_entry(worktree, envelope.payload, cx).await
    }

    pub async fn handle_rename_project_entry(
        this: Entity<Self>,
        request: proto::RenameProjectEntry,
        mut cx: AsyncApp,
    ) -> Result<proto::ProjectEntryResponse> {
        let entry_id = ProjectEntryId::from_proto(request.entry_id);
        let new_worktree_id = WorktreeId::from_proto(request.new_worktree_id);
        let rel_path = RelPath::from_proto(&request.new_path)
            .with_context(|| format!("received invalid relative path {:?}", &request.new_path))?;

        let (scan_id, task) = this.update(&mut cx, |this, cx| {
            let worktree = this
                .worktree_for_entry(entry_id, cx)
                .context("no such worktree")?;

            let Some((_, project_id)) = this.downstream_client else {
                bail!("no downstream client")
            };
            let entry = worktree
                .read(cx)
                .entry_for_id(entry_id)
                .ok_or_else(|| anyhow!("missing entry"))?;
            if entry.is_private && project_id != REMOTE_SERVER_PROJECT_ID {
                bail!("entry is private")
            }

            let scan_id = worktree.read(cx).scan_id();
            anyhow::Ok((
                scan_id,
                this.rename_entry(entry_id, (new_worktree_id, rel_path).into(), cx),
            ))
        })?;
        Ok(proto::ProjectEntryResponse {
            entry: match &task.await? {
                CreatedEntry::Included(entry) => Some(entry.into()),
                CreatedEntry::Excluded { .. } => None,
            },
            worktree_scan_id: scan_id as u64,
        })
    }

    pub async fn handle_expand_project_entry(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::ExpandProjectEntry>,
        mut cx: AsyncApp,
    ) -> Result<proto::ExpandProjectEntryResponse> {
        let entry_id = ProjectEntryId::from_proto(envelope.payload.entry_id);
        let worktree = this
            .update(&mut cx, |this, cx| this.worktree_for_entry(entry_id, cx))
            .context("invalid request")?;
        Worktree::handle_expand_entry(worktree, envelope.payload, cx).await
    }

    pub async fn handle_expand_all_for_project_entry(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::ExpandAllForProjectEntry>,
        mut cx: AsyncApp,
    ) -> Result<proto::ExpandAllForProjectEntryResponse> {
        let entry_id = ProjectEntryId::from_proto(envelope.payload.entry_id);
        let worktree = this
            .update(&mut cx, |this, cx| this.worktree_for_entry(entry_id, cx))
            .context("invalid request")?;
        Worktree::handle_expand_all_for_entry(worktree, envelope.payload, cx).await
    }

    pub async fn handle_allocate_worktree_id(
        _this: Entity<Self>,
        _envelope: TypedEnvelope<proto::AllocateWorktreeId>,
        cx: AsyncApp,
    ) -> Result<proto::AllocateWorktreeIdResponse> {
        let worktree_id = cx.update(|cx| WorktreeIdCounter::get(cx).next());
        Ok(proto::AllocateWorktreeIdResponse { worktree_id })
    }

    pub fn fs(&self) -> Option<Arc<dyn Fs>> {
        match &self.state {
            WorktreeStoreState::Local { fs } => Some(fs.clone()),
            WorktreeStoreState::Remote { .. } => None,
        }
    }

    pub fn paths(&self, cx: &App) -> WorktreePaths {
        let (mains, folders): (Vec<PathBuf>, Vec<PathBuf>) = self
            .visible_worktrees(cx)
            .map(|worktree| {
                let snapshot = worktree.read(cx).snapshot();
                let folder_path = snapshot.abs_path().to_path_buf();
                let main_path = snapshot.main_worktree_abs_path().to_path_buf();
                (main_path, folder_path)
            })
            .unzip();

        WorktreePaths {
            paths: PathList::new(&folders),
            main_paths: PathList::new(&mains),
        }
    }
}

#[derive(Clone, Debug)]
enum WorktreeHandle {
    Strong(Entity<Worktree>),
    Weak(WeakEntity<Worktree>),
}

impl WorktreeHandle {
    fn upgrade(&self) -> Option<Entity<Worktree>> {
        match self {
            WorktreeHandle::Strong(handle) => Some(handle.clone()),
            WorktreeHandle::Weak(handle) => handle.upgrade(),
        }
    }
}

/// What a remote project's file request says when its server predates it.
pub const PRIVATE_FILE_SERVER_OUT_OF_DATE: &str = "Update the remote server to sync this file";

/// The largest private file read or written over the connection. An `.env`
/// is a few kilobytes; anything near this is not one, and refusing it keeps a
/// misconfigured `private_files` glob from streaming a database dump.
pub const PRIVATE_FILE_MAX_BYTES: usize = 1024 * 1024;

const PRIVATE_FILE_TIMEOUT: Duration = Duration::from_secs(30);

/// Said after "the remote server did not answer within 30s;". A write that
/// timed out may still land; trying again is safe, because the host refuses
/// to replace a file that is no longer what was read.
const PRIVATE_FILE_UNANSWERED: &str = "try again";

impl WorktreeStore {
    /// Lets the machine that opened this project read and write its private
    /// files. Registered only by the remote server: a collaboration host
    /// serving these would hand its `.env` to every guest.
    pub fn init_private_files(client: &AnyProtoClient) {
        client.add_entity_request_handler(Self::handle_load_private_file);
        client.add_entity_request_handler(Self::handle_write_private_file);
    }

    /// A private file of a remote project, read and written on its host.
    ///
    /// `None` for a local project, whose files are read directly, and for a
    /// collaboration guest, whose host must never be asked for one.
    pub fn remote_private_file(
        &self,
        worktree_id: WorktreeId,
        path: Arc<RelPath>,
        cx: &App,
    ) -> Option<RemotePrivateFile> {
        let WorktreeStoreState::Remote {
            upstream_client,
            upstream_project_id,
            ..
        } = &self.state
        else {
            return None;
        };
        if upstream_client.is_via_collab() || *upstream_project_id != REMOTE_SERVER_PROJECT_ID {
            return None;
        }
        Some(RemotePrivateFile {
            client: upstream_client.clone(),
            executor: cx.background_executor().clone(),
            worktree_id,
            path,
            loaded_sha256: Arc::default(),
        })
    }

    /// Whether this machine's settings call `path` private.
    ///
    /// For a remote project the host asks the same question before it hands
    /// anything over, but with settings whose user layer this machine synced
    /// to it, so the two agree by construction and neither is a boundary.
    /// What limits a request is who may make it and that the path stays
    /// inside the worktree.
    pub fn is_path_private_here(&self, path: &ProjectPath, cx: &App) -> bool {
        use settings::Settings as _;
        worktree::WorktreeSettings::get(Some(path.into()), cx).is_path_private(&path.path)
    }

    pub async fn handle_load_private_file(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::LoadPrivateFile>,
        mut cx: AsyncApp,
    ) -> Result<proto::LoadPrivateFileResponse> {
        let payload = envelope.payload;
        let location = this.update(&mut cx, |this, cx| {
            this.private_file_location(payload.project_id, payload.worktree_id, &payload.path, cx)
        })?;
        let content = cx.background_spawn(async move { location.read() }).await?;
        Ok(proto::LoadPrivateFileResponse { content })
    }

    pub async fn handle_write_private_file(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::WritePrivateFile>,
        mut cx: AsyncApp,
    ) -> Result<proto::Ack> {
        let payload = envelope.payload;
        let location = this.update(&mut cx, |this, cx| {
            this.private_file_location(payload.project_id, payload.worktree_id, &payload.path, cx)
        })?;
        cx.background_spawn(async move {
            location.write(&payload.content, payload.replaces_sha256.as_deref())
        })
        .await?;
        Ok(proto::Ack {})
    }

    /// Every check that needs the worktree.
    ///
    /// The private-files test uses the server's settings, whose user layer is
    /// the client's synced copy: it keeps the server from serving arbitrary
    /// files by accident, not a client that edits its own settings. The
    /// boundary is that only the machine that opened the project is served,
    /// that the path stays inside the worktree, and that the file is written
    /// owner-only.
    fn private_file_location(
        &self,
        project_id: u64,
        worktree_id: u64,
        path: &str,
        cx: &App,
    ) -> Result<PrivateFileLocation> {
        let serves_its_owner = matches!(
            self.downstream_client,
            Some((_, downstream_id)) if downstream_id == REMOTE_SERVER_PROJECT_ID
        );
        if !serves_its_owner || project_id != REMOTE_SERVER_PROJECT_ID {
            bail!("private files are only served to the machine that opened this project");
        }
        let relative = RelPath::from_proto(path)
            .ok()
            .filter(|relative| !relative.is_empty())
            .filter(|_| !cfg!(windows) || names_a_path_inside_on_windows(path))
            .with_context(|| format!("{path:?} does not name a file inside this project"))?;
        let worktree = self
            .worktree_for_id(WorktreeId::from_proto(worktree_id), cx)
            .context("worktree not found")?;
        let worktree = worktree.read(cx);
        let local = worktree.as_local().context("worktree not found")?;
        if !local.is_path_private(&relative) {
            bail!("{} is not a private file", relative.as_unix_str());
        }
        Ok(PrivateFileLocation {
            root: worktree.abs_path().to_path_buf(),
            relative,
            windows_drive_mounts: windows_drive_mounts(),
        })
    }
}

/// A private file of a remote project's worktree, reached through its host.
///
/// Only the file's bytes travel. Remembers what it last read so a write can
/// say what it replaces: the caller backs that content up before writing, and
/// a file that changed in between must not be overwritten without its copy.
#[derive(Clone)]
pub struct RemotePrivateFile {
    client: AnyProtoClient,
    executor: gpui::BackgroundExecutor,
    worktree_id: WorktreeId,
    path: Arc<RelPath>,
    /// `None` until a load answers; then the hash of what it returned, or
    /// `Some(None)` when there was no file.
    loaded_sha256: Arc<parking_lot::Mutex<Option<Option<Vec<u8>>>>>,
}

impl RemotePrivateFile {
    pub fn path(&self) -> &Arc<RelPath> {
        &self.path
    }

    pub async fn load(&self) -> Result<Option<Vec<u8>>> {
        let request = self.client.request(proto::LoadPrivateFile {
            project_id: REMOTE_SERVER_PROJECT_ID,
            worktree_id: self.worktree_id.to_proto(),
            path: self.path.to_proto(),
        });
        let response =
            private_file_request(request, self.executor.timer(PRIVATE_FILE_TIMEOUT)).await?;
        *self.loaded_sha256.lock() = Some(response.content.as_deref().map(sha256));
        Ok(response.content)
    }

    /// Replaces the file, provided it is still what [`Self::load`] last saw.
    pub async fn write(&self, content: Vec<u8>) -> Result<()> {
        if content.len() > PRIVATE_FILE_MAX_BYTES {
            bail!(
                "this file is {} bytes; the limit is {PRIVATE_FILE_MAX_BYTES}",
                content.len()
            );
        }
        let Some(replaces_sha256) = self.loaded_sha256.lock().clone() else {
            bail!("the file must be read, and so backed up, before it is replaced");
        };
        let written_sha256 = sha256(&content);
        let request = self.client.request(proto::WritePrivateFile {
            project_id: REMOTE_SERVER_PROJECT_ID,
            worktree_id: self.worktree_id.to_proto(),
            path: self.path.to_proto(),
            content,
            replaces_sha256,
        });
        private_file_request(request, self.executor.timer(PRIVATE_FILE_TIMEOUT)).await?;
        *self.loaded_sha256.lock() = Some(Some(written_sha256));
        Ok(())
    }
}

async fn private_file_request<T>(
    request: impl Future<Output = Result<T>>,
    timer: impl Future<Output = ()>,
) -> Result<T> {
    crate::agent_server_store::remote_request_within(
        request,
        timer,
        PRIVATE_FILE_TIMEOUT,
        PRIVATE_FILE_SERVER_OUT_OF_DATE,
        PRIVATE_FILE_UNANSWERED,
    )
    .await
}

/// A path sent over the wire is slash-separated. A Windows host would read a
/// backslash as a separator and a colon as a drive or a stream, so a path
/// holding either names some other file than the one checked.
fn names_a_path_inside_on_windows(path: &str) -> bool {
    !path.contains(['\\', ':'])
}

/// Where this host mounts the Windows drives, when it is a WSL distro.
///
/// The default mount root only: one moved in `/etc/wsl.conf` is not noticed.
/// Worked out once per process, since it is asked on the foreground thread
/// for every request and the answer cannot change while the server runs.
fn windows_drive_mounts() -> Option<PathBuf> {
    static MOUNTS: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    MOUNTS
        .get_or_init(|| {
            if !cfg!(target_os = "linux") {
                return None;
            }
            let inside_wsl = std::env::var_os("WSL_DISTRO_NAME").is_some()
                || std::fs::read_to_string("/proc/sys/kernel/osrelease")
                    .is_ok_and(|release| release.to_ascii_lowercase().contains("microsoft"));
            inside_wsl.then(|| PathBuf::from("/mnt"))
        })
        .clone()
}

/// Whether `path` is under a drive letter directly below `mounts`, as
/// `/mnt/c/Users/me` is.
fn is_on_a_windows_drive(path: &Path, mounts: &Path) -> bool {
    let Ok(below) = path.strip_prefix(mounts) else {
        return false;
    };
    below
        .components()
        .next()
        .and_then(|drive| drive.as_os_str().to_str())
        .is_some_and(|drive| {
            drive.len() == 1 && drive.chars().all(|letter| letter.is_ascii_alphabetic())
        })
}

fn sha256(content: &[u8]) -> Vec<u8> {
    use sha2::Digest as _;
    sha2::Sha256::digest(content).to_vec()
}

/// A private file as the server resolves it.
///
/// Reached below the worktree root without following any link, so a request
/// can only ever touch a regular file inside the project.
///
/// A link at the file itself is refused for reading and for writing alike,
/// even one pointing inside the project (`.env -> .env.development`):
/// following it would write a file other than the one named, and replacing it
/// would quietly turn the link into a copy.
struct PrivateFileLocation {
    root: PathBuf,
    relative: Arc<RelPath>,
    /// Where this host mounts Windows drives, when it is a WSL distro.
    windows_drive_mounts: Option<PathBuf>,
}

impl PrivateFileLocation {
    fn read(&self) -> Result<Option<Vec<u8>>> {
        self.refuse_on_a_windows_drive()?;
        let components = self.components();
        let content =
            util::fs::read_file_below(&self.root, &components, PRIVATE_FILE_MAX_BYTES as u64)
                .map_err(|error| self.explain(error, "read"))?;
        log::debug!(
            "read private file {} ({} bytes)",
            self.relative.as_unix_str(),
            content.as_ref().map_or(0, Vec::len)
        );
        Ok(content)
    }

    /// Replaces the file, provided it still is what the writer read.
    ///
    /// The replacement is a new file: owner-only, owned by the user this
    /// server runs as, and detached from any hard link the old one shared.
    fn write(&self, content: &[u8], replaces_sha256: Option<&[u8]>) -> Result<()> {
        if content.len() > PRIVATE_FILE_MAX_BYTES {
            bail!(
                "this file is {} bytes; the limit is {PRIVATE_FILE_MAX_BYTES}",
                content.len()
            );
        }
        self.refuse_on_a_windows_drive()?;
        let shown = self.relative.as_unix_str();
        let still_as_read = |current: Option<&[u8]>| -> Result<()> {
            match (current, replaces_sha256) {
                (None, None) => Ok(()),
                (Some(current), Some(expected)) if sha256(current) == expected => Ok(()),
                (None, Some(_)) => {
                    bail!("{shown} was removed on the remote since it was read; pull again")
                }
                (Some(_), _) => {
                    bail!("{shown} changed on the remote since it was read; pull again")
                }
            }
        };
        let components = self.components();
        util::fs::replace_file_below(
            &self.root,
            &components,
            content,
            PRIVATE_FILE_MAX_BYTES as u64,
            still_as_read,
        )
        .map_err(|error| match error.downcast::<util::fs::FileBelowError>() {
            Ok(error) => self.explain(error, "write"),
            Err(refused) => refused,
        })?;
        log::debug!("wrote private file {shown} ({} bytes)", content.len());
        Ok(())
    }

    /// Reading or writing there touches the Windows machine, which opening
    /// the project in WSL means not to do -- and modes are not kept there, so
    /// a file written would be readable by every Windows user the drive allows.
    fn refuse_on_a_windows_drive(&self) -> Result<()> {
        if let Some(mounts) = &self.windows_drive_mounts
            && is_on_a_windows_drive(&canonical_or_as_is(&self.root), &canonical_or_as_is(mounts))
        {
            bail!(
                "{} lives on the Windows drive; open the project from the Linux filesystem",
                self.relative.as_unix_str()
            );
        }
        Ok(())
    }

    fn components(&self) -> Vec<&str> {
        self.relative.components().collect()
    }

    fn explain(&self, error: util::fs::FileBelowError, doing: &str) -> anyhow::Error {
        use util::fs::FileBelowError;

        let shown = self.relative.as_unix_str();
        match error {
            FileBelowError::Link => anyhow!(
                "{shown} is a symbolic link, or inside one; only a regular file in this project is synced"
            ),
            FileBelowError::NotAFile => anyhow!("{shown} is not a regular file"),
            FileBelowError::TooLarge => {
                anyhow!("{shown} is larger than {PRIVATE_FILE_MAX_BYTES} bytes")
            }
            FileBelowError::Io(error) => {
                anyhow!(error).context(format!("could not {doing} {shown}"))
            }
        }
    }
}

fn canonical_or_as_is(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod private_file_tests {
    use super::*;

    struct Sandbox(PathBuf);

    impl Sandbox {
        fn new(name: &str) -> Self {
            let directory = std::env::temp_dir()
                .join(format!("zode-private-file-{name}-{}", std::process::id()));
            std::fs::remove_dir_all(&directory).ok();
            std::fs::create_dir_all(directory.join("project")).expect("a sandbox");
            std::fs::create_dir_all(directory.join("outside")).expect("a sandbox");
            Self(directory)
        }

        fn project(&self) -> PathBuf {
            self.0.join("project")
        }

        #[cfg(unix)]
        fn outside(&self) -> PathBuf {
            self.0.join("outside")
        }

        fn location(&self, relative: &str) -> PrivateFileLocation {
            PrivateFileLocation {
                root: self.project(),
                relative: RelPath::from_proto(relative).expect("a relative path"),
                windows_drive_mounts: None,
            }
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn assert_refused<T>(result: Result<T>, expected: &str) {
        let Err(error) = result else {
            panic!("the request must be refused");
        };
        assert!(
            format!("{error:#}").contains(expected),
            "expected {expected:?}, got {error:#}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_project_is_neither_read_nor_written() {
        let sandbox = Sandbox::new("link-out");
        let secret = sandbox.outside().join("shadow");
        std::fs::write(&secret, b"root:x").unwrap();
        std::os::unix::fs::symlink(&secret, sandbox.project().join(".env")).unwrap();
        let location = sandbox.location(".env");

        assert_refused(location.read(), "is a symbolic link");
        assert_refused(location.write(b"A=1", None), "is a symbolic link");
        assert_refused(
            location.write(b"A=1", Some(&sha256(b"root:x"))),
            "is a symbolic link",
        );
        assert_eq!(std::fs::read(&secret).unwrap(), b"root:x");
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_link_out_of_the_project_is_refused_even_for_a_missing_file() {
        let sandbox = Sandbox::new("dir-link-out");
        std::os::unix::fs::symlink(sandbox.outside(), sandbox.project().join("config")).unwrap();
        let location = sandbox.location("config/.env");

        assert_refused(location.read(), "is a symbolic link");
        assert_refused(location.write(b"A=1", None), "is a symbolic link");
        assert!(
            !sandbox.outside().join(".env").exists(),
            "a file was written outside the project"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_nowhere_is_refused() {
        let sandbox = Sandbox::new("dangling");
        std::os::unix::fs::symlink(
            sandbox.outside().join("missing"),
            sandbox.project().join(".env"),
        )
        .unwrap();

        assert_refused(sandbox.location(".env").read(), "is a symbolic link");
    }

    #[test]
    fn a_file_larger_than_the_cap_is_refused_both_ways() {
        let sandbox = Sandbox::new("oversize");
        let oversize = vec![b'A'; PRIVATE_FILE_MAX_BYTES + 1];
        std::fs::write(sandbox.project().join(".env"), &oversize).unwrap();
        let location = sandbox.location(".env");

        assert_refused(location.read(), "larger than");
        assert_refused(
            location.write(&oversize, Some(&sha256(&oversize))),
            "the limit is",
        );
    }

    #[test]
    fn a_file_that_appeared_since_it_was_read_is_not_replaced() {
        // The writer read "no file", so it backed nothing up. Writing anyway
        // would destroy content nobody has a copy of.
        let sandbox = Sandbox::new("stale-absent");
        let target = sandbox.project().join(".env");
        std::fs::write(&target, b"CREATED=meanwhile").unwrap();

        assert_refused(
            sandbox.location(".env").write(b"A=1", None),
            "changed on the remote since it was read",
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"CREATED=meanwhile");
    }

    #[test]
    fn a_file_edited_since_it_was_read_is_not_replaced() {
        let sandbox = Sandbox::new("stale-content");
        let target = sandbox.project().join(".env");
        std::fs::write(&target, b"EDITED=1").unwrap();

        assert_refused(
            sandbox
                .location(".env")
                .write(b"A=1", Some(&sha256(b"READ=1"))),
            "changed on the remote since it was read",
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"EDITED=1");
    }

    #[test]
    fn a_file_replaced_as_read_is_written_for_its_owner_only() {
        let sandbox = Sandbox::new("replace");
        let target = sandbox.project().join("services/api/.env");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"OLD=1").unwrap();
        let location = sandbox.location("services/api/.env");

        assert_eq!(location.read().unwrap(), Some(b"OLD=1".to_vec()));
        location.write(b"NEW=1", Some(&sha256(b"OLD=1"))).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"NEW=1");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn a_missing_file_reads_as_absent_and_is_created_inside() {
        let sandbox = Sandbox::new("create");
        let location = sandbox.location("deploy/.env.production");

        assert_eq!(location.read().unwrap(), None);
        location.write(b"A=1", None).unwrap();
        assert_eq!(
            std::fs::read(sandbox.project().join("deploy/.env.production")).unwrap(),
            b"A=1"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_write_the_host_refuses_leaves_the_file_as_it_was() {
        use std::os::unix::fs::PermissionsExt as _;

        let sandbox = Sandbox::new("write-refused");
        let directory = sandbox.project().join("locked");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(".env"), b"OLD=1").unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o500)).unwrap();

        // Root, or anything else that overrides modes, writes into a 0500
        // directory anyway, and then there is no refusal to observe.
        let probe = directory.join("probe");
        if std::fs::write(&probe, b"").is_ok() {
            std::fs::remove_file(&probe).ok();
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
            eprintln!("skipped: this process can write where its mode says it cannot");
            return;
        }

        let result = sandbox
            .location("locked/.env")
            .write(b"NEW=1", Some(&sha256(b"OLD=1")));

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_refused(result, "could not write locked/.env");
        assert_eq!(std::fs::read(directory.join(".env")).unwrap(), b"OLD=1");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_inside_the_project_is_neither_read_nor_replaced() {
        // Following it would write the target; replacing it would quietly
        // turn the link into a copy. Either surprises whoever made the link.
        let sandbox = Sandbox::new("link-inside");
        let target = sandbox.project().join(".env.development");
        std::fs::write(&target, b"DEV=1").unwrap();
        std::os::unix::fs::symlink(".env.development", sandbox.project().join(".env")).unwrap();
        let location = sandbox.location(".env");

        assert_refused(location.read(), "is a symbolic link");
        assert_refused(
            location.write(b"A=1", Some(&sha256(b"DEV=1"))),
            "is a symbolic link",
        );
        assert!(
            std::fs::symlink_metadata(sandbox.project().join(".env"))
                .unwrap()
                .file_type()
                .is_symlink(),
            "the link was replaced"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"DEV=1");
    }

    #[test]
    fn a_file_on_a_windows_drive_is_neither_read_nor_written() {
        let sandbox = Sandbox::new("windows-drive");
        let root = sandbox.0.join("c").join("Users").join("me").join("acme");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".env"), b"OLD=1").unwrap();
        let location = PrivateFileLocation {
            root: root.clone(),
            relative: RelPath::from_proto(".env").expect("a relative path"),
            windows_drive_mounts: Some(sandbox.0.clone()),
        };

        let refusal = ".env lives on the Windows drive; open the project from the Linux filesystem";
        assert_refused(location.read(), refusal);
        assert_refused(location.write(b"NEW=1", Some(&sha256(b"OLD=1"))), refusal);
        assert_eq!(std::fs::read(root.join(".env")).unwrap(), b"OLD=1");
    }

    #[test]
    fn only_a_single_letter_under_the_mounts_is_a_windows_drive() {
        let mounts = Path::new("/mnt");
        for drive in ["/mnt/c", "/mnt/c/Users/me/acme", "/mnt/D/work"] {
            assert!(
                is_on_a_windows_drive(Path::new(drive), mounts),
                "{drive} is a Windows drive"
            );
        }
        for linux in [
            "/home/me/acme",
            "/mnt/wsl/acme",
            "/mnt/wslg",
            "/mnt",
            "/mnt/cc/acme",
        ] {
            assert!(
                !is_on_a_windows_drive(Path::new(linux), mounts),
                "{linux} is on the Linux filesystem"
            );
        }
    }

    #[test]
    fn a_path_a_windows_host_would_read_as_another_file_is_refused() {
        for path in [
            r"..\outside\.env",
            r"C:\Windows\win.ini",
            "C:.env",
            r"config\.env",
        ] {
            assert!(
                !names_a_path_inside_on_windows(path),
                "{path:?} must be refused on Windows"
            );
        }
        for path in [".env", "services/api/.env.production"] {
            assert!(names_a_path_inside_on_windows(path), "{path:?} is fine");
        }
    }

    #[test]
    fn a_host_that_never_answers_is_not_called_out_of_date() {
        let result = futures::executor::block_on(private_file_request(
            futures::future::pending::<Result<()>>(),
            futures::future::ready(()),
        ));

        let Err(error) = result else {
            panic!("a request nobody answered cannot succeed");
        };
        let message = format!("{error:#}");
        assert!(message.contains("try again"), "{message}");
        assert!(
            !message.contains(PRIVATE_FILE_SERVER_OUT_OF_DATE),
            "a timeout is not an old server: {message}"
        );
    }
}
