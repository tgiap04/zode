//! What the account's plan covers, as the server describes it.
//!
//! Read-only on this side. Nothing in the editor is gated by what is parsed
//! here: the server decides what an account may do and answers 402 when it
//! will not, so a plan that is stale or missing can only make a sentence less
//! precise, never lock somebody out of their own data.
//!
//! Every link the server sends is checked against [`crate::web_url`] before it
//! is kept. A response is not a reason to open an arbitrary page in someone's
//! browser.

use gpui::SharedString;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanTier {
    Free,
    Pro,
    Team,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanSource {
    Free,
    Subscription,
    Lifetime,
    TeamSeat,
    Grandfather,
    /// A source added on the server after this build. Shown as nothing rather
    /// than refusing the whole plan.
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Active,
    Grace,
    Canceling,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncDeviceUsage {
    pub used: u32,
    /// `None` on paid plans, which have no device limit.
    pub max: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanLimits {
    pub sync_devices: SyncDeviceUsage,
    pub env_sync: bool,
    /// `None` is unlimited: paid plans, and everyone while the server
    /// enforces no plan.
    pub relay_daily_bytes: Option<u64>,
    pub team_vault: bool,
    /// Whether the plan includes pushes to paired devices. A server from
    /// before the field existed does not say, and is read as "yes": its 402
    /// stays the truth, and "no" is only ever believed when it is stated.
    pub push_notifications: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamSummary {
    pub id: SharedString,
    pub name: SharedString,
    pub role: SharedString,
    pub key_version: u32,
}

/// The answer to `GET /api/account/plan`.
///
/// Held in memory only, never written to disk: it is the server's to state, and
/// a copy that outlived the session would be a copy that could be wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanSummary {
    pub tier: PlanTier,
    pub source: PlanSource,
    pub status: PlanStatus,
    /// ISO-8601, as the server sent it.
    pub renews_at: Option<SharedString>,
    pub ends_at: Option<SharedString>,
    pub limits: PlanLimits,
    pub team: Option<TeamSummary>,
    pub grandfather_env_until: Option<SharedString>,
    /// Always on the web origin; see [`on_web_origin`].
    pub upgrade_url: String,
    pub manage_url: String,
}

impl PlanSummary {
    /// Parses the plan route's body. An unknown tier is an error rather than a
    /// guess: calling a plan "Free" that is not would tell a paying user they
    /// had lost what they paid for.
    pub fn parse(body: &str, web_url: &str) -> anyhow::Result<Self> {
        let wire: PlanResponse = serde_json::from_str(body)?;
        Ok(Self {
            tier: wire.tier,
            source: wire.source,
            status: wire.status,
            renews_at: wire.renews_at.map(Into::into),
            ends_at: wire.ends_at.map(Into::into),
            limits: PlanLimits {
                sync_devices: SyncDeviceUsage {
                    used: wire.limits.sync_devices.used,
                    max: wire.limits.sync_devices.max,
                },
                env_sync: wire.limits.env_sync,
                relay_daily_bytes: wire.limits.relay_daily_bytes,
                team_vault: wire.limits.team_vault,
                push_notifications: wire.limits.push_notifications,
            },
            team: wire.team.map(|team| TeamSummary {
                id: team.id.into(),
                name: team.name.into(),
                role: team.role.into(),
                key_version: team.key_version,
            }),
            grandfather_env_until: wire
                .grandfather
                .and_then(|grandfather| grandfather.env_sync_until)
                .map(Into::into),
            upgrade_url: web_link(wire.upgrade_url, web_url, crate::PRICING_PATH),
            manage_url: web_link(wire.manage_url, web_url, crate::BILLING_PATH),
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanResponse {
    tier: PlanTier,
    source: PlanSource,
    status: PlanStatus,
    #[serde(default)]
    renews_at: Option<String>,
    #[serde(default)]
    ends_at: Option<String>,
    limits: LimitsResponse,
    #[serde(default)]
    team: Option<TeamResponse>,
    #[serde(default)]
    grandfather: Option<GrandfatherResponse>,
    #[serde(default)]
    upgrade_url: Option<String>,
    #[serde(default)]
    manage_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LimitsResponse {
    sync_devices: SyncDevicesResponse,
    env_sync: bool,
    relay_daily_bytes: Option<u64>,
    team_vault: bool,
    #[serde(default = "pushes_unless_told_otherwise")]
    push_notifications: bool,
}

fn pushes_unless_told_otherwise() -> bool {
    true
}

#[derive(Deserialize)]
struct SyncDevicesResponse {
    used: u32,
    max: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TeamResponse {
    id: String,
    name: String,
    role: String,
    key_version: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrandfatherResponse {
    #[serde(default)]
    env_sync_until: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanLimitReason {
    SyncDeviceLimit,
    EnvSyncNotInPlan,
    TeamVaultNotInPlan,
    TeamInactive,
    /// A member past the paid seats of a team that is still live.
    TeamSeatUnavailable,
    /// A reason added on the server after this build, carried as its slug.
    Other(String),
}

/// A 402 from the server: the plan does not cover what was just asked.
///
/// Not a failure of the session. A 403 means "sign in again" to this editor; a
/// 402 never does, and the two must never be routed to the same place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanLimit {
    pub reason: PlanLimitReason,
    pub limit: Option<u64>,
    pub used: Option<u64>,
    /// Always on the web origin; see [`on_web_origin`].
    pub upgrade_url: String,
}

impl PlanLimit {
    /// Reads a 402 body. Never fails: a body that cannot be read still says the
    /// plan refused the request, which is the part that matters.
    pub fn from_body(body: &str, web_url: &str) -> Self {
        // Only the fields named here are read. The slug fallback matches the
        // one `classify` uses for every other refusal.
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RefusalBody {
            error: String,
            #[serde(default)]
            limit: Option<u64>,
            #[serde(default)]
            used: Option<u64>,
            #[serde(default)]
            upgrade_url: Option<String>,
        }

        let parsed = serde_json::from_str::<RefusalBody>(body).ok();
        let reason = match parsed.as_ref().map(|parsed| parsed.error.as_str()) {
            Some("sync_device_limit") => PlanLimitReason::SyncDeviceLimit,
            Some("env_sync_not_in_plan") => PlanLimitReason::EnvSyncNotInPlan,
            Some("team_vault_not_in_plan") => PlanLimitReason::TeamVaultNotInPlan,
            Some("team_inactive") => PlanLimitReason::TeamInactive,
            Some("team_seat_unavailable") => PlanLimitReason::TeamSeatUnavailable,
            Some(other) => PlanLimitReason::Other(other.to_string()),
            None => PlanLimitReason::Other("http_402".into()),
        };
        let (limit, used, upgrade_url) = match parsed {
            Some(parsed) => (parsed.limit, parsed.used, parsed.upgrade_url),
            None => (None, None, None),
        };
        Self {
            reason,
            limit,
            used,
            upgrade_url: web_link(upgrade_url, web_url, crate::PRICING_PATH),
        }
    }

    /// What happened and what to do about it, in one or two sentences.
    ///
    /// The device sentence names the limit and not "device N of M": whether
    /// the server's `used` counts this device is not part of the contract, and
    /// a sentence that is off by one about somebody's own machines reads as a
    /// bug in the count.
    pub fn sentence(&self) -> String {
        match &self.reason {
            PlanLimitReason::SyncDeviceLimit => match self.limit {
                Some(limit) => format!(
                    "The Free plan syncs {limit} devices and this one is past the limit, so it won't sync. Revoke a device on the web or upgrade."
                ),
                None => "This device is past the Free plan's device limit, so it won't sync. Revoke a device on the web or upgrade.".to_string(),
            },
            PlanLimitReason::EnvSyncNotInPlan => {
                ".env sync is part of Pro. Nothing was uploaded; your synced files are still readable.".to_string()
            }
            PlanLimitReason::TeamVaultNotInPlan => {
                "Shared team files are part of the Team plan. Nothing was uploaded.".to_string()
            }
            PlanLimitReason::TeamInactive => {
                "Your team's plan has ended. Shared files are read-only.".to_string()
            }
            PlanLimitReason::TeamSeatUnavailable => {
                "Your team has more members than paid seats, and you are past them. Ask the team owner to add a seat.".to_string()
            }
            PlanLimitReason::Other(slug) => format!(
                "Your plan does not cover this ({slug}). See your plan on the web."
            ),
        }
    }
}

impl std::fmt::Display for PlanLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.sentence())
    }
}

/// Whether `candidate` is a page on the web app at `web_url`.
///
/// A plain prefix test is not enough: `https://zodekit.com.example` starts
/// with `https://zodekit.com`. The origin must end where the candidate's path,
/// query or fragment begins.
pub fn on_web_origin(candidate: &str, web_url: &str) -> bool {
    let origin = web_url.trim_end_matches('/');
    if origin.is_empty() {
        return false;
    }
    match candidate.strip_prefix(origin) {
        Some(rest) => rest.is_empty() || rest.starts_with(['/', '?', '#']),
        None => false,
    }
}

/// A link from the server if it is on the web origin, the page at `path` there
/// otherwise.
fn web_link(candidate: Option<String>, web_url: &str, path: &str) -> String {
    candidate
        .filter(|candidate| on_web_origin(candidate, web_url))
        .unwrap_or_else(|| crate::web_page(web_url, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEB: &str = "https://zodekit.com";

    fn full_plan() -> serde_json::Value {
        serde_json::json!({
            "tier": "team",
            "source": "team_seat",
            "status": "active",
            "renewsAt": "2026-12-03T10:00:00.000Z",
            "endsAt": null,
            "limits": {
                "syncDevices": { "used": 3, "max": null },
                "envSync": true,
                "relayDailyBytes": 21474836480u64,
                "teamVault": true
            },
            "team": { "id": "t1", "name": "Acme", "role": "admin", "keyVersion": 2 },
            "grandfather": null,
            "upgradeUrl": "https://zodekit.com/pricing",
            "manageUrl": "https://zodekit.com/account/billing"
        })
    }

    #[test]
    fn a_full_plan_parses_every_field() {
        let plan = PlanSummary::parse(&full_plan().to_string(), WEB).expect("a valid plan");
        assert_eq!(plan.tier, PlanTier::Team);
        assert_eq!(plan.source, PlanSource::TeamSeat);
        assert_eq!(plan.status, PlanStatus::Active);
        assert_eq!(
            plan.renews_at.as_ref().map(|at| at.as_ref()),
            Some("2026-12-03T10:00:00.000Z")
        );
        assert_eq!(plan.ends_at, None);
        assert_eq!(
            plan.limits,
            PlanLimits {
                sync_devices: SyncDeviceUsage { used: 3, max: None },
                env_sync: true,
                relay_daily_bytes: Some(21_474_836_480),
                team_vault: true,
                push_notifications: true,
            }
        );
        assert_eq!(
            plan.team,
            Some(TeamSummary {
                id: "t1".into(),
                name: "Acme".into(),
                role: "admin".into(),
                key_version: 2,
            })
        );
        assert_eq!(plan.upgrade_url, "https://zodekit.com/pricing");
        assert_eq!(plan.manage_url, "https://zodekit.com/account/billing");
    }

    #[test]
    fn a_minimal_free_plan_parses_and_falls_back_to_the_web_pages() {
        let body = serde_json::json!({
            "tier": "free",
            "source": "grandfather",
            "status": "active",
            "limits": {
                "syncDevices": { "used": 2, "max": 2 },
                "envSync": true,
                "relayDailyBytes": 2147483648u64,
                "teamVault": false
            },
            "grandfather": { "envSyncUntil": "2027-11-03T00:00:00.000Z" }
        });
        let plan = PlanSummary::parse(&body.to_string(), WEB).expect("a valid plan");
        assert_eq!(plan.tier, PlanTier::Free);
        assert_eq!(plan.limits.sync_devices.max, Some(2));
        assert_eq!(plan.team, None);
        assert_eq!(
            plan.grandfather_env_until.as_ref().map(|at| at.as_ref()),
            Some("2027-11-03T00:00:00.000Z")
        );
        assert_eq!(plan.upgrade_url, "https://zodekit.com/pricing");
        assert_eq!(plan.manage_url, "https://zodekit.com/account/billing");
    }

    /// Paid plans, and everyone while the server enforces no plan, have no
    /// relay cap; the server says so with `null`. Older servers send a number.
    #[test]
    fn an_unlimited_relay_is_null_and_does_not_lose_the_plan() {
        let mut body = full_plan();
        body["limits"]["relayDailyBytes"] = serde_json::Value::Null;
        let plan = PlanSummary::parse(&body.to_string(), WEB).expect("a valid plan");
        assert_eq!(plan.limits.relay_daily_bytes, None);
        assert!(plan.limits.env_sync, "the other limits are kept");

        body["limits"]["relayDailyBytes"] = 5_368_709_120u64.into();
        let plan = PlanSummary::parse(&body.to_string(), WEB).expect("a valid plan");
        assert_eq!(plan.limits.relay_daily_bytes, Some(5_368_709_120));
    }

    /// A member past the paid seats of a team that is still active: the plan has
    /// not ended, and only the owner can fix it, by adding a seat.
    #[test]
    fn a_member_past_the_paid_seats_is_told_to_ask_the_owner() {
        let limit = PlanLimit::from_body(r#"{"error":"team_seat_unavailable"}"#, WEB);
        let sentence = limit.sentence();
        assert!(sentence.contains("owner"), "{sentence}");
        assert!(!sentence.contains("ended"), "{sentence}");
    }

    /// Pushes are in the plan unless the server says they are not: a server
    /// from before the field must not switch them off here.
    #[test]
    fn push_notifications_are_read_when_stated_and_assumed_when_not() {
        let mut body = full_plan();
        let read = |body: &serde_json::Value| {
            PlanSummary::parse(&body.to_string(), WEB)
                .expect("a valid plan")
                .limits
                .push_notifications
        };
        assert!(read(&body), "a server that does not say");

        body["limits"]["pushNotifications"] = false.into();
        assert!(!read(&body));

        body["limits"]["pushNotifications"] = true.into();
        assert!(read(&body));
    }

    #[test]
    fn an_unknown_tier_is_refused_rather_than_guessed() {
        let mut body = full_plan();
        body["tier"] = "enterprise".into();
        assert!(PlanSummary::parse(&body.to_string(), WEB).is_err());
    }

    #[test]
    fn an_unknown_source_or_status_does_not_lose_the_plan() {
        let mut body = full_plan();
        body["source"] = "gift".into();
        body["status"] = "paused_for_review".into();
        let plan = PlanSummary::parse(&body.to_string(), WEB).expect("still a plan");
        assert_eq!(plan.source, PlanSource::Unknown);
        assert_eq!(plan.status, PlanStatus::Unknown);
    }

    #[test]
    fn links_to_another_origin_are_replaced_with_the_web_pages() {
        let mut body = full_plan();
        body["upgradeUrl"] = "https://zodekit.com.evil.example/pricing".into();
        body["manageUrl"] = "javascript:alert(1)".into();
        let plan = PlanSummary::parse(&body.to_string(), WEB).expect("a valid plan");
        assert_eq!(plan.upgrade_url, "https://zodekit.com/pricing");
        assert_eq!(plan.manage_url, "https://zodekit.com/account/billing");
    }

    #[test]
    fn the_origin_check_ends_where_the_path_begins() {
        assert!(on_web_origin("https://zodekit.com", WEB));
        assert!(on_web_origin("https://zodekit.com/pricing", WEB));
        assert!(on_web_origin("https://zodekit.com/pricing?plan=pro", WEB));
        assert!(on_web_origin(
            "https://zodekit.com/pricing",
            "https://zodekit.com/"
        ));
        assert!(!on_web_origin(
            "https://zodekit.com.evil.example/pricing",
            WEB
        ));
        assert!(!on_web_origin("https://zodekit.comevil.example", WEB));
        assert!(!on_web_origin("http://zodekit.com/pricing", WEB));
        assert!(!on_web_origin(
            "https://evil.example/?https://zodekit.com",
            WEB
        ));
        assert!(!on_web_origin("", WEB));
    }

    #[test]
    fn each_refusal_reason_is_read_with_its_numbers() {
        for (slug, reason) in [
            ("sync_device_limit", PlanLimitReason::SyncDeviceLimit),
            ("env_sync_not_in_plan", PlanLimitReason::EnvSyncNotInPlan),
            (
                "team_vault_not_in_plan",
                PlanLimitReason::TeamVaultNotInPlan,
            ),
            ("team_inactive", PlanLimitReason::TeamInactive),
        ] {
            let body = serde_json::json!({
                "error": slug,
                "limit": 2,
                "used": 3,
                "tier": "free",
                "upgradeUrl": "https://zodekit.com/pricing?from=editor"
            });
            let limit = PlanLimit::from_body(&body.to_string(), WEB);
            assert_eq!(limit.reason, reason, "{slug}");
            assert_eq!(limit.limit, Some(2));
            assert_eq!(limit.used, Some(3));
            assert_eq!(limit.upgrade_url, "https://zodekit.com/pricing?from=editor");
            assert!(!limit.sentence().is_empty(), "{slug} has nothing to say");
        }
    }

    #[test]
    fn an_old_server_body_with_only_the_slug_still_reads() {
        let limit = PlanLimit::from_body(r#"{"error":"sync_device_limit"}"#, WEB);
        assert_eq!(limit.reason, PlanLimitReason::SyncDeviceLimit);
        assert_eq!(limit.limit, None);
        assert_eq!(limit.upgrade_url, "https://zodekit.com/pricing");
    }

    #[test]
    fn a_missing_or_garbled_body_is_still_a_plan_refusal() {
        for body in ["", "<html>Payment Required</html>", r#"{"error":7}"#] {
            let limit = PlanLimit::from_body(body, WEB);
            assert_eq!(
                limit.reason,
                PlanLimitReason::Other("http_402".into()),
                "{body:?}"
            );
            assert_eq!(limit.upgrade_url, "https://zodekit.com/pricing");
        }
    }

    #[test]
    fn an_unknown_reason_keeps_its_slug() {
        let limit = PlanLimit::from_body(r#"{"error":"seat_limit"}"#, WEB);
        assert_eq!(limit.reason, PlanLimitReason::Other("seat_limit".into()));
        assert!(limit.sentence().contains("seat_limit"));
    }

    #[test]
    fn a_refusal_link_to_another_origin_is_never_kept() {
        let limit = PlanLimit::from_body(
            r#"{"error":"env_sync_not_in_plan","upgradeUrl":"https://evil.example/pricing"}"#,
            WEB,
        );
        assert_eq!(limit.upgrade_url, "https://zodekit.com/pricing");
    }

    #[test]
    fn the_sentences_say_what_happened_and_what_to_do() {
        let limit = |reason| PlanLimit {
            reason,
            limit: Some(2),
            used: Some(3),
            upgrade_url: format!("{WEB}/pricing"),
        };
        let device = limit(PlanLimitReason::SyncDeviceLimit).sentence();
        assert!(device.contains("2"), "{device}");
        assert!(device.contains("Free"), "{device}");
        assert!(device.contains("won't sync"), "{device}");
        assert!(device.contains("Revoke a device on the web"), "{device}");

        let env = limit(PlanLimitReason::EnvSyncNotInPlan).sentence();
        assert_eq!(
            env,
            ".env sync is part of Pro. Nothing was uploaded; your synced files are still readable."
        );

        let team = limit(PlanLimitReason::TeamInactive).sentence();
        assert_eq!(
            team,
            "Your team's plan has ended. Shared files are read-only."
        );

        // A refusal that carries no number still makes a sentence, and does not
        // invent one.
        let unnumbered = PlanLimit {
            limit: None,
            used: None,
            ..limit(PlanLimitReason::SyncDeviceLimit)
        }
        .sentence();
        assert!(!unnumbered.contains("None"), "{unnumbered}");
        assert!(unnumbered.contains("won't sync"), "{unnumbered}");
    }
}
