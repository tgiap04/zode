//! The launcher button, the window's chrome, and the two drags.
//!
//! The whole thing is drawn into the workspace's floating layer, so it sits over
//! the centre and the docks and under the notifications. It occupies no layout:
//! the outer element is `absolute` and `size_full`, and everything inside it is
//! positioned by hand.

use gpui::{
    Anchor, Bounds, ClickEvent, DragMoveEvent, MouseButton, MouseDownEvent, Pixels, Point, Size, px,
};
use ui::prelude::*;
use ui::{ContextMenu, ContextMenuEntry, PopoverMenu, Tooltip};

use crate::content::AGENTS;
use crate::host::{DraggedFloatingPane, Dragging, FloatingPane, Grab, Grip};

/// The grab strip along the top of the window, and the corner handles.
const TITLE_BAR_HEIGHT: Pixels = px(34.);
/// Everything the window can be asked to hold.
///
/// One list, read by both the `+` menu and the empty state. Two lists would be
/// two places to add the next entry, and the one used less would be the one that
/// fell behind.
#[derive(Clone, Copy)]
enum Entry {
    Terminal,
    NewNote,
    OpenNote,
    Agent(&'static str, IconName, &'static str),
}

impl Entry {
    fn all() -> Vec<Entry> {
        let mut entries = vec![Entry::Terminal, Entry::NewNote, Entry::OpenNote];
        entries.extend(
            AGENTS
                .iter()
                .copied()
                .map(|(agent, icon, label)| Entry::Agent(agent, icon, label)),
        );
        entries
    }

    fn label(self) -> &'static str {
        match self {
            Entry::Terminal => "New Terminal",
            Entry::NewNote => "New Markdown Note",
            Entry::OpenNote => "Open Markdown Note",
            Entry::Agent(_, _, label) => label,
        }
    }

    fn icon(self) -> IconName {
        match self {
            Entry::Terminal => IconName::Terminal,
            Entry::NewNote => IconName::Notepad,
            Entry::OpenNote => IconName::FileMarkdown,
            Entry::Agent(_, icon, _) => icon,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Entry::Terminal => "floating-pane-new-terminal",
            Entry::NewNote => "floating-pane-new-note",
            Entry::OpenNote => "floating-pane-open-note",
            Entry::Agent(agent, _, _) => agent,
        }
    }

    /// Whether a separator belongs above this entry.
    ///
    /// The agents are a different kind of thing from the three above them, and
    /// the first of them is where the list changes subject.
    fn opens_a_group(self) -> bool {
        matches!(self, Entry::Agent(agent, _, _) if AGENTS.first().is_some_and(|(first, _, _)| *first == agent))
    }

    fn run(self, pane: &mut FloatingPane, window: &mut Window, cx: &mut Context<FloatingPane>) {
        match self {
            Entry::Terminal => pane.open_terminal(window, cx),
            Entry::NewNote => pane.new_markdown_note(window, cx),
            Entry::OpenNote => pane.open_markdown_note(window, cx),
            Entry::Agent(agent, _, _) => pane.open_agent(agent, window, cx),
        }
    }
}

/// How far in from an edge counts as grabbing it.
///
/// Wide enough to hit without aiming, narrow enough that the tab bar and the
/// terminal underneath keep their own clicks.
const GRIP: Pixels = px(6.);

impl Render for FloatingPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .when(self.open, |layer| layer.child(self.render_window(cx)))
            .child(self.render_launcher(cx))
            // The layer's own size is the area the window may occupy, and
            // nothing tells it: docks open, the rail widens, the editor window
            // is resized. A `canvas` is the only place a real one can be read,
            // so it is read here and used on the next frame.
            .child({
                let this = cx.entity().downgrade();
                gpui::canvas(
                    move |bounds: Bounds<Pixels>, _window, cx: &mut gpui::App| {
                        let container = bounds.size;
                        // Deferred: this runs inside the prepaint of the very
                        // view it would update, and the size is wanted for the
                        // next frame anyway.
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
    /// Applies one frame of a drag.
    ///
    /// The container's bounds come from the event rather than from a field: this
    /// element is the floating layer, so its bounds *are* the area the window
    /// may occupy, and reading them here means a resized editor window needs no
    /// invalidation anywhere.
    fn follow_the_pointer(
        &mut self,
        event: &DragMoveEvent<DraggedFloatingPane>,
        cx: &mut Context<Self>,
    ) {
        let Some(dragging) = self.dragging else {
            return;
        };
        let container = event.bounds.size;
        let pointer = event.event.position - event.bounds.origin;
        match dragging.grab {
            Grab::Move => self.move_to(pointer - dragging.offset, container),
            Grab::Resize(corner) => self.resize_to(corner, pointer, container),
        }
        cx.notify();
    }

    /// Records what a press grabbed, before the drag begins.
    ///
    /// On mouse-down rather than in `on_drag`'s constructor, because the offset
    /// needs `&mut Self` to store and the constructor only lends `&mut App`.
    /// Without the offset the window's corner jumps to the pointer on the first
    /// move, however far in from the edge the press landed.
    fn grab(&mut self, grab: Grab, event: &MouseDownEvent, bounds: Bounds<Pixels>) {
        let offset = match grab {
            Grab::Move => event.position - bounds.origin,
            // A resize does not need one: the corner goes where the pointer is.
            Grab::Resize(_) => Point::default(),
        };
        self.dragging = Some(Dragging { grab, offset });
    }

    fn render_window(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors();
        // Laid out from the container, which is only known while painting. The
        // element is positioned by its stored offset and clamped by `canvas`
        // measurement on the next frame; until one has happened it takes the
        // opening corner.
        let placement = self.bounds_within(self.last_container.unwrap_or(Size {
            width: px(1280.),
            height: px(800.),
        }));

        div()
            .id("floating-pane")
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
                    } else {
                        self.pane.clone().into_any_element()
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
                                Tooltip::simple("Close \u{2014} ends its terminals and threads", cx)
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
                Some(ContextMenu::build(window, cx, move |mut menu, _, _| {
                    for entry in Entry::all() {
                        if entry.opens_a_group() {
                            menu = menu.separator().header("Agent");
                        }
                        let this = this.clone();
                        menu = menu.item(
                            ContextMenuEntry::new(entry.label())
                                .icon(entry.icon())
                                .handler(move |window, cx| {
                                    this.update(cx, |pane, cx| entry.run(pane, window, cx)).ok();
                                }),
                        );
                    }
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
    fn confirm_shut_down(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
