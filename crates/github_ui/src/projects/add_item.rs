//! Adding an existing issue or pull request of the repository to the
//! project, then, if asked, setting its status: two writes, since GitHub
//! cannot add and set in one call.

use editor::Editor;
use github::{
    GithubError, GithubResult, IssueFilter, IssueStateFilter, ProjectFieldKind, ProjectItemContent,
    ProjectV2Item, PullRequestStateFilter,
};
use gpui::{Context, Entity, SharedString, Task};

use crate::github_view_session::report_if_revoked;
use crate::projects::move_item::{FieldWriteOrigin, value_for};
use crate::projects::project_layout::ColumnKey;
use crate::projects::projects_section::ProjectsSection;

/// An issue or pull request of the repository that may be added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddCandidate {
    /// GraphQL node id.
    pub content_id: String,
    pub number: u64,
    pub title: String,
    pub pull_request: bool,
}

#[derive(Default)]
pub struct AddItemPicker {
    /// Made on the first draw, which has the window an editor needs.
    pub field: Option<Entity<Editor>>,
    pub candidates: Vec<AddCandidate>,
    /// The option of the status field the added item gets, if any.
    pub status: Option<String>,
    pub searching: bool,
    pub adding: bool,
    pub message: Option<SharedString>,
    pub(super) task: Option<Task<()>>,
}

impl ProjectsSection {
    pub fn add_item_picker(&self) -> Option<&AddItemPicker> {
        self.add_item.as_ref()
    }

    pub fn add_item_message(&self) -> Option<&str> {
        self.add_item
            .as_ref()?
            .message
            .as_ref()
            .map(|message| message.as_ref())
    }

    /// The single select an added item's status is set in: the board's
    /// column field, which falls back to `Status`.
    pub(super) fn status_field(&self) -> Option<&github::ProjectField> {
        self.column_field()
            .filter(|field| matches!(field.kind, ProjectFieldKind::SingleSelect(_)))
    }

    pub fn open_add_item(&mut self, cx: &mut Context<Self>) {
        if self.selected_project.is_none() || !self.may_write(cx) {
            return;
        }
        self.add_item = Some(AddItemPicker::default());
        cx.notify();
    }

    pub fn close_add_item(&mut self, cx: &mut Context<Self>) {
        self.add_item = None;
        cx.notify();
    }

    /// `#12` or `12` reads that pull request or issue; words search the
    /// repository's issues and match the titles of its open pull requests.
    /// What the project already holds is left out.
    pub fn search_add_item(&mut self, text: String, cx: &mut Context<Self>) {
        let text = text.trim().to_string();
        let in_project = self.numbers_in_project();
        let Some(picker) = &mut self.add_item else {
            return;
        };
        if text.is_empty() {
            return;
        }
        picker.searching = true;
        picker.message = None;
        let number = text.trim_start_matches('#').parse::<u64>().ok();
        let backend = self.context.backend.clone();
        let repository = self.context.repository.clone();
        picker.task = Some(cx.spawn(async move |this, cx| {
            let found: GithubResult<Vec<AddCandidate>> = match number {
                Some(number) => match backend.pull_request(&repository, number).await {
                    Ok(pull_request) => Ok(vec![AddCandidate {
                        content_id: pull_request.id,
                        number,
                        title: pull_request.title,
                        pull_request: true,
                    }]),
                    // Only "no such pull request" means the number may be an
                    // issue; a refused token or a lost connection is told as
                    // it is, not answered by a second request.
                    Err(GithubError::NotFound) => {
                        backend.issue(&repository, number).await.map(|issue| {
                            vec![AddCandidate {
                                content_id: issue.node_id,
                                number,
                                title: issue.title,
                                pull_request: false,
                            }]
                        })
                    }
                    Err(error) => Err(error),
                },
                None => {
                    let filter = IssueFilter {
                        state: IssueStateFilter::All,
                        ..IssueFilter::default()
                    };
                    let issues = backend.search_issues(&repository, &filter, &text, None);
                    let pull_requests =
                        backend.list_pull_requests(&repository, PullRequestStateFilter::Open, None);
                    let words = text
                        .split_whitespace()
                        .map(str::to_lowercase)
                        .collect::<Vec<_>>();
                    match (issues.await, pull_requests.await) {
                        (Ok(issues), Ok(pull_requests)) => {
                            let mut found = issues
                                .items
                                .into_iter()
                                .map(|issue| AddCandidate {
                                    content_id: issue.node_id,
                                    number: issue.number,
                                    title: issue.title,
                                    pull_request: false,
                                })
                                .collect::<Vec<_>>();
                            found.extend(
                                pull_requests
                                    .items
                                    .into_iter()
                                    .filter(|pull_request| {
                                        let title = pull_request.title.to_lowercase();
                                        words.iter().all(|word| title.contains(word.as_str()))
                                    })
                                    .map(|pull_request| AddCandidate {
                                        content_id: pull_request.id,
                                        number: pull_request.number,
                                        title: pull_request.title,
                                        pull_request: true,
                                    }),
                            );
                            Ok(found)
                        }
                        (Err(error), _) | (_, Err(error)) => Err(error),
                    }
                }
            };
            this.update(cx, |this, cx| {
                let repository = this.context.repository.clone();
                let Some(picker) = &mut this.add_item else {
                    return;
                };
                picker.searching = false;
                match found {
                    Ok(found) => {
                        let any_found = !found.is_empty();
                        picker.candidates = found
                            .into_iter()
                            .filter(|candidate| !in_project.contains(&candidate.number))
                            .collect();
                        picker.message = match (any_found, picker.candidates.is_empty()) {
                            (false, _) => {
                                Some(format!("Nothing in {repository} matches “{text}”."))
                            }
                            (true, true) => {
                                Some("What was found is already in the project.".to_string())
                            }
                            (true, false) => None,
                        }
                        .map(SharedString::from);
                    }
                    Err(error) => {
                        report_if_revoked(&error, &this.context.session, cx);
                        picker.candidates.clear();
                        picker.message = Some(error.to_string().into());
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Issue and pull request numbers of this repository the project holds.
    fn numbers_in_project(&self) -> Vec<u64> {
        let repository = self.context.repository.to_string();
        self.items
            .iter()
            .filter_map(|item| match &item.content {
                ProjectItemContent::Issue {
                    number,
                    repository: of,
                    ..
                }
                | ProjectItemContent::PullRequest {
                    number,
                    repository: of,
                    ..
                } if *of == repository => Some(*number),
                _ => None,
            })
            .collect()
    }

    pub fn set_add_item_status(&mut self, option_id: Option<String>, cx: &mut Context<Self>) {
        if let Some(picker) = &mut self.add_item {
            picker.status = option_id;
            cx.notify();
        }
    }

    /// Adds `candidate`, shows it, then sets the status picked, if any.
    pub fn add_item(&mut self, candidate: AddCandidate, cx: &mut Context<Self>) {
        let Some(project_id) = self.selected_project.clone() else {
            return;
        };
        if !self.may_write(cx) {
            return;
        }
        let Some(picker) = &mut self.add_item else {
            return;
        };
        if picker.adding {
            return;
        }
        picker.adding = true;
        picker.message = None;
        let status = picker.status.clone();
        let write = self
            .context
            .backend
            .add_project_item(project_id.clone(), candidate.content_id.clone());
        cx.spawn(async move |this, cx| {
            let result = write.await;
            this.update(cx, |this, cx| {
                // Another project was picked meanwhile: what it shows, and
                // the status write, belong to that one.
                if this.selected_project.as_deref() != Some(project_id.as_str()) {
                    if let Some(picker) = &mut this.add_item {
                        picker.adding = false;
                    }
                    if let Err(error) = result {
                        this.note_error(&error, cx);
                        this.note_write_error(&error);
                        this.move_message = Some(SharedString::from(format!(
                            "Could not add #{} to the project picked before: {error}.",
                            candidate.number
                        )));
                    }
                    cx.notify();
                    return;
                }
                match result {
                    Ok(item_id) => this.item_added(item_id, candidate, status, cx),
                    Err(error) => {
                        this.note_error(&error, cx);
                        this.note_write_error(&error);
                        if let Some(picker) = &mut this.add_item {
                            picker.adding = false;
                            picker.message = Some(SharedString::from(format!(
                                "Could not add #{}: {error}.",
                                candidate.number
                            )));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn item_added(
        &mut self,
        item_id: String,
        candidate: AddCandidate,
        status: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.add_item = None;
        if !self.items.iter().any(|item| item.id == item_id) {
            let repository = self.context.repository.to_string();
            let content = if candidate.pull_request {
                ProjectItemContent::PullRequest {
                    number: candidate.number,
                    title: candidate.title,
                    repository,
                }
            } else {
                ProjectItemContent::Issue {
                    number: candidate.number,
                    title: candidate.title,
                    repository,
                }
            };
            self.layout_changed();
            self.note_item_added(&item_id);
            self.items.push(ProjectV2Item {
                id: item_id.clone(),
                content,
                assignees: Vec::new(),
                assignee_avatars: Default::default(),
                state: None,
                sub_issues: None,
                labels: Vec::new(),
                field_values: Vec::new(),
            });
        }
        let Some(option_id) = status else {
            return;
        };
        let Some(field) = self.status_field().cloned() else {
            return;
        };
        if let Some(write) = value_for(&field, &ColumnKey::Option(option_id)) {
            self.send_field_write(&item_id, &field.id, write, FieldWriteOrigin::Choice, cx);
        }
    }
}
