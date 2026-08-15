//! The database column: a connection tree, a SQL scratch buffer and a result
//! grid, standing between the rail's tool dock and the editor.
//!
//! Everything it knows about a database arrives over the driver protocol in
//! [`database`]. Nothing here names an engine -- a driver formats its own
//! values and describes its own schema, so adding one costs this crate nothing.
//! That is the invariant phase 07 checks by bringing PostgreSQL up without
//! touching a line in here.

mod connection_modal;
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

pub use connection_store::{ConnectionConfig, DatabaseSettings};
pub use database_panel::DatabasePanel;

use connection_modal::ConnectionModal;

use gpui::App;
use workspace::Workspace;
use workspace::dock::DockColumn;

pub fn init(cx: &mut App) {
    driver_registry::init(cx);
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        // Toggles the column, not the focus.
        //
        // `toggle_panel_focus` puts focus in the panel and only closes anything
        // when `close_panel_on_toggle` is set, which is off by default -- so a
        // second press of a lit rail button did nothing at all. What the button
        // says it does is show and hide a column, so that is what it does.
        //
        // Hidden rather than closed: `set_open(false)` leaves the panel entity
        // in the dock, so the tree, the scratch buffer and any open session are
        // still there when it comes back.
        workspace.register_action(
            |workspace, _: &zed_actions::database::ToggleDatabase, window, cx| {
                let Some(dock) = workspace.dock_for_column(DockColumn::Database).cloned() else {
                    return;
                };
                let showing = dock.read(cx).is_open();
                dock.update(cx, |dock, cx| {
                    if showing {
                        dock.set_open(false, window, cx);
                    } else {
                        dock.set_open(true, window, cx);
                    }
                });
                if !showing {
                    workspace.focus_panel::<DatabasePanel>(window, cx);
                }
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
