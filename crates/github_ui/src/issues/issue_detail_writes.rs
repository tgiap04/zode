//! What an issue's tab writes: its title and description, its state, a
//! comment, labels, assignees and the milestone. Labels and assignees go
//! through GitHub's add/remove calls, never a whole-set replacement, so a
//! change someone made meanwhile is kept. After each write the issue is read
//! back and whatever GitHub silently ignored is said.

use editor::Editor;
use github::{GithubFuture, GithubUser, Issue, IssueState, IssueUpdate, StateReason};
use gpui::{App, AppContext as _, Context, Entity, Window};

use crate::github_view_session::session_ended_message;
use crate::issues::IssueDetailView;
use crate::issues::assignee_picker::{AssigneePicker, AssigneePickerEvent, AssigneeSource};
use crate::issues::issue_detail_view::{IssueComment, IssueEdit};
use crate::issues::issue_editor::{markdown_editor, single_line_editor};
use crate::markdown_composer::{MarkdownComposer, PreviewSource};
use crate::untrusted_text::reveal_hidden_characters;

pub(crate) const EDIT_CONFLICT: &str = "Someone changed this issue on GitHub after you started editing. \
     It has been reloaded; your text is still here. Save again to replace their version.";

impl IssueDetailView {
    pub(crate) fn show_issue(&mut self, issue: Issue, cx: &mut Context<Self>) {
        let body = issue.body.clone().unwrap_or_default();
        self.body = Some(self.markdown(&body, cx));
        self.issue = Some(issue);
        self.issue_error = None;
        cx.notify();
    }

    pub fn write_error(&self) -> Option<&str> {
        self.write_error.as_deref()
    }

    pub fn write_notice(&self) -> Option<&str> {
        self.write_notice.as_deref()
    }

    pub fn comment_error(&self) -> Option<&str> {
        self.comment_error.as_deref()
    }

    pub fn edit_error(&self) -> Option<&str> {
        self.edit.as_ref().and_then(|edit| edit.error.as_deref())
    }

    /// A triage, state or type write, or the edit's save, is on its way;
    /// those controls wait for it.
    pub fn is_busy(&self) -> bool {
        self.writing || self.is_saving_edit()
    }

    pub fn is_saving_edit(&self) -> bool {
        self.edit.as_ref().is_some_and(|edit| edit.saving)
    }

    pub fn is_editing(&self) -> bool {
        self.edit.is_some()
    }

    /// The labels and milestones the menus offer. A failure leaves them
    /// empty; the issue itself reports errors.
    pub(crate) fn load_options(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let repository = self.key.repository.clone();
        self.options_task = Some(cx.spawn(async move |this, cx| {
            let labels = backend.list_labels(&repository, None).await;
            let milestones = backend.list_milestones(&repository, None).await;
            let viewer = backend.viewer().await;
            let permission = backend.repository_permission(&repository).await;
            let issue_types = backend.list_issue_types(&repository).await;
            this.update(cx, |this, cx| {
                for error in [
                    labels.as_ref().err(),
                    milestones.as_ref().err(),
                    viewer.as_ref().err(),
                    permission.as_ref().err(),
                    issue_types.as_ref().err(),
                ]
                .into_iter()
                .flatten()
                {
                    this.note_error(error, cx);
                }
                match permission {
                    Ok(permission) => this.permission = permission,
                    Err(error) => log::info!("could not read the account's role: {error}"),
                }
                match issue_types {
                    Ok(issue_types) => this.issue_types = issue_types,
                    Err(error) => {
                        log::info!("could not read the repository's issue types: {error}")
                    }
                }
                match viewer {
                    Ok(viewer) => this.login = Some(viewer.login.into()),
                    Err(error) => log::info!("could not read the signed-in account: {error}"),
                }
                match labels {
                    Ok(page) => this.labels = page.items,
                    Err(error) => log::info!("could not read the repository's labels: {error}"),
                }
                match milestones {
                    Ok(page) => this.milestones = page.items,
                    Err(error) => log::info!("could not read the repository's milestones: {error}"),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn start_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(issue) = &self.issue else {
            return;
        };
        if self.edit.is_some() {
            return;
        }
        let languages = self.languages.clone();
        let text = issue.body.clone().unwrap_or_default();
        let body = markdown_editor(8, "Description (markdown)", &text, languages, window, cx);
        let source = PreviewSource {
            repository: self.key.repository.clone(),
            backend: self.backend.clone(),
            session: self.session.clone(),
        };
        let description = cx.new(|_| MarkdownComposer::new(body.clone(), source));
        self.edit = Some(IssueEdit {
            title: single_line_editor("Title", &issue.title, window, cx),
            body,
            description,
            base_updated_at: issue.updated_at.clone(),
            error: None,
            saving: false,
        });
        cx.notify();
    }

    pub fn cancel_editing(&mut self, cx: &mut Context<Self>) {
        if self.edit.as_ref().is_some_and(|edit| !edit.saving) {
            self.edit = None;
            cx.notify();
        }
    }

    /// Saves the title and description. Reads the issue first and stops if
    /// it changed since editing began: best effort, since GitHub has no
    /// conditional update, but it catches the common case without losing
    /// either side's text.
    pub fn save_edit(&mut self, cx: &mut Context<Self>) {
        if self.edit.as_ref().is_none_or(|edit| edit.saving) || !self.check_session(cx) {
            return;
        }
        let Some(edit) = &mut self.edit else {
            return;
        };
        let title = edit.title.read(cx).text(cx).trim().to_string();
        if title.is_empty() {
            edit.error = Some("A title is required.".to_string());
            cx.notify();
            return;
        }
        let body = edit.body.read(cx).text(cx);
        let base = edit.base_updated_at.clone();
        edit.saving = true;
        edit.error = None;
        let backend = self.backend.clone();
        let key = self.key.clone();
        self.edit_task = Some(cx.spawn(async move |this, cx| {
            let current = backend.issue(&key.repository, key.number).await;
            match current {
                Ok(current) if current.updated_at == base => {}
                other => {
                    this.update(cx, |this, cx| this.edit_stopped(other, cx))
                        .ok();
                    return;
                }
            }
            // A sign-out may land while the issue is read again.
            let live = this
                .update(cx, |this, cx| {
                    let live = !this.session_ended(cx);
                    if !live && let Some(edit) = &mut this.edit {
                        edit.saving = false;
                        edit.error = Some(session_ended_message("issue"));
                        cx.notify();
                    }
                    live
                })
                .unwrap_or(false);
            if !live {
                return;
            }
            let update = IssueUpdate {
                title: Some(title),
                body: Some(body),
                ..IssueUpdate::default()
            };
            let saved = backend
                .update_issue(&key.repository, key.number, update)
                .await;
            this.update(cx, |this, cx| match saved {
                Ok(issue) => {
                    this.edit = None;
                    this.show_issue(issue, cx);
                }
                Err(error) => {
                    this.note_error(&error, cx);
                    if let Some(edit) = &mut this.edit {
                        edit.saving = false;
                        edit.error = Some(format!("Could not save: {error}"));
                    }
                    cx.notify();
                }
            })
            .ok();
        }));
        cx.notify();
    }

    /// The edit could not go ahead: the check failed, or found the issue
    /// changed. The text being edited stays.
    fn edit_stopped(&mut self, current: github::GithubResult<Issue>, cx: &mut Context<Self>) {
        let message = match current {
            Ok(current) => {
                let updated_at = current.updated_at.clone();
                self.show_issue(current, cx);
                if let Some(edit) = &mut self.edit {
                    edit.base_updated_at = updated_at;
                }
                EDIT_CONFLICT.to_string()
            }
            Err(error) => {
                self.note_error(&error, cx);
                format!("Could not check the issue before saving: {error}")
            }
        };
        if let Some(edit) = &mut self.edit {
            edit.saving = false;
            edit.error = Some(message);
        }
        cx.notify();
    }

    pub fn close(&mut self, reason: StateReason, window: &mut Window, cx: &mut Context<Self>) {
        self.set_state(IssueState::Closed, reason, window, cx);
    }

    pub fn reopen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_state(IssueState::Open, StateReason::Reopened, window, cx);
    }

    fn set_state(
        &mut self,
        state: IssueState,
        reason: StateReason,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let update = IssueUpdate {
            state: Some(state),
            state_reason: Some(reason),
            ..IssueUpdate::default()
        };
        if !self.ready_to_write(cx) {
            return;
        }
        let write = self
            .backend
            .update_issue(&self.key.repository, self.key.number, update);
        let what = match state {
            IssueState::Open => "reopening",
            IssueState::Closed => "closing",
        };
        self.write_and_read_back(write, None, window, cx, move |issue| {
            (issue.state != state).then(|| what.to_string())
        });
    }

    pub fn add_label(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready_to_write(cx) {
            return;
        }
        let write =
            self.backend
                .add_labels(&self.key.repository, self.key.number, vec![name.clone()]);
        self.write_and_read_back(write, None, window, cx, move |issue| {
            (!issue.labels.iter().any(|label| label.name == name))
                .then(|| format!("the label “{}”", reveal_hidden_characters(&name)))
        });
    }

    pub fn remove_label(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready_to_write(cx) {
            return;
        }
        let write = self
            .backend
            .remove_label(&self.key.repository, self.key.number, name.clone());
        self.write_and_read_back(write, None, window, cx, move |issue| {
            issue
                .labels
                .iter()
                .any(|label| label.name == name)
                .then(|| format!("removing the label “{}”", reveal_hidden_characters(&name)))
        });
    }

    /// Assigns `user`, or unassigns them when the issue already has them.
    pub fn toggle_assignee(
        &mut self,
        user: GithubUser,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let assigned = self
            .issue
            .as_ref()
            .is_some_and(|issue| issue.assignees.contains(&user.login));
        if assigned {
            self.remove_assignee(user.login, window, cx);
        } else {
            self.add_assignees(vec![user.login], None, window, cx);
        }
    }

    pub fn add_assignees(
        &mut self,
        logins: Vec<String>,
        field: Option<Entity<Editor>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ready_to_write(cx) {
            return;
        }
        let write =
            self.backend
                .add_assignees(&self.key.repository, self.key.number, logins.clone());
        self.write_and_read_back(write, field, window, cx, move |issue| {
            let missing = logins
                .iter()
                .filter(|wanted| {
                    !issue
                        .assignees
                        .iter()
                        .any(|login| login.eq_ignore_ascii_case(wanted))
                })
                .map(|login| format!("@{}", reveal_hidden_characters(login)))
                .collect::<Vec<_>>();
            (!missing.is_empty()).then(|| format!("assigning {}", missing.join(", ")))
        });
    }

    pub fn remove_assignee(&mut self, login: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready_to_write(cx) {
            return;
        }
        let write = self.backend.remove_assignees(
            &self.key.repository,
            self.key.number,
            vec![login.clone()],
        );
        self.write_and_read_back(write, None, window, cx, move |issue| {
            issue
                .assignees
                .contains(&login)
                .then(|| format!("unassigning @{}", reveal_hidden_characters(&login)))
        });
    }

    /// `None` clears the milestone.
    pub fn set_milestone(
        &mut self,
        milestone: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ready_to_write(cx) {
            return;
        }
        let write = self
            .backend
            .set_milestone(&self.key.repository, self.key.number, milestone);
        self.write_and_read_back(write, None, window, cx, move |issue| {
            (issue.milestone.as_ref().map(|milestone| milestone.number) != milestone)
                .then(|| "the milestone".to_string())
        });
    }

    /// Runs one write, reads the issue back, and says what GitHub did not
    /// apply according to `not_applied`. `field` is emptied on success.
    pub(crate) fn write_and_read_back<T: Send + 'static>(
        &mut self,
        write: GithubFuture<T>,
        field: Option<Entity<Editor>>,
        window: &mut Window,
        cx: &mut Context<Self>,
        not_applied: impl FnOnce(&Issue) -> Option<String> + 'static,
    ) {
        self.writing = true;
        self.write_error = None;
        self.write_notice = None;
        let backend = self.backend.clone();
        let key = self.key.clone();
        self.write_task = Some(cx.spawn_in(window, async move |this, cx| {
            let written = write.await;
            let read_back = match &written {
                Ok(_) => Some(backend.issue(&key.repository, key.number).await),
                Err(_) => None,
            };
            this.update_in(cx, |this, window, cx| {
                this.writing = false;
                match (written, read_back) {
                    (Err(error), _) => {
                        this.note_error(&error, cx);
                        this.write_error = Some(error.to_string());
                    }
                    (Ok(_), Some(Ok(issue))) => {
                        if let Some(field) = field {
                            field.update(cx, |field, cx| field.clear(window, cx));
                        }
                        this.write_notice = not_applied(&issue).map(|what| this.not_applied(&what));
                        // This tab's own change moved `updated_at`; an edit
                        // left open must not take it for someone else's.
                        if let Some(edit) = &mut this.edit
                            && !edit.saving
                        {
                            edit.base_updated_at = issue.updated_at.clone();
                        }
                        this.show_issue(issue, cx);
                    }
                    (Ok(_), read_back) => {
                        let reason =
                            read_back
                                .and_then(Result::err)
                                .map_or_else(String::new, |error| {
                                    this.note_error(&error, cx);
                                    format!(": {error}")
                                });
                        this.write_error =
                            Some(format!("Saved, but could not read the issue back{reason}"));
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// One triage write at a time, since a second would race the first's
    /// read-back, and none once the session is over. The edit's save runs
    /// on its own task, so neither cancels the other; the controls still
    /// wait for it (see [`Self::is_busy`]).
    pub(crate) fn ready_to_write(&mut self, cx: &mut Context<Self>) -> bool {
        if self.writing || !self.check_session(cx) {
            return false;
        }
        // The save reads the issue back over whatever this would change.
        if self.is_saving_edit() {
            self.write_error =
                Some("Wait until the title and description are saved, then try again.".into());
            cx.notify();
            return false;
        }
        true
    }

    fn not_applied(&self, what: &str) -> String {
        let who = self
            .login
            .as_ref()
            .map_or("This account".to_string(), |login| {
                format!("@{}", reveal_hidden_characters(login))
            });
        format!(
            "GitHub did not apply {what}. {who} may not have permission to triage issues in {}.",
            self.key.repository
        )
    }

    pub(crate) fn comment_editor(&mut self, window: &mut Window, cx: &mut App) -> Entity<Editor> {
        if let Some(editor) = &self.comment_editor {
            return editor.clone();
        }
        let placeholder = "Leave a comment (markdown). ⌘↩ or Ctrl-↩ sends it.";
        let editor = markdown_editor(3, placeholder, "", self.languages.clone(), window, cx);
        let source = PreviewSource {
            repository: self.key.repository.clone(),
            backend: self.backend.clone(),
            session: self.session.clone(),
        };
        self.comment_composer = Some(cx.new(|_| MarkdownComposer::new(editor.clone(), source)));
        self.comment_editor = Some(editor.clone());
        editor
    }

    pub(crate) fn comment_composer(
        &mut self,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Entity<MarkdownComposer>> {
        self.comment_editor(window, cx);
        self.comment_composer.clone()
    }

    /// The assignee search, made on first draw since it needs a window.
    pub(crate) fn assignee_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<AssigneePicker> {
        if let Some((picker, _)) = &self.assignee_picker {
            return picker.clone();
        }
        let source = AssigneeSource {
            backend: self.backend.clone(),
            repository: self.key.repository.clone(),
            session: self.session.clone(),
        };
        let picker = cx.new(|cx| AssigneePicker::new(source, window, cx));
        let subscription = cx.subscribe_in(
            &picker,
            window,
            |this, _, event: &AssigneePickerEvent, window, cx| {
                let AssigneePickerEvent::Picked(user) = event;
                this.toggle_assignee(user.clone(), window, cx);
            },
        );
        self.assignee_picker = Some((picker.clone(), subscription));
        picker
    }

    /// Sends what is in the comment box. The box is emptied only once GitHub
    /// has the comment; on failure the text stays and the error shows under
    /// it.
    pub fn send_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.comment_editor.clone() else {
            return;
        };
        let text = editor.read(cx).text(cx);
        if self.sending_comment || text.trim().is_empty() || !self.check_session(cx) {
            return;
        }
        self.sending_comment = true;
        self.comment_error = None;
        let backend = self.backend.clone();
        let key = self.key.clone();
        self.comment_task = Some(cx.spawn_in(window, async move |this, cx| {
            let sent = backend.add_comment(&key.repository, key.number, text).await;
            this.update_in(cx, |this, window, cx| {
                this.sending_comment = false;
                match sent {
                    Ok(comment) => {
                        editor.update(cx, |editor, cx| editor.clear(window, cx));
                        if let Some(composer) = &this.comment_composer {
                            composer.update(cx, |composer, cx| composer.back_to_write(cx));
                        }
                        let body = this.markdown(&comment.body, cx);
                        this.comments.push(IssueComment { comment, body });
                        if let Some(issue) = &mut this.issue {
                            issue.comment_count += 1;
                        }
                    }
                    Err(error) => {
                        this.note_error(&error, cx);
                        this.comment_error = Some(format!("Could not send the comment: {error}"))
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }
}
