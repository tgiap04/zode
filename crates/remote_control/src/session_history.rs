//! What a paired device sees of the agent conversations on this Zode, and what
//! it can start from one.
//!
//! The device names a session by its id and nothing else. Which agent it is,
//! where it ran and how it is resumed all come from this host's own session
//! index, and a session only counts if it ran inside a folder that is open in a
//! window right now -- the same scope the history panel keeps, so a device
//! cannot see or start anything the person at this screen could not.
//!
//! Everything that decides something is a plain function over plain values;
//! the two async entry points, [`list`] and [`resume`], only gather what those
//! functions need from the running app.

use std::{
    cmp::Reverse,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use agent_sessions::{AgentKind, Fork, SessionIndex, SessionSummary, provider_for};
use agent_ui::{ResumeRefusal, SessionStore, resume_refusal, resume_session};
use futures::StreamExt as _;
use gpui::{AnyWindowHandle, App, AsyncApp, Entity, WeakEntity, WindowHandle};
use remote_relay_protocol::{Control, SessionCursor, SessionEntry, error_code};
use settings::WorktreeId;
use util::{ResultExt as _, paths::PathStyle, rel_path::RelPath};
use workspace::{MultiWorkspace, Toast, Workspace, notifications::NotificationId};
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

/// Room kept in one `sessions_list_reply` for everything but its entries: the
/// tag, the request id, the flags, and a cursor whose id is as long as an id
/// may be.
const REPLY_OVERHEAD: usize = 512;

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
}

impl RootScope {
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
        let Ok(inside) = RelPath::new(inside, PathStyle::local()) else {
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

/// The visible folders of every local project open in a window.
///
/// Remote projects are left out: a session's directory is a path on this
/// machine, and a folder that only exists on another one holds none.
pub(crate) fn open_roots(cx: &App) -> Vec<OpenRoot> {
    let mut roots: Vec<OpenRoot> = Vec::new();
    for handle in cx.windows() {
        let Some(window) = handle.downcast::<MultiWorkspace>() else {
            continue;
        };
        let Some(multi_workspace) = window.read(cx).log_err() else {
            continue;
        };
        for workspace in multi_workspace.workspaces() {
            let project = workspace.read(cx).project().read(cx);
            if !project.is_local() {
                continue;
            }
            for worktree in project.visible_worktrees(cx) {
                let worktree = worktree.read(cx);
                let worktree_id = worktree.id();
                if roots
                    .iter()
                    .any(|known| known.scope.worktree_id == worktree_id)
                {
                    continue;
                }
                // Every worktree of a local project is a local one. A folder
                // whose settings cannot be read is one whose private paths are
                // unknown, so it holds no sessions.
                let Some(hidden) = worktree.as_local().map(|local| Arc::new(local.settings()))
                else {
                    continue;
                };
                // Never the absolute path: a root with no name of its own
                // would otherwise put one in every `folder` it owns.
                let name = match worktree.root_name_str() {
                    "" => "root".to_string(),
                    name => name.to_string(),
                };
                roots.push(OpenRoot {
                    scope: RootScope {
                        worktree_id,
                        path: worktree.abs_path().to_path_buf(),
                        name,
                        hidden: Some(hidden),
                    },
                    workspace: workspace.downgrade(),
                    window,
                });
            }
        }
    }
    roots
}

/// The index in `roots` of the folder `cwd` belongs to: the deepest one that
/// holds it, by whole path components, so `/a/zode` does not hold
/// `/a/zode-kit`. A folder open twice goes to the one with the lowest id, so
/// the answer does not depend on which window was asked first.
///
/// A directory that climbs out with `..` belongs to no folder, however it
/// starts: the path is read from a store this editor does not own, and a
/// textual prefix is no proof of where it ends up.
pub(crate) fn deepest_root_index(roots: &[RootScope], cwd: &Path) -> Option<usize> {
    if cwd
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    roots
        .iter()
        .enumerate()
        .filter(|(_, root)| cwd.starts_with(&root.path))
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
}

impl Listed<'_> {
    fn is_at_or_before(&self, cursor: &SessionCursor) -> bool {
        (Reverse(self.updated_at), self.session.id.as_ref())
            <= (Reverse(cursor.updated_at), cursor.id.as_str())
    }

    fn cursor(&self) -> SessionCursor {
        SessionCursor {
            updated_at: self.updated_at,
            id: self.session.id.to_string(),
        }
    }
}

/// One page of the sessions that ran inside `roots`, newest first, and where
/// the next one starts if there is one.
///
/// `before` is exclusive. Order is `(updated_at descending, id ascending)` at
/// millisecond resolution -- the resolution the cursor has -- so two sessions
/// in the same millisecond keep one order from page to page.
/// `open_agent` is asked only about the sessions on the page.
pub(crate) fn page(
    sessions: &[SessionSummary],
    roots: &[RootScope],
    before: Option<&SessionCursor>,
    limit: usize,
    open_agent: impl Fn(&str) -> Option<String>,
) -> (Vec<SessionEntry>, Option<SessionCursor>) {
    let mut listed: Vec<Listed> = sessions
        .iter()
        .filter(|session| is_plausible_session_id(&session.id))
        .filter_map(|session| {
            let root = roots.get(deepest_root_index(roots, &session.cwd)?)?;
            if root.hides(&session.cwd) {
                return None;
            }
            Some(Listed {
                session,
                root,
                updated_at: unix_millis(session.updated_at),
            })
        })
        .collect();
    listed.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.session.id.cmp(&right.session.id))
    });
    if let Some(cursor) = before {
        let seen = listed.partition_point(|candidate| candidate.is_at_or_before(cursor));
        listed.drain(..seen);
    }
    let more_follow = listed.len() > limit;
    listed.truncate(limit);
    let next = listed.last().filter(|_| more_follow).map(Listed::cursor);
    let entries = listed
        .iter()
        .map(|listed| {
            let session = listed.session;
            let preview = session.preview.trim();
            SessionEntry {
                id: session.id.to_string(),
                agent: wire_agent(session.agent).to_string(),
                title: clip(display_title(session), MAX_TEXT_CHARS),
                preview: (!preview.is_empty()).then(|| clip(preview, MAX_TEXT_CHARS)),
                worktree_id: listed.root.worktree_id.to_proto().to_string(),
                folder: clip(&folder_label(listed.root, &session.cwd), MAX_TEXT_CHARS),
                branch: session
                    .branch
                    .as_deref()
                    .map(|branch| clip(branch, MAX_TEXT_CHARS)),
                updated_at: listed.updated_at,
                can_fork: provider_for(session.agent)
                    .resume_command(session, Fork::New)
                    .is_some(),
                open_agent_id: open_agent(&session.id),
            }
        })
        .collect();
    (entries, next)
}

/// A page as the messages that carry it: as many as it takes to keep each under
/// one frame, every one but the last marked `more`, and the cursor on the last
/// alone.
pub(crate) fn session_replies(
    request_id: u32,
    entries: Vec<SessionEntry>,
    next: Option<SessionCursor>,
) -> Vec<Control> {
    // A page holds at most `MAX_PAGE_LIMIT` entries and an entry that cannot
    // be sent is left out on its own, so the page needs no weight limit.
    let (batches, _) = file_browse::batch_by_json_size(
        entries,
        |entry| serde_json::to_vec(entry).log_err().map(|json| json.len()),
        REPLY_OVERHEAD,
        usize::MAX,
    );
    let last = batches.len() - 1;
    let mut next = next;
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

/// The sessions index, once it has been read from disk since this was called
/// if `refresh`, and in any case once it has been read at all.
///
/// Asking for a refresh is cheap: the store coalesces them, so any number of
/// devices asking at once cost one more sweep. What this waits for is the
/// generation moving on, which is the store's way of saying a sweep landed.
async fn read_index(refresh: bool, cx: &mut AsyncApp) -> Result<Arc<SessionIndex>, BrowseError> {
    let (notify, mut landed) = futures::channel::mpsc::unbounded::<()>();
    let (store, generation_before, _watch) = cx.update(|cx| {
        let store = SessionStore::global(cx);
        // Set up before the refresh is asked for, so a sweep that lands the
        // moment it is cannot be missed.
        let watch = cx.observe(&store, move |_, _| {
            notify.unbounded_send(()).log_err();
        });
        let generation = store.read(cx).generation();
        let sweep = refresh || generation == 0;
        if sweep {
            store.update(cx, |store, cx| store.refresh(cx));
        }
        (store, sweep.then_some(generation), watch)
    });
    loop {
        let (generation, index) = cx.update(|cx| {
            let store = store.read(cx);
            (store.generation(), store.index().clone())
        });
        if generation_before.is_none_or(|before| generation > before) {
            return Ok(index);
        }
        if landed.next().await.is_none() {
            return Err(BrowseError::internal("the session list could not be read"));
        }
    }
}

/// Answers a `sessions_list`. A first page is read fresh from disk, a later
/// one from what that read found, so a device paging through sees one
/// consistent list.
pub(crate) async fn list(
    request_id: u32,
    before: Option<SessionCursor>,
    limit: usize,
    agents: Entity<AgentMirror>,
    cx: &mut AsyncApp,
) -> Result<Vec<Control>, BrowseError> {
    let index = read_index(before.is_none(), cx).await?;
    let (entries, next) = cx.update(|cx| {
        let scopes: Vec<RootScope> = open_roots(cx).into_iter().map(|root| root.scope).collect();
        let mirror = agents.read(cx);
        page(index.sessions(), &scopes, before.as_ref(), limit, |id| {
            mirror.running_agent_for_session(id, cx)
        })
    });
    Ok(session_replies(request_id, entries, next))
}

/// Answers a `session_resume`: opens a tab on the session, or on a fork of it,
/// in the window whose folder holds the directory it ran in.
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
    let index = read_index(false, cx).await?;
    let session = index.find(&session_id).cloned().ok_or_else(not_found)?;

    let roots = cx.update(|cx| open_roots(cx));
    let scopes: Vec<RootScope> = roots.iter().map(|root| root.scope.clone()).collect();
    let target = deepest_root_index(&scopes, &session.cwd)
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

    match resume_refusal(&session, if fork { Fork::New } else { Fork::Continue }) {
        Some(ResumeRefusal::Unsupported) => {
            return Err(BrowseError::new(
                UNSUPPORTED,
                "this agent cannot do that with a session",
            ));
        }
        Some(ResumeRefusal::CwdGone) => return Err(not_found()),
        None => {}
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
