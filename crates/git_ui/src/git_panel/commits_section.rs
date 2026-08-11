use super::*;
use crate::commit_tooltip::CommitAvatar;
use gpui::DragMoveEvent;
use project::git_store::CommitDataState;

/// What the branch's position relative to its upstream reads as. A free function taking only the
/// upstream so all four cases can be asserted without fabricating a repository.
pub(super) fn tracking_status_label(upstream: Option<&Upstream>) -> SharedString {
    let Some(upstream) = upstream else {
        return "No upstream".into();
    };
    let remote = upstream.remote_name().unwrap_or("origin");

    match &upstream.tracking {
        UpstreamTracking::Gone => "Upstream gone".into(),
        UpstreamTracking::Tracked(status) => match (status.ahead, status.behind) {
            (0, 0) => format!("Up to date with {remote}").into(),
            (ahead, 0) => format!("↑{ahead} ahead of {remote}").into(),
            (0, behind) => format!("↓{behind} behind {remote}").into(),
            (ahead, behind) => format!("↑{ahead} ↓{behind} — {remote}").into(),
        },
    }
}

impl GitPanel {
    pub(super) fn commits_tracking_status_label(&self, cx: &App) -> SharedString {
        let branch = self
            .active_repository
            .as_ref()
            .and_then(|repo| repo.read(cx).branch.clone());
        match &branch {
            Some(branch) => tracking_status_label(branch.upstream.as_ref()),
            None => "No branch".into(),
        }
    }

    /// How many commits of the current branch's log have arrived so far.
    ///
    /// Reads the repository's cache without touching `graph_data`, which would start a fetch —
    /// keeping `render` free of that side effect.
    fn loaded_commit_count(&self, cx: &App) -> usize {
        let Some(repository) = self.active_repository.as_ref() else {
            return 0;
        };
        let Some(branch) = self.current_branch_ref(cx) else {
            return 0;
        };
        repository
            .read(cx)
            .get_graph_data(LogSource::Branch(branch), LogOrder::default())
            .map_or(0, |data| data.commit_data.len())
    }

    fn branch_log_error(&self, cx: &App) -> Option<SharedString> {
        let branch = self.current_branch_ref(cx)?;
        self.active_repository
            .as_ref()?
            .read(cx)
            .get_graph_data(LogSource::Branch(branch), LogOrder::default())?
            .error
            .clone()
    }

    pub(super) fn render_commits_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let expanded = self.section_expanded(PanelSectionKind::Commits);
        let branch_name = self
            .active_repository
            .as_ref()
            .and_then(|repo| repo.read(cx).branch.as_ref().map(|b| b.name().to_owned()));
        let label = match &branch_name {
            Some(branch) => format!("Commits ({branch})"),
            None => "Commits".to_string(),
        };

        PanelSection::new("git-panel-commits-section", label, expanded)
            .on_toggle(cx.listener(|this, _, _, cx| {
                this.toggle_section(PanelSectionKind::Commits, cx);
            }))
            .fixed_height(
                self.commits_section_height,
                self.render_commits_resize_handle(cx),
            )
            .when(expanded, |section| {
                section
                    .child(
                        h_flex()
                            .w_full()
                            .flex_none()
                            .px_2()
                            .py_1()
                            .gap_1()
                            .child(
                                Icon::new(IconName::CloudDownload)
                                    .size(IconSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(
                                Label::new(self.commits_tracking_status_label(cx))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted)
                                    .single_line(),
                            )
                            .into_any_element(),
                    )
                    .child(self.render_commit_rows(cx))
            })
            .into_any_element()
    }

    fn render_commit_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let commit_count = self.loaded_commit_count(cx);
        let Some(state) = self.commits_section.as_ref() else {
            return div().into_any_element();
        };

        // A failed `git log` must say so rather than sit on "Loading…" forever.
        if let Some(error) = self.branch_log_error(cx) {
            return h_flex()
                .w_full()
                .px_2()
                .py_1()
                .child(
                    Label::new(format!("Could not load commits: {error}"))
                        .size(LabelSize::Small)
                        .color(Color::Error),
                )
                .into_any_element();
        }

        if commit_count == 0 {
            return h_flex()
                .w_full()
                .px_2()
                .py_1()
                .child(
                    Label::new("Loading commits…")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .into_any_element();
        }

        uniform_list(
            "git-panel-commits",
            commit_count,
            cx.processor(move |this, range: Range<usize>, window, cx| {
                let Some(repository) = this.active_repository.clone() else {
                    return Vec::new();
                };
                let Some(branch) = this.current_branch_ref(cx) else {
                    return Vec::new();
                };

                // Topology first — sha and ref names are all the graph cache holds.
                let commits = repository
                    .read(cx)
                    .get_graph_data(LogSource::Branch(branch), LogOrder::default())
                    .map(|data| {
                        data.commit_data
                            .get(range.clone())
                            .unwrap_or_default()
                            .to_vec()
                    })
                    .unwrap_or_default();

                commits
                    .into_iter()
                    .enumerate()
                    .map(|(offset, commit)| {
                        this.render_commit_row(
                            range.start + offset,
                            commit,
                            &repository,
                            window,
                            cx,
                        )
                    })
                    .collect()
            }),
        )
        // `flex_1`, not `h_full`: `h_full` would claim the whole fixed height for the list alone,
        // pushing the status row above it past the box and forcing the wrapper to scroll on top of
        // the list's own scrolling. Same shape as `file_history_view`.
        .flex_1()
        .track_scroll(&state.scroll_handle)
        .into_any_element()
    }

    fn render_commit_row(
        &self,
        ix: usize,
        commit: Arc<git::repository::InitialGraphCommitData>,
        repository: &Entity<Repository>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Subject, author and timestamp live in a second per-commit cache, requested on demand.
        // Only visible rows ask, so scrolling pays for what it shows and nothing more.
        let data = repository.update(cx, |repository, cx| {
            repository.fetch_commit_data(commit.sha, cx).clone()
        });
        let sha = SharedString::from(commit.sha.to_string());

        let (subject, author_email, timestamp) = match &data {
            CommitDataState::Loaded(data) => (
                data.subject.clone(),
                Some(data.author_email.clone()),
                Some(data.commit_timestamp),
            ),
            CommitDataState::Loading => (SharedString::from("Loading…"), None, None),
        };

        let relative_time = timestamp.map(|timestamp| {
            let commit_time = OffsetDateTime::from_unix_timestamp(timestamp)
                .unwrap_or(OffsetDateTime::UNIX_EPOCH);
            time_format::format_localized_timestamp(
                commit_time,
                OffsetDateTime::now_utc(),
                time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC),
                time_format::TimestampFormat::Relative,
            )
        });

        let workspace = self.workspace.clone();
        let repo = repository.downgrade();

        h_flex()
            .id(("commit-row", ix))
            .h(COMMITS_ROW_HEIGHT)
            .w_full()
            .items_center()
            .px_2()
            .gap_1p5()
            .cursor_pointer()
            .hover(|this| this.bg(cx.theme().colors().element_hover))
            .on_click({
                let sha = sha.clone();
                move |_, window, cx| {
                    CommitView::open(
                        sha.to_string(),
                        repo.clone(),
                        workspace.clone(),
                        None,
                        None,
                        window,
                        cx,
                    );
                }
            })
            .child(
                CommitAvatar::new(&sha, author_email, None)
                    .size(px(14.))
                    .render(window, cx),
            )
            .children(commit.ref_names.iter().take(2).map(|ref_name| {
                h_flex()
                    .flex_none()
                    .px_1()
                    .rounded_sm()
                    .bg(cx.theme().colors().element_background)
                    .child(
                        Label::new(ref_name.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Muted)
                            .single_line(),
                    )
            }))
            .child(
                div().flex_1().overflow_hidden().child(
                    Label::new(subject)
                        .size(LabelSize::Small)
                        .single_line()
                        .truncate(),
                ),
            )
            .children(relative_time.map(|relative_time| {
                Label::new(relative_time)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted)
                    .single_line()
            }))
            .into_any_element()
    }

    fn render_commits_resize_handle(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("commits-section-resize-container")
            .relative()
            .w_full()
            .flex_none()
            .h(px(1.))
            .bg(cx.theme().colors().border_variant)
            .child(
                div()
                    .id("commits-section-resize-handle")
                    .absolute()
                    .top(px(-3.))
                    .w_full()
                    .h(px(7.))
                    .cursor_row_resize()
                    .block_mouse_except_scroll()
                    .on_drag(DraggedCommitsSectionHandle, |_, _, _, cx| {
                        cx.new(|_| gpui::Empty)
                    }),
            )
            .into_any_element()
    }

    /// Resizes by pointer *delta*, not by distance from `event.bounds`.
    ///
    /// The listener is registered on the panel root, so `event.bounds` is the panel's bounds, not
    /// the section's — measuring from its top would be off by the height of the title row and every
    /// section above, which with the default layout pins the height at its maximum immediately.
    /// A delta is independent of whose bounds GPUI reports.
    pub(super) fn resize_commits_section(
        &mut self,
        event: &DragMoveEvent<DraggedCommitsSectionHandle>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let position = event.event.position.y;
        if let Some(previous) = self.commits_resize_drag_start {
            let height = self.commits_section_height + (position - previous);
            self.commits_section_height =
                height.clamp(COMMITS_SECTION_MIN_HEIGHT, COMMITS_SECTION_MAX_HEIGHT);
        }
        self.commits_resize_drag_start = Some(position);
        cx.notify();
    }

    pub(super) fn end_commits_section_resize(&mut self, cx: &mut Context<Self>) {
        self.commits_resize_drag_start = None;
        self.serialize(cx);
    }
}
