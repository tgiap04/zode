use std::sync::Arc;

use github::{
    GithubError, OAuthScopes, ProjectDetails, ProjectItemContent, ProjectV2, ProjectV2Item,
    ProjectView, RepositoryId,
};
use std::collections::HashMap;

use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, SharedString, Task, Window,
};
use ui::TableInteractionState;
use workspace::Workspace;

use crate::github_view::{DetailKey, GithubContext};
use crate::github_view_session::report_if_revoked;
use crate::issues::open_issue_detail;
use crate::projects::add_item::AddItemPicker;
use crate::projects::card_menu::CardMenu;
use crate::projects::field_editor::{FieldEdit, FieldMenu};
use crate::projects::move_item::{FieldWriteKey, PendingFieldWrites};

/// Items read on their own before the section stops and offers more. GitHub
/// neither pages a board by column nor applies a view's filter, so every
/// item is read; the cap keeps a very large project from costing hundreds
/// of queries.
pub const PROJECT_ITEM_CAP: usize = 500;

/// What reading projects needs; `project` grants it too.
pub const PROJECT_READ_SCOPE: &str = "read:project";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectsLoad {
    Idle,
    Loading,
    Failed(GithubError),
}

pub enum ProjectsSectionEvent {
    /// An answer came back; the rate limits it reported may have moved.
    Loaded,
    /// The guidance's "Sign in with GitHub", which asks for the scope.
    SignInWithGithub,
    /// The guidance's "Check again", after `gh auth refresh -s project`.
    CheckAgain,
}

/// The Projects section (read only): the projects linked to the repository,
/// one picked with one of its views, and the picked project's items, read a
/// page at a time up to [`PROJECT_ITEM_CAP`].
///
/// Nothing is read while the token is known to lack `read:project`.
pub struct ProjectsSection {
    pub(crate) context: GithubContext,
    pub(super) scopes: OAuthScopes,
    pub(super) missing_scope: bool,
    pub(super) projects: Vec<ProjectV2>,
    pub(super) selected_project: Option<String>,
    pub(super) details: Option<ProjectDetails>,
    pub(super) selected_view: Option<u64>,
    pub(super) items: Vec<ProjectV2Item>,
    pub(super) items_cursor: Option<String>,
    pub(super) item_cap: usize,
    pub(super) load: ProjectsLoad,
    pub(crate) table_state: Entity<TableInteractionState>,
    /// Replacing it cancels the read before, so an older answer never lands
    /// over a newer one.
    pub(super) load_task: Option<Task<()>>,
    /// The board's, for picking a card and opening its menu from the keyboard.
    pub(super) focus_handle: FocusHandle,
    /// Item id of the card picked by click or keyboard.
    pub(super) selected_card: Option<String>,
    pub(super) card_menu: Option<CardMenu>,
    pub(super) field_writes: HashMap<FieldWriteKey, PendingFieldWrites>,
    pub(super) next_write: u64,
    /// Why the last move did not stick.
    pub(super) move_message: Option<SharedString>,
    /// A move was asked for while the token can only read projects.
    pub(super) write_scope_missing: bool,
    /// The text, number or date cell being typed into.
    pub(super) field_edit: Option<FieldEdit>,
    /// The options or iterations offered for a table cell.
    pub(super) field_menu: Option<FieldMenu>,
    /// Why a field was not changed: it belongs to the issue, or a write of a
    /// cell no longer being edited failed.
    pub(super) field_message: Option<SharedString>,
    pub(super) add_item: Option<AddItemPicker>,
    /// The item whose removal waits for a confirmation.
    pub(super) pending_removal: Option<String>,
    /// A project made here that GitHub's list may not hold yet; kept in the
    /// list until it does.
    pub(super) created_project: Option<ProjectV2>,
}

impl EventEmitter<ProjectsSectionEvent> for ProjectsSection {}

impl ProjectsSection {
    /// Silent until [`Self::reload`].
    pub fn new(context: GithubContext, scopes: OAuthScopes, cx: &mut Context<Self>) -> Self {
        Self {
            context,
            scopes,
            missing_scope: false,
            projects: Vec::new(),
            selected_project: None,
            details: None,
            selected_view: None,
            items: Vec::new(),
            items_cursor: None,
            item_cap: PROJECT_ITEM_CAP,
            load: ProjectsLoad::Idle,
            table_state: cx.new(|cx| TableInteractionState::new(cx)),
            load_task: None,
            focus_handle: cx.focus_handle(),
            selected_card: None,
            card_menu: None,
            field_writes: HashMap::new(),
            next_write: 0,
            move_message: None,
            write_scope_missing: false,
            field_edit: None,
            field_menu: None,
            field_message: None,
            add_item: None,
            pending_removal: None,
            created_project: None,
        }
    }

    /// Whether this section was made for `context`: the same repository,
    /// read through the same signed-in backend.
    pub fn serves(&self, context: &GithubContext) -> bool {
        self.context.repository == context.repository
            && self.context.login == context.login
            && std::ptr::addr_eq(
                Arc::as_ptr(&self.context.backend),
                Arc::as_ptr(&context.backend),
            )
    }

    /// Hands a refused token to the GitHub tab.
    pub(crate) fn note_error(&self, error: &GithubError, cx: &mut App) {
        report_if_revoked(error, &self.context.session, cx);
    }

    pub fn missing_scope(&self) -> bool {
        self.missing_scope
    }

    pub fn load_state(&self) -> &ProjectsLoad {
        &self.load
    }

    pub fn projects(&self) -> &[ProjectV2] {
        &self.projects
    }

    pub fn selected_project(&self) -> Option<&ProjectV2> {
        let selected = self.selected_project.as_deref()?;
        self.projects.iter().find(|project| project.id == selected)
    }

    pub fn details(&self) -> Option<&ProjectDetails> {
        self.details.as_ref()
    }

    /// The view picked, else the project's first.
    pub fn view(&self) -> Option<&ProjectView> {
        let views = &self.details.as_ref()?.views;
        self.selected_view
            .and_then(|number| views.iter().find(|view| view.number == number))
            .or_else(|| views.first())
    }

    pub fn items(&self) -> &[ProjectV2Item] {
        &self.items
    }

    /// More items exist than were read.
    pub fn has_more_items(&self) -> bool {
        self.items_cursor.is_some()
    }

    /// Items are the project's, not the view's, so nothing is read.
    pub fn select_view(&mut self, number: u64, cx: &mut Context<Self>) {
        self.selected_view = Some(number);
        cx.notify();
    }

    /// An issue opens in its own tab, a pull request on GitHub; a draft or a
    /// hidden item has nowhere to go.
    pub fn open_item(&self, index: usize, window: &mut Window, cx: &mut App) {
        let Some(item) = self.items.get(index) else {
            return;
        };
        match &item.content {
            ProjectItemContent::Issue {
                number, repository, ..
            } => {
                let Some(repository) = parse_repository(repository) else {
                    return;
                };
                let Some(workspace) = Workspace::for_window(window, cx) else {
                    return;
                };
                let key = DetailKey {
                    repository,
                    number: *number,
                };
                let backend = self.context.backend.clone();
                let session = self.context.session.clone();
                // Called from a card's click, while this section is being
                // updated; the tab is opened once that is over.
                window.defer(cx, move |window, cx| {
                    workspace.update(cx, |workspace, cx| {
                        open_issue_detail(workspace, backend, session, key, window, cx);
                    });
                });
            }
            ProjectItemContent::PullRequest {
                number, repository, ..
            } => {
                if let Some(repository) = parse_repository(repository) {
                    cx.open_url(&format!("https://github.com/{repository}/pull/{number}"));
                }
            }
            ProjectItemContent::DraftIssue { .. } | ProjectItemContent::Redacted => {}
        }
    }
}

/// `owner/name` as GitHub sent it, checked before it goes in a URL.
fn parse_repository(name_with_owner: &str) -> Option<RepositoryId> {
    let (owner, name) = name_with_owner.split_once('/')?;
    RepositoryId::new(owner, name)
}
