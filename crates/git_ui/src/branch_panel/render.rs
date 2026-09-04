use gpui::{AnyElement, FontWeight, IntoElement, Render, Window, list};
use ui::{Tooltip, prelude::*};

use crate::branch_panel::panel::BranchPanel;

impl Render for BranchPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_session_store(cx);
        // Rebuilding here rather than on each git event means a burst of events
        // costs one rebuild, and a panel nobody is looking at costs none.
        self.refresh_if_stale(cx);
        // After the rebuild: it reads the rows the rebuild just produced.
        self.track_agent_activity(cx);

        let empty = self.rows.is_empty();

        v_flex()
            .key_context("BranchPanel")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().colors().panel_background)
            .child(self.render_header(cx))
            .children(
                self.repos
                    .first()
                    .map(|repo| repo.id)
                    .and_then(|id| self.render_create_repo_prompt(id, cx)),
            )
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

    /// Title and one button.
    ///
    /// One button on purpose: `+` is the only thing this panel is *for* at the
    /// top level. Fetch, pull and push act on a repository rather than on the
    /// panel, so they live on the repository row's own menu -- putting them
    /// here made the header look like the panel's toolbar when it was really
    /// one repository's.
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
                Label::new("Worktrees")
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .weight(FontWeight::SEMIBOLD),
            )
            .children(repo_id.map(|id| {
                IconButton::new("create-worktree", IconName::Plus)
                    .icon_size(IconSize::Small)
                    .tooltip(|_, cx| Tooltip::simple("Create Worktree", cx))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.open_create_worktree_modal(id, window, cx);
                    }))
            }))
            .into_any_element()
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
