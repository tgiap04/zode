use super::*;

impl GitPanel {
    /// A thin line under the commit box, not a full-panel splash: the commit box is always
    /// present above it now, so this no longer owns the panel's empty space.
    pub(super) fn render_empty_state(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_no_repo = self.active_repository.is_none();
        let worktree_count = self.project.read(cx).visible_worktrees(cx).count();

        let label = if has_no_repo {
            "No Git repositories"
        } else {
            "No changes to commit"
        };

        v_flex()
            .w_full()
            .flex_none()
            .gap_1p5()
            .px_2()
            .py_1p5()
            .items_center()
            .child(Label::new(label).size(LabelSize::Small).color(Color::Muted))
            // TODO(phase-03): moves into the Repositories section.
            .when(has_no_repo && worktree_count > 0, |this| {
                this.child(
                    panel_filled_button("Initialize Repository")
                        .tooltip(Tooltip::for_action_title_in(
                            "git init",
                            &git::Init,
                            &self.focus_handle,
                        ))
                        .on_click(move |_, _, cx| {
                            cx.defer(move |cx| {
                                cx.dispatch_action(&git::Init);
                            })
                        }),
                )
            })
    }

    pub(super) fn render_entries(
        &self,
        has_write_access: bool,
        repo: Entity<Repository>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (is_tree_view, entry_count) = match &self.view_mode {
            GitPanelViewMode::Tree(state) => (true, state.logical_indices.len()),
            GitPanelViewMode::Flat => (false, self.entries.len()),
        };
        let repo = repo.downgrade();

        v_flex()
            .flex_1()
            .size_full()
            .overflow_hidden()
            .relative()
            .child(
                h_flex()
                    .flex_1()
                    .size_full()
                    .relative()
                    .overflow_hidden()
                    .child(
                        uniform_list(
                            "entries",
                            entry_count,
                            cx.processor(move |this, range: Range<usize>, window, cx| {
                                let Some(repo) = repo.upgrade() else {
                                    return Vec::new();
                                };
                                let repo = repo.read(cx);

                                let mut items = Vec::with_capacity(range.end - range.start);

                                for ix in range.into_iter().map(|ix| match &this.view_mode {
                                    GitPanelViewMode::Tree(state) => state.logical_indices[ix],
                                    GitPanelViewMode::Flat => ix,
                                }) {
                                    match &this.entries.get(ix) {
                                        Some(GitListEntry::Status(entry)) => {
                                            items.push(this.render_status_entry(
                                                ix,
                                                entry,
                                                0,
                                                has_write_access,
                                                repo,
                                                window,
                                                cx,
                                            ));
                                        }
                                        Some(GitListEntry::TreeStatus(entry)) => {
                                            items.push(this.render_status_entry(
                                                ix,
                                                &entry.entry,
                                                entry.depth,
                                                has_write_access,
                                                repo,
                                                window,
                                                cx,
                                            ));
                                        }
                                        Some(GitListEntry::Directory(entry)) => {
                                            items.push(this.render_directory_entry(
                                                ix,
                                                entry,
                                                has_write_access,
                                                window,
                                                cx,
                                            ));
                                        }
                                        Some(GitListEntry::Header(header)) => {
                                            items.push(this.render_list_header(
                                                ix,
                                                header,
                                                has_write_access,
                                                window,
                                                cx,
                                            ));
                                        }
                                        None => {}
                                    }
                                }

                                items
                            }),
                        )
                        .when(is_tree_view, |list| {
                            let indent_size = px(TREE_INDENT);
                            list.with_decoration(
                                ui::indent_guides(indent_size, IndentGuideColors::panel(cx))
                                    .with_compute_indents_fn(
                                        cx.entity(),
                                        |this, range, _window, _cx| {
                                            this.compute_visible_depths(range)
                                        },
                                    )
                                    .with_render_fn(cx.entity(), |_, params, _, _| {
                                        // Magic number to align the tree item is 3 here
                                        // because we're using 12px as the left-side padding
                                        // and 3 makes the alignment work with the bounding box of the icon
                                        let left_offset = px(TREE_INDENT + 3_f32);
                                        let indent_size = params.indent_size;
                                        let item_height = params.item_height;

                                        params
                                            .indent_guides
                                            .into_iter()
                                            .map(|layout| {
                                                let bounds = Bounds::new(
                                                    point(
                                                        layout.offset.x * indent_size + left_offset,
                                                        layout.offset.y * item_height,
                                                    ),
                                                    size(px(1.), layout.length * item_height),
                                                );
                                                RenderedIndentGuide {
                                                    bounds,
                                                    layout,
                                                    is_active: false,
                                                    hitbox: None,
                                                }
                                            })
                                            .collect()
                                    }),
                            )
                        })
                        .group("entries")
                        .size_full()
                        .flex_grow()
                        .with_width_from_item(self.max_width_item_index)
                        .track_scroll(&self.scroll_handle),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.deploy_panel_context_menu(event.position, window, cx)
                        }),
                    )
                    .custom_scrollbars(
                        Scrollbars::for_settings::<GitPanelScrollbarAccessor>()
                            .tracked_scroll_handle(&self.scroll_handle)
                            .with_track_along(
                                ScrollAxes::Horizontal,
                                cx.theme().colors().panel_background,
                            ),
                        window,
                        cx,
                    ),
            )
    }

    fn entry_label(&self, label: impl Into<SharedString>, color: Color) -> Label {
        Label::new(label.into()).color(color)
    }

    fn list_item_height(&self) -> Rems {
        rems(1.75)
    }

    fn render_list_header(
        &self,
        ix: usize,
        header: &GitHeaderEntry,
        has_write_access: bool,
        _window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let id: ElementId = ElementId::Name(format!("header_{}", ix).into());
        let checkbox_id: ElementId = ElementId::Name(format!("header_{}_checkbox", ix).into());
        let toggle_state = self.header_state(header.header);
        let section = header.header;
        let weak = cx.weak_entity();
        let show_checkbox_persistently = !matches!(&toggle_state, ToggleState::Unselected);

        h_flex()
            .id(id)
            .h(self.list_item_height())
            .w_full()
            .items_center()
            .pl_3()
            .pr_1()
            .gap_1p5()
            .border_1()
            .border_r_2()
            .child(
                // Smaller than the enclosing `PanelSection` header so the two header tiers read
                // as a hierarchy rather than as rivals (R5).
                h_flex().flex_1().child(
                    Label::new(header.title())
                        .color(Color::Muted)
                        .size(LabelSize::XSmall)
                        .line_height_style(LineHeightStyle::UiLabel)
                        .single_line(),
                ),
            )
            .child(
                div()
                    .flex_none()
                    .cursor_pointer()
                    .child(
                        Checkbox::new(checkbox_id, toggle_state)
                            .disabled(!has_write_access)
                            .fill()
                            .elevation(ElevationIndex::Surface)
                            .on_click_ext(move |_, _, window, cx| {
                                if !has_write_access {
                                    return;
                                }

                                weak.update(cx, |this, cx| {
                                    this.toggle_staged_for_entry(
                                        &GitListEntry::Header(GitHeaderEntry { header: section }),
                                        window,
                                        cx,
                                    );
                                    cx.stop_propagation();
                                })
                                .ok();
                            }),
                    )
                    .when(!show_checkbox_persistently, |this| {
                        this.visible_on_hover("entries")
                    }),
            )
            .into_any_element()
    }

    fn render_status_entry(
        &self,
        ix: usize,
        entry: &GitStatusEntry,
        depth: usize,
        has_write_access: bool,
        repo: &Repository,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let settings = GitPanelSettings::get_global(cx);
        let tree_view = settings.tree_view;
        let path_style = self.project.read(cx).path_style(cx);
        let git_path_style = ProjectSettings::get_global(cx).git.path_style;
        let display_name = entry.display_name(path_style);

        let selected = self.selected_entry == Some(ix);
        let marked = self.marked_entries.contains(&ix);
        let status_style = settings.status_style;
        let status = entry.status;
        let file_icon = if settings.file_icons {
            FileIcons::get_icon(entry.repo_path.as_std_path(), cx)
        } else {
            None
        };

        let has_conflict = status.is_conflicted();
        let is_modified = status.is_modified();
        let is_deleted = status.is_deleted();
        let is_created = status.is_created();

        let label_color = if status_style == StatusStyle::LabelColor {
            if has_conflict {
                Color::VersionControlConflict
            } else if is_created {
                Color::VersionControlAdded
            } else if is_modified {
                Color::VersionControlModified
            } else if is_deleted {
                // We don't want a bunch of red labels in the list
                Color::Disabled
            } else {
                Color::VersionControlAdded
            }
        } else {
            Color::Default
        };

        let path_color = if status.is_deleted() {
            Color::Disabled
        } else {
            Color::Muted
        };

        let id: ElementId = ElementId::Name(format!("entry_{}_{}", display_name, ix).into());
        let checkbox_wrapper_id: ElementId =
            ElementId::Name(format!("entry_{}_{}_checkbox_wrapper", display_name, ix).into());
        let checkbox_id: ElementId =
            ElementId::Name(format!("entry_{}_{}_checkbox", display_name, ix).into());

        let stage_status = GitPanel::stage_status_for_entry(entry, &repo);
        let mut is_staged: ToggleState = match stage_status {
            StageStatus::Staged => ToggleState::Selected,
            StageStatus::Unstaged => ToggleState::Unselected,
            StageStatus::PartiallyStaged => ToggleState::Indeterminate,
        };
        if self.show_placeholders && !self.has_staged_changes() && !entry.status.is_created() {
            is_staged = ToggleState::Selected;
        }

        let handle = cx.weak_entity();

        let selected_bg_alpha = 0.08;
        let marked_bg_alpha = 0.12;
        let state_opacity_step = 0.04;

        let info_color = cx.theme().status().info;

        let base_bg = match (selected, marked) {
            (true, true) => info_color.alpha(selected_bg_alpha + marked_bg_alpha),
            (true, false) => info_color.alpha(selected_bg_alpha),
            (false, true) => info_color.alpha(marked_bg_alpha),
            _ => cx.theme().colors().ghost_element_background,
        };

        let (hover_bg, active_bg) = if selected {
            (
                info_color.alpha(selected_bg_alpha + state_opacity_step),
                info_color.alpha(selected_bg_alpha + state_opacity_step * 2.0),
            )
        } else {
            (
                cx.theme().colors().ghost_element_hover,
                cx.theme().colors().ghost_element_active,
            )
        };

        let name_row = h_flex()
            .min_w_0()
            .flex_1()
            .gap_1()
            .when(settings.file_icons, |this| {
                this.child(
                    file_icon
                        .map(|file_icon| {
                            Icon::from_path(file_icon)
                                .size(IconSize::Small)
                                .color(Color::Muted)
                        })
                        .unwrap_or_else(|| {
                            Icon::new(IconName::File)
                                .size(IconSize::Small)
                                .color(Color::Muted)
                        }),
                )
            })
            .when(status_style != StatusStyle::LabelColor, |el| {
                el.child(git_status_icon(status))
            })
            .map(|this| {
                if tree_view {
                    this.pl(px(depth as f32 * TREE_INDENT)).child(
                        self.entry_label(display_name, label_color)
                            .when(status.is_deleted(), Label::strikethrough)
                            .truncate(),
                    )
                } else {
                    this.child(self.path_formatted(
                        entry.parent_dir(path_style),
                        path_color,
                        display_name,
                        label_color,
                        path_style,
                        git_path_style,
                        status.is_deleted(),
                    ))
                }
            });

        let id_for_diff_stat = id.clone();

        h_flex()
            .id(id)
            .h(self.list_item_height())
            .w_full()
            .pl_3()
            .pr_1()
            .gap_1p5()
            .border_1()
            .border_r_2()
            .when(selected && self.focus_handle.is_focused(window), |el| {
                el.border_color(cx.theme().colors().panel_focused_border)
            })
            .bg(base_bg)
            .hover(|s| s.bg(hover_bg))
            .active(|s| s.bg(active_bg))
            .child(name_row)
            .when(GitPanelSettings::get_global(cx).diff_stats, |el| {
                el.when_some(entry.diff_stat, move |this, stat| {
                    let id = format!("diff-stat-{}", id_for_diff_stat);
                    this.child(ui::DiffStat::new(
                        id,
                        stat.added as usize,
                        stat.deleted as usize,
                    ))
                })
            })
            .child(
                div()
                    .id(checkbox_wrapper_id)
                    .flex_none()
                    .occlude()
                    .cursor_pointer()
                    .child(
                        Checkbox::new(checkbox_id, is_staged)
                            .disabled(!has_write_access)
                            .fill()
                            .elevation(ElevationIndex::Surface)
                            .on_click_ext({
                                let entry = entry.clone();
                                let this = cx.weak_entity();
                                move |_, click, window, cx| {
                                    this.update(cx, |this, cx| {
                                        if !has_write_access {
                                            return;
                                        }
                                        if click.modifiers().shift {
                                            this.stage_bulk(ix, cx);
                                        } else {
                                            let list_entry =
                                                if GitPanelSettings::get_global(cx).tree_view {
                                                    GitListEntry::TreeStatus(GitTreeStatusEntry {
                                                        entry: entry.clone(),
                                                        depth,
                                                    })
                                                } else {
                                                    GitListEntry::Status(entry.clone())
                                                };
                                            this.toggle_staged_for_entry(&list_entry, window, cx);
                                        }
                                        cx.stop_propagation();
                                    })
                                    .ok();
                                }
                            })
                            .tooltip(move |_window, cx| {
                                let action = match stage_status {
                                    StageStatus::Staged => "Unstage",
                                    StageStatus::Unstaged | StageStatus::PartiallyStaged => "Stage",
                                };
                                let tooltip_name = action.to_string();

                                Tooltip::for_action(tooltip_name, &ToggleStaged, cx)
                            }),
                    ),
            )
            .on_click({
                cx.listener(move |this, event: &ClickEvent, window, cx| {
                    this.selected_entry = Some(ix);
                    cx.notify();
                    if event.click_count() > 1 || event.modifiers().secondary() {
                        this.open_file(&Default::default(), window, cx)
                    } else {
                        this.open_diff(&Default::default(), window, cx);
                        this.focus_handle.focus(window, cx);
                    }
                })
            })
            .on_mouse_down(
                MouseButton::Right,
                move |event: &MouseDownEvent, window, cx| {
                    // why isn't this happening automatically? we are passing MouseButton::Right to `on_mouse_down`?
                    if event.button != MouseButton::Right {
                        return;
                    }

                    let Some(this) = handle.upgrade() else {
                        return;
                    };
                    this.update(cx, |this, cx| {
                        this.deploy_entry_context_menu(event.position, ix, window, cx);
                    });
                    cx.stop_propagation();
                },
            )
            .into_any_element()
    }

    fn render_directory_entry(
        &self,
        ix: usize,
        entry: &GitTreeDirEntry,
        has_write_access: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        // TODO: Have not yet plugin the self.marked_entries. Not sure when and why we need that
        let selected = self.selected_entry == Some(ix);
        let label_color = Color::Muted;

        let id: ElementId = ElementId::Name(format!("dir_{}_{}", entry.name, ix).into());
        let checkbox_id: ElementId =
            ElementId::Name(format!("dir_checkbox_{}_{}", entry.name, ix).into());
        let checkbox_wrapper_id: ElementId =
            ElementId::Name(format!("dir_checkbox_wrapper_{}_{}", entry.name, ix).into());

        let selected_bg_alpha = 0.08;
        let state_opacity_step = 0.04;

        let info_color = cx.theme().status().info;
        let colors = cx.theme().colors();

        let (base_bg, hover_bg, active_bg) = if selected {
            (
                info_color.alpha(selected_bg_alpha),
                info_color.alpha(selected_bg_alpha + state_opacity_step),
                info_color.alpha(selected_bg_alpha + state_opacity_step * 2.0),
            )
        } else {
            (
                colors.ghost_element_background,
                colors.ghost_element_hover,
                colors.ghost_element_active,
            )
        };

        let settings = GitPanelSettings::get_global(cx);
        let folder_icon = if settings.folder_icons {
            FileIcons::get_folder_icon(entry.expanded, entry.key.path.as_std_path(), cx)
        } else {
            FileIcons::get_chevron_icon(entry.expanded, cx)
        };
        let fallback_folder_icon = if settings.folder_icons {
            if entry.expanded {
                IconName::FolderOpen
            } else {
                IconName::Folder
            }
        } else {
            if entry.expanded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            }
        };

        let stage_status = if let Some(repo) = &self.active_repository {
            self.stage_status_for_directory(entry, repo.read(cx))
        } else {
            util::debug_panic!(
                "Won't have entries to render without an active repository in Git Panel"
            );
            StageStatus::PartiallyStaged
        };

        let toggle_state: ToggleState = match stage_status {
            StageStatus::Staged => ToggleState::Selected,
            StageStatus::Unstaged => ToggleState::Unselected,
            StageStatus::PartiallyStaged => ToggleState::Indeterminate,
        };

        let name_row = h_flex()
            .min_w_0()
            .gap_1()
            .pl(px(entry.depth as f32 * TREE_INDENT))
            .child(
                folder_icon
                    .map(|folder_icon| {
                        Icon::from_path(folder_icon)
                            .size(IconSize::Small)
                            .color(Color::Muted)
                    })
                    .unwrap_or_else(|| {
                        Icon::new(fallback_folder_icon)
                            .size(IconSize::Small)
                            .color(Color::Muted)
                    }),
            )
            .child(self.entry_label(entry.name.clone(), label_color).truncate());

        h_flex()
            .id(id)
            .h(self.list_item_height())
            .min_w_0()
            .w_full()
            .pl_3()
            .pr_1()
            .gap_1p5()
            .justify_between()
            .border_1()
            .border_r_2()
            .when(selected && self.focus_handle.is_focused(window), |el| {
                el.border_color(cx.theme().colors().panel_focused_border)
            })
            .bg(base_bg)
            .hover(|s| s.bg(hover_bg))
            .active(|s| s.bg(active_bg))
            .child(name_row)
            .child(
                div()
                    .id(checkbox_wrapper_id)
                    .flex_none()
                    .occlude()
                    .cursor_pointer()
                    .child(
                        Checkbox::new(checkbox_id, toggle_state)
                            .disabled(!has_write_access)
                            .fill()
                            .elevation(ElevationIndex::Surface)
                            .on_click({
                                let entry = entry.clone();
                                let this = cx.weak_entity();
                                move |_, window, cx| {
                                    this.update(cx, |this, cx| {
                                        if !has_write_access {
                                            return;
                                        }
                                        this.toggle_staged_for_entry(
                                            &GitListEntry::Directory(entry.clone()),
                                            window,
                                            cx,
                                        );
                                        cx.stop_propagation();
                                    })
                                    .ok();
                                }
                            })
                            .tooltip(move |_window, cx| {
                                let action = match stage_status {
                                    StageStatus::Staged => "Unstage",
                                    StageStatus::Unstaged | StageStatus::PartiallyStaged => "Stage",
                                };
                                Tooltip::simple(format!("{action} folder"), cx)
                            }),
                    ),
            )
            .on_click({
                let key = entry.key.clone();
                cx.listener(move |this, _event: &ClickEvent, window, cx| {
                    this.selected_entry = Some(ix);
                    this.toggle_directory(&key, window, cx);
                })
            })
            .into_any_element()
    }

    fn path_formatted(
        &self,
        directory: Option<String>,
        path_color: Color,
        file_name: String,
        label_color: Color,
        path_style: PathStyle,
        git_path_style: GitPathStyle,
        strikethrough: bool,
    ) -> Div {
        let file_name_first = git_path_style == GitPathStyle::FileNameFirst;
        let file_path_first = git_path_style == GitPathStyle::FilePathFirst;

        let file_name = format!("{} ", file_name);

        h_flex()
            .min_w_0()
            .overflow_hidden()
            .when(file_path_first, |this| this.flex_row_reverse())
            .child(
                div().flex_none().child(
                    self.entry_label(file_name, label_color)
                        .when(strikethrough, Label::strikethrough),
                ),
            )
            .when_some(directory, |this, dir| {
                let path_name = if file_name_first {
                    dir
                } else {
                    format!("{dir}{}", path_style.primary_separator())
                };

                this.child(
                    self.entry_label(path_name, path_color)
                        .truncate_start()
                        .when(strikethrough, Label::strikethrough),
                )
            })
    }
}
