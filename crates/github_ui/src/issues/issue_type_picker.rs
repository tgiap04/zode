//! Issue types: the picker in an issue's tab and the type shown on a row.
//! A repository whose owner has no types shows none of this.

use github::IssueType;
use gpui::{Context, Window};
use ui::{ContextMenu, DropdownMenu, IconPosition, prelude::*};

use crate::issues::IssueDetailView;
use crate::issues::issue_actions::may_triage;
use crate::untrusted_text::reveal_hidden_characters;

/// The types a user may pick: GitHub refuses a disabled one.
pub fn type_choices(types: &[IssueType]) -> Vec<&IssueType> {
    types
        .iter()
        .filter(|issue_type| issue_type.is_enabled)
        .collect()
}

fn type_color(issue_type: Option<&IssueType>) -> Color {
    named_color(issue_type.and_then(|issue_type| issue_type.color.as_deref()))
}

/// GitHub names an issue type's or a project option's colour (`RED`,
/// `BLUE`, …) rather than giving one.
pub(crate) fn named_color(name: Option<&str>) -> Color {
    match name {
        Some("RED") => Color::Error,
        Some("GREEN") => Color::Success,
        Some("YELLOW" | "ORANGE") => Color::Warning,
        Some("BLUE") => Color::Info,
        Some("PURPLE" | "PINK") => Color::Accent,
        _ => Color::Muted,
    }
}

/// A type's dot and name, for a row or the issue's summary.
pub(crate) fn render_type_chip(name: &str, types: &[IssueType]) -> AnyElement {
    let issue_type = types.iter().find(|issue_type| issue_type.name == name);
    h_flex()
        .gap_0p5()
        .child(
            Icon::new(IconName::Circle)
                .size(IconSize::XSmall)
                .color(type_color(issue_type)),
        )
        .child(
            Label::new(reveal_hidden_characters(name).into_owned())
                .size(LabelSize::XSmall)
                .color(Color::Muted),
        )
        .into_any_element()
}

impl IssueDetailView {
    pub fn issue_types(&self) -> &[IssueType] {
        &self.issue_types
    }

    /// Hidden when the owner has no types, and below triage access.
    pub fn shows_type_picker(&self) -> bool {
        may_triage(self.permission) && !self.issue_types.is_empty()
    }

    /// `None` clears the type. The issue is read back, and a type GitHub
    /// ignored is said.
    pub fn set_issue_type(
        &mut self,
        issue_type: Option<IssueType>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.shows_type_picker() || !self.ready_to_write(cx) {
            return;
        }
        let Some(issue) = &self.issue else {
            return;
        };
        let wanted = issue_type
            .as_ref()
            .map(|issue_type| issue_type.name.clone());
        let write = self.backend.set_issue_type(
            issue.node_id.clone(),
            issue_type.map(|issue_type| issue_type.id),
        );
        self.write_and_read_back(write, None, window, cx, move |issue| {
            (issue.issue_type != wanted).then(|| "the issue type".to_string())
        });
    }

    pub(crate) fn render_type_picker(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.shows_type_picker() {
            return None;
        }
        let current = self
            .issue
            .as_ref()
            .and_then(|issue| issue.issue_type.clone());
        let choices = type_choices(&self.issue_types)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let this = cx.entity().downgrade();
        let menu = ContextMenu::build(window, cx, {
            let current = current.clone();
            move |mut menu, _, _| {
                menu = menu.entry("No type", None, {
                    let this = this.clone();
                    move |window, cx| {
                        this.update(cx, |this, cx| this.set_issue_type(None, window, cx))
                            .ok();
                    }
                });
                for choice in choices {
                    let this = this.clone();
                    let shown = reveal_hidden_characters(&choice.name).into_owned();
                    let toggled = current.as_ref() == Some(&choice.name);
                    menu = menu.toggleable_entry(
                        shown,
                        toggled,
                        IconPosition::Start,
                        None,
                        move |window, cx| {
                            let choice = choice.clone();
                            this.update(cx, |this, cx| {
                                this.set_issue_type(Some(choice), window, cx)
                            })
                            .ok();
                        },
                    );
                }
                menu
            }
        });
        let label = current.map_or("Type".to_string(), |name| {
            reveal_hidden_characters(&name).into_owned()
        });
        Some(
            h_flex()
                .gap_1()
                .child(
                    Label::new("Type")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(DropdownMenu::new("github-issue-type", label, menu).disabled(self.is_busy()))
                .into_any_element(),
        )
    }
}
