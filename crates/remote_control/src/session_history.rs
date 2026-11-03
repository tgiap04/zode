//! What a paired device sees of the agent conversations on this Zode, and what
//! it can start from one.
//!
//! The device names a session by its id and nothing else. Which agent it is,
//! where it ran and how it is resumed all come from this host's own session
//! index, and a session only counts if it ran inside a folder that is open in a
//! window right now -- the same scope the history panel keeps, so a device
//! cannot see or start anything the person at this screen could not.
//!
//! The sessions of a WSL distro, SSH host or dev container with a window open
//! here are listed too. The device knows nothing of hosts: it dedups, pages and
//! resumes by id alone, so a remote session's id carries its host --
//! `{token}:{id}`, the token a hash of the host's identity -- and a session on
//! this machine keeps the id it has always had.
//!
//! Everything that decides something is a plain function over plain values;
//! the two async entry points, [`list`] and [`resume`], only gather what those
//! functions need from the running app.

use std::{
    cmp::Reverse,
    collections::HashMap,
    fmt::Write as _,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use agent_sessions::{AgentKind, Fork, SessionIndex, SessionSummary, provider_for};
use agent_ui::{
    ProjectSessionSource, ResumeRefusal, SessionStore, project_session_source, resume_refusal_on,
    resume_session,
};
use futures::StreamExt as _;
use gpui::{AnyWindowHandle, App, AsyncApp, Entity, EntityId, Global, WeakEntity, WindowHandle};
use remote_relay_protocol::{
    Control, SessionCursor, SessionEntry, UnavailableHost, error_code, host_status,
};
use settings::{Settings as _, SettingsLocation, WorktreeId};
use sha2::{Digest as _, Sha256};
use util::{ResultExt as _, paths::PathStyle, rel_path::RelPath};
use workspace::{
    MultiWorkspace, RemoteConnectionIdentity, Toast, Workspace, notifications::NotificationId,
};
use worktree::WorktreeSettings;

use crate::{
    agent_mirror::AgentMirror,
    file_browse::{self, BrowseError},
    session_router::UNSUPPORTED,
};

/// How many sessions a page holds when the device does not say.
pub(crate) const DEFAULT_PAGE_LIMIT: usize = 50;
/// The most a device may ask a page to hold.
pub(crate) const MAX_PAGE_LIMIT: usize = 200;

/// The most characters of a title or a preview that are sent. Transcripts put
/// no limit on either.
const MAX_TEXT_CHARS: usize = 200;
/// The most characters of a session's title that go into the notice on this
/// screen.
const MAX_TOAST_TITLE_CHARS: usize = 60;
/// The most characters of a device's name that go into that notice. A device
/// names itself, so the name is no more trusted than a title.
const MAX_TOAST_DEVICE_CHARS: usize = 40;
/// The longest id that is looked up. Every agent's own ids are far shorter.
const MAX_SESSION_ID_BYTES: usize = 128;
/// The most characters of a host's name that are sent.
const MAX_HOST_LABEL_CHARS: usize = 60;
/// The most hosts a reply says it could not list.
const MAX_UNAVAILABLE_HOSTS: usize = 16;
/// How many hex digits of the hash of a host's identity name it in an id.
const HOST_TOKEN_LEN: usize = 12;

/// Room kept in one `sessions_list_reply` for everything but its entries: the
/// tag, the request id, the flags, and a cursor whose id is as long as an id
/// may be.
const REPLY_OVERHEAD: usize = 512;

/// How long after one read of the sessions from disk the next may be asked for.
/// A read opens every transcript there is, so a device that asks again as each
/// answer arrives would keep the disk busy for as long as it likes; inside this
/// time it is answered from the read that was just made.
pub(crate) const MIN_FRESH_READ_INTERVAL: Duration = Duration::from_secs(5);

/// What a device asked of the sessions on this host, once the router has
/// checked its shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRequest {
    List {
        before: Option<SessionCursor>,
        /// Already in `1..=MAX_PAGE_LIMIT`.
        limit: usize,
    },
    Resume {
        session_id: String,
        fork: bool,
    },
}

pub(crate) fn clamp_limit(limit: Option<u32>) -> usize {
    limit
        .map_or(DEFAULT_PAGE_LIMIT, |limit| {
            usize::try_from(limit).unwrap_or(MAX_PAGE_LIMIT)
        })
        .clamp(1, MAX_PAGE_LIMIT)
}

/// Whether `id` could be a session id: 1 to 128 bytes of ASCII letters, digits,
/// `.`, `_` and `-`, the first a letter or a digit.
///
/// A shape and not a parse, because the agents disagree about what an id is --
/// Claude's are UUIDs and opencode's are not. What decides whether a session
/// exists is the host's own index; this keeps what a device sends -- the id of a
/// session to open, and the id inside a paging cursor -- from being looked at,
/// logged or compared at any length or in any alphabet, and keeps any id this
/// host would hand to an agent's command line from reading as one of its flags
/// or as `.` or `..`. The same shape is required of the sessions listed, so
/// nothing is offered that could not be asked for.
pub(crate) fn is_plausible_session_id(id: &str) -> bool {
    (1..=MAX_SESSION_ID_BYTES).contains(&id.len())
        && id
            .bytes()
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// A remote host as the ids of its sessions name it: the first
/// [`HOST_TOKEN_LEN`] lowercase hex digits of the SHA-256 of its stable
/// identity.
///
/// A hash and not a counter, so the same host has the same token after a
/// restart and a list a device kept from before cannot resume on another host;
/// and a hash and not the identity itself, so a user name or port is not
/// spelled out on the wire -- though a short unsalted hash of them can still be
/// confirmed by guessing.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct HostToken(String);

impl HostToken {
    pub(crate) fn of(identity: &RemoteConnectionIdentity) -> Self {
        let digest = Sha256::digest(identity.stable_key().as_bytes());
        let mut token = String::with_capacity(HOST_TOKEN_LEN);
        for byte in digest.iter().take(HOST_TOKEN_LEN / 2) {
            write!(token, "{byte:02x}").log_err();
        }
        Self(token)
    }

    fn parse(text: &str) -> Option<Self> {
        (text.len() == HOST_TOKEN_LEN
            && text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')))
        .then(|| Self(text.to_string()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// The id a device knows a session, or a folder, by: the host's own for one on
/// this machine, and `{token}:{id}` for one on a remote host.
pub(crate) fn wire_id(host: Option<&HostToken>, id: &str) -> String {
    match host {
        Some(host) => format!("{}:{id}", host.as_str()),
        None => id.to_string(),
    }
}

/// The host and the session a device's id names, if it could name one. A
/// plausible session id never holds a `:`, so the first one is where the
/// token ends.
pub(crate) fn parse_wire_id(wire: &str) -> Option<(Option<HostToken>, &str)> {
    let (host, id) = match wire.split_once(':') {
        Some((token, id)) => (Some(HostToken::parse(token)?), id),
        None => (None, wire),
    };
    is_plausible_session_id(id).then_some((host, id))
}

/// The id a device knows the session a tab of `workspace` runs as `id` by,
/// so a tab on a remote host is matched to that host's entry and never to one
/// on this machine with the same id.
pub(crate) fn tab_session_id(workspace: &WeakEntity<Workspace>, id: &str, cx: &App) -> String {
    let host = workspace.upgrade().and_then(|workspace| {
        match project_session_source(workspace.read(cx).project(), cx)? {
            ProjectSessionSource::Remote { identity, .. } => Some(HostToken::of(&identity)),
            ProjectSessionSource::Here => None,
        }
    });
    wire_id(host.as_ref(), id)
}

/// `text` cut to at most `max_chars` characters, the last of them `…` when
/// anything was cut. Never inside a character.
pub(crate) fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars()
        .take(max_chars.saturating_sub(1))
        .chain(std::iter::once('…'))
        .collect()
}

/// The name an agent goes by on the wire, which is not the name it is shown
/// under. The one place it is spelled: both a session entry and an agent
/// summary read it from here.
pub(crate) fn wire_agent(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => "claude",
        AgentKind::Codex => "codex",
        AgentKind::Copilot => "copilot",
        AgentKind::OpenCode => "opencode",
    }
}

/// The remote host a folder is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RootHost {
    pub token: HostToken,
    /// What the host is shown as, already clipped.
    pub label: String,
}

/// A folder open in a window, as far as deciding which sessions belong to it.
#[derive(Clone)]
pub(crate) struct RootScope {
    pub worktree_id: WorktreeId,
    pub path: PathBuf,
    /// What the folder is called, and what a session's `folder` starts with.
    pub name: String,
    /// What the folder's own settings keep out of sight. `None` keeps nothing
    /// out.
    pub hidden: Option<Arc<WorktreeSettings>>,
    /// `None` for a folder on this machine.
    pub host: Option<RootHost>,
    /// How the folder's host spells its paths.
    pub path_style: PathStyle,
}

impl RootScope {
    fn host_token(&self) -> Option<&HostToken> {
        self.host.as_ref().map(|host| &host.token)
    }

    /// Whether `cwd` is at or under a path this folder's settings keep private
    /// or leave out of the scan -- the same two checks the file browser makes of
    /// what it shows -- so a session that ran there is not one the folder has.
    pub(crate) fn hides(&self, cwd: &Path) -> bool {
        let Some(settings) = &self.hidden else {
            return false;
        };
        let Ok(inside) = cwd.strip_prefix(&self.path) else {
            return false;
        };
        // A path that is not a plain relative one cannot be judged, so it is
        // not shown.
        let Ok(inside) = RelPath::new(inside, self.path_style) else {
            return true;
        };
        settings.is_path_private(&inside) || settings.is_path_excluded(&inside)
    }
}

/// An open folder and where a session in it can be opened.
pub(crate) struct OpenRoot {
    pub scope: RootScope,
    pub workspace: WeakEntity<Workspace>,
    pub window: WindowHandle<MultiWorkspace>,
}

/// The visible folders of every project open in a window whose sessions this
/// machine can reach: its own, and those of each WSL, SSH and dev container
/// host. A collaboration guest's are on someone else's machine and are left
/// out.
///
/// A folder on a remote host open in two windows is kept once, for the window
/// and workspace with the lowest ids, so which one a session opens in does not
/// depend on which was asked first. Each window holds the folder under a
/// worktree id of its own, so the folder is known by its host and path.
pub(crate) fn open_roots(cx: &App) -> Vec<OpenRoot> {
    let mut roots: Vec<OpenRoot> = Vec::new();
    let mut remote: Vec<((u64, u64), OpenRoot)> = Vec::new();
    for handle in cx.windows() {
        let Some(window) = handle.downcast::<MultiWorkspace>() else {
            continue;
        };
        let Some(multi_workspace) = window.read(cx).log_err() else {
            continue;
        };
        for workspace in multi_workspace.workspaces() {
            let project_entity = workspace.read(cx).project().clone();
            let Some(source) = project_session_source(&project_entity, cx) else {
                continue;
            };
            let (host, path_style) = match source {
                ProjectSessionSource::Here => (None, PathStyle::local()),
                ProjectSessionSource::Remote {
                    identity,
                    label,
                    path_style,
                } => (
                    Some(RootHost {
                        token: HostToken::of(&identity),
                        label: clip(&label, MAX_HOST_LABEL_CHARS),
                    }),
                    path_style,
                ),
            };
            let project = project_entity.read(cx);
            for worktree in project.visible_worktrees(cx) {
                let worktree = worktree.read(cx);
                let worktree_id = worktree.id();
                if host.is_none()
                    && roots
                        .iter()
                        .any(|known| known.scope.worktree_id == worktree_id)
                {
                    continue;
                }
                let hidden = match (&host, worktree.as_local()) {
                    (_, Some(local)) => Arc::new(local.settings()),
                    // A remote folder's settings are the ones the host sent for
                    // it, read where the worktree itself reads them.
                    (Some(_), None) => Arc::new(
                        WorktreeSettings::get(
                            Some(SettingsLocation {
                                worktree_id,
                                path: RelPath::empty(),
                            }),
                            cx,
                        )
                        .clone(),
                    ),
                    // A folder of a project on this machine that is not on
                    // this machine's disk has private paths nobody knows, so
                    // it holds no sessions.
                    (None, None) => continue,
                };
                // Never the absolute path: a root with no name of its own
                // would otherwise put one in every `folder` it owns.
                let name = match worktree.root_name_str() {
                    "" => "root".to_string(),
                    name => name.to_string(),
                };
                let root = OpenRoot {
                    scope: RootScope {
                        worktree_id,
                        path: worktree.abs_path().to_path_buf(),
                        name,
                        hidden: Some(hidden),
                        host: host.clone(),
                        path_style,
                    },
                    workspace: workspace.downgrade(),
                    window,
                };
                if host.is_some() {
                    let order = (window.window_id().as_u64(), workspace.entity_id().as_u64());
                    remote.push((order, root));
                } else {
                    roots.push(root);
                }
            }
        }
    }
    remote.sort_by_key(|(order, _)| *order);
    for (_, root) in remote {
        let known = roots.iter().any(|known| {
            known.scope.host_token() == root.scope.host_token()
                && known.scope.path == root.scope.path
        });
        if !known {
            roots.push(root);
        }
    }
    roots
}

/// The index in `roots` of the folder on `host` that `cwd` belongs to: the
/// deepest one that holds it, by whole path components, so `/a/zode` does not
/// hold `/a/zode-kit`. A folder open twice goes to the one with the lowest id,
/// so the answer does not depend on which window was asked first.
///
/// Only folders on the same host are candidates: `/home/me/repo` in a WSL
/// distro and on this machine are two directories.
///
/// A directory that climbs out with `..` belongs to no folder, however it
/// starts: the path is read from a store this editor does not own, and a
/// textual prefix is no proof of where it ends up.
pub(crate) fn deepest_root_index(
    roots: &[RootScope],
    host: Option<&HostToken>,
    cwd: &Path,
) -> Option<usize> {
    if cwd
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    roots
        .iter()
        .enumerate()
        .filter(|(_, root)| root.host_token() == host && cwd.starts_with(&root.path))
        .max_by_key(|(_, root)| (root.path.components().count(), Reverse(root.worktree_id)))
        .map(|(index, _)| index)
}

fn unix_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |elapsed| {
        u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
    })
}

/// What a session is called when it has no name of its own: the start of what
/// was said in it.
fn display_title(session: &SessionSummary) -> &str {
    let title = session.title.trim();
    if title.is_empty() {
        session.preview.trim()
    } else {
        title
    }
}

/// The folder as a device reads it: the root's name, then the path inside it.
fn folder_label(root: &RootScope, cwd: &Path) -> String {
    let mut label = root.name.clone();
    let Ok(inside) = cwd.strip_prefix(&root.path) else {
        return label;
    };
    for component in inside.components() {
        label.push('/');
        label.push_str(&component.as_os_str().to_string_lossy());
    }
    label
}

struct Listed<'a> {
    session: &'a SessionSummary,
    root: &'a RootScope,
    updated_at: u64,
    /// The id the device knows it by.
    id: String,
}

impl Listed<'_> {
    fn is_at_or_before(&self, cursor: &SessionCursor) -> bool {
        (Reverse(self.updated_at), self.id.as_str())
            <= (Reverse(cursor.updated_at), cursor.id.as_str())
    }

    fn cursor(&self) -> SessionCursor {
        SessionCursor {
            updated_at: self.updated_at,
            id: self.id.clone(),
        }
    }
}

/// The sessions one host holds.
pub(crate) struct HostSessions<'a> {
    /// `None` for this machine.
    pub host: Option<&'a HostToken>,
    pub sessions: &'a [SessionSummary],
}

/// [`page_across`] over this machine's sessions alone.
#[cfg(test)]
pub(crate) fn page(
    sessions: &[SessionSummary],
    roots: &[RootScope],
    before: Option<&SessionCursor>,
    limit: usize,
    open_agent: impl Fn(&str) -> Option<String>,
) -> (Vec<SessionEntry>, Option<SessionCursor>) {
    page_across(
        &[HostSessions {
            host: None,
            sessions,
        }],
        roots,
        before,
        limit,
        open_agent,
    )
}

/// One page of the sessions that ran inside `roots`, from every host in
/// `hosts`, newest first, and where the next one starts if there is one.
///
/// `before` is exclusive. Order is `(updated_at descending, id ascending)` at
/// millisecond resolution -- the resolution the cursor has -- by the id the
/// device knows, so two sessions in the same millisecond keep one order from
/// page to page, across hosts as much as on one.
/// `open_agent` is asked only about the sessions on the page, by that id.
pub(crate) fn page_across(
    hosts: &[HostSessions],
    roots: &[RootScope],
    before: Option<&SessionCursor>,
    limit: usize,
    open_agent: impl Fn(&str) -> Option<String>,
) -> (Vec<SessionEntry>, Option<SessionCursor>) {
    let mut listed: Vec<Listed> = hosts
        .iter()
        .flat_map(|host| {
            host.sessions
                .iter()
                .map(move |session| (host.host, session))
        })
        .filter(|(_, session)| is_plausible_session_id(&session.id))
        .filter_map(|(host, session)| {
            let root = roots.get(deepest_root_index(roots, host, &session.cwd)?)?;
            if root.hides(&session.cwd) {
                return None;
            }
            Some(Listed {
                session,
                root,
                updated_at: unix_millis(session.updated_at),
                id: wire_id(host, &session.id),
            })
        })
        .collect();
    listed.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    if let Some(cursor) = before {
        let seen = listed.partition_point(|candidate| candidate.is_at_or_before(cursor));
        listed.drain(..seen);
    }
    let more_follow = listed.len() > limit;
    listed.truncate(limit);
    let next = listed.last().filter(|_| more_follow).map(Listed::cursor);
    let entries = listed
        .into_iter()
        .map(|listed| {
            let session = listed.session;
            let preview = session.preview.trim();
            let host = listed.root.host.as_ref();
            SessionEntry {
                agent: wire_agent(session.agent).to_string(),
                title: clip(display_title(session), MAX_TEXT_CHARS),
                preview: (!preview.is_empty()).then(|| clip(preview, MAX_TEXT_CHARS)),
                worktree_id: wire_id(
                    host.map(|host| &host.token),
                    &listed.root.worktree_id.to_proto().to_string(),
                ),
                folder: clip(&folder_label(listed.root, &session.cwd), MAX_TEXT_CHARS),
                branch: session
                    .branch
                    .as_deref()
                    .map(|branch| clip(branch, MAX_TEXT_CHARS)),
                updated_at: listed.updated_at,
                can_fork: provider_for(session.agent)
                    .resume_command(session, Fork::New)
                    .is_some(),
                open_agent_id: open_agent(&listed.id),
                host: host.map(|host| host.label.clone()),
                id: listed.id,
            }
        })
        .collect();
    (entries, next)
}

/// What a reply says of a remote host whose store holds `generation` sweeps,
/// the last of which could not ask the host when `failed`: nothing when its
/// sessions are in the list.
pub(crate) fn host_notice(label: &str, generation: u64, failed: bool) -> Option<UnavailableHost> {
    let reason = if generation == 0 {
        host_status::READING
    } else if failed {
        host_status::UNAVAILABLE
    } else {
        return None;
    };
    Some(UnavailableHost {
        host: label.to_string(),
        reason: reason.to_string(),
    })
}

/// A page as the messages that carry it: as many as it takes to keep each under
/// one frame, every one but the last marked `more`, and the cursor and the
/// hosts that could not be listed on the last alone.
pub(crate) fn session_replies(
    request_id: u32,
    entries: Vec<SessionEntry>,
    next: Option<SessionCursor>,
    unavailable_hosts: Vec<UnavailableHost>,
) -> Vec<Control> {
    let mut unavailable_hosts = unavailable_hosts;
    unavailable_hosts.truncate(MAX_UNAVAILABLE_HOSTS);
    let notices_weight = if unavailable_hosts.is_empty() {
        0
    } else {
        serde_json::to_vec(&unavailable_hosts)
            .log_err()
            .map_or(0, |json| json.len())
    };
    // A page holds at most `MAX_PAGE_LIMIT` entries and an entry that cannot
    // be sent is left out on its own, so the page needs no weight limit.
    let (batches, _) = file_browse::batch_by_json_size(
        entries,
        |entry| serde_json::to_vec(entry).log_err().map(|json| json.len()),
        REPLY_OVERHEAD + notices_weight,
        usize::MAX,
    );
    let last = batches.len() - 1;
    let mut next = next;
    let mut unavailable_hosts = Some(unavailable_hosts);
    batches
        .into_iter()
        .enumerate()
        .map(|(index, sessions)| {
            let is_last = index == last;
            Control::SessionsListReply {
                request_id,
                sessions,
                more: !is_last,
                next: if is_last { next.take() } else { None },
                unavailable_hosts: if is_last {
                    unavailable_hosts.take().unwrap_or_default()
                } else {
                    Vec::new()
                },
            }
        })
        .collect()
}

/// What this screen says when a device starts a session.
pub(crate) fn resume_toast(device_name: &str, fork: bool, title: &str) -> String {
    let device_name = match device_name.trim() {
        "" => "A device".to_string(),
        name => clip(name, MAX_TOAST_DEVICE_CHARS),
    };
    let title = match title.trim() {
        "" => "Untitled".to_string(),
        title => clip(title, MAX_TOAST_TITLE_CHARS),
    };
    let did = if fork { "forked" } else { "resumed" };
    format!("{device_name} {did} “{title}”")
}

/// Whether `controls`, the answer to a resume, say something was started, and
/// not that the session was already running in a tab.
pub(crate) fn opened_a_tab(controls: &[Control]) -> bool {
    controls.iter().any(|control| {
        matches!(
            control,
            Control::SessionResumed {
                open_agent_id: None,
                ..
            }
        )
    })
}

/// Identifies the notices a device starting a session puts on screen. Each one
/// has a number of its own: sharing an id would let a second replace the first
/// before anyone had read it. They go away by themselves, and a device may only
/// start a session every few seconds, so they do not pile up.
pub(crate) struct ResumedToast;

static NEXT_TOAST_NUMBER: AtomicU64 = AtomicU64::new(0);

fn not_found() -> BrowseError {
    BrowseError::new(error_code::NOT_FOUND, "there is no such session")
}

/// When the last read of the sessions from disk that a device asked for was
/// done. One for the whole app: the disk is shared by every device.
#[derive(Default)]
struct FreshReads {
    landed_at: Option<Instant>,
    /// When a device last had each remote host's store read again, by store.
    /// Only the stores of hosts with a window open are kept.
    remote_asked_at: HashMap<EntityId, Instant>,
}

impl Global for FreshReads {}

/// Whether a read from disk may be started now: always for the first one, for
/// which there is nothing to answer from, and otherwise not until
/// [`MIN_FRESH_READ_INTERVAL`] after the last.
fn may_read_from_disk(generation: u64, last_landed_at: Option<Instant>, now: Instant) -> bool {
    generation == 0
        || last_landed_at
            .is_none_or(|landed| now.saturating_duration_since(landed) >= MIN_FRESH_READ_INTERVAL)
}

/// The sessions index. If `refresh`, and a read from disk is allowed, it is one
/// that began after this was called; otherwise the index as it is.
///
/// Asking for a read is cheap: the store coalesces them, so any number of
/// devices asking at once cost one more sweep. A sweep that is already running
/// may have read the disk before this was asked, so it is not the answer: the
/// store is asked for another to follow it, and what this waits for is the
/// store being idle again, which only happens once that one has landed.
async fn read_index(refresh: bool, cx: &mut AsyncApp) -> Result<Arc<SessionIndex>, BrowseError> {
    let (notify, mut landed) = futures::channel::mpsc::unbounded::<()>();
    let (store, reading, _watch) = cx.update(|cx| {
        let store = SessionStore::global(cx);
        // Set up before the refresh is asked for, so a sweep that lands the
        // moment it is cannot be missed.
        let watch = cx.observe(&store, move |_, _| {
            notify.unbounded_send(()).log_err();
        });
        let now = cx.background_executor().now();
        let last_landed_at = cx.default_global::<FreshReads>().landed_at;
        let generation = store.read(cx).generation();
        let reading =
            (refresh || generation == 0) && may_read_from_disk(generation, last_landed_at, now);
        if reading {
            store.update(cx, |store, cx| store.refresh(cx));
        }
        (store, reading, watch)
    });
    while reading && cx.update(|cx| store.read(cx).is_scanning()) {
        if landed.next().await.is_none() {
            return Err(BrowseError::internal("the session list could not be read"));
        }
    }
    Ok(cx.update(|cx| {
        if reading {
            let now = cx.background_executor().now();
            cx.default_global::<FreshReads>().landed_at = Some(now);
        }
        store.read(cx).index().clone()
    }))
}

/// A remote host with a window open, and the store its sessions are kept in.
struct RemoteSource {
    token: HostToken,
    label: String,
    store: Entity<SessionStore>,
}

/// The store of every remote host `roots` are on, once each, with every open
/// project on it registered, so a sweep can ask through any of them.
fn remote_sources(roots: &[OpenRoot], cx: &mut App) -> Vec<RemoteSource> {
    let mut sources: Vec<RemoteSource> = Vec::new();
    let mut registered: Vec<EntityId> = Vec::new();
    for root in roots {
        let Some(host) = &root.scope.host else {
            continue;
        };
        let Some(workspace) = root.workspace.upgrade() else {
            continue;
        };
        let project = workspace.read(cx).project().clone();
        if registered.contains(&project.entity_id()) {
            continue;
        }
        registered.push(project.entity_id());
        let store = SessionStore::for_project(&project, cx);
        if !sources.iter().any(|source| source.token == host.token) {
            sources.push(RemoteSource {
                token: host.token.clone(),
                label: host.label.clone(),
                store,
            });
        }
    }
    sources
}

/// The store a session on the host `token` names would be in, if a window on
/// that host is open. Never creates one: a host whose last window has closed
/// has nothing a device may open, whatever its store still holds.
fn open_remote_store(
    roots: &[OpenRoot],
    token: &HostToken,
    cx: &App,
) -> Option<Entity<SessionStore>> {
    roots
        .iter()
        .filter(|root| root.scope.host_token() == Some(token))
        .find_map(|root| {
            let workspace = root.workspace.upgrade()?;
            SessionStore::try_for_project(workspace.read(cx).project(), cx)
        })
}

/// What each remote host's store holds now, and what a reply says of those
/// whose sessions are missing. A first page has each store read again, at most
/// once every [`MIN_FRESH_READ_INTERVAL`], and never waits for it: a host that
/// is slow or has stopped answering must not hold up this machine's list.
fn remote_snapshots(
    sources: &[RemoteSource],
    refresh: bool,
    cx: &mut App,
) -> (Vec<(HostToken, Arc<SessionIndex>)>, Vec<UnavailableHost>) {
    let now = cx.background_executor().now();
    let mut asked_at = std::mem::take(&mut cx.default_global::<FreshReads>().remote_asked_at);
    asked_at.retain(|store, _| {
        sources
            .iter()
            .any(|source| source.store.entity_id() == *store)
    });
    let mut indexes = Vec::with_capacity(sources.len());
    let mut notices = Vec::new();
    for source in sources {
        let store_id = source.store.entity_id();
        let (generation, failed, index) = {
            let store = source.store.read(cx);
            (
                store.generation(),
                store.error().is_some(),
                store.index().clone(),
            )
        };
        let last_asked_at = asked_at.get(&store_id).copied();
        if (refresh || generation == 0) && may_read_from_disk(generation, last_asked_at, now) {
            source.store.update(cx, |store, cx| store.refresh(cx));
            asked_at.insert(store_id, now);
        }
        notices.extend(host_notice(&source.label, generation, failed));
        indexes.push((source.token.clone(), index));
    }
    cx.default_global::<FreshReads>().remote_asked_at = asked_at;
    (indexes, notices)
}

/// Answers a `sessions_list`. A first page is read fresh from disk, a later
/// one from what that read found, so a device paging through sees one
/// consistent list. A remote host's sessions are what its store holds when
/// the list is asked for, and the hosts it could not include are named on the
/// first page.
pub(crate) async fn list(
    request_id: u32,
    before: Option<SessionCursor>,
    limit: usize,
    agents: Entity<AgentMirror>,
    cx: &mut AsyncApp,
) -> Result<Vec<Control>, BrowseError> {
    let first_page = before.is_none();
    let index = read_index(first_page, cx).await?;
    let (scopes, remote, notices) = cx.update(|cx| {
        let roots = open_roots(cx);
        let sources = remote_sources(&roots, cx);
        let (remote, notices) = remote_snapshots(&sources, first_page, cx);
        let scopes: Vec<RootScope> = roots.into_iter().map(|root| root.scope).collect();
        (scopes, remote, notices)
    });
    // Judging every session on disk, thousands of them for a heavy user, is
    // not work for the thread that draws the windows.
    let (mut entries, next) = cx
        .background_executor()
        .spawn(async move {
            let hosts: Vec<HostSessions> = std::iter::once(HostSessions {
                host: None,
                sessions: index.sessions(),
            })
            .chain(remote.iter().map(|(token, index)| HostSessions {
                host: Some(token),
                sessions: index.sessions(),
            }))
            .collect();
            page_across(&hosts, &scopes, before.as_ref(), limit, |_| None)
        })
        .await;
    cx.update(|cx| {
        let mirror = agents.read(cx);
        for entry in &mut entries {
            entry.open_agent_id = mirror.running_agent_for_session(&entry.id, cx);
        }
    });
    let notices = if first_page { notices } else { Vec::new() };
    Ok(session_replies(request_id, entries, next, notices))
}

/// Answers a `session_resume`: opens a tab on the session, or on a fork of it,
/// in the window whose folder holds the directory it ran in, on the host the
/// id names.
///
/// Run from a task of its own and never from inside an update of the host:
/// what it calls reaches workspaces, and a tab it opens is announced to the
/// host.
pub(crate) async fn resume(
    request_id: u32,
    session_id: String,
    fork: bool,
    device_name: String,
    agents: Entity<AgentMirror>,
    cx: &mut AsyncApp,
) -> Result<Vec<Control>, BrowseError> {
    let (host, raw_id) = parse_wire_id(&session_id).ok_or_else(not_found)?;
    let session = match &host {
        None => read_index(false, cx).await?.find(raw_id).cloned(),
        // Never reconnects or creates anything for a device: with no window
        // open on the host there is nothing to open the session in.
        Some(token) => cx.update(|cx| {
            let roots = open_roots(cx);
            let store = open_remote_store(&roots, token, cx)?;
            store.read(cx).index().find(raw_id).cloned()
        }),
    }
    .ok_or_else(not_found)?;

    let roots = cx.update(|cx| open_roots(cx));
    let scopes: Vec<RootScope> = roots.iter().map(|root| root.scope.clone()).collect();
    let target = deepest_root_index(&scopes, host.as_ref(), &session.cwd)
        .and_then(|index| roots.get(index))
        .filter(|root| !root.scope.hides(&session.cwd))
        .ok_or_else(not_found)?;

    if !fork {
        let running = cx.update(|cx| agents.read(cx).running_agent_for_session(&session_id, cx));
        if let Some(open_agent_id) = running {
            return Ok(vec![Control::SessionResumed {
                request_id,
                open_agent_id: Some(open_agent_id),
            }]);
        }
    }

    // Asked of the host the target workspace's project is on, the one
    // `resume_session` asks again: a directory is only there on that host.
    let refusal = cx.update(|cx| {
        let workspace = target.workspace.upgrade()?;
        let host = workspace
            .read(cx)
            .project()
            .read(cx)
            .agent_session_host(cx)?;
        Some(resume_refusal_on(
            &host,
            &session,
            if fork { Fork::New } else { Fork::Continue },
            cx,
        ))
    });
    let Some(refusal) = refusal else {
        return Err(not_found());
    };
    match refusal.await {
        Ok(Some(ResumeRefusal::Unsupported)) => {
            return Err(BrowseError::new(
                UNSUPPORTED,
                "this agent cannot do that with a session",
            ));
        }
        Ok(Some(ResumeRefusal::CwdGone)) => return Err(not_found()),
        Ok(None) => {}
        // A host that could not be asked -- its connection dropped, say -- is
        // not one a tab may be opened on, and what went wrong there is not the
        // device's to read.
        Err(error) => {
            log::warn!("not resuming a session for a device: {error:#}");
            return Err(not_found());
        }
    }

    let message = resume_toast(&device_name, fork, display_title(&session));
    let toast_number = NEXT_TOAST_NUMBER.fetch_add(1, Ordering::Relaxed);
    // Not `WindowHandle::update`: that holds the window's root view for the
    // whole call, and the workspace code reaches back for it.
    let opened = AnyWindowHandle::from(target.window).update(cx, |root, window, cx| {
        let Some(workspace) = target.workspace.upgrade() else {
            return false;
        };
        resume_session(
            &workspace,
            &session,
            if fork { Fork::New } else { Fork::Continue },
            window,
            cx,
        );
        // The tab may open in a workspace of the window that is not the one in
        // front, where nobody is looking, so the one in front is told too.
        let in_front = root
            .downcast::<MultiWorkspace>()
            .ok()
            .map(|multi_workspace| multi_workspace.read(cx).workspace().clone())
            .filter(|in_front| *in_front != workspace);
        for shown_on in std::iter::once(workspace).chain(in_front) {
            shown_on.update(cx, |workspace, cx| {
                let id = NotificationId::composite::<ResumedToast>(("resumed", toast_number));
                workspace.show_toast(Toast::new(id, message.clone()).autohide(), cx);
            });
        }
        true
    });
    if !opened.log_err().unwrap_or(false) {
        return Err(not_found());
    }
    Ok(vec![Control::SessionResumed {
        request_id,
        open_agent_id: None,
    }])
}
