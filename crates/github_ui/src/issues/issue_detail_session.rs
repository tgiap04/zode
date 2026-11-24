//! What an issue's tab knows about where it writes: the sign-in it was
//! opened under, and the account and repository its header names.

use github::GithubError;
use gpui::{App, Context, Window};
use workspace::Workspace;

use crate::github_view_session::{
    current_session_backend, is_live_session, report_if_revoked, session_ended_message,
};
use crate::issues::issue_actions::target_line;
use crate::issues::{IssueDetailView, open_issue_detail};

impl IssueDetailView {
    /// Whether the account this tab was opened under is gone: signed out,
    /// refused, or replaced by another.
    pub fn session_ended(&self, cx: &App) -> bool {
        !is_live_session(&self.session, cx)
    }

    /// `false`, with the reason shown, once the session is over: the backend
    /// this tab holds must not write as an account the GitHub tab no longer
    /// shows.
    pub(crate) fn check_session(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.session_ended(cx) {
            return true;
        }
        self.write_error = Some(session_ended_message("issue"));
        cx.notify();
        false
    }

    /// Hands a refused token to the GitHub tab.
    pub(crate) fn note_error(&self, error: &GithubError, cx: &mut App) {
        report_if_revoked(error, &self.session, cx);
    }

    /// Opens this issue again with the GitHub tab's current account, or
    /// brings the GitHub tab forward to sign in.
    pub fn reopen_in_current_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = Workspace::for_window(window, cx) else {
            return;
        };
        let key = self.key.clone();
        // Clicked while this tab is being updated, and it is about to be
        // replaced.
        window.defer(cx, move |window, cx| {
            workspace.update(cx, |workspace, cx| {
                match current_session_backend(workspace, cx) {
                    Some((backend, session)) => {
                        open_issue_detail(workspace, backend, session, key, window, cx);
                    }
                    None => {
                        crate::open(workspace, window, cx);
                    }
                }
            });
        });
    }

    /// `@login → owner/repo (private)` for the issue's own repository, which
    /// a link may have opened rather than the one picked in the GitHub tab.
    pub fn target_line(&self) -> String {
        let login = self.login.as_ref().map_or("…", |login| login.as_ref());
        target_line(login, &self.key.repository, self.visibility)
    }

    pub fn visibility_error(&self) -> Option<&GithubError> {
        self.visibility_error.as_ref()
    }

    pub(crate) fn load_visibility(&mut self, cx: &mut Context<Self>) {
        self.visibility_error = None;
        let backend = self.backend.clone();
        let repository = self.key.repository.clone();
        self.visibility_task = Some(cx.spawn(async move |this, cx| {
            let visibility = backend.repository_visibility(&repository).await;
            this.update(cx, |this, cx| {
                match visibility {
                    Ok(visibility) => this.visibility = Some(visibility),
                    Err(error) => {
                        this.note_error(&error, cx);
                        this.visibility_error = Some(error);
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    pub fn retry_visibility(&mut self, cx: &mut Context<Self>) {
        self.load_visibility(cx);
    }
}
