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
mod entries;
mod host;
mod panel_item;
mod render;
mod split;

pub use host::FloatingPane;

use gpui::{App, AppContext as _, Context, Window};
use workspace::{SplitDirection, Workspace};

impl FloatingPane {
    /// Splits the active pane in the given direction, or does nothing.
    ///
    /// The guard is the whole reason this exists apart from `split_off`: a
    /// closed window has no pane worth splitting, and a window with nothing
    /// in any pane would end up showing two blank halves instead of the menu
    /// that tells somebody what the window can hold.
    pub(crate) fn split_active(
        &mut self,
        direction: SplitDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.open || self.is_empty(cx) {
            return;
        }
        self.split_off(&self.active_pane.clone(), direction, window, cx);
    }
}

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
        // `Pane::new` was handed this as the double-click action, so the pane
        // has been advertising it on empty tab-bar space with nothing behind
        // it. Registered here rather than dropped: a double-click on a tab bar
        // opening a new tab is what every other tab bar in the editor does.
        workspace.register_action({
            let view = view.downgrade();
            move |_workspace, _: &zed_actions::floating_pane::NewTerminal, window, cx| {
                view.update(cx, |this, cx| this.open_terminal(window, cx))
                    .ok();
            }
        });
        // Declared alongside `NewTerminal` in `zed_actions` but never wired to
        // anything -- reachable only through the `+` menu's own click handler.
        // `NewMarkdownNote` and `OpenMarkdownNote` for a keybinding to bind
        // under the `FloatingPane` context, the same way every other action
        // here is.
        workspace.register_action({
            let view = view.downgrade();
            move |_workspace, _: &zed_actions::floating_pane::NewMarkdownNote, window, cx| {
                view.update(cx, |this, cx| this.new_markdown_note(window, cx))
                    .ok();
            }
        });
        workspace.register_action({
            let view = view.downgrade();
            move |_workspace, _: &zed_actions::floating_pane::OpenMarkdownNote, window, cx| {
                view.update(cx, |this, cx| this.open_markdown_note(window, cx))
                    .ok();
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
        // Four window-scoped names for the one path `split_active` already
        // guards: `pane::SplitRight` et al reach the same `Event::Split` the
        // moment focus sits inside a pane, so these are not a second
        // implementation -- just a name a keymap can bind under the
        // `FloatingPane` context without touching the editor's own split
        // bindings, matching the three siblings above.
        workspace.register_action({
            let view = view.downgrade();
            move |_workspace, _: &zed_actions::floating_pane::SplitRight, window, cx| {
                view.update(cx, |this, cx| {
                    this.split_active(SplitDirection::Right, window, cx)
                })
                .ok();
            }
        });
        workspace.register_action({
            let view = view.downgrade();
            move |_workspace, _: &zed_actions::floating_pane::SplitLeft, window, cx| {
                view.update(cx, |this, cx| {
                    this.split_active(SplitDirection::Left, window, cx)
                })
                .ok();
            }
        });
        workspace.register_action({
            let view = view.downgrade();
            move |_workspace, _: &zed_actions::floating_pane::SplitUp, window, cx| {
                view.update(cx, |this, cx| {
                    this.split_active(SplitDirection::Up, window, cx)
                })
                .ok();
            }
        });
        workspace.register_action({
            let view = view.downgrade();
            move |_workspace, _: &zed_actions::floating_pane::SplitDown, window, cx| {
                view.update(cx, |this, cx| {
                    this.split_active(SplitDirection::Down, window, cx)
                })
                .ok();
            }
        });
    })
    .detach();
}

#[cfg(test)]
mod floating_pane_tests;
