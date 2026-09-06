//! The database as a tab: a connection tree, a SQL scratch buffer and a result
//! grid, beside the code.
//!
//! Everything it knows about a database arrives over the driver protocol in
//! [`database`]. Nothing here names an engine -- a driver formats its own
//! values and describes its own schema, so adding one costs this crate nothing.
//! That is the invariant phase 07 checks by bringing PostgreSQL up without
//! touching a line in here.

mod connection_modal;
mod connection_modal_download;
mod connection_modal_render;
mod connection_store;
mod connection_tree;
mod database_panel;
mod driver_registry;
mod panel_connections;
mod panel_credentials;
mod panel_layout;
mod panel_queries;
mod password_prompt;
mod query;
mod render;
mod result_grid;
mod session;
mod standalone;

pub use connection_store::{ConnectionConfig, DatabaseSettings};
pub use database_panel::DatabasePanel;
pub use standalone::SIDE_BY_SIDE_WIDTH;

use connection_modal::ConnectionModal;

use gpui::App;
use workspace::Workspace;

pub fn init(cx: &mut App) {
    driver_registry::init(cx);
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        // What the rail button does, and why it is not simply an open: the
        // button is a toggle, and a lit toggle that does nothing when pressed is
        // the whole complaint. Same shape as the agent buttons beside it.
        //
        // Put away rather than closed: closing would end every open session and
        // lose a half-written statement over the second press.
        workspace.register_action(
            |workspace, _: &zed_actions::database::ToggleDatabase, window, cx| {
                if standalone::put_away(workspace, window, cx) {
                    return;
                }
                standalone::open(workspace, window, cx);
            },
        );
        // A second tab, deliberately, where `ToggleDatabase` brings the first
        // forward; and a window of its own. Actions rather than plain click
        // handlers so buttons and keybindings reach the same code, and so neither
        // takes a shorter road that happens to work -- the note on `AddConnection`
        // below is why that matters here.
        workspace.register_action(
            |workspace, _: &zed_actions::database::OpenInEditorTab, window, cx| {
                standalone::open_in_editor_tab(workspace, window, cx);
            },
        );
        workspace.register_action(
            |workspace, _: &zed_actions::database::OpenInFloatingWindow, window, cx| {
                standalone::open_in_floating_window(workspace, window, cx);
            },
        );
        // The one place the dialog is opened, and it opens it from the
        // `&mut Workspace` this handler is already holding.
        //
        // `register_action` runs its callback inside `cx.listener`, so the
        // workspace is leased for the whole of it: anything reached from here
        // that takes a workspace handle and calls `update` on it aborts the
        // process. Both buttons dispatch this action rather than one of them
        // taking a shorter road that happens to work.
        workspace.register_action(
            |workspace, _: &zed_actions::database::AddConnection, window, cx| {
                let handle = cx.weak_entity();
                workspace.toggle_modal(window, cx, move |window, cx| {
                    ConnectionModal::new(handle, window, cx)
                });
            },
        );
    })
    .detach();
}

#[cfg(test)]
mod database_panel_tests;
