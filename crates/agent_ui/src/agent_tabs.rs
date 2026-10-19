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
use gpui::{App, Entity, EntityId, Global, SharedString, WeakEntity};
use project::AgentId;
use std::sync::Arc;

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
pub(crate) fn register(view: WeakEntity<AgentView>, cx: &mut App) {
    let tabs = cx.default_global::<AgentTabs>();
    tabs.views.retain(|view| view.upgrade().is_some());
    tabs.views.push(view);
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
