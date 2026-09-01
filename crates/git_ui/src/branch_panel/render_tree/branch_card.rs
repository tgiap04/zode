//! One branch, drawn as a card.
//!
//! The card is the panel's main object: a status dot, the branch name, whether
//! it is the checked-out one, and a second line saying where it tracks. Split
//! out of `render_tree.rs` to keep both files short enough to read at once.

use git::repository::Branch;
use gpui::{AnyElement, ClickEvent};
use project::git_store::RepositoryId;
use ui::{Chip, Indicator, Tooltip, prelude::*};

use crate::branch_panel::panel::BranchPanel;

impl BranchPanel {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn branch_card(
        &self,
        ix: usize,
        indent: Pixels,
        id: RepositoryId,
        branch: &Branch,
        agent_count: usize,
        expanded: bool,
        row: &crate::branch_panel::tree::TreeRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name: SharedString = branch.name().to_string().into();
        let is_head = branch.is_head;
        let tracking = Self::tracking_label(branch);
        let subtitle = branch_subtitle(branch);
        let (checkout, menu_branch) = (branch.clone(), branch.clone());

        let toggle_key = row.toggle_key();
        let colors = cx.theme().colors();
        let background = if is_head {
            colors.element_selected
        } else {
            colors.elevated_surface_background
        };
        let border = if is_head {
            colors.border_focused
        } else {
            colors.border_variant
        };
        let hover = colors.element_hover;
        let tooltip = name.clone();

        // The card is wrapped rather than given margins directly: it has to be
        // `w_full` to fill the panel instead of shrinking to its text, and a
        // `w_full` element with margins overflows its parent. The wrapper's
        // padding carries the indent and the gap between cards instead.
        div()
            .w_full()
            .pl(indent + px(8.))
            .pr_2()
            .py_1()
            .child(
                v_flex()
                    .id(("branch", ix))
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .gap_0p5()
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .bg(background)
                    .hover(|style| style.bg(hover))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1p5()
                            .child(Indicator::dot().color(if is_head {
                                Color::Accent
                            } else {
                                Color::Muted
                            }))
                            .child(div().flex_1().min_w_0().child(
                                Label::new(name).size(LabelSize::Small).truncate().color(
                                    if is_head {
                                        Color::Default
                                    } else {
                                        Color::Muted
                                    },
                                ),
                            ))
                            .when(is_head, |this| {
                                this.child(
                                    Chip::new("current")
                                        .label_size(LabelSize::XSmall)
                                        .label_color(Color::Accent),
                                )
                            })
                            .when_some(tracking, |this, label| {
                                this.child(
                                    Label::new(label)
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted),
                                )
                            }),
                    )
                    .when(agent_count > 0, |this| {
                        // Its own line, and its own click target. The title row
                        // already carries a name that needs the width, and a
                        // click there already means "check this branch out".
                        this.child(
                            h_flex()
                                .id(("branch-agents", ix))
                                .gap_1()
                                .child(
                                    Icon::new(if expanded {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronRight
                                    })
                                    .size(IconSize::XSmall)
                                    .color(Color::Muted),
                                )
                                .child(
                                    Label::new(if agent_count == 1 {
                                        "1 agent".to_string()
                                    } else {
                                        format!("{agent_count} agents")
                                    })
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                                )
                                .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                                    // Without this the click also reaches the
                                    // card and checks the branch out -- opening
                                    // a list is not asking to switch to it.
                                    cx.stop_propagation();
                                    if let Some(key) = toggle_key.clone() {
                                        panel.toggle_row(key, cx);
                                    }
                                })),
                        )
                    })
                    .child(
                        Label::new(subtitle)
                            .size(LabelSize::XSmall)
                            .color(Color::Disabled)
                            .truncate(),
                    )
                    .tooltip(move |_, cx| Tooltip::simple(tooltip.clone(), cx))
                    .on_click(cx.listener(move |panel, _: &ClickEvent, window, cx| {
                        panel.checkout_branch(id, checkout.clone(), window, cx);
                    }))
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(move |panel, event: &gpui::MouseDownEvent, window, cx| {
                            let menu =
                                panel.branch_context_menu(id, menu_branch.clone(), window, cx);
                            panel.open_context_menu(menu, event.position, window, cx);
                        }),
                    ),
            )
            .into_any_element()
    }
}

/// The second line of a card: where the branch tracks, or failing that what was
/// last committed on it. A branch with neither says so outright -- an empty
/// second line reads as a rendering fault rather than as information.
fn branch_subtitle(branch: &Branch) -> SharedString {
    if let Some(name) = branch
        .upstream
        .as_ref()
        .and_then(|upstream| upstream.stripped_ref_name())
    {
        return name.to_string().into();
    }
    branch
        .most_recent_commit
        .as_ref()
        .map(|commit| commit.subject.clone())
        .unwrap_or_else(|| "no upstream".into())
}
