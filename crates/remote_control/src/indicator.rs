//! The status bar item that says a device is in control, and the one place
//! the person at this screen is asked about a pairing.
//!
//! It is registered outside the table of items the user may hide, on purpose:
//! while a device is connected there must be no setting that makes this
//! disappear, which includes `status_bar.show`: the status bar is kept on
//! screen for as long as a device is in control.

use gpui::{Anchor, AnyElement, App, Entity, Notification, Subscription, WeakEntity, Window};
use ui::{ButtonLike, ContextMenu, PopoverMenu, Tooltip, prelude::*};
use util::ResultExt as _;
use workspace::{MultiWorkspace, StatusItemView, Toast, Workspace, notifications::NotificationId};

use crate::{
    pairing_flow::PendingDecision,
    pairing_modal::PairingModal,
    qr_pairing_modal,
    remote_host::{HostState, RemoteHost, RemoteHostEvent, SessionInfo, problem_message},
    trusted_devices_modal::TrustedDevicesModal,
};

struct SessionStartedToast;
struct DevicePairedToast;
struct ProblemToast;

pub struct RemoteControlIndicator {
    host: Entity<RemoteHost>,
    workspace: WeakEntity<Workspace>,
    _subscriptions: Vec<Subscription>,
}

/// Whether stopping in `state` is something a larger plan answers. Only the
/// relay's daily allowance is; every other stop has a different way out.
pub(crate) fn upgrade_offered(state: &HostState) -> bool {
    matches!(
        state,
        HostState::Halted(remote_relay_client::StopReason::QuotaExceeded)
    )
}

/// What the indicator adds while pushes are refused for want of a plan.
pub(crate) const PUSH_NEEDS_PRO: &str = "Push notifications to your devices need Pro.";

/// [`idle_status`], with a word about pushes when the plan stopped them.
pub(crate) fn idle_status_with_push(
    state: &HostState,
    push_needs_upgrade: bool,
) -> Option<(String, Color)> {
    let (tooltip, color) = idle_status(state)?;
    if !push_needs_upgrade {
        return Some((tooltip, color));
    }
    let color = if color == Color::Muted {
        Color::Warning
    } else {
        color
    };
    Some((format!("{tooltip} {PUSH_NEEDS_PRO}"), color))
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

/// What the item says while remote control is on and nobody is connected:
/// a tooltip and how loudly to colour the icon. `None` when it is off, so
/// nothing shows.
pub(crate) fn idle_status(state: &HostState) -> Option<(String, Color)> {
    let status = match state {
        HostState::Disabled => return None,
        HostState::Online => (
            "Remote control is on. Your paired devices can connect.".to_string(),
            Color::Muted,
        ),
        HostState::Starting => (
            "Remote control is starting\u{2026}".to_string(),
            Color::Muted,
        ),
        HostState::Reconnecting => (
            "Remote control lost the relay and is reconnecting\u{2026}".to_string(),
            Color::Warning,
        ),
        HostState::Failed(reason) => (
            format!("Remote control could not start: {reason}"),
            Color::Error,
        ),
        HostState::SignedOut => (problem_message(state)?, Color::Warning),
        HostState::Halted(_) => (problem_message(state)?, Color::Error),
    };
    Some(status)
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

/// Puts the question a device is waiting on in `workspace`, unless it is
/// already there. `false` when nothing is waiting, or when another modal that
/// cannot be dismissed keeps its place: the question is then asked for again
/// the next time the host changes or a window comes forward, rather than being
/// counted as shown.
pub(crate) fn open_pairing_question(
    host: Entity<RemoteHost>,
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> bool {
    let Some(pending) = host.read(cx).pending_pairing().cloned() else {
        return false;
    };
    let session_id = pending.session_id;
    let shows_it = |workspace: &Workspace, cx: &App| {
        workspace
            .active_modal::<PairingModal>(cx)
            .is_some_and(|modal| modal.read(cx).session_id() == session_id)
    };
    if !shows_it(workspace, cx)
        && let Some(elsewhere) = window_showing_question(session_id, window.window_handle(), cx)
    {
        // A second copy would be a second way to turn the device away: closing
        // either one dismisses the request. The open one is brought forward.
        cx.defer(move |cx| {
            elsewhere
                .update(cx, |_, window, _| window.activate_window())
                .log_err();
        });
        return true;
    }
    if !shows_it(workspace, cx) {
        let modal_host = host.clone();
        workspace.toggle_modal(window, cx, move |window, cx| {
            PairingModal::new(modal_host, pending, window, cx)
        });
    }
    let on_screen = shows_it(workspace, cx);
    if on_screen {
        host.update(cx, |host, _| host.claim_pairing_presentation(session_id));
    }
    on_screen
}

/// The workspace window, other than `except`, whose modal is the question for
/// `session_id`.
fn window_showing_question(
    session_id: u32,
    except: gpui::AnyWindowHandle,
    cx: &App,
) -> Option<gpui::WindowHandle<MultiWorkspace>> {
    cx.windows()
        .into_iter()
        .filter(|handle| *handle != except)
        .filter_map(|handle| handle.downcast::<MultiWorkspace>())
        .find(|handle| {
            handle.read(cx).is_ok_and(|multi| {
                multi
                    .workspace()
                    .read(cx)
                    .active_modal::<PairingModal>(cx)
                    .is_some_and(|modal| modal.read(cx).session_id() == session_id)
            })
        })
}

const PAIRING_NOTIFICATION_PREFIX: &str = "remote-control-pairing-";

pub(crate) fn is_pairing_notification(id: &str) -> bool {
    id.strip_prefix(PAIRING_NOTIFICATION_PREFIX)
        .is_some_and(|session| session.parse::<u32>().is_ok())
}

/// What the OS shows when somebody asks to pair while Zode is in the
/// background. The id is how a click is told apart from the agent
/// notifications', which share the platform's one click handler.
fn pairing_notification(session_id: u32, device_name: &str) -> Notification {
    let device_name = device_name.trim();
    let title = if device_name.is_empty() {
        "A device wants to control this Zode".to_string()
    } else {
        format!("{device_name} wants to control this Zode")
    };
    Notification {
        id: format!("{PAIRING_NOTIFICATION_PREFIX}{session_id}").into(),
        title: title.into(),
        body: "Switch to Zode to compare the six digits.".into(),
        awaits_user: true,
    }
}

/// Whether a window that can put the pairing question on screen is in front.
/// Only workspace windows carry an indicator, so an active window of another
/// kind is no reason to stay quiet.
fn a_workspace_window_is_active(cx: &mut App) -> bool {
    cx.windows().into_iter().any(|handle| {
        handle
            .downcast::<MultiWorkspace>()
            .and_then(|window| window.is_active(cx))
            .unwrap_or(false)
    })
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
                    RemoteHostEvent::DevicePaired { device_name } => {
                        this.announce_paired(device_name, window, cx);
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

    /// Once, in one window: a phone read the QR code and is now trusted.
    fn announce_paired(&mut self, device_name: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !shows_toast(window, cx) {
            return;
        }
        let message = if device_name.is_empty() {
            "A device is now paired with this Zode".to_string()
        } else {
            format!("{device_name} is now paired with this Zode")
        };
        self.workspace
            .update(cx, |workspace, cx| {
                workspace.show_toast(
                    Toast::new(NotificationId::unique::<DevicePairedToast>(), message),
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
        let upgrade = upgrade_offered(self.host.read(cx).state());
        self.workspace
            .update(cx, |workspace, cx| {
                let toast = Toast::new(NotificationId::unique::<ProblemToast>(), message);
                let toast = if retry {
                    toast.on_click("Try again", move |_window, cx| {
                        host.update(cx, |host, cx| host.retry(cx));
                    })
                } else if upgrade {
                    toast.on_click("Upgrade\u{2026}", |_window, cx| {
                        cx.open_url(&zode_account::pricing_url());
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
            self.call_back_about_pairing(&pending, cx);
            return;
        }
        if self.host.read(cx).pairing_is_presented(pending.session_id) {
            return;
        }
        let host = self.host.clone();
        self.workspace
            .update(cx, |workspace, cx| {
                open_pairing_question(host, workspace, window, cx);
            })
            .log_err();
    }

    /// The question cannot be put on screen because no window is in front, and
    /// whoever asked to pair is most likely in their browser, with two minutes
    /// before it gives up. Asks the OS to call them back, once per question and
    /// from whichever window hears of it first. A click only brings Zode
    /// forward, and the window coming to the front puts the question on screen.
    fn call_back_about_pairing(&self, pending: &PendingDecision, cx: &mut Context<Self>) {
        if !cx.can_post_notifications() || a_workspace_window_is_active(cx) {
            return;
        }
        let session_id = pending.session_id;
        if !self
            .host
            .update(cx, |host, _| host.claim_pairing_notification(session_id))
        {
            return;
        }
        cx.post_notification(pairing_notification(session_id, &pending.peer_name));
    }

    fn menu(
        host: Entity<RemoteHost>,
        workspace: WeakEntity<Workspace>,
        anyone_connected: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<ContextMenu> {
        let push_needs_upgrade = host.read(cx).push_needs_upgrade();
        let upgrade = upgrade_offered(host.read(cx).state()) || push_needs_upgrade;
        ContextMenu::build(window, cx, move |menu, _window, _cx| {
            let disconnect_host = host.clone();
            let manage_host = host.clone();
            let pair_host = host.clone();
            let pair_workspace = workspace.clone();
            menu.when(push_needs_upgrade, |menu| menu.label(PUSH_NEEDS_PRO))
                .when(upgrade, |menu| {
                    menu.entry("Upgrade\u{2026}", None, |_window, cx| {
                        cx.open_url(&zode_account::pricing_url());
                    })
                    .separator()
                })
                .when(anyone_connected, |menu| {
                    menu.entry("Disconnect All Devices", None, move |_window, cx| {
                        disconnect_host.update(cx, |host, cx| host.disconnect_all(cx));
                    })
                })
                .entry("Turn Off Remote Control", None, |_window, cx| {
                    crate::disable_remote_control(cx);
                })
                .separator()
                .entry(
                    "Pair a Phone with QR Code\u{2026}",
                    None,
                    move |window, cx| {
                        let host = pair_host.clone();
                        pair_workspace
                            .update(cx, |workspace, cx| {
                                qr_pairing_modal::open(host, workspace, window, cx);
                            })
                            .log_err();
                    },
                )
                .entry("Manage Trusted Devices\u{2026}", None, move |window, cx| {
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

impl RemoteControlIndicator {
    /// A quiet icon while remote control is on and nobody is connected, so
    /// that "on" is never invisible; nothing at all while it is off. The
    /// element stays in the bar either way, so it can never be the thing a
    /// settings toggle forgot to bring back.
    fn render_idle(&self, cx: &App) -> AnyElement {
        let host = self.host.read(cx);
        let Some((tooltip, color)) = idle_status_with_push(host.state(), host.push_needs_upgrade())
        else {
            return div().into_any_element();
        };
        let host = self.host.clone();
        let workspace = self.workspace.clone();
        PopoverMenu::new("remote-control-idle")
            .menu(move |window, cx| {
                Some(Self::menu(
                    host.clone(),
                    workspace.clone(),
                    false,
                    window,
                    cx,
                ))
            })
            .anchor(Anchor::BottomRight)
            .trigger_with_tooltip(
                IconButton::new("remote-control-idle-trigger", IconName::Screen)
                    .icon_size(IconSize::Small)
                    .icon_color(color),
                Tooltip::text(tooltip),
            )
            .into_any_element()
    }
}

impl Render for RemoteControlIndicator {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.render_status(cx);
        match self.render_waiting_request(cx) {
            Some(request) => h_flex()
                .gap_1()
                .child(request)
                .child(status)
                .into_any_element(),
            None => status,
        }
    }
}

impl RemoteControlIndicator {
    /// A request is answered in a modal that only opens by itself in a window
    /// that is in front. This button is the way to it from any window, at any
    /// time, for as long as the device is waiting.
    fn render_waiting_request(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let device = self
            .host
            .read(cx)
            .pending_pairing()?
            .peer_name
            .trim()
            .to_string();
        let tooltip = if device.is_empty() {
            "A device wants to control this Zode. Click to compare the six digits.".to_string()
        } else {
            format!("{device} wants to control this Zode. Click to compare the six digits.")
        };
        let host = self.host.clone();
        let workspace = self.workspace.clone();
        // Wrapped so a test can find its bounds and click it.
        Some(
            div()
                .debug_selector(|| "remote-control-review-pairing".into())
                .child(
                    Button::new("remote-control-review-pairing", "Pairing Request")
                        .style(ButtonStyle::Tinted(ui::TintColor::Accent))
                        .label_size(LabelSize::Small)
                        .start_icon(Icon::new(IconName::Screen).size(IconSize::Small))
                        .tooltip(Tooltip::text(tooltip))
                        .on_click(move |_, window, cx| {
                            workspace
                                .update(cx, |workspace, cx| {
                                    open_pairing_question(host.clone(), workspace, window, cx);
                                })
                                .log_err();
                        }),
                )
                .into_any_element(),
        )
    }

    fn render_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let sessions = self.host.read(cx).sessions();
        if sessions.is_empty() {
            return self.render_idle(cx);
        }
        let label = controlled_by(&sessions);
        let host = self.host.clone();
        let workspace = self.workspace.clone();

        PopoverMenu::new("remote-control-indicator")
            .menu(move |window, cx| {
                Some(Self::menu(
                    host.clone(),
                    workspace.clone(),
                    true,
                    window,
                    cx,
                ))
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
            )
            .into_any_element()
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
    fn the_notification_names_the_device_and_is_keyed_by_the_session() {
        let notification = pairing_notification(7, "Ada's laptop");
        assert_eq!(notification.id.to_string(), "remote-control-pairing-7");
        assert_eq!(
            notification.title.to_string(),
            "Ada's laptop wants to control this Zode"
        );
        assert_eq!(
            notification.body.to_string(),
            "Switch to Zode to compare the six digits."
        );
    }

    #[test]
    fn the_notification_falls_back_to_a_device_when_the_name_is_blank() {
        for blank in ["", "   "] {
            assert_eq!(
                pairing_notification(1, blank).title.to_string(),
                "A device wants to control this Zode"
            );
        }
    }

    #[test]
    fn the_idle_icon_shows_only_while_it_is_on_and_says_how_it_is() {
        assert_eq!(idle_status(&HostState::Disabled), None);
        assert_eq!(
            idle_status(&HostState::Online).map(|(_, color)| color),
            Some(Color::Muted)
        );
        assert_eq!(
            idle_status(&HostState::Reconnecting).map(|(_, color)| color),
            Some(Color::Warning)
        );
        assert_eq!(
            idle_status(&HostState::SignedOut).map(|(_, color)| color),
            Some(Color::Warning)
        );
        let (tooltip, color) =
            idle_status(&HostState::Failed("no keychain".into())).expect("shown");
        assert_eq!(color, Color::Error);
        assert!(tooltip.contains("no keychain"));
        let (tooltip, color) = idle_status(&HostState::Halted(
            remote_relay_client::StopReason::QuotaExceeded,
        ))
        .expect("shown");
        assert_eq!(color, Color::Error);
        assert!(tooltip.contains("allowance"));
        assert!(tooltip.contains("resets 00:00 UTC"), "{tooltip}");
        assert!(tooltip.contains("Pro has no daily limit"), "{tooltip}");
    }

    #[test]
    fn a_plan_without_pushes_is_said_while_remote_control_is_on() {
        let (tooltip, color) = idle_status_with_push(&HostState::Online, true).expect("shown");
        assert!(tooltip.contains(PUSH_NEEDS_PRO), "{tooltip}");
        assert_eq!(color, Color::Warning);
        let (tooltip, color) = idle_status_with_push(&HostState::Online, false).expect("shown");
        assert!(!tooltip.contains(PUSH_NEEDS_PRO));
        assert_eq!(color, Color::Muted);
        let (_, color) =
            idle_status_with_push(&HostState::Failed("no keychain".into()), true).expect("shown");
        assert_eq!(color, Color::Error, "a louder state keeps its colour");
        assert_eq!(idle_status_with_push(&HostState::Disabled, true), None);
    }

    #[test]
    fn only_the_relay_allowance_offers_an_upgrade() {
        use remote_relay_client::StopReason;
        assert!(upgrade_offered(&HostState::Halted(
            StopReason::QuotaExceeded
        )));
        for state in [
            HostState::Disabled,
            HostState::Online,
            HostState::SignedOut,
            HostState::Halted(StopReason::Revoked),
            HostState::Halted(StopReason::SessionEnded),
            HostState::Halted(StopReason::Replaced),
            HostState::Failed("no keychain".into()),
        ] {
            assert!(!upgrade_offered(&state), "{state:?}");
        }
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
