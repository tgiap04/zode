use crate::Sidebar;
use crate::project_list::ListEntry;
use gpui::{AnyElement, App, Context, SharedString, Window, px};
use settings::Settings as _;
use ui::{Tooltip, prelude::*};
use workspace::{SidebarSide, WorkspaceSettings};

/// The edge the whole sidebar column stands against. Every part of the column
/// that has a side -- the order of rail and panel, the rail's own separator, the
/// active-project pill -- reads this one value, so they cannot end up mirrored
/// against each other.
pub(crate) fn rail_side(cx: &App) -> SidebarSide {
    WorkspaceSettings::get_global(cx).multi_project.sidebar_side
}

/// Width of the always-visible project rail. Sized so a 32px project
/// square sits centred with room for the active-project indicator on the
/// leading edge.
pub const RAIL_WIDTH: Pixels = px(48.0);

const RAIL_ITEM_SIZE: Pixels = px(48.0);

/// One knob for every glyph in the column, so they cannot drift apart.
///
/// Worth knowing if you tune it: `IconSize` is rems against the UI font size, not
/// pixels -- `Small` is `rems_from_px(14.)`, which on the shipped 13px font draws
/// about 11px, while the column's own 48px width does not move.
pub(crate) const RAIL_ICON_SIZE: IconSize = IconSize::Small;

/// Absolute, not `gap_1`: spacing helpers are rems against the UI font, so on the
/// shipped 13px font `gap_1` yields 3px, and 3px between 19.5px glyphs in a fixed
/// 48px column reads as one run-on strip. The column's width does not shrink with
/// the font, so neither should the air between its buttons.
pub(crate) const RAIL_ICON_GAP: Pixels = px(10.0);
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

    /// The always-visible project switcher. Unlike the panel, this is not
    /// gated on `MultiWorkspace::sidebar_open` -- it is the primary way to
    /// switch projects, and the panel (with its filter input and project
    /// names) is the secondary, on-demand view over the same data.
    pub(crate) fn render_rail(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let panels = self.render_rail_panels(window, cx);
        let has_panels = panels.is_some();
        let colors = cx.theme().colors();
        let entries = self.contents.rail_entries.clone();
        let side = rail_side(cx);

        v_flex()
            .id("project-rail")
            .debug_selector(|| "project-rail".into())
            .h_full()
            .w(RAIL_WIDTH)
            .flex_shrink_0()
            .bg(colors.title_bar_background)
            // The separator belongs on the edge facing the rest of the window,
            // which flips with the column. Drawn on the fixed right it would sit
            // against the window frame on a right-docked rail and leave the seam
            // that matters -- rail against panel -- with nothing on it.
            .map(|el| match side {
                SidebarSide::Left => el.border_r_1(),
                SidebarSide::Right => el.border_l_1(),
            })
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
                            .map(|(ix, entry)| self.render_rail_item(ix, entry, side, cx)),
                    ),
            )
            .children(panels)
            .child(self.render_rail_footer(has_panels, cx))
            .into_any_element()
    }

    fn render_rail_item(
        &self,
        ix: usize,
        entry: &ListEntry,
        side: SidebarSide,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
            // Discord-style rail this is modelled on. It rides the rail's outer
            // edge, so it flips with the column rather than crossing to the side
            // the separator is already on.
            .when(is_active, |el| {
                el.child(
                    div()
                        .absolute()
                        .map(|pill| match side {
                            SidebarSide::Left => pill.left_0(),
                            SidebarSide::Right => pill.right_0(),
                        })
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
    ///
    /// `follows_panels` draws the seam against the panel switcher above. Absent
    /// that block the footer sits straight under the project squares, where a
    /// rule would divide nothing.
    fn render_rail_footer(&self, follows_panels: bool, cx: &mut Context<Self>) -> AnyElement {
        let panel_open = self.panel_open(cx);
        let border = cx.theme().colors().border;

        v_flex()
            .flex_shrink_0()
            .py(RAIL_ICON_GAP)
            .gap(RAIL_ICON_GAP)
            .items_center()
            .when(follows_panels, |el| el.border_t_1().border_color(border))
            .child(
                // Not a tree glyph: the panel switcher directly above already
                // carries `FileTree` and `ListTree` from the project and
                // outline panels, and a third tree in the same column reads as
                // a duplicate.
                IconButton::new("project-rail-toggle-panel", IconName::Menu)
                    .icon_size(RAIL_ICON_SIZE)
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
                    .icon_size(RAIL_ICON_SIZE)
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
