//! What a paired device may read of the folders open on this Zode: listings,
//! file contents and the uncommitted diff. Read-only by construction -- nothing
//! here writes, and nothing runs a program.
//!
//! A device names a folder by id and a path inside it, and the path is only
//! ever looked up in the folder's scanned snapshot, never joined onto the disk.
//! Whatever the snapshot does not contain -- including everything the user's
//! `file_scan_exclusions` hide, and anything that reaches outside the folder
//! through a symlink -- is "not found", whatever the path says. So is a file
//! the `private_files` setting names, a symlink of any kind, and anything that
//! is not an ordinary file on disk.

use std::{
    cmp::Ordering,
    collections::BinaryHeap,
    future::Future,
    io::Read as _,
    path::{Path, PathBuf},
    pin::pin,
    sync::Arc,
    time::Duration,
};

use anyhow::bail;
use fs::Fs;
use futures::{
    channel::oneshot,
    future::{Either, select},
};
use git::bounded_diff::{BoundedDiff, PathFilter, ScopedDiff};
use gpui::{App, AppContext as _, Context, Entity, Subscription, Task, WeakEntity};
use project::{
    Project,
    git_store::{LocalRepositoryState, RepositoryState},
};
use remote_relay_protocol::{Control, FileEntry, FileKind, MAX_INNER_PAYLOAD_LEN, error_code};
use settings::WorktreeId;
use util::{ResultExt as _, rel_path::RelPath};
use worktree::{Entry, Snapshot, Worktree, WorktreeSettings};

/// The most entries a listing carries; a longer directory is cut and says so.
pub const MAX_LIST_ENTRIES: usize = 2000;
/// The largest file that is sent.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// The most of a diff that is sent.
pub const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;
/// How long a request may take before it is answered with an error, whatever
/// the disk is doing.
pub const REQUEST_DEADLINE: Duration = Duration::from_secs(30);
/// How long git may spend on one diff once it has started, which leaves the
/// request room to say what went wrong before its own deadline.
pub const DIFF_DEADLINE: Duration = Duration::from_secs(20);

/// The most a whole listing may weigh once serialized, so that however the
/// names are escaped it stays inside what the host reserves for one answer.
const MAX_LIST_BYTES: usize = 1536 * 1024;

/// Room kept in one control message for everything but the entries.
const LIST_REPLY_OVERHEAD: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowseError {
    pub code: &'static str,
    pub message: String,
}

impl BrowseError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn not_found() -> Self {
        Self::new(error_code::NOT_FOUND, "there is no such file or folder")
    }

    pub(crate) fn internal(message: &str) -> Self {
        Self::new(error_code::INTERNAL, message)
    }
}

/// The reply to a request, or an `internal` error if `deadline` finishes first.
/// The request is dropped, which stops it where it can be stopped.
pub(crate) async fn answer_within<Reply>(
    deadline: impl Future<Output = ()>,
    reply: impl Future<Output = Result<Reply, BrowseError>>,
) -> Result<Reply, BrowseError> {
    match select(pin!(reply), pin!(deadline)).await {
        Either::Left((reply, _)) => reply,
        Either::Right(((), _)) => Err(BrowseError::internal("the request took too long")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileRequest {
    List {
        worktree_id: Option<String>,
        path: String,
    },
    Read {
        worktree_id: String,
        path: String,
    },
    Diff {
        worktree_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub entries: Vec<FileEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileReply {
    Listing(Listing),
    File(Vec<u8>),
    Diff { text: Vec<u8>, truncated: bool },
}

/// Finds the folders open in any window, and answers [`FileRequest`]s about
/// them. Holds only weak handles: it keeps no project alive.
pub struct FileBrowser {
    projects: Vec<WeakEntity<Project>>,
    _observe_new: Subscription,
}

impl FileBrowser {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let this = cx.weak_entity();
        let observe_new = cx.observe_new::<Project>(move |_project, _window, cx| {
            let project = cx.entity();
            this.update(cx, |this, _| this.track(&project)).log_err();
        });
        Self {
            projects: Vec::new(),
            _observe_new: observe_new,
        }
    }

    fn track(&mut self, project: &Entity<Project>) {
        self.projects.retain(|known| known.upgrade().is_some());
        self.projects.push(project.downgrade());
    }

    /// Every folder the user can see in a project on this machine, once each.
    fn visible_worktrees(&self, cx: &App) -> Vec<(Entity<Project>, Entity<Worktree>)> {
        let mut found: Vec<(Entity<Project>, Entity<Worktree>)> = Vec::new();
        for project in self.projects.iter().filter_map(WeakEntity::upgrade) {
            if !project.read(cx).is_local() {
                continue;
            }
            for worktree in project.read(cx).visible_worktrees(cx) {
                let id = worktree.read(cx).id();
                if found.iter().all(|(_, known)| known.read(cx).id() != id) {
                    found.push((project.clone(), worktree));
                }
            }
        }
        found
    }

    fn worktree(
        &self,
        worktree_id: &str,
        cx: &App,
    ) -> Result<(Entity<Project>, Entity<Worktree>), BrowseError> {
        let id = worktree_id
            .parse::<u64>()
            .map(WorktreeId::from_proto)
            .map_err(|_| BrowseError::not_found())?;
        self.visible_worktrees(cx)
            .into_iter()
            .find(|(_, worktree)| worktree.read(cx).id() == id)
            .ok_or_else(BrowseError::not_found)
    }

    pub fn handle(
        &self,
        request: FileRequest,
        cx: &mut App,
    ) -> Task<Result<FileReply, BrowseError>> {
        match request {
            FileRequest::List { worktree_id, path } => {
                let listing = self.list(worktree_id.as_deref(), path, cx);
                cx.background_spawn(async move { Ok(FileReply::Listing(listing.await?)) })
            }
            FileRequest::Read { worktree_id, path } => {
                let contents = self.read(&worktree_id, path, cx);
                cx.background_spawn(async move { Ok(FileReply::File(contents.await?)) })
            }
            FileRequest::Diff { worktree_id } => {
                let diff = self.diff(&worktree_id, cx);
                cx.background_spawn(async move {
                    let (text, truncated) = diff.await?;
                    Ok(FileReply::Diff { text, truncated })
                })
            }
        }
    }

    fn list(
        &self,
        worktree_id: Option<&str>,
        path: String,
        cx: &mut App,
    ) -> Task<Result<Listing, BrowseError>> {
        let Some(worktree_id) = worktree_id else {
            return Task::ready(Ok(self.roots(cx)));
        };
        let snapshot = match self.worktree(worktree_id, cx) {
            Ok((_, worktree)) => worktree.read(cx).snapshot(),
            Err(error) => return Task::ready(Err(error)),
        };
        cx.background_spawn(async move { list_children(&snapshot, &path) })
    }

    fn roots(&self, cx: &App) -> Listing {
        let mut roots: Vec<FileEntry> = self
            .visible_worktrees(cx)
            .into_iter()
            .map(|(_, worktree)| {
                let worktree = worktree.read(cx);
                let name = match worktree.root_name_str() {
                    "" => worktree.abs_path().display().to_string(),
                    name => name.to_string(),
                };
                FileEntry {
                    name,
                    kind: if worktree.is_single_file() {
                        FileKind::File
                    } else {
                        FileKind::Directory
                    },
                    size: None,
                    worktree_id: Some(worktree.id().to_proto().to_string()),
                }
            })
            .collect();
        roots.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.worktree_id.cmp(&right.worktree_id))
        });
        let truncated = roots.len() > MAX_LIST_ENTRIES;
        roots.truncate(MAX_LIST_ENTRIES);
        Listing {
            entries: roots,
            truncated,
        }
    }

    fn read(
        &self,
        worktree_id: &str,
        path: String,
        cx: &App,
    ) -> Task<Result<Vec<u8>, BrowseError>> {
        let (fs, snapshot, settings) = match self.worktree(worktree_id, cx) {
            Ok((project, worktree)) => {
                let worktree = worktree.read(cx);
                let Some(settings) = worktree.as_local().map(|local| local.settings()) else {
                    return Task::ready(Err(BrowseError::not_found()));
                };
                (project.read(cx).fs().clone(), worktree.snapshot(), settings)
            }
            Err(error) => return Task::ready(Err(error)),
        };
        cx.background_spawn(async move { read_text(fs, &snapshot, &settings, &path).await })
    }

    fn diff(&self, worktree_id: &str, cx: &mut App) -> Task<Result<(Vec<u8>, bool), BrowseError>> {
        let (project, worktree) = match self.worktree(worktree_id, cx) {
            Ok(found) => found,
            Err(error) => return Task::ready(Err(error)),
        };
        let worktree_path = worktree.read(cx).abs_path();
        let is_single_file = worktree.read(cx).is_single_file();
        let Some(settings) = worktree.read(cx).as_local().map(|local| local.settings()) else {
            return Task::ready(Err(BrowseError::not_found()));
        };
        let repository = project
            .read(cx)
            .git_store()
            .read(cx)
            .repositories()
            .values()
            .filter(|repository| {
                worktree_path.starts_with(repository.read(cx).work_directory_abs_path.as_ref())
            })
            .max_by_key(|repository| {
                repository
                    .read(cx)
                    .work_directory_abs_path
                    .as_os_str()
                    .len()
            })
            .cloned();
        let not_in_a_repository = || {
            BrowseError::new(
                error_code::NOT_FOUND,
                "this folder is not in a git repository",
            )
        };
        let Some(repository) = repository else {
            return Task::ready(Err(not_in_a_repository()));
        };
        let Some(scope) = repository.read(cx).abs_path_to_repo_path(&worktree_path) else {
            return Task::ready(Err(not_in_a_repository()));
        };
        // Whatever the folder does not show -- private files, and what the scan
        // exclusions hide -- the diff does not show either.
        let keep: PathFilter = Arc::new({
            let scope = scope.clone();
            move |path| {
                path.strip_prefix(&scope).is_ok_and(|inside| {
                    if is_single_file && inside.is_empty() {
                        !single_file_is_hidden(&settings, &worktree_path)
                    } else {
                        !settings.is_path_private(inside) && !settings.is_path_excluded(inside)
                    }
                })
            }
        });
        let request = ScopedDiff {
            scope,
            keep,
            max_bytes: MAX_DIFF_BYTES,
            deadline: DIFF_DEADLINE,
            executor: cx.background_executor().clone(),
        };
        let (stop_wanting, still_wanted) = oneshot::channel();
        // `HEAD` as the base makes this the working tree against the last
        // commit, staged and unstaged changes together; the plain worktree
        // diff would leave out whatever is already staged.
        let diff = repository.update(cx, |repository, _| {
            repository.send_job(None, move |state, _| {
                diff_while_wanted(state, request, still_wanted)
            })
        });
        cx.background_spawn(async move {
            // Dropped with this task, which is what a request that timed out or
            // whose session ended does to it, and which tells the job above
            // that nobody wants its answer any more.
            let _stop_wanting = stop_wanting;
            match diff.await {
                Ok(Ok(Some(diff))) => Ok((diff.text.into_bytes(), diff.truncated)),
                Ok(Ok(None)) => Err(BrowseError::new(
                    error_code::NOT_FOUND,
                    "this repository has no commit to compare with",
                )),
                Ok(Err(error)) => {
                    log::warn!("could not diff for a remote device: {error:#}");
                    Err(BrowseError::internal("the diff could not be made"))
                }
                Err(_cancelled) => Err(BrowseError::internal("the diff was cancelled")),
            }
        })
    }
}

/// The diff the repository's job queue was asked for, unless `wanted` ends
/// first. The queue runs one job at a time and does not know that the request
/// behind a job is gone, so a job that went on would hold up every other git
/// job of the repository for an answer nobody reads: one still waiting its turn
/// is skipped, and one that is running is dropped, which stops its git.
async fn diff_while_wanted(
    state: RepositoryState,
    request: ScopedDiff,
    mut wanted: oneshot::Receiver<()>,
) -> anyhow::Result<Option<BoundedDiff>> {
    if wanted.try_recv().is_err() {
        bail!("the request was dropped before its turn");
    }
    let RepositoryState::Local(LocalRepositoryState { backend, .. }) = state else {
        bail!("the repository is not on this machine");
    };
    match select(backend.diff_head_to_worktree_scoped(request), wanted).await {
        Either::Left((diff, _)) => diff,
        Either::Right(_) => bail!("the request was dropped while its diff was being made"),
    }
}

/// A client's path as a path inside the snapshot, or `None` for an absolute
/// path or one that would have to be rewritten to be a plain relative path: a
/// `..`, a `.` or an empty component anywhere but the very start or end. A
/// leading `./` and a trailing `/` are dropped, which changes nothing about
/// which entry is meant.
fn snapshot_path(path: &str) -> Option<&RelPath> {
    RelPath::unix(path).ok()
}

/// Whether `entry` is acceptable in itself: inside the folder, not private,
/// and not a symlink.
fn is_reachable_itself(entry: &Entry) -> bool {
    !entry.is_external && !entry.is_private && entry.canonical_path.is_none()
}

/// Whether a device may see `entry` at all. A symlink is never followed, and
/// neither is anything below one: the target is not in the snapshot under the
/// name the device used, so what it is cannot be told from here.
fn is_reachable(snapshot: &Snapshot, entry: &Entry) -> bool {
    is_reachable_itself(entry)
        && entry
            .path
            .ancestors()
            .skip(1)
            .filter(|ancestor| !ancestor.is_empty())
            .filter_map(|ancestor| snapshot.entry_for_path(ancestor))
            .all(|ancestor| ancestor.canonical_path.is_none())
}

/// Whether the folder's settings hide the one file a folder of a single file
/// is. The settings name paths inside a folder and this one has none, so the
/// file is asked about by its name, as it was when the folder was opened. A
/// name that cannot be judged is hidden.
fn single_file_is_hidden(settings: &WorktreeSettings, absolute: &Path) -> bool {
    let Some(name) = absolute
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| RelPath::unix(name).ok())
    else {
        return true;
    };
    settings.is_path_private(name) || settings.is_path_excluded(name)
}

struct Ranked<Key, Item> {
    key: Key,
    item: Item,
}

impl<Key: Ord, Item> Ord for Ranked<Key, Item> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}

impl<Key: Ord, Item> PartialOrd for Ranked<Key, Item> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<Key: Ord, Item> PartialEq for Ranked<Key, Item> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<Key: Ord, Item> Eq for Ranked<Key, Item> {}

/// The `count` first of `items` by `key`, in order, and how many items there
/// were. Holds the keys of `count` items at a time, however many there are.
pub(crate) fn first_by_key<Item, Key: Ord>(
    items: impl Iterator<Item = Item>,
    count: usize,
    key: impl Fn(&Item) -> Key,
) -> (Vec<Item>, usize) {
    let mut first = BinaryHeap::with_capacity(count.saturating_add(1));
    let mut total = 0;
    for item in items {
        total += 1;
        first.push(Ranked {
            key: key(&item),
            item,
        });
        if first.len() > count {
            first.pop();
        }
    }
    let ordered = first
        .into_sorted_vec()
        .into_iter()
        .map(|ranked| ranked.item)
        .collect();
    (ordered, total)
}

fn list_children(snapshot: &Snapshot, path: &str) -> Result<Listing, BrowseError> {
    let parent = snapshot_path(path).ok_or_else(BrowseError::not_found)?;
    let directory = snapshot
        .entry_for_path(parent)
        .filter(|entry| entry.is_dir() && is_reachable(snapshot, entry))
        .ok_or_else(BrowseError::not_found)?;
    // The directory is reachable, and so is everything above it: only a child
    // itself can be what makes it unreachable.
    let (children, total) = first_by_key(
        snapshot
            .child_entries(&directory.path)
            .filter(|entry| is_reachable_itself(entry)),
        MAX_LIST_ENTRIES,
        |entry| {
            let name = entry.path.file_name().unwrap_or_default();
            (!entry.is_dir(), name.to_lowercase(), name.to_string())
        },
    );
    let truncated = total > MAX_LIST_ENTRIES;
    let entries = children
        .into_iter()
        .map(|entry| FileEntry {
            name: entry.path.file_name().unwrap_or_default().to_string(),
            kind: if entry.is_dir() {
                FileKind::Directory
            } else {
                FileKind::File
            },
            size: entry.is_file().then_some(entry.size),
            worktree_id: None,
        })
        .collect();
    Ok(Listing { entries, truncated })
}

async fn read_text(
    fs: Arc<dyn Fs>,
    snapshot: &Snapshot,
    settings: &WorktreeSettings,
    path: &str,
) -> Result<Vec<u8>, BrowseError> {
    let relative = snapshot_path(path).ok_or_else(BrowseError::not_found)?;
    let entry = snapshot
        .entry_for_path(relative)
        .filter(|entry| entry.is_file() && !entry.is_fifo && is_reachable(snapshot, entry))
        .ok_or_else(BrowseError::not_found)?;
    // The flag on the entry of a single file is not to be trusted: a rescan
    // judges it by an empty path, which no setting names.
    if entry.path.is_empty() && single_file_is_hidden(settings, snapshot.abs_path()) {
        return Err(BrowseError::not_found());
    }
    let too_large = || {
        BrowseError::new(
            error_code::TOO_LARGE,
            format!("files over {MAX_FILE_BYTES} bytes are not sent"),
        )
    };
    if entry.size > MAX_FILE_BYTES {
        return Err(too_large());
    }
    // The path on disk comes from the entry the snapshot returned, not from
    // the client's string.
    let absolute: PathBuf = if entry.path.is_empty() {
        snapshot.abs_path().to_path_buf()
    } else {
        snapshot.abs_path().join(entry.path.as_std_path())
    };
    let metadata = fs
        .metadata(&absolute)
        .await
        .map_err(|error| {
            log::warn!("could not stat a file for a remote device: {error:#}");
            BrowseError::internal("the file could not be read")
        })?
        // The snapshot may be a moment old: what is on disk now may have been
        // swapped for something the snapshot never allowed.
        .filter(|metadata| !metadata.is_symlink && metadata.is_regular_file)
        .ok_or_else(BrowseError::not_found)?;
    // The check above covers the file itself. A folder on the way to it may
    // have become a symlink since the scan, and only resolving the whole path
    // shows where it leads. The root is compared as resolved, so a folder that
    // is itself opened through a symlink still works.
    let real = fs.canonicalize(&absolute).await.log_err();
    let root = fs.canonicalize(snapshot.abs_path()).await.log_err();
    let real = match (real, root) {
        (Some(real), Some(root)) if real.starts_with(&root) => real,
        _ => return Err(BrowseError::not_found()),
    };
    // The snapshot may be a moment old; the disk decides what is too large.
    if metadata.len > MAX_FILE_BYTES {
        return Err(too_large());
    }
    // Reads at most one byte more than is allowed, so a file that has grown
    // since it was measured costs no more memory than one that has not. The
    // read blocks, which is why this runs on the background executor.
    let mut bytes = Vec::new();
    fs.open_sync(&real)
        .await
        .and_then(|reader| Ok(reader.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?))
        .map_err(|error| {
            log::warn!("could not read a file for a remote device: {error:#}");
            BrowseError::internal("the file could not be read")
        })?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(too_large());
    }
    if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
        return Err(BrowseError::new(
            error_code::BINARY,
            "this file is not text",
        ));
    }
    Ok(bytes)
}

/// `items` cut into batches whose serialized size, with `overhead` bytes of
/// message around each, fits one frame; and whether any item was left out.
/// `json_len` is what an item weighs once serialized, or `None` if it cannot be.
///
/// There is always at least one batch, so an empty answer is still an answer.
/// An item that cannot be measured is not one that can be sent either, and
/// neither is one too large for a frame of its own: both are left out, so a
/// single bad item cannot make the whole message unsendable. Reaching
/// `total_limit` bytes of items leaves out the rest.
pub(crate) fn batch_by_json_size<Item>(
    items: impl IntoIterator<Item = Item>,
    json_len: impl Fn(&Item) -> Option<usize>,
    overhead: usize,
    total_limit: usize,
) -> (Vec<Vec<Item>>, bool) {
    let budget = MAX_INNER_PAYLOAD_LEN - overhead;
    let mut batches: Vec<Vec<Item>> = vec![Vec::new()];
    let mut used = 0;
    let mut total: usize = 0;
    let mut cut = false;
    for item in items {
        // Each item costs its JSON and a comma.
        let Some(cost) = json_len(&item).map(|length| length + 1) else {
            cut = true;
            continue;
        };
        if cost > budget {
            log::warn!("leaving out an item of {cost} bytes, which no message can carry");
            cut = true;
            continue;
        }
        if total.saturating_add(cost) > total_limit {
            cut = true;
            break;
        }
        total += cost;
        let current = batches.last().map_or(0, Vec::len);
        if used + cost > budget && current > 0 {
            batches.push(Vec::new());
            used = 0;
        }
        used += cost;
        if let Some(batch) = batches.last_mut() {
            batch.push(item);
        }
    }
    (batches, cut)
}

/// A listing as the control messages that carry it: as many as it takes to keep
/// each under one frame, every one but the last marked `more`.
pub fn list_replies(request_id: u32, listing: Listing) -> Vec<Control> {
    let Listing { entries, truncated } = listing;
    let (batches, cut) = batch_by_json_size(
        entries,
        |entry| serde_json::to_vec(entry).log_err().map(|json| json.len()),
        LIST_REPLY_OVERHEAD,
        MAX_LIST_BYTES,
    );
    let truncated = truncated || cut;
    let last = batches.len() - 1;
    batches
        .into_iter()
        .enumerate()
        .map(|(index, entries)| Control::FilesListReply {
            request_id,
            entries,
            truncated,
            more: index < last,
        })
        .collect()
}
