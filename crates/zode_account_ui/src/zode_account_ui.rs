//! What the account looks like: the sign-in window, and the actions the rail
//! dispatches.
//!
//! The rail button itself lives in `sidebar`, beside the other rail items, and
//! reaches this crate only through [`zed_actions::account`] — so the sidebar
//! does not depend on the account's UI, and this crate does not depend on the
//! sidebar.

mod diff_modal;
mod recovery_key_modal;
mod sign_in_modal;
mod sync_modal;

use gpui::{App, Window};
use ui::prelude::*;
use workspace::Workspace;
use zode_account::{Account, AccountStatus};
use zode_sync::{SyncSession, SyncStatus};

pub use diff_modal::DiffModal;
pub use recovery_key_modal::{RecoveryKeyModal, RecoveryKeyMode};
pub use sign_in_modal::SignInModal;
pub use sync_modal::SyncModal;

/// Where a signed-in user goes to manage their account.
///
/// Points straight at the device list rather than a landing page: the reason
/// someone opens this from the editor is to see, rename, or remove a machine.
const ACCOUNT_WEB_PATH: &str = "/account/devices";

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace
            .register_action(sign_in)
            .register_action(sign_out)
            .register_action(open_account_on_web)
            .register_action(sync_settings)
            .register_action(show_recovery_key)
            .register_action(enter_recovery_key)
            .register_action(rotate_recovery_key);
    })
    .detach();
}

fn sign_in(
    workspace: &mut Workspace,
    _: &zed_actions::account::SignIn,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(account) = Account::global(cx) else {
        return;
    };
    if account.read(cx).status().is_signed_in() {
        return;
    }

    // Started before the modal opens so the modal has a code to show as soon
    // as one arrives, rather than opening onto an empty box.
    account.update(cx, |account, cx| account.sign_in(cx));
    workspace.toggle_modal(window, cx, |_window, cx| SignInModal::new(account, cx));
}

fn sign_out(
    _: &mut Workspace,
    _: &zed_actions::account::SignOut,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(account) = Account::global(cx) else {
        return;
    };
    account
        .update(cx, |account, cx| account.sign_out(cx))
        .detach();
}

fn open_account_on_web(
    _: &mut Workspace,
    _: &zed_actions::account::OpenAccountOnWeb,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    // Derived from the same base the account service uses, so pointing the
    // editor at a local backend points this at the same place.
    let base = zode_account::api_url();
    let origin = base.strip_suffix("/api").unwrap_or(&base);
    cx.open_url(&format!("{origin}{ACCOUNT_WEB_PATH}"));
}

/// Opens the push/pull window, asking for a recovery key first when this
/// machine has none.
///
/// The key is read from the OS keychain, which is not a network call — so
/// opening this window on a machine that has never synced still sends nothing.
fn sync_settings(
    // The window is opened from the spawned task below, once the keychain has
    // answered, so this borrow is not the one that opens it.
    _workspace: &mut Workspace,
    _: &zed_actions::account::SyncSettings,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(session) = SyncSession::global(cx) else {
        return;
    };
    // Opens the diff window the moment there is something to decide. Attached
    // here rather than globally so a user who never opens sync never gets an
    // observer either.
    watch_for_divergence(&session, window, cx);

    let loading = session.update(cx, |session, cx| session.load_key(cx));
    let handle = cx.entity().downgrade();
    let session_for_task = session.clone();

    cx.spawn_in(window, async move |_, cx| {
        loading.await;
        let needs_key = session_for_task.read_with(cx, |session, _| !session.has_key());

        _ = handle.update_in(cx, |workspace, window, cx| {
            if needs_key {
                // First time on this machine. Which question to ask is the
                // user's to answer -- creating a key on a machine that should
                // have adopted an existing one is how synced data gets
                // stranded under a key nobody kept.
                workspace.toggle_modal(window, cx, |window, cx| {
                    RecoveryKeyModal::new(session_for_task, RecoveryKeyMode::Create, window, cx)
                });
            } else {
                workspace.toggle_modal(window, cx, |_window, cx| {
                    SyncModal::new(session_for_task, cx)
                });
            }
        });
    })
    .detach();
}

/// Opens the diff window the moment the session has something to decide.
///
/// `observe_in` rather than `observe`: opening a modal needs a `Window`, and
/// the plain observer does not carry one.
fn watch_for_divergence(
    session: &gpui::Entity<SyncSession>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    cx.observe_in(session, window, |workspace, session, window, cx| {
        if session.read(cx).pending().is_none() {
            return;
        }
        workspace.toggle_modal(window, cx, |_window, cx| DiffModal::new(session, cx));
    })
    .detach();
}

fn show_recovery_key(
    workspace: &mut Workspace,
    _: &zed_actions::account::ShowRecoveryKey,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(session) = SyncSession::global(cx) else {
        return;
    };
    workspace.toggle_modal(window, cx, |window, cx| {
        RecoveryKeyModal::new(session, RecoveryKeyMode::Reveal, window, cx)
    });
}

fn enter_recovery_key(
    workspace: &mut Workspace,
    _: &zed_actions::account::EnterRecoveryKey,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(session) = SyncSession::global(cx) else {
        return;
    };
    workspace.toggle_modal(window, cx, |window, cx| {
        RecoveryKeyModal::new(session, RecoveryKeyMode::Enter, window, cx)
    });
}

/// Replaces the recovery key and re-encrypts everything stored under it.
///
/// The only action that actually cuts a lost machine off from synced data —
/// revoking a device on the web ends its session but leaves it holding a key
/// that still opens whatever it already downloaded.
fn rotate_recovery_key(
    workspace: &mut Workspace,
    _: &zed_actions::account::RotateRecoveryKey,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(session) = SyncSession::global(cx) else {
        return;
    };
    workspace.toggle_modal(window, cx, |window, cx| {
        RecoveryKeyModal::new(session, RecoveryKeyMode::Rotate, window, cx)
    });
}

/// One line describing what sync last did, for the account menu.
pub fn sync_summary(status: &SyncStatus) -> Option<SharedString> {
    match status {
        SyncStatus::Idle => None,
        SyncStatus::Working => Some("Syncing…".into()),
        SyncStatus::Done(text) => Some(text.clone()),
        SyncStatus::NeedsKey => Some("No recovery key on this machine".into()),
        SyncStatus::KeyMismatch => Some("Stored data needs a different recovery key".into()),
        SyncStatus::Failed(text) => Some(text.clone()),
    }
}

/// One line describing the account, for a tooltip or a menu header.
pub fn status_summary(status: &AccountStatus) -> SharedString {
    match status {
        AccountStatus::SignedOut => "Sign in to Zode".into(),
        AccountStatus::WaitingForApproval { user_code, .. } => {
            format!("Waiting for approval — {user_code}").into()
        }
        AccountStatus::SignedIn(user) => user.email.clone(),
        AccountStatus::Offline(user) => format!("{} (offline)", user.email).into(),
    }
}
