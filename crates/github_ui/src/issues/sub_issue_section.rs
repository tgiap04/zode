//! An issue's place in an epic, in its tab: its sub-issues with their
//! progress, its parent, and the writes that change them. Removing a link
//! waits for a confirmation; moves go through GitHub's `reprioritizeSubIssue`.

use std::collections::HashSet;

use editor::Editor;
use github::{
    GithubError, GithubFuture, GithubResult, Issue, IssueFilter, IssueStateFilter,
    MAX_SUB_ISSUE_DEPTH, MAX_SUB_ISSUES_PER_PARENT, ParentIssue, SubIssuePosition,
};
use gpui::{App, Context, Entity, Task, Window};
use workspace::Workspace;

use crate::issues::IssueDetailView;
use crate::issues::epic_tree::read_sub_issues;
use crate::issues::issue_actions::may_triage;
use crate::issues::issue_editor::{
    CreateIssueDialog, CreateIssueOptions, CreateIssueOwner, IssueTarget,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveDirection {
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerPurpose {
    AddSubIssue,
    SetParent,
}

/// A link waiting for the user to confirm its removal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingRemoval {
    SubIssue(Issue),
    Parent(ParentIssue),
}

/// Finds an existing issue of the repository by `#number` or by words.
pub(crate) struct IssuePicker {
    pub purpose: PickerPurpose,
    /// Made on first draw, since it needs a window.
    pub field: Option<Entity<Editor>>,
    pub results: Vec<Issue>,
    pub searching: bool,
    pub error: Option<String>,
    pub task: Option<Task<()>>,
}

#[derive(Default)]
pub(crate) struct EpicSection {
    pub sub_issues: Vec<Issue>,
    pub sub_issues_error: Option<GithubError>,
    pub parent: Option<ParentIssue>,
    pub parent_error: Option<GithubError>,
    pub pending_removal: Option<PendingRemoval>,
    pub picker: Option<IssuePicker>,
    /// What went wrong with the last sub-issue or parent change, in words.
    pub error: Option<String>,
    pub writing: bool,
    pub load_task: Option<Task<()>>,
    pub write_task: Option<Task<()>>,
}

/// Where a move puts the sub-issue at `index`, or `None` when it is already
/// at that end. Moving to the top names the first sibling as the one to go
/// before, since there is nothing to go after.
pub fn move_position(
    sub_issues: &[Issue],
    index: usize,
    direction: MoveDirection,
) -> Option<SubIssuePosition> {
    if index >= sub_issues.len() {
        return None;
    }
    let node_id = |index: usize| sub_issues.get(index).map(|issue| issue.node_id.clone());
    match direction {
        MoveDirection::Up if index == 0 => None,
        MoveDirection::Up if index == 1 => node_id(0).map(SubIssuePosition::Before),
        MoveDirection::Up => node_id(index - 2).map(SubIssuePosition::After),
        MoveDirection::Down => node_id(index + 1).map(SubIssuePosition::After),
    }
}

fn full_parent_error(number: u64) -> String {
    format!(
        "#{number} already has {MAX_SUB_ISSUES_PER_PARENT} sub-issues, the most GitHub allows. \
         Remove one before adding another."
    )
}

/// A failed sub-issue write in words, naming GitHub's limits when it hit one.
pub fn sub_issue_write_error(error: &GithubError) -> String {
    if let GithubError::Rejected { message } = error {
        let message = message.to_lowercase();
        if message.contains("depth") || message.contains("levels") || message.contains("nested") {
            return format!(
                "GitHub allows sub-issues at most {MAX_SUB_ISSUE_DEPTH} levels deep, and this \
                 change would go deeper. ({error})"
            );
        }
        if message.contains(&MAX_SUB_ISSUES_PER_PARENT.to_string()) {
            return format!(
                "A parent can have at most {MAX_SUB_ISSUES_PER_PARENT} sub-issues. ({error})"
            );
        }
    }
    error.to_string()
}

impl IssueDetailView {
    pub fn sub_issues(&self) -> &[Issue] {
        &self.epic.sub_issues
    }

    pub fn parent_issue(&self) -> Option<&ParentIssue> {
        self.epic.parent.as_ref()
    }

    pub fn epic_error(&self) -> Option<&str> {
        self.epic.error.as_deref()
    }

    pub fn pending_removal(&self) -> Option<&PendingRemoval> {
        self.epic.pending_removal.as_ref()
    }

    pub fn picker_results(&self) -> &[Issue] {
        self.epic
            .picker
            .as_ref()
            .map_or(&[], |picker| picker.results.as_slice())
    }

    /// Adding, removing, moving and re-parenting need triage access. When
    /// GitHub did not say, they are offered and GitHub decides.
    pub fn may_edit_epic(&self) -> bool {
        may_triage(self.permission)
    }

    /// Whether a write can start now: allowed, the issue is loaded, no
    /// other sub-issue write is on its way, and the session is not over (the
    /// tab's banner says so).
    fn ready_for_epic_write(&self, cx: &App) -> Option<&Issue> {
        if !self.may_edit_epic() || self.epic.writing || self.session_ended(cx) {
            return None;
        }
        self.issue.as_ref()
    }

    /// The reason the issue cannot take another sub-issue, if it cannot.
    fn sub_issue_room_error(&self, issue: &Issue) -> Option<String> {
        let count = issue
            .sub_issues_summary
            .total
            .max(self.epic.sub_issues.len() as u64);
        (count >= MAX_SUB_ISSUES_PER_PARENT).then(|| full_parent_error(issue.number))
    }

    pub(crate) fn load_epic(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let key = self.key.clone();
        self.epic.load_task = Some(cx.spawn(async move |this, cx| {
            let sub_issues =
                read_sub_issues(backend.clone(), key.repository.clone(), key.number).await;
            let parent = backend.issue_parent(&key.repository, key.number).await;
            this.update(cx, |this, cx| {
                match sub_issues {
                    Ok(sub_issues) => {
                        this.epic.sub_issues = sub_issues;
                        this.epic.sub_issues_error = None;
                    }
                    Err(error) => {
                        this.note_error(&error, cx);
                        this.epic.sub_issues_error = Some(error);
                    }
                }
                match parent {
                    Ok(parent) => {
                        this.epic.parent = parent;
                        this.epic.parent_error = None;
                    }
                    Err(error) => {
                        this.note_error(&error, cx);
                        this.epic.parent_error = Some(error);
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Runs one sub-issue or parent write through the token's queue, then
    /// reads the issue and its links back.
    fn write_epic(&mut self, write: GithubFuture<()>, cx: &mut Context<Self>) {
        self.epic.writing = true;
        self.epic.error = None;
        self.epic.pending_removal = None;
        self.epic.write_task = Some(cx.spawn(async move |this, cx| {
            let written = write.await;
            this.update(cx, |this, cx| {
                this.epic.writing = false;
                if let Err(error) = &written {
                    this.note_error(error, cx);
                    this.epic.error = Some(sub_issue_write_error(error));
                }
                this.refresh_issue_after_own_write(cx);
                this.load_epic(cx);
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    pub fn open_picker(&mut self, purpose: PickerPurpose, cx: &mut Context<Self>) {
        if !self.may_edit_epic() {
            return;
        }
        self.epic.picker = Some(IssuePicker {
            purpose,
            field: None,
            results: Vec::new(),
            searching: false,
            error: None,
            task: None,
        });
        cx.notify();
    }

    pub fn close_picker(&mut self, cx: &mut Context<Self>) {
        self.epic.picker = None;
        cx.notify();
    }

    /// `#12` or `12` reads that issue; anything else asks GitHub's search.
    /// The issue itself and those already linked to it are left out.
    pub fn search_picker(&mut self, text: String, cx: &mut Context<Self>) {
        let text = text.trim().to_string();
        let Some(picker) = &mut self.epic.picker else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let mut excluded = self
            .epic
            .sub_issues
            .iter()
            .map(|issue| issue.number)
            .collect::<HashSet<_>>();
        excluded.insert(self.key.number);
        if let Some(parent) = &self.epic.parent
            && parent.repository == self.key.repository
        {
            excluded.insert(parent.number);
        }
        let number = text.trim_start_matches('#').parse::<u64>().ok();
        picker.searching = true;
        picker.error = None;
        let backend = self.backend.clone();
        let repository = self.key.repository.clone();
        picker.task = Some(cx.spawn(async move |this, cx| {
            let found: GithubResult<Vec<Issue>> = match number {
                Some(number) => backend
                    .issue(&repository, number)
                    .await
                    .map(|issue| vec![issue]),
                None => {
                    let filter = IssueFilter {
                        state: IssueStateFilter::All,
                        ..IssueFilter::default()
                    };
                    backend
                        .search_issues(&repository, &filter, &text, None)
                        .await
                        .map(|page| page.items)
                }
            };
            this.update(cx, |this, cx| {
                let Some(picker) = &mut this.epic.picker else {
                    return;
                };
                picker.searching = false;
                match found {
                    Ok(issues) => {
                        picker.results = issues
                            .into_iter()
                            .filter(|issue| !excluded.contains(&issue.number))
                            .collect();
                    }
                    Err(GithubError::NotFound) if number.is_some() => {
                        picker.results.clear();
                        let number = number.unwrap_or_default();
                        picker.error = Some(format!("{repository} has no issue #{number}."));
                    }
                    Err(error) => picker.error = Some(error.to_string()),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// What clicking a picker result does, by the picker's purpose.
    pub(crate) fn pick(&mut self, issue: Issue, cx: &mut Context<Self>) {
        match self.epic.picker.as_ref().map(|picker| picker.purpose) {
            Some(PickerPurpose::AddSubIssue) => self.add_sub_issue(issue, cx),
            Some(PickerPurpose::SetParent) => self.set_parent(issue, cx),
            None => {}
        }
    }

    pub fn add_sub_issue(&mut self, sub_issue: Issue, cx: &mut Context<Self>) {
        let Some(issue) = self.ready_for_epic_write(cx) else {
            return;
        };
        if let Some(error) = self.sub_issue_room_error(issue) {
            self.epic.error = Some(error);
            cx.notify();
            return;
        }
        let write = self
            .backend
            .add_sub_issue(&self.key.repository, self.key.number, sub_issue.id);
        self.epic.picker = None;
        self.write_epic(write, cx);
    }

    /// Opens the "New issue" dialog; the issue it creates is then added as a
    /// sub-issue of this one.
    pub fn create_sub_issue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(issue) = self.ready_for_epic_write(cx) else {
            return;
        };
        if let Some(error) = self.sub_issue_room_error(issue) {
            self.epic.error = Some(error);
            cx.notify();
            return;
        }
        let Some(login) = self.login.clone() else {
            self.epic.error = Some(
                "Zode is still reading which account is signed in; try again in a moment."
                    .to_string(),
            );
            cx.notify();
            return;
        };
        let Some(workspace) = Workspace::for_window(window, cx) else {
            return;
        };
        let target = IssueTarget {
            backend: self.backend.clone(),
            repository: self.key.repository.clone(),
            login,
            session: self.session.clone(),
        };
        let owner = CreateIssueOwner::SubIssueOf {
            detail: cx.entity().downgrade(),
            parent_number: self.key.number,
        };
        let options = CreateIssueOptions {
            labels: self.labels.clone(),
            milestones: self.milestones.clone(),
            issue_types: self.issue_types.clone(),
            draft: Default::default(),
            languages: self.languages.clone(),
        };
        // Called from a click while this tab is being updated, and the
        // dialog writes back to it once the issue exists.
        window.defer(cx, move |window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.toggle_modal(window, cx, move |window, cx| {
                    CreateIssueDialog::new(target, owner, options, window, cx)
                });
            });
        });
    }

    /// The dialog created `issue` under this one; `linked` says whether it
    /// was then added as a sub-issue.
    pub(crate) fn sub_issue_created(
        &mut self,
        issue: Issue,
        linked: GithubResult<()>,
        notice: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.epic.error = linked.err().map(|error| {
            format!(
                "Created #{}, but could not add it as a sub-issue: {}",
                issue.number,
                sub_issue_write_error(&error)
            )
        });
        self.write_notice = notice;
        self.refresh_issue_after_own_write(cx);
        self.load_epic(cx);
        cx.notify();
    }

    /// Makes `parent` this issue's parent, replacing the one it has.
    pub fn set_parent(&mut self, parent: Issue, cx: &mut Context<Self>) {
        let Some(issue) = self.ready_for_epic_write(cx) else {
            return;
        };
        if parent.number == issue.number {
            return;
        }
        let write = self
            .backend
            .replace_parent(&self.key.repository, parent.number, issue.id);
        self.epic.picker = None;
        self.write_epic(write, cx);
    }

    /// By node id: sub-issues may come from several repositories, where the
    /// same number names different issues.
    pub fn request_remove_sub_issue(&mut self, node_id: &str, cx: &mut Context<Self>) {
        if self.ready_for_epic_write(cx).is_none() {
            return;
        }
        self.epic.pending_removal = self
            .epic
            .sub_issues
            .iter()
            .find(|sub_issue| sub_issue.node_id == node_id)
            .cloned()
            .map(PendingRemoval::SubIssue);
        cx.notify();
    }

    pub fn request_clear_parent(&mut self, cx: &mut Context<Self>) {
        if self.ready_for_epic_write(cx).is_none() {
            return;
        }
        self.epic.pending_removal = self.epic.parent.clone().map(PendingRemoval::Parent);
        cx.notify();
    }

    pub fn cancel_removal(&mut self, cx: &mut Context<Self>) {
        self.epic.pending_removal = None;
        cx.notify();
    }

    pub fn confirm_removal(&mut self, cx: &mut Context<Self>) {
        let Some(issue) = self.ready_for_epic_write(cx) else {
            return;
        };
        let write = match self.epic.pending_removal.clone() {
            Some(PendingRemoval::SubIssue(sub_issue)) => {
                self.backend
                    .remove_sub_issue(&self.key.repository, self.key.number, sub_issue.id)
            }
            Some(PendingRemoval::Parent(parent)) => {
                self.backend
                    .remove_sub_issue(&parent.repository, parent.number, issue.id)
            }
            None => return,
        };
        self.write_epic(write, cx);
    }

    pub fn move_sub_issue(
        &mut self,
        node_id: &str,
        direction: MoveDirection,
        cx: &mut Context<Self>,
    ) {
        let Some(issue) = self.ready_for_epic_write(cx) else {
            return;
        };
        let Some(index) = self
            .epic
            .sub_issues
            .iter()
            .position(|sub_issue| sub_issue.node_id == node_id)
        else {
            return;
        };
        let Some(position) = move_position(&self.epic.sub_issues, index, direction) else {
            return;
        };
        let Some(moved) = self
            .epic
            .sub_issues
            .get(index)
            .map(|sub_issue| sub_issue.node_id.clone())
        else {
            return;
        };
        let write = self
            .backend
            .reprioritize_sub_issue(issue.node_id.clone(), moved, position);
        self.write_epic(write, cx);
    }
}
