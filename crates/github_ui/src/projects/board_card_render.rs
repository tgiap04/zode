//! One card of a project board: what the item is and where it stands, its
//! title, labels, people, and the view's other fields that hold a value.

use github::{ProjectItemContent, ProjectItemState, ProjectV2Item};
use gpui::{Context, IntoElement, MouseButton, MouseDownEvent, px};
use ui::{Tooltip, prelude::*};

use crate::avatar::render_people;
use crate::issues::{named_color, render_label_chip};
use crate::projects::drag_state::{CardDrop, DraggedProjectCard};
use crate::projects::project_layout::{CardField, CardIcon, card_fields, item_title};
use crate::projects::projects_section::ProjectsSection;
use crate::untrusted_text::reveal_hidden_characters;

const SHOWN_LABELS: usize = 3;

/// The icon, its colour and what it means. Issues and pull requests have
/// their own shapes, so colour is never the only difference.
fn state_icon(item: &ProjectV2Item) -> Option<(IconName, Color, &'static str)> {
    use ProjectItemState::*;
    Some(match (&item.content, item.state) {
        (ProjectItemContent::Issue { .. }, Some(Open)) => {
            (IconName::TodoProgress, Color::Success, "Open issue")
        }
        (ProjectItemContent::Issue { .. }, Some(NotPlanned)) => {
            (IconName::XCircle, Color::Muted, "Closed as not planned")
        }
        (ProjectItemContent::Issue { .. }, Some(_)) => {
            (IconName::TodoComplete, Color::Accent, "Closed issue")
        }
        (ProjectItemContent::Issue { .. }, None) => (IconName::TodoProgress, Color::Muted, "Issue"),
        (ProjectItemContent::PullRequest { .. }, Some(Merged)) => {
            (IconName::PullRequest, Color::Accent, "Merged pull request")
        }
        (ProjectItemContent::PullRequest { .. }, Some(Closed | NotPlanned)) => {
            (IconName::PullRequest, Color::Error, "Closed pull request")
        }
        (ProjectItemContent::PullRequest { .. }, Some(Draft)) => {
            (IconName::PullRequest, Color::Muted, "Draft pull request")
        }
        (ProjectItemContent::PullRequest { .. }, _) => {
            (IconName::PullRequest, Color::Success, "Open pull request")
        }
        (ProjectItemContent::DraftIssue { .. }, _) => {
            (IconName::Circle, Color::Muted, "Draft issue")
        }
        (ProjectItemContent::Redacted, _) => return None,
    })
}

/// `repo #12`: the repository without its owner, which the board's own
/// repository almost always shares.
fn reference(item: &ProjectV2Item) -> Option<String> {
    match &item.content {
        ProjectItemContent::Issue {
            number, repository, ..
        }
        | ProjectItemContent::PullRequest {
            number, repository, ..
        } => {
            let name = repository.rsplit('/').next().unwrap_or(repository);
            Some(format!("{name} #{number}"))
        }
        ProjectItemContent::DraftIssue { .. } => Some("Draft".to_string()),
        ProjectItemContent::Redacted => None,
    }
}

/// The icon of a count, green once every sub-issue is done.
pub(crate) fn card_icon(icon: CardIcon, value: &str) -> (IconName, Color) {
    match icon {
        CardIcon::PullRequest => (IconName::PullRequest, Color::Success),
        CardIcon::SubIssues => {
            let done = value
                .split_once('/')
                .is_some_and(|(completed, total)| completed == total);
            (
                IconName::ListTree,
                if done { Color::Success } else { Color::Muted },
            )
        }
    }
}

fn render_field_chip(position: usize, field: &CardField, cx: &App) -> AnyElement {
    let colors = cx.theme().colors();
    let name = reveal_hidden_characters(&field.name).into_owned();
    let value = reveal_hidden_characters(&field.value).into_owned();
    h_flex()
        .id(("github-card-field", position))
        .flex_none()
        .max_w_full()
        .h(px(20.))
        .gap_1()
        .px_1p5()
        .rounded_full()
        .bg(colors.element_background)
        .border_1()
        .border_color(colors.border_variant)
        .when_some(field.icon, |chip, icon| {
            let (icon, color) = card_icon(icon, &field.value);
            chip.child(Icon::new(icon).size(IconSize::XSmall).color(color))
        })
        .when_some(field.color.as_deref(), |chip, color| {
            chip.child(
                div()
                    .flex_none()
                    .size(px(8.))
                    .rounded_full()
                    .bg(named_color(Some(color)).color(cx)),
            )
        })
        .child(
            Label::new(value.clone())
                .size(LabelSize::XSmall)
                .single_line()
                .truncate(),
        )
        .tooltip(Tooltip::text(format!("{name}: {value}")))
        .into_any_element()
}

fn render_faces(item: &ProjectV2Item, cx: &App) -> Option<AnyElement> {
    render_people("github-card-faces", &item_people(item), px(140.), cx)
}

/// `(login, avatar URL)` of the item's assignees, logins made safe to draw.
pub(crate) fn item_people(item: &ProjectV2Item) -> Vec<(String, Option<String>)> {
    item.assignees
        .iter()
        .map(|login| {
            (
                reveal_hidden_characters(login).into_owned(),
                item.assignee_avatars.get(login).cloned(),
            )
        })
        .collect()
}

impl ProjectsSection {
    pub(crate) fn render_card(
        &self,
        index: usize,
        item: &ProjectV2Item,
        cx: &mut Context<ProjectsSection>,
    ) -> AnyElement {
        let hidden = item.content == ProjectItemContent::Redacted;
        let opens = matches!(
            item.content,
            ProjectItemContent::Issue { .. } | ProjectItemContent::PullRequest { .. }
        );
        let fields = self
            .details()
            .map(|details| card_fields(self.view(), details, item))
            .unwrap_or_default();
        let selector_id = item.id.clone();
        let colors = cx.theme().colors();
        let hover = colors.element_hover;
        let selected = self.selected_card.as_deref() == Some(item.id.as_str());
        let title = reveal_hidden_characters(item_title(item)).into_owned();
        let dragged = DraggedProjectCard {
            item_id: item.id.clone(),
            title: title.clone().into(),
        };
        let item_id = item.id.clone();
        let hidden_labels = item.labels.len().saturating_sub(SHOWN_LABELS);
        let header = h_flex()
            .gap_1()
            .when_some(state_icon(item), |row, (icon, color, meaning)| {
                row.child(
                    div()
                        .id("github-card-state")
                        .flex_none()
                        .tooltip(Tooltip::text(meaning))
                        .child(Icon::new(icon).size(IconSize::XSmall).color(color)),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .children(reference(item).map(|reference| {
                        Label::new(reveal_hidden_characters(&reference).into_owned())
                            .size(LabelSize::XSmall)
                            .color(Color::Muted)
                            .single_line()
                            .truncate()
                    })),
            )
            .children(render_faces(item, cx));
        v_flex()
            .id(("github-project-card", index))
            .debug_selector(move || format!("github-project-card-{selector_id}"))
            .gap_1p5()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(if selected {
                colors.border_focused
            } else {
                colors.border_variant
            })
            .bg(colors.editor_background)
            .on_mouse_down(MouseButton::Left, {
                let item_id = item_id.clone();
                cx.listener(move |this, _: &MouseDownEvent, _window, cx| {
                    this.select_card(item_id.clone(), cx)
                })
            })
            .drag_over::<DraggedProjectCard>(move |style, _, _, cx| {
                style.border_color(cx.theme().colors().drop_target_border)
            })
            .on_drop({
                let item_id = item_id.clone();
                cx.listener(move |this, card: &DraggedProjectCard, _, cx| {
                    this.drop_card(card, CardDrop::Card(item_id.clone()), cx)
                })
            })
            // A hidden item can be dropped on, but not moved: the token
            // cannot see what it is.
            .when(!hidden, |card| {
                card.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        this.select_card(item_id.clone(), cx);
                        this.deploy_card_menu(item_id.clone(), Some(event.position), window, cx);
                        cx.stop_propagation();
                    }),
                )
                .on_drag(dragged, |card, _, _, cx| cx.new(|_| card.clone()))
            })
            .when(opens, |card| {
                card.cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.open_item(index, window, cx)),
                    )
            })
            .child(header)
            .child(Label::new(title).color(if hidden { Color::Muted } else { Color::Default }))
            .when(!item.labels.is_empty(), |card| {
                card.child(
                    h_flex()
                        .flex_wrap()
                        .gap_1()
                        .children(
                            item.labels
                                .iter()
                                .take(SHOWN_LABELS)
                                .map(|label| render_label_chip(label, cx)),
                        )
                        .when(hidden_labels > 0, |row| {
                            row.child(
                                Label::new(format!("+{hidden_labels}"))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                        }),
                )
            })
            .when(!fields.is_empty(), |card| {
                card.child(
                    h_flex().flex_wrap().gap_1().children(
                        fields
                            .iter()
                            .enumerate()
                            .map(|(position, field)| render_field_chip(position, field, cx)),
                    ),
                )
            })
            .children(self.render_card_menu(&item.id))
            .into_any_element()
    }
}
