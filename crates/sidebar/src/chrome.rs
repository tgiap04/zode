use crate::Sidebar;
use gpui::Context;
use recent_projects::sidebar_recent_projects::SidebarRecentProjects;
use ui::{PopoverMenu, Tooltip, prelude::*};

impl Sidebar {
    pub(crate) fn render_filter_input(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_1()
            .child(self.filter_editor.clone())
            .when(self.has_filter_query(cx), |this| {
                this.child(
                    IconButton::new("clear-filter", IconName::Close)
                        .icon_size(IconSize::Small)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.reset_filter_editor_text(window, cx);
                        })),
                )
            })
    }

    /// FR4: the "add project" button opens `SidebarRecentProjects`'s
    /// existing popover -- that popover already implements the whole
    /// picker/connect flow, this just triggers it.
    pub(crate) fn render_recent_projects_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.active_workspace(cx).map(|ws| ws.downgrade());
        let project_groups = self
            .multi_workspace
            .upgrade()
            .map(|mw| mw.read(cx).project_group_keys())
            .unwrap_or_default();
        let focus_handle = self.focus_handle.clone();
        PopoverMenu::new("recent-projects-popover")
            .with_handle(self.recent_projects_popover_handle.clone())
            .trigger(
                IconButton::new("add-project", IconName::Plus)
                    .icon_size(IconSize::Small)
                    .tooltip(Tooltip::text("Add Project")),
            )
            .menu(move |window, cx| {
                let workspace = workspace.clone()?;
                Some(SidebarRecentProjects::popover(
                    workspace,
                    project_groups.clone(),
                    focus_handle.clone(),
                    window,
                    cx,
                ))
            })
            .anchor(gpui::Anchor::BottomLeft)
    }

    pub(crate) fn render_empty_state(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                Icon::new(IconName::FileTree)
                    .size(IconSize::XLarge)
                    .color(Color::Muted),
            )
            .child(Label::new("No projects open").color(Color::Muted))
    }

    pub(crate) fn render_no_results(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .child(Label::new("No matching projects").color(Color::Muted))
    }
}
