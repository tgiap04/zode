use std::sync::Arc;

use agent_settings::AgentSettings;
use gpui::Action as _;
use gpui::{
    AsyncWindowContext, Entity, EventEmitter, FocusHandle, Focusable, Subscription, WeakEntity,
    Window,
};
use project::{AgentId, Project};
use settings::SidebarDockPosition;
use ui::prelude::*;
use workspace::{
    Pane, SidebarSide, SplitDirection, Workspace,
    dock::{DockPosition, Panel, PanelEvent},
    pane,
    pane_group::PaneGroup,
};
use zed_actions::agent::AgentViewMode;

use crate::agent_view::AgentView;
use settings::Settings as _;

/// The dock section the agents live in.
///
/// It keeps a pane group of its own, the way `TerminalPanel` does. That is what
/// lets two agents stand side by side, become tabs when one is dragged onto the
/// other, or split the other way — none of which is written here, because a
/// `PaneGroup` already does all of it.
///
/// The group is emphatically **not** `workspace.panes`. `Workspace::open_path`
/// falls back to the active *workspace* pane, so no file the editor opens can
/// land in here. That is the property the agent moved into a dock to get, and it
/// survives the group living inside.
pub struct AgentPanel {
    active_pane: Entity<Pane>,
    center: PaneGroup,
    project: Entity<Project>,
    workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl AgentPanel {
    pub fn new(workspace: &Workspace, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let project = workspace.project().clone();
        let pane = Self::new_pane(workspace.weak_handle(), project.clone(), window, cx);
        let subscription = cx.subscribe_in(&pane, window, Self::handle_pane_event);

        Self {
            center: PaneGroup::new(pane.clone()),
            active_pane: pane,
            project,
            workspace: workspace.weak_handle(),
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![subscription],
        }
    }

    /// Built with every workspace, holding nothing.
    ///
    /// No agent starts here — the first one starts when someone asks for it, so a
    /// session that never opens an agent never runs one.
    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> anyhow::Result<Entity<Self>> {
        workspace.update_in(&mut cx, |workspace, window, cx| {
            cx.new(|cx| AgentPanel::new(workspace, window, cx))
        })
    }

    /// Brings `agent` up, adding it beside whatever is already open.
    ///
    /// A second agent arrives as its own section rather than a tab: two agents are
    /// two conversations to watch at once, which is the whole reason for opening
    /// the second one. Dragging its tab is how someone says otherwise.
    pub fn show(
        &mut self,
        agent: AgentId,
        mode: AgentViewMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((pane, view)) = self.view_for(&agent, cx) {
            pane.update(cx, |pane, cx| {
                let index = pane.index_for_item(&view);
                if let Some(index) = index {
                    pane.activate_item(index, true, true, window, cx);
                }
            });
            view.update(cx, |view, cx| view.show(mode, window, cx));
            return;
        }

        let view = cx.new(|cx| {
            AgentView::new(
                agent,
                mode,
                self.project.clone(),
                self.workspace.clone(),
                window,
                cx,
            )
        });

        if self.active_pane.read(cx).items_len() > 0 {
            let new_pane = self.add_pane(window, cx);
            self.center
                .split(&self.active_pane, &new_pane, SplitDirection::Right, cx);
            self.active_pane = new_pane;
        }

        self.active_pane.update(cx, |pane, cx| {
            pane.add_item(Box::new(view), true, true, None, window, cx);
        });
        window.focus(&self.active_pane.focus_handle(cx), cx);
        cx.notify();
    }

    fn add_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<Pane> {
        let pane = Self::new_pane(self.workspace.clone(), self.project.clone(), window, cx);
        self._subscriptions
            .push(cx.subscribe_in(&pane, window, Self::handle_pane_event));
        pane
    }

    fn view_for(&self, agent: &AgentId, cx: &App) -> Option<(Entity<Pane>, Entity<AgentView>)> {
        self.center.panes().into_iter().find_map(|pane| {
            let view = pane
                .read(cx)
                .items()
                .filter_map(|item| item.downcast::<AgentView>())
                .find(|view| view.read(cx).is_agent(agent))?;
            Some((pane.clone(), view))
        })
    }

    fn new_pane(
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Pane> {
        let panel = cx.entity().downgrade();
        cx.new(|cx| {
            let mut pane = Pane::new(
                workspace,
                project,
                Default::default(),
                None,
                zed_actions::agent::OpenAgent {
                    agent: project::CLAUDE_CODE_AGENT_ID.to_string(),
                    mode: None,
                }
                .boxed_clone(),
                false,
                window,
                cx,
            );
            pane.set_can_navigate(false, cx);
            pane.display_nav_history_buttons(None);
            pane.set_should_display_tab_bar(|_, _| true);
            pane.set_zoom_out_on_close(false);

            // Only agents belong in here. An editor tab dragged over this dock is
            // refused at the predicate rather than being talked out of it later,
            // which is the same reason the agent left the centre group at all.
            pane.set_can_split(Some(Arc::new(move |_pane, dragged_item, _window, cx| {
                let Some(tab) = dragged_item.downcast_ref::<pane::DraggedTab>() else {
                    return false;
                };
                let Some(item) = tab.pane.read(cx).item_for_index(tab.ix) else {
                    return false;
                };
                if item.downcast::<AgentView>().is_none() {
                    return false;
                }
                panel.upgrade().is_some()
            })));
            pane
        })
    }

    fn handle_pane_event(
        &mut self,
        pane: &Entity<Pane>,
        event: &pane::Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            &pane::Event::Split { direction, mode } => {
                // Only moving an agent across makes sense here. A cloned agent
                // would be a second live process pretending to be the same one,
                // and an empty agent pane has nothing to put in it.
                if !matches!(mode, pane::SplitMode::MovePane) {
                    return;
                }
                let Some(item) = pane.update(cx, |pane, cx| pane.take_active_item(window, cx))
                else {
                    return;
                };
                let new_pane = self.add_pane(window, cx);
                new_pane.update(cx, |new_pane, cx| {
                    new_pane.add_item(item, true, true, None, window, cx);
                });
                self.center.split(pane, &new_pane, direction, cx);
                window.focus(&new_pane.focus_handle(cx), cx);
                self.active_pane = new_pane;
                cx.notify();
            }
            pane::Event::Focus => {
                self.active_pane = pane.clone();
            }
            pane::Event::Remove { .. } => {
                // The last pane stays: an empty dock with no pane has nowhere to
                // put the next agent.
                if self.center.panes().len() > 1
                    && let Ok(_) = self.center.remove(pane, cx)
                {
                    self.active_pane = self.center.first_pane();
                    window.focus(&self.active_pane.focus_handle(cx), cx);
                }
                cx.notify();
            }
            _ => {}
        }
    }
}

impl Focusable for AgentPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.active_pane
            .read(cx)
            .active_item()
            .map(|item| item.item_focus_handle(cx))
            .unwrap_or_else(|| self.focus_handle.clone())
    }
}

impl EventEmitter<PanelEvent> for AgentPanel {}

impl Render for AgentPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let center = self
            .workspace
            .update(cx, |workspace, cx| {
                self.center.render(
                    workspace.zoomed_item(),
                    &workspace::PaneRenderContext {
                        follower_states: &Default::default(),
                        active_call: workspace.active_call(),
                        active_pane: &self.active_pane,
                        app_state: workspace.app_state(),
                        project: workspace.project(),
                        workspace: &workspace.weak_handle(),
                    },
                    window,
                    cx,
                )
            })
            .ok();

        div()
            .size_full()
            .track_focus(&self.focus_handle)
            .children(center)
    }
}

impl Panel for AgentPanel {
    fn persistent_name() -> &'static str {
        "Agent Panel"
    }

    fn panel_key() -> &'static str {
        "AgentPanel"
    }

    fn position(&self, _window: &Window, cx: &App) -> DockPosition {
        match AgentSettings::get_global(cx).sidebar_side() {
            SidebarSide::Left => DockPosition::Left,
            SidebarSide::Right => DockPosition::Right,
        }
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    fn set_position(
        &mut self,
        position: DockPosition,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let side = match position {
            DockPosition::Left => SidebarDockPosition::Left,
            _ => SidebarDockPosition::Right,
        };
        let fs = self.project.read(cx).fs().clone();
        settings::update_settings_file(fs, cx, move |settings, _| {
            settings.agent.get_or_insert_default().sidebar_side = Some(side)
        });
    }

    fn default_size(&self, _window: &Window, _cx: &App) -> Pixels {
        px(480.)
    }

    /// No icon, and therefore no button.
    ///
    /// Both renderers skip a panel whose icon is `None` — the rail at
    /// `rail_panels.rs:78` and the status bar at `dock.rs:1272`. The rail already
    /// draws one button per agent with that agent's own mark, and a generic button
    /// for "the agent panel" would stand for both of them at once.
    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        None
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        None
    }

    fn toggle_action(&self) -> Box<dyn gpui::Action> {
        Box::new(zed_actions::agent::OpenAgent {
            agent: project::CLAUDE_CODE_AGENT_ID.to_string(),
            mode: None,
        })
    }

    fn pane(&self) -> Option<Entity<Pane>> {
        Some(self.active_pane.clone())
    }

    /// Closed until asked for: opening it starts a process.
    fn starts_open(&self, _window: &Window, _cx: &App) -> bool {
        false
    }

    fn activation_priority(&self) -> u32 {
        9
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, UpdateGlobal as _};
    use project::FakeFs;
    use serde_json::json;
    use settings::SettingsStore;
    use util::path;
    use workspace::MultiWorkspace;

    async fn panel(cx: &mut TestAppContext) -> (Entity<AgentPanel>, &mut gpui::VisualTestContext) {
        cx.update(|cx| {
            let store = SettingsStore::test(cx);
            cx.set_global(store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
        });

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/test"), json!({})).await;
        let project = Project::test(fs, [path!("/test").as_ref()], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |multi, _| multi.workspace().clone());

        let panel = workspace.update_in(cx, |workspace, window, cx| {
            cx.new(|cx| AgentPanel::new(workspace, window, cx))
        });
        (panel, cx)
    }

    /// The panel docks where its setting says, and dragging it across writes that
    /// setting back — so the two directions have to agree.
    #[gpui::test]
    async fn the_panel_docks_on_the_side_the_setting_names(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;

        for (side, expected) in [
            (SidebarDockPosition::Left, DockPosition::Left),
            (SidebarDockPosition::Right, DockPosition::Right),
        ] {
            cx.update(|_window, cx| {
                SettingsStore::update_global(cx, |store, _cx| {
                    let mut agent_settings = store.get::<AgentSettings>(None).clone();
                    agent_settings.sidebar_side = side;
                    store.override_global(agent_settings);
                });
            });
            cx.run_until_parked();

            assert_eq!(
                cx.update(|window, cx| panel.read(cx).position(window, cx)),
                expected,
                "the panel must dock where `sidebar_side` says"
            );
        }
    }

    /// The dock's pane group must never be one of the workspace's own panes, or
    /// `open_path` — which falls back to the active workspace pane — would put an
    /// editor tab inside an agent. That property is the whole reason the agent
    /// moved out of the centre group.
    #[gpui::test]
    async fn the_agent_panes_are_not_workspace_panes(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;

        let (agent_panes, workspace_panes) = cx.update(|_window, cx| {
            let agent = panel
                .read(cx)
                .center
                .panes()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>();
            let workspace = panel
                .read(cx)
                .workspace
                .read_with(cx, |workspace, _| workspace.panes().to_vec())
                .unwrap_or_default();
            (agent, workspace)
        });

        assert!(!agent_panes.is_empty());
        for pane in &agent_panes {
            assert!(
                !workspace_panes.contains(pane),
                "an agent pane in `workspace.panes` is a pane the editor can open files into"
            );
        }
    }
}
