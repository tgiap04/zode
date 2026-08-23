//! The database away from its column: an editor tab, or a window of its own.
//!
//! Both hosts hand the view a width nobody chose for it -- a tab is as wide as
//! the pane holds, a window as wide as it was dragged -- so both lay themselves
//! out by measuring that width rather than by a flag somebody toggled. That is
//! the whole difference from the column, and it is why `Host` names two
//! behaviours rather than three places.
//!
//! Neither host shares a session with the column. Each builds its own view with
//! its own connections and its own scratch buffer, which is what the person who
//! asked for this chose: the alternative needs one `FocusHandle` in two dispatch
//! trees at once, and a focus handle cannot be in two.

use crate::database_panel::{DatabasePanel, Host};
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

/// Hides the column, so the database is in one place rather than two.
///
/// Hidden and not closed, matching `ToggleDatabase`: the panel entity stays in
/// the dock with its tree, its scratch buffer and any open session, so bringing
/// the column back brings back what was in it.
fn hide_the_column(workspace: &Workspace, window: &mut Window, cx: &mut App) {
    let Some(dock) = workspace
        .dock_for_column(workspace::dock::DockColumn::Database)
        .cloned()
    else {
        return;
    };
    dock.update(cx, |dock, cx| dock.set_open(false, window, cx));
}

pub(crate) fn open_in_editor_tab(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let handle = cx.weak_entity();
    let view = standalone_view(workspace, handle, window, cx);
    hide_the_column(workspace, window, cx);
    workspace.add_item_to_active_pane(Box::new(view), None, true, window, cx);
}

pub(crate) fn open_in_floating_window(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    hide_the_column(workspace, window, cx);

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
    pub(crate) fn is_standalone(&self) -> bool {
        self.host == Host::Standalone
    }

    /// Whether the table list stands beside the data rather than above it.
    ///
    /// In the column, never: it is one column wide and there is nothing to
    /// stand beside. Standing on its own, it depends on the width measured last
    /// frame. Full screen keeps its own answer, because that is the column being
    /// given the window rather than a width being measured.
    pub(crate) fn side_by_side(&self) -> bool {
        match self.host {
            Host::Column => self.full_screen,
            Host::Standalone => stands_side_by_side(self.measured_width),
        }
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
