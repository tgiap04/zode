//! The agent tabs of the whole process, for the questions no single tab can
//! answer.
//!
//! A tab knows its own session. Whether another tab is running a session, or is
//! about to, is a question about all of them across every window, and nothing
//! else keeps that list: the workspaces hold their tabs, and a delete, a resume
//! or a search for a transcript does not belong to any one workspace. Tabs are
//! held weakly, so the registry never keeps one alive.

use crate::agent_view::AgentView;
use collections::HashSet;
use gpui::{
    App, BorrowAppContext as _, Context, Entity, EntityId, Global, SharedString, Subscription,
    WeakEntity, Window,
};
use project::AgentId;
use std::sync::Arc;
use workspace::{MultiWorkspace, Workspace};

#[derive(Default)]
struct AgentTabs {
    views: Vec<WeakEntity<AgentView>>,
    /// Sessions a tab is about to be built for. A tab is built from a deferred
    /// task, and a second request landing before it runs would otherwise find
    /// nothing and build a second one.
    opening: HashSet<(AgentId, SharedString)>,
}

impl Global for AgentTabs {}

/// Records a tab as it is built. Tabs that have gone are dropped here, so the
/// list is bounded by what is alive plus whatever has closed since the last
/// tab opened.
///
/// Opening a tab and closing one both notify the global, which is what lets a
/// list of sessions say which have a tab without anyone telling it.
pub(crate) fn register(cx: &mut Context<AgentView>) {
    cx.on_release(|_, cx| announce_change(cx)).detach();
    let view = cx.weak_entity();
    let tabs = cx.default_global::<AgentTabs>();
    tabs.views.retain(|view| view.upgrade().is_some());
    tabs.views.push(view);
    announce_change(cx);
}

pub(crate) fn announce_change(cx: &mut App) {
    if cx.has_global::<AgentTabs>() {
        cx.update_global::<AgentTabs, _>(|_, _| {});
    }
}

/// Calls `on_change` whenever a tab opens or closes, or starts following a new
/// session.
pub fn observe_tabs<T: 'static>(
    cx: &mut Context<T>,
    on_change: impl FnMut(&mut T, &mut Context<T>) + 'static,
) -> Subscription {
    cx.observe_global::<AgentTabs>(on_change)
}

/// Every agent tab in the process, whichever window or workspace holds it.
pub fn all_agent_tabs(cx: &App) -> Vec<Entity<AgentView>> {
    live(cx)
}

fn live(cx: &App) -> Vec<Entity<AgentView>> {
    cx.try_global::<AgentTabs>()
        .map(|tabs| tabs.views.iter().filter_map(WeakEntity::upgrade).collect())
        .unwrap_or_default()
}

/// Every session a tab whose CLI is alive is running, with the agent that runs
/// it.
///
/// Alive, not merely open: a tab whose CLI ended is scrollback that nothing
/// writes to, and whatever happens to its transcript costs it nothing.
///
/// Reads every tab, so it must not be called from inside an update of one.
pub(crate) fn running_sessions(cx: &App) -> HashSet<(AgentId, SharedString)> {
    live(cx)
        .iter()
        .filter_map(|view| {
            let view = view.read(cx);
            let id = view.session_id()?;
            view.is_working(cx)
                .then(|| (view.agent_id().clone(), SharedString::from(id.to_string())))
        })
        .collect()
}

/// Every session that has a tab, whatever state its CLI is in. A tab whose CLI
/// ended still holds the conversation on screen, and is where a person would go
/// to read it.
///
/// Reads every tab, so it must not be called from inside an update of one.
pub(crate) fn open_sessions(cx: &App) -> HashSet<(AgentId, SharedString)> {
    live(cx)
        .iter()
        .filter_map(|view| {
            let view = view.read(cx);
            let id = view.session_id()?;
            Some((view.agent_id().clone(), SharedString::from(id.to_string())))
        })
        .collect()
}

/// The tab that has `session_id` open under `agent`, if any.
pub(crate) fn open_tab(agent: &AgentId, session_id: &str, cx: &App) -> Option<Entity<AgentView>> {
    live(cx).into_iter().find(|view| {
        let view = view.read(cx);
        view.is_agent(agent) && view.session_id() == Some(session_id)
    })
}

/// Brings `view` to the front wherever it lives: its window raised, its
/// workspace made the active one, and its pane showing it.
///
/// `window` is the one the caller is running in. It is already being updated,
/// so it cannot be reached through its handle and is handled directly; any
/// other window is entered through its handle.
pub fn go_to_tab(view: &Entity<AgentView>, window: &mut Window, cx: &mut App) {
    let Some(workspace) = view.read(cx).workspace().upgrade() else {
        return;
    };
    cx.activate(true);
    if let Some(Some(multi)) = window.root::<MultiWorkspace>()
        && multi
            .read(cx)
            .workspaces()
            .any(|candidate| candidate == &workspace)
    {
        multi.update(cx, |multi, cx| {
            multi.activate(workspace.clone(), None, window, cx)
        });
        reveal(&workspace, view, window, cx);
        return;
    }
    for handle in cx.windows() {
        let Some(handle) = handle.downcast::<MultiWorkspace>() else {
            continue;
        };
        let workspace = workspace.clone();
        let view = view.clone();
        // Fails for a window that is closing or is the one already updating;
        // neither can hold the tab we are after.
        let found = handle
            .update(cx, |multi, window, cx| {
                if !multi.workspaces().any(|candidate| candidate == &workspace) {
                    return false;
                }
                multi.activate(workspace.clone(), None, window, cx);
                reveal(&workspace, &view, window, cx);
                true
            })
            .unwrap_or(false);
        if found {
            return;
        }
    }
}

fn reveal(
    workspace: &Entity<Workspace>,
    view: &Entity<AgentView>,
    window: &mut Window,
    cx: &mut App,
) {
    window.activate_window();
    workspace.update(cx, |workspace, cx| {
        workspace.activate_item(view, true, true, window, cx)
    });
}

/// The sessions tabs other than `except` follow, whatever state their CLI is
/// in. The one asking is left out because its own id is not one it can mistake
/// for a successor.
///
/// Reads the other tabs, never `except`, so it can be called from inside an
/// update of that one.
pub(crate) fn followed_session_ids(except: EntityId, cx: &App) -> Vec<Arc<str>> {
    live(cx)
        .iter()
        .filter(|view| view.entity_id() != except)
        .filter_map(|view| view.read(cx).session_id().map(Arc::from))
        .collect()
}

/// Claims `session_id` as being opened by `agent`. `false` when it already is --
/// by an earlier request that has not built its tab yet, or by a tab that has
/// been built and is still starting its CLI -- and the caller opens nothing.
pub(crate) fn begin_opening(agent: &AgentId, session_id: &SharedString, cx: &mut App) -> bool {
    if is_opening(agent, session_id, cx) {
        return false;
    }
    cx.default_global::<AgentTabs>()
        .opening
        .insert((agent.clone(), session_id.clone()));
    true
}

/// Releases a claim made by [`begin_opening`], whether or not a tab came of it.
pub(crate) fn finish_opening(agent: &AgentId, session_id: &SharedString, cx: &mut App) {
    cx.default_global::<AgentTabs>()
        .opening
        .remove(&(agent.clone(), session_id.clone()));
}

/// Whether a tab for `session_id` is being opened.
///
/// A tab counts until its CLI has started, not only until it is built:
/// resolving the binary and looking the session up take long enough for a
/// second request to arrive, and the tab is not yet running anything a check
/// for a running session could find.
fn is_opening(agent: &AgentId, session_id: &SharedString, cx: &App) -> bool {
    let Some(tabs) = cx.try_global::<AgentTabs>() else {
        return false;
    };
    tabs.opening.contains(&(agent.clone(), session_id.clone()))
        || live(cx).iter().any(|view| {
            let view = view.read(cx);
            view.is_agent(agent)
                && view.is_starting()
                && view.session_id() == Some(session_id.as_ref())
        })
}

/// How many sessions are claimed as opening and have not been released.
#[cfg(test)]
pub(crate) fn claims_outstanding(cx: &App) -> usize {
    cx.try_global::<AgentTabs>()
        .map_or(0, |tabs| tabs.opening.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_view::{SessionIntent, SessionOrigin};
    use fs::FakeFs;
    use gpui::{AppContext as _, TestAppContext};
    use project::Project;
    use zed_actions::agent::AgentViewMode;

    fn tab(
        agent: &str,
        session: Option<&str>,
        project: &Entity<Project>,
        cx: &mut TestAppContext,
    ) -> Entity<AgentView> {
        let intent = session.map_or(SessionIntent::Untracked, |id| {
            SessionIntent::Tracked(id.to_string().into())
        });
        let project = project.clone();
        let agent = AgentId::new(agent.to_string());
        cx.new(|cx| {
            AgentView::test_new(
                agent,
                AgentViewMode::Terminal,
                project,
                WeakEntity::new_invalid(),
                SessionOrigin::new(intent, None),
                cx,
            )
        })
    }

    async fn project(cx: &mut TestAppContext) -> Entity<Project> {
        cx.update(|cx| {
            let store = settings::SettingsStore::test(cx);
            cx.set_global(store);
        });
        Project::test(FakeFs::new(cx.executor()), [], cx).await
    }

    #[gpui::test]
    async fn only_a_tab_whose_cli_is_alive_is_running_its_session(cx: &mut TestAppContext) {
        let project = project(cx).await;
        let alive = tab(project::CLAUDE_CODE_AGENT_ID, Some("one"), &project, cx);
        let dead = tab(project::CLAUDE_CODE_AGENT_ID, Some("two"), &project, cx);
        let untracked = tab(project::CLAUDE_CODE_AGENT_ID, None, &project, cx);
        alive.update(cx, |view, cx| view.simulate_cli_alive(true, cx));
        untracked.update(cx, |view, cx| view.simulate_cli_alive(true, cx));
        drop(dead);

        let running = cx.update(|cx| running_sessions(cx));
        let claude = AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string());
        assert!(running.contains(&(claude.clone(), SharedString::from("one"))));
        assert!(!running.contains(&(claude, SharedString::from("two"))));
        assert_eq!(running.len(), 1, "a tab with no session runs none");
    }

    #[gpui::test]
    async fn a_tab_opening_or_closing_notifies_observers(cx: &mut TestAppContext) {
        struct Probe(usize);
        let project = project(cx).await;
        let probe = cx.new(|_| Probe(0));
        let _subscription = probe.update(cx, |_, cx| {
            observe_tabs(cx, |probe: &mut Probe, _| probe.0 += 1)
        });

        let tab = tab(project::CLAUDE_CODE_AGENT_ID, Some("one"), &project, cx);
        cx.run_until_parked();
        let opened = probe.read_with(cx, |probe, _| probe.0);
        assert!(opened > 0, "opening a tab must notify");

        drop(tab);
        // Dropped outside an update, so the release is only seen by the next one.
        cx.update(|_| {});
        cx.run_until_parked();
        assert!(
            probe.read_with(cx, |probe, _| probe.0) > opened,
            "closing a tab must notify"
        );
    }

    #[gpui::test]
    async fn a_tab_is_open_for_its_session_alive_or_not(cx: &mut TestAppContext) {
        let project = project(cx).await;
        let alive = tab(project::CLAUDE_CODE_AGENT_ID, Some("one"), &project, cx);
        let exited = tab(project::CLAUDE_CODE_AGENT_ID, Some("two"), &project, cx);
        let _untracked = tab(project::CLAUDE_CODE_AGENT_ID, None, &project, cx);
        alive.update(cx, |view, cx| view.simulate_cli_alive(true, cx));

        let claude = AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string());
        let open = cx.update(|cx| open_sessions(cx));
        assert_eq!(open.len(), 2, "an untracked tab opens no session");
        assert!(open.contains(&(claude.clone(), SharedString::from("two"))));
        assert_eq!(
            cx.update(|cx| open_tab(&claude, "two", cx)),
            Some(exited),
            "a tab whose CLI is gone is still where the session is open"
        );
        let codex = AgentId::new(project::CODEX_AGENT_ID.to_string());
        assert!(cx.update(|cx| open_tab(&codex, "two", cx)).is_none());
    }

    #[gpui::test]
    async fn a_session_is_followed_by_every_tab_but_the_one_asking(cx: &mut TestAppContext) {
        let project = project(cx).await;
        let asking = tab(project::CLAUDE_CODE_AGENT_ID, Some("mine"), &project, cx);
        let _other = tab(project::CLAUDE_CODE_AGENT_ID, Some("theirs"), &project, cx);
        let _stopped = tab(project::CLAUDE_CODE_AGENT_ID, Some("idle"), &project, cx);

        let mut followed = cx.update(|cx| followed_session_ids(asking.entity_id(), cx));
        followed.sort();
        assert_eq!(followed, vec![Arc::from("idle"), Arc::from("theirs")]);
    }

    #[gpui::test]
    async fn a_closed_tab_is_neither_running_nor_followed(cx: &mut TestAppContext) {
        let project = project(cx).await;
        let closed = tab(project::CLAUDE_CODE_AGENT_ID, Some("gone"), &project, cx);
        closed.update(cx, |view, cx| view.simulate_cli_alive(true, cx));
        drop(closed);
        cx.run_until_parked();

        assert!(cx.update(|cx| running_sessions(cx)).is_empty());
        assert!(
            cx.update(|cx| followed_session_ids(EntityId::from(0u64), cx))
                .is_empty()
        );
    }

    #[gpui::test]
    async fn a_session_is_opened_once_until_the_claim_is_released(cx: &mut TestAppContext) {
        let agent = AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string());
        let id = SharedString::from("session");
        let other_agent = AgentId::new(project::CODEX_AGENT_ID.to_string());

        assert!(cx.update(|cx| begin_opening(&agent, &id, cx)));
        assert!(
            !cx.update(|cx| begin_opening(&agent, &id, cx)),
            "a second request for the same session opens nothing"
        );
        assert!(
            cx.update(|cx| begin_opening(&other_agent, &id, cx)),
            "the same id under another agent is another session"
        );
        assert!(cx.update(|cx| begin_opening(&agent, &"another".into(), cx)));

        cx.update(|cx| finish_opening(&agent, &id, cx));
        assert!(
            cx.update(|cx| begin_opening(&agent, &id, cx)),
            "released, it can be opened again"
        );
    }

    #[gpui::test]
    async fn a_tab_still_starting_holds_its_session_as_opening(cx: &mut TestAppContext) {
        let project = project(cx).await;
        let agent = AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string());
        let starting = tab(project::CLAUDE_CODE_AGENT_ID, Some("session"), &project, cx);

        assert!(
            !cx.update(|cx| begin_opening(&agent, &"session".into(), cx)),
            "built but not yet running its CLI is still opening"
        );
        assert!(cx.update(|cx| begin_opening(&agent, &"other".into(), cx)));
        drop(starting);
    }
}
