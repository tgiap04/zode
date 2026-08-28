//! The database as an editor tab, or a window of its own.
//!
//! There is no column. It had one, and it was the wrong shape: a result grid is
//! the one thing here that cannot be made narrow and stay readable, and a column
//! is the one place that cannot be wide. Both hosts here are given a width nobody
//! chose -- a tab is as wide as its pane, a window as wide as it was dragged --
//! so both lay themselves out by measuring it.
//!
//! Each view has its own connections and its own scratch buffer. Sharing one
//! between two hosts needs one `FocusHandle` in two dispatch trees at once, and a
//! focus handle cannot be in two.

use crate::database_panel::DatabasePanel;
use gpui::{
    App, Bounds, Entity, EventEmitter, Pixels, SharedString, TitlebarOptions, WeakEntity, Window,
    WindowBounds, WindowKind, WindowOptions, px, size,
};
use ui::prelude::*;
use workspace::Workspace;
use workspace::item::Item;

/// At or above this the table list stands beside the data; below it everything
/// stacks into one column.
///
/// Chosen to be the width at which the list *and* a useful number of result
/// columns both fit: narrower than this the side-by-side layout gives the data
/// less room than the stacked one does, which is the opposite of the point.
pub const SIDE_BY_SIDE_WIDTH: Pixels = px(720.);

/// How a standalone view lays itself out at a given width.
///
/// Unmeasured reads as side by side, and that is a considered guess rather than
/// a fallback: a fresh tab or window is nearly always wider than the breakpoint,
/// so guessing the other way would flash the stacked layout on the first frame
/// of the common case. The wrong guess costs one frame either way.
pub(crate) fn stands_side_by_side(width: Option<Pixels>) -> bool {
    width.is_none_or(|width| width >= SIDE_BY_SIDE_WIDTH)
}

/// This view emits nothing to its pane.
///
/// An empty enum rather than `()`: it cannot be constructed, so it says at the
/// type level that there is no event here, instead of leaving a unit event
/// somebody might later think means something.
pub enum DatabaseItemEvent {}

impl EventEmitter<DatabaseItemEvent> for DatabasePanel {}

impl Item for DatabasePanel {
    type Event = DatabaseItemEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        "Database".into()
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<ui::Icon> {
        Some(ui::Icon::new(IconName::Database))
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        Some("Database Opened")
    }
}

/// Builds a view for a host that is not the column.
///
/// Reads the workspace's two facts out *before* creating the entity. Both
/// callers need that: one is inside a leased workspace, the other inside a
/// different window's context. See `DatabasePanel::standalone`.
fn standalone_view(
    workspace: &Workspace,
    handle: WeakEntity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<DatabasePanel> {
    let languages = workspace.project().read(cx).languages().clone();
    cx.new(|cx| DatabasePanel::standalone(handle, languages, window, cx))
}

/// Brings the database tab forward, or opens one.
///
/// Existing tab first: a second would be a second set of connections and a second
/// scratch buffer, each unaware of the other.
pub(crate) fn open(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    // Collected before the mutable borrow: `items_of_type` borrows the workspace
    // and `activate_item` needs it back.
    let existing = workspace.items_of_type::<DatabasePanel>(cx).next();
    if let Some(existing) = existing {
        workspace.activate_item(&existing, true, true, window, cx);
        return;
    }
    open_in_editor_tab(workspace, window, cx);
}

/// Steps back to the tab that was being read, if the database tab is in front.
///
/// Put away, not closed: closing would end every open session and lose a
/// half-written statement over the second press of a button whose whole job is to
/// be pressed twice. Same shape as `AgentView::put_away`.
pub(crate) fn put_away(workspace: &Workspace, window: &mut Window, cx: &mut App) -> bool {
    let pane = workspace.active_pane().clone();

    // The whole read is taken through one borrow: reaching for the pane again
    // partway through is how this repo has previously turned a stale read into an
    // abort.
    let previous = {
        let pane = pane.read(cx);
        let showing_database = pane
            .active_item()
            .and_then(|item| item.downcast::<DatabasePanel>())
            .is_some();
        if !showing_database {
            return false;
        }
        // The last entry is the tab just activated -- `Pane::update_history`
        // dedupes then pushes -- so the one before it is where the press goes.
        pane.activation_history()
            .iter()
            .rev()
            .skip(1)
            .find_map(|entry| {
                pane.items()
                    .position(|item| item.item_id() == entry.entity_id)
            })
    };

    // Nothing to step back to: a pane holding only this stays as it is. Closing
    // instead would end a live session over a button press.
    if let Some(index) = previous {
        pane.update(cx, |pane, cx| {
            pane.activate_item(index, true, true, window, cx);
        });
        return true;
    }
    false
}

pub(crate) fn open_in_editor_tab(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let handle = cx.weak_entity();
    let view = standalone_view(workspace, handle, window, cx);
    workspace.add_item_to_active_pane(Box::new(view), None, true, window, cx);
}

pub(crate) fn open_in_floating_window(
    workspace: &mut Workspace,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let handle: WeakEntity<Workspace> = cx.weak_entity();
    let languages = workspace.project().read(cx).languages().clone();

    // `AlwaysOnTop`, and the other three kinds are each wrong in their own way.
    //
    // `PopUp` sets `NSWindowStyleMaskNonactivatingPanel` on macOS, so the window
    // never becomes active and never takes keyboard focus -- right for a
    // notification, useless for something you type SQL into. `Floating` is an
    // activating panel and does stand above other applications, but an `NSPanel`
    // is absent from window cycling, the Window menu and Mission Control, so
    // once it loses focus there is no way back to it except the mouse. `Normal`
    // is findable but sits under whatever you switch to.
    //
    // `AlwaysOnTop` is the pair that was actually wanted: an ordinary `NSWindow`
    // -- so it is in all three lists -- at `NSFloatingWindowLevel`, which is a
    // global window level rather than a parent relationship and so keeps it over
    // other applications. On Linux and Windows it degrades to an ordinary
    // window: findable, not raised.
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1100.), px(720.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Database".into()),
            ..Default::default()
        }),
        kind: WindowKind::AlwaysOnTop,
        window_min_size: Some(size(px(480.), px(320.))),
        ..Default::default()
    };

    if let Err(error) = cx.open_window(options, |window, cx| {
        cx.new(|cx| DatabasePanel::standalone(handle, languages, window, cx))
    }) {
        log::error!("could not open the database in a window of its own: {error}");
    }
}

impl DatabasePanel {
    /// Whether the table list stands beside the data rather than above it,
    /// by the width measured last frame.
    pub(crate) fn side_by_side(&self) -> bool {
        stands_side_by_side(self.measured_width)
    }

    pub(crate) fn note_measured_width(&mut self, width: Pixels, cx: &mut Context<Self>) {
        // Only when the answer changes, not on every frame: this runs from a
        // prepaint and a `notify` per frame would be a redraw loop.
        let was = self.side_by_side();
        self.measured_width = Some(width);
        if self.side_by_side() != was {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The breakpoint is inclusive and the unmeasured case is side by side.
    /// Both are decisions rather than details -- an exclusive comparison would
    /// make the layout at exactly the breakpoint depend on rounding, and an
    /// unmeasured stacked guess would flash on every wide tab.
    #[test]
    fn the_breakpoint_is_inclusive_and_unmeasured_stands_side_by_side() {
        assert!(stands_side_by_side(None), "unmeasured must guess wide");
        assert!(stands_side_by_side(Some(SIDE_BY_SIDE_WIDTH)));
        assert!(stands_side_by_side(Some(SIDE_BY_SIDE_WIDTH + px(1.))));
        assert!(!stands_side_by_side(Some(SIDE_BY_SIDE_WIDTH - px(1.))));
        assert!(!stands_side_by_side(Some(px(0.))));
    }
}
