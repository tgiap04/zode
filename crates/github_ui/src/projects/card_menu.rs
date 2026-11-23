//! A card's "Move to" menu, opened by a right click or, for the card picked
//! with the arrow keys, by `menu::SecondaryConfirm`.

use github::ProjectItemContent;
use gpui::{
    AnyElement, Context, DismissEvent, Entity, Focusable as _, IntoElement as _,
    ParentElement as _, Pixels, Point, Subscription, Window,
};
use ui::ContextMenu;

use crate::projects::project_layout::{ColumnKey, board_columns};
use crate::projects::projects_section::ProjectsSection;
use crate::untrusted_text::reveal_hidden_characters;

pub struct CardMenu {
    pub item_id: String,
    pub menu: Entity<ContextMenu>,
    /// Where the right click was; `None` opens it at the card.
    pub position: Option<Point<Pixels>>,
    pub _dismissed: Subscription,
}

impl ProjectsSection {
    pub fn selected_card(&self) -> Option<&str> {
        self.selected_card.as_deref()
    }

    /// The card whose menu is open.
    pub fn card_menu_item(&self) -> Option<&str> {
        self.card_menu.as_ref().map(|menu| menu.item_id.as_str())
    }

    /// "Move to" each other column of the board, then "Clear" when the card
    /// has a value, then "Remove from project…". Nothing opens for a hidden
    /// item, or when the token can only read (the guidance is shown instead).
    pub fn deploy_card_menu(
        &mut self,
        item_id: String,
        position: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hidden = self
            .items
            .iter()
            .find(|item| item.id == item_id)
            .is_none_or(|item| item.content == ProjectItemContent::Redacted);
        let Some(details) = self.details() else {
            return;
        };
        if hidden {
            return;
        }
        let field_name = self.column_field().map(|field| field.name.clone());
        let current = self
            .current_column(&item_id)
            .filter(|_| field_name.is_some());
        let targets = board_columns(self.view(), details, &[])
            .into_iter()
            .filter(|column| {
                current.is_some()
                    && column.key != ColumnKey::NoValue
                    && Some(&column.key) != current.as_ref()
            })
            .map(|column| (column.key, column.name))
            .collect::<Vec<_>>();
        if !self.may_write(cx) {
            return;
        }
        let this = cx.entity().downgrade();
        let clears = field_name.filter(|_| current.is_some_and(|key| key != ColumnKey::NoValue));
        let menu_item = item_id.clone();
        let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
            let move_to = |menu: ContextMenu, label: String, key: ColumnKey| {
                let this = this.clone();
                let item_id = menu_item.clone();
                menu.entry(label, None, move |_, cx| {
                    this.update(cx, |this, cx| this.move_item(&item_id, &key, cx))
                        .ok();
                })
            };
            for (key, name) in targets {
                let label = format!("Move to {}", reveal_hidden_characters(&name));
                menu = move_to(menu, label, key);
            }
            if let Some(field_name) = clears {
                menu = menu.separator();
                let label = format!("Clear {}", reveal_hidden_characters(&field_name));
                menu = move_to(menu, label, ColumnKey::NoValue);
            }
            let this = this.clone();
            let item_id = menu_item.clone();
            menu.separator()
                .entry("Remove from project…", None, move |_, cx| {
                    this.update(cx, |this, cx| this.request_remove_item(&item_id, cx))
                        .ok();
                })
        });
        window.focus(&menu.focus_handle(cx), cx);
        let dismissed = cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, window, cx| {
            this.card_menu = None;
            // Back to the board, so the arrow keys keep working.
            window.focus(&this.focus_handle, cx);
            cx.notify();
        });
        self.card_menu = Some(CardMenu {
            item_id,
            menu,
            position,
            _dismissed: dismissed,
        });
        cx.notify();
    }

    pub(super) fn select_card(&mut self, item_id: String, cx: &mut Context<Self>) {
        self.selected_card = Some(item_id);
        cx.notify();
    }

    /// Steps through the cards column by column, top to bottom.
    pub(super) fn select_adjacent_card(&mut self, step: isize, cx: &mut Context<Self>) {
        let Some(details) = self.details() else {
            return;
        };
        let order = board_columns(self.view(), details, self.items())
            .into_iter()
            .flat_map(|column| column.items)
            .filter_map(|index| self.items().get(index).map(|item| item.id.clone()))
            .collect::<Vec<_>>();
        let Some(last) = order.len().checked_sub(1) else {
            return;
        };
        let current = self
            .selected_card
            .as_ref()
            .and_then(|selected| order.iter().position(|id| id == selected));
        let next = match current {
            None if step < 0 => last,
            None => 0,
            Some(position) => position.saturating_add_signed(step).min(last),
        };
        if let Some(id) = order.get(next).cloned() {
            self.select_card(id, cx);
        }
    }

    pub(super) fn open_selected_card(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self
            .selected_card
            .as_ref()
            .and_then(|selected| self.items.iter().position(|item| &item.id == selected))
        else {
            return;
        };
        self.open_item(index, window, cx);
    }

    /// The open menu, drawn by the card it belongs to: at the right click,
    /// or under the card when opened from the keyboard.
    pub(super) fn render_card_menu(&self, item_id: &str) -> Option<AnyElement> {
        let card_menu = self
            .card_menu
            .as_ref()
            .filter(|card_menu| card_menu.item_id == item_id)?;
        let mut anchored = gpui::anchored().anchor(gpui::Anchor::TopLeft);
        if let Some(position) = card_menu.position {
            anchored = anchored.position(position);
        }
        Some(
            gpui::deferred(anchored.child(card_menu.menu.clone()))
                .with_priority(1)
                .into_any_element(),
        )
    }
}
