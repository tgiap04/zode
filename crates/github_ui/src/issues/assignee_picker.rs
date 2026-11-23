//! The assignee field: type part of a login or name and pick from the
//! accounts GitHub says the repository's issues can be assigned to.
//!
//! Typing is debounced, so a burst of keys sends one query; a new query
//! drops the one still waiting, and an answer that is no longer awaited is
//! thrown away.

use std::sync::Arc;
use std::time::Duration;

use editor::{Editor, EditorEvent};
use github::{GithubBackend, GithubUser, RepositoryId};
use gpui::{App, Context, Entity, EventEmitter, Focusable, Subscription, Task, Window};

use crate::github_view_session::{SessionStamp, is_live_session, report_if_revoked};
use crate::issues::issue_editor::single_line_editor;

/// How long typing must pause before GitHub is asked.
pub const ASSIGNEE_SEARCH_DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssigneeResults {
    /// Nothing asked yet.
    Idle,
    Searching,
    Found(Vec<GithubUser>),
    Failed(String),
}

pub enum AssigneePickerEvent {
    /// The user chose this account; the owner assigns or unassigns it.
    Picked(GithubUser),
}

/// Where the picker searches, and under which sign-in.
#[derive(Clone)]
pub(crate) struct AssigneeSource {
    pub backend: Arc<dyn GithubBackend>,
    pub repository: RepositoryId,
    pub session: SessionStamp,
}

pub struct AssigneePicker {
    source: AssigneeSource,
    pub(crate) query_editor: Entity<Editor>,
    query: String,
    results: AssigneeResults,
    /// Logins already chosen, shown with a check; picking one again
    /// removes it.
    pub(crate) selected: Vec<String>,
    pub(crate) highlighted: usize,
    open: bool,
    search_counter: u64,
    awaited_search: u64,
    search_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<AssigneePickerEvent> for AssigneePicker {}

impl Focusable for AssigneePicker {
    fn focus_handle(&self, cx: &App) -> gpui::FocusHandle {
        self.query_editor.focus_handle(cx)
    }
}

impl AssigneePicker {
    pub(crate) fn new(source: AssigneeSource, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query_editor = single_line_editor("Search people by login or name", "", window, cx);
        let edited = cx.subscribe_in(
            &query_editor,
            window,
            |this, editor, event: &EditorEvent, _window, cx| match event {
                EditorEvent::BufferEdited => {
                    let text = editor.read(cx).text(cx);
                    if text != this.query {
                        this.set_query(text, cx);
                    }
                }
                EditorEvent::Focused => this.open(cx),
                EditorEvent::Blurred => this.close(cx),
                _ => {}
            },
        );
        Self {
            source,
            query_editor,
            query: String::new(),
            results: AssigneeResults::Idle,
            selected: Vec::new(),
            highlighted: 0,
            open: false,
            search_counter: 0,
            awaited_search: 0,
            search_task: None,
            _subscriptions: vec![edited],
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn results(&self) -> &AssigneeResults {
        &self.results
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn found(&self) -> &[GithubUser] {
        match &self.results {
            AssigneeResults::Found(users) => users,
            _ => &[],
        }
    }

    pub(crate) fn set_selected(&mut self, logins: Vec<String>) {
        self.selected = logins;
    }

    /// Shows the results, asking for the first assignable accounts the first
    /// time.
    pub fn open(&mut self, cx: &mut Context<Self>) {
        self.open = true;
        if self.results == AssigneeResults::Idle {
            let query = self.query.clone();
            self.set_query(query, cx);
        }
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        cx.notify();
    }

    /// Asks GitHub for `query` once typing pauses.
    pub fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        self.query = query;
        self.open = true;
        self.highlighted = 0;
        if !is_live_session(&self.source.session, cx) {
            self.search_task = None;
            self.results = AssigneeResults::Failed(
                "Signed out or account changed; people cannot be searched.".to_string(),
            );
            cx.notify();
            return;
        }
        self.search_counter = self.search_counter.wrapping_add(1).max(1);
        let search = self.search_counter;
        self.awaited_search = search;
        self.results = AssigneeResults::Searching;
        let backend = self.source.backend.clone();
        let repository = self.source.repository.clone();
        let query = self.query.trim().to_string();
        // Replacing the task drops the one still waiting, so only the last
        // pause in typing reaches GitHub.
        self.search_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(ASSIGNEE_SEARCH_DEBOUNCE)
                .await;
            let found = backend.assignable_users(&repository, &query).await;
            this.update(cx, |this, cx| {
                if this.awaited_search != search {
                    return;
                }
                match found {
                    Ok(users) => this.results = AssigneeResults::Found(users),
                    Err(error) => {
                        report_if_revoked(&error, &this.source.session, cx);
                        this.results = AssigneeResults::Failed(error.to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    pub fn select_next(&mut self, cx: &mut Context<Self>) {
        let count = self.found().len();
        if count > 0 {
            self.highlighted = (self.highlighted + 1) % count;
            cx.notify();
        }
    }

    pub fn select_previous(&mut self, cx: &mut Context<Self>) {
        let count = self.found().len();
        if count > 0 {
            self.highlighted = (self.highlighted + count - 1) % count;
            cx.notify();
        }
    }

    /// Enter: picks the highlighted account.
    pub fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(user) = self.found().get(self.highlighted).cloned() {
            self.pick(user, window, cx);
        }
    }

    /// Hands `user` to the owner and empties the field for the next one.
    pub fn pick(&mut self, user: GithubUser, window: &mut Window, cx: &mut Context<Self>) {
        let had_query = !self.query.trim().is_empty();
        self.query.clear();
        self.query_editor
            .update(cx, |editor, cx| editor.clear(window, cx));
        cx.emit(AssigneePickerEvent::Picked(user));
        // The results were for the text just cleared; the next pick starts
        // from the first assignable people again.
        if had_query {
            self.set_query(String::new(), cx);
        }
        cx.notify();
    }
}
