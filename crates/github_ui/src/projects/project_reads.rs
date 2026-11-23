use std::collections::HashSet;
use std::sync::Arc;

use github::{GithubBackend, GithubError, ProjectV2};
use gpui::{AsyncApp, Context, WeakEntity};

use crate::projects::projects_section::{
    PROJECT_ITEM_CAP, PROJECT_READ_SCOPE, ProjectsLoad, ProjectsSection, ProjectsSectionEvent,
};

impl ProjectsSection {
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
        if self.selected_project.is_none() {
            self.clear_project();
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
        self.item_cap = PROJECT_ITEM_CAP;
        self.clear_project();
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
        self.item_cap = PROJECT_ITEM_CAP;
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
    let details = backend.project_details(project_id.clone()).await;
    let cap = this
        .update(cx, |this, cx| {
            this.clear_project();
            match details {
                Ok(details) => {
                    this.details = Some(details);
                    cx.notify();
                    Some(this.item_cap)
                }
                Err(error) => {
                    this.fail(error, cx);
                    None
                }
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
                let known = this
                    .items
                    .iter()
                    .map(|item| item.id.clone())
                    .collect::<HashSet<_>>();
                this.items.extend(
                    page.items
                        .into_iter()
                        .filter(|item| !known.contains(&item.id)),
                );
                this.items_cursor = page.next_cursor;
                if stalled {
                    log::warn!("a project's item pages stopped moving on; reading stopped there");
                    this.items_cursor = None;
                }
                let next = this.items_cursor.clone().filter(|_| this.items.len() < cap);
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
