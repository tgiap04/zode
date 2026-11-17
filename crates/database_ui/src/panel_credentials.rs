//! The connection's context menu, and the password box it can open.
//!
//! Split from `panel_connections.rs` because it answers a different question:
//! that file is about walking a database, this one is about being allowed to.

use crate::database_panel::DatabasePanel;
use crate::password_prompt::PasswordPrompt;
use crate::safe_mode::SafetyLevel;
use crate::tree_menus::icon_entry;
use gpui::{Pixels, Point, Window};
use ui::{IconPosition, prelude::*};

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
        // Team connections are not in settings, so neither pinned by name nor
        // shared again from here.
        let personal = node.is_personal();
        let shareable = personal && crate::team_connections::can_share(cx);
        let safety = self.safety_of(index, cx);
        let panel = cx.weak_entity();

        self.open_context_menu(position, window, cx, move |menu, _window, _cx| {
            let (pin_label, pin_icon) = if pinned {
                ("Unpin from This Project", IconName::Unpin)
            } else {
                ("Pin to This Project", IconName::Pin)
            };
            menu.when(connected, |menu| {
                let menu = icon_entry(menu, "Refresh", IconName::RotateCw, {
                    let panel = panel.clone();
                    move |_window, cx| {
                        panel
                            .update(cx, |panel, cx| panel.refresh_connection(index, cx))
                            .ok();
                    }
                });
                icon_entry(menu, "New Query", IconName::SquarePlus, {
                    let panel = panel.clone();
                    move |window, cx| {
                        panel
                            .update(cx, |panel, cx| panel.new_query(index, window, cx))
                            .ok();
                    }
                })
                .separator()
            })
            .when(personal, |menu| {
                icon_entry(menu, pin_label, pin_icon, {
                    let panel = panel.clone();
                    let name = name.clone();
                    move |_window, cx| {
                        panel
                            .update(cx, |panel, cx| panel.toggle_pin(&name, cx))
                            .ok();
                    }
                })
                .separator()
            })
            .when(connected, |menu| {
                icon_entry(menu, "Disconnect", IconName::Disconnected, {
                    let panel = panel.clone();
                    move |_window, cx| {
                        panel
                            .update(cx, |panel, cx| panel.disconnect(index, cx))
                            .ok();
                    }
                })
            })
            .when(!connected, |menu| {
                icon_entry(menu, "Connect Again", IconName::Power, {
                    let panel = panel.clone();
                    move |window, cx| {
                        panel
                            .update(cx, |panel, cx| panel.reconnect(index, window, cx))
                            .ok();
                    }
                })
            })
            .separator()
            .map(|menu| {
                icon_entry(menu, "Set Password…", IconName::LockOutlined, {
                    let panel = panel.clone();
                    move |window, cx| {
                        panel
                            .update(cx, |panel, cx| panel.prompt_for_password(index, window, cx))
                            .ok();
                    }
                })
            })
            .submenu_with_icon("Safety", IconName::Warning, {
                let panel = panel.clone();
                move |mut submenu, _window, _cx| {
                    for level in SafetyLevel::ALL {
                        let panel = panel.clone();
                        submenu = submenu.toggleable_entry(
                            level.label(),
                            level == safety,
                            IconPosition::Start,
                            None,
                            move |_window, cx| {
                                panel
                                    .update(cx, |panel, cx| panel.set_safety(index, level, cx))
                                    .ok();
                            },
                        );
                    }
                    submenu
                }
            })
            .when(shareable, |menu| {
                icon_entry(menu.separator(), "Share with Team…", IconName::Public, {
                    let panel = panel.clone();
                    move |window, cx| {
                        panel
                            .update(cx, |panel, cx| panel.share_with_team(index, window, cx))
                            .ok();
                    }
                })
            })
            .when(personal, |menu| {
                icon_entry(menu.separator(), "Remove Connection…", IconName::Trash, {
                    let panel = panel.clone();
                    move |window, cx| {
                        panel
                            .update(cx, |panel, cx| {
                                panel.confirm_remove_connection(index, window, cx)
                            })
                            .ok();
                    }
                })
            })
        });
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
        // Held by identity: the box stays up while the list may be rebuilt, and
        // the password is for this connection, not for whichever one is at its
        // old place when the box closes.
        let Some(key) = self.node_key(index) else {
            return;
        };
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
                                .update(cx, |panel, cx| {
                                    if let Some(index) = panel.index_of_key(&key) {
                                        panel.reconnect(index, window, cx);
                                    }
                                })
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
