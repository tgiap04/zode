use github::{
    DetectedRepository, RemoteName, RepositoryId, RepositoryRemotes, RepositoryVisibility,
    detect_repositories,
};
use gpui::{App, Context};
use project::Project;
use project::git_store::{GitStoreEvent, RepositoryEvent};
use ui::{Chip, prelude::*};

use crate::github_view::GithubView;

pub const NO_REPOSITORY_EXPLANATION: &str = "None of this project's git remotes (origin or upstream) points at a repository on github.com. Remotes through an SSH host alias such as `git@github.com-work:` are not recognised; use `git@github.com:` or an https URL.";

/// The github.com repositories behind the project's git repositories, in a
/// stable order: by working directory, origin before upstream.
pub fn project_repositories(project: &Project, cx: &App) -> Vec<DetectedRepository> {
    let git_store = project.git_store().read(cx);
    let mut repositories = git_store
        .repositories()
        .values()
        .map(|repository| {
            let repository = repository.read(cx);
            (
                repository.work_directory_abs_path.clone(),
                RepositoryRemotes {
                    origin_url: repository.remote_origin_url.clone(),
                    upstream_url: repository.remote_upstream_url.clone(),
                },
            )
        })
        .collect::<Vec<_>>();
    repositories.sort_by(|(left, _), (right, _)| left.cmp(right));
    let remotes = repositories
        .into_iter()
        .map(|(_, remotes)| remotes)
        .collect::<Vec<_>>();
    detect_repositories(&remotes)
}

/// Whether `event` can change which repositories the project has or what
/// their remotes are. Statuses, stashes, conflicts and the like arrive by the
/// hundred while files change, and none of them touches either.
pub(crate) fn can_change_repositories(event: &GitStoreEvent) -> bool {
    match event {
        GitStoreEvent::RepositoryAdded
        | GitStoreEvent::RepositoryRemoved(_)
        // Remotes are read again by a reload, which announces a change to
        // them only by the job it ran in ending.
        | GitStoreEvent::JobsUpdated => true,
        GitStoreEvent::RepositoryUpdated(_, event, _) => matches!(
            event,
            RepositoryEvent::HeadChanged
                | RepositoryEvent::BranchListChanged
                | RepositoryEvent::GitWorktreeListChanged
        ),
        GitStoreEvent::ActiveRepositoryChanged(_)
        | GitStoreEvent::IndexWriteError(_)
        | GitStoreEvent::ConflictsUpdated => false,
    }
}

/// Keeps the user's pick while it is still offered, else the first one.
pub fn choose_active(
    choices: &[DetectedRepository],
    current: Option<&RepositoryId>,
) -> Option<RepositoryId> {
    current
        .filter(|current| choices.iter().any(|choice| &choice.repository == *current))
        .cloned()
        .or_else(|| choices.first().map(|choice| choice.repository.clone()))
}

pub fn visibility_label(visibility: RepositoryVisibility) -> &'static str {
    match visibility {
        RepositoryVisibility::Public => "Public",
        RepositoryVisibility::Private => "Private",
        RepositoryVisibility::Internal => "Internal",
    }
}

fn remote_label(remote: RemoteName) -> &'static str {
    match remote {
        RemoteName::Origin => "origin",
        RemoteName::Upstream => "upstream",
    }
}

impl GithubView {
    pub(crate) fn render_repository_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.repositories.is_empty() {
            return Label::new(NO_REPOSITORY_EXPLANATION)
                .size(LabelSize::Small)
                .color(Color::Muted)
                .into_any_element();
        }
        let visibility = self
            .active_repository
            .as_ref()
            .and_then(|repository| self.visibility.get(repository))
            .copied();
        h_flex()
            .flex_wrap()
            .gap_1()
            .children(self.repositories.iter().enumerate().map(|(index, choice)| {
                let repository = choice.repository.clone();
                let is_active = self.active_repository.as_ref() == Some(&repository);
                Button::new(
                    ("github-repository", index),
                    format!("{} ({})", repository, remote_label(choice.remote)),
                )
                .label_size(LabelSize::Small)
                .toggle_state(is_active)
                .on_click(cx.listener(move |this, _, _window, cx| {
                    this.select_repository(repository.clone(), cx)
                }))
            }))
            .when_some(visibility, |row, visibility| {
                row.child(Chip::new(visibility_label(visibility)))
            })
            .into_any_element()
    }
}
