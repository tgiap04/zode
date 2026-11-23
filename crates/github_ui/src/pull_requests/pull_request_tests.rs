use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use futures::FutureExt as _;
use futures::future::LocalBoxFuture;
use git::repository::FetchOptions;
use github::{
    ChecksState, FakeGithubBackend, FakeWrite, GithubBackend, GithubError, MergeMethod,
    MergeStateStatus, Mergeable, PullRequestDetails, RepositoryId, RepositoryVisibility,
    fake_pull_request, fake_pull_request_details,
};
use gpui::{App, AppContext as _, AsyncApp, Entity, TestAppContext, VisualTestContext};
use ui::Color;
use workspace::Workspace;

use crate::github_markdown::GithubMarkdown;
use crate::github_view::{AuthState, DetailKey, GithubSection, ProjectAccess};
use crate::github_view_session::{end_test_session, session_ended_message, test_session};
use crate::issues::issues_test_support::{
    context, init, repository, signed_in_github_tab, workspace_window,
};
use crate::pull_requests::fork_checkout::ForkWarning;
use crate::pull_requests::pr_checkout::CheckoutSource;
use crate::pull_requests::{
    CheckoutBlocker, CheckoutGit, CheckoutOutcome, CheckoutRequest, MergeAvailability, MergeDialog,
    MergePhase, PullRequestDetailView, PullRequestList, PullRequestMine, checkout_blocker,
    checks_label, default_branch_name, matching_remote, merge_availability,
    open_pull_request_detail, run_checkout, validate_branch_name,
};

fn key(number: u64) -> DetailKey {
    DetailKey {
        repository: repository(),
        number,
    }
}

fn details(number: u64) -> PullRequestDetails {
    let mut pull_request = fake_pull_request(number, "Fix the thing");
    pull_request.head_ref_name = "fix".to_string();
    fake_pull_request_details(pull_request)
}

#[test]
fn the_checks_rollup_reads_as_a_tick_a_cross_or_running() {
    assert_eq!(checks_label(ChecksState::Passing), ("✓", Color::Success));
    assert_eq!(checks_label(ChecksState::Failing), ("✗", Color::Error));
    assert_eq!(
        checks_label(ChecksState::Running),
        ("running", Color::Warning)
    );
}

#[test]
fn an_unknown_mergeability_is_still_being_checked() {
    let mut checking = details(7);
    checking.mergeable = Mergeable::Unknown;
    assert_eq!(merge_availability(&checking), MergeAvailability::Checking);
    let mut status_unknown = details(7);
    status_unknown.merge_state_status = MergeStateStatus::Unknown;
    assert_eq!(
        merge_availability(&status_unknown),
        MergeAvailability::Checking
    );
}

#[test]
fn blocked_requirements_are_never_merged_past() {
    let mut blocked = details(7);
    blocked.merge_state_status = MergeStateStatus::Blocked;
    assert!(matches!(
        merge_availability(&blocked),
        MergeAvailability::Unavailable(_)
    ));
}

#[test]
fn only_the_methods_the_repository_allows_are_offered() {
    let mut squash_only = details(7);
    squash_only.settings.merge_commit_allowed = false;
    squash_only.settings.rebase_merge_allowed = false;
    let MergeAvailability::Available(options) = merge_availability(&squash_only) else {
        panic!("mergeable");
    };
    assert_eq!(options.methods, vec![MergeMethod::Squash]);
}

fn offers_deletion(details: &PullRequestDetails) -> bool {
    match merge_availability(details) {
        MergeAvailability::Available(options) => options.offers_branch_deletion,
        other => panic!("expected a mergeable pull request, got {other:?}"),
    }
}

#[test]
fn branch_deletion_is_hidden_for_default_protected_and_fork_heads() {
    assert!(offers_deletion(&details(7)), "a plain head branch");

    let mut default_head = details(7);
    default_head.pull_request.head_ref_name = "main".to_string();
    if let Some(head) = &mut default_head.head_branch {
        head.name = "main".to_string();
    }
    assert!(!offers_deletion(&default_head), "the default branch");

    let mut protected = details(7);
    if let Some(head) = &mut protected.head_branch {
        head.is_protected = true;
    }
    assert!(!offers_deletion(&protected), "a protected branch");

    let mut from_fork = details(7);
    from_fork.pull_request.is_cross_repository = true;
    assert!(!offers_deletion(&from_fork), "a fork's branch");

    let mut moved = details(7);
    if let Some(head) = &mut moved.head_branch {
        head.oid = Some("other".to_string());
    }
    assert!(!offers_deletion(&moved), "a branch no longer at the head");

    let mut github_deletes = details(7);
    github_deletes.settings.delete_branch_on_merge = true;
    assert!(
        !offers_deletion(&github_deletes),
        "GitHub deletes it itself"
    );
}

#[test]
fn a_merge_queue_leaves_the_merge_to_github() {
    let mut queued = details(7);
    queued.is_merge_queue_enabled = true;
    assert_eq!(merge_availability(&queued), MergeAvailability::MergeQueue);
}

#[test]
fn unsafe_branch_names_are_refused() {
    for name in [
        "-x",
        "a..b",
        "x\u{202E}y",
        "a b",
        "a~1",
        "x.lock",
        ".hidden",
        "a/",
        "HEAD",
        "",
    ] {
        assert!(validate_branch_name(name).is_err(), "{name:?}");
    }
    for name in ["pr-7-fix", "pr-7-feature/login", "fix-ü"] {
        assert_eq!(validate_branch_name(name), Ok(()), "{name:?}");
    }
    assert_eq!(default_branch_name(7, "fix"), "pr-7-fix");
    assert_eq!(default_branch_name(7, "fi\u{202E}x"), "pr-7");
}

#[test]
fn checkout_uses_the_remote_that_points_at_the_repository() {
    let fork = Some("git@github.com:someone/repo.git");
    let ours = Some("https://github.com/Octo/Repo.git");
    assert_eq!(matching_remote(fork, ours, &repository()), Some("upstream"));
    assert_eq!(matching_remote(ours, fork, &repository()), Some("origin"));
    assert_eq!(matching_remote(fork, None, &repository()), None);
}

#[test]
fn only_a_collab_guest_or_a_missing_remote_blocks_checkout() {
    let same_repository = fake_pull_request(7, "Fix");
    assert_eq!(
        checkout_blocker(&same_repository, ProjectAccess::CollabGuest, true),
        Some(CheckoutBlocker::CollabGuest)
    );
    assert_eq!(
        checkout_blocker(&same_repository, ProjectAccess::Owner, true),
        None
    );
    assert_eq!(
        checkout_blocker(&same_repository, ProjectAccess::Owner, false),
        Some(CheckoutBlocker::NoMatchingRemote)
    );
    let mut from_fork = same_repository.clone();
    from_fork.is_cross_repository = true;
    assert_eq!(
        checkout_blocker(&from_fork, ProjectAccess::Owner, true),
        None,
        "a fork is checked out after its warning"
    );
    let mut bidi_head = same_repository;
    bidi_head.head_ref_name = "fi\u{202E}x".to_string();
    assert_eq!(
        checkout_blocker(&bidi_head, ProjectAccess::Owner, true),
        Some(CheckoutBlocker::UnsafeHeadName)
    );
    bidi_head.is_cross_repository = true;
    assert_eq!(
        checkout_blocker(&bidi_head, ProjectAccess::Owner, true),
        None,
        "a fork's head name never reaches git"
    );
}

#[test]
fn the_fork_warning_names_the_author_the_source_and_what_will_run() {
    assert_eq!(ForkWarning::for_pull_request(&details(7)), None);
    let mut from_fork = fork_details(7);
    from_fork.pull_request.author = Some("out\u{202E}sider".to_string());
    let warning = ForkWarning::for_pull_request(&from_fork).expect("a fork is warned about");
    let message = warning.message();
    assert!(message.contains("@out"), "{message}");
    assert!(
        !message.contains('\u{202E}'),
        "hidden characters are shown: {message}"
    );
    assert!(message.contains("someone/repo"), "{message}");
    assert!(message.contains("trusted worktree"), "{message}");
    for runs in [
        "project settings",
        "tasks",
        "build scripts",
        "language servers",
    ] {
        assert!(message.contains(runs), "{runs}: {message}");
    }
    from_fork.head_repository = None;
    let deleted = ForkWarning::for_pull_request(&from_fork).expect("still a fork");
    assert!(deleted.message().contains("a deleted repository"));
}

/// The head every pull request here was shown at.
fn shown_head() -> String {
    details(7).pull_request.head_ref_oid
}

struct FakeGit {
    dirty: bool,
    branches: Vec<String>,
    /// What the remote holds; a fetch brings all of it in.
    remote: HashMap<String, String>,
    /// A host older than the base field: it drops the base and makes the
    /// branch from HEAD.
    ignores_base: bool,
    refs: RefCell<HashMap<String, String>>,
    commands: RefCell<Vec<String>>,
}

impl Default for FakeGit {
    fn default() -> Self {
        Self::with_remote_head(&shown_head())
    }
}

impl FakeGit {
    /// The pull request's head, as both a same-repository branch and a
    /// fork's pull ref, is at `oid` on the remote.
    fn with_remote_head(oid: &str) -> Self {
        Self {
            dirty: false,
            branches: Vec::new(),
            remote: HashMap::from_iter([
                ("refs/remotes/upstream/fix".to_string(), oid.to_string()),
                (
                    "refs/remotes/upstream/pull/7/head".to_string(),
                    oid.to_string(),
                ),
            ]),
            ignores_base: false,
            refs: RefCell::new(HashMap::from_iter([(
                "HEAD".to_string(),
                "h3ad".to_string(),
            )])),
            commands: RefCell::new(Vec::new()),
        }
    }

    fn run(&self, command: String) -> LocalBoxFuture<'static, Result<()>> {
        self.commands.borrow_mut().push(command);
        futures::future::ready(Ok(())).boxed_local()
    }

    fn has_ref(&self, name: &str) -> bool {
        self.refs.borrow().contains_key(name)
    }
}

impl CheckoutGit for FakeGit {
    fn has_uncommitted_changes(&self, _cx: &App) -> bool {
        self.dirty
    }

    fn local_branches(&self, _cx: &mut App) -> LocalBoxFuture<'static, Result<Vec<String>>> {
        futures::future::ready(Ok(self.branches.clone())).boxed_local()
    }

    fn fetch(&self, options: FetchOptions, _cx: &mut App) -> LocalBoxFuture<'static, Result<()>> {
        self.refs.borrow_mut().extend(self.remote.clone());
        self.run(format!("fetch {options}"))
    }

    fn create_branch(
        &self,
        name: String,
        base: String,
        _cx: &mut App,
    ) -> LocalBoxFuture<'static, Result<()>> {
        let start = if self.ignores_base { "HEAD" } else { &base };
        let commit = self.refs.borrow().get(start).cloned();
        if let Some(commit) = commit {
            self.refs
                .borrow_mut()
                .insert(format!("refs/heads/{name}"), commit);
        }
        self.run(format!("create {name} {base}"))
    }

    fn change_branch(&self, name: String, _cx: &mut App) -> LocalBoxFuture<'static, Result<()>> {
        self.run(format!("switch {name}"))
    }

    fn resolve_commit(
        &self,
        reference: String,
        _cx: &mut App,
    ) -> LocalBoxFuture<'static, Result<String>> {
        let commit = self
            .refs
            .borrow()
            .get(&reference)
            .cloned()
            .with_context(|| format!("unknown revision {reference}"));
        futures::future::ready(commit).boxed_local()
    }

    fn force_delete_new_branch<'a>(
        &'a self,
        name: String,
        _cx: &'a mut AsyncApp,
    ) -> LocalBoxFuture<'a, Result<()>> {
        self.refs.borrow_mut().remove(&format!("refs/heads/{name}"));
        self.run(format!("force-delete {name}"))
    }
}

fn request(branch_name: &str, reuse_existing: bool) -> CheckoutRequest {
    CheckoutRequest {
        remote: "upstream".to_string(),
        source: CheckoutSource::SameRepository {
            head_ref_name: "fix".to_string(),
        },
        branch_name: branch_name.to_string(),
        reuse_existing,
        expected_head: shown_head(),
    }
}

fn fork_request(branch_name: &str) -> CheckoutRequest {
    CheckoutRequest {
        source: CheckoutSource::Fork { number: 7 },
        ..request(branch_name, false)
    }
}

fn fork_details(number: u64) -> PullRequestDetails {
    let mut pull_request = fake_pull_request(number, "Fix the thing");
    pull_request.head_ref_name = "main".to_string();
    pull_request.is_cross_repository = true;
    pull_request.author = Some("stranger".to_string());
    let mut details = fake_pull_request_details(pull_request);
    details.head_repository = Some("someone/repo".to_string());
    details
}

async fn checkout(
    git: &Rc<FakeGit>,
    request: CheckoutRequest,
    cx: &mut TestAppContext,
) -> CheckoutOutcome {
    let git: Rc<dyn CheckoutGit> = git.clone();
    run_checkout(git, request, &mut cx.to_async()).await
}

#[gpui::test]
async fn checkout_fetches_the_matching_remote_then_branches_from_it(cx: &mut TestAppContext) {
    let git = Rc::new(FakeGit::default());
    let outcome = checkout(&git, request("pr-7-fix", false), cx).await;
    assert_eq!(
        outcome,
        CheckoutOutcome::CheckedOut {
            branch: "pr-7-fix".to_string()
        }
    );
    assert_eq!(
        *git.commands.borrow(),
        vec![
            "fetch upstream",
            "create pr-7-fix refs/remotes/upstream/fix",
            "switch pr-7-fix"
        ]
    );
}

#[gpui::test]
async fn a_fork_is_fetched_from_the_base_repository_pull_ref(cx: &mut TestAppContext) {
    let git = Rc::new(FakeGit::default());
    let outcome = checkout(&git, fork_request("pr-7"), cx).await;
    assert_eq!(
        outcome,
        CheckoutOutcome::CheckedOut {
            branch: "pr-7".to_string()
        }
    );
    assert_eq!(
        *git.commands.borrow(),
        vec![
            "fetch upstream refs/pull/7/head:refs/remotes/upstream/pull/7/head",
            "create pr-7 refs/remotes/upstream/pull/7/head",
            "switch pr-7"
        ]
    );
}

/// A host older than `GitCreateBranch.base_branch` drops the base and
/// makes the branch from HEAD, and still answers success.
#[gpui::test]
async fn a_branch_not_at_the_fetched_head_is_deleted_and_not_switched_to(cx: &mut TestAppContext) {
    for request in [request("pr-7-fix", false), fork_request("pr-7-fix")] {
        let git = Rc::new(FakeGit {
            ignores_base: true,
            ..FakeGit::default()
        });
        let base = request.source.fetch_and_base("upstream").1;
        let outcome = checkout(&git, request, cx).await;
        let CheckoutOutcome::Failed(reason) = &outcome else {
            panic!("expected a failure, got {outcome:?}");
        };
        assert!(reason.contains("was deleted"), "{reason}");
        let commands = git.commands.borrow().clone();
        assert_eq!(
            commands.last().map(String::as_str),
            Some("force-delete pr-7-fix")
        );
        assert!(
            commands
                .iter()
                .all(|command| !command.starts_with("switch")),
            "{commands:?}"
        );
        assert!(commands.contains(&format!("create pr-7-fix {base}")));
        assert!(!git.has_ref("refs/heads/pr-7-fix"));
    }
}

#[gpui::test]
async fn a_head_that_moved_after_it_was_shown_is_not_checked_out(cx: &mut TestAppContext) {
    for request in [request("pr-7-fix", false), fork_request("pr-7")] {
        let git = Rc::new(FakeGit::with_remote_head("pushed"));
        let outcome = checkout(&git, request, cx).await;
        assert_eq!(
            outcome,
            CheckoutOutcome::PullRequestChanged {
                fetched: "pushed".to_string()
            }
        );
        let commands = git.commands.borrow().clone();
        assert_eq!(commands.len(), 1, "only the fetch ran: {commands:?}");
        assert!(commands[0].starts_with("fetch"), "{commands:?}");
    }
}

#[gpui::test]
async fn a_fork_checkout_still_refuses_bad_names_and_a_dirty_tree(cx: &mut TestAppContext) {
    let git = Rc::new(FakeGit::default());
    for name in ["-x", "a..b", "pr\u{202E}7"] {
        let outcome = checkout(&git, fork_request(name), cx).await;
        assert!(
            matches!(outcome, CheckoutOutcome::Refused(_)),
            "{name:?}: {outcome:?}"
        );
    }
    let dirty = Rc::new(FakeGit {
        dirty: true,
        ..FakeGit::default()
    });
    let outcome = checkout(&dirty, fork_request("pr-7"), cx).await;
    assert!(
        matches!(outcome, CheckoutOutcome::Refused(_)),
        "{outcome:?}"
    );
    assert!(git.commands.borrow().is_empty());
    assert!(dirty.commands.borrow().is_empty());
}

#[gpui::test]
async fn a_dirty_tree_is_reported_and_left_alone(cx: &mut TestAppContext) {
    let git = Rc::new(FakeGit {
        dirty: true,
        ..FakeGit::default()
    });
    let outcome = checkout(&git, request("pr-7-fix", false), cx).await;
    let CheckoutOutcome::Refused(reason) = outcome else {
        panic!("expected a refusal, got {outcome:?}");
    };
    assert!(reason.contains("uncommitted changes"), "{reason}");
    assert!(git.commands.borrow().is_empty(), "git was not touched");
}

#[gpui::test]
async fn an_edited_name_is_checked_before_git_runs(cx: &mut TestAppContext) {
    let git = Rc::new(FakeGit::default());
    for name in ["-x", "a..b", "pr\u{202E}7"] {
        let outcome = checkout(&git, request(name, false), cx).await;
        assert!(
            matches!(outcome, CheckoutOutcome::Refused(_)),
            "{name:?}: {outcome:?}"
        );
    }
    assert!(git.commands.borrow().is_empty());
}

#[gpui::test]
async fn an_existing_branch_is_asked_about_before_it_is_reused(cx: &mut TestAppContext) {
    let git = Rc::new(FakeGit {
        branches: vec!["pr-7-fix".to_string()],
        ..FakeGit::default()
    });
    let asked = checkout(&git, request("pr-7-fix", false), cx).await;
    assert_eq!(
        asked,
        CheckoutOutcome::BranchExists {
            branch: "pr-7-fix".to_string()
        }
    );
    assert!(git.commands.borrow().is_empty());
    let reused = checkout(&git, request("pr-7-fix", true), cx).await;
    assert_eq!(
        reused,
        CheckoutOutcome::SwitchedToExisting {
            branch: "pr-7-fix".to_string()
        }
    );
    assert_eq!(*git.commands.borrow(), vec!["switch pr-7-fix"]);
}

fn open(
    workspace: &Entity<Workspace>,
    backend: Arc<dyn GithubBackend>,
    number: u64,
    cx: &mut VisualTestContext,
) -> Entity<PullRequestDetailView> {
    let view = workspace.update_in(cx, |workspace, window, cx| {
        open_pull_request_detail(workspace, backend, test_session(), key(number), window, cx)
    });
    cx.run_until_parked();
    view
}

fn merge_dialog(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Option<Entity<MergeDialog>> {
    workspace.read_with(cx, |workspace, cx| {
        workspace.active_modal::<MergeDialog>(cx)
    })
}

fn ask_to_merge(
    view: &Entity<PullRequestDetailView>,
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Option<Entity<MergeDialog>> {
    view.update_in(cx, |view, window, cx| view.request_merge(window, cx));
    cx.run_until_parked();
    merge_dialog(workspace, cx)
}

#[gpui::test]
async fn a_merge_needs_the_dialog_and_cancelling_sends_nothing(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake =
        Arc::new(FakeGithubBackend::new().with_pull_request_details(&repository(), details(7)));
    let view = open(&workspace, fake.clone(), 7, cx);
    let dialog = ask_to_merge(&view, &workspace, cx).expect("the merge dialog opened");
    assert_eq!(fake.writes(), Vec::new(), "asking opens the dialog only");
    dialog.update(cx, |dialog, cx| dialog.cancel(cx));
    cx.run_until_parked();
    assert!(merge_dialog(&workspace, cx).is_none(), "the dialog closed");
    assert_eq!(fake.writes(), Vec::new(), "cancelling sends nothing");
}

#[gpui::test]
async fn confirming_merges_at_the_head_shown_and_deletes_only_that_branch(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake =
        Arc::new(FakeGithubBackend::new().with_pull_request_details(&repository(), details(7)));
    let view = open(&workspace, fake.clone(), 7, cx);
    let dialog = ask_to_merge(&view, &workspace, cx).expect("the merge dialog opened");
    dialog.update(cx, |dialog, cx| {
        dialog.set_method(MergeMethod::Squash, cx);
        dialog.set_delete_branch(true, cx);
        dialog.confirm(cx);
    });
    cx.run_until_parked();
    let head = details(7).pull_request.head_ref_oid;
    assert_eq!(
        fake.writes(),
        vec![
            FakeWrite::MergePullRequest("PR_7".to_string(), MergeMethod::Squash, head.clone()),
            FakeWrite::DeleteBranchAt("R_octo_repo".to_string(), "fix".to_string(), head),
        ]
    );
    assert!(matches!(
        dialog.read_with(cx, |dialog, _| dialog.phase().clone()),
        MergePhase::Merged { .. }
    ));
}

#[gpui::test]
async fn a_head_that_moved_is_not_merged_and_the_tab_reloads(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake =
        Arc::new(FakeGithubBackend::new().with_pull_request_details(&repository(), details(7)));
    let view = open(&workspace, fake.clone(), 7, cx);
    let dialog = ask_to_merge(&view, &workspace, cx).expect("the merge dialog opened");
    fake.push_to_pull_request(&repository(), 7, "pushed");
    dialog.update(cx, |dialog, cx| dialog.confirm(cx));
    cx.run_until_parked();
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.phase().clone()),
        MergePhase::HeadChanged
    );
    assert_eq!(fake.writes(), Vec::new(), "nothing was merged");
    let shown_head = view.read_with(cx, |view, _| {
        view.details()
            .map(|details| details.pull_request.head_ref_oid.clone())
    });
    assert_eq!(shown_head.as_deref(), Some("pushed"), "the tab reloaded");
}

/// Read again on confirm, anything that makes it a different merge from
/// the one confirmed stops it: blocked, retargeted, back to draft, or the
/// chosen method no longer allowed.
#[gpui::test]
async fn a_pull_request_changed_since_the_dialog_opened_is_not_merged(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let changes: [(u64, fn(&mut PullRequestDetails)); 4] = [
        (7, |details| {
            details.merge_state_status = MergeStateStatus::Blocked;
        }),
        (8, |details| {
            details.pull_request.base_ref_name = "release".to_string();
        }),
        (9, |details| details.pull_request.is_draft = true),
        (10, |details| details.settings.squash_merge_allowed = false),
    ];
    let mut fake = FakeGithubBackend::new();
    for (number, _) in &changes {
        fake = fake.with_pull_request_details(&repository(), details(*number));
    }
    let fake = Arc::new(fake);
    for (number, change) in changes {
        let view = open(&workspace, fake.clone(), number, cx);
        let dialog = ask_to_merge(&view, &workspace, cx).expect("the merge dialog opened");
        dialog.update(cx, |dialog, cx| dialog.set_method(MergeMethod::Squash, cx));
        fake.edit_pull_request_details(&repository(), number, change);
        dialog.update(cx, |dialog, cx| dialog.confirm(cx));
        cx.run_until_parked();
        let phase = dialog.read_with(cx, |dialog, _| dialog.phase().clone());
        assert!(
            matches!(phase, MergePhase::NoLongerMergeable(_)),
            "#{number}: {phase:?}"
        );
        assert_eq!(fake.writes(), Vec::new(), "#{number}: nothing was merged");
        dialog.update(cx, |dialog, cx| dialog.cancel(cx));
        cx.run_until_parked();
    }
}

#[gpui::test]
async fn a_visibility_that_cannot_be_read_says_so_and_can_be_retried(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake =
        Arc::new(FakeGithubBackend::new().with_pull_request_details(&repository(), details(7)));
    let view = open(&workspace, fake.clone(), 7, cx);
    fake.set_failure(Some(GithubError::NotFound));
    let dialog = ask_to_merge(&view, &workspace, cx).expect("the merge dialog opened");
    cx.run_until_parked();
    let error = dialog.read_with(cx, |dialog, _| {
        dialog.visibility_error().map(str::to_string)
    });
    assert!(error.is_some(), "the failure is shown");
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert!(cx.debug_bounds("github-merge-visibility-retry").is_some());

    fake.set_failure(None);
    dialog.update(cx, |dialog, cx| dialog.retry_visibility(cx));
    cx.run_until_parked();
    let (error, visibility) = dialog.read_with(cx, |dialog, _| {
        (
            dialog.visibility_error().map(str::to_string),
            dialog.visibility,
        )
    });
    assert_eq!(error, None);
    assert_eq!(visibility, Some(RepositoryVisibility::Public));
    assert_eq!(fake.writes(), Vec::new());
}

#[gpui::test]
async fn the_tab_names_the_account_and_the_repository_it_writes_to(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(
        FakeGithubBackend::new()
            .with_pull_request_details(&repository(), details(7))
            .with_repository_visibility(&repository(), RepositoryVisibility::Private),
    );
    let view = open(&workspace, fake, 7, cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.target_line()),
        Some("@octocat → octo/repo (private)".to_string())
    );
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert!(cx.debug_bounds("github-pr-target").is_some());
}

#[gpui::test]
async fn a_merge_queue_opens_no_dialog(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let mut queued = details(7);
    queued.is_merge_queue_enabled = true;
    let fake = Arc::new(FakeGithubBackend::new().with_pull_request_details(&repository(), queued));
    let view = open(&workspace, fake.clone(), 7, cx);
    assert!(ask_to_merge(&view, &workspace, cx).is_none());
    assert_eq!(fake.writes(), Vec::new());
}

#[gpui::test]
async fn a_collab_guest_tab_offers_no_checkout(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake =
        Arc::new(FakeGithubBackend::new().with_pull_request_details(&repository(), details(7)));
    let view = open(&workspace, fake, 7, cx);
    view.update_in(cx, |view, window, cx| {
        view.access = ProjectAccess::CollabGuest;
        view.start_checkout(window, cx);
    });
    let (blocker, state) = view.read_with(cx, |view, cx| {
        (view.checkout_blocker(cx), view.checkout_state().clone())
    });
    assert_eq!(blocker, Some(CheckoutBlocker::CollabGuest));
    assert_eq!(state, crate::pull_requests::CheckoutState::Idle);
}

#[gpui::test]
async fn a_pull_request_link_opens_its_tab(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake: Arc<dyn GithubBackend> =
        Arc::new(FakeGithubBackend::new().with_pull_request_details(&repository(), details(3)));
    let body =
        cx.new(|cx| GithubMarkdown::new("See the fix", &repository(), fake, test_session(), cx));
    body.update_in(cx, |body, window, cx| {
        body.open_link("https://github.com/octo/repo/pull/3", window, cx)
    });
    cx.run_until_parked();
    let tabs = workspace.read_with(cx, |workspace, cx| {
        workspace
            .items_of_type::<PullRequestDetailView>(cx)
            .collect::<Vec<_>>()
    });
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].read_with(cx, |tab, _| tab.key().clone()), key(3));
    assert_eq!(cx.opened_url(), None, "opened inside Zode");
}

#[gpui::test]
async fn mine_narrows_to_authored_or_review_requested(cx: &mut TestAppContext) {
    init(cx);
    let mut authored = fake_pull_request(1, "Mine");
    authored.author = Some("octocat".to_string());
    let mut asked = fake_pull_request(2, "Asked");
    asked.author = Some("hubot".to_string());
    asked.requested_reviewers = vec!["octocat".to_string()];
    let mut other = fake_pull_request(3, "Other");
    other.author = Some("hubot".to_string());
    let fake = FakeGithubBackend::new()
        .with_pull_request(&repository(), authored)
        .with_pull_request(&repository(), asked)
        .with_pull_request(&repository(), other);
    let list = cx.new(|cx| {
        let mut list = PullRequestList::new(context(Arc::new(fake)));
        list.reload(cx);
        list
    });
    cx.run_until_parked();
    let numbers = |list: &Entity<PullRequestList>, cx: &mut TestAppContext| {
        list.read_with(cx, |list, _| {
            list.visible_rows()
                .iter()
                .map(|pull_request| pull_request.number)
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(numbers(&list, cx), vec![1, 2, 3]);
    list.update(cx, |list, cx| {
        list.set_mine(Some(PullRequestMine::Authored), cx)
    });
    assert_eq!(numbers(&list, cx), vec![1]);
    list.update(cx, |list, cx| {
        list.set_mine(Some(PullRequestMine::ReviewRequested), cx)
    });
    assert_eq!(numbers(&list, cx), vec![2]);
}

#[test]
fn a_repository_id_is_compared_without_case() {
    let upper = RepositoryId::new("OCTO", "REPO").expect("valid");
    assert_eq!(
        matching_remote(Some("https://github.com/octo/repo"), None, &upper),
        Some("origin")
    );
}

/// After Disconnect or a switch of account, a tab opened before must not
/// write as an account the GitHub tab no longer shows.
#[gpui::test]
async fn a_tab_from_an_ended_session_refuses_to_write(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake =
        Arc::new(FakeGithubBackend::new().with_pull_request_details(&repository(), details(7)));
    let view = open(&workspace, fake.clone(), 7, cx);
    cx.update(|_, cx| {
        crate::github_view_session::end_test_session(cx);
    });
    view.update_in(cx, |view, window, cx| {
        let editor = view.comment_editor(window, cx);
        editor.update(cx, |editor, cx| {
            editor.set_text("Internal note", window, cx)
        });
        view.send_comment(window, cx);
    });
    cx.run_until_parked();
    assert_eq!(fake.writes(), Vec::new(), "the comment was not sent");
    let error = view.read_with(cx, |view, _| view.comment_error().map(str::to_string));
    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("reopen")),
        "{error:?}"
    );
    assert!(
        ask_to_merge(&view, &workspace, cx).is_none(),
        "no merge dialog for an ended session"
    );
}

#[gpui::test]
async fn a_token_refused_in_pull_requests_signs_the_github_tab_out(cx: &mut TestAppContext) {
    let fake = Arc::new(
        FakeGithubBackend::new().with_pull_request(&repository(), fake_pull_request(7, "Fix")),
    );
    let (_workspace, view, cx) = signed_in_github_tab(
        fake.clone(),
        github::OAuthScopes::Unknown,
        GithubSection::PullRequests,
        cx,
    )
    .await;
    let list = view
        .read_with(cx, |view, _| view.pull_request_list().cloned())
        .expect("the list is shown");
    fake.set_failure(Some(GithubError::TokenRevoked));
    list.update(cx, |list, cx| list.reload(cx));
    cx.run_until_parked();
    assert!(matches!(
        view.read_with(cx, |view, _| view.auth().clone()),
        AuthState::Revoked { .. }
    ));
}

#[gpui::test]
async fn a_merge_is_not_sent_before_the_visibility_is_read(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake =
        Arc::new(FakeGithubBackend::new().with_pull_request_details(&repository(), details(7)));
    let view = open(&workspace, fake.clone(), 7, cx);
    fake.set_failure(Some(GithubError::NotFound));
    let dialog = ask_to_merge(&view, &workspace, cx).expect("the merge dialog opened");
    fake.set_failure(None);

    dialog.update(cx, |dialog, cx| dialog.confirm(cx));
    cx.run_until_parked();
    assert_eq!(fake.writes(), Vec::new(), "nothing is merged");
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.phase().clone()),
        MergePhase::Confirming
    );
}

/// A sign-out that lands while the merge reads the pull request again
/// stops the merge.
#[gpui::test]
async fn signing_out_while_the_merge_reads_again_sends_no_merge(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake =
        Arc::new(FakeGithubBackend::new().with_pull_request_details(&repository(), details(7)));
    let view = open(&workspace, fake.clone(), 7, cx);
    let dialog = ask_to_merge(&view, &workspace, cx).expect("the merge dialog opened");

    let release = fake.hold_next_pull_request_details();
    dialog.update(cx, |dialog, cx| dialog.confirm(cx));
    cx.run_until_parked();
    cx.update(|_, cx| end_test_session(cx));
    release.send(()).expect("the read is waiting");
    cx.run_until_parked();

    assert_eq!(fake.writes(), Vec::new(), "nothing was merged");
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.phase().clone()),
        MergePhase::Failed(session_ended_message("pull request"))
    );
}

/// A sign-out that lands while the merge is on its way keeps the branch:
/// the deletion is a second write the ended session must not send.
#[gpui::test]
async fn signing_out_while_the_merge_is_sent_deletes_no_branch(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let fake =
        Arc::new(FakeGithubBackend::new().with_pull_request_details(&repository(), details(7)));
    let view = open(&workspace, fake.clone(), 7, cx);
    let dialog = ask_to_merge(&view, &workspace, cx).expect("the merge dialog opened");

    let release = fake.hold_next_merge();
    dialog.update(cx, |dialog, cx| {
        dialog.set_delete_branch(true, cx);
        dialog.confirm(cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| end_test_session(cx));
    release.send(()).expect("the merge is waiting");
    cx.run_until_parked();

    let head = details(7).pull_request.head_ref_oid;
    assert_eq!(
        fake.writes(),
        vec![FakeWrite::MergePullRequest(
            "PR_7".to_string(),
            MergeMethod::Merge,
            head
        )],
        "merged, and no branch deletion sent"
    );
    let note = match dialog.read_with(cx, |dialog, _| dialog.phase().clone()) {
        MergePhase::Merged { branch_note } => branch_note,
        other => panic!("expected merged, got {other:?}"),
    };
    assert!(
        note.as_deref()
            .is_some_and(|note| note.contains("fix was kept")
                && note.contains(&session_ended_message("pull request"))),
        "{note:?}"
    );
}
