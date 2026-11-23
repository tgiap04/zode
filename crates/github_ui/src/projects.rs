//! The Projects section (read only): a project linked to the repository,
//! one of its saved views, drawn as a board or a table.

mod add_item;
mod add_item_render;
mod board_card_render;
mod board_view;
mod card_menu;
mod create_project;
mod create_project_render;
mod drag_state;
mod field_editor;
mod field_editor_render;
mod move_item;
mod project_layout;
mod project_reads;
mod projects_render;
mod projects_section;
mod remove_item;
mod table_view;

pub use project_layout::{
    BoardColumn, ColumnKey, board_columns, cell_text, column_field, item_title, shows_board,
    sorted_items, table_fields,
};
pub use projects_render::MISSING_SCOPE_GUIDANCE;
pub use projects_section::{
    PROJECT_ITEM_CAP, PROJECT_READ_SCOPE, ProjectsLoad, ProjectsSection, ProjectsSectionEvent,
};

use gpui::{AppContext as _, Context, Entity};

use crate::github_view::{AuthState, GithubSection, GithubView};

impl GithubView {
    pub fn projects_section(&self) -> Option<&Entity<ProjectsSection>> {
        self.projects_section.as_ref()
    }

    /// Keeps the Projects section in step with the account and repository:
    /// dropped while either is missing, made (and read) when the section is
    /// shown for a new pair. Nothing is read before the section is shown.
    pub(crate) fn sync_projects_section(&mut self, cx: &mut Context<Self>) {
        let (Some(context), AuthState::SignedIn(session)) = (self.context(), &self.auth) else {
            self.projects_section = None;
            self._projects_section_subscription = None;
            return;
        };
        if self
            .projects_section
            .as_ref()
            .is_some_and(|section| section.read(cx).serves(&context))
        {
            return;
        }
        if self.section != GithubSection::Projects {
            self.projects_section = None;
            self._projects_section_subscription = None;
            return;
        }
        let scopes = session.scopes.clone();
        let section = cx.new(|cx| {
            let mut section = ProjectsSection::new(context, scopes, cx);
            section.reload(cx);
            section
        });
        self._projects_section_subscription = Some(cx.subscribe(
            &section,
            |this, section: Entity<ProjectsSection>, event, cx| match event {
                ProjectsSectionEvent::Loaded => {
                    this.rate_limits = section.read(cx).context.backend.rate_limits();
                    cx.notify();
                }
                ProjectsSectionEvent::SignInWithGithub => this.sign_in_with_github(cx),
                ProjectsSectionEvent::CheckAgain => this.connect(cx),
            },
        ));
        self.projects_section = Some(section);
    }
}

#[cfg(test)]
mod add_item_tests;
#[cfg(test)]
mod create_project_tests;
#[cfg(test)]
mod field_editor_tests;
#[cfg(test)]
mod move_item_tests;
#[cfg(test)]
mod project_layout_tests;
#[cfg(test)]
mod projects_section_tests;
