//! The window's state: where it is, how big, and what it is holding.

use std::collections::HashMap;

use gpui::{
    AnyWeakView, Bounds, Context, Entity, EntityId, EventEmitter, FocusHandle, Focusable, Pixels,
    Point, Size, Subscription, WeakEntity, Window, px, size,
};
use ui::prelude::*;
use workspace::{Pane, PaneGroup, Workspace};

/// Where the window sits when it is first opened, measured from the
/// bottom-right corner so it lands beside the button that opened it.
const OPENING_SIZE: Size<Pixels> = size(px(720.), px(440.));
/// Clear of the launcher button, which sits in the same corner.
const OPENING_INSET: Pixels = px(64.);

/// Below this the tab bar cannot show a tab and its close button together, and
/// a terminal has no usable line.
///
/// It measures the window, not the panes inside it. Splitting divides this area
/// again and nothing stops it: `PaneAxis::resize` holds a divider drag to 80pt
/// across and 100pt down, but a split being *made* passes through no such
/// guard, so a window at this size split four ways leaves 80pt panes. Left
/// reachable rather than grown with the pane count, which would refuse a resize
/// somebody asked for over a split they had forgotten about. Closing a pane
/// undoes it.
pub(crate) const SMALLEST: Size<Pixels> = size(px(320.), px(180.));

/// How much of the window must stay inside the workspace when it is dragged.
///
/// Not zero: a window dragged fully past an edge cannot be dragged back, since
/// the title bar that moves it is what went out of reach. This keeps enough of
/// that bar on screen to grab.
pub(crate) const MUST_STAY_VISIBLE: Pixels = px(80.);

/// What a drag in progress is doing.
///
/// The grabbed handle is carried rather than recomputed from the pointer: once
/// the pointer has moved, which handle it started on is no longer derivable, and
/// guessing it mid-drag makes a window flip between growing and moving.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Grab {
    Move,
    Resize(Grip),
}

/// One of the eight places a window can be pulled by.
///
/// Named by the sides it moves rather than by position, because that *is* the
/// rule: `East` moves the right edge and holds the left, `SouthEast` moves both
/// the right and the bottom. One table below reads the two axes out of the name
/// and the resize needs no per-handle arithmetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Grip {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

/// Which edge of an axis a grip moves, if it moves that axis at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Near,
    Far,
}

impl Grip {
    pub(crate) const ALL: [Grip; 8] = [
        Grip::North,
        Grip::South,
        Grip::East,
        Grip::West,
        Grip::NorthEast,
        Grip::NorthWest,
        Grip::SouthEast,
        Grip::SouthWest,
    ];

    /// The vertical edge this grip moves: the top, the bottom, or neither.
    pub(crate) fn vertical(self) -> Option<Side> {
        match self {
            Grip::North | Grip::NorthEast | Grip::NorthWest => Some(Side::Near),
            Grip::South | Grip::SouthEast | Grip::SouthWest => Some(Side::Far),
            Grip::East | Grip::West => None,
        }
    }

    /// The horizontal edge this grip moves: the left, the right, or neither.
    pub(crate) fn horizontal(self) -> Option<Side> {
        match self {
            Grip::West | Grip::NorthWest | Grip::SouthWest => Some(Side::Near),
            Grip::East | Grip::NorthEast | Grip::SouthEast => Some(Side::Far),
            Grip::North | Grip::South => None,
        }
    }
}

/// A drag that has started, with the pointer's offset inside the thing it
/// grabbed.
///
/// The offset is what stops the window jumping: without it the window's corner
/// snaps to the pointer on the first move, however far in from the edge the
/// press landed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Dragging {
    pub(crate) grab: Grab,
    pub(crate) offset: Point<Pixels>,
}

/// The payload dragged, carrying nothing.
///
/// GPUI's drag needs a value and a view to render under the pointer; the window
/// is already drawn where it is, so there is nothing to draw and nothing to
/// carry. What the drag is *doing* lives in `FloatingPane::dragging`, which is
/// set on mouse-down before the drag starts -- the payload cannot hold it,
/// because the offset is only known once the press position is compared against
/// bounds this crate reads from the drag-move event.
#[derive(Clone)]
pub(crate) struct DraggedFloatingPane;

impl Render for DraggedFloatingPane {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

pub struct FloatingPane {
    pub(crate) workspace: WeakEntity<Workspace>,
    /// The project the window's panes build their items against.
    ///
    /// Cached rather than read back through `workspace` on every use: the
    /// drag-to-split path calls into this crate from inside a
    /// `Workspace` lease, where reading `self.workspace` a second time is a
    /// `double_lease_panic` -- an abort, not a recoverable error. Caching it
    /// here means every method that needs the project already has it.
    pub(crate) project: Entity<project::Project>,
    /// The tabs, arranged as a group of one or more panes. A real pane, for
    /// the reasons in the crate docs.
    ///
    /// Built once and kept even while the window is closed: it holds live
    /// terminals and agent threads, and rebuilding it on each open would end
    /// them.
    pub(crate) center: PaneGroup,
    /// The pane new tabs land in and focus moves to.
    ///
    /// Kept apart from `center` because the group's tree carries no notion of
    /// "the one the user is looking at" -- that is what a leader/decorator
    /// needs to render, and what every `open_*` method targets.
    pub(crate) active_pane: Entity<Pane>,
    pub(crate) open: bool,
    /// Top-left of the window, relative to the workspace body it floats over.
    ///
    /// Relative rather than absolute so a resized editor window does not leave
    /// it stranded; `clamp_into` re-reads the container on every frame that
    /// draws it.
    pub(crate) position: Option<Point<Pixels>>,
    pub(crate) size: Size<Pixels>,
    pub(crate) dragging: Option<Dragging>,
    /// Building whatever the menu last asked for.
    ///
    /// One at a time and held in a field, not detached: each of these ends in a
    /// process or a file read, and a detached one would keep going -- and keep
    /// adding a tab -- after this window is gone.
    pub(crate) opening: Option<gpui::Task<()>>,
    /// The floating layer's size, read from the last frame that drew it.
    ///
    /// Measured rather than asked for: the layer spans the centre and the docks,
    /// and that area changes with every dock, the rail and the editor window.
    /// `None` until the first frame, where the window falls back to a guess for
    /// one frame -- invisible, because the clamp corrects it immediately after.
    pub(crate) last_container: Option<Size<Pixels>>,
    pub(crate) focus_handle: FocusHandle,
    /// The pane currently filling the whole window, if one has been zoomed.
    ///
    /// `PaneGroup::render` draws whichever pane matches its own `zoomed`
    /// argument as an empty `div` -- in the workspace that pane is drawn again
    /// as a separate top layer. This window has no such layer, so it is always
    /// `None` that goes into `center.render`, and this field is what draws the
    /// zoomed pane itself instead.
    pub(crate) zoomed: Option<AnyWeakView>,
    /// One subscription per live pane, keyed by entity id so the exact
    /// subscription for a removed pane can be dropped in the same step that
    /// removes it. A `Vec` would only ever grow as panes split and collapse.
    pub(crate) pane_subscriptions: HashMap<EntityId, Subscription>,
}

pub enum FloatingPaneEvent {}

impl EventEmitter<FloatingPaneEvent> for FloatingPane {}

impl Focusable for FloatingPane {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl FloatingPane {
    /// Takes the project rather than reading it back off the workspace.
    ///
    /// This is built from inside `observe_new`, where the workspace is already
    /// mutably borrowed; reaching back through the handle to read it would
    /// borrow it twice and abort. It is also why the project is kept as a
    /// field rather than re-derived later -- see the field's own doc comment.
    pub fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<project::Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let this = cx.weak_entity();
        let pane = Self::build_pane(&workspace, project.clone(), this, window, cx);
        let mut floating_pane = Self {
            workspace,
            project,
            center: PaneGroup::new(pane.clone()),
            active_pane: pane.clone(),
            open: false,
            position: None,
            size: OPENING_SIZE,
            dragging: None,
            opening: None,
            last_container: None,
            focus_handle: cx.focus_handle(),
            zoomed: None,
            pane_subscriptions: HashMap::new(),
        };
        floating_pane.subscribe_to(&pane, window, cx);
        floating_pane
    }

    /// Opens the window, or puts it away.
    ///
    /// Put away rather than emptied: the tabs keep their terminals and threads,
    /// so the button can be pressed twice without killing what is inside. That
    /// is also what the Minimize entry in the menu does -- the two are the same
    /// operation reached two ways.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = !self.open;
        if self.open && self.active_pane.read(cx).items_len() > 0 {
            // Focus only when there is something to focus. On an empty window
            // the menu is the content, and stealing focus into an empty pane
            // would take the keyboard away from the editor for nothing.
            self.focus_pane(window, cx);
        }
        cx.notify();
    }

    /// Whether the window has nothing in it, and so shows the menu as its body.
    ///
    /// Totalled over every pane in the group, not just the active one: the
    /// menu is the body only when the *whole* window is empty, not when one
    /// pane of several happens to be -- which matters as soon as the group holds
    /// more than one.
    pub(crate) fn is_empty(&self, cx: &gpui::App) -> bool {
        self.center
            .panes()
            .iter()
            .all(|pane| pane.read(cx).items_len() == 0)
    }

    /// Ends everything the window is holding, and puts it away.
    ///
    /// The point is the resources, not the pixels: minimising keeps every
    /// terminal and agent thread running behind a hidden window, which is right
    /// for "I will be back in a minute" and wrong for "I am done". This is the
    /// other one -- it closes every tab, so the shells exit and the threads
    /// stop.
    ///
    /// Routed through `Pane::close_all_items` rather than dropping the pane, so
    /// an unsaved note still gets its save prompt. Losing a note to a button
    /// labelled "free some memory" would be the worst possible trade.
    pub(crate) fn shut_down(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Every pane in the group, not just the active one: the group can
        // hold more than one, and a shut-down that left a
        // second pane's terminals running would defeat the whole point of the
        // button.
        let closing: Vec<_> = self
            .center
            .panes()
            .into_iter()
            .map(|pane| {
                pane.update(cx, |pane, cx| {
                    pane.close_all_items(&workspace::pane::CloseAllItems::default(), window, cx)
                })
            })
            .collect();
        self.open = false;
        cx.notify();
        self.opening = Some(cx.spawn(async move |_this, _cx| {
            for closing in closing {
                if let Err(error) = closing.await {
                    log::error!("could not close the floating window's tabs: {error}");
                }
            }
        }));
    }

    pub(crate) fn focus_pane(&self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.active_pane.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }

    /// The window's rectangle inside a container of this size.
    ///
    /// Computed on every frame rather than stored, because the container is the
    /// workspace body and it changes with the editor window, the docks and the
    /// rail. A stored rectangle would be right until the first time somebody
    /// opened a dock.
    pub(crate) fn bounds_within(&self, container: Size<Pixels>) -> Bounds<Pixels> {
        let size = Size {
            width: self.size.width.min(container.width).max(SMALLEST.width),
            height: self.size.height.min(container.height).max(SMALLEST.height),
        };
        let origin = self.position.unwrap_or_else(|| Point {
            x: container.width - size.width - OPENING_INSET,
            y: container.height - size.height - OPENING_INSET,
        });
        Bounds {
            origin: clamp_into(origin, size, container),
            size,
        }
    }

    /// Records the layer's measured size, and keeps the window inside it.
    ///
    /// Only redraws when something actually moved: this runs from a prepaint,
    /// and an unconditional `notify` per frame is a redraw loop.
    pub(crate) fn note_container(&mut self, container: Size<Pixels>, cx: &mut Context<Self>) {
        let was = self.last_container;
        self.last_container = Some(container);
        let before = self.position;
        if let Some(position) = self.position {
            self.position = Some(clamp_into(position, self.size, container));
        }
        if was != self.last_container || before != self.position {
            cx.notify();
        }
    }

    pub(crate) fn move_to(&mut self, origin: Point<Pixels>, container: Size<Pixels>) {
        self.position = Some(clamp_into(origin, self.size, container));
    }

    /// Pulls one edge or corner to a new place, holding everything it does not
    /// move.
    ///
    /// Holding the *opposite* side rather than the origin is the whole
    /// difference between resizing and moving: dragging the top must leave the
    /// bottom alone, and that means the origin moves as the height changes.
    ///
    /// The two axes are settled independently and identically. An edge grip
    /// simply has nothing to say about one of them, which is why `North` and
    /// `NorthEast` need no separate arithmetic.
    pub(crate) fn resize_to(
        &mut self,
        grip: Grip,
        pointer: Point<Pixels>,
        container: Size<Pixels>,
    ) {
        let current = self.bounds_within(container);
        let (left, width) = pull(
            grip.horizontal(),
            current.origin.x,
            current.size.width,
            pointer.x,
            SMALLEST.width,
        );
        let (top, height) = pull(
            grip.vertical(),
            current.origin.y,
            current.size.height,
            pointer.y,
            SMALLEST.height,
        );

        self.size = Size {
            width: width.min(container.width),
            height: height.min(container.height),
        };
        self.position = Some(clamp_into(Point { x: left, y: top }, self.size, container));
    }
}

/// One axis of a resize: where the moving edge lands, and what that leaves.
///
/// `None` means this grip does not touch this axis, so the axis is returned
/// untouched -- that is what makes an edge grip and a corner grip the same code.
///
/// The pointer is clamped *before* the extent is taken from it, so pulling an
/// edge past its opposite stops at the minimum rather than turning the
/// rectangle inside out.
fn pull(
    side: Option<Side>,
    near: Pixels,
    extent: Pixels,
    pointer: Pixels,
    smallest: Pixels,
) -> (Pixels, Pixels) {
    let far = near + extent;
    match side {
        None => (near, extent),
        Some(Side::Far) => {
            let far = pointer.max(near + smallest);
            (near, far - near)
        }
        Some(Side::Near) => {
            let near = pointer.min(far - smallest).max(px(0.));
            (near, far - near)
        }
    }
}

/// Keeps a window reachable inside its container.
///
/// Both edges are clamped, but the far edge only to `MUST_STAY_VISIBLE` rather
/// than fully inside: a window wider than the container has to hang off
/// somewhere, and refusing to place it at all would leave it invisible.
fn clamp_into(origin: Point<Pixels>, size: Size<Pixels>, container: Size<Pixels>) -> Point<Pixels> {
    let furthest_x = (container.width - MUST_STAY_VISIBLE).max(px(0.));
    let furthest_y = (container.height - MUST_STAY_VISIBLE).max(px(0.));
    Point {
        x: origin
            .x
            .min(furthest_x)
            .max(MUST_STAY_VISIBLE - size.width)
            .max(px(0.).min(container.width - size.width)),
        y: origin.y.min(furthest_y).max(px(0.)),
    }
}
