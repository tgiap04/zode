//! The Pull requests section: the list, a pull request's own tab with its
//! checks, reviews, files and comments, checkout of a pull request (a fork's
//! only after a warning), and a merge that only runs from its confirmation dialog.

mod branch_name;
mod check_link;
mod fork_checkout;
mod merge_modal;
mod merge_modal_render;
mod pr_checkout;
mod pr_detail_checkout;
mod pr_detail_render;
mod pr_detail_view;
mod pr_labels;
mod pr_list;
mod pr_list_render;

pub use branch_name::{BranchNameError, default_branch_name, validate_branch_name};
pub use merge_modal::{
    MergeAvailability, MergeDialog, MergeOptions, MergePhase, merge_availability,
};
pub use pr_checkout::{
    CheckoutBlocker, CheckoutGit, CheckoutOutcome, CheckoutRequest, checkout_blocker,
    matching_remote, run_checkout,
};
pub use pr_detail_checkout::CheckoutState;
pub use pr_detail_view::{PullRequestDetailView, open_pull_request_detail};
pub use pr_labels::checks_label;
pub use pr_list::{PullRequestList, PullRequestListEvent, PullRequestMine};

use gpui::{AppContext as _, Context, Entity};

use crate::github_view::{GithubSection, GithubView};

impl GithubView {
    pub fn pull_request_list(&self) -> Option<&Entity<PullRequestList>> {
        self.pull_request_list.as_ref()
    }

    /// Keeps the list in step with the account and repository: dropped while
    /// either is missing, made (and read) when the section is shown for a new
    /// pair. Nothing is read before the section is shown.
    pub(crate) fn sync_pull_request_list(&mut self, cx: &mut Context<Self>) {
        let Some(context) = self.context() else {
            self.pull_request_list = None;
            self._pull_request_list_subscription = None;
            return;
        };
        if self
            .pull_request_list
            .as_ref()
            .is_some_and(|list| list.read(cx).serves(&context))
        {
            return;
        }
        if self.section != GithubSection::PullRequests {
            self.pull_request_list = None;
            self._pull_request_list_subscription = None;
            return;
        }
        let list = cx.new(|cx| {
            let mut list = PullRequestList::new(context);
            list.reload(cx);
            list
        });
        self._pull_request_list_subscription = Some(cx.subscribe(
            &list,
            |this, list: Entity<PullRequestList>, PullRequestListEvent::Loaded, cx| {
                this.rate_limits = list.read(cx).context().backend.rate_limits();
                cx.notify();
            },
        ));
        self.pull_request_list = Some(list);
    }
}

#[cfg(test)]
mod fork_checkout_tests;
#[cfg(test)]
mod pull_request_tests;
