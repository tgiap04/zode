use crate::Sidebar;
use gpui::{AnyElement, App, Context, Window, px};
use ui::{Tooltip, prelude::*};
use workspace::pane_group::SURFACE_ROUNDING;

/// Width of the always-visible project rail. Sized so a 32px project
/// square sits centred with room for the active-project indicator on the
/// leading edge.
pub const RAIL_WIDTH: Pixels = px(48.0);

/// Every glyph in the column is drawn by this, so they cannot drift apart.
///
/// `ButtonSize` and `IconSize` are rems against the UI font size, not pixels. On
/// the shipped 15px font `Large` draws 30px and `Medium` 15px, just under the 32px
/// project squares, while the column's own 48px width does not move.
pub(crate) fn rail_button(id: impl Into<ElementId>, icon: IconName) -> IconButton {
    IconButton::new(id, icon)
        .size(ButtonSize::Large)
        .icon_size(IconSize::Medium)
}

/// Absolute, not a spacing helper: those are rems against the UI font, and the
/// column's width does not shrink with the font, so neither should the air
/// between its buttons. Small because a 30px button already pads its 15px glyph
/// by 7.5px on each side; this is added on top of that.
pub(crate) const RAIL_ICON_GAP: Pixels = px(4.0);

/// Between a block's first or last glyph and the rule that separates it from
/// the next block, wider than the gap between glyphs so a block reads as one.
pub(crate) const RAIL_BLOCK_PADDING: Pixels = px(8.0);

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
        // Copied out: `colors()` borrows `cx`, and the listeners below need it
        // mutably.
        let accent = ui::brand_accent(cx);
        let colors = cx.theme().colors().clone();
        let entries = self.contents.rail_entries.clone();

        v_flex()
            .id("project-rail")
            .debug_selector(|| "project-rail".into())
            // The rail catches every drop that lands on it, so a project
            // released on the footer or the agents block is simply not a
            // placement rather than a trip to a new window. Without a handler
            // here the window root would take it and ask to move the project out
            // -- "not on a row" is not the same question as "off the rail".
            .on_drop(
                cx.listener(|this, _dragged: &workspace::DraggedProject, _window, cx| {
                    this.set_drop_gap(None, cx);
                }),
            )
            .h_full()
            .w(RAIL_WIDTH)
            .flex_shrink_0()
            .bg(colors.title_bar_background)
            // A rule across the top, so the rail starts on the same line the
            // panels beside it do rather than running up into the title bar.
            .border_t_1()
            // The separator belongs on the edge facing the rest of the window,
            // which flips with the column. Drawn on the fixed right it would sit
            // against the window frame on a right-docked rail and leave the seam
            // that matters -- rail against panel -- with nothing on it.
            //
            // The corner rounds on that same inward edge, and only there: the
            // outward edge is flush against the window frame, where a radius
            // would open a notch onto the frame rather than a seam. Same radius
            // as the docks, which the rail now stands beside as a card of the
            // same layout.
            .border_r_1()
            .rounded_tr(SURFACE_ROUNDING)
            .border_color(colors.border)
            .child(
                v_flex()
                    .id("project-rail-items")
                    .debug_selector(|| "project-rail-items".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    // Only ever *clears*, and never claims a gap. Handlers here
                    // fire in an order that put this one after the rows -- it was
                    // overwriting a row's answer with its own -- so the rule is
                    // that every element claims only while the pointer is inside
                    // its own bounds, and those bounds do not overlap. Then the
                    // order stops mattering.
                    .on_drag_move(cx.listener(
                        |this,
                         event: &gpui::DragMoveEvent<workspace::DraggedProject>,
                         _window,
                         cx| {
                            if !event.bounds.contains(&event.event.position) {
                                this.set_drop_gap(None, cx);
                            }
                        },
                    ))
                    .on_drop(cx.listener(
                        |this, dragged: &workspace::DraggedProject, _window, cx| {
                            let Some(gap) = this.drop_gap else {
                                return;
                            };
                            let key = dragged.key.clone();
                            this.drop_project_at_gap(&key, gap, cx);
                        },
                    ))
                    .children(entries.iter().enumerate().flat_map(|(ix, entry)| {
                        // The line stands in the gap the project would land in,
                        // which is a place between rows rather than a row.
                        let indicator = (self.drop_gap == Some(ix)).then(|| drop_indicator(accent));
                        indicator
                            .into_iter()
                            .chain(std::iter::once(self.render_rail_item(ix, entry, cx)))
                    }))
                    .children(
                        (self.drop_gap == Some(entries.len())).then(|| drop_indicator(accent)),
                    )
                    // The empty space under the last row, as an element of its
                    // own so it can claim the end of the list without overlapping
                    // any row.
                    .child(div().flex_1().min_h_0().w_full().on_drag_move(cx.listener({
                        let count = entries.len();
                        move |this,
                              event: &gpui::DragMoveEvent<workspace::DraggedProject>,
                              _window,
                              cx| {
                            if event.bounds.contains(&event.event.position) {
                                this.set_drop_gap(Some(count), cx);
                            }
                        }
                    }))),
            )
            // The blocks between the project list and the footer scroll as a
            // group once they no longer fit.
            //
            // They used to be `flex_shrink_0` siblings of the footer, which on a
            // short window meant the scrolling project list collapsed to nothing
            // and then the *footer* was cut off -- taking Settings and Open
            // Project with it, the two buttons a person needs most when the rail
            // has run out of room. Nothing changes on a window tall enough to
            // hold them: a scroll container with room to spare draws no
            // scrollbar and takes no space.
            .child(
                v_flex()
                    .id("project-rail-blocks")
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(panels)
                    .child(self.render_rail_tools(window, cx))
                    .child(self.render_rail_agents(window, cx)),
            )
            .child(self.render_rail_footer(cx))
            .into_any_element()
    }

    /// The buttons for the two tools that open as editor tabs.
    ///
    /// One block, because they are one group: both open a tab of their own the
    /// way the agent buttons below them do, and the person who asked for the
    /// container button asked for it here, beside the database. Neither is a
    /// panel of the tool dock, so `render_rail_panels` would never draw either.
    fn render_rail_tools(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .flex_shrink_0()
            .py(RAIL_BLOCK_PADDING)
            .gap(RAIL_ICON_GAP)
            .items_center()
            .border_t_1()
            .border_color(cx.theme().colors().border)
            .child(self.render_rail_database(window, cx))
            .child(self.render_rail_container(window, cx))
            .into_any_element()
    }

    /// Toggle for the wide panel plus an "add project" button -- the rail
    /// alone shows no project names, so the panel needs a discoverable way
    /// in that isn't only the `cmd-alt-j` keybinding.
    ///
    /// The seam above is unconditional: the agent block always draws, so there is
    /// always a block between the project squares and this footer to rule off from.
    fn render_rail_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let panel_open = self.panel_open(cx);
        let border = cx.theme().colors().border;

        v_flex()
            .flex_shrink_0()
            .py(RAIL_BLOCK_PADDING)
            .gap(RAIL_ICON_GAP)
            .items_center()
            .border_t_1()
            .border_color(border)
            .child(
                // Not a tree glyph: the panel switcher directly above already
                // carries `ListTree` from the outline panel, and a second tree
                // in the same column reads as a duplicate. (The project panel
                // used to put a third one here; it stands in its own dock's
                // header now, under a folder.)
                rail_button("project-rail-toggle-panel", IconName::Menu)
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
                rail_button("project-rail-open-project", IconName::Plus)
                    .tooltip(move |_window, cx| {
                        Tooltip::for_action("Open Project", &workspace::Open::DEFAULT, cx)
                    })
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(workspace::Open::DEFAULT), cx);
                    }),
            )
            .child(
                // `OpenSettings` is the settings *window*; `OpenSettingsFile` is
                // settings.json as a tab. Worth checking twice, because
                // `OpenSettingsFile` claims the string "zed_actions::OpenSettings"
                // as a deprecated alias -- so that old spelling now resolves to
                // the JSON file, and a keymap copied from anywhere older wires
                // the wrong one of the two.
                rail_button("project-rail-open-settings", IconName::Settings)
                    .tooltip(move |_window, cx| {
                        Tooltip::for_action("Open Settings", &zed_actions::OpenSettings, cx)
                    })
                    // Dispatched rather than called, but not for the panel
                    // toggle's reason above: `on_click` hands out `&mut App`, so
                    // there is no `Sidebar` borrow here to be reentrant with.
                    // This crate simply has no `settings_ui` dependency to call
                    // into, and going through the action keeps the button on the
                    // same path as the command palette and any keybinding.
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(zed_actions::OpenSettings), cx);
                    }),
            )
            // Last, below the settings gear: every activity bar that has an
            // account puts it at the bottom, and a user looking for it looks
            // there first.
            .child(self.render_rail_account(cx))
            .into_any_element()
    }
}

/// The line that says where a dragged project will land.
fn drop_indicator(colour: gpui::Hsla) -> AnyElement {
    div()
        .w_full()
        .h(px(2.0))
        .flex_shrink_0()
        .bg(colour)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use crate::rail_test_support::{TwoProjects, two_projects};
    use gpui::TestAppContext;
    use ui::ButtonSize;

    /// Every glyph in the column is one size, whichever block draws it. A
    /// button drawn by hand with its own `IconButton::new` would leave the
    /// default 22px box among 32px ones, and nothing but a measurement notices.
    #[gpui::test]
    async fn every_rail_glyph_button_is_large(cx: &mut TestAppContext) {
        let (TwoProjects { .. }, cx) = two_projects(cx).await;
        let rem_size = cx.update(|window, _| window.rem_size());
        let large = ButtonSize::Large.rems().to_pixels(rem_size);

        for selector in [
            "ICON-Menu",
            "ICON-Plus",
            "ICON-Settings",
            "ICON-Database",
            "ICON-Docker",
            "ICON-AiClaude",
        ] {
            let bounds = cx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} must be drawn on the rail"));
            assert_eq!(bounds.size.height, large, "{selector} height");
            assert_eq!(bounds.size.width, large, "{selector} width");
        }
    }

    /// One glyph per meaning: the database tool is `Database` here and in every
    /// other surface that names it, not the `DatabaseZap` variant.
    #[gpui::test]
    async fn the_database_button_wears_the_database_glyph(cx: &mut TestAppContext) {
        let (TwoProjects { .. }, cx) = two_projects(cx).await;
        assert!(cx.debug_bounds("ICON-Database").is_some());
        assert!(cx.debug_bounds("ICON-DatabaseZap").is_none());
    }
}
