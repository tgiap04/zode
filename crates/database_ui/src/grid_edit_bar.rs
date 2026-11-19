//! The bar over the pager that appears once a cell has been edited: how many
//! edits are waiting, what is wrong with them, and the buttons that save,
//! discard or show them.

use crate::database_panel::DatabasePanel;
use crate::grid_editing::changes_label;
use crate::query::QueryState;
use gpui::Context;
use ui::{Tooltip, prelude::*};
use zed_actions::database as actions;

impl DatabasePanel {
    pub(crate) fn render_edit_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let QueryState::Done(page) = &self.query else {
            return None;
        };
        let edits = &page.edits;
        if edits.staged.is_empty() && edits.failure.is_none() {
            return None;
        }
        let invalid = edits.invalid_count();
        let saving = edits.saving.is_some();
        // What failed last, else what is wrong now: a refusal from the database
        // is news, and the check on each value is the standing state.
        let message = edits
            .failure
            .clone()
            .map(|failure| failure.to_string())
            .or_else(|| {
                page.first_problem().map(|problem| match invalid {
                    0 => problem,
                    1 => format!("1 invalid value — {problem}"),
                    count => format!("{count} invalid values — first: {problem}"),
                })
            });
        let label = if saving {
            "Saving…".to_string()
        } else {
            changes_label(edits.staged.len())
        };

        Some(
            h_flex()
                .flex_none()
                .w_full()
                .gap_2()
                .px_2()
                .py_1()
                .border_t_1()
                .border_color(cx.theme().colors().border)
                .bg(cx.theme().colors().surface_background)
                .debug_selector(|| "database-edit-bar".into())
                .child(Label::new(label).size(LabelSize::Small))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .debug_selector(|| "database-edit-message".into())
                        .children(message.map(|text| {
                            Label::new(text)
                                .size(LabelSize::XSmall)
                                .color(Color::Error)
                                .truncate()
                        })),
                )
                .child(
                    div()
                        .debug_selector(|| "database-edit-preview".into())
                        .child(
                            Button::new("database-edit-preview", "Preview SQL")
                                .size(ButtonSize::Compact)
                                .disabled(saving)
                                .on_click(cx.listener(|this, _event, window, cx| {
                                    this.show_save_preview(window, cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .debug_selector(|| "database-edit-discard".into())
                        .child(
                            Button::new("database-edit-discard", "Discard")
                                .size(ButtonSize::Compact)
                                .disabled(saving)
                                .tooltip(|_window, cx| {
                                    Tooltip::for_action(
                                        "Discard Changes",
                                        &actions::DiscardChanges,
                                        cx,
                                    )
                                })
                                .on_click(cx.listener(|this, _event, window, cx| {
                                    this.discard_changes(window, cx);
                                })),
                        ),
                )
                .child(
                    div().debug_selector(|| "database-edit-save".into()).child(
                        Button::new("database-edit-save", "Save")
                            .size(ButtonSize::Compact)
                            .style(ButtonStyle::Filled)
                            .disabled(saving || invalid > 0)
                            .tooltip(move |_window, cx| {
                                if invalid > 0 {
                                    Tooltip::simple("Fix the invalid values first", cx)
                                } else {
                                    Tooltip::for_action("Save Changes", &actions::SaveChanges, cx)
                                }
                            })
                            .on_click(cx.listener(|this, _event, window, cx| {
                                this.request_save(window, cx);
                            })),
                    ),
                )
                .into_any_element(),
        )
    }
}
