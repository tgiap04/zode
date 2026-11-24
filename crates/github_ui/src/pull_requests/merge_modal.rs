//! Merging a pull request, only from a dialog that names the account, the
//! repository, the pull request and the branch it lands on. On confirm the
//! pull request is read again and merged only if it is still the merge that
//! was shown: same head, same base, not a draft, and still mergeable with
//! the chosen method. The head branch is deleted only when it is ours to
//! delete and still at that commit.

use std::sync::Arc;

use github::{
    GithubBackend, MergeMethod, MergeStateStatus, Mergeable, PullRequestDetails, PullRequestState,
    RepositoryVisibility,
};
use gpui::{
    App, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, SharedString, Task,
    WeakEntity, Window,
};
use workspace::{DismissDecision, ModalView};

use crate::github_view::DetailKey;
use crate::github_view_session::{
    SessionStamp, is_live_session, report_if_revoked, session_ended_message,
};
use crate::pull_requests::pr_detail_view::PullRequestDetailView;
use crate::untrusted_text::reveal_hidden_characters;

/// Whether, and how, the pull request can be merged from Zode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MergeAvailability {
    Available(MergeOptions),
    /// The base branch merges through a merge queue, which Zode does not
    /// drive: the merge is left to GitHub.
    MergeQueue,
    /// GitHub is still working out whether it merges cleanly.
    Checking,
    /// Why it cannot be merged now.
    Unavailable(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeOptions {
    /// Only what the repository allows, in GitHub's order.
    pub methods: Vec<MergeMethod>,
    /// "Delete the branch after merging" is offered.
    pub offers_branch_deletion: bool,
    /// The repository deletes merged head branches itself.
    pub github_deletes_branch: bool,
}

pub fn merge_availability(details: &PullRequestDetails) -> MergeAvailability {
    let pull_request = &details.pull_request;
    if pull_request.state != PullRequestState::Open {
        return MergeAvailability::Unavailable("Only an open pull request can be merged.");
    }
    if details.is_merge_queue_enabled || details.is_in_merge_queue {
        return MergeAvailability::MergeQueue;
    }
    if pull_request.is_draft || details.merge_state_status == MergeStateStatus::Draft {
        return MergeAvailability::Unavailable("A draft cannot be merged.");
    }
    match details.mergeable {
        Mergeable::Unknown => return MergeAvailability::Checking,
        Mergeable::Conflicting => {
            return MergeAvailability::Unavailable("It has conflicts with the base branch.");
        }
        Mergeable::Mergeable => {}
    }
    // Blocked is what an admin could merge past; Zode never does.
    match details.merge_state_status {
        MergeStateStatus::Clean | MergeStateStatus::HasHooks | MergeStateStatus::Unstable => {}
        MergeStateStatus::Unknown => return MergeAvailability::Checking,
        MergeStateStatus::Blocked => {
            return MergeAvailability::Unavailable("Required reviews or checks are not satisfied.");
        }
        MergeStateStatus::Behind => {
            return MergeAvailability::Unavailable("The head branch is behind its base.");
        }
        MergeStateStatus::Dirty | MergeStateStatus::Draft => {
            return MergeAvailability::Unavailable("It cannot be merged cleanly.");
        }
    }
    let methods = details.settings.allowed_methods();
    if methods.is_empty() {
        return MergeAvailability::Unavailable("The repository allows no merge method.");
    }
    let head_is_ours = details.head_branch.as_ref().is_some_and(|head| {
        !head.is_protected
            && head.oid.as_deref() == Some(pull_request.head_ref_oid.as_str())
            && details.settings.default_branch.as_deref() != Some(head.name.as_str())
    });
    let github_deletes_branch = details.settings.delete_branch_on_merge;
    MergeAvailability::Available(MergeOptions {
        methods,
        offers_branch_deletion: !pull_request.is_cross_repository
            && details.viewer_can_delete_head_ref
            && head_is_ours
            && !github_deletes_branch,
        github_deletes_branch,
    })
}

/// The options the pull request, read again, can still be merged with by
/// `method`; or, when it is no longer the merge that was confirmed, why.
fn recheck(
    shown: &PullRequestDetails,
    current: &PullRequestDetails,
    method: MergeMethod,
) -> Result<MergeOptions, MergePhase> {
    let (shown, now) = (&shown.pull_request, &current.pull_request);
    if now.head_ref_oid != shown.head_ref_oid {
        return Err(MergePhase::HeadChanged);
    }
    let changed = |reason: String| Err(MergePhase::NoLongerMergeable(reason));
    if now.base_ref_name != shown.base_ref_name {
        return changed(format!(
            "it now targets {} instead of {}.",
            reveal_hidden_characters(&now.base_ref_name),
            reveal_hidden_characters(&shown.base_ref_name)
        ));
    }
    if now.is_draft != shown.is_draft {
        return changed("it was turned back into a draft.".to_string());
    }
    match merge_availability(current) {
        MergeAvailability::Available(options) if options.methods.contains(&method) => Ok(options),
        MergeAvailability::Available(_) => changed(format!(
            "the repository no longer allows “{}”.",
            method.label()
        )),
        MergeAvailability::MergeQueue => changed("it now merges through a merge queue.".into()),
        MergeAvailability::Checking => {
            changed("GitHub is checking again whether it can merge.".into())
        }
        MergeAvailability::Unavailable(reason) => changed(reason.to_lowercase()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MergePhase {
    Confirming,
    Merging,
    Merged {
        /// What became of the head branch, when its deletion was asked for.
        branch_note: Option<String>,
    },
    /// Someone pushed since the dialog opened: nothing was merged.
    HeadChanged,
    /// Read again, the pull request is no longer what was confirmed (its
    /// base, draft state or mergeability changed): nothing was merged.
    NoLongerMergeable(String),
    Failed(String),
}

pub struct MergeDialog {
    pub(crate) key: DetailKey,
    pub(crate) login: SharedString,
    backend: Arc<dyn GithubBackend>,
    pub(crate) details: PullRequestDetails,
    pub(crate) options: MergeOptions,
    pub(crate) method: MergeMethod,
    pub(crate) delete_branch: bool,
    pub(crate) phase: MergePhase,
    pub(crate) visibility: Option<RepositoryVisibility>,
    /// Why the repository's visibility could not be read; the merge waits
    /// for it.
    pub(crate) visibility_error: Option<String>,
    detail: WeakEntity<PullRequestDetailView>,
    pub(crate) focus_handle: FocusHandle,
    /// The sign-in the dialog was opened under; a later one is not this
    /// backend's account.
    session: SessionStamp,
    visibility_task: Option<Task<()>>,
    merge_task: Option<Task<()>>,
}

impl EventEmitter<DismissEvent> for MergeDialog {}

impl Focusable for MergeDialog {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ModalView for MergeDialog {
    fn on_before_dismiss(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> DismissDecision {
        DismissDecision::Dismiss(self.phase != MergePhase::Merging)
    }
}

impl MergeDialog {
    pub(crate) fn new(
        key: DetailKey,
        login: SharedString,
        backend: Arc<dyn GithubBackend>,
        details: PullRequestDetails,
        options: MergeOptions,
        detail: WeakEntity<PullRequestDetailView>,
        session: SessionStamp,
        cx: &mut Context<Self>,
    ) -> Self {
        let method = options
            .methods
            .first()
            .copied()
            .unwrap_or(MergeMethod::Merge);
        let mut dialog = Self {
            key,
            login,
            backend,
            details,
            options,
            method,
            delete_branch: false,
            phase: MergePhase::Confirming,
            visibility: None,
            visibility_error: None,
            detail,
            focus_handle: cx.focus_handle(),
            session,
            visibility_task: None,
            merge_task: None,
        };
        dialog.read_visibility(cx);
        dialog
    }

    /// The merge waits for the visibility, so a failure is shown with a
    /// retry rather than leaving the button disabled for good.
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
                        report_if_revoked(&error, &this.session, cx);
                        this.visibility_error = Some(error.to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn phase(&self) -> &MergePhase {
        &self.phase
    }

    pub fn visibility_error(&self) -> Option<&str> {
        self.visibility_error.as_deref()
    }

    pub fn retry_visibility(&mut self, cx: &mut Context<Self>) {
        if self.visibility.is_none() {
            self.read_visibility(cx);
            cx.notify();
        }
    }

    pub fn options(&self) -> &MergeOptions {
        &self.options
    }

    pub fn set_method(&mut self, method: MergeMethod, cx: &mut Context<Self>) {
        if self.options.methods.contains(&method) {
            self.method = method;
            cx.notify();
        }
    }

    pub fn set_delete_branch(&mut self, delete: bool, cx: &mut Context<Self>) {
        self.delete_branch = delete && self.options.offers_branch_deletion;
        cx.notify();
    }

    /// Reads the pull request again first and sends nothing unless it is
    /// still the merge the dialog showed (see [`recheck`]).
    pub fn confirm(&mut self, cx: &mut Context<Self>) {
        // The dialog names the repository it merges in, public or private;
        // nothing is sent before that has been read.
        if self.phase != MergePhase::Confirming || self.visibility.is_none() {
            return;
        }
        if !is_live_session(&self.session, cx) {
            self.phase = MergePhase::Failed(session_ended_message("pull request"));
            cx.notify();
            return;
        }
        self.phase = MergePhase::Merging;
        let backend = self.backend.clone();
        let key = self.key.clone();
        let shown = self.details.clone();
        let shown_head = shown.pull_request.head_ref_oid.clone();
        let pull_request_id = shown.pull_request.id.clone();
        let method = self.method;
        let session = self.session.clone();
        let delete = self.delete_branch.then(|| {
            (
                self.details.settings.repository_node_id.clone(),
                self.details.pull_request.head_ref_name.clone(),
            )
        });
        self.merge_task = Some(cx.spawn(async move |this, cx| {
            let current = backend
                .pull_request_details(&key.repository, key.number)
                .await;
            let phase = match current.map(|current| recheck(&shown, &current, method)) {
                Err(error) => {
                    cx.update(|cx| report_if_revoked(&error, &session, cx));
                    MergePhase::Failed(format!("Could not read it again: {error}"))
                }
                Ok(Err(changed)) => changed,
                // A sign-out may land while the pull request is read again.
                Ok(Ok(_)) if !cx.update(|cx| is_live_session(&session, cx)) => {
                    MergePhase::Failed(session_ended_message("pull request"))
                }
                Ok(Ok(options_now)) => {
                    let delete = delete.filter(|_| options_now.offers_branch_deletion);
                    match backend
                        .merge_pull_request(pull_request_id, method, shown_head.clone())
                        .await
                    {
                        Err(error) => {
                            cx.update(|cx| report_if_revoked(&error, &session, cx));
                            MergePhase::Failed(error.to_string())
                        }
                        Ok(()) => MergePhase::Merged {
                            branch_note: match delete {
                                None => None,
                                Some((_, branch))
                                    if !cx.update(|cx| is_live_session(&session, cx)) =>
                                {
                                    Some(format!(
                                        "Merged, but {branch} was kept. {}",
                                        session_ended_message("pull request")
                                    ))
                                }
                                Some((repository_id, branch)) => Some(
                                    match backend
                                        .delete_branch_at(repository_id, branch.clone(), shown_head)
                                        .await
                                    {
                                        Ok(()) => format!("Deleted {branch}."),
                                        Err(error) => {
                                            cx.update(|cx| report_if_revoked(&error, &session, cx));
                                            format!("Merged, but {branch} was kept: {error}")
                                        }
                                    },
                                ),
                            },
                        },
                    }
                }
            };
            this.update(cx, |this, cx| {
                this.phase = phase;
                if let Some(detail) = this.detail.upgrade() {
                    detail.update(cx, |detail, cx| detail.reload(cx));
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Before the merge is sent this sends nothing; while it is on its way
    /// the dialog stays.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.phase != MergePhase::Merging {
            cx.emit(DismissEvent);
        }
    }
}
