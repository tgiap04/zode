//! The "New issue" dialog's properties: labels, assignees, milestone and
//! type, each on its own icon-labelled row.

use gpui::{AnyElement, Window, px};
use ui::{ContextMenu, DropdownMenu, DropdownStyle, IconPosition, Tooltip, prelude::*};

use crate::issues::assignee_picker_render::render_person_chip;
use crate::issues::issue_editor::CreateIssueDialog;
use crate::issues::issue_list_render::{filter_trigger, label_menu_entry};
use crate::issues::issue_row_render::render_label_chip;
use crate::issues::issue_type_picker::render_type_chip;
use crate::untrusted_text::reveal_hidden_characters;

pub(crate) fn property_row(
    icon: IconName,
    name: &'static str,
    content: impl IntoElement,
    cx: &App,
) -> AnyElement {
    h_flex()
        .items_start()
        .gap_3()
        .px_2()
        .py_1p5()
        .border_b_1()
        .border_color(cx.theme().colors().border_variant)
        .child(
            h_flex()
                .flex_none()
                .w(px(96.))
                .h(px(24.))
                .gap_1p5()
                .child(Icon::new(icon).size(IconSize::Small).color(Color::Muted))
                .child(Label::new(name).size(LabelSize::Small).color(Color::Muted)),
        )
        .child(div().flex_1().min_w_0().child(content))
        .into_any_element()
}

/// A chip with a small button that takes it off.
pub(crate) fn removable(
    chip: impl IntoElement,
    id: impl Into<ElementId>,
    what: String,
    disabled: bool,
    on_remove: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    h_flex()
        .flex_none()
        .gap_0p5()
        .child(chip)
        .child(
            IconButton::new(id, IconName::Close)
                .icon_size(IconSize::XSmall)
                .tooltip(Tooltip::text(format!("Remove {what}")))
                .disabled(disabled)
                .on_click(on_remove),
        )
        .into_any_element()
}

impl CreateIssueDialog {
    fn render_labels(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        let labels = self.labels.clone();
        let chosen = self.chosen_labels.clone();
        let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
            if labels.is_empty() {
                return menu.label("This repository has no labels");
            }
            for label in &labels {
                let this = this.clone();
                let name = label.name.clone();
                menu = label_menu_entry(menu, label, chosen.contains(&label.name), move |_, cx| {
                    let name = name.clone();
                    this.update(cx, |this, cx| this.toggle_label(name, cx)).ok();
                });
            }
            menu
        });
        let chips = self
            .labels
            .iter()
            .filter(|label| self.chosen_labels.contains(&label.name))
            .enumerate()
            .map(|(index, label)| {
                let name = label.name.clone();
                removable(
                    render_label_chip(label, cx),
                    ("github-create-issue-remove-label", index),
                    format!("the label {}", reveal_hidden_characters(&label.name)),
                    false,
                    cx.listener(move |this, _, _window, cx| this.toggle_label(name.clone(), cx)),
                )
            })
            .collect::<Vec<_>>();
        h_flex()
            .gap_1()
            .flex_wrap()
            .children(chips)
            .child(
                DropdownMenu::new_with_element(
                    "github-create-issue-labels",
                    filter_trigger(IconName::Plus, None, "Add label".to_string(), false),
                    menu,
                )
                .style(DropdownStyle::Ghost)
                .trigger_size(ButtonSize::Compact),
            )
            .into_any_element()
    }

    fn render_assignees(&self, cx: &mut Context<Self>) -> AnyElement {
        let chips = self
            .chosen_assignees
            .iter()
            .enumerate()
            .map(|(index, user)| {
                let removed = user.clone();
                removable(
                    render_person_chip(user, cx),
                    ("github-create-issue-remove-assignee", index),
                    format!("@{}", reveal_hidden_characters(&user.login)),
                    false,
                    cx.listener(move |this, _, _window, cx| {
                        this.toggle_assignee(removed.clone(), cx)
                    }),
                )
            })
            .collect::<Vec<_>>();
        v_flex()
            .gap_1()
            .when(!chips.is_empty(), |column| {
                column.child(h_flex().gap_1().flex_wrap().children(chips))
            })
            .child(self.assignee_picker.clone())
            .into_any_element()
    }

    fn render_milestone(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        let milestones = self.milestones.clone();
        let selected = self.milestone;
        let menu = ContextMenu::build(window, cx, move |menu, _, _| {
            let mut menu = menu.toggleable_entry(
                "No milestone",
                selected.is_none(),
                IconPosition::Start,
                None,
                {
                    let this = this.clone();
                    move |_, cx| {
                        this.update(cx, |this, cx| this.set_milestone(None, cx))
                            .ok();
                    }
                },
            );
            for milestone in milestones {
                let this = this.clone();
                let shown = reveal_hidden_characters(&milestone.title).into_owned();
                let number = milestone.number;
                menu = menu.toggleable_entry(
                    shown,
                    selected == Some(number),
                    IconPosition::Start,
                    None,
                    move |_, cx| {
                        this.update(cx, |this, cx| this.set_milestone(Some(number), cx))
                            .ok();
                    },
                );
            }
            menu
        });
        let text = self
            .milestones
            .iter()
            .find(|milestone| Some(milestone.number) == self.milestone)
            .map_or("None".to_string(), |milestone| {
                reveal_hidden_characters(&milestone.title).into_owned()
            });
        DropdownMenu::new("github-create-issue-milestone", text, menu)
            .style(DropdownStyle::Ghost)
            .trigger_size(ButtonSize::Compact)
            .into_any_element()
    }

    fn render_type(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        let types = self.issue_types.clone();
        let selected = self.issue_type.clone();
        let menu = ContextMenu::build(window, cx, {
            let selected = selected.clone();
            move |menu, _, _| {
                let mut menu = menu.toggleable_entry(
                    "No type",
                    selected.is_none(),
                    IconPosition::Start,
                    None,
                    {
                        let this = this.clone();
                        move |_, cx| {
                            this.update(cx, |this, cx| this.set_issue_type(None, cx))
                                .ok();
                        }
                    },
                );
                for issue_type in types {
                    let this = this.clone();
                    let shown = reveal_hidden_characters(&issue_type.name).into_owned();
                    let id = issue_type.id.clone();
                    menu = menu.toggleable_entry(
                        shown,
                        selected.as_ref() == Some(&issue_type.id),
                        IconPosition::Start,
                        None,
                        move |_, cx| {
                            let id = id.clone();
                            this.update(cx, |this, cx| this.set_issue_type(Some(id), cx))
                                .ok();
                        },
                    );
                }
                menu
            }
        });
        let chosen = self
            .issue_types
            .iter()
            .find(|issue_type| Some(&issue_type.id) == selected.as_ref());
        let trigger = match chosen {
            Some(issue_type) => render_type_chip(&issue_type.name, &self.issue_types),
            None => Label::new("None")
                .size(LabelSize::Small)
                .color(Color::Muted)
                .into_any_element(),
        };
        DropdownMenu::new_with_element("github-create-issue-type", trigger, menu)
            .style(DropdownStyle::Ghost)
            .trigger_size(ButtonSize::Compact)
            .into_any_element()
    }

    pub(crate) fn render_properties(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let labels = self.render_labels(window, cx);
        let assignees = self.render_assignees(cx);
        let milestone = self.render_milestone(window, cx);
        let issue_type = (!self.issue_types.is_empty()).then(|| self.render_type(window, cx));
        v_flex()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors().border_variant)
            .child(property_row(IconName::Bookmark, "Labels", labels, cx))
            .child(property_row(IconName::Person, "Assignees", assignees, cx))
            .child(property_row(IconName::Pin, "Milestone", milestone, cx))
            .when_some(issue_type, |column, issue_type| {
                column.child(property_row(IconName::SquareDot, "Type", issue_type, cx))
            })
            .into_any_element()
    }
}
