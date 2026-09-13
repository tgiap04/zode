//! The two orders, held apart.
//!
//! A dock draws its stacked sections in `stack_order`, which the user arranges,
//! and everything outside the dock -- the project rail, the status bar -- draws
//! from `panel_entries`, which stays in `activation_priority` order. Nothing but
//! the design keeps those two apart, so the negative tests here are the only
//! thing that would catch a future change quietly collapsing them back together.

use crate::dock::test::{OtherTestPanel, TestPanel};
use crate::{MultiWorkspace, Workspace};
use gpui::{
    AppContext as _, Entity, EntityId, Modifiers, MouseButton, Point, TestAppContext,
    VisualTestContext, px,
};
use project::{FakeFs, Project};
use settings::SettingsStore;

use super::DockPosition;

fn init_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let settings_store = SettingsStore::test(cx);
        cx.set_global(settings_store);
        cx.set_global(db::AppDatabase::test_new());
        theme_settings::init(theme::LoadThemes::JustBase, cx);
    });
}

/// A left dock showing `priorities.len()` stacked panels, in priority order.
///
/// The priorities are handed in rather than fixed so a test can give two panels
/// different `default_size`s and prove the column did not follow the drag.
async fn stacked_left_dock<'a>(
    priorities: &[u32],
    cx: &'a mut TestAppContext,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

    let priorities = priorities.to_vec();
    workspace.update_in(cx, |workspace, window, cx| {
        for priority in priorities.iter() {
            let panel = cx.new(|cx| TestPanel::new(DockPosition::Left, *priority, cx));
            workspace.add_panel(panel, window, cx);
        }
        workspace.left_dock().update(cx, |dock, cx| {
            dock.set_open(true, window, cx);
            for index in 0..priorities.len() {
                dock.show_panel(index, window, cx);
            }
        });
    });
    cx.run_until_parked();
    (workspace, cx)
}

/// A left dock of two panels of DIFFERENT types.
///
/// `persistent_name` names a panel *type*, so the serialized record cannot tell
/// two `TestPanel`s apart -- `apply_stack_state`'s own de-duplication drops the
/// second. Any test that round-trips through `DockStackState` needs two types,
/// which is also what the real docks have (GitPanel, BranchPanel, OutlinePanel).
async fn distinct_stacked_left_dock(
    cx: &mut TestAppContext,
) -> (Entity<Workspace>, &mut VisualTestContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

    workspace.update_in(cx, |workspace, window, cx| {
        let first = cx.new(|cx| TestPanel::new(DockPosition::Left, 100, cx));
        let second = cx.new(|cx| OtherTestPanel::new(DockPosition::Left, 101, cx));
        workspace.add_panel(first, window, cx);
        workspace.add_panel(second, window, cx);
        workspace.left_dock().update(cx, |dock, cx| {
            dock.set_open(true, window, cx);
            dock.show_panel(0, window, cx);
            dock.show_panel(1, window, cx);
        });
    });
    cx.run_until_parked();
    (workspace, cx)
}

fn drawn_order(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<EntityId> {
    workspace.read_with(cx, |workspace, cx| {
        workspace
            .left_dock()
            .read(cx)
            .stacked_panels()
            .map(|panel| panel.panel_id())
            .collect()
    })
}

fn button_order(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<EntityId> {
    workspace.read_with(cx, |workspace, cx| {
        workspace
            .left_dock()
            .read(cx)
            .panels()
            .map(|panel| panel.panel_id())
            .collect()
    })
}

/// Moving the second section above the first reorders what is drawn.
#[gpui::test]
async fn moving_a_section_reorders_the_stack(cx: &mut TestAppContext) {
    let (workspace, cx) = stacked_left_dock(&[100, 101], cx).await;
    let before = drawn_order(&workspace, cx);
    assert_eq!(before.len(), 2, "both panels should be showing");

    let moved = workspace.update(cx, |workspace, cx| {
        workspace
            .left_dock()
            .update(cx, |dock, cx| dock.move_stacked_panel(before[1], 0, cx))
    });
    assert!(moved, "moving the lower section to the top gap is a real move");

    let after = drawn_order(&workspace, cx);
    assert_eq!(
        after,
        vec![before[1], before[0]],
        "the dragged section should now be drawn first"
    );
}

/// **Negative test.** The rail and the status bar draw from `panels()`, and the
/// user asked for those to keep a fixed order. Nothing but the design enforces
/// that, so this is what catches a change that quietly makes the buttons follow
/// the drag.
#[gpui::test]
async fn reordering_the_stack_leaves_the_button_order_alone(cx: &mut TestAppContext) {
    let (workspace, cx) = stacked_left_dock(&[100, 101], cx).await;
    let buttons_before = button_order(&workspace, cx);
    let drawn = drawn_order(&workspace, cx);

    workspace.update(cx, |workspace, cx| {
        workspace
            .left_dock()
            .update(cx, |dock, cx| dock.move_stacked_panel(drawn[1], 0, cx))
    });

    assert_eq!(
        button_order(&workspace, cx),
        buttons_before,
        "panels() feeds the rail and the status bar and must not follow the drag"
    );
    assert_ne!(
        drawn_order(&workspace, cx),
        buttons_before,
        "the drawn order did move, so the two orders are genuinely held apart"
    );
}

/// **Negative test.** The column's width comes from `panel_entries[0]`, which a
/// reorder does not touch. True by construction — which is exactly what the last
/// defect in this area also believed, so it is asserted rather than trusted.
#[gpui::test]
async fn reordering_the_stack_leaves_the_column_width_alone(cx: &mut TestAppContext) {
    let (workspace, cx) = stacked_left_dock(&[100, 101], cx).await;

    let drawn = drawn_order(&workspace, cx);
    // A width the user chose, so a column that re-derived its extent from the
    // newly-topmost panel's default would read differently afterwards.
    workspace.update_in(cx, |workspace, window, cx| {
        workspace.left_dock().update(cx, |dock, cx| {
            dock.resize_active_panel(Some(px(317.)), None, window, cx);
        });
    });
    let width_before = workspace.update_in(cx, |workspace, window, cx| {
        workspace
            .left_dock()
            .read(cx)
            .stored_active_panel_size(window, cx)
    });

    workspace.update(cx, |workspace, cx| {
        workspace
            .left_dock()
            .update(cx, |dock, cx| dock.move_stacked_panel(drawn[1], 0, cx))
    });

    let width_after = workspace.update_in(cx, |workspace, window, cx| {
        workspace
            .left_dock()
            .read(cx)
            .stored_active_panel_size(window, cx)
    });
    assert_eq!(
        width_before, width_after,
        "reordering sections must not resize the column"
    );
}

/// **Negative test.** `active_panel_index` indexes `panel_entries`, which a
/// reorder does not touch, so focus bookkeeping must not drift.
#[gpui::test]
async fn reordering_the_stack_leaves_the_active_index_alone(cx: &mut TestAppContext) {
    let (workspace, cx) = stacked_left_dock(&[100, 101], cx).await;
    let drawn = drawn_order(&workspace, cx);
    let active_before =
        workspace.read_with(cx, |workspace, cx| workspace.left_dock().read(cx).active_panel_index());

    workspace.update(cx, |workspace, cx| {
        workspace
            .left_dock()
            .update(cx, |dock, cx| dock.move_stacked_panel(drawn[1], 0, cx))
    });

    assert_eq!(
        workspace.read_with(cx, |workspace, cx| workspace
            .left_dock()
            .read(cx)
            .active_panel_index()),
        active_before,
        "a reorder is not a focus change"
    );
}

/// Dropping a section on either of its own edges is not a move, and must not
/// spend a persist or a notify.
#[gpui::test]
async fn dropping_a_section_on_its_own_edge_does_nothing(cx: &mut TestAppContext) {
    let (workspace, cx) = stacked_left_dock(&[100, 101], cx).await;
    let drawn = drawn_order(&workspace, cx);

    for gap in [0, 1] {
        let moved = workspace.update(cx, |workspace, cx| {
            workspace
                .left_dock()
                .update(cx, |dock, cx| dock.move_stacked_panel(drawn[0], gap, cx))
        });
        assert!(
            !moved,
            "gap {gap} names where the first section already is, so it is not a move"
        );
    }
    assert_eq!(drawn_order(&workspace, cx), drawn, "nothing should have moved");
}

/// The height shares travel with the section rather than being reset, so the
/// others close up and the dragged one keeps what the user gave it.
#[gpui::test]
async fn the_height_shares_follow_the_section(cx: &mut TestAppContext) {
    let (workspace, cx) = stacked_left_dock(&[100, 101, 102], cx).await;
    let drawn = drawn_order(&workspace, cx);

    let flexes = workspace.read_with(cx, |workspace, cx| {
        workspace.left_dock().read(cx).stack_flexes()
    });
    *flexes.lock() = vec![0.5, 1.0, 1.5];

    workspace.update(cx, |workspace, cx| {
        workspace
            .left_dock()
            .update(cx, |dock, cx| dock.move_stacked_panel(drawn[0], 3, cx))
    });

    let after = flexes.lock().clone();
    assert_eq!(
        after,
        vec![1.0, 1.5, 0.5],
        "the moved section's share should travel with it, the rest closing up"
    );
    assert_eq!(
        after.len(),
        3,
        "PaneAxisElement indexes flexes raw, so the length must never change"
    );
}

/// A drawn index is not an index into `stack_order`, which holds the hidden
/// panels too — so a move computed in drawn space lands in the wrong absolute
/// slot whenever something put away sits above the target.
///
/// The wrong slot is **invisible while the panel stays hidden**: both the right
/// and the wrong answer produce the same drawn order. It only surfaces when the
/// hidden panel comes back, which is the whole point of `stack_order` holding
/// every panel — a section put away returns where the user left it. So the
/// assertion that matters here is the one after the panel is shown again; an
/// earlier draft of this test stopped before that and passed against a
/// deliberately broken index.
#[gpui::test]
async fn a_hidden_panel_comes_back_where_the_user_left_it(cx: &mut TestAppContext) {
    let (workspace, cx) = stacked_left_dock(&[100, 101, 102], cx).await;
    let drawn = drawn_order(&workspace, cx);
    assert_eq!(drawn.len(), 3);
    let (first, second, third) = (drawn[0], drawn[1], drawn[2]);

    // Put the TOP one away, so the two that remain both sit below something
    // hidden and their drawn indices no longer match their absolute ones.
    workspace.update_in(cx, |workspace, window, cx| {
        workspace.left_dock().update(cx, |dock, cx| {
            dock.hide_panel_by_id(first, window, cx);
        });
    });
    assert_eq!(
        drawn_order(&workspace, cx),
        vec![second, third],
        "the top section is away, the other two still read in order"
    );

    // Move the lower of the two above the other.
    workspace.update(cx, |workspace, cx| {
        workspace
            .left_dock()
            .update(cx, |dock, cx| dock.move_stacked_panel(third, 0, cx))
    });
    assert_eq!(
        drawn_order(&workspace, cx),
        vec![third, second],
        "the visible pair swapped"
    );

    // Bring the hidden one back. A move that used the drawn index as an
    // absolute one will have jumped the section over it, and this is where that
    // shows.
    workspace.update_in(cx, |workspace, window, cx| {
        workspace.left_dock().update(cx, |dock, cx| {
            let index = dock.panels().position(|panel| panel.panel_id() == first);
            if let Some(index) = index {
                dock.show_panel(index, window, cx);
            }
        });
    });

    assert_eq!(
        drawn_order(&workspace, cx),
        vec![first, third, second],
        "the panel that was put away must return above the pair, where it was \
         left -- not below a section that moved past it while it was hidden"
    );
}

/// The arrangement survives a restart, and the rail's order still does not.
#[gpui::test]
async fn the_dragged_arrangement_round_trips(cx: &mut TestAppContext) {
    let (workspace, cx) = distinct_stacked_left_dock(cx).await;
    let drawn = drawn_order(&workspace, cx);

    workspace.update(cx, |workspace, cx| {
        workspace
            .left_dock()
            .update(cx, |dock, cx| dock.move_stacked_panel(drawn[1], 0, cx))
    });

    let (recorded, names_in_priority_order) = workspace.read_with(cx, |workspace, cx| {
        let dock = workspace.left_dock().read(cx);
        (
            dock.stack_state(),
            dock.panels()
                .map(|panel| panel.persistent_name().to_string())
                .collect::<Vec<_>>(),
        )
    });

    let drawn_names = workspace.read_with(cx, |workspace, cx| {
        workspace
            .left_dock()
            .read(cx)
            .stacked_panels()
            .map(|panel| panel.persistent_name().to_string())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        recorded.showing, drawn_names,
        "the record must carry the dragged order, not the entry order"
    );
    assert_ne!(
        recorded.showing, names_in_priority_order,
        "the dragged order genuinely differs from the priority order, \
         so this test is proving something"
    );
}

/// A record written in a non-priority order, with unequal shares, must hand each
/// section back the height it was recorded with.
///
/// This is the latent defect the phase closes: `apply_stack_state` used to
/// resolve the record into entry order while `flexes` stayed indexed by drawn
/// position, so the two index spaces disagreed and each section inherited its
/// neighbour's height.
#[gpui::test]
async fn a_restored_stack_gives_each_section_its_recorded_height(cx: &mut TestAppContext) {
    let (workspace, cx) = distinct_stacked_left_dock(cx).await;
    let drawn = drawn_order(&workspace, cx);

    // Arrange, then give the two sections clearly different shares.
    workspace.update(cx, |workspace, cx| {
        workspace
            .left_dock()
            .update(cx, |dock, cx| dock.move_stacked_panel(drawn[1], 0, cx))
    });
    let flexes = workspace.read_with(cx, |workspace, cx| {
        workspace.left_dock().read(cx).stack_flexes()
    });
    *flexes.lock() = vec![0.4, 1.6];

    let recorded = workspace.read_with(cx, |workspace, cx| {
        workspace.left_dock().read(cx).stack_state()
    });

    // Put the dock back the way it started, then restore the record over it.
    workspace.update_in(cx, |workspace, window, cx| {
        workspace.left_dock().update(cx, |dock, cx| {
            dock.move_stacked_panel(drawn[0], 0, cx);
            *dock.stack_flexes().lock() = vec![1.0, 1.0];
            dock.apply_stack_state(&recorded, window, cx);
        });
    });

    let restored_names = workspace.read_with(cx, |workspace, cx| {
        workspace
            .left_dock()
            .read(cx)
            .stacked_panels()
            .map(|panel| panel.persistent_name().to_string())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        restored_names, recorded.showing,
        "the sections must come back in the recorded order"
    );
    assert_eq!(
        flexes.lock().clone(),
        vec![0.4, 1.6],
        "and each one with the height recorded against it, not its neighbour's"
    );
}

/// A real drag, driven at the frame level: press, a path of moves, release.
///
/// Every assertion here would also pass against a stack that cannot be dragged
/// at all if the gesture never armed — so `has_active_drag()` is asserted
/// mid-gesture. That is the defect that got through review on the project rail
/// once, and the reason the check sits in the middle rather than at the end.
#[gpui::test]
async fn dragging_a_header_onto_another_reorders_the_stack(cx: &mut TestAppContext) {
    let (workspace, cx) = distinct_stacked_left_dock(cx).await;
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();

    let before = drawn_order(&workspace, cx);
    let top = cx
        .debug_bounds("dock-stack-header:0")
        .expect("the first section's header must be drawn");
    let bottom = cx
        .debug_bounds("dock-stack-header:1")
        .expect("and the second's");

    cx.simulate_mouse_down(bottom.center(), MouseButton::Left, Modifiers::default());
    assert!(
        !cx.update(|_, cx| cx.has_active_drag()),
        "a press alone is not a drag"
    );

    // A hand does not travel in one jump, and the drag only arms on a move.
    let mut armed_mid_gesture = false;
    for fraction in [0.25, 0.5, 0.75, 1.0] {
        let y = bottom.center().y + (top.center().y - bottom.center().y) * fraction;
        cx.simulate_mouse_move(
            Point {
                x: top.center().x,
                y,
            },
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.run_until_parked();
        armed_mid_gesture |= cx.update(|_, cx| cx.has_active_drag());
    }
    assert!(
        armed_mid_gesture,
        "the gesture never armed a drag, so nothing below this proves anything"
    );

    // Released on the upper half of the top header, so the gap is above it.
    let landing = Point {
        x: top.center().x,
        y: top.origin.y + px(2.),
    };
    cx.simulate_mouse_move(landing, Some(MouseButton::Left), Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_up(landing, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();

    assert_eq!(
        drawn_order(&workspace, cx),
        vec![before[1], before[0]],
        "the dragged section should have landed above the one it was dropped on"
    );
}

/// **Negative test.** The same real gesture, released away from every header,
/// must change nothing and must not leave the drop indicator behind.
#[gpui::test]
async fn a_drag_released_off_the_headers_changes_nothing(cx: &mut TestAppContext) {
    let (workspace, cx) = distinct_stacked_left_dock(cx).await;
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();

    let before = drawn_order(&workspace, cx);
    let bottom = cx
        .debug_bounds("dock-stack-header:1")
        .expect("the second section's header must be drawn");

    cx.simulate_mouse_down(bottom.center(), MouseButton::Left, Modifiers::default());
    // Out into the centre pane, far from any header.
    for step in 1..=4 {
        cx.simulate_mouse_move(
            Point {
                x: bottom.center().x + px(400. * step as f32),
                y: bottom.center().y,
            },
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.run_until_parked();
    }
    let away = Point {
        x: bottom.center().x + px(1600.),
        y: bottom.center().y,
    };
    cx.simulate_mouse_up(away, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();

    assert_eq!(
        drawn_order(&workspace, cx),
        before,
        "a drag released off the stack is not a reorder"
    );
    assert!(
        !cx.update(|_, cx| cx.has_active_drag()),
        "the drag must not still be in flight after the release"
    );
}

/// Dragging a section DOWNWARD, which is the direction every other test in this
/// file happened to miss.
///
/// A gap index names the slot in the order as it stands *before* the section is
/// lifted out; the remove-then-insert correction belongs in the flex array, not
/// in the lookup of what to land against. Mixing the two made every downward
/// move land one slot early — and, at the top of the stack, made it silently do
/// nothing at all. Backward moves are immune by construction, which is exactly
/// why a suite of them proved nothing.
#[gpui::test]
async fn moving_a_section_downward_lands_on_the_requested_gap(cx: &mut TestAppContext) {
    let (workspace, cx) = stacked_left_dock(&[100, 101, 102, 103], cx).await;
    let before = drawn_order(&workspace, cx);
    assert_eq!(before.len(), 4);

    // Into the middle: between the second and third sections.
    let moved = workspace.update(cx, |workspace, cx| {
        workspace
            .left_dock()
            .update(cx, |dock, cx| dock.move_stacked_panel(before[0], 2, cx))
    });
    assert!(moved, "moving down two gaps is a real move");
    assert_eq!(
        drawn_order(&workspace, cx),
        vec![before[1], before[0], before[2], before[3]],
        "the section should sit in the gap it was dropped in, not one short of it"
    );
}

/// The gap past the last section is the one no section index can name.
#[gpui::test]
async fn moving_a_section_to_the_last_gap_puts_it_last(cx: &mut TestAppContext) {
    let (workspace, cx) = stacked_left_dock(&[100, 101, 102], cx).await;
    let before = drawn_order(&workspace, cx);

    let moved = workspace.update(cx, |workspace, cx| {
        workspace
            .left_dock()
            .update(cx, |dock, cx| dock.move_stacked_panel(before[0], 3, cx))
    });
    assert!(moved);
    assert_eq!(
        drawn_order(&workspace, cx),
        vec![before[1], before[2], before[0]],
        "dropped past the last section, it must come to rest last"
    );
}

/// A downward move across a hidden panel: the two failure modes — drawn-vs-
/// absolute index, and pre-vs-post-removal index — compound here, so this is
/// the case that pins both at once.
#[gpui::test]
async fn moving_downward_past_a_hidden_panel_still_lands_right(cx: &mut TestAppContext) {
    let (workspace, cx) = stacked_left_dock(&[100, 101, 102, 103], cx).await;
    let all = drawn_order(&workspace, cx);

    // Put the second one away, so `stack_order` and the drawn order diverge.
    workspace.update_in(cx, |workspace, window, cx| {
        workspace.left_dock().update(cx, |dock, cx| {
            dock.hide_panel_by_id(all[1], window, cx);
        });
    });
    let showing = drawn_order(&workspace, cx);
    assert_eq!(showing, vec![all[0], all[2], all[3]]);

    // Drop the first section into the gap between the other two.
    workspace.update(cx, |workspace, cx| {
        workspace
            .left_dock()
            .update(cx, |dock, cx| dock.move_stacked_panel(showing[0], 2, cx))
    });

    assert_eq!(
        drawn_order(&workspace, cx),
        vec![all[2], all[0], all[3]],
        "the hidden panel must not absorb a slot the drawn order was counting"
    );
}

/// The shares and the identities have to agree after a move — checking the flex
/// numbers alone let a reordering bug through, because the flex array is
/// permuted by its own self-consistent arithmetic.
#[gpui::test]
async fn the_height_shares_stay_with_the_sections_they_belong_to(cx: &mut TestAppContext) {
    let (workspace, cx) = stacked_left_dock(&[100, 101, 102], cx).await;
    let before = drawn_order(&workspace, cx);

    let flexes = workspace.read_with(cx, |workspace, cx| {
        workspace.left_dock().read(cx).stack_flexes()
    });
    *flexes.lock() = vec![0.5, 1.0, 1.5];

    workspace.update(cx, |workspace, cx| {
        workspace
            .left_dock()
            .update(cx, |dock, cx| dock.move_stacked_panel(before[0], 3, cx))
    });

    assert_eq!(
        drawn_order(&workspace, cx),
        vec![before[1], before[2], before[0]],
        "identity first: the sections must actually be in this order"
    );
    assert_eq!(
        flexes.lock().clone(),
        vec![1.0, 1.5, 0.5],
        "and each share must belong to the section now at that position"
    );
}

/// The user's report: three sections, drag the last one between the first two.
#[gpui::test]
async fn moving_the_last_section_between_the_first_two(cx: &mut TestAppContext) {
    let (workspace, cx) = stacked_left_dock(&[100, 101, 102], cx).await;
    let before = drawn_order(&workspace, cx);
    assert_eq!(before.len(), 3);

    let moved = workspace.update(cx, |workspace, cx| {
        workspace
            .left_dock()
            .update(cx, |dock, cx| dock.move_stacked_panel(before[2], 1, cx))
    });
    assert!(moved, "dropping the last section into the first gap is a real move");
    assert_eq!(
        drawn_order(&workspace, cx),
        vec![before[0], before[2], before[1]],
        "it should come to rest between the other two"
    );
}

/// The user's report, driven at the frame level: three sections, drag the last
/// one's header onto the upper half of the middle one's header.
#[gpui::test]
async fn dragging_the_last_header_between_the_first_two(cx: &mut TestAppContext) {
    let (workspace, cx) = stacked_left_dock(&[100, 101, 102], cx).await;
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();

    let before = drawn_order(&workspace, cx);
    let middle = cx
        .debug_bounds("dock-stack-header:1")
        .expect("the middle section's header must be drawn");
    let last = cx
        .debug_bounds("dock-stack-header:2")
        .expect("the last section's header must be drawn");

    cx.simulate_mouse_down(last.center(), MouseButton::Left, Modifiers::default());
    for fraction in [0.25, 0.5, 0.75, 1.0] {
        let y = last.center().y + (middle.center().y - last.center().y) * fraction;
        cx.simulate_mouse_move(
            Point { x: last.center().x, y },
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.run_until_parked();
    }
    assert!(
        cx.update(|_, cx| cx.has_active_drag()),
        "the drag must be in flight before the release"
    );

    // Upper half of the middle header: the gap above it, i.e. between the first
    // and second sections.
    let landing = Point {
        x: middle.center().x,
        y: middle.origin.y + px(2.),
    };
    cx.simulate_mouse_move(landing, Some(MouseButton::Left), Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_up(landing, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();

    assert_eq!(
        drawn_order(&workspace, cx),
        vec![before[0], before[2], before[1]],
        "the last section should have landed between the other two"
    );
}

/// Releasing over a section's BODY, not its thin header strip.
///
/// A header is a ~24px strip at the top of a section that is otherwise hundreds
/// of pixels tall, so "drag it between those two" lands on a panel's content far
/// more often than on a header. If only the header accepts the drop, the gesture
/// the user actually performs does nothing at all.
#[gpui::test]
async fn releasing_over_a_section_body_still_reorders(cx: &mut TestAppContext) {
    let (workspace, cx) = stacked_left_dock(&[100, 101, 102], cx).await;
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();

    let before = drawn_order(&workspace, cx);
    let first = cx
        .debug_bounds("dock-stacked-panel:0")
        .expect("the first section must be drawn");
    let last_header = cx
        .debug_bounds("dock-stack-header:2")
        .expect("the last section's header must be drawn");

    cx.simulate_mouse_down(last_header.center(), MouseButton::Left, Modifiers::default());
    for fraction in [0.25, 0.5, 0.75, 1.0] {
        let y = last_header.center().y + (first.center().y - last_header.center().y) * fraction;
        cx.simulate_mouse_move(
            Point { x: first.center().x, y },
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.run_until_parked();
    }

    // The lower half of the FIRST section's body -- nowhere near a header, but
    // unambiguously "below the first section".
    let landing = Point {
        x: first.center().x,
        y: first.bottom() - px(4.),
    };
    cx.simulate_mouse_move(landing, Some(MouseButton::Left), Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_up(landing, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();

    assert_eq!(
        drawn_order(&workspace, cx),
        vec![before[0], before[2], before[1]],
        "a release over the first section's lower half must drop it below that section"
    );
}
