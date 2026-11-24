//! What the issue list writes: the "New issue" dialog, a row's right-click
//! menu, and the selection that bulk actions work on.

use std::collections::BTreeSet;

use github::{Issue, IssueState, IssueUpdate, RepositoryPermission, StateReason};
use gpui::{ClipboardItem, Context, DismissEvent, Focusable as _, Pixels, Point, Window};
use ui::ContextMenu;
use workspace::Workspace;

use crate::github_markdown::issue_page;
use crate::github_view::DetailKey;
use crate::github_view_session::is_live_session;
use crate::issues::bulk_issue_actions::BulkIssueDialog;
use crate::issues::issue_actions::{
    BulkIssueAction, IssueMenuEntry, close_reason_label, may_triage, row_menu_entries,
};
use crate::issues::issue_editor::{CreateIssueDialog, CreateIssueOptions, CreateIssueOwner};
use crate::issues::issue_list::IssueList;

pub(crate) const SIGNED_OUT_WRITE: &str =
    "Signed out or account changed. Nothing was sent; reconnect in the GitHub tab, then try again.";

impl IssueList {
    pub fn selected(&self) -> &BTreeSet<u64> {
        &self.selected
    }

    pub fn toggle_selected(&mut self, number: u64, cx: &mut Context<Self>) {
        if !self.selected.remove(&number) {
            self.selected.insert(number);
        }
        cx.notify();
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selected.clear();
        cx.notify();
    }

    pub fn permission(&self) -> Option<RepositoryPermission> {
        self.permission
    }

    /// Bulk actions are offered unless GitHub said the account may not
    /// triage.
    pub fn offers_bulk_actions(&self) -> bool {
        may_triage(self.permission)
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn write_error(&self) -> Option<&str> {
        self.write_error.as_deref()
    }

    pub fn dismiss_messages(&mut self, cx: &mut Context<Self>) {
        self.notice = None;
        self.write_error = None;
        cx.notify();
    }

    /// A newly created issue goes first, where GitHub's newest-first list
    /// puts it, with what GitHub ignored, if anything.
    pub fn insert_created(
        &mut self,
        issue: Issue,
        ignored: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let number = issue.number;
        self.rows.retain(|row| row.number != number);
        self.rows.insert(0, issue);
        self.rows_changed();
        self.saved_draft = Default::default();
        self.notice = Some(match ignored {
            Some(ignored) => format!("Created #{number}. {ignored}"),
            None => format!("Created #{number}."),
        });
        cx.notify();
    }

    pub(crate) fn replace_row(&mut self, issue: Issue, cx: &mut Context<Self>) {
        let number = issue.number;
        self.update_row(number, move |row| *row = issue, cx);
    }

    pub(crate) fn update_row(
        &mut self,
        number: u64,
        change: impl FnOnce(&mut Issue),
        cx: &mut Context<Self>,
    ) {
        if let Some(row) = self.rows.iter_mut().find(|row| row.number == number) {
            change(row);
            self.rows_changed();
            cx.notify();
        }
    }

    /// Reads rows again whose write may or may not have reached GitHub, such
    /// as the one a stopped bulk action was sending, so they show what is
    /// there rather than what was last known.
    pub(crate) fn refresh_rows(&mut self, numbers: Vec<u64>, cx: &mut Context<Self>) {
        for number in numbers {
            if self.row_writes.contains_key(&number) {
                continue;
            }
            let backend = self.context.backend.clone();
            let repository = self.context.repository.clone();
            let task = cx.spawn(async move |this, cx| {
                let issue = backend.issue(&repository, number).await;
                this.update(cx, |this, cx| {
                    this.row_writes.remove(&number);
                    match issue {
                        Ok(issue) => this.replace_row(issue, cx),
                        Err(error) => {
                            this.note_error(&error, cx);
                            this.write_error = Some(format!(
                                "Could not check #{number} after stopping: {error}. \
                                 Reload to see where it stands."
                            ));
                            cx.notify();
                        }
                    }
                })
                .ok();
            });
            self.row_writes.insert(number, task);
        }
        cx.notify();
    }

    pub fn row_menu(&self, number: u64) -> Vec<IssueMenuEntry> {
        self.rows
            .iter()
            .find(|row| row.number == number)
            .map(|issue| row_menu_entries(issue, self.permission, &self.context.login))
            .unwrap_or_default()
    }

    /// Closes (with a reason) or reopens one issue, from its row's menu.
    pub fn set_issue_state(
        &mut self,
        number: u64,
        state: IssueState,
        reason: Option<StateReason>,
        cx: &mut Context<Self>,
    ) {
        if self.row_writes.contains_key(&number) {
            self.write_error = Some(format!(
                "#{number} is still being changed. Try again once it finishes."
            ));
            cx.notify();
            return;
        }
        if !is_live_session(&self.context.session, cx) {
            self.write_error = Some(SIGNED_OUT_WRITE.to_string());
            cx.notify();
            return;
        }
        let backend = self.context.backend.clone();
        let repository = self.context.repository.clone();
        let update = IssueUpdate {
            state: Some(state),
            state_reason: reason,
            ..IssueUpdate::default()
        };
        self.write_error = None;
        let task = cx.spawn(async move |this, cx| {
            let answer = backend.update_issue(&repository, number, update).await;
            this.update(cx, |this, cx| {
                this.row_writes.remove(&number);
                match answer {
                    Ok(issue) => this.replace_row(issue, cx),
                    Err(error) => {
                        this.note_error(&error, cx);
                        this.write_error = Some(format!("Could not change #{number}: {error}"));
                        cx.notify();
                    }
                }
            })
            .ok();
        });
        self.row_writes.insert(number, task);
        cx.notify();
    }

    pub fn run_menu_entry(
        &mut self,
        number: u64,
        entry: IssueMenuEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let page = || {
            issue_page(&DetailKey {
                repository: self.context.repository.clone(),
                number,
            })
        };
        match entry {
            IssueMenuEntry::OpenInTab => self.open_issue(number, window, cx),
            IssueMenuEntry::OpenOnGithub => cx.open_url(&page()),
            IssueMenuEntry::CopyLink => cx.write_to_clipboard(ClipboardItem::new_string(page())),
            IssueMenuEntry::Close(reason) => {
                self.set_issue_state(number, IssueState::Closed, Some(reason), cx)
            }
            IssueMenuEntry::Reopen => {
                self.set_issue_state(number, IssueState::Open, Some(StateReason::Reopened), cx)
            }
        }
    }

    pub(crate) fn deploy_row_menu(
        &mut self,
        number: u64,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.row_menu(number);
        let this = cx.entity().downgrade();
        let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
            for entry in entries {
                let label = match &entry {
                    IssueMenuEntry::OpenInTab => "Open in tab",
                    IssueMenuEntry::OpenOnGithub => "Open on GitHub",
                    IssueMenuEntry::CopyLink => "Copy link",
                    IssueMenuEntry::Close(reason) => close_reason_label(*reason),
                    IssueMenuEntry::Reopen => "Reopen",
                };
                if matches!(
                    entry,
                    IssueMenuEntry::Close(StateReason::Completed) | IssueMenuEntry::Reopen
                ) {
                    menu = menu.separator();
                }
                let this = this.clone();
                menu = menu.entry(label, None, move |window, cx| {
                    let entry = entry.clone();
                    this.update(cx, |this, cx| {
                        this.run_menu_entry(number, entry, window, cx)
                    })
                    .ok();
                });
            }
            menu
        });
        window.focus(&menu.focus_handle(cx), cx);
        let subscription = cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
            this.context_menu = None;
            cx.notify();
        });
        self.context_menu = Some((menu, position, subscription));
        cx.notify();
    }

    /// Opens the "New issue" dialog over the workspace.
    pub fn new_issue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = Workspace::for_window(window, cx) else {
            return;
        };
        let languages = workspace.read(cx).project().read(cx).languages().clone();
        let target = (&self.context).into();
        let owner = CreateIssueOwner::List(cx.entity().downgrade());
        let options = CreateIssueOptions {
            labels: self.labels.clone(),
            milestones: self.milestones.clone(),
            issue_types: self.issue_types.clone(),
            draft: self.saved_draft.clone(),
            languages: Some(languages),
        };
        // Called from a click while this list is being updated, and the
        // dialog writes back to it when it closes.
        window.defer(cx, move |window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.toggle_modal(window, cx, move |window, cx| {
                    CreateIssueDialog::new(target, owner, options, window, cx)
                });
            });
        });
    }

    /// Opens the confirmation for `action` over every selected issue, in the
    /// list's order. Nothing is written until it is confirmed.
    pub fn start_bulk(
        &mut self,
        action: BulkIssueAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let issues = self
            .rows
            .iter()
            .filter(|issue| self.selected.contains(&issue.number))
            .map(|issue| (issue.number, issue.title.clone()))
            .collect::<Vec<_>>();
        if issues.is_empty() {
            return;
        }
        let Some(workspace) = Workspace::for_window(window, cx) else {
            return;
        };
        let context = self.context.clone();
        let list = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.toggle_modal(window, cx, move |_window, cx| {
                    BulkIssueDialog::new(context, list, action, issues, cx)
                });
            });
        });
    }
}
