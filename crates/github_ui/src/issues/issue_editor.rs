//! The "New issue" dialog, and the markdown editors the issue tab writes
//! with.

use std::sync::Arc;

use editor::{Editor, EditorEvent};
use github::{
    GithubBackend, GithubError, GithubUser, IssueType, Label, Milestone, NewIssue, RepositoryId,
    RepositoryVisibility,
};
use gpui::{
    App, AppContext as _, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    SharedString, Subscription, Task, WeakEntity, Window,
};
use language::LanguageRegistry;
use workspace::{DismissDecision, ModalView};

use crate::github_view::GithubContext;
use crate::github_view_session::{
    SessionStamp, is_live_session, report_if_revoked, session_ended_message,
};
use crate::issues::IssueDetailView;
use crate::issues::assignee_picker::{AssigneePicker, AssigneePickerEvent, AssigneeSource};
use crate::issues::issue_actions::{ignored_fields, ignored_notice, target_line};
use crate::issues::issue_list::IssueList;
use crate::markdown_composer::{MarkdownComposer, PreviewSource};

/// A multi-line editor for GitHub-flavoured markdown, highlighted when the
/// Markdown grammar is loaded.
pub(crate) fn markdown_editor(
    min_lines: usize,
    placeholder: &str,
    text: &str,
    languages: Option<Arc<LanguageRegistry>>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    let editor = cx.new(|cx| {
        let mut editor = Editor::auto_height(min_lines, 24, window, cx);
        editor.set_soft_wrap();
        editor.set_placeholder_text(placeholder, window, cx);
        editor.set_text(text, window, cx);
        editor
    });
    let buffer = editor.read(cx).buffer().read(cx).as_singleton();
    if let (Some(languages), Some(buffer)) = (languages, buffer) {
        let language = languages.language_for_name("Markdown");
        cx.spawn(async move |cx| match language.await {
            Ok(language) => {
                buffer.update(cx, |buffer, cx| buffer.set_language(Some(language), cx));
            }
            Err(error) => log::debug!("the issue editor has no markdown grammar: {error}"),
        })
        .detach();
    }
    editor
}

pub(crate) fn single_line_editor(
    placeholder: &str,
    text: &str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    cx.new(|cx| {
        let mut editor = Editor::single_line(window, cx);
        editor.set_placeholder_text(placeholder, window, cx);
        editor.set_text(text, window, cx);
        editor
    })
}

/// What was put into a "New issue" dialog closed without creating it, so
/// the next one opens with it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IssueDraftText {
    pub title: String,
    pub body: String,
    pub assignees: Vec<GithubUser>,
}

/// What the list hands the dialog: the choices its filters already read,
/// and the text left from the last dialog.
pub(crate) struct CreateIssueOptions {
    pub labels: Vec<Label>,
    pub milestones: Vec<Milestone>,
    /// Empty when the repository's owner has no issue types.
    pub issue_types: Vec<IssueType>,
    pub draft: IssueDraftText,
    pub languages: Option<Arc<LanguageRegistry>>,
}

/// Who writes, and where.
#[derive(Clone)]
pub(crate) struct IssueTarget {
    pub backend: Arc<dyn GithubBackend>,
    pub repository: RepositoryId,
    pub login: SharedString,
    pub session: SessionStamp,
}

impl From<&GithubContext> for IssueTarget {
    fn from(context: &GithubContext) -> Self {
        Self {
            backend: context.backend.clone(),
            repository: context.repository.clone(),
            login: context.login.clone(),
            session: context.session.clone(),
        }
    }
}

impl IssueTarget {
    pub(crate) fn assignee_source(&self) -> AssigneeSource {
        AssigneeSource {
            backend: self.backend.clone(),
            repository: self.repository.clone(),
            session: self.session.clone(),
        }
    }
}

/// What opened the dialog, which hears about the created issue.
pub(crate) enum CreateIssueOwner {
    List(WeakEntity<IssueList>),
    /// The created issue is then added as a sub-issue of `parent_number`.
    SubIssueOf {
        detail: WeakEntity<IssueDetailView>,
        parent_number: u64,
    },
}

pub const TITLE_REQUIRED: &str = "A title is required.";

pub struct CreateIssueDialog {
    pub(crate) target: IssueTarget,
    owner: CreateIssueOwner,
    pub(crate) title: Entity<Editor>,
    pub(crate) body: Entity<Editor>,
    /// Write and Preview tabs and the toolbar, around `body`.
    pub(crate) description: Entity<MarkdownComposer>,
    pub(crate) assignee_picker: Entity<AssigneePicker>,
    pub(crate) labels: Vec<Label>,
    pub(crate) milestones: Vec<Milestone>,
    pub(crate) issue_types: Vec<IssueType>,
    pub(crate) chosen_labels: Vec<String>,
    pub(crate) chosen_assignees: Vec<GithubUser>,
    pub(crate) milestone: Option<u64>,
    /// The chosen issue type's node id.
    pub(crate) issue_type: Option<String>,
    pub(crate) visibility: Option<RepositoryVisibility>,
    pub(crate) visibility_error: Option<GithubError>,
    /// Shown under the title field.
    pub(crate) title_error: Option<&'static str>,
    pub(crate) error: Option<String>,
    pub(crate) submitting: bool,
    created: bool,
    _visibility_task: Task<()>,
    submit_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DismissEvent> for CreateIssueDialog {}

impl Focusable for CreateIssueDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.title.focus_handle(cx)
    }
}

impl ModalView for CreateIssueDialog {
    /// Kept open while GitHub is creating the issue; closed otherwise, with
    /// what was typed handed back to the list for the next dialog.
    fn on_before_dismiss(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> DismissDecision {
        if self.submitting {
            return DismissDecision::Dismiss(false);
        }
        if let CreateIssueOwner::List(list) = &self.owner {
            let draft = (!self.created).then(|| self.draft_text(cx));
            list.update(cx, |list, _| list.saved_draft = draft.unwrap_or_default())
                .ok();
        }
        DismissDecision::Dismiss(true)
    }
}

impl CreateIssueDialog {
    pub(crate) fn new(
        target: IssueTarget,
        owner: CreateIssueOwner,
        options: CreateIssueOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let CreateIssueOptions {
            labels,
            milestones,
            issue_types,
            draft,
            languages,
        } = options;
        let title = single_line_editor("What is this about?", &draft.title, window, cx);
        let body = markdown_editor(
            6,
            "Describe the problem or the idea. Markdown is supported.",
            &draft.body,
            languages,
            window,
            cx,
        );
        let description = cx.new(|_| {
            MarkdownComposer::new(
                body.clone(),
                PreviewSource {
                    repository: target.repository.clone(),
                    backend: target.backend.clone(),
                    session: target.session.clone(),
                },
            )
        });
        let source = target.assignee_source();
        let assignee_picker = cx.new(|cx| AssigneePicker::new(source, window, cx));
        let title_edited = cx.subscribe(&title, |this, _, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::BufferEdited) {
                this.title_error = None;
                cx.notify();
            }
        });
        let picked = cx.subscribe(
            &assignee_picker,
            |this, _, event: &AssigneePickerEvent, cx| {
                let AssigneePickerEvent::Picked(user) = event;
                this.toggle_assignee(user.clone(), cx);
            },
        );
        let backend = target.backend.clone();
        let repository = target.repository.clone();
        let visibility_task = cx.spawn(async move |this, cx| {
            let visibility = backend.repository_visibility(&repository).await;
            this.update(cx, |this, cx| {
                match visibility {
                    Ok(visibility) => this.visibility = Some(visibility),
                    Err(error) => {
                        report_if_revoked(&error, &this.target.session, cx);
                        this.visibility_error = Some(error);
                    }
                }
                cx.notify();
            })
            .ok();
        });
        let mut this = Self {
            target,
            owner,
            title,
            body,
            description,
            assignee_picker,
            labels,
            milestones,
            issue_types: issue_types
                .into_iter()
                .filter(|issue_type| issue_type.is_enabled)
                .collect(),
            chosen_labels: Vec::new(),
            chosen_assignees: draft.assignees,
            milestone: None,
            issue_type: None,
            visibility: None,
            visibility_error: None,
            title_error: None,
            error: None,
            submitting: false,
            created: false,
            _visibility_task: visibility_task,
            submit_task: None,
            _subscriptions: vec![title_edited, picked],
        };
        this.sync_picker(cx);
        this
    }

    pub fn target_line(&self) -> String {
        target_line(&self.target.login, &self.target.repository, self.visibility)
    }

    pub fn error(&self) -> Option<&str> {
        self.title_error.or(self.error.as_deref())
    }

    /// The primary button is live only with a title, a confirmed repository
    /// and nothing on its way.
    pub fn can_create(&self, cx: &App) -> bool {
        !self.submitting
            && self.visibility.is_some()
            && !self.title.read(cx).text(cx).trim().is_empty()
    }

    pub fn chosen_assignees(&self) -> &[GithubUser] {
        &self.chosen_assignees
    }

    pub fn assignee_picker(&self) -> &Entity<AssigneePicker> {
        &self.assignee_picker
    }

    pub fn toggle_label(&mut self, name: String, cx: &mut Context<Self>) {
        if let Some(index) = self.chosen_labels.iter().position(|chosen| *chosen == name) {
            self.chosen_labels.remove(index);
        } else {
            self.chosen_labels.push(name);
        }
        cx.notify();
    }

    /// Adds `user` to the assignees, or takes them off when already there.
    pub fn toggle_assignee(&mut self, user: GithubUser, cx: &mut Context<Self>) {
        if let Some(index) = self
            .chosen_assignees
            .iter()
            .position(|chosen| chosen.login == user.login)
        {
            self.chosen_assignees.remove(index);
        } else {
            self.chosen_assignees.push(user);
        }
        self.sync_picker(cx);
        cx.notify();
    }

    fn sync_picker(&mut self, cx: &mut Context<Self>) {
        let logins = self
            .chosen_assignees
            .iter()
            .map(|user| user.login.clone())
            .collect();
        self.assignee_picker.update(cx, |picker, cx| {
            picker.set_selected(logins);
            cx.notify();
        });
    }

    pub fn set_milestone(&mut self, milestone: Option<u64>, cx: &mut Context<Self>) {
        self.milestone = milestone;
        cx.notify();
    }

    /// An issue type's node id; `None` leaves the issue without a type.
    pub fn set_issue_type(&mut self, issue_type: Option<String>, cx: &mut Context<Self>) {
        self.issue_type = issue_type;
        cx.notify();
    }

    pub(crate) fn draft_text(&self, cx: &App) -> IssueDraftText {
        IssueDraftText {
            title: self.title.read(cx).text(cx),
            body: self.body.read(cx).text(cx),
            assignees: self.chosen_assignees.clone(),
        }
    }

    pub fn draft(&self, cx: &App) -> NewIssue {
        let text = self.draft_text(cx);
        NewIssue {
            title: text.title.trim().to_string(),
            body: (!text.body.trim().is_empty()).then_some(text.body),
            labels: self.chosen_labels.clone(),
            assignees: text.assignees.into_iter().map(|user| user.login).collect(),
            milestone: self.milestone,
        }
    }

    /// Sends the issue once, through the token's write queue, then sets its
    /// type when one was chosen. On failure the dialog stays open with
    /// everything typed, and says why.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.submitting {
            return;
        }
        let draft = self.draft(cx);
        if draft.title.is_empty() {
            self.title_error = Some(TITLE_REQUIRED);
            cx.notify();
            return;
        }
        if !is_live_session(&self.target.session, cx) {
            self.error = Some(session_ended_message("dialog"));
            cx.notify();
            return;
        }
        if self.visibility.is_none() {
            self.error = Some(
                "Zode has not confirmed which repository this goes to yet; try again in a moment."
                    .to_string(),
            );
            cx.notify();
            return;
        }
        self.submitting = true;
        self.error = None;
        let backend = self.target.backend.clone();
        let repository = self.target.repository.clone();
        let login = self.target.login.clone();
        let parent_number = match &self.owner {
            CreateIssueOwner::List(_) => None,
            CreateIssueOwner::SubIssueOf { parent_number, .. } => Some(*parent_number),
        };
        let issue_type = self.issue_type.as_ref().and_then(|wanted| {
            self.issue_types
                .iter()
                .find(|issue_type| &issue_type.id == wanted)
                .cloned()
        });
        self.submit_task = Some(cx.spawn(async move |this, cx| {
            let mut created = backend.create_issue(&repository, draft.clone()).await;
            let type_applied = match (&mut created, issue_type) {
                (Ok(issue), Some(issue_type)) => {
                    let set = backend
                        .set_issue_type(issue.node_id.clone(), Some(issue_type.id.clone()))
                        .await;
                    if set.is_ok() {
                        issue.issue_type = Some(issue_type.name);
                    }
                    set.err()
                }
                _ => None,
            };
            let linked = match (&created, parent_number) {
                (Ok(issue), Some(parent_number)) => Some(
                    backend
                        .add_sub_issue(&repository, parent_number, issue.id)
                        .await,
                ),
                _ => None,
            };
            this.update(cx, |this, cx| {
                this.submitting = false;
                match created {
                    Ok(issue) => {
                        let mut ignored = ignored_fields(&draft, &issue);
                        if let Some(error) = &type_applied {
                            report_if_revoked(error, &this.target.session, cx);
                            ignored.push("type");
                        }
                        let notice = ignored_notice(&ignored, &login, &repository);
                        this.created = true;
                        match &this.owner {
                            CreateIssueOwner::List(list) => {
                                list.update(cx, |list, cx| list.insert_created(issue, notice, cx))
                                    .ok();
                            }
                            CreateIssueOwner::SubIssueOf { detail, .. } => {
                                let linked = linked.unwrap_or(Ok(()));
                                detail
                                    .update(cx, |detail, cx| {
                                        detail.sub_issue_created(issue, linked, notice, cx)
                                    })
                                    .ok();
                            }
                        }
                        cx.emit(DismissEvent);
                    }
                    Err(error) => {
                        report_if_revoked(&error, &this.target.session, cx);
                        this.error = Some(error.to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}
