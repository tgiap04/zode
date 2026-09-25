//! The launcher button, the window's chrome, and the two drags.
//!
//! The whole thing is drawn into the workspace's floating layer, so it sits over
//! the centre and the docks and under the notifications. It occupies no layout:
//! the outer element is `absolute` and `size_full`, and everything inside it is
//! positioned by hand.
//!
//! Everything here is "how the floating window draws itself". `Entry`,
//! `entry_items` and the tab-bar/title-bar `+` menus that read it live in
//! `entries.rs` instead -- the seam this file's own note once named for a later
//! change, cut once the list of things a window can hold grew past the three it
//! started with. What stayed is the window chrome: the title bar, the grips, the
//! two drags, and the Split menu, which has no other natural home.

use gpui::{
    Anchor, Bounds, ClickEvent, DragMoveEvent, MouseButton, MouseDownEvent, Pixels, Point, Size,
    WeakEntity, px,
};
use ui::prelude::*;
use ui::{ContextMenu, ContextMenuEntry, PopoverMenu, Tooltip};
use workspace::{
    ActivatePaneDown, ActivatePaneLeft, ActivatePaneRight, ActivatePaneUp, ActivePaneDecorator,
    MovePaneDown, MovePaneLeft, MovePaneRight, MovePaneUp, SplitDirection, SwapPaneDown,
    SwapPaneLeft, SwapPaneRight, SwapPaneUp,
};

use crate::entries::{Entry, entry_items};
use crate::host::{DraggedFloatingPane, Dragging, FloatingPane, Grab, Grip};

/// The grab strip along the top of the window, and the corner handles.
const TITLE_BAR_HEIGHT: Pixels = px(34.);

/// The four directions a pane can be split, paired with a label and an icon.
///
/// One list, read by the tab bar's Split button and the `+` menu's Split
/// submenu, so the two can never offer a different set of directions.
/// `ui::IconName` has no per-direction split icon -- only `Split` and
/// `SplitAlt` exist, confirmed before writing this -- so `Split` stands for
/// the horizontal pair and `SplitAlt` for the vertical one rather than
/// inventing a new asset.
const SPLIT_DIRECTIONS: [(SplitDirection, &str, IconName); 4] = [
    (SplitDirection::Right, "Split Right", IconName::Split),
    (SplitDirection::Left, "Split Left", IconName::Split),
    (SplitDirection::Up, "Split Up", IconName::SplitAlt),
    (SplitDirection::Down, "Split Down", IconName::SplitAlt),
];

/// `SPLIT_DIRECTIONS`, as menu entries against one window.
///
/// Shared by the tab bar's Split button and the `+` menu's Split submenu, so
/// the two lists cannot drift apart. `pub(crate)` because the `+` menu's own
/// Split submenu is built from `entries.rs` now, alongside the rest of that
/// menu.
pub(crate) fn split_entries(mut menu: ContextMenu, this: &WeakEntity<FloatingPane>) -> ContextMenu {
    for (direction, label, icon) in SPLIT_DIRECTIONS {
        let this = this.clone();
        menu = menu.item(
            ContextMenuEntry::new(label)
                .icon(icon)
                .handler(move |window, cx| {
                    this.update(cx, |pane, cx| pane.split_active(direction, window, cx))
                        .ok();
                }),
        );
    }
    menu
}

/// The Split button in the tab bar's left slot.
///
/// Built the same way `tab_bar_menu` is: a wrapped `IconButton` with its own
/// id, so a test can tell it apart from the tab bar's `+`.
pub(crate) fn tab_bar_split_button(this: WeakEntity<FloatingPane>) -> AnyElement {
    h_flex()
        .debug_selector(|| "floating-pane-tab-bar-split".into())
        .child(
            PopoverMenu::new("floating-pane-tab-bar-split-menu")
                .trigger_with_tooltip(
                    IconButton::new("floating-pane-split", IconName::Split)
                        .icon_size(IconSize::Small),
                    Tooltip::text("Split Pane"),
                )
                .anchor(Anchor::TopRight)
                .menu(move |window, cx| {
                    let this = this.clone();
                    Some(ContextMenu::build(window, cx, move |menu, _, _| {
                        split_entries(menu, &this)
                    }))
                })
                .into_any_element(),
        )
        .into_any_element()
}

/// How far in from an edge counts as grabbing it.
///
/// Wide enough to hit without aiming, narrow enough that the tab bar and the
/// terminal underneath keep their own clicks.
const GRIP: Pixels = px(6.);

impl Render for FloatingPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Not `occlude`: the layer covers the whole workspace, and occluding it
        // would swallow every click meant for the code underneath. Only the
        // button and the window itself occlude, which is why they are the only
        // two things drawn here.
        div()
            .absolute()
            .size_full()
            // The move and resize both land here rather than on the handles: a
            // pointer moving faster than the frame rate leaves the handle it
            // grabbed, and a listener on the handle would stop receiving the
            // very events that are meant to follow it.
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<DraggedFloatingPane>, _window, cx| {
                    this.follow_the_pointer(event, cx);
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event, _window, cx| {
                    if this.dragging.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .when(self.open, |layer| {
                layer.child(self.render_window(window, cx))
            })
            .child(self.render_launcher(cx))
            // The layer's own size is the area the window may occupy, and
            // nothing tells it: docks open, the rail widens, the editor window
            // is resized. A `canvas` is the only place a real one can be read,
            // so it is read here and used on the next frame.
            .child({
                let this = cx.entity().downgrade();
                // What the last frame measured, carried into the closure rather
                // than read back out of the entity: the comparison below has to
                // happen inside a prepaint, and a value copied at render time
                // needs no borrow there.
                let measured = self.last_container;
                gpui::canvas(
                    move |bounds: Bounds<Pixels>, _window, cx: &mut gpui::App| {
                        let container = bounds.size;
                        // The layer is drawn on every frame of the editor's
                        // life, open or shut, and the size it reports is the
                        // same one on nearly all of them. Deferring regardless
                        // costs a boxed closure and an effect cycle per frame
                        // to re-store a value that has not changed.
                        //
                        // Only the container is checked, and that is enough:
                        // every path that moves or resizes the window clamps
                        // against this same container as it writes, so a
                        // position reached without the container changing is
                        // already inside it and `note_container` would find
                        // nothing to correct.
                        if measured == Some(container) {
                            return;
                        }
                        // Deferred: this runs inside the prepaint of the very
                        // view it would update, and a `notify` raised during a
                        // draw phase is discarded rather than queued.
                        cx.defer(move |cx| {
                            this.update(cx, |this, cx| this.note_container(container, cx))
                                .ok();
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full()
            })
    }
}

impl FloatingPane {
    /// Dispatch shim: pulls the window-space pointer and the layer's own
    /// window-space origin out of the event and hands them to [`Self::follow`],
    /// which is the part a test can reach -- `DragMoveEvent`'s fields are
    /// private, so nothing outside gpui can build one to call this directly.
    fn follow_the_pointer(
        &mut self,
        event: &DragMoveEvent<DraggedFloatingPane>,
        cx: &mut Context<Self>,
    ) {
        self.follow(event.event.position, event.bounds.origin, event.bounds.size);
        cx.notify();
    }

    /// Applies one frame of a drag, given the pointer in window space and the
    /// floating layer's own origin in that same window space.
    ///
    /// The two grabs read the layer origin differently on purpose. A resize
    /// grabs a corner of the window and needs it converted into the layer-space
    /// `bounds_within` already works in, so it subtracts the layer origin out.
    /// A move instead reads `dragging.anchor`, which already carries that
    /// origin baked into a constant that cancels it (see the field's doc
    /// comment on `Dragging`) -- so the move arm never touches `layer_origin`
    /// at all. That asymmetry, stated in the two arms below, is what keeps a
    /// title-bar drag from jumping by the layer's own position on screen.
    pub(crate) fn follow(
        &mut self,
        pointer_in_window: Point<Pixels>,
        layer_origin: Point<Pixels>,
        container: Size<Pixels>,
    ) {
        let Some(dragging) = self.dragging else {
            return;
        };
        match dragging.grab {
            Grab::Move => self.move_to(dragging.anchor + pointer_in_window, container),
            Grab::Resize(corner) => {
                self.resize_to(corner, pointer_in_window - layer_origin, container)
            }
        }
    }

    /// Records what a press grabbed, before the drag begins.
    ///
    /// On mouse-down rather than in `on_drag`'s constructor, because the anchor
    /// needs `&mut Self` to store and the constructor only lends `&mut App`.
    /// Without it the window's corner jumps to the pointer on the first move,
    /// however far in from the edge the press landed.
    pub(crate) fn grab(&mut self, grab: Grab, event: &MouseDownEvent, bounds: Bounds<Pixels>) {
        let anchor = match grab {
            Grab::Move => bounds.origin - event.position,
            // A resize does not need one: the corner goes where the pointer is.
            Grab::Resize(_) => Point::default(),
        };
        self.dragging = Some(Dragging { grab, anchor });
    }

    fn render_window(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors();
        // Laid out from the container, which is only known while painting. The
        // element is positioned by its stored offset and clamped by `canvas`
        // measurement on the next frame; until one has happened it takes the
        // opening corner.
        let placement = self.bounds_within(self.last_container.unwrap_or(Size {
            width: px(1280.),
            height: px(800.),
        }));

        // Resolved once, and the field dropped with it when it no longer
        // upgrades: a weak left pointing at a pane that is gone would be
        // retried on every frame for the life of the window.
        let zoomed = self.zoomed.as_ref().and_then(|view| view.upgrade());
        if zoomed.is_none() {
            self.zoomed = None;
        }

        div()
            .id("floating-pane")
            // What the keymap block `"context": "FloatingPane"` matches
            // against. Action dispatch already bubbles from the focused pane up
            // through this div, so no `track_focus` is needed here for that.
            .key_context("FloatingPane")
            .occlude()
            .absolute()
            .left(placement.origin.x)
            .top(placement.origin.y)
            .w(placement.size.width)
            .h(placement.size.height)
            .flex()
            .flex_col()
            .rounded_lg()
            .overflow_hidden()
            .bg(colors.elevated_surface_background)
            .border_1()
            .border_color(colors.border)
            .shadow_lg()
            // Re-handles the workspace's own pane-navigation actions here,
            // the same way `terminal_panel` does inside the dock: GPUI
            // dispatches from the focused element outward, so these only run
            // while focus is somewhere inside this window, and the
            // workspace's own handlers see the keystroke exactly as before
            // whenever it isn't. `ToggleZoom` needs no entry of its own --
            // `Pane::render` already registers it on the pane itself, which
            // is what actually emits `Event::ZoomIn`/`ZoomOut` that
            // `handle_pane_event` reacts to.
            .on_action(cx.listener(|this, _: &ActivatePaneLeft, window, cx| {
                this.activate_in(SplitDirection::Left, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ActivatePaneRight, window, cx| {
                this.activate_in(SplitDirection::Right, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ActivatePaneUp, window, cx| {
                this.activate_in(SplitDirection::Up, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ActivatePaneDown, window, cx| {
                this.activate_in(SplitDirection::Down, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SwapPaneLeft, _window, cx| {
                this.swap_in(SplitDirection::Left, cx);
            }))
            .on_action(cx.listener(|this, _: &SwapPaneRight, _window, cx| {
                this.swap_in(SplitDirection::Right, cx);
            }))
            .on_action(cx.listener(|this, _: &SwapPaneUp, _window, cx| {
                this.swap_in(SplitDirection::Up, cx);
            }))
            .on_action(cx.listener(|this, _: &SwapPaneDown, _window, cx| {
                this.swap_in(SplitDirection::Down, cx);
            }))
            .on_action(cx.listener(|this, _: &MovePaneLeft, _window, cx| {
                this.move_active_to_border(SplitDirection::Left, cx);
            }))
            .on_action(cx.listener(|this, _: &MovePaneRight, _window, cx| {
                this.move_active_to_border(SplitDirection::Right, cx);
            }))
            .on_action(cx.listener(|this, _: &MovePaneUp, _window, cx| {
                this.move_active_to_border(SplitDirection::Up, cx);
            }))
            .on_action(cx.listener(|this, _: &MovePaneDown, _window, cx| {
                this.move_active_to_border(SplitDirection::Down, cx);
            }))
            .child(self.render_title_bar(placement, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    // Empty, the window *is* the menu. Opening straight onto a
                    // terminal decided for somebody what they came for, and a
                    // blank rectangle would have told them nothing.
                    .child(if self.is_empty(cx) {
                        self.render_empty_state(cx)
                    } else if let Some(zoomed) = zoomed {
                        // `PaneGroup::render` draws whichever pane matches its
                        // `zoomed` argument as an empty `div` -- the workspace
                        // draws that pane again as a separate top layer. This
                        // window has no such layer, so `center.render` below
                        // always gets `None` and the zoomed pane is drawn here
                        // instead, filling the window on its own.
                        div().size_full().child(zoomed).into_any_element()
                    } else {
                        self.center
                            .render(
                                None,
                                &ActivePaneDecorator::new(&self.active_pane, &self.workspace),
                                window,
                                cx,
                            )
                            .into_any_element()
                    }),
            )
            .children(
                Grip::ALL
                    .into_iter()
                    .map(|grip| self.render_grip(grip, placement, cx)),
            )
            .into_any_element()
    }

    /// The strip that moves the window, with the menu and the minimise button.
    fn render_title_bar(&self, placement: Bounds<Pixels>, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .id("floating-pane-title")
            .h(TITLE_BAR_HEIGHT)
            .flex_shrink_0()
            .px_1()
            .gap_1()
            .justify_between()
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .cursor(gpui::CursorStyle::OpenHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _window, _cx| {
                    this.grab(Grab::Move, event, placement);
                }),
            )
            .on_drag(DraggedFloatingPane, |_, _, _, cx| {
                cx.new(|_| DraggedFloatingPane)
            })
            .child(self.render_menu(cx))
            .child(
                h_flex()
                    .gap_0p5()
                    // Two buttons, and the difference between them is the whole
                    // reason the second exists: one hides the window and leaves
                    // everything running, the other ends it.
                    .child(
                        IconButton::new("floating-pane-minimise", IconName::Dash)
                            .icon_size(IconSize::Small)
                            .tooltip(|_window, cx| {
                                Tooltip::for_action(
                                    "Minimise \u{2014} keeps its terminals running",
                                    &zed_actions::floating_pane::ToggleFloatingPane,
                                    cx,
                                )
                            })
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.toggle(window, cx)
                            })),
                    )
                    .child(
                        IconButton::new("floating-pane-close", IconName::Close)
                            .icon_size(IconSize::Small)
                            .tooltip(|_window, cx| {
                                Tooltip::for_action(
                                    "Close \u{2014} ends its terminals and threads",
                                    &zed_actions::floating_pane::CloseFloatingPane,
                                    cx,
                                )
                            })
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.confirm_shut_down(window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    /// The `+` menu, for when the window already has tabs in it.
    fn render_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        PopoverMenu::new("floating-pane-menu")
            .trigger_with_tooltip(
                IconButton::new("floating-pane-add", IconName::Plus).icon_size(IconSize::Small),
                Tooltip::text("New\u{2026}"),
            )
            .anchor(Anchor::TopLeft)
            .menu(move |window, cx| {
                let this = this.clone();
                Some(ContextMenu::build(window, cx, move |menu, _, _| {
                    let menu = entry_items(menu, &this);
                    // Takes `this` outright: nothing after it needs a handle.
                    menu.separator().entry("Minimise", None, move |window, cx| {
                        this.update(cx, |this, cx| this.toggle(window, cx)).ok();
                    })
                }))
            })
            .into_any_element()
    }

    /// One of the eight resize handles.
    ///
    /// Drawn as strips and squares laid over the window's own border rather than
    /// inside it: a handle with width of its own would be a band around the
    /// edge that swallows clicks meant for the tab bar or the terminal. The
    /// corners are children after the edges, so where the two overlap the corner
    /// wins -- which is what a pointer in the very corner means.
    fn render_grip(
        &self,
        grip: Grip,
        placement: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (id, cursor) = match grip {
            Grip::North => ("grip-n", gpui::CursorStyle::ResizeUpDown),
            Grip::South => ("grip-s", gpui::CursorStyle::ResizeUpDown),
            Grip::East => ("grip-e", gpui::CursorStyle::ResizeLeftRight),
            Grip::West => ("grip-w", gpui::CursorStyle::ResizeLeftRight),
            Grip::NorthWest => ("grip-nw", gpui::CursorStyle::ResizeUpLeftDownRight),
            Grip::SouthEast => ("grip-se", gpui::CursorStyle::ResizeUpLeftDownRight),
            Grip::NorthEast => ("grip-ne", gpui::CursorStyle::ResizeUpRightDownLeft),
            Grip::SouthWest => ("grip-sw", gpui::CursorStyle::ResizeUpRightDownLeft),
        };

        div()
            .id(id)
            .absolute()
            .map(|handle| match grip {
                // Edges span the side they sit on, inset by the corner squares
                // so the corner keeps its own reach.
                Grip::North => handle.top_0().left(GRIP).right(GRIP).h(GRIP),
                Grip::South => handle.bottom_0().left(GRIP).right(GRIP).h(GRIP),
                Grip::West => handle.left_0().top(GRIP).bottom(GRIP).w(GRIP),
                Grip::East => handle.right_0().top(GRIP).bottom(GRIP).w(GRIP),
                Grip::NorthWest => handle.top_0().left_0().size(GRIP),
                Grip::NorthEast => handle.top_0().right_0().size(GRIP),
                Grip::SouthWest => handle.bottom_0().left_0().size(GRIP),
                Grip::SouthEast => handle.bottom_0().right_0().size(GRIP),
            })
            .cursor(cursor)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                    this.grab(Grab::Resize(grip), event, placement);
                    // The title bar is 34pt tall and the north grip sits inside
                    // its first 6, so both listeners contain the press. GPUI
                    // dispatches the bubble phase in *reverse* registration
                    // order, which means these grips -- painted after the title
                    // bar -- run first and the title bar ran last and overwrote
                    // them: dragging the top edge moved the window instead of
                    // resizing it. The same overlap applies to all four corners
                    // and the upper stretch of both side edges.
                    //
                    // Safe for the drag itself: `on_drag` records its pending
                    // press from a listener GPUI registers during paint, after
                    // this one, so in reverse order it has already run. That is
                    // the same pairing the dock's resize handle uses.
                    cx.stop_propagation();
                }),
            )
            .on_drag(DraggedFloatingPane, |_, _, _, cx| {
                cx.new(|_| DraggedFloatingPane)
            })
            .into_any_element()
    }

    /// The same list, laid out as the window's body.
    ///
    /// This is what a freshly opened window shows. Opening straight onto a
    /// terminal would decide for somebody what they came for; a blank rectangle
    /// would tell them nothing.
    fn render_empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_px()
            .p_4()
            .debug_selector(|| "floating-pane-empty".into())
            .children(Entry::all().into_iter().map(|entry| {
                h_flex()
                    .id(entry.id())
                    .w(px(300.))
                    .px_2()
                    .py_1()
                    .gap_2()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|row| row.bg(cx.theme().colors().element_hover))
                    .child(
                        Icon::new(entry.icon())
                            .size(IconSize::Small)
                            .color(Color::Muted),
                    )
                    .child(Label::new(entry.label()).size(LabelSize::Small))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        entry.run(this, window, cx)
                    }))
            }))
            .into_any_element()
    }

    /// Asks before ending everything the window holds.
    ///
    /// Asked because it is not undoable: a shell with a half-finished command
    /// and an agent mid-answer both die, and the button that does it sits one
    /// pixel from the one that does not.
    pub(crate) fn confirm_shut_down(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Nothing running is nothing to lose, and a dialog over an empty window
        // is a dialog that teaches people to dismiss dialogs.
        if self.is_empty(cx) {
            self.shut_down(window, cx);
            return;
        }
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            "Close the floating window?",
            Some(
                "Its terminals and agent threads will end. Minimise instead to \
                 keep them running.",
            ),
            &["Close", "Cancel"],
            cx,
        );
        self.opening = Some(cx.spawn_in(window, async move |this, cx| {
            if answer.await.ok() == Some(0) {
                this.update_in(cx, |this, window, cx| this.shut_down(window, cx))
                    .ok();
            }
        }));
    }

    /// The button in the bottom-right corner that opens the window.
    ///
    /// Always drawn, open or not: it is also how the window is put away, and a
    /// control that vanishes when it works is one nobody finds twice.
    ///
    /// Given a surface of its own rather than left as a bare icon. It sits over
    /// the editor, where a bare glyph would land on top of code and read as part
    /// of it -- the one place in the app where a button has no panel behind it.
    fn render_launcher(&self, cx: &mut Context<Self>) -> AnyElement {
        let open = self.open;
        let colors = cx.theme().colors();
        div()
            .absolute()
            .right(px(16.))
            .bottom(px(16.))
            .occlude()
            .rounded_full()
            .bg(colors.elevated_surface_background)
            .border_1()
            .border_color(colors.border)
            .shadow_md()
            .child(
                IconButton::new("floating-pane-launcher", IconName::Screen)
                    .icon_size(IconSize::Small)
                    .shape(ui::IconButtonShape::Square)
                    .toggle_state(open)
                    .tooltip(move |_window, cx| {
                        Tooltip::for_action(
                            if open {
                                "Put the floating window away"
                            } else {
                                "Open a floating window"
                            },
                            &zed_actions::floating_pane::ToggleFloatingPane,
                            cx,
                        )
                    })
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, window, cx| this.toggle(window, cx)),
                    ),
            )
            .into_any_element()
    }
}
