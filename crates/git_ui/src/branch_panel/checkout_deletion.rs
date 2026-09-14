//! Decides which agent sessions deleting a checkout would take, and what the
//! confirmation says about it. Nothing here deletes anything -- these are the
//! two pure questions a delete needs answered before it asks, kept apart from
//! the wiring that actually removes the worktree and stops its agent tabs so
//! that destructive code can be reviewed separately from the rule that
//! decides what it reaches.
//!
//! ## The rule that governs this file
//!
//! A worktree path is **never** mapped forward onto a provider's own store
//! directory. Claude's store directory is a lossy encoding of the cwd a
//! session ran in -- `/a/b-c` and `/a-b-c` both become `-a-b-c` (see
//! `agent_sessions::claude::ClaudeProvider`'s own doc) -- so a forward map
//! from "this worktree" to "that directory" can delete a *different*
//! project's transcripts, and nobody notices until they are gone. The only
//! safe route is the indexed sessions: each one's `cwd` was read out of its
//! own transcript, not guessed from a path, and `SessionSummary::is_within`
//! is a whole-component prefix match against that real value. Nothing below
//! ever builds a store path out of `worktree` itself -- every path a delete
//! could take comes from a session's own `Deletion`, resolved by its
//! provider.
//!
//! ## Nested worktrees
//!
//! `git worktree add <worktree>/inner` is legal, and `inner` survives the
//! removal of its parent. A plain prefix match against `worktree` alone
//! would sweep `inner`'s sessions too, so [`plan_session_deletion`] drops any
//! session that also falls within one of `other_live_worktrees` before it is
//! ever counted.
//!
//! A worktree merely sharing a name prefix (`/repo/wt-a` vs. `/repo/wt-ab`)
//! needs no such guard: `SessionSummary::is_within` compares whole path
//! components, so the two never match each other in the first place.
//!
//! ## Canonicalisation
//!
//! If a provider recorded a canonicalised cwd (`/private/var/...`) for a
//! worktree the caller passes as `/var/...`, the match misses and that
//! transcript survives the delete. That is the safe direction to fail in --
//! a leftover, not a loss -- so `worktree` and `other_live_worktrees` are
//! compared exactly as given, with no canonicalisation on either side.
//! Canonicalising `worktree` before the match would only widen what a
//! destructive operation can reach, which is the wrong direction to "fix"
//! this in.

// Nothing calls this module yet -- the checkout menu and the worktree picker
// will wire it in once their own changes land. It ships now, fully tested, so
// the rule that decides what a delete takes can be reviewed on its own,
// before the change that lets it actually run anything.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_sessions::{AgentKind, SessionIndex, SessionProvider};
use gpui::{App, Entity, Task, WeakEntity, Window};
use util::ResultExt as _;
use workspace::Workspace;
use workspace::pane::SaveIntent;

/// The scope sentence [`confirm_detail`] passes to `agent_ui::delete_all_detail`.
///
/// Not `agent_ui::PROJECT_SCOPE`: that sentence disclaims a search box the
/// session-history panel has and a checkout confirmation does not. A
/// checkout's scope is the worktree being removed, stated plainly instead.
const WORKTREE_SCOPE: &str = "Every session that ran in this worktree goes.";

/// Which indexed sessions deleting `worktree` would take.
///
/// Selects every session whose real `cwd` is within `worktree` and within
/// none of `other_live_worktrees` (the nested-worktree guard above), then
/// resolves what each one's own provider says a delete of it would take.
///
/// Providers are resolved once per [`AgentKind`] that actually appears in the
/// scoped sessions, not once per session: two of the four canonicalise a path
/// on construction, and a plan holding many sessions from one agent has no
/// reason to pay that syscall on every row.
pub(crate) fn plan_session_deletion(
    index: &SessionIndex,
    worktree: &Path,
    other_live_worktrees: &[PathBuf],
) -> agent_ui::DeleteAll {
    let roots = [worktree.to_path_buf()];
    let mut providers: HashMap<AgentKind, Arc<dyn SessionProvider>> = HashMap::new();
    let scoped = agent_ui::sessions_in_project(index.sessions().as_ref(), &roots)
        .filter(|session| !session.is_within(other_live_worktrees))
        .map(|session| {
            let provider = providers
                .entry(session.agent)
                .or_insert_with(|| agent_sessions::provider_for(session.agent));
            let deletion = provider.deletion(session);
            (session, deletion)
        });
    agent_ui::plan_delete_all(scoped)
}

/// Today's confirmation body, for a worktree deletion that reaches no
/// sessions. Kept as its own function -- and its text copied verbatim from
/// `checkout_menu`'s existing prompt rather than retyped -- so a plan holding
/// nothing produces exactly the sentence a user already sees today.
fn checkout_removal_detail(label: &str, path: &Path) -> String {
    format!(
        "{}\n\nThe worktree at {} will be removed. Its branch and its commits stay.",
        label,
        path.display()
    )
}

/// The confirmation body for deleting the checkout at `path` (labelled
/// `label`), given what `plan` would additionally take.
///
/// An empty plan returns [`checkout_removal_detail`] unchanged -- there is
/// nothing new to say. A non-empty plan appends `agent_ui::delete_all_detail`
/// (which already separates the recoverable trash count from opencode's
/// unrecoverable own-store count and reports the byte total, worded against
/// [`WORKTREE_SCOPE`]) plus one line naming the running agents this delete
/// stops, so the prompt does not describe only the git half of what pressing
/// the button does.
pub(crate) fn confirm_detail(label: &str, path: &Path, plan: &agent_ui::DeleteAll) -> String {
    let removal = checkout_removal_detail(label, path);
    if plan.is_empty() {
        return removal;
    }
    let sessions = agent_ui::delete_all_detail(
        plan.trashed(),
        plan.commanded(),
        plan.total_bytes,
        WORKTREE_SCOPE,
    );
    format!(
        "{removal}\n\n{sessions}\n\nAny agent still running in this worktree will be stopped first."
    )
}

/// The buttons for the confirmation: today's `["Delete", "Cancel"]` when the
/// plan takes nothing beyond the checkout itself, and a third route once
/// sessions are involved, so a user who wants to keep the transcripts is not
/// forced to choose only between taking everything and cancelling outright.
pub(crate) fn confirm_buttons(plan: &agent_ui::DeleteAll) -> &'static [&'static str] {
    if plan.is_empty() {
        &["Delete", "Cancel"]
    } else {
        &[
            "Delete Worktree and Sessions",
            "Delete Worktree Only",
            "Cancel",
        ]
    }
}

/// Every agent tab this window can reach that is running in `worktree`,
/// paired with the workspace that owns it -- `Workspace::pane_for` is
/// per-workspace, so [`close_agent_tabs`] needs to know which workspace's
/// panes to search, not just which view to close.
///
/// A tab's checkout is its workspace's root: `AgentView::open_tracked` takes
/// no working directory of its own, so an agent runs in whatever directory
/// its workspace's project is rooted at -- the same rule `agents_by_checkout`
/// already encodes (`data.rs`: *"A running agent has no other checkout it
/// could be editing"*). So this walks workspaces, not tabs, from two sources,
/// de-duplicated by entity: `panel_workspace` (the workspace the branch panel
/// itself belongs to) and whatever `other_reachable_workspaces` the caller
/// passes -- in practice `MultiWorkspace::retained_workspaces()`, the
/// checkouts this window has open but is not currently showing.
///
/// That list comes in as a parameter instead of being derived here from a
/// `Window`, because it cannot be: reaching a `MultiWorkspace` through
/// `WindowHandle::read`/`::update` fails ("window not found") for the very
/// window a caller is already inside, and holding `&Window` at all means
/// being there. A caller with `&MultiWorkspace` already in hand -- from its
/// own `window_handle.update(cx, ...)` inside a spawned task, the way
/// `worktree_service.rs` reaches `MultiWorkspace` -- already has
/// `retained_workspaces()` for free and just passes it through.
///
/// Two tab populations are deliberately left unreached here, not silently
/// approximated: other windows (`cx.windows()` could reach them, but closing
/// tabs in a window the user is not looking at promises more than a checkout
/// menu ever has), and floating-pane tabs (`Workspace::items_of_type` walks
/// `self.panes`; a `FloatingPane` owns its pane outside that list).
pub(crate) fn agent_tabs_in(
    worktree: &Path,
    panel_workspace: &WeakEntity<Workspace>,
    other_reachable_workspaces: &[Entity<Workspace>],
    cx: &App,
) -> Vec<(Entity<Workspace>, Entity<agent_ui::AgentView>)> {
    let mut candidates: Vec<Entity<Workspace>> = Vec::new();
    if let Some(workspace) = panel_workspace.upgrade() {
        candidates.push(workspace);
    }
    candidates.extend(other_reachable_workspaces.iter().cloned());

    let mut seen = HashSet::with_capacity(candidates.len());
    let mut tabs = Vec::new();
    for workspace in candidates {
        if !seen.insert(workspace.entity_id()) {
            continue;
        }
        let rooted_here = workspace
            .read(cx)
            .root_paths(cx)
            .iter()
            .any(|root| root.starts_with(worktree));
        if !rooted_here {
            continue;
        }
        for view in workspace.read(cx).items_of_type::<agent_ui::AgentView>(cx) {
            tabs.push((workspace.clone(), view));
        }
    }
    tabs
}

/// Closes every tab [`agent_tabs_in`] found, which is enough to kill its CLI:
/// `AgentView` does not override `Item::is_dirty`, so `SaveIntent::Skip` fires
/// no save prompt here -- an agent tab has nothing to save, and a prompt would
/// stack a second modal on the one the user just answered to get here.
/// Dropping the closed tab's `TerminalView` then runs `Drop for Terminal`,
/// which asks the CLI to shut down and kills it if that does not land in time.
///
/// Runs as its own spawned task rather than against a `Window` the caller
/// might already be inside: a caller holding `panel_workspace` (or the
/// window's `MultiWorkspace`) leased and updating one of these same
/// workspaces synchronously is exactly the abort this codebase has hit
/// before (`split_for_drop`, `show_error_toast`). Spawning gives each
/// workspace's `update_in` its own later turn, once nothing above it is
/// mid-update.
pub(crate) fn close_agent_tabs(
    tabs: Vec<(Entity<Workspace>, Entity<agent_ui::AgentView>)>,
    window: &Window,
    cx: &App,
) -> Task<()> {
    window.spawn(cx, async move |cx| {
        let mut closing = Vec::with_capacity(tabs.len());
        for (workspace, view) in tabs {
            let item_id = view.entity_id();
            let pane_close = workspace.update_in(cx, |workspace, window, cx| {
                workspace.pane_for(&view).map(|pane| {
                    pane.update(cx, |pane, cx| {
                        pane.close_item_by_id(item_id, SaveIntent::Skip, window, cx)
                    })
                })
            });
            match pane_close {
                Ok(Some(task)) => closing.push(task),
                Ok(None) => {}
                Err(err) => {
                    log::error!("failed to reach an agent tab's workspace to close it: {err:#}")
                }
            }
        }
        for close in closing {
            close.await.log_err();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_sessions::SessionSummary;
    use std::time::SystemTime;

    /// A Claude session: `deletion()` reports `Trash` only when `log_path` is
    /// set, `Nothing` otherwise -- so this builder controls that shape by
    /// giving each session a log path under its own `cwd`.
    fn claude_session(id: &str, cwd: &str) -> SessionSummary {
        let cwd = PathBuf::from(cwd);
        let log_path = cwd.join(format!("{id}.jsonl"));
        SessionSummary {
            id: Arc::from(id),
            agent: AgentKind::Claude,
            title: id.to_string(),
            preview: String::new(),
            preview_speaker: None,
            cwd,
            branch: None,
            model: None,
            updated_at: SystemTime::UNIX_EPOCH,
            log_path: Some(log_path),
            log_bytes: 1024,
        }
    }

    /// An opencode session: `deletion()` always reports `Command` -- opencode
    /// has no file of its own this editor can trash.
    fn opencode_session(id: &str, cwd: &str) -> SessionSummary {
        SessionSummary {
            agent: AgentKind::OpenCode,
            ..claude_session(id, cwd)
        }
    }

    #[test]
    fn a_session_inside_the_worktree_is_taken() {
        let index = SessionIndex::new(vec![claude_session("a", "/repo/wt-a")]);
        let plan = plan_session_deletion(&index, Path::new("/repo/wt-a"), &[]);
        assert_eq!(plan.count(), 1);
        assert_eq!(plan.targets[0].id.as_ref(), "a");
    }

    #[test]
    fn a_session_in_a_nested_live_worktree_is_not_taken() {
        let index = SessionIndex::new(vec![
            claude_session("outer", "/repo/wt-a"),
            claude_session("inner", "/repo/wt-a/inner"),
        ]);
        let other_live = [PathBuf::from("/repo/wt-a/inner")];
        let plan = plan_session_deletion(&index, Path::new("/repo/wt-a"), &other_live);
        assert_eq!(plan.count(), 1);
        assert_eq!(plan.targets[0].id.as_ref(), "outer");
    }

    #[test]
    fn a_sibling_worktree_sharing_a_name_prefix_is_untouched() {
        let index = SessionIndex::new(vec![claude_session("sibling", "/repo/wt-ab")]);
        let plan = plan_session_deletion(&index, Path::new("/repo/wt-a"), &[]);
        assert!(plan.is_empty());
    }

    #[test]
    fn a_worktree_with_no_sessions_produces_an_empty_plan() {
        let index = SessionIndex::new(vec![claude_session("elsewhere", "/repo/other")]);
        let plan = plan_session_deletion(&index, Path::new("/repo/wt-a"), &[]);
        assert!(plan.is_empty());
    }

    #[test]
    fn a_worktree_with_no_sessions_keeps_todays_prompt_exactly() {
        let plan = agent_ui::DeleteAll {
            targets: Vec::new(),
            total_bytes: 0,
        };
        let detail = confirm_detail("wt-a", Path::new("/repo/wt-a"), &plan);
        assert_eq!(
            detail,
            "wt-a\n\nThe worktree at /repo/wt-a will be removed. Its branch and its commits stay."
        );
        assert_eq!(confirm_buttons(&plan), &["Delete", "Cancel"]);
    }

    #[test]
    fn a_plan_holding_only_trashed_sessions_says_they_are_recoverable() {
        let index = SessionIndex::new(vec![claude_session("a", "/repo/wt-a")]);
        let plan = plan_session_deletion(&index, Path::new("/repo/wt-a"), &[]);
        let detail = confirm_detail("wt-a", Path::new("/repo/wt-a"), &plan);
        assert!(detail.contains("will move to the trash"));
        assert!(detail.contains("can be recovered from there"));
        assert!(detail.contains(WORKTREE_SCOPE));
        assert!(detail.contains("stopped first"));
        assert!(!detail.contains("cannot be undone"));
    }

    #[test]
    fn a_plan_holding_an_opencode_session_says_it_cannot_be_undone() {
        let index = SessionIndex::new(vec![opencode_session("a", "/repo/wt-a")]);
        let plan = plan_session_deletion(&index, Path::new("/repo/wt-a"), &[]);
        let detail = confirm_detail("wt-a", Path::new("/repo/wt-a"), &plan);
        assert!(detail.contains("their agents' own"));
        assert!(detail.contains("This cannot be undone."));
        assert!(detail.contains(WORKTREE_SCOPE));
    }

    #[test]
    fn a_mixed_plan_names_both_the_trashed_and_the_commanded_counts() {
        let index = SessionIndex::new(vec![
            claude_session("trashed", "/repo/wt-a"),
            opencode_session("commanded", "/repo/wt-a"),
        ]);
        let plan = plan_session_deletion(&index, Path::new("/repo/wt-a"), &[]);
        let detail = confirm_detail("wt-a", Path::new("/repo/wt-a"), &plan);
        assert!(detail.contains("1 session will move to the trash"));
        assert!(detail.contains("1 session will be deleted from their"));
        assert!(detail.contains("This cannot be undone."));
        assert!(detail.contains(WORKTREE_SCOPE));
    }

    #[test]
    fn the_prompt_names_the_byte_total_and_the_count() {
        let index = SessionIndex::new(vec![
            claude_session("a", "/repo/wt-a"),
            claude_session("b", "/repo/wt-a"),
        ]);
        let plan = plan_session_deletion(&index, Path::new("/repo/wt-a"), &[]);
        assert_eq!(plan.total_bytes, 2048);
        let detail = confirm_detail("wt-a", Path::new("/repo/wt-a"), &plan);
        assert!(detail.contains("2 sessions will move to the trash (2 KB)"));
    }

    #[test]
    fn the_buttons_offer_a_worktree_only_route_when_sessions_exist() {
        let index = SessionIndex::new(vec![claude_session("a", "/repo/wt-a")]);
        let plan = plan_session_deletion(&index, Path::new("/repo/wt-a"), &[]);
        assert_eq!(
            confirm_buttons(&plan),
            &[
                "Delete Worktree and Sessions",
                "Delete Worktree Only",
                "Cancel"
            ]
        );
    }
}

/// [`agent_tabs_in`] and [`close_agent_tabs`] need a real `Workspace` (they
/// read `root_paths` and walk panes), so these run as `gpui::test`s rather
/// than the plain `#[test]`s above. No test starts a real agent CLI --
/// `AgentView::open_tracked` builds the tab and returns; there is no agent
/// binary in a test environment for `AgentView::new`'s own `start` to find,
/// so the tab lands in an errored state deterministically rather than
/// spawning a real pty. What is asserted is the tab's presence and its
/// removal, not anything about a process.
#[cfg(test)]
mod agent_tab_tests {
    use super::*;
    use gpui::TestAppContext;
    use serde_json::json;
    use workspace::MultiWorkspace;

    fn init_test(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
            // Its own in-memory database, per test -- otherwise every test in
            // this binary shares the process-wide fallback and the ones that
            // write agent-view rows in parallel intermittently lose the table
            // lock.
            cx.set_global(db::AppDatabase::test_new());
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            editor::init(cx);
        });
    }

    async fn workspace_rooted_at<'a>(
        cx: &'a mut TestAppContext,
        root: &str,
    ) -> (Entity<Workspace>, &'a mut gpui::VisualTestContext) {
        let fs = fs::FakeFs::new(cx.background_executor.clone());
        fs.insert_tree(root, json!({ "file.txt": "contents" }))
            .await;
        let project = project::Project::test(fs, [root.as_ref()], cx).await;
        cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx))
    }

    async fn open_agent_tab(workspace: &Entity<Workspace>, cx: &mut gpui::VisualTestContext) {
        workspace.update_in(cx, |workspace, window, cx| {
            agent_ui::AgentView::open_tracked(
                workspace,
                project::CLAUDE_CODE_AGENT_ID,
                Default::default(),
                window,
                cx,
            );
        });
        cx.run_until_parked();
    }

    #[gpui::test]
    async fn tabs_in_the_removed_checkout_are_closed(cx: &mut TestAppContext) {
        init_test(cx);
        let (workspace, cx) = workspace_rooted_at(cx, "/repo/wt-a").await;
        open_agent_tab(&workspace, cx).await;
        open_agent_tab(&workspace, cx).await;

        let worktree = Path::new("/repo/wt-a");
        let panel_workspace = workspace.downgrade();
        let tabs = workspace.read_with(cx, |_workspace, cx| {
            agent_tabs_in(worktree, &panel_workspace, &[], cx)
        });
        assert_eq!(tabs.len(), 2, "both open tabs belong to this checkout");

        let close = workspace.update_in(cx, |_workspace, window, cx| {
            close_agent_tabs(tabs, window, cx)
        });
        close.await;
        cx.run_until_parked();

        workspace.read_with(cx, |workspace, cx| {
            assert_eq!(
                workspace.items_of_type::<agent_ui::AgentView>(cx).count(),
                0,
                "closing an agent tab has to remove it, not merely stop tracking it"
            );
        });
    }

    #[gpui::test]
    async fn tabs_in_another_checkout_are_left_alone(cx: &mut TestAppContext) {
        init_test(cx);
        let (workspace, cx) = workspace_rooted_at(cx, "/repo/wt-b").await;
        open_agent_tab(&workspace, cx).await;

        let worktree = Path::new("/repo/wt-a");
        let panel_workspace = workspace.downgrade();
        let tabs = workspace.read_with(cx, |_workspace, cx| {
            agent_tabs_in(worktree, &panel_workspace, &[], cx)
        });
        assert!(
            tabs.is_empty(),
            "a tab rooted outside the removed worktree must not be collected"
        );
    }

    /// The panel's own workspace (`workspace_b`) is rooted elsewhere; the tab
    /// only turns up through the `other_reachable_workspaces` a caller passes
    /// in -- in production, `MultiWorkspace::retained_workspaces()`, read by
    /// whoever already holds `&MultiWorkspace`, never re-derived by this
    /// function itself. See `agent_tabs_in`'s own doc for why it cannot
    /// re-derive that list from a `Window`.
    #[gpui::test]
    async fn a_retained_workspace_in_this_window_is_covered(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = fs::FakeFs::new(cx.background_executor.clone());
        fs.insert_tree("/repo/wt-a", json!({ "file.txt": "contents" }))
            .await;
        fs.insert_tree("/repo/wt-b", json!({ "file.txt": "contents" }))
            .await;
        let project_a = project::Project::test(fs.clone(), ["/repo/wt-a".as_ref()], cx).await;
        let project_b = project::Project::test(fs.clone(), ["/repo/wt-b".as_ref()], cx).await;

        let (multi_workspace, cx) = cx
            .add_window_view(|window, cx| MultiWorkspace::test_new(project_a.clone(), window, cx));

        let workspace_a =
            multi_workspace.read_with(cx, |multi_workspace, _| multi_workspace.workspace().clone());
        open_agent_tab(&workspace_a, cx).await;

        // Retains `workspace_a` before a second workspace activates, the way
        // switching checkouts does -- so it survives in `retained_workspaces`
        // rather than being torn down when `workspace_b` becomes active.
        multi_workspace.update(cx, |multi_workspace, cx| {
            multi_workspace.test_enable_background_retention(cx);
        });
        let workspace_b = multi_workspace.update_in(cx, |multi_workspace, window, cx| {
            multi_workspace.test_add_workspace(project_b.clone(), window, cx)
        });
        cx.run_until_parked();

        let retained = multi_workspace.read_with(cx, |multi_workspace, _| {
            multi_workspace.retained_workspaces().to_vec()
        });

        let worktree = Path::new("/repo/wt-a");
        let panel_workspace = workspace_b.downgrade();
        let tabs = workspace_b.read_with(cx, |_workspace_b, cx| {
            agent_tabs_in(worktree, &panel_workspace, &retained, cx)
        });
        assert_eq!(
            tabs.len(),
            1,
            "the tab lives in a retained workspace, not the panel's own"
        );
        assert_eq!(tabs[0].0.entity_id(), workspace_a.entity_id());
    }

    #[gpui::test]
    async fn closing_no_tabs_is_not_an_error(cx: &mut TestAppContext) {
        init_test(cx);
        let (workspace, cx) = workspace_rooted_at(cx, "/repo/wt-a").await;

        let close = workspace.update_in(cx, |_workspace, window, cx| {
            close_agent_tabs(Vec::new(), window, cx)
        });
        close.await;
    }
}
