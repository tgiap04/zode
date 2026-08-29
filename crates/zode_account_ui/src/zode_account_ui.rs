//! What the account looks like: the sign-in window, and the actions the rail
//! dispatches.
//!
//! The rail button itself lives in `sidebar`, beside the other rail items, and
//! reaches this crate only through [`zed_actions::account`] — so the sidebar
//! does not depend on the account's UI, and this crate does not depend on the
//! sidebar.

mod sign_in_modal;

use gpui::{App, Window};
use ui::prelude::*;
use workspace::Workspace;
use zode_account::{Account, AccountStatus};

pub use sign_in_modal::SignInModal;

/// Where a signed-in user goes to manage their account.
const ACCOUNT_WEB_PATH: &str = "/account";

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace
            .register_action(sign_in)
            .register_action(sign_out)
            .register_action(open_account_on_web);
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
