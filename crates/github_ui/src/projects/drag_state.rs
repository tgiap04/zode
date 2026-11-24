//! Dragging a card to another column, or to another place in its column.

use gpui::{Context, IntoElement, Render, SharedString, Window};
use ui::prelude::*;

use crate::projects::project_layout::ColumnKey;
use crate::projects::projects_section::ProjectsSection;

#[derive(Clone, Debug)]
pub struct DraggedProjectCard {
    pub item_id: String,
    pub title: SharedString,
}

impl Render for DraggedProjectCard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(rems(16.))
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors().border_focused)
            .bg(cx.theme().colors().editor_background)
            .child(Label::new(self.title.clone()).truncate())
    }
}

/// Where a card was let go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardDrop {
    /// On a column's empty space: the column's value, the place kept.
    Column(ColumnKey),
    /// On a card: that card's column, and its place when the view allows it.
    Card(String),
}

impl ProjectsSection {
    /// On another column: that column's value. On a card: that card's
    /// column, and the card's place when the view has no sort. Within a
    /// column under a sort nothing happens.
    pub fn drop_card(
        &mut self,
        card: &DraggedProjectCard,
        target: CardDrop,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.current_column(&card.item_id) else {
            return;
        };
        match target {
            CardDrop::Column(column) => {
                if column != from {
                    self.move_item(&card.item_id, &column, cx);
                }
            }
            CardDrop::Card(target_id) => {
                if target_id == card.item_id {
                    return;
                }
                let Some(column) = self.current_column(&target_id) else {
                    return;
                };
                if column == from && !self.can_reorder() {
                    return;
                }
                let after = self.item_before_in_column(&column, &target_id, &card.item_id);
                if column != from {
                    self.move_item(&card.item_id, &column, cx);
                }
                if self.can_reorder() {
                    self.reorder_item(&card.item_id, after, cx);
                }
            }
        }
    }

    /// The card drawn just above `target_id` in `column`, leaving out the
    /// card being moved. Placing the moved card after it in the project's
    /// order puts it right above the target in the column, whatever other
    /// columns' items lie between.
    fn item_before_in_column(
        &self,
        column: &ColumnKey,
        target_id: &str,
        moved_id: &str,
    ) -> Option<String> {
        let columns = self.columns()?;
        let ids = columns
            .iter()
            .find(|candidate| &candidate.key == column)?
            .items
            .iter()
            .filter_map(|index| self.items().get(*index).map(|item| item.id.clone()))
            .filter(|id| id != moved_id)
            .collect::<Vec<_>>();
        let target = ids.iter().position(|id| id == target_id)?;
        target
            .checked_sub(1)
            .and_then(|before| ids.get(before).cloned())
    }

    /// Puts the item right after `after_id` in the project's order (first
    /// when `None`). A failure re-reads the project: later moves may have
    /// been placed after this one, so only GitHub knows the order now.
    pub fn reorder_item(
        &mut self,
        item_id: &str,
        after_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(project_id) = self.selected_project.clone() else {
            return;
        };
        if after_id.as_deref() == Some(item_id) || !self.can_reorder() || !self.may_write(cx) {
            return;
        }
        let Some(from) = self.items.iter().position(|item| item.id == item_id) else {
            return;
        };
        let moved = self.items.remove(from);
        let to = after_id
            .as_ref()
            .and_then(|after_id| self.items.iter().position(|item| &item.id == after_id))
            .map_or(0, |after| after + 1);
        self.items.insert(to, moved);
        self.layout_changed();
        self.move_message = None;
        let write =
            self.context
                .backend
                .move_project_item(project_id, item_id.to_string(), after_id);
        let item_id = item_id.to_string();
        cx.spawn(async move |this, cx| {
            if let Err(error) = write.await {
                this.update(cx, |this, cx| {
                    this.note_error(&error, cx);
                    this.note_write_error(&error);
                    let title = this.title_of(&item_id);
                    this.move_message = Some(SharedString::from(format!(
                        "Could not reorder “{title}”: {error}. Reading the project again."
                    )));
                    this.reread_project(cx);
                })
                .ok();
            }
        })
        .detach();
        cx.notify();
    }
}
