use std::collections::HashSet;
use std::sync::Arc;

use editor::Editor;
use github::{
    Comment, GithubBackend, GithubError, Issue, IssueType, Label, Milestone, RepositoryPermission,
    RepositoryVisibility,
};
use gpui::{
    App, AppContext as _, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, Focusable,
    SharedString, Subscription, Task, Window,
};
use language::LanguageRegistry;
use ui::IconName;
use workspace::Workspace;
use workspace::item::Item;

use crate::github_markdown::{GithubMarkdown, issue_page};
use crate::github_view::DetailKey;
use crate::github_view_session::{SessionStamp, observe_session, replace_stale_tab};
use crate::issues::assignee_picker::AssigneePicker;
use crate::issues::sub_issue_section::EpicSection;
use crate::markdown_composer::MarkdownComposer;
use crate::untrusted_text::reveal_hidden_characters;

pub(crate) struct IssueComment {
    pub comment: Comment,
    pub body: Entity<GithubMarkdown>,
}

/// One issue in its own tab: found again by its [`DetailKey`], never
/// restored at start-up. Comments are read a page at a time.
pub struct IssueDetailView {
    pub(crate) key: DetailKey,
    pub(crate) backend: Arc<dyn GithubBackend>,
    /// The sign-in `backend` belongs to, as the list or link that opened
    /// the tab had it; once it is over the tab refuses to write and offers
    /// to reopen.
    pub(crate) session: SessionStamp,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) issue: Option<Issue>,
    pub(crate) body: Option<Entity<GithubMarkdown>>,
    pub(crate) issue_error: Option<GithubError>,
    pub(crate) comments: Vec<IssueComment>,
    pub(crate) next_comment_cursor: Option<String>,
    pub(crate) comments_error: Option<GithubError>,
    pub(crate) loading_comments: bool,
    /// Names each load, so a late answer to an older one is dropped.
    load_counter: u64,
    awaited_issue: u64,
    awaited_comments: u64,
    issue_task: Option<Task<()>>,
    comments_task: Option<Task<()>>,
    pub(crate) languages: Option<Arc<LanguageRegistry>>,
    /// The signed-in account, for "Assign me", read with the first load.
    pub(crate) login: Option<SharedString>,
    pub(crate) edit: Option<IssueEdit>,
    pub(crate) comment_editor: Option<Entity<Editor>>,
    /// Write and Preview tabs and the toolbar, around `comment_editor`.
    pub(crate) comment_composer: Option<Entity<MarkdownComposer>>,
    pub(crate) comment_error: Option<String>,
    pub(crate) sending_comment: bool,
    pub(crate) assignee_picker: Option<(Entity<AssigneePicker>, Subscription)>,
    /// The labels and milestones the menus offer, read with the first load.
    pub(crate) labels: Vec<Label>,
    pub(crate) milestones: Vec<Milestone>,
    /// Empty when the repository's owner has no issue types.
    pub(crate) issue_types: Vec<IssueType>,
    /// `None` until read, or when GitHub did not say.
    pub(crate) permission: Option<RepositoryPermission>,
    pub(crate) epic: EpicSection,
    /// A label, assignee, milestone or state change on its way.
    pub(crate) writing: bool,
    pub(crate) write_error: Option<String>,
    /// What GitHub ignored, read back after a write.
    pub(crate) write_notice: Option<String>,
    pub(crate) write_task: Option<Task<()>>,
    /// The title and description save, apart from `write_task` so neither
    /// cancels the other.
    pub(crate) edit_task: Option<Task<()>>,
    pub(crate) comment_task: Option<Task<()>>,
    pub(crate) options_task: Option<Task<()>>,
    pub(crate) visibility: Option<RepositoryVisibility>,
    pub(crate) visibility_error: Option<GithubError>,
    pub(crate) visibility_task: Option<Task<()>>,
    _session_subscription: Subscription,
}

/// The title and description being edited, and the `updated_at` they were
/// taken from, to notice someone else's edit before overwriting it.
pub(crate) struct IssueEdit {
    pub title: Entity<Editor>,
    pub body: Entity<Editor>,
    /// Write and Preview tabs and the toolbar, around `body`.
    pub description: Entity<MarkdownComposer>,
    pub base_updated_at: String,
    pub error: Option<String>,
    pub saving: bool,
}

pub enum IssueDetailEvent {}

impl EventEmitter<IssueDetailEvent> for IssueDetailView {}

impl Focusable for IssueDetailView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Item for IssueDetailView {
    type Event = IssueDetailEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        match &self.issue {
            Some(issue) => format!(
                "#{} {}",
                issue.number,
                reveal_hidden_characters(&issue.title)
            )
            .into(),
            None => format!("#{}", self.key.number).into(),
        }
    }

    fn tab_tooltip_text(&self, _cx: &App) -> Option<SharedString> {
        Some(format!("{}#{}", self.key.repository, self.key.number).into())
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<ui::Icon> {
        Some(ui::Icon::new(IconName::Github))
    }
}

/// Brings the issue's tab forward, or opens one under `session`, the
/// opener's, and reads the issue.
pub fn open_issue_detail(
    workspace: &mut Workspace,
    backend: Arc<dyn GithubBackend>,
    session: SessionStamp,
    key: DetailKey,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Entity<IssueDetailView> {
    // Collected first: `items_of_type` borrows the workspace and
    // `activate_item` needs it back.
    let existing = workspace
        .items_of_type::<IssueDetailView>(cx)
        .find(|view| view.read(cx).key == key);
    if let Some(existing) = &existing
        && !existing.read(cx).session_ended(cx)
    {
        workspace.activate_item(existing, true, true, window, cx);
        return existing.clone();
    }
    let languages = workspace.project().read(cx).languages().clone();
    let view = cx.new(|cx| {
        let mut view = IssueDetailView::new(key, backend, session, cx);
        view.languages = Some(languages);
        view.reload(cx);
        view
    });
    match existing {
        Some(stale) => replace_stale_tab(workspace, &stale, &view, window, cx),
        None => workspace.add_item_to_active_pane(Box::new(view.clone()), None, true, window, cx),
    }
    view
}

impl IssueDetailView {
    /// Silent until [`Self::reload`].
    pub fn new(
        key: DetailKey,
        backend: Arc<dyn GithubBackend>,
        session: SessionStamp,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            key,
            backend,
            session,
            focus_handle: cx.focus_handle(),
            issue: None,
            body: None,
            issue_error: None,
            comments: Vec::new(),
            next_comment_cursor: None,
            comments_error: None,
            loading_comments: false,
            load_counter: 0,
            awaited_issue: 0,
            awaited_comments: 0,
            issue_task: None,
            comments_task: None,
            languages: None,
            login: None,
            edit: None,
            comment_editor: None,
            comment_composer: None,
            comment_error: None,
            sending_comment: false,
            assignee_picker: None,
            labels: Vec::new(),
            milestones: Vec::new(),
            issue_types: Vec::new(),
            permission: None,
            epic: EpicSection::default(),
            writing: false,
            write_error: None,
            write_notice: None,
            write_task: None,
            edit_task: None,
            comment_task: None,
            options_task: None,
            visibility: None,
            visibility_error: None,
            visibility_task: None,
            _session_subscription: observe_session(cx),
        }
    }

    pub fn key(&self) -> &DetailKey {
        &self.key
    }

    pub fn issue(&self) -> Option<&Issue> {
        self.issue.as_ref()
    }

    pub fn comments(&self) -> impl Iterator<Item = &Comment> {
        self.comments.iter().map(|comment| &comment.comment)
    }

    pub fn has_more_comments(&self) -> bool {
        self.next_comment_cursor.is_some()
    }

    /// The issue's address, built from its key rather than taken from the
    /// answer.
    pub fn page_url(&self) -> String {
        issue_page(&self.key)
    }

    pub fn open_on_github(&self, cx: &mut App) {
        cx.open_url(&self.page_url());
    }

    pub fn copy_link(&self, cx: &mut App) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.page_url()));
    }

    /// The issue, its first page of comments, its sub-issues and parent,
    /// again.
    /// Nothing is read once the session is over: the token may have been
    /// disconnected.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self.session_ended(cx) {
            return;
        }
        if self.visibility.is_none() {
            self.load_visibility(cx);
        }
        self.reload_issue(cx);
        self.fetch_comments(None, cx);
        self.load_epic(cx);
        if self.options_task.is_none() {
            self.load_options(cx);
        }
    }

    pub(crate) fn reload_issue(&mut self, cx: &mut Context<Self>) {
        let load = self.next_load();
        self.awaited_issue = load;
        let backend = self.backend.clone();
        let key = self.key.clone();
        self.issue_task = Some(cx.spawn(async move |this, cx| {
            let issue = backend.issue(&key.repository, key.number).await;
            this.update(cx, |this, cx| {
                if this.awaited_issue != load {
                    return;
                }
                match issue {
                    Ok(issue) => this.show_issue(issue, cx),
                    Err(error) => {
                        this.note_error(&error, cx);
                        this.issue_error = Some(error);
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
            && !self.session_ended(cx)
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
                            if !known.insert(comment.id) {
                                continue;
                            }
                            let body = this.markdown(&comment.body, cx);
                            this.comments.push(IssueComment { comment, body });
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
}
