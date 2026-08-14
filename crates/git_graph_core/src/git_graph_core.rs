//! Lane layout and painting for the git commit graph.
//!
//! Split out of `git_graph` so the graph tab and the git panel's Graph section can share one
//! implementation instead of each carrying its own copy of the lane maths.

use collections::{BTreeMap, HashMap};
use git::{Oid, repository::InitialGraphCommitData};
use gpui::{Bounds, Hsla, PathBuilder, Pixels, Window, point, px};
use smallvec::{SmallVec, smallvec};
use std::{ops::Range, rc::Rc, sync::Arc};
use theme::AccentColors;

pub const COMMIT_CIRCLE_RADIUS: Pixels = px(3.5);
pub const COMMIT_CIRCLE_STROKE_WIDTH: Pixels = px(1.5);
pub const LANE_WIDTH: Pixels = px(16.0);
pub const LEFT_PADDING: Pixels = px(12.0);
pub const LINE_WIDTH: Pixels = px(1.5);
/// Extra vertical breathing room added to the UI line height when computing the graph's row
/// height, so commit dots and lines have space around them.
pub const ROW_VERTICAL_PADDING: Pixels = px(4.0);

pub fn accent_colors_count(accents: &AccentColors) -> usize {
    accents.0.len()
}

#[derive(Copy, Clone, Debug)]
pub struct BranchColor(pub u8);

#[derive(Debug)]
pub enum LaneState {
    Empty,
    Active {
        child: Oid,
        parent: Oid,
        color: Option<BranchColor>,
        starting_row: usize,
        starting_col: usize,
        destination_column: Option<usize>,
        segments: SmallVec<[CommitLineSegment; 1]>,
    },
}

impl LaneState {
    pub fn to_commit_lines(
        &mut self,
        ending_row: usize,
        lane_column: usize,
        parent_column: usize,
        parent_color: BranchColor,
    ) -> Option<CommitLine> {
        let state = std::mem::replace(self, LaneState::Empty);

        match state {
            LaneState::Active {
                #[cfg_attr(not(test), allow(unused_variables))]
                parent,
                #[cfg_attr(not(test), allow(unused_variables))]
                child,
                color,
                starting_row,
                starting_col,
                destination_column,
                mut segments,
            } => {
                let final_destination = destination_column.unwrap_or(parent_column);
                let final_color = color.unwrap_or(parent_color);

                Some(CommitLine {
                    #[cfg(any(test, feature = "test-support"))]
                    child,
                    #[cfg(any(test, feature = "test-support"))]
                    parent,
                    child_column: starting_col,
                    full_interval: starting_row..ending_row,
                    color_idx: final_color.0 as usize,
                    segments: {
                        match segments.last_mut() {
                            Some(CommitLineSegment::Straight { to_row })
                                if *to_row == usize::MAX =>
                            {
                                if final_destination != lane_column {
                                    *to_row = ending_row - 1;

                                    let curved_line = CommitLineSegment::Curve {
                                        to_column: final_destination,
                                        on_row: ending_row,
                                        curve_kind: CurveKind::Checkout,
                                    };

                                    if *to_row == starting_row {
                                        let last_index = segments.len() - 1;
                                        segments[last_index] = curved_line;
                                    } else {
                                        segments.push(curved_line);
                                    }
                                } else {
                                    *to_row = ending_row;
                                }
                            }
                            Some(CommitLineSegment::Curve {
                                on_row,
                                to_column,
                                curve_kind,
                            }) if *on_row == usize::MAX => {
                                if *to_column == usize::MAX {
                                    *to_column = final_destination;
                                }
                                if matches!(curve_kind, CurveKind::Merge) {
                                    *on_row = starting_row + 1;
                                    if *on_row < ending_row {
                                        if *to_column != final_destination {
                                            segments.push(CommitLineSegment::Straight {
                                                to_row: ending_row - 1,
                                            });
                                            segments.push(CommitLineSegment::Curve {
                                                to_column: final_destination,
                                                on_row: ending_row,
                                                curve_kind: CurveKind::Checkout,
                                            });
                                        } else {
                                            segments.push(CommitLineSegment::Straight {
                                                to_row: ending_row,
                                            });
                                        }
                                    } else if *to_column != final_destination {
                                        segments.push(CommitLineSegment::Curve {
                                            to_column: final_destination,
                                            on_row: ending_row,
                                            curve_kind: CurveKind::Checkout,
                                        });
                                    }
                                } else {
                                    *on_row = ending_row;
                                    if *to_column != final_destination {
                                        segments.push(CommitLineSegment::Straight {
                                            to_row: ending_row,
                                        });
                                        segments.push(CommitLineSegment::Curve {
                                            to_column: final_destination,
                                            on_row: ending_row,
                                            curve_kind: CurveKind::Checkout,
                                        });
                                    }
                                }
                            }
                            Some(CommitLineSegment::Curve {
                                on_row, to_column, ..
                            }) => {
                                if *on_row < ending_row {
                                    if *to_column != final_destination {
                                        segments.push(CommitLineSegment::Straight {
                                            to_row: ending_row - 1,
                                        });
                                        segments.push(CommitLineSegment::Curve {
                                            to_column: final_destination,
                                            on_row: ending_row,
                                            curve_kind: CurveKind::Checkout,
                                        });
                                    } else {
                                        segments.push(CommitLineSegment::Straight {
                                            to_row: ending_row,
                                        });
                                    }
                                } else if *to_column != final_destination {
                                    segments.push(CommitLineSegment::Curve {
                                        to_column: final_destination,
                                        on_row: ending_row,
                                        curve_kind: CurveKind::Checkout,
                                    });
                                }
                            }
                            _ => {}
                        }

                        segments
                    },
                })
            }
            LaneState::Empty => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            LaneState::Empty => true,
            LaneState::Active { .. } => false,
        }
    }
}

pub struct CommitEntry {
    pub data: Arc<InitialGraphCommitData>,
    pub lane: usize,
    pub color_idx: usize,
    /// Lanes drawn on this commit's row — its own and every branch running past it.
    ///
    /// Per row, unlike `max_lanes`, so a view can start its subject clear of the lanes actually
    /// beside it rather than clear of the deepest merge anywhere in the log.
    pub lanes: usize,
}

pub type ActiveLaneIdx = usize;

pub enum AllCommitCount {
    NotLoaded,
    Loaded(usize),
}

#[derive(Debug)]
pub enum CurveKind {
    Merge,
    Checkout,
}

#[derive(Debug)]
pub enum CommitLineSegment {
    Straight {
        to_row: usize,
    },
    Curve {
        to_column: usize,
        on_row: usize,
        curve_kind: CurveKind,
    },
}

#[derive(Debug)]
pub struct CommitLine {
    #[cfg(any(test, feature = "test-support"))]
    pub child: Oid,
    #[cfg(any(test, feature = "test-support"))]
    pub parent: Oid,
    pub child_column: usize,
    pub full_interval: Range<usize>,
    pub color_idx: usize,
    pub segments: SmallVec<[CommitLineSegment; 1]>,
}

impl CommitLine {
    pub fn get_first_visible_segment_idx(
        &self,
        first_visible_row: usize,
    ) -> Option<(usize, usize)> {
        if first_visible_row > self.full_interval.end {
            return None;
        } else if first_visible_row <= self.full_interval.start {
            return Some((0, self.child_column));
        }

        let mut current_column = self.child_column;

        for (idx, segment) in self.segments.iter().enumerate() {
            match segment {
                CommitLineSegment::Straight { to_row } => {
                    if *to_row >= first_visible_row {
                        return Some((idx, current_column));
                    }
                }
                CommitLineSegment::Curve {
                    to_column, on_row, ..
                } => {
                    if *on_row >= first_visible_row {
                        return Some((idx, current_column));
                    }
                    current_column = *to_column;
                }
            }
        }

        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CommitLineKey {
    pub child: Oid,
    pub parent: Oid,
}

pub struct GraphData {
    pub lane_states: SmallVec<[LaneState; 8]>,
    pub lane_colors: HashMap<ActiveLaneIdx, BranchColor>,
    pub parent_to_lanes: HashMap<Oid, SmallVec<[usize; 1]>>,
    pub next_color: BranchColor,
    pub accent_colors_count: usize,
    pub commits: Vec<Rc<CommitEntry>>,
    pub max_commit_count: AllCommitCount,
    pub max_lanes: usize,
    pub lines: Vec<Rc<CommitLine>>,
    pub active_commit_lines: HashMap<CommitLineKey, usize>,
    pub active_commit_lines_by_parent: HashMap<Oid, SmallVec<[usize; 1]>>,
}

impl GraphData {
    pub fn new(accent_colors_count: usize) -> Self {
        GraphData {
            lane_states: SmallVec::default(),
            lane_colors: HashMap::default(),
            parent_to_lanes: HashMap::default(),
            next_color: BranchColor(0),
            accent_colors_count,
            commits: Vec::default(),
            max_commit_count: AllCommitCount::NotLoaded,
            max_lanes: 0,
            lines: Vec::default(),
            active_commit_lines: HashMap::default(),
            active_commit_lines_by_parent: HashMap::default(),
        }
    }

    pub fn clear(&mut self) {
        self.lane_states.clear();
        self.lane_colors.clear();
        self.parent_to_lanes.clear();
        self.commits.clear();
        self.lines.clear();
        self.active_commit_lines.clear();
        self.active_commit_lines_by_parent.clear();
        self.next_color = BranchColor(0);
        self.max_commit_count = AllCommitCount::NotLoaded;
        self.max_lanes = 0;
    }

    pub fn first_empty_lane_idx(&mut self) -> ActiveLaneIdx {
        self.lane_states
            .iter()
            .position(LaneState::is_empty)
            .unwrap_or_else(|| {
                self.lane_states.push(LaneState::Empty);
                self.lane_states.len() - 1
            })
    }

    pub fn get_lane_color(&mut self, lane_idx: ActiveLaneIdx) -> BranchColor {
        let accent_colors_count = self.accent_colors_count;
        *self.lane_colors.entry(lane_idx).or_insert_with(|| {
            let color_idx = self.next_color;
            self.next_color = BranchColor((self.next_color.0 + 1) % accent_colors_count as u8);
            color_idx
        })
    }

    pub fn add_commits(&mut self, commits: &[Arc<InitialGraphCommitData>]) {
        self.commits.reserve(commits.len());
        self.lines.reserve(commits.len() / 2);

        for commit in commits.iter() {
            let commit_row = self.commits.len();

            let commit_lane = self
                .parent_to_lanes
                .get(&commit.sha)
                .and_then(|lanes| lanes.first().copied());

            let commit_lane = commit_lane.unwrap_or_else(|| self.first_empty_lane_idx());

            let commit_color = self.get_lane_color(commit_lane);

            if let Some(lanes) = self.parent_to_lanes.remove(&commit.sha) {
                for lane_column in lanes {
                    let state = &mut self.lane_states[lane_column];

                    if let LaneState::Active {
                        starting_row,
                        segments,
                        ..
                    } = state
                    {
                        if let Some(CommitLineSegment::Curve {
                            to_column,
                            curve_kind: CurveKind::Merge,
                            ..
                        }) = segments.first_mut()
                        {
                            let curve_row = *starting_row + 1;
                            let would_overlap =
                                if lane_column != commit_lane && curve_row < commit_row {
                                    self.commits[curve_row..commit_row]
                                        .iter()
                                        .any(|c| c.lane == commit_lane)
                                } else {
                                    false
                                };

                            if would_overlap {
                                *to_column = lane_column;
                            }
                        }
                    }

                    if let Some(commit_line) =
                        state.to_commit_lines(commit_row, lane_column, commit_lane, commit_color)
                    {
                        self.lines.push(Rc::new(commit_line));
                    }
                }
            }

            commit
                .parents
                .iter()
                .enumerate()
                .for_each(|(parent_idx, parent)| {
                    if parent_idx == 0 {
                        self.lane_states[commit_lane] = LaneState::Active {
                            parent: *parent,
                            child: commit.sha,
                            color: Some(commit_color),
                            starting_col: commit_lane,
                            starting_row: commit_row,
                            destination_column: None,
                            segments: smallvec![CommitLineSegment::Straight { to_row: usize::MAX }],
                        };

                        self.parent_to_lanes
                            .entry(*parent)
                            .or_default()
                            .push(commit_lane);
                    } else {
                        let new_lane = self.first_empty_lane_idx();

                        self.lane_states[new_lane] = LaneState::Active {
                            parent: *parent,
                            child: commit.sha,
                            color: None,
                            starting_col: commit_lane,
                            starting_row: commit_row,
                            destination_column: None,
                            segments: smallvec![CommitLineSegment::Curve {
                                to_column: usize::MAX,
                                on_row: usize::MAX,
                                curve_kind: CurveKind::Merge,
                            },],
                        };

                        self.parent_to_lanes
                            .entry(*parent)
                            .or_default()
                            .push(new_lane);
                    }
                });

            self.max_lanes = self.max_lanes.max(self.lane_states.len());

            // The last *occupied* lane, not `lane_states.len()`: a lane that has been freed
            // stays in the vector as `Empty` and would count as width nothing is drawn in.
            //
            // Never fewer than this commit's own lane, though. A commit with no parents releases
            // its lane as it is processed, so the states no longer mention the row the node is
            // still drawn on — the root commit came out as occupying nothing at all.
            let lanes = self
                .lane_states
                .iter()
                .rposition(|lane| !lane.is_empty())
                .map_or(0, |last| last + 1)
                .max(commit_lane + 1);

            self.commits.push(Rc::new(CommitEntry {
                data: commit.clone(),
                lane: commit_lane,
                color_idx: commit_color.0 as usize,
                lanes,
            }));
        }

        self.max_commit_count = AllCommitCount::Loaded(self.commits.len());
    }
}

pub fn lane_center_x(bounds: Bounds<Pixels>, lane: f32) -> Pixels {
    bounds.origin.x + LEFT_PADDING + lane * LANE_WIDTH + LANE_WIDTH / 2.0
}

pub fn to_row_center(
    to_row: usize,
    row_height: Pixels,
    scroll_offset: Pixels,
    bounds: Bounds<Pixels>,
) -> Pixels {
    bounds.origin.y + to_row as f32 * row_height + row_height / 2.0 - scroll_offset
}

pub fn draw_commit_circle(center_x: Pixels, center_y: Pixels, color: Hsla, window: &mut Window) {
    let radius = COMMIT_CIRCLE_RADIUS;

    let mut builder = PathBuilder::fill();

    // Start at the rightmost point of the circle
    builder.move_to(point(center_x + radius, center_y));

    // Draw the circle using two arc_to calls (top half, then bottom half)
    builder.arc_to(
        point(radius, radius),
        px(0.),
        false,
        true,
        point(center_x - radius, center_y),
    );
    builder.arc_to(
        point(radius, radius),
        px(0.),
        false,
        true,
        point(center_x + radius, center_y),
    );
    builder.close();

    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// Paints the lane graph — row highlights, commit circles and the lines between them — for one
/// viewport's worth of commits.
///
/// Takes everything it needs by value so both the graph tab and the git panel's Graph section can
/// call it. There is deliberately no second copy of this logic anywhere.
pub struct LanePaint {
    pub rows: Vec<Rc<CommitEntry>>,
    pub commit_lines: Vec<Rc<CommitLine>>,
    pub first_visible_row: usize,
    pub vertical_scroll_offset: Pixels,
    pub row_height: Pixels,
    pub hovered_entry_idx: Option<usize>,
    pub selected_entry_idx: Option<usize>,
    pub accent_colors: AccentColors,
    pub hover_bg: Hsla,
    pub selected_bg: Hsla,
}

pub fn paint_lanes(params: LanePaint, bounds: Bounds<Pixels>, window: &mut Window) {
    let LanePaint {
        rows,
        commit_lines,
        first_visible_row,
        vertical_scroll_offset,
        row_height,
        hovered_entry_idx,
        selected_entry_idx,
        accent_colors,
        hover_bg,
        selected_bg,
    } = params;
    let mut lines: BTreeMap<usize, Vec<_>> = BTreeMap::new();

    window.paint_layer(bounds, |window| {
        for visible_row_idx in 0..rows.len() {
            let absolute_row_idx = first_visible_row + visible_row_idx;
            let is_hovered = hovered_entry_idx == Some(absolute_row_idx);
            let is_selected = selected_entry_idx == Some(absolute_row_idx);

            if is_hovered || is_selected {
                let row_y =
                    bounds.origin.y + visible_row_idx as f32 * row_height - vertical_scroll_offset;

                let row_bounds = Bounds::new(
                    point(bounds.origin.x, row_y),
                    gpui::Size {
                        width: bounds.size.width,
                        height: row_height,
                    },
                );

                let bg_color = if is_selected { selected_bg } else { hover_bg };
                window.paint_quad(gpui::fill(row_bounds, bg_color));
            }
        }

        for (row_idx, row) in rows.into_iter().enumerate() {
            let row_color = accent_colors.color_for_index(row.color_idx as u32);
            let row_y_center = bounds.origin.y + row_idx as f32 * row_height + row_height / 2.0
                - vertical_scroll_offset;

            let commit_x = lane_center_x(bounds, row.lane as f32);

            draw_commit_circle(commit_x, row_y_center, row_color, window);
        }

        for line in commit_lines {
            let Some((start_segment_idx, start_column)) =
                line.get_first_visible_segment_idx(first_visible_row)
            else {
                continue;
            };

            let line_x = lane_center_x(bounds, start_column as f32);

            let start_row = line.full_interval.start as i32 - first_visible_row as i32;

            let from_y = bounds.origin.y + start_row as f32 * row_height + row_height / 2.0
                - vertical_scroll_offset
                + COMMIT_CIRCLE_RADIUS;

            let mut current_row = from_y;
            let mut current_column = line_x;

            let mut builder = PathBuilder::stroke(LINE_WIDTH);
            builder.move_to(point(line_x, from_y));

            let segments = &line.segments[start_segment_idx..];
            let desired_curve_height = row_height / 3.0;
            let desired_curve_width = LANE_WIDTH / 3.0;

            for (segment_idx, segment) in segments.iter().enumerate() {
                let is_last = segment_idx + 1 == segments.len();

                match segment {
                    CommitLineSegment::Straight { to_row } => {
                        let mut dest_row = to_row_center(
                            to_row - first_visible_row,
                            row_height,
                            vertical_scroll_offset,
                            bounds,
                        );
                        if is_last {
                            dest_row -= COMMIT_CIRCLE_RADIUS;
                        }

                        let dest_point = point(current_column, dest_row);

                        current_row = dest_point.y;
                        builder.line_to(dest_point);
                        builder.move_to(dest_point);
                    }
                    CommitLineSegment::Curve {
                        to_column,
                        on_row,
                        curve_kind,
                    } => {
                        let mut to_column = lane_center_x(bounds, *to_column as f32);

                        let mut to_row = to_row_center(
                            *on_row - first_visible_row,
                            row_height,
                            vertical_scroll_offset,
                            bounds,
                        );

                        // This means that this branch was a checkout
                        let going_right = to_column > current_column;
                        let column_shift = if going_right {
                            COMMIT_CIRCLE_RADIUS + COMMIT_CIRCLE_STROKE_WIDTH
                        } else {
                            -COMMIT_CIRCLE_RADIUS - COMMIT_CIRCLE_STROKE_WIDTH
                        };

                        match curve_kind {
                            CurveKind::Checkout => {
                                if is_last {
                                    to_column -= column_shift;
                                }

                                let available_curve_width = (to_column - current_column).abs();
                                let available_curve_height = (to_row - current_row).abs();
                                let curve_width = desired_curve_width.min(available_curve_width);
                                let curve_height = desired_curve_height.min(available_curve_height);
                                let signed_curve_width = if going_right {
                                    curve_width
                                } else {
                                    -curve_width
                                };
                                let curve_start = point(current_column, to_row - curve_height);
                                let curve_end = point(current_column + signed_curve_width, to_row);
                                let curve_control = point(current_column, to_row);

                                builder.move_to(point(current_column, current_row));
                                builder.line_to(curve_start);
                                builder.move_to(curve_start);
                                builder.curve_to(curve_end, curve_control);
                                builder.move_to(curve_end);
                                builder.line_to(point(to_column, to_row));
                            }
                            CurveKind::Merge => {
                                if is_last {
                                    to_row -= COMMIT_CIRCLE_RADIUS;
                                }

                                let merge_start = point(
                                    current_column + column_shift,
                                    current_row - COMMIT_CIRCLE_RADIUS,
                                );
                                let available_curve_width = (to_column - merge_start.x).abs();
                                let available_curve_height = (to_row - merge_start.y).abs();
                                let curve_width = desired_curve_width.min(available_curve_width);
                                let curve_height = desired_curve_height.min(available_curve_height);
                                let signed_curve_width = if going_right {
                                    curve_width
                                } else {
                                    -curve_width
                                };
                                let curve_start =
                                    point(to_column - signed_curve_width, merge_start.y);
                                let curve_end = point(to_column, merge_start.y + curve_height);
                                let curve_control = point(to_column, merge_start.y);

                                builder.move_to(merge_start);
                                builder.line_to(curve_start);
                                builder.move_to(curve_start);
                                builder.curve_to(curve_end, curve_control);
                                builder.move_to(curve_end);
                                builder.line_to(point(to_column, to_row));
                            }
                        }
                        current_row = to_row;
                        current_column = to_column;
                        builder.move_to(point(current_column, current_row));
                    }
                }
            }

            builder.close();
            lines.entry(line.color_idx).or_default().push(builder);
        }

        for (color_idx, builders) in lines {
            let line_color = accent_colors.color_for_index(color_idx as u32);

            for builder in builders {
                if let Ok(path) = builder.build() {
                    // we paint each color on it's own layer to stop overlapping lines
                    // of different colors changing the color of a line
                    window.paint_layer(bounds, |window| {
                        window.paint_path(path, line_color);
                    });
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(byte: u8) -> Oid {
        Oid::from_bytes(&[byte; 20]).expect("twenty bytes make a valid oid")
    }

    fn commit(sha: u8, parents: &[u8]) -> Arc<InitialGraphCommitData> {
        Arc::new(InitialGraphCommitData {
            sha: oid(sha),
            parents: parents.iter().copied().map(oid).collect(),
            ref_names: Vec::new(),
        })
    }

    #[test]
    fn a_log_with_no_branches_leaves_every_row_on_one_lane() {
        let mut data = GraphData::new(8);
        data.add_commits(&[commit(1, &[2]), commit(2, &[3]), commit(3, &[])]);

        assert!(
            data.commits.iter().all(|commit| commit.lanes == 1),
            "nothing runs beside these commits, so no row should reserve a second lane: {:?}",
            data.commits.iter().map(|c| c.lanes).collect::<Vec<_>>()
        );
    }

    /// The point of recording lanes per row rather than reading `max_lanes`.
    ///
    /// A view that indents by `max_lanes` gives every row the width of the most tangled commit in
    /// the log — a commit sitting alone on the first lane is pushed out as if a branch ran beside
    /// it. This is that difference, stated as a property rather than as exact lane numbers, which
    /// are the graph builder's business and not this field's.
    #[test]
    fn a_branch_widens_only_the_rows_it_runs_through() {
        let mut data = GraphData::new(8);
        data.add_commits(&[
            commit(1, &[2, 3]),
            commit(2, &[4]),
            commit(3, &[4]),
            commit(4, &[5]),
            commit(5, &[]),
        ]);

        assert!(
            data.max_lanes >= 2,
            "the fixture has to actually branch for this to test anything"
        );
        assert!(
            data.commits.iter().any(|commit| commit.lanes < data.max_lanes),
            "some row must be narrower than the deepest one, or per-row lanes buys nothing \
             over max_lanes: {:?} against max {}",
            data.commits.iter().map(|c| c.lanes).collect::<Vec<_>>(),
            data.max_lanes
        );
        assert!(
            data.commits.iter().all(|commit| commit.lanes <= data.max_lanes),
            "and none may exceed it"
        );
    }
}
