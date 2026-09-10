use crate::{AgentView, agent_view::SessionIntent, session_history::panel::AgentHistoryPanel};
use agent_sessions::{Fork, SessionProvider, SessionSummary};
use gpui::{App, ClipboardItem, Context, Entity, Window};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use util::ResultExt as _;

/// Whether the directory a session ran in is still there.
///
/// The row's badge and every control that would run something in that directory
/// read this. A resume into a directory that no longer exists would start the CLI
/// in a place with none of the files the conversation is about.
pub(crate) fn cwd_exists(session: &SessionSummary) -> bool {
    !session.cwd.as_os_str().is_empty() && session.cwd.is_dir()
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
    let paths = provider.paths_to_trash(session);
    if paths.is_empty() {
        return;
    }
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
        for path in paths {
            if !path.exists() {
                continue;
            }
            fs.trash(
                &path,
                fs::RemoveOptions {
                    recursive: true,
                    ignore_if_not_exists: true,
                },
            )
            .await
            .log_err();
        }
        // Drop the entry rather than re-sweeping: the delete already knows
        // exactly what it removed, and a sweep would open every other
        // transcript on disk to learn one fact it was told.
        store.update(cx, |store, cx| store.forget(&id, cx));
    })
    .detach();
}

/// One session's worth of a bulk delete: the id to forget, and the files that
/// have to reach the trash before it may be forgotten.
pub(crate) struct DeleteTarget {
    pub(crate) id: Arc<str>,
    pub(crate) paths: Vec<PathBuf>,
}

/// Everything a "delete all" will take, decided before anything is asked or
/// touched.
///
/// Built as plain data so the count and the size in the confirmation are the
/// same numbers the sweep then acts on -- a prompt that says "12 sessions" and
/// a sweep that takes 14 is the kind of disagreement nobody notices until it
/// has already happened.
pub(crate) struct DeleteAll {
    pub(crate) targets: Vec<DeleteTarget>,
    pub(crate) total_bytes: u64,
}

impl DeleteAll {
    pub(crate) fn count(&self) -> usize {
        self.targets.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.targets.is_empty()
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

/// Turns the in-scope sessions and their paths into the plan to act on.
///
/// A session whose provider offers no paths is dropped rather than carried: it
/// has nothing on disk to take, so counting it would inflate the number in the
/// prompt and forgetting it would drop a row describing a file that is still
/// there.
pub(crate) fn plan_delete_all<'a>(
    scoped: impl IntoIterator<Item = (&'a SessionSummary, Vec<PathBuf>)>,
) -> DeleteAll {
    let mut targets = Vec::new();
    let mut total_bytes = 0;
    for (session, paths) in scoped {
        if paths.is_empty() {
            continue;
        }
        total_bytes += session.log_bytes;
        targets.push(DeleteTarget {
            id: session.id.clone(),
            paths,
        });
    }
    DeleteAll {
        targets,
        total_bytes,
    }
}

/// The body of the confirmation.
///
/// Two numbers and one clarification. The paths are deliberately left out --
/// two hundred lines of `~/.claude/projects/...` is a wall, not a confirmation,
/// and the count and the size are the two facts that change the answer. The
/// second paragraph exists because the panel is filtered: someone looking at
/// three rows needs telling that this takes all forty.
pub(crate) fn delete_all_detail(count: usize, total_bytes: u64) -> String {
    let plural = if count == 1 { "" } else { "s" };
    // `concat!` rather than one wrapped literal: a trailing-backslash
    // continuation only strips the next line's indentation while it survives
    // the formatter, and when it does not the spaces land *inside* the string.
    // Four of them after a blank line is an indented code block in CommonMark,
    // which would render the safety sentence as monospaced output.
    format!(
        concat!(
            "{count} session{plural} will move to the trash ({bytes}).\n\n",
            "Every session for this project goes, not just the ones the search ",
            "shows. They move to the OS trash and can be recovered from there.",
        ),
        count = count,
        plural = plural,
        bytes = format_bytes(total_bytes),
    )
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
        let scoped: Vec<(&SessionSummary, Vec<PathBuf>)> =
            sessions_in_project(self.sessions(cx), &roots)
                .map(|session| {
                    let paths = self
                        .provider_for(session)
                        .map(|provider| provider.paths_to_trash(session))
                        .unwrap_or_default();
                    (session, paths)
                })
                .collect();
        let plan = plan_delete_all(scoped);

        // The truthful half of the empty rule. The button is already disabled
        // when this project has no sessions, but "has sessions" and "has files
        // to take" are different questions, and only this one has been to the
        // providers.
        if plan.is_empty() {
            return;
        }

        let prompt = window.prompt(
            gpui::PromptLevel::Warning,
            "Delete all history for this project?",
            Some(&delete_all_detail(plan.count(), plan.total_bytes)),
            &["Move to Trash", "Cancel"],
            cx,
        );

        let fs = workspace.read(cx).project().read(cx).fs().clone();
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

            let mut forget: HashSet<Arc<str>> = HashSet::default();
            for target in targets {
                // The unit of success is the session, not the path: a session is
                // forgotten only once everything it owns is really gone, so a
                // half-deleted session stays listed and keeps describing the
                // disk. The sweep never aborts early either -- one unreadable
                // transcript must not strand the two hundred behind it.
                let mut every_path_gone = true;
                for path in &target.paths {
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

    #[test]
    fn sessions_with_nothing_on_disk_are_not_planned() {
        let with_files = session("keeps", "/root", 100);
        let without = session("empty", "/root", 8_000);
        let plan = plan_delete_all(vec![
            (&with_files, vec![PathBuf::from("/logs/keeps.jsonl")]),
            (&without, Vec::new()),
        ]);

        assert_eq!(plan.count(), 1);
        assert_eq!(plan.targets[0].id.as_ref(), "keeps");
        assert_eq!(
            plan.total_bytes, 100,
            "a session with no files owns no bytes to take"
        );
    }

    #[test]
    fn the_plan_tallies_only_what_it_will_take() {
        let first = session("first", "/root", 1024);
        let second = session("second", "/root", 1024);
        let huge_but_pathless = session("huge", "/root", 8 * 1024 * 1024);
        let plan = plan_delete_all(vec![
            (&first, vec![PathBuf::from("/logs/first")]),
            (&second, vec![PathBuf::from("/logs/second")]),
            (&huge_but_pathless, Vec::new()),
        ]);

        assert_eq!(plan.count(), 2);
        assert_eq!(plan.total_bytes, 2048);
        assert!(!plan.is_empty());
    }

    /// The detail is rendered as markdown, and CommonMark turns any line
    /// indented four spaces or more after a blank line into a code block. A
    /// wrapped string literal is exactly how those spaces get in -- the safety
    /// sentence would then be shown monospaced, reading like output rather than
    /// like a warning. `contains` assertions cannot see this, so the shape of
    /// every line is checked directly.
    #[test]
    fn the_prompt_is_prose_not_an_accidental_code_block() {
        let detail = delete_all_detail(3, 4096);
        for line in detail.lines() {
            assert!(
                !line.starts_with("    "),
                "a line indented four spaces renders as a code block: {line:?}"
            );
        }
    }

    #[test]
    fn the_prompt_names_the_count_and_the_size() {
        let one = delete_all_detail(1, 1024);
        assert!(one.contains("1 session will"), "singular, got: {one}");
        assert!(one.contains("1 KB"), "got: {one}");

        let many = delete_all_detail(42, 13 * 1024 * 1024);
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
}
