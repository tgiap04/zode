use gpui::{Point, Size, TestAppContext, VisualContext as _, VisualTestContext, px, size};
use settings::Settings as _;

use crate::host::{FloatingPane, Grip, MUST_STAY_VISIBLE, SMALLEST};

fn init_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let settings_store = settings::SettingsStore::test(cx);
        cx.set_global(settings_store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        project::DisableAiSettings::register(cx);
        crate::init(cx);
    });
}

/// A window in a real workspace, and the visual context it lives in.
///
/// Both are returned because the window outlives the borrow that made it: every
/// test below needs the same context back to drive frames.
async fn a_window(cx: &mut TestAppContext) -> (gpui::Entity<FloatingPane>, &mut VisualTestContext) {
    init_test(cx);
    let fs = fs::FakeFs::new(cx.executor());
    let project = project::Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
    let project = workspace.read_with(cx, |workspace, _| workspace.project().clone());
    let handle = workspace.downgrade();
    let view = cx.new_window_entity(|window, cx| FloatingPane::new(handle, project, window, cx));
    (view, cx)
}

/// A window painted as a window root, so real mouse events reach real hitboxes.
///
/// The geometry tests below call the methods directly; these do not. Which
/// handle a press lands on is decided by GPUI's dispatch, and dispatch is
/// exactly where the top edge was found to be broken -- a test that called
/// `grab` itself would have passed throughout.
async fn a_painted_window(
    cx: &mut TestAppContext,
) -> (gpui::Entity<FloatingPane>, &mut VisualTestContext) {
    init_test(cx);
    let fs = fs::FakeFs::new(cx.executor());
    let project = project::Project::test(fs, [], cx).await;
    let (multi_workspace, host_cx) =
        cx.add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(host_cx, |mw, _| mw.workspace().clone());
    let project = workspace.read_with(host_cx, |workspace, _| workspace.project().clone());
    let handle = workspace.downgrade();

    let (pane, cx) =
        cx.add_window_view(|window, cx| FloatingPane::new(handle, project, window, cx));
    pane.update_in(cx, |pane, window, cx| pane.toggle(window, cx));
    // Parked twice: the first frame paints and its canvas measures the layer,
    // the deferred update lands, and the second frame lays the window out
    // inside the size that measurement found. Hit testing needs the second.
    cx.run_until_parked();
    cx.update(|_window, _cx| {});
    cx.run_until_parked();
    (pane, cx)
}

mod dispatch {
    use super::*;
    use gpui::{Modifiers, MouseButton};

    use crate::host::Grab;

    /// Every edge and every corner claims its own press.
    ///
    /// The top edge was the one reported, but the fault was never specific to
    /// it: the title bar is 34pt tall and overlaps the north grip, both upper
    /// corners and the top of both side edges. GPUI dispatches the bubble phase
    /// in **reverse** registration order, so the grips -- painted after the
    /// title bar -- ran first and the title bar ran last and overwrote them.
    /// Dragging any of those moved the window instead of resizing it.
    #[gpui::test]
    async fn every_edge_and_corner_claims_its_own_press(cx: &mut TestAppContext) {
        let (pane, cx) = a_painted_window(cx).await;
        let container = pane
            .read_with(cx, |pane, _| pane.last_container)
            .expect("the canvas measured the layer");
        let bounds = pane.read_with(cx, |pane, _| pane.bounds_within(container));
        // Three pixels in: comfortably inside the 6pt band, and clear of the
        // rounding at its edges.
        let inside = px(3.);
        let near_x = bounds.origin.x + inside;
        let far_x = bounds.origin.x + bounds.size.width - inside;
        let near_y = bounds.origin.y + inside;
        let far_y = bounds.origin.y + bounds.size.height - inside;
        let mid_x = bounds.origin.x + bounds.size.width / 2.;
        let mid_y = bounds.origin.y + bounds.size.height / 2.;

        for (grip, x, y) in [
            (Grip::North, mid_x, near_y),
            (Grip::South, mid_x, far_y),
            (Grip::West, near_x, mid_y),
            (Grip::East, far_x, mid_y),
            (Grip::NorthWest, near_x, near_y),
            (Grip::NorthEast, far_x, near_y),
            (Grip::SouthWest, near_x, far_y),
            (Grip::SouthEast, far_x, far_y),
        ] {
            pane.update(cx, |pane, _| pane.dragging = None);
            cx.simulate_mouse_down(Point { x, y }, MouseButton::Left, Modifiers::default());
            pane.read_with(cx, |pane, _| {
                assert_eq!(
                    pane.dragging.map(|dragging| dragging.grab),
                    Some(Grab::Resize(grip)),
                    "a press at ({x:?}, {y:?}) must be {grip:?}"
                );
            });
        }
    }

    /// And the title bar still moves the window where no grip covers it.
    ///
    /// The other half: a fix that stopped the title bar everywhere would leave
    /// the window undraggable.
    #[gpui::test]
    async fn a_press_in_the_title_bar_still_moves(cx: &mut TestAppContext) {
        let (pane, cx) = a_painted_window(cx).await;
        let container = pane
            .read_with(cx, |pane, _| pane.last_container)
            .expect("the canvas measured the layer");
        let bounds = pane.read_with(cx, |pane, _| pane.bounds_within(container));

        // Well below the 6pt north grip, still inside the 34pt title bar.
        cx.simulate_mouse_down(
            Point {
                x: bounds.origin.x + bounds.size.width / 2.,
                y: bounds.origin.y + px(20.),
            },
            MouseButton::Left,
            Modifiers::default(),
        );

        pane.read_with(cx, |pane, _| {
            assert_eq!(
                pane.dragging.map(|dragging| dragging.grab),
                Some(Grab::Move),
                "the title bar is still how a window is moved"
            );
        });
    }
}

/// The geometry, which is where a floating window is either usable or a trap.
mod geometry {
    use super::*;

    const CONTAINER: Size<gpui::Pixels> = size(px(1200.), px(800.));

    /// A window is opened by the button, so it opens near the button.
    #[gpui::test]
    async fn it_opens_beside_the_button_that_opened_it(cx: &mut TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.read_with(cx, |window, _| {
            let bounds = window.bounds_within(CONTAINER);
            // The far edges, not the origin: a 720pt window in a 1200pt
            // container has its left edge left of centre however it is
            // anchored, so the origin says nothing about which corner it is in.
            let right_gap = CONTAINER.width - (bounds.origin.x + bounds.size.width);
            let bottom_gap = CONTAINER.height - (bounds.origin.y + bounds.size.height);
            assert!(
                right_gap >= px(0.) && right_gap < px(120.),
                "it must open in the corner the button is in, not the middle: \
                 {right_gap:?} from the right edge"
            );
            assert!(
                bottom_gap >= px(0.) && bottom_gap < px(120.),
                "{bottom_gap:?} from the bottom edge"
            );
            assert!(
                bottom_gap > px(0.),
                "and clear of the button, which sits in that same corner"
            );
        });
    }

    /// Dragged past an edge, enough stays reachable to drag it back.
    ///
    /// This is the whole reason the clamp exists: the strip that moves a window
    /// is at its top, so a window dragged fully off cannot be recovered by the
    /// means that put it there.
    #[gpui::test]
    async fn it_cannot_be_dragged_out_of_reach(cx: &mut TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.update(cx, |window, _| {
            window.move_to(
                Point {
                    x: px(9999.),
                    y: px(9999.),
                },
                CONTAINER,
            );
            // The *stored* position, not the rectangle read back. Reading through
            // `bounds_within` would clamp it a second time and pass whether or
            // not the move clamped -- and an unclamped stored position is the
            // real fault: the drag stops following until the pointer travels all
            // the way back to wherever it was allowed to reach.
            let stored = window.position.expect("a move stores a position");
            assert!(
                stored.x <= CONTAINER.width - MUST_STAY_VISIBLE,
                "some of the title bar must stay on screen to grab: {stored:?}"
            );
            assert!(
                stored.y <= CONTAINER.height - MUST_STAY_VISIBLE,
                "{stored:?}"
            );

            window.move_to(
                Point {
                    x: px(-9999.),
                    y: px(-9999.),
                },
                CONTAINER,
            );
            let stored = window.position.expect("a move stores a position");
            assert!(
                stored.y >= px(0.),
                "and the title bar cannot go above the top, where nothing can \
                 reach it at all: {stored:?}"
            );
        });
    }

    /// Pulling the bottom-right grows the window and leaves the top-left alone.
    #[gpui::test]
    async fn the_south_east_grip_holds_the_north_west(cx: &mut TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.update(cx, |window, _| {
            window.move_to(
                Point {
                    x: px(100.),
                    y: px(100.),
                },
                CONTAINER,
            );
            window.resize_to(
                Grip::SouthEast,
                Point {
                    x: px(700.),
                    y: px(500.),
                },
                CONTAINER,
            );
            let bounds = window.bounds_within(CONTAINER);
            assert_eq!(
                bounds.origin,
                Point {
                    x: px(100.),
                    y: px(100.)
                }
            );
            assert_eq!(bounds.size, size(px(600.), px(400.)));
        });
    }

    /// Pulling the top-left moves the origin and leaves the bottom-right alone.
    ///
    /// The opposite corner staying put is the entire difference between a resize
    /// and a move; get it wrong and the window slides away as it is resized.
    #[gpui::test]
    async fn the_north_west_grip_holds_the_south_east(cx: &mut TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.update(cx, |window, _| {
            window.move_to(
                Point {
                    x: px(100.),
                    y: px(100.),
                },
                CONTAINER,
            );
            window.resize_to(
                Grip::SouthEast,
                Point {
                    x: px(700.),
                    y: px(500.),
                },
                CONTAINER,
            );
            let before = window.bounds_within(CONTAINER);
            let far_corner = Point {
                x: before.origin.x + before.size.width,
                y: before.origin.y + before.size.height,
            };

            window.resize_to(
                Grip::NorthWest,
                Point {
                    x: px(200.),
                    y: px(180.),
                },
                CONTAINER,
            );
            let after = window.bounds_within(CONTAINER);
            assert_eq!(
                after.origin,
                Point {
                    x: px(200.),
                    y: px(180.)
                }
            );
            assert_eq!(
                Point {
                    x: after.origin.x + after.size.width,
                    y: after.origin.y + after.size.height,
                },
                far_corner,
                "the corner nobody grabbed must not have moved"
            );
        });
    }

    /// Neither grip can shrink the window past the point of being usable, and
    /// neither can turn it inside out by being pulled past the other corner.
    #[gpui::test]
    async fn a_grip_cannot_shrink_it_away_or_invert_it(cx: &mut TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.update(cx, |window, _| {
            window.move_to(
                Point {
                    x: px(100.),
                    y: px(100.),
                },
                CONTAINER,
            );

            window.resize_to(
                Grip::SouthEast,
                Point {
                    x: px(0.),
                    y: px(0.),
                },
                CONTAINER,
            );
            // The stored size, for the same reason the move test reads the
            // stored position: `bounds_within` floors at the minimum on the way
            // out, so reading through it passes even when nothing was floored on
            // the way in -- and a stored size of zero is what makes the next
            // drag of that corner jump.
            assert_eq!(window.size, SMALLEST, "pulled inwards past the minimum");

            window.resize_to(
                Grip::NorthWest,
                Point {
                    x: px(9999.),
                    y: px(9999.),
                },
                CONTAINER,
            );
            assert!(
                window.size.width >= SMALLEST.width && window.size.height >= SMALLEST.height,
                "pulled past the opposite corner: {:?}",
                window.size
            );
        });
    }

    /// Every one of the eight grips moves the sides it names and holds the rest.
    ///
    /// The table is the specification: `East` moves the right edge only,
    /// `NorthWest` moves the top and the left, and in every case the sides not
    /// named must not have shifted by a pixel. Getting one wrong makes that
    /// handle slide the window instead of resizing it -- a fault that is
    /// invisible until somebody grabs that particular edge.
    #[gpui::test]
    async fn each_grip_moves_the_sides_it_names_and_no_others(cx: &mut TestAppContext) {
        let (window, cx) = a_window(cx).await;
        // Left, top, right, bottom: which of the four a grip is allowed to move.
        for (grip, moves_left, moves_top, moves_right, moves_bottom) in [
            (Grip::North, false, true, false, false),
            (Grip::South, false, false, false, true),
            (Grip::West, true, false, false, false),
            (Grip::East, false, false, true, false),
            (Grip::NorthWest, true, true, false, false),
            (Grip::NorthEast, false, true, true, false),
            (Grip::SouthWest, true, false, false, true),
            (Grip::SouthEast, false, false, true, true),
        ] {
            window.update(cx, |window, _| {
                window.move_to(
                    Point {
                        x: px(300.),
                        y: px(200.),
                    },
                    CONTAINER,
                );
                window.resize_to(
                    Grip::SouthEast,
                    Point {
                        x: px(900.),
                        y: px(600.),
                    },
                    CONTAINER,
                );
                let before = window.bounds_within(CONTAINER);

                // Pulled inwards on both axes, so a grip that wrongly moves an
                // edge it should not moves it somewhere clearly different.
                window.resize_to(
                    grip,
                    Point {
                        x: px(400.),
                        y: px(300.),
                    },
                    CONTAINER,
                );
                let after = window.bounds_within(CONTAINER);

                let moved = |a: gpui::Pixels, b: gpui::Pixels| a != b;
                assert_eq!(
                    moved(before.origin.x, after.origin.x),
                    moves_left,
                    "{grip:?}: the left edge"
                );
                assert_eq!(
                    moved(before.origin.y, after.origin.y),
                    moves_top,
                    "{grip:?}: the top edge"
                );
                assert_eq!(
                    moved(
                        before.origin.x + before.size.width,
                        after.origin.x + after.size.width
                    ),
                    moves_right,
                    "{grip:?}: the right edge"
                );
                assert_eq!(
                    moved(
                        before.origin.y + before.size.height,
                        after.origin.y + after.size.height
                    ),
                    moves_bottom,
                    "{grip:?}: the bottom edge"
                );
            });
        }
    }

    /// A container smaller than the window still produces a drawable rectangle.
    ///
    /// A very short editor window is the case that would otherwise hand the
    /// layout a negative size.
    #[gpui::test]
    async fn a_container_smaller_than_the_window_still_works(cx: &mut TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.read_with(cx, |window, _| {
            let cramped = size(px(200.), px(120.));
            let bounds = window.bounds_within(cramped);
            assert!(bounds.size.width >= SMALLEST.width);
            assert!(bounds.size.height >= SMALLEST.height);
            assert!(bounds.origin.x >= px(0.) || bounds.size.width > cramped.width);
            assert!(bounds.origin.y >= px(0.));
        });
    }

    /// A resized editor window still reaches the layer's measurement.
    ///
    /// The measurement is skipped on the frames that report the size already
    /// held, which is nearly all of them. This pins the frames that do not: a
    /// guard that also swallowed a real change would leave the window laying
    /// itself out inside a container that no longer exists, and the only symptom
    /// would be a window stranded off the edge of a shrunken editor.
    #[gpui::test]
    async fn a_resized_container_still_reaches_the_window(cx: &mut TestAppContext) {
        let (window, cx) = a_painted_window(cx).await;
        let settled = window
            .read_with(cx, |window, _| window.last_container)
            .expect("the first frame measures the layer");

        let resized = size(settled.width - px(240.), settled.height - px(160.));
        cx.simulate_resize(resized);
        cx.run_until_parked();

        assert_eq!(
            window.read_with(cx, |window, _| window.last_container),
            Some(resized),
            "a container that actually changed must still be recorded"
        );
    }
}

mod opening {
    use super::*;

    /// A freshly opened window shows the menu, and starts nothing.
    ///
    /// Opening straight onto a terminal decided for somebody what they came
    /// for -- and started a shell process to do it, which is the cost this
    /// answers.
    #[gpui::test]
    async fn it_opens_onto_the_menu_and_starts_nothing(cx: &mut TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            assert!(window.open);
        });
        cx.run_until_parked();

        window.read_with(cx, |window, cx| {
            assert!(
                window.is_empty(cx),
                "nothing may be started until something is chosen"
            );
        });
    }

    /// The button is a toggle, and putting the window away must not empty it.
    ///
    /// Closing would end the terminal and the agent thread inside. That is the
    /// same complaint the rail buttons already answered: a lit toggle that
    /// destroys what it opened is not a toggle.
    #[gpui::test]
    async fn putting_it_away_keeps_what_is_inside(cx: &mut TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        let tabs = window.read_with(cx, |window, cx| window.active_pane.read(cx).items_len());
        assert_eq!(tabs, 1, "the note opened");

        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            assert!(!window.open, "put away");
        });
        cx.run_until_parked();

        window.read_with(cx, |window, cx| {
            assert_eq!(
                window.active_pane.read(cx).items_len(),
                tabs,
                "and its tabs are still there"
            );
        });
    }

    /// Closing is the other one: it ends what minimising keeps.
    ///
    /// The whole reason the second button exists -- the first frees no memory.
    #[gpui::test]
    async fn closing_ends_what_minimising_keeps(cx: &mut TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();
        window.read_with(cx, |window, cx| {
            assert_eq!(window.active_pane.read(cx).items_len(), 1);
        });

        window.update_in(cx, |window, window_handle, cx| {
            window.shut_down(window_handle, cx)
        });
        cx.run_until_parked();

        window.read_with(cx, |window, cx| {
            assert!(!window.open, "and it is put away too");
            assert_eq!(
                window.active_pane.read(cx).items_len(),
                0,
                "nothing left running: that is the point of the button"
            );
        });
    }
}

/// Everything the window is asked to hold has to land in it -- the second
/// time as well as the first.
///
/// Reported: the first choice opens in the window, and every one after it
/// opens in the editor instead.
#[cfg(test)]
mod repeated_opens {
    use super::a_window;
    use gpui::TestAppContext;

    #[gpui::test]
    async fn a_second_note_joins_the_first(cx: &mut TestAppContext) {
        let (window, cx) = a_window(cx).await;

        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();
        window.read_with(cx, |window, cx| {
            assert_eq!(
                window.active_pane.read(cx).items_len(),
                1,
                "the first note opened"
            );
        });

        window.update_in(cx, |window, window_handle, cx| {
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        window.read_with(cx, |window, cx| {
            assert_eq!(
                window.active_pane.read(cx).items_len(),
                2,
                "the second note must join the window, not go to the editor"
            );
        });
    }

    /// The `+` at the end of the tab bar has to be the window's own.
    ///
    /// This is the reported bug. A `Pane` draws a `+` by default, and the
    /// default offers New File, New Terminal and the agents as *workspace*
    /// actions -- which resolve against the editor's active pane. This pane is
    /// not one of those, so every entry on it opened behind the window.
    ///
    /// It only showed up after the first tab, because until then the empty
    /// state covers the pane and that list was always correct. Hence "the first
    /// one works, the rest go to the IDE".
    #[gpui::test]
    async fn the_tab_bar_offers_this_window_and_not_the_editor(cx: &mut TestAppContext) {
        use gpui::Modifiers;

        let (window, cx) = super::a_painted_window(cx).await;

        window.update_in(cx, |window, window_handle, cx| {
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();
        cx.update(|_window, _cx| {});
        cx.run_until_parked();

        let plus = cx
            .debug_bounds("floating-pane-tab-bar-add")
            .expect("a tab bar with a tab in it must draw its own new-item button");
        cx.simulate_click(plus.center(), Modifiers::default());
        cx.run_until_parked();

        assert!(
            cx.debug_bounds("MENU_ITEM-New Markdown Note").is_some(),
            "the tab bar must offer what this window can hold"
        );
        assert!(
            cx.debug_bounds("MENU_ITEM-New File").is_none(),
            "New File is a workspace action: it opens in the editor behind this window"
        );
        assert!(
            cx.debug_bounds("MENU_ITEM-New Center Terminal").is_none(),
            "and so is New Center Terminal"
        );
    }

    /// The agent path has the most indirection of the three: it defers through
    /// the workspace and comes back to add the tab, so it is the one that could
    /// lose track of where it was told to put it.
    #[gpui::test]
    async fn agents_land_in_the_window_both_times(cx: &mut TestAppContext) {
        let (window, cx) = a_window(cx).await;

        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            window.open_agent(project::CLAUDE_CODE_AGENT_ID, window_handle, cx);
        });
        cx.run_until_parked();
        window.read_with(cx, |window, cx| {
            assert_eq!(
                window.active_pane.read(cx).items_len(),
                1,
                "the first agent opened"
            );
        });

        window.update_in(cx, |window, window_handle, cx| {
            window.open_agent(project::CODEX_AGENT_ID, window_handle, cx);
        });
        cx.run_until_parked();

        let in_editor = window.read_with(cx, |window, cx| {
            window
                .workspace
                .read_with(cx, |workspace, cx| {
                    workspace
                        .panes()
                        .iter()
                        .map(|pane| pane.read(cx).items_len())
                        .sum::<usize>()
                })
                .unwrap()
        });
        window.read_with(cx, |window, cx| {
            assert_eq!(
                window.active_pane.read(cx).items_len(),
                2,
                "the second agent must join the window"
            );
        });
        assert_eq!(in_editor, 0, "and the editor must have collected neither");
    }

    /// And the editor must not have collected any of them.
    #[gpui::test]
    async fn the_editor_collects_none_of_them(cx: &mut TestAppContext) {
        let (window, cx) = a_window(cx).await;

        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();
        window.update_in(cx, |window, window_handle, cx| {
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        let in_editor = window.read_with(cx, |window, cx| {
            window
                .workspace
                .read_with(cx, |workspace, cx| {
                    workspace
                        .panes()
                        .iter()
                        .map(|pane| pane.read(cx).items_len())
                        .sum::<usize>()
                })
                .unwrap()
        });
        assert_eq!(in_editor, 0, "nothing chosen here belongs to the editor");
    }
}

/// The one-pane shape a window opens with.
///
/// A fresh window holds a `PaneGroup` of exactly one pane. These pin that
/// shape, so a change that grows the group has something to prove itself
/// against.
mod splitting {
    use super::a_window;
    use gpui::{AppContext as _, Focusable as _, VisualContext as _};

    /// A fresh window holds one pane, and that pane is the active one.
    #[gpui::test]
    async fn one_pane_until_something_splits_it(cx: &mut gpui::TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.read_with(cx, |window, _| {
            assert_eq!(
                window.center.panes().len(),
                1,
                "nothing has split the group yet"
            );
            assert_eq!(
                window.center.panes(),
                vec![&window.active_pane],
                "the one pane in the group is the active one"
            );
        });
    }

    /// The window is the menu until something is opened, and stays that way
    /// only for as long as every pane in the group is empty.
    #[gpui::test]
    async fn an_empty_window_is_still_the_menu(cx: &mut gpui::TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.read_with(cx, |window, cx| {
            assert!(window.is_empty(cx), "nothing has been opened yet");
        });

        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        window.read_with(cx, |window, cx| {
            assert!(!window.is_empty(cx), "a note is open in the one pane");
        });
    }

    /// Closing the last tab brings the menu back, now asserted through the
    /// group-totalled `is_empty` rather than the single pane's item count.
    #[gpui::test]
    async fn closing_the_last_tab_brings_the_menu_back(cx: &mut gpui::TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();
        window.read_with(cx, |window, cx| {
            assert!(!window.is_empty(cx), "the note opened");
        });

        window.update_in(cx, |window, window_handle, cx| {
            window.shut_down(window_handle, cx);
        });
        cx.run_until_parked();

        window.read_with(cx, |window, cx| {
            assert!(
                window.is_empty(cx),
                "closing the only tab empties the only pane"
            );
        });
    }

    /// `Event::Split` with `MovePane` takes the active item into a fresh pane,
    /// which the group must now hold as a second member -- and that new pane
    /// becomes the active one.
    #[gpui::test]
    async fn a_split_makes_a_second_pane(cx: &mut gpui::TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();
        window.update_in(cx, |window, window_handle, cx| {
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        let first_pane = window.read_with(cx, |window, _| window.active_pane.clone());
        first_pane.update(cx, |_pane, cx| {
            cx.emit(workspace::pane::Event::Split {
                direction: workspace::SplitDirection::Right,
                mode: workspace::SplitMode::MovePane,
            });
        });
        cx.run_until_parked();

        window.read_with(cx, |window, _| {
            assert_eq!(
                window.center.panes().len(),
                2,
                "the split must add a pane to the group"
            );
            assert!(
                window.active_pane != first_pane,
                "the new pane becomes the active one"
            );
        });
    }

    /// Closing the only item a split pane holds must collapse that pane back
    /// out of the group, rather than leaving an empty pane sitting in it.
    #[gpui::test]
    async fn closing_the_last_item_in_a_split_pane_collapses_it(cx: &mut gpui::TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();
        window.update_in(cx, |window, window_handle, cx| {
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        let first_pane = window.read_with(cx, |window, _| window.active_pane.clone());
        first_pane.update(cx, |_pane, cx| {
            cx.emit(workspace::pane::Event::Split {
                direction: workspace::SplitDirection::Right,
                mode: workspace::SplitMode::MovePane,
            });
        });
        cx.run_until_parked();
        let new_pane = window.read_with(cx, |window, _| window.active_pane.clone());
        window.read_with(cx, |window, _| {
            assert_eq!(window.center.panes().len(), 2, "the split must have landed");
        });

        // Held rather than dropped: a `Task` dropped before it is polled
        // cancels its work, and closing an item is genuinely asynchronous.
        let closing = window.update_in(cx, |_window, window_handle, cx| {
            new_pane.update(cx, |pane, cx| {
                pane.close_all_items(
                    &workspace::pane::CloseAllItems::default(),
                    window_handle,
                    cx,
                )
            })
        });
        cx.run_until_parked();
        drop(closing);

        window.read_with(cx, |window, _| {
            assert_eq!(
                window.center.panes().len(),
                1,
                "an emptied split pane must collapse back out of the group"
            );
        });
    }

    /// The last pane in the group is never removed: closing its last item
    /// must leave the group with exactly that one pane, and let the window's
    /// menu take over as its body instead of shrinking the group to nothing.
    #[gpui::test]
    async fn the_last_pane_survives_an_empty_window(cx: &mut gpui::TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        window.update_in(cx, |window, window_handle, cx| {
            window.shut_down(window_handle, cx);
        });
        cx.run_until_parked();

        window.read_with(cx, |window, cx| {
            assert_eq!(
                window.center.panes().len(),
                1,
                "the last pane must survive an emptied window"
            );
            assert!(window.is_empty(cx), "and the menu must take over its body");
        });
    }

    /// `MovePane` on a pane holding only one item would strand that item
    /// alone in a new pane and leave the source with nothing -- the split
    /// would collapse the instant it was made. It falls back to opening a
    /// terminal in the new pane instead, so the split actually holds.
    #[gpui::test]
    async fn moving_the_only_tab_does_not_split_into_nothing(cx: &mut gpui::TestAppContext) {
        // The fallback opens a real shell, whose PTY reader thread the test
        // scheduler would otherwise report as non-deterministic activity.
        cx.executor().allow_parking();
        let (window, cx) = a_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        let first_pane = window.read_with(cx, |window, cx| {
            assert_eq!(
                window.active_pane.read(cx).items_len(),
                1,
                "the pane being split holds exactly one tab"
            );
            window.active_pane.clone()
        });
        first_pane.update(cx, |_pane, cx| {
            cx.emit(workspace::pane::Event::Split {
                direction: workspace::SplitDirection::Right,
                mode: workspace::SplitMode::MovePane,
            });
        });
        cx.run_until_parked();

        window.read_with(cx, |window, cx| {
            assert_eq!(
                window.center.panes().len(),
                2,
                "the fallback still produces two panes"
            );
            assert_eq!(
                first_pane.read(cx).items_len(),
                1,
                "the source pane keeps its only tab"
            );
        });
    }

    /// `ZoomIn` must fill the window with the zoomed pane on its own -- and
    /// `PaneGroup::render` draws a zoomed pane as an empty `div`, so a frame
    /// painted after `ZoomIn` still has to draw *something*, or the window has
    /// silently gone blank. `ZoomOut` must put the group back.
    #[gpui::test]
    async fn zoom_fills_the_floating_window(cx: &mut gpui::TestAppContext) {
        let (window, cx) = super::a_painted_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        let pane = window.read_with(cx, |window, _| window.active_pane.clone());
        pane.update(cx, |_pane, cx| {
            cx.emit(workspace::pane::Event::ZoomIn);
        });
        cx.run_until_parked();
        cx.update(|_window, _cx| {});
        cx.run_until_parked();

        window.read_with(cx, |window, cx| {
            assert!(
                window.zoomed.is_some(),
                "ZoomIn must record the zoomed pane"
            );
            assert!(
                pane.read(cx).is_zoomed(),
                "and mark the pane itself as zoomed"
            );
        });

        pane.update(cx, |_pane, cx| {
            cx.emit(workspace::pane::Event::ZoomOut);
        });
        cx.run_until_parked();

        window.read_with(cx, |window, cx| {
            assert!(window.zoomed.is_none(), "ZoomOut must clear it");
            assert!(!pane.read(cx).is_zoomed(), "and un-zoom the pane");
        });
    }

    /// Every live pane keeps exactly one subscription -- a collapse must drop
    /// the removed pane's subscription in the same step, or the map grows
    /// forever as panes split and close.
    #[gpui::test]
    async fn a_removed_pane_drops_its_subscription(cx: &mut gpui::TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();
        window.update_in(cx, |window, window_handle, cx| {
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        let first_pane = window.read_with(cx, |window, _| window.active_pane.clone());
        first_pane.update(cx, |_pane, cx| {
            cx.emit(workspace::pane::Event::Split {
                direction: workspace::SplitDirection::Right,
                mode: workspace::SplitMode::MovePane,
            });
        });
        cx.run_until_parked();
        let new_pane = window.read_with(cx, |window, _| window.active_pane.clone());

        window.read_with(cx, |window, _| {
            assert_eq!(
                window.pane_subscriptions.len(),
                window.center.panes().len(),
                "one subscription per pane after the split"
            );
        });

        // Held rather than dropped: a `Task` dropped before it is polled
        // cancels its work, and closing an item is genuinely asynchronous.
        let closing = window.update_in(cx, |_window, window_handle, cx| {
            new_pane.update(cx, |pane, cx| {
                pane.close_all_items(
                    &workspace::pane::CloseAllItems::default(),
                    window_handle,
                    cx,
                )
            })
        });
        cx.run_until_parked();
        drop(closing);

        window.read_with(cx, |window, _| {
            assert_eq!(
                window.pane_subscriptions.len(),
                window.center.panes().len(),
                "the collapsed pane's subscription must go with it"
            );
        });
    }

    /// The tab bar's left slot has to carry the window's own Split control,
    /// not the empty slot the pane's default buttons were replaced with.
    #[gpui::test]
    async fn the_tab_bar_offers_a_split(cx: &mut gpui::TestAppContext) {
        let (window, cx) = super::a_painted_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();
        cx.update(|_window, _cx| {});
        cx.run_until_parked();

        assert!(
            cx.debug_bounds("floating-pane-tab-bar-split").is_some(),
            "a tab bar with a tab in it must draw the window's own Split button"
        );
    }

    /// The `+` menu's Split submenu has to offer the same four directions the
    /// tab-bar button does.
    ///
    /// The submenu row draws no debug selector of its own -- unlike a flat
    /// entry, it opens on hover rather than carrying a fixed identity -- so
    /// it is found by walking the pointer down from the last labelled row
    /// until hovering lands on it and its own entries (which do carry a
    /// selector) appear.
    #[gpui::test]
    async fn the_plus_menu_offers_a_split_submenu(cx: &mut gpui::TestAppContext) {
        use gpui::Modifiers;

        let (window, cx) = super::a_painted_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();
        cx.update(|_window, _cx| {});
        cx.run_until_parked();

        let plus = cx
            .debug_bounds("floating-pane-tab-bar-add")
            .expect("a tab bar with a tab in it must draw its own new-item button");
        cx.simulate_click(plus.center(), Modifiers::default());
        cx.run_until_parked();

        let last_fixed_entry = cx
            .debug_bounds("MENU_ITEM-Open Markdown Note")
            .expect("the menu must still offer what it always has");

        let mut found = false;
        let x = last_fixed_entry.center().x;
        let mut y = last_fixed_entry.bottom() + gpui::px(4.);
        for _ in 0..60 {
            cx.simulate_mouse_move(gpui::Point { x, y }, None, Modifiers::default());
            cx.run_until_parked();
            if cx.debug_bounds("MENU_ITEM-Split Right").is_some() {
                found = true;
                break;
            }
            y += gpui::px(4.);
        }

        assert!(
            found,
            "the `+` menu must offer a Split submenu with all four directions"
        );
    }

    /// `split_active` is what every one of `floating_pane::SplitRight`'s
    /// three siblings and the two menu routes delegate to (see
    /// `render.rs::split_entries`) -- exercised directly here for the same
    /// reason none of its five siblings above dispatch their action through
    /// a real keymap either: `workspace.register_action` wires a name to a
    /// method, and the method is where the behaviour actually lives.
    /// `center.panes()` must grow by one, and nothing about `self.workspace`
    /// is touched in doing it.
    #[gpui::test]
    async fn the_action_splits_the_active_pane(cx: &mut gpui::TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        window.update_in(cx, |window, window_handle, cx| {
            window.split_active(workspace::SplitDirection::Right, window_handle, cx);
        });
        cx.run_until_parked();

        window.read_with(cx, |window, _| {
            assert_eq!(
                window.center.panes().len(),
                2,
                "the action must split the floating window's group"
            );
        });
    }

    /// Splitting a window with nothing in any pane would leave the user
    /// staring at two blank halves instead of the menu that tells them what
    /// the window can hold, so the guard must refuse.
    #[gpui::test]
    async fn splitting_an_empty_window_does_nothing(cx: &mut gpui::TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
        });
        cx.run_until_parked();

        window.update_in(cx, |window, window_handle, cx| {
            window.split_active(workspace::SplitDirection::Right, window_handle, cx);
        });
        cx.run_until_parked();

        window.read_with(cx, |window, cx| {
            assert_eq!(
                window.center.panes().len(),
                1,
                "an empty window must not be split"
            );
            assert!(window.is_empty(cx), "and must still show as empty");
        });
    }

    /// A closed window has no visible pane worth splitting; the action must
    /// be a no-op rather than mutate a window nobody can see.
    #[gpui::test]
    async fn splitting_a_closed_window_does_nothing(cx: &mut gpui::TestAppContext) {
        let (window, cx) = a_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            window.new_markdown_note(window_handle, cx);
            // Put away again: the window now holds a tab but is closed.
            window.toggle(window_handle, cx);
        });
        cx.run_until_parked();

        window.update_in(cx, |window, window_handle, cx| {
            window.split_active(workspace::SplitDirection::Right, window_handle, cx);
        });
        cx.run_until_parked();

        window.read_with(cx, |window, _| {
            assert_eq!(
                window.center.panes().len(),
                1,
                "a closed window must not be split"
            );
        });
    }

    /// `find_pane_in_direction` reads real geometry, so this needs the window
    /// actually painted -- an unpainted group has no bounding boxes to find a
    /// direction from.
    #[gpui::test]
    async fn activating_in_a_direction_moves_focus(cx: &mut gpui::TestAppContext) {
        let (window, cx) = super::a_painted_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        let left_pane = window.read_with(cx, |window, _| window.active_pane.clone());
        window.update_in(cx, |window, window_handle, cx| {
            window.split_off(
                &left_pane.clone(),
                workspace::SplitDirection::Right,
                window_handle,
                cx,
            );
        });
        // A second paint: the split just changed the geometry the first
        // frame measured, and `find_pane_in_direction` below needs the
        // up-to-date bounding boxes, not the ones from before the split.
        cx.run_until_parked();
        cx.update(|_window, _cx| {});
        cx.run_until_parked();

        let right_pane = window.read_with(cx, |window, _| window.active_pane.clone());
        assert_ne!(left_pane, right_pane, "the split must have a new active pane");

        // Asserted against the window's real focus rather than the
        // `active_pane` field: `Pane::focus_in` only re-emits `Event::Focus`
        // once per focus/blur cycle (guarded by its own `was_focused`), which
        // depends on paint-driven dispatch-tree bookkeeping this test does
        // not otherwise need. `activate_in`'s actual contract is "moves
        // window focus", so that is what gets checked.
        window.update_in(cx, |_window, window_handle, cx| {
            assert!(
                right_pane.focus_handle(cx).is_focused(window_handle),
                "the split must have left focus in the new pane"
            );
        });

        window.update_in(cx, |window, window_handle, cx| {
            window.activate_in(workspace::SplitDirection::Left, window_handle, cx);
            assert!(
                left_pane.focus_handle(cx).is_focused(window_handle),
                "ActivatePaneLeft must move window focus to the pane on that side"
            );
        });
    }

    /// The leftmost pane has nothing further left: the chord must be a no-op
    /// rather than walking out into the editor behind the window.
    #[gpui::test]
    async fn activating_past_the_edge_stays_inside(cx: &mut gpui::TestAppContext) {
        let (window, cx) = super::a_painted_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        let only_pane = window.read_with(cx, |window, _| window.active_pane.clone());

        window.update_in(cx, |window, window_handle, cx| {
            window.activate_in(workspace::SplitDirection::Left, window_handle, cx);
        });
        cx.run_until_parked();

        window.read_with(cx, |window, _| {
            assert_eq!(
                window.active_pane, only_pane,
                "a group with nothing further left must leave focus exactly where it was"
            );
        });
    }

    /// `swap_in` must exchange the two panes' places in the group, not just
    /// move focus between them.
    #[gpui::test]
    async fn swapping_exchanges_two_panes(cx: &mut gpui::TestAppContext) {
        let (window, cx) = super::a_painted_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        let left_pane = window.read_with(cx, |window, _| window.active_pane.clone());
        window.update_in(cx, |window, window_handle, cx| {
            window.split_off(
                &left_pane.clone(),
                workspace::SplitDirection::Right,
                window_handle,
                cx,
            );
        });
        cx.run_until_parked();
        cx.update(|_window, _cx| {});
        cx.run_until_parked();

        let before = window.read_with(cx, |window, _| {
            window.center.panes().into_iter().cloned().collect::<Vec<_>>()
        });

        window.update_in(cx, |window, _window_handle, cx| {
            window.swap_in(workspace::SplitDirection::Left, cx);
        });
        cx.run_until_parked();

        let after = window.read_with(cx, |window, _| {
            window.center.panes().into_iter().cloned().collect::<Vec<_>>()
        });
        assert_eq!(
            after,
            before.into_iter().rev().collect::<Vec<_>>(),
            "swapping the two panes must reverse their order in the group"
        );
    }

    /// A single-pane group has no border to move to: `move_to_border` returns
    /// `Ok(false)`, and the guard around it must not log that as an error.
    #[gpui::test]
    async fn moving_to_a_border_does_not_panic_on_a_single_pane(cx: &mut gpui::TestAppContext) {
        let (window, cx) = super::a_painted_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        window.update_in(cx, |window, _window_handle, cx| {
            window.move_active_to_border(workspace::SplitDirection::Left, cx);
        });
        cx.run_until_parked();

        window.read_with(cx, |window, _| {
            assert_eq!(
                window.center.panes().len(),
                1,
                "a single pane must survive MovePaneLeft untouched"
            );
        });
    }

    /// The risk that matters most in this module: GPUI dispatches an action
    /// from the focused element outward, so the floating window's own
    /// `on_action` handlers must only fire while focus is inside it. Wired
    /// into a *real* workspace via `register_floating_layer`, the same way
    /// `floating_pane::init` wires the production one, so the two render
    /// trees are genuine siblings and dispatch has real ancestry to bubble
    /// through -- not two unrelated windows that could never prove this
    /// either way.
    #[gpui::test]
    async fn pane_navigation_stays_within_whichever_side_has_focus(cx: &mut gpui::TestAppContext) {
        super::init_test(cx);
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (multi_workspace, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
        let ws_project = workspace.read_with(cx, |workspace, _| workspace.project().clone());
        let handle = workspace.downgrade();

        let floating = cx.new_window_entity(|window, cx| {
            crate::host::FloatingPane::new(handle, ws_project, window, cx)
        });
        workspace.update(cx, |workspace, cx| {
            workspace.register_floating_layer(floating.clone(), cx);
        });
        floating.update_in(cx, |floating, window, cx| {
            floating.toggle(window, cx);
            floating.new_markdown_note(window, cx);
        });
        cx.run_until_parked();

        let floating_left = floating.read_with(cx, |floating, _| floating.active_pane.clone());
        floating.update_in(cx, |floating, window, cx| {
            floating.split_off(
                &floating_left.clone(),
                workspace::SplitDirection::Right,
                window,
                cx,
            );
        });
        cx.run_until_parked();
        cx.update(|_window, _cx| {});
        cx.run_until_parked();
        let floating_right = floating.read_with(cx, |floating, _| floating.active_pane.clone());
        assert_ne!(floating_left, floating_right);

        // `Workspace::activate_pane_in_direction` focuses the *active item*
        // of the pane it lands on, not the pane itself -- an empty pane has
        // nothing for it to focus, and this test would pass for the wrong
        // reason (nothing to focus, so nothing happened) instead of proving
        // it actually moved between real editor panes.
        let editor_left = workspace.read_with(cx, |workspace, _| workspace.active_pane().clone());
        workspace.update_in(cx, |workspace, window, cx| {
            let item = cx.new(|cx| workspace::item::test::TestItem::new(cx));
            workspace.add_item_to_active_pane(Box::new(item), None, false, window, cx);
        });
        let editor_right = workspace.update_in(cx, |workspace, window, cx| {
            let new_pane = workspace.split_pane(
                editor_left.clone(),
                workspace::SplitDirection::Right,
                window,
                cx,
            );
            let item = cx.new(|cx| workspace::item::test::TestItem::new(cx));
            new_pane.update(cx, |pane, cx| {
                pane.add_item(Box::new(item), true, true, None, window, cx);
            });
            new_pane
        });
        cx.run_until_parked();
        cx.update(|_window, _cx| {});
        cx.run_until_parked();

        // Focus in the editor's own right pane: the chord must move real
        // window focus to the editor's left pane. If the floating window's
        // handler ran instead, focus would land on `floating_left` (its own
        // group has nowhere else to go from `floating_right`) rather than
        // `editor_left` -- the two are different entities, so this
        // distinguishes "the right handler ran" from "the wrong one did".
        // Checked against real focus rather than the `active_pane` fields:
        // those only update through `Pane::focus_in`'s own once-per-cycle
        // `Event::Focus`, which is a separate mechanism from what this test
        // is proving.
        workspace.update_in(cx, |_workspace, window, cx| {
            window.focus(&editor_right.focus_handle(cx), cx);
        });
        // A real paint: `Workspace::activate_pane_in_direction` reads its own
        // `active_pane` field, which only follows real focus once a draw has
        // compared the previous and current focus paths and fired
        // `Pane::focus_in`. Without it, the workspace would still think
        // `editor_left` was active and the chord would (correctly) do
        // nothing -- which would make this test pass for the wrong reason.
        cx.run_until_parked();
        cx.update(|_window, _cx| {});
        cx.run_until_parked();
        cx.dispatch_action(workspace::ActivatePaneLeft);
        cx.run_until_parked();

        workspace.update_in(cx, |_workspace, window, cx| {
            // `contains_focused`, not `is_focused`: the workspace's own
            // navigation focuses the pane's active *item*, not the pane's
            // own root handle.
            assert!(
                editor_left.focus_handle(cx).contains_focused(window, cx),
                "with focus in the editor, ActivatePaneLeft must still move the workspace's own focus"
            );
        });

        // Focus moves into the floating window's own right pane: the same
        // chord must now move focus only within it. If the workspace's own
        // handler ran instead, focus would stay on `floating_right` (the
        // workspace's active pane is already leftmost, so its handler is a
        // no-op) rather than moving to `floating_left`.
        workspace.update_in(cx, |_workspace, window, cx| {
            window.focus(&floating_right.focus_handle(cx), cx);
        });
        cx.run_until_parked();
        cx.dispatch_action(workspace::ActivatePaneLeft);
        cx.run_until_parked();

        workspace.update_in(cx, |_workspace, window, cx| {
            // `contains_focused` again: `floating_left` holds the markdown
            // note from setup, and `Pane::focus_in` redirects a direct pane
            // focus onto its active item.
            assert!(
                floating_left.focus_handle(cx).contains_focused(window, cx),
                "with focus in the floating window, ActivatePaneLeft must move focus within it"
            );
        });
    }
}

/// `Pane::split_for_drop` had zero callers anywhere in the repo before this
/// module -- the hook had never run, in production or in a test. Every test
/// here stages a real drop rather than calling the closure directly, so a
/// lease violation on `self.workspace` would show up as a failing test
/// instead of a crash nobody could reproduce.
mod dropping {
    use gpui::{AppContext as _, VisualContext as _};
    use workspace::DraggedTab;

    /// The gate in front of everything else in this module.
    ///
    /// `Pane::handle_drag_move` records no split direction unless
    /// `can_split_predicate` says the edge is a target, and it answers `false`
    /// when that predicate is unset. A pane without one therefore accepts an
    /// edge drop as an ordinary tab move and never splits -- silently, with
    /// the split hook wired correctly and never reached. This window shipped
    /// in exactly that state, because the drop tests all wrote
    /// `drag_split_direction` themselves and so began on the far side of it.
    #[gpui::test]
    async fn the_edge_is_a_split_target_at_all(cx: &mut gpui::TestAppContext) {
        let (window, cx) = super::a_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        let pane = window.read_with(cx, |window, _| window.active_pane.clone());
        let predicate = pane
            .read_with(cx, |pane, _| pane.can_split_predicate())
            .expect("a pane whose edges cannot be split targets can never be split by a drop");

        let from_here = DraggedTab {
            pane: pane.clone(),
            item: pane
                .read_with(cx, |pane, _| pane.active_item())
                .expect("the note just opened is this pane's active item"),
            ix: 0,
            detail: 0,
            is_active: true,
        };

        // This pane's only tab, dropped on this pane's own edge: refused,
        // because the source empties and the collapse rule undoes the split
        // the instant it is made.
        let allowed = pane.update_in(cx, |pane, window, cx| {
            predicate(pane, &from_here as &dyn std::any::Any, window, cx)
        });
        assert!(
            !allowed,
            "a pane's only tab dropped on its own edge must not be treated as a split"
        );

        // The same pane once it holds two: now there is something left behind,
        // so the split holds and the drag is allowed.
        window.update_in(cx, |window, window_handle, cx| {
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();
        let allowed = pane.update_in(cx, |pane, window, cx| {
            predicate(pane, &from_here as &dyn std::any::Any, window, cx)
        });
        assert!(
            allowed,
            "a pane holding more than one tab must accept an edge drop as a split"
        );
    }

    /// The headline: a tab dropped on a floating pane's edge must split the
    /// *floating* group, and the workspace's own centre group -- the one
    /// `Workspace::split_pane` reaches for without this hook -- must stay
    /// exactly as it was. A test that only counted the floating window's
    /// panes would pass even if the drop had silently rearranged the editor
    /// behind it.
    #[gpui::test]
    async fn a_tab_dropped_on_an_edge_splits_the_floating_group_not_the_editor(
        cx: &mut gpui::TestAppContext,
    ) {
        super::init_test(cx);
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (multi_workspace, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
        let ws_project = workspace.read_with(cx, |workspace, _| workspace.project().clone());
        let handle = workspace.downgrade();

        let floating = cx.new_window_entity(|window, cx| {
            crate::host::FloatingPane::new(handle, ws_project, window, cx)
        });
        workspace.update(cx, |workspace, cx| {
            workspace.register_floating_layer(floating.clone(), cx);
        });
        floating.update_in(cx, |floating, window, cx| {
            floating.toggle(window, cx);
            floating.new_markdown_note(window, cx);
        });
        cx.run_until_parked();

        // Dragged from the editor's own pane, not from a tab already inside
        // the floating window: dragging the floating window's only tab onto
        // its own edge would empty that pane out from under itself, and the
        // pane's own "last item closed" collapse would immediately undo the
        // split -- proving nothing about the hook this test exists for.
        let editor_pane = workspace.read_with(cx, |workspace, _| workspace.active_pane().clone());
        workspace.update_in(cx, |workspace, window, cx| {
            let item = cx.new(|cx| workspace::item::test::TestItem::new(cx));
            workspace.add_item_to_active_pane(Box::new(item), None, false, window, cx);
        });
        cx.run_until_parked();
        let item = editor_pane
            .read_with(cx, |pane, _| pane.active_item())
            .expect("the item just added must be the editor pane's active item");

        let floating_pane = floating.read_with(cx, |floating, _| floating.active_pane.clone());

        // The gate, asserted on the path a person actually takes: a tab from
        // the editor, hovered over this window's edge. Without it the drop
        // below is staged on a direction real hover never records.
        let predicate = floating_pane
            .read_with(cx, |pane, _| pane.can_split_predicate())
            .expect("the floating pane must mark its edges as split targets");
        let from_editor = DraggedTab {
            pane: editor_pane.clone(),
            item: item.clone(),
            ix: 0,
            detail: 0,
            is_active: true,
        };
        assert!(
            floating_pane.update_in(cx, |pane, window, cx| {
                predicate(pane, &from_editor as &dyn std::any::Any, window, cx)
            }),
            "a tab dragged in from the editor must be allowed to split this window"
        );
        // Written directly because `Pane::handle_drag_move`, which is what
        // fills this in from a real pointer, is private to `workspace`. That
        // leaves the gate in front of it untested here -- and that gate was
        // shut for the life of this feature. `the_edge_is_a_split_target_at_all`
        // below covers it; this test starts one step past it.
        floating_pane.update(cx, |pane, _| {
            pane.drag_split_direction = Some(workspace::SplitDirection::Right);
        });

        let dragged_tab = DraggedTab {
            pane: editor_pane.clone(),
            item,
            ix: 0,
            detail: 0,
            is_active: true,
        };
        floating_pane.update_in(cx, |pane, window, cx| {
            pane.handle_tab_drop(&dragged_tab, 0, false, window, cx);
        });
        cx.run_until_parked();
        cx.update(|_window, _cx| {});
        cx.run_until_parked();

        floating.read_with(cx, |floating, _| {
            assert_eq!(
                floating.center.panes().len(),
                2,
                "the drop must split the floating window's own group"
            );
        });
        workspace.read_with(cx, |workspace, _| {
            assert_eq!(
                workspace.panes().len(),
                1,
                "the editor's centre group must be untouched by a drop on the floating window"
            );
        });
    }

    /// Set in `content.rs::build_pane`, so every pane the window builds --
    /// including one created by an earlier drop -- carries the hook. Setting
    /// it only on the pane the window started with would make the routing
    /// bug reappear the moment somebody dropped a tab on a split pane.
    #[gpui::test]
    async fn every_pane_the_window_builds_carries_the_hook(cx: &mut gpui::TestAppContext) {
        let (window, cx) = super::a_window(cx).await;
        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            window.new_markdown_note(window_handle, cx);
        });
        cx.run_until_parked();

        let first_pane = window.read_with(cx, |window, _| window.active_pane.clone());
        window.update_in(cx, |window, window_handle, cx| {
            window.split_off(
                &first_pane.clone(),
                workspace::SplitDirection::Right,
                window_handle,
                cx,
            );
        });
        cx.run_until_parked();

        window.read_with(cx, |window, cx| {
            for pane in window.center.panes() {
                assert!(
                    pane.read(cx).split_for_drop().is_some(),
                    "every pane the floating window builds must carry the drop hook"
                );
            }
        });
    }

    /// A drop with no split direction -- the tab bar's own body, not an edge
    /// -- must still just add the tab, exactly as it did before this hook
    /// existed. Dragged across from the editor's own pane, so the drop
    /// genuinely crosses panes rather than reordering a tab already there.
    #[gpui::test]
    async fn a_drop_on_the_centre_still_just_adds_the_tab(cx: &mut gpui::TestAppContext) {
        super::init_test(cx);
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (multi_workspace, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
        let ws_project = workspace.read_with(cx, |workspace, _| workspace.project().clone());
        let handle = workspace.downgrade();

        let floating = cx.new_window_entity(|window, cx| {
            crate::host::FloatingPane::new(handle, ws_project, window, cx)
        });
        workspace.update(cx, |workspace, cx| {
            workspace.register_floating_layer(floating.clone(), cx);
        });
        floating.update_in(cx, |floating, window, cx| {
            floating.toggle(window, cx);
        });
        cx.run_until_parked();

        let floating_pane = floating.read_with(cx, |floating, _| floating.active_pane.clone());
        let items_before = floating_pane.read_with(cx, |pane, _| pane.items_len());

        let editor_pane = workspace.read_with(cx, |workspace, _| workspace.active_pane().clone());
        workspace.update_in(cx, |workspace, window, cx| {
            let item = cx.new(|cx| workspace::item::test::TestItem::new(cx));
            workspace.add_item_to_active_pane(Box::new(item), None, false, window, cx);
        });
        cx.run_until_parked();
        let item = editor_pane
            .read_with(cx, |pane, _| pane.active_item())
            .expect("the item just added must be the editor pane's active item");

        let dragged_tab = DraggedTab {
            pane: editor_pane.clone(),
            item,
            ix: 0,
            detail: 0,
            is_active: true,
        };
        // `drag_split_direction` defaults to `None` on a pane nothing has
        // hovered an edge of yet -- a centre drop, not an edge one.
        floating_pane.update_in(cx, |pane, window, cx| {
            let ix = pane.items_len();
            pane.handle_tab_drop(&dragged_tab, ix, false, window, cx);
        });
        cx.run_until_parked();
        cx.update(|_window, _cx| {});
        cx.run_until_parked();

        floating.read_with(cx, |floating, _| {
            assert_eq!(
                floating.center.panes().len(),
                1,
                "a centre drop must not split the group"
            );
        });
        floating_pane.read_with(cx, |pane, _| {
            assert_eq!(
                pane.items_len(),
                items_before + 1,
                "a centre drop must still add the dropped tab"
            );
        });
    }

    /// Builds no `FloatingPane` of its own: it drives the one
    /// `floating_pane::init`'s own `cx.observe_new` wires up for real, the
    /// same instant a workspace comes up, through the same actions a
    /// keybinding would reach -- `ToggleFloatingPane`, then `NewTerminal` to
    /// get a tab into it, then `SplitRight`. The path from the action,
    /// through `Workspace::register_action` and `key_context("FloatingPane")`,
    /// down to `split_active`, had no machine proof before this test -- only a
    /// reading of the code.
    ///
    /// `NewTerminal` rather than `new_markdown_note` (every sibling test's
    /// usual way in): it is the only registered action that seeds a tab, and
    /// this test never holds an `Entity<FloatingPane>` to call a method on --
    /// `init`'s observer owns the only instance, reached solely through the
    /// actions it registered, exactly like a real keybinding would. A second
    /// manually-built `FloatingPane` registered as its own layer was tried
    /// first and did not work: overriding `register_floating_layer` a second
    /// time left every action dispatched afterward, on either pane, silently
    /// unreachable -- observable directly, since `ToggleFloatingPane` bound
    /// to that second pane never flipped its own `open` field either. Given
    /// that, the one instance `init` already owns is the only one this test
    /// can reach at all.
    ///
    /// With no handle to assert against, the split is proven the only way
    /// left -- the active pane's own tab bar redraws narrower and its
    /// trailing `+` button moves left, which nothing else in this sequence
    /// can cause.
    #[gpui::test]
    async fn split_right_dispatched_through_a_real_workspace_splits_the_floating_group(
        cx: &mut gpui::TestAppContext,
    ) {
        super::init_test(cx);
        // `NewTerminal` spawns a real shell process through the project, the
        // same as any other terminal tab in this codebase's tests.
        cx.executor().allow_parking();
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (_multi_workspace, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        cx.run_until_parked();

        cx.dispatch_action(zed_actions::floating_pane::ToggleFloatingPane);
        cx.dispatch_action(zed_actions::floating_pane::NewTerminal);
        cx.run_until_parked();
        cx.update(|_window, _cx| {});
        cx.run_until_parked();

        let add_button_before = cx
            .debug_bounds("floating-pane-tab-bar-add")
            .expect("a tab bar with a tab in it must draw its own new-item button");

        cx.dispatch_action(zed_actions::floating_pane::SplitRight);
        cx.run_until_parked();
        cx.update(|_window, _cx| {});
        cx.run_until_parked();

        let add_button_after = cx
            .debug_bounds("floating-pane-tab-bar-add")
            .expect("the active pane must still draw its own new-item button after the split");

        assert!(
            add_button_after.origin.x < add_button_before.origin.x,
            "dispatching SplitRight through the real workspace must split the active pane, \
             narrowing it and moving its own `+` button left"
        );
    }
}
