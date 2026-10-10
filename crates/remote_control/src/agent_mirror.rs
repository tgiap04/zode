//! Watches every agent tab and turns what it says about itself into the
//! summaries a remote device lists.
//!
//! Modelled on `agent_notify`: tabs are found through `observe_new` and
//! followed through their own events, and nothing in here owns a tab. The
//! internal types never leave this file -- what goes over the wire is
//! `AgentSummary`, so the wire format does not depend on them.
//!
//! Finding tabs is always on and costs a weak handle each. Following them is
//! only on while remote control is, so a Zode that has the feature switched
//! off carries no subscription and does no work per tab event.

use std::collections::HashMap;

use agent_ui::{AgentView, AgentViewEvent};
use gpui::{App, Context, EntityId, EventEmitter, Subscription, WeakEntity};
use project::builtin_agent;
use remote_relay_protocol::{AgentStatus, AgentSummary};
use util::ResultExt as _;

/// The id a terminal is listed and attached under.
pub fn terminal_id(entity_id: EntityId) -> String {
    format!("terminal-{}", entity_id.as_u64())
}

fn agent_id_without_terminal(entity_id: EntityId) -> String {
    format!("agent-{}", entity_id.as_u64())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentMirrorEvent {
    /// A tab appeared or one of its fields changed.
    Changed(AgentSummary),
    Removed {
        id: String,
    },
}

struct Tracked {
    view: WeakEntity<AgentView>,
    last: Option<AgentSummary>,
    /// Present only while mirroring is on.
    following: Vec<Subscription>,
    _release: Subscription,
}

pub struct AgentMirror {
    agents: HashMap<EntityId, Tracked>,
    active: bool,
    _observe_new: Subscription,
}

impl EventEmitter<AgentMirrorEvent> for AgentMirror {}

/// How a tab reads from the other side of the world.
///
/// A tab with no terminal is one that has not started or has handed its
/// terminal back, and neither is "finished" in a way worth announcing; one
/// whose CLI is no longer running is.
pub(crate) fn status_for(
    has_terminal: bool,
    working: bool,
    answering: bool,
    awaiting_approval: bool,
) -> AgentStatus {
    if !has_terminal {
        AgentStatus::Idle
    } else if !working {
        AgentStatus::Finished
    } else if awaiting_approval {
        AgentStatus::WaitingForInput
    } else if answering {
        AgentStatus::Working
    } else {
        AgentStatus::Idle
    }
}

fn summarize(view: &AgentView, view_id: EntityId, cx: &App) -> AgentSummary {
    let terminal_entity = view
        .terminal()
        .map(|terminal_view| terminal_view.read(cx).terminal().entity_id());
    let agent = view.agent_id();
    let name = builtin_agent(agent.as_ref())
        .map(|builtin| builtin.display_name.to_string())
        .unwrap_or_else(|| agent.to_string());
    AgentSummary {
        // A tab that has a terminal is listed under the terminal's id, so a
        // device can attach to what it sees without a second lookup.
        id: terminal_entity.map_or_else(|| agent_id_without_terminal(view_id), terminal_id),
        name,
        status: status_for(
            terminal_entity.is_some(),
            view.is_working(cx),
            view.is_answering(),
            view.awaiting_approval(),
        ),
        title: Some(view.tab_label().to_string()),
    }
}

impl AgentMirror {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let this = cx.entity();
        let observe_new = cx.observe_new::<AgentView>(move |_view, window, cx| {
            if window.is_none() {
                // A view with no window is not on anyone's screen.
                return;
            }
            let view = cx.entity();
            this.update(cx, |this, cx| this.track(view, cx));
        });
        Self {
            agents: HashMap::default(),
            active: false,
            _observe_new: observe_new,
        }
    }

    fn track(&mut self, view: gpui::Entity<AgentView>, cx: &mut Context<Self>) {
        let id = view.entity_id();
        if self.agents.contains_key(&id) {
            return;
        }
        // `observe_new` fires while the view is still being built, and reading
        // it then panics. Nothing here needs it until the build is over, so
        // the first read is deferred and everything else goes through events.
        let release = cx.observe_release(&view, move |this, _view, cx| this.untrack(id, cx));
        self.agents.insert(
            id,
            Tracked {
                view: view.downgrade(),
                last: None,
                following: Vec::new(),
                _release: release,
            },
        );
        if self.active {
            let weak = cx.weak_entity();
            let view = view.downgrade();
            cx.defer(move |cx| {
                weak.update(cx, |this, cx| this.follow(id, view, cx))
                    .log_err();
            });
        }
    }

    fn untrack(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(tracked) = self.agents.remove(&id) else {
            return;
        };
        if let Some(last) = tracked.last {
            cx.emit(AgentMirrorEvent::Removed { id: last.id });
        }
    }

    /// Starts following tabs, existing and future. Idempotent.
    pub fn activate(&mut self, cx: &mut Context<Self>) {
        if self.active {
            return;
        }
        self.active = true;
        let known: Vec<(EntityId, WeakEntity<AgentView>)> = self
            .agents
            .iter()
            .map(|(id, tracked)| (*id, tracked.view.clone()))
            .collect();
        let weak = cx.weak_entity();
        cx.defer(move |cx| {
            for (id, view) in known {
                weak.update(cx, |this, cx| this.follow(id, view, cx))
                    .log_err();
            }
        });
    }

    /// Stops following. Tabs stay known; their last summaries are forgotten so
    /// the next activation reports everything afresh.
    pub fn deactivate(&mut self) {
        self.active = false;
        for tracked in self.agents.values_mut() {
            tracked.following.clear();
            tracked.last = None;
        }
    }

    fn follow(&mut self, id: EntityId, view: WeakEntity<AgentView>, cx: &mut Context<Self>) {
        if !self.active {
            return;
        }
        let Some(entity) = view.upgrade() else {
            return;
        };
        let Some(tracked) = self.agents.get_mut(&id) else {
            return;
        };
        if !tracked.following.is_empty() {
            return;
        }
        tracked.following = vec![
            cx.subscribe(
                &entity,
                |this, view, event: &AgentViewEvent, cx| match event {
                    AgentViewEvent::UpdateTab
                    | AgentViewEvent::Activity
                    | AgentViewEvent::Turn(_)
                    | AgentViewEvent::Attention => this.refresh(view.entity_id(), cx),
                    AgentViewEvent::Close => {}
                },
            ),
            // A tab's terminal arrives, and is replaced, through plain
            // notifications rather than an event of its own.
            cx.observe(&entity, |this, view, cx| this.refresh(view.entity_id(), cx)),
        ];
        self.refresh(id, cx);
    }

    fn refresh(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(tracked) = self.agents.get(&id) else {
            return;
        };
        let Some(view) = tracked.view.upgrade() else {
            return;
        };
        let summary = summarize(view.read(cx), id, cx);
        let Some(tracked) = self.agents.get_mut(&id) else {
            return;
        };
        if tracked.last.as_ref() == Some(&summary) {
            return;
        }
        let renamed_from = tracked
            .last
            .replace(summary.clone())
            .filter(|previous| previous.id != summary.id);
        if let Some(previous) = renamed_from {
            // The terminal behind the tab changed, so the old listing is gone.
            cx.emit(AgentMirrorEvent::Removed { id: previous.id });
        }
        cx.emit(AgentMirrorEvent::Changed(summary));
    }

    /// Every tab as it reads right now, in a stable order.
    pub fn summaries(&self, cx: &App) -> Vec<AgentSummary> {
        let mut summaries: Vec<(EntityId, AgentSummary)> = self
            .agents
            .iter()
            .filter_map(|(id, tracked)| {
                let view = tracked.view.upgrade()?;
                Some((*id, summarize(view.read(cx), *id, cx)))
            })
            .collect();
        summaries.sort_by_key(|(id, _)| id.as_u64());
        summaries.into_iter().map(|(_, summary)| summary).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_ui::AgentView;
    use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
    use settings::Settings as _;
    use std::cell::RefCell;
    use std::rc::Rc;
    use workspace::Workspace;

    #[test]
    fn a_status_reads_from_the_four_facts_about_a_tab() {
        use AgentStatus::*;
        assert_eq!(status_for(false, false, false, false), Idle, "not started");
        assert_eq!(
            status_for(true, false, false, false),
            Finished,
            "its CLI ended"
        );
        assert_eq!(
            status_for(true, true, false, false),
            Idle,
            "alive, at its prompt"
        );
        assert_eq!(status_for(true, true, true, false), Working);
        assert_eq!(
            status_for(true, true, true, true),
            WaitingForInput,
            "a dialog beats activity"
        );
        assert_eq!(status_for(true, true, false, true), WaitingForInput);
        assert_eq!(
            status_for(true, false, false, true),
            Finished,
            "a dead CLI waits for nothing"
        );
    }

    fn init_settings(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let store = settings::SettingsStore::test(cx);
            cx.set_global(store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            editor::init(cx);
            project::DisableAiSettings::register(cx);
        });
    }

    async fn workspace(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
        init_settings(cx);
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (multi, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi.read_with(cx, |multi, _| multi.workspace().clone());
        (workspace, cx)
    }

    struct Recorded {
        events: Rc<RefCell<Vec<AgentMirrorEvent>>>,
        _subscription: Subscription,
    }

    fn record(mirror: &Entity<AgentMirror>, cx: &mut VisualTestContext) -> Recorded {
        let events = Rc::new(RefCell::new(Vec::new()));
        let subscription = cx.update(|_, cx| {
            let events = events.clone();
            cx.subscribe(mirror, move |_, event: &AgentMirrorEvent, _| {
                events.borrow_mut().push(event.clone());
            })
        });
        Recorded {
            events,
            _subscription: subscription,
        }
    }

    fn open_tab(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<AgentView> {
        workspace.update_in(cx, |workspace, window, cx| {
            AgentView::open(workspace, project::CLAUDE_CODE_AGENT_ID, None, window, cx);
        });
        cx.run_until_parked();
        workspace
            .read_with(cx, |workspace, cx| {
                workspace.items_of_type::<AgentView>(cx).next()
            })
            .expect("the tab opened")
    }

    #[gpui::test]
    async fn tabs_are_listed_whether_or_not_the_mirror_is_following(cx: &mut TestAppContext) {
        let (workspace, cx) = workspace(cx).await;
        let mirror = cx.update(|_, cx| cx.new(AgentMirror::new));
        let _tab = open_tab(&workspace, cx);

        let summaries = mirror.read_with(cx, |mirror, cx| mirror.summaries(cx));
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].name, "Claude Code");
        assert_eq!(summaries[0].status, AgentStatus::Idle);
        assert!(
            summaries[0].id.starts_with("agent-"),
            "no terminal yet: {}",
            summaries[0].id
        );
    }

    #[gpui::test]
    async fn an_inactive_mirror_follows_nothing(cx: &mut TestAppContext) {
        let (workspace, cx) = workspace(cx).await;
        let mirror = cx.update(|_, cx| cx.new(AgentMirror::new));
        let recorded = record(&mirror, cx);
        let tab = open_tab(&workspace, cx);
        tab.update(cx, |tab, cx| tab.simulate_cli_alive(true, cx));
        cx.run_until_parked();
        assert!(recorded.events.borrow().is_empty());
        mirror.read_with(cx, |mirror, _| {
            assert!(
                mirror
                    .agents
                    .values()
                    .all(|tracked| tracked.following.is_empty())
            );
        });
    }

    #[gpui::test]
    async fn changes_are_reported_once_each_while_active(cx: &mut TestAppContext) {
        let (workspace, cx) = workspace(cx).await;
        let mirror = cx.update(|_, cx| cx.new(AgentMirror::new));
        let tab = open_tab(&workspace, cx);
        let recorded = record(&mirror, cx);
        mirror.update(cx, |mirror, cx| mirror.activate(cx));
        cx.run_until_parked();

        let first = recorded.events.borrow().clone();
        assert_eq!(
            first.len(),
            1,
            "the starting state is reported on activation"
        );
        assert!(
            matches!(&first[0], AgentMirrorEvent::Changed(summary) if summary.status == AgentStatus::Idle)
        );

        tab.update(cx, |tab, cx| tab.simulate_approval(true, cx));
        cx.run_until_parked();
        tab.update(cx, |tab, cx| tab.simulate_approval(true, cx));
        cx.run_until_parked();

        let events = recorded.events.borrow().clone();
        // No terminal in this tab, so a dialog does not change what it reads
        // as; the point is that nothing is said twice for the same answer.
        let changes: Vec<_> = events
            .iter()
            .filter(|event| matches!(event, AgentMirrorEvent::Changed(_)))
            .collect();
        assert_eq!(changes.len(), 1, "{events:?}");
    }

    #[gpui::test]
    async fn a_tab_that_goes_away_is_reported_as_removed(cx: &mut TestAppContext) {
        let (workspace, cx) = workspace(cx).await;
        let mirror = cx.update(|_, cx| cx.new(AgentMirror::new));
        let tab = open_tab(&workspace, cx);
        let recorded = record(&mirror, cx);
        mirror.update(cx, |mirror, cx| mirror.activate(cx));
        cx.run_until_parked();
        let id = match &recorded.events.borrow()[0] {
            AgentMirrorEvent::Changed(summary) => summary.id.clone(),
            other => panic!("unexpected {other:?}"),
        };

        // The release observer is gpui's to fire; what is this module's is
        // what it does when it does.
        mirror.update(cx, |mirror, cx| mirror.untrack(tab.entity_id(), cx));
        assert!(
            recorded
                .events
                .borrow()
                .contains(&AgentMirrorEvent::Removed { id }),
            "{:?}",
            recorded.events.borrow()
        );
        assert!(mirror.read_with(cx, |mirror, cx| mirror.summaries(cx).is_empty()));
    }

    #[gpui::test]
    async fn deactivating_forgets_what_was_said(cx: &mut TestAppContext) {
        let (workspace, cx) = workspace(cx).await;
        let mirror = cx.update(|_, cx| cx.new(AgentMirror::new));
        let _tab = open_tab(&workspace, cx);
        let recorded = record(&mirror, cx);
        mirror.update(cx, |mirror, cx| mirror.activate(cx));
        cx.run_until_parked();
        mirror.update(cx, |mirror, _| mirror.deactivate());
        mirror.update(cx, |mirror, cx| mirror.activate(cx));
        cx.run_until_parked();
        assert_eq!(
            recorded.events.borrow().len(),
            2,
            "a fresh activation says everything again"
        );
    }
}
