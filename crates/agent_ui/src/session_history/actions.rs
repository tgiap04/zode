use crate::{AgentView, agent_view::SessionIntent, session_history::panel::AgentHistoryPanel};
use agent_sessions::{Fork, SessionProvider, SessionSummary};
use gpui::{App, ClipboardItem, Context, Entity, Window};
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
    workspace.update(cx, |workspace, cx| {
        AgentView::open_tracked(workspace, agent, intent, window, cx);
    });
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
        let Some(provider) = self.provider_for(session) else {
            return;
        };
        let paths = provider.paths_to_trash(session);
        if paths.is_empty() {
            return;
        }
        let Some(fs) = self
            .workspace()
            .update(cx, |workspace, cx| {
                workspace.project().read(cx).fs().clone()
            })
            .log_err()
        else {
            return;
        };

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
        let id = session.id.clone();
        cx.spawn_in(window, async move |this, cx| {
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
            // Drop the row rather than re-reading both stores: the list is a view
            // of what was found, and one gone session does not invalidate the rest.
            this.update(cx, |this, cx| {
                this.store.update(cx, |store, cx| store.forget(&id, cx));
                this.counts.remove(&id);
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
    use super::format_bytes;

    #[test]
    fn sizes_read_the_way_a_person_would_say_them() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(900), "900 B");
        assert_eq!(format_bytes(2048), "2 KB");
        assert_eq!(format_bytes(13 * 1024 * 1024), "13.0 MB");
    }
}
