//! Several selected issues closed or labelled at once, only after a dialog
//! that names every one of them. The writes go one by one through the
//! token's write queue, so the dialog shows progress, can stop between two
//! writes, and ends with what happened to each issue.

use github::{GithubError, IssueState, IssueUpdate, RepositoryVisibility};
use gpui::{
    App, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, Subscription, Task,
    WeakEntity, Window,
};
use workspace::{DismissDecision, ModalView};

use crate::github_view::GithubContext;
use crate::github_view_session::{HeldSessions, is_live_session, report_if_revoked};
use crate::issues::issue_actions::{BulkIssueAction, target_line};
use crate::issues::issue_list::IssueList;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BulkItemStatus {
    Waiting,
    Running,
    Done,
    /// GitHub answered without an error but did not apply the change.
    NotApplied,
    Failed(String),
    /// Never sent: stopped before its turn.
    Cancelled,
    /// Stopped while waiting for GitHub; it may or may not have been applied.
    Interrupted,
    /// Never sent: the account was signed out or changed first.
    SignedOut,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BulkItem {
    pub number: u64,
    pub title: String,
    pub status: BulkItemStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BulkPhase {
    Confirming,
    Running,
    Finished,
}

pub struct BulkIssueDialog {
    pub(crate) context: GithubContext,
    list: WeakEntity<IssueList>,
    pub(crate) action: BulkIssueAction,
    pub(crate) items: Vec<BulkItem>,
    pub(crate) phase: BulkPhase,
    pub(crate) visibility: Option<RepositoryVisibility>,
    pub(crate) visibility_error: Option<GithubError>,
    pub(crate) focus_handle: FocusHandle,
    visibility_task: Option<Task<()>>,
    run_task: Option<Task<()>>,
    _session_subscription: Subscription,
}

impl EventEmitter<DismissEvent> for BulkIssueDialog {}

impl Focusable for BulkIssueDialog {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ModalView for BulkIssueDialog {
    /// A click outside must not throw away a run or its report: Cancel stops
    /// it and Close dismisses it.
    fn on_before_dismiss(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> DismissDecision {
        DismissDecision::Dismiss(self.phase != BulkPhase::Running)
    }
}

impl BulkIssueDialog {
    pub(crate) fn new(
        context: GithubContext,
        list: WeakEntity<IssueList>,
        action: BulkIssueAction,
        issues: Vec<(u64, String)>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut dialog = Self {
            context,
            list,
            action,
            items: issues
                .into_iter()
                .map(|(number, title)| BulkItem {
                    number,
                    title,
                    status: BulkItemStatus::Waiting,
                })
                .collect(),
            phase: BulkPhase::Confirming,
            visibility: None,
            visibility_error: None,
            focus_handle: cx.focus_handle(),
            visibility_task: None,
            run_task: None,
            // A sign-out mid-run stops the writes still to come.
            _session_subscription: cx.observe_global::<HeldSessions>(|this, cx| {
                this.stop_if_signed_out(cx);
            }),
        };
        dialog.load_visibility(cx);
        dialog
    }

    /// Confirming waits for this: the dialog must name the repository it
    /// writes to, public or private.
    fn load_visibility(&mut self, cx: &mut Context<Self>) {
        self.visibility_error = None;
        let backend = self.context.backend.clone();
        let repository = self.context.repository.clone();
        self.visibility_task = Some(cx.spawn(async move |this, cx| {
            let visibility = backend.repository_visibility(&repository).await;
            this.update(cx, |this, cx| {
                match visibility {
                    Ok(visibility) => this.visibility = Some(visibility),
                    Err(error) => {
                        report_if_revoked(&error, &this.context.session, cx);
                        this.visibility_error = Some(error);
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    pub fn visibility_error(&self) -> Option<&GithubError> {
        self.visibility_error.as_ref()
    }

    pub fn retry_visibility(&mut self, cx: &mut Context<Self>) {
        if self.phase == BulkPhase::Confirming {
            self.load_visibility(cx);
        }
    }

    pub fn items(&self) -> &[BulkItem] {
        &self.items
    }

    pub fn phase(&self) -> BulkPhase {
        self.phase
    }

    pub fn target_line(&self) -> String {
        target_line(
            &self.context.login,
            &self.context.repository,
            self.visibility,
        )
    }

    /// Settled items, and all of them.
    pub fn progress(&self) -> (usize, usize) {
        let settled = self
            .items
            .iter()
            .filter(|item| {
                !matches!(
                    item.status,
                    BulkItemStatus::Waiting | BulkItemStatus::Running
                )
            })
            .count();
        (settled, self.items.len())
    }

    /// `2 done, 1 failed, 1 cancelled`, leaving out the counts that are zero.
    pub fn report(&self) -> String {
        let count = |wanted: fn(&BulkItemStatus) -> bool| {
            self.items
                .iter()
                .filter(|item| wanted(&item.status))
                .count()
        };
        let parts = [
            (count(|status| *status == BulkItemStatus::Done), "done"),
            (
                count(|status| *status == BulkItemStatus::NotApplied),
                "not applied",
            ),
            (
                count(|status| matches!(status, BulkItemStatus::Failed(_))),
                "failed",
            ),
            (
                count(|status| {
                    matches!(
                        status,
                        BulkItemStatus::Cancelled | BulkItemStatus::Interrupted
                    )
                }),
                "cancelled",
            ),
            (
                count(|status| *status == BulkItemStatus::SignedOut),
                "not sent: signed out",
            ),
        ];
        parts
            .iter()
            .filter(|(count, _)| *count > 0)
            .map(|(count, what)| format!("{count} {what}"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Starts the writes. Nothing is sent before this, and not before the
    /// repository the dialog names has been confirmed.
    pub fn confirm(&mut self, cx: &mut Context<Self>) {
        if self.phase != BulkPhase::Confirming || self.visibility.is_none() {
            return;
        }
        if !is_live_session(&self.context.session, cx) {
            self.stop_signed_out(cx);
            return;
        }
        self.phase = BulkPhase::Running;
        let backend = self.context.backend.clone();
        let repository = self.context.repository.clone();
        let action = self.action.clone();
        let session = self.context.session.clone();
        let numbers = self
            .items
            .iter()
            .map(|item| item.number)
            .collect::<Vec<_>>();
        self.run_task = Some(cx.spawn(async move |this, cx| {
            for (index, number) in numbers.into_iter().enumerate() {
                let started = this.update(cx, |this, cx| {
                    if !is_live_session(&this.context.session, cx) {
                        // This task is the one running: it ends by returning.
                        this.mark_signed_out(cx);
                        return false;
                    }
                    this.set_status(index, BulkItemStatus::Running, cx);
                    true
                });
                if !matches!(started, Ok(true)) {
                    return;
                }
                let (status, change) = match &action {
                    BulkIssueAction::Close(reason) => {
                        let update = IssueUpdate {
                            state: Some(IssueState::Closed),
                            state_reason: Some(*reason),
                            ..IssueUpdate::default()
                        };
                        match backend.update_issue(&repository, number, update).await {
                            Ok(issue) if issue.state == IssueState::Closed => {
                                (BulkItemStatus::Done, Some(RowChange::Issue(issue)))
                            }
                            Ok(issue) => {
                                (BulkItemStatus::NotApplied, Some(RowChange::Issue(issue)))
                            }
                            Err(error) => {
                                cx.update(|cx| report_if_revoked(&error, &session, cx));
                                (BulkItemStatus::Failed(error.to_string()), None)
                            }
                        }
                    }
                    BulkIssueAction::AddLabel(name) => {
                        match backend
                            .add_labels(&repository, number, vec![name.clone()])
                            .await
                        {
                            Ok(labels) => {
                                let applied = labels.iter().any(|label| &label.name == name);
                                let status = if applied {
                                    BulkItemStatus::Done
                                } else {
                                    BulkItemStatus::NotApplied
                                };
                                (status, Some(RowChange::Labels(labels)))
                            }
                            Err(error) => {
                                cx.update(|cx| report_if_revoked(&error, &session, cx));
                                (BulkItemStatus::Failed(error.to_string()), None)
                            }
                        }
                    }
                };
                this.update(cx, |this, cx| {
                    this.set_status(index, status, cx);
                    if let Some(change) = change {
                        this.list
                            .update(cx, |list, cx| match change {
                                RowChange::Issue(issue) => list.replace_row(issue, cx),
                                RowChange::Labels(labels) => {
                                    list.update_row(number, |issue| issue.labels = labels, cx)
                                }
                            })
                            .ok();
                    }
                })
                .ok();
            }
            this.update(cx, |this, cx| this.finish(cx)).ok();
        }));
        cx.notify();
    }

    /// Before the run: closes the dialog. During it: stops before the next
    /// write and reports what was not done. After it: closes the dialog.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        match self.phase {
            BulkPhase::Confirming | BulkPhase::Finished => cx.emit(DismissEvent),
            BulkPhase::Running => {
                self.run_task = None;
                for item in &mut self.items {
                    item.status = match item.status {
                        BulkItemStatus::Running => BulkItemStatus::Interrupted,
                        BulkItemStatus::Waiting => BulkItemStatus::Cancelled,
                        ref settled => settled.clone(),
                    };
                }
                self.finish(cx);
            }
        }
    }

    fn stop_if_signed_out(&mut self, cx: &mut Context<Self>) {
        if self.phase == BulkPhase::Running && !is_live_session(&self.context.session, cx) {
            self.stop_signed_out(cx);
        }
    }

    /// Ends the dialog after a sign-out: nothing more is sent, and every
    /// item still waiting says why.
    fn stop_signed_out(&mut self, cx: &mut Context<Self>) {
        self.run_task = None;
        self.mark_signed_out(cx);
    }

    fn mark_signed_out(&mut self, cx: &mut Context<Self>) {
        for item in &mut self.items {
            item.status = match item.status {
                BulkItemStatus::Running => BulkItemStatus::Interrupted,
                BulkItemStatus::Waiting => BulkItemStatus::SignedOut,
                ref settled => settled.clone(),
            };
        }
        self.finish(cx);
    }

    fn set_status(&mut self, index: usize, status: BulkItemStatus, cx: &mut Context<Self>) {
        if let Some(item) = self.items.get_mut(index) {
            item.status = status;
        }
        cx.notify();
    }

    fn finish(&mut self, cx: &mut Context<Self>) {
        self.phase = BulkPhase::Finished;
        self.list
            .update(cx, |list, cx| list.clear_selection(cx))
            .ok();
        cx.notify();
    }
}

enum RowChange {
    Issue(github::Issue),
    Labels(Vec<github::Label>),
}
