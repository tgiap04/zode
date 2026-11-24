use std::collections::HashSet;
use std::sync::Arc;

use github::{GithubBackend, GithubError, ProjectV2, ProjectV2Item};
use gpui::{AsyncApp, Context, WeakEntity};

use crate::projects::move_item::show_value;
use crate::projects::projects_section::{
    PROJECT_ITEM_CAP, PROJECT_READ_SCOPE, ProjectsLoad, ProjectsSection, ProjectsSectionEvent,
};

/// Which items the user added or removed while a read was on its way. Such
/// a read can predate them: without this it would drop an added item and
/// bring a removed one back, as `recent_writes` does for field values.
#[derive(Default)]
pub(super) struct MembershipChanges {
    /// Added since the read began, in order.
    added: Vec<String>,
    /// Removals that landed since the read began.
    removed: HashSet<String>,
}

impl MembershipChanges {
    /// What happened before the read is no longer news to it.
    fn begin_read(&mut self) {
        self.added.clear();
        self.removed.clear();
    }

    fn is_removed(&self, item_id: &str) -> bool {
        self.removed.contains(item_id)
    }
}

impl ProjectsSection {
    pub(super) fn note_item_added(&mut self, item_id: &str) {
        let changes = &mut self.membership_changes;
        changes.removed.remove(item_id);
        if !changes.added.iter().any(|added| added == item_id) {
            changes.added.push(item_id.to_string());
        }
    }

    pub(super) fn note_removal_sent(&mut self, item_id: &str) {
        self.membership_changes
            .added
            .retain(|added| added != item_id);
    }

    /// GitHub took the item out. A read that began before and brought it
    /// back is undone too, now or when it lands.
    pub(super) fn note_removal_landed(&mut self, item_id: &str) {
        self.membership_changes.removed.insert(item_id.to_string());
        let before = self.items.len();
        self.items.retain(|item| item.id != item_id);
        if self.items.len() != before {
            self.layout_changed();
        }
    }

    /// `staged`, a whole read, with the adds and removals made since it
    /// began: removed items out, added ones in from what is shown.
    fn with_membership_changes(&self, mut staged: Vec<ProjectV2Item>) -> Vec<ProjectV2Item> {
        let changes = &self.membership_changes;
        staged.retain(|item| !changes.is_removed(&item.id));
        for item_id in &changes.added {
            if staged.iter().any(|item| &item.id == item_id) {
                continue;
            }
            if let Some(item) = self.items.iter().find(|item| &item.id == item_id) {
                staged.push(item.clone());
            }
        }
        // Pages were overlaid as they came in; a write that landed since
        // an earlier page was read is laid over it again here.
        self.overlay_field_writes(&mut staged);
        staged
    }

    /// The projects list, then the picked project (kept when it is still
    /// there) with its fields, views and items.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self.scopes.includes(PROJECT_READ_SCOPE) == Some(false) {
            self.missing_scope = true;
            self.load_task = None;
            self.load = ProjectsLoad::Idle;
            cx.notify();
            return;
        }
        self.missing_scope = false;
        self.move_message = None;
        self.load = ProjectsLoad::Loading;
        let backend = self.context.backend.clone();
        let repository = self.context.repository.clone();
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let projects = backend.list_projects(&repository, None).await;
            let picked = this
                .update(cx, |this, cx| match projects {
                    Ok(page) => this.take_projects(page.items, cx),
                    Err(error) => {
                        this.fail(error, cx);
                        None
                    }
                })
                .ok()
                .flatten();
            if let Some(project_id) = picked {
                read_project(this, backend, project_id, cx).await;
            }
        }));
        cx.notify();
    }

    /// Keeps the picked project when it is still listed, else picks the
    /// first open one. Returns the project to read.
    fn take_projects(
        &mut self,
        projects: Vec<ProjectV2>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        self.projects = projects;
        if let Some(created) = self.created_project.take()
            && !self.projects.iter().any(|project| project.id == created.id)
        {
            self.projects.push(created.clone());
            self.created_project = Some(created);
        }
        let kept = self
            .selected_project
            .clone()
            .filter(|id| self.projects.iter().any(|project| &project.id == id));
        if kept.is_none() {
            self.item_cap = PROJECT_ITEM_CAP;
        }
        self.selected_project = kept.or_else(|| {
            self.projects
                .iter()
                .find(|project| !project.closed)
                .or_else(|| self.projects.first())
                .map(|project| project.id.clone())
        });
        self.adopt_project(self.selected_project.clone());
        if self.selected_project.is_none() {
            self.load = ProjectsLoad::Idle;
            cx.emit(ProjectsSectionEvent::Loaded);
        }
        cx.notify();
        self.selected_project.clone()
    }

    /// A project just made from the dialog: listed and picked at once, then
    /// the list read again around it.
    pub fn project_created(&mut self, project: ProjectV2, cx: &mut Context<Self>) {
        self.selected_project = Some(project.id.clone());
        self.selected_view = None;
        self.layout_changed();
        self.item_cap = PROJECT_ITEM_CAP;
        self.adopt_project(Some(project.id.clone()));
        if !self.projects.iter().any(|listed| listed.id == project.id) {
            self.projects.push(project.clone());
        }
        self.created_project = Some(project);
        self.reload(cx);
    }

    pub fn select_project(&mut self, project_id: String, cx: &mut Context<Self>) {
        if self.selected_project.as_deref() == Some(project_id.as_str()) {
            return;
        }
        self.selected_project = Some(project_id.clone());
        self.selected_view = None;
        self.layout_changed();
        self.item_cap = PROJECT_ITEM_CAP;
        self.adopt_project(Some(project_id.clone()));
        self.load = ProjectsLoad::Loading;
        let backend = self.context.backend.clone();
        self.load_task = Some(cx.spawn(async move |this, cx| {
            read_project(this, backend, project_id, cx).await;
        }));
        cx.notify();
    }

    /// Another [`PROJECT_ITEM_CAP`] items after the ones read.
    pub fn load_more(&mut self, cx: &mut Context<Self>) {
        let (Some(cursor), Some(project_id), ProjectsLoad::Idle) = (
            self.items_cursor.clone(),
            self.selected_project.clone(),
            &self.load,
        ) else {
            return;
        };
        self.item_cap = self.items.len() + PROJECT_ITEM_CAP;
        self.load = ProjectsLoad::Loading;
        let backend = self.context.backend.clone();
        let cap = self.item_cap;
        self.load_task = Some(cx.spawn(async move |this, cx| {
            read_items(this, backend, project_id, Some(cursor), cap, cx).await;
        }));
        cx.notify();
    }

    /// The picked project's fields, views and items again, the view kept, as
    /// many items as were read before.
    pub(super) fn reread_project(&mut self, cx: &mut Context<Self>) {
        let Some(project_id) = self.selected_project.clone() else {
            return;
        };
        self.load = ProjectsLoad::Loading;
        let backend = self.context.backend.clone();
        self.load_task = Some(cx.spawn(async move |this, cx| {
            read_project(this, backend, project_id, cx).await;
        }));
        cx.notify();
    }

    /// Keeps [`Self::item_cap`]: a project read again keeps what "Load more"
    /// asked for.
    fn clear_project(&mut self) {
        self.details = None;
        self.items.clear();
        self.items_cursor = None;
        self.layout_changed();
    }

    /// Makes `project_id` the project the items belong to. Another one's
    /// items, and everything that points into them (writes under way, the
    /// open menus and editors), are dropped at once, not when the new
    /// project's answer arrives; the same project keeps them while it is read
    /// again.
    pub(super) fn adopt_project(&mut self, project_id: Option<String>) {
        if self.items_project == project_id {
            return;
        }
        self.clear_project();
        self.layout.forget_column_lists();
        self.field_writes.clear();
        self.recent_writes.clear();
        self.membership_changes = MembershipChanges::default();
        self.project_epoch += 1;
        self.items_project = project_id;
        self.selected_card = None;
        self.card_menu = None;
        self.field_edit = None;
        self.field_menu = None;
        self.field_message = None;
        self.move_message = None;
        self.add_item = None;
        self.pending_removal = None;
    }

    /// Puts the values still being written, or written since the read
    /// started, over what a read brought: it may predate them.
    pub(super) fn overlay_field_writes(&self, items: &mut [ProjectV2Item]) {
        let recent = self.recent_writes.iter();
        let pending = self
            .field_writes
            .iter()
            .map(|(key, pending)| (key, &pending.shown));
        for (key, value) in recent.chain(pending) {
            if let Some(item) = items.iter_mut().find(|item| item.id == key.item_id) {
                show_value(item, &key.field_id, value.clone());
            }
        }
    }

    fn fail(&mut self, error: GithubError, cx: &mut Context<Self>) {
        self.note_error(&error, cx);
        match error {
            GithubError::MissingScope { scope } if scope.contains("project") => {
                self.missing_scope = true;
                self.load = ProjectsLoad::Idle;
            }
            error => self.load = ProjectsLoad::Failed(error),
        }
        cx.emit(ProjectsSectionEvent::Loaded);
        cx.notify();
    }
}

async fn read_project(
    this: WeakEntity<ProjectsSection>,
    backend: Arc<dyn GithubBackend>,
    project_id: String,
    cx: &mut AsyncApp,
) {
    // Before asking: a write that lands from here on is newer than the read.
    this.update(cx, |this, _| {
        this.adopt_project(Some(project_id.clone()));
        this.recent_writes.clear();
        this.membership_changes.begin_read();
    })
    .ok();
    let details = backend.project_details(project_id.clone()).await;
    let cap = this
        .update(cx, |this, cx| match details {
            Ok(details) => {
                this.details = Some(details);
                this.layout_changed();
                cx.notify();
                Some(this.item_cap)
            }
            Err(error) => {
                this.fail(error, cx);
                None
            }
        })
        .ok()
        .flatten();
    if let Some(cap) = cap {
        read_items(this, backend, project_id, None, cap, cx).await;
    }
}

/// Pages of items appended as they come, until the last page or `cap`. A
/// page that claims more under any cursor this reading already sent (not
/// only the one it was asked with: cursors may take turns) ends the
/// reading, so GitHub is not asked the same thing until the rate limit. An
/// empty page under a new cursor (every item on it hidden) is read past.
async fn read_items(
    this: WeakEntity<ProjectsSection>,
    backend: Arc<dyn GithubBackend>,
    project_id: String,
    mut cursor: Option<String>,
    cap: usize,
    cx: &mut AsyncApp,
) {
    let mut sent_cursors = HashSet::new();
    // A reading from the start replaces the items shown, in one go once it
    // is whole, so the board is never emptied to be filled again. With
    // nothing shown yet each page appears as it comes.
    let replaces = cursor.is_none();
    let mut staged: Vec<ProjectV2Item> = Vec::new();
    loop {
        if let Some(sent) = &cursor {
            sent_cursors.insert(sent.clone());
        }
        let page = backend.list_project_items(project_id.clone(), cursor).await;
        let next = this
            .update(cx, |this, cx| {
                let page = match page {
                    Ok(page) => page,
                    Err(error) => {
                        this.fail(error, cx);
                        return None;
                    }
                };
                let stalled = page
                    .next_cursor
                    .as_ref()
                    .is_some_and(|next| sent_cursors.contains(next));
                let shown = if replaces { &staged } else { &this.items };
                let known = shown
                    .iter()
                    .map(|item| item.id.clone())
                    .collect::<HashSet<_>>();
                let mut fresh = page
                    .items
                    .into_iter()
                    .filter(|item| {
                        !known.contains(&item.id) && !this.membership_changes.is_removed(&item.id)
                    })
                    .collect::<Vec<_>>();
                this.overlay_field_writes(&mut fresh);
                this.items_cursor = page.next_cursor;
                if stalled {
                    log::warn!("a project's item pages stopped moving on; reading stopped there");
                    this.items_cursor = None;
                }
                let next = this.items_cursor.clone().filter(|_| {
                    let held = if replaces {
                        staged.len()
                    } else {
                        this.items.len()
                    };
                    held + fresh.len() < cap
                });
                if replaces {
                    staged.extend(fresh);
                    if next.is_none() || this.items.is_empty() {
                        this.items = this.with_membership_changes(staged.clone());
                        this.layout_changed();
                    }
                } else {
                    this.items.extend(fresh);
                    this.layout_changed();
                }
                if next.is_none() {
                    this.load = ProjectsLoad::Idle;
                    cx.emit(ProjectsSectionEvent::Loaded);
                }
                cx.notify();
                next
            })
            .ok()
            .flatten();
        match next {
            Some(next) => cursor = Some(next),
            None => return,
        }
    }
}
