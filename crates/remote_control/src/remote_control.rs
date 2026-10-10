//! Controlling this Zode from another device of the same account.
//!
//! The Zode that is controlled runs a [`RemoteHost`]: while the
//! `remote_control` setting is on and somebody is signed in it keeps one
//! connection to the relay, lets only devices that were paired in person talk
//! to it, and shows their agents and terminals -- and lets them type -- over an
//! end-to-end encrypted channel the relay cannot read. While the setting is off
//! it does none of this: no connection, and no tap on any terminal.
//!
//! The wire formats and the cryptography are in `remote_relay_protocol`; the
//! connection to the relay is in `remote_relay_client`. This crate is what
//! turns them into a feature: who may connect, what they are shown, and how the
//! person at this screen stays in charge.

mod agent_mirror;
mod file_browse;
mod host_environment;
mod hosting;
mod ide_bridge;
mod indicator;
mod pairing_flow;
mod pairing_modal;
mod remote_control_settings;
mod remote_host;
mod session_router;
mod terminal_mirror;
mod trusted_devices_modal;

#[cfg(test)]
mod file_browse_tests;
#[cfg(test)]
mod remote_control_tests;

use std::sync::Arc;

use gpui::{App, AppContext as _, Context, Window, actions};
use remote_relay_client::WebSocketTransport;
use workspace::Workspace;
use zode_account::Account;

pub use agent_mirror::{AgentMirror, AgentMirrorEvent};
pub use file_browse::FileBrowser;
pub use host_environment::HostEnvironment;
pub use indicator::RemoteControlIndicator;
pub use pairing_flow::PendingDecision;
pub use remote_control_settings::RemoteControlSettings;
pub use remote_host::{HostState, RemoteHost, RemoteHostEvent, SessionInfo};
pub use terminal_mirror::{TerminalMirror, TerminalMirrorEvent};

actions!(
    remote_control,
    [
        /// Disconnects every device that is controlling this Zode right now.
        DisconnectAll,
        /// Turns remote control off. Devices that are connected are
        /// disconnected, and nothing is listened for until it is turned on
        /// again.
        Disable,
        /// Shows the devices trusted to control this Zode, and lets you remove
        /// them.
        ManageDevices,
    ]
);

/// Turns remote control off: everything comes down at once, then the setting
/// is written so it stays off. The two are separate because the write is
/// asynchronous and can fail, and a device must not stay in control until it
/// succeeds; a failure is reported rather than left in a log.
pub(crate) fn disable_remote_control(cx: &mut App) {
    if let Some(host) = RemoteHost::global(cx) {
        host.update(cx, |host, cx| host.turn_off(cx));
    }
    let written = settings::update_settings_file_with_completion(
        <dyn fs::Fs>::global(cx),
        cx,
        |content, _| {
            content.remote_control.get_or_insert_default().enabled = Some(false);
        },
    );
    cx.spawn(async move |cx| {
        let failure = match written.await {
            Ok(Ok(())) => return,
            Ok(Err(error)) => format!("{error:#}"),
            Err(_cancelled) => "the settings update was dropped".to_string(),
        };
        if let Some(host) = cx.update(|cx| RemoteHost::global(cx)) {
            host.update(cx, |host, cx| {
                host.report_problem(
                    format!(
                        "Remote control is off for now, but turning it off could not be saved \
                         to your settings: {failure}"
                    ),
                    cx,
                );
            });
        }
    })
    .detach();
}

/// Starts the feature, or leaves it unborn: nothing in here connects to
/// anything until the setting is on and an account is signed in.
pub fn init(cx: &mut App) {
    if RemoteHost::global(cx).is_some() {
        return;
    }
    let Some(account) = Account::global(cx) else {
        log::warn!("remote control needs the account, which is not set up");
        return;
    };
    let agents = cx.new(AgentMirror::new);
    let terminals = cx.new(TerminalMirror::new);
    let files = cx.new(FileBrowser::new);
    let environment = HostEnvironment::from_account(&account, cx);
    let app_version = release_channel::AppVersion::global(cx).to_string();
    let host = cx.new(|cx| {
        RemoteHost::new(
            account,
            environment,
            Arc::new(WebSocketTransport),
            agents,
            terminals,
            Some(files),
            app_version,
            cx,
        )
    });
    RemoteHost::set_global(host.clone(), cx);

    cx.observe_new(move |workspace: &mut Workspace, window, cx| {
        let Some(window) = window else {
            return;
        };
        register_workspace(workspace, &host, window, cx);
    })
    .detach();
}

fn register_workspace(
    workspace: &mut Workspace,
    host: &gpui::Entity<RemoteHost>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    workspace.register_action(|_, _: &DisconnectAll, _window, cx| {
        if let Some(host) = RemoteHost::global(cx) {
            host.update(cx, |host, cx| host.disconnect_all(cx));
        }
    });
    workspace.register_action(|_, _: &Disable, _window, cx| disable_remote_control(cx));
    workspace.register_action(|workspace, _: &ManageDevices, window, cx| {
        let Some(host) = RemoteHost::global(cx) else {
            return;
        };
        workspace.toggle_modal(window, cx, move |window, cx| {
            trusted_devices_modal::TrustedDevicesModal::new(host, window, cx)
        });
    });

    let weak_workspace = cx.weak_entity();
    let indicator =
        cx.new(|cx| RemoteControlIndicator::new(host.clone(), weak_workspace, window, cx));
    workspace.status_bar().update(cx, |status_bar, cx| {
        status_bar.add_right_item(indicator, window, cx);
    });
}
