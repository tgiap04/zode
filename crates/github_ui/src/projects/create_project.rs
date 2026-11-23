//! The "New project" dialog: a Projects (v2) project created under the
//! signed-in account or the organization owning the repository, linked to
//! the repository, and picked in the Projects section once it exists.

use editor::{Editor, EditorEvent};
use github::{GithubError, ProjectOwner, ProjectOwners, ProjectV2};
use gpui::{
    App, AppContext as _, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    SharedString, Subscription, Task, WeakEntity, Window,
};
use workspace::{DismissDecision, ModalView, Workspace};

use crate::github_view::GithubContext;
use crate::github_view_session::{is_live_session, report_if_revoked, session_ended_message};
use crate::projects::move_item::PROJECT_WRITE_SCOPE;
use crate::projects::projects_section::ProjectsSection;

/// GitHub's limit on a project title.
pub const MAX_PROJECT_TITLE_CHARS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerChoice {
    Viewer,
    Organization,
}

pub(crate) enum OwnersLoad {
    Loading,
    Loaded(ProjectOwners),
    Failed(SharedString),
}

pub struct CreateProjectDialog {
    pub(crate) context: GithubContext,
    section: WeakEntity<ProjectsSection>,
    pub(crate) title: Entity<Editor>,
    pub(crate) owners: OwnersLoad,
    pub(crate) owner: OwnerChoice,
    pub(crate) missing_scope: bool,
    pub(crate) error: Option<SharedString>,
    pub(crate) creating: bool,
    _owners_task: Option<Task<()>>,
    create_task: Option<Task<()>>,
    _title_subscription: Subscription,
}

impl EventEmitter<DismissEvent> for CreateProjectDialog {}

impl Focusable for CreateProjectDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.title.focus_handle(cx)
    }
}

impl ModalView for CreateProjectDialog {
    /// Kept open while GitHub is creating the project, so its answer has
    /// somewhere to land.
    fn on_before_dismiss(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> DismissDecision {
        DismissDecision::Dismiss(!self.creating)
    }
}

impl ProjectsSection {
    /// Opens the dialog over the workspace. A token known to lack `project`
    /// opens it with the scope guidance in place of the form.
    pub fn open_create_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = Workspace::for_window(window, cx) else {
            return;
        };
        let context = self.context.clone();
        let section = cx.entity().downgrade();
        let missing_scope = self.scopes.includes(PROJECT_WRITE_SCOPE) == Some(false);
        // Called from a click while this section is being updated; the
        // dialog is made once that is over.
        window.defer(cx, move |window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.toggle_modal(window, cx, move |window, cx| {
                    CreateProjectDialog::new(context, section, missing_scope, window, cx)
                });
            });
        });
    }
}

impl CreateProjectDialog {
    pub(crate) fn new(
        context: GithubContext,
        section: WeakEntity<ProjectsSection>,
        missing_scope: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("Project title", window, cx);
            editor
        });
        let title_subscription = cx.subscribe(&title, |_, _, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::BufferEdited) {
                cx.notify();
            }
        });
        let owners_task = (!missing_scope).then(|| {
            let backend = context.backend.clone();
            let repository = context.repository.clone();
            cx.spawn(async move |this, cx| {
                let owners = backend.project_owners(&repository).await;
                this.update(cx, |this, cx| {
                    this.owners = match owners {
                        Ok(owners) => {
                            // A repository an organization owns usually keeps its
                            // projects there too; the account stays one click away.
                            if owners.organization.is_some() {
                                this.owner = OwnerChoice::Organization;
                            }
                            OwnersLoad::Loaded(owners)
                        }
                        Err(error) => {
                            report_if_revoked(&error, &this.context.session, cx);
                            OwnersLoad::Failed(
                                format!("Could not read who may own the project: {error}").into(),
                            )
                        }
                    };
                    cx.notify();
                })
                .ok();
            })
        });
        Self {
            context,
            section,
            title,
            owners: OwnersLoad::Loading,
            owner: OwnerChoice::Viewer,
            missing_scope,
            error: None,
            creating: false,
            _owners_task: owners_task,
            create_task: None,
            _title_subscription: title_subscription,
        }
    }

    /// The accounts offered, with their logins: the signed-in account, and
    /// the organization only when it owns the repository.
    pub fn owner_choices(&self) -> Vec<(OwnerChoice, String)> {
        let OwnersLoad::Loaded(owners) = &self.owners else {
            return Vec::new();
        };
        let mut choices = vec![(OwnerChoice::Viewer, owners.viewer.login.clone())];
        if let Some(organization) = &owners.organization {
            choices.push((OwnerChoice::Organization, organization.login.clone()));
        }
        choices
    }

    pub(crate) fn owner_account(&self, choice: OwnerChoice) -> Option<&ProjectOwner> {
        let OwnersLoad::Loaded(owners) = &self.owners else {
            return None;
        };
        match choice {
            OwnerChoice::Viewer => Some(&owners.viewer),
            OwnerChoice::Organization => owners.organization.as_ref(),
        }
    }

    pub fn select_owner(&mut self, choice: OwnerChoice, cx: &mut Context<Self>) {
        if self
            .owner_choices()
            .iter()
            .any(|(offered, _)| *offered == choice)
        {
            self.owner = choice;
            cx.notify();
        }
    }

    pub fn owner(&self) -> OwnerChoice {
        self.owner
    }

    pub fn missing_scope(&self) -> bool {
        self.missing_scope
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_ref().map(|error| error.as_ref())
    }

    pub fn target_line(&self) -> String {
        format!("@{}", self.context.login)
    }

    pub fn title_text(&self, cx: &App) -> String {
        self.title.read(cx).text(cx).trim().to_string()
    }

    /// Why the title cannot be sent, if it cannot.
    pub fn title_problem(&self, cx: &App) -> Option<String> {
        let title = self.title_text(cx);
        if title.is_empty() {
            Some("A title is required.".to_string())
        } else if title.chars().count() > MAX_PROJECT_TITLE_CHARS {
            Some(format!(
                "A title is at most {MAX_PROJECT_TITLE_CHARS} characters."
            ))
        } else {
            None
        }
    }

    pub fn can_create(&self, cx: &App) -> bool {
        !self.creating
            && !self.missing_scope
            && matches!(self.owners, OwnersLoad::Loaded(_))
            && self.title_problem(cx).is_none()
    }

    /// Sends the project once, through the token's write queue. On failure
    /// the dialog stays open with the title kept, and says why.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.creating || self.missing_scope {
            return;
        }
        if let Some(problem) = self.title_problem(cx) {
            self.error = Some(problem.into());
            cx.notify();
            return;
        }
        if !is_live_session(&self.context.session, cx) {
            self.error = Some(session_ended_message("dialog").into());
            cx.notify();
            return;
        }
        let OwnersLoad::Loaded(owners) = &self.owners else {
            return;
        };
        let owner_id = match (self.owner, &owners.organization) {
            (OwnerChoice::Organization, Some(organization)) => organization.id.clone(),
            _ => owners.viewer.id.clone(),
        };
        let write = self.context.backend.create_project(
            owner_id,
            self.title_text(cx),
            owners.repository_id.clone(),
        );
        self.creating = true;
        self.error = None;
        self.create_task = Some(cx.spawn(async move |this, cx| {
            let created = write.await;
            this.update(cx, |this, cx| {
                this.creating = false;
                match created {
                    Ok(project) => this.created(project, cx),
                    Err(error) => this.failed(error, cx),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn created(&mut self, project: ProjectV2, cx: &mut Context<Self>) {
        self.section
            .update(cx, |section, cx| section.project_created(project, cx))
            .ok();
        cx.emit(DismissEvent);
    }

    fn failed(&mut self, error: GithubError, cx: &mut Context<Self>) {
        report_if_revoked(&error, &self.context.session, cx);
        match &error {
            GithubError::MissingScope { scope } if scope.contains("project") => {
                self.missing_scope = true;
                self.section
                    .update(cx, |section, _| section.note_write_error(&error))
                    .ok();
            }
            _ => self.error = Some(error.to_string().into()),
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    pub(crate) fn section(&self) -> &WeakEntity<ProjectsSection> {
        &self.section
    }
}
