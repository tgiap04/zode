use std::collections::HashSet;
use std::sync::Arc;

use editor::Editor;
use github::{Comment, GithubBackend, GithubError, PullRequestDetails, RepositoryVisibility};
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, SharedString,
    Task, WeakEntity, Window,
};
use language::LanguageRegistry;
use project::Project;
use ui::IconName;
use workspace::Workspace;
use workspace::item::Item;

use crate::github_markdown::GithubMarkdown;
use crate::github_view::{DetailKey, ProjectAccess};
use crate::github_view_session::{
    SessionStamp, is_live_session, observe_session, replace_stale_tab, report_if_revoked,
    session_ended_message,
};
use crate::issues::{markdown_editor, target_line};
use crate::markdown_composer::{MarkdownComposer, PreviewSource};
use crate::pull_requests::merge_modal::{MergeAvailability, MergeDialog, merge_availability};
use crate::untrusted_text::reveal_hidden_characters;

pub(crate) struct PullRequestComment {
    pub comment: Comment,
    pub body: Entity<GithubMarkdown>,
}

/// One pull request in its own tab: found again by its [`DetailKey`], never
/// restored at start-up. Comments are read a page at a time.
pub struct PullRequestDetailView {
    pub(crate) key: DetailKey,
    pub(crate) backend: Arc<dyn GithubBackend>,
    pub(crate) project: WeakEntity<Project>,
    pub(crate) access: ProjectAccess,
    pub(crate) focus_handle: FocusHandle,
    /// The signed-in account, named in the header and the merge dialog.
    pub(crate) login: Option<SharedString>,
    pub(crate) visibility: Option<RepositoryVisibility>,
    pub(crate) visibility_error: Option<String>,
    visibility_task: Option<Task<()>>,
    /// The sign-in this tab was opened under, as its opener had it; once it
    /// ends, the backend held here is not the account the GitHub tab shows.
    pub(crate) session: SessionStamp,
    _session_subscription: gpui::Subscription,
    pub(crate) details: Option<PullRequestDetails>,
    pub(crate) body: Option<Entity<GithubMarkdown>>,
    pub(crate) error: Option<GithubError>,
    pub(crate) comments: Vec<PullRequestComment>,
    pub(crate) next_comment_cursor: Option<String>,
    pub(crate) comments_error: Option<GithubError>,
    pub(crate) loading_comments: bool,
    load_counter: u64,
    awaited_details: u64,
    awaited_comments: u64,
    details_task: Option<Task<()>>,
    comments_task: Option<Task<()>>,
    login_task: Option<Task<()>>,
    pub(crate) languages: Option<Arc<LanguageRegistry>>,
    pub(crate) comment_editor: Option<Entity<Editor>>,
    /// Write and Preview tabs and the toolbar, around `comment_editor`.
    pub(crate) comment_composer: Option<Entity<MarkdownComposer>>,
    pub(crate) comment_error: Option<String>,
    pub(crate) sending_comment: bool,
    pub(crate) comment_task: Option<Task<()>>,
    pub(crate) checkout: crate::pull_requests::pr_detail_checkout::CheckoutState,
    pub(crate) branch_editor: Option<Entity<Editor>>,
    pub(crate) checkout_task: Option<Task<()>>,
    /// The fork warning was accepted for the checkout under way.
    pub(crate) fork_warning_accepted: bool,
    /// Where git's password prompt is shown during a checkout's fetch.
    pub(crate) workspace: WeakEntity<Workspace>,
}

pub enum PullRequestDetailEvent {}

impl EventEmitter<PullRequestDetailEvent> for PullRequestDetailView {}

impl Focusable for PullRequestDetailView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Item for PullRequestDetailView {
    type Event = PullRequestDetailEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        match &self.details {
            Some(details) => format!(
                "#{} {}",
                details.pull_request.number,
                reveal_hidden_characters(&details.pull_request.title)
            )
            .into(),
            None => format!("#{}", self.key.number).into(),
        }
    }

    fn tab_tooltip_text(&self, _cx: &App) -> Option<SharedString> {
        Some(format!("{}#{}", self.key.repository, self.key.number).into())
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<ui::Icon> {
        Some(ui::Icon::new(IconName::GitBranch))
    }
}

/// Brings the pull request's tab forward, or opens one under `session`, the
/// opener's, and reads it.
pub fn open_pull_request_detail(
    workspace: &mut Workspace,
    backend: Arc<dyn GithubBackend>,
    session: SessionStamp,
    key: DetailKey,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Entity<PullRequestDetailView> {
    // Collected first: `items_of_type` borrows the workspace and
    // `activate_item` needs it back.
    let existing = workspace
        .items_of_type::<PullRequestDetailView>(cx)
        .find(|view| view.read(cx).key == key);
    if let Some(existing) = &existing
        && !existing.read(cx).session_ended(cx)
    {
        workspace.activate_item(existing, true, true, window, cx);
        return existing.clone();
    }
    let project = workspace.project().clone();
    let access = ProjectAccess::of(project.read(cx));
    let languages = project.read(cx).languages().clone();
    let workspace_handle = cx.entity().downgrade();
    let view = cx.new(|cx| {
        let mut view =
            PullRequestDetailView::new(key, backend, session, project.downgrade(), access, cx);
        view.languages = Some(languages);
        view.workspace = workspace_handle;
        view.reload(cx);
        view
    });
    match existing {
        Some(stale) => replace_stale_tab(workspace, &stale, &view, window, cx),
        None => workspace.add_item_to_active_pane(Box::new(view.clone()), None, true, window, cx),
    }
    view
}

impl PullRequestDetailView {
    /// Silent until [`Self::reload`].
    pub fn new(
        key: DetailKey,
        backend: Arc<dyn GithubBackend>,
        session: SessionStamp,
        project: WeakEntity<Project>,
        access: ProjectAccess,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            key,
            backend,
            project,
            access,
            focus_handle: cx.focus_handle(),
            login: None,
            visibility: None,
            visibility_error: None,
            visibility_task: None,
            session,
            _session_subscription: observe_session(cx),
            details: None,
            body: None,
            error: None,
            comments: Vec::new(),
            next_comment_cursor: None,
            comments_error: None,
            loading_comments: false,
            load_counter: 0,
            awaited_details: 0,
            awaited_comments: 0,
            details_task: None,
            comments_task: None,
            login_task: None,
            languages: None,
            comment_editor: None,
            comment_composer: None,
            comment_error: None,
            sending_comment: false,
            comment_task: None,
            checkout: Default::default(),
            branch_editor: None,
            checkout_task: None,
            fork_warning_accepted: false,
            workspace: WeakEntity::new_invalid(),
        }
    }

    pub fn key(&self) -> &DetailKey {
        &self.key
    }

    pub fn details(&self) -> Option<&PullRequestDetails> {
        self.details.as_ref()
    }

    pub fn comments(&self) -> impl Iterator<Item = &Comment> {
        self.comments.iter().map(|comment| &comment.comment)
    }

    pub fn has_more_comments(&self) -> bool {
        self.next_comment_cursor.is_some()
    }

    pub fn comment_error(&self) -> Option<&str> {
        self.comment_error.as_deref()
    }

    /// Built from the key rather than taken from the answer.
    pub fn page_url(&self) -> String {
        format!(
            "https://github.com/{}/pull/{}",
            self.key.repository, self.key.number
        )
    }

    pub fn open_on_github(&self, cx: &mut App) {
        cx.open_url(&self.page_url());
    }

    /// `@login → owner/repo (visibility)`: who writes, and where, from this
    /// tab's comment, merge and checkout. A link can open a pull request of
    /// a repository other than the one picked in the GitHub tab.
    pub fn target_line(&self) -> Option<String> {
        let login = self.login.as_deref()?;
        Some(target_line(login, &self.key.repository, self.visibility))
    }

    /// The account this tab was opened under is gone: signed out, refused,
    /// or replaced by another.
    pub fn session_ended(&self, cx: &App) -> bool {
        !is_live_session(&self.session, cx)
    }

    /// Hands a refused token to the GitHub tab.
    pub(crate) fn note_error(&self, error: &GithubError, cx: &mut App) {
        report_if_revoked(error, &self.session, cx);
    }

    /// `false`, with the reason shown, once the session is over.
    fn check_session(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.session_ended(cx) {
            return true;
        }
        self.comment_error = Some(session_ended_message("pull request"));
        cx.notify();
        false
    }

    fn read_visibility(&mut self, cx: &mut Context<Self>) {
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
                        this.visibility_error = Some(error.to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn merge_availability(&self) -> Option<MergeAvailability> {
        self.details.as_ref().map(merge_availability)
    }

    /// The pull request, its first page of comments, and who is signed in.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.reload_details(cx);
        self.fetch_comments(None, cx);
        if self.visibility.is_none() {
            self.read_visibility(cx);
        }
        if self.login.is_none() && self.login_task.is_none() {
            let backend = self.backend.clone();
            self.login_task = Some(cx.spawn(async move |this, cx| {
                let viewer = backend.viewer().await;
                this.update(cx, |this, cx| {
                    match viewer {
                        Ok(viewer) => this.login = Some(viewer.login.into()),
                        Err(error) => {
                            this.note_error(&error, cx);
                            log::info!("could not read the signed-in account: {error}");
                        }
                    }
                    this.login_task = None;
                    cx.notify();
                })
                .ok();
            }));
        }
    }

    fn reload_details(&mut self, cx: &mut Context<Self>) {
        let load = self.next_load();
        self.awaited_details = load;
        let backend = self.backend.clone();
        let key = self.key.clone();
        self.details_task = Some(cx.spawn(async move |this, cx| {
            let details = backend
                .pull_request_details(&key.repository, key.number)
                .await;
            this.update(cx, |this, cx| {
                if this.awaited_details != load {
                    return;
                }
                match details {
                    Ok(details) => {
                        let unchanged_body = this
                            .details
                            .as_ref()
                            .is_some_and(|shown| shown.body == details.body);
                        if !unchanged_body {
                            this.body = Some(this.markdown(&details.body, cx));
                        }
                        this.details = Some(details);
                        this.error = None;
                    }
                    Err(error) => {
                        this.note_error(&error, cx);
                        this.error = Some(error);
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn load_more_comments(&mut self, cx: &mut Context<Self>) {
        if let Some(cursor) = self.next_comment_cursor.clone()
            && !self.loading_comments
        {
            self.fetch_comments(Some(cursor), cx);
        }
    }

    fn next_load(&mut self) -> u64 {
        self.load_counter = self.load_counter.wrapping_add(1).max(1);
        self.load_counter
    }

    pub(crate) fn markdown(&self, source: &str, cx: &mut Context<Self>) -> Entity<GithubMarkdown> {
        let repository = self.key.repository.clone();
        let backend = self.backend.clone();
        let session = self.session.clone();
        cx.new(|cx| GithubMarkdown::new(source, &repository, backend, session, cx))
    }

    fn fetch_comments(&mut self, cursor: Option<String>, cx: &mut Context<Self>) {
        let load = self.next_load();
        self.awaited_comments = load;
        self.loading_comments = true;
        let appending = cursor.is_some();
        let backend = self.backend.clone();
        let key = self.key.clone();
        self.comments_task = Some(cx.spawn(async move |this, cx| {
            let page = backend
                .list_comments(&key.repository, key.number, cursor)
                .await;
            this.update(cx, |this, cx| {
                if this.awaited_comments != load {
                    return;
                }
                this.loading_comments = false;
                match page {
                    Ok(page) => {
                        if !appending {
                            this.comments.clear();
                        }
                        let mut known = this
                            .comments
                            .iter()
                            .map(|known| known.comment.id)
                            .collect::<HashSet<_>>();
                        for comment in page.items {
                            if known.insert(comment.id) {
                                let body = this.markdown(&comment.body, cx);
                                this.comments.push(PullRequestComment { comment, body });
                            }
                        }
                        this.next_comment_cursor = page.next_cursor;
                        this.comments_error = None;
                    }
                    Err(error) => {
                        this.note_error(&error, cx);
                        this.comments_error = Some(error);
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
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

    /// Sends what is in the comment box, through the token's write queue.
    /// The box is emptied only once GitHub has the comment.
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
                        this.comments.push(PullRequestComment { comment, body });
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

    /// Opens the merge dialog; nothing is sent until it is confirmed there.
    pub fn request_merge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(details), Some(login)) = (self.details.clone(), self.login.clone()) else {
            return;
        };
        let MergeAvailability::Available(options) = merge_availability(&details) else {
            return;
        };
        if !self.check_session(cx) {
            return;
        }
        let session = self.session.clone();
        let Some(workspace) = Workspace::for_window(window, cx) else {
            return;
        };
        let key = self.key.clone();
        let backend = self.backend.clone();
        let detail = cx.weak_entity();
        // Asked for from a click while this tab is being updated; the dialog
        // opens once that is over.
        window.defer(cx, move |window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.toggle_modal(window, cx, |_window, cx| {
                    MergeDialog::new(key, login, backend, details, options, detail, session, cx)
                });
            });
        });
    }
}
