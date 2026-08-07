use crate::Sidebar;
use crate::project_list::ListEntry;
use gpui::{AnyElement, App, Context, SharedString, Window, px};
use ui::{Tooltip, prelude::*};
use workspace::{Sidebar as WorkspaceSidebar, SidebarSide};

/// Width of the always-visible project rail. Sized so a 32px project
/// square sits centred with room for the active-project indicator on the
/// leading edge.
pub const RAIL_WIDTH: Pixels = px(48.0);

const RAIL_ITEM_SIZE: Pixels = px(48.0);
const RAIL_SQUARE_SIZE: Pixels = px(32.0);

/// One or two letters standing in for the project, Discord-style. Word
/// boundaries win over raw prefix characters so `my-cool-app` reads `MA`
/// rather than `MY`.
fn project_initials(label: &str) -> SharedString {
    let mut initials = String::new();
    for word in label.split(|c: char| !c.is_alphanumeric()) {
        let Some(first) = word.chars().next() else {
            continue;
        };
        initials.extend(first.to_uppercase());
        if initials.chars().count() == 2 {
            return initials.into();
        }
    }
    if initials.is_empty() {
        // Nothing alphanumeric to work with (e.g. a path of only
        // separators) -- fall back to raw leading characters so the square
        // is never blank.
        initials.extend(label.chars().take(2).flat_map(char::to_uppercase));
    }
    initials.into()
}

fn rail_tooltip(entry: &ListEntry) -> SharedString {
    if entry.is_reindexing {
        format!("{} — re-indexing after waking", entry.label).into()
    } else if entry.activity == Some(project::ProjectActivity::Hibernated) {
        format!("{} — hibernated", entry.label).into()
    } else {
        entry.label.clone()
    }
}

impl Sidebar {
    /// Whether the wide panel is showing. Owned by `MultiWorkspace` rather
    /// than this entity, because the toggle action and its keybinding are
    /// registered there.
    pub(crate) fn panel_open(&self, cx: &App) -> bool {
        self.multi_workspace
            .upgrade()
            .is_some_and(|multi_workspace| multi_workspace.read(cx).sidebar_open())
    }

    /// macOS draws its window controls over the window's own top-left
    /// corner, which the rail occupies now that it is always visible.
    /// Reserve that strip so the topmost project square stays clickable.
    pub(crate) fn top_inset(&self, window: &Window, cx: &App) -> Pixels {
        if cfg!(target_os = "macos")
            && !window.is_fullscreen()
            && self.side(cx) == SidebarSide::Left
        {
            ui::utils::platform_title_bar_height(window)
        } else {
            px(0.0)
        }
    }

    /// The always-visible project switcher. Unlike the panel, this is not
    /// gated on `MultiWorkspace::sidebar_open` -- it is the primary way to
    /// switch projects, and the panel (with its filter input and project
    /// names) is the secondary, on-demand view over the same data.
    pub(crate) fn render_rail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors();
        let entries = self.contents.rail_entries.clone();

        v_flex()
            .id("project-rail")
            .h_full()
            .w(RAIL_WIDTH)
            .flex_shrink_0()
            .bg(colors.title_bar_background)
            .border_r_1()
            .border_color(colors.border)
            .child(
                v_flex()
                    .id("project-rail-items")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(
                        entries
                            .iter()
                            .enumerate()
                            .map(|(ix, entry)| self.render_rail_item(ix, entry, cx)),
                    ),
            )
            .child(self.render_rail_footer(cx))
            .into_any_element()
    }

    fn render_rail_item(&self, ix: usize, entry: &ListEntry, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors();
        let warning = cx.theme().status().warning;
        let is_active = entry.is_active;
        let is_hibernated = entry.activity == Some(project::ProjectActivity::Hibernated);

        let square_bg = if is_active {
            colors.element_selected
        } else {
            colors.element_background
        };
        let key_for_click = entry.key.clone();

        div()
            .id(("project-rail-item", ix))
            .relative()
            .w_full()
            .h(RAIL_ITEM_SIZE)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            // Leading indicator pill marking the active project, in the
            // Discord-style rail this is modelled on.
            .when(is_active, |el| {
                el.child(
                    div()
                        .absolute()
                        .left_0()
                        .h(px(24.0))
                        .w(px(3.0))
                        .rounded_sm()
                        .bg(colors.text_accent),
                )
            })
            .child(
                div()
                    .size(RAIL_SQUARE_SIZE)
                    .rounded_md()
                    .bg(square_bg)
                    .border_1()
                    .map(|el| {
                        if is_active {
                            el.border_color(colors.border_selected)
                        } else {
                            el.border_color(colors.border_transparent)
                        }
                    })
                    .when(is_hibernated && !is_active, |el| el.opacity(0.6))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Label::new(project_initials(&entry.label))
                            .size(LabelSize::Small)
                            .color(if is_active {
                                Color::Default
                            } else {
                                Color::Muted
                            }),
                    ),
            )
            // FR7 parity with the panel rows: a project mid-reindex after
            // waking gets a corner dot, since the rail has no room for the
            // panel's icon-plus-tooltip treatment.
            .when(entry.is_reindexing, |el| {
                el.child(
                    div()
                        .absolute()
                        .top(px(6.0))
                        .right(px(6.0))
                        .size(px(6.0))
                        .rounded_full()
                        .bg(warning),
                )
            })
            .hover(|s| s.bg(colors.element_hover))
            .tooltip(Tooltip::text(rail_tooltip(entry)))
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                this.activate_or_open_workspace_for_group(&key_for_click, window, cx);
            }))
            .into_any_element()
    }

    /// Toggle for the wide panel plus an "add project" button -- the rail
    /// alone shows no project names, so the panel needs a discoverable way
    /// in that isn't only the `cmd-alt-j` keybinding.
    fn render_rail_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let panel_open = self.panel_open(cx);

        v_flex()
            .flex_shrink_0()
            .py_1()
            .gap_1()
            .items_center()
            .child(
                IconButton::new("project-rail-toggle-panel", IconName::ListTree)
                    .icon_size(IconSize::Small)
                    .toggle_state(panel_open)
                    .tooltip(move |_window, cx| {
                        Tooltip::for_action(
                            if panel_open {
                                "Hide Project List"
                            } else {
                                "Show Project List"
                            },
                            &workspace::ToggleWorkspaceSidebar,
                            cx,
                        )
                    })
                    // Dispatch rather than calling `MultiWorkspace::toggle_sidebar`
                    // directly: a `cx.listener` body runs inside `Sidebar::update`,
                    // and `toggle_sidebar` reaches back through `SidebarHandle`
                    // (`prepare_for_focus`/`focus`), which borrows this very entity
                    // again. `Window::dispatch_action` defers, so the borrow is
                    // released before the action runs.
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(workspace::ToggleWorkspaceSidebar), cx);
                    }),
            )
            .child(
                IconButton::new("project-rail-open-project", IconName::Plus)
                    .icon_size(IconSize::Small)
                    .tooltip(move |_window, cx| {
                        Tooltip::for_action("Open Project", &workspace::Open::DEFAULT, cx)
                    })
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(workspace::Open::DEFAULT), cx);
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::project_initials;

    #[test]
    fn initials_prefer_word_boundaries() {
        assert_eq!(project_initials("my-cool-app").as_ref(), "MC");
        assert_eq!(project_initials("zode").as_ref(), "Z");
        assert_eq!(project_initials("examio_be").as_ref(), "EB");
        assert_eq!(project_initials("").as_ref(), "");
        assert_eq!(project_initials("///").as_ref(), "//");
    }
}
