//! Which sign-in a list, dialog or detail tab was made under.
//!
//! Each GitHub tab (one per workspace) holds at most one signed-in account:
//! a login and the token behind it. What it makes is stamped with that
//! account and the workspace's project, and may write only while a GitHub
//! tab of that workspace still holds the same account. A switch to another
//! account ends it there. A refused token or a disconnect ends the account
//! everywhere it is held: every tab holding it moves to "token refused" or
//! signed out, and what they made writes nothing. A check that fails, or
//! another window signing in, says nothing about this token and ends nothing.

use std::collections::HashMap;
use std::sync::Arc;

use github::{GithubBackend, GithubError, TokenIdentity};
use gpui::{App, Context, Entity, EntityId, Global, SharedString, Subscription, Window};
use workspace::Workspace;
use workspace::item::Item;

use crate::github_view::{AuthState, GithubView};

/// The signed-in account a session stands for.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SessionAccount {
    pub login: SharedString,
    /// `None` only for a session a test installed without a token.
    pub token: Option<TokenIdentity>,
}

/// The session something was made under: an account, as held by the GitHub
/// tab of one workspace.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SessionStamp {
    /// The project of the workspace whose GitHub tab holds the account.
    scope: EntityId,
    account: SessionAccount,
}

impl SessionStamp {
    pub(crate) fn new(scope: EntityId, account: SessionAccount) -> Self {
        Self { scope, account }
    }
}

/// Why an account's sessions ended in every tab that holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ended {
    /// GitHub refused the token.
    Refused,
    /// The user disconnected the account, in any window.
    Disconnected,
}

#[derive(Default)]
pub(crate) struct HeldSessions {
    /// Each GitHub tab holding a session, by the tab's entity.
    held: HashMap<EntityId, SessionStamp>,
    /// Accounts ended everywhere. Only the tabs holding one move to "token
    /// refused" or signed out; an entry goes once no tab holds it.
    ended: HashMap<SessionAccount, Ended>,
}

impl Global for HeldSessions {}

/// Whether what was made under `stamp` may still read and write: its
/// workspace's GitHub tab holds the same account, and neither GitHub nor the
/// user has ended it.
pub fn is_live_session(stamp: &SessionStamp, cx: &App) -> bool {
    cx.try_global::<HeldSessions>().is_some_and(|sessions| {
        !sessions.ended.contains_key(&stamp.account)
            && sessions.held.values().any(|held| held == stamp)
    })
}

/// What a detail tab says once its session is over.
pub fn session_ended_message(what: &str) -> String {
    format!("Signed out or account changed — reopen this {what}.")
}

/// The session the GitHub tab `holder` holds, if any.
pub(crate) fn held_session(holder: EntityId, cx: &App) -> Option<SessionStamp> {
    cx.try_global::<HeldSessions>()
        .and_then(|sessions| sessions.held.get(&holder).cloned())
}

/// `holder` is now signed in as `stamp`; whatever it held before ends.
pub(crate) fn hold_session(holder: EntityId, stamp: SessionStamp, cx: &mut App) {
    if let Some(sessions) = cx.try_global::<HeldSessions>()
        && sessions.held.get(&holder) == Some(&stamp)
        && !sessions.ended.contains_key(&stamp.account)
    {
        return;
    }
    let sessions = cx.default_global::<HeldSessions>();
    sessions.ended.remove(&stamp.account);
    sessions.held.insert(holder, stamp);
}

/// `holder` signed out, was refused, or was closed.
pub(crate) fn release_session(holder: EntityId, cx: &mut App) {
    let holds = cx
        .try_global::<HeldSessions>()
        .is_some_and(|sessions| sessions.held.contains_key(&holder));
    if !holds {
        return;
    }
    let HeldSessions { held, ended } = cx.global_mut::<HeldSessions>();
    held.remove(&holder);
    ended.retain(|account, _| held.values().any(|stamp| &stamp.account == account));
}

fn ended_by(stamp: &SessionStamp, cx: &App) -> Option<Ended> {
    cx.try_global::<HeldSessions>()
        .and_then(|sessions| sessions.ended.get(&stamp.account).copied())
}

/// Whether GitHub refused the token of `stamp`'s account.
pub(crate) fn is_refused(stamp: &SessionStamp, cx: &App) -> bool {
    ended_by(stamp, cx) == Some(Ended::Refused)
}

/// Whether the user disconnected `stamp`'s account, in this window or another.
pub(crate) fn is_disconnected(stamp: &SessionStamp, cx: &App) -> bool {
    ended_by(stamp, cx) == Some(Ended::Disconnected)
}

/// The user disconnected the account `holder` holds. The token is gone for
/// good, so every other tab holding the same account ends with this one:
/// they would otherwise keep writing with a token the user just let go.
pub(crate) fn disconnect_everywhere(holder: EntityId, cx: &mut App) {
    let Some(stamp) = held_session(holder, cx) else {
        return;
    };
    cx.global_mut::<HeldSessions>()
        .ended
        .entry(stamp.account)
        .or_insert(Ended::Disconnected);
}

/// Tells the GitHub tabs that GitHub refused the token behind `stamp`, from
/// a read or a write anywhere. Every tab holding that account moves to
/// "token refused"; a tab holding another account is left alone, and a
/// refusal of an account no tab holds any more is old news.
pub fn report_if_revoked(error: &GithubError, stamp: &SessionStamp, cx: &mut App) {
    if *error != GithubError::TokenRevoked {
        return;
    }
    let news = cx.try_global::<HeldSessions>().is_some_and(|sessions| {
        !sessions.ended.contains_key(&stamp.account)
            && sessions
                .held
                .values()
                .any(|held| held.account == stamp.account)
    });
    if news {
        cx.global_mut::<HeldSessions>()
            .ended
            .insert(stamp.account.clone(), Ended::Refused);
    }
}

/// Redraws `T` whenever a session starts or ends, so a tab notices it is
/// stale.
pub(crate) fn observe_session<T: 'static>(cx: &mut Context<T>) -> Subscription {
    cx.observe_global::<HeldSessions>(|_, cx| cx.notify())
}

/// The backend and session of the workspace's GitHub tab, when it is signed
/// in: what "Reopen" in a stale detail tab reads with.
pub fn current_session_backend(
    workspace: &Workspace,
    cx: &App,
) -> Option<(Arc<dyn GithubBackend>, SessionStamp)> {
    workspace.items_of_type::<GithubView>(cx).find_map(|view| {
        let view = view.read(cx);
        let AuthState::SignedIn(session) = &view.auth else {
            return None;
        };
        let stamp = view.session_stamp()?;
        is_live_session(&stamp, cx).then(|| (session.backend.clone(), stamp))
    })
}

/// Puts `replacement` where `stale` is and closes `stale`: a tab from an
/// older session is never brought forward again.
pub(crate) fn replace_stale_tab<T: Item>(
    workspace: &mut Workspace,
    stale: &Entity<T>,
    replacement: &Entity<T>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(pane) = workspace.pane_for(stale) else {
        workspace.add_item_to_active_pane(Box::new(replacement.clone()), None, true, window, cx);
        return;
    };
    let index = pane.read(cx).index_for_item(stale);
    workspace.add_item(
        pane.clone(),
        Box::new(replacement.clone()),
        index,
        true,
        true,
        window,
        cx,
    );
    pane.update(cx, |pane, cx| {
        pane.remove_item(stale.entity_id(), false, false, window, cx)
    });
}

/// Stands for a GitHub tab in tests that make lists and detail tabs without
/// one.
#[cfg(test)]
const TEST_HOLDER: u64 = u64::MAX;

/// The session `issues_test_support::context` and the tests' detail tabs
/// are made under; live once [`hold_test_session`] ran.
#[cfg(test)]
pub(crate) fn test_session() -> SessionStamp {
    SessionStamp::new(
        EntityId::from(TEST_HOLDER),
        SessionAccount {
            login: "octocat".into(),
            token: None,
        },
    )
}

#[cfg(test)]
pub(crate) fn hold_test_session(cx: &mut App) {
    hold_session(EntityId::from(TEST_HOLDER), test_session(), cx);
}

/// Ends [`test_session`], as a sign-out would.
#[cfg(test)]
pub(crate) fn end_test_session(cx: &mut App) {
    release_session(EntityId::from(TEST_HOLDER), cx);
}
