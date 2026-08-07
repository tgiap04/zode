use crate::Sidebar;
use gpui::{AnyElement, Context, SharedString, px};
use project::ProjectGroupKey;
use std::hash::{Hash, Hasher};
use ui::{ContextMenu, PopoverMenu, PopoverMenuHandle, prelude::*};

/// A stable id for `project_group_key`, for element/handle ids that must
/// stay tied to the project itself rather than its current list position
/// -- unlike a list index, this doesn't go stale if the entry list
/// reorders or shrinks while this project's menu is open.
fn stable_id_for_group(project_group_key: &ProjectGroupKey) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    project_group_key.hash(&mut hasher);
    hasher.finish()
}

impl Sidebar {
    /// FR5: close ("Remove Project") and move-to-new-window. Deliberately
    /// simpler than the pre-hard-fork sidebar's version, which also listed
    /// every open workspace within a group individually with its own close
    /// button -- that supported a "several workspaces share one project
    /// group" scenario this phase doesn't call for; add it back if a real
    /// need for it shows up.
    pub(crate) fn render_project_header_ellipsis_menu(
        &self,
        project_group_key: &ProjectGroupKey,
        group_name: &SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let multi_workspace = self.multi_workspace.clone();
        let group_id = stable_id_for_group(project_group_key);
        let project_group_key = project_group_key.clone();

        let show_open_in_new_window = multi_workspace
            .read_with(cx, |mw, _| {
                project_group_key.host().is_none() && mw.project_group_keys().len() >= 2
            })
            .unwrap_or(false);

        let menu_handle: PopoverMenuHandle<ContextMenu> = self
            .project_header_menu_handles
            .get(&project_group_key)
            .cloned()
            .unwrap_or_default();
        let is_menu_open = menu_handle.is_deployed();

        PopoverMenu::new(format!("project-header-menu-{group_id}"))
            .with_handle(menu_handle)
            .trigger(
                IconButton::new(
                    SharedString::from(format!("ellipsis-menu-{group_id}")),
                    IconName::Ellipsis,
                )
                .icon_size(IconSize::Small)
                .when(!is_menu_open, |el| el.visible_on_hover(group_name)),
            )
            .menu(move |window, cx| {
                let project_group_key = project_group_key.clone();
                let open_multi_workspace = multi_workspace.clone();
                let remove_multi_workspace = multi_workspace.clone();

                Some(ContextMenu::build(window, cx, move |menu, _window, _cx| {
                    menu.when(show_open_in_new_window, |this| {
                        let project_group_key = project_group_key.clone();
                        let open_multi_workspace = open_multi_workspace.clone();
                        this.entry(
                            "Open Project in New Window",
                            Some(Box::new(workspace::MoveProjectToNewWindow)),
                            move |window, cx| {
                                open_multi_workspace
                                    .update(cx, |mw, cx| {
                                        mw.open_project_group_in_new_window(
                                            &project_group_key,
                                            window,
                                            cx,
                                        )
                                        .detach_and_log_err(cx);
                                    })
                                    .ok();
                            },
                        )
                    })
                    .entry("Remove Project", None, {
                        let project_group_key = project_group_key.clone();
                        move |window, cx| {
                            remove_multi_workspace
                                .update(cx, |mw, cx| {
                                    mw.remove_project_group(&project_group_key, window, cx)
                                        .detach_and_log_err(cx);
                                })
                                .ok();
                        }
                    })
                }))
            })
            .anchor(gpui::Anchor::TopRight)
            .offset(gpui::Point {
                x: px(0.),
                y: px(1.),
            })
            .into_any_element()
    }
}
