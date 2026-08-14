use super::*;
use git_graph_core::{GraphData, LANE_WIDTH, LEFT_PADDING, LanePaint, accent_colors_count};
use gpui::Bounds;

/// Widest a subject may be pushed. The panel is 360px by default, so lanes have to yield to the
/// subject rather than the other way round — the graph tab is the place to go when a branch is too
/// tangled to read here, which is what the toolbar's expand button is for.
const MAX_LANE_COLUMN_WIDTH: Pixels = px(96.);

/// Where a row's subject starts: clear of the lanes drawn *on that row*.
///
/// Takes the row's own count, never `max_lanes`. Sized to the log's deepest merge, every subject
/// in the repository was pushed out by the single most tangled commit in it — a commit sitting
/// alone on the first lane was indented as if five branches ran beside it. Per row, a subject sits
/// against the lane it belongs to, which is also what makes the graph readable as a shape: the
/// text steps in and out with the branches instead of standing off in one column.
fn subject_indent(lanes: usize) -> Pixels {
    (LEFT_PADDING + LANE_WIDTH * lanes.max(1) as f32).min(MAX_LANE_COLUMN_WIDTH)
}

/// Present only once the Graph section has been expanded at least once. Absence is the laziness:
/// building this is what asks a repository for graph data, and asking is what starts the fetch.
pub(super) struct GraphSectionState {
    pub(super) graph_data: GraphData,
    /// `Branch(_)` is "Auto" — follow HEAD; `All` shows every ref.
    log_source: LogSource,
    loaded_for_branch: Option<SharedString>,
    /// How many commits have already been fed to `graph_data`, so the stream can be appended
    /// rather than rebuilt from scratch on every batch.
    consumed_commits: usize,
    /// Read by the lane canvas to follow the list's scrolling, and by the panel's tests, which take
    /// `last_item_size` as the proof that the list was laid out with a height at all.
    pub(super) scroll_handle: UniformListScrollHandle,
}

impl GraphSectionState {
    fn is_auto(&self) -> bool {
        matches!(self.log_source, LogSource::Branch(_))
    }
}

impl GitPanel {
    /// Only ever called from the expand path and from the invalidation, never from `render` —
    /// `Repository::graph_data` starts the fetch.
    pub(super) fn ensure_graph_loaded(&mut self, cx: &mut Context<Self>) {
        let Some(repository) = self.active_repository.clone() else {
            return;
        };
        let Some(branch) = self.current_branch_ref(cx) else {
            return;
        };
        let accents = accent_colors_count(cx.theme().accents());

        let state = self.graph_section.get_or_insert_with(|| GraphSectionState {
            graph_data: GraphData::new(accents),
            log_source: LogSource::Branch(branch.clone()),
            loaded_for_branch: None,
            consumed_commits: 0,
            scroll_handle: UniformListScrollHandle::new(),
        });

        // "Auto" follows HEAD, so a checkout re-resolves which branch is being shown.
        if state.is_auto() {
            state.log_source = LogSource::Branch(branch.clone());
        }
        if state.loaded_for_branch.as_ref() == Some(&branch) && state.consumed_commits > 0 {
            return;
        }
        state.loaded_for_branch = Some(branch);
        state.graph_data.clear();
        state.consumed_commits = 0;

        let log_source = state.log_source.clone();
        repository.update(cx, |repository, cx| {
            repository.graph_data(log_source, LogOrder::default(), 0..usize::MAX, cx);
        });
        self.sync_graph_section(cx);
    }

    /// Appends whatever the fetch has streamed in since last time. Reads through `get_graph_data`,
    /// which never starts a fetch, so this is safe to call on every graph event.
    pub(super) fn sync_graph_section(&mut self, cx: &mut Context<Self>) {
        let Some(repository) = self.active_repository.clone() else {
            return;
        };
        let Some(state) = self.graph_section.as_mut() else {
            return;
        };
        let log_source = state.log_source.clone();
        let repository = repository.read(cx);
        let Some(data) = repository.get_graph_data(log_source, LogOrder::default()) else {
            return;
        };
        if data.commit_data.len() <= state.consumed_commits {
            return;
        }
        let fresh = data.commit_data[state.consumed_commits..].to_vec();
        state.consumed_commits = data.commit_data.len();
        state.graph_data.add_commits(&fresh);
        cx.notify();
    }

    pub(super) fn invalidate_graph(&mut self, cx: &mut Context<Self>) {
        if self.graph_section.is_none() {
            return;
        }
        if self.section_expanded(PanelSectionKind::Graph) {
            if let Some(state) = self.graph_section.as_mut() {
                state.loaded_for_branch = None;
                state.consumed_commits = 0;
                state.graph_data.clear();
            }
            self.ensure_graph_loaded(cx);
        } else {
            self.graph_section = None;
        }
    }

    /// Whether the graph is following the checked-out branch rather than showing every ref.
    pub(super) fn graph_log_source_is_auto(&self) -> bool {
        self.graph_section
            .as_ref()
            .map_or(true, |state| state.is_auto())
    }

    pub(super) fn toggle_graph_log_source(&mut self, cx: &mut Context<Self>) {
        let Some(branch) = self.current_branch_ref(cx) else {
            return;
        };
        if let Some(state) = self.graph_section.as_mut() {
            state.log_source = if state.is_auto() {
                LogSource::All
            } else {
                LogSource::Branch(branch)
            };
            state.loaded_for_branch = None;
            state.consumed_commits = 0;
            state.graph_data.clear();
        }
        self.ensure_graph_loaded(cx);
    }

    pub(super) fn render_graph_section(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let expanded = self.section_expanded(PanelSectionKind::Graph);

        PanelSection::new("git-panel-graph-section", "Graph", expanded)
            .on_toggle(cx.listener(|this, _, _, cx| {
                this.toggle_section(PanelSectionKind::Graph, cx);
            }))
            .actions(self.render_graph_toolbar(cx))
            .fixed_height(
                self.graph_section_height,
                self.render_graph_resize_handle(cx),
            )
            .when(expanded, |section| {
                section.child(self.render_graph_rows(window, cx))
            })
            .into_any_element()
    }

    fn render_graph_toolbar(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let is_auto = self.graph_log_source_is_auto();

        vec![
            Button::new("graph-log-source", if is_auto { "Auto" } else { "All" })
                .label_size(LabelSize::XSmall)
                .size(ButtonSize::Compact)
                .tooltip(Tooltip::text(if is_auto {
                    "Following the current branch — switch to all refs"
                } else {
                    "Showing all refs — switch to the current branch"
                }))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_graph_log_source(cx)))
                .into_any_element(),
            IconButton::new("graph-scroll-to-head", IconName::ArrowCircle)
                .icon_size(IconSize::Small)
                .icon_color(Color::Muted)
                .tooltip(Tooltip::text("Scroll to HEAD"))
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(state) = this.graph_section.as_ref() {
                        state
                            .scroll_handle
                            .scroll_to_item(0, gpui::ScrollStrategy::Top);
                        cx.notify();
                    }
                }))
                .into_any_element(),
            IconButton::new("graph-refresh", IconName::RotateCw)
                .icon_size(IconSize::Small)
                .icon_color(Color::Muted)
                .tooltip(Tooltip::text("Reload the graph"))
                .on_click(cx.listener(|this, _, _, cx| this.invalidate_graph(cx)))
                .into_any_element(),
            // Took `Open Git Graph` over from the Changes overflow menu, where phase 02 parked it.
            IconButton::new("graph-open-tab", IconName::Maximize)
                .icon_size(IconSize::Small)
                .icon_color(Color::Muted)
                .tooltip(Tooltip::for_action_title("Open Git Graph", &Open))
                .on_click(|_, window, cx| window.dispatch_action(Open.boxed_clone(), cx))
                .into_any_element(),
        ]
    }

    fn render_graph_rows(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.graph_section.as_ref() else {
            return div().into_any_element();
        };
        let commit_count = state.graph_data.commits.len();

        if commit_count == 0 {
            return h_flex()
                .w_full()
                .px_2()
                .py_1()
                .child(
                    Label::new("Loading graph…")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .into_any_element();
        }

        // The canvas alone is sized to the whole log: it is absolutely positioned, so its width
        // only bounds where lines may be painted and costs the subjects nothing.
        let lane_column_width = subject_indent(state.graph_data.max_lanes);

        // A column, not a bare `div`: `div()` lays its children out in a row, where the `flex_1`
        // below grows the list's width and leaves its height at zero. A `uniform_list` contributes
        // no height of its own, so in a row it renders no rows at all — the lane canvas, which sets
        // `h_full` outright, would be the only thing on screen.
        v_flex()
            .relative()
            .size_full()
            .child(self.render_lane_canvas(lane_column_width, window, cx))
            .child(
                uniform_list(
                    "git-panel-graph",
                    commit_count,
                    cx.processor(move |this, range: Range<usize>, window, cx| {
                        let Some(repository) = this.active_repository.clone() else {
                            return Vec::new();
                        };
                        let rows = this
                            .graph_section
                            .as_ref()
                            .map(|state| {
                                state
                                    .graph_data
                                    .commits
                                    .get(range.clone())
                                    .unwrap_or_default()
                                    .iter()
                                    .map(|commit| (commit.data.sha, commit.lanes))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();

                        rows.into_iter()
                            .enumerate()
                            .map(|(offset, (sha, lanes))| {
                                this.render_graph_row(
                                    range.start + offset,
                                    sha,
                                    subject_indent(lanes),
                                    &repository,
                                    window,
                                    cx,
                                )
                            })
                            .collect()
                    }),
                )
                .flex_1()
                .track_scroll(&state.scroll_handle),
            )
            .into_any_element()
    }

    /// The lane graph itself, painted by the same function the graph tab uses.
    fn render_lane_canvas(
        &self,
        lane_column_width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.graph_section.as_ref() else {
            return div().into_any_element();
        };

        let row_height = panel_row_height(window);
        let scroll_offset_y = -state.scroll_handle.0.borrow().base_handle.offset().y;
        let scroll_offset_y = scroll_offset_y.max(px(0.));
        let first_visible_row = (scroll_offset_y / row_height).floor() as usize;
        let vertical_scroll_offset = scroll_offset_y - (first_visible_row as f32 * row_height);

        let commit_count = state.graph_data.commits.len();
        let visible_rows = (self.graph_section_height / row_height).ceil() as usize + 1;
        let range = first_visible_row.min(commit_count.saturating_sub(1))
            ..(first_visible_row + visible_rows).min(commit_count);

        let rows = state.graph_data.commits[range.clone()].to_vec();
        let commit_lines = state
            .graph_data
            .lines
            .iter()
            .filter(|line| {
                line.full_interval.start <= range.end && line.full_interval.end >= range.start
            })
            .cloned()
            .collect::<Vec<_>>();

        let accent_colors = cx.theme().accents().clone();
        let hover_bg = cx.theme().colors().element_hover.opacity(0.6);
        let selected_bg = cx.theme().colors().element_selected;

        div()
            .absolute()
            .top_0()
            .left_0()
            .h_full()
            .w(lane_column_width)
            .child(
                gpui::canvas(
                    |_, _, _| {},
                    move |bounds: Bounds<Pixels>, _: (), window: &mut Window, _cx: &mut App| {
                        git_graph_core::paint_lanes(
                            LanePaint {
                                rows,
                                commit_lines,
                                first_visible_row,
                                vertical_scroll_offset,
                                row_height,
                                // The panel does not track hover or selection on graph rows; the
                                // row list above handles its own highlighting.
                                hovered_entry_idx: None,
                                selected_entry_idx: None,
                                accent_colors,
                                hover_bg,
                                selected_bg,
                            },
                            bounds,
                            window,
                        )
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }

    fn render_graph_row(
        &self,
        ix: usize,
        sha: git::Oid,
        subject_indent: Pixels,
        repository: &Entity<Repository>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let data = repository.update(cx, |repository, cx| {
            repository.fetch_commit_data(sha, cx).clone()
        });
        let subject = match &data {
            project::git_store::CommitDataState::Loaded(data) => data.subject.clone(),
            project::git_store::CommitDataState::Loading => SharedString::from("…"),
        };
        let sha = SharedString::from(sha.to_string());
        let workspace = self.workspace.clone();
        let repo = repository.downgrade();

        h_flex()
            .id(("graph-row", ix))
            .h(panel_row_height(window))
            .w_full()
            .items_center()
            .pr_2()
            .cursor_pointer()
            .hover(|this| this.bg(cx.theme().colors().element_hover))
            .on_click({
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
            // The subject starts where *this row's* lanes end, so it sits against its own
            // branch rather than against the deepest one in the log.
            .child(div().flex_none().w(subject_indent))
            .child(
                div().flex_1().overflow_hidden().child(
                    Label::new(subject)
                        .size(LabelSize::Small)
                        .single_line()
                        .truncate(),
                ),
            )
            .into_any_element()
    }

    /// The section's top rule, with a hit area straddling it so the boundary itself is grabbable.
    fn render_graph_resize_handle(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("graph-section-resize-container")
            .relative()
            .w_full()
            .flex_none()
            .h(px(1.))
            .bg(cx.theme().colors().border_variant)
            .child(
                div()
                    .id("graph-section-resize-handle")
                    .absolute()
                    .top(px(-3.))
                    .w_full()
                    .h(px(7.))
                    .cursor_row_resize()
                    .block_mouse_except_scroll()
                    .on_drag(DraggedGraphSectionHandle, |_, _, _, cx| {
                        cx.new(|_| gpui::Empty)
                    }),
            )
            .into_any_element()
    }

    pub(super) fn resize_graph_section(
        &mut self,
        event: &gpui::DragMoveEvent<DraggedGraphSectionHandle>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let position = event.event.position.y;
        if let Some(previous) = self.graph_resize_drag_start {
            self.graph_section_height =
                height_dragged_from_top_edge(self.graph_section_height, previous, position);
        }
        self.graph_resize_drag_start = Some(position);
        cx.notify();
    }

    pub(super) fn end_graph_section_resize(&mut self, cx: &mut Context<Self>) {
        self.graph_resize_drag_start = None;
        self.serialize(cx);
    }
}
