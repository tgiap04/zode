use std::path::Path;
use std::sync::Arc;

use fs::FakeFs;
use git_ui::askpass_modal::AskPassModal;
use github::{FakeGithubBackend, PullRequestDetails, fake_pull_request, fake_pull_request_details};
use gpui::{Entity, TestAppContext, VisualTestContext};
use project::Project;
use serde_json::json;
use util::path;
use workspace::{MultiWorkspace, Workspace};

use crate::github_view::DetailKey;
use crate::github_view_session::test_session;
use crate::issues::issues_test_support::{init, repository, workspace_window};
use crate::pull_requests::fork_checkout::pull_request_fetch_askpass;
use crate::pull_requests::{CheckoutState, PullRequestDetailView, open_pull_request_detail};

const FORK_HEAD: &str = "f0rk0000";

fn dot_git() -> &'static Path {
    Path::new(path!("/project/.git"))
}

fn fork_details() -> PullRequestDetails {
    let mut pull_request = fake_pull_request(7, "Fix the thing");
    pull_request.head_ref_name = "main".to_string();
    pull_request.head_ref_oid = FORK_HEAD.to_string();
    pull_request.is_cross_repository = true;
    pull_request.author = Some("stranger".to_string());
    let mut details = fake_pull_request_details(pull_request);
    details.head_repository = Some("someone/repo".to_string());
    details
}

/// A project whose `upstream` is the selected repository and whose
/// `upstream` holds the fork's head as `refs/pull/7/head`.
async fn project_window(
    cx: &mut TestAppContext,
) -> (Arc<FakeFs>, Entity<Workspace>, &mut VisualTestContext) {
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/project"),
        json!({ ".git": {}, "README.md": "# project" }),
    )
    .await;
    fs.set_remote_for_repo(dot_git(), "origin", "https://github.com/me/repo.git");
    fs.set_remote_for_repo(dot_git(), "upstream", "https://github.com/octo/repo.git");
    fs.set_branch_name(dot_git(), Some("main"));
    fs.with_git_state(dot_git(), false, |state| {
        state
            .remote_refs
            .entry("upstream".to_string())
            .or_default()
            .insert("refs/pull/7/head".to_string(), FORK_HEAD.to_string());
    })
    .expect("the fake repository exists");
    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace =
        multi_workspace.read_with(cx, |multi_workspace, _| multi_workspace.workspace().clone());
    cx.run_until_parked();
    (fs, workspace, cx)
}

fn open_fork(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Entity<PullRequestDetailView> {
    open_fork_shown_at(FORK_HEAD, workspace, cx)
}

/// The fork's tab, showing its head at `shown_head`.
fn open_fork_shown_at(
    shown_head: &str,
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Entity<PullRequestDetailView> {
    open_fork_on_backend(shown_head, workspace, cx).0
}

/// As [`open_fork_shown_at`], with the backend, so a test can push to the
/// pull request after the tab was opened.
fn open_fork_on_backend(
    shown_head: &str,
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> (Entity<PullRequestDetailView>, Arc<FakeGithubBackend>) {
    let mut details = fork_details();
    details.pull_request.head_ref_oid = shown_head.to_string();
    let backend =
        Arc::new(FakeGithubBackend::new().with_pull_request_details(&repository(), details));
    let key = DetailKey {
        repository: repository(),
        number: 7,
    };
    let view = workspace.update_in(cx, |workspace, window, cx| {
        open_pull_request_detail(workspace, backend.clone(), test_session(), key, window, cx)
    });
    cx.run_until_parked();
    (view, backend)
}

fn fetched_pull_ref(fs: &FakeFs) -> Option<String> {
    fs.with_git_state(dot_git(), false, |state| {
        state.refs.get("refs/remotes/upstream/pull/7/head").cloned()
    })
    .expect("the fake repository exists")
}

#[gpui::test]
async fn a_fork_warns_before_anything_and_cancelling_fetches_nothing(cx: &mut TestAppContext) {
    init(cx);
    let (fs, workspace, cx) = project_window(cx).await;
    let view = open_fork(&workspace, cx);
    let blocker = view.read_with(cx, |view, cx| view.checkout_blocker(cx));
    assert_eq!(blocker, None, "a fork can be checked out after its warning");

    view.update_in(cx, |view, window, cx| view.start_checkout(window, cx));
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "the fork warning is shown first");
    let (title, detail) = cx.pending_prompt().unwrap_or_default();
    assert!(title.contains("fork"), "{title}");
    assert!(detail.contains("@stranger"), "{detail}");
    assert!(detail.contains("someone/repo"), "{detail}");
    assert!(detail.contains("trusted worktree"), "{detail}");
    assert!(
        detail.contains(path!("/project")),
        "the warning names the worktree that switches: {detail}"
    );
    assert_eq!(
        view.read_with(cx, |view, cx| view.checkout_worktree_path(cx)),
        Some(path!("/project").to_string())
    );
    assert_eq!(
        view.read_with(cx, |view, _| view.checkout_state().clone()),
        CheckoutState::ForkWarning
    );

    view.update_in(cx, |view, window, cx| {
        view.confirm_checkout(false, window, cx)
    });
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.checkout_state().clone()),
        CheckoutState::Idle
    );
    assert_eq!(fetched_pull_ref(&fs), None, "nothing was fetched");
}

#[gpui::test]
async fn an_accepted_warning_checks_the_fork_out_from_the_base_repository(cx: &mut TestAppContext) {
    init(cx);
    let (fs, workspace, cx) = project_window(cx).await;
    let view = open_fork(&workspace, cx);
    view.update_in(cx, |view, window, cx| view.start_checkout(window, cx));
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "the fork warning is shown first");
    cx.simulate_prompt_answer("Check out anyway");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.checkout_state().clone()),
        CheckoutState::Naming { error: None }
    );

    view.update_in(cx, |view, window, cx| {
        view.branch_editor(window, cx);
        view.confirm_checkout(false, window, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.checkout_state().clone()),
        CheckoutState::Finished("Checked out pr-7-main.".to_string())
    );
    assert_eq!(fetched_pull_ref(&fs).as_deref(), Some(FORK_HEAD));
    let (branch_head, current) = fs
        .with_git_state(dot_git(), false, |state| {
            (
                state.refs.get("refs/heads/pr-7-main").cloned(),
                state.current_branch_name.clone(),
            )
        })
        .expect("the fake repository exists");
    assert_eq!(branch_head.as_deref(), Some(FORK_HEAD));
    assert_eq!(current.as_deref(), Some("pr-7-main"));
}

/// The warning was accepted for the head that was shown; a head pushed
/// after that is not checked out, and nothing is created.
#[gpui::test]
async fn a_fork_pushed_after_its_warning_was_accepted_is_not_checked_out(cx: &mut TestAppContext) {
    init(cx);
    let (fs, workspace, cx) = project_window(cx).await;
    let view = open_fork_shown_at("0ld0000", &workspace, cx);
    view.update_in(cx, |view, window, cx| view.start_checkout(window, cx));
    cx.run_until_parked();
    cx.simulate_prompt_answer("Check out anyway");
    cx.run_until_parked();
    view.update_in(cx, |view, window, cx| {
        view.branch_editor(window, cx);
        view.confirm_checkout(false, window, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.checkout_state().clone()),
        CheckoutState::PullRequestChanged(FORK_HEAD.to_string())
    );
    let (branches, current) = fs
        .with_git_state(dot_git(), false, |state| {
            (state.branches.clone(), state.current_branch_name.clone())
        })
        .expect("the fake repository exists");
    assert!(!branches.contains("pr-7-main"), "{branches:?}");
    assert_eq!(current.as_deref(), Some("main"));
}

/// A host that makes the branch from HEAD whatever base it is sent: `switch
/// -c` has already moved onto it, so the user is put back before it is
/// deleted, and ends where they began.
#[gpui::test]
async fn a_branch_made_at_the_wrong_commit_leaves_the_user_where_they_were(
    cx: &mut TestAppContext,
) {
    init(cx);
    let (fs, workspace, cx) = project_window(cx).await;
    fs.with_git_state(dot_git(), false, |state| {
        state.simulated_create_branch_drops_base = true;
        state.branches.insert("main".to_string());
    })
    .expect("the fake repository exists");
    let view = open_fork(&workspace, cx);
    view.update_in(cx, |view, window, cx| view.start_checkout(window, cx));
    cx.run_until_parked();
    cx.simulate_prompt_answer("Check out anyway");
    cx.run_until_parked();
    view.update_in(cx, |view, window, cx| {
        view.branch_editor(window, cx);
        view.confirm_checkout(false, window, cx);
    });
    cx.run_until_parked();
    let CheckoutState::Failed(reason) = view.read_with(cx, |view, _| view.checkout_state().clone())
    else {
        panic!("a branch at the wrong commit is a failure");
    };
    assert!(reason.contains("was deleted"), "{reason}");
    assert!(reason.contains("back on main"), "{reason}");
    let (branches, current) = fs
        .with_git_state(dot_git(), false, |state| {
            (state.branches.clone(), state.current_branch_name.clone())
        })
        .expect("the fake repository exists");
    assert!(!branches.contains("pr-7-main"), "{branches:?}");
    assert_eq!(current.as_deref(), Some("main"));
}

/// The warning was accepted for the head shown then. A push that a reload
/// shows afterwards is code nobody accepted, even though it is the head the
/// fetch brings in.
#[gpui::test]
async fn a_fork_head_reloaded_after_the_warning_was_accepted_is_not_checked_out(
    cx: &mut TestAppContext,
) {
    init(cx);
    let (fs, workspace, cx) = project_window(cx).await;
    let (view, backend) = open_fork_on_backend("0ld0000", &workspace, cx);
    view.update_in(cx, |view, window, cx| view.start_checkout(window, cx));
    cx.run_until_parked();
    cx.simulate_prompt_answer("Check out anyway");
    cx.run_until_parked();
    backend.edit_pull_request_details(&repository(), 7, |details| {
        details.pull_request.head_ref_oid = FORK_HEAD.to_string();
    });
    view.update(cx, |view, cx| view.reload(cx));
    cx.run_until_parked();
    view.update_in(cx, |view, window, cx| {
        view.branch_editor(window, cx);
        view.confirm_checkout(false, window, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.checkout_state().clone()),
        CheckoutState::PullRequestChanged(FORK_HEAD.to_string()),
        "the head accepted was the old one"
    );
    let branches = fs
        .with_git_state(dot_git(), false, |state| state.branches.clone())
        .expect("the fake repository exists");
    assert!(!branches.contains("pr-7-main"), "{branches:?}");
}

/// Closing the tab does not stop a checkout halfway: it would leave a
/// fetched ref, or a new branch, that nothing reports.
#[gpui::test]
async fn closing_the_tab_mid_checkout_lets_the_checkout_finish(cx: &mut TestAppContext) {
    init(cx);
    let (fs, workspace, cx) = project_window(cx).await;
    let view = open_fork(&workspace, cx);
    view.update_in(cx, |view, window, cx| view.start_checkout(window, cx));
    cx.run_until_parked();
    cx.simulate_prompt_answer("Check out anyway");
    cx.run_until_parked();
    view.update_in(cx, |view, window, cx| {
        view.branch_editor(window, cx);
        view.confirm_checkout(false, window, cx);
    });
    assert_eq!(
        view.read_with(cx, |view, _| view.checkout_state().clone()),
        CheckoutState::Running
    );
    // The test's own handle would keep the tab alive.
    drop(view);
    let closing = workspace.update_in(cx, |workspace, window, cx| {
        workspace.active_pane().update(cx, |pane, cx| {
            pane.close_active_item(&workspace::CloseActiveItem::default(), window, cx)
        })
    });
    closing.await.expect("the tab closes");
    cx.run_until_parked();
    let (branch_head, current) = fs
        .with_git_state(dot_git(), false, |state| {
            (
                state.refs.get("refs/heads/pr-7-main").cloned(),
                state.current_branch_name.clone(),
            )
        })
        .expect("the fake repository exists");
    assert_eq!(branch_head.as_deref(), Some(FORK_HEAD));
    assert_eq!(current.as_deref(), Some("pr-7-main"));
}

#[gpui::test]
async fn git_asking_for_a_password_during_the_fetch_opens_the_prompt(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let window = cx.update(|window, _| window.window_handle());
    let mut askpass =
        cx.update(|_, cx| pull_request_fetch_askpass(workspace.downgrade(), window, cx));
    let _answer = askpass.ask_password("Password for 'https://github.com':".to_string());
    cx.run_until_parked();
    let prompt_open = workspace.read_with(cx, |workspace, cx| {
        workspace.active_modal::<AskPassModal>(cx).is_some()
    });
    assert!(
        prompt_open,
        "git's password prompt is shown in the workspace"
    );
}
