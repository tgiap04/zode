//! The container list as an editor tab, or a window of its own.
//!
//! There is no column. It had one, briefly, and it was the wrong shape: a list
//! of containers with an image name, ports and a status is a wide thing, and a
//! column is the one place that cannot be wide. Both hosts here are given a
//! width nobody chose -- a tab is as wide as its pane, a window as wide as it
//! was dragged -- so both lay themselves out by measuring it.
//!
//! Each view is its own: its own backends, its own list, its own focus. Sharing
//! one between two hosts needs one `FocusHandle` in two dispatch trees at once,
//! and a focus handle cannot be in two.

use std::sync::Arc;

use gpui::{
    App, Bounds, EventEmitter, Pixels, SharedString, TitlebarOptions, Window, WindowBounds,
    WindowKind, WindowOptions, px, size,
};
use ui::prelude::*;
use workspace::Workspace;
use workspace::item::Item;

use crate::container_panel::ContainerPanel;

/// At or above this the list shows the columns that only pay for themselves when
/// there is room: the status text and the published ports.
///
/// Chosen as the width at which those two fit *without* squeezing the image
/// name, which is the column that says what a container actually is. Narrower
/// than this, a five-column table gives every column less than a useful amount,
/// which is the opposite of the point.
pub const WIDE_ENOUGH_FOR_EVERY_COLUMN: Pixels = px(720.);

/// Whether a standalone view has room for every column.
///
/// Unmeasured reads as wide, and that is a considered guess rather than a
/// fallback: a fresh tab or window is nearly always wider than the breakpoint,
/// so guessing the other way would flash the narrow layout on the first frame of
/// the common case. The wrong guess costs one frame either way.
pub(crate) fn has_room_for_every_column(width: Option<Pixels>) -> bool {
    width.is_none_or(|width| width >= WIDE_ENOUGH_FOR_EVERY_COLUMN)
}

/// This view emits nothing to its pane.
///
/// An empty enum rather than `()`: it cannot be constructed, so it says at the
/// type level that there is no event here, instead of leaving a unit event
/// somebody might later think means something.
pub enum ContainerItemEvent {}

impl EventEmitter<ContainerItemEvent> for ContainerPanel {}

impl Item for ContainerPanel {
    type Event = ContainerItemEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        "Containers".into()
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<ui::Icon> {
        Some(ui::Icon::new(IconName::Docker))
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        Some("Containers Opened")
    }
}

impl ContainerPanel {
    /// A view for a tab or a window of its own, with a backend of its own.
    ///
    /// Takes nothing from the workspace, unlike the column's constructor: both
    /// callers run inside `register_action`, which leases the workspace for the
    /// whole handler, and the floating window builds its view in a *different*
    /// window's context where a second `&Workspace` cannot be borrowed alongside
    /// the `App` that creates the entity.
    /// A view carrying an engine choice made elsewhere.
    ///
    /// Used by the floating window, which cannot reach a `&Workspace` in the
    /// context that builds it.
    pub fn standalone(
        backends: Vec<Arc<dyn container::ContainerBackend>>,
        active: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self::build(backends, cx);
        panel.active_backend = active.min(panel.backends.len().saturating_sub(1));
        panel.reload(cx);
        panel.start_watching(cx);
        panel
    }

    /// Whether the list shows every column, by the width measured last frame.
    pub(crate) fn shows_every_column(&self) -> bool {
        has_room_for_every_column(self.measured_width)
    }

    pub(crate) fn note_measured_width(&mut self, width: Pixels, cx: &mut Context<Self>) {
        // Only when the answer changes, not on every frame: this runs from a
        // prepaint and a `notify` per frame would be a redraw loop.
        let was = self.shows_every_column();
        self.measured_width = Some(width);
        if self.shows_every_column() != was {
            cx.notify();
        }
    }
}

/// The engines a new view should offer, and which one it starts on.
///
/// Inherited from a container tab that is already open, when there is one, so a
/// second window does not arrive on a different engine from the tab it was
/// opened beside. Falls back to the full list otherwise.
fn backends_for_a_new_view(
    workspace: &Workspace,
    cx: &App,
) -> (Vec<Arc<dyn container::ContainerBackend>>, usize) {
    workspace
        .items_of_type::<ContainerPanel>(cx)
        .next()
        .map(|view| {
            let view = view.read(cx);
            (view.backends.clone(), view.active_backend)
        })
        .unwrap_or_else(|| (crate::container_panel::engines_for_a_new_view(cx), 0))
}

/// Brings the container tab forward, or opens one.
///
/// Existing tab first: a second tab would be a second list of the same thing,
/// each with its own engine choice and its own listener on `docker events`.
pub(crate) fn open(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    // Collected before the mutable borrow: `items_of_type` borrows the workspace
    // and `activate_item` needs it back.
    let existing = workspace.items_of_type::<ContainerPanel>(cx).next();
    if let Some(existing) = existing {
        workspace.activate_item(&existing, true, true, window, cx);
        return;
    }
    open_in_editor_tab(workspace, window, cx);
}

/// Steps back to the tab that was being read, if the container tab is the one in
/// front.
///
/// Put away, not closed: closing would drop the engine choice and kill the
/// listener over the second press of a button whose whole job is to be pressed
/// twice. Copied in shape from `AgentView::put_away`, which is the behaviour the
/// rail's other buttons already have.
pub(crate) fn put_away(workspace: &Workspace, window: &mut Window, cx: &mut App) -> bool {
    let pane = workspace.active_pane().clone();

    // The whole read is taken through one borrow: reaching for the pane again
    // partway through is how this repo has previously turned a stale read into
    // an abort.
    let previous = {
        let pane = pane.read(cx);
        let showing_containers = pane
            .active_item()
            .and_then(|item| item.downcast::<ContainerPanel>())
            .is_some();
        if !showing_containers {
            return false;
        }
        // The last entry is the tab just activated -- `Pane::update_history`
        // dedupes then pushes -- so the one before it is where the press goes.
        // Entries are walked past rather than trusted: an item closed since
        // leaves its own behind.
        pane.activation_history()
            .iter()
            .rev()
            .skip(1)
            .find_map(|entry| {
                pane.items()
                    .position(|item| item.item_id() == entry.entity_id)
            })
    };

    // Nothing to step back to: a pane holding only this stays exactly as it is.
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
    let (backends, active) = backends_for_a_new_view(workspace, cx);
    let handle = cx.weak_entity();
    let view = cx.new(|cx| {
        let mut panel = ContainerPanel::standalone(backends, active, cx);
        // A tab lives in this workspace, so it can open terminals and show
        // confirmations here. The floating window below cannot -- see
        // `ContainerPanel::workspace`.
        panel.workspace = Some(handle);
        panel
    });
    workspace.add_item_to_active_pane(Box::new(view), None, true, window, cx);
}

pub(crate) fn open_in_floating_window(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    // Read before `open_window` -- that closure runs in a *different* window's
    // context, where a second `&Workspace` cannot be borrowed alongside the `App`
    // creating the entity.
    let (backends, active) = backends_for_a_new_view(workspace, cx);
    let _ = window;

    // `AlwaysOnTop`, for the reasons `database_ui::standalone` paid to learn:
    // `PopUp` never takes keyboard focus, `Floating` is absent from window
    // cycling and Mission Control so there is no way back to it but the mouse,
    // and `Normal` sits under whatever you switch to. `AlwaysOnTop` is an
    // ordinary window at a floating level -- findable *and* raised. On Linux and
    // Windows it degrades to an ordinary window.
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(900.), px(600.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Containers".into()),
            ..Default::default()
        }),
        kind: WindowKind::AlwaysOnTop,
        window_min_size: Some(size(px(420.), px(280.))),
        ..Default::default()
    };

    if let Err(error) = cx.open_window(options, |_window, cx| {
        cx.new(|cx| ContainerPanel::standalone(backends, active, cx))
    }) {
        log::error!("could not open the containers in a window of their own: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The breakpoint is inclusive and the unmeasured case shows every column.
    /// Both are decisions rather than details -- an exclusive comparison would
    /// make the layout at exactly the breakpoint depend on rounding, and an
    /// unmeasured narrow guess would flash on every wide tab.
    #[test]
    fn the_breakpoint_is_inclusive_and_unmeasured_shows_every_column() {
        assert!(
            has_room_for_every_column(None),
            "unmeasured must guess wide"
        );
        assert!(has_room_for_every_column(Some(
            WIDE_ENOUGH_FOR_EVERY_COLUMN
        )));
        assert!(has_room_for_every_column(Some(
            WIDE_ENOUGH_FOR_EVERY_COLUMN + px(1.)
        )));
        assert!(!has_room_for_every_column(Some(
            WIDE_ENOUGH_FOR_EVERY_COLUMN - px(1.)
        )));
        assert!(!has_room_for_every_column(Some(px(0.))));
    }
}
