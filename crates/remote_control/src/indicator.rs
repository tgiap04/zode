//! The status bar item that says a device is in control, and the one place
//! the person at this screen is asked about a pairing.
//!
//! It is registered outside the table of items the user may hide, on purpose:
//! while a device is connected there must be no setting that makes this
//! disappear, which includes `status_bar.show`: the status bar is kept on
//! screen for as long as a device is in control.

use gpui::{Anchor, App, Entity, Subscription, WeakEntity, Window};
use ui::{ButtonLike, ContextMenu, PopoverMenu, Tooltip, prelude::*};
use util::ResultExt as _;
use workspace::{MultiWorkspace, StatusItemView, Toast, Workspace, notifications::NotificationId};

use crate::{
    pairing_modal::PairingModal,
    remote_host::{RemoteHost, RemoteHostEvent, SessionInfo},
    trusted_devices_modal::TrustedDevicesModal,
};

struct SessionStartedToast;
struct ProblemToast;

pub struct RemoteControlIndicator {
    host: Entity<RemoteHost>,
    workspace: WeakEntity<Workspace>,
    _subscriptions: Vec<Subscription>,
}

/// "Controlled by Ada's laptop", "Controlled by 2 devices".
pub(crate) fn controlled_by(sessions: &[SessionInfo]) -> String {
    match sessions {
        [] => String::new(),
        [only] if only.device_name.is_empty() => "Controlled by a device".to_string(),
        [only] => format!("Controlled by {}", only.device_name),
        several => format!("Controlled by {} devices", several.len()),
    }
}

/// Whether the window that is looking at this decides it is the one to speak.
///
/// Every window has an indicator and hears every event, but a toast should
/// appear once: in the window somebody is looking at, or, when Zode is in the
/// background, in the workspace window used most recently, so a device taking
/// control is not missed just because the person is in another application.
fn is_toast_window<H: PartialEq>(
    this: &H,
    this_is_active: bool,
    app_has_active_window: bool,
    stack: Option<&[H]>,
    is_workspace_window: impl Fn(&H) -> bool,
) -> bool {
    if this_is_active {
        return true;
    }
    if app_has_active_window {
        return false;
    }
    stack
        .and_then(|stack| stack.iter().find(|handle| is_workspace_window(handle)))
        .is_some_and(|frontmost| frontmost == this)
}

fn shows_toast(window: &Window, cx: &App) -> bool {
    let stack = cx.window_stack();
    is_toast_window(
        &window.window_handle(),
        window.is_window_active(),
        cx.active_window().is_some(),
        stack.as_deref(),
        |handle| handle.downcast::<MultiWorkspace>().is_some(),
    )
}

impl RemoteControlIndicator {
    pub fn new(
        host: Entity<RemoteHost>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            cx.observe_in(&host, window, |this, _host, window, cx| {
                this.present_pairing_if_needed(window, cx);
                this.keep_status_bar_visible(cx);
                cx.notify();
            }),
            cx.subscribe_in(
                &host,
                window,
                |this, _host, event: &RemoteHostEvent, window, cx| match event {
                    RemoteHostEvent::SessionStarted { device_name } => {
                        this.announce(device_name, window, cx);
                    }
                    RemoteHostEvent::Problem { message, retry } => {
                        this.report_problem(message, *retry, window, cx);
                    }
                    RemoteHostEvent::Changed => {}
                },
            ),
            cx.observe_window_activation(window, |this, window, cx| {
                this.present_pairing_if_needed(window, cx);
            }),
        ];
        // The workspace is still being built when this runs, so it cannot be
        // updated until that is done.
        cx.defer_in(window, |this, _window, cx| this.keep_status_bar_visible(cx));
        Self {
            host,
            workspace,
            _subscriptions: subscriptions,
        }
    }

    /// While a device is in control the status bar stays, whatever
    /// `status_bar.show` says: this indicator lives in it.
    fn keep_status_bar_visible(&mut self, cx: &mut Context<Self>) {
        let forced = !self.host.read(cx).sessions().is_empty();
        self.workspace
            .update(cx, |workspace, cx| {
                workspace.set_status_bar_forced_visible(forced, cx);
            })
            .log_err();
    }

    /// Once, in one window: a device just took control.
    fn announce(&mut self, device_name: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !shows_toast(window, cx) {
            return;
        }
        let message = if device_name.is_empty() {
            "A device is now controlling this Zode".to_string()
        } else {
            format!("{device_name} is now controlling this Zode")
        };
        self.workspace
            .update(cx, |workspace, cx| {
                workspace.show_toast(
                    Toast::new(NotificationId::unique::<SessionStartedToast>(), message),
                    cx,
                );
            })
            .log_err();
    }

    /// Tells the person that remote control stopped or could not do something.
    fn report_problem(
        &mut self,
        message: &str,
        retry: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !shows_toast(window, cx) {
            return;
        }
        let host = self.host.clone();
        let message = message.to_string();
        self.workspace
            .update(cx, |workspace, cx| {
                let toast = Toast::new(NotificationId::unique::<ProblemToast>(), message);
                let toast = if retry {
                    toast.on_click("Try again", move |_window, cx| {
                        host.update(cx, |host, cx| host.retry(cx));
                    })
                } else {
                    toast
                };
                workspace.show_toast(toast, cx);
            })
            .log_err();
    }

    fn present_pairing_if_needed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pending) = self.host.read(cx).pending_pairing().cloned() else {
            return;
        };
        if !window.is_window_active() {
            return;
        }
        let session_id = pending.session_id;
        if !self
            .host
            .update(cx, |host, _| host.claim_pairing_presentation(session_id))
        {
            return;
        }
        let host = self.host.clone();
        self.workspace
            .update(cx, |workspace, cx| {
                workspace.toggle_modal(window, cx, move |window, cx| {
                    PairingModal::new(host, pending, window, cx)
                });
            })
            .log_err();
    }

    fn menu(
        host: Entity<RemoteHost>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<ContextMenu> {
        ContextMenu::build(window, cx, move |menu, _window, _cx| {
            let disconnect_host = host.clone();
            let manage_host = host.clone();
            menu.entry("Disconnect all devices", None, move |_window, cx| {
                disconnect_host.update(cx, |host, cx| host.disconnect_all(cx));
            })
            .entry("Turn off remote control", None, |_window, cx| {
                crate::disable_remote_control(cx);
            })
            .separator()
            .entry("Manage trusted devices...", None, move |window, cx| {
                let host = manage_host.clone();
                workspace
                    .update(cx, |workspace, cx| {
                        workspace.toggle_modal(window, cx, move |window, cx| {
                            TrustedDevicesModal::new(host, window, cx)
                        });
                    })
                    .log_err();
            })
        })
    }
}

impl Render for RemoteControlIndicator {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sessions = self.host.read(cx).sessions();
        let label = controlled_by(&sessions);
        let host = self.host.clone();
        let workspace = self.workspace.clone();

        // Empty, not absent, when nobody is connected: the item stays in the bar
        // so it can never be the thing a settings toggle forgot to bring back.
        div().when(!sessions.is_empty(), |this| {
            this.child(
                PopoverMenu::new("remote-control-indicator")
                    .menu(move |window, cx| {
                        Some(Self::menu(host.clone(), workspace.clone(), window, cx))
                    })
                    .anchor(Anchor::BottomRight)
                    .trigger_with_tooltip(
                        ButtonLike::new("remote-control-trigger")
                            .style(ButtonStyle::Subtle)
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(
                                        Icon::new(IconName::Screen)
                                            .size(IconSize::Small)
                                            .color(Color::Warning),
                                    )
                                    .child(
                                        Label::new(label.clone())
                                            .size(LabelSize::Small)
                                            .color(Color::Warning),
                                    ),
                            ),
                        Tooltip::text(format!("{label}. Click to disconnect or turn it off.")),
                    ),
            )
        })
    }
}

impl StatusItemView for RemoteControlIndicator {
    /// Nothing to do: who is in control does not depend on the tab in front.
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn workspace::ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(name: &str) -> SessionInfo {
        SessionInfo {
            session_id: 1,
            device_id: "d".into(),
            device_name: name.into(),
        }
    }

    #[test]
    fn the_label_names_the_device_or_counts_them() {
        assert_eq!(controlled_by(&[]), "");
        assert_eq!(
            controlled_by(&[session("Ada's laptop")]),
            "Controlled by Ada's laptop"
        );
        assert_eq!(controlled_by(&[session("")]), "Controlled by a device");
        assert_eq!(
            controlled_by(&[session("a"), session("b"), session("c")]),
            "Controlled by 3 devices"
        );
    }

    #[test]
    fn the_toast_goes_to_the_active_window_or_else_the_frontmost_workspace_window() {
        let is_workspace = |handle: &u32| *handle != 99;
        // The window somebody is looking at speaks.
        assert!(is_toast_window(&2, true, true, Some(&[1, 2]), is_workspace));
        // Another window of this app is the active one: stay quiet.
        assert!(!is_toast_window(
            &1,
            false,
            true,
            Some(&[2, 1]),
            is_workspace
        ));
        // Zode is in the background: the frontmost workspace window speaks,
        // skipping a window that is not one.
        assert!(is_toast_window(
            &1,
            false,
            false,
            Some(&[99, 1, 2]),
            is_workspace
        ));
        assert!(!is_toast_window(
            &2,
            false,
            false,
            Some(&[99, 1, 2]),
            is_workspace
        ));
        // No ordering from the platform: nobody guesses.
        assert!(!is_toast_window(&1, false, false, None, is_workspace));
    }
}
