//! Taking an item out of the project, after a confirmation; the issue or
//! pull request itself stays.

use github::ProjectItemContent;
use gpui::{Context, SharedString};

use crate::projects::project_layout::item_title;
use crate::projects::projects_section::ProjectsSection;

impl ProjectsSection {
    pub fn pending_removal(&self) -> Option<&str> {
        self.pending_removal.as_deref()
    }

    /// What the confirmation asks, saying what happens to the issue.
    pub fn removal_question(&self) -> Option<String> {
        let item_id = self.pending_removal.as_deref()?;
        let item = self.items.iter().find(|item| item.id == item_id)?;
        let title = item_title(item);
        let fate = match &item.content {
            ProjectItemContent::Issue { repository, .. } => {
                format!("The issue is not deleted; it stays in {repository}.")
            }
            ProjectItemContent::PullRequest { repository, .. } => {
                format!("The pull request is not deleted; it stays in {repository}.")
            }
            ProjectItemContent::DraftIssue { .. } => {
                "A draft lives only in the project, so it is deleted with it.".to_string()
            }
            ProjectItemContent::Redacted => "The item itself is not deleted.".to_string(),
        };
        Some(format!("Remove “{title}” from the project? {fate}"))
    }

    /// Asks first; nothing is sent until [`Self::confirm_remove_item`].
    pub fn request_remove_item(&mut self, item_id: &str, cx: &mut Context<Self>) {
        if !self.items.iter().any(|item| item.id == item_id) || !self.may_write(cx) {
            return;
        }
        self.pending_removal = Some(item_id.to_string());
        cx.notify();
    }

    pub fn cancel_remove_item(&mut self, cx: &mut Context<Self>) {
        self.pending_removal = None;
        cx.notify();
    }

    /// Takes the item out at once; a failure puts it back where it was.
    pub fn confirm_remove_item(&mut self, cx: &mut Context<Self>) {
        let (Some(project_id), Some(item_id)) =
            (self.selected_project.clone(), self.pending_removal.take())
        else {
            return;
        };
        cx.notify();
        let Some(index) = self.items.iter().position(|item| item.id == item_id) else {
            return;
        };
        let removed = self.items.remove(index);
        self.note_removal_sent(&item_id);
        self.layout_changed();
        if self.selected_card.as_deref() == Some(item_id.as_str()) {
            self.selected_card = None;
        }
        if self
            .field_edit
            .as_ref()
            .is_some_and(|edit| edit.item_id == item_id)
        {
            self.field_edit = None;
        }
        self.move_message = None;
        let write = self
            .context
            .backend
            .delete_project_item(project_id.clone(), item_id.clone());
        cx.spawn(async move |this, cx| {
            let result = write.await;
            this.update(cx, |this, cx| {
                let same_project = this.selected_project.as_deref() == Some(project_id.as_str());
                if result.is_ok() && same_project {
                    this.note_removal_landed(&item_id);
                }
                if let Err(error) = result {
                    this.note_error(&error, cx);
                    this.note_write_error(&error);
                    let title = item_title(&removed).to_string();
                    // Another project may be shown by now, or the item read
                    // back in with it: only put it back where it is missing.
                    this.move_message = Some(SharedString::from(if same_project {
                        format!("Could not remove “{title}”: {error}. It is back in the project.")
                    } else {
                        format!(
                            "Could not remove “{title}” from the project picked before: {error}."
                        )
                    }));
                    if same_project && !this.items.iter().any(|item| item.id == removed.id) {
                        let at = index.min(this.items.len());
                        this.note_item_added(&removed.id);
                        this.items.insert(at, removed);
                        this.layout_changed();
                    }
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}
