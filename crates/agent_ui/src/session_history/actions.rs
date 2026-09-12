use crate::{AgentView, agent_view::SessionIntent, session_history::panel::AgentHistoryPanel};
use agent_sessions::{AgentCommand, AgentKind, Deletion, Fork, SessionProvider, SessionSummary};
use futures::{FutureExt as _, select_biased};
use gpui::{App, AppContext as _, AsyncWindowContext, ClipboardItem, Context, Entity, Window};
use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use util::ResultExt as _;
use workspace::notifications::NotificationId;

/// How long a single `Deletion::Command` invocation may run before the delete
/// gives up on it and treats it as a failure (M6). A wedged CLI call must not
/// stall every trash and delete queued behind it in the same sequential sweep.
const COMMAND_DELETE_TIMEOUT: Duration = Duration::from_secs(10);

/// Whether the directory a session ran in is still there.
///
/// The row's badge and every control that would run something in that directory
/// read this. A resume into a directory that no longer exists would start the CLI
/// in a place with none of the files the conversation is about.
pub(crate) fn cwd_exists(session: &SessionSummary) -> bool {
    is_local_absolute_path(&session.cwd) && session.cwd.is_dir()
}

/// M2 -- every provider's `cwd` is untrusted data (it is read from a store this
/// editor does not own), and it reaches an `is_dir()` call here, plus, for a
/// `Deletion::Command`, a subprocess `current_dir` in
/// [`run_resolved_command_delete`]. On Windows, merely stat-ing a UNC path
/// (`\\host\share`) is itself a side effect -- it can trigger an outbound SMB
/// authentication attempt -- so the shape has to be rejected *before* any
/// `is_dir()` call, not after. The minimum this rejects: anything empty,
/// relative, or -- on Windows only -- not an ordinary local drive path.
fn is_local_absolute_path(path: &Path) -> bool {
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return false;
    }
    #[cfg(windows)]
    {
        matches!(
            path.components().next(),
            Some(std::path::Component::Prefix(prefix))
                if matches!(
                    prefix.kind(),
                    std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
                )
        )
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Opens a tab back on `session`, or branches a new one off it.
///
/// Free rather than a method on the history panel: the sidebar reaches the same
/// sessions through the shared session index, and the rules about what may be
/// resumed -- the agent must support it, the working directory must still exist,
/// a fork must not carry the id it forked from -- belong to the operation, not
/// to whichever surface asked for it.
pub fn resume_session(
    workspace: &Entity<workspace::Workspace>,
    session: &SessionSummary,
    fork: Fork,
    window: &mut Window,
    cx: &mut App,
) {
    let provider = agent_sessions::provider_for(session.agent);
    // Asked only so the control stays disabled where the agent cannot honour
    // it — Codex has no fork. The command itself is rebuilt at spawn time from
    // the id, so what comes back here is discarded.
    if provider.resume_command(session, fork).is_none() {
        return;
    }
    if !cwd_exists(session) {
        return;
    }
    let agent = session.agent.builtin_agent_id();
    // A fork is deliberately NOT tracked. `--fork-session` makes the CLI mint
    // a *new* id, so a tab carrying the id we resumed from would come back on
    // the original conversation rather than the fork — the one failure this
    // whole feature exists to avoid, and silent. Until a flag exists to name a
    // fork's id, a forked tab is an untracked tab.
    let intent = match fork {
        Fork::Continue => SessionIntent::Tracked(session.id.to_string().into()),
        Fork::New => SessionIntent::Untracked,
    };
    // `Fork::New` is already `Untracked`, and `SessionOrigin::new` drops the
    // title for an untracked tab -- so a fork does not inherit the name of the
    // conversation it branched from without a second branch here.
    let origin = crate::SessionOrigin::new(intent, Some(session.title.as_str()));
    workspace.update(cx, |workspace, cx| {
        AgentView::open_tracked(workspace, agent, origin, window, cx);
    });
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
    let provider = agent_sessions::provider_for(session.agent);
    match provider.deletion(session) {
        Deletion::Nothing => offer_to_drop_the_row(session, window, cx),
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

    let detail = format!(
        "{}\n\n{} will move to the trash ({}).",
        session.title,
        paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n"),
        format_bytes(session.log_bytes)
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
    let detail = format!(
        "{}\n\n{agent_label}'s own store holds this session, not a file this editor \
         can move to the trash. Deleting it asks {agent_label} to remove it there. \
         This cannot be undone.",
        session.title
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

/// What running a `Deletion::Command` once found out.
#[derive(Debug)]
enum CommandDeleteOutcome {
    /// The session is gone -- either the command exited zero, or it did not,
    /// but the store no longer holds the id either way (see
    /// [`run_command_delete`]): the goal was already met.
    Gone,
    /// The CLI that owns this session is not on this machine. Kept distinct
    /// from [`Self::Failed`] because a missing binary must not be offered the
    /// same way a `Trash` failure is -- see H3 in the phase notes.
    BinaryMissing(project::AgentBinaryMissing),
    /// The command ran, or tried to, and failed, and the store still holds
    /// the session. Carries text already stripped of control characters, fit
    /// to put in a toast.
    Failed(String),
}

/// Runs one `Deletion::Command`, resolving the binary through
/// `AgentServerStore` -- never the advisory `command.program` -- and racing it
/// against [`COMMAND_DELETE_TIMEOUT`] (M6) so one wedged invocation cannot stall
/// a sequential sweep.
///
/// A non-zero exit is not treated as final: `opencode session delete` on an id
/// already gone exits 1, so a failure re-checks `provider.find` and only keeps
/// the row if the store still holds the session (C2/H9) -- the store is the
/// authority, not the exit code.
///
/// Generic over the context so the same logic serves both the single delete
/// (`AsyncWindowContext`, for the missing-binary prompt) and the bulk sweep
/// (`AsyncApp`, which never prompts mid-sweep).
async fn run_command_delete<C: gpui::AppContext>(
    provider: &Arc<dyn SessionProvider>,
    agent_store: &Entity<project::AgentServerStore>,
    executor: &gpui::BackgroundExecutor,
    agent: AgentKind,
    id: &Arc<str>,
    command: &AgentCommand,
    cx: &mut C,
) -> CommandDeleteOutcome {
    let agent_id = project::AgentId::new(agent.builtin_agent_id());
    let resolve = agent_store.update(cx, |store, cx| store.resolve_agent_binary(&agent_id, cx));
    let binary = match resolve.await {
        Ok(project::AgentBinary::Found(path)) => path,
        Ok(project::AgentBinary::Missing(missing)) => {
            return CommandDeleteOutcome::BinaryMissing(missing);
        }
        Err(error) => return CommandDeleteOutcome::Failed(error.to_string()),
    };
    run_resolved_command_delete(provider, executor, &binary, id, command, cx).await
}

/// The half of [`run_command_delete`] that runs once a binary is already in
/// hand: builds the real subprocess and races it through [`race_command_delete`].
///
/// No `--` argv separator is inserted here, even though an earlier draft of
/// this mechanism called for one. Verified against opencode's real CLI (the
/// only agent that reaches this path today): a `--` placed before its
/// `session`/`delete` subcommand words stops them being recognised at all,
/// falling through to opencode's default action -- which launches its
/// interactive TUI as a detached child process. That is a strictly worse
/// outcome than the wedged call M6's timeout exists to survive, since it
/// would happen on *every* delete rather than an occasional stuck one. A
/// provider whose id shape needs protecting from being read as a flag (H1)
/// carries its own guard in its own `args`, at the position that actually
/// works for its CLI; this spawn site trusts `command.args` verbatim.
async fn run_resolved_command_delete<C: gpui::AppContext>(
    provider: &Arc<dyn SessionProvider>,
    executor: &gpui::BackgroundExecutor,
    binary: &std::path::Path,
    id: &Arc<str>,
    command: &AgentCommand,
    cx: &mut C,
) -> CommandDeleteOutcome {
    let mut process = util::command::Command::new(binary);
    process.args(command.args.iter());
    // M2: the same shape guard `cwd_exists` applies, and for the same reason
    // -- `command.cwd` is untrusted data reaching a subprocess `current_dir`,
    // and on Windows merely stat-ing a UNC path is itself a network side
    // effect, so the check has to run before `is_dir()` rather than after.
    if is_local_absolute_path(&command.cwd) && command.cwd.is_dir() {
        process.current_dir(&command.cwd);
    }
    race_command_delete(provider, executor, id, process.output(), cx).await
}

/// Races one command-delete attempt against [`COMMAND_DELETE_TIMEOUT`] (M6),
/// then resolves the outcome -- checking back with `provider.find` when the
/// attempt failed or ran out of time (C2/H9), since a wedged or non-zero exit
/// for a session the store no longer holds means the goal was already met.
///
/// Generic over the future that actually produces the process's output, which
/// is what makes this independently testable: a real `smol::process` child
/// plus its own OS reactor thread does not compose safely with
/// `#[gpui::test]`'s deterministic dispatcher (phase 04 hit two different
/// failure modes -- a wrong outcome once, a panic inside `blocking::Executor`'s
/// thread pool the next -- trying exactly that). Passing the run as a plain
/// `Future` lets a test drive the timeout and the idempotency re-check with a
/// value built by hand (`std::future::ready`/`std::future::pending`), with no
/// subprocess and no reactor involved, so M6 and C2/H9 get a real test instead
/// of staying implemented-but-unproven a second time.
async fn race_command_delete<C: gpui::AppContext>(
    provider: &Arc<dyn SessionProvider>,
    executor: &gpui::BackgroundExecutor,
    id: &Arc<str>,
    run: impl Future<Output = std::io::Result<std::process::Output>>,
    cx: &mut C,
) -> CommandDeleteOutcome {
    let spawned = select_biased! {
        output = run.fuse() => Some(output),
        _ = executor.timer(COMMAND_DELETE_TIMEOUT).fuse() => None,
    };

    if let Some(Ok(output)) = &spawned {
        if output.status.success() {
            return CommandDeleteOutcome::Gone;
        }
    }

    let recheck_provider = provider.clone();
    let recheck_id = id.to_string();
    let still_there = cx
        .background_spawn(async move { recheck_provider.find(&recheck_id) })
        .await;
    match still_there {
        Ok(None) => CommandDeleteOutcome::Gone,
        Ok(Some(_)) | Err(_) => {
            let message = match spawned {
                Some(Ok(output)) => {
                    let stderr = strip_ansi_first_line(&output.stderr);
                    if stderr.is_empty() {
                        format!("exited with {}", output.status)
                    } else {
                        stderr
                    }
                }
                Some(Err(error)) => error.to_string(),
                None => format!("timed out after {}s", COMMAND_DELETE_TIMEOUT.as_secs()),
            };
            CommandDeleteOutcome::Failed(message)
        }
    }
}

/// Strips ANSI CSI escape sequences from `bytes`, then returns the first
/// non-empty line of what remains.
///
/// "Trim control characters" is not enough (M3): that removes the `ESC`
/// (`0x1B`) byte alone and leaves e.g. `[91m` behind as ordinary printable
/// text -- exactly the garbage a strip exists to prevent. A CSI sequence is
/// `ESC` `[`, then any number of bytes, ending at the first one in
/// `0x40..=0x7E` (its "final byte"); this scans for that shape and drops the
/// whole run, `ESC` included.
fn strip_ansi_first_line(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    let mut iter = bytes.iter().copied().peekable();
    while let Some(byte) = iter.next() {
        if byte == 0x1B && iter.peek() == Some(&b'[') {
            iter.next();
            for next in iter.by_ref() {
                if (0x40..=0x7E).contains(&next) {
                    break;
                }
            }
            continue;
        }
        out.push(byte);
    }
    String::from_utf8_lossy(&out)
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .to_string()
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
pub(crate) struct DeleteTarget {
    pub(crate) id: Arc<str>,
    pub(crate) agent: AgentKind,
    pub(crate) deletion: Deletion,
}

/// Everything a "delete all" will take, decided before anything is asked or
/// touched.
///
/// Built as plain data so the counts and the size in the confirmation are the
/// same numbers the sweep then acts on -- a prompt that says "12 sessions" and
/// a sweep that takes 14 is the kind of disagreement nobody notices until it
/// has already happened.
pub(crate) struct DeleteAll {
    pub(crate) targets: Vec<DeleteTarget>,
    /// Only ever grown by `Trash` targets: a `Command` session has no file this
    /// editor can stat, so it always carries `log_bytes: 0` and would otherwise
    /// inflate this total with a number nobody measured.
    pub(crate) total_bytes: u64,
}

impl DeleteAll {
    pub(crate) fn count(&self) -> usize {
        self.targets.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// How many targets move to the OS trash and can be recovered from there.
    pub(crate) fn trashed(&self) -> usize {
        self.targets
            .iter()
            .filter(|target| matches!(target.deletion, Deletion::Trash(_)))
            .count()
    }

    /// How many targets go through their agent's own CLI and cannot be
    /// recovered. C1 exists because this number used to be invisible to the
    /// bulk prompt.
    pub(crate) fn commanded(&self) -> usize {
        self.targets
            .iter()
            .filter(|target| matches!(target.deletion, Deletion::Command(_)))
            .count()
    }
}

/// The sessions a bulk delete may take: every one that ran inside this
/// workspace's roots.
///
/// **It takes no query, and that is the point.** The panel's list is narrowed
/// twice -- by project, then by whatever is typed in the search box -- and only
/// the first narrowing belongs to a delete. A button that took "everything you
/// can currently see" would quietly mean something different depending on a
/// half-typed filter. Adding a `query` parameter here is how that regression
/// would arrive, so there is nowhere to put one.
pub(crate) fn sessions_in_project<'a>(
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
/// something that is still there.
pub(crate) fn plan_delete_all<'a>(
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
            deletion,
        });
    }
    DeleteAll {
        targets,
        total_bytes,
    }
}

/// The body of the confirmation.
///
/// Branches on which kinds of session are in the plan (C1): a `Command`
/// session is not recoverable, so a prompt that only ever said "move to the
/// trash ... can be recovered from there" would tell the user the opposite of
/// the truth the moment the plan held even one of them. The two counts are
/// never merged into one -- their consequences differ, and merging them is
/// exactly how a bulk sweep would say "recoverable" about a batch that is not.
///
/// The trashed-only branch is byte-for-byte the sentence this function always
/// used to produce, so the common case reads exactly as it did before this
/// phase.
pub(crate) fn delete_all_detail(trashed: usize, commanded: usize, total_bytes: u64) -> String {
    const SCOPE_SENTENCE: &str =
        "Every session for this project goes, not just the ones the search shows.";
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
                    "Every session for this project goes, not just the ones the search ",
                    "shows. They move to the OS trash and can be recovered from there.",
                ),
                count = trashed,
                plural = plural,
                bytes = format_bytes(total_bytes),
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
                scope = SCOPE_SENTENCE,
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
                scope = SCOPE_SENTENCE,
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
fn offer_to_drop_the_row(session: &SessionSummary, window: &mut Window, cx: &mut App) {
    let agent = session.agent.label();
    let detail = format!(
        "{}\n\nNothing this session owns is still on disk, so there is nothing to \
         move to the trash.\n\nRemoving it here only takes it off this list. It does \
         not touch {agent}'s own store, so the row comes back if {agent} still lists \
         the session.",
        session.title
    );
    let prompt = window.prompt(
        gpui::PromptLevel::Info,
        "Nothing left to delete",
        Some(&detail),
        &["Remove From List", "Cancel"],
        cx,
    );

    let store = crate::SessionStore::global(cx);
    let id = session.id.clone();
    cx.spawn(async move |cx| {
        if prompt.await.ok() != Some(0) {
            return;
        }
        store.update(cx, |store, cx| store.forget(&id, cx));
    })
    .detach();
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
            cx.reveal_path(&path);
        }
    }

    pub(crate) fn open_working_directory(&self, session: &SessionSummary, cx: &mut App) {
        // Revealed rather than opened as a project: this panel is scoped to the
        // project already open, and swapping that out from under the user because
        // they clicked a menu entry would be a surprise.
        if cwd_exists(session) {
            cx.reveal_path(&session.cwd);
        }
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

        let roots = self.project_roots(cx);
        let scoped: Vec<(&SessionSummary, Deletion)> =
            sessions_in_project(self.sessions(cx), &roots)
                .map(|session| {
                    let deletion = self
                        .provider_for(session)
                        .map(|provider| provider.deletion(session))
                        .unwrap_or(Deletion::Nothing);
                    (session, deletion)
                })
                .collect();
        let plan = plan_delete_all(scoped);

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

        // C1: a plan holding even one `Command` target is a plan that cannot be
        // fully recovered, so both the label and the detail text below derive
        // from the same test rather than one being updated and the other not.
        let irreversible = plan.commanded() > 0;
        let buttons: &[&str] = if irreversible {
            &["Delete", "Cancel"]
        } else {
            &["Move to Trash", "Cancel"]
        };
        let prompt = window.prompt(
            gpui::PromptLevel::Warning,
            "Delete all history for this project?",
            Some(&delete_all_detail(
                plan.trashed(),
                plan.commanded(),
                plan.total_bytes,
            )),
            buttons,
            cx,
        );

        let fs = workspace.read(cx).project().read(cx).fs().clone();
        let agent_store = workspace
            .read(cx)
            .project()
            .read(cx)
            .agent_server_store()
            .clone();
        let store = crate::SessionStore::global(cx);
        let targets = plan.targets;

        cx.spawn(async move |this, cx| {
            if prompt.await.ok() != Some(0) {
                return;
            }
            this.update(cx, |this, cx| {
                this.deleting = true;
                cx.notify();
            })
            .ok();

            let executor = cx.background_executor().clone();
            let mut forget: HashSet<Arc<str>> = HashSet::default();
            for target in targets {
                // The unit of success is the session, not what it took to get
                // there: a session is forgotten only once everything it owns
                // is really gone, so a half-deleted session stays listed and
                // keeps describing the truth. The sweep never aborts early
                // either -- one failure must not strand the two hundred
                // behind it. Commands run sequentially and one at a time: they
                // are writers against one sqlite database in WAL mode, and N
                // concurrent writers is contention at best.
                match target.deletion {
                    Deletion::Nothing => {}
                    Deletion::Trash(paths) => {
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
                    Deletion::Command(command) => {
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
            delete_all_detail(3, 0, 4096),
            delete_all_detail(0, 3, 0),
            delete_all_detail(3, 3, 4096),
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
        let one = delete_all_detail(1, 0, 1024);
        assert!(one.contains("1 session will"), "singular, got: {one}");
        assert!(one.contains("1 KB"), "got: {one}");

        let many = delete_all_detail(42, 0, 13 * 1024 * 1024);
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
        let one = delete_all_detail(0, 1, 0);
        assert!(one.contains("1 session will"), "singular, got: {one}");
        assert!(
            one.contains("cannot be undone"),
            "an irreversible-only plan must say so, got: {one}"
        );
        assert!(
            !one.contains(" B)") && !one.contains(" KB)") && !one.contains(" MB)"),
            "there is no byte figure to report for a Command-only plan, got: {one}"
        );

        let many = delete_all_detail(0, 7, 0);
        assert!(many.contains("7 sessions will"), "plural, got: {many}");
    }

    /// The mixed branch: both counts appear, stated separately -- never one
    /// merged total, which is exactly how C1's scenario happens.
    #[test]
    fn the_mixed_prompt_states_both_counts_separately() {
        let detail = delete_all_detail(3, 40, 40 * 1024);
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

    /// M3: opencode's real failure output, byte for byte. "Trim control
    /// characters" alone would leave `[91m`/`[1m` behind as ordinary text --
    /// this is the one thing that proves a real CSI scan instead.
    #[test]
    fn the_csi_scanner_leaves_only_the_message() {
        let raw = b"\x1b[91m\x1b[1mError: \x1b[0mSession not found: x";
        assert_eq!(strip_ansi_first_line(raw), "Error: Session not found: x");
    }

    #[test]
    fn the_csi_scanner_is_a_no_op_on_plain_text() {
        assert_eq!(
            strip_ansi_first_line(b"plain error, no escapes"),
            "plain error, no escapes"
        );
    }

    #[test]
    fn the_csi_scanner_skips_a_blank_first_line() {
        assert_eq!(
            strip_ansi_first_line(b"\n\x1b[0mSession not found: x"),
            "Session not found: x"
        );
    }

    /// A stand-in for whatever `provider.find` would answer, so the timeout
    /// race and the idempotency re-check can be driven without a real store.
    struct FindOnlyProvider(Option<SessionSummary>);

    impl SessionProvider for FindOnlyProvider {
        fn agent(&self) -> AgentKind {
            AgentKind::OpenCode
        }
        fn availability(&self) -> agent_sessions::Availability {
            agent_sessions::Availability::Ready
        }
        fn list(&self) -> anyhow::Result<Vec<SessionSummary>> {
            Ok(Vec::new())
        }
        fn find(&self, _id: &str) -> anyhow::Result<Option<SessionSummary>> {
            Ok(self.0.clone())
        }
        fn new_session_command(&self, _id: &str, _cwd: &std::path::Path) -> Option<AgentCommand> {
            None
        }
        fn counts(
            &self,
            _session: &SessionSummary,
        ) -> anyhow::Result<agent_sessions::SessionCounts> {
            Ok(agent_sessions::SessionCounts::default())
        }
        fn resume_command(&self, _session: &SessionSummary, _fork: Fork) -> Option<AgentCommand> {
            None
        }
        fn deletion(&self, _session: &SessionSummary) -> Deletion {
            Deletion::Nothing
        }
    }

    fn stub_session(id: &str) -> SessionSummary {
        SessionSummary {
            id: Arc::from(id),
            agent: AgentKind::OpenCode,
            title: id.to_string(),
            preview: String::new(),
            preview_speaker: None,
            cwd: PathBuf::from("/w/one"),
            branch: None,
            model: None,
            updated_at: SystemTime::UNIX_EPOCH,
            log_path: None,
            log_bytes: 0,
        }
    }

    /// A fabricated exit, with no subprocess anywhere behind it -- see
    /// `race_command_delete`'s doc comment for why that is exactly the point.
    #[cfg(unix)]
    fn fake_exit_status(code: i32) -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        // The raw wait-status encoding: a normal exit packs the code into the
        // high byte.
        std::process::ExitStatus::from_raw(code << 8)
    }
    #[cfg(windows)]
    fn fake_exit_status(code: i32) -> std::process::ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code as u32)
    }

    /// C2/H9 -- the mechanism behind a Critical finding, reachable for the
    /// first time now that opencode is a real `Deletion::Command` producer:
    /// a command that exits non-zero for a session the store no longer holds
    /// must be treated as success, not failure. `opencode session delete` on
    /// an id already gone is exactly this shape.
    #[gpui::test]
    async fn a_failed_exit_is_forgiven_once_the_store_no_longer_holds_the_session(
        cx: &mut gpui::TestAppContext,
    ) {
        let provider: Arc<dyn SessionProvider> = Arc::new(FindOnlyProvider(None));
        let id: Arc<str> = Arc::from("ses_gone");
        let executor = cx.background_executor.clone();
        let run = std::future::ready(Ok(std::process::Output {
            status: fake_exit_status(1),
            stdout: Vec::new(),
            stderr: b"Error: Session not found: ses_gone".to_vec(),
        }));

        let outcome = cx
            .spawn(async move |mut cx| {
                race_command_delete(&provider, &executor, &id, run, &mut cx).await
            })
            .await;

        assert!(
            matches!(outcome, CommandDeleteOutcome::Gone),
            "a non-zero exit for a session the store no longer holds must be \
             forgiven, got {outcome:?}"
        );
    }

    /// The other half of C2/H9: a non-zero exit for a session the store
    /// *still* holds must not be silently forgiven -- the row has to stay,
    /// and the failure has to be reported.
    #[gpui::test]
    async fn a_failed_exit_is_reported_when_the_store_still_holds_the_session(
        cx: &mut gpui::TestAppContext,
    ) {
        let provider: Arc<dyn SessionProvider> =
            Arc::new(FindOnlyProvider(Some(stub_session("ses_here"))));
        let id: Arc<str> = Arc::from("ses_here");
        let executor = cx.background_executor.clone();
        let run = std::future::ready(Ok(std::process::Output {
            status: fake_exit_status(1),
            stdout: Vec::new(),
            stderr: b"boom".to_vec(),
        }));

        let outcome = cx
            .spawn(async move |mut cx| {
                race_command_delete(&provider, &executor, &id, run, &mut cx).await
            })
            .await;

        match outcome {
            CommandDeleteOutcome::Failed(message) => assert_eq!(message, "boom"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    /// M6 -- a command that never returns must be abandoned by the timer
    /// rather than blocking the caller forever. It falls through to the same
    /// idempotency re-check a failed exit does (C2/H9), which this also
    /// proves: the timeout path and the non-zero-exit path share one ending.
    #[gpui::test]
    async fn a_command_that_never_returns_is_abandoned_after_the_timeout(
        cx: &mut gpui::TestAppContext,
    ) {
        let provider: Arc<dyn SessionProvider> =
            Arc::new(FindOnlyProvider(Some(stub_session("ses_stuck"))));
        let id: Arc<str> = Arc::from("ses_stuck");
        let executor = cx.background_executor.clone();

        let task = cx.spawn(async move |mut cx| {
            race_command_delete(
                &provider,
                &executor,
                &id,
                std::future::pending::<std::io::Result<std::process::Output>>(),
                &mut cx,
            )
            .await
        });

        cx.run_until_parked();
        cx.executor().advance_clock(COMMAND_DELETE_TIMEOUT);
        cx.run_until_parked();
        let outcome = task.await;

        match outcome {
            CommandDeleteOutcome::Failed(message) => {
                assert!(message.contains("timed out"), "got: {message}");
            }
            other => panic!("expected Failed(timed out...), got {other:?}"),
        }
    }

    /// Builds a real, local `AgentServerStore` with an empty project
    /// environment -- enough for `resolve_agent_binary` to run its real
    /// logic. `ProjectEnvironment::get_cli_environment` always answers
    /// `Some(HashMap::default())` under `cfg!(test)`/`test-support`, which
    /// short-circuits before any real shell would be spawned, so this never
    /// touches a real login shell or a real filesystem.
    fn test_agent_server_store(cx: &mut gpui::TestAppContext) -> Entity<project::AgentServerStore> {
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
            let fs: Arc<dyn fs::Fs> =
                Arc::new(fs::RealFs::new(None, cx.background_executor().clone()));
            let worktree_store = cx.new(|cx| {
                project::worktree_store::WorktreeStore::local(
                    false,
                    fs,
                    project::worktree_store::WorktreeIdCounter::get(cx),
                )
            });
            let project_environment = cx.new(|cx| {
                project::ProjectEnvironment::new(None, worktree_store.downgrade(), None, false, cx)
            });
            cx.new(|cx| project::AgentServerStore::local(project_environment, cx))
        })
    }

    /// H3 -- a `Command` delete with the CLI absent must show its own
    /// message, not `offer_to_drop_the_row`'s "nothing is still on disk"
    /// text, and must not forget the row: the session might still be real,
    /// this machine just cannot ask its agent to remove it.
    #[gpui::test]
    async fn a_missing_binary_is_reported_distinctly_and_keeps_the_row(
        cx: &mut gpui::TestAppContext,
    ) {
        let provider: Arc<dyn SessionProvider> =
            Arc::new(FindOnlyProvider(Some(stub_session("ses_needs_opencode"))));
        let id: Arc<str> = Arc::from("ses_needs_opencode");
        let agent_store = test_agent_server_store(cx);
        let executor = cx.background_executor.clone();
        let command = AgentCommand {
            program: "opencode".to_string(),
            args: vec![
                "session".to_string(),
                "delete".to_string(),
                "ses_needs_opencode".to_string(),
            ],
            cwd: PathBuf::from("/w/one"),
        };

        let outcome = cx
            .spawn(async move |mut cx| {
                run_command_delete(
                    &provider,
                    &agent_store,
                    &executor,
                    AgentKind::OpenCode,
                    &id,
                    &command,
                    &mut cx,
                )
                .await
            })
            .await;

        match outcome {
            CommandDeleteOutcome::BinaryMissing(missing) => {
                assert_eq!(missing.binary, "opencode");
            }
            other => panic!("expected BinaryMissing, got {other:?}"),
        }
    }
}
