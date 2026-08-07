use crate::Sidebar;
use collections::HashMap;
use gpui::{Context, Entity, EntityId};
use workspace::MultiWorkspace;

impl Sidebar {
    /// FR7: keeps one `Event::ActivityChanged` subscription alive per
    /// currently-open project, adding new ones and dropping stale ones so
    /// this stays in sync with the live project set on every
    /// `update_entries` call rather than growing unbounded as projects
    /// open and close.
    pub(crate) fn resync_project_activity_subscriptions(
        &mut self,
        multi_workspace: &Entity<MultiWorkspace>,
        cx: &mut Context<Self>,
    ) {
        let live_projects: HashMap<EntityId, Entity<project::Project>> = multi_workspace
            .read(cx)
            .workspaces()
            .map(|workspace| {
                let project = workspace.read(cx).project().clone();
                (project.entity_id(), project)
            })
            .collect();

        self.project_activity_subscriptions
            .retain(|id, _| live_projects.contains_key(id));

        for (id, project) in live_projects {
            self.project_activity_subscriptions
                .entry(id)
                .or_insert_with(|| {
                    cx.subscribe(&project, |this, _project, event: &project::Event, cx| {
                        if matches!(event, project::Event::ActivityChanged(_)) {
                            this.update_entries(cx);
                        }
                    })
                });
        }
    }
}
