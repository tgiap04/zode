use crate::database_panel::DatabasePanel;
use crate::panel_layout::{DraggedSplit, Split};
use gpui::{Context, DragMoveEvent, Window, uniform_list};
use ui::{Tooltip, prelude::*};
use zed_actions::database as actions;

impl Render for DatabasePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `SURFACE_MARGIN`/`SURFACE_ROUNDING`, as the agent column carries: an
        // own column is a surface beside the centre, not a tool dock, and
        // `Dock::render` leaves the dock chrome off for both.
        v_flex()
            .key_context("DatabasePanel")
            .track_focus(&self.focus_handle)
            .on_action(
                cx.listener(|this, _: &actions::RunQuery, window, cx| this.run_query(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &actions::CancelQuery, _window, cx| this.cancel_query(cx)),
            )
            .on_action(
                cx.listener(|this, _: &actions::NextPage, window, cx| this.page(true, window, cx)),
            )
            .on_action(cx.listener(|this, _: &actions::PreviousPage, window, cx| {
                this.page(false, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &actions::CopyResultsAsCsv, _window, cx| {
                    this.copy_page_as_csv(cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &actions::ToggleFullScreen, window, cx| {
                    this.toggle_full_screen(window, cx)
                }),
            )
            // Escape leaves full screen and nothing else: it is the gesture
            // everyone tries first, and with the rail hidden the button is the
            // only other way back.
            .on_action(cx.listener(|this, _: &menu::Cancel, window, cx| {
                if this.full_screen {
                    this.toggle_full_screen(window, cx);
                }
            }))
            .size_full()
            .p(workspace::pane_group::SURFACE_MARGIN)
            .child(
                v_flex()
                    .size_full()
                    .min_h_0()
                    .rounded(workspace::pane_group::SURFACE_ROUNDING)
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .bg(cx.theme().colors().panel_background)
                    .overflow_hidden()
                    .on_drag_move(cx.listener(
                        |this, event: &DragMoveEvent<DraggedSplit>, _window, cx| {
                            this.on_split_dragged(event, cx);
                        },
                    ))
                    .child(self.render_header(cx))
                    .map(|element| {
                        if self.connections.is_empty() {
                            element.child(self.render_empty())
                        } else if self.full_screen {
                            element.child(self.render_full_screen(window, cx))
                        } else {
                            element
                                .child(self.render_tree(cx))
                                // The SQL buffer and the grid appear only once
                                // there is a connection to run against: two
                                // empty boxes above an unconfigured tree read as
                                // broken rather than as waiting.
                                .when(self.active.is_some(), |element| {
                                    element
                                        .child(self.render_split_handle(Split::TreeAndSql, cx))
                                        .child(self.render_sql(window, cx))
                                        .child(self.render_split_handle(Split::SqlAndResults, cx))
                                        .child(self.render_result_area(true, window, cx))
                                })
                        }
                    })
                    // Deferred and anchored at the pointer, so the menu is not
                    // clipped by the tree's own scroll area.
                    .children(self.context_menu.as_ref().map(|(menu, position, _)| {
                        gpui::deferred(
                            gpui::anchored()
                                .position(*position)
                                .anchor(gpui::Anchor::TopLeft)
                                .child(menu.clone()),
                        )
                        .with_priority(1)
                    })),
            )
    }
}

impl DatabasePanel {
    /// The table list on the left; the rows above the statement on the right.
    ///
    /// Deliberately not the column's own order. Stacked in a column, the tree
    /// reads first because it is what you act on; given the window, the rows are
    /// what you came to read, so they take the top and the height nobody has to
    /// drag back. The statement sits under them, where it is typed into.
    ///
    /// Everything on the right is drawn whether or not a connection is open --
    /// unlike the column, where the same two regions would be empty boxes. Here
    /// they are the layout that was asked for, and the data view says in words
    /// that it is waiting for a table.
    fn render_full_screen(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(self.render_tree(cx))
            .child(self.render_split_handle(Split::TreeAndBody, cx))
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .min_h_0()
                    .child(self.render_result_area(false, window, cx))
                    .child(self.render_split_handle(Split::SqlAndResults, cx))
                    .child(self.render_sql(window, cx)),
            )
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let can_run = self.active.is_some();
        let full_screen = self.full_screen;
        h_flex()
            .flex_none()
            .w_full()
            .px_2()
            .py_1()
            .gap_1()
            .justify_between()
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .child(Label::new("Database").size(LabelSize::Small))
            .child(
                h_flex()
                    .gap_1()
                    .when(can_run, |element| {
                        element.child(
                            IconButton::new("database-run", IconName::PlayFilled)
                                .icon_size(IconSize::Small)
                                .tooltip(|_window, cx| {
                                    Tooltip::for_action("Run", &actions::RunQuery, cx)
                                })
                                .on_click(cx.listener(|this, _event, window, cx| {
                                    this.run_query(window, cx);
                                })),
                        )
                    })
                    .child(
                        IconButton::new(
                            "database-full-screen",
                            if self.full_screen {
                                IconName::Minimize
                            } else {
                                IconName::Maximize
                            },
                        )
                        .icon_size(IconSize::Small)
                        .tooltip(move |_window, cx| {
                            Tooltip::for_action(
                                if full_screen {
                                    "Exit full screen"
                                } else {
                                    "Full screen"
                                },
                                &actions::ToggleFullScreen,
                                cx,
                            )
                        })
                        .on_click(cx.listener(
                            |this, _event, window, cx| {
                                this.toggle_full_screen(window, cx);
                            },
                        )),
                    )
                    .child(
                        IconButton::new("database-add-connection", IconName::Plus)
                            .icon_size(IconSize::Small)
                            .tooltip(|_window, cx| {
                                Tooltip::for_action("Add connection", &actions::AddConnection, cx)
                            })
                            // Dispatched, not called: the workspace action is
                            // the one road to this dialog, so both buttons that
                            // open it behave identically.
                            .on_click(|_event, window, cx| {
                                window.dispatch_action(Box::new(actions::AddConnection), cx)
                            }),
                    ),
            )
    }

    fn render_empty(&self) -> impl IntoElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_1()
            .child(
                Icon::new(IconName::Database)
                    .size(IconSize::XLarge)
                    .color(Color::Muted),
            )
            .child(Label::new("No database connections").color(Color::Muted))
            .child(
                Button::new("database-empty-add", "Add a connection")
                    .style(ButtonStyle::Filled)
                    .on_click(|_event, window, cx| {
                        window.dispatch_action(Box::new(actions::AddConnection), cx)
                    }),
            )
    }

    /// Virtualised: a Postgres with a few thousand tables is one connection
    /// away, and drawing every row of it each frame is not an option.
    ///
    /// The parent is a `v_flex` with `min_h_0`, not a bare `div()`: `div()` lays
    /// out as a row, and `uniform_list` has no intrinsic height -- under one it
    /// draws full width and zero rows, silently and without a panic.
    fn render_tree(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows();
        let count = rows.len();
        let rows = std::rc::Rc::new(rows);

        // Fixed once there is something below it, flexible when there is not:
        // the results are the only region that should absorb a resized window,
        // and a tree sharing that job would move every time the grid did.
        //
        // `h(..)` without `flex_none`, so a panel too short for both still
        // shrinks the tree rather than pushing the grid out of sight.
        //
        // Full screen turns that on its side -- a fixed width, the full height,
        // and `flex_none` because a row would otherwise squeeze it to fit the
        // grid beside it. It stays a `v_flex` with `min_h_0` in both: the list
        // inside has no intrinsic height, and under a row parent it would draw
        // full width and zero rows, silently and without a panic.
        v_flex()
            .map(|tree| {
                if self.full_screen {
                    tree.w(self.tree_width)
                        .h_full()
                        .flex_none()
                        .border_r_1()
                        .border_color(cx.theme().colors().border)
                } else if self.active.is_some() {
                    tree.h(self.tree_height)
                } else {
                    tree.flex_1()
                }
            })
            .min_h_0()
            .debug_selector(|| "database-tree-column".into())
            .child(
                uniform_list(
                    "database-tree",
                    count,
                    cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                        range
                            .filter_map(|index| {
                                rows.get(index).map(|row| this.render_row(row, window, cx))
                            })
                            .collect()
                    }),
                )
                .size_full(),
            )
    }

    fn render_sql(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .h(self.sql_height)
            .w_full()
            .flex_none()
            .border_t_1()
            .border_color(cx.theme().colors().border)
            .debug_selector(|| "database-query-input".into())
            .child(
                div()
                    .size_full()
                    .p_1()
                    .overflow_hidden()
                    .child(self.sql_editor.clone()),
            )
    }

    /// `divider_above` because the same region sits under the scratch buffer in
    /// the column and above it in full screen, and the line belongs between them
    /// once rather than on both sides of it.
    fn render_result_area(
        &self,
        divider_above: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .when(divider_above, |element| {
                element
                    .border_t_1()
                    .border_color(cx.theme().colors().border)
            })
            .debug_selector(|| "database-data-view".into())
            .child(self.render_results(window, cx))
    }
}
