//! One project square on the rail: what it draws, and what it offers.
//!
//! Split out of `rail.rs` because two features land on this one element -- its
//! own menu and (next) drag-to-reorder -- and a render function carrying both
//! outgrows the file it was living in.

use crate::Sidebar;
use crate::context_menu::stable_id_for_group;
use crate::project_list::ListEntry;
use crate::project_menu::build_project_menu;
use crate::rail::RAIL_WIDTH;
use gpui::{AnyElement, Context, SharedString};
use ui::{Tooltip, prelude::*, right_click_menu};
use workspace::DraggedProject;
use workspace::project_appearance::label_colour_for;

const RAIL_ITEM_SIZE: Pixels = px(48.0);
const RAIL_SQUARE_SIZE: Pixels = px(32.0);

/// One or two letters standing in for the project, Discord-style. Word
/// boundaries win over raw prefix characters so `my-cool-app` reads `MA`
/// rather than `MY`.
fn project_initials(label: &str) -> SharedString {
    let mut initials = String::new();
    for word in label.split(|c: char| !c.is_alphanumeric()) {
        let Some(first) = word.chars().next() else {
            continue;
        };
        initials.extend(first.to_uppercase());
        if initials.chars().count() == 2 {
            return initials.into();
        }
    }
    if initials.is_empty() {
        // Nothing alphanumeric to work with (e.g. a path of only
        // separators) -- fall back to raw leading characters so the square
        // is never blank.
        initials.extend(label.chars().take(2).flat_map(char::to_uppercase));
    }
    initials.into()
}

fn rail_tooltip(entry: &ListEntry) -> SharedString {
    if entry.is_reindexing {
        format!("{} — re-indexing after waking", entry.label).into()
    } else if entry.activity == Some(project::ProjectActivity::Hibernated) {
        format!("{} — hibernated", entry.label).into()
    } else {
        entry.label.clone()
    }
}

impl Sidebar {
    pub(crate) fn render_rail_item(
        &self,
        ix: usize,
        entry: &ListEntry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors();
        let warning = cx.theme().status().warning;
        let is_active = entry.is_active;
        let is_hibernated = entry.activity == Some(project::ProjectActivity::Hibernated);

        // Set by hand through this item's own menu, and it wins: the point of
        // choosing a colour is to pick this square out of a column of squares,
        // which the active-state background would undo.
        let (custom_initials, custom_colour) = self
            .multi_workspace
            .read_with(cx, |multi_workspace, _| {
                multi_workspace.project_presentation(&entry.key)
            })
            .unwrap_or((None, None));
        // A picker that is open wins over what is stored, so the avatar is the
        // preview. Nothing has been written yet at this point.
        let custom_colour = self
            .colour_preview
            .as_ref()
            .filter(|(key, _)| *key == entry.key)
            .map(|(_, colour)| *colour)
            .or(custom_colour);
        let square_bg = match custom_colour {
            Some(colour) => colour,
            None if is_active => colors.element_selected,
            None => colors.element_background,
        };
        let initials = custom_initials.unwrap_or_else(|| project_initials(&entry.label));
        let label_colour = match custom_colour {
            // A colour someone picked can be anything, so the text over it is
            // computed rather than themed -- white on a pale yellow is not a
            // style choice, it is unreadable.
            Some(colour) => Color::Custom(label_colour_for(colour)),
            None if is_active => Color::Default,
            None => Color::Muted,
        };
        // Copied out one by one: `cx.theme().colors()` hands back a reference
        // borrowed from `cx`, and the trigger closure below outlives this call.
        let accent = colors.text_accent;
        let border_selected = colors.border_selected;
        let border_transparent = colors.border_transparent;
        let element_hover = colors.element_hover;
        let sidebar = cx.entity().downgrade();
        let key = entry.key.clone();
        let key_for_drag = entry.key.clone();
        let label_for_drag = entry.label.clone();
        let label = entry.label.clone();
        let tooltip = rail_tooltip(entry);
        let is_reindexing = entry.is_reindexing;

        // Right-click, because that is the gesture anyone tries on an avatar.
        // Left-click still switches project: `right_click_menu` does not touch
        // the left button, and the test below holds that.
        right_click_menu(("project-rail-item-menu", stable_id_for_group(&entry.key)))
            .menu({
                let multi_workspace = self.multi_workspace.clone();
                let key = key.clone();
                let sidebar_for_menu = cx.entity().downgrade();
                move |window, cx| {
                    build_project_menu(
                        multi_workspace.clone(),
                        sidebar_for_menu.clone(),
                        key.clone(),
                        label.clone(),
                        window,
                        cx,
                    )
                }
            })
            .trigger(move |_is_open, _window, _cx| {
                let sidebar = sidebar.clone();
                let sidebar_for_drag = sidebar.clone();
                let key_for_click = key.clone();
                div()
                    .id(("project-rail-item", ix))
                    .debug_selector(move || format!("project-rail-item:{ix}"))
                    .relative()
                    // The column's width spelled out, not `w_full`: this row now
                    // sits inside `right_click_menu`, which requests
                    // `Style::default()` and takes no styling of its own, so it
                    // shrink-wraps its child. `w_full` against a shrink-wrapped
                    // parent resolved to the square's own 32px -- the row lost
                    // its centring, the hover background stopped spanning the
                    // column, and the active-project pill sat on the window edge.
                    .w(RAIL_WIDTH)
                    .h(RAIL_ITEM_SIZE)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_drag(
                        DraggedProject {
                            key: key_for_drag.clone(),
                            label: label_for_drag.clone(),
                            initials: initials.clone(),
                            colour: custom_colour,
                        },
                        |dragged, _, _, cx| {
                            cx.new(|_| DraggedProject {
                                key: dragged.key.clone(),
                                label: dragged.label.clone(),
                                initials: dragged.initials.clone(),
                                colour: dragged.colour,
                            })
                        },
                    )
                    // Which gap the pointer is over, decided by the row it is
                    // inside and which half of it. `on_drag_move` is *not*
                    // hitbox-filtered (unlike `on_mouse_move` -- see `div.rs`),
                    // so the bounds check is ours to make; the upside is that a
                    // row's real screen bounds arrive with the event, which keeps
                    // this right when the list is scrolled.
                    .on_drag_move(
                        move |event: &gpui::DragMoveEvent<DraggedProject>, _window, cx| {
                            if !event.bounds.contains(&event.event.position) {
                                return;
                            }
                            let lower_half = event.event.position.y > event.bounds.center().y;
                            let gap = if lower_half { ix + 1 } else { ix };
                            sidebar_for_drag
                                .update(cx, |sidebar, cx| sidebar.set_drop_gap(Some(gap), cx))
                                .ok();
                        },
                    )
                    // Leading indicator pill marking the active project, in the
                    // Discord-style rail this is modelled on. It rides the rail's
                    // outer edge, so it flips with the column rather than crossing
                    // to the side the separator is already on.
                    .when(is_active, |el| {
                        el.child(
                            div()
                                .absolute()
                                .left_0()
                                .h(px(24.0))
                                .w(px(3.0))
                                .rounded_sm()
                                .bg(accent),
                        )
                    })
                    .child(
                        div()
                            .size(RAIL_SQUARE_SIZE)
                            .rounded_md()
                            .bg(square_bg)
                            .border_1()
                            .map(|el| {
                                if is_active {
                                    el.border_color(border_selected)
                                } else {
                                    el.border_color(border_transparent)
                                }
                            })
                            .when(is_hibernated && !is_active, |el| el.opacity(0.6))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Label::new(initials.clone())
                                    .size(LabelSize::Small)
                                    .color(label_colour),
                            ),
                    )
                    // FR7 parity with the panel rows: a project mid-reindex after
                    // waking gets a corner dot, since the rail has no room for the
                    // panel's icon-plus-tooltip treatment.
                    .when(is_reindexing, |el| {
                        el.child(
                            div()
                                .absolute()
                                .top(px(6.0))
                                .right(px(6.0))
                                .size(px(6.0))
                                .rounded_full()
                                .bg(warning),
                        )
                    })
                    .hover(move |s| s.bg(element_hover))
                    .tooltip(Tooltip::text(tooltip))
                    // Through the handle rather than `cx.listener`: this closure is
                    // rebuilt on every frame the menu asks for a trigger, and a
                    // listener cannot be handed out twice.
                    .on_click(move |_: &gpui::ClickEvent, window, cx| {
                        sidebar
                            .update(cx, |sidebar, cx| {
                                sidebar.activate_or_open_workspace_for_group(
                                    &key_for_click,
                                    window,
                                    cx,
                                );
                            })
                            .ok();
                    })
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::project_initials;

    #[test]
    fn initials_prefer_word_boundaries() {
        assert_eq!(project_initials("my-cool-app").as_ref(), "MC");
        assert_eq!(project_initials("zode").as_ref(), "Z");
        assert_eq!(project_initials("examio_be").as_ref(), "EB");
        assert_eq!(project_initials("").as_ref(), "");
        assert_eq!(project_initials("///").as_ref(), "//");
    }
}

#[cfg(test)]
mod menu_tests {
    use crate::Sidebar;
    use crate::sidebar_tests::init_test;
    use fs::FakeFs;
    use gpui::{
        AppContext as _, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext,
    };
    use project::Project;
    use serde_json::json;
    use util::path;
    use workspace::MultiWorkspace;

    /// The gesture the menu answers to is the right button, and only that.
    ///
    /// Both halves matter. A left click on the avatar switches project, and that
    /// is the rail's whole job — a menu that also opened on the left button
    /// would take the primary action away. The inverse mistake was paid for in
    /// this tree the same week, on an ellipsis wrapped in `right_click_menu`
    /// that answered no gesture anyone tried.
    #[gpui::test]
    async fn the_avatar_answers_the_right_button_and_not_the_left(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/root_a"), json!({ "a.txt": "" }))
            .await;
        let project = Project::test(fs, [path!("/root_a").as_ref()], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

        multi_workspace.update_in(cx, |mw, window, cx| {
            let mw_entity = cx.entity();
            let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
            mw.register_sidebar(sidebar, cx);
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        let avatar = cx
            .debug_bounds("project-rail-item:0")
            .expect("the window's own project must draw a square on the rail");
        // The row has to span the column. Wrapping the item in `right_click_menu`
        // took this away once: that element requests `Style::default()` and
        // accepts no styling, so it shrink-wraps, and the row's `w_full` resolved
        // to the 32px square instead of the 48px column. Every visible symptom
        // followed -- the square stopped being centred, the hover background
        // stopped spanning the row, and the active-project pill landed on the
        // window's edge. State assertions see none of that.
        let rail = cx
            .debug_bounds("project-rail")
            .expect("the rail must be drawn");
        assert_eq!(
            avatar.size.width, rail.size.width,
            "the row must span the rail: {avatar:?} inside {rail:?}"
        );
        assert_eq!(
            avatar.origin.x, rail.origin.x,
            "and start at its leading edge, not indented into it"
        );

        // Left first: whatever else it does, it must not be this.
        cx.simulate_click(avatar.center(), Modifiers::default());
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("MENU_ITEM-Remove Project").is_none(),
            "a left click must stay the project switcher, not open a menu"
        );

        cx.simulate_event(MouseDownEvent {
            position: avatar.center(),
            modifiers: Modifiers::default(),
            button: MouseButton::Right,
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: avatar.center(),
            modifiers: Modifiers::default(),
            button: MouseButton::Right,
            click_count: 1,
        });
        cx.run_until_parked();

        // Named entries, not a count: the menu's own `MENU_ITEM-{label}` probes
        // say which entries are really on screen.
        for label in [
            "MENU_ITEM-Reveal in Finder",
            "MENU_ITEM-Copy Project Path",
            "MENU_ITEM-Change Initials…",
            "MENU_ITEM-Change Colour…",
            "MENU_ITEM-Remove Project",
        ] {
            assert!(
                cx.debug_bounds(label).is_some(),
                "the menu must carry {label}"
            );
        }
        // One project open, so there is nowhere to move it to and the entry is
        // left out rather than offered as a dead one.
        assert!(
            cx.debug_bounds("MENU_ITEM-Open Project in New Window")
                .is_none(),
            "with a single project there is no second window to move it to"
        );
    }
}

#[cfg(test)]
mod drag_tests {
    use crate::Sidebar;
    use crate::sidebar_tests::init_test;
    use fs::FakeFs;
    use gpui::{AppContext as _, Modifiers, MouseButton, Pixels, Point, TestAppContext, px};
    use project::Project;
    use serde_json::json;
    use util::path;
    use workspace::{MultiWorkspace, PathList, ProjectGroup, ProjectGroupKey};

    /// A window on one project, with a second registered — what the rail looks
    /// like as soon as anyone has two projects open.
    async fn two_projects_on_a_rail(
        cx: &mut TestAppContext,
    ) -> (
        gpui::Entity<MultiWorkspace>,
        gpui::Entity<Sidebar>,
        &mut gpui::VisualTestContext,
    ) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        for root in [path!("/root_a"), path!("/root_b")] {
            fs.insert_tree(root, json!({ "a.txt": "" })).await;
        }
        let project = Project::test(fs.clone(), [path!("/root_a").as_ref()], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

        multi_workspace.update(cx, |mw, _| {
            mw.test_add_project_group(ProjectGroup {
                key: ProjectGroupKey::new(None, PathList::new(&[path!("/root_b")])),
                workspaces: Vec::new(),
                expanded: true,
            });
        });
        let sidebar = multi_workspace.update_in(cx, |mw, window, cx| {
            let mw_entity = cx.entity();
            let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
            mw.register_sidebar(sidebar.clone(), cx);
            sidebar
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        (multi_workspace, sidebar, cx)
    }

    /// The order the rail is drawing -- the list `render_rail` iterates, not the
    /// model behind it.
    fn rail_order(
        cx: &mut gpui::VisualTestContext,
        sidebar: &gpui::Entity<Sidebar>,
    ) -> Vec<String> {
        sidebar.read_with(cx, |sidebar, _| {
            sidebar
                .contents
                .rail_entries
                .iter()
                .map(|entry| {
                    entry
                        .key
                        .path_list()
                        .paths()
                        .first()
                        .map(|path| path.display().to_string())
                        .unwrap_or_default()
                })
                .collect()
        })
    }

    /// Dragging to the very top puts the project first; dragging into the empty
    /// space below the last row puts it last.
    ///
    /// These are the two places a *row* index cannot name, and the two places
    /// people aim when they want a project first or last. Treating every drop
    /// that missed a row as "append" — which is what this did — sent a drag to
    /// the top of the rail to the bottom, and made a drag to the bottom look
    /// like nothing happened when the project was already there. Both read as
    /// "drag and drop doesn't work".
    #[gpui::test]
    async fn the_ends_of_the_rail_are_drop_targets(cx: &mut TestAppContext) {
        let (_multi_workspace, sidebar, cx) = two_projects_on_a_rail(cx).await;
        // Read from what the rail actually draws, not from the model. A reorder
        // that updates the model while the sidebar keeps painting its cached
        // entries looks exactly like drag-and-drop not working, and a test that
        // reads the model cannot tell the difference.
        assert_eq!(
            rail_order(cx, &sidebar),
            vec![path!("/root_a").to_string(), path!("/root_b").to_string()]
        );

        let second = cx
            .debug_bounds("project-rail-item:1")
            .expect("second avatar");
        let items = cx
            .debug_bounds("project-rail-items")
            .expect("the list container");

        // The second project, dragged to the very top of the list.
        drag(
            cx,
            second.center(),
            Point {
                x: items.center().x,
                y: items.origin.y + px(1.),
            },
        );
        assert_eq!(
            rail_order(cx, &sidebar),
            vec![path!("/root_b").to_string(), path!("/root_a").to_string()],
            "a drop at the top of the rail puts the project first"
        );

        // And back down into the empty space well below the last row — with two
        // projects the container is far taller than the rows, and that space has
        // to mean "last" rather than nothing.
        //
        // Bounds re-read: the rows have just swapped, so the coordinates from
        // before the first drag now point at the other project.
        let top_row = cx
            .debug_bounds("project-rail-item:0")
            .expect("the row that is now first");
        let last_row = cx
            .debug_bounds("project-rail-item:1")
            .expect("and the one that is now second");
        let below = Point {
            x: items.center().x,
            y: last_row.bottom() + px(120.),
        };
        assert!(
            items.contains(&below),
            "the test needs real empty space below the rows to aim at"
        );
        drag(cx, top_row.center(), below);
        assert_eq!(
            rail_order(cx, &sidebar),
            vec![path!("/root_a").to_string(), path!("/root_b").to_string()],
            "a drop below the last row puts the project last"
        );
    }

    /// Press, drag along a path, release.
    fn drag(cx: &mut gpui::VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) {
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        // A hand does not travel in one jump, and the drag only arms on a move.
        for step in 1..=4 {
            let fraction = step as f32 / 4.0;
            cx.simulate_mouse_move(
                Point {
                    x: from.x + (to.x - from.x) * fraction,
                    y: from.y + (to.y - from.y) * fraction,
                },
                Some(MouseButton::Left),
                Modifiers::default(),
            );
            cx.run_until_parked();
        }
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
    }

    /// A real drag between two avatars reorders the rail.
    ///
    /// Frame-level, not a call to the handler: mouse down, a path of moves with
    /// the button held, mouse up. That is the only way to find out whether the
    /// drag arms at all, whether the avatar's own `on_drop` is the one that
    /// receives it, and whether the window root's "move to a new window" handler
    /// stays out of it. None of those are visible to a test that calls
    /// `move_project_group` directly, and this is the test whose absence let a
    /// real defect through review.
    #[gpui::test]
    async fn a_drag_between_avatars_reorders_the_rail(cx: &mut TestAppContext) {
        let (_multi_workspace, sidebar, cx) = two_projects_on_a_rail(cx).await;
        // The rail's own list, not the model behind it: a reorder that moves the
        // model while the sidebar keeps painting its cached entries is exactly
        // the defect this suite missed once already.
        let before = rail_order(cx, &sidebar);
        assert_eq!(
            before,
            vec![path!("/root_a").to_string(), path!("/root_b").to_string()],
            "two projects, in a known order, before anything is dragged"
        );

        let first = cx
            .debug_bounds("project-rail-item:0")
            .expect("the first avatar must be drawn");
        let second = cx
            .debug_bounds("project-rail-item:1")
            .expect("and the second");

        cx.simulate_mouse_down(second.center(), MouseButton::Left, Modifiers::default());
        assert!(
            !cx.update(|_, cx| cx.has_active_drag()),
            "a press alone is not a drag"
        );

        // Not a straight line: a hand drifts out of a 48px column and comes
        // back, and a drag that leaves the rail must still be a reorder when it
        // is released on an avatar rather than a move to a new window.
        for (x, y) in [
            (second.center().x + px(120.), second.center().y),
            (second.center().x + px(200.), second.center().y - px(20.)),
            (second.center().x + px(60.), first.center().y),
            (first.center().x, first.center().y),
        ] {
            cx.simulate_mouse_move(
                Point { x, y },
                Some(MouseButton::Left),
                Modifiers::default(),
            );
            cx.run_until_parked();
        }
        assert!(
            cx.update(|_, cx| cx.has_active_drag()),
            "moving with the button held must arm the drag, or nothing below is \
             testing a drop"
        );

        cx.simulate_mouse_up(first.center(), MouseButton::Left, Modifiers::default());
        cx.run_until_parked();

        let after = rail_order(cx, &sidebar);
        assert_eq!(
            after,
            vec![path!("/root_b").to_string(), path!("/root_a").to_string()],
            "the dragged project must take the place it was dropped on, got {after:?}"
        );
        assert!(
            !cx.update(|_, cx| cx.has_active_drag()),
            "and the drag must be over"
        );
    }
}
