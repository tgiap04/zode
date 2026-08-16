//! The connection's context menu, and the password box it can open.
//!
//! Split from `panel_connections.rs` because it answers a different question:
//! that file is about walking a database, this one is about being allowed to.

use crate::database_panel::DatabasePanel;
use crate::password_prompt::PasswordPrompt;
use gpui::{DismissEvent, Focusable as _, Pixels, Point, Window};
use ui::{ContextMenu, prelude::*};

impl DatabasePanel {
    /// Opens the menu for a connection row.
    ///
    /// A menu rather than the bare right-click-to-pin this replaces: pinning is
    /// no longer the only thing worth doing to a connection, and a gesture that
    /// silently does one of several possible things is a gesture nobody trusts.
    pub(crate) fn deploy_connection_menu(
        &mut self,
        index: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self.connections.get(index) else {
            return;
        };
        let name = node.config.name.clone();
        let pinned = self.is_pinned(&name);
        let connected = self.is_connected(index);
        let panel = cx.weak_entity();

        let menu = ContextMenu::build(window, cx, move |menu, _window, _cx| {
            let pin_label = if pinned {
                "Unpin from This Project"
            } else {
                "Pin to This Project"
            };
            menu.entry(pin_label, None, {
                let panel = panel.clone();
                let name = name.clone();
                move |_window, cx| {
                    panel
                        .update(cx, |panel, cx| panel.toggle_pin(&name, cx))
                        .ok();
                }
            })
            .separator()
            .when(connected, |menu| {
                menu.entry("Disconnect", None, {
                    let panel = panel.clone();
                    move |_window, cx| {
                        panel
                            .update(cx, |panel, cx| panel.disconnect(index, cx))
                            .ok();
                    }
                })
            })
            .when(!connected, |menu| {
                menu.entry("Connect Again", None, {
                    let panel = panel.clone();
                    move |window, cx| {
                        panel
                            .update(cx, |panel, cx| panel.reconnect(index, window, cx))
                            .ok();
                    }
                })
            })
            .separator()
            .entry("Set Password…", None, {
                let panel = panel.clone();
                move |window, cx| {
                    panel
                        .update(cx, |panel, cx| panel.prompt_for_password(index, window, cx))
                        .ok();
                }
            })
        });

        cx.focus_view(&menu, window);
        let subscription =
            cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, window, cx| {
                if this
                    .context_menu
                    .as_ref()
                    .is_some_and(|(menu, _, _)| menu.focus_handle(cx).contains_focused(window, cx))
                {
                    cx.focus_self(window);
                }
                this.context_menu.take();
                cx.notify();
            });

        self.context_menu = Some((menu, position, subscription));
        cx.notify();
    }

    /// Asks for a password and puts it in the keychain.
    ///
    /// Reached two ways, and both matter: deliberately, before a first
    /// connection, and automatically when a driver reports that credentials
    /// were rejected -- which is the moment someone actually has the password
    /// in mind.
    pub(crate) fn prompt_for_password(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self.connections.get(index) else {
            return;
        };
        let config = node.config.clone();
        let panel = cx.weak_entity();

        self.workspace
            .update(cx, |workspace, cx| {
                workspace.toggle_modal(window, cx, move |window, cx| {
                    PasswordPrompt::new(
                        config.name.clone().into(),
                        config.credential_key().to_string(),
                        config.name.clone(),
                        Box::new(move |window, cx| {
                            panel
                                .update(cx, |panel, cx| panel.reconnect(index, window, cx))
                                .ok();
                        }),
                        window,
                        cx,
                    )
                });
            })
            .ok();
    }
}
