use crate::Sidebar;
use crate::project_list::ListEntry;
use gpui::{AnyElement, Context, SharedString, Window, px};
use remote::RemoteConnectionOptions;
use ui::{GradientFade, HighlightedLabel, Tab, Tooltip, prelude::*};
use workspace::project_avatar::ProjectAvatar;

impl Sidebar {
    /// FR2: renders one project row. `is_group_header_after_first` (a
    /// border separating consecutive rows) is applied by the caller in
    /// the list's own render loop, not here.
    pub(crate) fn render_list_entry(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(entry) = self.contents.entries.get(ix).cloned() else {
            return div().into_any_element();
        };
        let is_focused = self.focus_handle.contains_focused(window, cx);
        let is_selected = is_focused && self.selection == Some(ix);
        self.project_header_menu_handles
            .entry(entry.key.clone())
            .or_default();
        self.render_project_header(ix, &entry, is_selected, cx)
    }

    pub(crate) fn render_remote_project_icon(
        &self,
        ix: usize,
        host: Option<&RemoteConnectionOptions>,
    ) -> Option<AnyElement> {
        let remote_icon_per_type = match host? {
            RemoteConnectionOptions::Wsl(_) => IconName::Linux,
            RemoteConnectionOptions::Docker(_) => IconName::Box,
            _ => IconName::Server,
        };

        Some(
            div()
                .id(format!("remote-project-icon-{ix}"))
                .child(
                    Icon::new(remote_icon_per_type)
                        .size(IconSize::XSmall)
                        .color(Color::Muted),
                )
                .tooltip(Tooltip::text("Remote Project"))
                .into_any_element(),
        )
    }

    /// FR7: surfaces `ProjectActivity` on the row -- re-indexing takes
    /// priority over the hibernated icon since it means the project just
    /// woke and is mid-restart, a more specific (and more actionable)
    /// state than "asleep."
    fn render_activity_indicator(&self, ix: usize, entry: &ListEntry) -> Option<AnyElement> {
        if entry.is_reindexing {
            return Some(
                div()
                    .id(format!("reindexing-icon-{ix}"))
                    .child(
                        Icon::new(IconName::Warning)
                            .size(IconSize::XSmall)
                            .color(Color::Warning),
                    )
                    .tooltip(Tooltip::text("Re-indexing after waking"))
                    .into_any_element(),
            );
        }
        if entry.activity == Some(project::ProjectActivity::Hibernated) {
            return Some(
                div()
                    .id(format!("hibernated-icon-{ix}"))
                    .child(
                        Icon::new(IconName::Clock)
                            .size(IconSize::XSmall)
                            .color(Color::Muted),
                    )
                    .tooltip(Tooltip::text("Hibernated — will wake when opened"))
                    .into_any_element(),
            );
        }
        None
    }

    /// FR2: a project header row. Click activates the project
    /// (`activate_or_open_workspace_for_group`) — there's no expand/collapse
    /// here, see `ListEntry`'s own doc comment for why.
    fn render_project_header(
        &self,
        ix: usize,
        entry: &ListEntry,
        is_focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = &entry.key;
        let host = key.host();
        let id = SharedString::from(format!("project-header-{ix}"));
        let group_name = SharedString::from(format!("header-group-{ix}"));

        let workspace::ProjectPresentation {
            initials: custom_initials,
            colour: custom_colour,
            logo,
        } = self
            .multi_workspace
            .read_with(cx, |multi_workspace, _| {
                multi_workspace.project_presentation(key)
            })
            .unwrap_or_default();
        let initials =
            custom_initials.unwrap_or_else(|| crate::rail_item::project_initials(&entry.label));

        let label = if entry.highlight_positions.is_empty() {
            Label::new(entry.label.clone())
                .when(!entry.is_active, |this| this.color(Color::Muted))
                .into_any_element()
        } else {
            HighlightedLabel::new(entry.label.clone(), entry.highlight_positions.clone())
                .when(!entry.is_active, |this| this.color(Color::Muted))
                .into_any_element()
        };

        let color = cx.theme().colors();
        // The rail sits on the same ground as the buffer beside it, so it takes the
        // title bar's colour and nothing else.
        //
        // This used to blend a quarter of `panel_background` over that, which read
        // as a grey lift against the editor. The bundled themes now carry the same
        // value for both tokens, so the blend would be a no-op there anyway -- but
        // it is removed rather than left in, because on any theme where the two
        // differ (every third-party one) the tint comes straight back.
        //
        // `background.blend` is kept: `title_bar_background` is allowed to be
        // translucent, and a theme that makes it so needs something underneath.
        let base_bg = color.background.blend(color.title_bar_background);
        let hover_base = color
            .element_active
            .blend(color.element_background.opacity(0.2));
        let hover_solid = base_bg.blend(hover_base);

        let group_name_for_gradient = group_name.clone();
        let gradient_overlay = move || {
            GradientFade::new(base_bg, hover_solid, hover_solid)
                .width(px(64.0))
                .right(px(-2.0))
                .gradient_stop(0.75)
                .group_name(group_name_for_gradient.clone())
        };

        let key_for_click = key.clone();

        h_flex()
            .id(id)
            .group(&group_name)
            .cursor_pointer()
            .relative()
            .h(Tab::content_height(cx))
            .w_full()
            .pl_2()
            .pr_1p5()
            .justify_between()
            .border_1()
            .map(|this| {
                if is_focused {
                    this.border_color(color.border_focused)
                } else {
                    this.border_color(gpui::transparent_black())
                }
            })
            .hover(|s| s.bg(hover_solid))
            .child(
                h_flex()
                    .relative()
                    .min_w_0()
                    .w_full()
                    .gap_1()
                    // A logo is drawn only when the project has one -- this row
                    // carries no avatar today, and always drawing an initials
                    // square here would be a design change nobody asked for.
                    .when_some(logo, |this, logo| {
                        this.child(
                            div()
                                .flex_shrink_0()
                                .debug_selector(move || format!("project-header-avatar:{ix}"))
                                .child(
                                    ProjectAvatar::new(initials.clone(), custom_colour, Some(logo))
                                        .size(px(16.))
                                        .background(color.element_background),
                                ),
                        )
                    })
                    .child(label)
                    .when_some(
                        self.render_remote_project_icon(ix, host.as_ref()),
                        |this, icon| this.child(icon),
                    )
                    .when_some(self.render_activity_indicator(ix, entry), |this, icon| {
                        this.child(icon)
                    }),
            )
            .child(gradient_overlay())
            .child(
                h_flex()
                    .child(gradient_overlay())
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(self.render_project_header_ellipsis_menu(key, &group_name, cx)),
            )
            .on_mouse_down(gpui::MouseButton::Right, {
                let menu_handle = self
                    .project_header_menu_handles
                    .get(key)
                    .cloned()
                    .unwrap_or_default();
                move |_, window, cx| {
                    cx.stop_propagation();
                    menu_handle.toggle(window, cx);
                }
            })
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                this.activate_or_open_workspace_for_group(&key_for_click, window, cx);
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use crate::Sidebar;
    use crate::sidebar_tests::init_test;
    use fs::FakeFs;
    use gpui::{AppContext as _, TestAppContext, px};
    use project::Project;
    use serde_json::json;
    use util::path;
    use workspace::MultiWorkspace;

    /// A project row draws its avatar only once a logo has actually been set
    /// -- this row has no avatar today, so a bare initials square here would
    /// be a design change nobody asked for -- and a long project name must
    /// shrink the label rather than the fixed-size avatar next to it.
    #[gpui::test]
    async fn the_panel_row_draws_the_same_avatar(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        let long_name = "a-very-long-project-name-that-should-truncate-in-the-panel-row";
        let project_root = std::path::Path::new(path!("/root_a")).join(long_name);
        fs.insert_tree(
            path!("/root_a"),
            json!({ long_name: { "a.txt": "" }, "source_logo.png": "" }),
        )
        .await;
        let project = Project::test(fs, [project_root.as_path()], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

        multi_workspace.update_in(cx, |mw, window, cx| {
            let mw_entity = cx.entity();
            let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
            mw.register_sidebar(sidebar, cx);
        });
        // The row lives in the panel, not the rail -- it draws nothing until
        // the panel is actually open.
        multi_workspace.update(cx, |mw, cx| mw.open_sidebar(cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        // No logo yet: the row must not carry an avatar probe at all.
        assert!(
            cx.debug_bounds("project-header-avatar:0").is_none(),
            "a project without a logo must not draw an avatar on the panel row"
        );

        let key = multi_workspace.read_with(cx, |mw, cx| mw.project_groups(cx)[0].key.clone());
        multi_workspace
            .update(cx, |mw, cx| {
                mw.set_project_logo(
                    &key,
                    std::path::PathBuf::from(path!("/root_a/source_logo.png")),
                    cx,
                )
            })
            .await;
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        let avatar = cx
            .debug_bounds("project-header-avatar:0")
            .expect("with a logo set, the panel row must draw an avatar");
        assert_eq!(
            avatar.size.width,
            px(16.),
            "a long label must shrink instead of squeezing the fixed-size avatar"
        );
    }
}
