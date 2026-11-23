//! Where fields are edited: a cell's editor (Enter saves, Escape cancels),
//! a cell's menu of options or iterations, and the board's panel for the
//! picked card.

use editor::Editor;
use github::{ProjectField, ProjectFieldKind, ProjectItemContent, ProjectV2Item};
use gpui::{DismissEvent, Entity, Focusable as _, Pixels, Point, WeakEntity, Window};
use ui::{ContextMenu, DropdownMenu, IconPosition, prelude::*};

use crate::projects::field_editor::{FieldEdit, FieldEditing, FieldMenu, field_editing};
use crate::projects::move_item::column_key_of;
use crate::projects::project_layout::{ColumnKey, cell_text, item_title, value_of};
use crate::projects::projects_section::ProjectsSection;
use crate::untrusted_text::reveal_hidden_characters;

/// What drawing an open editor needs, cheap to hand to a table's rows.
#[derive(Clone)]
pub(super) struct FieldEditorParts {
    editor: Entity<Editor>,
    error: Option<SharedString>,
    saving: bool,
}

impl FieldEditorParts {
    pub(super) fn of(edit: &FieldEdit) -> Self {
        Self {
            editor: edit.editor.clone(),
            error: edit.error.clone(),
            saving: edit.saving.is_some(),
        }
    }

    pub(super) fn render(&self, section: WeakEntity<ProjectsSection>, cx: &App) -> AnyElement {
        let border = if self.error.is_some() {
            cx.theme().status().error
        } else {
            cx.theme().colors().border_focused
        };
        let on_confirm = section.clone();
        let on_cancel = section.clone();
        v_flex()
            .debug_selector(|| "github-project-field-editor".to_string())
            .key_context("GithubProjectFieldEditor")
            .w_full()
            .on_action(move |_: &menu::Confirm, _window, cx| {
                on_confirm
                    .update(cx, |section, cx| section.save_field_edit(cx))
                    .ok();
            })
            .on_action(move |_: &menu::Cancel, _window, cx| {
                on_cancel
                    .update(cx, |section, cx| section.cancel_field_edit(cx))
                    .ok();
            })
            // A single-line editor passes Escape on as its own `Cancel`.
            .on_action(move |_: &editor::actions::Cancel, _window, cx| {
                section
                    .update(cx, |section, cx| section.cancel_field_edit(cx))
                    .ok();
            })
            .child(
                div()
                    .px_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(border)
                    .child(self.editor.clone()),
            )
            .when(self.saving, |column| {
                column.child(
                    Label::new("Saving…")
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
            })
            .when_some(self.error.clone(), |column, error| {
                column.child(
                    Label::new(reveal_hidden_characters(&error).into_owned())
                        .size(LabelSize::XSmall)
                        .color(Color::Error),
                )
            })
            .into_any_element()
    }
}

/// The entries of a choice field: its options, or its iterations not yet
/// finished (and the item's own, even if it is), then "Clear".
fn choices(field: &ProjectField, current: &ColumnKey) -> Vec<(ColumnKey, String)> {
    match &field.kind {
        ProjectFieldKind::SingleSelect(options) => options
            .iter()
            .map(|option| (ColumnKey::Option(option.id.clone()), option.name.clone()))
            .collect(),
        ProjectFieldKind::Iteration(iterations) => iterations
            .iter()
            .filter(|iteration| {
                !iteration.completed || *current == ColumnKey::Iteration(iteration.id.clone())
            })
            .map(|iteration| {
                (
                    ColumnKey::Iteration(iteration.id.clone()),
                    iteration.title.clone(),
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn choice_id(key: &ColumnKey) -> Option<String> {
    match key {
        ColumnKey::Option(id) | ColumnKey::Iteration(id) => Some(id.clone()),
        ColumnKey::NoValue => None,
    }
}

fn choice_menu(
    section: WeakEntity<ProjectsSection>,
    item: &ProjectV2Item,
    field: &ProjectField,
    window: &mut Window,
    cx: &mut App,
) -> Entity<ContextMenu> {
    let current = column_key_of(value_of(item, &field.id));
    let entries = choices(field, &current);
    let item_id = item.id.clone();
    let field_id = field.id.clone();
    let clear = format!("Clear {}", reveal_hidden_characters(&field.name));
    ContextMenu::build(window, cx, move |mut menu, _, _| {
        let has_value = current != ColumnKey::NoValue;
        for (key, name) in entries {
            let (section, item_id, field_id) = (section.clone(), item_id.clone(), field_id.clone());
            let toggled = key == current;
            menu = menu.toggleable_entry(
                reveal_hidden_characters(&name).into_owned(),
                toggled,
                IconPosition::Start,
                None,
                move |_, cx| {
                    section
                        .update(cx, |section, cx| {
                            section.set_field_choice(&item_id, &field_id, choice_id(&key), cx)
                        })
                        .ok();
                },
            );
        }
        if has_value {
            menu = menu.separator().entry(clear, None, move |_, cx| {
                section
                    .update(cx, |section, cx| {
                        section.set_field_choice(&item_id, &field_id, None, cx)
                    })
                    .ok();
            });
        }
        menu
    })
}

impl ProjectsSection {
    pub(super) fn deploy_field_menu(
        &mut self,
        item_id: &str,
        field: &ProjectField,
        position: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self.items.iter().find(|item| item.id == item_id) else {
            return;
        };
        let menu = choice_menu(cx.entity().downgrade(), item, field, window, cx);
        window.focus(&menu.focus_handle(cx), cx);
        let dismissed = cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, _, cx| {
            this.field_menu = None;
            cx.notify();
        });
        self.field_menu = Some(FieldMenu {
            menu,
            position,
            _dismissed: dismissed,
        });
        cx.notify();
    }

    pub(super) fn render_field_menu(&self) -> Option<AnyElement> {
        let field_menu = self.field_menu.as_ref()?;
        let mut anchored = gpui::anchored().anchor(gpui::Anchor::TopLeft);
        if let Some(position) = field_menu.position {
            anchored = anchored.position(position);
        }
        Some(
            gpui::deferred(anchored.child(field_menu.menu.clone()))
                .with_priority(1)
                .into_any_element(),
        )
    }

    /// The picked card's fields on a board: typed ones edited in place,
    /// choices from a menu, the issue's own sent to the issue.
    pub(super) fn render_item_panel(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let item_id = self.selected_card.as_deref()?;
        let index = self.items.iter().position(|item| item.id == item_id)?;
        let item = self.items.get(index)?;
        let details = self.details()?;
        let hidden = item.content == ProjectItemContent::Redacted;
        let opens = matches!(
            item.content,
            ProjectItemContent::Issue { .. } | ProjectItemContent::PullRequest { .. }
        );
        let section = cx.entity().downgrade();
        let rows = details
            .fields
            .iter()
            .filter(|field| field_editing(field) != FieldEditing::NotEditable)
            .map(|field| {
                let name = reveal_hidden_characters(&field.name).into_owned();
                let value = reveal_hidden_characters(&cell_text(field, item)).into_owned();
                let control = match field_editing(field) {
                    _ if hidden => Label::new(value).size(LabelSize::Small).into_any_element(),
                    FieldEditing::Typed => match self
                        .field_edit()
                        .filter(|edit| edit.item_id == item.id && edit.field_id == field.id)
                    {
                        Some(edit) => FieldEditorParts::of(edit).render(section.clone(), cx),
                        None => {
                            let (item_id, field_id) = (item.id.clone(), field.id.clone());
                            Button::new(
                                SharedString::from(format!("github-project-edit-{field_id}")),
                                if value.is_empty() {
                                    "Empty".to_string()
                                } else {
                                    value
                                },
                            )
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.start_field_edit(&item_id, &field_id, window, cx)
                            }))
                            .into_any_element()
                        }
                    },
                    FieldEditing::Choice => DropdownMenu::new(
                        SharedString::from(format!("github-project-choice-{}", field.id)),
                        if value.is_empty() {
                            "None".to_string()
                        } else {
                            value
                        },
                        choice_menu(section.clone(), item, field, window, cx),
                    )
                    .into_any_element(),
                    FieldEditing::OnTheIssue | FieldEditing::NotEditable => Label::new(value)
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .into_any_element(),
                };
                v_flex()
                    .gap_0p5()
                    .child(Label::new(name).size(LabelSize::XSmall).color(Color::Muted))
                    .child(control)
            })
            .collect::<Vec<_>>();
        let remove_id = item.id.clone();
        Some(
            v_flex()
                .id("github-project-item-panel")
                .debug_selector(|| "github-project-item-panel".to_string())
                .w(rems(18.))
                .flex_none()
                .h_full()
                .gap_2()
                .p_2()
                .rounded_md()
                .bg(cx.theme().colors().panel_background)
                .overflow_y_scroll()
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Label::new(reveal_hidden_characters(item_title(item)).into_owned())
                                .truncate(),
                        )
                        .child(div().flex_1())
                        .child(
                            IconButton::new("github-project-item-panel-close", IconName::Close)
                                .icon_size(IconSize::Small)
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.selected_card = None;
                                    this.field_edit = None;
                                    cx.notify();
                                })),
                        ),
                )
                .children(rows)
                .when(opens, |panel| {
                    panel.child(
                        Label::new(
                            "Assignees, labels and milestone belong to the issue: edit them there.",
                        )
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                    )
                    .child(
                        Button::new("github-project-item-open", "Edit on the issue")
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_item(index, window, cx)
                            })),
                    )
                })
                .child(
                    Button::new("github-project-item-remove", "Remove from project…")
                        .label_size(LabelSize::Small)
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            this.request_remove_item(&remove_id, cx)
                        })),
                )
                .into_any_element(),
        )
    }
}
