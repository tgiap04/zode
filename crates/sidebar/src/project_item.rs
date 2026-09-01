use crate::Sidebar;
use crate::project_list::{ListEntry, PanelRow, WorktreeRow};
use gpui::{AnyElement, Context, SharedString, Window, px};
use remote::RemoteConnectionOptions;
use ui::{GradientFade, HighlightedLabel, Tab, Tooltip, prelude::*};

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
            .entry(entry.key().clone())
            .or_default();
        match entry {
            PanelRow::Project(entry) => self.render_project_header(ix, &entry, is_selected, cx),
            PanelRow::Worktree(row) => self.render_worktree_row(ix, &row, is_selected, cx),
        }
    }

    /// One open workspace under a project: its branch, and whether it is the
    /// repository's own checkout or a linked worktree.
    ///
    /// Indented rather than iconified into the header, because the point of the
    /// row is that several of them sit under one project at once -- that is
    /// what working on features in parallel looks like from here.
    fn render_worktree_row(
        &self,
        ix: usize,
        row: &WorktreeRow,
        is_selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let workspace = row.workspace.clone();
        let tooltip: SharedString = row.path.display().to_string().into();
        let colors = cx.theme().colors();
        let background = if row.is_active {
            colors.element_selected
        } else {
            colors.elevated_surface_background
        };
        let border = if row.is_active {
            colors.border_focused
        } else {
            colors.border_variant
        };
        let hover = colors.element_hover;

        div()
            .w_full()
            .pl_4()
            .pr_2()
            .py_0p5()
            .child(
                h_flex()
                    .id(("worktree-row", ix))
                    .w_full()
                    .gap_1p5()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .bg(background)
                    .when(is_selected, |this| this.border_color(colors.border_focused))
                    .hover(|style| style.bg(hover))
                    .child(
                        Icon::new(if row.is_main {
                            IconName::FolderGit
                        } else {
                            IconName::GitWorktree
                        })
                        .size(IconSize::XSmall)
                        .color(if row.is_active {
                            Color::Accent
                        } else {
                            Color::Muted
                        }),
                    )
                    .child(
                        div().flex_1().min_w_0().child(
                            Label::new(row.label.clone())
                                .size(LabelSize::Small)
                                .truncate()
                                .color(if row.is_active {
                                    Color::Default
                                } else {
                                    Color::Muted
                                }),
                        ),
                    )
                    .tooltip(move |_, cx| Tooltip::simple(tooltip.clone(), cx))
                    .on_click(cx.listener(move |sidebar, _, window, cx| {
                        sidebar.activate_worktree(&workspace, window, cx);
                    })),
            )
            .into_any_element()
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

    /// The affordance for opening and closing a project's worktrees.
    ///
    /// Its own click target rather than the whole row: clicking the row already
    /// means "switch to this project", and one gesture cannot mean two things.
    fn render_expansion_chevron(
        &self,
        ix: usize,
        entry: &ListEntry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = entry.key.clone();
        IconButton::new(
            ("project-expansion", ix),
            if entry.expanded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            },
        )
        .icon_size(IconSize::XSmall)
        .icon_color(Color::Muted)
        .on_click(cx.listener(move |sidebar, _, _, cx| {
            sidebar.toggle_project_expansion(&key, cx);
        }))
        .into_any_element()
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
                    .child(self.render_expansion_chevron(ix, entry, cx))
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
                    .gap_1()
                    .child(gradient_overlay())
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .when(entry.worktree_count > 1, |this| {
                        // Only worth saying when there is more than the main
                        // checkout: "1" under every project is noise, and the
                        // number is here to answer "how much is hidden".
                        this.child(
                            Label::new(entry.worktree_count.to_string())
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
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
