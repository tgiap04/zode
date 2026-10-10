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

use std::{future::Future, io::Read as _, path::PathBuf, pin::pin, sync::Arc, time::Duration};

use fs::Fs;
use futures::future::{Either, select};
use git::bounded_diff::{PathFilter, ScopedDiff};
use gpui::{App, AppContext as _, Context, Entity, Subscription, Task, WeakEntity};
use project::{
    Project,
    git_store::{LocalRepositoryState, RepositoryState},
};
use remote_relay_protocol::{Control, FileEntry, FileKind, MAX_INNER_PAYLOAD_LEN, error_code};
use settings::WorktreeId;
use util::{ResultExt as _, rel_path::RelPath};
use worktree::{Entry, Snapshot, Worktree};

/// The most entries a listing carries; a longer directory is cut and says so.
pub const MAX_LIST_ENTRIES: usize = 2000;
/// The largest file that is sent.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// The most of a diff that is sent.
pub const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;
/// How long a request may take before it is answered with an error, whatever
/// the disk is doing.
pub const REQUEST_DEADLINE: Duration = Duration::from_secs(30);

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
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn not_found() -> Self {
        Self::new(error_code::NOT_FOUND, "there is no such file or folder")
    }

    fn internal(message: &str) -> Self {
        Self::new(error_code::INTERNAL, message)
    }
}

/// The reply to a request, or an `internal` error if `deadline` finishes first.
/// The request is dropped, which stops it where it can be stopped.
pub(crate) async fn answer_within(
    deadline: impl Future<Output = ()>,
    reply: impl Future<Output = Result<FileReply, BrowseError>>,
) -> Result<FileReply, BrowseError> {
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
        let (fs, snapshot) = match self.worktree(worktree_id, cx) {
            Ok((project, worktree)) => {
                (project.read(cx).fs().clone(), worktree.read(cx).snapshot())
            }
            Err(error) => return Task::ready(Err(error)),
        };
        cx.background_spawn(async move { read_text(fs, &snapshot, &path).await })
    }

    fn diff(&self, worktree_id: &str, cx: &mut App) -> Task<Result<(Vec<u8>, bool), BrowseError>> {
        let (project, worktree) = match self.worktree(worktree_id, cx) {
            Ok(found) => found,
            Err(error) => return Task::ready(Err(error)),
        };
        let worktree_path = worktree.read(cx).abs_path();
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
                    !settings.is_path_private(inside) && !settings.is_path_excluded(inside)
                })
            }
        });
        let request = ScopedDiff {
            scope,
            keep,
            max_bytes: MAX_DIFF_BYTES,
        };
        // `HEAD` as the base makes this the working tree against the last
        // commit, staged and unstaged changes together; the plain worktree
        // diff would leave out whatever is already staged.
        let diff = repository.update(cx, |repository, _| {
            repository.send_job(None, move |state, _| async move {
                match state {
                    RepositoryState::Local(LocalRepositoryState { backend, .. }) => {
                        backend.diff_head_to_worktree_scoped(request).await
                    }
                    RepositoryState::Remote(_) => {
                        Err(anyhow::anyhow!("the repository is not on this machine"))
                    }
                }
            })
        });
        cx.background_spawn(async move {
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

/// A client's path as a path inside the snapshot, or `None` for an absolute
/// path or one that would have to be rewritten to be a plain relative path: a
/// `..`, a `.` or an empty component anywhere but the very start or end. A
/// leading `./` and a trailing `/` are dropped, which changes nothing about
/// which entry is meant.
fn snapshot_path(path: &str) -> Option<&RelPath> {
    RelPath::unix(path).ok()
}

/// Whether a device may see `entry` at all. A symlink is never followed, and
/// neither is anything below one: the target is not in the snapshot under the
/// name the device used, so what it is cannot be told from here.
fn is_reachable(snapshot: &Snapshot, entry: &Entry) -> bool {
    !entry.is_external
        && !entry.is_private
        && !entry
            .path
            .ancestors()
            .filter(|ancestor| !ancestor.is_empty())
            .filter_map(|ancestor| snapshot.entry_for_path(ancestor))
            .any(|ancestor| ancestor.canonical_path.is_some())
}

fn list_children(snapshot: &Snapshot, path: &str) -> Result<Listing, BrowseError> {
    let parent = snapshot_path(path).ok_or_else(BrowseError::not_found)?;
    let directory = snapshot
        .entry_for_path(parent)
        .filter(|entry| entry.is_dir() && is_reachable(snapshot, entry))
        .ok_or_else(BrowseError::not_found)?;
    let mut children: Vec<&Entry> = snapshot
        .child_entries(&directory.path)
        .filter(|entry| is_reachable(snapshot, entry))
        .collect();
    children.sort_by_cached_key(|entry| {
        let name = entry.path.file_name().unwrap_or_default();
        (!entry.is_dir(), name.to_lowercase(), name.to_string())
    });
    let truncated = children.len() > MAX_LIST_ENTRIES;
    let entries = children
        .into_iter()
        .take(MAX_LIST_ENTRIES)
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
    path: &str,
) -> Result<Vec<u8>, BrowseError> {
    let relative = snapshot_path(path).ok_or_else(BrowseError::not_found)?;
    let entry = snapshot
        .entry_for_path(relative)
        .filter(|entry| entry.is_file() && !entry.is_fifo && is_reachable(snapshot, entry))
        .ok_or_else(BrowseError::not_found)?;
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

/// A listing as the control messages that carry it: as many as it takes to keep
/// each under one frame, every one but the last marked `more`.
pub fn list_replies(request_id: u32, listing: Listing) -> Vec<Control> {
    let Listing {
        entries,
        mut truncated,
    } = listing;
    let budget = MAX_INNER_PAYLOAD_LEN - LIST_REPLY_OVERHEAD;
    let mut batches: Vec<Vec<FileEntry>> = vec![Vec::new()];
    let mut used = 0;
    let mut total = 0;
    for entry in entries {
        // Each entry costs its JSON and a comma. An entry that cannot be
        // measured is not one that can be sent either.
        let Some(cost) = serde_json::to_vec(&entry)
            .log_err()
            .map(|json| json.len() + 1)
        else {
            continue;
        };
        if total + cost > MAX_LIST_BYTES {
            truncated = true;
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
            batch.push(entry);
        }
    }
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
