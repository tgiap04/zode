use super::*;

impl GitPanel {
    fn render_overflow_menu(
        &self,
        id: impl Into<ElementId>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();
        let has_tracked_changes = self.has_tracked_changes();
        let has_staged_changes = self.has_staged_changes();
        let has_unstaged_changes = self.has_unstaged_changes();
        let has_new_changes = self.new_count > 0;
        let has_discardable_changes = self.has_discardable_changes();
        let has_stash_items = self.stash_entries.entries.len() > 0;
        let can_uncommit = self.can_uncommit(cx);
        let show_branch_diff = self.show_branch_diff(cx);

        PopoverMenu::new(id.into())
            .trigger(
                IconButton::new("overflow-menu-trigger", IconName::Ellipsis)
                    .icon_size(IconSize::Small)
                    .icon_color(Color::Muted),
            )
            .menu(move |window, cx| {
                Some(git_panel_context_menu(
                    focus_handle.clone(),
                    GitMenuState {
                        has_tracked_changes,
                        has_staged_changes,
                        has_unstaged_changes,
                        has_new_changes,
                        has_discardable_changes,
                        sort_by_path: GitPanelSettings::get_global(cx).sort_by_path,
                        has_stash_items,
                        tree_view: GitPanelSettings::get_global(cx).tree_view,
                        can_uncommit,
                        show_branch_diff,
                    },
                    window,
                    cx,
                ))
            })
            .anchor(Anchor::TopRight)
    }

    pub(super) fn render_git_commit_menu(
        &self,
        id: impl Into<ElementId>,
        keybinding_target: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        PopoverMenu::new(id.into())
            .trigger(
                ui::ButtonLike::new_rounded_right("commit-split-button-right")
                    .layer(ui::ElevationIndex::ModalSurface)
                    .size(ButtonSize::None)
                    .child(
                        h_flex()
                            .px_1()
                            .h_full()
                            .justify_center()
                            .border_l_1()
                            .border_color(cx.theme().colors().border)
                            .child(Icon::new(IconName::ChevronDown).size(IconSize::XSmall)),
                    ),
            )
            .menu({
                let git_panel = cx.entity();
                let has_previous_commit = self.head_commit(cx).is_some();
                let amend = self.amend_pending();
                let signoff = self.signoff_enabled;

                move |window, cx| {
                    Some(ContextMenu::build(window, cx, |context_menu, _, _| {
                        context_menu
                            .when_some(keybinding_target.clone(), |el, keybinding_target| {
                                el.context(keybinding_target)
                            })
                            .when(has_previous_commit, |this| {
                                this.toggleable_entry(
                                    "Amend",
                                    amend,
                                    IconPosition::Start,
                                    Some(Box::new(Amend)),
                                    {
                                        let git_panel = git_panel.downgrade();
                                        move |_, cx| {
                                            git_panel
                                                .update(cx, |git_panel, cx| {
                                                    git_panel.toggle_amend_pending(cx);
                                                })
                                                .ok();
                                        }
                                    },
                                )
                            })
                            .toggleable_entry(
                                "Signoff",
                                signoff,
                                IconPosition::Start,
                                Some(Box::new(Signoff)),
                                move |window, cx| window.dispatch_action(Box::new(Signoff), cx),
                            )
                    }))
                }
            })
            .anchor(Anchor::TopRight)
    }

    pub fn configure_commit_button(&self, cx: &mut Context<Self>) -> (bool, &'static str) {
        if self.has_unstaged_conflicts() {
            (false, "You must resolve conflicts before committing")
        } else if !self.has_staged_changes() && !self.has_tracked_changes() && !self.amend_pending {
            (false, "No changes to commit")
        } else if self.pending_commit.is_some() {
            (false, "Commit in progress")
        } else if !self.has_commit_message(cx) {
            (false, "No commit message")
        } else if !self.has_write_access(cx) {
            (false, "You do not have write access to this project")
        } else {
            (true, self.commit_button_title())
        }
    }

    pub fn commit_button_title(&self) -> &'static str {
        if self.amend_pending {
            if self.has_staged_changes() {
                "Amend"
            } else if self.has_tracked_changes() {
                "Amend Tracked"
            } else {
                "Amend"
            }
        } else if self.has_staged_changes() {
            "Commit"
        } else {
            "Commit Tracked"
        }
    }

    pub(super) fn render_title_row(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let zoomed = self.zoomed;

        self.panel_header_container(window, cx)
            .px_2()
            .justify_between()
            .child(
                Label::new("Source Control")
                    .size(LabelSize::Small)
                    .line_height_style(LineHeightStyle::UiLabel)
                    .single_line(),
            )
            .child(
                h_flex()
                    .gap_0p5()
                    .child(self.render_overflow_menu("git-panel-title-overflow-menu", cx))
                    .child(
                        IconButton::new("git-panel-toggle-zoom", IconName::Maximize)
                            .icon_size(IconSize::Small)
                            .icon_color(Color::Muted)
                            .toggle_state(zoomed)
                            .selected_icon(IconName::Minimize)
                            .tooltip(move |_, cx| {
                                Tooltip::for_action(
                                    if zoomed { "Zoom Out" } else { "Zoom In" },
                                    &ToggleZoom,
                                    cx,
                                )
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_zoom(&ToggleZoom, window, cx);
                            })),
                    )
                    .child(
                        IconButton::new("git-panel-close", IconName::Close)
                            .icon_size(IconSize::Small)
                            .icon_color(Color::Muted)
                            .tooltip(Tooltip::for_action_title_in(
                                "Close Panel",
                                &Close,
                                &self.focus_handle,
                            ))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_panel(&Close, window, cx);
                            })),
                    ),
            )
    }

    /// Rescans every visible worktree on the machine that owns it, which is the only way to
    /// pick up a repository the file watcher missed (a clone made inside a WSL or SSH folder).
    pub(super) fn reload_repositories(&mut self, cx: &mut Context<Self>) {
        if self.reloading_repositories {
            return;
        }
        let rescans = self.project.update(cx, |project, cx| {
            let worktree_ids = project
                .visible_worktrees(cx)
                .map(|worktree| worktree.read(cx).id())
                .collect::<Vec<_>>();
            worktree_ids
                .into_iter()
                .map(|worktree_id| {
                    let rescan = project.rescan_worktree(worktree_id, cx);
                    async move { (worktree_id, rescan.await) }
                })
                .collect::<Vec<_>>()
        });
        let shown_for_at_least = cx.background_executor().timer(MIN_RELOAD_SPINNER);
        let workspace = self.workspace.clone();
        self.reloading_repositories = true;
        self._reload_repositories_task = Some(cx.spawn(async move |panel, cx| {
            let (results, ()) =
                futures::join!(futures::future::join_all(rescans), shown_for_at_least);
            let failures = panel
                .read_with(cx, |panel, cx| {
                    let project = panel.project.read(cx);
                    results
                        .into_iter()
                        .filter_map(|(worktree_id, result)| {
                            let error = result.err()?;
                            // A remote worktree removed mid-reload fails its rescan with an
                            // entity-released error that says nothing the user could act on.
                            if project.worktree_for_id(worktree_id, cx).is_none() {
                                log::debug!(
                                    "reload of removed worktree {worktree_id} failed: {error:#}"
                                );
                                return None;
                            }
                            Some(error)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            // One message per distinct failure: a guest would otherwise get the same refusal
            // once per worktree.
            let mut messages: Vec<String> = Vec::new();
            for error in failures {
                let message = format!("{error:#}");
                if !messages.contains(&message) {
                    messages.push(message);
                }
            }
            if !messages.is_empty() {
                let error = anyhow::anyhow!(messages.join("\n"));
                log::error!("{error:#}");
                workspace
                    .update(cx, |workspace, cx| workspace.show_error(&error, cx))
                    .log_err();
            }
            panel
                .update(cx, |panel, cx| {
                    panel.reloading_repositories = false;
                    cx.notify();
                })
                // A failed update means the panel is gone, and so is this task.
                .ok();
        }));
        cx.notify();
    }

    /// One row per repository, in a stable order, above the `Changes` section.
    pub(super) fn render_repositories_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let reloading = self.reloading_repositories;
        let expanded = self.section_expanded(PanelSectionKind::Repositories);
        let is_guest = self.project.read(cx).is_via_collab();
        let active_repo_id = self.active_repository.as_ref().map(|repo| repo.read(cx).id);
        let git_panel = cx.entity();
        let repositories = self.project.read(cx).git_store().read(cx).repositories();

        let rows = self
            .sorted_repo_ids
            .iter()
            .filter_map(|repo_id| {
                let repo_entity = repositories.get(repo_id)?.clone();
                let repo = repo_entity.read(cx);
                Some(
                    RepositoryRow::new(
                        SharedString::from(Arc::from(repo.display_name().trim_end_matches("/"))),
                        repo.branch.clone(),
                        repo.head_commit.clone(),
                        repo_entity.clone(),
                        Some(*repo_id) == active_repo_id,
                        Some(git_panel.clone()),
                    )
                    .into_any_element(),
                )
            })
            .collect::<Vec<_>>();

        let has_rows = !rows.is_empty();
        let section = PanelSection::new("git-panel-repositories-section", "Repositories", expanded)
            .on_toggle(cx.listener(|this, _, _, cx| {
                this.toggle_section(PanelSectionKind::Repositories, cx);
            }))
            .actions([div()
                .debug_selector(|| "reload-repositories".into())
                // A disabled button takes no mouse handlers of its own, so without this its press
                // lands on the header and folds the section it was meant to refresh.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    IconButton::new("reload-repositories", IconName::RotateCw)
                        .icon_size(IconSize::Small)
                        .icon_color(Color::Muted)
                        .loading(reloading)
                        .disabled(is_guest)
                        .tooltip(Tooltip::text(if is_guest {
                            "Reload is not available to guests"
                        } else if reloading {
                            "Reloading…"
                        } else {
                            "Reload Repositories"
                        }))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.reload_repositories(cx);
                        })),
                )
                .into_any_element()])
            .map(|section| {
                if rows.is_empty() {
                    section.child(self.render_no_repositories(cx).into_any_element())
                } else {
                    // Capped and scrollable: until the user resizes it, the section takes its
                    // natural height so `Changes` keeps the rest, but a workspace with many
                    // repositories must not grow this list until `Changes` — the only shrinkable
                    // child — is squeezed to nothing.
                    section.child(
                        v_flex()
                            .id("repository-rows")
                            .debug_selector(|| "repository-rows".into())
                            .w_full()
                            .map(|this| match self.repositories_section_height {
                                Some(height) => this.h(height),
                                None => this.max_h(MAX_REPOSITORY_ROWS_HEIGHT),
                            })
                            .overflow_y_scroll()
                            .children(rows)
                            .into_any_element(),
                    )
                }
            });

        // The handle goes under the section rather than through `PanelSection::fixed_height`, which
        // hangs it on the top edge: that edge borders the title row, which has nothing to give.
        v_flex()
            .w_full()
            .flex_none()
            .child(section)
            .when(expanded && has_rows, |this| {
                this.child(self.render_repositories_resize_handle(cx))
            })
            .into_any_element()
    }

    /// The section's bottom rule, with a hit area straddling it so the boundary with `Changes` is
    /// grabbable.
    fn render_repositories_resize_handle(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("repositories-section-resize-container")
            .relative()
            .w_full()
            .flex_none()
            .h(px(1.))
            .bg(cx.theme().colors().border_variant)
            .child(
                div()
                    .id("repositories-section-resize-handle")
                    .debug_selector(|| "repositories-section-resize-handle".into())
                    .absolute()
                    .top(px(-3.))
                    .w_full()
                    .h(px(7.))
                    .cursor_row_resize()
                    .block_mouse_except_scroll()
                    .on_drag(DraggedRepositoriesSectionHandle, |_, _, _, cx| {
                        cx.new(|_| gpui::Empty)
                    }),
            )
            .into_any_element()
    }

    pub(super) fn resize_repositories_section(
        &mut self,
        event: &gpui::DragMoveEvent<DraggedRepositoriesSectionHandle>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let position = event.event.position.y;
        if let Some(previous) = self.repositories_resize_drag_start {
            // Before the first resize the rows have no stored height, only the one their count
            // gives them, so the drag starts from what is on screen rather than jumping.
            let natural_height = (REPOSITORY_ROW_HEIGHT * self.sorted_repo_ids.len() as f32)
                .min(MAX_REPOSITORY_ROWS_HEIGHT);
            let height = self.repositories_section_height.unwrap_or(natural_height);
            self.repositories_section_height =
                Some(height_dragged_from_bottom_edge(height, previous, position));
        }
        self.repositories_resize_drag_start = Some(position);
        cx.notify();
    }

    pub(super) fn end_repositories_section_resize(&mut self, cx: &mut Context<Self>) {
        self.repositories_resize_drag_start = None;
        self.serialize(cx);
    }

    /// Took over `Initialize Repository` from the changes empty state.
    fn render_no_repositories(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let worktree_count = self.project.read(cx).visible_worktrees(cx).count();

        h_flex()
            .w_full()
            .flex_none()
            .px_2()
            .py_1p5()
            .gap_1p5()
            .justify_between()
            .child(
                Label::new("No Git repositories")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .when(worktree_count > 0, |this| {
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

    /// Hover actions for the `Changes` section header.
    pub(super) fn render_changes_section_actions(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let has_entries = self.entry_count > 0;
        let all_staged = self.total_staged_count() == self.entry_count && self.entry_count > 0;
        let has_tracked_changes = self.has_tracked_changes();

        let (stage_icon, stage_tooltip, stage_action, stage) = if all_staged {
            (
                IconName::Dash,
                "Unstage All",
                UnstageAll.boxed_clone(),
                false,
            )
        } else {
            (IconName::Plus, "Stage All", StageAll.boxed_clone(), true)
        };

        vec![
            IconButton::new("changes-section-stage-all", stage_icon)
                .icon_size(IconSize::Small)
                .icon_color(Color::Muted)
                .disabled(!has_entries)
                .tooltip(Tooltip::for_action_title_in(
                    stage_tooltip,
                    stage_action.as_ref(),
                    &self.focus_handle,
                ))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change_all_files_stage(stage, cx);
                }))
                .into_any_element(),
            IconButton::new("changes-section-discard", IconName::Undo)
                .icon_size(IconSize::Small)
                .icon_color(Color::Muted)
                .disabled(!has_tracked_changes)
                .tooltip(Tooltip::for_action_title_in(
                    "Discard Tracked Changes",
                    &RestoreTrackedFiles,
                    &self.focus_handle,
                ))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.restore_tracked_files(&RestoreTrackedFiles, window, cx);
                }))
                .into_any_element(),
            self.render_overflow_menu("git-panel-changes-overflow-menu", cx)
                .into_any_element(),
        ]
    }
}
