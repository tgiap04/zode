//! The container list: what is running on this machine, in a tab beside the code.
//!
//! Two engines answer here and they are not the same shape -- see
//! [`container::ContainerBackend`]. Nothing in this crate branches on which
//! engine it is holding: it asks the backend which kinds and which actions it
//! offers, and draws that. The day a `if backend == Kubernetes` appears here,
//! that seam has been breached.

mod confirm_modal;
mod container_panel;
mod detail;
mod render;
mod standalone;
mod terminal;

pub use container_panel::ContainerPanel;
pub use standalone::WIDE_ENOUGH_FOR_EVERY_COLUMN;

use gpui::App;
use workspace::Workspace;

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        // Toggles the column, not the focus.
        //
        // `toggle_panel_focus` only closes anything when `close_panel_on_toggle`
        // is set, which is off by default -- so a second press of a lit rail
        // button would do nothing at all. What the button says it does is show
        // and hide a column, so that is what it does.
        //
        // Hidden rather than closed: `set_open(false)` leaves the panel entity
        // in the dock, so the list it had is still there when it comes back.
        // What the rail button does, and why it is not simply an open: the
        // button is a toggle, and a lit toggle that does nothing when pressed is
        // the whole complaint. Same shape as the agent buttons beside it.
        //
        // Put away rather than closed: closing would drop the engine choice and
        // kill the listener over the second press of a button whose whole job is
        // to be pressed twice.
        workspace.register_action(
            |workspace, _: &zed_actions::container::ToggleContainer, window, cx| {
                if standalone::put_away(workspace, window, cx) {
                    return;
                }
                standalone::open(workspace, window, cx);
            },
        );
        // A second tab, deliberately, where `ToggleContainer` brings the first
        // one forward. Two lists of the same thing side by side is a reasonable
        // thing to want; arriving at one by accident is not.
        workspace.register_action(
            |workspace, _: &zed_actions::container::OpenInEditorTab, window, cx| {
                standalone::open_in_editor_tab(workspace, window, cx);
            },
        );
        workspace.register_action(
            |workspace, _: &zed_actions::container::OpenInFloatingWindow, window, cx| {
                standalone::open_in_floating_window(workspace, window, cx);
            },
        );
    })
    .detach();
}

/// Engines every newly opened view is built over, when something has replaced
/// them.
///
/// Test-only, and for a defect worth naming: opening the tab asks a real engine,
/// so a test that opens the tab runs `docker`. That child's exit wakes the GPUI
/// test scheduler from a blocking thread, and the scheduler aborts the run as
/// nondeterministic -- which it did, on two different tests, in two consecutive
/// full runs. A test that presses the button wants to know the button works, not
/// what is installed on the machine running it.
#[cfg(any(test, feature = "test-support"))]
pub(crate) struct EnginesForTest(pub(crate) Vec<std::sync::Arc<dyn container::ContainerBackend>>);

#[cfg(any(test, feature = "test-support"))]
impl gpui::Global for EnginesForTest {}

/// Builds every tab and window opened from here over engines that never leave
/// the process.
///
/// Deliberately not applied to `ContainerPanel::default_backends`, which stays
/// real so the test that checks the real engine list still checks the real one.
#[cfg(any(test, feature = "test-support"))]
pub fn use_fake_engines_for_test(cx: &mut App) {
    use container::fake_backend::FakeBackend;
    use std::sync::Arc;

    cx.set_global(EnginesForTest(vec![
        Arc::new(FakeBackend::docker()),
        Arc::new(FakeBackend::empty(
            container::BackendKind::Podman,
            &[container::ResourceKind::Container],
        )),
        Arc::new(FakeBackend::empty(
            container::BackendKind::Kubernetes,
            &[container::ResourceKind::Pod],
        )),
    ]));
}

#[cfg(test)]
mod container_panel_tests;
