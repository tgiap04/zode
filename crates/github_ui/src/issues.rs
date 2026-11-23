//! The Issues section: the list with its filters and search, and an issue's
//! own tab.

mod assignee_picker;
mod assignee_picker_render;
mod bulk_issue_actions;
mod bulk_issue_dialog_render;
mod epic_tree;
mod issue_actions;
mod issue_detail_render;
mod issue_detail_session;
mod issue_detail_view;
mod issue_detail_write_render;
mod issue_detail_writes;
mod issue_editor;
mod issue_editor_properties_render;
mod issue_editor_render;
mod issue_filters;
mod issue_list;
mod issue_list_render;
mod issue_list_selection_render;
mod issue_list_states_render;
mod issue_list_writes;
mod issue_row_render;
mod issue_type_picker;
mod sub_issue_section;
mod sub_issue_section_render;

pub use bulk_issue_actions::{BulkIssueDialog, BulkItem, BulkItemStatus, BulkPhase};
pub use issue_actions::{BulkIssueAction, IssueMenuEntry};
pub use issue_detail_view::{IssueDetailView, open_issue_detail};
pub use issue_editor::{CreateIssueDialog, IssueDraftText};
pub use issue_filters::{IssueListFilters, MineFilter, label_color, matches_query};
pub use issue_list::{IssueList, IssueListEvent, ListLoad, ListPlaceholder};
pub use issue_list_render::updated_label;

// Shared with the pull request tab, which writes and shows comments alike.
pub(crate) use issue_actions::target_line;
pub(crate) use issue_detail_write_render::callout;
pub(crate) use issue_editor::{markdown_editor, single_line_editor};
pub(crate) use issue_editor_render::{composer_field, editor_field, render_target};
pub(crate) use issue_row_render::render_label_chip;
pub(crate) use issue_type_picker::named_color;

#[cfg(test)]
mod assignee_picker_tests;
#[cfg(test)]
mod bulk_issue_tests;
#[cfg(test)]
mod epic_tests;
#[cfg(test)]
mod issue_actions_tests;
#[cfg(test)]
mod issue_detail_tests;
#[cfg(test)]
mod issue_list_tests;
#[cfg(test)]
mod issue_write_race_tests;
#[cfg(test)]
mod issue_write_tests;
#[cfg(test)]
pub(crate) mod issues_test_support;
