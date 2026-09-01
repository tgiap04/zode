//! The small shared piece of a leaf row.
//!
//! Worktrees, stashes and tags get a single line rather than a card: they are
//! context for the branches, not the thing the panel is about.

use gpui::Div;
use ui::prelude::*;

use crate::branch_panel::panel::BranchPanel;

impl BranchPanel {
    pub(super) fn leaf_row(
        id: (&'static str, usize),
        indent: Pixels,
        icon: IconName,
        label: SharedString,
    ) -> gpui::Stateful<Div> {
        h_flex()
            .id(id)
            .w_full()
            .pl(indent + px(12.))
            .pr_2()
            .py_0p5()
            .gap_1p5()
            .child(Icon::new(icon).size(IconSize::XSmall).color(Color::Muted))
            .child(
                div().flex_1().min_w_0().child(
                    Label::new(label)
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .truncate(),
                ),
            )
    }
}
