use crate::{AgentView, agent_view::SessionIntent, session_history::panel::AgentHistoryPanel};
use agent_sessions::{
    AgentCommand, AgentKind, Deletion, Fork, SessionProvider, SessionSummary,
    wire::WireDeleteOutcome,
};
use gpui::{
    App, AppContext as _, AsyncWindowContext, ClipboardItem, Context, Entity, SharedString, Task,
    Window,
};
use project::agent_session_host::{AgentSessionHost, CommandDeleteOutcome, run_command_delete};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use util::ResultExt as _;
use workspace::notifications::NotificationId;

/// Whether the directory a session ran in is still there.
///
/// The row's badge and every control that would run something in that directory
/// read this. A resume into a directory that no longer exists would start the CLI
/// in a place with none of the files the conversation is about.
pub(crate) fn cwd_exists(session: &SessionSummary) -> bool {
    directory_exists(&session.cwd)
}

pub(crate) use project::agent_session_host::directory_exists;

/// Why [`resume_session`] would do nothing for a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeRefusal {
    /// The agent has no way to do what was asked -- Codex and Copilot cannot
    /// fork.
    Unsupported,
    /// The directory the session ran in is not there any more.
    CwdGone,
}

/// Whether [`resume_session`] would refuse `session`, and why.
///
/// `resume_session` returns nothing and opens nothing when it refuses, which is
/// right for a button that is already disabled and wrong for a caller that has
/// to tell somebody else why nothing happened. This is the same two checks, in
/// the same order, asked first.
pub fn resume_refusal(session: &SessionSummary, fork: Fork) -> Option<ResumeRefusal> {
    // Asked only so the control stays disabled where the agent cannot honour
    // it — Codex has no fork. The command itself is rebuilt at spawn time from
    // the id, so what comes back here is discarded.
    if agent_sessions::provider_for(session.agent)
        .resume_command(session, fork)
        .is_none()
    {
        return Some(ResumeRefusal::Unsupported);
    }
    if !cwd_exists(session) {
        return Some(ResumeRefusal::CwdGone);
    }
    None
}

/// [`resume_refusal`], answered on the background executor.
///
/// The check stats the directory the session ran in and builds the agent's
/// store, which for two of the four stores canonicalizes a path. On a stalled
/// network mount that is as long as the mount stalls, and a caller on the
/// foreground thread -- which is every caller there is -- would hold the whole
/// window for it. [`resume_session`] uses this; a caller that reports the
/// refusal itself should wait on it rather than call [`resume_refusal`].
pub fn resume_refusal_in_background(
    session: &SessionSummary,
    fork: Fork,
    cx: &App,
) -> Task<Option<ResumeRefusal>> {
    let session = session.clone();
    cx.background_spawn(async move { resume_refusal(&session, fork) })
}

/// What a tab opened onto `session` is bound to.
///
/// A fork is deliberately NOT tracked. `--fork-session` makes the CLI mint a
/// *new* id, so a tab carrying the id we resumed from would come back on the
/// original conversation rather than the fork -- the one failure this whole
/// feature exists to avoid, and silent. It is not untracked either: a tab with
/// no intent starts the CLI with no arguments, and nothing would have been
/// forked. It carries its source instead, which is only read to build the
/// command that branches off it.
fn intent_for_resume(session: &SessionSummary, fork: Fork) -> SessionIntent {
    let id: gpui::SharedString = session.id.to_string().into();
    match fork {
        Fork::Continue => SessionIntent::Tracked(id),
        Fork::New => SessionIntent::Fork(id),
    }
}

/// Opens a tab back on `session`, or branches a new one off it.
///
/// Free rather than a method on the history panel: the sidebar reaches the same
/// sessions through the shared session index, and the rules about what may be
/// resumed -- the agent must support it, the working directory must still exist,
/// a fork must not carry the id it forked from -- belong to the operation, not
/// to whichever surface asked for it.
///
/// Whether it may be resumed is decided off the foreground thread (see
/// [`resume_refusal_in_background`]), so the tab opens a moment after this
/// returns, and not at all when the answer is no.
pub fn resume_session(
    workspace: &Entity<workspace::Workspace>,
    session: &SessionSummary,
    fork: Fork,
    window: &mut Window,
    cx: &mut App,
) {
    // The checks below stat this machine's disk and build this machine's
    // providers. For a session on a remote host they answer about the wrong
    // machine: nothing at all on Windows with WSL, or, where a local path
    // happens to match, a tab that starts fresh while looking resumed.
    if workspace.read(cx).project().read(cx).is_via_remote_server() {
        refuse_remote_resume(workspace, cx);
        return;
    }
    let refusal = resume_refusal_in_background(session, fork, cx);
    let agent = session.agent.builtin_agent_id();
    // `SessionOrigin::new` drops the title of a tab that is not tracked -- so a
    // fork does not inherit the name of the conversation it branched from
    // without a second branch here.
    // Pinned to this very summary, not just its id: the caller has already
    // looked the session up and checked it against what may be opened, and a
    // second lookup by id can name a different copy of it.
    let origin = crate::SessionOrigin::new(
        intent_for_resume(session, fork),
        Some(session.title.as_str()),
    )
    .anchored_on(session);
    let workspace = workspace.downgrade();
    window
        .spawn(cx, async move |cx| {
            if refusal.await.is_some() {
                return;
            }
            workspace
                .update_in(cx, |workspace, window, cx| {
                    AgentView::open_tracked(workspace, agent, origin, window, cx);
                })
                .log_err();
        })
        .detach();
}

/// What resuming a session on a remote host says, until a tab can run the
/// agent there.
pub(crate) const REMOTE_RESUME_UNSUPPORTED: &str = "Resuming a remote session is not supported yet";

fn refuse_remote_resume(workspace: &Entity<workspace::Workspace>, cx: &mut App) {
    workspace.update(cx, |workspace, cx| {
        show_message(workspace, named_message(REMOTE_RESUME_UNSUPPORTED), cx)
    });
}

/// A message whose notification is named by its own text, so the same message
/// raised twice refreshes one toast rather than stacking a second.
fn named_message(text: &'static str) -> (NotificationId, SharedString) {
    let text = SharedString::new_static(text);
    (NotificationId::named(text.clone()), text)
}

fn show_message(
    workspace: &mut workspace::Workspace,
    (id, text): (NotificationId, SharedString),
    cx: &mut Context<workspace::Workspace>,
) {
    workspace.show_notification(id, cx, |cx| {
        cx.new(|cx| {
            workspace::notifications::simple_message_notification::MessageNotification::new(
                text, cx,
            )
        })
    });
}

/// What a confirmation adds when sessions it is about to take are running in
/// open tabs.
///
/// A delete does not stop the agent writing. It may keep appending to a
/// transcript after it has been moved, or write it again, and the tab reading it
/// then goes quiet or the row comes back. Nothing here refuses: the person may
/// well mean it, and closing the tab is theirs to decide.
pub(crate) fn running_in_tabs_note(running: usize, total: usize) -> Option<String> {
    let which = match (running, total) {
        (0, _) => return None,
        (_, 1) => "An open tab is running this session.".to_string(),
        (1, _) => "1 of these sessions is running in an open tab.".to_string(),
        (running, _) => format!("{running} of these sessions are running in open tabs."),
    };
    Some(format!(
        "{which} The agent may keep writing to a transcript after it is moved, so a tab \
         can stop reporting finished turns and the session can come back in this list. \
         Close the tab first to avoid both."
    ))
}

/// How many of `sessions` a tab whose agent is alive is running.
pub(crate) fn running_in_tabs<'a>(
    sessions: impl IntoIterator<Item = (AgentKind, &'a Arc<str>)>,
    cx: &App,
) -> usize {
    let running = crate::agent_tabs::running_sessions(cx);
    if running.is_empty() {
        return 0;
    }
    sessions
        .into_iter()
        .filter(|(agent, id)| {
            running.contains(&(
                project::AgentId::new(agent.builtin_agent_id()),
                gpui::SharedString::from(id.to_string()),
            ))
        })
        .count()
}

/// `detail` with the note about open tabs added when there is one.
fn with_running_note(detail: String, note: Option<String>) -> String {
    match note {
        Some(note) => format!("{detail}\n\n{note}"),
        None => detail,
    }
}

/// Moves a session's transcript to the trash, after asking.
///
/// Free rather than a method on the history panel, for the reason
/// [`resume_session`] is: the panel is no longer the only surface listing
/// sessions, and what a delete takes -- and what it warns about before taking
/// it -- belongs to the operation rather than to whichever list asked.
///
/// The confirmation names every path and the bytes involved: "delete session"
/// and "delete forty megabytes of a conversation nobody has read since" look
/// identical from a menu.
pub fn delete_session(
    workspace: &Entity<workspace::Workspace>,
    session: &SessionSummary,
    window: &mut Window,
    cx: &mut App,
) {
    // The paths a provider here names are this machine's, and `project.fs()`
    // of a remote project is this machine's too: a delete here would take a
    // local transcript that merely shares a path with the remote one. A remote
    // project's sessions are deleted by its host, and a collaboration guest's
    // by nobody.
    match session_host(workspace, cx) {
        Some(AgentSessionHost::Local(_)) => {}
        Some(AgentSessionHost::Remote(remote)) => {
            let project = workspace.read(cx).project().clone();
            let store = crate::SessionStore::for_project(&project, cx);
            delete_on_remote_host(
                workspace,
                store,
                session,
                move |agent, id, cx| remote.delete(agent, &id, cx),
                window,
                cx,
            );
            return;
        }
        None => {
            notify_shared_delete_refused(workspace, cx);
            return;
        }
    }
    let provider = agent_sessions::provider_for(session.agent);
    match provider.deletion(session) {
        Deletion::Nothing => {
            offer_to_drop_the_row(crate::SessionStore::global(cx), session, window, cx)
        }
        Deletion::Trash(paths) => delete_via_trash(workspace, session, paths, window, cx),
        Deletion::Command(command) => {
            delete_via_command(workspace, provider, session, command, window, cx)
        }
    }
}

/// The `Trash` half of [`delete_session`]: moves every path to the OS trash and
/// forgets the row only once every one of them is really gone.
///
/// A path already missing is not a failure -- `fs::RemoveOptions::ignore_if_not_exists`
/// says so, and the `!path.exists()` check below skips the call rather than
/// routing a no-op through it. What must not happen is forgetting the row when a
/// real path survived the attempt (H7): a permission error, a full disk, or a
/// mount with no trash service must leave the row describing a transcript that is
/// still there, matching the discipline `delete_all` already keeps in
/// `every_path_gone`.
fn delete_via_trash(
    workspace: &Entity<workspace::Workspace>,
    session: &SessionSummary,
    paths: Vec<PathBuf>,
    window: &mut Window,
    cx: &mut App,
) {
    let fs = workspace.read(cx).project().read(cx).fs().clone();

    let detail = with_running_note(
        format!(
            "{}\n\n{} will move to the trash ({}).",
            session.title,
            paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
            format_bytes(session.log_bytes)
        ),
        running_in_tabs_note(running_in_tabs([(session.agent, &session.id)], cx), 1),
    );
    let prompt = window.prompt(
        gpui::PromptLevel::Warning,
        "Delete this session?",
        Some(&detail),
        &["Move to Trash", "Cancel"],
        cx,
    );

    let store = crate::SessionStore::global(cx);
    let id = session.id.clone();
    cx.spawn(async move |cx| {
        if prompt.await.ok() != Some(0) {
            return;
        }
        let mut every_path_gone = true;
        for path in paths {
            if !path.exists() {
                continue;
            }
            let trashed = fs
                .trash(
                    &path,
                    fs::RemoveOptions {
                        recursive: true,
                        ignore_if_not_exists: true,
                    },
                )
                .await
                .log_err();
            if trashed.is_none() {
                every_path_gone = false;
            }
        }
        // Drop the entry rather than re-sweeping: the delete already knows
        // exactly what it removed, and a sweep would open every other
        // transcript on disk to learn one fact it was told. But only once
        // every path really went (H7) -- a row must not outlive the one thing
        // that was tracking it.
        if every_path_gone {
            store.update(cx, |store, cx| store.forget(&id, cx));
        }
    })
    .detach();
}

/// The `Command` half of [`delete_session`]. Not recoverable -- the prompt says
/// so -- and the binary is resolved through `AgentServerStore` rather than
/// trusting `command.program`, which `agent_task` already treats as advisory
/// only.
fn delete_via_command(
    workspace: &Entity<workspace::Workspace>,
    provider: Arc<dyn SessionProvider>,
    session: &SessionSummary,
    command: AgentCommand,
    window: &mut Window,
    cx: &mut App,
) {
    let agent = session.agent;
    let agent_label = agent.label();
    let detail = with_running_note(
        format!(
            "{}\n\n{agent_label}'s own store holds this session, not a file this editor \
             can move to the trash. Deleting it asks {agent_label} to remove it there. \
             This cannot be undone.",
            session.title
        ),
        running_in_tabs_note(running_in_tabs([(session.agent, &session.id)], cx), 1),
    );
    let prompt = window.prompt(
        gpui::PromptLevel::Warning,
        "Delete this session?",
        Some(&detail),
        &["Delete", "Cancel"],
        cx,
    );

    let agent_store = workspace
        .read(cx)
        .project()
        .read(cx)
        .agent_server_store()
        .clone();
    let workspace_weak = workspace.downgrade();
    let store = crate::SessionStore::global(cx);
    let id = session.id.clone();
    let title = session.title.clone();

    window
        .spawn(cx, async move |cx| {
            if prompt.await.ok() != Some(0) {
                return;
            }
            let executor = cx.background_executor().clone();
            let outcome =
                run_command_delete(&provider, &agent_store, &executor, agent, &id, &command, cx)
                    .await;
            match outcome {
                CommandDeleteOutcome::Gone => {
                    store.update(cx, |store, cx| store.forget(&id, cx));
                }
                CommandDeleteOutcome::BinaryMissing(missing) => {
                    let binary = missing.binary;
                    let detail = format!(
                        "{title}\n\n`{binary}` was not found on this machine, so \
                         {agent_label} cannot be asked to remove this session from its \
                         own store. Zode cannot delete it without {binary}.\n\nRemoving \
                         it here only takes it off this list. It does not touch \
                         {agent_label}'s own store, so the row comes back if \
                         {agent_label} still lists the session.",
                    );
                    let title_text = format!("{binary} is not installed");
                    let follow_up = cx.prompt(
                        gpui::PromptLevel::Warning,
                        &title_text,
                        Some(&detail),
                        &["Remove From List", "Cancel"],
                    );
                    if follow_up.await.ok() != Some(0) {
                        return;
                    }
                    store.update(cx, |store, cx| store.forget(&id, cx));
                }
                CommandDeleteOutcome::Failed(message) => {
                    notify_command_delete_failed(&workspace_weak, agent_label, &id, &message, cx);
                }
            }
        })
        .detach();
}

/// Where `workspace`'s project keeps its sessions; `None` for a collaboration
/// guest. See [`project::Project::agent_session_host`].
fn session_host(workspace: &Entity<workspace::Workspace>, cx: &App) -> Option<AgentSessionHost> {
    workspace.read(cx).project().read(cx).agent_session_host(cx)
}

/// What a delete on a remote host will do, as far as the confirmation can say
/// before asking it: opencode keeps its sessions in a database only its CLI
/// may write, so its delete cannot be undone; every other store's files go to
/// the host's trash.
///
/// Decided by agent rather than by the provider's own `deletion`, which would
/// read this machine's disk for a session that is not on it.
fn deletes_through_cli(agent: AgentKind) -> bool {
    matches!(agent, AgentKind::OpenCode)
}

/// The body of a remote delete's confirmation, and its confirm button.
///
/// Worded per agent from the row's own paths, to name everything the host's
/// provider takes rather than only the transcript: Claude's subagent folder
/// beside it, and Copilot's whole session folder.
pub(crate) fn remote_delete_prompt(session: &SessionSummary) -> (String, &'static str) {
    let agent_label = session.agent.label();
    if deletes_through_cli(session.agent) {
        return (
            format!(
                "{agent_label}'s own store on the remote host holds this session. Deleting \
                 it asks {agent_label} there to remove it. This cannot be undone."
            ),
            "Delete",
        );
    }
    let log = session
        .log_path
        .as_ref()
        .map(|path| path.display().to_string());
    let what = match (session.agent, log) {
        (AgentKind::Claude, Some(log)) => format!("{log} and its subagent folder"),
        (AgentKind::Copilot, Some(_)) => match session.log_path.as_deref().and_then(Path::parent) {
            Some(folder) => format!("The session folder {}", folder.display()),
            None => "The session folder".to_string(),
        },
        (AgentKind::Copilot, None) => "The session folder".to_string(),
        (_, Some(log)) => log,
        (_, None) => "Its files".to_string(),
    };
    (
        format!(
            "{what} will move to the trash on the remote host ({}).",
            format_bytes(session.log_bytes)
        ),
        "Move to Trash",
    )
}

/// The remote half of [`delete_session`]: asks, then has the host delete the
/// session by its id. The host finds the session in its own store and decides
/// what to take, so nothing here names a path to it.
///
/// `delete` is the request to the host, taken as a function so the outcomes
/// can be driven without a connection.
pub(crate) fn delete_on_remote_host(
    workspace: &Entity<workspace::Workspace>,
    store: Entity<crate::SessionStore>,
    session: &SessionSummary,
    delete: impl FnOnce(AgentKind, Arc<str>, &mut App) -> Task<anyhow::Result<WireDeleteOutcome>>
    + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let agent_label = session.agent.label();
    let (body, confirm) = remote_delete_prompt(session);
    let detail = with_running_note(
        format!("{}\n\n{body}", session.title),
        running_in_tabs_note(running_in_tabs([(session.agent, &session.id)], cx), 1),
    );
    let prompt = window.prompt(
        gpui::PromptLevel::Warning,
        "Delete this session?",
        Some(&detail),
        &[confirm, "Cancel"],
        cx,
    );

    let workspace_weak = workspace.downgrade();
    let agent = session.agent;
    let id = session.id.clone();
    let title = session.title.clone();
    window
        .spawn(cx, async move |cx| {
            if prompt.await.ok() != Some(0) {
                return;
            }
            let outcome = match cx.update(|_, cx| delete(agent, id.clone(), cx)) {
                Ok(deleting) => deleting.await,
                Err(error) => Err(error),
            };
            match outcome {
                Ok(WireDeleteOutcome::Gone) => {
                    store.update(cx, |store, cx| store.forget(&id, cx));
                }
                Ok(WireDeleteOutcome::NothingToDelete) => {
                    let detail = nothing_left_detail(&title, agent_label, "on the remote host");
                    let follow_up = cx.prompt(
                        gpui::PromptLevel::Info,
                        NOTHING_LEFT_TITLE,
                        Some(&detail),
                        &["Remove From List", "Cancel"],
                    );
                    if follow_up.await.ok() == Some(0) {
                        store.update(cx, |store, cx| store.forget(&id, cx));
                    }
                }
                Ok(WireDeleteOutcome::BinaryMissing { binary }) => {
                    let message = format!(
                        "`{binary}` is not installed on the remote host, so {agent_label} \
                         cannot be asked to remove it"
                    );
                    notify_command_delete_failed(&workspace_weak, agent_label, &id, &message, cx);
                }
                Ok(WireDeleteOutcome::Failed { message }) => {
                    notify_command_delete_failed(&workspace_weak, agent_label, &id, &message, cx);
                }
                Err(error) => {
                    notify_command_delete_failed(
                        &workspace_weak,
                        agent_label,
                        &id,
                        &format!("{error:#}"),
                        cx,
                    );
                }
            }
        })
        .detach();
}

/// Type tag for the failed-command-delete toast. Paired with the session id it
/// gives one notification per session, so deleting the same session twice
/// refreshes one toast rather than stacking a second copy -- the idiom
/// `missing_binary::notify` already uses.
struct CommandDeleteFailed;

fn notify_command_delete_failed(
    workspace: &gpui::WeakEntity<workspace::Workspace>,
    agent_label: &'static str,
    id: &Arc<str>,
    message: &str,
    cx: &mut AsyncWindowContext,
) {
    let text = format!("Deleting the {agent_label} session failed: {message}");
    workspace
        .update(cx, |workspace, cx| {
            workspace.show_notification(
                NotificationId::composite::<CommandDeleteFailed>(id.clone()),
                cx,
                |cx| {
                    cx.new(|cx| {
                        workspace::notifications::simple_message_notification::MessageNotification::new(
                            text.clone(),
                            cx,
                        )
                    })
                },
            );
        })
        .log_err();
}

/// One session's worth of a bulk delete: the id to forget, and what has to
/// happen to it before it may be forgotten.
///
/// Produced by [`plan_delete_all`] and consumed by [`execute_session_deletion`].
/// A caller assembling its own bulk delete builds these from a session and the
/// `Deletion` its provider reports, and hands the `Vec` straight to
/// `execute_session_deletion` -- there is nothing else to do with one.
pub struct DeleteTarget {
    /// The session id to forget from the shared index once every path or
    /// command below is confirmed gone.
    pub id: Arc<str>,
    /// The agent that owns this session, needed to resolve the right CLI for
    /// a `Deletion::Command` target.
    pub agent: AgentKind,
    /// What this target actually takes.
    pub deletion: TargetDeletion,
}

/// What one target of a bulk delete takes.
pub enum TargetDeletion {
    /// What this machine's provider says the session owns: nothing, a set of
    /// paths to trash, or a command to run against the agent's own store. The
    /// local sweep acts on exactly this.
    Here(Deletion),
    /// A session on the project's remote host, as the confirmation counts it.
    /// The host decides what goes, so this carries nothing a sweep here could
    /// act on -- no path, no command.
    OnHost(HostDeletion),
}

impl From<Deletion> for TargetDeletion {
    fn from(deletion: Deletion) -> Self {
        Self::Here(deletion)
    }
}

/// What a remote host's delete of a session is expected to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostDeletion {
    /// Files the host moves to its trash.
    Trash,
    /// A record only the agent's CLI on the host may remove, for good.
    Command,
}

/// Everything a "delete all" will take, decided before anything is asked or
/// touched.
///
/// Built as plain data so the counts and the size in the confirmation are the
/// same numbers the sweep then acts on -- a prompt that says "12 sessions" and
/// a sweep that takes 14 is the kind of disagreement nobody notices until it
/// has already happened. A caller builds one with [`plan_delete_all`], reads
/// [`Self::trashed`]/[`Self::commanded`]/[`Self::total_bytes`] to word its own
/// confirmation (see [`delete_all_detail`]), then passes `targets` to
/// [`execute_session_deletion`] once the user agrees.
pub struct DeleteAll {
    pub targets: Vec<DeleteTarget>,
    /// Only ever grown by `Trash` targets: a `Command` session has no file this
    /// editor can stat, so it always carries `log_bytes: 0` and would otherwise
    /// inflate this total with a number nobody measured.
    pub total_bytes: u64,
}

impl DeleteAll {
    /// How many sessions the plan will take in total, trash and command
    /// targets combined.
    pub fn count(&self) -> usize {
        self.targets.len()
    }

    /// Whether the plan takes nothing at all -- every in-scope session
    /// answered `Deletion::Nothing`. A caller checks this before prompting:
    /// there is nothing to confirm.
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// How many targets move to the OS trash and can be recovered from there.
    pub fn trashed(&self) -> usize {
        self.targets
            .iter()
            .filter(|target| {
                matches!(
                    target.deletion,
                    TargetDeletion::Here(Deletion::Trash(_))
                        | TargetDeletion::OnHost(HostDeletion::Trash)
                )
            })
            .count()
    }

    /// How many targets go through their agent's own CLI and cannot be
    /// recovered. The bulk prompt has to say so whenever this is not zero.
    pub fn commanded(&self) -> usize {
        self.targets
            .iter()
            .filter(|target| {
                matches!(
                    target.deletion,
                    TargetDeletion::Here(Deletion::Command(_))
                        | TargetDeletion::OnHost(HostDeletion::Command)
                )
            })
            .count()
    }
}

/// The sessions a bulk delete may take: every one that ran inside `roots`.
///
/// **It takes no query, and that is the point.** The panel's list is narrowed
/// twice -- by project, then by whatever is typed in the search box -- and only
/// the first narrowing belongs to a delete. A button that took "everything you
/// can currently see" would quietly mean something different depending on a
/// half-typed filter. Adding a `query` parameter here is how that regression
/// would arrive, so there is nowhere to put one. A caller outside the panel
/// passes whatever roots define its own scope -- a single checkout's path, for
/// instance -- and feeds the result straight to [`plan_delete_all`].
pub fn sessions_in_project<'a>(
    sessions: &'a [SessionSummary],
    roots: &'a [PathBuf],
) -> impl Iterator<Item = &'a SessionSummary> {
    sessions
        .iter()
        .filter(move |session| session.is_within(roots))
}

/// Turns the in-scope sessions and their deletions into the plan to act on.
///
/// A session whose provider answers `Nothing` is dropped rather than carried:
/// it has nothing on disk or in a store to take, so counting it would inflate
/// the number in the prompt and forgetting it would drop a row describing
/// something that is still there. A caller resolves each session's `Deletion`
/// through its own provider lookup (`agent_sessions::provider_for` for a
/// caller with no reason to cache providers), pairs it with the session, and
/// hands the pairs here.
pub fn plan_delete_all<'a>(
    scoped: impl IntoIterator<Item = (&'a SessionSummary, Deletion)>,
) -> DeleteAll {
    let mut targets = Vec::new();
    let mut total_bytes = 0;
    for (session, deletion) in scoped {
        if matches!(deletion, Deletion::Nothing) {
            continue;
        }
        // A `Command` session always carries `log_bytes: 0` -- there is no
        // file here for this editor to have measured -- so only a `Trash`
        // session ever moves this total. Added unconditionally because that
        // invariant is what makes doing so safe.
        total_bytes += session.log_bytes;
        targets.push(DeleteTarget {
            id: session.id.clone(),
            agent: session.agent,
            deletion: deletion.into(),
        });
    }
    DeleteAll {
        targets,
        total_bytes,
    }
}

/// [`plan_delete_all`] for sessions on a remote host, counted without reading
/// anything: this machine has none of their files.
fn plan_remote_delete_all<'a>(sessions: impl IntoIterator<Item = &'a SessionSummary>) -> DeleteAll {
    let mut targets = Vec::new();
    let mut total_bytes = 0;
    for session in sessions {
        let Some(deletion) = remote_deletion_preview(session) else {
            continue;
        };
        total_bytes += session.log_bytes;
        targets.push(DeleteTarget {
            id: session.id.clone(),
            agent: session.agent,
            deletion: TargetDeletion::OnHost(deletion),
        });
    }
    DeleteAll {
        targets,
        total_bytes,
    }
}

/// The session-history panel's own scope sentence, passed to
/// [`delete_all_detail`] as its `scope` argument. States plainly that the
/// search box narrows what is *shown*, not what a bulk delete *takes* --
/// wrong outside the panel, where there is no search box to disclaim, so a
/// caller with a different scope (a single checkout's sessions, say) passes
/// its own sentence instead of this one.
pub const PROJECT_SCOPE: &str =
    "Every session for this project goes, not just the ones the search shows.";

/// The body of the confirmation.
///
/// Branches on which kinds of session are in the plan: a `Command`
/// session is not recoverable, so a prompt that only ever said "move to the
/// trash ... can be recovered from there" would tell the user the opposite of
/// the truth the moment the plan held even one of them. The two counts are
/// never merged into one -- their consequences differ, and merging them is
/// exactly how a bulk sweep would say "recoverable" about a batch that is not.
///
/// `scope` is a sentence describing what the plan reaches -- the panel passes
/// [`PROJECT_SCOPE`], a caller with a narrower or differently-worded scope
/// passes its own. With `scope == PROJECT_SCOPE`, every branch below produces
/// exactly the string this function always has.
pub fn delete_all_detail(
    trashed: usize,
    commanded: usize,
    total_bytes: u64,
    scope: &str,
) -> String {
    // `concat!` rather than one wrapped literal in every branch below: a
    // trailing-backslash continuation only strips the next line's indentation
    // while it survives the formatter, and when it does not the spaces land
    // *inside* the string. Four of them after a blank line is an indented code
    // block in CommonMark, which would render a safety sentence as monospaced
    // output -- the irreversibility sentence is the worst one to lose to it.
    match (trashed > 0, commanded > 0) {
        (true, false) => {
            let plural = if trashed == 1 { "" } else { "s" };
            format!(
                concat!(
                    "{count} session{plural} will move to the trash ({bytes}).\n\n",
                    "{scope} They move to the OS trash and can be recovered from there.",
                ),
                count = trashed,
                plural = plural,
                bytes = format_bytes(total_bytes),
                scope = scope,
            )
        }
        (false, true) => {
            let plural = if commanded == 1 { "" } else { "s" };
            format!(
                concat!(
                    "{count} session{plural} will be deleted from their agents' own ",
                    "stores. This cannot be undone.\n\n",
                    "{scope}",
                ),
                count = commanded,
                plural = plural,
                scope = scope,
            )
        }
        (true, true) => {
            let trash_plural = if trashed == 1 { "" } else { "s" };
            let command_plural = if commanded == 1 { "" } else { "s" };
            format!(
                concat!(
                    "{trashed} session{trash_plural} will move to the trash ({bytes}) and ",
                    "can be recovered from there.\n\n",
                    "{commanded} session{command_plural} will be deleted from their ",
                    "agents' own stores. This cannot be undone.\n\n",
                    "{scope}",
                ),
                trashed = trashed,
                trash_plural = trash_plural,
                bytes = format_bytes(total_bytes),
                commanded = commanded,
                command_plural = command_plural,
                scope = scope,
            )
        }
        // `plan.is_empty()` is checked before this is ever called, so this arm
        // is unreachable in practice. Kept as a real sentence rather than a
        // panic or an `unreachable!()`: a caller that skips the empty check
        // some day gets a truthful string, not a crash.
        (false, false) => "Nothing is left to delete.".to_string(),
    }
}

/// What to do when a session owns nothing a delete could take.
///
/// Returning quietly is what this used to do, and from the outside it is
/// indistinguishable from a broken button: the user presses Delete and the row
/// sits there. Every route into it is a real state -- a transcript deleted
/// outside the editor, a store that keeps its own record of a session whose
/// files are gone -- so it is worth saying which one they are in.
///
/// The row can still go, and that is all that is on offer here: the agents'
/// stores belong to the agents, and this editor does not write to them. Said
/// plainly, because a row that reappears on the next sweep with no explanation
/// is the second half of the same confusion.
fn offer_to_drop_the_row(
    store: Entity<crate::SessionStore>,
    session: &SessionSummary,
    window: &mut Window,
    cx: &mut App,
) {
    let detail = nothing_left_detail(&session.title, session.agent.label(), "on disk");
    let prompt = window.prompt(
        gpui::PromptLevel::Info,
        NOTHING_LEFT_TITLE,
        Some(&detail),
        &["Remove From List", "Cancel"],
        cx,
    );

    let id = session.id.clone();
    cx.spawn(async move |cx| {
        if prompt.await.ok() != Some(0) {
            return;
        }
        store.update(cx, |store, cx| store.forget(&id, cx));
    })
    .detach();
}

const NOTHING_LEFT_TITLE: &str = "Nothing left to delete";

/// The body of [`offer_to_drop_the_row`]'s prompt. `place` says where nothing
/// is left: this machine's disk, or the remote host.
fn nothing_left_detail(title: &str, agent: &str, place: &str) -> String {
    format!(
        "{title}\n\nNothing this session owns is still {place}, so there is nothing to \
         move to the trash.\n\nRemoving it here only takes it off this list. It does \
         not touch {agent}'s own store, so the row comes back if {agent} still lists \
         the session."
    )
}

/// Runs a plan already agreed to: takes every `target`, then forgets the ones
/// it actually finished.
///
/// Free rather than a method on the panel, for the reason [`delete_session`]
/// is: a second surface with its own targets -- a worktree checkout, for
/// instance -- runs the identical sweep rather than reimplementing it.
///
/// Where the sweep runs follows the project. A local project's runs here,
/// through the project's `fs` and agent-server store: `Command` runs one at a
/// time (they are writers against one sqlite database in WAL mode), one
/// failure never strands the rest of the batch, and a session is forgotten
/// only once everything it owns is confirmed gone, matching the rule
/// [`delete_via_trash`] keeps for a single delete. A remote project's runs on
/// its host, one session at a time by agent and id (see
/// [`sweep_on_remote_host`]). A collaboration guest's is refused with a
/// notification. Either sweep forgets through the store rather than any
/// panel, so a window closed mid-sweep still leaves the index agreeing with
/// the stores.
///
/// The returned [`Task`] must be awaited or detached like any other -- dropped
/// early, it cancels the rest of the sweep the same way any other `Task` would.
pub fn execute_session_deletion(
    workspace: &Entity<workspace::Workspace>,
    targets: Vec<DeleteTarget>,
    cx: &mut App,
) -> Task<()> {
    let project = workspace.read(cx).project().clone();
    if project.read(cx).is_local() {
        let fs = project.read(cx).fs().clone();
        let agent_store = project.read(cx).agent_server_store().clone();
        return sweep_session_deletion(fs, agent_store, targets, cx);
    }
    let Some(remote) = project.read(cx).remote_session_host(cx) else {
        notify_shared_delete_refused(workspace, cx);
        return Task::ready(());
    };
    let store = crate::SessionStore::for_project(&project, cx);
    let sweep = sweep_on_remote_host(
        store,
        targets,
        move |agent, id, cx| remote.delete(agent, &id, cx),
        cx,
    );
    let workspace = workspace.downgrade();
    cx.spawn(async move |cx| {
        let Some(message) = sweep.await.stopped_message() else {
            return;
        };
        workspace
            .update(cx, |workspace, cx| {
                show_message(
                    workspace,
                    (
                        NotificationId::unique::<RemoteSweepStopped>(),
                        message.into(),
                    ),
                    cx,
                )
            })
            .log_err();
    })
}

/// What refusing a delete in a collaboration guest's project tells the
/// reader: its sessions are on the collaborator's machine.
pub(crate) const SHARED_DELETE_REFUSAL: &str =
    "Deleting sessions of a shared project is not supported";

fn notify_shared_delete_refused(workspace: &Entity<workspace::Workspace>, cx: &mut App) {
    workspace.update(cx, |workspace, cx| {
        show_message(workspace, named_message(SHARED_DELETE_REFUSAL), cx)
    });
}

struct RemoteSweepStopped;

/// How a sweep on a remote host went.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RemoteSweepReport {
    /// Targets there was something to ask the host about.
    pub(crate) planned: usize,
    /// Targets the host said are gone, and so were forgotten.
    pub(crate) deleted: usize,
    /// Why the sweep ended before asking about every target: the host could
    /// not be reached at all.
    pub(crate) stopped: Option<String>,
}

impl RemoteSweepReport {
    /// The one message a sweep the host stopped answering ends on.
    pub(crate) fn stopped_message(&self) -> Option<String> {
        let error = self.stopped.as_ref()?;
        Some(format!(
            "Deleted {} of {} sessions; the remote host stopped answering: {error}",
            self.deleted, self.planned
        ))
    }
}

/// The sweep behind [`execute_session_deletion`] for a remote project: each
/// target is deleted by the host, one at a time, by agent and id alone.
///
/// A session is forgotten only once the host says it is gone. An answer that
/// the host could not delete one session moves on to the next, as the local
/// sweep does; no answer at all ends the batch, since every request after it
/// would wait out the same deadline on the same dead link.
pub(crate) fn sweep_on_remote_host(
    store: Entity<crate::SessionStore>,
    targets: Vec<DeleteTarget>,
    delete: impl Fn(AgentKind, Arc<str>, &mut App) -> Task<anyhow::Result<WireDeleteOutcome>> + 'static,
    cx: &mut App,
) -> Task<RemoteSweepReport> {
    cx.spawn(async move |cx| {
        let targets: Vec<DeleteTarget> = targets
            .into_iter()
            .filter(|target| !matches!(target.deletion, TargetDeletion::Here(Deletion::Nothing)))
            .collect();
        let mut report = RemoteSweepReport {
            planned: targets.len(),
            deleted: 0,
            stopped: None,
        };
        let mut forget: HashSet<Arc<str>> = HashSet::default();
        for target in targets {
            let outcome = cx
                .update(|cx| delete(target.agent, target.id.clone(), cx))
                .await;
            match outcome {
                Ok(WireDeleteOutcome::Gone) => {
                    forget.insert(target.id);
                }
                Ok(WireDeleteOutcome::NothingToDelete) => {
                    log::info!(
                        "bulk delete: session {} has nothing left on the remote host, kept",
                        target.id
                    );
                }
                Ok(WireDeleteOutcome::BinaryMissing { binary }) => {
                    log::warn!(
                        "bulk delete: {binary} missing on the remote host, session {} kept",
                        target.id
                    );
                }
                Ok(WireDeleteOutcome::Failed { message }) => {
                    log::warn!("bulk delete: session {} failed: {message}", target.id);
                }
                Err(error) => {
                    log::warn!(
                        "bulk delete: the remote host did not answer for session {}: {error:#}",
                        target.id
                    );
                    report.stopped = Some(format!("{error:#}"));
                    break;
                }
            }
        }
        report.deleted = forget.len();
        store.update(cx, |store, cx| store.forget_many(&forget, cx));
        report
    })
}

/// What a remote session's delete is expected to take, for the bulk
/// confirmation to count, or `None` when there is nothing to ask the host.
/// Built without reading anything -- this machine has none of the session's
/// files.
///
/// Copilot's session folder is all its delete takes and is there whether or
/// not a transcript is; the host finds it. Claude's and Codex's stores own
/// nothing without their transcript.
fn remote_deletion_preview(session: &SessionSummary) -> Option<HostDeletion> {
    if deletes_through_cli(session.agent) {
        return Some(HostDeletion::Command);
    }
    match (session.agent, &session.log_path) {
        (_, Some(_)) | (AgentKind::Copilot, None) => Some(HostDeletion::Trash),
        (_, None) => None,
    }
}

/// The local sweep behind [`execute_session_deletion`], given what it acts
/// through.
pub(crate) fn sweep_session_deletion(
    fs: Arc<dyn fs::Fs>,
    agent_store: Entity<project::AgentServerStore>,
    targets: Vec<DeleteTarget>,
    cx: &mut App,
) -> Task<()> {
    let store = crate::SessionStore::global(cx);

    cx.spawn(async move |cx| {
        let executor = cx.background_executor().clone();
        let mut forget: HashSet<Arc<str>> = HashSet::default();
        for target in targets {
            // The unit of success is the session, not what it took to get
            // there: a session is forgotten only once everything it owns
            // is really gone, so a half-deleted session stays listed and
            // keeps describing the truth. The sweep never aborts early
            // either -- one failure must not strand the rest of the batch
            // behind it. Commands run sequentially and one at a time.
            match target.deletion {
                TargetDeletion::Here(Deletion::Nothing) => {}
                TargetDeletion::OnHost(_) => {
                    log::error!(
                        "bulk delete: session {} is on a remote host, nothing taken here",
                        target.id
                    );
                }
                TargetDeletion::Here(Deletion::Trash(paths)) => {
                    let mut every_path_gone = true;
                    for path in &paths {
                        let trashed = fs
                            .trash(
                                path,
                                fs::RemoveOptions {
                                    recursive: true,
                                    ignore_if_not_exists: true,
                                },
                            )
                            .await
                            .log_err();
                        if trashed.is_none() {
                            every_path_gone = false;
                        }
                    }
                    if every_path_gone {
                        forget.insert(target.id);
                    }
                }
                TargetDeletion::Here(Deletion::Command(command)) => {
                    let provider = agent_sessions::provider_for(target.agent);
                    let outcome = run_command_delete(
                        &provider,
                        &agent_store,
                        &executor,
                        target.agent,
                        &target.id,
                        &command,
                        cx,
                    )
                    .await;
                    match outcome {
                        CommandDeleteOutcome::Gone => {
                            forget.insert(target.id);
                        }
                        // A per-session toast for a sweep of two hundred is a
                        // toast storm; the rows that stay behind are the
                        // visible report, matching how a failed `fs.trash`
                        // above is handled.
                        CommandDeleteOutcome::BinaryMissing(missing) => {
                            log::warn!(
                                "bulk delete: {} missing, session {} kept",
                                missing.binary,
                                target.id
                            );
                        }
                        CommandDeleteOutcome::Failed(message) => {
                            log::warn!("bulk delete: session {} failed: {message}", target.id);
                        }
                    }
                }
            }
        }

        // Through the store handle, not the panel: a window closed mid-sweep
        // must still leave the shared index agreeing with the disk.
        store.update(cx, |store, cx| store.forget_many(&forget, cx));
    })
}

impl AgentHistoryPanel {
    /// Continue a session, or branch a new one off it.
    pub(crate) fn resume(
        &mut self,
        session: &SessionSummary,
        fork: Fork,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace().upgrade() else {
            return;
        };
        if self.remote {
            refuse_remote_resume(&workspace, cx);
            return;
        }
        resume_session(&workspace, session, fork, window, cx);
    }

    pub(crate) fn copy_resume_command(&self, session: &SessionSummary, cx: &mut App) {
        let Some(provider) = self.provider_for(session) else {
            return;
        };
        // The line a person would type, quoted for a shell, because pasting it
        // into one is exactly what it is for.
        if let Some(command) = provider.resume_command(session, Fork::Continue) {
            cx.write_to_clipboard(ClipboardItem::new_string(command.to_shell_string()));
        }
    }

    pub(crate) fn copy(&self, text: String, cx: &mut App) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// Open the transcript as an ordinary editor item.
    pub(crate) fn open_log(
        &mut self,
        session: &SessionSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = session.log_path.clone() else {
            return;
        };
        self.workspace()
            .update(cx, |workspace, cx| {
                workspace
                    .open_abs_path(path, workspace::OpenOptions::default(), window, cx)
                    .detach_and_log_err(cx);
            })
            .log_err();
    }

    pub(crate) fn reveal_log(&self, session: &SessionSummary, cx: &mut App) {
        if let Some(path) = session.log_path.clone() {
            self.reveal(&path, cx);
        }
    }

    pub(crate) fn open_working_directory(&self, session: &SessionSummary, cx: &mut App) {
        // Revealed rather than opened as a project: this panel is scoped to the
        // project already open, and swapping that out from under the user because
        // they clicked a menu entry would be a surprise.
        if self.cwd_alive(&session.cwd) {
            self.reveal(&session.cwd, cx);
        }
    }

    /// Shows `path` in the system file manager, through the project: a WSL
    /// project's paths are the distro's, and the project maps them to the
    /// `\\wsl$` share this machine can open. Callers only offer this where
    /// [`Self::can_reveal`] says the file manager can reach the path at all.
    fn reveal(&self, path: &std::path::Path, cx: &mut App) {
        if !self.can_reveal(cx) {
            return;
        }
        let Some(workspace) = self.workspace().upgrade() else {
            return;
        };
        let project = workspace.read(cx).project().clone();
        project.update(cx, |project, cx| project.reveal_path(path, cx));
    }

    /// Move a session's transcript to the OS trash, after asking.
    ///
    /// The prompt names the path and the size, because this is the one thing in
    /// the panel that takes something away. The trash rather than a delete: it is
    /// the user's own conversation, and it is recoverable from there.
    pub(crate) fn delete(
        &mut self,
        session: &SessionSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace().upgrade() else {
            return;
        };
        self.counts.remove(&session.id);
        delete_session(&workspace, session, window, cx);
    }

    /// Move every session of this project to the OS trash, after asking once.
    ///
    /// A method rather than a free function, unlike [`delete_session`]: nothing
    /// but the panel header offers this, and the sweep needs a handle back to
    /// the panel to clear its cached counts. It resolves providers through
    /// [`Self::provider_for`] rather than the free `provider_for` so N sessions
    /// cost one provider rather than N -- two of the three call
    /// `std::fs::canonicalize` on construction -- and so the tests that replace
    /// `self.providers` never read the developer's real history.
    pub(crate) fn delete_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A sweep of a few hundred transcripts is not instant, and a second
        // click during it would raise a second prompt over a list already being
        // emptied.
        if self.deleting {
            return;
        }
        let Some(workspace) = self.workspace().upgrade() else {
            return;
        };
        // Before the plan is built: see `delete_session`.
        let remote = match session_host(&workspace, cx) {
            Some(AgentSessionHost::Local(_)) => false,
            Some(AgentSessionHost::Remote(_)) => true,
            None => {
                notify_shared_delete_refused(&workspace, cx);
                return;
            }
        };

        let roots = self.project_roots(cx);
        let in_project = sessions_in_project(self.sessions(cx), &roots);
        let plan = if remote {
            plan_remote_delete_all(in_project)
        } else {
            plan_delete_all(in_project.map(|session| {
                let deletion = self
                    .provider_for(session)
                    .map(|provider| provider.deletion(session))
                    .unwrap_or(Deletion::Nothing);
                (session, deletion)
            }))
        };

        // The truthful half of the empty rule. The button is already disabled
        // when this project has no sessions, but "has sessions" and "has
        // something a delete can take" are different questions, and only this
        // one has been to the providers.
        //
        // Said rather than swallowed, for the reason `offer_to_drop_the_row`
        // gives: a button that does nothing and explains nothing reads as
        // broken. Nothing is offered here, though -- dropping a whole project's
        // rows for sessions whose files are already gone is a larger promise
        // than this button made, and each row can still be taken on its own.
        if plan.is_empty() {
            let answer = window.prompt(
                gpui::PromptLevel::Info,
                "Nothing left to delete",
                Some(
                    "None of this project's sessions still has anything on disk, so \
                     there is nothing to move to the trash.",
                ),
                &["OK"],
                cx,
            );
            cx.spawn(async move |_, _| {
                answer.await.ok();
            })
            .detach();
            return;
        }

        log::debug!(
            "delete_all: sweeping {} sessions ({} trash, {} command)",
            plan.count(),
            plan.trashed(),
            plan.commanded()
        );

        // A plan holding even one `Command` target is a plan that cannot be
        // fully recovered, so both the label and the detail text below derive
        // from the same test rather than one being updated and the other not.
        let irreversible = plan.commanded() > 0;
        let buttons: &[&str] = if irreversible {
            &["Delete", "Cancel"]
        } else {
            &["Move to Trash", "Cancel"]
        };
        let detail = with_running_note(
            delete_all_detail(
                plan.trashed(),
                plan.commanded(),
                plan.total_bytes,
                PROJECT_SCOPE,
            ),
            running_in_tabs_note(
                running_in_tabs(
                    plan.targets.iter().map(|target| (target.agent, &target.id)),
                    cx,
                ),
                plan.count(),
            ),
        );
        let prompt = window.prompt(
            gpui::PromptLevel::Warning,
            "Delete all history for this project?",
            Some(&detail),
            buttons,
            cx,
        );

        let targets = plan.targets;

        cx.spawn(async move |this, cx| {
            if prompt.await.ok() != Some(0) {
                return;
            }
            let deletion = this.update(cx, |this, cx| {
                this.deleting = true;
                cx.notify();
                execute_session_deletion(&workspace, targets, cx)
            });
            let Ok(deletion) = deletion else {
                return;
            };
            deletion.await;

            this.update(cx, |this, cx| {
                this.counts.clear();
                this.deleting = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Whether a fork is on offer for this session's agent. Claude has
    /// `--fork-session`; Codex has nothing equivalent, so the control is disabled
    /// rather than drawn as if it worked.
    pub(crate) fn can_fork(&self, session: &SessionSummary) -> bool {
        self.provider_for(session)
            .and_then(|provider: Arc<dyn SessionProvider>| {
                provider.resume_command(session, Fork::New)
            })
            .is_some()
    }
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    const MB: f64 = 1024. * 1024.;
    const KB: f64 = 1024.;
    let bytes = bytes as f64;
    if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_sessions::AgentKind;
    use std::path::Path;
    use std::time::SystemTime;

    fn session(id: &str, cwd: &str, log_bytes: u64) -> SessionSummary {
        SessionSummary {
            id: Arc::from(id),
            agent: AgentKind::Claude,
            title: id.to_string(),
            preview: String::new(),
            preview_speaker: None,
            cwd: PathBuf::from(cwd),
            branch: None,
            model: None,
            updated_at: SystemTime::UNIX_EPOCH,
            log_path: None,
            log_bytes,
        }
    }

    fn session_of(agent: AgentKind, cwd: &Path) -> SessionSummary {
        SessionSummary {
            agent,
            ..session("s", &cwd.to_string_lossy(), 0)
        }
    }

    #[test]
    fn continuing_tracks_the_session_and_forking_branches_off_it_without_tracking_it() {
        let session = session_of(AgentKind::Claude, &std::env::temp_dir());
        assert_eq!(
            intent_for_resume(&session, Fork::Continue),
            SessionIntent::Tracked("s".into())
        );
        assert_eq!(
            intent_for_resume(&session, Fork::New),
            SessionIntent::Fork("s".into())
        );
    }

    #[test]
    fn a_session_that_can_be_resumed_has_no_refusal() {
        let here = std::env::temp_dir();
        for fork in [Fork::Continue, Fork::New] {
            assert_eq!(
                resume_refusal(&session_of(AgentKind::Claude, &here), fork),
                None,
                "{fork:?}"
            );
        }
        assert_eq!(
            resume_refusal(&session_of(AgentKind::Codex, &here), Fork::Continue),
            None
        );
    }

    #[test]
    fn an_agent_with_no_fork_refuses_one_even_where_its_directory_is_fine() {
        let here = std::env::temp_dir();
        for agent in [AgentKind::Codex, AgentKind::Copilot] {
            assert_eq!(
                resume_refusal(&session_of(agent, &here), Fork::New),
                Some(ResumeRefusal::Unsupported),
                "{agent:?}"
            );
        }
    }

    #[test]
    fn a_directory_that_is_gone_or_is_not_a_local_path_refuses_the_resume() {
        let gone = std::env::temp_dir().join("agent-ui-resume-refusal-no-such-directory");
        for cwd in [gone.as_path(), Path::new("relative/dir"), Path::new("")] {
            assert_eq!(
                resume_refusal(&session_of(AgentKind::Claude, cwd), Fork::Continue),
                Some(ResumeRefusal::CwdGone),
                "{cwd:?}"
            );
        }
    }

    #[test]
    fn an_unsupported_fork_is_reported_before_a_missing_directory() {
        let gone = std::env::temp_dir().join("agent-ui-resume-refusal-no-such-directory");
        assert_eq!(
            resume_refusal(&session_of(AgentKind::Codex, &gone), Fork::New),
            Some(ResumeRefusal::Unsupported)
        );
    }

    fn selected(sessions: &[SessionSummary], roots: &[PathBuf]) -> Vec<String> {
        sessions_in_project(sessions, roots)
            .map(|session| session.id.to_string())
            .collect()
    }

    #[test]
    fn sizes_read_the_way_a_person_would_say_them() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(900), "900 B");
        assert_eq!(format_bytes(2048), "2 KB");
        assert_eq!(format_bytes(13 * 1024 * 1024), "13.0 MB");
    }

    #[test]
    fn only_this_projects_sessions_are_selected() {
        let sessions = vec![
            session("a", "/root", 0),
            session("b", "/root", 0),
            session("c", "/other", 0),
            session("d", "/root", 0),
        ];
        assert_eq!(
            selected(&sessions, &[PathBuf::from("/root")]),
            vec!["a", "b", "d"]
        );
    }

    /// A window with no folder open has no project, so a delete scoped to "this
    /// project" must take nothing at all -- not fall through to everything.
    #[test]
    fn a_workspace_with_no_roots_selects_nothing() {
        let sessions = vec![session("a", "/root", 0), session("b", "/other", 0)];
        assert!(selected(&sessions, &[]).is_empty());
    }

    #[test]
    fn a_session_in_a_subdirectory_of_a_root_counts() {
        let sessions = vec![session("deep", "/root/crates/ui", 0)];
        assert_eq!(selected(&sessions, &[PathBuf::from("/root")]), vec!["deep"]);
    }

    /// The scope-drift guard. `sessions_in_project` has no query parameter, so a
    /// session the search box would hide is still selected. If someone ever adds
    /// one, this stops compiling or stops passing.
    #[test]
    fn the_search_filter_cannot_narrow_the_selection() {
        let sessions = vec![
            SessionSummary {
                title: "nothing anyone would type".to_string(),
                ..session("hidden", "/root", 0)
            },
            session("visible", "/root", 0),
        ];
        assert_eq!(
            selected(&sessions, &[PathBuf::from("/root")]),
            vec!["hidden", "visible"],
            "selection is by project alone; the filter has no say in it"
        );
    }

    fn command(program: &str) -> AgentCommand {
        AgentCommand {
            program: program.to_string(),
            args: vec!["session".to_string(), "delete".to_string()],
            cwd: PathBuf::from("/root"),
        }
    }

    #[test]
    fn sessions_with_nothing_to_take_are_not_planned() {
        let with_files = session("keeps", "/root", 100);
        let without = session("empty", "/root", 8_000);
        let plan = plan_delete_all(vec![
            (
                &with_files,
                Deletion::Trash(vec![PathBuf::from("/logs/keeps.jsonl")]),
            ),
            (&without, Deletion::Nothing),
        ]);

        assert_eq!(plan.count(), 1);
        assert_eq!(plan.targets[0].id.as_ref(), "keeps");
        assert_eq!(
            plan.total_bytes, 100,
            "a session with no files owns no bytes to take"
        );
    }

    #[test]
    fn the_plan_tallies_only_what_it_will_trash() {
        let first = session("first", "/root", 1024);
        let second = session("second", "/root", 1024);
        let huge_but_pathless = session("huge", "/root", 8 * 1024 * 1024);
        let plan = plan_delete_all(vec![
            (&first, Deletion::Trash(vec![PathBuf::from("/logs/first")])),
            (
                &second,
                Deletion::Trash(vec![PathBuf::from("/logs/second")]),
            ),
            (&huge_but_pathless, Deletion::Nothing),
        ]);

        assert_eq!(plan.count(), 2);
        assert_eq!(plan.total_bytes, 2048);
        assert!(!plan.is_empty());
    }

    /// A `Command` target is kept, never dropped like `Nothing` is, and never
    /// taxed for bytes it does not carry (`log_bytes` is 0 for one anyway, but
    /// the plan must not assume that -- it must simply never add it).
    #[test]
    fn a_command_target_is_kept_and_counted_separately_from_trash() {
        let trashed = session("trashed", "/root", 500);
        let commanded = session("commanded", "/root", 0);
        let plan = plan_delete_all(vec![
            (
                &trashed,
                Deletion::Trash(vec![PathBuf::from("/logs/trashed.jsonl")]),
            ),
            (&commanded, Deletion::Command(command("opencode"))),
        ]);

        assert_eq!(plan.count(), 2);
        assert_eq!(plan.trashed(), 1);
        assert_eq!(plan.commanded(), 1);
        assert_eq!(
            plan.total_bytes, 500,
            "a Command target must not inflate the trash byte total"
        );
    }

    /// C1's mechanism: the bulk button label is derived from `commanded() > 0`,
    /// so a plan holding even one irreversible target must report it.
    #[test]
    fn commanded_reports_true_only_when_the_plan_holds_one() {
        let trash_only = session("t", "/root", 10);
        let plan = plan_delete_all(vec![(
            &trash_only,
            Deletion::Trash(vec![PathBuf::from("/logs/t.jsonl")]),
        )]);
        assert_eq!(
            plan.commanded(),
            0,
            "an all-Trash plan is fully recoverable"
        );

        let commanded_session = session("c", "/root", 0);
        let mixed = plan_delete_all(vec![
            (
                &trash_only,
                Deletion::Trash(vec![PathBuf::from("/logs/t.jsonl")]),
            ),
            (&commanded_session, Deletion::Command(command("opencode"))),
        ]);
        assert!(
            mixed.commanded() > 0,
            "one Command target is enough to make the whole plan irreversible"
        );
    }

    /// The detail is rendered as markdown, and CommonMark turns any line
    /// indented four spaces or more after a blank line into a code block. A
    /// wrapped string literal is exactly how those spaces get in -- the safety
    /// sentence would then be shown monospaced, reading like output rather than
    /// like a warning. `contains` assertions cannot see this, so the shape of
    /// every line is checked directly, in every branch.
    #[test]
    fn the_prompt_is_prose_not_an_accidental_code_block() {
        for detail in [
            delete_all_detail(3, 0, 4096, PROJECT_SCOPE),
            delete_all_detail(0, 3, 0, PROJECT_SCOPE),
            delete_all_detail(3, 3, 4096, PROJECT_SCOPE),
        ] {
            for line in detail.lines() {
                assert!(
                    !line.starts_with("    "),
                    "a line indented four spaces renders as a code block: {line:?}"
                );
            }
        }
    }

    /// The trashed-only branch is the function's original text -- unchanged
    /// down to the byte, so the common case reads exactly as it always has.
    #[test]
    fn the_trashed_only_prompt_is_unchanged() {
        let one = delete_all_detail(1, 0, 1024, PROJECT_SCOPE);
        assert!(one.contains("1 session will"), "singular, got: {one}");
        assert!(one.contains("1 KB"), "got: {one}");

        let many = delete_all_detail(42, 0, 13 * 1024 * 1024, PROJECT_SCOPE);
        assert!(many.contains("42 sessions will"), "plural, got: {many}");
        assert!(many.contains("13.0 MB"), "got: {many}");

        for detail in [&one, &many] {
            assert!(
                detail.contains("not just the ones the search shows"),
                "the prompt must say the filter is ignored, got: {detail}"
            );
            assert!(
                detail.contains("recovered"),
                "the prompt must say the files are recoverable, got: {detail}"
            );
        }
    }

    /// The commanded-only branch: no byte figure (there is none to report),
    /// and an explicit, unambiguous statement that this cannot be undone.
    #[test]
    fn the_commanded_only_prompt_carries_no_byte_figure_and_says_it_cannot_be_undone() {
        let one = delete_all_detail(0, 1, 0, PROJECT_SCOPE);
        assert!(one.contains("1 session will"), "singular, got: {one}");
        assert!(
            one.contains("cannot be undone"),
            "an irreversible-only plan must say so, got: {one}"
        );
        assert!(
            !one.contains(" B)") && !one.contains(" KB)") && !one.contains(" MB)"),
            "there is no byte figure to report for a Command-only plan, got: {one}"
        );

        let many = delete_all_detail(0, 7, 0, PROJECT_SCOPE);
        assert!(many.contains("7 sessions will"), "plural, got: {many}");
    }

    /// The mixed branch: both counts appear, stated separately -- never one
    /// merged total, which is exactly how C1's scenario happens.
    #[test]
    fn the_mixed_prompt_states_both_counts_separately() {
        let detail = delete_all_detail(3, 40, 40 * 1024, PROJECT_SCOPE);
        assert!(
            detail.contains("3 session") && detail.contains("40 session"),
            "both counts must appear on their own, got: {detail}"
        );
        assert!(
            detail.contains("40 KB"),
            "the trash byte figure must still be reported, got: {detail}"
        );
        assert!(
            detail.contains("cannot be undone"),
            "the irreversible half must still say so, got: {detail}"
        );
        assert!(
            !detail.contains("43 session"),
            "the two counts must never be merged into one total, got: {detail}"
        );
    }

    /// The parameterisation itself: a caller with its own scope sentence sees
    /// that sentence in the output and never `PROJECT_SCOPE`'s -- proven in
    /// both a branch that has a scope sentence on its own line (mixed) and
    /// one that folds it into the middle of a paragraph (trashed-only).
    #[test]
    fn a_caller_scope_replaces_the_project_sentence() {
        const WORKTREE_SCOPE: &str = "Every session under this checkout goes.";

        let trashed_only = delete_all_detail(1, 0, 1024, WORKTREE_SCOPE);
        assert!(
            trashed_only.contains(WORKTREE_SCOPE),
            "the caller's own scope sentence must appear, got: {trashed_only}"
        );
        assert!(
            !trashed_only.contains(PROJECT_SCOPE),
            "the panel's scope sentence must not leak into another caller's \
             prompt, got: {trashed_only}"
        );

        let mixed = delete_all_detail(3, 40, 40 * 1024, WORKTREE_SCOPE);
        assert!(mixed.contains(WORKTREE_SCOPE), "got: {mixed}");
        assert!(!mixed.contains(PROJECT_SCOPE), "got: {mixed}");
    }

    /// A remote bulk delete is counted without reading anything: opencode's
    /// sessions as irreversible, a transcript as trash with its size, a
    /// Copilot session as its folder whether or not it has a transcript, and a
    /// Codex session with no rollout as nothing to take.
    #[test]
    fn a_remote_plan_counts_what_the_host_will_take_without_reading_it() {
        let transcript = SessionSummary {
            log_path: Some(PathBuf::from("/home/u/.claude/projects/-p/a.jsonl")),
            ..session("a", "/home/u/p", 2048)
        };
        let opencode = SessionSummary {
            agent: AgentKind::OpenCode,
            ..session("b", "/home/u/p", 0)
        };
        let no_files = SessionSummary {
            agent: AgentKind::Codex,
            ..session("c", "/home/u/p", 0)
        };
        let copilot_without_transcript = SessionSummary {
            agent: AgentKind::Copilot,
            ..session("d", "/home/u/p", 0)
        };

        let plan = plan_remote_delete_all([
            &transcript,
            &opencode,
            &no_files,
            &copilot_without_transcript,
        ]);

        let planned: Vec<&str> = plan
            .targets
            .iter()
            .map(|target| target.id.as_ref())
            .collect();
        assert_eq!(planned, vec!["a", "b", "d"]);
        assert_eq!(plan.trashed(), 2);
        assert_eq!(plan.commanded(), 1);
        assert_eq!(plan.total_bytes, 2048);
        assert!(
            plan.targets
                .iter()
                .all(|target| matches!(target.deletion, TargetDeletion::OnHost(_))),
            "a remote plan carries nothing a sweep here could act on"
        );
    }

    /// The prompt names what the host's provider will take, not only the
    /// transcript: Claude's subagent folder goes with it, and Copilot's whole
    /// session folder.
    #[test]
    fn a_remote_delete_prompt_names_everything_the_host_takes() {
        let with_log = |agent: AgentKind, log: &str| SessionSummary {
            agent,
            log_path: Some(PathBuf::from(log)),
            ..session("s", "/home/u/p", 2048)
        };

        let (claude, confirm) = remote_delete_prompt(&with_log(
            AgentKind::Claude,
            "/home/u/.claude/projects/-p/s.jsonl",
        ));
        assert_eq!(
            claude,
            "/home/u/.claude/projects/-p/s.jsonl and its subagent folder will move to the \
             trash on the remote host (2 KB)."
        );
        assert_eq!(confirm, "Move to Trash");

        let (copilot, _) = remote_delete_prompt(&with_log(
            AgentKind::Copilot,
            "/home/u/.copilot/session-state/abc/events.jsonl",
        ));
        assert!(
            copilot.starts_with("The session folder /home/u/.copilot/session-state/abc will move"),
            "{copilot}"
        );
        let (copilot_without_log, _) = remote_delete_prompt(&SessionSummary {
            agent: AgentKind::Copilot,
            ..session("s", "/home/u/p", 0)
        });
        assert!(
            copilot_without_log.starts_with("The session folder will move"),
            "{copilot_without_log}"
        );

        let (codex, _) = remote_delete_prompt(&with_log(
            AgentKind::Codex,
            "/home/u/.codex/sessions/rollout-s.jsonl",
        ));
        assert!(
            codex.starts_with("/home/u/.codex/sessions/rollout-s.jsonl will move"),
            "{codex}"
        );

        let (opencode, confirm) = remote_delete_prompt(&SessionSummary {
            agent: AgentKind::OpenCode,
            ..session("s", "/home/u/p", 0)
        });
        assert!(opencode.contains("cannot be undone"), "{opencode}");
        assert_eq!(confirm, "Delete");
    }
}
