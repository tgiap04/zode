use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use github::{
    GithubError, Issue, IssueStateFilter, IssueType, Label, Milestone, RepositoryPermission,
};
use gpui::{App, Context, Entity, EventEmitter, Pixels, Point, Subscription, Task, Window};
use ui::ContextMenu;
use workspace::Workspace;

use crate::github_view::{DetailKey, GithubContext};
use crate::github_view_session::report_if_revoked;
use crate::issues::epic_tree::{EpicTree, ListEntry};
use crate::issues::issue_editor::IssueDraftText;
use crate::issues::issue_filters::{IssueListFilters, MineFilter, matches_query};
use crate::issues::issue_list_render::SearchField;
use crate::issues::open_issue_detail;
use crate::issues::paged_reads::{MAX_OPTION_PAGES, options_note, read_pages};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListLoad {
    Idle,
    Loading,
    LoadingMore,
    Failed(GithubError),
}

/// What the list shows in place of rows, or above them for a failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListPlaceholder {
    /// The first page is on its way and nothing is shown yet.
    Loading,
    /// The last load failed; Retry reads it again.
    Failed(GithubError),
    /// Nothing under the chosen filters.
    NoIssues,
    /// Loaded issues exist, but none has the typed text.
    NoMatches(String),
    /// GitHub's search found nothing for this text.
    NoSearchResults(String),
}

pub enum IssueListEvent {
    /// An answer came back; the rate limits it reported may have moved.
    Loaded,
}

/// The issues of one repository, for one signed-in account.
///
/// Rows are read a page at a time and only when asked: on opening, on
/// Reload (which GitHub answers from its ETag at no cost when nothing
/// changed), on a filter change, on "Load more", and on Enter in the search
/// field — the only time the Search API, with its 30-a-minute limit, is used.
pub struct IssueList {
    pub(crate) context: GithubContext,
    filters: IssueListFilters,
    query: String,
    /// The text last searched for with Enter; `None` while the rows are the
    /// plain list.
    searched: Option<String>,
    pub(crate) rows: Vec<Issue>,
    next_cursor: Option<String>,
    load: ListLoad,
    /// Names each load, so an answer that is no longer the one awaited is
    /// dropped rather than shown over a newer one. `0` is "none".
    load_counter: u64,
    awaited_load: u64,
    pub(crate) labels: Vec<Label>,
    pub(crate) milestones: Vec<Milestone>,
    /// There are more labels or milestones than were read.
    labels_cut_short: bool,
    milestones_cut_short: bool,
    /// Empty when the repository's owner has no issue types.
    pub(crate) issue_types: Vec<IssueType>,
    pub(crate) epic_tree: EpicTree,
    /// What the list draws, kept between frames. Dropped by
    /// [`Self::rows_changed`] wherever the rows, the filters, the typed text
    /// or the opened rows change.
    pub(crate) entries: Option<Rc<Vec<ListEntry>>>,
    #[cfg(test)]
    pub(crate) rebuilds: usize,
    pub(crate) search_field: Option<SearchField>,
    load_task: Option<Task<()>>,
    options_task: Option<Task<()>>,
    /// Issue numbers ticked for a bulk action.
    pub(crate) selected: BTreeSet<u64>,
    /// `None` until read, or when GitHub did not say.
    pub(crate) permission: Option<RepositoryPermission>,
    /// What the last write did that the user should know about.
    pub(crate) notice: Option<String>,
    pub(crate) write_error: Option<String>,
    /// Text left in a "New issue" dialog closed without creating it.
    pub(crate) saved_draft: IssueDraftText,
    pub(crate) context_menu: Option<(Entity<ContextMenu>, Point<Pixels>, Subscription)>,
    /// One write per issue: a second to the same issue is refused until
    /// the first settles, while other issues' writes go ahead.
    pub(crate) row_writes: HashMap<u64, Task<()>>,
}

impl EventEmitter<IssueListEvent> for IssueList {}

impl IssueList {
    /// Silent until [`Self::reload`].
    pub fn new(context: GithubContext) -> Self {
        Self {
            context,
            filters: IssueListFilters::default(),
            query: String::new(),
            searched: None,
            rows: Vec::new(),
            next_cursor: None,
            load: ListLoad::Idle,
            load_counter: 0,
            awaited_load: 0,
            labels: Vec::new(),
            milestones: Vec::new(),
            labels_cut_short: false,
            milestones_cut_short: false,
            issue_types: Vec::new(),
            epic_tree: EpicTree::default(),
            entries: None,
            #[cfg(test)]
            rebuilds: 0,
            search_field: None,
            load_task: None,
            options_task: None,
            selected: BTreeSet::new(),
            permission: None,
            notice: None,
            write_error: None,
            saved_draft: IssueDraftText::default(),
            context_menu: None,
            row_writes: HashMap::default(),
        }
    }

    pub fn context(&self) -> &GithubContext {
        &self.context
    }

    /// Whether this list was made for `context`: the same repository and
    /// account, under the same token. A new backend for the same token (a
    /// Reload) still counts, so the selection and draft are kept.
    pub fn serves(&self, context: &GithubContext) -> bool {
        self.context.repository == context.repository
            && self.context.login == context.login
            && self.context.session == context.session
    }

    pub fn filters(&self) -> &IssueListFilters {
        &self.filters
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn searched(&self) -> Option<&str> {
        self.searched.as_deref()
    }

    pub fn load_state(&self) -> &ListLoad {
        &self.load
    }

    pub fn rows(&self) -> &[Issue] {
        &self.rows
    }

    pub fn labels(&self) -> &[Label] {
        &self.labels
    }

    pub fn milestones(&self) -> &[Milestone] {
        &self.milestones
    }

    pub fn issue_types(&self) -> &[IssueType] {
        &self.issue_types
    }

    /// Offered only when the repository's owner has issue types.
    pub fn offers_type_filter(&self) -> bool {
        !self.issue_types.is_empty()
    }

    pub fn has_more(&self) -> bool {
        self.next_cursor.is_some()
    }

    /// Ticked rows the typed text or the filters keep out of view; they are
    /// still selected, and a bulk action takes them.
    pub fn hidden_selected_count(&self) -> usize {
        self.hidden_selected_among(self.visible_rows().iter().map(|issue| issue.number))
    }

    pub(crate) fn hidden_selected_among(&self, visible: impl Iterator<Item = u64>) -> usize {
        let visible = visible.collect::<HashSet<_>>();
        self.selected
            .iter()
            .filter(|number| !visible.contains(number))
            .count()
    }

    /// What the filter menus lack, when GitHub has more labels or milestones
    /// than were read.
    pub fn options_note(&self) -> Option<String> {
        options_note(
            self.labels_cut_short.then_some(self.labels.len()),
            self.milestones_cut_short.then_some(self.milestones.len()),
        )
    }

    #[cfg(test)]
    pub(crate) fn row_rebuilds(&self) -> usize {
        self.rebuilds
    }

    /// The drawn rows are stale: the rows, a filter, the typed text or an
    /// opened row changed.
    pub(crate) fn rows_changed(&mut self) {
        self.entries = None;
    }

    /// The loaded rows the filters and the typed text let through.
    pub fn visible_rows(&self) -> Vec<&Issue> {
        let login = self.context.login.as_ref();
        self.rows
            .iter()
            .filter(|issue| self.filters.admits(issue, login))
            .filter(|issue| matches_query(issue, &self.query))
            .collect()
    }

    /// What stands in for the rows, if anything.
    pub fn placeholder(&self) -> Option<ListPlaceholder> {
        self.placeholder_for(self.visible_rows().is_empty())
    }

    /// [`Self::placeholder`], told whether any row is visible, which the
    /// caller has already worked out.
    pub(crate) fn placeholder_for(&self, no_visible_rows: bool) -> Option<ListPlaceholder> {
        if let ListLoad::Failed(error) = &self.load {
            return Some(ListPlaceholder::Failed(error.clone()));
        }
        if !no_visible_rows {
            return None;
        }
        Some(match (&self.load, &self.searched) {
            (ListLoad::Loading, _) => ListPlaceholder::Loading,
            (_, Some(searched)) => ListPlaceholder::NoSearchResults(searched.clone()),
            _ if !self.query.trim().is_empty() && !self.rows.is_empty() => {
                ListPlaceholder::NoMatches(self.query.clone())
            }
            _ => ListPlaceholder::NoIssues,
        })
    }

    /// The first page again, in whichever mode the list is in, and the labels
    /// and milestones the filters offer.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.load_options(cx);
        self.fetch(None, cx);
    }

    /// The next page. After a page that failed it asks for that page again;
    /// a failed first page leaves no cursor, so there is nothing to ask for.
    pub fn load_more(&mut self, cx: &mut Context<Self>) {
        if let Some(cursor) = self.next_cursor.clone()
            && matches!(self.load, ListLoad::Idle | ListLoad::Failed(_))
        {
            self.fetch(Some(cursor), cx);
        }
    }

    pub fn set_state(&mut self, state: IssueStateFilter, cx: &mut Context<Self>) {
        self.filters.state = state;
        self.fetch(None, cx);
    }

    pub fn set_mine(&mut self, mine: Option<MineFilter>, cx: &mut Context<Self>) {
        self.filters.mine = mine;
        self.fetch(None, cx);
    }

    pub fn set_label(&mut self, label: Option<String>, cx: &mut Context<Self>) {
        self.filters.label = label;
        self.fetch(None, cx);
    }

    pub fn set_milestone(&mut self, milestone: Option<Milestone>, cx: &mut Context<Self>) {
        self.filters.milestone = milestone;
        self.fetch(None, cx);
    }

    /// An issue type's name; `None` lets every type through.
    pub fn set_issue_type_filter(&mut self, issue_type: Option<String>, cx: &mut Context<Self>) {
        self.filters.issue_type = issue_type;
        self.fetch(None, cx);
    }

    /// Narrows the loaded rows as the text is typed; nothing is sent.
    pub fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        self.query = query;
        self.rows_changed();
        cx.notify();
    }

    /// Enter: asks GitHub's search for the typed text, or goes back to the
    /// plain list when the field is empty.
    pub fn search(&mut self, cx: &mut Context<Self>) {
        let text = self.query.trim();
        self.searched = (!text.is_empty()).then(|| text.to_string());
        self.fetch(None, cx);
    }

    pub fn open_issue(&self, number: u64, window: &mut Window, cx: &mut App) {
        let key = DetailKey {
            repository: self.context.repository.clone(),
            number,
        };
        self.open_issue_key(key, window, cx);
    }

    /// Opens an issue that may be in another repository, such as a
    /// sub-issue.
    pub fn open_issue_key(&self, key: DetailKey, window: &mut Window, cx: &mut App) {
        let Some(workspace) = Workspace::for_window(window, cx) else {
            return;
        };
        let backend = self.context.backend.clone();
        let session = self.context.session.clone();
        // Called from a row's click, while this list is being updated; the
        // tab is opened once that is over.
        window.defer(cx, move |window, cx| {
            workspace.update(cx, |workspace, cx| {
                open_issue_detail(workspace, backend, session, key, window, cx);
            });
        });
    }

    /// Hands a refused token to the GitHub tab.
    pub(crate) fn note_error(&self, error: &GithubError, cx: &mut App) {
        report_if_revoked(error, &self.context.session, cx);
    }

    fn next_load(&mut self) -> u64 {
        self.load_counter = self.load_counter.wrapping_add(1).max(1);
        self.load_counter
    }

    /// `None` reads the first page and replaces the rows; a cursor appends.
    fn fetch(&mut self, cursor: Option<String>, cx: &mut Context<Self>) {
        // A filter may have changed just before this.
        self.rows_changed();
        let load = self.next_load();
        self.awaited_load = load;
        let appending = cursor.is_some();
        self.load = if appending {
            ListLoad::LoadingMore
        } else {
            ListLoad::Loading
        };
        let backend = self.context.backend.clone();
        let repository = self.context.repository.clone();
        let filter = self.filters.to_issue_filter(&self.context.login);
        let searched = self.searched.clone();
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let page = match searched {
                Some(text) => {
                    backend
                        .search_issues(&repository, &filter, &text, cursor)
                        .await
                }
                None => backend.list_issues(&repository, &filter, cursor).await,
            };
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
                                .map(|issue| issue.number)
                                .collect::<HashSet<_>>();
                            this.rows.extend(
                                page.items
                                    .into_iter()
                                    .filter(|issue| !known.contains(&issue.number)),
                            );
                        } else {
                            this.forget_opened_rows();
                            this.rows = page.items;
                            let shown = this
                                .rows
                                .iter()
                                .map(|issue| issue.number)
                                .collect::<HashSet<_>>();
                            this.selected.retain(|number| shown.contains(number));
                        }
                        this.next_cursor = page.next_cursor;
                        this.load = ListLoad::Idle;
                    }
                    Err(error) => {
                        this.note_error(&error, cx);
                        // A failed page can be asked for again; a failed first
                        // page cannot, its cursor was the previous list's.
                        if !appending {
                            this.next_cursor = None;
                        }
                        this.load = ListLoad::Failed(error);
                    }
                }
                this.rows_changed();
                cx.emit(IssueListEvent::Loaded);
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// The labels and milestones for the filter menus, page after page up
    /// to a limit. A failure leaves the menus empty; the list itself reports
    /// errors.
    fn load_options(&mut self, cx: &mut Context<Self>) {
        let backend = self.context.backend.clone();
        let repository = self.context.repository.clone();
        self.options_task = Some(cx.spawn(async move |this, cx| {
            let labels = read_pages(MAX_OPTION_PAGES, usize::MAX, |cursor| {
                backend.list_labels(&repository, cursor)
            })
            .await;
            let milestones = read_pages(MAX_OPTION_PAGES, usize::MAX, |cursor| {
                backend.list_milestones(&repository, cursor)
            })
            .await;
            let permission = backend.repository_permission(&repository).await;
            let issue_types = backend.list_issue_types(&repository).await;
            this.update(cx, |this, cx| {
                for error in [
                    labels.as_ref().err(),
                    milestones.as_ref().err(),
                    permission.as_ref().err(),
                    issue_types.as_ref().err(),
                ]
                .into_iter()
                .flatten()
                {
                    this.note_error(error, cx);
                }
                match issue_types {
                    Ok(issue_types) => this.issue_types = issue_types,
                    Err(error) => {
                        log::info!("could not read the repository's issue types: {error}")
                    }
                }
                match permission {
                    Ok(permission) => this.permission = permission,
                    Err(error) => log::info!("could not read the account's role: {error}"),
                }
                match labels {
                    Ok(read) => {
                        this.labels = read.items;
                        this.labels_cut_short = read.cut_short;
                    }
                    Err(error) => log::info!("could not read the repository's labels: {error}"),
                }
                match milestones {
                    Ok(read) => {
                        this.milestones = read.items;
                        this.milestones_cut_short = read.cut_short;
                    }
                    Err(error) => {
                        log::info!("could not read the repository's milestones: {error}")
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }
}
