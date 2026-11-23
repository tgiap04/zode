//! The GitHub tab: an editor tab opened from the sidebar rail, holding the
//! sign-in (Zode's device flow or the GitHub CLI), the repository picker, and
//! the Issues / Pull requests / Projects sections.
//!
//! The tab is not serialized, so it is never restored at start-up and opening
//! Zode sends nothing to GitHub.

mod avatar;
mod device_flow;
mod github_markdown;
mod github_settings;
mod github_view;
mod github_view_render;
mod github_view_session;
mod issues;
mod markdown_composer;
mod markdown_composer_render;
mod markdown_format;
mod projects;
mod pull_requests;
mod repo_picker;
mod sign_in;
#[cfg(any(test, feature = "test-support"))]
mod test_support;
mod untrusted_text;

pub use avatar::{AVATAR_HOST, initials, trusted_avatar_url};
pub use device_flow::{DeviceCode, DeviceFlowError, SCOPES, is_github_page};
pub use github_markdown::{
    GithubMarkdown, LinkTarget, classify_link, github_markdown_options, link_issue_references,
    link_texts, shown_host_mismatch,
};
pub use github_settings::GithubPanelSettings;
pub use github_view::{
    AuthState, DetailKey, DeviceFlowStep, GithubContext, GithubSection, GithubView,
    GithubViewEvent, ProjectAccess, Session, SignInOptions,
};
pub use github_view_render::{rate_limit_summary, revoked_advice};
pub use github_view_session::{
    SessionStamp, current_session_backend, is_live_session, report_if_revoked,
    session_ended_message,
};
pub use issues::{
    BulkIssueAction, BulkIssueDialog, BulkItem, BulkItemStatus, BulkPhase, CreateIssueDialog,
    IssueDetailView, IssueDraftText, IssueList, IssueListEvent, IssueListFilters, IssueMenuEntry,
    ListLoad, ListPlaceholder, MineFilter, label_color, matches_query, open_issue_detail,
    updated_label,
};
pub use projects::{
    BoardColumn, ColumnKey, MISSING_SCOPE_GUIDANCE, PROJECT_ITEM_CAP, PROJECT_READ_SCOPE,
    ProjectsLoad, ProjectsSection, ProjectsSectionEvent, board_columns, cell_text, column_field,
    item_title, shows_board, sorted_items, table_fields,
};
pub use pull_requests::{
    BranchNameError, CheckoutBlocker, CheckoutGit, CheckoutOutcome, CheckoutRequest, CheckoutState,
    MergeAvailability, MergeDialog, MergeOptions, MergePhase, PullRequestDetailView,
    PullRequestList, PullRequestListEvent, PullRequestMine, checkout_blocker, checks_label,
    default_branch_name, matching_remote, merge_availability, open_pull_request_detail,
    run_checkout, validate_branch_name,
};
pub use repo_picker::NO_REPOSITORY_EXPLANATION;
pub use sign_in::{BackendFactory, GithubEnvironment, TokenStorage, session_token};
#[cfg(any(test, feature = "test-support"))]
pub use test_support::{FakeGhHost, MemoryKeychain, ScriptedHttp, use_test_environment};
pub use untrusted_text::{is_hidden_character, reveal_hidden_characters};

use gpui::{App, Context, Entity, SharedString, Window};
use ui::prelude::*;
use workspace::Workspace;
use workspace::item::Item;

pub fn init(cx: &mut App) {
    sign_in::install_client_id(cx);
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace.register_action(|workspace, _: &zode_actions::github::Toggle, window, cx| {
            if put_away(workspace, window, cx) {
                return;
            }
            open(workspace, window, cx);
        });
        workspace.register_action(|workspace, _: &zode_actions::github::SignIn, window, cx| {
            let view = open(workspace, window, cx);
            view.update(cx, |view, cx| {
                if !matches!(view.auth(), AuthState::SigningIn(_)) {
                    view.sign_in_with_github(cx);
                }
            });
        });
        workspace.register_action(
            |workspace, _: &zode_actions::github::Disconnect, window, cx| {
                let view = open(workspace, window, cx);
                view.update(cx, |view, cx| view.disconnect(cx));
            },
        );
    })
    .detach();
}

impl Item for GithubView {
    type Event = GithubViewEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        "GitHub".into()
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<ui::Icon> {
        Some(ui::Icon::new(IconName::Github))
    }
}

/// Brings the GitHub tab forward, or opens one and looks for a token.
///
/// The settings page's Sign in and Disconnect go through here too: whatever
/// they start replaces the token lookup before it runs, so nothing is sent.
pub fn open(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Entity<GithubView> {
    // Collected first: `items_of_type` borrows the workspace and
    // `activate_item` needs it back.
    let existing = workspace.items_of_type::<GithubView>(cx).next();
    if let Some(existing) = existing {
        workspace.activate_item(&existing, true, true, window, cx);
        return existing;
    }
    let project = workspace.project().clone();
    let view = cx.new(|cx| {
        let mut view = GithubView::new(project, cx);
        view.connect(cx);
        view
    });
    workspace.add_item_to_active_pane(Box::new(view.clone()), None, true, window, cx);
    view
}

/// Steps back to the tab that was being read when the GitHub tab is the one
/// in front. Put away, not closed, so a second press of the rail button does
/// not throw away the signed-in state. Same shape as the container tab's.
fn put_away(workspace: &Workspace, window: &mut Window, cx: &mut App) -> bool {
    let pane = workspace.active_pane().clone();
    // One read through one borrow: reaching for the pane again partway
    // through is how a stale read turns into an abort.
    let previous = {
        let pane = pane.read(cx);
        let showing_github = pane
            .active_item()
            .and_then(|item| item.downcast::<GithubView>())
            .is_some();
        if !showing_github {
            return false;
        }
        // The last entry is this tab; the one before it is where to go. An
        // item closed since leaves its entry behind, so entries are checked.
        pane.activation_history()
            .iter()
            .rev()
            .skip(1)
            .find_map(|entry| {
                pane.items()
                    .position(|item| item.item_id() == entry.entity_id)
            })
    };
    let Some(index) = previous else {
        return false;
    };
    pane.update(cx, |pane, cx| {
        pane.activate_item(index, true, true, window, cx);
    });
    true
}

#[cfg(test)]
mod avatar_tests;
#[cfg(test)]
mod device_flow_tests;
#[cfg(test)]
mod github_markdown_tests;
#[cfg(test)]
mod github_view_session_tests;
#[cfg(test)]
mod github_view_tests;
#[cfg(test)]
mod markdown_composer_tests;
#[cfg(test)]
mod markdown_format_tests;
#[cfg(test)]
mod repo_picker_tests;
#[cfg(test)]
mod untrusted_text_tests;
