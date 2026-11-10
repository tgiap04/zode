//! What the account looks like: the sign-in window, and the actions the rail
//! dispatches.
//!
//! The rail button itself lives in `sidebar`, beside the other rail items, and
//! reaches this crate only through [`zed_actions::account`] — so the sidebar
//! does not depend on the account's UI, and this crate does not depend on the
//! sidebar.

mod diff_modal;
mod plan_badge;
mod recovery_key_modal;
mod sign_in_modal;
mod sync_modal;

use gpui::{App, Window, actions};
use ui::prelude::*;
use workspace::Workspace;
use zode_account::{Account, AccountStatus, PlanSource, PlanStatus, PlanSummary, PlanTier};
use zode_sync::{SyncSession, SyncStatus};

pub use diff_modal::DiffModal;
pub use plan_badge::{PlanBadge, PlanBadgeKind, plan_badge};
pub use recovery_key_modal::{RecoveryKeyModal, RecoveryKeyMode};
pub use sign_in_modal::SignInModal;
pub use sync_modal::SyncModal;

/// Where a signed-in user goes to manage their account.
///
/// Points straight at the device list rather than a landing page: the reason
/// someone opens this from the editor is to see, rename, or remove a machine.
const ACCOUNT_WEB_PATH: &str = "/account/devices";

actions!(
    zode_account,
    [
        /// Opens the plans page on the web. Paying happens there, never in the
        /// editor.
        UpgradePlan,
        /// Opens the billing page on the web.
        ManagePlan
    ]
);

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace
            .register_action(sign_in)
            .register_action(sign_out)
            .register_action(open_account_on_web)
            .register_action(sync_settings)
            .register_action(show_recovery_key)
            .register_action(enter_recovery_key)
            .register_action(rotate_recovery_key)
            .register_action(|_, _: &UpgradePlan, _window, cx| {
                cx.open_url(&zode_account::pricing_url())
            })
            .register_action(|_, _: &ManagePlan, _window, cx| {
                cx.open_url(&zode_account::billing_url())
            });
    })
    .detach();
}

/// Reads the plan, if someone is signed in. For the moments the README names:
/// the account menu or a sync window opening.
pub fn refresh_plan(cx: &mut App) {
    if let Some(account) = Account::global(cx) {
        account.update(cx, |account, cx| account.refresh_plan(cx));
    }
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
    // `web_url`, not `api_url`. Stripping `/api` off the API base was right
    // only while the API answered on the site's own host; once it moved to
    // `api.zodekit.site` this opened a link to a host that serves no pages.
    cx.open_url(&format!("{}{ACCOUNT_WEB_PATH}", zode_account::web_url()));
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
    refresh_plan(cx);
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
        SyncStatus::PlanLimited(limit) => Some(limit.sentence().into()),
        SyncStatus::Failed(text) => Some(text.clone()),
    }
}

/// One line naming the plan, for the account menu.
pub fn plan_summary(plan: &PlanSummary) -> SharedString {
    match plan.tier {
        PlanTier::Free => match plan.limits.sync_devices.max {
            Some(max) => format!(
                "Plan: Free · {} of {max} synced devices",
                plan.limits.sync_devices.used
            )
            .into(),
            None => "Plan: Free".into(),
        },
        PlanTier::Pro => {
            let when = if plan.source == PlanSource::Lifetime {
                Some("lifetime".to_string())
            } else if plan.status == PlanStatus::Canceling {
                plan.ends_at
                    .as_ref()
                    .and_then(|at| short_date(at))
                    .map(|date| format!("ends {date}"))
            } else {
                plan.renews_at
                    .as_ref()
                    .and_then(|at| short_date(at))
                    .map(|date| format!("renews {date}"))
            };
            match when {
                Some(when) => format!("Plan: Pro ({when})").into(),
                None => "Plan: Pro".into(),
            }
        }
        PlanTier::Team => match &plan.team {
            Some(team) => format!("Plan: Team \u{2014} {}", team.name).into(),
            None => "Plan: Team".into(),
        },
    }
}

/// "3 Dec" from an ISO-8601 timestamp, read from its date part only. Anything
/// that is not one gives `None` rather than a wrong date.
fn short_date(iso: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let date = iso.get(..10)?;
    let mut parts = date.split('-');
    let (_year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    let month: usize = month.parse().ok()?;
    let day: u8 = day.parse().ok()?;
    let name = MONTHS.get(month.checked_sub(1)?)?;
    (1..=31).contains(&day).then(|| format!("{day} {name}"))
}

/// What the account menu offers about the plan.
#[derive(Debug, PartialEq, Eq)]
pub struct PlanMenu {
    pub line: Option<SharedString>,
    pub upgrade: bool,
    pub manage: bool,
}

/// Free is offered an upgrade, a paid plan its billing page, and an account
/// whose plan has not been read yet both, rather than a guess.
pub fn plan_menu(plan: Option<&PlanSummary>) -> PlanMenu {
    match plan {
        None => PlanMenu {
            line: None,
            upgrade: true,
            manage: true,
        },
        Some(plan) => PlanMenu {
            line: Some(plan_summary(plan)),
            upgrade: plan.tier == PlanTier::Free,
            manage: plan.tier != PlanTier::Free,
        },
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

#[cfg(test)]
mod tests {
    use super::*;
    use zode_account::{PlanLimits, SyncDeviceUsage, TeamSummary};

    fn plan(tier: PlanTier, source: PlanSource) -> PlanSummary {
        PlanSummary {
            tier,
            source,
            status: PlanStatus::Active,
            renews_at: None,
            ends_at: None,
            limits: PlanLimits {
                sync_devices: SyncDeviceUsage {
                    used: 2,
                    max: Some(2),
                },
                env_sync: false,
                relay_daily_bytes: 2_147_483_648,
                team_vault: false,
            },
            team: None,
            grandfather_env_until: None,
            upgrade_url: "https://zodekit.site/pricing".into(),
            manage_url: "https://zodekit.site/account/billing".into(),
        }
    }

    #[test]
    fn free_counts_the_synced_devices() {
        assert_eq!(
            plan_summary(&plan(PlanTier::Free, PlanSource::Free)),
            "Plan: Free · 2 of 2 synced devices"
        );
    }

    #[test]
    fn pro_says_when_it_renews_or_ends() {
        let mut pro = plan(PlanTier::Pro, PlanSource::Subscription);
        pro.limits.sync_devices.max = None;
        pro.renews_at = Some("2026-12-03T10:00:00.000Z".into());
        assert_eq!(plan_summary(&pro), "Plan: Pro (renews 3 Dec)");

        pro.status = PlanStatus::Canceling;
        pro.renews_at = None;
        pro.ends_at = Some("2027-01-15T00:00:00.000Z".into());
        assert_eq!(plan_summary(&pro), "Plan: Pro (ends 15 Jan)");

        let lifetime = plan(PlanTier::Pro, PlanSource::Lifetime);
        assert_eq!(plan_summary(&lifetime), "Plan: Pro (lifetime)");

        // A date the server wrote some other way is left out, not misread.
        let mut odd = plan(PlanTier::Pro, PlanSource::Subscription);
        odd.renews_at = Some("next month".into());
        assert_eq!(plan_summary(&odd), "Plan: Pro");
    }

    #[test]
    fn team_names_the_team() {
        let mut team = plan(PlanTier::Team, PlanSource::TeamSeat);
        team.team = Some(TeamSummary {
            id: "t1".into(),
            name: "Acme".into(),
            role: "member".into(),
            key_version: 1,
        });
        assert_eq!(plan_summary(&team), "Plan: Team — Acme");
    }

    #[test]
    fn the_menu_offers_what_fits_the_plan() {
        assert_eq!(
            plan_menu(None),
            PlanMenu {
                line: None,
                upgrade: true,
                manage: true,
            }
        );
        let free = plan_menu(Some(&plan(PlanTier::Free, PlanSource::Free)));
        assert_eq!(
            free.line.as_ref().map(|line| line.as_ref()),
            Some("Plan: Free · 2 of 2 synced devices")
        );
        assert!(free.upgrade && !free.manage);
        let pro = plan_menu(Some(&plan(PlanTier::Pro, PlanSource::Subscription)));
        assert!(!pro.upgrade && pro.manage);
    }

    #[test]
    fn a_plan_refusal_is_summarised_by_its_sentence() {
        let limit = zode_account::PlanLimit::from_body(
            r#"{"error":"sync_device_limit","limit":2}"#,
            &zode_account::web_url(),
        );
        assert_eq!(
            sync_summary(&SyncStatus::PlanLimited(limit.clone())),
            Some(limit.sentence().into())
        );
    }
}
