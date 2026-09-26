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

use gpui::{App, Context, WeakEntity};
use ui::IconName;
use workspace::Workspace;

/// Every engine a surface outside this crate may offer, in the order the
/// switcher shows them: the index `standalone_in_workspace` expects, its
/// glyph, and its label. Shaped like `agent_ui::agent_marks()` -- the pattern
/// `floating_pane` already consumes -- so a caller there never has to learn
/// the words "Docker" or "Kubernetes" for itself.
///
/// Always `ContainerPanel::default_backends`, never
/// `container_panel::engines_for_a_new_view`: this is read once to build a
/// menu's labels and glyphs, not to open a real view, so it must keep
/// answering with the real three engines even under a test that has swapped
/// the live list for fakes.
pub fn engine_marks() -> impl Iterator<Item = (usize, IconName, &'static str)> {
    ContainerPanel::default_backends()
        .into_iter()
        .enumerate()
        .map(|(index, backend)| {
            (
                index,
                engine_mark_icon(backend.kind()),
                backend.kind().label(),
            )
        })
}

/// The glyph for an engine, kept in step with `render::engine_icon` by hand.
///
/// A second copy rather than a shared one: the original is private to
/// `render.rs`, and duplicating a two-arm match costs less than widening that
/// module's own boundary to lend it out.
fn engine_mark_icon(kind: container::BackendKind) -> IconName {
    match kind {
        container::BackendKind::Docker => IconName::Docker,
        container::BackendKind::Podman | container::BackendKind::Kubernetes => IconName::Box,
    }
}

/// A standalone panel already attached to a workspace, for a host that cannot
/// pass one to `Workspace`'s own constructor path.
///
/// `ContainerPanel::standalone` (in `standalone.rs`) leaves `workspace` unset,
/// the same way `ContainerPanel::build` does -- its two callers each set the
/// field themselves afterwards, because one runs inside a leased `Workspace`
/// and the other, the floating window, builds its view in a different
/// window's context where a second `&Workspace` cannot be borrowed alongside
/// the `App` creating the entity. `workspace` is `pub(crate)`, so a caller
/// outside this crate needs this function to set it rather than reaching for
/// the field itself.
///
/// Reads through `container_panel::engines_for_a_new_view`, not
/// `default_backends` directly, so a test that opens a container tab through
/// this path is exercising the same swappable engine list `container_ui`'s
/// own tests already rely on -- not spawning a real `docker`.
pub fn standalone_in_workspace(
    engine: usize,
    workspace: WeakEntity<Workspace>,
    cx: &mut Context<ContainerPanel>,
) -> ContainerPanel {
    let backends = container_panel::engines_for_a_new_view(cx);
    let mut panel = ContainerPanel::standalone(backends, engine, cx);
    panel.workspace = Some(workspace);
    panel
}

/// Whether a panel was given a workspace to open terminals and confirmations
/// in.
///
/// Test-only, and exists only so a test outside this crate can assert the
/// "the floating window's panel must not have a `None` workspace" invariant
/// without this crate handing out read access to the field itself, which is
/// `pub(crate)` on purpose.
#[cfg(any(test, feature = "test-support"))]
pub fn has_workspace_for_test(panel: &ContainerPanel) -> bool {
    panel.workspace.is_some()
}

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
