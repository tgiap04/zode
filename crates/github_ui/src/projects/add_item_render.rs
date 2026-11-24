use editor::Editor;
use github::ProjectFieldKind;
use gpui::{AppContext as _, Context, Window};
use ui::{Callout, ContextMenu, DropdownMenu, IconPosition, prelude::*};

use crate::projects::projects_section::ProjectsSection;
use crate::untrusted_text::reveal_hidden_characters;

impl ProjectsSection {
    /// The search field (Enter searches), the status the added item gets,
    /// and what was found, each added by a click.
    pub(super) fn render_add_item(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let status_field = self.status_field().cloned();
        let picker = self.add_item.as_mut()?;
        let field = picker
            .field
            .get_or_insert_with(|| {
                cx.new(|cx| {
                    let mut editor = Editor::single_line(window, cx);
                    editor.set_placeholder_text("#number or words, then Enter", window, cx);
                    editor
                })
            })
            .clone();
        let status_control = status_field.and_then(|field| {
            let ProjectFieldKind::SingleSelect(options) = &field.kind else {
                return None;
            };
            let picked = picker.status.clone();
            let label = options
                .iter()
                .find(|option| Some(&option.id) == picked.as_ref())
                .map_or_else(
                    || format!("No {}", field.name),
                    |option| option.name.clone(),
                );
            let options = options.clone();
            let this = cx.entity().downgrade();
            let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
                for option in options {
                    let this = this.clone();
                    let toggled = picked.as_ref() == Some(&option.id);
                    menu = menu.toggleable_entry(
                        reveal_hidden_characters(&option.name).into_owned(),
                        toggled,
                        IconPosition::Start,
                        None,
                        move |_, cx| {
                            let id = option.id.clone();
                            this.update(cx, |this, cx| this.set_add_item_status(Some(id), cx))
                                .ok();
                        },
                    );
                }
                let this = this.clone();
                menu.separator()
                    .entry(format!("No {}", field.name), None, move |_, cx| {
                        this.update(cx, |this, cx| this.set_add_item_status(None, cx))
                            .ok();
                    })
            });
            Some(DropdownMenu::new(
                "github-project-add-status",
                reveal_hidden_characters(&label).into_owned(),
                menu,
            ))
        });
        let status = if picker.adding {
            Some(("Adding…".to_string(), Color::Muted))
        } else if picker.searching {
            Some(("Searching…".to_string(), Color::Muted))
        } else {
            picker.message.as_ref().map(|message| {
                (
                    reveal_hidden_characters(message).into_owned(),
                    Color::Warning,
                )
            })
        };
        let results = picker
            .candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| {
                let picked = candidate.clone();
                let kind = if candidate.pull_request {
                    "pull request"
                } else {
                    "issue"
                };
                h_flex()
                    .id(("github-project-add-result", index))
                    .gap_1()
                    .px_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(|row| row.bg(cx.theme().colors().element_hover))
                    .on_click(
                        cx.listener(move |this, _, _window, cx| this.add_item(picked.clone(), cx)),
                    )
                    .child(
                        Label::new(format!(
                            "#{} {}",
                            candidate.number,
                            reveal_hidden_characters(&candidate.title)
                        ))
                        .size(LabelSize::Small)
                        .truncate(),
                    )
                    .child(Label::new(kind).size(LabelSize::XSmall).color(Color::Muted))
            })
            .collect::<Vec<_>>();
        Some(
            v_flex()
                .debug_selector(|| "github-project-add-item".to_string())
                .gap_1()
                .p_1()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().colors().border)
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Label::new("Add an issue or pull request of this repository")
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        )
                        .child(div().flex_1())
                        .children(status_control)
                        .child(
                            IconButton::new("github-project-add-close", IconName::Close)
                                .icon_size(IconSize::Small)
                                .on_click(
                                    cx.listener(|this, _, _window, cx| this.close_add_item(cx)),
                                ),
                        ),
                )
                .child(
                    div()
                        .key_context("GithubProjectAddItem")
                        .on_action(cx.listener({
                            let field = field.clone();
                            move |this, _: &menu::Confirm, _window, cx| {
                                let text = field.read(cx).text(cx);
                                this.search_add_item(text, cx)
                            }
                        }))
                        .px_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(cx.theme().colors().border)
                        .child(field),
                )
                .children(
                    status
                        .map(|(text, color)| Label::new(text).size(LabelSize::Small).color(color)),
                )
                .children(results)
                .into_any_element(),
        )
    }

    pub(super) fn render_removal_confirmation(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let question = self.removal_question()?;
        Some(
            v_flex()
                .debug_selector(|| "github-project-remove-confirm".to_string())
                .gap_1()
                .child(
                    Callout::new()
                        .severity(Severity::Warning)
                        .description(reveal_hidden_characters(&question).into_owned()),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("github-project-remove-yes", "Remove from project")
                                .label_size(LabelSize::Small)
                                .style(ButtonStyle::Filled)
                                .on_click(
                                    cx.listener(|this, _, _window, cx| {
                                        this.confirm_remove_item(cx)
                                    }),
                                ),
                        )
                        .child(
                            Button::new("github-project-remove-no", "Cancel")
                                .label_size(LabelSize::Small)
                                .on_click(
                                    cx.listener(|this, _, _window, cx| this.cancel_remove_item(cx)),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}
