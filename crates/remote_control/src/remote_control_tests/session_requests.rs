//! Session history as a paired browser meets it: the real session, the real
//! encryption, windows over fake disks, and a session store that sweeps
//! providers made here, never the developer's own `~/.claude`.

use std::{
    any::TypeId,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, UNIX_EPOCH},
};

use agent_sessions::{
    AgentCommand, AgentKind, Availability, Deletion, Fork, SessionCounts, SessionProvider,
    SessionSummary,
};
use agent_ui::{AgentView, SessionIntent, SessionOrigin, SessionStore};
use fs::FakeFs;
use gpui::{UpdateGlobal as _, VisualTestContext};
use project::{Project, agent_session_host::AgentSessionHost};
use remote_relay_protocol::{
    Control, SessionCursor, SessionEntry, UnavailableHost, error_code, host_status,
};
use serde_json::json;
use util::path;
use workspace::{
    MultiWorkspace, RemoteConnectionIdentity, Workspace, notifications::NotificationId,
};

use super::*;
use crate::{
    hosting::RESUME_COOLDOWN,
    session_history::{
        HostToken, MIN_FRESH_READ_INTERVAL, ResumedToast, RootScope, open_roots, page,
    },
};

const NEWEST: &str = "0d6c6a52-9a3f-4a51-9f0e-6c1b2a3d4e5f";

/// Lists what it is given and counts how often it is asked.
struct FakeProvider {
    sessions: Vec<SessionSummary>,
    sweeps: Arc<AtomicUsize>,
}

impl SessionProvider for FakeProvider {
    fn agent(&self) -> AgentKind {
        AgentKind::Claude
    }

    fn availability(&self) -> Availability {
        Availability::Ready
    }

    fn list(&self) -> anyhow::Result<Vec<SessionSummary>> {
        self.sweeps.fetch_add(1, Ordering::SeqCst);
        Ok(self.sessions.clone())
    }

    fn find(&self, _id: &str) -> anyhow::Result<Option<SessionSummary>> {
        Ok(None)
    }

    fn new_session_command(&self, _id: &str, _cwd: &Path) -> Option<AgentCommand> {
        None
    }

    fn counts(&self, _session: &SessionSummary) -> anyhow::Result<SessionCounts> {
        Ok(SessionCounts::default())
    }

    fn resume_command(&self, _session: &SessionSummary, _fork: Fork) -> Option<AgentCommand> {
        None
    }

    fn deletion(&self, _session: &SessionSummary) -> Deletion {
        Deletion::Nothing
    }
}

/// Lists the sessions a script says it lists on each read: the n-th read
/// returns what the n-th entry holds, and every one after the last returns the
/// last.
struct ScriptedProvider {
    reads: Vec<Vec<SessionSummary>>,
    sweeps: Arc<AtomicUsize>,
}

impl SessionProvider for ScriptedProvider {
    fn agent(&self) -> AgentKind {
        AgentKind::Claude
    }

    fn availability(&self) -> Availability {
        Availability::Ready
    }

    fn list(&self) -> anyhow::Result<Vec<SessionSummary>> {
        let index = self.sweeps.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .reads
            .get(index)
            .or(self.reads.last())
            .cloned()
            .unwrap_or_default())
    }

    fn find(&self, _id: &str) -> anyhow::Result<Option<SessionSummary>> {
        Ok(None)
    }

    fn new_session_command(&self, _id: &str, _cwd: &Path) -> Option<AgentCommand> {
        None
    }

    fn counts(&self, _session: &SessionSummary) -> anyhow::Result<SessionCounts> {
        Ok(SessionCounts::default())
    }

    fn resume_command(&self, _session: &SessionSummary, _fork: Fork) -> Option<AgentCommand> {
        None
    }

    fn deletion(&self, _session: &SessionSummary) -> Deletion {
        Deletion::Nothing
    }
}

fn session(id: &str, agent: AgentKind, cwd: &Path, secs: u64) -> SessionSummary {
    SessionSummary {
        id: Arc::from(id),
        agent,
        title: format!("title of {id}"),
        preview: "p".repeat(500),
        preview_speaker: None,
        cwd: cwd.to_path_buf(),
        branch: Some("main".to_string()),
        model: None,
        updated_at: UNIX_EPOCH + Duration::from_secs(secs),
        log_path: None,
        log_bytes: 0,
    }
}

/// A folder that exists on the real disk, because whether a session's
/// directory is still there is read from it, and on the fake one the project
/// scans.
struct Folder {
    _directory: tempfile::TempDir,
    root: PathBuf,
}

impl Folder {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("a temporary folder");
        // `dunce`, not `std::fs::canonicalize`: on Windows the standard one
        // returns a `\\?\C:\...` verbatim path, while a worktree's path has that
        // prefix stripped (`SanitizedPath`), so no session under this folder
        // would ever be found inside it.
        let root = dunce::canonicalize(directory.path()).expect("it exists");
        for inside in ["crates/terminal", "crates/ui", "vault/keys", "dist/out"] {
            std::fs::create_dir_all(root.join(inside)).expect("a directory");
        }
        Self {
            _directory: directory,
            root,
        }
    }

    fn name(&self) -> String {
        self.root
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .into_owned()
    }
}

async fn connected(
    cx: &mut TestAppContext,
    with_files: bool,
    sessions_on: bool,
) -> (Rig, Browser, Connected) {
    let rig = rig_with_files(
        cx,
        true,
        AccountStatus::SignedIn(AccountUser {
            id: USER_ID.into(),
            email: "ada@example.com".into(),
            name: None,
            avatar_url: None,
        }),
        with_files,
    )
    .await;
    if sessions_on {
        rig.host
            .update(cx, |host, _| host.set_sessions_available(true));
    }
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let mut connected = open_session(&rig, &browser, cx);
    connected.send(&browser, &hello(1));
    cx.run_until_parked();
    (rig, browser, connected)
}

async fn greeted(cx: &mut TestAppContext) -> (Rig, Browser, Connected) {
    let (rig, browser, mut connected) = connected(cx, false, true).await;
    connected.controls(&browser);
    (rig, browser, connected)
}

fn install_sessions(cx: &mut TestAppContext, sessions: Vec<SessionSummary>) -> Arc<AtomicUsize> {
    let sweeps = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(FakeProvider {
        sessions,
        sweeps: sweeps.clone(),
    });
    cx.update(|cx| SessionStore::install_global_for_test(vec![provider], cx));
    sweeps
}

/// One window holding a workspace for each folder. The first is put in the
/// background and the last is in front, which is where a person who has been
/// switching between projects leaves them.
async fn window<'a>(
    cx: &'a mut TestAppContext,
    folders: &[&Folder],
) -> (Vec<Entity<Workspace>>, &'a mut VisualTestContext) {
    let fs = FakeFs::new(cx.executor());
    for folder in folders {
        fs.insert_tree(
            &folder.root,
            json!({ "crates": { "terminal": {}, "ui": {} } }),
        )
        .await;
    }
    let roots: Vec<&Path> = folders.iter().map(|folder| folder.root.as_path()).collect();
    window_over(cx, fs, &roots).await
}

/// [`window`] over folders that are already on `fs`.
async fn window_over<'a>(
    cx: &'a mut TestAppContext,
    fs: Arc<FakeFs>,
    roots: &[&Path],
) -> (Vec<Entity<Workspace>>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        editor::init(cx);
        project::DisableAiSettings::register(cx);
    });
    let mut projects = Vec::new();
    for root in roots {
        projects.push(Project::test(fs.clone(), [*root], cx).await);
    }
    let mut projects = projects.into_iter();
    let first = projects.next().expect("a folder");
    let (multi, cx) = cx.add_window_view(|window, cx| MultiWorkspace::test_new(first, window, cx));
    let mut workspaces = vec![multi.read_with(cx, |multi, _| multi.workspace().clone())];
    for project in projects {
        multi.update_in(cx, |multi, window, cx| {
            multi.test_enable_background_retention(cx);
            workspaces.push(multi.test_add_workspace(project, window, cx));
        });
    }
    cx.run_until_parked();
    (workspaces, cx)
}

fn worktree_id_of(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> String {
    workspace.read_with(cx, |workspace, cx| {
        workspace
            .project()
            .read(cx)
            .visible_worktrees(cx)
            .next()
            .expect("a worktree")
            .read(cx)
            .id()
            .to_proto()
            .to_string()
    })
}

/// The session each agent tab of `workspace` is on, in tab order.
fn tabs(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<Option<String>> {
    workspace.read_with(cx, |workspace, cx| {
        workspace
            .items_of_type::<AgentView>(cx)
            .map(|tab| tab.read(cx).session_id().map(str::to_string))
            .collect()
    })
}

/// How many notices for a device starting a session `workspace` is showing.
/// Other notices come and go as tabs open, so only these are counted.
fn resume_toasts(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> usize {
    workspace.read_with(cx, |workspace, _| {
        workspace
            .notification_ids()
            .iter()
            .filter(|id| {
                matches!(id, NotificationId::Composite(kind, _) if *kind == TypeId::of::<ResumedToast>())
            })
            .count()
    })
}

fn told_of_a_resume(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> bool {
    resume_toasts(workspace, cx) > 0
}

fn list_request(request_id: u32, before: Option<SessionCursor>, limit: Option<u32>) -> Control {
    Control::SessionsList {
        request_id,
        before,
        limit,
    }
}

fn resume_request(request_id: u32, session_id: &str, fork: bool) -> Control {
    Control::SessionResume {
        request_id,
        session_id: session_id.to_string(),
        fork,
    }
}

fn pages(controls: Vec<Control>) -> Vec<(Vec<SessionEntry>, bool, Option<SessionCursor>)> {
    controls
        .into_iter()
        .filter_map(|control| match control {
            Control::SessionsListReply {
                sessions,
                more,
                next,
                ..
            } => Some((sessions, more, next)),
            _ => None,
        })
        .collect()
}

/// The requests that were refused, by code and request id. Unlike
/// [`only_error`] it tolerates the tab updates a session being opened sends.
fn request_errors(controls: &[Control]) -> Vec<(&str, u32)> {
    controls
        .iter()
        .filter_map(|control| match control {
            Control::Error {
                code,
                request_id: Some(id),
                ..
            } => Some((code.as_str(), *id)),
            _ => None,
        })
        .collect()
}

fn only_error(controls: &[Control]) -> (&str, u32) {
    match controls {
        [
            Control::Error {
                code,
                request_id: Some(request_id),
                ..
            },
        ] => (code.as_str(), *request_id),
        other => panic!("not a single request error: {other:?}"),
    }
}

/// The running app builds its host through `init`, not through the rig, so this
/// is the one test that says `init` switches the history on.
#[gpui::test]
async fn the_running_app_offers_session_history(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let store = SettingsStore::test(cx);
        cx.set_global(store);
        release_channel::init_test(
            "9.9.9".parse().expect("a version"),
            release_channel::ReleaseChannel::Dev,
            cx,
        );
        let account = cx.new(|_| Account::for_test(AccountStatus::SignedOut));
        Account::set_global(account, cx);
        crate::init(cx);
    });
    let host = cx
        .update(|cx| RemoteHost::global(cx))
        .expect("init builds the host");
    assert!(host.read_with(cx, |host, _| host.offers_sessions()));
}

#[gpui::test]
async fn the_sessions_capability_is_advertised_only_when_it_is_switched_on(
    cx: &mut TestAppContext,
) {
    for (with_files, on, expected) in [
        (false, false, vec!["terminal"]),
        (false, true, vec!["terminal", "sessions"]),
        (true, true, vec!["terminal", "files", "sessions"]),
    ] {
        let (_rig, browser, mut connected) = connected(cx, with_files, on).await;
        let controls = connected.controls(&browser);
        assert!(
            matches!(
                &controls[0],
                Control::HelloAck { capabilities, .. } if capabilities == &expected
            ),
            "{with_files} {on}: {controls:?}"
        );
    }
}

#[gpui::test]
async fn a_list_is_scoped_to_the_open_folders_ordered_paged_and_says_what_each_can_do(
    cx: &mut TestAppContext,
) {
    let (rig, browser, mut connected) = greeted(cx).await;
    let folder = Folder::new();
    let outside = Folder::new();
    let sibling = PathBuf::from(format!("{}-kit/web", folder.root.display()));
    let sweeps = install_sessions(
        cx,
        vec![
            session(
                NEWEST,
                AgentKind::Claude,
                &folder.root.join("crates/terminal"),
                300,
            ),
            session("middle", AgentKind::Claude, &folder.root, 200),
            session(
                "oldest",
                AgentKind::Codex,
                &folder.root.join("crates/ui"),
                100,
            ),
            session("elsewhere", AgentKind::Claude, &outside.root, 400),
            session("sibling", AgentKind::Claude, &sibling, 500),
        ],
    );
    let (workspaces, cx) = window(cx, &[&folder]).await;
    let worktree_id = worktree_id_of(&workspaces[0], cx);

    connected.send(&browser, &list_request(1, None, Some(2)));
    cx.run_until_parked();
    let first = pages(connected.controls(&browser));
    let [(entries, false, Some(next))] = &first[..] else {
        panic!("{first:?}");
    };
    let seen: Vec<(&str, &str, &str, bool)> = entries
        .iter()
        .map(|entry| {
            (
                entry.id.as_str(),
                entry.folder.as_str(),
                entry.agent.as_str(),
                entry.can_fork,
            )
        })
        .collect();
    let name = folder.name();
    assert_eq!(
        seen,
        [
            (
                NEWEST,
                format!("{name}/crates/terminal").as_str(),
                "claude",
                true
            ),
            ("middle", name.as_str(), "claude", true),
        ]
    );
    assert!(entries.iter().all(|entry| entry.worktree_id == worktree_id));
    assert_eq!(entries[0].updated_at, 300_000);
    assert_eq!(
        entries[0].title,
        "title of 0d6c6a52-9a3f-4a51-9f0e-6c1b2a3d4e5f"
    );
    assert_eq!(
        entries[0]
            .preview
            .as_deref()
            .map(|text| text.chars().count()),
        Some(200),
        "a preview is clipped"
    );
    assert_eq!(
        *next,
        SessionCursor {
            updated_at: 200_000,
            id: "middle".into()
        }
    );
    assert_eq!(
        sweeps.load(Ordering::SeqCst),
        1,
        "a first page reads the disk"
    );

    connected.send(&browser, &list_request(2, Some(next.clone()), Some(2)));
    cx.run_until_parked();
    let second = pages(connected.controls(&browser));
    let [(entries, false, None)] = &second[..] else {
        panic!("{second:?}");
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "oldest");
    assert_eq!(entries[0].agent, "codex");
    assert!(!entries[0].can_fork, "Codex cannot fork");
    assert_eq!(sweeps.load(Ordering::SeqCst), 1, "a later page does not");

    cx.executor().advance_clock(MIN_FRESH_READ_INTERVAL);
    connected.send(&browser, &list_request(3, None, None));
    cx.run_until_parked();
    let all = pages(connected.controls(&browser));
    assert_eq!(
        all[0].0.len(),
        3,
        "with no limit the whole scope is one page"
    );
    assert_eq!(
        sweeps.load(Ordering::SeqCst),
        2,
        "and a first page reads again"
    );
    for entry in &all[0].0 {
        assert!(!entry.folder.starts_with('/'), "{}", entry.folder);
    }
    assert_eq!(
        rig.host.read_with(cx, |host, _| host
            .file_requests_in_flight(connected.session_id)),
        0
    );
}

#[gpui::test]
async fn a_first_page_asked_again_at_once_is_answered_from_the_read_just_made(
    cx: &mut TestAppContext,
) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let folder = Folder::new();
    let sweeps = install_sessions(cx, vec![session("a", AgentKind::Claude, &folder.root, 1)]);
    let (_workspaces, cx) = window(cx, &[&folder]).await;

    for request_id in 1..=5 {
        connected.send(&browser, &list_request(request_id, None, None));
        cx.run_until_parked();
        let listed = pages(connected.controls(&browser));
        assert_eq!(listed[0].0.len(), 1, "every ask is answered");
    }
    assert_eq!(
        sweeps.load(Ordering::SeqCst),
        1,
        "five asks in a row cost one read of the disk"
    );

    cx.executor()
        .advance_clock(MIN_FRESH_READ_INTERVAL - Duration::from_millis(1));
    connected.send(&browser, &list_request(6, None, None));
    cx.run_until_parked();
    connected.controls(&browser);
    assert_eq!(sweeps.load(Ordering::SeqCst), 1, "not quite long enough");

    cx.executor().advance_clock(Duration::from_millis(1));
    connected.send(&browser, &list_request(7, None, None));
    cx.run_until_parked();
    connected.controls(&browser);
    assert_eq!(
        sweeps.load(Ordering::SeqCst),
        2,
        "and once it has passed a first page reads again"
    );
}

/// A sweep that is running when a list is asked for began before the ask, so
/// what it read may be missing a session made a moment ago. The page must come
/// from a read that began after.
#[gpui::test(iterations = 30)]
async fn a_sweep_already_running_is_not_the_answer_to_a_fresh_list(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let folder = Folder::new();
    let old = session("old", AgentKind::Claude, &folder.root, 100);
    let new = session("new", AgentKind::Claude, &folder.root, 200);
    let sweeps = Arc::new(AtomicUsize::new(0));
    // The first read is the one that fills the store. The second is the sweep
    // that is running when the list is asked for, and read the disk before
    // `new` was made; the third began after.
    let provider = Arc::new(ScriptedProvider {
        reads: vec![vec![old.clone()], vec![old.clone()], vec![old, new]],
        sweeps: sweeps.clone(),
    });
    cx.update(|cx| SessionStore::install_global_for_test(vec![provider], cx));
    let (_workspaces, cx) = window(cx, &[&folder]).await;
    connected.send(&browser, &list_request(1, None, None));
    cx.run_until_parked();
    connected.controls(&browser);
    cx.executor().advance_clock(MIN_FRESH_READ_INTERVAL);

    // Started by something else, and not yet run when the ask arrives.
    cx.update(|_, cx| {
        let store = SessionStore::global(cx);
        store.update(cx, |store, cx| store.refresh(cx));
    });
    connected.send(&browser, &list_request(2, None, None));
    cx.run_until_parked();
    let listed = pages(connected.controls(&browser));
    let ids: Vec<&str> = listed[0].0.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(
        ids,
        ["new", "old"],
        "{} reads",
        sweeps.load(Ordering::SeqCst)
    );
}

#[gpui::test]
async fn a_list_with_no_window_open_is_empty(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let folder = Folder::new();
    install_sessions(cx, vec![session("a", AgentKind::Claude, &folder.root, 1)]);

    connected.send(&browser, &list_request(1, None, None));
    cx.run_until_parked();
    let replies = pages(connected.controls(&browser));
    assert_eq!(replies, vec![(Vec::new(), false, None)]);
}

#[gpui::test]
async fn resuming_opens_a_tab_on_the_session_in_the_workspace_that_holds_it(
    cx: &mut TestAppContext,
) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let behind = Folder::new();
    let in_front = Folder::new();
    install_sessions(
        cx,
        vec![session(
            NEWEST,
            AgentKind::Claude,
            &behind.root.join("crates/ui"),
            300,
        )],
    );
    let (workspaces, cx) = window(cx, &[&behind, &in_front]).await;
    let (behind_workspace, front_workspace) = (&workspaces[0], &workspaces[1]);

    connected.send(&browser, &resume_request(5, NEWEST, false));
    cx.run_until_parked();
    let controls = connected.controls(&browser);

    assert!(
        controls.contains(&Control::SessionResumed {
            request_id: 5,
            open_agent_id: None
        }),
        "{controls:?}"
    );
    assert_eq!(tabs(behind_workspace, cx), [Some(NEWEST.to_string())]);
    assert!(
        tabs(front_workspace, cx).is_empty(),
        "the other project is left alone"
    );
    assert_eq!(
        resume_toasts(behind_workspace, cx),
        1,
        "the workspace that got the tab says so"
    );
    assert_eq!(
        resume_toasts(front_workspace, cx),
        1,
        "and so does the one in front, which is where the person is looking"
    );

    let announced: Vec<_> = controls
        .iter()
        .filter_map(|control| match control {
            Control::AgentUpdate { agent } if agent.session_id.as_deref() == Some(NEWEST) => {
                Some(agent)
            }
            _ => None,
        })
        .collect();
    assert_eq!(announced.len(), 1, "{controls:?}");
    assert_eq!(announced[0].agent.as_deref(), Some("claude"));
}

#[gpui::test]
async fn a_fork_opens_a_tab_that_is_not_on_the_original_session(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let folder = Folder::new();
    install_sessions(
        cx,
        vec![session(NEWEST, AgentKind::Claude, &folder.root, 300)],
    );
    let (workspaces, cx) = window(cx, &[&folder]).await;

    connected.send(&browser, &resume_request(6, NEWEST, true));
    cx.run_until_parked();
    let controls = connected.controls(&browser);

    assert!(
        controls.contains(&Control::SessionResumed {
            request_id: 6,
            open_agent_id: None
        }),
        "{controls:?}"
    );
    assert_eq!(
        tabs(&workspaces[0], cx),
        [None],
        "a fork is a new session with an id of its own"
    );
    assert_eq!(
        resume_toasts(&workspaces[0], cx),
        1,
        "one workspace is both the one with the tab and the one in front, and is told once"
    );
}

#[gpui::test]
async fn a_session_already_running_in_a_tab_is_answered_with_that_tab(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let folder = Folder::new();
    install_sessions(
        cx,
        vec![session(NEWEST, AgentKind::Claude, &folder.root, 300)],
    );
    let (workspaces, cx) = window(cx, &[&folder]).await;
    workspaces[0].update_in(cx, |workspace, window, cx| {
        AgentView::open_tracked(
            workspace,
            "claude-acp",
            SessionOrigin::new(SessionIntent::Tracked(NEWEST.into()), Some("running")),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    let tab = workspaces[0]
        .read_with(cx, |workspace, cx| {
            workspace.items_of_type::<AgentView>(cx).next()
        })
        .expect("the tab opened");
    tab.update(cx, |tab, cx| tab.simulate_cli_alive(true, cx));
    let listed_as = format!("agent-{}", tab.entity_id().as_u64());
    connected.controls(&browser);

    connected.send(&browser, &list_request(1, None, None));
    cx.run_until_parked();
    let listed = pages(connected.controls(&browser));
    assert_eq!(
        listed[0].0[0].open_agent_id.as_deref(),
        Some(listed_as.as_str()),
        "the list says which tab has it"
    );

    connected.send(&browser, &resume_request(2, NEWEST, false));
    cx.run_until_parked();
    assert_eq!(
        connected.controls(&browser),
        [Control::SessionResumed {
            request_id: 2,
            open_agent_id: Some(listed_as)
        }]
    );
    assert_eq!(tabs(&workspaces[0], cx).len(), 1, "no second tab");
    assert!(!told_of_a_resume(&workspaces[0], cx), "nothing was started");

    connected.send(&browser, &resume_request(3, NEWEST, true));
    cx.run_until_parked();
    connected.controls(&browser);
    assert_eq!(tabs(&workspaces[0], cx).len(), 2, "a fork is always new");
}

#[gpui::test]
async fn a_tab_whose_cli_ended_does_not_stop_the_session_being_resumed(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let folder = Folder::new();
    install_sessions(
        cx,
        vec![session(NEWEST, AgentKind::Claude, &folder.root, 300)],
    );
    let (workspaces, cx) = window(cx, &[&folder]).await;
    workspaces[0].update_in(cx, |workspace, window, cx| {
        AgentView::open_tracked(
            workspace,
            "claude-acp",
            SessionOrigin::new(SessionIntent::Tracked(NEWEST.into()), Some("ended")),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    connected.controls(&browser);

    connected.send(&browser, &resume_request(1, NEWEST, false));
    cx.run_until_parked();

    assert_eq!(tabs(&workspaces[0], cx).len(), 2);
}

#[gpui::test]
async fn a_fork_where_the_agent_cannot_fork_is_unsupported(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let folder = Folder::new();
    install_sessions(
        cx,
        vec![session("codex-one", AgentKind::Codex, &folder.root, 300)],
    );
    let (workspaces, cx) = window(cx, &[&folder]).await;

    connected.send(&browser, &resume_request(4, "codex-one", true));
    cx.run_until_parked();
    let controls = connected.controls(&browser);
    assert_eq!(
        only_error(&controls),
        (crate::session_router::UNSUPPORTED, 4)
    );
    assert!(tabs(&workspaces[0], cx).is_empty());
    assert!(!told_of_a_resume(&workspaces[0], cx));

    connected.send(&browser, &resume_request(5, "codex-one", false));
    cx.run_until_parked();
    assert!(
        connected
            .controls(&browser)
            .contains(&Control::SessionResumed {
                request_id: 5,
                open_agent_id: None
            }),
        "continuing it is fine"
    );
    assert_eq!(tabs(&workspaces[0], cx).len(), 1);
}

#[gpui::test]
async fn a_session_the_host_does_not_hold_or_may_not_open_is_not_found_and_opens_nothing(
    cx: &mut TestAppContext,
) {
    let (rig, browser, mut connected) = greeted(cx).await;
    let folder = Folder::new();
    let outside = Folder::new();
    let sweeps = install_sessions(
        cx,
        vec![
            session("outside", AgentKind::Claude, &outside.root, 300),
            session("deleted", AgentKind::Claude, &folder.root.join("gone"), 200),
            session(
                "escaping",
                AgentKind::Claude,
                &folder.root.join("../.."),
                100,
            ),
        ],
    );
    let (workspaces, cx) = window(cx, &[&folder]).await;

    for (request_id, id) in [
        (1, "no-such-session"),
        (2, "../../etc/passwd"),
        (3, "has a space"),
        (4, "outside"),
        (5, "deleted"),
        (6, "escaping"),
    ] {
        connected.send(&browser, &resume_request(request_id, id, false));
        cx.run_until_parked();
        let controls = connected.controls(&browser);
        assert_eq!(
            only_error(&controls),
            (error_code::NOT_FOUND, request_id),
            "{id}"
        );
    }
    assert!(tabs(&workspaces[0], cx).is_empty());
    assert!(!told_of_a_resume(&workspaces[0], cx));
    assert_eq!(
        sweeps.load(Ordering::SeqCst),
        1,
        "one read serves every lookup, and a shape that cannot be an id causes none"
    );
    assert!(
        !browser.client.was_told_closed(connected.session_id),
        "the session lives"
    );
    assert_eq!(rig.host.read_with(cx, |host, _| host.sessions().len()), 1);
}

#[gpui::test]
async fn a_second_resume_sent_before_the_first_is_done_is_rate_limited(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let folder = Folder::new();
    install_sessions(
        cx,
        vec![
            session(NEWEST, AgentKind::Claude, &folder.root, 300),
            session(
                "second",
                AgentKind::Claude,
                &folder.root.join("crates/ui"),
                200,
            ),
        ],
    );
    let (workspaces, cx) = window(cx, &[&folder]).await;

    connected.send(&browser, &resume_request(1, NEWEST, false));
    connected.send(&browser, &resume_request(2, "second", false));
    cx.run_until_parked();
    let controls = connected.controls(&browser);

    assert_eq!(
        request_errors(&controls),
        [(error_code::RATE_LIMITED, 2)],
        "{controls:?}"
    );
    assert!(controls.contains(&Control::SessionResumed {
        request_id: 1,
        open_agent_id: None
    }));
    assert_eq!(tabs(&workspaces[0], cx), [Some(NEWEST.to_string())]);

    connected.send(&browser, &resume_request(3, "second", false));
    cx.run_until_parked();
    assert_eq!(
        request_errors(&connected.controls(&browser)),
        [(error_code::RATE_LIMITED, 3)],
        "the first is done, but a session was only just opened"
    );

    cx.executor().advance_clock(RESUME_COOLDOWN);
    connected.send(&browser, &resume_request(4, "second", false));
    cx.run_until_parked();
    assert!(
        connected
            .controls(&browser)
            .contains(&Control::SessionResumed {
                request_id: 4,
                open_agent_id: None
            }),
        "once the first is done and its cooldown has passed the place is free again"
    );
}

#[gpui::test]
async fn a_session_opened_a_moment_ago_cannot_be_opened_again_until_the_cooldown_passes(
    cx: &mut TestAppContext,
) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let folder = Folder::new();
    install_sessions(
        cx,
        vec![
            session(NEWEST, AgentKind::Claude, &folder.root, 300),
            session(
                "other",
                AgentKind::Claude,
                &folder.root.join("crates/ui"),
                200,
            ),
        ],
    );
    let (workspaces, cx) = window(cx, &[&folder]).await;

    connected.send(&browser, &resume_request(1, NEWEST, false));
    cx.run_until_parked();
    assert!(
        connected
            .controls(&browser)
            .contains(&Control::SessionResumed {
                request_id: 1,
                open_agent_id: None
            })
    );
    assert_eq!(tabs(&workspaces[0], cx), [Some(NEWEST.to_string())]);

    // A double tap, a fork of it and another session: the tab is only just
    // there and its agent is not running, so nothing yet says it is open.
    for (request_id, id, fork) in [(2, NEWEST, false), (3, NEWEST, true), (4, "other", false)] {
        connected.send(&browser, &resume_request(request_id, id, fork));
        cx.run_until_parked();
        assert_eq!(
            request_errors(&connected.controls(&browser)),
            [(error_code::RATE_LIMITED, request_id)],
            "request {request_id}"
        );
    }
    assert_eq!(tabs(&workspaces[0], cx).len(), 1, "nothing more was opened");

    cx.executor()
        .advance_clock(RESUME_COOLDOWN - Duration::from_millis(1));
    connected.send(&browser, &resume_request(5, NEWEST, false));
    cx.run_until_parked();
    assert_eq!(
        request_errors(&connected.controls(&browser)),
        [(error_code::RATE_LIMITED, 5)],
        "a millisecond early"
    );

    cx.executor().advance_clock(Duration::from_millis(1));
    connected.send(&browser, &resume_request(6, NEWEST, false));
    cx.run_until_parked();
    assert!(
        connected
            .controls(&browser)
            .contains(&Control::SessionResumed {
                request_id: 6,
                open_agent_id: None
            })
    );
    assert_eq!(tabs(&workspaces[0], cx).len(), 2);
}

/// What each of `connections` was sent, in order.
fn controls_of(browser: &Browser, connections: &mut [&mut Connected]) -> Vec<Vec<Control>> {
    let mut found = vec![Vec::new(); connections.len()];
    for (session_id, payload) in browser.client.take_binary() {
        let Some(position) = connections
            .iter()
            .position(|connection| connection.session_id == session_id)
        else {
            continue;
        };
        let frame = connections[position]
            .channel
            .open(&payload)
            .expect("the host's frames open");
        if frame.kind == InnerKind::Control {
            found[position].push(read_control(&frame).expect("the host speaks the protocol"));
        }
    }
    found
}

#[gpui::test]
async fn one_device_has_one_session_opened_at_a_time_however_many_connections_it_has(
    cx: &mut TestAppContext,
) {
    let (rig, browser, mut first) = greeted(cx).await;
    let mut second = open_session(&rig, &browser, cx);
    second.send(&browser, &hello(1));
    cx.run_until_parked();
    controls_of(&browser, &mut [&mut first, &mut second]);
    let other_device = Browser::new(&rig, "browser-2");
    pair(&rig, &other_device, cx);
    let mut third = open_session(&rig, &other_device, cx);
    third.send(&other_device, &hello(1));
    cx.run_until_parked();
    third.controls(&other_device);

    let folder = Folder::new();
    install_sessions(
        cx,
        vec![
            session(NEWEST, AgentKind::Claude, &folder.root, 300),
            session(
                "other",
                AgentKind::Claude,
                &folder.root.join("crates/ui"),
                200,
            ),
        ],
    );
    let (workspaces, cx) = window(cx, &[&folder]).await;

    first.send(&browser, &resume_request(1, NEWEST, false));
    second.send(&browser, &resume_request(2, "other", false));
    cx.run_until_parked();
    let [from_first, from_second] = &controls_of(&browser, &mut [&mut first, &mut second])[..]
    else {
        panic!("two connections");
    };
    assert!(from_first.contains(&Control::SessionResumed {
        request_id: 1,
        open_agent_id: None
    }));
    assert_eq!(
        request_errors(from_second),
        [(error_code::RATE_LIMITED, 2)],
        "the device is already having one opened, on another connection"
    );
    assert_eq!(tabs(&workspaces[0], cx), [Some(NEWEST.to_string())]);

    second.send(&browser, &resume_request(3, "other", false));
    cx.run_until_parked();
    let from_second = controls_of(&browser, &mut [&mut first, &mut second]).remove(1);
    assert_eq!(
        request_errors(&from_second),
        [(error_code::RATE_LIMITED, 3)],
        "the first is done, and the cooldown belongs to the device and not to the connection"
    );

    third.send(&other_device, &resume_request(4, "other", false));
    cx.run_until_parked();
    assert!(
        third
            .controls(&other_device)
            .contains(&Control::SessionResumed {
                request_id: 4,
                open_agent_id: None
            }),
        "another device is not held up by it"
    );

    cx.executor().advance_clock(RESUME_COOLDOWN);
    second.send(&browser, &resume_request(5, NEWEST, true));
    cx.run_until_parked();
    let from_second = controls_of(&browser, &mut [&mut first, &mut second]).remove(1);
    assert!(from_second.contains(&Control::SessionResumed {
        request_id: 5,
        open_agent_id: None
    }));
    assert_eq!(tabs(&workspaces[0], cx).len(), 3);
}

#[gpui::test]
async fn the_notices_of_two_sessions_stay_up_side_by_side_until_each_has_had_its_time(
    cx: &mut TestAppContext,
) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let folder = Folder::new();
    install_sessions(
        cx,
        vec![
            session(NEWEST, AgentKind::Claude, &folder.root, 300),
            session(
                "other",
                AgentKind::Claude,
                &folder.root.join("crates/ui"),
                200,
            ),
        ],
    );
    let (workspaces, cx) = window(cx, &[&folder]).await;

    connected.send(&browser, &resume_request(1, NEWEST, false));
    cx.run_until_parked();
    assert_eq!(resume_toasts(&workspaces[0], cx), 1);

    cx.executor().advance_clock(RESUME_COOLDOWN);
    connected.send(&browser, &resume_request(2, "other", false));
    cx.run_until_parked();
    assert_eq!(
        resume_toasts(&workspaces[0], cx),
        2,
        "the second does not replace the first before it has been read"
    );

    // The first has been up for five seconds by now, the second for two. If
    // they shared an id, the first's timer would take the second down with it.
    cx.executor().advance_clock(Duration::from_millis(2001));
    assert_eq!(resume_toasts(&workspaces[0], cx), 1);

    cx.executor().advance_clock(Duration::from_secs(3));
    assert_eq!(resume_toasts(&workspaces[0], cx), 0);
}

#[gpui::test]
async fn a_session_under_a_private_or_excluded_path_is_neither_listed_nor_opened(
    cx: &mut TestAppContext,
) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.project.worktree.private_files = Some(vec!["**/vault".to_string()].into());
                settings.project.worktree.file_scan_exclusions = Some(vec!["**/dist".into()]);
            });
        });
    });
    let folder = Folder::new();
    install_sessions(
        cx,
        vec![
            session(
                "plain",
                AgentKind::Claude,
                &folder.root.join("crates/ui"),
                100,
            ),
            session(
                "private",
                AgentKind::Claude,
                &folder.root.join("vault/keys"),
                300,
            ),
            session(
                "excluded",
                AgentKind::Claude,
                &folder.root.join("dist/out"),
                200,
            ),
        ],
    );
    let (workspaces, cx) = window(cx, &[&folder]).await;

    connected.send(&browser, &list_request(1, None, None));
    cx.run_until_parked();
    let listed = pages(connected.controls(&browser));
    assert_eq!(
        listed[0]
            .0
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["plain"]
    );

    for (request_id, id) in [(2, "private"), (3, "excluded")] {
        connected.send(&browser, &resume_request(request_id, id, false));
        cx.run_until_parked();
        assert_eq!(
            only_error(&connected.controls(&browser)),
            (error_code::NOT_FOUND, request_id),
            "{id}"
        );
    }
    assert!(tabs(&workspaces[0], cx).is_empty());

    connected.send(&browser, &resume_request(4, "plain", false));
    cx.run_until_parked();
    assert!(
        connected
            .controls(&browser)
            .contains(&Control::SessionResumed {
                request_id: 4,
                open_agent_id: None
            }),
        "what the folder shows can still be opened"
    );
    assert_eq!(tabs(&workspaces[0], cx), [Some("plain".to_string())]);
}

#[gpui::test]
async fn a_folder_with_no_name_of_its_own_is_called_root_and_never_by_its_path(
    cx: &mut TestAppContext,
) {
    let (_rig, _browser, _connected) = greeted(cx).await;
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/"), json!({ "work": {} })).await;
    let (_workspaces, cx) = window_over(cx, fs, &[Path::new(path!("/"))]).await;

    // Not through the window's own `update`: it holds the window, and a window
    // that is being updated cannot be read.
    let scopes: Vec<RootScope> = cx
        .cx
        .update(|cx| open_roots(cx).into_iter().map(|root| root.scope).collect());
    let [scope] = &scopes[..] else {
        panic!("one folder is open, not {}", scopes.len());
    };
    assert_eq!(scope.name, "root");

    let inside = session("s", AgentKind::Claude, &scope.path.join("work"), 1);
    let (entries, _) = page(&[inside], &scopes, None, 50, |_| None);
    assert_eq!(entries[0].folder, "root/work");
    let mut characters = entries[0].folder.chars();
    let drive_letter = characters.next().is_some_and(|c| c.is_ascii_alphabetic())
        && characters.next() == Some(':');
    assert!(
        !drive_letter && !entries[0].folder.contains(':'),
        "{}",
        entries[0].folder
    );
}

#[gpui::test]
async fn a_collaboration_guests_project_holds_no_sessions(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let folder = Folder::new();
    install_sessions(
        cx,
        vec![session(NEWEST, AgentKind::Claude, &folder.root, 300)],
    );
    let (workspaces, cx) = window(cx, &[&folder]).await;
    connected.send(&browser, &list_request(1, None, None));
    cx.run_until_parked();
    assert_eq!(
        pages(connected.controls(&browser))[0].0.len(),
        1,
        "the session is listed while the project is local"
    );

    // The folder is on this machine's disk and the project is not, which is
    // what a session started in a folder of someone else's project looks like
    // from here.
    workspaces[0].update(cx, |workspace, cx| {
        workspace
            .project()
            .update(cx, |project, _| project.mark_as_collab_for_testing());
    });
    assert!(!workspaces[0].read_with(cx, |workspace, cx| workspace.project().read(cx).is_local()));
    assert_eq!(
        workspaces[0].read_with(cx, |workspace, cx| workspace
            .project()
            .read(cx)
            .visible_worktrees(cx)
            .count()),
        1,
        "it still has the folder open"
    );

    connected.send(&browser, &list_request(2, None, None));
    cx.run_until_parked();
    assert_eq!(
        pages(connected.controls(&browser)),
        vec![(Vec::new(), false, None)]
    );

    connected.send(&browser, &resume_request(3, NEWEST, false));
    cx.run_until_parked();
    assert_eq!(
        only_error(&connected.controls(&browser)),
        (error_code::NOT_FOUND, 3)
    );
    assert!(tabs(&workspaces[0], cx).is_empty());
}

#[gpui::test]
async fn without_the_capability_both_requests_are_unsupported(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = connected(cx, false, false).await;
    connected.controls(&browser);
    let folder = Folder::new();
    let sweeps = install_sessions(
        cx,
        vec![session(NEWEST, AgentKind::Claude, &folder.root, 300)],
    );
    let (workspaces, cx) = window(cx, &[&folder]).await;

    for (request_id, request) in [
        (1, list_request(1, None, None)),
        (2, resume_request(2, NEWEST, false)),
    ] {
        connected.send(&browser, &request);
        cx.run_until_parked();
        let controls = connected.controls(&browser);
        assert_eq!(
            only_error(&controls),
            (crate::session_router::UNSUPPORTED, request_id)
        );
    }
    assert_eq!(sweeps.load(Ordering::SeqCst), 0, "nothing was read");
    assert!(tabs(&workspaces[0], cx).is_empty());
}

/// The remote host the tests' WSL distro stands for.
const UBUNTU: u64 = 7;

fn token(host: u64) -> String {
    HostToken::of(&RemoteConnectionIdentity::Mock { id: host })
        .as_str()
        .to_string()
}

fn on_host(host: u64, id: &str) -> String {
    format!("{}:{id}", token(host))
}

/// Has `workspace`'s project read as one on the remote host `host`, shown as
/// `label`, whose sessions are `sessions`. The store is not swept until
/// something asks it to be.
fn put_on_host(
    workspace: &Entity<Workspace>,
    host: u64,
    label: &str,
    sessions: Vec<SessionSummary>,
    cx: &mut VisualTestContext,
) -> (Entity<SessionStore>, Arc<AtomicUsize>) {
    let sweeps = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(FakeProvider {
        sessions,
        sweeps: sweeps.clone(),
    });
    let project = workspace.read_with(cx, |workspace, _| workspace.project().clone());
    let store = cx.cx.update(|cx| {
        agent_ui::assign_remote_host_for_test(
            &project,
            RemoteConnectionIdentity::Mock { id: host },
            label,
            AgentSessionHost::Local(vec![provider]),
            cx,
        );
        SessionStore::for_project(&project, cx)
    });
    (store, sweeps)
}

/// Lets `store` finish a sweep, as one a list started would have by now.
fn sweep(store: &Entity<SessionStore>, cx: &mut VisualTestContext) {
    store.update(cx, |store, cx| store.refresh(cx));
    cx.run_until_parked();
}

fn list_reply(controls: Vec<Control>) -> (Vec<SessionEntry>, Vec<UnavailableHost>) {
    let mut entries = Vec::new();
    let mut notices = Vec::new();
    for control in controls {
        if let Control::SessionsListReply {
            sessions,
            unavailable_hosts,
            ..
        } = control
        {
            entries.extend(sessions);
            notices.extend(unavailable_hosts);
        }
    }
    (entries, notices)
}

fn reading(label: &str) -> UnavailableHost {
    UnavailableHost {
        host: label.into(),
        reason: host_status::READING.into(),
    }
}

/// Every device paired before remote hosts were listed reads these bytes; a
/// list of this machine's sessions alone must still be exactly them.
#[gpui::test]
async fn a_list_of_this_machines_sessions_alone_is_byte_for_byte_what_it_was(
    cx: &mut TestAppContext,
) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let folder = Folder::new();
    install_sessions(
        cx,
        vec![
            session(
                NEWEST,
                AgentKind::Claude,
                &folder.root.join("crates/terminal"),
                300,
            ),
            session("older", AgentKind::Codex, &folder.root, 200),
        ],
    );
    let (workspaces, cx) = window(cx, &[&folder]).await;
    let worktree_id = worktree_id_of(&workspaces[0], cx);
    let name = folder.name();
    let preview = format!("{}…", "p".repeat(199));

    let raw = |connected: &mut Connected| -> Vec<String> {
        connected
            .receive(&browser)
            .into_iter()
            .filter(|(kind, _, _)| *kind == InnerKind::Control)
            .map(|(_, _, payload)| String::from_utf8(payload).expect("JSON is text"))
            .collect()
    };
    connected.send(&browser, &list_request(1, None, Some(1)));
    cx.run_until_parked();
    assert_eq!(
        raw(&mut connected),
        [format!(
            r#"{{"t":"sessions_list_reply","request_id":1,"sessions":[{{"id":"{NEWEST}","agent":"claude","title":"title of {NEWEST}","preview":"{preview}","worktree_id":"{worktree_id}","folder":"{name}/crates/terminal","branch":"main","updated_at":300000,"can_fork":true}}],"more":false,"next":{{"updated_at":300000,"id":"{NEWEST}"}}}}"#
        )]
    );

    let cursor = SessionCursor {
        updated_at: 300_000,
        id: NEWEST.into(),
    };
    connected.send(&browser, &list_request(2, Some(cursor), Some(1)));
    cx.run_until_parked();
    assert_eq!(
        raw(&mut connected),
        [format!(
            r#"{{"t":"sessions_list_reply","request_id":2,"sessions":[{{"id":"older","agent":"codex","title":"title of older","preview":"{preview}","worktree_id":"{worktree_id}","folder":"{name}","branch":"main","updated_at":200000,"can_fork":false}}],"more":false}}"#
        )],
        "a cursor a device kept from before still pages"
    );
}

/// A home copied into a distro holds the same session id as this machine's.
/// The device dedups by id, so each must have an id of its own, and the
/// remote one says which host it is on.
#[gpui::test]
async fn the_same_session_on_this_machine_and_a_host_is_listed_twice_with_ids_of_its_own(
    cx: &mut TestAppContext,
) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let here = Folder::new();
    let there = Folder::new();
    install_sessions(
        cx,
        vec![session(NEWEST, AgentKind::Claude, &here.root, 300)],
    );
    let (workspaces, cx) = window(cx, &[&here, &there]).await;
    let local_worktree = worktree_id_of(&workspaces[0], cx);
    let remote_worktree = worktree_id_of(&workspaces[1], cx);
    put_on_host(
        &workspaces[1],
        UBUNTU,
        "Ubuntu",
        vec![session(NEWEST, AgentKind::Claude, &there.root, 300)],
        cx,
    );

    connected.send(&browser, &list_request(1, None, None));
    cx.run_until_parked();
    let (entries, notices) = list_reply(connected.controls(&browser));
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        [NEWEST],
        "the host's store had not been read yet"
    );
    assert_eq!(notices, [reading("Ubuntu")]);

    connected.send(&browser, &list_request(2, None, None));
    cx.run_until_parked();
    let (entries, notices) = list_reply(connected.controls(&browser));
    let mut seen: Vec<(String, String, Option<String>)> = entries
        .into_iter()
        .map(|entry| (entry.id, entry.worktree_id, entry.host))
        .collect();
    // Same millisecond, so the order is by id, which the token decides.
    seen.sort();
    let mut expected = vec![
        (
            on_host(UBUNTU, NEWEST),
            on_host(UBUNTU, &remote_worktree),
            Some("Ubuntu".to_string()),
        ),
        (NEWEST.to_string(), local_worktree, None),
    ];
    expected.sort();
    assert_eq!(seen, expected);
    assert!(notices.is_empty(), "{notices:?}");
}

/// A host whose connection has stopped answering must not hold up this
/// machine's list, however long it takes.
#[gpui::test]
async fn a_host_that_never_answers_does_not_hold_up_the_list(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let here = Folder::new();
    let there = Folder::new();
    install_sessions(cx, vec![session("local", AgentKind::Claude, &here.root, 1)]);
    let (workspaces, cx) = window(cx, &[&here, &there]).await;
    let (store, _) = put_on_host(
        &workspaces[1],
        UBUNTU,
        "Ubuntu",
        vec![session("remote", AgentKind::Claude, &there.root, 2)],
        cx,
    );
    store.update(cx, |store, cx| store.begin_endless_sweep_for_test(cx));

    for request_id in 1..=2 {
        connected.send(&browser, &list_request(request_id, None, None));
        cx.run_until_parked();
        let (entries, notices) = list_reply(connected.controls(&browser));
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["local"]
        );
        assert_eq!(notices, [reading("Ubuntu")]);
    }
}

/// What went wrong on a host -- an address, a user, a path -- is not the
/// device's to read: it is told the host is unavailable, and no more.
#[gpui::test]
async fn a_host_whose_last_read_failed_is_named_unavailable_and_nothing_more(
    cx: &mut TestAppContext,
) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let here = Folder::new();
    let there = Folder::new();
    install_sessions(cx, vec![session("local", AgentKind::Claude, &here.root, 1)]);
    let (workspaces, cx) = window(cx, &[&here, &there]).await;
    let (store, _) = put_on_host(&workspaces[1], UBUNTU, "Ubuntu", Vec::new(), cx);
    store.update(cx, |store, cx| {
        store.fail_sweep_for_test("ssh alice@10.0.0.5: /home/alice/.claude unreadable", cx)
    });

    connected.send(&browser, &list_request(1, None, None));
    cx.run_until_parked();
    let replies: Vec<String> = connected
        .receive(&browser)
        .into_iter()
        .filter(|(kind, _, _)| *kind == InnerKind::Control)
        .map(|(_, _, payload)| String::from_utf8(payload).expect("JSON is text"))
        .collect();
    let [reply] = &replies[..] else {
        panic!("{replies:?}");
    };
    assert!(
        reply.ends_with(r#""unavailable_hosts":[{"host":"Ubuntu","reason":"unavailable"}]}"#),
        "{reply}"
    );
    for leaked in ["alice", "10.0.0.5", ".claude"] {
        assert!(!reply.contains(leaked), "{leaked} in {reply}");
    }
}

/// Each first page may have a host's store read again, but no more often than
/// this machine's: a device asking as each answer arrives must not keep the
/// host busy.
#[gpui::test]
async fn a_hosts_store_is_read_again_at_most_once_an_interval(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let there = Folder::new();
    install_sessions(cx, Vec::new());
    let (workspaces, cx) = window(cx, &[&there]).await;
    let (_store, sweeps) = put_on_host(
        &workspaces[0],
        UBUNTU,
        "Ubuntu",
        vec![session("remote", AgentKind::Claude, &there.root, 2)],
        cx,
    );

    for request_id in 1..=3 {
        connected.send(&browser, &list_request(request_id, None, None));
        cx.run_until_parked();
        connected.controls(&browser);
    }
    assert_eq!(sweeps.load(Ordering::SeqCst), 1, "three asks, one read");

    cx.executor().advance_clock(MIN_FRESH_READ_INTERVAL);
    connected.send(&browser, &list_request(4, None, None));
    cx.run_until_parked();
    let (entries, _) = list_reply(connected.controls(&browser));
    assert_eq!(sweeps.load(Ordering::SeqCst), 2);
    assert_eq!(entries[0].id, on_host(UBUNTU, "remote"));
}

#[gpui::test]
async fn resuming_a_hosts_session_opens_it_in_that_hosts_window(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let here = Folder::new();
    let there = Folder::new();
    install_sessions(
        cx,
        vec![session(NEWEST, AgentKind::Claude, &here.root, 300)],
    );
    let (workspaces, cx) = window(cx, &[&here, &there]).await;
    let (store, _) = put_on_host(
        &workspaces[1],
        UBUNTU,
        "Ubuntu",
        vec![session(
            NEWEST,
            AgentKind::Claude,
            &there.root.join("crates/ui"),
            300,
        )],
        cx,
    );
    sweep(&store, cx);
    let wire = on_host(UBUNTU, NEWEST);

    connected.send(&browser, &resume_request(5, &wire, false));
    cx.run_until_parked();
    let controls = connected.controls(&browser);

    assert!(
        controls.contains(&Control::SessionResumed {
            request_id: 5,
            open_agent_id: None
        }),
        "{controls:?}"
    );
    assert!(tabs(&workspaces[0], cx).is_empty(), "not this machine's");
    assert_eq!(tabs(&workspaces[1], cx), [Some(NEWEST.to_string())]);
    assert!(told_of_a_resume(&workspaces[1], cx));
    let announced: Vec<_> = controls
        .iter()
        .filter_map(|control| match control {
            Control::AgentUpdate { agent } => agent.session_id.clone(),
            _ => None,
        })
        .collect();
    assert_eq!(
        announced,
        [wire],
        "the device finds the tab by the id it asked for"
    );
}

/// The host is asked whether the folder is still there: a session whose
/// folder has gone opens nothing, and the device is told so.
#[gpui::test]
async fn a_hosts_session_whose_folder_is_gone_is_not_found_and_opens_nothing(
    cx: &mut TestAppContext,
) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let there = Folder::new();
    install_sessions(cx, Vec::new());
    let (workspaces, cx) = window(cx, &[&there]).await;
    let (store, _) = put_on_host(
        &workspaces[0],
        UBUNTU,
        "Ubuntu",
        vec![session(
            "gone",
            AgentKind::Claude,
            &there.root.join("gone"),
            1,
        )],
        cx,
    );
    sweep(&store, cx);

    connected.send(
        &browser,
        &resume_request(3, &on_host(UBUNTU, "gone"), false),
    );
    cx.run_until_parked();
    assert_eq!(
        only_error(&connected.controls(&browser)),
        (error_code::NOT_FOUND, 3)
    );
    assert!(tabs(&workspaces[0], cx).is_empty());
    assert!(!told_of_a_resume(&workspaces[0], cx));
}

/// A host's store keeps its list after its last window closes; a device that
/// kept a list from before must still not open anything on it.
#[gpui::test]
async fn a_host_whose_last_window_has_closed_has_nothing_to_resume(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let there = Folder::new();
    install_sessions(cx, Vec::new());
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(&there.root, json!({ "crates": {} })).await;
    cx.update(|cx| {
        editor::init(cx);
        project::DisableAiSettings::register(cx);
    });
    let project = Project::test(fs, [there.root.as_path()], cx).await;
    let (multi, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    cx.run_until_parked();
    let workspace = multi.read_with(cx, |multi, _| multi.workspace().clone());
    let (store, _) = put_on_host(
        &workspace,
        UBUNTU,
        "Ubuntu",
        vec![session(NEWEST, AgentKind::Claude, &there.root, 1)],
        cx,
    );
    sweep(&store, cx);
    let wire = on_host(UBUNTU, NEWEST);
    connected.send(&browser, &list_request(1, None, None));
    cx.run_until_parked();
    let (entries, _) = list_reply(connected.controls(&browser));
    assert_eq!(entries[0].id, wire, "listed while the window is open");

    multi.update_in(cx, |_, window, _| window.remove_window());
    cx.run_until_parked();
    assert_eq!(
        store.read_with(cx, |store, _| store.index().len()),
        1,
        "the store still holds it"
    );

    connected.send(&browser, &resume_request(2, &wire, false));
    cx.run_until_parked();
    assert_eq!(
        only_error(&connected.controls(&browser)),
        (error_code::NOT_FOUND, 2)
    );
    connected.send(&browser, &list_request(3, None, None));
    cx.run_until_parked();
    assert_eq!(
        list_reply(connected.controls(&browser)),
        (Vec::new(), Vec::new()),
        "and it is listed no more"
    );
}

/// A tab on this machine running a session is not the tab of the same id on
/// a host.
#[gpui::test]
async fn a_tab_running_this_machines_session_is_not_the_hosts(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let here = Folder::new();
    let there = Folder::new();
    install_sessions(
        cx,
        vec![session(NEWEST, AgentKind::Claude, &here.root, 300)],
    );
    let (workspaces, cx) = window(cx, &[&here, &there]).await;
    let (store, _) = put_on_host(
        &workspaces[1],
        UBUNTU,
        "Ubuntu",
        vec![session(NEWEST, AgentKind::Claude, &there.root, 300)],
        cx,
    );
    sweep(&store, cx);
    workspaces[0].update_in(cx, |workspace, window, cx| {
        AgentView::open_tracked(
            workspace,
            "claude-acp",
            SessionOrigin::new(SessionIntent::Tracked(NEWEST.into()), Some("running")),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    let tab = workspaces[0]
        .read_with(cx, |workspace, cx| {
            workspace.items_of_type::<AgentView>(cx).next()
        })
        .expect("the tab opened");
    tab.update(cx, |tab, cx| tab.simulate_cli_alive(true, cx));
    let listed_as = format!("agent-{}", tab.entity_id().as_u64());
    connected.controls(&browser);

    connected.send(&browser, &list_request(1, None, None));
    cx.run_until_parked();
    let (entries, _) = list_reply(connected.controls(&browser));
    let mut open: Vec<(&str, Option<&str>)> = entries
        .iter()
        .map(|entry| (entry.id.as_str(), entry.open_agent_id.as_deref()))
        .collect();
    open.sort();
    let wire = on_host(UBUNTU, NEWEST);
    let mut expected = vec![(wire.as_str(), None), (NEWEST, Some(listed_as.as_str()))];
    expected.sort();
    assert_eq!(open, expected);

    connected.send(&browser, &resume_request(2, &wire, false));
    cx.run_until_parked();
    assert!(
        connected
            .controls(&browser)
            .contains(&Control::SessionResumed {
                request_id: 2,
                open_agent_id: None
            }),
        "a tab is opened on the host rather than pointing at this machine's"
    );
    assert_eq!(tabs(&workspaces[1], cx), [Some(NEWEST.to_string())]);
}
