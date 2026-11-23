use gpui::{Context, IntoElement};
use ui::prelude::*;

use crate::projects::drag_state::{CardDrop, DraggedProjectCard};
use crate::projects::project_layout::{BoardColumn, board_columns};
use crate::projects::projects_section::ProjectsSection;
use crate::untrusted_text::reveal_hidden_characters;

const COLUMN_WIDTH: f32 = 18.;

impl ProjectsSection {
    pub(crate) fn render_board(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(details) = self.details() else {
            return div().into_any_element();
        };
        let columns = board_columns(self.view(), details, self.items());
        h_flex()
            .id("github-project-board")
            .key_context("GithubProjectBoard")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &menu::SelectNext, _window, cx| {
                this.select_adjacent_card(1, cx)
            }))
            .on_action(cx.listener(|this, _: &menu::SelectPrevious, _window, cx| {
                this.select_adjacent_card(-1, cx)
            }))
            .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| {
                this.open_selected_card(window, cx)
            }))
            .on_action(cx.listener(|this, _: &menu::SecondaryConfirm, window, cx| {
                if let Some(item_id) = this.selected_card.clone() {
                    this.deploy_card_menu(item_id, None, window, cx);
                }
            }))
            .size_full()
            .items_start()
            .gap_2()
            .overflow_x_scroll()
            .children(
                columns
                    .into_iter()
                    .enumerate()
                    .map(|(position, column)| self.render_column(position, column, cx)),
            )
            .into_any_element()
    }

    fn render_column(
        &self,
        position: usize,
        column: BoardColumn,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors();
        let drop_background = colors.drop_target_background;
        let key = column.key.clone();
        v_flex()
            .w(rems(COLUMN_WIDTH))
            .flex_none()
            .h_full()
            .gap_1()
            .p_1()
            .rounded_md()
            .bg(colors.panel_background)
            .child(
                h_flex()
                    .gap_2()
                    .px_1()
                    .child(Label::new(
                        reveal_hidden_characters(&column.name).into_owned(),
                    ))
                    .child(
                        Label::new(column.items.len().to_string())
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            )
            .child(
                v_flex()
                    .id(("github-project-column", position))
                    .debug_selector(move || format!("github-project-column-{position}"))
                    .flex_1()
                    .min_h_0()
                    .gap_1()
                    .overflow_y_scroll()
                    .drag_over::<DraggedProjectCard>(move |style, _, _, _| {
                        style.bg(drop_background)
                    })
                    .on_drop(cx.listener(move |this, card: &DraggedProjectCard, _, cx| {
                        this.drop_card(card, CardDrop::Column(key.clone()), cx)
                    }))
                    .children(column.items.into_iter().filter_map(|index| {
                        let item = self.items().get(index)?;
                        Some(self.render_card(index, item, cx))
                    })),
            )
            .into_any_element()
    }
}
