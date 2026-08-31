//! A window that floats over the editor, holding tabs of its own.
//!
//! A button sits in the bottom-right corner of the workspace. Pressing it opens
//! a small window over the code -- draggable, resizable, and holding whatever
//! you put in it: a terminal, an agent thread, a markdown note.
//!
//! It holds a real [`workspace::Pane`], not a tab strip written here. Tabs,
//! their close buttons, the activation history, dragging a tab out -- all of it
//! already exists and already behaves the way the editor's tabs behave. Writing
//! a second tab strip would mean writing those again and then watching the two
//! drift apart. Everything it can hold is already a `workspace::Item`:
//! `TerminalView`, `AgentView` and `Editor`.
//!
//! Why a floating window rather than another dock: a dock takes width from the
//! code for as long as it is open. This one is meant for the thing you need for
//! a minute -- a command, a question, a note -- and then dismiss.

mod content;
mod host;
mod render;

pub use host::FloatingPane;

use gpui::{App, AppContext as _};
use workspace::Workspace;

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        // `observe_new` runs once without a window while the workspace is being
        // restored; there is nothing to build a pane in yet.
        let Some(window) = window else {
            return;
        };
        // The project is read here, where the workspace is already borrowed, and
        // handed in. Reading it back through the handle inside `cx.new` would
        // borrow the workspace a second time while this observer still holds it,
        // which aborts the process -- a trap this repo has paid for before.
        let handle = cx.weak_entity();
        let project = workspace.project().clone();
        let view = cx.new(|cx| FloatingPane::new(handle, project, window, cx));
        workspace.register_floating_layer(view.clone(), cx);

        // Actions rather than plain click handlers, so a keybinding and a button
        // reach the same code and cannot drift apart.
        workspace.register_action({
            let view = view.downgrade();
            move |_workspace, _: &zed_actions::floating_pane::ToggleFloatingPane, window, cx| {
                view.update(cx, |this, cx| this.toggle(window, cx)).ok();
            }
        });
        // Through `confirm_shut_down` rather than `shut_down`, so the keystroke
        // asks the same question the button does. A shortcut that ends running
        // terminals without a word would be the one place in this feature where
        // the keyboard is more dangerous than the mouse.
        workspace.register_action({
            let view = view.downgrade();
            move |_workspace, _: &zed_actions::floating_pane::CloseFloatingPane, window, cx| {
                view.update(cx, |this, cx| this.confirm_shut_down(window, cx))
                    .ok();
            }
        });
    })
    .detach();
}

#[cfg(test)]
mod floating_pane_tests;
