use crate::Sidebar;
use agent_ui::{AgentView, AgentViewEvent};
use collections::HashMap;
use gpui::{Context, Entity, EntityId};
use workspace::{MultiWorkspace, Workspace};

impl Sidebar {
    /// Keeps the rail's waiting-agents count live for projects that are not in
    /// front. `MultiWorkspaceEvent` says nothing about what happens inside a
    /// background workspace, so this listens to each workspace's tab changes
    /// and to each agent tab's own `Attention` edge. Both maps are pruned to
    /// what is alive on every call, so a closed tab or workspace leaves nothing
    /// behind.
    pub(crate) fn resync_agent_attention_subscriptions(
        &mut self,
        multi_workspace: &Entity<MultiWorkspace>,
        cx: &mut Context<Self>,
    ) {
        let workspaces: HashMap<EntityId, Entity<Workspace>> = multi_workspace
            .read(cx)
            .workspaces()
            .map(|workspace| (workspace.entity_id(), workspace.clone()))
            .collect();

        let mut live_views: HashMap<EntityId, Entity<AgentView>> = HashMap::default();
        for workspace in workspaces.values() {
            for view in workspace.read(cx).items_of_type::<AgentView>(cx) {
                live_views.insert(view.entity_id(), view);
            }
        }

        self.workspace_item_subscriptions
            .retain(|id, _| workspaces.contains_key(id));
        self.agent_attention_subscriptions
            .retain(|id, _| live_views.contains_key(id));

        for (id, workspace) in workspaces {
            self.workspace_item_subscriptions
                .entry(id)
                .or_insert_with(|| {
                    cx.subscribe(
                        &workspace,
                        |this, _workspace, event: &workspace::Event, cx| match event {
                            workspace::Event::ItemAdded { item } => {
                                if item.downcast::<AgentView>().is_some() {
                                    this.update_entries(cx);
                                }
                            }
                            workspace::Event::ItemRemoved { item_id } => {
                                if this.agent_attention_subscriptions.contains_key(item_id) {
                                    this.update_entries(cx);
                                }
                            }
                            _ => {}
                        },
                    )
                });
        }

        for (id, view) in live_views {
            self.agent_attention_subscriptions
                .entry(id)
                .or_insert_with(|| {
                    cx.subscribe(&view, |this, _view, event: &AgentViewEvent, cx| {
                        if matches!(event, AgentViewEvent::Attention) {
                            this.update_entries(cx);
                        }
                    })
                });
        }
    }
}
