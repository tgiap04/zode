use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::anyhow;
use collections::HashSet;
use fs::Fs;
use gpui::{AsyncWindowContext, Entity, SharedString, WeakEntity};
use project::Project;
use project::git_store::Repository;
use project::project_settings::ProjectSettings;
use project::trusted_worktrees::{PathTrust, TrustedWorktrees};
use remote::RemoteConnectionOptions;
use settings::Settings;
use workspace::{MultiWorkspace, OpenMode, PreviousWorkspaceState, Workspace, dock::DockPosition};
use zed_actions::NewWorktreeBranchTarget;

use util::ResultExt as _;

use crate::git_panel::show_error_toast;
use crate::worktree_names;

/// Whether a worktree operation is creating a new one or switching to an
/// existing one. Controls whether the source workspace's state (dock layout,
/// open files, agent panel draft) is inherited by the destination.
enum WorktreeOperation {
    Create,
    Switch,
}

/// Classifies the project's visible worktrees into git-managed repositories
/// and non-git paths. Each unique repository is returned only once.
pub fn classify_worktrees(
    project: &Project,
    cx: &gpui::App,
) -> (Vec<Entity<Repository>>, Vec<PathBuf>) {
    let repositories = project.repositories(cx).clone();
    let mut git_repos: Vec<Entity<Repository>> = Vec::new();
    let mut non_git_paths: Vec<PathBuf> = Vec::new();
    let mut seen_repo_ids = HashSet::default();

    for worktree in project.visible_worktrees(cx) {
        let wt_path = worktree.read(cx).abs_path();

        let matching_repo = repositories
            .iter()
            .filter_map(|(id, repo)| {
                let work_dir = repo.read(cx).work_directory_abs_path.clone();
                if wt_path.starts_with(work_dir.as_ref()) {
                    Some((*id, repo.clone(), work_dir.as_ref().components().count()))
                } else {
                    None
                }
            })
            .max_by(
                |(left_id, _left_repo, left_depth), (right_id, _right_repo, right_depth)| {
                    left_depth
                        .cmp(right_depth)
                        .then_with(|| left_id.cmp(right_id))
                },
            );

        if let Some((id, repo, _)) = matching_repo {
            if seen_repo_ids.insert(id) {
                git_repos.push(repo);
            }
        } else {
            non_git_paths.push(wt_path.to_path_buf());
        }
    }

    (git_repos, non_git_paths)
}

/// Resolves a branch target into the ref the new worktree should be based on.
/// Returns `None` for `CurrentBranch`, meaning "use the current HEAD".
/// The ref a new worktree is based on, for the variants that check out an
/// existing one. `None` means HEAD.
pub fn resolve_worktree_branch_target(branch_target: &NewWorktreeBranchTarget) -> Option<String> {
    match branch_target {
        NewWorktreeBranchTarget::CurrentBranch => None,
        NewWorktreeBranchTarget::NewBranch { base, .. } => base.clone(),
        NewWorktreeBranchTarget::ExistingBranch { name } => Some(name.clone()),
    }
}

/// The branch a new worktree should create for itself, if any.
pub fn new_branch_for_worktree(branch_target: &NewWorktreeBranchTarget) -> Option<String> {
    match branch_target {
        NewWorktreeBranchTarget::NewBranch { name, .. } => Some(name.clone()),
        NewWorktreeBranchTarget::CurrentBranch | NewWorktreeBranchTarget::ExistingBranch { .. } => {
            None
        }
    }
}

/// Kicks off an async git-worktree creation for each repository. Returns:
///
/// - `creation_infos`: a vec of `(repo, new_path, receiver)` tuples.
/// - `path_remapping`: `(old_work_dir, new_worktree_path)` pairs for remapping editor tabs.
fn start_worktree_creations(
    git_repos: &[Entity<Repository>],
    worktree_name: Option<String>,
    existing_worktree_names: &[String],
    existing_worktree_paths: &HashSet<PathBuf>,
    location: Option<&Path>,
    base_ref: Option<String>,
    // `new_branch`: the branch the new worktree creates for itself, `None` for
    // a detached checkout.
    new_branch: Option<String>,
    worktree_directory_setting: &str,
    rng: &mut impl rand::Rng,
    cx: &mut gpui::App,
) -> anyhow::Result<(
    Vec<(
        Entity<Repository>,
        PathBuf,
        futures::channel::oneshot::Receiver<anyhow::Result<()>>,
    )>,
    Vec<(PathBuf, PathBuf)>,
)> {
    let mut creation_infos = Vec::new();
    let mut path_remapping = Vec::new();

    let worktree_name = match worktree_name {
        // Checked before anything is created, because the name is joined into
        // the path the worktree is made at: one that escapes that directory
        // would be created somewhere nobody asked for.
        Some(name) => {
            worktree_names::validate_worktree_name(&name)
                .map_err(|reason| anyhow::anyhow!("{reason}: {name:?}"))?;
            name
        }
        None => {
            let existing_refs: Vec<&str> =
                existing_worktree_names.iter().map(|s| s.as_str()).collect();
            worktree_names::generate_worktree_name(&existing_refs, rng)
                .unwrap_or_else(|| "worktree".to_string())
        }
    };

    // The branch is a second name with its own rules, and it does not have to
    // match the worktree's -- `CreateWorktree` carries them separately.
    if let Some(branch_name) = new_branch.as_deref() {
        worktree_names::validate_worktree_name(branch_name)
            .map_err(|reason| anyhow::anyhow!("{reason}: {branch_name:?}"))?;
    }

    for repo in git_repos {
        let (work_dir, new_path, receiver) = repo.update(cx, |repo, _cx| {
            let new_path = match location {
                Some(directory) => {
                    repo.path_for_new_linked_worktree_in(directory, &worktree_name)?
                }
                None => {
                    repo.path_for_new_linked_worktree(&worktree_name, worktree_directory_setting)?
                }
            };
            if existing_worktree_paths.contains(&new_path) {
                anyhow::bail!("A worktree already exists at {}", new_path.display());
            }
            // A worktree with a branch of its own is the point of the panel:
            // a detached checkout has nowhere to commit, so parallel feature
            // work cannot happen in one.
            let target = match new_branch.clone() {
                Some(branch_name) => git::repository::CreateWorktreeTarget::NewBranch {
                    branch_name,
                    base_sha: base_ref.clone(),
                },
                None => git::repository::CreateWorktreeTarget::Detached {
                    base_sha: base_ref.clone(),
                },
            };
            let receiver = repo.create_worktree(target, new_path.clone());
            let work_dir = repo.work_directory_abs_path.clone();
            anyhow::Ok((work_dir, new_path, receiver))
        })?;
        path_remapping.push((work_dir.to_path_buf(), new_path.clone()));
        creation_infos.push((repo.clone(), new_path, receiver));
    }

    Ok((creation_infos, path_remapping))
}

/// Waits for every in-flight worktree creation to complete. If any
/// creation fails, all successfully-created worktrees are rolled back
/// (removed) so the project isn't left in a half-migrated state.
pub async fn await_and_rollback_on_failure(
    creation_infos: Vec<(
        Entity<Repository>,
        PathBuf,
        futures::channel::oneshot::Receiver<anyhow::Result<()>>,
    )>,
    fs: Arc<dyn Fs>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<Vec<PathBuf>> {
    // Only the worktrees that were actually created. A path whose
    // `git worktree add` failed was never this code's to remove, and rolling
    // one back is not harmless: `remove_worktree(force)` deletes the directory
    // outright before it asks git anything (see `Repository::remove_worktree`).
    //
    // That is how a name colliding with a directory already on disk became data
    // loss. `git worktree add` refuses a non-empty directory, so creation
    // failed; rollback then recursively deleted that directory and everything
    // in it -- work the user had there, which git had just declined to touch.
    //
    // The cost of the narrower list is an orphaned admin entry when git fails
    // *after* registering the worktree. `git worktree prune` clears that, and a
    // stale entry is a far smaller harm than deleting a directory this process
    // did not create.
    let mut outcomes = Vec::new();
    for (repo, new_path, receiver) in creation_infos {
        let outcome = match receiver.await {
            Ok(result) => result,
            Err(_canceled) => Err(anyhow!("Worktree creation was canceled")),
        };
        outcomes.push((repo, new_path, outcome));
    }
    let (created, first_error) = partition_creations(outcomes);

    let Some(err) = first_error else {
        return Ok(created.into_iter().map(|(_repo, path)| path).collect());
    };

    // All-or-nothing: a creation that failed for one repository undoes the ones
    // that succeeded, so the project is not left half-migrated.
    let mut rollback_futures = Vec::new();
    for (rollback_repo, rollback_path) in &created {
        let receiver = cx
            .update(|_, cx| {
                rollback_repo.update(cx, |repo, _cx| {
                    repo.remove_worktree(rollback_path.clone(), true)
                })
            })
            .ok();

        rollback_futures.push((rollback_path.clone(), receiver));
    }

    let mut rollback_failures: Vec<String> = Vec::new();
    for (path, receiver_opt) in rollback_futures {
        let mut git_remove_failed = false;

        if let Some(receiver) = receiver_opt {
            match receiver.await {
                Ok(Ok(())) => {}
                Ok(Err(rollback_err)) => {
                    log::error!(
                        "git worktree remove failed for {}: {rollback_err}",
                        path.display()
                    );
                    git_remove_failed = true;
                }
                Err(canceled) => {
                    log::error!(
                        "git worktree remove failed for {}: {canceled}",
                        path.display()
                    );
                    git_remove_failed = true;
                }
            }
        } else {
            log::error!(
                "failed to dispatch git worktree remove for {}",
                path.display()
            );
            git_remove_failed = true;
        }

        if git_remove_failed {
            if let Err(fs_err) = fs
                .remove_dir(
                    &path,
                    fs::RemoveOptions {
                        recursive: true,
                        ignore_if_not_exists: true,
                    },
                )
                .await
            {
                let msg = format!("{}: failed to remove directory: {fs_err}", path.display());
                log::error!("{}", msg);
                rollback_failures.push(msg);
            }
        }
    }
    let mut error_message = format!("Failed to create worktree: {err}");
    if !rollback_failures.is_empty() {
        error_message.push_str("\n\nFailed to clean up: ");
        error_message.push_str(&rollback_failures.join(", "));
    }
    Err(anyhow!(error_message))
}

/// Splits creation outcomes into the worktrees that exist and the first failure.
///
/// Its own function because the difference between "attempted" and "created" is
/// the whole of a data-loss bug: the rollback below deletes each path it is
/// given, recursively and before git is consulted, so a path whose creation
/// failed must never reach it.
fn partition_creations<T>(
    outcomes: impl IntoIterator<Item = (T, PathBuf, anyhow::Result<()>)>,
) -> (Vec<(T, PathBuf)>, Option<anyhow::Error>) {
    let mut created = Vec::new();
    let mut first_error = None;
    for (owner, path, outcome) in outcomes {
        match outcome {
            Ok(()) => created.push((owner, path)),
            Err(err) => {
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
        }
    }
    (created, first_error)
}

/// Propagates worktree trust from the source workspace to the new workspace.
/// If the source project's worktrees are all trusted, the new worktree paths
/// will also be trusted automatically.
fn maybe_propagate_worktree_trust(
    source_workspace: &WeakEntity<Workspace>,
    new_workspace: &Entity<Workspace>,
    paths: &[PathBuf],
    cx: &mut AsyncWindowContext,
) {
    cx.update(|_, cx| {
        if ProjectSettings::get_global(cx).session.trust_all_worktrees {
            return;
        }
        let Some(trusted_store) = TrustedWorktrees::try_get_global(cx) else {
            return;
        };

        let source_is_trusted = source_workspace
            .upgrade()
            .map(|workspace| {
                let source_worktree_store = workspace.read(cx).project().read(cx).worktree_store();
                !trusted_store
                    .read(cx)
                    .has_restricted_worktrees(&source_worktree_store, cx)
            })
            .unwrap_or(false);

        if !source_is_trusted {
            return;
        }

        let worktree_store = new_workspace.read(cx).project().read(cx).worktree_store();
        let paths_to_trust: HashSet<_> = paths
            .iter()
            .filter_map(|path| {
                let (worktree, _) = worktree_store.read(cx).find_worktree(path, cx)?;
                Some(PathTrust::Worktree(worktree.read(cx).id()))
            })
            .collect();

        if !paths_to_trust.is_empty() {
            trusted_store.update(cx, |store, cx| {
                store.trust(&worktree_store, paths_to_trust, cx);
            });
        }
    })
    .ok();
}

/// Handles the `CreateWorktree` action generically, without any agent panel involvement.
/// Creates a new git worktree, opens the workspace, restores layout and files.
pub fn handle_create_worktree(
    workspace: &mut Workspace,
    action: &zed_actions::CreateWorktree,
    window: &mut gpui::Window,
    fallback_focused_dock: Option<DockPosition>,
    cx: &mut gpui::Context<Workspace>,
) {
    let project = workspace.project().clone();

    if project.read(cx).repositories(cx).is_empty() {
        log::error!("create_worktree: no git repository in the project");
        return;
    }
    if project.read(cx).is_via_collab() {
        log::error!("create_worktree: not supported in collab projects");
        return;
    }

    // One at a time: both flows move the window to another workspace, and two
    // of those racing would leave the second acting on state the first had
    // already replaced.
    //
    // The refusal is logged. The title bar does show what is in flight, so this
    // is not silent to the user -- but a dropped click left nothing behind to
    // read afterwards, and "switching feels unresponsive" was reported long
    // before anyone could tell it was this.
    if let Some(in_flight) = workspace.active_worktree_creation().label.clone() {
        log::info!("ignoring worktree request: `{in_flight}` is still in flight");
        return;
    }

    let agent = action.agent.clone();
    let previous_state =
        workspace.capture_state_for_worktree_switch(window, fallback_focused_dock, cx);
    let workspace_handle = workspace.weak_handle();
    let window_handle = window.window_handle().downcast::<MultiWorkspace>();
    let remote_connection_options = project.read(cx).remote_connection_options(cx);

    let (git_repos, non_git_paths) = classify_worktrees(project.read(cx), cx);

    if git_repos.is_empty() {
        show_error_toast(
            cx.entity(),
            "worktree create",
            anyhow!("No git repositories found in the project"),
            cx,
        );
        return;
    }

    if remote_connection_options.is_some() {
        let is_disconnected = project
            .read(cx)
            .remote_client()
            .is_some_and(|client| client.read(cx).is_disconnected());
        if is_disconnected {
            show_error_toast(
                cx.entity(),
                "worktree create",
                anyhow!("Cannot create worktree: remote connection is not active"),
                cx,
            );
            return;
        }
    }

    let worktree_name = action.worktree_name.clone();
    let branch_target = action.branch_target.clone();
    let location = action.location.clone();
    let display_name: SharedString = worktree_name
        .as_deref()
        .unwrap_or("worktree")
        .to_string()
        .into();

    workspace.set_active_worktree_creation(Some(display_name), false, cx);

    cx.spawn_in(window, async move |_workspace_entity, mut cx| {
        let result = do_create_worktree(
            git_repos,
            non_git_paths,
            worktree_name,
            branch_target,
            previous_state,
            workspace_handle.clone(),
            window_handle,
            remote_connection_options,
            agent,
            location,
            &mut cx,
        )
        .await;

        if let Err(err) = &result {
            log::error!("Failed to create worktree: {err}");
            workspace_handle
                .update(cx, |workspace, cx| {
                    workspace.set_active_worktree_creation(None, false, cx);
                    show_error_toast(cx.entity(), "worktree create", anyhow!("{err:#}"), cx);
                })
                .ok();
        }

        result
    })
    .detach_and_log_err(cx);
}

pub fn handle_switch_worktree(
    workspace: &mut Workspace,
    action: &zed_actions::SwitchWorktree,
    window: &mut gpui::Window,
    fallback_focused_dock: Option<DockPosition>,
    cx: &mut gpui::Context<Workspace>,
) {
    let project = workspace.project().clone();

    if project.read(cx).repositories(cx).is_empty() {
        log::error!("switch_to_worktree: no git repository in the project");
        return;
    }
    if project.read(cx).is_via_collab() {
        log::error!("switch_to_worktree: not supported in collab projects");
        return;
    }

    // One at a time: both flows move the window to another workspace, and two
    // of those racing would leave the second acting on state the first had
    // already replaced.
    //
    // The refusal is logged. The title bar does show what is in flight, so this
    // is not silent to the user -- but a dropped click left nothing behind to
    // read afterwards, and "switching feels unresponsive" was reported long
    // before anyone could tell it was this.
    if let Some(in_flight) = workspace.active_worktree_creation().label.clone() {
        log::info!("ignoring worktree request: `{in_flight}` is still in flight");
        return;
    }

    let previous_state =
        workspace.capture_state_for_worktree_switch(window, fallback_focused_dock, cx);
    let workspace_handle = workspace.weak_handle();
    let window_handle = window.window_handle().downcast::<MultiWorkspace>();
    let remote_connection_options = project.read(cx).remote_connection_options(cx);

    let (git_repos, non_git_paths) = classify_worktrees(project.read(cx), cx);

    let git_repo_work_dirs: Vec<PathBuf> = git_repos
        .iter()
        .map(|repo| repo.read(cx).work_directory_abs_path.to_path_buf())
        .collect();

    let display_name: SharedString = action.display_name.clone().into();

    workspace.set_active_worktree_creation(Some(display_name), true, cx);

    let worktree_path = action.path.clone();
    let agent = action.agent.clone();

    cx.spawn_in(window, async move |_workspace_entity, mut cx| {
        let result = do_switch_worktree(
            worktree_path,
            git_repo_work_dirs,
            non_git_paths,
            previous_state,
            workspace_handle.clone(),
            window_handle,
            remote_connection_options,
            &mut cx,
        )
        .await;

        // Only once the switch has landed: the window's active workspace is
        // the destination now, and that is the directory the agent has to run
        // in. A failed switch leaves the reader where they were, so starting
        // anything would put it in the wrong checkout.
        if result.is_ok()
            && let Some(agent) = agent
            && let Some(window_handle) = window_handle
        {
            window_handle
                .update(cx, |multi_workspace, window, cx| {
                    multi_workspace.workspace().update(cx, |workspace, cx| {
                        agent_ui::AgentView::open_tracked(
                            workspace,
                            &agent,
                            Default::default(),
                            window,
                            cx,
                        );
                    });
                })
                .log_err();
        }

        if let Err(err) = &result {
            log::error!("Failed to switch worktree: {err}");
            workspace_handle
                .update(cx, |workspace, cx| {
                    workspace.set_active_worktree_creation(None, false, cx);
                    show_error_toast(cx.entity(), "worktree switch", anyhow!("{err:#}"), cx);
                })
                .ok();
        }

        result
    })
    .detach_and_log_err(cx);
}

async fn do_create_worktree(
    git_repos: Vec<Entity<Repository>>,
    non_git_paths: Vec<PathBuf>,
    worktree_name: Option<String>,
    branch_target: NewWorktreeBranchTarget,
    previous_state: PreviousWorkspaceState,
    workspace: WeakEntity<Workspace>,
    window_handle: Option<gpui::WindowHandle<MultiWorkspace>>,
    remote_connection_options: Option<RemoteConnectionOptions>,
    agent: Option<String>,
    // An absolute directory overriding `git.worktree_directory` for this
    // creation only. The form offers it; the setting remains what every other
    // entry point uses.
    location: Option<PathBuf>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    // List existing worktrees from all repos to detect name collisions
    let worktree_receivers: Vec<_> = cx.update(|_, cx| {
        git_repos
            .iter()
            .map(|repo| repo.update(cx, |repo, _cx| repo.worktrees()))
            .collect()
    })?;
    let worktree_directory_setting = cx.update(|_, cx| {
        ProjectSettings::get_global(cx)
            .git
            .worktree_directory
            .clone()
    })?;

    let mut existing_worktree_names = Vec::new();
    let mut existing_worktree_paths = HashSet::default();
    for result in futures::future::join_all(worktree_receivers).await {
        match result {
            Ok(Ok(worktrees)) => {
                for worktree in worktrees {
                    if let Some(name) = worktree
                        .path
                        .parent()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                    {
                        existing_worktree_names.push(name.to_string());
                    }
                    existing_worktree_paths.insert(worktree.path.clone());
                }
            }
            Ok(Err(err)) => {
                Err::<(), _>(err).log_err();
            }
            Err(_) => {}
        }
    }

    let mut rng = rand::rng();

    let base_ref = resolve_worktree_branch_target(&branch_target);
    let new_branch = new_branch_for_worktree(&branch_target);

    let (creation_infos, path_remapping) = cx.update(|_, cx| {
        start_worktree_creations(
            &git_repos,
            worktree_name,
            &existing_worktree_names,
            &existing_worktree_paths,
            location.as_deref(),
            base_ref,
            new_branch,
            &worktree_directory_setting,
            &mut rng,
            cx,
        )
    })??;

    let fs = cx.update(|_, cx| <dyn Fs>::global(cx))?;

    let created_paths = await_and_rollback_on_failure(creation_infos, fs, cx).await?;

    let mut all_paths = created_paths;
    let has_non_git = !non_git_paths.is_empty();
    all_paths.extend(non_git_paths.iter().cloned());

    open_worktree_workspace(
        all_paths,
        path_remapping,
        non_git_paths,
        has_non_git,
        previous_state,
        workspace,
        window_handle,
        remote_connection_options,
        WorktreeOperation::Create,
        agent,
        cx,
    )
    .await
}

async fn do_switch_worktree(
    worktree_path: PathBuf,
    git_repo_work_dirs: Vec<PathBuf>,
    non_git_paths: Vec<PathBuf>,
    previous_state: PreviousWorkspaceState,
    workspace: WeakEntity<Workspace>,
    window_handle: Option<gpui::WindowHandle<MultiWorkspace>>,
    remote_connection_options: Option<RemoteConnectionOptions>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    let path_remapping: Vec<(PathBuf, PathBuf)> = git_repo_work_dirs
        .iter()
        .map(|work_dir| (work_dir.clone(), worktree_path.clone()))
        .collect();

    let mut all_paths = vec![worktree_path];
    let has_non_git = !non_git_paths.is_empty();
    all_paths.extend(non_git_paths.iter().cloned());

    open_worktree_workspace(
        all_paths,
        path_remapping,
        non_git_paths,
        has_non_git,
        previous_state,
        workspace,
        window_handle,
        remote_connection_options,
        WorktreeOperation::Switch,
        // Switching goes to a checkout that already exists; whatever is running
        // there is already running.
        None,
        cx,
    )
    .await
}

/// How long a worktree switch may take before it is worth saying so.
///
/// Logged rather than measured behind `ZED_MEASUREMENTS`: this is a latency
/// nobody could put a number on until it was instrumented -- the report that
/// prompted it was "switching feels laggy", which is not something a bug
/// report can act on. A line in the log when a switch runs long turns the next
/// such report into a measurement. Well under the threshold nothing is printed,
/// so it costs one `Instant::now()` on a path that already spawns tasks.
const SLOW_WORKTREE_SWITCH: std::time::Duration = std::time::Duration::from_millis(250);

/// Core workspace opening logic shared by both create and switch flows.
async fn open_worktree_workspace(
    all_paths: Vec<PathBuf>,
    path_remapping: Vec<(PathBuf, PathBuf)>,
    non_git_paths: Vec<PathBuf>,
    has_non_git: bool,
    previous_state: PreviousWorkspaceState,
    workspace: WeakEntity<Workspace>,
    window_handle: Option<gpui::WindowHandle<MultiWorkspace>>,
    remote_connection_options: Option<RemoteConnectionOptions>,
    operation: WorktreeOperation,
    agent: Option<String>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    let started_at = std::time::Instant::now();
    let window_handle = window_handle
        .ok_or_else(|| anyhow!("No window handle available for workspace creation"))?;

    let focused_dock = previous_state.focused_dock;

    let is_creating_new_worktree = matches!(operation, WorktreeOperation::Create);

    let source_for_transfer = if is_creating_new_worktree {
        Some(workspace.clone())
    } else {
        None
    };

    let (workspace_task, modal_workspace) =
        window_handle.update(cx, |multi_workspace, window, cx| {
            let path_list = util::path_list::PathList::new(&all_paths);
            let active_workspace = multi_workspace.workspace().clone();
            let modal_workspace = active_workspace.clone();

            // Carried on both paths, not creation alone. A checkout nobody has
            // opened yet has no layout of its own, so arriving at one shut the
            // docks the reader was working in -- the layout they are leaving is
            // the only starting point that is not a guess.
            //
            // A starting point rather than an override: a checkout that *has*
            // been opened before carries a record of its own, and
            // `Workspace::load_workspace` overwrites both fields this sets
            // while restoring it. So a dock deliberately shut over there stays
            // shut, and only a checkout with nothing recorded inherits.
            let dock_structure = previous_state.dock_structure;
            let dock_stacks = previous_state.dock_stacks;
            // Always `None` on the switch path: `do_switch_worktree` passes no
            // agent, because a switch starts its agent once the switch has
            // landed rather than while the workspace is still being built.
            let agent = agent.clone();
            let init: Option<
                Box<
                    dyn FnOnce(&mut Workspace, &mut gpui::Window, &mut gpui::Context<Workspace>)
                        + Send,
                >,
            > = Some(Box::new(
                move |workspace: &mut Workspace,
                      window: &mut gpui::Window,
                      cx: &mut gpui::Context<Workspace>| {
                    workspace.set_dock_layout(dock_structure, dock_stacks, window, cx);
                    if let Some(agent) = agent {
                        agent_ui::AgentView::open_tracked(
                            workspace,
                            &agent,
                            Default::default(),
                            window,
                            cx,
                        );
                    }
                },
            ));

            let task = multi_workspace.find_or_create_workspace_with_source_workspace(
                path_list,
                remote_connection_options,
                None,
                move |connection_options, window, cx| {
                    remote_connection::connect_with_modal(
                        &active_workspace,
                        connection_options,
                        window,
                        cx,
                    )
                },
                &[],
                init,
                OpenMode::Add,
                source_for_transfer.clone(),
                window,
                cx,
            );
            (task, modal_workspace)
        })?;

    let result = workspace_task.await;
    // Split out because the two halves fail differently: reaching the workspace
    // is a find or a full open, and everything after it is work done on a
    // workspace already in hand. A total alone cannot tell those apart.
    let reached_workspace_at = started_at.elapsed();
    remote_connection::dismiss_connection_modal(&modal_workspace, cx);
    let new_workspace = result?;

    let panels_task = new_workspace.update(cx, |workspace, _cx| workspace.take_panels_task());

    if let Some(task) = panels_task {
        task.await.log_err();
    }

    // Both of these serve the *create* path and nothing else, which is why
    // they are behind this branch rather than run for every switch.
    //
    // Creating a worktree makes a checkout that nothing has looked at yet: the
    // remapping below turns the previously-open file paths into paths under it
    // and opens them, and neither can work until the project has scanned and
    // the repository's own view has caught up with the `git worktree add` that
    // just ran.
    //
    // Switching has neither problem. The target workspace is already open, so
    // its scan finished long ago -- and `barrier` is not a cheap check that
    // notices this. It appends an empty job to each repository's *serial* queue
    // and waits for it, so it waits for every git job already queued: on the
    // switch path, a status refresh that activating the project had itself just
    // triggered. Nothing below consumed the result. It was latency in front of
    // every switch, buying nothing.
    //
    // The delay was worse than its own length. `handle_switch_worktree` refuses
    // to start while a switch is in flight, and the in-flight flag is only
    // cleared after these awaits -- so a second switch during the wait did
    // nothing at all, which is what made bouncing between two worktrees feel
    // unresponsive rather than merely slow.
    if is_creating_new_worktree {
        new_workspace
            .update(cx, |workspace, cx| {
                workspace.project().read(cx).wait_for_initial_scan(cx)
            })
            .await;

        new_workspace
            .update(cx, |workspace, cx| {
                let repos = workspace
                    .project()
                    .read(cx)
                    .repositories(cx)
                    .values()
                    .cloned()
                    .collect::<Vec<_>>();

                let tasks = repos
                    .into_iter()
                    .map(|repo| repo.update(cx, |repo, _| repo.barrier()));
                futures::future::join_all(tasks)
            })
            .await;
    }

    // Runs for both, and after the waits above rather than before: it resolves
    // each path to a worktree, which for a freshly created checkout only exists
    // once the scan has registered it. A switch's worktrees were registered when
    // the workspace was opened.
    maybe_propagate_worktree_trust(&workspace, &new_workspace, &all_paths, cx);

    if is_creating_new_worktree {
        window_handle.update(cx, |_multi_workspace, window, cx| {
            new_workspace.update(cx, |workspace, cx| {
                if has_non_git {
                    struct WorktreeCreationToast;
                    let toast_id =
                        workspace::notifications::NotificationId::unique::<WorktreeCreationToast>();
                    workspace.show_toast(
                        workspace::Toast::new(
                            toast_id,
                            "Some project folders are not git repositories. \
                             They were included as-is without creating a worktree.",
                        ),
                        cx,
                    );
                }

                // Remap every previously-open file path into the new worktree.
                let remap_path = |original_path: PathBuf| -> Option<PathBuf> {
                    let best_match = path_remapping
                        .iter()
                        .filter_map(|(old_root, new_root)| {
                            original_path.strip_prefix(old_root).ok().map(|relative| {
                                (old_root.components().count(), new_root.join(relative))
                            })
                        })
                        .max_by_key(|(depth, _)| *depth);

                    if let Some((_, remapped_path)) = best_match {
                        return Some(remapped_path);
                    }

                    for non_git in &non_git_paths {
                        if original_path.starts_with(non_git) {
                            return Some(original_path);
                        }
                    }
                    None
                };

                let remapped_active_path =
                    previous_state.active_file_path.and_then(|p| remap_path(p));

                let mut paths_to_open: Vec<PathBuf> = Vec::new();
                let mut seen = HashSet::default();
                for path in previous_state.open_file_paths {
                    if let Some(remapped) = remap_path(path) {
                        if remapped_active_path.as_ref() != Some(&remapped)
                            && seen.insert(remapped.clone())
                        {
                            paths_to_open.push(remapped);
                        }
                    }
                }

                if let Some(active) = &remapped_active_path {
                    if seen.insert(active.clone()) {
                        paths_to_open.push(active.clone());
                    }
                }

                if !paths_to_open.is_empty() {
                    let should_focus_center = focused_dock.is_none();
                    let open_task = workspace.open_paths(
                        paths_to_open,
                        workspace::OpenOptions {
                            focus: Some(false),
                            ..Default::default()
                        },
                        None,
                        window,
                        cx,
                    );
                    cx.spawn_in(window, async move |workspace, cx| {
                        for item in open_task.await.into_iter().flatten() {
                            item.log_err();
                        }
                        if should_focus_center {
                            workspace.update_in(cx, |workspace, window, cx| {
                                workspace.focus_center_pane(window, cx);
                            })?;
                        }
                        anyhow::Ok(())
                    })
                    .detach_and_log_err(cx);
                }
            });
        })?;
    }

    let elapsed = started_at.elapsed();
    if elapsed > SLOW_WORKTREE_SWITCH {
        log::info!(
            "worktree {} took {elapsed:?} ({reached_workspace_at:?} of it reaching the workspace)",
            if is_creating_new_worktree {
                "create"
            } else {
                "switch"
            },
        );
    }

    // Clear the creation status on the SOURCE workspace so its title bar
    // stops showing the loading indicator immediately.
    workspace
        .update(cx, |ws, cx| {
            ws.set_active_worktree_creation(None, false, cx);
        })
        .ok();

    window_handle.update(cx, |multi_workspace, window, cx| {
        multi_workspace.activate(new_workspace.clone(), source_for_transfer, window, cx);

        new_workspace.update(cx, |workspace, cx| {
            workspace.run_create_worktree_tasks(window, cx);
        });
    })?;

    if is_creating_new_worktree {
        if let Some(dock_position) = focused_dock {
            window_handle.update(cx, |_multi_workspace, window, cx| {
                new_workspace.update(cx, |workspace, cx| {
                    let dock = workspace.dock_at_position(dock_position);
                    if let Some(panel) = dock.read(cx).active_panel() {
                        panel.panel_focus_handle(cx).focus(window, cx);
                    }
                });
            })?;
        }
    }

    anyhow::Ok(())
}

#[cfg(test)]
mod rollback_tests {
    use super::partition_creations;
    use anyhow::anyhow;
    use std::path::PathBuf;

    fn path(name: &str) -> PathBuf {
        PathBuf::from(format!("/worktrees/{name}/project"))
    }

    /// The defect this exists for. Rolling back a creation deletes each path it
    /// is handed -- recursively, and before git is asked whether the path is a
    /// worktree at all. A path whose `git worktree add` failed was never this
    /// code's to delete: `git worktree add` refuses a directory that already
    /// has files in it, so the most likely reason a creation failed is that
    /// somebody's work is sitting there.
    #[test]
    fn a_worktree_that_failed_to_be_created_is_not_rolled_back() {
        let (created, error) = partition_creations(vec![
            ("repo-a", path("ok"), Ok(())),
            ("repo-b", path("collided"), Err(anyhow!("already exists"))),
        ]);

        assert!(error.is_some(), "the failure must still be reported");
        assert_eq!(
            created,
            vec![("repo-a", path("ok"))],
            "only the worktree that was actually created may be rolled back"
        );
        assert!(
            !created.iter().any(|(_, p)| *p == path("collided")),
            "the path git refused to touch must not be handed to a recursive delete"
        );
    }

    /// All-or-nothing is still the intent: a failure elsewhere undoes the ones
    /// that did succeed, so the project is not left half-migrated.
    #[test]
    fn worktrees_that_were_created_are_still_rolled_back() {
        let (created, error) = partition_creations(vec![
            ("repo-a", path("one"), Ok(())),
            ("repo-b", path("two"), Ok(())),
            ("repo-c", path("three"), Err(anyhow!("no"))),
        ]);

        assert!(error.is_some());
        assert_eq!(
            created,
            vec![("repo-a", path("one")), ("repo-b", path("two"))]
        );
    }

    #[test]
    fn nothing_is_rolled_back_when_every_creation_succeeded() {
        let (created, error) = partition_creations(vec![
            ("repo-a", path("one"), Ok(())),
            ("repo-b", path("two"), Ok(())),
        ]);

        assert!(error.is_none(), "no failure means no rollback at all");
        assert_eq!(created.len(), 2);
    }

    /// The first failure is what the person is told about; later ones would
    /// only bury it.
    #[test]
    fn the_first_failure_is_the_one_reported() {
        let (_created, error) = partition_creations(vec![
            ("repo-a", path("one"), Err(anyhow!("first"))),
            ("repo-b", path("two"), Err(anyhow!("second"))),
        ]);

        assert_eq!(error.expect("a failure").to_string(), "first");
    }
}

#[cfg(test)]
mod branch_target_tests {
    use super::{new_branch_for_worktree, resolve_worktree_branch_target};
    use zed_actions::NewWorktreeBranchTarget;

    /// The two questions a target answers are separate: what to base the
    /// checkout on, and whether to give it a branch. Confusing them is silent
    /// -- the worktree still appears, just with nowhere to commit.
    #[test]
    fn a_new_branch_is_based_on_head_and_names_itself() {
        let target = NewWorktreeBranchTarget::NewBranch {
            name: "feat.parser".into(),
            base: None,
        };

        assert_eq!(resolve_worktree_branch_target(&target), None);
        assert_eq!(
            new_branch_for_worktree(&target),
            Some("feat.parser".to_string())
        );
    }

    /// The reason `base` exists. "Create `x` based on `develop`" used to be
    /// expressible only by giving up the branch and checking `develop` out
    /// detached -- a worktree that looks right and has nowhere to commit.
    #[test]
    fn a_new_branch_can_start_from_something_other_than_head() {
        let target = NewWorktreeBranchTarget::NewBranch {
            name: "feat.parser".into(),
            base: Some("develop".into()),
        };

        assert_eq!(
            new_branch_for_worktree(&target),
            Some("feat.parser".to_string()),
            "it must still create the branch that was named"
        );
        assert_eq!(
            resolve_worktree_branch_target(&target),
            Some("develop".to_string()),
            "and start it from the branch that was picked"
        );
    }

    #[test]
    fn an_existing_branch_is_the_base_and_creates_nothing() {
        let target = NewWorktreeBranchTarget::ExistingBranch {
            name: "develop".into(),
        };

        assert_eq!(
            resolve_worktree_branch_target(&target),
            Some("develop".to_string())
        );
        assert_eq!(new_branch_for_worktree(&target), None);
    }

    #[test]
    fn the_current_branch_is_neither() {
        let target = NewWorktreeBranchTarget::CurrentBranch;

        assert_eq!(resolve_worktree_branch_target(&target), None);
        assert_eq!(new_branch_for_worktree(&target), None);
    }
}
