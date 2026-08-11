use super::*;

impl GitPanel {
    fn render_overflow_menu(&self, id: impl Into<ElementId>) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();
        let has_tracked_changes = self.has_tracked_changes();
        let has_staged_changes = self.has_staged_changes();
        let has_unstaged_changes = self.has_unstaged_changes();
        let has_new_changes = self.new_count > 0;
        let has_stash_items = self.stash_entries.entries.len() > 0;

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
                        sort_by_path: GitPanelSettings::get_global(cx).sort_by_path,
                        has_stash_items,
                        tree_view: GitPanelSettings::get_global(cx).tree_view,
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
                    .child(self.render_overflow_menu("git-panel-title-overflow-menu"))
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
            self.render_overflow_menu("git-panel-changes-overflow-menu")
                .into_any_element(),
        ]
    }

    pub(super) fn render_panel_header(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        self.active_repository.as_ref()?;

        let (text, action, stage, tooltip) =
            if self.total_staged_count() == self.entry_count && self.entry_count > 0 {
                ("Unstage All", UnstageAll.boxed_clone(), false, "git reset")
            } else {
                ("Stage All", StageAll.boxed_clone(), true, "git add --all")
            };

        let change_string = match self.changes_count {
            0 => "No Changes".to_string(),
            1 => "1 Change".to_string(),
            count => format!("{} Changes", count),
        };

        Some(
            self.panel_header_container(window, cx)
                .px_2()
                .justify_between()
                .child(
                    panel_button(change_string)
                        .color(Color::Muted)
                        .tooltip(Tooltip::for_action_title_in(
                            "Open Diff",
                            &Diff,
                            &self.focus_handle,
                        ))
                        .on_click(|_, _, cx| {
                            cx.defer(|cx| {
                                cx.dispatch_action(&Diff);
                            })
                        }),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(self.render_overflow_menu("overflow_menu"))
                        .child(
                            panel_filled_button(text)
                                .tooltip(Tooltip::for_action_title_in(
                                    tooltip,
                                    action.as_ref(),
                                    &self.focus_handle,
                                ))
                                .disabled(self.entry_count == 0)
                                .on_click({
                                    let git_panel = cx.weak_entity();
                                    move |_, _, cx| {
                                        git_panel
                                            .update(cx, |git_panel, cx| {
                                                git_panel.change_all_files_stage(stage, cx);
                                            })
                                            .ok();
                                    }
                                }),
                        ),
                ),
        )
    }
}
