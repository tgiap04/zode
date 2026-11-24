//! The list's issue tree: a parent row shows how many of its sub-issues are
//! done and opens to show them, read from GitHub only when opened.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use github::{
    GithubBackend, GithubError, GithubResult, Issue, MAX_SUB_ISSUE_DEPTH,
    MAX_SUB_ISSUES_PER_PARENT, RepositoryId, SubIssuesSummary,
};
use gpui::{Context, Task};

use crate::github_view::DetailKey;
use crate::issues::issue_list::IssueList;
use crate::issues::paged_reads::read_pages;

/// Pages read for one parent's sub-issues: GitHub allows 100 of them, so a
/// longer chain of pages means the cursor is not leading anywhere.
const MAX_SUB_ISSUE_PAGES: usize = 10;

/// The sub-issues under an opened row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChildrenLoad {
    Loading,
    Loaded(Vec<Issue>),
    Failed(GithubError),
}

/// One row of the list as drawn: an issue and how deep it sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub depth: usize,
    pub key: DetailKey,
    pub issue: Issue,
}

/// One line of the list as drawn, all of one height: an issue, or what an
/// opened row's sub-issues are doing while they are not shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ListEntry {
    Row(TreeRow),
    Status { depth: usize, text: String },
}

/// The opened rows. Forgotten when the list is read again, so a reload never
/// shows sub-issues from before it.
#[derive(Default)]
pub(crate) struct EpicTree {
    opened: HashMap<DetailKey, ChildrenLoad>,
    /// Kept so a read is dropped with its row when the row is closed.
    tasks: HashMap<DetailKey, Task<()>>,
}

/// `2/5 done`, or `None` for an issue without sub-issues.
pub fn progress_label(summary: SubIssuesSummary) -> Option<String> {
    (summary.total > 0).then(|| format!("{}/{} done", summary.completed, summary.total))
}

/// The repository an issue belongs to, read from its page address. A
/// sub-issue may live in another repository of the same owner.
pub fn issue_repository(issue: &Issue) -> Option<RepositoryId> {
    let path = issue.html_url.strip_prefix("https://github.com/")?;
    let mut parts = path.split('/');
    let (owner, name, kind) = (parts.next()?, parts.next()?, parts.next()?);
    if kind != "issues" {
        return None;
    }
    RepositoryId::new(owner, name)
}

/// Every sub-issue of one issue, page after page, up to GitHub's limit.
pub(crate) async fn read_sub_issues(
    backend: Arc<dyn GithubBackend>,
    repository: RepositoryId,
    number: u64,
) -> GithubResult<Vec<Issue>> {
    let read = read_pages(
        MAX_SUB_ISSUE_PAGES,
        MAX_SUB_ISSUES_PER_PARENT as usize,
        |cursor| backend.list_sub_issues(&repository, number, cursor),
    )
    .await?;
    Ok(read.items)
}

impl IssueList {
    /// Opens a parent row, reading its sub-issues the first time, or closes
    /// it.
    pub fn toggle_sub_issues(&mut self, key: DetailKey, cx: &mut Context<Self>) {
        self.rows_changed();
        if self.epic_tree.opened.remove(&key).is_some() {
            self.epic_tree.tasks.remove(&key);
            cx.notify();
            return;
        }
        self.epic_tree
            .opened
            .insert(key.clone(), ChildrenLoad::Loading);
        let backend = self.context.backend.clone();
        let task = cx.spawn({
            let key = key.clone();
            async move |this, cx| {
                let read = read_sub_issues(backend, key.repository.clone(), key.number).await;
                this.update(cx, |this, cx| {
                    this.rows_changed();
                    if let Some(load) = this.epic_tree.opened.get_mut(&key) {
                        *load = match read {
                            Ok(sub_issues) => ChildrenLoad::Loaded(sub_issues),
                            Err(error) => ChildrenLoad::Failed(error),
                        };
                    }
                    cx.notify();
                })
                .ok();
            }
        });
        self.epic_tree.tasks.insert(key, task);
        cx.notify();
    }

    pub fn sub_issues_of(&self, key: &DetailKey) -> Option<&ChildrenLoad> {
        self.epic_tree.opened.get(key)
    }

    pub(crate) fn forget_opened_rows(&mut self) {
        self.epic_tree = EpicTree::default();
        self.rows_changed();
    }

    /// The key a row is found and opened by: its own repository, which for a
    /// sub-issue may not be the list's.
    pub(crate) fn row_key(&self, issue: &Issue) -> DetailKey {
        DetailKey {
            repository: issue_repository(issue).unwrap_or_else(|| self.context.repository.clone()),
            number: issue.number,
        }
    }

    /// The visible rows with the sub-issues of every opened row under it.
    pub fn tree_rows(&self) -> Vec<TreeRow> {
        self.build_entries()
            .into_iter()
            .filter_map(|entry| match entry {
                ListEntry::Row(row) => Some(row),
                ListEntry::Status { .. } => None,
            })
            .collect()
    }

    pub(crate) fn build_entries(&self) -> Vec<ListEntry> {
        let mut entries = Vec::new();
        for issue in self.visible_rows() {
            self.push_entries(issue, 0, &mut entries);
        }
        entries
    }

    /// What the list draws, built again only after something it is built
    /// from changed: drawing happens on every frame, and filtering every row
    /// and copying it each time is the cost of a long list.
    pub(crate) fn ensure_rows(&mut self) -> Rc<Vec<ListEntry>> {
        if let Some(entries) = &self.entries {
            return entries.clone();
        }
        let entries = Rc::new(self.build_entries());
        #[cfg(test)]
        {
            self.rebuilds += 1;
        }
        self.entries = Some(entries.clone());
        entries
    }

    fn push_entries(&self, issue: &Issue, depth: usize, entries: &mut Vec<ListEntry>) {
        let key = self.row_key(issue);
        let opened = self.epic_tree.opened.get(&key);
        entries.push(ListEntry::Row(TreeRow {
            depth,
            key,
            issue: issue.clone(),
        }));
        let text = match opened {
            Some(ChildrenLoad::Loading) => Some("Loading sub-issues…".to_string()),
            Some(ChildrenLoad::Failed(error)) => Some(error.to_string()),
            Some(ChildrenLoad::Loaded(_)) | None => None,
        };
        if let Some(text) = text {
            entries.push(ListEntry::Status { depth, text });
        }
        // GitHub stops nesting at this depth; the bound also keeps a cycle
        // in a broken answer from recursing forever.
        if depth + 1 >= MAX_SUB_ISSUE_DEPTH {
            return;
        }
        if let Some(ChildrenLoad::Loaded(sub_issues)) = opened {
            for sub_issue in sub_issues {
                self.push_entries(sub_issue, depth + 1, entries);
            }
        }
    }
}
