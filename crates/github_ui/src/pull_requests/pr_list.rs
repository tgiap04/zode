use std::collections::HashSet;
use std::sync::Arc;

use github::{PullRequest, PullRequestStateFilter};
use gpui::{App, Context, EventEmitter, Task, Window};
use workspace::Workspace;

use crate::github_view::{DetailKey, GithubContext};
use crate::github_view_session::report_if_revoked;
use crate::issues::ListLoad;
use crate::pull_requests::open_pull_request_detail;

/// "Mine", as GitHub's own pull request page offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullRequestMine {
    Authored,
    ReviewRequested,
}

impl PullRequestMine {
    pub const ALL: [Self; 2] = [Self::Authored, Self::ReviewRequested];

    pub fn label(self) -> &'static str {
        match self {
            Self::Authored => "Created by me",
            Self::ReviewRequested => "Review requested",
        }
    }

    pub fn admits(self, pull_request: &PullRequest, login: &str) -> bool {
        match self {
            Self::Authored => pull_request.author.as_deref() == Some(login),
            Self::ReviewRequested => pull_request
                .requested_reviewers
                .iter()
                .any(|reviewer| reviewer == login),
        }
    }
}

pub enum PullRequestListEvent {
    /// An answer came back; the rate limits it reported may have moved.
    Loaded,
}

/// The pull requests of one repository, for one signed-in account, read a
/// page at a time and only when asked. "Mine" narrows the loaded pages.
pub struct PullRequestList {
    pub(crate) context: GithubContext,
    state: PullRequestStateFilter,
    mine: Option<PullRequestMine>,
    rows: Vec<PullRequest>,
    next_cursor: Option<String>,
    load: ListLoad,
    load_counter: u64,
    awaited_load: u64,
    load_task: Option<Task<()>>,
}

impl EventEmitter<PullRequestListEvent> for PullRequestList {}

impl PullRequestList {
    /// Silent until [`Self::reload`].
    pub fn new(context: GithubContext) -> Self {
        Self {
            context,
            state: PullRequestStateFilter::Open,
            mine: None,
            rows: Vec::new(),
            next_cursor: None,
            load: ListLoad::Idle,
            load_counter: 0,
            awaited_load: 0,
            load_task: None,
        }
    }

    pub fn context(&self) -> &GithubContext {
        &self.context
    }

    /// Made for `context`: the same repository, through the same backend.
    pub fn serves(&self, context: &GithubContext) -> bool {
        self.context.repository == context.repository
            && self.context.login == context.login
            && std::ptr::addr_eq(
                Arc::as_ptr(&self.context.backend),
                Arc::as_ptr(&context.backend),
            )
    }

    pub fn state(&self) -> PullRequestStateFilter {
        self.state
    }

    pub fn mine(&self) -> Option<PullRequestMine> {
        self.mine
    }

    pub fn load_state(&self) -> &ListLoad {
        &self.load
    }

    pub fn has_more(&self) -> bool {
        self.next_cursor.is_some()
    }

    pub fn visible_rows(&self) -> Vec<&PullRequest> {
        let login = self.context.login.as_ref();
        self.rows
            .iter()
            .filter(|pull_request| {
                self.mine
                    .is_none_or(|mine| mine.admits(pull_request, login))
            })
            .collect()
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.fetch(None, cx);
    }

    pub fn load_more(&mut self, cx: &mut Context<Self>) {
        if let Some(cursor) = self.next_cursor.clone()
            && self.load == ListLoad::Idle
        {
            self.fetch(Some(cursor), cx);
        }
    }

    pub fn set_state(&mut self, state: PullRequestStateFilter, cx: &mut Context<Self>) {
        self.state = state;
        self.fetch(None, cx);
    }

    /// Narrows what is loaded; nothing is sent.
    pub fn set_mine(&mut self, mine: Option<PullRequestMine>, cx: &mut Context<Self>) {
        self.mine = mine;
        cx.notify();
    }

    pub fn open_pull_request(&self, number: u64, window: &mut Window, cx: &mut App) {
        let Some(workspace) = Workspace::for_window(window, cx) else {
            return;
        };
        let backend = self.context.backend.clone();
        let session = self.context.session.clone();
        let key = DetailKey {
            repository: self.context.repository.clone(),
            number,
        };
        // Called from a row's click, while this list is being updated.
        window.defer(cx, move |window, cx| {
            workspace.update(cx, |workspace, cx| {
                open_pull_request_detail(workspace, backend, session, key, window, cx);
            });
        });
    }

    fn fetch(&mut self, cursor: Option<String>, cx: &mut Context<Self>) {
        self.load_counter = self.load_counter.wrapping_add(1).max(1);
        let load = self.load_counter;
        self.awaited_load = load;
        let appending = cursor.is_some();
        self.load = if appending {
            ListLoad::LoadingMore
        } else {
            ListLoad::Loading
        };
        let backend = self.context.backend.clone();
        let repository = self.context.repository.clone();
        let state = self.state;
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let page = backend.list_pull_requests(&repository, state, cursor).await;
            this.update(cx, |this, cx| {
                if this.awaited_load != load {
                    return;
                }
                match page {
                    Ok(page) => {
                        if appending {
                            let known = this
                                .rows
                                .iter()
                                .map(|pull_request| pull_request.number)
                                .collect::<HashSet<_>>();
                            this.rows.extend(
                                page.items
                                    .into_iter()
                                    .filter(|pull_request| !known.contains(&pull_request.number)),
                            );
                        } else {
                            this.rows = page.items;
                        }
                        this.next_cursor = page.next_cursor;
                        this.load = ListLoad::Idle;
                    }
                    Err(error) => {
                        report_if_revoked(&error, &this.context.session, cx);
                        this.load = ListLoad::Failed(error);
                    }
                }
                cx.emit(PullRequestListEvent::Loaded);
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }
}
