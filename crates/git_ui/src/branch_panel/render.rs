use gpui::{AnyElement, FontWeight, IntoElement, Render, Window, list};
use ui::{Tooltip, prelude::*};

use crate::branch_panel::panel::BranchPanel;
use crate::branch_panel::remote::RemoteOp;

impl Render for BranchPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Rebuilding here rather than on each git event means a burst of events
        // costs one rebuild, and a panel nobody is looking at costs none.
        self.refresh_if_stale(cx);

        let empty = self.rows.is_empty();

        v_flex()
            .key_context("BranchPanel")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().colors().panel_background)
            .on_action(cx.listener(|panel, _: &menu::Confirm, window, cx| {
                panel.confirm_new_branch(window, cx)
            }))
            .on_action(cx.listener(|panel, _: &menu::Cancel, _, cx| panel.cancel_new_branch(cx)))
            .child(self.render_header(cx))
            .children(
                self.repos
                    .first()
                    .map(|repo| repo.id)
                    .and_then(|id| self.render_create_repo_prompt(id, cx)),
            )
            .children(self.render_filter(cx))
            .children(self.render_new_branch_field(cx))
            .child(if empty {
                self.render_empty(cx)
            } else {
                list(
                    self.list_state.clone(),
                    cx.processor(|panel, ix: usize, _window, cx| match panel.rows.get(ix) {
                        Some(row) => {
                            let row = row.clone();
                            panel.render_row(ix, &row, cx)
                        }
                        None => div().into_any_element(),
                    }),
                )
                .size_full()
                .into_any_element()
            })
            .children(self.render_context_menu())
    }
}

impl BranchPanel {
    /// Deferred so the menu paints above the list rather than inside its
    /// scroll area.
    fn render_context_menu(&self) -> Option<AnyElement> {
        let (menu, position, _) = self.context_menu.as_ref()?;
        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(*position)
                    .anchor(gpui::Anchor::TopLeft)
                    .child(menu.clone()),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// The panel's own title bar: what this panel is, and the actions that
    /// belong to the whole panel rather than to one row. The remote operations
    /// live here because they are the ones worth a single click; everything
    /// per-repository is on the repository row itself.
    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let repo_id = self.repos.first().map(|repo| repo.id);

        h_flex()
            .w_full()
            .flex_none()
            .h(px(34.))
            .px_2()
            .justify_between()
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                Label::new("Branches")
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .weight(FontWeight::SEMIBOLD),
            )
            .child(
                h_flex()
                    .gap_0p5()
                    .child(
                        IconButton::new("toggle-filter", IconName::Sliders)
                            .icon_size(IconSize::Small)
                            .toggle_state(self.filter_visible)
                            .tooltip(|_, cx| Tooltip::simple("Filter Branches", cx))
                            .on_click(
                                cx.listener(|panel, _, window, cx| panel.toggle_filter(window, cx)),
                            ),
                    )
                    .when_some(repo_id, |this, id| {
                        this.child(self.remote_button(id, RemoteOp::Fetch, cx))
                            .child(self.remote_button(id, RemoteOp::Pull, cx))
                            .child(self.remote_button(id, RemoteOp::Push, cx))
                    }),
            )
            .into_any_element()
    }

    /// How far a branch has drifted from its upstream. `None` when there is no
    /// upstream or the two are level -- "up 0 down 0" is noise.
    pub(crate) fn tracking_label(branch: &git::repository::Branch) -> Option<SharedString> {
        let status = branch.tracking_status()?;
        (status.ahead + status.behind > 0)
            .then(|| format!("\u{2191}{} \u{2193}{}", status.ahead, status.behind).into())
    }

    fn render_filter(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if !self.filter_visible {
            return None;
        }
        Some(
            h_flex()
                .w_full()
                .flex_none()
                .px_2()
                .py_1()
                .border_b_1()
                .border_color(cx.theme().colors().border_variant)
                .child(self.filter_editor.clone())
                .into_any_element(),
        )
    }

    /// Distinguishes "this project has no git repository" from "the panel is
    /// broken" -- a blank panel says neither.
    fn render_empty(&self, _cx: &Context<Self>) -> AnyElement {
        v_flex()
            .size_full()
            .p_3()
            .child(
                Label::new("No git repository in this project")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .into_any_element()
    }
}
