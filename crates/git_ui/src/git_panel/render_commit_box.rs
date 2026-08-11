use super::*;

impl GitPanel {
    pub(crate) fn render_remote_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let branch = self.active_repository.as_ref()?.read(cx).branch.clone();
        if !self.can_push_and_pull(cx) {
            return None;
        }
        Some(
            h_flex()
                .gap_1()
                .flex_shrink_0()
                .when_some(branch, |this, branch| {
                    let focus_handle = Some(self.focus_handle(cx));

                    this.children(render_remote_button(
                        "remote-button",
                        &branch,
                        focus_handle,
                        true,
                    ))
                })
                .into_any_element(),
        )
    }

    pub fn render_footer(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let active_repository = self.active_repository.clone()?;
        let panel_editor_style = panel_editor_style(true, window, cx);

        let editor_focus_handle = self.commit_editor.focus_handle(cx);
        let expand_tooltip_focus_handle = editor_focus_handle;

        let branch = active_repository.read(cx).branch.clone();
        let head_commit = active_repository.read(cx).head_commit.clone();

        let footer_size = px(32.);
        let gap = px(9.0);
        let max_height = panel_editor_style
            .text
            .line_height_in_pixels(window.rem_size())
            * MAX_PANEL_EDITOR_LINES
            + gap;

        let git_panel = cx.entity();
        let display_name = SharedString::from(Arc::from(
            active_repository
                .read(cx)
                .display_name()
                .trim_end_matches("/"),
        ));
        let editor_is_long = self.commit_editor.update(cx, |editor, cx| {
            editor.max_point(cx).row().0 >= MAX_PANEL_EDITOR_LINES as u32
        });

        let footer = v_flex()
            .child(PanelRepoFooter::new(
                display_name,
                branch,
                head_commit,
                Some(git_panel),
            ))
            .child(
                panel_editor_container(window, cx)
                    .id("commit-editor-container")
                    .relative()
                    .w_full()
                    .h(max_height + footer_size)
                    .border_t_1()
                    .border_color(cx.theme().colors().border)
                    .cursor_text()
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        window.focus(&this.commit_editor.focus_handle(cx), cx);
                    }))
                    .child(
                        h_flex()
                            .id("commit-footer")
                            .border_t_1()
                            .when(editor_is_long, |el| {
                                el.border_color(cx.theme().colors().border_variant)
                            })
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .w_full()
                            .px_2()
                            .h(footer_size)
                            .flex_none()
                            .justify_between()
                            .child(h_flex().gap_0p5().child(self.render_commit_button(cx))),
                    )
                    .child(
                        div()
                            .pr_2p5()
                            .on_action(|&zed_actions::editor::MoveUp, _, cx| {
                                cx.stop_propagation();
                            })
                            .on_action(|&zed_actions::editor::MoveDown, _, cx| {
                                cx.stop_propagation();
                            })
                            .child(EditorElement::new(&self.commit_editor, panel_editor_style)),
                    )
                    .child(
                        h_flex()
                            .absolute()
                            .top_2()
                            .right_2()
                            .opacity(0.5)
                            .hover(|this| this.opacity(1.0))
                            .child(
                                panel_icon_button("expand-commit-editor", IconName::Maximize)
                                    .icon_size(IconSize::Small)
                                    .size(ui::ButtonSize::Default)
                                    .tooltip(move |_window, cx| {
                                        Tooltip::for_action_in(
                                            "Open Commit Modal",
                                            &git::ExpandCommitEditor,
                                            &expand_tooltip_focus_handle,
                                            cx,
                                        )
                                    })
                                    .on_click(cx.listener({
                                        move |_, _, window, cx| {
                                            window.dispatch_action(
                                                git::ExpandCommitEditor.boxed_clone(),
                                                cx,
                                            )
                                        }
                                    })),
                            ),
                    ),
            );

        Some(footer)
    }

    fn render_commit_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (can_commit, tooltip) = self.configure_commit_button(cx);
        let title = self.commit_button_title();
        let commit_tooltip_focus_handle = self.commit_editor.focus_handle(cx);
        let amend = self.amend_pending();
        let signoff = self.signoff_enabled;

        let label_color = if self.pending_commit.is_some() {
            Color::Disabled
        } else {
            Color::Default
        };

        div()
            .id("commit-wrapper")
            .on_hover(cx.listener(move |this, hovered, _, cx| {
                this.show_placeholders =
                    *hovered && !this.has_staged_changes() && !this.has_unstaged_conflicts();
                cx.notify()
            }))
            .child(SplitButton::new(
                ButtonLike::new_rounded_left(ElementId::Name(
                    format!("split-button-left-{}", title).into(),
                ))
                .layer(ElevationIndex::ModalSurface)
                .size(ButtonSize::Compact)
                .child(
                    Label::new(title)
                        .size(LabelSize::Small)
                        .color(label_color)
                        .mr_0p5(),
                )
                .on_click({
                    let git_panel = cx.weak_entity();
                    move |_, window, cx| {
                        telemetry::event!("Git Committed", source = "Git Panel");
                        git_panel
                            .update(cx, |git_panel, cx| {
                                git_panel.commit_changes(
                                    CommitOptions {
                                        amend,
                                        signoff,
                                        allow_empty: false,
                                    },
                                    window,
                                    cx,
                                );
                            })
                            .ok();
                    }
                })
                .disabled(!can_commit || self.modal_open)
                .tooltip({
                    let handle = commit_tooltip_focus_handle.clone();
                    move |_window, cx| {
                        if can_commit {
                            Tooltip::with_meta_in(
                                tooltip,
                                Some(if amend { &git::Amend } else { &git::Commit }),
                                format!(
                                    "git commit{}{}",
                                    if amend { " --amend" } else { "" },
                                    if signoff { " --signoff" } else { "" }
                                ),
                                &handle.clone(),
                                cx,
                            )
                        } else {
                            Tooltip::simple(tooltip, cx)
                        }
                    }
                }),
                self.render_git_commit_menu(
                    ElementId::Name(format!("split-button-right-{}", title).into()),
                    Some(commit_tooltip_focus_handle),
                    cx,
                )
                .into_any_element(),
            ))
    }

    pub(super) fn render_pending_amend(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .py_1p5()
            .px_2()
            .gap_1p5()
            .justify_between()
            .border_t_1()
            .border_color(cx.theme().colors().border.opacity(0.8))
            .child(
                div()
                    .flex_grow()
                    .overflow_hidden()
                    .max_w(relative(0.85))
                    .child(
                        Label::new("This will update your most recent commit.")
                            .size(LabelSize::Small)
                            .truncate(),
                    ),
            )
            .child(
                panel_button("Cancel")
                    .size(ButtonSize::Default)
                    .on_click(cx.listener(|this, _, _, cx| this.set_amend_pending(false, cx))),
            )
    }

    pub(super) fn render_previous_commit(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let active_repository = self.active_repository.as_ref()?;
        let branch = active_repository.read(cx).branch.as_ref()?;
        let commit = branch.most_recent_commit.as_ref()?.clone();
        let workspace = self.workspace.clone();
        let this = cx.entity();

        Some(
            h_flex()
                .p_1p5()
                .gap_1p5()
                .justify_between()
                .border_t_1()
                .border_color(cx.theme().colors().border.opacity(0.8))
                .child(
                    div()
                        .id("commit-msg-hover")
                        .cursor_pointer()
                        .px_1()
                        .rounded_sm()
                        .line_clamp(1)
                        .hover(|s| s.bg(cx.theme().colors().element_hover))
                        .child(
                            Label::new(commit.subject.clone())
                                .size(LabelSize::Small)
                                .truncate(),
                        )
                        .on_click({
                            let commit = commit.clone();
                            let repo = active_repository.downgrade();
                            move |_, window, cx| {
                                CommitView::open(
                                    commit.sha.to_string(),
                                    repo.clone(),
                                    workspace.clone(),
                                    None,
                                    None,
                                    window,
                                    cx,
                                );
                            }
                        })
                        .hoverable_tooltip({
                            let repo = active_repository.clone();
                            move |window, cx| {
                                GitPanelMessageTooltip::new(
                                    this.clone(),
                                    commit.sha.clone(),
                                    repo.clone(),
                                    window,
                                    cx,
                                )
                                .into()
                            }
                        }),
                )
                .child(
                    h_flex()
                        .gap_0p5()
                        .when(commit.has_parent, |this| {
                            let has_unstaged = self.has_unstaged_changes();
                            this.child(
                                panel_icon_button("undo", IconName::Undo)
                                    .icon_size(IconSize::Small)
                                    .tooltip(move |_window, cx| {
                                        Tooltip::with_meta(
                                            "Uncommit",
                                            Some(&git::Uncommit),
                                            if has_unstaged {
                                                "git reset HEAD^ --soft"
                                            } else {
                                                "git reset HEAD^"
                                            },
                                            cx,
                                        )
                                    })
                                    .on_click(
                                        cx.listener(|this, _, window, cx| {
                                            this.uncommit(window, cx)
                                        }),
                                    ),
                            )
                        })
                        .child(
                            panel_icon_button("git-graph-button", IconName::GitGraph)
                                .icon_size(IconSize::Small)
                                .tooltip(|_window, cx| {
                                    Tooltip::for_action("Open Git Graph", &Open, cx)
                                })
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Open.boxed_clone(), cx)
                                }),
                        ),
                ),
        )
    }
}
