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

        let tabs = window.read_with(cx, |window, cx| window.pane.read(cx).items_len());
        assert_eq!(tabs, 1, "the note opened");

        window.update_in(cx, |window, window_handle, cx| {
            window.toggle(window_handle, cx);
            assert!(!window.open, "put away");
        });
        cx.run_until_parked();

        window.read_with(cx, |window, cx| {
            assert_eq!(
                window.pane.read(cx).items_len(),
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
            assert_eq!(window.pane.read(cx).items_len(), 1);
        });

        window.update_in(cx, |window, window_handle, cx| {
            window.shut_down(window_handle, cx)
        });
        cx.run_until_parked();

        window.read_with(cx, |window, cx| {
            assert!(!window.open, "and it is put away too");
            assert_eq!(
                window.pane.read(cx).items_len(),
                0,
                "nothing left running: that is the point of the button"
            );
        });
    }
}
