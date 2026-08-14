use std::collections::HashMap;
use std::sync::Arc;

use gpui::Action as _;
use gpui::{
    Anchor, AsyncWindowContext, Entity, EntityId, EventEmitter, FocusHandle, Focusable,
    Subscription, WeakEntity, Window,
};
use project::{AgentId, Project};
use ui::prelude::*;
use ui::{ContextMenu, PopoverMenu, Tooltip};
use workspace::{
    AppState, Pane, SidebarSide, SplitDirection, Workspace, WorkspaceSettings,
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
    /// Held rather than read back off the workspace, so `render` needs nothing
    /// from it. See the comment there.
    app_state: Arc<AppState>,
    focus_handle: FocusHandle,
    /// The pane asking for the editor's space, if one is. Held here rather than
    /// on the workspace so zooming an agent cannot take the tool dock sharing
    /// this column's `DockPosition` down with it.
    ///
    /// Cleared wherever a pane stops being drawable — not only on `ZoomOut`.
    /// A stale handle here reads as "still zoomed" to `fills_the_center`, and
    /// the workspace stands the editor down for a pane nobody can see.
    zoomed_pane: Option<WeakEntity<Pane>>,
    /// Keyed by pane, so a pane's subscription leaves with the pane.
    ///
    /// A `Vec` grew one entry per split for the life of the window: `add_pane`
    /// pushed, and the `Remove` arm below dropped the pane from the group
    /// without ever dropping the subscription that watched it.
    _subscriptions: HashMap<EntityId, Subscription>,
}

/// How many agents may stand side by side in the column before the next one
/// arrives as a tab instead.
///
/// The column is 480px by default and a conversation needs room to be read;
/// splitting without limit turns the fourth agent into a sliver too narrow to
/// use, and the one after that into a sliver of a sliver. A tab is the graceful
/// end of that sequence, and dragging it back out is how someone overrules it.
const MAX_SIDE_BY_SIDE_AGENTS: usize = 3;

impl AgentPanel {
    pub fn new(workspace: &Workspace, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let project = workspace.project().clone();
        let pane = Self::new_pane(workspace.weak_handle(), project.clone(), window, cx);
        let subscription = cx.subscribe_in(&pane, window, Self::handle_pane_event);

        Self {
            center: PaneGroup::new(pane.clone()),
            _subscriptions: HashMap::from([(pane.entity_id(), subscription)]),
            active_pane: pane,
            project,
            workspace: workspace.weak_handle(),
            app_state: workspace.app_state().clone(),
            focus_handle: cx.focus_handle(),
            zoomed_pane: None,
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

        self.show_new(agent, mode, window, cx);
    }

    /// Starts another session of `agent` beside the ones already running.
    ///
    /// Same shape as `show`, minus the step that hands back the existing view.
    /// Two sessions of one agent are two conversations, each with its own
    /// process — which is why this is reached only from the deliberate `+`
    /// menu, never from a rail click that someone may have meant as "come back
    /// to what I had".
    pub fn show_new(
        &mut self,
        agent: AgentId,
        mode: AgentViewMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

        // Past the cap the agent joins the active pane as a tab rather than
        // taking a share of a column that has none left to give.
        if self.active_pane.read(cx).items_len() > 0
            && self.center.panes().len() < MAX_SIDE_BY_SIDE_AGENTS
        {
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
        self._subscriptions.insert(
            pane.entity_id(),
            cx.subscribe_in(&pane, window, Self::handle_pane_event),
        );
        pane
    }

    /// Lets go of a pane that is no longer drawn.
    ///
    /// Both halves matter. The subscription is what made `_subscriptions` grow
    /// for the life of the window, and the zoom handle is what left the
    /// workspace hiding the editor for a pane that no longer exists.
    fn forget_pane(&mut self, pane: &Entity<Pane>) {
        self._subscriptions.remove(&pane.entity_id());
        if self
            .zoomed_pane
            .as_ref()
            .is_some_and(|zoomed| zoomed.entity_id() == pane.entity_id())
        {
            self.zoomed_pane = None;
        }
    }

    fn is_empty(&self, cx: &App) -> bool {
        self.center
            .panes()
            .iter()
            .all(|pane| pane.read(cx).items_len() == 0)
    }

    /// Puts the dock away the moment the panel holds nothing.
    ///
    /// A dock remembers being open across restarts, but what stands inside this
    /// one is not serialized — so a session that had Claude up comes back to a
    /// section with nothing in it. Empty, it is worse than absent: it takes width
    /// off the editor, draws a border around a void, and offers nothing to click,
    /// since this panel deliberately has no icon of its own.
    ///
    /// The same holds after the last agent is closed, which is why this is a rule
    /// about the panel's contents rather than a patch on the restore path.
    fn close_if_empty(&mut self, cx: &mut Context<Self>) {
        if !self.is_empty(cx) {
            return;
        }
        // Zoom does not outlive the agents it was zooming. Left standing, the
        // next agent opened comes straight back filling the centre, and the
        // editor disappears for someone who never asked for it — the pane is
        // still alive here, so `fills_the_center` cannot tell on its own that
        // there is nothing left to fill it with.
        if let Some(pane) = self.zoomed_pane.take().and_then(|pane| pane.upgrade()) {
            pane.update(cx, |pane, cx| pane.set_zoomed(false, cx));
        }
        cx.emit(PanelEvent::Close);
    }

    /// Whether `agent` currently has an open pane in this panel.
    ///
    /// The rail's icon uses this to show itself as active — a running agent
    /// stays lit whether or not its pane happens to have focus, or the dock
    /// happens to be the one on screen right now. Two agents can be open at
    /// once, so this is a per-agent signal rather than a single "the active
    /// one" index.
    pub fn has_agent(&self, agent: &AgentId, cx: &App) -> bool {
        self.view_for(agent, cx).is_some()
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
                Some(Arc::new(|dragged_item, _window, cx: &mut App| {
                    // Runs from a `MouseUpEvent` handler, never mid-render, so
                    // reading the source pane here is safe — unlike the split
                    // predicate below, which the pane calls from inside its own
                    // update and which has to read around the live borrow.
                    dragged_item
                        .downcast_ref::<pane::DraggedTab>()
                        .and_then(|tab| tab.pane.read(cx).item_for_index(tab.ix))
                        .is_some_and(|item| item.downcast::<AgentView>().is_some())
                })),
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
            Self::apply_tab_bar_buttons(&mut pane, cx);

            // Only agents belong in here, and a tab can arrive by two doors: onto
            // the tab bar, which `can_drop` above guards, and onto an edge to
            // split, which the predicate below guards. Guarding one and writing
            // the comment as though both were covered is how an editor tab could
            // be dropped in here for as long as it could.
            //
            // Dropping a tab on this pane's edge must split *this* group. Left to
            // itself `handle_tab_drop` calls `Workspace::split_pane`, which splits
            // the centre group — where these panes do not exist, so the drag lands
            // nowhere at all. That is why dragging Codex under Claude did nothing.
            let panel_for_split = panel.clone();
            pane.set_split_for_drop(Some(Arc::new(move |pane, direction, window, cx| {
                let panel = panel_for_split.upgrade()?;
                let pane = pane.clone();
                Some(panel.update(cx, |panel, cx| {
                    let new_pane = panel.add_pane(window, cx);
                    panel.center.split(&pane, &new_pane, direction, cx);
                    panel.active_pane = new_pane.clone();
                    new_pane
                }))
            })));

            pane.set_can_split(Some(Arc::new(move |pane, dragged_item, _window, cx| {
                let Some(tab) = dragged_item.downcast_ref::<pane::DraggedTab>() else {
                    return false;
                };
                // Read through the borrow already in hand when the tab comes from
                // this very pane. Reaching for `tab.pane.read(cx)` there reads a
                // pane that is mid-update — which aborts the process rather than
                // failing the drag, and dragging a tab within one pane is the
                // ordinary case, not the corner one.
                let item = if tab.pane == cx.entity() {
                    pane.item_for_index(tab.ix)
                } else {
                    tab.pane.read(cx).item_for_index(tab.ix)
                };
                let Some(item) = item else {
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

    /// Replaces the editor pane's "New…" menu with one made of agents.
    ///
    /// The default offers New File, Open File, Search Project, Search Symbols
    /// and two kinds of terminal — every one of which opens something into the
    /// *centre* group, not into this column, so the menu named actions that
    /// could not land where it was drawn. What belongs on a `+` here is another
    /// agent, and this is the only place a second session of an agent already
    /// running can be started.
    ///
    /// Built from `BUILTIN_AGENTS` rather than naming Claude and Codex, so a
    /// third agent appears here the day it is added.
    fn apply_tab_bar_buttons(pane: &mut Pane, cx: &mut Context<Pane>) {
        pane.set_render_tab_bar_buttons(cx, move |pane, window, cx| {
            if !pane.has_focus(window, cx) && !pane.context_menu_focused(window, cx) {
                return (None, None);
            }
            let can_split = pane.items_len() > 1;
            let right_children = h_flex()
                .gap(DynamicSpacing::Base04.rems(cx))
                .child(
                    PopoverMenu::new("agent-pane-tab-bar-new")
                        .trigger_with_tooltip(
                            IconButton::new("plus", IconName::Plus).icon_size(IconSize::Small),
                            Tooltip::text("New Agent"),
                        )
                        .anchor(Anchor::TopRight)
                        .with_handle(pane.new_item_context_menu_handle.clone())
                        .menu(move |window, cx| {
                            Some(ContextMenu::build(window, cx, |menu, _, _| {
                                project::BUILTIN_AGENTS.iter().fold(menu, |menu, agent| {
                                    menu.action(
                                        format!("New {}", agent.display_name),
                                        zed_actions::agent::NewAgent {
                                            agent: agent.id.to_string(),
                                            mode: None,
                                        }
                                        .boxed_clone(),
                                    )
                                })
                            }))
                        }),
                )
                .child(
                    PopoverMenu::new("agent-pane-tab-bar-split")
                        .trigger_with_tooltip(
                            IconButton::new("split", IconName::Split)
                                .icon_size(IconSize::Small)
                                .disabled(!can_split),
                            Tooltip::text("Split Pane"),
                        )
                        .anchor(Anchor::TopRight)
                        .with_handle(pane.split_item_context_menu_handle.clone())
                        .menu(move |window, cx| {
                            // `MovePane` only: a cloned agent would be a second
                            // live process pretending to be the same session,
                            // which is what `handle_pane_event` already refuses.
                            let mode = pane::SplitMode::MovePane;
                            ContextMenu::build(window, cx, |menu, _, _| {
                                menu.action(
                                    "Split Right",
                                    workspace::SplitRight { mode }.boxed_clone(),
                                )
                                .action("Split Left", workspace::SplitLeft { mode }.boxed_clone())
                                .action("Split Up", workspace::SplitUp { mode }.boxed_clone())
                                .action("Split Down", workspace::SplitDown { mode }.boxed_clone())
                            })
                            .into()
                        }),
                )
                .child({
                    let zoomed = pane.is_zoomed();
                    IconButton::new("toggle_zoom", IconName::Maximize)
                        .icon_size(IconSize::Small)
                        .toggle_state(zoomed)
                        .selected_icon(IconName::Minimize)
                        .on_click(cx.listener(|pane, _, window, cx| {
                            pane.toggle_zoom(&workspace::ToggleZoom, window, cx);
                        }))
                        .tooltip(move |_window, cx| {
                            Tooltip::for_action(
                                if zoomed { "Zoom Out" } else { "Zoom In" },
                                &workspace::ToggleZoom,
                                cx,
                            )
                        })
                })
                .into_any_element()
                .into();
            (None, right_children)
        });
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
                    self.forget_pane(pane);
                    self.active_pane = self.center.first_pane();
                    window.focus(&self.active_pane.focus_handle(cx), cx);
                }
                self.close_if_empty(cx);
                cx.notify();
            }
            // Handled here rather than let through to the workspace: the dock's
            // zoom is keyed by `DockPosition`, which this column shares with the
            // tool dock beside it. Falling through to `_ => {}` is why the
            // maximise button only ever lit up.
            pane::Event::ZoomIn => {
                self.zoomed_pane = Some(pane.downgrade());
                pane.update(cx, |pane, cx| pane.set_zoomed(true, cx));
                cx.notify();
            }
            pane::Event::ZoomOut => {
                self.zoomed_pane = None;
                pane.update(cx, |pane, cx| pane.set_zoomed(false, cx));
                cx.notify();
            }
            _ => {}
        }
    }
}

impl Focusable for AgentPanel {
    /// Straight to the pane, the way `TerminalPanel` does it.
    ///
    /// Walking from here into the active item costs another read of the pane on a
    /// path the workspace calls constantly, including from inside pane updates —
    /// and the pane already forwards focus to its active item.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.active_pane.focus_handle(cx)
    }
}

impl EventEmitter<PanelEvent> for AgentPanel {}

impl Render for AgentPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A zoomed pane is drawn *in place of* the group, never handed to it.
        // `PaneGroup::render` renders the pane it is given as an empty div, on
        // the understanding that the workspace's absolute overlay draws it
        // instead (`workspace.rs`, `.children(self.zoomed…)`). This panel has no
        // such overlay, so handing it over left the column drawing a blank
        // surface at full width — measured: dock 1920px, surface 1912x1065,
        // agent view absent. Here zoom means "only this one, in place", so it is
        // the group that steps aside.
        if let Some(pane) = self.zoomed_pane.as_ref().and_then(|pane| pane.upgrade()) {
            return div()
                .id("agent-panel-root")
                .debug_selector(|| "agent-panel-root".into())
                .size_full()
                .track_focus(&self.focus_handle)
                .child(
                    div()
                        .id("agent-panel-surface")
                        .debug_selector(|| "agent-panel-surface".into())
                        .size_full()
                        .overflow_hidden()
                        .bg(cx.theme().colors().editor_background)
                        .child(pane),
                );
        }

        // Nothing is read back off the workspace here. This runs from element
        // layout, after `Workspace::render` has returned, so leasing it did not
        // abort — but this crate has paid for that shape twice (`ccd151f`), and
        // the `.ok()` that used to trail the call guarded nothing: a `WeakEntity`
        // update returns `Err` only for a dropped entity, while a live lease
        // aborts the process.
        //
        // `active_call` is `None` rather than the workspace's: it is reached
        // only after `follower_states` yields a match (`pane_group.rs`,
        // `PaneLeaderDecorator::decorate`), and an agent pane has no followers
        // to match. `app_state` is held on the panel instead.
        let center = self.center.render(
            None,
            &workspace::PaneRenderContext {
                follower_states: &Default::default(),
                active_call: None,
                active_pane: &self.active_pane,
                app_state: &self.app_state,
                project: &self.project,
                workspace: &self.workspace,
            },
            window,
            cx,
        );

        // The gap and the corner belong to `Dock::render`, which draws them for
        // every panel it holds — this one used to carry its own copy of that
        // recipe and was the reason the dock had to make an exception for it.
        // What is left here is the fill: agents are read like a buffer, not like
        // a tool panel, so the surface takes the editor's background.
        div()
            .id("agent-panel-root")
            .debug_selector(|| "agent-panel-root".into())
            .size_full()
            .track_focus(&self.focus_handle)
            .child(
                div()
                    .id("agent-panel-surface")
                    .debug_selector(|| "agent-panel-surface".into())
                    .size_full()
                    .overflow_hidden()
                    .bg(cx.theme().colors().editor_background)
                    .child(center),
            )
    }
}

impl Panel for AgentPanel {
    fn persistent_name() -> &'static str {
        "Agent Panel"
    }

    /// Zoomed, this column asks for the editor's space too.
    ///
    /// `Workspace::render_centre_with_agent` reads it and stands the centre
    /// down. Not routed through `PanelEvent::ZoomIn`, which would put the dock
    /// machinery in charge: that keys on `DockPosition`, which this column
    /// shares with the tool dock beside it.
    /// Upgraded rather than merely checked for `Some`: a pane can be removed
    /// while the handle to it stays, and answering yes for a pane that no longer
    /// exists stands the editor down in favour of nothing at all.
    fn fills_the_center(&self, _window: &Window, _cx: &App) -> bool {
        self.zoomed_pane
            .as_ref()
            .is_some_and(|pane| pane.upgrade().is_some())
    }

    fn panel_key() -> &'static str {
        "AgentPanel"
    }

    /// The side of the editor the agent column stands on — the rail's side.
    ///
    /// Not `agent.sidebar_side`: the agent belongs beside the buttons that open
    /// it, and a setting of its own only let the two drift to opposite edges.
    /// Asked of the workspace so this cannot disagree with the column it is
    /// drawn in.
    /// Reads the setting rather than asking the workspace, which computes the
    /// same answer: `Workspace::add_panel` calls this from inside its own
    /// update, so reaching back through the workspace handle here aborts the
    /// process — the trap `ccd151f` already paid for once.
    fn position(&self, _window: &Window, cx: &App) -> DockPosition {
        match WorkspaceSettings::get_global(cx).multi_project.sidebar_side {
            SidebarSide::Left => DockPosition::Left,
            SidebarSide::Right => DockPosition::Right,
        }
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    /// Ignored: the column follows the rail, so there is nothing to set here.
    ///
    /// Dragging it across would move the agent away from the buttons that open
    /// it. Moving the rail moves both.
    fn set_position(
        &mut self,
        _position: DockPosition,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
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

    /// Claims the workspace's agent column rather than a place in a side dock.
    ///
    /// A dock is one column, so sharing one with the git or project panel means
    /// either taking turns with it or being stacked into its width. The agent
    /// is a working surface, so it stands beside the editor instead.
    fn is_agent_panel(&self) -> bool {
        true
    }

    /// Closed until asked for: opening it starts a process.
    fn starts_open(&self, _window: &Window, _cx: &App) -> bool {
        false
    }

    /// Shown holding nothing, it closes straight back — see `close_if_empty`.
    ///
    /// This is the one hook every path to a visible panel runs through: restoring
    /// the dock from the last session, `focus_panel`, and the generic dock toggle
    /// all reach `set_active(true)`.
    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            self.close_if_empty(cx);
        }
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
    use workspace::{MultiWorkspace, pane::SaveIntent, pane_group::SURFACE_MARGIN};

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

    /// Whether the agent's column is taking a section of the window.
    ///
    /// Always the agent column, never a side dock: that is where `add_panel`
    /// routes an agent, so looking it up by `Panel::position` would find a dock
    /// the agent is not in.
    fn dock_is_open(panel: &Entity<AgentPanel>, cx: &mut gpui::VisualTestContext) -> bool {
        cx.update(|_window, cx| {
            panel
                .read(cx)
                .workspace
                .read_with(cx, |workspace, cx| {
                    workspace.agent_dock().read(cx).is_open()
                })
                .unwrap_or(false)
        })
    }

    /// The dock remembers being open from the last session, but what stood inside
    /// it does not — so the panel comes back holding nothing, and the window gets
    /// a blank strip where an agent used to be.
    #[gpui::test]
    async fn a_dock_shown_holding_nothing_closes_itself(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;

        panel
            .read_with(cx, |panel, _| panel.workspace.clone())
            .update_in(cx, |workspace, window, cx| {
                workspace.add_panel(panel.clone(), window, cx);
                workspace.focus_panel::<AgentPanel>(window, cx);
            })
            .unwrap();
        cx.run_until_parked();

        assert!(
            !dock_is_open(&panel, cx),
            "a panel with no agent in it must not take a section of the window"
        );
    }

    /// The agent column records what it was showing, so a restart comes back
    /// to it rather than to an empty column.
    ///
    /// This used to stack the agent with a test panel to prove a two-panel
    /// stack round-trips. It cannot any more, and that is the point of the
    /// change: the agent has a column to itself, and `add_panel` will not route
    /// anything else into it. A stack of two distinct panel types is now only
    /// reachable in the three ordinary docks — see `workspace`'s own
    /// `a_dock_stack_round_trips_and_tolerates_having_none` for the mechanism,
    /// and the gap noted in phase 02 of the plan for what is left uncovered.
    #[gpui::test]
    async fn the_agent_column_records_what_it_was_showing(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.add_panel(panel.clone(), window, cx);
                workspace.focus_panel::<AgentPanel>(window, cx);
            })
            .unwrap();
        panel.update_in(cx, |panel, window, cx| {
            panel.show(
                AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
                AgentViewMode::Terminal,
                window,
                cx,
            );
        });
        cx.run_until_parked();

        let saved = workspace
            .read_with(cx, |workspace, cx| {
                workspace.agent_dock().read(cx).stack_state()
            })
            .unwrap();
        assert_eq!(
            saved.showing,
            vec![AgentPanel::persistent_name().to_string()],
            "the agent column should record the agent panel standing in it"
        );

        // Emptied and handed the record back, the shape a fresh process is in.
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.agent_dock().update(cx, |dock, cx| {
                    dock.hide_panel_by_id(panel.entity_id(), window, cx);
                    assert_eq!(dock.visible_panels().count(), 0);
                    assert!(dock.apply_stack_state(&saved, window, cx));
                    // Opened afterwards, the order `restore_state` uses: the
                    // record says which panels, the serialized dock says
                    // whether the column was up at all.
                    dock.set_open(true, window, cx);
                    assert_eq!(
                        dock.visible_panels().count(),
                        1,
                        "the agent should come back to its column"
                    );
                });
            })
            .unwrap();
    }

    /// Opening an agent must not put away a panel docked on the other side.
    ///
    /// A dock shows one panel at a time, so the agent and a tool panel can only
    /// coexist by standing on opposite edges — this pins that they then really
    /// do, rather than one closing the other through some shared path.
    #[gpui::test]
    async fn an_agent_and_a_panel_on_the_other_side_stay_open_together(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        // The rail's side, which is what puts the agent column on the right —
        // `agent.sidebar_side` no longer decides this.
        cx.update(|_window, cx| {
            SettingsStore::update_global(cx, |settings, cx| {
                settings.update_user_settings(cx, |settings| {
                    settings
                        .workspace
                        .multi_project
                        .get_or_insert_default()
                        .sidebar_side = Some(SidebarSide::Right);
                });
            });
        });
        cx.run_until_parked();

        let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());
        let other = cx.new(|cx| workspace::dock::test::TestPanel::new(DockPosition::Left, 1, cx));
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.add_panel(panel.clone(), window, cx);
                workspace.add_panel(other.clone(), window, cx);
            })
            .unwrap();

        panel.update_in(cx, |panel, window, cx| {
            panel.show(
                AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
                AgentViewMode::Terminal,
                window,
                cx,
            );
        });
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.focus_panel::<AgentPanel>(window, cx);
                workspace.focus_panel::<workspace::dock::test::TestPanel>(window, cx);
            })
            .unwrap();
        cx.run_until_parked();

        let (panel_dock, agent_column) = workspace
            .read_with(cx, |workspace, cx| {
                (
                    workspace.left_dock().read(cx).is_open(),
                    workspace.agent_dock().read(cx).is_open(),
                )
            })
            .unwrap();
        assert!(panel_dock, "the panel docked left should be open");
        assert!(
            agent_column,
            "and the agent should still be up in its own column beside it"
        );
    }

    /// The dock draws its surface, but the agent inside it draws nothing —
    /// the reported symptom. Drawing is the only way to catch it: every
    /// state assertion (`has_agent`, `items_len`, dock open) passes while
    /// the panel paints an empty box, so this measures real bounds.
    #[gpui::test]
    async fn a_shown_agent_actually_occupies_the_panel(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.add_panel(panel.clone(), window, cx)
            })
            .unwrap();

        panel.update_in(cx, |panel, window, cx| {
            panel.show(
                AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
                AgentViewMode::Terminal,
                window,
                cx,
            );
        });
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.focus_panel::<AgentPanel>(window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        let root = cx
            .debug_bounds("agent-panel-root")
            .expect("the panel root should be drawn");
        assert!(
            root.size.height > px(0.),
            "the panel root itself has no height, so nothing inside it can: {root:?}"
        );

        let surface = cx
            .debug_bounds("agent-panel-surface")
            .expect("the panel surface should be drawn");
        assert!(
            surface.size.width > px(0.) && surface.size.height > px(0.),
            "the panel surface drew with no area: {surface:?} (root was {root:?})"
        );

        let agent = cx
            .debug_bounds("agent-view")
            .expect("an agent is open in the panel, so its view must be drawn");
        assert!(
            agent.size.width > px(0.) && agent.size.height > px(0.),
            "the agent drew inside the surface but with no area: {agent:?}"
        );

        // Filling the surface, not merely present in it: the failure this
        // guards against left the surface at full width and zero height, so
        // a "greater than zero" bound alone would pass on a sliver.
        assert_eq!(
            agent.size.width, surface.size.width,
            "the agent should span the surface it sits in"
        );
        let tab_bar = surface.size.height - agent.size.height;
        assert!(
            tab_bar > px(0.) && tab_bar < surface.size.height / 4.,
            "the only height the agent should give up is the pane's tab bar — \
             the row carrying its name, which is itself part of showing an agent. \
             Got {tab_bar:?} of {surface:?}"
        );
        // Exactly, not "apart from an inset": the gap and corner moved up to
        // `Dock::render`, so what this panel is handed is already inset and
        // everything it draws should fill it.
        assert_eq!(
            surface.size, root.size,
            "the surface should fill the panel it was handed, \
             surface {surface:?} in root {root:?}"
        );

        // And that the inset is real. Without this the assertion above would
        // hold just as well if the dock had stopped insetting anything at all —
        // panel filling panel proves nothing about the gap the user asked for.
        let column = cx
            .debug_bounds("dock-panel")
            .expect("the column holding the panel should be drawn");
        assert!(
            column.size.width - root.size.width >= SURFACE_MARGIN * 2.,
            "the dock should hold its panel inset on both sides, leaving the gap \
             that separates this column from the one beside it — \
             column {column:?} around root {root:?}"
        );
    }

    /// The rail's active-icon effect reads this directly, so it has to track
    /// an agent's pane through the whole lifecycle the rail cares about: not
    /// open, open, and closed again — not just the middle state.
    #[gpui::test]
    async fn has_agent_tracks_a_pane_through_its_lifecycle(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let claude = AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string());
        let codex = AgentId::new(project::CODEX_AGENT_ID.to_string());

        assert!(
            !panel.read_with(cx, |panel, cx| panel.has_agent(&claude, cx)),
            "nobody has opened either agent yet"
        );

        panel.update_in(cx, |panel, window, cx| {
            panel.show(claude.clone(), AgentViewMode::Terminal, window, cx);
        });
        cx.run_until_parked();

        assert!(
            panel.read_with(cx, |panel, cx| panel.has_agent(&claude, cx)),
            "claude has a pane now, so the rail's claude icon must light up"
        );
        assert!(
            !panel.read_with(cx, |panel, cx| panel.has_agent(&codex, cx)),
            "opening claude must not light up codex's icon too"
        );

        let (pane, item) = panel.read_with(cx, |panel, cx| {
            let pane = panel.active_pane.clone();
            let item = pane
                .read(cx)
                .items()
                .next()
                .expect("claude's item")
                .item_id();
            (pane, item)
        });
        pane.update_in(cx, |pane, window, cx| {
            pane.close_item_by_id(item, SaveIntent::Skip, window, cx)
        })
        .await
        .expect("closing claude");
        cx.run_until_parked();

        assert!(
            !panel.read_with(cx, |panel, cx| panel.has_agent(&claude, cx)),
            "claude's pane is gone, so the rail must stop showing it as active"
        );
    }

    /// The rule above puts an ordering under the rail click, and backwards it
    /// costs the whole feature: open the dock first and it shuts on the empty
    /// panel, then the agent arrives into a section nobody can see.
    #[gpui::test]
    async fn a_rail_click_opens_the_dock_onto_its_agent(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.add_panel(panel.clone(), window, cx)
            })
            .unwrap();

        workspace
            .update_in(cx, |workspace, window, cx| {
                AgentView::open(
                    workspace,
                    project::CLAUDE_CODE_AGENT_ID,
                    Some(AgentViewMode::Terminal),
                    window,
                    cx,
                )
            })
            .unwrap();
        cx.run_until_parked();

        assert_eq!(
            panel.read_with(cx, |panel, cx| panel.active_pane.read(cx).items_len()),
            1,
            "the click should have put its agent in the panel"
        );
        assert!(
            dock_is_open(&panel, cx),
            "and the dock has to be open, or the agent is running where nobody can see it"
        );
    }

    /// Two sessions of one agent stand side by side; a rail click still returns
    /// to the first.
    ///
    /// These pull in opposite directions and both matter: `show` deduplicates so
    /// a stray rail click never spends another CLI process, and `show_new` — the
    /// `+` menu's path — must not inherit that, or a second Claude Code is
    /// impossible to ask for.
    #[gpui::test]
    async fn a_second_session_of_one_agent_stands_beside_the_first(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.add_panel(panel.clone(), window, cx)
            })
            .unwrap();

        let claude = AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string());
        panel.update_in(cx, |panel, window, cx| {
            panel.show(claude.clone(), AgentViewMode::Terminal, window, cx);
        });
        cx.run_until_parked();

        let first_pane = panel.read_with(cx, |panel, _| panel.active_pane.clone());

        // The gesture the rail makes: come back to what is already running.
        panel.update_in(cx, |panel, window, cx| {
            panel.show(claude.clone(), AgentViewMode::Terminal, window, cx);
        });
        cx.run_until_parked();
        assert_eq!(
            panel.read_with(cx, |panel, cx| agent_view_count(panel, cx)),
            1,
            "a rail click on a running agent must return to it, not start a second one"
        );

        // The gesture the `+` menu makes: another one, deliberately.
        panel.update_in(cx, |panel, window, cx| {
            panel.show_new(claude.clone(), AgentViewMode::Terminal, window, cx);
        });
        cx.run_until_parked();
        assert_eq!(
            panel.read_with(cx, |panel, cx| agent_view_count(panel, cx)),
            2,
            "the + menu must be able to start a second session of an agent already open"
        );
        assert_ne!(
            panel.read_with(cx, |panel, _| panel.active_pane.clone()),
            first_pane,
            "and it should arrive as its own section, the way a second agent does"
        );
    }

    /// The column takes only agents.
    ///
    /// A tab reaches a pane by two doors — the tab bar (`can_drop_predicate`)
    /// and an edge to split (`can_split_predicate`) — and only the second was
    /// guarded, so an editor tab could be dropped straight into the agent
    /// column. This asserts the first door specifically.
    #[gpui::test]
    async fn an_editor_tab_cannot_be_dropped_into_the_agent_column(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.add_panel(panel.clone(), window, cx)
            })
            .unwrap();

        panel.update_in(cx, |panel, window, cx| {
            panel.show(
                AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
                AgentViewMode::Terminal,
                window,
                cx,
            );
        });
        cx.run_until_parked();

        let agent_pane = panel.read_with(cx, |panel, _| panel.active_pane.clone());
        let editor_pane = workspace
            .read_with(cx, |workspace, _| workspace.active_pane().clone())
            .unwrap();
        workspace
            .update_in(cx, |workspace, window, cx| {
                let item = cx.new(|cx| workspace::item::test::TestItem::new(cx));
                workspace.add_item_to_active_pane(Box::new(item), None, true, window, cx);
            })
            .unwrap();
        cx.run_until_parked();

        let editor_tab = pane::DraggedTab {
            pane: editor_pane.clone(),
            item: editor_pane
                .read_with(cx, |pane, _| {
                    pane.item_for_index(0).map(|item| item.boxed_clone())
                })
                .expect("the item just added"),
            ix: 0,
            detail: 0,
            is_active: true,
        };
        let agent_tab = pane::DraggedTab {
            pane: agent_pane.clone(),
            item: agent_pane
                .read_with(cx, |pane, _| {
                    pane.item_for_index(0).map(|item| item.boxed_clone())
                })
                .expect("the agent just shown"),
            ix: 0,
            detail: 0,
            is_active: true,
        };

        let accepts = |tab: &pane::DraggedTab, cx: &mut gpui::VisualTestContext| {
            let predicate = agent_pane
                .read_with(cx, |pane, _| pane.can_drop_predicate())
                .expect("the agent pane must carry a drop predicate at all");
            cx.update(|window, cx| predicate(tab as &dyn std::any::Any, window, cx))
        };

        assert!(
            !accepts(&editor_tab, cx),
            "an editor tab dropped on the agent column's tab bar must be refused"
        );
        assert!(
            accepts(&agent_tab, cx),
            "and an agent tab must still be accepted, or the guard has closed the door on \
             rearranging agents too"
        );
    }

    /// Zoom is local to the column, and reversible.
    ///
    /// Routed through the dock's zoom it would key on `DockPosition`, which this
    /// column shares with the tool dock beside it — so the git panel would
    /// vanish along with the editor.
    #[gpui::test]
    async fn zooming_an_agent_claims_the_center_and_gives_it_back(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.add_panel(panel.clone(), window, cx)
            })
            .unwrap();

        panel.update_in(cx, |panel, window, cx| {
            panel.show(
                AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
                AgentViewMode::Terminal,
                window,
                cx,
            );
        });
        // Docked and focused, or the column is never drawn and every bounds
        // assertion below reads `None` for a reason that has nothing to do with
        // zoom — which is exactly how this test first passed a broken fix.
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.focus_panel::<AgentPanel>(window, cx);
            })
            .unwrap();
        cx.run_until_parked();

        let fills = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| panel.read(cx).fills_the_center(window, cx))
        };
        assert!(
            !fills(cx),
            "an agent starts sharing the row with the editor"
        );

        let pane = panel.read_with(cx, |panel, _| panel.active_pane.clone());
        pane.update_in(cx, |pane, window, cx| {
            pane.toggle_zoom(&workspace::ToggleZoom, window, cx);
        });
        cx.run_until_parked();
        assert!(
            fills(cx),
            "zoomed, the column has to claim the centre or the button does nothing — \
             which is exactly what it did while ZoomIn fell through to the catch-all arm"
        );

        // Claiming the centre is worth nothing if the column then draws an empty
        // box, which is what happened when the zoomed pane was handed to
        // `PaneGroup::render`: that renders it as an empty div, on the
        // understanding that the workspace's absolute overlay is drawing it —
        // and this panel has no overlay. Both halves of the screen went blank
        // while every state assertion above still passed.
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let agent = cx
            .debug_bounds("agent-view")
            .expect("the zoomed agent must still be drawn");
        assert!(
            agent.size.width > px(0.) && agent.size.height > px(0.),
            "the zoomed agent drew with no area: {agent:?}"
        );

        pane.update_in(cx, |pane, window, cx| {
            pane.toggle_zoom(&workspace::ToggleZoom, window, cx);
        });
        cx.run_until_parked();
        assert!(!fills(cx), "and unzooming has to give it back");
    }

    /// Zoom must not outlive the agent it was zooming.
    ///
    /// `zoomed_pane` was cleared only on `ZoomOut`, and `fills_the_center` asked
    /// nothing more of it than `is_some()`. Closing the last agent left the
    /// handle standing, so the *next* agent opened came back filling the centre
    /// and the editor vanished for someone who never pressed the button.
    /// Measured before the fix: `after close: fills=true`,
    /// `after reopen: dock_open=true fills=true`.
    #[gpui::test]
    async fn closing_a_zoomed_agent_does_not_leave_the_next_one_zoomed(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.add_panel(panel.clone(), window, cx)
            })
            .unwrap();
        let agent = AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string());

        let open = |cx: &mut gpui::VisualTestContext| {
            let agent = agent.clone();
            panel.update_in(cx, |panel, window, cx| {
                panel.show(agent, AgentViewMode::Terminal, window, cx);
            });
            workspace
                .update_in(cx, |workspace, window, cx| {
                    workspace.focus_panel::<AgentPanel>(window, cx);
                })
                .unwrap();
            cx.run_until_parked();
        };
        let fills = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| panel.read(cx).fills_the_center(window, cx))
        };

        open(cx);
        let pane = panel.read_with(cx, |panel, _| panel.active_pane.clone());
        pane.update_in(cx, |pane, window, cx| {
            pane.toggle_zoom(&workspace::ToggleZoom, window, cx);
        });
        cx.run_until_parked();
        assert!(fills(cx), "precondition: the column is zoomed");

        pane.update_in(cx, |pane, window, cx| {
            pane.close_active_item(
                &pane::CloseActiveItem {
                    save_intent: Some(SaveIntent::Skip),
                    close_pinned: true,
                },
                window,
                cx,
            )
        })
        .await
        .unwrap();
        cx.run_until_parked();
        assert!(
            !fills(cx),
            "an empty column cannot be filling the centre — there is nothing in it to fill with"
        );
        assert!(
            !pane.read_with(cx, |pane, _| pane.is_zoomed()),
            "and the maximise button must not still read as zoomed"
        );

        open(cx);
        assert!(
            !fills(cx),
            "reopening an agent must not come back hiding the editor"
        );
    }

    /// A pane's subscription has to leave with the pane.
    ///
    /// `_subscriptions` was a `Vec` that only ever grew: `add_pane` pushed, and
    /// the `Remove` arm dropped the pane from the group without dropping what
    /// watched it. Every split-then-close cycle left an entry behind for the
    /// life of the window.
    #[gpui::test]
    async fn closing_a_split_agent_drops_what_was_watching_it(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.add_panel(panel.clone(), window, cx)
            })
            .unwrap();

        for agent in [project::CLAUDE_CODE_AGENT_ID, project::CODEX_AGENT_ID] {
            panel.update_in(cx, |panel, window, cx| {
                panel.show(
                    AgentId::new(agent.to_string()),
                    AgentViewMode::Terminal,
                    window,
                    cx,
                );
            });
        }
        cx.run_until_parked();

        let (panes, watched) = panel.read_with(cx, |panel, _| {
            (panel.center.panes().len(), panel._subscriptions.len())
        });
        assert_eq!(panes, 2, "two agents should stand in two panes");
        assert_eq!(watched, 2, "one subscription per pane");

        let second = panel.read_with(cx, |panel, _| panel.active_pane.clone());
        second
            .update_in(cx, |pane, window, cx| {
                pane.close_active_item(
                    &pane::CloseActiveItem {
                        save_intent: Some(SaveIntent::Skip),
                        close_pinned: true,
                    },
                    window,
                    cx,
                )
            })
            .await
            .unwrap();
        cx.run_until_parked();

        panel.read_with(cx, |panel, _| {
            assert_eq!(panel.center.panes().len(), 1, "one pane left");
            assert_eq!(
                panel._subscriptions.len(),
                1,
                "and one subscription — the closed pane's must have gone with it"
            );
        });
    }

    /// The column stops splitting before a section is too narrow to read.
    #[gpui::test]
    async fn agents_past_the_cap_arrive_as_tabs_rather_than_slivers(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.add_panel(panel.clone(), window, cx)
            })
            .unwrap();

        let opened = MAX_SIDE_BY_SIDE_AGENTS + 2;
        for _ in 0..opened {
            panel.update_in(cx, |panel, window, cx| {
                panel.show_new(
                    AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
                    AgentViewMode::Terminal,
                    window,
                    cx,
                );
            });
        }
        cx.run_until_parked();

        panel.read_with(cx, |panel, cx| {
            assert_eq!(
                panel.center.panes().len(),
                MAX_SIDE_BY_SIDE_AGENTS,
                "the column must stop splitting at the cap"
            );
            let items: usize = panel
                .center
                .panes()
                .iter()
                .map(|pane| pane.read(cx).items_len())
                .sum();
            assert_eq!(
                items, opened,
                "and every agent asked for is still open, as a tab where there was no room to split"
            );
        });
    }

    /// Rename has to reach the view, not just the tab.
    ///
    /// Both ways in — the tab's context menu and a double-click on the tab —
    /// dispatch through the *item's* focus handle (`Pane::render_tab`). A
    /// handler registered only on the tab's own element is in that path just
    /// while the tab is unselected, which is never the tab someone right-clicks
    /// to rename. Nothing happened, and nothing errored either.
    #[gpui::test]
    async fn rename_reaches_the_agent_and_enter_commits_it(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.add_panel(panel.clone(), window, cx)
            })
            .unwrap();

        panel.update_in(cx, |panel, window, cx| {
            panel.show(
                AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
                AgentViewMode::Terminal,
                window,
                cx,
            );
        });
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.focus_panel::<AgentPanel>(window, cx);
            })
            .unwrap();
        cx.run_until_parked();

        let view = panel.read_with(cx, |panel, cx| {
            panel
                .active_pane
                .read(cx)
                .items()
                .find_map(|item| item.downcast::<AgentView>())
                .expect("the agent just shown")
        });
        assert_eq!(
            view.read_with(cx, |view, _| view.tab_label()),
            "Claude Code",
            "the tab starts on the agent's own name"
        );

        // Exactly what `Pane::render_tab` does for both the menu entry and the
        // double-click: dispatch through the item's focus handle.
        cx.update(|window, cx| {
            view.read(cx)
                .focus_handle(cx)
                .dispatch_action(&crate::RenameAgent, window, cx);
        });
        cx.run_until_parked();

        let editor = view
            .read_with(cx, |view, _| view.rename_editor().cloned())
            .expect("dispatching Rename must open the editor, or the menu entry does nothing");
        assert!(
            cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)),
            "and the editor has to take focus, or there is nothing to type into"
        );
        assert_eq!(
            editor.read_with(cx, |editor, cx| editor.text(cx)),
            "Claude Code",
            "opening it selects the current name, so typing replaces it"
        );

        editor.update_in(cx, |editor, window, cx| {
            editor.set_text("Refactor run", window, cx);
        });
        cx.update(|window, cx| {
            editor
                .focus_handle(cx)
                .dispatch_action(&menu::Confirm, window, cx);
        });
        cx.run_until_parked();

        assert_eq!(
            view.read_with(cx, |view, _| view.tab_label()),
            "Refactor run",
            "Enter must commit the new name"
        );
        assert!(
            view.read_with(cx, |view, _| view.rename_editor().is_none()),
            "and close the editor behind it"
        );
    }

    fn agent_view_count(panel: &AgentPanel, cx: &App) -> usize {
        panel
            .center
            .panes()
            .iter()
            .map(|pane| {
                pane.read(cx)
                    .items()
                    .filter_map(|item| item.downcast::<AgentView>())
                    .count()
            })
            .sum()
    }

    /// And the same rule read from the other end: the section is worth its width
    /// only while an agent is standing in it.
    #[gpui::test]
    async fn closing_the_last_agent_puts_the_dock_away(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.add_panel(panel.clone(), window, cx)
            })
            .unwrap();

        panel.update_in(cx, |panel, window, cx| {
            panel.show(
                AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
                AgentViewMode::Terminal,
                window,
                cx,
            );
        });
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.focus_panel::<AgentPanel>(window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        assert!(
            dock_is_open(&panel, cx),
            "an agent is open, so the dock must be showing it"
        );

        let (pane, agent) = panel.read_with(cx, |panel, cx| {
            let pane = panel.active_pane.clone();
            let agent = pane
                .read(cx)
                .items()
                .next()
                .expect("the agent just shown")
                .item_id();
            (pane, agent)
        });
        pane.update_in(cx, |pane, window, cx| {
            pane.close_item_by_id(agent, SaveIntent::Skip, window, cx)
        })
        .await
        .expect("closing an agent");
        cx.run_until_parked();

        assert!(
            !dock_is_open(&panel, cx),
            "the last agent is gone, so its section must go with it"
        );
    }

    /// The column stands on the rail's side, and moves when the rail moves.
    ///
    /// This used to assert it followed `agent.sidebar_side`, a setting of its
    /// own — which is exactly how the agent and the buttons that open it could
    /// end up at opposite edges. That setting no longer decides this.
    #[gpui::test]
    async fn the_column_stands_on_the_rails_side(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;
        let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());

        for (rail, expected) in [
            (SidebarSide::Left, DockPosition::Left),
            (SidebarSide::Right, DockPosition::Right),
        ] {
            cx.update(|_window, cx| {
                SettingsStore::update_global(cx, |settings, cx| {
                    settings.update_user_settings(cx, |settings| {
                        settings
                            .workspace
                            .multi_project
                            .get_or_insert_default()
                            .sidebar_side = Some(rail);
                    });
                });
            });
            cx.run_until_parked();

            assert_eq!(
                cx.update(|window, cx| panel.read(cx).position(window, cx)),
                expected,
                "the agent column must stand on the rail's side"
            );
            assert_eq!(
                workspace
                    .read_with(cx, |workspace, cx| workspace
                        .agent_dock()
                        .read(cx)
                        .position())
                    .unwrap(),
                expected,
                "and the column itself must move, or its border and resize handle \
                 stay on the edge it left"
            );
        }
    }

    /// An agent is added to a docked, focused panel and the window is drawn.
    ///
    /// This does **not** cover the drag path, where the re-entrant read that
    /// aborted the process actually lived — reproducing that needs a real
    /// `DraggedTab` and the drag machinery behind it. What it does cover is the
    /// add-and-paint path, which is what a rail click runs.
    #[gpui::test]
    async fn showing_an_agent_in_a_docked_panel_draws(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;

        // In the dock and focused, which is the path a rail click takes: that is
        // where the workspace starts asking this panel for its focus handle.
        let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());
        workspace
            .update_in(cx, |workspace, window, cx| {
                workspace.add_panel(panel.clone(), window, cx);
                workspace.focus_panel::<AgentPanel>(window, cx);
            })
            .unwrap();
        cx.run_until_parked();

        // Terminal mode: `start` is spawned, so nothing runs before the assertion.
        panel.update_in(cx, |panel, window, cx| {
            panel.show(
                AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
                AgentViewMode::Terminal,
                window,
                cx,
            );
        });

        // Drawing is the missing half: the panic this guards against happens while
        // the window is painting the pane, not while the item is being added.
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    /// Dragging a tab to a pane edge is `Pane::split` underneath, so that is what
    /// this drives — the drag machinery itself needs a real `DraggedTab`.
    #[gpui::test]
    async fn dragging_one_of_two_agents_out_makes_a_second_section(cx: &mut TestAppContext) {
        let (panel, cx) = panel(cx).await;

        for agent in [project::CLAUDE_CODE_AGENT_ID, project::CODEX_AGENT_ID] {
            panel.update_in(cx, |panel, window, cx| {
                panel.show(
                    AgentId::new(agent.to_string()),
                    AgentViewMode::Terminal,
                    window,
                    cx,
                );
            });
        }
        cx.run_until_parked();

        // Both in one pane is what a user gets after dragging them together.
        let pane = panel.read_with(cx, |panel, _| panel.active_pane.clone());
        panel.update_in(cx, |panel, window, cx| {
            let other: Vec<_> = panel
                .center
                .panes()
                .into_iter()
                .filter(|p| **p != pane)
                .cloned()
                .collect();
            for source in other {
                let item = source
                    .update(cx, |source, cx| source.take_active_item(window, cx))
                    .unwrap();
                pane.update(cx, |pane, cx| {
                    pane.add_item(item, false, false, None, window, cx)
                });
            }
        });
        cx.run_until_parked();
        assert_eq!(
            panel.read_with(cx, |panel, cx| panel.active_pane.read(cx).items_len()),
            2,
            "both agents should now be tabs in one pane"
        );
        let before = panel.read_with(cx, |panel, _| panel.center.panes().len());

        pane.update_in(cx, |pane, window, cx| {
            pane.split(SplitDirection::Down, pane::SplitMode::MovePane, window, cx);
        });
        cx.run_until_parked();

        assert_eq!(
            panel.read_with(cx, |panel, _| panel.center.panes().len()),
            before + 1,
            "splitting one agent away from the other must add a section"
        );
        assert_eq!(
            panel.read_with(cx, |panel, cx| panel.active_pane.read(cx).items_len()),
            1,
            "the agent that moved should be alone in the new section"
        );
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
